#![allow(clippy::unwrap_used, clippy::expect_used)]
use axum::{http::StatusCode, response::IntoResponse, Json};
use crate::pty::{self, CreateSessionOptions};
use crate::session::{self, SessionManager};
use crate::settings::{self, RestoreConfig};
use crate::shell_profiles;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc, time::{SystemTime, UNIX_EPOCH}};
use tracing::{debug, warn};

const STATE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct WorksiteState {
    pub version: u32,
    pub updated_at: u64,
    #[serde(default)]
    pub active_pane_id: Option<String>,
    #[serde(default)]
    pub tabs: Vec<RestoredTab>,
    #[serde(default)]
    pub panes: Vec<RestoredPane>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RestoredTab {
    pub tab_id: String,
    pub layout: serde_json::Value,
    #[serde(default)]
    pub active_pane_id: Option<String>,
    #[serde(default)]
    pub shell_profile_id: Option<String>,
    #[serde(default)]
    pub shell_profile_name: Option<String>,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub workspace_roots: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RestoredPane {
    pub pane_id: String,
    pub tab_id: String,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub shell_profile_id: Option<String>,
    #[serde(default)]
    pub shell_profile_name: Option<String>,
    #[serde(default)]
    pub output_tail: Vec<String>,
    #[serde(default)]
    pub recent_commands: Vec<String>,
}

fn state_path() -> PathBuf {
    settings::config_dir().join("worksite_state.json")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().try_into().unwrap_or(u64::MAX))
}

#[allow(clippy::unused_async)]
pub async fn get_restore_state() -> impl IntoResponse {
    Json(load_state().unwrap_or_default())
}

#[allow(clippy::unused_async)]
pub async fn delete_restore_state() -> impl IntoResponse {
    match clear_state() {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))).into_response(),
    }
}

pub fn load_state() -> Option<WorksiteState> {
    let path = state_path();
    let data = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<WorksiteState>(&data) {
        Ok(state) if state.version == STATE_VERSION => Some(state),
        Ok(_) => None,
        Err(e) => {
            warn!("Failed to parse worksite state: {e}");
            None
        }
    }
}

pub fn save_state(manager: &SessionManager) {
    let settings = settings::load_settings();
    if !settings.restore.enabled {
        return;
    }
    if let Err(e) = save_state_inner(manager, &settings.restore) {
        warn!("Failed to save worksite state: {e}");
    }
}

fn save_state_inner(manager: &SessionManager, restore_config: &RestoreConfig) -> Result<(), String> {
    let state = snapshot(manager, restore_config);
    let path = state_path();
    let dir = path.parent().ok_or_else(|| "invalid state path".to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

/// # Errors
/// Returns `Err` if the worksite state file exists but cannot be removed.
pub fn clear_state() -> Result<(), String> {
    let path = state_path();
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// # Panics
/// May panic if any internal mutex is poisoned.
pub fn snapshot(manager: &SessionManager, restore_config: &RestoreConfig) -> WorksiteState {
    let max_commands = restore_config.max_commands_per_pane.max(1);
    let output_tail_lines = restore_config.max_output_tail_lines.max(1);

    let mut tabs = Vec::new();
    let mut panes = Vec::new();
    let order = manager.tab_order.lock().expect("mutex poisoned").clone();

    for tab_id in order {
        let Some(tab_val) = manager.tab_layouts.get(&tab_id) else { continue };
        let Some(layout) = tab_val.get("layout").cloned() else { continue };
        let leaf_ids = session::collect_leaf_pane_ids(&layout);
        if leaf_ids.is_empty() {
            continue;
        }
        let active_pane_id = tab_val
            .get("active_pane_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let shell_profile_id = tab_val
            .get("shell_profile_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let shell_profile_name = tab_val
            .get("shell_profile_name")
            .and_then(|v| v.as_str())
            .map(String::from);
        let group_id = tab_val.get("group_id").and_then(|v| v.as_str()).map(String::from);
        let workspace_roots = tab_val
            .get("workspace_roots")
            .and_then(|v| v.as_array())
            .map(|roots| roots.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        tabs.push(RestoredTab {
            tab_id: tab_id.clone(),
            layout: layout.clone(),
            active_pane_id,
            shell_profile_id: shell_profile_id.clone(),
            shell_profile_name: shell_profile_name.clone(),
            group_id,
            workspace_roots,
        });

        for pane_id in leaf_ids {
            let session = manager.sessions.get(&pane_id);
            let cwd = session.as_ref().map(|s| s.cwd_state.lock().expect("mutex poisoned").cwd.clone());
            let output_tail = if restore_config.persist_output_tails {
                session
                    .as_ref()
                    .map(|s| {
                        let screen = s.screen.lock().expect("mutex poisoned");
                        let mut lines = screen.snapshot_scrollback_plain(Some(output_tail_lines));
                        lines.push(screen.snapshot_plain());
                        lines
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let pane_profile_id = session
                .as_ref()
                .and_then(|s| s.shell_profile_id.clone())
                .or_else(|| shell_profile_id.clone());
            let pane_profile_name = session
                .as_ref()
                .and_then(|s| s.shell_profile_name.clone())
                .or_else(|| shell_profile_name.clone());
            let recent_commands: Vec<String> = session
                .as_ref()
                .map(|s| {
                    let cmds = s.recent_commands.lock().expect("mutex poisoned");
                    if cmds.len() > max_commands {
                        cmds[cmds.len() - max_commands..].to_vec()
                    } else {
                        cmds.clone()
                    }
                })
                .unwrap_or_default();
            panes.push(RestoredPane {
                pane_id,
                tab_id: tab_id.clone(),
                cwd,
                shell_profile_id: pane_profile_id,
                shell_profile_name: pane_profile_name,
                output_tail,
                recent_commands,
            });
        }
    }

    WorksiteState {
        version: STATE_VERSION,
        updated_at: now_ms(),
        active_pane_id: manager.active_pane_id.lock().expect("mutex poisoned").clone(),
        tabs,
        panes,
    }
}

/// # Panics
/// May panic if any internal mutex is poisoned.
pub fn restore(manager: &Arc<SessionManager>) {
    let settings = settings::load_settings();
    if !settings.restore.enabled {
        return;
    }
    let Some(state) = load_state() else { return };
    if state.tabs.is_empty() || state.panes.is_empty() {
        return;
    }

    for pane in &state.panes {
        if manager.sessions.contains_key(&pane.pane_id) {
            continue;
        }
        let shell_profile = shell_profiles::find_profile(pane.shell_profile_id.as_deref());
        match pty::create_session_with_options(
            manager,
            &pane.pane_id,
            CreateSessionOptions { cwd: pane.cwd.clone(), shell_profile, ..CreateSessionOptions::default() },
        ) {
            Ok((session, _)) => {
                session
                    .recent_commands
                    .lock()
                    .expect("mutex poisoned")
                    .clone_from(&pane.recent_commands);
            }
            Err(e) => warn!("Failed to restore pane {}: {}", pane.pane_id, e),
        }
    }

    for tab in state.tabs {
        manager.insert_tab(
            tab.tab_id,
            serde_json::json!({
                "layout": tab.layout,
                "active_pane_id": tab.active_pane_id,
                "shell_profile_id": tab.shell_profile_id,
                "shell_profile_name": tab.shell_profile_name,
                "group_id": tab.group_id,
                "workspace_roots": tab.workspace_roots,
            }),
        );
    }

    if let Some(active) = state.active_pane_id {
        *manager.active_pane_id.lock().expect("mutex poisoned") = Some(active);
    }
    debug!("Restored worksite state");
}
