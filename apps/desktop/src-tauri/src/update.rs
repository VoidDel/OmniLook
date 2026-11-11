//! Updates (Windows): `latest.json` of the newest GitHub release, whose packages are signed with
//! Omni Look's minisign key (the public half is in `tauri.conf.json`; the private half signs
//! releases in CI and never enters the repository). Same scheme as Omni Download.
//!
//! - The settings window checks when it opens and on "check for updates", shows what is new, and
//!   installs when told: the package downloads (with progress), the installer runs (its hooks stop
//!   the agent and start the new one), the settings window comes back in the new version.
//! - The agent, once a day, starts `omnilook --update-check`: a process without a window that
//!   checks and, the first time a version is seen, opens the settings window to show it.

use std::{path::PathBuf, sync::Mutex, time::SystemTime};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::paths;

/// Hours between the agent's background checks.
#[cfg_attr(not(windows), allow(dead_code))]
pub const EVERY_HOURS: u64 = 24;

/// The update found by the last check, until it is installed.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    /// Release notes (Markdown), possibly empty.
    notes: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    downloaded: u64,
    total: Option<u64>,
}

/// When the last background check ran, and the last version it told the user about.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Checked {
    /// Seconds since 1970.
    pub at: u64,
    pub told: Option<String>,
}

fn checked_path() -> PathBuf {
    paths::local_dir().join("update.json")
}

pub fn load_checked() -> Checked {
    std::fs::read(checked_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_checked(c: &Checked) {
    let p = checked_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, serde_json::to_vec(c).unwrap_or_default());
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Whether a background check is due.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn due() -> bool {
    now().saturating_sub(load_checked().at) >= EVERY_HOURS * 3600
}

async fn find(app: &AppHandle) -> Result<Option<Update>, String> {
    app.updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())
}

/// A newer version, if there is one.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let found = find(&app).await?;
    let info = found.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        notes: u.body.clone().unwrap_or_default(),
    });
    *app.state::<Pending>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = found;
    Ok(info)
}

/// Downloads and installs the update found by the last check. Progress arrives as
/// `update:progress` events; the installer then replaces this version and starts the new one.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    let update = app
        .state::<Pending>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or("no update to install")?;
    let mut downloaded = 0u64;
    let mut last = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                // About a hundred events for the whole package.
                let step = total.map_or(1 << 20, |t| (t / 100).max(1 << 16));
                if downloaded - last >= step || total == Some(downloaded) {
                    last = downloaded;
                    let _ = app.emit("update:progress", UpdateProgress { downloaded, total });
                }
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart();
}

/// The agent's daily check (`omnilook --update-check`, no window): notes the time, and the
/// first time it sees a version opens the settings window, which shows it.
pub async fn background_check(app: &AppHandle) {
    let mut checked = load_checked();
    checked.at = now();
    save_checked(&checked);
    let Ok(Some(update)) = find(app).await else {
        return;
    };
    if checked.told.as_deref() == Some(update.version.as_str()) {
        return;
    }
    checked.told = Some(update.version.clone());
    save_checked(&checked);
    let _ = std::process::Command::new(crate::system::exe())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
