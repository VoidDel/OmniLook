//! Named pipes between Omni Look's processes (`docs/development-plan.md` §3.1, §6). Every
//! exchange is one connection: the client writes one line, the server answers one line and
//! hangs up. That keeps both sides on plain blocking I/O (a synchronous pipe handle cannot read
//! and write at the same time from two threads).
//!
//! Only the current user can open a pipe (its DACL names the user's SID); the first instance is
//! created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a second server cannot take the name over;
//! and a client checks that the server runs as the same user before trusting it.

use std::{
    sync::Arc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE,
            HANDLE, HLOCAL, LocalFree,
        },
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SDDL_REVISION_1,
            },
            GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
            TOKEN_USER, TokenUser,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE,
            FlushFileBuffers, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
        },
        System::{
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
                GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, WaitNamedPipeW,
            },
            Threading::{
                GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    },
    core::{Error, HSTRING, PWSTR, Result},
};

/// Longest line accepted: requests carry a path and a title.
const MAX_LINE: usize = 64 << 10;

struct Owned(HANDLE);

// A kernel handle may be used from any thread; each pipe instance is used by one at a time.
unsafe impl Send for Owned {}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn fail(msg: &str) -> Error {
    Error::new(windows::Win32::Foundation::E_FAIL, msg)
}

/// The SID of the user a process runs as (`None`: this process).
fn sid_of(process: Option<HANDLE>) -> Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(
            process.unwrap_or_else(|| GetCurrentProcess()),
            TOKEN_QUERY,
            &mut token,
        )?;
        let token = Owned(token);
        let mut len = 0u32;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            len,
            &mut len,
        )?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut s)?;
        let out = s.to_string().map_err(|_| fail("SID"));
        LocalFree(Some(HLOCAL(s.0.cast())));
        out
    }
}

/// `\\.\pipe\omnilook-<tag>-<user SID>`.
pub fn pipe_name(tag: &str) -> Result<String> {
    Ok(format!(r"\\.\pipe\omnilook-{tag}-{}", sid_of(None)?))
}

fn create_instance(name: &HSTRING, sid: &str, first: bool) -> Result<Owned> {
    unsafe {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(format!("D:P(A;;GA;;;{sid})")),
            SDDL_REVISION_1,
            &mut sd,
            None,
        )?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };
        let mut mode = PIPE_ACCESS_DUPLEX;
        if first {
            mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let h = CreateNamedPipeW(
            name,
            mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            MAX_LINE as u32,
            MAX_LINE as u32,
            0,
            Some(&sa),
        );
        LocalFree(Some(HLOCAL(sd.0)));
        if h.is_invalid() {
            return Err(Error::from_thread());
        }
        Ok(Owned(h))
    }
}

/// Reads up to the first newline (without it).
fn read_line(h: HANDLE) -> Result<String> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0u32;
        unsafe { ReadFile(h, Some(&mut buf), Some(&mut n), None)? };
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
        if let Some(i) = out.iter().position(|&b| b == b'\n') {
            out.truncate(i);
            break;
        }
        if out.len() > MAX_LINE {
            return Err(fail("line too long"));
        }
    }
    String::from_utf8(out).map_err(|_| fail("not UTF-8"))
}

fn write_line(h: HANDLE, line: &str) -> Result<()> {
    let mut data = line.as_bytes().to_vec();
    data.push(b'\n');
    let mut written = 0u32;
    unsafe {
        WriteFile(h, Some(&data), Some(&mut written), None)?;
        FlushFileBuffers(h)?;
    }
    Ok(())
}

pub type Handler = Arc<dyn Fn(String) -> String + Send + Sync>;

/// Serves `name` on background threads: each connection's line goes to `handler`, its answer
/// goes back. Fails when a server for `name` already exists (another instance is running).
pub fn serve(name: &str, handler: Handler) -> Result<JoinHandle<()>> {
    let sid = sid_of(None)?;
    let wide = HSTRING::from(name);
    let first = create_instance(&wide, &sid, true)?;
    let t = std::thread::Builder::new()
        .name("pipe".into())
        .spawn(move || {
            let mut next = Some(first);
            loop {
                let pipe = match next.take() {
                    Some(p) => p,
                    None => match create_instance(&wide, &sid, false) {
                        Ok(p) => p,
                        Err(_) => {
                            std::thread::sleep(Duration::from_millis(100));
                            continue;
                        }
                    },
                };
                let connected = unsafe { ConnectNamedPipe(pipe.0, None) };
                if let Err(e) = connected
                    && e.code() != ERROR_PIPE_CONNECTED.to_hresult()
                {
                    continue;
                }
                let handler = handler.clone();
                let _ = std::thread::Builder::new()
                    .name("pipe-client".into())
                    .spawn(move || {
                        // The whole `Owned` (closures capture fields otherwise).
                        let pipe = pipe;
                        if let Ok(line) = read_line(pipe.0) {
                            let answer = handler(line);
                            let _ = write_line(pipe.0, &answer);
                        }
                        unsafe {
                            let _ = DisconnectNamedPipe(pipe.0);
                        }
                    });
            }
        })
        .map_err(|_| fail("spawn"))?;
    Ok(t)
}

/// Sends one line to the server of `name` and returns its answer. Refuses a server that does
/// not run as the current user.
pub fn request(name: &str, line: &str, wait: Duration) -> Result<String> {
    let wide = HSTRING::from(name);
    let start = Instant::now();
    let h = loop {
        match unsafe {
            CreateFileW(
                &wide,
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        } {
            Ok(h) => break Owned(h),
            Err(e) if e.code() == ERROR_PIPE_BUSY.to_hresult() && start.elapsed() < wait => {
                let left = wait.saturating_sub(start.elapsed()).as_millis() as u32;
                unsafe {
                    let _ = WaitNamedPipeW(&wide, left.max(1));
                }
            }
            Err(e) => return Err(e),
        }
    };
    unsafe {
        let mut pid = 0u32;
        GetNamedPipeServerProcessId(h.0, &mut pid)?;
        let server = Owned(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?);
        if sid_of(Some(server.0))? != sid_of(None)? {
            return Err(fail("pipe server runs as another user"));
        }
    }
    write_line(h.0, line)?;
    read_line(h.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_single_server() {
        let name = pipe_name(&format!("test-{}", std::process::id())).unwrap();
        serve(&name, Arc::new(|l: String| format!("echo {l}"))).unwrap();
        assert!(
            serve(&name, Arc::new(|l| l)).is_err(),
            "second server refused"
        );
        for i in 0..5 {
            let a = request(&name, &format!("hello {i}"), Duration::from_secs(2)).unwrap();
            assert_eq!(a, format!("echo hello {i}"));
        }
        // Concurrent clients.
        let hs: Vec<_> = (0..8)
            .map(|i| {
                let name = name.clone();
                std::thread::spawn(move || {
                    request(&name, &i.to_string(), Duration::from_secs(5)).unwrap()
                })
            })
            .collect();
        for (i, h) in hs.into_iter().enumerate() {
            assert_eq!(h.join().unwrap(), format!("echo {i}"));
        }
        assert!(request(r"\\.\pipe\omnilook-nobody", "x", Duration::ZERO).is_err());
    }
}
