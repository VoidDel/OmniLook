//! The windows elsewhere than on Windows (Linux, `docs/development-plan.md` §7): no agent and no
//! hotkey, a preview is a window of its own started from the command line (a file manager's
//! custom action). It takes the focus like any window; the arrow keys go through the files of its
//! folder; there are no system preview handlers.

use std::path::Path;

use tauri::{AppHandle, Emitter, Manager, WebviewWindow, WebviewWindowBuilder, Wry};

use super::{ViewState, sibling};

/// No preview handlers here.
pub type HostSlot = ();

pub fn view_window<'a>(
    b: WebviewWindowBuilder<'a, Wry, tauri::App>,
    _gpu: bool,
) -> WebviewWindowBuilder<'a, Wry, tauri::App> {
    b
}

pub fn settings_window<'a>(
    b: WebviewWindowBuilder<'a, Wry, tauri::App>,
) -> WebviewWindowBuilder<'a, Wry, tauri::App> {
    b
}

/// The first file is drawn: show the window in the middle of the screen, with the focus.
pub fn first_shown(app: &AppHandle, _state: &ViewState) {
    if let Some(w) = app.get_webview_window("view") {
        let _ = w.center();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn closing(_state: &ViewState) {}

pub fn settings_saved() {}

/// An arrow key: the next file of the folder, by name.
pub fn nav(app: &AppHandle, state: &ViewState, step: i32) {
    let Some(cur) = state.shown() else {
        return;
    };
    if let Some(next) = sibling(&cur.path, step) {
        let shown = state.set(next, None);
        let _ = app.emit_to("view", "show", shown);
    }
}

pub fn handlers(_path: &Path) -> bool {
    false
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn host_show(
    id: u64,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    bg: Option<u32>,
    fg: Option<u32>,
) -> Result<String, String> {
    let _ = (id, x, y, w, h, bg, fg);
    Err("no preview handler".into())
}

#[tauri::command]
pub fn host_area(x: i32, y: i32, w: i32, h: i32) {
    let _ = (x, y, w, h);
}

#[tauri::command]
pub fn host_unload() {}

pub fn window_event(_window: &tauri::Window, _event: &tauri::WindowEvent) {}

pub fn window_icons(_w: &WebviewWindow) {}

pub fn serve_view(_app: AppHandle) {}

pub fn view_pipe() -> String {
    String::new()
}

pub fn serve_settings(_app: AppHandle) {}
