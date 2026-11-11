//! Where Omni Look keeps things.

use std::path::PathBuf;

/// `OMNILOOK_HOME` puts everything (settings, profiles, logs) in one folder and leaves the registry
/// alone: for tests, so they never touch the real settings or what Windows starts.
pub fn isolated() -> bool {
    std::env::var_os("OMNILOOK_HOME").is_some()
}

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
pub fn config_dir() -> PathBuf {
    env_dir("APPDATA")
}

/// Machine-local data: WebView2 profiles and logs, `%LOCALAPPDATA%\OmniLook`.
pub fn local_dir() -> PathBuf {
    env_dir("LOCALAPPDATA")
}

/// The WebView2 profile for a GPU mode: WebView2 refuses to share a profile between processes
/// started with different browser arguments.
pub fn webview_dir(gpu: bool) -> PathBuf {
    local_dir()
        .join("WebView")
        .join(if gpu { "gpu" } else { "cpu" })
}
