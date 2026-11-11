//! The command line's side of the agent pipe (`docs/development-plan.md` §6.2).

use std::{
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
    time::{Duration, Instant},
};

use omnilook_win::pipe;

use crate::{
    paths,
    protocol::{Reply, Request},
    system,
};

/// `CREATE_NO_WINDOW | DETACHED_PROCESS`: the agent has no console.
const DETACHED: u32 = 0x0800_0000 | 0x0000_0008;

pub fn agent_pipe() -> Option<String> {
    paths::pipe("agent").ok()
}

pub fn send(req: &Request, wait: Duration) -> Reply {
    let Some(name) = agent_pipe() else {
        return Reply::err("no pipe name");
    };
    let line = serde_json::to_string(req).unwrap_or_default();
    match pipe::request(&name, &line, wait) {
        Ok(answer) => Reply::parse(&answer),
        Err(e) => Reply::err(e),
    }
}

fn agent_running() -> bool {
    send(&Request::Ping, Duration::from_millis(500)).ok
}

/// Our standard handles stop being inheritable. `Command` lets a child inherit every inheritable
/// handle, whatever its own stdio: the agent would keep a caller's output pipe open (OmniZip
/// waiting for us to finish would wait for the agent to exit).
fn keep_std_handles() {
    use windows::Win32::{
        Foundation::{HANDLE_FLAG_INHERIT, HANDLE_FLAGS, SetHandleInformation},
        System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
    };
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        unsafe {
            if let Ok(h) = GetStdHandle(which)
                && !h.is_invalid()
            {
                let _ = SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0));
            }
        }
    }
}

/// Starts the agent unless it runs, and waits until it answers.
pub fn ensure_agent() -> bool {
    if agent_running() {
        return true;
    }
    keep_std_handles();
    if Command::new(system::exe())
        .arg("--agent")
        // Not the caller's handles: a caller waiting for its output to end would wait for the
        // agent to exit.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED)
        .spawn()
        .is_err()
    {
        return false;
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
        if agent_running() {
            return true;
        }
    }
    false
}

pub fn preview(path: PathBuf, title: Option<String>) -> ExitCode {
    let path = std::path::absolute(&path).unwrap_or(path);
    if !path.exists() {
        eprintln!("omnilook: {} does not exist", path.display());
        return ExitCode::from(2);
    }
    if !ensure_agent() {
        eprintln!("omnilook: the agent did not start");
        return ExitCode::FAILURE;
    }
    let r = send(&Request::Preview { path, title }, Duration::from_secs(5));
    if r.ok {
        ExitCode::SUCCESS
    } else {
        eprintln!("omnilook: {}", r.error.unwrap_or_default());
        ExitCode::FAILURE
    }
}

pub fn quit() -> ExitCode {
    if agent_running() {
        send(&Request::Quit, Duration::from_secs(2));
    }
    ExitCode::SUCCESS
}
