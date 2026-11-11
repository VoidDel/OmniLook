//! The two WebView windows (`docs/development-plan.md` §3.1), each in its own short-lived
//! process: the preview (`view`, started by the agent per preview, exits when closed) and the
//! settings (`settings`, started from the Start menu).

mod look;

use std::{
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use omnilook_core::{Kind, detect};
use omnilook_win::{events::window_rect, pipe};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::{
        ASFW_ANY, AllowSetForegroundWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_SHOWWINDOW, SetWindowPos,
    },
};

use crate::{
    cli::ViewArgs,
    client, paths,
    protocol::{Reply, Request, ViewCommand},
    settings::Settings,
    system,
};

/// The file a preview shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    id: u64,
    path: PathBuf,
    name: String,
    title: Option<String>,
    kind: Kind,
    size: u64,
    /// Milliseconds since 1970.
    modified: Option<u64>,
    /// The file through `look://`, for images, PDF and media.
    url: String,
}

struct Current {
    shown: Shown,
    token: String,
}

pub struct ViewState {
    current: Mutex<Option<Current>>,
    next: AtomicU64,
    anchor: Option<isize>,
    /// The window has been shown once (later files keep its place).
    placed: AtomicBool,
    /// This process's pipe, for the agent.
    pipe: String,
}

impl ViewState {
    fn path_for(&self, token: &str) -> Option<PathBuf> {
        self.current
            .lock()
            .ok()?
            .as_ref()
            .filter(|c| c.token == token)
            .map(|c| c.shown.path.clone())
    }

    fn set(&self, path: PathBuf, title: Option<String>) -> Shown {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let token = format!("{id:x}{:x}{nanos:x}", std::process::id());
        let meta = std::fs::metadata(&path).ok();
        let shown = Shown {
            id,
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
            title,
            kind: detect(&path),
            size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            modified: meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64),
            url: format!("http://look.localhost/f/{token}"),
            path,
        };
        if let Ok(mut c) = self.current.lock() {
            *c = Some(Current {
                shown: shown.clone(),
                token,
            });
        }
        shown
    }

    fn shown(&self) -> Option<Shown> {
        self.current.lock().ok()?.as_ref().map(|c| c.shown.clone())
    }
}

/// `--disable-gpu` halves WebView2's memory; only video needs the GPU (M0, `docs/m0-results.md`).
fn browser_args(gpu: bool) -> String {
    let mut a = String::from("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
    if !gpu {
        a.push_str(" --disable-gpu");
    }
    a
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

// --- preview commands -------------------------------------------------------------------------

/// The page is listening: what to show first.
#[tauri::command]
fn view_ready(state: tauri::State<'_, ViewState>) -> Option<Shown> {
    state.shown()
}

/// The first file is drawn: show the window (once) and tell the agent files can come.
#[tauri::command]
fn view_show(app: AppHandle, state: tauri::State<'_, ViewState>) {
    if state.placed.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(w) = app.get_webview_window("view") {
        place_and_show(&w, state.anchor);
    }
    notify_agent(Request::ViewReady {
        pid: std::process::id(),
        pipe: state.pipe.clone(),
    });
}

#[tauri::command]
fn view_close(app: AppHandle) {
    let _ = client::send(
        &Request::ViewClosed {
            pid: std::process::id(),
        },
        Duration::from_secs(1),
    );
    app.exit(0);
}

#[tauri::command]
fn view_nav(step: i32) {
    notify_agent(Request::Nav {
        pid: std::process::id(),
        step,
    });
}

#[tauri::command]
fn open_default(state: tauri::State<'_, ViewState>) -> bool {
    state.shown().is_some_and(|s| system::open_default(&s.path))
}

/// Text of the file shown (`id`): the first 2 MB, decoded as UTF-8.
#[tauri::command]
fn read_text(id: u64, state: tauri::State<'_, ViewState>) -> Result<String, String> {
    let s = state.shown().filter(|s| s.id == id).ok_or("stale")?;
    let mut f = std::fs::File::open(&s.path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(&mut f, 2 << 20), &mut buf)
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

// --- settings commands ------------------------------------------------------------------------

#[tauri::command]
fn get_settings() -> Settings {
    Settings::load()
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), String> {
    settings.save().map_err(|e| e.to_string())?;
    if !paths::isolated() {
        system::set_autostart(settings.autostart);
    }
    notify_agent(Request::Reload);
    Ok(())
}

#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// --- processes --------------------------------------------------------------------------------

enum Role {
    View(ViewArgs),
    Settings,
}

fn run(role: Role) -> ExitCode {
    let gpu = matches!(&role, Role::View(a) if a.gpu);
    let mut builder = tauri::Builder::default();
    if let Role::View(args) = &role {
        let pipe_name = pipe::pipe_name(&format!("view-{}", std::process::id())).unwrap_or_default();
        let state = ViewState {
            current: Mutex::new(None),
            next: AtomicU64::new(0),
            anchor: args.anchor,
            placed: AtomicBool::new(false),
            pipe: pipe_name,
        };
        state.set(args.path.clone(), args.title.clone());
        builder = builder
            .manage(state)
            .register_asynchronous_uri_scheme_protocol("look", |ctx, req, responder| {
                look::serve(ctx.app_handle(), req, responder)
            });
    }
    let result = builder
        .invoke_handler(tauri::generate_handler![
            view_ready,
            view_show,
            view_close,
            view_nav,
            open_default,
            read_text,
            get_settings,
            save_settings,
            app_version
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            match &role {
                Role::View(args) => {
                    let name = args
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    WebviewWindowBuilder::new(app, "view", WebviewUrl::App("index.html".into()))
                        .title(format!("{name} - Omni Look"))
                        .inner_size(960.0, 680.0)
                        .min_inner_size(420.0, 280.0)
                        .decorations(false)
                        .visible(false)
                        .focused(false)
                        .skip_taskbar(true)
                        .additional_browser_args(&browser_args(gpu))
                        .data_directory(paths::webview_dir(gpu))
                        .build()?;
                    serve_view_pipe(handle);
                }
                Role::Settings => {
                    WebviewWindowBuilder::new(
                        app,
                        "settings",
                        WebviewUrl::App("index.html".into()),
                    )
                    .title("Omni Look")
                    .inner_size(760.0, 560.0)
                    .min_inner_size(560.0, 420.0)
                    .decorations(false)
                    .visible(false)
                    .additional_browser_args(&browser_args(false))
                    .data_directory(paths::webview_dir(false))
                    .build()?;
                    serve_settings_pipe(handle);
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!());
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

/// The agent sends files and "close" here.
fn serve_view_pipe(app: AppHandle) {
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
                // Answer before going.
                std::thread::spawn(move || {
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

/// A second "open settings" brings this window to the front.
fn serve_settings_pipe(app: AppHandle) {
    let Ok(name) = pipe::pipe_name("settings") else {
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

pub fn run_view(args: ViewArgs) -> ExitCode {
    run(Role::View(args))
}

pub fn run_settings() -> ExitCode {
    run(Role::Settings)
}

/// Asks a running settings window to come forward (it may take the focus: we were just started
/// by the user).
pub fn focus_existing_settings() -> bool {
    let Ok(name) = pipe::pipe_name("settings") else {
        return false;
    };
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    pipe::request(&name, "focus", Duration::from_millis(500)).is_ok()
}
