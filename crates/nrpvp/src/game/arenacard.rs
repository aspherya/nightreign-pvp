//! ARENA-0 (dev only, console `arena_card`): make the Arena card (`NightBossMenuParam` row 17, `gamedata/nightbossmenu-rows.json`)
//! show in the Roundtable expedition list. Game 1.03.3 / exe 1.3.3.0, MEMORY_MAP §28.
//!
//! The list builder `exe+0x9E9330` shows rows 0..9 by position, then one card per byte `b` of the Everdark rotation list (row
//! `b + 10`), then row 100. The rotation list is a `vector<i8>` in `CSServerInterface` (FD4 slot `exe+0x3C17248`): the getter
//! `exe+0xAFED50` returns `instance + 0x30`, and `exe+0x9E5B10` copies `[+0x08, +0x10)` (begin, end) from it. So byte 7 in that
//! list shows row 17. The capacity at `+0x18` is `[HYPOTHESIS: MSVC vector layout, allocator first]`: the byte is only written
//! when `end < capacity`, never past it, and nothing is allocated here.

use std::sync::Mutex;

use super::mem;

/// `CSServerInterface` FD4 singleton slot (exe 1.3.3.0, `nrpvp.log` singleton list).
const RVA_SERVER_INTERFACE: usize = 0x3C1_7248;
/// The rotation list object inside it (the getter `exe+0xAFED50` returns `instance + 0x30`).
const ROTATION: usize = 0x30;
const BEGIN: usize = 0x08;
const END: usize = 0x10;
const CAP: usize = 0x18;
/// Row 17 = byte 7 (the list builder adds 10).
pub(crate) const ARENA_BYTE: u8 = 7;
/// A rotation list longer than this is not one (the game has 9 Everdark rows).
const MAX_LEN: usize = 64;

static LAST: Mutex<String> = Mutex::new(String::new());

fn record(line: String) {
    tracing::info!("{line}");
    *LAST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = line;
}

/// The last answer (console reply).
pub(crate) fn reply() -> String {
    LAST.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// What to do with the list, from the bytes, the room left and the request (pure; tested).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    Nothing,
    /// Append the byte at `end` (room is there).
    Append,
    /// Remove the byte at this index (shift the rest down, end - 1).
    Remove(usize),
    /// Asked to add, but the list has no spare capacity.
    NoRoom,
}

pub(crate) fn plan(bytes: &[u8], room: usize, want: Option<bool>) -> Plan {
    let at = bytes.iter().position(|&b| b == ARENA_BYTE);
    match (want, at) {
        (Some(true), None) if room > 0 => Plan::Append,
        (Some(true), None) => Plan::NoRoom,
        (Some(false), Some(i)) => Plan::Remove(i),
        _ => Plan::Nothing,
    }
}

/// Our flag that warps this game's player into Heolstor's arena (event 99100900, `gamedata/emevd-arena.json`).
const FLAG_WARP: u32 = 10_007_500;
/// After a Commence on the Arena card: how long LOCAL stands in the expedition before the warp.
const WARP_AFTER_MS: u64 = 3_000;

/// Main-loop state of [`tick`].
static PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SINCE: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Main loop, every tick. (1) The list hook holds hwbp slot 1 only at the Roundtable with no queue running: the quiet invite takes the
/// same slot only after a match is found, so the two never need it at once. (2) After a Commence on the Arena card, once LOCAL has
/// been in the expedition [`WARP_AFTER_MS`], set [`FLAG_WARP`] once.
pub(crate) fn tick(base: usize, in_expedition: bool, local: bool) {
    use std::sync::atomic::Ordering;
    let idle = crate::mm::phase() == crate::mm::PHASE_IDLE;
    if !in_expedition && idle {
        if !hook::armed() {
            if let Err(e) = hook::arm(base) {
                hook::note_once(&e);
            }
        }
    } else if hook::armed() {
        hook::disarm();
    }
    if crate::game::menuflow::take_arena_commence() {
        PENDING.store(true, Ordering::Relaxed);
        tracing::info!("arena_card: Commence on the Arena card; the warp follows {WARP_AFTER_MS} ms into the expedition");
    }
    if !PENDING.load(Ordering::Relaxed) {
        return;
    }
    let mut since = SINCE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !(in_expedition && local) {
        *since = None;
        return;
    }
    let t = *since.get_or_insert_with(std::time::Instant::now);
    if t.elapsed().as_millis() as u64 >= WARP_AFTER_MS {
        PENDING.store(false, Ordering::Relaxed);
        *since = None;
        match crate::game::flags::set(base, FLAG_WARP, true) {
            Ok(_) => {
                tracing::info!("arena_card: flag {FLAG_WARP} ON: warping into Heolstor's arena")
            }
            Err(e) => tracing::warn!("arena_card: flag {FLAG_WARP} not set: {e}"),
        }
    }
}

/// Console request codes (`arena_card`, `arena_card 1`, `0`, `hook`, `unhook`).
pub(crate) const REQ_SHOW: u8 = 0;
pub(crate) const REQ_ADD: u8 = 1;
pub(crate) const REQ_REMOVE: u8 = 2;
pub(crate) const REQ_HOOK: u8 = 3;
pub(crate) const REQ_UNHOOK: u8 = 4;

/// Console `arena_card [0|1|hook|unhook]` (main loop).
pub(crate) fn console_cmd(base: usize, req: u8) {
    let r = match req {
        REQ_HOOK => hook::arm(base),
        REQ_UNHOOK => Ok(hook::disarm()),
        REQ_ADD => run(base, Some(true)),
        REQ_REMOVE => run(base, Some(false)),
        _ => run(base, None).map(|s| format!("{s}; {}", hook::state())),
    };
    record(match r {
        Ok(s) => format!("arena_card: {s}"),
        Err(e) => format!("arena_card: error: {e}"),
    });
}

/// The normal-card route: the list builder's first loop (`exe+0x9E9330`) takes rows by POSITION 0..9 (`inc esi; cmp esi, 0xA` then
/// `jl exe+0x9E93A0` at `exe+0x9E9665`). An execute breakpoint on that `jl`: when the loop is about to end (esi = 10), esi becomes
/// [`ROW17_POSITION`] and the loop runs once more, so row 17 is added like the first ten (not as an Everdark card); then esi = 18 and
/// the loop ends. Registers rbx / rdi are set right before the `jl` (the loop's own values), r12 / r13 / r14 / r15 are kept by the body.
mod hook {
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
    use std::sync::Once;

    use windows::Win32::Foundation::STATUS_SINGLE_STEP;
    use windows::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_POINTERS,
    };

    use crate::dr::{EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH};

    const RVA_LOOP_JL: usize = 0x9E_9665;
    const RVA_LOOP_TOP: usize = 0x9E_93A0;
    /// The loop's own end: after row position 9.
    const LOOP_END: u64 = 10;
    /// Position of row 17 in the sorted table (rows 0..16 come before it).
    const ROW17_POSITION: u64 = 17;
    /// The quiet-invite slot: free while no invite runs (checked before use).
    const SLOT: usize = crate::hwbp::SLOT_QUIET_VTABLE;

    static ADDR: AtomicUsize = AtomicUsize::new(0);
    static TOP: AtomicUsize = AtomicUsize::new(0);
    static HITS: AtomicU32 = AtomicU32::new(0);
    static ADDED: AtomicU32 = AtomicU32::new(0);
    static VEH: Once = Once::new();
    static ARMED: AtomicBool = AtomicBool::new(false);

    unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
        // SAFETY: Windows passes a valid EXCEPTION_POINTERS; both pointers are null-checked before use.
        unsafe {
            if info.is_null() {
                return EXCEPTION_CONTINUE_SEARCH;
            }
            let (rec, ctx) = ((*info).ExceptionRecord, (*info).ContextRecord);
            if rec.is_null() || ctx.is_null() || (*rec).ExceptionCode != STATUS_SINGLE_STEP {
                return EXCEPTION_CONTINUE_SEARCH;
            }
            let at = ADDR.load(Ordering::Relaxed);
            if at == 0 || (*ctx).Rip as usize != at {
                return EXCEPTION_CONTINUE_SEARCH;
            }
            HITS.fetch_add(1, Ordering::Relaxed);
            if (*ctx).Rsi & 0xFFFF_FFFF == LOOP_END {
                (*ctx).Rsi = ROW17_POSITION;
                (*ctx).Rip = TOP.load(Ordering::Relaxed) as u64;
                ADDED.fetch_add(1, Ordering::Relaxed);
            }
            (*ctx).EFlags |= EFLAGS_RF;
        }
        EXCEPTION_CONTINUE_EXECUTION
    }

    pub(super) fn arm(base: usize) -> Result<String, String> {
        if ARMED.load(Ordering::Relaxed) {
            return Ok(state());
        }
        if !crate::hwbp::is_free(SLOT) {
            return Err(format!("hwbp slot {SLOT} is in use"));
        }
        TOP.store(base + RVA_LOOP_TOP, Ordering::Relaxed);
        // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature and passes on everything that is not its own address.
        VEH.call_once(|| unsafe {
            let _ = AddVectoredExceptionHandler(1, Some(veh));
        });
        ADDR.store(base + RVA_LOOP_JL, Ordering::Relaxed);
        crate::hwbp::set(SLOT, base + RVA_LOOP_JL);
        ARMED.store(true, Ordering::Relaxed);
        Ok(format!("{}: open the expedition menu", state()))
    }

    pub(super) fn disarm() -> String {
        if ARMED.swap(false, Ordering::Relaxed) {
            crate::hwbp::clear(SLOT);
            // The address stays known to the handler (a late hit on another thread must still be ours).
        }
        state()
    }

    pub(super) fn armed() -> bool {
        ARMED.load(Ordering::Relaxed)
    }

    /// Log an arming failure once per distinct reason (the main loop retries every tick).
    pub(super) fn note_once(e: &str) {
        static LAST: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
        let mut last = LAST
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *last != e {
            tracing::info!("arena_card: list hook not armed: {e}");
            *last = e.to_string();
        }
    }

    pub(super) fn state() -> String {
        let armed = if ARMED.load(Ordering::Relaxed) {
            "on"
        } else {
            "off"
        };
        format!(
            "hook {armed} (hwbp slot {SLOT} on exe+0x{RVA_LOOP_JL:X}), {} hit(s), row 17 added {} time(s)",
            HITS.load(Ordering::Relaxed),
            ADDED.load(Ordering::Relaxed)
        )
    }
}

fn run(base: usize, want: Option<bool>) -> Result<String, String> {
    let inst = mem::read::<usize>(base + RVA_SERVER_INTERFACE)
        .filter(|p| mem::is_ptr(*p))
        .ok_or("no CSServerInterface instance")?;
    let v = inst + ROTATION;
    let begin = mem::read::<usize>(v + BEGIN).ok_or("rotation begin unreadable")?;
    let end = mem::read::<usize>(v + END).ok_or("rotation end unreadable")?;
    let cap = mem::read::<usize>(v + CAP).ok_or("rotation capacity unreadable")?;
    let len = end
        .checked_sub(begin)
        .filter(|n| *n <= MAX_LEN)
        .ok_or_else(|| format!("not a list: begin {begin:#X} end {end:#X} cap {cap:#X}"))?;
    let bytes = if len == 0 {
        Vec::new()
    } else {
        let mut out = vec![0u8; len];
        if !mem::copy_out(begin, &mut out) {
            return Err(format!("rotation bytes at {begin:#X} unreadable"));
        }
        out
    };
    let room = if begin != 0 && cap >= end {
        cap - end
    } else {
        0
    };
    let shown = |b: &[u8]| -> String {
        let rows: Vec<String> = b
            .iter()
            .map(|x| format!("{x} (row {})", i32::from(*x as i8) + 10))
            .collect();
        format!("[{}]", rows.join(", "))
    };
    let before = format!(
        "list at {v:#X}: {} byte(s) {} begin {begin:#X} end {end:#X} cap {cap:#X} (room {room})",
        bytes.len(),
        shown(&bytes)
    );
    match plan(&bytes, room, want) {
        Plan::Nothing => Ok(before),
        Plan::NoRoom if begin == 0 && end == 0 && cap == 0 => {
            // Offline / no server rotation: the list was never allocated. Ask the list's own allocator (`v + 0`, the one the
            // game's copy `exe+0x9E5B10` calls as `[vtable+0x50](allocator, size, 1)`) for a buffer, so a later free by the
            // game frees its own memory.
            let p = allocate(v, ALLOC_BYTES)?;
            mem::write_bytes(p, &[ARENA_BYTE])?;
            mem::write_bytes(v + BEGIN, &p.to_le_bytes())?;
            mem::write_bytes(v + CAP, &(p + ALLOC_BYTES).to_le_bytes())?;
            write_end(v, p + 1)?;
            Ok(format!(
                "{before}; allocated {ALLOC_BYTES} bytes at {p:#X} with the list's allocator, added byte {ARENA_BYTE} (row 17, Arena): open the expedition menu"
            ))
        }
        Plan::NoRoom => Err(format!(
            "{before}; no spare capacity, byte {ARENA_BYTE} not added"
        )),
        Plan::Append => {
            mem::write_bytes(end, &[ARENA_BYTE])?;
            write_end(v, end + 1)?;
            Ok(format!(
                "{before}; added byte {ARENA_BYTE} (row 17, Arena): reopen the expedition menu"
            ))
        }
        Plan::Remove(i) => {
            let rest: Vec<u8> = bytes[i + 1..].to_vec();
            for (k, b) in rest.iter().enumerate() {
                mem::write_bytes(begin + i + k, &[*b])?;
            }
            write_end(v, end - 1)?;
            Ok(format!("{before}; removed byte {ARENA_BYTE}"))
        }
    }
}

/// Bytes asked from the game's allocator for an empty list (room for the 9 Everdark bytes and ours).
const ALLOC_BYTES: usize = 16;
/// The allocate slot of the list allocator's vtable (`exe+0x9E5B6A`: `call [rax+0x50]` with rdx = size, r8d = 1).
const VT_ALLOCATE: usize = 0x50;

/// `allocator->allocate(size, 1)` through the list's own allocator object at `v + 0`.
fn allocate(v: usize, size: usize) -> Result<usize, String> {
    let alloc = mem::read::<usize>(v)
        .filter(|p| mem::is_ptr(*p))
        .ok_or("the list has no allocator")?;
    let vt = mem::read::<usize>(alloc)
        .filter(|p| mem::is_ptr(*p))
        .ok_or("allocator vtable unreadable")?;
    let f = mem::read::<usize>(vt + VT_ALLOCATE)
        .filter(|p| mem::is_ptr(*p))
        .ok_or("allocator allocate slot unreadable")?;
    // SAFETY: `f` is the allocate entry of the game's own allocator vtable, called with the arguments the game itself passes at
    // exe+0x9E5B6A (this, size, 1); it returns a heap pointer or null. Dev-only research path.
    let call: unsafe extern "system" fn(usize, usize, u32) -> usize =
        unsafe { std::mem::transmute(f) };
    let p = unsafe { call(alloc, size, 1) };
    if mem::is_ptr(p) {
        Ok(p)
    } else {
        Err(format!("allocator returned {p:#X}"))
    }
}

fn write_end(v: usize, end: usize) -> Result<(), String> {
    mem::write_bytes(v + END, &end.to_le_bytes()).map_err(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arena_byte_is_added_only_with_room_and_only_once() {
        assert_eq!(plan(&[0, 3], 2, Some(true)), Plan::Append);
        assert_eq!(plan(&[], 1, Some(true)), Plan::Append);
        assert_eq!(plan(&[0, 3], 0, Some(true)), Plan::NoRoom);
        assert_eq!(plan(&[0, 7], 2, Some(true)), Plan::Nothing);
        assert_eq!(plan(&[0, 7, 3], 0, Some(false)), Plan::Remove(1));
        assert_eq!(plan(&[0, 3], 0, Some(false)), Plan::Nothing);
        assert_eq!(plan(&[0, 3], 5, None), Plan::Nothing);
    }
}
