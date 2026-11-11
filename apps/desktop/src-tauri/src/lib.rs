//! Omni Look (`docs/development-plan.md` §3.1): one executable in several roles.
//!
//! | Command line | Role |
//! |---|---|
//! | `omnilook --agent` | resident agent: the hotkey hook and the pipe, no WebView |
//! | `omnilook --view --path <file> …` | one preview window; started by the agent, exits on close |
//! | `omnilook preview <file> [--title …]` / `omnilook <file>` | asks the agent to preview (OmniZip) |
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
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod ui;

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
        Mode::Preview { path, title } => client::preview(path, title),
        Mode::Quit => client::quit(),
        Mode::Usage(msg) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
    }
}

#[cfg(not(windows))]
pub fn main() -> ExitCode {
    eprintln!("Omni Look runs on Windows (docs/development-plan.md §7)");
    ExitCode::FAILURE
}
