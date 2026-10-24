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

pub fn focus(hwnd: HWND) -> bool {
    // SAFETY: window calls on a live handle. An ALT tap first lifts the foreground lock Windows puts on background processes.
    unsafe {
        if GetForegroundWindow() == hwnd {
            return true;
        }
        ShowWindow(hwnd, SW_RESTORE);
        key_event(0x12, false); // VK_MENU
        key_event(0x12, true);
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        std::thread::sleep(Duration::from_millis(150));
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
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT,
    };
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
        for f in [MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP] {
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

/// Like [`keys`]; with a window, `click fx fy` items click inside it.
pub fn keys_in(hwnd: Option<HWND>, seq: &str) -> Result<(), String> {
    for part in seq.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let mut it = part.split_whitespace();
        let head = it.next().unwrap_or("");
        if head.eq_ignore_ascii_case("click") {
            let h = hwnd.ok_or("click needs the game window")?;
            let fx: f32 = it
                .next()
                .and_then(|v| v.parse().ok())
                .ok_or("click fx fy (fractions 0..1)")?;
            let fy: f32 = it
                .next()
                .and_then(|v| v.parse().ok())
                .ok_or("click fx fy (fractions 0..1)")?;
            click(h, fx, fy);
            std::thread::sleep(Duration::from_millis(250));
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
        // BGRA → RGB, and halve very wide captures so a picture stays readable and small.
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
