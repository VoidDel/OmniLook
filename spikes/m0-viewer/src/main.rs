//! M0 spike (`docs/development-plan.md` §10 M0). Not product code: it answers whether a warm,
//! hidden Tauri window can show a preview fast and without taking the focus, whether WebView2
//! plays PDFs and media from a custom protocol, and whether system preview handlers can be
//! hosted in the window. Everything it measures goes to the log (`M0_LOG`, default
//! `%TEMP%\omnilook-m0.log`).
//!
//! `omnilook-m0 [--handler] <file>` previews a file (forwarded to the running instance; the same
//! file again closes it). In Explorer or on the desktop, Ctrl+Space previews the selection.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
mod host;
#[cfg(windows)]
mod protocol;

#[cfg(windows)]
fn main() {
    app::run();
}

#[cfg(windows)]
mod app {
    use std::{
        io::Write,
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
        time::{Duration, Instant, SystemTime},
    };

    use crate::{host, protocol};
    use omnilook_win::{
        explorer::{self, Com, View},
        hook::{self, HookEvent},
        trigger::Decision,
    };
    use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
    use windows::Win32::{
        Foundation::{HWND, RECT},
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowRect, HWND_NOTOPMOST, HWND_TOPMOST, SW_HIDE,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowPos, ShowWindow,
        },
    };

    pub struct Shown {
        pub id: u64,
        pub path: PathBuf,
        pub token: String,
        pub t0: Instant,
    }

    pub struct M0 {
        pub shown: Mutex<Option<Shown>>,
        next: AtomicU64,
        log: Mutex<std::fs::File>,
        start: Instant,
    }

    impl M0 {
        pub fn log(&self, line: impl AsRef<str>) {
            let ms = self.start.elapsed().as_millis();
            let line = format!("{ms:>8} ms  {}", line.as_ref());
            eprintln!("{line}");
            if let Ok(mut f) = self.log.lock() {
                let _ = writeln!(f, "{line}");
                let _ = f.flush();
            }
        }
    }

    fn kind(path: &Path) -> &'static str {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" | "avif" => "image",
            "txt" | "md" | "rs" | "json" | "toml" | "log" | "csv" | "xml" | "ini" | "js" | "ts"
            | "py" | "c" | "h" | "cpp" => "text",
            "pdf" => "pdf",
            "mp4" | "m4v" | "webm" | "mkv" | "mov" | "avi" | "wmv" | "ogv" | "flv" => "video",
            "mp3" | "wav" | "flac" | "ogg" | "oga" | "opus" | "m4a" | "aac" | "wma" => "audio",
            "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt" | "vsdx" | "rtf" | "msg" | "eml"
            | "ttf" | "otf" => "handler",
            _ => "other",
        }
    }

    fn window(app: &AppHandle) -> Option<(WebviewWindow, HWND)> {
        let w = app.get_webview_window("preview")?;
        let h = HWND(w.hwnd().ok()?.0);
        Some((w, h))
    }

/// WebView2's memory target: low while hidden ( turns it off to compare).
    pub fn memory_level(win: &WebviewWindow, low: bool) {
        if std::env::var_os("M0_LOWMEM").is_some_and(|v| v == "0") {
            return;
        }
        let _ = win.with_webview(move |wv| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
                ICoreWebView2_19,
            };
            use windows::core::Interface;
            let level = if low {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            };
            if let Ok(core) = wv.controller().CoreWebView2()
                && let Ok(c) = core.cast::<ICoreWebView2_19>()
            {
                let _ = c.SetMemoryUsageTargetLevel(level);
            }
        });
    }

    pub fn hide(app: &AppHandle) {
        let st = app.state::<Arc<M0>>();
        let _ = app.run_on_main_thread(host::unload);
        if let Some((w, h)) = window(app) {
            unsafe {
                let _ = SetWindowPos(
                    h,
                    Some(HWND_NOTOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
                let _ = ShowWindow(h, SW_HIDE);
            }
            let _ = w.emit("clear", ());
            memory_level(&w, true);
        }
        hook::set_preview_open(false);
        if let Some(s) = st.shown.lock().unwrap().take() {
            st.log(format!("#{} closed", s.id));
        }
    }

    fn class_of(h: HWND) -> String {
        explorer::class_name(h)
    }

    /// Shows `path`. `anchor` is the window to center on (the Explorer window).
    pub fn preview(
        app: &AppHandle,
        path: PathBuf,
        t0: Instant,
        source: &str,
        anchor: Option<isize>,
        force: Option<&'static str>,
    ) {
        let st = app.state::<Arc<M0>>().inner().clone();
        let Some((win, hwnd)) = window(app) else {
            return;
        };
        if source == "cli"
            && st
                .shown
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.path == path)
        {
            hide(app);
            return;
        }
        let _ = app.run_on_main_thread(host::unload);
        let id = st.next.fetch_add(1, Ordering::Relaxed) + 1;
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let token = format!("{id}x{nanos:x}");
        let k = force.unwrap_or_else(|| kind(&path));
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        *st.shown.lock().unwrap() = Some(Shown {
            id,
            path: path.clone(),
            token: token.clone(),
            t0,
        });
        st.log(format!(
            "#{id} {source} {k} {} ({size} bytes): selection read {} µs after the key",
            path.display(),
            t0.elapsed().as_micros()
        ));
        let _ = win.emit(
            "preview",
            serde_json::json!({
                "id": id,
                "name": name,
                "kind": k,
                "size": size,
                "url": format!("http://look.localhost/f/{token}"),
            }),
        );
        // Center on the Explorer window, or on the primary monitor; size by that monitor's scale.
        // Size and position go through Tauri (so the webview follows); only the showing is raw
        // Win32, to keep the focus where it is.
        let mut area = RECT::default();
        let anchored = anchor.is_some_and(|a| unsafe {
            GetWindowRect(HWND(a as *mut _), &mut area).is_ok()
        });
        let monitors = win.available_monitors().unwrap_or_default();
        let inside = |m: &tauri::Monitor, x: i32, y: i32| {
            let (p, s) = (m.position(), m.size());
            x >= p.x && y >= p.y && x < p.x + s.width as i32 && y < p.y + s.height as i32
        };
        let monitor = if anchored {
            let (cx, cy) = ((area.left + area.right) / 2, (area.top + area.bottom) / 2);
            monitors.iter().find(|m| inside(m, cx, cy)).cloned()
        } else {
            None
        }
        .or_else(|| win.primary_monitor().ok().flatten());
        if !anchored && let Some(m) = &monitor {
            let (p, s) = (m.position(), m.size());
            area = RECT {
                left: p.x,
                top: p.y,
                right: p.x + s.width as i32,
                bottom: p.y + s.height as i32,
            };
        }
        let scale = monitor.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);
        let (w, h) = ((960.0 * scale) as i32, (680.0 * scale) as i32);
        let x = (area.left + area.right - w) / 2;
        let y = (area.top + area.bottom - h) / 2;
        let t_place = Instant::now();
        let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
        let _ = win.set_size(tauri::PhysicalSize::new(w as u32, h as u32));
        let place_us = t_place.elapsed().as_micros();
        let before = unsafe { GetForegroundWindow() };
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        st.log(format!("#{id} placed at scale {scale} in {place_us} µs"));
        let after = unsafe { GetForegroundWindow() };
        memory_level(&win, false);
        hook::set_preview_open(true);
        st.log(format!(
            "#{id} shown {} µs after the key; foreground {} → {} ({})",
            t0.elapsed().as_micros(),
            class_of(before),
            class_of(after),
            if before == after {
                "focus kept"
            } else {
                "FOCUS TAKEN"
            }
        ));
    }

    fn on_hook(app: &AppHandle, e: HookEvent, current: &mut Option<PathBuf>) {
        let st = app.state::<Arc<M0>>();
        let open = st.shown.lock().unwrap().is_some();
        let f = &e.focus;
        st.log(format!(
            "key {:?} -> {:?} [{} | focus {} in {}] file_view={} {:?}",
            e.key, e.decision, f.foreground_class, f.focus_class, f.parent_class, f.in_file_view, e.mods
        ));
        match e.decision {
            Decision::Pass => {}
            Decision::Close => {
                hide(app);
                *current = None;
            }
            Decision::Toggle if open => {
                hide(app);
                *current = None;
            }
            Decision::Toggle | Decision::Follow => {
                if e.decision == Decision::Follow {
                    std::thread::sleep(Duration::from_millis(30));
                }
                let pick = View::of(&e.focus).and_then(|v| match v {
                    Some(v) => v.snapshot().map(Some),
                    None => Ok(None),
                });
                let pick = match pick {
                    Ok(Some(s)) => s
                        .focused
                        .clone()
                        .filter(|f| s.selected.contains(f))
                        .or_else(|| s.selected.first().cloned()),
                    Ok(None) => None,
                    Err(err) => {
                        st.log(format!("selection error: {err}"));
                        None
                    }
                };
                match pick {
                    Some(p) if current.as_ref() != Some(&p) => {
                        *current = Some(p.clone());
                        let source = if e.decision == Decision::Follow {
                            "follow"
                        } else {
                            "hotkey"
                        };
                        preview(app, p, e.at, source, Some(e.focus.foreground), None);
                    }
                    Some(_) => {}
                    None if e.decision == Decision::Toggle => st.log("hotkey: nothing selected"),
                    None => {}
                }
            }
        }
    }

    #[tauri::command]
    fn painted(app: AppHandle, id: u64, what: String, detail: String, page_ms: f64) {
        let st = app.state::<Arc<M0>>();
        let t0 = st
            .shown
            .lock()
            .unwrap()
            .as_ref()
            .filter(|s| s.id == id)
            .map(|s| s.t0);
        match t0 {
            Some(t0) => st.log(format!(
                "#{id} painted {what} {detail}: {} ms after the key (page {page_ms:.1} ms)",
                t0.elapsed().as_millis()
            )),
            None => st.log(format!("#{id} painted {what} {detail} (stale)")),
        }
    }

    #[tauri::command]
    fn page_log(app: AppHandle, line: String) {
        app.state::<Arc<M0>>().log(format!("page: {line}"));
    }

    #[tauri::command]
    fn close_preview(app: AppHandle) {
        hide(&app);
    }

    /// Runs on the main thread (a synchronous command), where COM is single-threaded.
    #[tauri::command]
    fn host_handler(
        app: AppHandle,
        id: u64,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<String, String> {
        let st = app.state::<Arc<M0>>();
        let path = st
            .shown
            .lock()
            .unwrap()
            .as_ref()
            .filter(|s| s.id == id)
            .map(|s| s.path.clone())
            .ok_or("stale")?;
        let (_, parent) = window(&app).ok_or("no window")?;
        let rect = RECT {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        };
        let r = host::show(parent, &path, rect);
        st.log(format!("#{id} handler: {r:?}"));
        r
    }

    #[tauri::command]
    fn handler_rect(x: i32, y: i32, w: i32, h: i32) {
        host::resize(RECT {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        });
    }

    fn file_arg(argv: &[String]) -> Option<(PathBuf, Option<&'static str>)> {
        let force = argv.iter().any(|a| a == "--handler").then_some("handler");
        argv.iter()
            .skip(1)
            .find(|a| !a.starts_with("--"))
            .map(|p| (PathBuf::from(p), force))
    }

    pub fn run() {
        let log_path = std::env::var_os("M0_LOG")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("omnilook-m0.log"));
        let st = Arc::new(M0 {
            shown: Mutex::new(None),
            next: AtomicU64::new(0),
            // Append: a second instance (forwarding its arguments) opens the same log.
            log: Mutex::new(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path)
                    .expect("log file"),
            ),
            start: Instant::now(),
        });
        st.log(format!("start pid {}", std::process::id()));
        tauri::Builder::default()
            .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
                if argv.iter().any(|a| a == "--quit") {
                    app.state::<Arc<M0>>().log("quit");
                    app.exit(0);
                } else if let Some((p, force)) = file_arg(&argv) {
                    preview(app, p, Instant::now(), "cli", None, force);
                }
            }))
            .manage(st.clone())
            .register_asynchronous_uri_scheme_protocol("look", |ctx, req, responder| {
                protocol::serve(ctx.app_handle(), req, responder)
            })
            .invoke_handler(tauri::generate_handler![
                painted,
                page_log,
                close_preview,
                host_handler,
                handler_rect
            ])
            .setup(move |app| {
                WebviewWindowBuilder::new(app, "preview", WebviewUrl::App("index.html".into()))
                    .title("Omni Look M0")
                    .inner_size(960.0, 680.0)
                    .visible(false)
                    .focused(false)
                    .skip_taskbar(true)
                    .build()?;
                if let Some(w) = app.get_webview_window("preview") {
                    memory_level(&w, true);
                }
                st.log("window created (hidden)");
                let (tx, rx) = mpsc::channel::<HookEvent>();
                let hook = hook::install(move |e| {
                    let _ = tx.send(e);
                })
                .map_err(|e| e.to_string())?;
                let handle = app.handle().clone();
                std::thread::Builder::new()
                    .name("shell".into())
                    .spawn(move || {
                        let _hook = hook;
                        let _com = Com::init();
                        let mut current = None;
                        for e in rx {
                            on_hook(&handle, e, &mut current);
                        }
                    })?;
                st.log("hook installed");
                let argv: Vec<String> = std::env::args().collect();
                if let Some((p, force)) = file_arg(&argv) {
                    let handle = app.handle().clone();
                    std::thread::spawn(move || {
                        // Let the page load first.
                        std::thread::sleep(Duration::from_millis(1500));
                        preview(&handle, p, Instant::now(), "cli", None, force);
                    });
                }
                Ok(())
            })
            .run(tauri::generate_context!())
            .expect("run");
    }
}
