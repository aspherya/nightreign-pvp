//! `nro desk`: a helper that does the desktop work (keyboard input to the game window, screenshots) for other nro processes.
//!
//! Why: processes started by an agent's sandboxed shell (and possibly an MCP server) cannot read the screen or reach other windows
//! (GetDC of the screen fails with "invalid handle"), while a process started from the user's own terminal can. Start `nro desk` once
//! in a normal terminal; every `nro keys` / `nro shot` (CLI or MCP) then goes through it over `\\.\pipe\nro-desk` (local only, one
//! JSON line per request and per reply). Without it they run in-process and say so when the desktop is not reachable.

use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::time::Duration;

use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

use crate::sys;

pub const PIPE: &str = r"\\.\pipe\nro-desk";

pub fn available() -> bool {
    std::fs::read_dir(r"\\.\pipe\")
        .map(|d| d.flatten().any(|e| e.file_name() == "nro-desk"))
        .unwrap_or(false)
}

/// Send one request to a running `nro desk`.
pub fn ask(req: &Value) -> Result<Value, String> {
    let mut f = None;
    for _ in 0..50 {
        match OpenOptions::new().read(true).write(true).open(PIPE) {
            Ok(x) => {
                f = Some(x);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    let f = f.ok_or("nro desk is not answering")?;
    let mut w = f.try_clone().map_err(|e| e.to_string())?;
    writeln!(w, "{req}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(f)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&line).map_err(|e| format!("bad reply {line:?}: {e}"))?;
    match v.get("error").and_then(|e| e.as_str()) {
        Some(e) => Err(e.to_string()),
        None => Ok(v.get("ok").cloned().unwrap_or(Value::Null)),
    }
}

/// Do one desktop request here.
pub fn local(req: &Value) -> Result<Value, String> {
    let pid = req.get("pid").and_then(|v| v.as_u64()).ok_or("no pid")? as u32;
    let h = sys::main_window(pid).ok_or("no game window")?;
    match req.get("op").and_then(|v| v.as_str()) {
        Some("keys") => {
            if !sys::focus(h) {
                return Err("could not bring the game window to the front".into());
            }
            sys::keys_in(
                Some(h),
                req.get("seq").and_then(|v| v.as_str()).unwrap_or(""),
            )
            .map(|_| json!("ok"))
        }
        Some("shot") => {
            let p = req.get("path").and_then(|v| v.as_str()).ok_or("no path")?;
            let (w, hh) = sys::screenshot(h, Path::new(p))?;
            if sys::looks_black(Path::new(p)) {
                return Err(format!(
                    "{p}: the picture is black; this process cannot read the screen (start `nro desk` in a normal terminal)"
                ));
            }
            Ok(json!(format!("{p} ({w}x{hh})")))
        }
        other => Err(format!("unknown desk op {other:?}")),
    }
}

/// A desktop request: through `nro desk` when it runs, else here.
pub fn request(req: Value) -> Result<Value, String> {
    if available() {
        ask(&req)
    } else {
        local(&req)
    }
}

/// The server loop (`nro desk`). Runs until the terminal is closed.
pub fn serve() -> Result<(), String> {
    sys::dpi_aware();
    let name: Vec<u16> = PIPE.encode_utf16().chain(std::iter::once(0)).collect();
    eprintln!("nro desk: serving {PIPE} (keys and screenshots for nro); Ctrl-C to stop");
    loop {
        // SAFETY: creating a pipe instance with a NUL-terminated name; default security (the current user).
        let h = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                std::ptr::null(),
            )
        };
        if h.is_null() || h as isize == -1 {
            return Err("CreateNamedPipeW failed".into());
        }
        // SAFETY: waits for one client on the handle just created (an already-connected client also counts).
        unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) };
        // SAFETY: we own the handle; File takes it over and closes it on drop.
        let f = unsafe { File::from_raw_handle(h as _) };
        let mut w = match f.try_clone() {
            Ok(w) => w,
            Err(_) => continue,
        };
        let mut line = String::new();
        if BufReader::new(&f).read_line(&mut line).is_ok() {
            let reply = match serde_json::from_str::<Value>(&line) {
                Ok(req) => {
                    eprintln!("nro desk: {req}");
                    match local(&req) {
                        Ok(v) => json!({"ok": v}),
                        Err(e) => json!({"error": e}),
                    }
                }
                Err(e) => json!({"error": format!("bad request: {e}")}),
            };
            let _ = writeln!(w, "{reply}");
            let _ = w.flush();
        }
        // SAFETY: the handle is still open (owned by `f`). Flush first: a disconnect throws away a reply the client has not read yet.
        unsafe {
            windows_sys::Win32::Storage::FileSystem::FlushFileBuffers(h);
            DisconnectNamedPipe(h)
        };
    }
}
