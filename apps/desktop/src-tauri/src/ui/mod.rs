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

use omnilook_core::{Kind, archive, audio, detect, folder, hex, markdown, office, text};
use omnilook_win::{events::window_rect, handler, pipe};
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
    /// Media the window can play (`audio::UNPLAYABLE` cannot).
    playable: bool,
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
    /// The thread running system preview handlers, started on first use.
    host: Mutex<Option<Arc<handler::Host>>>,
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
            kind: view_kind(&path),
            playable: audio::playable(&path),
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
    fn close_host(&self) {
        let host = self.host.lock().ok().and_then(|mut h| h.take());
        if let Some(h) = host {
            h.unload_and_wait(Duration::from_millis(800));
        }
    }

    fn shown(&self) -> Option<Shown> {
        self.current.lock().ok()?.as_ref().map(|c| c.shown.clone())
    }

    /// The path and token of the file shown, if it is still `id`.
    fn token_of(&self, id: u64) -> Option<(PathBuf, String)> {
        self.current
            .lock()
            .ok()?
            .as_ref()
            .filter(|c| c.shown.id == id)
            .map(|c| (c.shown.path.clone(), c.token.clone()))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MarkdownPreview {
    html: String,
    truncated: bool,
    encoding: String,
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
fn view_close(app: AppHandle, state: tauri::State<'_, ViewState>) {
    state.close_host();
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

/// The path of the file shown, if it is still `id`.
fn path_of(state: &ViewState, id: u64) -> Result<PathBuf, String> {
    state
        .shown()
        .filter(|s| s.id == id)
        .map(|s| s.path)
        .ok_or_else(|| "stale".into())
}

/// Runs a previewer off the window's thread.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn read_text(
    id: u64,
    state: tauri::State<'_, ViewState>,
) -> Result<text::TextPreview, String> {
    let path = path_of(&state, id)?;
    blocking(move || text::read(&path).map_err(|e| e.to_string())).await
}

/// Token classes for the text (`text::highlight`), asked for after the plain text is shown.
/// The file is read again rather than the text sent back: reading is cheaper.
#[tauri::command]
async fn highlight(id: u64, state: tauri::State<'_, ViewState>) -> Result<Vec<u32>, String> {
    let path = path_of(&state, id)?;
    blocking(move || {
        let t = text::read(&path).map_err(|e| e.to_string())?;
        Ok(text::highlight(&path, &t.text))
    })
    .await
}

/// Markdown rendered and cleaned (`markdown::render`); its local images come from `look://`'s
/// `/r/<token>/<address>`, which serves only images, next to this file.
#[tauri::command]
async fn read_markdown(
    id: u64,
    state: tauri::State<'_, ViewState>,
) -> Result<MarkdownPreview, String> {
    let (path, token) = state.token_of(id).ok_or("stale")?;
    blocking(move || {
        let t = text::read(&path).map_err(|e| e.to_string())?;
        let html = markdown::render(&t.text, |dest| {
            Some(format!(
                "http://look.localhost/r/{token}/{}",
                look::encode(dest)
            ))
        });
        Ok(MarkdownPreview {
            html,
            truncated: t.truncated,
            encoding: t.encoding,
        })
    })
    .await
}

/// Tags, length and format of a sound file (and whether it has a cover, served at
/// `look://`'s `/c/<token>`).
#[tauri::command]
async fn read_audio(
    id: u64,
    state: tauri::State<'_, ViewState>,
) -> Result<audio::AudioInfo, String> {
    let path = path_of(&state, id)?;
    blocking(move || Ok(audio::read(&path))).await
}

/// The kind of preview, with the system's preview handlers: a type no built-in previewer reads
/// goes to its handler when the system has one (fonts, Outlook items, CAD viewers…). A type listed
/// for handlers that has none here (Word files without Office) gets the file card from the page.
fn view_kind(path: &std::path::Path) -> Kind {
    match detect(path) {
        Kind::Binary if handlers(path) => Kind::Handler,
        // Word, Excel, PowerPoint without Office: their text, read here.
        Kind::Handler if !handlers(path) && office::handles(path) => Kind::Office,
        k => k,
    }
}

/// Whether the system has a preview handler for the file. A test can pretend it has none
/// (`OMNILOOK_NO_HANDLERS`, isolated instances only), to see what a computer without Office shows.
fn handlers(path: &std::path::Path) -> bool {
    if paths::isolated() && std::env::var_os("OMNILOOK_NO_HANDLERS").is_some() {
        return false;
    }
    handler::available(path)
}

/// Word, Excel or PowerPoint text without Office (`office::read`).
#[tauri::command]
async fn read_office(
    id: u64,
    state: tauri::State<'_, ViewState>,
) -> Result<office::OfficePreview, String> {
    let path = path_of(&state, id)?;
    blocking(move || office::read(&path)).await
}

/// Shows the file with its system preview handler over the page's area (`x`, `y`, `w`, `h` in
/// window pixels), in the page's colours (`0x00BBGGRR`). Waits up to 10 s: a handler that does
/// not answer by then is given up on.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn host_show(
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
fn host_area(x: i32, y: i32, w: i32, h: i32, state: tauri::State<'_, ViewState>) {
    if let Some(host) = state.host.lock().ok().and_then(|h| h.clone()) {
        host.set_area(handler::Area { x, y, w, h });
    }
}

#[tauri::command]
fn host_unload(state: tauri::State<'_, ViewState>) {
    if let Some(host) = state.host.lock().ok().and_then(|h| h.clone()) {
        host.unload();
    }
}

/// A link clicked in a preview: web and mail addresses only.
#[tauri::command]
fn open_url(url: String) -> bool {
    system::open_url(&url)
}

#[tauri::command]
async fn read_hex(id: u64, state: tauri::State<'_, ViewState>) -> Result<hex::HexPreview, String> {
    let path = path_of(&state, id)?;
    blocking(move || hex::read(&path).map_err(|e| e.to_string())).await
}

#[tauri::command]
async fn read_folder(
    id: u64,
    state: tauri::State<'_, ViewState>,
) -> Result<folder::FolderPreview, String> {
    let path = path_of(&state, id)?;
    blocking(move || folder::read(&path, Duration::from_millis(400)).map_err(|e| e.to_string()))
        .await
}

#[tauri::command]
async fn read_archive(
    id: u64,
    locale: String,
    state: tauri::State<'_, ViewState>,
) -> Result<archive::ArchivePreview, String> {
    let path = path_of(&state, id)?;
    blocking(move || Ok(archive::read(&path, Some(&locale)))).await
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
        let pipe_name = paths::pipe(&format!("view-{}", std::process::id())).unwrap_or_default();
        let state = ViewState {
            current: Mutex::new(None),
            next: AtomicU64::new(0),
            anchor: args.anchor,
            placed: AtomicBool::new(false),
            pipe: pipe_name,
            host: Mutex::new(None),
        };
        state.set(args.path.clone(), args.title.clone());
        builder = builder
            .manage(state)
            .register_asynchronous_uri_scheme_protocol("look", |ctx, req, responder| {
                look::serve(ctx.app_handle(), req, responder)
            });
    }
    let result = builder
        .on_window_event(|window, event| match event {
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
        })
        .invoke_handler(tauri::generate_handler![
            view_ready,
            view_show,
            view_close,
            view_nav,
            open_default,
            read_text,
            highlight,
            read_hex,
            read_folder,
            read_archive,
            read_markdown,
            read_audio,
            read_office,
            host_show,
            host_area,
            host_unload,
            open_url,
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
                        .build()
                        .inspect(window_icons)?;
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
                    .build()
                    .inspect(window_icons)?;
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

/// A new window's taskbar, Alt+Tab and title icons at its DPI (`win_icon`): Tauri sets only a
/// 32 px small icon, which the taskbar stretches.
fn window_icons(w: &WebviewWindow) {
    if let Ok(h) = w.hwnd() {
        crate::win_icon::apply(h.0 as isize);
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

/// A second "open settings" brings this window to the front.
fn serve_settings_pipe(app: AppHandle) {
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

pub fn run_view(args: ViewArgs) -> ExitCode {
    run(Role::View(args))
}

pub fn run_settings() -> ExitCode {
    run(Role::Settings)
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
