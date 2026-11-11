//! The low-level keyboard hook (`docs/development-plan.md` §4.1). The hook procedure only looks
//! at window classes and asks [`decide`]; whatever needs COM happens on the receiving side of
//! the sink, which must return at once (send into a channel). Windows removes hooks that are
//! slow to answer.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Instant,
};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::{
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME,
                VK_LEFT, VK_LWIN, VK_MENU, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_RWIN, VK_SHIFT,
                VK_SPACE, VK_UP,
            },
            WindowsAndMessaging::{
                CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG, PostThreadMessageW,
                SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
                WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
            },
        },
    },
    core::Result,
};

use crate::{
    explorer::{self, Focus},
    trigger::{Decision, Hotkey, Key, KeyContext, Modifiers, decide},
};

/// A key the hook looked at.
#[derive(Debug, Clone)]
pub struct HookEvent {
    pub at: Instant,
    pub key: Key,
    pub decision: Decision,
    pub focus: Focus,
    pub mods: Modifiers,
}

type Sink = Box<dyn Fn(HookEvent) + Send + Sync>;

struct State {
    sink: Sink,
    last_typing: Mutex<Option<Instant>>,
    /// A Space press was swallowed: swallow its repeats and its release too.
    swallowing: AtomicBool,
}

static STATE: Mutex<Option<Arc<State>>> = Mutex::new(None);
static HOTKEY: AtomicU8 = AtomicU8::new(0);
static PREVIEW_OPEN: AtomicBool = AtomicBool::new(false);

pub fn set_hotkey(h: Hotkey) {
    HOTKEY.store(
        match h {
            Hotkey::CtrlSpace => 0,
            Hotkey::Space => 1,
        },
        Ordering::Relaxed,
    );
}

fn hotkey() -> Hotkey {
    match HOTKEY.load(Ordering::Relaxed) {
        1 => Hotkey::Space,
        _ => Hotkey::CtrlSpace,
    }
}

/// Tells the hook whether a preview is showing (Escape and arrow keys only matter then).
pub fn set_preview_open(open: bool) {
    PREVIEW_OPEN.store(open, Ordering::Relaxed);
}

fn classify(vk: u32) -> Key {
    let vk = VIRTUAL_KEY(vk as u16);
    match vk {
        VK_SPACE => Key::Space,
        VK_ESCAPE => Key::Escape,
        VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN | VK_HOME | VK_END | VK_PRIOR | VK_NEXT => Key::Move,
        // 0-9 and A-Z.
        VIRTUAL_KEY(0x30..=0x39 | 0x41..=0x5A) => Key::Typing,
        _ => Key::Other,
    }
}

fn pressed(vk: VIRTUAL_KEY) -> bool {
    (unsafe { GetAsyncKeyState(i32::from(vk.0)) }) < 0
}

/// Returns whether to swallow the key.
fn handle(st: &State, vk: u32, down: bool) -> bool {
    let key = classify(vk);
    if key == Key::Space && !down {
        return st.swallowing.swap(false, Ordering::Relaxed);
    }
    if !down || key == Key::Other {
        return false;
    }
    let now = Instant::now();
    if key == Key::Typing {
        *st.last_typing.lock().unwrap_or_else(|e| e.into_inner()) = Some(now);
        return false;
    }
    if key == Key::Space && st.swallowing.load(Ordering::Relaxed) {
        return true; // auto-repeat of a swallowed press
    }
    let focus = explorer::focus();
    let mods = Modifiers {
        ctrl: pressed(VK_CONTROL),
        alt: pressed(VK_MENU),
        shift: pressed(VK_SHIFT),
        win: pressed(VK_LWIN) || pressed(VK_RWIN),
    };
    let since_typing_ms = st
        .last_typing
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .map(|t| now.duration_since(t).as_millis() as u64);
    let cx = KeyContext {
        surface: focus.surface,
        in_file_view: focus.in_file_view,
        mods,
        since_typing_ms,
        preview_open: PREVIEW_OPEN.load(Ordering::Relaxed),
    };
    let decision = decide(hotkey(), key, &cx);
    (st.sink)(HookEvent {
        at: now,
        key,
        decision,
        focus,
        mods,
    });
    match decision {
        Decision::Toggle => {
            st.swallowing.store(true, Ordering::Relaxed);
            true
        }
        Decision::Close => true,
        Decision::Pass | Decision::Follow => false,
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let msg = wparam.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        let state = STATE.lock().ok().and_then(|s| s.clone());
        if (down || up)
            && let Some(st) = state
            && handle(&st, kb.vkCode, down)
        {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The installed hook; dropping it removes the hook.
pub struct Hook {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

/// Installs the hook on its own thread (a low-level hook needs a message loop on the thread
/// that installed it). One hook per process.
pub fn install(sink: impl Fn(HookEvent) + Send + Sync + 'static) -> Result<Hook> {
    {
        let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if s.is_some() {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_FAIL,
                "hook already installed",
            ));
        }
        *s = Some(Arc::new(State {
            sink: Box::new(sink),
            last_typing: Mutex::new(None),
            swallowing: AtomicBool::new(false),
        }));
    }
    let (tx, rx) = mpsc::channel::<Result<u32>>();
    let thread = std::thread::Builder::new()
        .name("omnilook-hook".into())
        .spawn(move || unsafe {
            let hook = GetModuleHandleW(None).and_then(|m| {
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(HINSTANCE(m.0)), 0)
            });
            let hook = match hook {
                Ok(h) => {
                    let _ = tx.send(Ok(GetCurrentThreadId()));
                    h
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
            let _ = UnhookWindowsHookEx(hook);
        })
        .expect("spawn hook thread");
    match rx.recv() {
        Ok(Ok(thread_id)) => Ok(Hook {
            thread_id,
            thread: Some(thread),
        }),
        Ok(Err(e)) => {
            *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Err(e)
        }
        Err(_) => {
            *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Err(windows::core::Error::new(
                windows::Win32::Foundation::E_FAIL,
                "hook thread ended",
            ))
        }
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}
