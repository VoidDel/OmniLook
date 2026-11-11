//! Omni Look (`docs/development-plan.md` §3.1): one executable in several roles.
//!
//! | Command line | Role |
//! |---|---|
//! | `omnilook --agent` | resident agent: the hotkey hook and the pipe, no WebView |
//! | `omnilook --view --path <file> …` | one preview window; started by the agent, exits on close |
//! | `omnilook preview <file> [--title …] [--nav <pipe>]` / `omnilook <file>` | asks the agent to preview (OmniZip) |
//! | `omnilook` | starts the agent if needed and opens the settings window |
//! | `omnilook --quit` | stops the agent and its preview |

pub mod cli;
pub mod paths;
pub mod protocol;
pub mod settings;

#[cfg(windows)]
mod agent;
#[cfg(windows)]
mod client;
#[cfg(windows)]
mod system;
#[cfg(not(windows))]
#[path = "system_unix.rs"]
mod system;
#[cfg(windows)]
mod tray;
mod ui;
#[cfg(windows)]
mod win_icon;

use std::process::ExitCode;

#[cfg(windows)]
pub fn main() -> ExitCode {
    use cli::Mode;
    match cli::parse(std::env::args_os().skip(1)) {
        Mode::Agent => agent::run(),
        Mode::View(args) => ui::run_view(args),
        Mode::Settings => {
            // The hotkey works once Omni Look has been started, whatever the autostart setting.
            let _ = client::ensure_agent();
            if ui::focus_existing_settings() {
                return ExitCode::SUCCESS;
            }
            ui::run_settings()
        }
        Mode::Preview { path, title, nav } => client::preview(path, title, nav),
        Mode::Quit => client::quit(),
        Mode::Usage(msg) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
    }
}

/// Elsewhere than on Windows (Linux, `docs/development-plan.md` §7) there is no agent and no
/// hotkey: `omnilook <file>` opens a preview window of its own (a file manager's custom action
/// runs it), `omnilook` the settings.
#[cfg(not(windows))]
pub fn main() -> ExitCode {
    use cli::{Mode, ViewArgs};
    match cli::parse(std::env::args_os().skip(1)) {
        Mode::View(args) => ui::run_view(args),
        Mode::Preview { path, title, .. } => {
            let path = std::path::absolute(&path).unwrap_or(path);
            if !path.exists() {
                eprintln!("omnilook: {} does not exist", path.display());
                return ExitCode::from(2);
            }
            ui::run_view(ViewArgs {
                path,
                title,
                anchor: None,
                gpu: false,
            })
        }
        Mode::Settings => ui::run_settings(),
        Mode::Quit => ExitCode::SUCCESS,
        Mode::Agent => {
            eprintln!("omnilook: the agent (hotkey) is for Windows; run omnilook <file>");
            ExitCode::from(2)
        }
        Mode::Usage(msg) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
    }
}
