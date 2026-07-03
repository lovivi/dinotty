#![allow(clippy::unwrap_used, clippy::expect_used)]
use axum::{response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShellProfileKind {
    Unix,
    Wsl,
    Powershell,
    Cmd,
    Custom,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ShellProfile {
    pub id: String,
    pub name: String,
    pub kind: ShellProfileKind,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub wsl_distro: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    pub source: String,
}

#[derive(Serialize)]
pub struct ShellProfilesResponse {
    pub profiles: Vec<ShellProfile>,
    pub default_profile_id: Option<String>,
}

#[allow(clippy::unused_async)]
pub async fn list_profiles() -> impl IntoResponse {
    let profiles = builtin_profiles();
    let default_profile_id =
        profiles.iter().find(|profile| profile.is_default).map(|p| p.id.clone());
    Json(ShellProfilesResponse { profiles, default_profile_id })
}

#[must_use]
pub fn builtin_profiles() -> Vec<ShellProfile> {
    if cfg!(windows) {
        windows_profiles()
    } else {
        unix_profiles()
    }
}

#[must_use]
pub fn default_profile() -> Option<ShellProfile> {
    builtin_profiles().into_iter().find(|profile| profile.is_default)
}

#[must_use]
pub fn find_profile(profile_id: Option<&str>) -> Option<ShellProfile> {
    let profiles = builtin_profiles();
    profile_id
        .and_then(|id| profiles.iter().find(|profile| profile.id == id).cloned())
        .or_else(|| profiles.into_iter().find(|profile| profile.is_default))
}

fn unix_profiles() -> Vec<ShellProfile> {
    let shell = crate::pty::get_shell();
    let name =
        Path::new(&shell).file_name().and_then(|name| name.to_str()).unwrap_or("Shell").to_string();
    vec![ShellProfile {
        id: "unix-default".into(),
        name,
        kind: ShellProfileKind::Unix,
        command: shell.clone(),
        args: crate::pty::get_shell_args(&shell).into_iter().map(String::from).collect(),
        wsl_distro: None,
        is_default: true,
        source: "builtin".into(),
    }]
}

#[cfg(windows)]
fn windows_profiles() -> Vec<ShellProfile> {
    let mut profiles = Vec::new();

    if command_exists("wsl.exe") {
        profiles.push(ShellProfile {
            id: "wsl-default".into(),
            name: "WSL".into(),
            kind: ShellProfileKind::Wsl,
            command: "wsl.exe".into(),
            args: Vec::new(),
            wsl_distro: None,
            is_default: true,
            source: "builtin".into(),
        });
    }

    if command_exists("pwsh.exe") {
        profiles.push(ShellProfile {
            id: "powershell".into(),
            name: "PowerShell".into(),
            kind: ShellProfileKind::Powershell,
            command: "pwsh.exe".into(),
            args: vec!["-NoLogo".into()],
            wsl_distro: None,
            is_default: profiles.is_empty(),
            source: "builtin".into(),
        });
    } else if command_exists("powershell.exe") {
        profiles.push(ShellProfile {
            id: "powershell".into(),
            name: "PowerShell".into(),
            kind: ShellProfileKind::Powershell,
            command: "powershell.exe".into(),
            args: vec!["-NoLogo".into()],
            wsl_distro: None,
            is_default: profiles.is_empty(),
            source: "builtin".into(),
        });
    }

    profiles.push(ShellProfile {
        id: "cmd".into(),
        name: "Command Prompt".into(),
        kind: ShellProfileKind::Cmd,
        command: "cmd.exe".into(),
        args: Vec::new(),
        wsl_distro: None,
        is_default: profiles.is_empty(),
        source: "builtin".into(),
    });

    profiles
}

#[cfg(not(windows))]
fn windows_profiles() -> Vec<ShellProfile> {
    Vec::new()
}

#[cfg(windows)]
fn command_exists(command: &str) -> bool {
    let mut cmd = std::process::Command::new("where.exe");
    cmd.arg(command);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    cmd.output().map(|output| output.status.success()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_builtin_profiles_have_default_on_non_windows() {
        if cfg!(windows) {
            return;
        }
        let profiles = builtin_profiles();
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].is_default);
        assert_eq!(profiles[0].kind, ShellProfileKind::Unix);
    }

    #[test]
    fn find_profile_falls_back_to_default() {
        let default = default_profile().unwrap();
        let found = find_profile(Some("missing-profile")).unwrap();
        assert_eq!(found.id, default.id);
    }
}
