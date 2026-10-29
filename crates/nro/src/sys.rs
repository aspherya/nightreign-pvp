//! Win32 adapters: processes, hashes, the game window, keyboard input, screenshots. Small wrappers; policy lives in `nro-core`.

// HWND is an opaque window handle that only Win32 interprets (a stale one makes the call fail); nothing here dereferences it.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetClientRect, GetForegroundWindow, GetWindowThreadProcessId,
    IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

pub use nro_core::preflight::Process;

/// Upper-case hex SHA-256 of a file, `None` if it cannot be read.
pub fn sha256(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02X}")).collect())
}

fn wide_to_string(w: &[u16]) -> String {
    let n = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..n])
}

pub fn image_path(pid: u32) -> Option<String> {
    // SAFETY: plain Win32 calls with owned buffers; the handle is closed before returning.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Full paths of the modules loaded in `pid` (empty when the process cannot be opened).
pub fn modules(pid: u32) -> Vec<String> {
    use windows_sys::Win32::System::ProcessStatus::{
        EnumProcessModulesEx, GetModuleFileNameExW, LIST_MODULES_ALL,
    };
    use windows_sys::Win32::System::Threading::PROCESS_VM_READ;
    // SAFETY: plain Win32 calls with owned buffers; the handle is closed before returning.
    unsafe {
        let h = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | 0x0400 | PROCESS_VM_READ,
            0,
            pid,
        );
        if h.is_null() {
            return Vec::new();
        }
        let mut mods = vec![std::ptr::null_mut(); 2048];
        let mut need = 0u32;
        let mut out = Vec::new();
        if EnumProcessModulesEx(
            h,
            mods.as_mut_ptr(),
            (mods.len() * std::mem::size_of::<*mut core::ffi::c_void>()) as u32,
            &mut need,
            LIST_MODULES_ALL,
        ) != 0
        {
            let n = (need as usize / std::mem::size_of::<*mut core::ffi::c_void>()).min(mods.len());
            for m in &mods[..n] {
                let mut buf = [0u16; 1024];
                let len = GetModuleFileNameExW(h, *m, buf.as_mut_ptr(), buf.len() as u32);
                if len > 0 {
                    out.push(String::from_utf16_lossy(&buf[..len as usize]));
                }
            }
        }
        CloseHandle(h);
        out
    }
}

/// Diagnostic: the stack words of thread `tid` that point into a module (a poor man's backtrace), first 14.
fn stack_scan(pid: u32, tid: u32) -> Vec<String> {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        GetThreadContext, ReadProcessMemory, CONTEXT,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, THREAD_GET_CONTEXT};
    let mut out = Vec::new();
    // SAFETY: handles opened and closed here; CONTEXT is aligned by its own definition; reads go through ReadProcessMemory.
    unsafe {
        let ht = OpenThread(THREAD_GET_CONTEXT, 0, tid);
        let hp = OpenProcess(0x0010 | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if ht.is_null() || hp.is_null() {
            return vec!["cannot open thread or process".into()];
        }
        let mut ctx: CONTEXT = std::mem::zeroed();
        ctx.ContextFlags = 0x0010_0001; // CONTEXT_CONTROL
        if GetThreadContext(ht, &mut ctx) != 0 {
            let mut buf = [0u8; 0x800];
            let mut got = 0usize;
            if ReadProcessMemory(
                hp,
                ctx.Rsp as *const _,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut got,
            ) != 0
            {
                for w in buf[..got].as_chunks::<8>().0 {
                    let v = usize::from_le_bytes(*w);
                    if v > 0x7FF0_0000_0000 || (0x1_4000_0000..0x1_8000_0000).contains(&v) {
                        let m = module_of(pid, v);
                        if !m.starts_with("no module") && !out.contains(&m) {
                            out.push(m);
                            if out.len() >= 14 {
                                break;
                            }
                        }
                    }
                }
            }
        }
        CloseHandle(ht);
        CloseHandle(hp);
    }
    out
}

/// `module+offset` for an address in `pid` (diagnostic).
pub fn module_of(pid: u32, addr: usize) -> String {
    use windows_sys::Win32::System::ProcessStatus::{
        EnumProcessModulesEx, GetModuleFileNameExW, GetModuleInformation, LIST_MODULES_ALL,
        MODULEINFO,
    };
    // SAFETY: plain Win32 calls with owned buffers; the handle is closed before returning.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | 0x0400 | 0x0010, 0, pid);
        if h.is_null() {
            return "?".into();
        }
        let mut mods = vec![std::ptr::null_mut(); 2048];
        let mut need = 0u32;
        let mut out = format!("no module holds 0x{addr:X}");
        if EnumProcessModulesEx(
            h,
            mods.as_mut_ptr(),
            (mods.len() * 8) as u32,
            &mut need,
            LIST_MODULES_ALL,
        ) != 0
        {
            for m in &mods[..(need as usize / 8).min(mods.len())] {
                let mut mi: MODULEINFO = std::mem::zeroed();
                if GetModuleInformation(h, *m, &mut mi, std::mem::size_of::<MODULEINFO>() as u32)
                    != 0
                {
                    let base = mi.lpBaseOfDll as usize;
                    if addr >= base && addr < base + mi.SizeOfImage as usize {
                        let mut buf = [0u16; 1024];
                        let n = GetModuleFileNameExW(h, *m, buf.as_mut_ptr(), 1024);
                        out = format!(
                            "{}+0x{:X}",
                            String::from_utf16_lossy(&buf[..n as usize]),
                            addr - base
                        );
                        break;
                    }
                }
            }
        }
        CloseHandle(h);
        out
    }
}

pub fn processes() -> Vec<Process> {
    let mut out = Vec::new();
    // SAFETY: toolhelp snapshot walk; dwSize is set as the API requires; the snapshot handle is closed.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap as isize == -1 {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let name = wide_to_string(&e.szExeFile);
            out.push(Process {
                pid: e.th32ProcessID,
                path: image_path(e.th32ProcessID),
                name,
            });
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    out
}

pub fn kill(pid: u32) -> bool {
    // SAFETY: terminate a process we started (callers check ownership); handle closed.
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if h.is_null() {
            return false;
        }
        let ok = TerminateProcess(h, 1) != 0;
        CloseHandle(h);
        ok
    }
}

/// A handle (as usize, 0 = none) that outlives the process, to read its exit code afterwards.
pub fn open_for_exit(pid: u32) -> usize {
    // SAFETY: plain OpenProcess; the caller passes the value to `exit_code`, which closes it.
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | 0x0010_0000, 0, pid) as usize }
}

/// The exit code of the process behind `h` (from `open_for_exit`), `None` while it still runs; closes the handle when it returns Some.
pub fn exit_code(h: usize) -> Option<u32> {
    use windows_sys::Win32::System::Threading::GetExitCodeProcess;
    if h == 0 {
        return None;
    }
    let mut code = 0u32;
    // SAFETY: `h` came from OpenProcess above.
    let ok = unsafe { GetExitCodeProcess(h as _, &mut code) };
    if ok != 0 && code != 259 {
        // SAFETY: closing our own handle.
        unsafe { CloseHandle(h as _) };
        return Some(code);
    }
    None
}

/// Diagnostic (NRO_DEBUG_ATTACH=1): attach as a debugger to `pid`, return a text about the first access violation (address, module+offset,
/// access type) or why none was seen. Blocks until the process exits or `max_ms` pass.
pub fn first_crash(pid: u32, max_ms: u32) -> String {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        ContinueDebugEvent, DebugActiveProcess, DebugSetProcessKillOnExit, WaitForDebugEvent,
        DEBUG_EVENT,
    };
    // SAFETY: standard debug loop on a process we just attached to; the event struct is zero-initialised and owned here.
    unsafe {
        if DebugActiveProcess(pid) == 0 {
            return format!(
                "DebugActiveProcess failed, error {}",
                windows_sys::Win32::Foundation::GetLastError()
            );
        }
        DebugSetProcessKillOnExit(0);
        let t0 = std::time::Instant::now();
        let mut seen = Vec::new();
        let mut avs: Vec<String> = Vec::new();
        loop {
            let mut ev: DEBUG_EVENT = std::mem::zeroed();
            if WaitForDebugEvent(&mut ev, 200) == 0 {
                if t0.elapsed().as_millis() as u32 > max_ms {
                    return format!("no unhandled AV in {max_ms} ms; first-chance AVs: {avs:?}; events: {seen:?}");
                }
                continue;
            }
            let mut cont = 0x8001_0001u32; // DBG_EXCEPTION_NOT_HANDLED
            match ev.dwDebugEventCode {
                1 => {
                    let e = ev.u.Exception;
                    let code = e.ExceptionRecord.ExceptionCode as u32;
                    if code == 0xC000_0005 {
                        let addr = e.ExceptionRecord.ExceptionAddress as usize;
                        let info = e.ExceptionRecord.ExceptionInformation;
                        let what = if e.ExceptionRecord.NumberParameters >= 2 {
                            format!(
                                "{} of 0x{:X}",
                                ["read", "write", "?", "?", "?", "?", "?", "?", "execute"]
                                    [(info[0] as usize).min(8)],
                                info[1]
                            )
                        } else {
                            String::new()
                        };
                        let line = format!(
                            "AV at 0x{addr:X} = {} ({what}), first chance {}",
                            module_of(pid, addr),
                            e.dwFirstChance
                        );
                        if e.dwFirstChance == 0 {
                            let stack = stack_scan(pid, ev.dwThreadId);
                            return format!(
                                "UNHANDLED {line}; stack: {stack:?}; earlier AVs: {avs:?}"
                            );
                        }
                        if avs.len() < 12 {
                            avs.push(line);
                        }
                    }
                    seen.push(format!("0x{code:X}"));
                    if code == 0x8000_0003 || code == 0x4000_001F || code == 0x4000_001E {
                        cont = 0x0001_0002; // DBG_CONTINUE for breakpoints
                    }
                }
                5 => {
                    return format!(
                        "process exited, code 0x{:X}; events before: {seen:?}",
                        ev.u.ExitProcess.dwExitCode
                    )
                }
                _ => cont = 0x0001_0002,
            }
            ContinueDebugEvent(ev.dwProcessId, ev.dwThreadId, cont as i32);
        }
    }
}

pub fn alive(pid: u32) -> bool {
    image_path(pid).is_some()
}

struct Find {
    pid: u32,
    best: HWND,
    area: i64,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
    let f = &mut *(lp as *mut Find);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == f.pid && IsWindowVisible(hwnd) != 0 {
        let mut r: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut r);
        let a = (r.right - r.left) as i64 * (r.bottom - r.top) as i64;
        if a > f.area {
            f.area = a;
            f.best = hwnd;
        }
    }
    1
}

/// The biggest visible top-level window of a process.
pub fn main_window(pid: u32) -> Option<HWND> {
    let mut f = Find {
        pid,
        best: std::ptr::null_mut(),
        area: 0,
    };
    // SAFETY: the callback only reads window data and writes into `f`, which outlives the call.
    unsafe { EnumWindows(Some(enum_cb), &mut f as *mut Find as LPARAM) };
    (!f.best.is_null()).then_some(f.best)
}

/// The window in front: `true` when it is `hwnd`.
pub fn is_foreground(hwnd: HWND) -> bool {
    // SAFETY: no arguments; returns a handle value only.
    unsafe { GetForegroundWindow() == hwnd }
}

/// Visible windows of other processes that lie above `hwnd` in the z-order and overlap its rectangle (the user's PC: a window over
/// the game would get our key presses and spoil screenshots). Ignored: cloaked windows (other virtual desktops, hidden UWP frames),
/// click-through layered overlays (`WS_EX_TRANSPARENT`), and windows smaller than 8 px. Returns "pid exe: title" per window.
pub fn occluders(hwnd: HWND) -> Vec<String> {
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindow, GetWindowLongW, GetWindowRect, GetWindowTextW, GWL_EXSTYLE, GW_HWNDPREV,
        WS_EX_TRANSPARENT,
    };
    let mut out = Vec::new();
    // SAFETY: read-only window queries on handles from the z-order walk; buffers are sized here.
    unsafe {
        let mut me: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut me) == 0 {
            return out;
        }
        let mut own = 0u32;
        GetWindowThreadProcessId(hwnd, &mut own);
        let mut w = GetWindow(hwnd, GW_HWNDPREV);
        let mut n = 0;
        while !w.is_null() && n < 2000 {
            n += 1;
            let mut pid = 0u32;
            GetWindowThreadProcessId(w, &mut pid);
            let mut cloaked = 0u32;
            DwmGetWindowAttribute(
                w,
                DWMWA_CLOAKED as u32,
                (&mut cloaked as *mut u32).cast(),
                4,
            );
            let ex = GetWindowLongW(w, GWL_EXSTYLE) as u32;
            let mut r: RECT = std::mem::zeroed();
            GetWindowRect(w, &mut r);
            let overlap = r.left < me.right
                && r.right > me.left
                && r.top < me.bottom
                && r.bottom > me.top
                && r.right - r.left >= 8
                && r.bottom - r.top >= 8;
            if pid != own
                && IsWindowVisible(w) != 0
                && cloaked == 0
                && ex & WS_EX_TRANSPARENT == 0
                && overlap
            {
                let mut t = [0u16; 128];
                let len = GetWindowTextW(w, t.as_mut_ptr(), t.len() as i32).max(0) as usize;
                let exe = image_path(pid)
                    .and_then(|p| p.rsplit('\\').next().map(String::from))
                    .unwrap_or_default();
                out.push(format!(
                    "{pid} {exe}: {}",
                    String::from_utf16_lossy(&t[..len])
                ));
            }
            w = GetWindow(w, GW_HWNDPREV);
        }
    }
    out
}

pub fn focus(hwnd: HWND) -> bool {
    // SAFETY: window calls on a live handle. An ALT tap first lifts the foreground lock Windows puts on background processes.
    unsafe {
        // Several tries: with two game windows the second SetForegroundWindow is often refused the first time.
        for _ in 0..6 {
            if GetForegroundWindow() == hwnd {
                return true;
            }
            ShowWindow(hwnd, SW_RESTORE);
            key_event(0x12, false); // VK_MENU
            key_event(0x12, true);
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
            std::thread::sleep(Duration::from_millis(250));
        }
        GetForegroundWindow() == hwnd
    }
}

fn is_extended(vk: u16) -> bool {
    matches!(
        vk,
        0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0x6F | 0x90 | 0xA3 | 0xA5
    )
}

fn key_event(vk: u16, up: bool) {
    // SAFETY: SendInput with one well-formed keyboard INPUT built here.
    unsafe {
        let scan = MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16;
        let mut flags = KEYEVENTF_SCANCODE;
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        if is_extended(vk) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        let inp = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        SendInput(1, &inp, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Virtual-key code for a key name used in scenarios and `nro keys`.
pub fn vk(name: &str) -> Option<u16> {
    let n = name.to_ascii_uppercase();
    Some(match n.as_str() {
        "ENTER" | "RETURN" => 0x0D,
        "ESC" | "ESCAPE" => 0x1B,
        "SPACE" => 0x20,
        "TAB" => 0x09,
        "BACK" | "BACKSPACE" => 0x08,
        "UP" => 0x26,
        "DOWN" => 0x28,
        "LEFT" => 0x25,
        "RIGHT" => 0x27,
        "SHIFT" => 0x10,
        "CTRL" => 0x11,
        "ALT" => 0x12,
        "F1" => 0x70,
        "F2" => 0x71,
        "F3" => 0x72,
        "F4" => 0x73,
        "F5" => 0x74,
        "F6" => 0x75,
        "F7" => 0x76,
        "F8" => 0x77,
        "F9" => 0x78,
        "F10" => 0x79,
        "F11" => 0x7A,
        "F12" => 0x7B,
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_alphanumeric() => s.as_bytes()[0] as u16,
        _ => return None,
    })
}

/// One key: press, hold `hold`, release.
pub fn tap(vk: u16, hold: Duration) {
    key_event(vk, false);
    std::thread::sleep(hold);
    key_event(vk, true);
}

/// A key sequence: `"ENTER; E*3; DOWN 300ms; wait 2s"`. `K*n` taps n times; `K 300ms` holds; `wait Ns|Nms` pauses.
pub fn keys(seq: &str) -> Result<(), String> {
    keys_in(None, seq)
}

/// Mouse click at a point given as fractions (0..1) of the window's client area.
pub fn click(hwnd: HWND, fx: f32, fy: f32) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    };
    click_with(hwnd, fx, fy, [MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP]);
}

/// Middle-button (wheel) click at a point given as fractions of the client area (the map's "Pin" button, RW-9 markers).
pub fn mclick(hwnd: HWND, fx: f32, fy: f32) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    };
    click_with(hwnd, fx, fy, [MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP]);
}

fn click_with(hwnd: HWND, fx: f32, fy: f32, flags: [u32; 2]) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::MOUSEINPUT;
    use windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos;
    // SAFETY: window geometry reads and SendInput with mouse INPUTs built here.
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut r);
        let mut p = POINT {
            x: ((r.right - r.left) as f32 * fx) as i32,
            y: ((r.bottom - r.top) as f32 * fy) as i32,
        };
        ClientToScreen(hwnd, &mut p);
        SetCursorPos(p.x, p.y);
        std::thread::sleep(Duration::from_millis(60));
        for f in flags {
            let inp = INPUT {
                r#type: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: f,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            SendInput(1, &inp, std::mem::size_of::<INPUT>() as i32);
            std::thread::sleep(Duration::from_millis(80));
        }
    }
}

/// Relative mouse movement in counts (mouse-look): games that read raw mouse input (Elden Ring's camera) see it.
pub fn mouse_move(dx: i32, dy: i32) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOUSEEVENTF_MOVE, MOUSEINPUT};
    let inp = INPUT {
        r#type: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // SAFETY: SendInput with one relative mouse INPUT built here.
    unsafe { SendInput(1, &inp, std::mem::size_of::<INPUT>() as i32) };
}

/// Press the left button at one point of the window's client area (fractions 0..1), move to another in small steps, release.
pub fn drag(hwnd: HWND, from: (f32, f32), to: (f32, f32)) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos;
    let button = |flag| {
        let inp = INPUT {
            r#type: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flag,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        // SAFETY: SendInput with one mouse INPUT built here.
        unsafe { SendInput(1, &inp, std::mem::size_of::<INPUT>() as i32) };
    };
    let at = |fx: f32, fy: f32| -> (i32, i32) {
        // SAFETY: window geometry reads on a live handle.
        unsafe {
            let mut r: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut r);
            let mut p = POINT {
                x: ((r.right - r.left) as f32 * fx) as i32,
                y: ((r.bottom - r.top) as f32 * fy) as i32,
            };
            ClientToScreen(hwnd, &mut p);
            (p.x, p.y)
        }
    };
    let (x0, y0) = at(from.0, from.1);
    let (x1, y1) = at(to.0, to.1);
    // SAFETY: cursor moves only.
    unsafe { SetCursorPos(x0, y0) };
    std::thread::sleep(Duration::from_millis(150));
    button(MOUSEEVENTF_LEFTDOWN);
    std::thread::sleep(Duration::from_millis(150));
    for i in 1..=20 {
        let x = x0 + (x1 - x0) * i / 20;
        let y = y0 + (y1 - y0) * i / 20;
        // SAFETY: cursor moves only.
        unsafe { SetCursorPos(x, y) };
        std::thread::sleep(Duration::from_millis(30));
    }
    std::thread::sleep(Duration::from_millis(150));
    button(MOUSEEVENTF_LEFTUP);
}

/// Like [`keys`]; with a window, `click fx fy` items click inside it.
pub fn keys_in(hwnd: Option<HWND>, seq: &str) -> Result<(), String> {
    for part in seq.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let mut it = part.split_whitespace();
        let head = it.next().unwrap_or("");
        // Every key or click goes only to the game window: stop the moment another window is in front (the user's PC).
        if let Some(h) = hwnd {
            if !head.eq_ignore_ascii_case("wait") && !is_foreground(h) {
                return Err(format!(
                    "stopped before {part:?}: the game window is no longer in front"
                ));
            }
        }
        if head.eq_ignore_ascii_case("click") || head.eq_ignore_ascii_case("mclick") {
            let h = hwnd.ok_or("click needs the game window")?;
            let fx: f32 = it
                .next()
                .and_then(|v| v.parse().ok())
                .ok_or("click fx fy (fractions 0..1)")?;
            let fy: f32 = it
                .next()
                .and_then(|v| v.parse().ok())
                .ok_or("click fx fy (fractions 0..1)")?;
            if head.eq_ignore_ascii_case("mclick") {
                mclick(h, fx, fy);
            } else {
                click(h, fx, fy);
            }
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        if head.eq_ignore_ascii_case("drag") {
            let h = hwnd.ok_or("drag needs a window")?;
            let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
            if v.len() != 4 {
                return Err("drag fx1 fy1 fx2 fy2 (fractions 0..1)".into());
            }
            drag(h, (v[0], v[1]), (v[2], v[3]));
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        if head.eq_ignore_ascii_case("mousemove") {
            let dx: i32 = it
                .next()
                .and_then(|v| v.parse().ok())
                .ok_or("mousemove dx dy (counts)")?;
            let dy: i32 = it
                .next()
                .and_then(|v| v.parse().ok())
                .ok_or("mousemove dx dy (counts)")?;
            mouse_move(dx, dy);
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        let arg = it.next();
        if head.eq_ignore_ascii_case("wait") {
            std::thread::sleep(parse_dur(arg.unwrap_or("1s"))?);
            continue;
        }
        let (name, times) = match head.split_once('*') {
            Some((n, t)) => (
                n,
                t.parse::<u32>()
                    .map_err(|_| format!("bad repeat in {part:?}"))?,
            ),
            None => (head, 1),
        };
        let code = vk(name).ok_or_else(|| format!("unknown key {name:?}"))?;
        let hold = match arg {
            Some(a) => parse_dur(a)?,
            None => Duration::from_millis(80),
        };
        for _ in 0..times {
            tap(code, hold);
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    Ok(())
}

pub fn parse_dur(s: &str) -> Result<Duration, String> {
    let s = s.trim().to_ascii_lowercase();
    if let Some(v) = s.strip_suffix("ms") {
        return v
            .parse()
            .map(Duration::from_millis)
            .map_err(|_| format!("bad duration {s:?}"));
    }
    if let Some(v) = s.strip_suffix('s') {
        return v
            .parse::<f64>()
            .map(Duration::from_secs_f64)
            .map_err(|_| format!("bad duration {s:?}"));
    }
    Err(format!("bad duration {s:?} (use 300ms or 2s)"))
}

/// Screenshot of a window's client area (copied from the screen, so the window is brought to the front first), saved as PNG.
pub fn screenshot(hwnd: HWND, out: &Path) -> Result<(u32, u32), String> {
    focus(hwnd);
    std::thread::sleep(Duration::from_millis(200));
    // SAFETY: GDI calls on DCs and a bitmap created and released here; the pixel buffer is sized from the bitmap header.
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut r);
        let mut p = POINT { x: 0, y: 0 };
        ClientToScreen(hwnd, &mut p);
        let (w, h) = ((r.right - r.left).max(1), (r.bottom - r.top).max(1));
        let screen = GetDC(std::ptr::null_mut());
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp);
        BitBlt(mem, 0, 0, w, h, screen, p.x, p.y, SRCCOPY);
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        GetDIBits(
            mem,
            bmp,
            0,
            h as u32,
            px.as_mut_ptr().cast(),
            &mut bi,
            DIB_RGB_COLORS,
        );
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);
        // BGRA â†’ RGB, and halve very wide captures so a picture stays readable and small.
        let step = if w > 1920 { 2 } else { 1 };
        let (ow, oh) = ((w / step) as u32, (h / step) as u32);
        let mut rgb = Vec::with_capacity((ow * oh * 3) as usize);
        for y in (0..h).step_by(step as usize).take(oh as usize) {
            for x in (0..w).step_by(step as usize).take(ow as usize) {
                let i = ((y * w + x) * 4) as usize;
                rgb.extend_from_slice(&[px[i + 2], px[i + 1], px[i]]);
            }
        }
        if let Some(d) = out.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let f = std::fs::File::create(out).map_err(|e| e.to_string())?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), ow, oh);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .and_then(|mut wr| wr.write_image_data(&rgb))
            .map_err(|e| e.to_string())?;
        Ok((ow, oh))
    }
}

/// Physical pixels everywhere (the game window is sized in physical pixels; without this a 125 % display scales our rectangles).
pub fn dpi_aware() {
    use windows_sys::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    // SAFETY: a process-wide setting with a valid constant; failure (already set) is harmless.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// The PNG is all (near) black: the capture did not see the screen.
pub fn looks_black(p: &Path) -> bool {
    let Ok(f) = std::fs::File::open(p) else {
        return false;
    };
    let Ok(mut r) = png::Decoder::new(std::io::BufReader::new(f)).read_info() else {
        return false;
    };
    let mut buf = vec![0; r.output_buffer_size()];
    if r.next_frame(&mut buf).is_err() {
        return false;
    }
    !buf.iter().step_by(97).any(|&b| b > 8)
}
