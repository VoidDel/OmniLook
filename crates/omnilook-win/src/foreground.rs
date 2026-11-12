//! Closing the preview when the user moves elsewhere (`docs/development-plan.md` §4.3): the agent
//! watches which window is in front, and the preview stays while that is the preview itself or
//! the window it came from (the Explorer window it follows, the desktop, or the caller's window for
//! a command-line preview, such as OmniZip's).

/// A window that has come to the front, as far as the decision needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// `0` while no window is in front (between two, during a switch).
    pub handle: isize,
    pub pid: u32,
    /// Its top-level owner: a dialog counts as the window that opened it.
    pub root: isize,
    pub root_class: String,
    /// On screen (visible, with a size). Windows that come to the front unseen are not somewhere
    /// the user went: the Excel previewer's server brings its hidden main window forward as it
    /// starts.
    pub shown: bool,
}

/// Where a preview came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Home {
    pub root: isize,
    /// The desktop, which is one of two windows (`Progman`, `WorkerW`) depending on the wallpaper.
    pub desktop: bool,
}

pub fn is_desktop_class(class: &str) -> bool {
    class == "Progman" || class == "WorkerW"
}

/// Whether the preview of process `view_pid`, opened from `home`, stays open once `w` is in front.
pub fn stays(w: &Window, home: Option<Home>, view_pid: u32) -> bool {
    if w.handle == 0 || w.pid == view_pid || !w.shown {
        return true;
    }
    match home {
        Some(h) if h.root == w.root => true,
        Some(h) if h.desktop => is_desktop_class(&w.root_class),
        _ => false,
    }
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use std::cell::RefCell;

    use windows::Win32::{
        Foundation::HWND,
        UI::{
            Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
            WindowsAndMessaging::{
                EVENT_SYSTEM_FOREGROUND, GA_ROOTOWNER, GetAncestor, GetForegroundWindow,
                GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, WINEVENT_OUTOFCONTEXT,
                WINEVENT_SKIPOWNPROCESS,
            },
        },
    };

    use super::{Home, Window, is_desktop_class};
    use crate::explorer::class_name;

    pub fn describe(hwnd: HWND) -> Window {
        if hwnd.is_invalid() {
            return Window {
                handle: 0,
                pid: 0,
                root: 0,
                root_class: String::new(),
                shown: false,
            };
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let root = unsafe { GetAncestor(hwnd, GA_ROOTOWNER) };
        let root = if root.is_invalid() { hwnd } else { root };
        let mut r = Default::default();
        let sized =
            unsafe { GetWindowRect(hwnd, &mut r) }.is_ok() && r.right > r.left && r.bottom > r.top;
        Window {
            handle: hwnd.0 as isize,
            pid,
            root: root.0 as isize,
            root_class: class_name(root),
            shown: sized && unsafe { IsWindowVisible(hwnd) }.as_bool(),
        }
    }

    /// The window in front now.
    pub fn current() -> Window {
        describe(unsafe { GetForegroundWindow() })
    }

    /// The window in front now, as a home (a command-line preview: the caller's window).
    pub fn current_home() -> Option<Home> {
        home_of(unsafe { GetForegroundWindow() })
    }

    pub fn home_of(hwnd: HWND) -> Option<Home> {
        let w = describe(hwnd);
        (w.handle != 0).then(|| Home {
            root: w.root,
            desktop: is_desktop_class(&w.root_class),
        })
    }

    type Sink = Box<dyn Fn(HWND)>;

    thread_local! {
        static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    }

    unsafe extern "system" fn on_event(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        _object: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        SINK.with(|s| {
            if let Some(f) = &*s.borrow() {
                f(hwnd);
            }
        });
    }

    /// Watches foreground changes; dropping it stops. The events arrive on this thread, which
    /// must pump messages; the agent's own windows (the tray menu) are left out.
    pub struct Watch(HWINEVENTHOOK);

    pub fn watch(sink: impl Fn(HWND) + 'static) -> windows::core::Result<Watch> {
        SINK.with(|s| *s.borrow_mut() = Some(Box::new(sink)));
        let h = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if h.is_invalid() {
            SINK.with(|s| *s.borrow_mut() = None);
            return Err(windows::core::Error::from_thread());
        }
        Ok(Watch(h))
    }

    impl Drop for Watch {
        fn drop(&mut self) {
            unsafe {
                let _ = UnhookWinEvent(self.0);
            }
            SINK.with(|s| *s.borrow_mut() = None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(handle: isize, pid: u32, root: isize, class: &str) -> Window {
        Window {
            handle,
            pid,
            root,
            root_class: class.into(),
            shown: true,
        }
    }

    #[test]
    fn what_keeps_the_preview() {
        let explorer = Some(Home {
            root: 100,
            desktop: false,
        });
        // The preview itself, and nothing in front for a moment.
        assert!(stays(&w(7, 42, 7, "Tauri Window"), explorer, 42));
        assert!(stays(&w(0, 0, 0, ""), explorer, 42));
        // The Explorer window it follows, and a dialog that window opened.
        assert!(stays(&w(100, 9, 100, "CabinetWClass"), explorer, 42));
        assert!(stays(&w(555, 9, 100, "CabinetWClass"), explorer, 42));
        // Another Explorer window, another app, the taskbar.
        assert!(!stays(&w(200, 9, 200, "CabinetWClass"), explorer, 42));
        assert!(!stays(&w(300, 77, 300, "Chrome_WidgetWin_1"), explorer, 42));
        assert!(!stays(&w(400, 9, 400, "Shell_TrayWnd"), explorer, 42));
        // The desktop is either of its two windows.
        let desktop = Some(Home {
            root: 10,
            desktop: true,
        });
        assert!(stays(&w(11, 9, 11, "WorkerW"), desktop, 42));
        assert!(stays(&w(10, 9, 10, "Progman"), desktop, 42));
        assert!(!stays(&w(100, 9, 100, "CabinetWClass"), desktop, 42));
        // Nowhere known to come from: only the preview keeps it.
        assert!(!stays(&w(100, 9, 100, "CabinetWClass"), None, 42));
        // A window that comes to the front unseen (Excel's, as its previewer starts).
        let hidden = Window {
            shown: false,
            ..w(500, 33, 500, "XLMAIN")
        };
        assert!(stays(&hidden, explorer, 42));
        assert!(!stays(&w(500, 33, 500, "XLMAIN"), explorer, 42));
    }
}
