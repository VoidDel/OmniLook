//! The notification-area icon (`docs/development-plan.md` §3.1): off by default; the settings turn
//! it on. Drawn with Win32 because the agent does not load Tauri. Its menu opens the settings or
//! stops Omni Look.

use std::cell::Cell;

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi},
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
                Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
                DestroyMenu, GetCursorPos, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW,
                MF_SEPARATOR, MF_STRING, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
                SM_CXSMICON, SM_CYSMICON, SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON,
                TrackPopupMenu, WINDOW_EX_STYLE, WM_APP, WM_CONTEXTMENU, WM_LBUTTONUP,
                WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
            },
        },
    },
    core::{HSTRING, PCWSTR, w},
};

use crate::system;

const CALLBACK: u32 = WM_APP + 1;
const ID: u32 = 1;
const CMD_SETTINGS: usize = 1;
const CMD_QUIT: usize = 2;
/// The icon Tauri embeds in the executable (`tauri_utils::platform::WINDOWS_APP_ICON_RESOURCE_ID`).
const ICON_RESOURCE: u16 = 32512;

thread_local! {
    static WINDOW: Cell<Option<HWND>> = const { Cell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    static LABELS: Cell<(&'static str, &'static str)> = const { Cell::new(("Settings", "Quit")) };
}

/// Menu texts in the UI language (the agent has no translation files).
fn labels(lang: &str) -> (&'static str, &'static str) {
    let l = lang.to_ascii_lowercase();
    if l.starts_with("zh") {
        if l.contains("tw") || l.contains("hk") || l.contains("hant") {
            ("設定", "結束 Omni Look")
        } else {
            ("设置", "退出 Omni Look")
        }
    } else if l.starts_with("ja") {
        ("設定", "Omni Look を終了")
    } else {
        ("Settings", "Quit Omni Look")
    }
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut d = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: CALLBACK,
        ..Default::default()
    };
    // The small-icon size at the system's scale, drawn from the frame of that size: LoadIconW
    // gives the 32 px large icon, which the notification area shrinks (soft, and smaller-looking
    // than its neighbours).
    unsafe {
        if let Ok(m) = GetModuleHandleW(None) {
            let dpi = GetDpiForSystem();
            let (w, h) = (
                GetSystemMetricsForDpi(SM_CXSMICON, dpi),
                GetSystemMetricsForDpi(SM_CYSMICON, dpi),
            );
            if let Ok(icon) = LoadImageW(
                Some(m.into()),
                PCWSTR(ICON_RESOURCE as usize as *const u16),
                IMAGE_ICON,
                w,
                h,
                LR_DEFAULTCOLOR,
            ) {
                d.hIcon = HICON(icon.0);
            }
        }
    }
    for (i, c) in "Omni Look".encode_utf16().enumerate() {
        d.szTip[i] = c;
    }
    d
}

fn add(hwnd: HWND) {
    let d = data(hwnd);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &d);
        // The notification area keeps its own copy.
        if !d.hIcon.is_invalid() {
            let _ = DestroyIcon(d.hIcon);
        }
    }
}

fn menu(hwnd: HWND) {
    let (settings, quit) = LABELS.with(Cell::get);
    unsafe {
        let Ok(m) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(m, MF_STRING, CMD_SETTINGS, &HSTRING::from(settings));
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(m, MF_STRING, CMD_QUIT, &HSTRING::from(quit));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // The menu closes when clicking elsewhere only if its window is in front.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            m,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(m);
        match cmd.0 as usize {
            CMD_SETTINGS => {
                let _ = std::process::Command::new(system::exe()).spawn();
            }
            CMD_QUIT => PostQuitMessage(0),
            _ => {}
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == CALLBACK {
        match (lp.0 & 0xFFFF) as u32 {
            WM_LBUTTONUP => {
                let _ = std::process::Command::new(system::exe()).spawn();
            }
            WM_RBUTTONUP | WM_CONTEXTMENU => menu(hwnd),
            _ => {}
        }
        return LRESULT(0);
    }
    // Explorer restarted: the notification area is new and empty.
    if msg != 0 && msg == TASKBAR_CREATED.with(Cell::get) {
        add(hwnd);
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// Shows or hides the icon (on the agent's thread, which pumps messages).
pub fn set(visible: bool, lang: &str) {
    LABELS.with(|l| l.set(labels(lang)));
    let current = WINDOW.with(Cell::get);
    match (visible, current) {
        (true, None) => unsafe {
            let Ok(m) = GetModuleHandleW(None) else {
                return;
            };
            let class = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: m.into(),
                lpszClassName: w!("OmniLookTray"),
                ..Default::default()
            };
            RegisterClassW(&class);
            TASKBAR_CREATED.with(|t| t.set(RegisterWindowMessageW(w!("TaskbarCreated"))));
            let Ok(hwnd) = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("OmniLookTray"),
                w!("Omni Look"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                // A top-level window (never shown): message-only windows miss the broadcast
                // that Explorer has restarted.
                None,
                None,
                Some(m.into()),
                None,
            ) else {
                return;
            };
            add(hwnd);
            WINDOW.with(|w| w.set(Some(hwnd)));
        },
        (false, Some(hwnd)) => unsafe {
            let d = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: ID,
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_DELETE, &d);
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
            WINDOW.with(|w| w.set(None));
        },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::labels;

    #[test]
    fn menu_languages() {
        assert_eq!(labels("zh-CN").0, "设置");
        assert_eq!(labels("zh-TW").0, "設定");
        assert_eq!(labels("ja-JP").1, "Omni Look を終了");
        assert_eq!(labels("de-DE").0, "Settings");
    }
}
