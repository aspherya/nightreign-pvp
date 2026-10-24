//! ORCH-1 (dev only): a virtual gamepad, so an agent can press buttons in a game whose window it cannot see or focus (the
//! orchestrator, `nightreign-orchestrator`, runs where screen input does not reach the game).
//!
//! The game reads pads through `XINPUT1_4.dll` ordinal 2 (`XInputGetState`), imported by ordinal (exe 1.3.3.0 import table, checked
//! 2026-10-23). On first use this module points that one import-table slot at [`get_state`] (a data write in the IAT, no game
//! code is patched). [`get_state`] calls the real function and, for pad 0, ORs in the buttons and sticks of the current entry of a
//! timed queue that the console fills (`pad`, `pad_seq`). When the queue is empty the pad looks exactly like the real one (or "not
//! connected" when there is none), so nothing changes for a player at the keyboard.
//!
//! The hook runs on the game's input thread: it takes no lock that the pipe thread can hold for long (a `try_lock` on the queue;
//! a contended call simply reports the last state), never allocates and never logs. Log lines (pipe and main threads) start with
//! `vpad:`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, Once, PoisonError};
use std::time::{Duration, Instant};

use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{VirtualProtect, PAGE_PROTECTION_FLAGS, PAGE_READWRITE};
use windows::Win32::System::Threading::GetCurrentThreadId;

/// `XINPUT_GAMEPAD` button bits.
pub(crate) const BUTTONS: [(&str, u16); 14] = [
    ("UP", 0x0001),
    ("DOWN", 0x0002),
    ("LEFT", 0x0004),
    ("RIGHT", 0x0008),
    ("START", 0x0010),
    ("BACK", 0x0020),
    ("LS", 0x0040),
    ("RS", 0x0080),
    ("LB", 0x0100),
    ("RB", 0x0200),
    ("A", 0x1000),
    ("B", 0x2000),
    ("X", 0x4000),
    ("Y", 0x8000),
];

/// One pad state held for a while.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Pad {
    pub buttons: u16,
    pub lt: u8,
    pub rt: u8,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct XInputState {
    packet: u32,
    buttons: u16,
    lt: u8,
    rt: u8,
    lx: i16,
    ly: i16,
    rx: i16,
    ry: i16,
}

type GetStateFn = unsafe extern "system" fn(u32, *mut XInputState) -> u32;

const ERROR_SUCCESS: u32 = 0;
const XINPUT_ORDINAL_GET_STATE: u64 = 2;

static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static INSTALL: Once = Once::new();
static INSTALL_RESULT: Mutex<String> = Mutex::new(String::new());
static CALLS: AtomicU64 = AtomicU64::new(0);
/// Pad 0 reports "connected" (a zero state when no real pad is there) so the game's device scan adopts it.
static CONNECTED: AtomicBool = AtomicBool::new(false);
static POSTED: AtomicU64 = AtomicU64::new(0);
static THREAD: AtomicU32 = AtomicU32::new(0);
static PACKET: AtomicU32 = AtomicU32::new(0x4000_0000);
/// The state reported last for pad 0 (packed buttons | lt << 16 | rt << 24), for `pad_status`.
static LAST: AtomicU64 = AtomicU64::new(0);

struct Queue {
    items: VecDeque<(Pad, Duration)>,
    started: Option<Instant>,
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue {
    items: VecDeque::new(),
    started: None,
});

/// The pad state now, popping finished entries (pure on the queue; unit tested).
fn current(q: &mut Queue, now: Instant) -> Option<Pad> {
    loop {
        let (pad, dur) = *q.items.front()?;
        let start = *q.started.get_or_insert(now);
        if now.duration_since(start) < dur {
            return Some(pad);
        }
        q.items.pop_front();
        q.started = None;
        if q.items.is_empty() {
            return None;
        }
    }
}

unsafe extern "system" fn get_state(idx: u32, out: *mut XInputState) -> u32 {
    CALLS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: GetCurrentThreadId has no preconditions.
    THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
    let orig = ORIGINAL.load(Ordering::Relaxed);
    let mut rc = 1167; // ERROR_DEVICE_NOT_CONNECTED
    if orig != 0 {
        // SAFETY: `orig` is the import-table value we replaced: the real XInputGetState.
        rc = unsafe { std::mem::transmute::<usize, GetStateFn>(orig)(idx, out) };
    }
    if idx != 0 || out.is_null() {
        return rc;
    }
    let pad = match QUEUE.try_lock() {
        Ok(mut q) => current(&mut q, Instant::now()),
        Err(_) => None,
    };
    let connected = CONNECTED.load(Ordering::Relaxed);
    let p = match pad {
        Some(p) => p,
        None if connected && rc != ERROR_SUCCESS => Pad::default(),
        None => return rc,
    };
    // SAFETY: the caller passed a writable XINPUT_STATE (checked non-null above).
    unsafe {
        let o = &mut *out;
        if rc != ERROR_SUCCESS {
            *o = XInputState::default();
        }
        o.buttons |= p.buttons;
        o.lt = o.lt.max(p.lt);
        o.rt = o.rt.max(p.rt);
        if p.lx != 0 || p.ly != 0 {
            o.lx = p.lx;
            o.ly = p.ly;
        }
        if p.rx != 0 || p.ry != 0 {
            o.rx = p.rx;
            o.ry = p.ry;
        }
        // A new packet number every call while we drive the pad, so the game never treats it as unchanged.
        o.packet = PACKET.fetch_add(1, Ordering::Relaxed);
        LAST.store(
            o.buttons as u64 | (o.lt as u64) << 16 | (o.rt as u64) << 24,
            Ordering::Relaxed,
        );
    }
    ERROR_SUCCESS
}

/// Point the exe's `XINPUT1_4.dll` ordinal 2 import at [`get_state`]. Idempotent; returns what happened.
pub(crate) fn install() -> String {
    INSTALL.call_once(|| {
        let r = install_once();
        match &r {
            Ok(m) => tracing::info!("vpad: {m}"),
            Err(e) => tracing::warn!("vpad: not installed: {e}"),
        }
        *INSTALL_RESULT
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = match r {
            Ok(m) => m,
            Err(e) => format!("not installed: {e}"),
        };
    });
    INSTALL_RESULT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn rd<T: Copy>(a: usize) -> Result<T, String> {
    crate::game::mem::read::<T>(a).ok_or_else(|| format!("unreadable 0x{a:X}"))
}

fn install_once() -> Result<String, String> {
    // SAFETY: GetModuleHandleW(None) returns the exe's own base; no preconditions.
    let base = unsafe { GetModuleHandleW(None) }
        .map_err(|e| e.to_string())?
        .0 as usize;
    let pe = base + rd::<u32>(base + 0x3C)? as usize;
    let imp = rd::<u32>(pe + 0x18 + 0x70 + 8)? as usize; // OptionalHeader64.DataDirectory[1].VirtualAddress
    if imp == 0 {
        return Err("no import directory".into());
    }
    let mut d = base + imp;
    loop {
        let oft = rd::<u32>(d)? as usize;
        let name = rd::<u32>(d + 12)? as usize;
        let ft = rd::<u32>(d + 16)? as usize;
        if name == 0 {
            return Err("XINPUT1_4.dll is not imported".into());
        }
        let n = crate::game::mem::read_cstr(base + name, 64).unwrap_or_default();
        if n.eq_ignore_ascii_case("XINPUT1_4.dll") {
            let mut i = 0;
            loop {
                let thunk = rd::<u64>(base + if oft != 0 { oft } else { ft } + i * 8)?;
                if thunk == 0 {
                    return Err("ordinal 2 not in the XINPUT1_4 imports".into());
                }
                if thunk & (1 << 63) != 0 && thunk & 0xFFFF == XINPUT_ORDINAL_GET_STATE {
                    let slot = base + ft + i * 8;
                    let old = rd::<usize>(slot)?;
                    ORIGINAL.store(old, Ordering::Relaxed);
                    let mut prev = PAGE_PROTECTION_FLAGS(0);
                    // SAFETY: `slot` is the 8-byte IAT entry of this module; made writable for the store and put back.
                    unsafe {
                        VirtualProtect(slot as *const _, 8, PAGE_READWRITE, &mut prev)
                            .map_err(|e| e.to_string())?;
                        std::ptr::write_volatile(
                            slot as *mut usize,
                            get_state as *const () as usize,
                        );
                        let mut tmp = PAGE_PROTECTION_FLAGS(0);
                        let _ = VirtualProtect(slot as *const _, 8, prev, &mut tmp);
                    }
                    return Ok(format!(
                        "XInputGetState import at exe+0x{:X} now goes through the virtual pad (real function 0x{old:X})",
                        slot - base
                    ));
                }
                i += 1;
            }
        }
        d += 20;
    }
}

struct Windows {
    pid: u32,
    found: Vec<isize>,
}

unsafe extern "system" fn enum_cb(
    hwnd: windows::Win32::Foundation::HWND,
    lp: windows::Win32::Foundation::LPARAM,
) -> windows::core::BOOL {
    // SAFETY: `lp` is the &mut Windows passed by `connect`, alive for the whole EnumWindows call.
    unsafe {
        let w = &mut *(lp.0 as *mut Windows);
        let mut pid = 0u32;
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == w.pid {
            w.found.push(hwnd.0 as isize);
        }
    }
    true.into()
}

/// `pad_connect [0|1]`: pad 0 reports connected (or not), then every top-level window of the game gets `WM_DEVICECHANGE`
/// (`DBT_DEVNODES_CHANGED`), which makes the game scan its XInput pads again (exe+0x2210C79 -> exe+0x21CC7F0, the scan of pads 0..3).
pub(crate) fn connect(on: bool) -> String {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, PostMessageW, WM_DEVICECHANGE};
    let installed = install();
    CONNECTED.store(on, Ordering::Relaxed);
    let mut w = Windows {
        pid: std::process::id(),
        found: Vec::new(),
    };
    // SAFETY: the callback only reads window data into `w`, which outlives the call.
    let _ = unsafe { EnumWindows(Some(enum_cb), LPARAM(&mut w as *mut Windows as isize)) };
    let mut posted = 0;
    for h in &w.found {
        // SAFETY: posting a message to a window of this process; DBT_DEVNODES_CHANGED = 7 carries no pointer.
        if unsafe {
            PostMessageW(
                Some(HWND(*h as *mut _)),
                WM_DEVICECHANGE,
                WPARAM(7),
                LPARAM(0),
            )
        }
        .is_ok()
        {
            posted += 1;
        }
    }
    POSTED.fetch_add(posted, Ordering::Relaxed);
    tracing::info!(
        "vpad: pad 0 {}; WM_DEVICECHANGE posted to {posted} window(s)",
        if on { "connected" } else { "disconnected" }
    );
    format!(
        "pad 0 {}; device change posted to {posted} of {} window(s); {installed}",
        if on { "connected" } else { "disconnected" },
        w.found.len()
    )
}

/// `A+DOWN`, `none` → buttons.
pub(crate) fn parse_buttons(s: &str) -> Result<u16, String> {
    let mut b = 0;
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let p = part.to_ascii_uppercase();
        if p == "NONE" {
            continue;
        }
        b |= BUTTONS
            .iter()
            .find(|(n, _)| *n == p)
            .map(|(_, v)| *v)
            .ok_or_else(|| {
                format!(
                    "unknown button {part:?} (buttons: {} or none)",
                    BUTTONS.map(|(n, _)| n).join(" ")
                )
            })?;
    }
    Ok(b)
}

/// One sequence item: `A`, `A 200`, `wait 500`, `LT 300` (trigger), `LS 0 1 1500` (stick x y in -1..1, ms), `RS -1 0 400`.
pub(crate) fn parse_item(s: &str) -> Result<(Pad, Duration), String> {
    let w: Vec<&str> = s.split_whitespace().collect();
    let ms = |x: Option<&&str>, d: u64| -> Result<Duration, String> {
        x.map(|v| v.parse::<u64>().map_err(|_| format!("bad ms {v:?}")))
            .transpose()
            .map(|v| Duration::from_millis(v.unwrap_or(d)))
    };
    let axis = |v: &str| -> Result<i16, String> {
        let f: f32 = v.parse().map_err(|_| format!("bad axis {v:?}"))?;
        Ok((f.clamp(-1.0, 1.0) * 32767.0) as i16)
    };
    let head = w.first().ok_or("empty item")?.to_ascii_uppercase();
    let mut pad = Pad::default();
    let dur = match head.as_str() {
        "WAIT" => ms(w.get(1), 500)?,
        "LT" | "RT" => {
            if head == "LT" {
                pad.lt = 255
            } else {
                pad.rt = 255
            }
            ms(w.get(1), 120)?
        }
        "LS" | "RS" if w.len() >= 3 => {
            let (x, y) = (axis(w[1])?, axis(w[2])?);
            if head == "LS" {
                (pad.lx, pad.ly) = (x, y)
            } else {
                (pad.rx, pad.ry) = (x, y)
            }
            ms(w.get(3), 500)?
        }
        _ => {
            pad.buttons = parse_buttons(w[0])?;
            ms(w.get(1), 120)?
        }
    };
    if dur > Duration::from_secs(60) {
        return Err("an item lasts at most 60000 ms".into());
    }
    Ok((pad, dur))
}

/// Queue a sequence (`;`-separated items). A release gap of 120 ms follows every press so repeated presses register.
pub(crate) fn queue(seq: &str) -> Result<String, String> {
    let mut items = Vec::new();
    for it in seq.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (p, d) = parse_item(it)?;
        let press = p != Pad::default();
        items.push((p, d));
        if press {
            items.push((Pad::default(), Duration::from_millis(120)));
        }
    }
    let total: Duration = items.iter().map(|(_, d)| *d).sum();
    let installed = if CONNECTED.load(Ordering::Relaxed) {
        install()
    } else {
        connect(true)
    };
    let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    q.items.extend(items);
    let n = q.items.len();
    drop(q);
    tracing::info!("vpad: queued {seq:?} ({} ms)", total.as_millis());
    Ok(format!(
        "queued, {} ms; {n} item(s) waiting; {installed}",
        total.as_millis()
    ))
}

pub(crate) fn clear() -> String {
    let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    let n = q.items.len();
    q.items.clear();
    q.started = None;
    format!("cleared {n} item(s)")
}

pub(crate) fn status() -> String {
    let q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    let last = LAST.load(Ordering::Relaxed);
    format!(
        "vpad: {}; pad 0 {}; device-change posts {}; XInputGetState calls {} (thread {}); {} item(s) waiting; last driven state buttons 0x{:04X} lt {} rt {}",
        INSTALL_RESULT.lock().unwrap_or_else(PoisonError::into_inner),
        if CONNECTED.load(Ordering::Relaxed) { "connected" } else { "not forced" },
        POSTED.load(Ordering::Relaxed),
        CALLS.load(Ordering::Relaxed),
        THREAD.load(Ordering::Relaxed),
        q.items.len(),
        last & 0xFFFF,
        (last >> 16) & 0xFF,
        (last >> 24) & 0xFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_parse() {
        assert_eq!(
            parse_item("A").unwrap(),
            (
                Pad {
                    buttons: 0x1000,
                    ..Pad::default()
                },
                Duration::from_millis(120)
            )
        );
        assert_eq!(parse_item("a+down 300").unwrap().0.buttons, 0x1002);
        assert_eq!(
            parse_item("wait 700").unwrap(),
            (Pad::default(), Duration::from_millis(700))
        );
        let (p, d) = parse_item("LS 0 1 1500").unwrap();
        assert_eq!((p.lx, p.ly, d), (0, 32767, Duration::from_millis(1500)));
        assert_eq!(parse_item("RT").unwrap().0.rt, 255);
        assert!(parse_item("Q").is_err());
        assert!(parse_item("A 70000").is_err());
    }

    #[test]
    fn the_queue_plays_in_order() {
        let mut q = Queue {
            items: VecDeque::new(),
            started: None,
        };
        let t0 = Instant::now();
        q.items.push_back((
            Pad {
                buttons: 1,
                ..Pad::default()
            },
            Duration::from_millis(100),
        ));
        q.items.push_back((
            Pad {
                buttons: 2,
                ..Pad::default()
            },
            Duration::from_millis(100),
        ));
        assert_eq!(current(&mut q, t0).unwrap().buttons, 1);
        assert_eq!(
            current(&mut q, t0 + Duration::from_millis(50))
                .unwrap()
                .buttons,
            1
        );
        assert_eq!(
            current(&mut q, t0 + Duration::from_millis(150))
                .unwrap()
                .buttons,
            2
        );
        assert_eq!(current(&mut q, t0 + Duration::from_millis(400)), None);
        assert!(q.items.is_empty());
    }
}
