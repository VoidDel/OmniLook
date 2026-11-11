//! Explorer windows and the desktop (`docs/development-plan.md` §4.2): which window and tab has
//! the keyboard, and what is selected in it.
//!
//! [`focus`] is cheap (window classes only, no COM) and runs inside the keyboard hook. Everything
//! else talks to Explorer over COM and must run on a thread that called [`Com::init`].

use std::path::PathBuf;

use windows::{
    Win32::{
        Foundation::HWND,
        System::{
            Com::{
                CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize, IServiceProvider,
            },
            Variant::{VARIANT, VT_I4},
        },
        UI::{
            Shell::{
                _SVGIO, CSIDL_DESKTOP, IFolderView2, IShellBrowser, IShellItem, IShellItemArray,
                IShellWindows, IWebBrowser2, SID_STopLevelBrowser, SIGDN_FILESYSPATH,
                SVGIO_ALLVIEW, SVGIO_FLAG_VIEWORDER, SVGIO_SELECTION, SVSI_DESELECTOTHERS,
                SVSI_ENSUREVISIBLE, SVSI_FOCUSED, SVSI_SELECT, SWC_DESKTOP,
                SWFO_NEEDDISPATCH, ShellWindows,
            },
            WindowsAndMessaging::{
                FindWindowExW, GA_PARENT, GUITHREADINFO, GetAncestor, GetClassNameW,
                GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IsWindowVisible,
            },
        },
    },
    core::{Interface, PCWSTR, Result, w},
};

use crate::trigger::Surface;

/// COM on the current thread (single-threaded apartment, as the Shell expects).
pub struct Com(());

impl Com {
    pub fn init() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
        Ok(Self(()))
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..usize::try_from(n).unwrap_or(0)])
}

/// The window with the keyboard and the control focused in it.
#[derive(Debug, Clone)]
pub struct Focus {
    /// The foreground window handle (as an integer, so a `Focus` can cross threads).
    pub foreground: isize,
    pub surface: Surface,
    pub in_file_view: bool,
    pub foreground_class: String,
    pub focus_class: String,
    pub parent_class: String,
}

impl Focus {
    pub fn window(&self) -> HWND {
        HWND(self.foreground as *mut _)
    }
}

/// Explorer's file list is a `DirectUIHWND` in a `SHELLDLL_DefView`; the desktop's is a
/// `SysListView32` there. The address bar, search box and rename box are other controls.
pub fn focus() -> Focus {
    let foreground = unsafe { GetForegroundWindow() };
    let foreground_class = class_name(foreground);
    let surface = match foreground_class.as_str() {
        "CabinetWClass" => Surface::Explorer,
        "Progman" | "WorkerW" => Surface::Desktop,
        _ => Surface::Other,
    };
    let (mut focus_class, mut parent_class) = (String::new(), String::new());
    if surface != Surface::Other {
        let thread = unsafe { GetWindowThreadProcessId(foreground, None) };
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetGUIThreadInfo(thread, &mut info) }.is_ok() && !info.hwndFocus.is_invalid()
        {
            focus_class = class_name(info.hwndFocus);
            parent_class = class_name(unsafe { GetAncestor(info.hwndFocus, GA_PARENT) });
        }
    }
    let in_file_view = parent_class == "SHELLDLL_DefView"
        && matches!(focus_class.as_str(), "DirectUIHWND" | "SysListView32");
    Focus {
        foreground: foreground.0 as isize,
        surface,
        in_file_view,
        foreground_class,
        focus_class,
        parent_class,
    }
}

fn variant_i4(v: i32) -> VARIANT {
    let mut var = VARIANT::default();
    unsafe {
        (*var.Anonymous.Anonymous).vt = VT_I4;
        (*var.Anonymous.Anonymous).Anonymous.lVal = v;
    }
    var
}

fn path_of(item: &IShellItem) -> Option<PathBuf> {
    unsafe {
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

fn shell_windows() -> Result<IShellWindows> {
    unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }
}

fn browser_of(disp: &windows::Win32::System::Com::IDispatch) -> Result<IShellBrowser> {
    let sp: IServiceProvider = disp.cast()?;
    unsafe { sp.QueryService(&SID_STopLevelBrowser) }
}

/// The tab Explorer shows: the first `ShellTabWindowClass` child of the window (Windows 11
/// keeps the visible tab first). `None` before tabs existed.
pub fn front_tab(top: HWND) -> Option<HWND> {
    unsafe { FindWindowExW(Some(top), None, w!("ShellTabWindowClass"), PCWSTR::null()) }.ok()
}

/// The folder view in one Explorer tab or on the desktop.
pub struct View {
    browser: IShellBrowser,
    folder_view: IFolderView2,
    /// The window [`IShellBrowser`] reports: the tab on Windows 11.
    pub tab: HWND,
}

/// What a view shows and has selected.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub folder: Option<PathBuf>,
    pub selected: Vec<PathBuf>,
    /// Selected items without a file system path (Recycle Bin, phones, zip folders …).
    pub selected_virtual: usize,
    pub focused: Option<PathBuf>,
}

impl View {
    fn new(browser: IShellBrowser) -> Result<Self> {
        unsafe {
            let tab = browser.GetWindow()?;
            let folder_view: IFolderView2 = browser.QueryActiveShellView()?.cast()?;
            Ok(Self {
                browser,
                folder_view,
                tab,
            })
        }
    }

    /// The view in Explorer window `top`, in the tab in front.
    pub fn explorer(top: HWND) -> Result<Option<Self>> {
        let windows = shell_windows()?;
        let front = front_tab(top);
        let mut tabs = Vec::new();
        for i in 0..unsafe { windows.Count()? } {
            let Ok(disp) = (unsafe { windows.Item(&variant_i4(i)) }) else {
                continue;
            };
            let Ok(wb) = disp.cast::<IWebBrowser2>() else {
                continue;
            };
            if unsafe { wb.HWND() }.map(|h| h.0) != Ok(top.0 as isize) {
                continue;
            }
            let Ok(browser) = browser_of(&disp) else {
                continue;
            };
            let Ok(view) = Self::new(browser) else {
                continue;
            };
            tabs.push(view);
        }
        if tabs.len() > 1 {
            if let Some(i) = tabs.iter().position(|v| Some(v.tab) == front) {
                return Ok(Some(tabs.swap_remove(i)));
            }
            if let Some(i) = tabs
                .iter()
                .position(|v| unsafe { IsWindowVisible(v.tab) }.as_bool())
            {
                return Ok(Some(tabs.swap_remove(i)));
            }
        }
        Ok(tabs.into_iter().next())
    }

    pub fn desktop() -> Result<Self> {
        let windows = shell_windows()?;
        let mut hwnd = 0i32;
        let disp = unsafe {
            windows.FindWindowSW(
                &variant_i4(CSIDL_DESKTOP as i32),
                &VARIANT::default(),
                SWC_DESKTOP,
                &mut hwnd,
                SWFO_NEEDDISPATCH,
            )?
        };
        Self::new(browser_of(&disp)?)
    }

    /// The view behind the window that has the keyboard.
    pub fn of(focus: &Focus) -> Result<Option<Self>> {
        match focus.surface {
            Surface::Explorer => Self::explorer(focus.window()),
            Surface::Desktop => Self::desktop().map(Some),
            Surface::Other => Ok(None),
        }
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let fv = &self.folder_view;
        let mut s = Snapshot {
            folder: unsafe { fv.GetFolder::<IShellItem>() }
                .ok()
                .and_then(|i| path_of(&i)),
            ..Default::default()
        };
        if let Ok(items) = unsafe { fv.Items::<IShellItemArray>(SVGIO_SELECTION) } {
            for i in 0..unsafe { items.GetCount()? } {
                match path_of(&unsafe { items.GetItemAt(i)? }) {
                    Some(p) => s.selected.push(p),
                    None => s.selected_virtual += 1,
                }
            }
        }
        s.focused = unsafe { fv.GetFocusedItem() }
            .ok()
            .filter(|&i| i >= 0)
            .and_then(|i| unsafe { fv.GetItem::<IShellItem>(i) }.ok())
            .and_then(|i| path_of(&i));
        Ok(s)
    }

    /// Every item in the order the view shows them (for moving between files).
    pub fn items(&self) -> Result<Vec<PathBuf>> {
        let flags = _SVGIO(SVGIO_ALLVIEW.0 | SVGIO_FLAG_VIEWORDER.0);
        let Ok(items) = (unsafe { self.folder_view.Items::<IShellItemArray>(flags) }) else {
            return Ok(Vec::new());
        };
        let n = unsafe { items.GetCount()? };
        Ok((0..n)
            .filter_map(|i| unsafe { items.GetItemAt(i) }.ok())
            .filter_map(|i| path_of(&i))
            .collect())
    }

    pub fn browser(&self) -> &IShellBrowser {
        &self.browser
    }

    /// Selects `path` alone (focused, scrolled into view), as a click would. Returns whether it
    /// is in this view.
    pub fn select(&self, path: &std::path::Path) -> Result<bool> {
        let Some(i) = self.items()?.iter().position(|p| p == path) else {
            return Ok(false);
        };
        let flags = SVSI_SELECT.0 | SVSI_DESELECTOTHERS.0 | SVSI_FOCUSED.0 | SVSI_ENSUREVISIBLE.0;
        unsafe { self.folder_view.SelectItem(i as i32, flags as u32)? };
        Ok(true)
    }
}

/// One Explorer tab, for diagnostics.
#[derive(Debug, Clone)]
pub struct TabInfo {
    pub top: HWND,
    pub tab: HWND,
    pub tab_class: String,
    pub tab_visible: bool,
    /// The tab is the first `ShellTabWindowClass` child of its window.
    pub front: bool,
    pub snapshot: Snapshot,
}

/// Every Explorer tab, in `IShellWindows` order.
pub fn tabs() -> Result<Vec<TabInfo>> {
    let windows = shell_windows()?;
    let mut out = Vec::new();
    for i in 0..unsafe { windows.Count()? } {
        let Ok(disp) = (unsafe { windows.Item(&variant_i4(i)) }) else {
            continue;
        };
        let Ok(wb) = disp.cast::<IWebBrowser2>() else {
            continue;
        };
        let Ok(top) = (unsafe { wb.HWND() }) else {
            continue;
        };
        let top = HWND(top.0 as *mut _);
        let Ok(view) = browser_of(&disp).and_then(View::new) else {
            continue;
        };
        out.push(TabInfo {
            top,
            tab: view.tab,
            tab_class: class_name(view.tab),
            tab_visible: unsafe { IsWindowVisible(view.tab) }.as_bool(),
            front: front_tab(top) == Some(view.tab),
            snapshot: view.snapshot().unwrap_or_default(),
        });
    }
    Ok(out)
}
