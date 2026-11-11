//! The two WebView windows (`docs/development-plan.md` §3.1), each in its own short-lived
//! process: the preview (`view`, started by the agent per preview, exits when closed) and the
//! settings (`settings`, started from the Start menu).

mod look;
#[cfg(not(windows))]
mod other;
#[cfg(windows)]
mod win;

#[cfg(not(windows))]
use other as platform;
#[cfg(windows)]
use win as platform;

use std::{
    path::PathBuf,
    process::ExitCode,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use crate::{cli::ViewArgs, paths, settings::Settings, system, update};
use omnilook_core::{Kind, archive, audio, detect, folder, font, hex, markdown, office, text};
use serde::Serialize;
use tauri::{AppHandle, WebviewUrl, WebviewWindowBuilder};

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
    /// The Explorer window to center on (Windows).
    #[cfg_attr(not(windows), allow(dead_code))]
    anchor: Option<isize>,
    /// The window has been shown once (later files keep its place).
    placed: AtomicBool,
    /// This process's pipe, for the agent (Windows).
    #[cfg_attr(not(windows), allow(dead_code))]
    pipe: String,
    /// The thread running system preview handlers (Windows), started on first use.
    #[cfg_attr(not(windows), allow(dead_code))]
    host: platform::HostSlot,
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
            url: format!("{}/f/{token}", look::BASE),
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
    platform::first_shown(&app, &state);
}

#[tauri::command]
fn view_close(app: AppHandle, state: tauri::State<'_, ViewState>) {
    platform::closing(&state);
    app.exit(0);
}

#[tauri::command]
fn view_nav(app: AppHandle, state: tauri::State<'_, ViewState>, step: i32) {
    platform::nav(&app, &state, step);
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
            Some(format!("{}/r/{token}/{}", look::BASE, look::encode(dest)))
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
        Kind::Binary if platform::handlers(path) => Kind::Handler,
        // Word, Excel, PowerPoint without Office: their text, read here.
        Kind::Handler if !platform::handlers(path) && office::handles(path) => Kind::Office,
        k => k,
    }
}

/// What a font file says about itself, names in the UI language where it has them.
#[tauri::command]
async fn read_font(
    id: u64,
    locale: String,
    state: tauri::State<'_, ViewState>,
) -> Result<font::FontInfo, String> {
    let path = path_of(&state, id)?;
    blocking(move || Ok(font::read(&path, &locale))).await
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
    #[cfg(windows)]
    if !paths::isolated() {
        system::set_autostart(settings.autostart);
    }
    platform::settings_saved();
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
    /// No window: the agent's daily update check.
    UpdateCheck,
}

fn run(role: Role) -> ExitCode {
    let gpu = matches!(&role, Role::View(a) if a.gpu);
    let mut builder = tauri::Builder::default();
    if let Role::View(args) = &role {
        let pipe_name = platform::view_pipe();
        let state = ViewState {
            current: Mutex::new(None),
            next: AtomicU64::new(0),
            anchor: args.anchor,
            placed: AtomicBool::new(false),
            pipe: pipe_name,
            host: Default::default(),
        };
        state.set(args.path.clone(), args.title.clone());
        builder = builder
            .manage(state)
            .register_asynchronous_uri_scheme_protocol("look", |ctx, req, responder| {
                look::serve(ctx.app_handle(), req, responder)
            });
    }
    if !matches!(role, Role::View(_)) {
        builder = builder
            .plugin(tauri_plugin_updater::Builder::new().build())
            .manage(update::Pending::default());
    }
    let result = builder
        .on_window_event(platform::window_event)
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
            read_font,
            platform::host_show,
            platform::host_area,
            platform::host_unload,
            update::update_check,
            update::update_install,
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
                    let b = WebviewWindowBuilder::new(
                        app,
                        "view",
                        WebviewUrl::App("index.html".into()),
                    )
                    .title(format!("{name} - Omni Look"))
                    .inner_size(960.0, 680.0)
                    .min_inner_size(420.0, 280.0)
                    .decorations(false)
                    .visible(false)
                    .data_directory(paths::webview_dir(gpu));
                    platform::view_window(b, gpu)
                        .build()
                        .inspect(platform::window_icons)?;
                    platform::serve_view(handle);
                }
                Role::Settings => {
                    let b = WebviewWindowBuilder::new(
                        app,
                        "settings",
                        WebviewUrl::App("index.html".into()),
                    )
                    .title("Omni Look")
                    .inner_size(760.0, 560.0)
                    .min_inner_size(560.0, 420.0)
                    .decorations(false)
                    .visible(false)
                    .data_directory(paths::webview_dir(false));
                    platform::settings_window(b)
                        .build()
                        .inspect(platform::window_icons)?;
                    platform::serve_settings(handle);
                }
                Role::UpdateCheck => {
                    tauri::async_runtime::spawn(async move {
                        update::background_check(&handle).await;
                        handle.exit(0);
                    });
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

pub fn run_view(args: ViewArgs) -> ExitCode {
    run(Role::View(args))
}

pub fn run_settings() -> ExitCode {
    run(Role::Settings)
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn run_update_check() -> ExitCode {
    run(Role::UpdateCheck)
}

/// Asks a running settings window to come forward (Windows: the agent and its pipes).
#[cfg(windows)]
pub fn focus_existing_settings() -> bool {
    platform::focus_existing_settings()
}

/// The file `step` places from `path` among the files of its folder, by name (natural order:
/// `img2` before `img10`), hidden ones left out. `None` at either end.
#[cfg_attr(windows, allow(dead_code))]
pub fn sibling(path: &std::path::Path, step: i32) -> Option<PathBuf> {
    let dir = path.parent()?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| !n.to_string_lossy().starts_with('.'))
        })
        .collect();
    let key = |p: &PathBuf| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    files.sort_by(|a, b| natural(&key(a), &key(b)));
    let i = files.iter().position(|p| p == path)? as i64 + i64::from(step);
    usize::try_from(i).ok().and_then(|i| files.get(i)).cloned()
}

/// Compares names with runs of digits by their value.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut n = String::new();
                while let Some(c) = x.next_if(char::is_ascii_digit) {
                    n.push(c);
                }
                let mut m = String::new();
                while let Some(d) = y.next_if(char::is_ascii_digit) {
                    m.push(d);
                }
                let (n, m) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let o = n.len().cmp(&m.len()).then_with(|| n.cmp(m));
                if o.is_ne() {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.cmp(&d);
                if o.is_ne() {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siblings_in_natural_order() {
        let d = std::env::temp_dir().join(format!("omnilook-sib-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        for n in ["img10.png", "img2.png", "a.txt", ".hidden"] {
            std::fs::write(d.join(n), b"x").unwrap();
        }
        let name = |p: Option<PathBuf>| {
            p.and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        };
        assert_eq!(
            name(sibling(&d.join("a.txt"), 1)).as_deref(),
            Some("img2.png")
        );
        assert_eq!(
            name(sibling(&d.join("img2.png"), 1)).as_deref(),
            Some("img10.png")
        );
        assert_eq!(sibling(&d.join("img10.png"), 1), None);
        assert_eq!(sibling(&d.join("a.txt"), -1), None);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
