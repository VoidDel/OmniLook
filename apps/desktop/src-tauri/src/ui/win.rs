//! The windows' Windows side: the preview shown over Explorer without taking the focus, the agent
//! told about it, system preview handlers, the pipes, per-DPI icons.

use std::{path::Path, sync::Arc, time::Duration};

use omnilook_win::{events::window_rect, handler, pipe};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, WebviewWindowBuilder, Wry};
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::{
        ASFW_ANY, AllowSetForegroundWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_SHOWWINDOW, SetWindowPos,
    },
};

use super::{ViewState, blocking, path_of};
use crate::{
    client, paths,
    protocol::{Reply, Request, ViewCommand},
};

/// The thread running system preview handlers, started on first use.
pub type HostSlot = std::sync::Mutex<Option<Arc<handler::Host>>>;

/// `--disable-gpu` halves WebView2's memory; only video needs the GPU (M0, `docs/m0-results.md`).
fn browser_args(gpu: bool) -> String {
    let mut a = String::from("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
    if !gpu {
        a.push_str(" --disable-gpu");
    }
    a
}

/// The preview window: hidden until drawn, never in the taskbar, not taking the focus.
pub fn view_window<'a>(
    b: WebviewWindowBuilder<'a, Wry, tauri::App>,
    gpu: bool,
) -> WebviewWindowBuilder<'a, Wry, tauri::App> {
    b.focused(false)
        .skip_taskbar(true)
        .additional_browser_args(&browser_args(gpu))
}

pub fn settings_window<'a>(
    b: WebviewWindowBuilder<'a, Wry, tauri::App>,
) -> WebviewWindowBuilder<'a, Wry, tauri::App> {
    b.additional_browser_args(&browser_args(false))
}

fn notify_agent(req: Request) {
    std::thread::spawn(move || {
        let _ = client::send(&req, Duration::from_secs(2));
    });
}

fn hwnd(w: &WebviewWindow) -> Option<HWND> {
    w.hwnd().ok().map(|h| HWND(h.0))
}

/// Centers the preview on the Explorer window (or the primary monitor), sized for that monitor's
/// scale, and shows it on top without taking the focus.
fn place_and_show(w: &WebviewWindow, anchor: Option<isize>) {
    let mut area = anchor.and_then(|a| window_rect(HWND(a as *mut _)));
    let monitors = w.available_monitors().unwrap_or_default();
    let monitor = area
        .and_then(|r| {
            let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
            monitors.into_iter().find(|m| {
                let (p, s) = (m.position(), m.size());
                cx >= p.x && cy >= p.y && cx < p.x + s.width as i32 && cy < p.y + s.height as i32
            })
        })
        .or_else(|| w.primary_monitor().ok().flatten());
    if area.is_none()
        && let Some(m) = &monitor
    {
        let (p, s) = (m.position(), m.size());
        area = Some(RECT {
            left: p.x,
            top: p.y,
            right: p.x + s.width as i32,
            bottom: p.y + s.height as i32,
        });
    }
    let scale = monitor.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);
    let (width, height) = ((960.0 * scale) as i32, (680.0 * scale) as i32);
    if let Some(a) = area {
        let x = (a.left + a.right - width) / 2;
        let y = (a.top + a.bottom - height) / 2;
        let _ = w.set_size(tauri::PhysicalSize::new(width as u32, height as u32));
        let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    }
    if let Some(h) = hwnd(w) {
        unsafe {
            let _ = SetWindowPos(
                h,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }
}

/// The first file is drawn: show the window and tell the agent files can come.
pub fn first_shown(app: &AppHandle, state: &ViewState) {
    if let Some(w) = app.get_webview_window("view") {
        place_and_show(&w, state.anchor);
    }
    notify_agent(Request::ViewReady {
        pid: std::process::id(),
        pipe: state.pipe.clone(),
    });
}

/// The settings changed: the agent reloads what it uses (hotkey, icon, closing on blur).
pub fn settings_saved() {
    notify_agent(Request::Reload);
}

/// The user closes the preview: the handler goes first, the agent is told.
pub fn closing(state: &ViewState) {
    state.close_host();
    let _ = client::send(
        &Request::ViewClosed {
            pid: std::process::id(),
        },
        Duration::from_secs(1),
    );
}

/// An arrow key: the agent knows what comes next (Explorer's order, or the caller's).
pub fn nav(_app: &AppHandle, _state: &ViewState, step: i32) {
    notify_agent(Request::Nav {
        pid: std::process::id(),
        step,
    });
}

/// Whether the system has a preview handler for the file. A test can pretend it has none
/// (`OMNILOOK_NO_HANDLERS`, isolated instances only), to see what a computer without Office shows.
pub fn handlers(path: &Path) -> bool {
    if paths::isolated() && std::env::var_os("OMNILOOK_NO_HANDLERS").is_some() {
        return false;
    }
    handler::available(path)
}

impl ViewState {
    /// The handler thread, started on first use; keys a handler passes back reach the page as
    /// `host-key` events.
    fn host(&self, app: &AppHandle) -> Result<Arc<handler::Host>, String> {
        let mut h = self.host.lock().map_err(|e| e.to_string())?;
        if let Some(h) = h.as_ref() {
            return Ok(h.clone());
        }
        let app = app.clone();
        let host = handler::Host::start(move |k| {
            let name = match (k.vk, k.ctrl) {
                (0x1B, _) => "Escape",
                (0x20, true) => "CtrlSpace",
                (0x25, false) => "ArrowLeft",
                (0x27, false) => "ArrowRight",
                _ => return false,
            };
            let _ = app.emit_to("view", "host-key", name);
            true
        })
        .map(Arc::new)
        .map_err(|e| e.to_string())?;
        *h = Some(host.clone());
        Ok(host)
    }

    /// A handler that did not answer leaves its thread stuck: the next file gets a new one.
    fn drop_host(&self) {
        if let Ok(mut h) = self.host.lock() {
            h.take();
        }
    }

    /// The window is closing: let the handler go first.
    pub(super) fn close_host(&self) {
        let host = self.host.lock().ok().and_then(|mut h| h.take());
        if let Some(h) = host {
            h.unload_and_wait(Duration::from_millis(800));
        }
    }
}

/// Shows the file with its system preview handler over the page's area (`x`, `y`, `w`, `h` in
/// window pixels), in the page's colours (`0x00BBGGRR`). Waits up to 10 s: a handler that does
/// not answer by then is given up on.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn host_show(
    app: AppHandle,
    id: u64,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    bg: Option<u32>,
    fg: Option<u32>,
    state: tauri::State<'_, ViewState>,
) -> Result<String, String> {
    let path = path_of(&state, id)?;
    let parent = app
        .get_webview_window("view")
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
        .ok_or("no window")?;
    let host = state.host(&app)?;
    let area = handler::Area { x, y, w, h };
    let r =
        blocking(move || host.show(parent, &path, area, bg.zip(fg), Duration::from_secs(10))).await;
    if matches!(&r, Err(e) if e == "timeout") {
        state.drop_host();
    }
    r
}

#[tauri::command]
pub fn host_area(x: i32, y: i32, w: i32, h: i32, state: tauri::State<'_, ViewState>) {
    if let Some(host) = state.host.lock().ok().and_then(|h| h.clone()) {
        host.set_area(handler::Area { x, y, w, h });
    }
}

#[tauri::command]
pub fn host_unload(state: tauri::State<'_, ViewState>) {
    if let Some(host) = state.host.lock().ok().and_then(|h| h.clone()) {
        host.unload();
    }
}

pub fn window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    match event {
        // A system preview handler's window lies over the page in a popup: it follows.
        tauri::WindowEvent::Moved(_) if window.label() == "view" => {
            if let Some(state) = window.try_state::<ViewState>()
                && let Some(host) = state.host.lock().ok().and_then(|h| h.clone())
            {
                host.follow_window();
            }
        }
        // Moved to a display with another scale: icons for the new DPI.
        tauri::WindowEvent::ScaleFactorChanged { .. } => {
            if let Ok(h) = window.hwnd() {
                crate::win_icon::apply(h.0 as isize);
            }
        }
        tauri::WindowEvent::Destroyed => {
            if let Ok(h) = window.hwnd() {
                crate::win_icon::forget(h.0 as isize);
            }
        }
        _ => {}
    }
}

/// A new window's taskbar, Alt+Tab and title icons at its DPI (`win_icon`): Tauri sets only a
/// 32 px small icon, which the taskbar stretches.
pub fn window_icons(w: &WebviewWindow) {
    if let Ok(h) = w.hwnd() {
        crate::win_icon::apply(h.0 as isize);
    }
}

/// The agent sends files and "close" here.
pub fn serve_view(app: AppHandle) {
    let name = app.state::<ViewState>().pipe.clone();
    let handler: pipe::Handler = Arc::new(move |line: String| {
        match serde_json::from_str::<ViewCommand>(&line) {
            Ok(ViewCommand::Show { path, title }) => {
                let shown = app.state::<ViewState>().set(path, title);
                let _ = app.emit_to("view", "show", shown);
                Reply::ok().line()
            }
            Ok(ViewCommand::Close) => {
                let app = app.clone();
                // Answer before going (the agent waits a second at most); the handler goes first.
                std::thread::spawn(move || {
                    app.state::<ViewState>().close_host();
                    std::thread::sleep(Duration::from_millis(30));
                    app.exit(0);
                });
                Reply::ok().line()
            }
            Err(e) => Reply::err(e).line(),
        }
    });
    let _ = pipe::serve(&name, handler);
}

/// This process's pipe for the agent.
pub fn view_pipe() -> String {
    paths::pipe(&format!("view-{}", std::process::id())).unwrap_or_default()
}

/// A second "open settings" brings this window to the front.
pub fn serve_settings(app: AppHandle) {
    let Ok(name) = paths::pipe("settings") else {
        return;
    };
    let handler: pipe::Handler = Arc::new(move |_line: String| {
        if let Some(w) = app.get_webview_window("settings") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
        Reply::ok().line()
    });
    let _ = pipe::serve(&name, handler);
}

/// Asks a running settings window to come forward (it may take the focus: we were just started
/// by the user).
pub fn focus_existing_settings() -> bool {
    let Ok(name) = paths::pipe("settings") else {
        return false;
    };
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    pipe::request(&name, "focus", Duration::from_millis(500)).is_ok()
}
