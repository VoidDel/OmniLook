//! The system's preview handlers (`docs/development-plan.md` §5.2): the ones Explorer's preview
//! pane uses (Word, Excel, PowerPoint, Outlook items, fonts, and whatever else registers one),
//! shown in a child window of the preview window.
//!
//! Each handler runs out of process in the system's surrogate (`prevhost.exe`), never inside Omni
//! Look. Its window is not a child of the preview window but a borderless popup the preview window
//! owns, laid over the page's area: WebView2 composes its page above the other child windows, and
//! a handler that draws with GDI (the font previewer) stayed hidden under it. An owned popup is
//! always above its owner, hides with it, and (not activating) leaves the keyboard to the page. Creating and starting one can take a second, and a handler may hang: so it all happens
//! on a thread of its own ([`Host`]) that the window's thread only waits for, with a time limit.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicIsize, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};

use windows::{
    Win32::{
        Foundation::{
            COLORREF, E_NOTIMPL, ERROR_SUCCESS, HWND, LPARAM, POINT, RECT, S_FALSE, WPARAM,
        },
        Graphics::Gdi::{
            ClientToScreen, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RedrawWindow,
        },
        System::{
            Com::{
                CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize, STGM_READ, STGM_SHARE_DENY_NONE,
            },
            Ole::IObjectWithSite,
            Registry::{HKEY_CLASSES_ROOT, RRF_RT_REG_SZ, RegGetValueW},
            Threading::GetCurrentThreadId,
        },
        UI::{
            Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL},
            Shell::{
                IInitializeWithItem, IPreviewHandler, IPreviewHandlerFrame,
                IPreviewHandlerFrame_Impl, IPreviewHandlerVisuals, IShellItem,
                PREVIEWHANDLERFRAMEINFO,
                PropertiesSystem::{IInitializeWithFile, IInitializeWithStream},
                SHCreateItemFromParsingName, SHCreateStreamOnFileEx,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
                PostThreadMessageW, SW_HIDE, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW,
                SetWindowPos, ShowWindowAsync, TranslateMessage, WM_APP, WM_KEYDOWN, WM_QUIT,
                WM_SYSKEYDOWN, WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{GUID, HSTRING, Interface, PCWSTR, implement, w},
};

const SHELLEX_PREVIEW: &str = r"ShellEx\{8895b1c6-b41f-4c1c-a562-0d564250836f}";

fn reg_default(sub: &str) -> Option<String> {
    let sub = HSTRING::from(sub);
    let mut buf = [0u16; 512];
    let mut cb = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CLASSES_ROOT,
            &sub,
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut cb),
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..cb as usize / 2]);
    Some(s.trim_end_matches('\0').to_owned())
}

/// The preview handler registered for the file's extension: under the extension, under its
/// ProgID, or for its perceived type (`SystemFileAssociations`).
fn handler_clsid(path: &Path) -> Option<GUID> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let s = reg_default(&format!(r".{ext}\{SHELLEX_PREVIEW}"))
        .or_else(|| {
            let progid = reg_default(&format!(".{ext}"))?;
            reg_default(&format!(r"{progid}\{SHELLEX_PREVIEW}"))
        })
        .or_else(|| reg_default(&format!(r"SystemFileAssociations\.{ext}\{SHELLEX_PREVIEW}")))?;
    GUID::try_from(s.trim().trim_matches(|c| c == '{' || c == '}')).ok()
}

/// Whether the system has a preview handler for this file's type.
pub fn available(path: &Path) -> bool {
    handler_clsid(path).is_some()
}

/// Where the handler draws, in the preview window's client pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Area {
    fn inner(self) -> RECT {
        RECT {
            left: 0,
            top: 0,
            right: self.w,
            bottom: self.h,
        }
    }
}

/// A key the handler passed back (it does not use it): the preview acts on it as on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub vk: u16,
    pub ctrl: bool,
}

type KeySink = Arc<dyn Fn(Key) -> bool + Send + Sync>;

/// The handler's frame: what it calls with keys it does not want (Escape, Ctrl+Space, arrows).
#[implement(IPreviewHandlerFrame)]
struct Frame {
    keys: KeySink,
}

impl IPreviewHandlerFrame_Impl for Frame_Impl {
    fn GetWindowContext(&self) -> windows::core::Result<PREVIEWHANDLERFRAMEINFO> {
        Err(E_NOTIMPL.into())
    }

    fn TranslateAccelerator(&self, msg: *const MSG) -> windows::core::Result<()> {
        // SAFETY: the handler passes a message it received.
        let m = unsafe { msg.as_ref() };
        if let Some(m) = m
            && (m.message == WM_KEYDOWN || m.message == WM_SYSKEYDOWN)
        {
            let key = Key {
                vk: m.wParam.0 as u16,
                ctrl: unsafe { GetKeyState(VK_CONTROL.0.into()) } < 0,
            };
            if (self.keys)(key) {
                return Ok(());
            }
        }
        // Not ours: the handler handles it.
        Err(S_FALSE.into())
    }
}

struct Hosted {
    handler: IPreviewHandler,
    /// The popup the handler draws in.
    child: HWND,
    /// The preview window, whose client area `Area` is in.
    parent: HWND,
}

/// `area` (client pixels of `parent`) on the screen.
fn on_screen(parent: HWND, area: Area) -> (i32, i32) {
    let mut pt = POINT {
        x: area.x,
        y: area.y,
    };
    unsafe {
        let _ = ClientToScreen(parent, &mut pt);
    }
    (pt.x, pt.y)
}

enum Cmd {
    Show {
        parent: isize,
        path: PathBuf,
        area: Area,
        /// Background and text, as `0x00BBGGRR`.
        colors: Option<(u32, u32)>,
        reply: mpsc::Sender<Result<String, String>>,
    },
    Area(Area),
    /// Answered once done, when asked.
    Unload(Option<mpsc::Sender<()>>),
}

thread_local! {
    static HOSTED: RefCell<Option<Hosted>> = const { RefCell::new(None) };
}

fn unload_here() {
    HOSTED.with(|h| {
        if let Some(h) = h.borrow_mut().take() {
            unsafe {
                let _ = h.handler.Unload();
                let _ = DestroyWindow(h.child);
            }
        }
    });
}

fn area_here(area: Area) {
    HOSTED.with(|h| {
        if let Some(h) = h.borrow().as_ref() {
            let (x, y) = on_screen(h.parent, area);
            unsafe {
                let _ = SetWindowPos(
                    h.child,
                    None,
                    x,
                    y,
                    area.w,
                    area.h,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
                let _ = h.handler.SetRect(&area.inner());
            }
        }
    });
}

fn show_here(
    parent: isize,
    path: &Path,
    area: Area,
    colors: Option<(u32, u32)>,
    keys: &KeySink,
    child_out: &AtomicIsize,
) -> Result<String, String> {
    unload_here();
    let clsid = handler_clsid(path).ok_or("no preview handler")?;
    // Out of process only (the system's surrogate): a handler's code never runs in Omni Look.
    let handler: IPreviewHandler = unsafe { CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER) }
        .map_err(|e| format!("create {clsid:?}: {e}"))?;
    let file = HSTRING::from(path.as_os_str());
    // The file's path when the handler takes one (some, like the font previewer, need the file
    // itself), else a shell item, else a stream (Office's handlers take only streams).
    let how = unsafe {
        if let Ok(i) = handler.cast::<IInitializeWithFile>() {
            i.Initialize(&file, STGM_READ.0).map(|_| "file")
        } else if let Ok(i) = handler.cast::<IInitializeWithItem>() {
            SHCreateItemFromParsingName::<_, _, IShellItem>(&file, None)
                .and_then(|item| i.Initialize(&item, STGM_READ.0))
                .map(|_| "item")
        } else if let Ok(i) = handler.cast::<IInitializeWithStream>() {
            SHCreateStreamOnFileEx(&file, (STGM_READ | STGM_SHARE_DENY_NONE).0, 0, false, None)
                .and_then(|s| i.Initialize(&s, STGM_READ.0))
                .map(|_| "stream")
        } else {
            Err(windows::Win32::Foundation::E_NOINTERFACE.into())
        }
    }
    .map_err(|e| format!("initialize: {e}"))?;
    // Hidden until drawn: the page's "loading" shows meanwhile.
    let parent = HWND(parent as *mut _);
    let (x, y) = on_screen(parent, area);
    let child = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            w!("STATIC"),
            PCWSTR::null(),
            WS_POPUP | WS_CLIPCHILDREN,
            x,
            y,
            area.w,
            area.h,
            // The owner, not a parent: see the module's comment.
            Some(parent),
            None,
            None,
            None,
        )
    }
    .map_err(|e| format!("handler window: {e}"))?;
    child_out.store(child.0 as isize, Ordering::Relaxed);
    let done = unsafe {
        if let Ok(site) = handler.cast::<IObjectWithSite>() {
            let frame: IPreviewHandlerFrame = Frame { keys: keys.clone() }.into();
            let _ = site.SetSite(&frame);
        }
        if let (Some((bg, fg)), Ok(v)) = (colors, handler.cast::<IPreviewHandlerVisuals>()) {
            let _ = v.SetBackgroundColor(COLORREF(bg));
            let _ = v.SetTextColor(COLORREF(fg));
        }
        handler
            .SetWindow(child, &area.inner())
            .and_then(|_| handler.SetRect(&area.inner()))
            .and_then(|_| handler.DoPreview())
    };
    if let Err(e) = done {
        unsafe {
            let _ = handler.Unload();
            let _ = DestroyWindow(child);
        }
        child_out.store(0, Ordering::Relaxed);
        return Err(format!("preview: {e}"));
    }
    unsafe {
        let _ = SetWindowPos(
            child,
            None,
            x,
            y,
            area.w,
            area.h,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
        );
        // Some handlers (the font previewer) drew while the window was hidden and wait to be
        // asked again: lay out once more, and mark the whole tree for painting (no waiting on
        // the handler's process).
        let _ = handler.SetRect(&area.inner());
        let _ = RedrawWindow(
            Some(child),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_FRAME,
        );
    }
    HOSTED.with(|h| {
        *h.borrow_mut() = Some(Hosted {
            handler,
            child,
            parent,
        })
    });
    Ok(how.to_owned())
}

/// The thread that runs preview handlers. Dropping it unloads the handler and ends the thread
/// (unless the thread is stuck in a handler, which is then left behind).
pub struct Host {
    tx: Mutex<mpsc::Sender<Cmd>>,
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
    /// The child window of the handler being shown, so a stuck thread's window can be hidden from
    /// another thread.
    child: Arc<AtomicIsize>,
    /// The last area asked for, to place the popup again when the preview window moves.
    last: Mutex<Option<Area>>,
}

impl Host {
    /// Starts the thread. `keys` gets the keys a handler passes back; it answers whether the
    /// preview used them.
    pub fn start(
        keys: impl Fn(Key) -> bool + Send + Sync + 'static,
    ) -> windows::core::Result<Host> {
        let keys: KeySink = Arc::new(keys);
        let child = Arc::new(AtomicIsize::new(0));
        let child2 = child.clone();
        let (tx, rx) = mpsc::channel::<Cmd>();
        let (ready_tx, ready_rx) = mpsc::channel::<u32>();
        let thread = std::thread::Builder::new()
            .name("omnilook-handler".into())
            .spawn(move || unsafe {
                if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                    return;
                }
                let _ = ready_tx.send(GetCurrentThreadId());
                let mut msg = MSG::default();
                // The loop also carries the calls to and from the handler's process.
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.hwnd.is_invalid() && msg.message == WM_APP {
                        while let Ok(cmd) = rx.try_recv() {
                            match cmd {
                                Cmd::Show {
                                    parent,
                                    path,
                                    area,
                                    colors,
                                    reply,
                                } => {
                                    let r = show_here(parent, &path, area, colors, &keys, &child2);
                                    let _ = reply.send(r);
                                }
                                Cmd::Area(a) => area_here(a),
                                Cmd::Unload(done) => {
                                    unload_here();
                                    child2.store(0, Ordering::Relaxed);
                                    if let Some(d) = done {
                                        let _ = d.send(());
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                unload_here();
                CoUninitialize();
            })
            .map_err(|e| {
                windows::core::Error::new(windows::Win32::Foundation::E_FAIL, e.to_string())
            })?;
        let thread_id = ready_rx.recv_timeout(Duration::from_secs(5)).map_err(|_| {
            windows::core::Error::new(windows::Win32::Foundation::E_FAIL, "handler thread")
        })?;
        Ok(Host {
            tx: Mutex::new(tx),
            thread_id,
            thread: Some(thread),
            child,
            last: Mutex::new(None),
        })
    }

    fn send(&self, cmd: Cmd) -> bool {
        let sent = self
            .tx
            .lock()
            .map(|tx| tx.send(cmd).is_ok())
            .unwrap_or(false);
        sent && unsafe { PostThreadMessageW(self.thread_id, WM_APP, WPARAM(0), LPARAM(0)) }.is_ok()
    }

    /// Shows `path` with its handler in a child of `parent` and waits up to `limit`. On time out
    /// the half-made window is hidden and `Err("timeout")` returned: the thread is then stuck in
    /// the handler, and the caller should start another [`Host`] for the next file.
    pub fn show(
        &self,
        parent: isize,
        path: &Path,
        area: Area,
        colors: Option<(u32, u32)>,
        limit: Duration,
    ) -> Result<String, String> {
        if let Ok(mut l) = self.last.lock() {
            *l = Some(area);
        }
        let (reply, rx) = mpsc::channel();
        let cmd = Cmd::Show {
            parent,
            path: path.to_owned(),
            area,
            colors,
            reply,
        };
        if !self.send(cmd) {
            return Err("handler thread gone".into());
        }
        match rx.recv_timeout(limit) {
            Ok(r) => r,
            Err(_) => {
                self.hide();
                Err("timeout".into())
            }
        }
    }

    /// Hides the handler's window without waiting for the thread (which may be stuck).
    pub fn hide(&self) {
        let c = self.child.load(Ordering::Relaxed);
        if c != 0 {
            unsafe {
                let _ = ShowWindowAsync(HWND(c as *mut _), SW_HIDE);
            }
        }
    }

    pub fn set_area(&self, area: Area) {
        if let Ok(mut l) = self.last.lock() {
            *l = Some(area);
        }
        self.send(Cmd::Area(area));
    }

    /// The preview window moved: the popup follows.
    pub fn follow_window(&self) {
        let last = self.last.lock().ok().and_then(|l| *l);
        if let Some(a) = last {
            self.send(Cmd::Area(a));
        }
    }

    pub fn unload(&self) {
        self.hide();
        self.send(Cmd::Unload(None));
    }

    /// Unloads and waits up to `limit` (the window is closing: the handler's process should
    /// let go of the file and its memory now, not when COM notices the window is gone).
    pub fn unload_and_wait(&self, limit: Duration) {
        self.hide();
        let (tx, rx) = mpsc::channel();
        if self.send(Cmd::Unload(Some(tx))) {
            let _ = rx.recv_timeout(limit);
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.hide();
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        // Not joined: a thread stuck in a handler would hold the window's thread too.
        drop(self.thread.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_has_no_handler() {
        assert!(!available(Path::new("a.txt-not-registered-anywhere")));
        assert!(!available(Path::new("no-extension")));
    }
}
