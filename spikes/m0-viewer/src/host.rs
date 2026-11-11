//! Hosting a system preview handler (`docs/development-plan.md` §5.2) in a child window of the
//! preview window. Main thread only (single-threaded COM, the window's thread).

use std::{cell::RefCell, path::Path, time::Instant};

use windows::{
    Win32::{
        Foundation::{ERROR_SUCCESS, HWND, RECT},
        System::{
            Com::{CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER, CoCreateInstance, STGM_READ, STGM_SHARE_DENY_NONE},
            Registry::{HKEY_CLASSES_ROOT, RRF_RT_REG_SZ, RegGetValueW},
        },
        UI::{
            Shell::{
                IInitializeWithItem, IPreviewHandler, IShellItem, PropertiesSystem::{IInitializeWithFile, IInitializeWithStream},
                SHCreateItemFromParsingName, SHCreateStreamOnFileEx,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, HWND_TOP, SWP_SHOWWINDOW, SetWindowPos, WINDOW_EX_STYLE,
                WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_VISIBLE,
            },
        },
    },
    core::{GUID, HSTRING, Interface, PCWSTR, w},
};

struct Hosted {
    handler: IPreviewHandler,
    child: HWND,
}

thread_local! {
    static HOSTED: RefCell<Option<Hosted>> = const { RefCell::new(None) };
}

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

/// The preview handler registered for the file's extension, directly or through its ProgID.
fn handler_clsid(path: &Path) -> Option<GUID> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let s = reg_default(&format!(r".{ext}\{SHELLEX_PREVIEW}")).or_else(|| {
        let progid = reg_default(&format!(".{ext}"))?;
        reg_default(&format!(r"{progid}\{SHELLEX_PREVIEW}"))
    })?;
    GUID::try_from(s.trim_matches(|c| c == '{' || c == '}')).ok()
}

pub fn unload() {
    HOSTED.with(|h| {
        if let Some(h) = h.borrow_mut().take() {
            unsafe {
                let _ = h.handler.Unload();
                let _ = DestroyWindow(h.child);
            }
        }
    });
}

pub fn resize(rect: RECT) {
    HOSTED.with(|h| {
        if let Some(h) = h.borrow().as_ref() {
            let (w, ht) = (rect.right - rect.left, rect.bottom - rect.top);
            unsafe {
                let _ = SetWindowPos(h.child, Some(HWND_TOP), rect.left, rect.top, w, ht, SWP_SHOWWINDOW);
                let inner = RECT { left: 0, top: 0, right: w, bottom: ht };
                let _ = h.handler.SetRect(&inner);
            }
        }
    });
}

pub fn show(parent: HWND, path: &Path, rect: RECT, step: &dyn Fn(&str)) -> Result<String, String> {
    unload();
    let t = Instant::now();
    step("create");
    let clsid = handler_clsid(path).ok_or("no preview handler registered")?;
    let (handler, how): (IPreviewHandler, &str) =
        match unsafe { CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER) } {
            Ok(h) => (h, "out of process"),
            Err(e1) => match unsafe { CoCreateInstance(&clsid, None, CLSCTX_INPROC_SERVER) } {
                Ok(h) => (h, "IN PROCESS"),
                Err(e2) => return Err(format!("create {clsid:?}: {e1}; {e2}")),
            },
        };
    let created = t.elapsed();
    step(how);
    step("initialize");
    let file = HSTRING::from(path.as_os_str());
    let init = unsafe {
        if let Ok(i) = handler.cast::<IInitializeWithStream>() {
            SHCreateStreamOnFileEx(&file, (STGM_READ | STGM_SHARE_DENY_NONE).0, 0, false, None)
                .and_then(|s| i.Initialize(&s, STGM_READ.0))
                .map(|_| "stream")
        } else if let Ok(i) = handler.cast::<IInitializeWithFile>() {
            i.Initialize(&file, STGM_READ.0).map(|_| "file")
        } else if let Ok(i) = handler.cast::<IInitializeWithItem>() {
            SHCreateItemFromParsingName::<_, _, IShellItem>(&file, None)
                .and_then(|item| i.Initialize(&item, STGM_READ.0))
                .map(|_| "item")
        } else {
            Err(windows::core::Error::from(windows::Win32::Foundation::E_NOINTERFACE))
        }
    }
    .map_err(|e| format!("initialize: {e}"))?;
    step(init);
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    let child = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            rect.left,
            rect.top,
            w,
            h,
            Some(parent),
            None,
            None,
            None,
        )
    }
    .map_err(|e| format!("child window: {e}"))?;
    unsafe {
        let _ = SetWindowPos(child, Some(HWND_TOP), rect.left, rect.top, w, h, SWP_SHOWWINDOW);
    }
    let inner = RECT { left: 0, top: 0, right: w, bottom: h };
    step("SetWindow");
    let r = unsafe {
        handler.SetWindow(child, &inner).and_then(|_| {
            step("SetRect");
            handler.SetRect(&inner)
        }).and_then(|_| {
            step("DoPreview");
            handler.DoPreview()
        })
    };
    step("done");
    if let Err(e) = r {
        unsafe {
            let _ = handler.Unload();
            let _ = DestroyWindow(child);
        }
        return Err(format!("preview: {e}"));
    }
    HOSTED.with(|hh| *hh.borrow_mut() = Some(Hosted { handler, child }));
    Ok(format!(
        "{how}, initialized with {init}, created in {} ms, shown in {} ms",
        created.as_millis(),
        t.elapsed().as_millis()
    ))
}
