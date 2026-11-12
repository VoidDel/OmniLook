//! The resident agent (`docs/development-plan.md` §3.1, §4): the hotkey hook, the pipe the command
//! line and the preview process talk to, and the preview process's life. No WebView here: the
//! agent stays at a megabyte or two.
//!
//! Everything happens on the main thread, which pumps messages: Explorer calls the selection
//! sink there, and the hook, the pipe and the waiting threads post their events to it.

use std::{
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

/// How long after a preview opens a foreground change is taken as part of opening it.
const SETTLE: Duration = Duration::from_millis(1000);
/// How long a window must stay in front before the preview closes for it.
const RECHECK: Duration = Duration::from_millis(300);

use omnilook_core::detect;
use omnilook_win::{
    events::Subscription,
    explorer::{Com, Focus, View},
    foreground::{self, Home},
    hook::{self, HookEvent},
    pipe,
    trigger::{self, Decision},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
    UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, IsWindow, MSG, PostQuitMessage, PostThreadMessageW,
        TranslateMessage, WM_APP,
    },
};

use crate::{
    paths,
    protocol::{NavAnswer, NavAsk, Reply, Request, ViewCommand},
    settings::{Hotkey, Settings},
    system::{self, Log},
    tray, update,
};

enum Event {
    Key(HookEvent),
    /// Explorer's selection changed in the view being followed.
    Selection,
    Request(Request, mpsc::Sender<Reply>),
    /// A preview process ended (closed, crashed or stopped).
    ViewExited(u32),
    /// Another window came to the front (its handle, as an integer to cross threads).
    Foreground(isize),
    /// A while after a window came to the front that would close the preview.
    ForegroundAgain,
    /// Time to see whether a daily update check is due.
    UpdateTick,
    /// The caller answered which file is next to `from` (§6.3).
    NavDone {
        from: PathBuf,
        answer: Option<NavAnswer>,
    },
}

/// Sends an event to the main thread and wakes its message loop.
#[derive(Clone)]
struct Wake {
    tx: mpsc::Sender<Event>,
    thread: u32,
}

impl Wake {
    fn send(&self, e: Event) {
        if self.tx.send(e).is_ok() {
            unsafe {
                let _ = PostThreadMessageW(self.thread, WM_APP, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// The preview process.
struct ViewProc {
    pid: u32,
    gpu: bool,
    /// Its own pipe, once it is ready for files.
    pipe: Option<String>,
    /// The file it shows (or will, once ready) and the one wanted now.
    has: PathBuf,
    want: PathBuf,
    title: Option<String>,
}

/// The Explorer view a preview follows (opened with the hotkey).
struct Followed {
    view: View,
    _sub: Option<Subscription>,
    window: HWND,
}

struct Agent {
    wake: Wake,
    log: Log,
    view: Option<ViewProc>,
    followed: Option<Followed>,
    /// The window the preview was opened from (`foreground::stays`).
    home: Option<Home>,
    /// When the preview opened from `home`: the foreground often changes once just after (the
    /// Run dialog or a launcher closing), and that change moves the home instead of closing.
    opened: Option<Instant>,
    /// Close the preview when another window comes to the front (a setting).
    close_on_blur: bool,
    /// The caller's pipe for the previous / next file (OmniZip inside an archive, §6.3).
    caller_nav: Option<String>,
    /// A question to it is under way: further arrow keys wait for its answer.
    nav_busy: bool,
}

fn set_hotkey(s: &Settings) {
    hook::set_hotkey(match s.hotkey {
        Hotkey::CtrlSpace => trigger::Hotkey::CtrlSpace,
        Hotkey::Space => trigger::Hotkey::Space,
    });
}

/// The file a preview opens on: the focused item when it is selected, else the first selected.
fn pick(view: &View) -> Option<PathBuf> {
    let s = view.snapshot().ok()?;
    s.focused
        .clone()
        .filter(|f| s.selected.contains(f))
        .or_else(|| s.selected.first().cloned())
}

impl Agent {
    fn handle(&mut self, e: Event) -> bool {
        match e {
            Event::Key(k) => self.key(k),
            Event::Selection => self.follow(),
            Event::Request(r, reply) => {
                let quit = matches!(r, Request::Quit);
                let _ = reply.send(self.request(r));
                if quit {
                    self.close_view();
                    return false;
                }
            }
            Event::ViewExited(pid) => {
                if self.view.as_ref().is_some_and(|v| v.pid == pid) {
                    self.log.line(format!("view {pid} exited"));
                    self.view_gone();
                }
            }
            Event::NavDone { from, answer } => {
                self.nav_busy = false;
                let current = self.view.as_ref().is_some_and(|v| v.want == from);
                match answer {
                    Some(NavAnswer {
                        path: Some(path),
                        title,
                        ..
                    }) if current => self.show(path, title, None),
                    Some(NavAnswer { error: Some(e), .. }) => {
                        self.log.line(format!("caller's navigation: {e}"));
                    }
                    _ => {}
                }
            }
            Event::UpdateTick => {
                if !paths::isolated() && Settings::load().check_updates && update::due() {
                    self.log.line("update check");
                    let _ = Command::new(system::exe())
                        .arg("--update-check")
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .creation_flags(0x0800_0000)
                        .spawn();
                }
            }
            Event::Foreground(h) => self.foreground(HWND(h as *mut _)),
            Event::ForegroundAgain => self.foreground_again(),
        }
        true
    }

    /// Closes the preview once the user has moved to a window other than the preview and the one
    /// it was opened from.
    fn foreground(&mut self, hwnd: HWND) {
        let Some(v) = &self.view else {
            return;
        };
        if !self.close_on_blur {
            return;
        }
        let w = foreground::describe(hwnd);
        if self.opened.is_some_and(|t| t.elapsed() < SETTLE) {
            if w.pid != v.pid && w.handle != 0 && w.shown {
                self.home = foreground::home_of(hwnd);
            }
            return;
        }
        // A window can be in front for a moment only (a preview handler's server starting), and
        // a new one comes to the front before it is shown: look again before closing.
        let seen = foreground::Window { shown: true, ..w };
        if !foreground::stays(&seen, self.home, v.pid) {
            let wake = self.wake.clone();
            std::thread::spawn(move || {
                std::thread::sleep(RECHECK);
                wake.send(Event::ForegroundAgain);
            });
        }
    }

    /// The window in front a moment after one that would close the preview: closes it if that
    /// is still somewhere else.
    fn foreground_again(&mut self) {
        let Some(v) = &self.view else {
            return;
        };
        if !self.close_on_blur {
            return;
        }
        let w = foreground::current();
        if !foreground::stays(&w, self.home, v.pid) {
            self.log.line(format!(
                "foreground {} ({}): closing the preview",
                w.root_class, w.pid
            ));
            self.close_view();
        }
    }

    fn key(&mut self, k: HookEvent) {
        match k.decision {
            Decision::Pass => {}
            Decision::Close => self.close_view(),
            Decision::Toggle if self.view.is_some() => self.close_view(),
            Decision::Toggle => self.open_from(&k.focus),
            // The arrow key reaches Explorer after the hook; look once it has moved.
            Decision::Follow => {
                std::thread::sleep(Duration::from_millis(30));
                self.follow();
            }
        }
    }

    fn open_from(&mut self, focus: &Focus) {
        let view = match View::of(focus) {
            Ok(Some(v)) => v,
            Ok(None) => return,
            Err(e) => {
                self.log.line(format!("selection: {e}"));
                return;
            }
        };
        let Some(path) = pick(&view) else {
            return;
        };
        let wake = self.wake.clone();
        let sub = view
            .subscribe(move || wake.send(Event::Selection))
            .map_err(|e| self.log.line(format!("subscribe: {e}")))
            .ok();
        let window = focus.window();
        self.home = foreground::home_of(window);
        self.opened = Some(Instant::now());
        self.followed = Some(Followed {
            view,
            _sub: sub,
            window,
        });
        self.show(path, None, Some(window));
    }

    fn follow(&mut self) {
        if self.view.is_none() {
            return;
        }
        let Some(f) = &self.followed else {
            return;
        };
        if !unsafe { IsWindow(Some(f.window)) }.as_bool() {
            self.close_view();
            return;
        }
        if let Some(p) = pick(&f.view)
            && self.view.as_ref().is_some_and(|v| v.want != p)
        {
            self.show(p, None, None);
        }
    }

    fn spawn(&mut self, path: PathBuf, title: Option<String>, anchor: Option<HWND>, gpu: bool) {
        let mut cmd = Command::new(system::exe());
        cmd.arg("--view").arg("--path").arg(&path);
        if let Some(t) = &title {
            cmd.arg("--title").arg(t);
        }
        if let Some(a) = anchor {
            cmd.arg("--anchor").arg((a.0 as isize).to_string());
        }
        if gpu {
            cmd.arg("--gpu");
        }
        // CREATE_NO_WINDOW: no console flashes for a debug build.
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match cmd.creation_flags(0x0800_0000).spawn() {
            Ok(mut child) => {
                let pid = child.id();
                self.log
                    .line(format!("view {pid} for {} (gpu {gpu})", path.display()));
                let wake = self.wake.clone();
                std::thread::spawn(move || {
                    let _ = child.wait();
                    wake.send(Event::ViewExited(pid));
                });
                self.view = Some(ViewProc {
                    pid,
                    gpu,
                    pipe: None,
                    has: path.clone(),
                    want: path,
                    title,
                });
                hook::set_preview_open(true);
            }
            Err(e) => self.log.line(format!("spawn view: {e}")),
        }
    }

    fn send_view(pipe: &str, c: &ViewCommand) -> bool {
        let line = serde_json::to_string(c).unwrap_or_default();
        pipe::request(pipe, &line, Duration::from_secs(1))
            .map(|a| Reply::parse(&a).ok)
            .unwrap_or(false)
    }

    /// Shows `path`: in the running preview when it has the right GPU mode, else in a new one.
    fn show(&mut self, path: PathBuf, title: Option<String>, anchor: Option<HWND>) {
        // Video that cannot be played here shows its details only: no GPU needed.
        let gpu = detect(&path).wants_gpu() && omnilook_core::audio::playable(&path);
        if let Some(v) = &mut self.view
            && v.gpu == gpu
        {
            v.want = path.clone();
            v.title = title.clone();
            let Some(p) = v.pipe.clone() else {
                return; // sent once it is ready
            };
            let show = ViewCommand::Show {
                path: path.clone(),
                title: title.clone(),
            };
            if Self::send_view(&p, &show) {
                if let Some(v) = &mut self.view {
                    v.has = path;
                }
                return;
            }
        }
        // Another GPU mode, or the preview stopped answering: start again.
        let (followed, home, opened) = (self.followed.take(), self.home, self.opened);
        let caller_nav = self.caller_nav.take();
        let anchor = anchor.or(followed.as_ref().map(|f| f.window));
        self.close_view();
        self.followed = followed;
        self.home = home;
        self.opened = opened;
        self.caller_nav = caller_nav;
        self.spawn(path, title, anchor, gpu);
    }

    fn close_view(&mut self) {
        if let Some(v) = self.view.take() {
            let closed = v
                .pipe
                .as_deref()
                .is_some_and(|p| Self::send_view(p, &ViewCommand::Close));
            if !closed {
                system::terminate(v.pid);
            }
        }
        self.view_gone();
    }

    fn view_gone(&mut self) {
        self.view = None;
        self.followed = None;
        self.caller_nav = None;
        self.home = None;
        self.opened = None;
        hook::set_preview_open(false);
    }

    /// The file `step` away from the one shown, in the order Explorer shows them; selecting it
    /// there makes the preview follow, as an arrow key in Explorer would.
    fn nav(&mut self, step: i32) -> Reply {
        // From OmniZip inside an archive: ask it, off this thread (it may unpack the file).
        if self.followed.is_none()
            && let (Some(pipe), Some(v)) = (self.caller_nav.clone(), &self.view)
        {
            if self.nav_busy {
                return Reply::ok();
            }
            self.nav_busy = true;
            let from = v.want.clone();
            let ask = NavAsk {
                op: if step < 0 { "prev" } else { "next" }.into(),
                path: from.clone(),
            };
            let wake = self.wake.clone();
            std::thread::spawn(move || {
                let line = serde_json::to_string(&ask).unwrap_or_default();
                let answer = pipe::request(&pipe, &line, Duration::from_secs(2))
                    .ok()
                    .and_then(|a| serde_json::from_str::<NavAnswer>(&a).ok());
                wake.send(Event::NavDone { from, answer });
            });
            return Reply::ok();
        }
        let (Some(f), Some(v)) = (&self.followed, &self.view) else {
            return Reply::err("not following an Explorer view");
        };
        let items = match f.view.items() {
            Ok(i) => i,
            Err(e) => return Reply::err(e),
        };
        let Some(i) = items.iter().position(|p| *p == v.want) else {
            return Reply::err("not in the view");
        };
        let j = i as i64 + i64::from(step);
        let Some(next) = usize::try_from(j).ok().and_then(|j| items.get(j)).cloned() else {
            return Reply::ok(); // at an end
        };
        let _ = f.view.select(&next);
        self.show(next, None, None);
        Reply::ok()
    }

    fn request(&mut self, r: Request) -> Reply {
        match r {
            Request::Ping => Reply::ok(),
            Request::Preview { path, title, nav } => {
                if self.view.as_ref().is_some_and(|v| v.want == path) {
                    self.close_view();
                } else {
                    // Not from Explorer: nothing to follow. The caller's window (OmniZip's,
                    // a terminal) is in front now, and is where the preview comes from.
                    self.followed = None;
                    self.home = foreground::current_home();
                    self.opened = Some(Instant::now());
                    self.caller_nav = nav;
                    self.show(path, title, None);
                }
                Reply::ok()
            }
            Request::ViewReady { pid, pipe } => {
                let mut resend = None;
                if let Some(v) = &mut self.view
                    && v.pid == pid
                {
                    v.pipe = Some(pipe.clone());
                    if v.want != v.has {
                        resend = Some((v.want.clone(), v.title.clone()));
                    }
                }
                if let Some((path, title)) = resend {
                    // Answer first: the preview waits for this reply.
                    std::thread::spawn(move || {
                        let _ = Self::send_view(&pipe, &ViewCommand::Show { path, title });
                    });
                    if let Some(v) = &mut self.view {
                        v.has = v.want.clone();
                    }
                }
                Reply::ok()
            }
            Request::ViewClosed { pid } => {
                if self.view.as_ref().is_some_and(|v| v.pid == pid) {
                    self.view_gone();
                }
                Reply::ok()
            }
            Request::Nav { pid, step } => {
                if self.view.as_ref().is_some_and(|v| v.pid == pid) {
                    self.nav(step)
                } else {
                    Reply::err("not the current preview")
                }
            }
            Request::Reload => {
                let s = Settings::load();
                set_hotkey(&s);
                tray::set(s.tray, &system::ui_language(&s.language));
                self.close_on_blur = s.close_on_blur;
                Reply::ok()
            }
            Request::Quit => Reply::ok(),
        }
    }
}

pub fn run() -> ExitCode {
    // The agent has no Tauri window, so nothing makes it DPI aware (Tauri windows do that when
    // they start): Windows would hand it 96 DPI and stretch its tray icon and menu at 150 % or
    // 175 %. Per-monitor aware like the windows; refused only if already set.
    // SAFETY: a plain Win32 call, before any window of this process exists.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let log = Log::open("agent.log");
    let _com = match Com::init() {
        Ok(c) => c,
        Err(e) => {
            log.line(format!("COM: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let (tx, rx) = mpsc::channel();
    let wake = Wake {
        tx,
        thread: unsafe { GetCurrentThreadId() },
    };
    let Ok(name) = paths::pipe("agent") else {
        return ExitCode::FAILURE;
    };
    let w = wake.clone();
    let handler: pipe::Handler = Arc::new(move |line: String| {
        let req = match serde_json::from_str::<Request>(&line) {
            Ok(r) => r,
            Err(e) => return Reply::err(e).line(),
        };
        let (rtx, rrx) = mpsc::channel();
        w.send(Event::Request(req, rtx));
        rrx.recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| Reply::err("agent busy"))
            .line()
    });
    if pipe::serve(&name, handler).is_err() {
        // Another agent has the pipe.
        return ExitCode::SUCCESS;
    }
    log.line(format!("agent {} started on {name}", std::process::id()));
    let settings = Settings::load();
    set_hotkey(&settings);
    // Starting with Windows is the installer's and the settings window's business.
    if !paths::isolated() {
        system::register();
    }
    let w = wake.clone();
    // An isolated agent (a test) leaves the keys to the user's own agent unless asked: two hooks
    // would both answer the hotkey.
    let hooked = !paths::isolated() || std::env::var_os("OMNILOOK_HOOK").is_some();
    let _hook = if hooked {
        match hook::install(move |e| w.send(Event::Key(e))) {
            Ok(h) => Some(h),
            Err(e) => {
                log.line(format!("hook: {e}"));
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    tray::set(settings.tray, &system::ui_language(&settings.language));
    // A while after starting (not with the sign-in rush), then every hour: is a check due?
    let w = wake.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(120));
        loop {
            w.send(Event::UpdateTick);
            std::thread::sleep(Duration::from_secs(3600));
        }
    });
    let w = wake.clone();
    let _watch = foreground::watch(move |h| w.send(Event::Foreground(h.0 as isize)))
        .map_err(|e| log.line(format!("foreground watch: {e}")))
        .ok();
    let mut agent = Agent {
        wake,
        log,
        view: None,
        followed: None,
        home: None,
        opened: None,
        caller_nav: None,
        nav_busy: false,
        close_on_blur: settings.close_on_blur,
    };
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.hwnd.is_invalid() && msg.message == WM_APP {
                while let Ok(e) = rx.try_recv() {
                    if !agent.handle(e) {
                        PostQuitMessage(0);
                    }
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // Stopped from the tray menu (or --quit): the preview goes too.
    agent.close_view();
    tray::set(false, "");
    agent.log.line("agent stopped");
    ExitCode::SUCCESS
}
