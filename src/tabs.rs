#![allow(clippy::unwrap_used, clippy::expect_used)]
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

use crate::pty::{self, CreateSessionOptions};
use crate::restore_state;
use crate::session::{self, SessionManager, SyncMsg};
use crate::shell_profiles;

/// Pick the first existing directory from `workspace_roots`, falling back to
/// `None` if none of them exist on disk. Used to give new terminals a sane
/// starting cwd without crashing on stale paths.
fn cwd_from_workspace_roots(roots: &[String]) -> Option<PathBuf> {
    for root in roots {
        let p = PathBuf::from(root);
        if p.is_dir() {
            return Some(p);
        }
        tracing::warn!("Workspace root {:?} does not exist on disk, skipping", root);
    }
    if !roots.is_empty() {
        tracing::warn!(
            "All {} workspace root(s) do not exist — falling back to default cwd",
            roots.len()
        );
    }
    None
}

// ─── Request/Response types ────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct CreateTabRequest {
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub workspace_roots: Vec<String>,
}

#[derive(Deserialize)]
pub struct SplitPaneRequest {
    pub pane_id: String,
    pub direction: String, // "horizontal" or "vertical"
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateLayoutRequest {
    pub layout: serde_json::Value,
    pub active_pane_id: String,
}

#[derive(Deserialize)]
pub struct UpdateTabMetaRequest {
    #[serde(default)]
    pub group_id: Option<Option<String>>,
    #[serde(default)]
    pub workspace_roots: Option<Vec<String>>,
}

// ─── GET /api/tabs ─────────────────────────────────────────────────

#[allow(clippy::unused_async)]
pub async fn list_tabs(State(manager): State<Arc<SessionManager>>) -> impl IntoResponse {
    let (tabs, active_pane_id) = manager.tab_list();
    Json(serde_json::json!({
        "tabs": tabs,
        "active_pane_id": active_pane_id,
    }))
}

// ─── POST /api/tabs ────────────────────────────────────────────────

/// # Panics
/// Panics if the internal mutex is poisoned.
#[allow(clippy::unused_async)]
pub async fn create_tab(
    State(manager): State<Arc<SessionManager>>,
    body: Option<Json<CreateTabRequest>>,
) -> impl IntoResponse {
    let req = body.map_or_else(CreateTabRequest::default, |Json(req)| req);
    let tab_id = uuid::Uuid::new_v4().to_string();
    let pane_id = uuid::Uuid::new_v4().to_string();

    let shell_profile = shell_profiles::find_profile(req.profile_id.as_deref());
    let shell_profile_id = shell_profile.as_ref().map(|profile| profile.id.clone());
    let shell_profile_name = shell_profile.as_ref().map(|profile| profile.name.clone());
    let group_id = req.group_id.clone();
    let workspace_roots = req.workspace_roots.clone();
    let cwd = cwd_from_workspace_roots(&workspace_roots);

    // Create PTY session
    let (_session, _shell_type) = match pty::create_session_with_options(
        &manager,
        &pane_id,
        CreateSessionOptions { cwd, shell_profile, ..CreateSessionOptions::default() },
    ) {
        Ok(x) => x,
        Err(e) => {
            tracing::error!("Failed to create PTY: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e })))
                .into_response();
        }
    };

    // Create initial layout with single leaf
    let layout = serde_json::json!({
        "type": "leaf",
        "paneId": pane_id,
        "title": "Terminal",
        "ratio": 1,
        "zoomed": false,
    });

    // Store tab
    manager.insert_tab(
        tab_id.clone(),
        serde_json::json!({
            "layout": layout,
            "active_pane_id": pane_id,
            "shell_profile_id": shell_profile_id.clone(),
            "shell_profile_name": shell_profile_name.clone(),
            "group_id": group_id.clone(),
            "workspace_roots": workspace_roots.clone(),
        }),
    );

    // Set as active tab
    *manager.active_pane_id.lock().expect("mutex poisoned") = Some(pane_id.clone());
    restore_state::save_state(&manager);

    // Broadcast to all sync clients
    manager.broadcast_sync(&SyncMsg::TabCreated {
        tab_id: tab_id.clone(),
        pane_id: pane_id.clone(),
        layout: Some(layout.clone()),
        group_id: group_id.clone(),
        workspace_roots: Some(workspace_roots.clone()),
    });

    Json(serde_json::json!({
        "tab_id": tab_id,
        "pane_id": pane_id,
        "layout": layout,
        "shell_profile_id": shell_profile_id,
        "shell_profile_name": shell_profile_name,
        "group_id": group_id,
        "workspace_roots": workspace_roots,
    }))
    .into_response()
}

// ─── DELETE /api/tabs/{tab_id} ─────────────────────────────────────

#[allow(clippy::unused_async)]
pub async fn close_tab(
    State(manager): State<Arc<SessionManager>>,
    Path(tab_id): Path<String>,
) -> impl IntoResponse {
    // Get tab layout to find all leaf pane IDs
    let leaf_ids: Vec<String> = manager
        .tab_layouts
        .get(&tab_id)
        .and_then(|v| v.get("layout").cloned())
        .map(|layout| session::collect_leaf_pane_ids(&layout))
        .unwrap_or_default();

    // Kill and remove all PTY sessions
    for leaf_id in &leaf_ids {
        manager.kill_and_remove(leaf_id);
    }

    // Remove tab
    manager.remove_tab(&tab_id);
    restore_state::save_state(&manager);

    // Broadcast to all sync clients
    manager.broadcast_sync(&SyncMsg::TabClosed { tab_id: tab_id.clone() });

    Json(serde_json::json!({ "ok": true })).into_response()
}

// ─── POST /api/tabs/{tab_id}/pane ──────────────────────────────────

#[allow(clippy::unused_async)]
pub async fn split_pane(
    State(manager): State<Arc<SessionManager>>,
    Path(tab_id): Path<String>,
    Json(req): Json<SplitPaneRequest>,
) -> impl IntoResponse {
    // Verify tab exists
    let tab_val = match manager.tab_layouts.get(&tab_id) {
        Some(v) => v.value().clone(),
        None => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "tab not found" })))
                .into_response();
        }
    };

    let layout = match tab_val.get("layout") {
        Some(l) => l.clone(),
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "tab has no layout" })),
            )
                .into_response();
        }
    };

    // Verify target pane exists in layout
    let leaf_ids = session::collect_leaf_pane_ids(&layout);
    if !leaf_ids.contains(&req.pane_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "pane not found in tab" })),
        )
            .into_response();
    }

    let new_pane_id = uuid::Uuid::new_v4().to_string();

    let source_session = manager.sessions.get(&req.pane_id).map(|s| Arc::clone(s.value()));
    let source_cwd = source_session
        .as_ref()
        .and_then(|s| s.cwd_state.lock().ok().map(|state| state.cwd.clone()));
    // Fallback: if the source session's tracked CWD is unavailable (e.g. title
    // OSC hasn't fired yet on WSL), try the tab's workspace_roots.
    let cwd = source_cwd.or_else(|| {
        tab_val.get("workspace_roots").and_then(|v| v.as_array()).and_then(|roots| {
            roots.iter().find_map(|r| {
                let p = PathBuf::from(r.as_str()?);
                p.is_dir().then_some(p)
            })
        })
    });
    let shell_profile = shell_profiles::find_profile(req.profile_id.as_deref().or_else(|| {
        source_session.as_ref().and_then(|session| session.shell_profile_id.as_deref())
    }));

    // Create PTY for new pane
    let (_session, _shell_type) = match pty::create_session_with_options(
        &manager,
        &new_pane_id,
        CreateSessionOptions { cwd, shell_profile, ..CreateSessionOptions::default() },
    ) {
        Ok(x) => x,
        Err(e) => {
            tracing::error!("Failed to create PTY for split: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e })))
                .into_response();
        }
    };

    // Update layout tree
    let Some(new_layout) =
        session::insert_pane_into_layout(&layout, &req.pane_id, &req.direction, &new_pane_id)
    else {
        // Clean up PTY if layout update fails
        manager.kill_and_remove(&new_pane_id);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "failed to update layout" })),
        )
            .into_response();
    };

    // Store updated layout
    let active_pane_id = new_pane_id.clone();
    manager.merge_tab_layout(tab_id.clone(), new_layout.clone(), Some(active_pane_id.clone()));
    restore_state::save_state(&manager);

    // Broadcast to all sync clients
    manager.broadcast_sync(&SyncMsg::LayoutUpdated {
        pane_id: tab_id,
        layout: new_layout.clone(),
        active_pane_id,
    });

    Json(serde_json::json!({
        "new_pane_id": new_pane_id,
        "layout": new_layout,
    }))
    .into_response()
}

// ─── DELETE /api/tabs/{tab_id}/pane/{pane_id} ──────────────────────

#[allow(clippy::unused_async)]
pub async fn close_pane(
    State(manager): State<Arc<SessionManager>>,
    Path((tab_id, pane_id)): Path<(String, String)>,
) -> impl IntoResponse {
    // Verify tab exists
    let tab_val = match manager.tab_layouts.get(&tab_id) {
        Some(v) => v.value().clone(),
        None => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "tab not found" })))
                .into_response();
        }
    };

    let layout = match tab_val.get("layout") {
        Some(l) => l.clone(),
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "tab has no layout" })),
            )
                .into_response();
        }
    };

    // Verify pane exists in layout
    let leaf_ids = session::collect_leaf_pane_ids(&layout);
    if !leaf_ids.contains(&pane_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "pane not found in tab" })),
        )
            .into_response();
    }

    // Kill and remove PTY session
    manager.kill_and_remove(&pane_id);

    // Update layout
    if leaf_ids.len() <= 1 {
        // Last pane - remove entire tab
        manager.remove_tab(&tab_id);
        restore_state::save_state(&manager);

        // Broadcast tab closed
        manager.broadcast_sync(&SyncMsg::TabClosed { tab_id: tab_id.clone() });

        Json(serde_json::json!({ "ok": true, "tab_closed": true }))
    } else {
        // Remove pane from layout
        let new_layout = session::remove_pane_from_layout(&layout, &pane_id)
            .unwrap_or(serde_json::Value::Null);

        let new_leaf_ids = session::collect_leaf_pane_ids(&new_layout);
        let active = tab_val
            .get("active_pane_id")
            .and_then(|v| v.as_str());
        let active_pane_id = active
            .filter(|id| new_leaf_ids.iter().any(|lid| lid == *id))
            .or_else(|| new_leaf_ids.first().map(std::string::String::as_str))
            .unwrap_or("")
            .to_string();

        manager.merge_tab_layout(tab_id.clone(), new_layout.clone(), Some(active_pane_id.clone()));
        restore_state::save_state(&manager);

        // Broadcast layout updated
        manager.broadcast_sync(&SyncMsg::LayoutUpdated {
            pane_id: tab_id,
            layout: new_layout.clone(),
            active_pane_id: active_pane_id.clone(),
        });

        Json(serde_json::json!({ "ok": true, "tab_closed": false, "layout": new_layout, "active_pane_id": active_pane_id }))
    }
    .into_response()
}

// ─── PUT /api/tabs/{tab_id}/pane/{pane_id}/activate ────────────────

/// # Panics
/// Panics if the internal mutex is poisoned.
#[allow(clippy::unused_async)]
pub async fn activate_pane(
    State(manager): State<Arc<SessionManager>>,
    Path((tab_id, pane_id)): Path<(String, String)>,
) -> impl IntoResponse {
    // Verify tab exists
    let tab_val = match manager.tab_layouts.get(&tab_id) {
        Some(v) => v.value().clone(),
        None => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "tab not found" })))
                .into_response();
        }
    };

    // Verify pane exists in layout
    let layout = tab_val.get("layout").cloned().unwrap_or(serde_json::Value::Null);
    let leaf_ids = session::collect_leaf_pane_ids(&layout);
    if !leaf_ids.contains(&pane_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "pane not found in tab" })),
        )
            .into_response();
    }

    // Update active pane
    manager.merge_tab_layout(tab_id.clone(), layout, Some(pane_id.clone()));

    // Update global active pane
    *manager.active_pane_id.lock().expect("mutex poisoned") = Some(pane_id.clone());
    restore_state::save_state(&manager);

    // Broadcast to all sync clients
    manager.broadcast_sync(&SyncMsg::TabActivated { pane_id });

    Json(serde_json::json!({ "ok": true })).into_response()
}

// ─── PUT /api/tabs/{tab_id}/layout ─────────────────────────────────

#[allow(clippy::unused_async)]
pub async fn update_layout(
    State(manager): State<Arc<SessionManager>>,
    Path(tab_id): Path<String>,
    Json(req): Json<UpdateLayoutRequest>,
) -> impl IntoResponse {
    // Verify tab exists
    if !manager.tab_layouts.contains_key(&tab_id) {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "tab not found" })))
            .into_response();
    }

    // Store updated layout
    manager.merge_tab_layout(tab_id.clone(), req.layout.clone(), Some(req.active_pane_id.clone()));
    restore_state::save_state(&manager);

    // Broadcast to all sync clients
    manager.broadcast_sync(&SyncMsg::LayoutUpdated {
        pane_id: tab_id,
        layout: req.layout,
        active_pane_id: req.active_pane_id,
    });

    Json(serde_json::json!({ "ok": true })).into_response()
}

// ─── PUT /api/tabs/{tab_id}/meta ───────────────────────────────────

#[allow(clippy::unused_async)]
pub async fn update_tab_meta(
    State(manager): State<Arc<SessionManager>>,
    Path(tab_id): Path<String>,
    Json(req): Json<UpdateTabMetaRequest>,
) -> impl IntoResponse {
    if !manager.tab_layouts.contains_key(&tab_id) {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "tab not found" })))
            .into_response();
    }

    let mut metadata = serde_json::json!({});
    if let Some(group_id) = req.group_id {
        metadata["group_id"] = match group_id {
            Some(id) => serde_json::Value::String(id),
            None => serde_json::Value::Null,
        };
    }
    if let Some(workspace_roots) = req.workspace_roots {
        metadata["workspace_roots"] = serde_json::Value::Array(
            workspace_roots.into_iter().map(serde_json::Value::String).collect(),
        );
    }
    manager.merge_tab_meta(&tab_id, &metadata);
    restore_state::save_state(&manager);

    Json(serde_json::json!({ "ok": true })).into_response()
}
