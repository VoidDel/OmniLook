//! What Omni Look registers with Windows (`docs/development-plan.md` §6.1), and small Win32
//! helpers: opening files, stopping processes, the agent's log.

use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_SUCCESS},
        System::{
            Registry::{
                HKEY, HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW,
                RegGetValueW, RegSetKeyValueW,
            },
            Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess},
        },
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    },
    core::{HSTRING, PCWSTR, w},
};

use crate::paths;

/// The calling protocol version OmniZip checks (`docs/development-plan.md` §6.2).
pub const PROTOCOL_VERSION: u32 = 1;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "Omni Look";

fn set_sz(key: HKEY, sub: &str, name: Option<&str>, value: &str) {
    let data: Vec<u16> = value.encode_utf16().chain([0]).collect();
    let name = name.map(HSTRING::from);
    unsafe {
        let _ = RegSetKeyValueW(
            key,
            &HSTRING::from(sub),
            name.as_ref()
                .map(|n| PCWSTR(n.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * 2) as u32,
        );
    }
}

fn get_sz(key: HKEY, sub: &str, name: &str) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut cb = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            key,
            &HSTRING::from(sub),
            &HSTRING::from(name),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut cb),
        )
    };
    (r == ERROR_SUCCESS).then(|| {
        String::from_utf16_lossy(&buf[..cb as usize / 2])
            .trim_end_matches('\0')
            .to_owned()
    })
}

/// The UI language for a setting: the setting itself, or Windows' when it is `auto`.
pub fn ui_language(setting: &str) -> String {
    if setting != "auto" && !setting.is_empty() {
        return setting.to_owned();
    }
    let mut buf = [0u16; 85];
    let n = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buf) };
    String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize])
}

pub fn exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("omnilook.exe"))
}

/// `HKCU\Software\OmniLook` (where OmniZip looks for Omni Look) and `App Paths\omnilook.exe`.
/// Written by the running agent, so a moved or portable copy keeps them right.
pub fn register() {
    let exe = exe();
    let exe_s = exe.to_string_lossy();
    let dir = exe
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    set_sz(
        HKEY_CURRENT_USER,
        r"Software\OmniLook",
        Some("InstallPath"),
        &dir,
    );
    set_sz(HKEY_CURRENT_USER, r"Software\OmniLook", Some("Exe"), &exe_s);
    let v = PROTOCOL_VERSION.to_le_bytes();
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\OmniLook"),
            w!("ProtocolVersion"),
            REG_DWORD.0,
            Some(v.as_ptr().cast()),
            4,
        );
    }
    set_sz(
        HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\CurrentVersion\App Paths\omnilook.exe",
        None,
        &exe_s,
    );
}

/// Starting the agent when signing in.
pub fn set_autostart(on: bool) {
    if on {
        let cmd = format!("\"{}\" --agent", exe().display());
        if get_sz(HKEY_CURRENT_USER, RUN, RUN_NAME).as_deref() != Some(cmd.as_str()) {
            set_sz(HKEY_CURRENT_USER, RUN, Some(RUN_NAME), &cmd);
        }
    } else {
        unsafe {
            let _ = RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(RUN),
                &HSTRING::from(RUN_NAME),
            );
        }
    }
}

/// Opens a file with its default program.
pub fn open_default(path: &Path) -> bool {
    let r = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(path.as_os_str()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success.
    r.0 as usize > 32
}

pub fn terminate(pid: u32) {
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let _ = TerminateProcess(h, 1);
            let _ = CloseHandle(h);
        }
    }
}

/// A plain log in `%LOCALAPPDATA%\OmniLook\<name>`, started afresh when it passes 1 MB.
pub struct Log(Mutex<Option<std::fs::File>>);

impl Log {
    pub fn open(name: &str) -> Self {
        let dir = paths::local_dir();
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join(name);
        let big = std::fs::metadata(&p)
            .map(|m| m.len() > 1 << 20)
            .unwrap_or(false);
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(!big)
            .write(true)
            .truncate(big)
            .open(p)
            .ok();
        Self(Mutex::new(f))
    }

    pub fn line(&self, s: impl AsRef<str>) {
        let secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        if let Ok(mut f) = self.0.lock()
            && let Some(f) = f.as_mut()
        {
            let _ = writeln!(f, "{secs} {}", s.as_ref());
        }
    }
}
