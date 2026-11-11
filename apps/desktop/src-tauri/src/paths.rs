//! Where Omni Look keeps things.

use std::path::PathBuf;

/// `OMNILOOK_HOME` puts everything (settings, profiles, logs) in one folder and leaves the registry
/// alone: for tests, so they never touch the real settings or what Windows starts.
pub fn isolated() -> bool {
    std::env::var_os("OMNILOOK_HOME").is_some()
}

/// The name of one of Omni Look's pipes (`agent`, `settings`, `view-<pid>`). An isolated
/// instance adds a mark of its folder: a test's `--quit` or preview must never reach the agent
/// the user is running (the names are per user, not per folder).
#[cfg(windows)]
pub fn pipe(tag: &str) -> std::io::Result<String> {
    let tag = match std::env::var_os("OMNILOOK_HOME") {
        Some(home) => {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            home.hash(&mut h);
            format!("{tag}-iso{:08x}", h.finish() as u32)
        }
        None => tag.to_owned(),
    };
    omnilook_win::pipe::pipe_name(&tag).map_err(std::io::Error::other)
}

#[cfg(windows)]
fn env_dir(var: &str) -> PathBuf {
    if let Some(home) = std::env::var_os("OMNILOOK_HOME") {
        return PathBuf::from(home);
    }
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("OmniLook")
}

/// Settings (roams with the user): `%APPDATA%\OmniLook`.
#[cfg(windows)]
pub fn config_dir() -> PathBuf {
    env_dir("APPDATA")
}

/// Machine-local data: WebView2 profiles and logs, `%LOCALAPPDATA%\OmniLook`.
#[cfg(windows)]
pub fn local_dir() -> PathBuf {
    env_dir("LOCALAPPDATA")
}

/// The XDG base directories: `$XDG_CONFIG_HOME` (`~/.config`) for settings, `$XDG_DATA_HOME`
/// (`~/.local/share`) for the web view's data.
#[cfg(not(windows))]
fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    if let Some(home) = std::env::var_os("OMNILOOK_HOME") {
        return PathBuf::from(home);
    }
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
        .unwrap_or_else(std::env::temp_dir)
        .join("omnilook")
}

#[cfg(not(windows))]
pub fn config_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config")
}

#[cfg(not(windows))]
pub fn local_dir() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share")
}

/// The WebView2 profile for a GPU mode: WebView2 refuses to share a profile between processes
/// started with different browser arguments.
pub fn webview_dir(gpu: bool) -> PathBuf {
    local_dir()
        .join("WebView")
        .join(if gpu { "gpu" } else { "cpu" })
}
