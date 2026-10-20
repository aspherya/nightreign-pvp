//! CON-2: the console's named pipe, `\\.\pipe\nrpvp-<pid>` (one per game process, so several Nucleus instances never clash).
//!
//! Protocol (text, UTF-8): the client writes one line per request; the server answers with zero or more lines followed by a line
//! holding only [`END`]. `nrcon.exe` is the client. One client at a time; the next one connects after it hangs up.
//!
//! Safety: local clients only (`PIPE_REJECT_REMOTE_CLIENTS`), and the pipe's DACL grants access only to the user running the game
//! and SYSTEM (falls back to the default DACL, with a warning, if the user's SID cannot be read). Everything runs on this thread; a
//! panic here ends the console, never the game.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::FromRawHandle;
use std::panic::{catch_unwind, AssertUnwindSafe};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// End-of-reply marker line (ASCII EOT).
pub(crate) const END: &str = "\u{4}";
const BUF: u32 = 64 * 1024;

pub(crate) fn pipe_name(pid: u32) -> String {
    format!(r"\\.\pipe\nrpvp-{pid}")
}

pub(crate) fn spawn(label: String) {
    let r = std::thread::Builder::new()
        .name("nrpvp-console".into())
        .spawn(move || {
            if catch_unwind(AssertUnwindSafe(|| run(&label))).is_err() {
                tracing::error!("console: pipe thread panicked; the console is off");
            }
        });
    if r.is_err() {
        tracing::warn!("console: could not spawn the pipe thread");
    }
}

fn run(label: &str) {
    let name = pipe_name(std::process::id());
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let sd = user_only_descriptor();
    if sd.is_none() {
        tracing::warn!("console: could not build a user-only pipe DACL; using the default one");
    }
    tracing::info!("console: listening on {name} (nrcon.exe; `help` lists the commands)");
    let sa = sd.map(|p| SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: p.0,
        bInheritHandle: false.into(),
    });
    let create = || {
        // SAFETY: `wide` is NUL-terminated and outlives the call; `sa` (if any) points at a descriptor kept alive for the process.
        let h = unsafe {
            CreateNamedPipeW(
                PCWSTR(wide.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                BUF,
                BUF,
                0,
                sa.as_ref().map(|s| s as *const _),
            )
        };
        if h.is_invalid() {
            tracing::error!(
                "console: CreateNamedPipeW failed ({:?}); the console is off",
                windows::core::Error::from_thread()
            );
            None
        } else {
            Some(h)
        }
    };
    // A spare instance is always waiting while a client is served, so the pipe name never disappears between two clients (a
    // client that connects meanwhile waits for its first reply until the current client hangs up).
    let Some(mut next) = create() else {
        return;
    };
    loop {
        let h = next;
        // SAFETY: valid pipe handle; blocks until a client connects (ERROR_PIPE_CONNECTED = it connected before this call).
        let connected = match unsafe { ConnectNamedPipe(h, None) } {
            Ok(()) => true,
            Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult(),
        };
        let Some(spare) = create() else {
            // SAFETY: we own the handle.
            unsafe {
                let _ = CloseHandle(h);
            }
            return;
        };
        next = spare;
        if !connected {
            // SAFETY: we own the handle.
            unsafe {
                let _ = CloseHandle(h);
            }
            continue;
        }
        // SAFETY: we own `h`; the File closes it when dropped.
        let file = unsafe { File::from_raw_handle(h.0) };
        serve(file, label);
    }
}

/// Answer request lines until the client hangs up.
fn serve(file: File, label: &str) {
    let Ok(mut out) = file.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let req = line.trim_end_matches(['\r', '\n']);
        // `whoami` is the pipe's own: `nrcon list` uses it to tell game instances apart.
        let reply = if req.trim() == "whoami" {
            format!("pid {} {}", std::process::id(), label)
        } else {
            super::execute_line(req)
        };
        let mut text = reply;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(END);
        text.push('\n');
        if out
            .write_all(text.as_bytes())
            .and_then(|_| out.flush())
            .is_err()
        {
            return;
        }
    }
}

/// A security descriptor that grants full access to the current user and SYSTEM only. Leaked on purpose (one per process).
fn user_only_descriptor() -> Option<PSECURITY_DESCRIPTOR> {
    let sid = current_user_sid()?;
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})(A;;GA;;;SY)")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: NUL-terminated SDDL string; the descriptor is allocated by the API and intentionally never freed.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut sd,
            None,
        )
        .ok()?;
    }
    Some(sd)
}

/// `S-1-5-21-…` of the user this process runs as.
fn current_user_sid() -> Option<String> {
    // SAFETY: standard token query: the buffer is sized by the first call, aligned as u64, and the token handle is closed.
    unsafe {
        let mut tok = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok).ok()?;
        let mut len = 0u32;
        let _ = GetTokenInformation(tok, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        let got = GetTokenInformation(tok, TokenUser, Some(buf.as_mut_ptr().cast()), len, &mut len);
        let _ = CloseHandle(tok);
        got.ok()?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut s).ok()?;
        let out = s.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(s.0.cast())));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_the_current_user_sid() {
        assert_eq!(pipe_name(42), r"\\.\pipe\nrpvp-42");
        let sid = current_user_sid().expect("token user");
        assert!(sid.starts_with("S-1-"), "{sid}");
        assert!(user_only_descriptor().is_some());
    }

    /// End to end on this process: the real server thread, a `File` client speaking the same protocol as nrcon.
    #[test]
    fn round_trip_over_the_real_pipe() {
        use std::time::{Duration, Instant};
        spawn("test-label".into());
        let name = pipe_name(std::process::id());
        let t0 = Instant::now();
        let f = loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&name)
            {
                Ok(f) => break f,
                Err(e) if t0.elapsed() < Duration::from_secs(5) => {
                    let _ = e;
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => panic!("could not open {name}: {e}"),
            }
        };
        let mut w = f.try_clone().unwrap();
        let mut r = BufReader::new(f);
        let mut ask = |q: &str| {
            w.write_all(format!("{q}\n").as_bytes()).unwrap();
            let mut lines = Vec::new();
            loop {
                let mut l = String::new();
                assert!(r.read_line(&mut l).unwrap() > 0, "server hung up");
                let l = l.trim_end_matches(['\r', '\n']).to_string();
                if l == END {
                    return lines;
                }
                lines.push(l);
            }
        };
        assert_eq!(
            ask("whoami"),
            vec![format!("pid {} test-label", std::process::id())]
        );
        assert_eq!(ask(r#"echo "over the pipe""#), vec!["over the pipe"]);
        assert!(ask("help").iter().any(|l| l.contains("mm_queue_solo")));
        assert!(ask("").is_empty());
    }
}
