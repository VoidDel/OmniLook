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
