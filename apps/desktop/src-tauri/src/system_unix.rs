//! Opening files and links elsewhere than on Windows (Linux: `xdg-open`, the desktop's choice).

use std::{
    path::Path,
    process::{Command, Stdio},
};

pub fn exe() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_else(|_| "omnilook".into())
}

fn xdg_open(arg: &std::ffi::OsStr) -> bool {
    Command::new("xdg-open")
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// Opens the file with the program the desktop uses for its type.
pub fn open_default(path: &Path) -> bool {
    xdg_open(path.as_os_str())
}

/// Opens a web or mail address from a preview (a link in Markdown). Anything else (`file:`, a
/// program) is refused.
pub fn open_url(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    if !(l.starts_with("http://") || l.starts_with("https://") || l.starts_with("mailto:")) {
        return false;
    }
    xdg_open(url.trim().as_ref())
}
