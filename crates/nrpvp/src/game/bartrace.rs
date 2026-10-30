//! T-041 (dev only, on by default; console `bar_trace [0|1]`): which game instruction writes the downed LOCAL player's
//! flags dword `PlayerIns+0x730..+0x733` (downs, "go down" `+0x731`, "revive completed" `+0x732`; game 1.03.3 / exe 1.3.3.0,
//! MEMORY_MAP section on the downed player). Step 1 watched the revive bar `+0x714` instead (TESTING §3).
//!
//! While LOCAL's state byte (`PlayerIns+0x710`) is 1 or 2 and tracing is ON, hardware slot 3 (`hwbp::SLOT_LABELS`, only if free) is a
//! 4-byte write watch on the bar. A data breakpoint is a trap: RIP already points AFTER the writing instruction, so the handler
//! changes neither RIP nor RF. The VEH records into a fixed ring of atomics (no logging, allocation or locks there); the main loop
//! drains it every tick ([`tick`]) and logs. Our own downed rule also writes the bar from the main thread; those hits show a RIP in
//! `nrpvp.dll`.
//!
//! Log lines start with `T-041`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, Once, PoisonError};

use windows::Win32::Foundation::STATUS_SINGLE_STEP;
use windows::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};
use windows::Win32::System::Threading::GetCurrentThreadId;

use super::combat_state::CombatCtl;
use super::layout;
use crate::dr::{EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH};
use crate::hwbp;

const SLOT: usize = hwbp::SLOT_LABELS;
const RING: usize = 64;
const STACK_QWORDS: usize = 16;
const MAX_LINES: u32 = 200;
const GPR_NAMES: [&str; 16] = [
    "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13",
    "r14", "r15",
];

/// Off by default: watching `+0x730` killed the game silently (T-041 run 2026-10-21 23:48, the anti-tamper reacting, HYPOTHESIS).
static ENABLED: AtomicBool = AtomicBool::new(false);
/// The VEH records only while this is set; it swallows our write traps either way (a trap with no handler would be fatal).
static RECORD: AtomicBool = AtomicBool::new(false);
/// Console `rune_watch 1` (RW-8 research): watch the rune counter of LOCAL's `PlayerGameData` instead of the downed flags.
static RUNES: AtomicBool = AtomicBool::new(false);
/// Console `trace_write <addr>`: watch this address (4-byte write watch) instead of the downed flags; 0 = none.
static TARGET: AtomicUsize = AtomicUsize::new(0);
/// Offset of the rune count in `PlayerGameData`: two equal copies (+0x6C and +0x110) changed together in a Nucleus match (2026-10-25).
const RUNE_OFF: usize = 0x6C;
/// Last address watched (never reset to 0 once set, so late traps after a disarm are still recognised as ours).
static ADDR: AtomicUsize = AtomicUsize::new(0);
/// Records written by the VEH so far (also the ring head).
static HEAD: AtomicU32 = AtomicU32::new(0);
static VEH_ONCE: Once = Once::new();

struct Rec {
    /// 0 = free / being written, else `index + 1` of the record it holds.
    seq: AtomicU32,
    rip: AtomicU64,
    value: AtomicU64,
    tid: AtomicU64,
    gpr: [AtomicU64; 16],
    stack: [AtomicU64; STACK_QWORDS],
}

#[allow(clippy::declare_interior_mutable_const)]
const REC0: Rec = Rec {
    seq: AtomicU32::new(0),
    rip: AtomicU64::new(0),
    value: AtomicU64::new(0),
    tid: AtomicU64::new(0),
    gpr: [const { AtomicU64::new(0) }; 16],
    stack: [const { AtomicU64::new(0) }; STACK_QWORDS],
};
static RECS: [Rec; RING] = [REC0; RING];

pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Records the VEH has captured so far.
pub(crate) fn hits() -> u64 {
    u64::from(HEAD.load(Ordering::Relaxed))
}

/// Console `trace_write <addr>` (research): trace the writers of one 4-byte aligned address (`0` stops).
pub(crate) fn set_target(addr: usize) -> Result<(), &'static str> {
    if addr != 0 && !addr.is_multiple_of(4) {
        return Err("the address must be 4-byte aligned");
    }
    TARGET.store(addr, Ordering::Relaxed);
    if addr != 0 {
        set_enabled(true);
    }
    tracing::info!("research trace_write: {addr:#X}");
    Ok(())
}

/// Console `rune_watch 0|1`: trace the writers of LOCAL's rune count (turns the bar trace on).
pub(crate) fn set_runes(on: bool) {
    RUNES.store(on, Ordering::Relaxed);
    if on {
        set_enabled(true);
    }
    tracing::info!("RW-8 rune_watch: {}", if on { "ON" } else { "OFF" });
}

/// Console `bar_trace 0|1`. The VEH is registered once, on the first enable.
pub(crate) fn set_enabled(on: bool) {
    if on {
        VEH_ONCE.call_once(|| {
            // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature and returns CONTINUE_SEARCH for anything that is not a
            // write trap of slot 3.
            if unsafe { AddVectoredExceptionHandler(1, Some(veh)) }.is_null() {
                tracing::warn!("T-041 bar_trace: AddVectoredExceptionHandler failed");
            }
        });
    }
    ENABLED.store(on, Ordering::Relaxed);
    tracing::info!("T-041 bar_trace: {}", if on { "ON" } else { "OFF" });
}

/// Bounds `(low, high)` of the current thread's stack from the TEB (`gs:[8]` StackBase, `gs:[16]` StackLimit).
#[inline]
fn stack_bounds() -> (usize, usize) {
    let (hi, lo): (usize, usize);
    // SAFETY: reads two qwords of the TEB through gs, which always points at the current thread's TEB on x86_64 Windows.
    unsafe {
        std::arch::asm!("mov {0}, gs:[8]", "mov {1}, gs:[16]", out(reg) hi, out(reg) lo,
            options(nostack, preserves_flags, readonly));
    }
    (lo, hi)
}

unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS; pointers are null-checked. The watched address was a live PlayerIns field
    // when armed and the CPU just trapped on a write to it, so reading 4 bytes there is safe; the stack words are read only after the
    // range [rsp, rsp+0x80) is checked against this thread's TEB stack bounds. Nothing here allocates, logs or locks.
    unsafe {
        if info.is_null() {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let rec = (*info).ExceptionRecord;
        let ctx = (*info).ContextRecord;
        if rec.is_null() || ctx.is_null() || (*rec).ExceptionCode != STATUS_SINGLE_STEP {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let addr = ADDR.load(Ordering::Relaxed);
        // Slot 3 hit, and slot 3 is a data watch (RW3 = 01 write, or 11 read/write for `trace_read`, in DR7 bits 28-29), and we have armed it at least once.
        if addr == 0 || (*ctx).Dr6 & 8 == 0 || !matches!(((*ctx).Dr7 >> 28) & 3, 1 | 3) {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        if RECORD.load(Ordering::Relaxed) {
            let idx = HEAD.fetch_add(1, Ordering::AcqRel);
            let r = &RECS[idx as usize % RING];
            r.seq.store(0, Ordering::Release);
            r.rip.store((*ctx).Rip, Ordering::Relaxed);
            r.value
                .store(u64::from(*(addr as *const u32)), Ordering::Relaxed);
            r.tid
                .store(u64::from(GetCurrentThreadId()), Ordering::Relaxed);
            let regs = [
                (*ctx).Rax,
                (*ctx).Rcx,
                (*ctx).Rdx,
                (*ctx).Rbx,
                (*ctx).Rsp,
                (*ctx).Rbp,
                (*ctx).Rsi,
                (*ctx).Rdi,
                (*ctx).R8,
                (*ctx).R9,
                (*ctx).R10,
                (*ctx).R11,
                (*ctx).R12,
                (*ctx).R13,
                (*ctx).R14,
                (*ctx).R15,
            ];
            for (i, v) in regs.iter().enumerate() {
                r.gpr[i].store(*v, Ordering::Relaxed);
            }
            let rsp = (*ctx).Rsp as usize;
            let (lo, hi) = stack_bounds();
            let ok = rsp.is_multiple_of(8)
                && rsp >= lo
                && lo != 0
                && rsp.checked_add(STACK_QWORDS * 8).is_some_and(|e| e <= hi);
            for i in 0..STACK_QWORDS {
                let v = if ok {
                    *((rsp + i * 8) as *const u64)
                } else {
                    0
                };
                r.stack[i].store(v, Ordering::Relaxed);
            }
            r.seq.store(idx.wrapping_add(1), Ordering::Release);
        }
        (*ctx).Dr6 = 0;
    }
    EXCEPTION_CONTINUE_EXECUTION
}

/// One captured write (copied out of the ring).
struct Hit {
    rip: u64,
    value: u32,
    tid: u32,
    gpr: [u64; 16],
    stack: [u64; STACK_QWORDS],
}

fn read_rec(n: u32) -> Option<Hit> {
    let r = &RECS[n as usize % RING];
    let want = n.wrapping_add(1);
    if r.seq.load(Ordering::Acquire) != want {
        return None;
    }
    let hit = Hit {
        rip: r.rip.load(Ordering::Relaxed),
        value: r.value.load(Ordering::Relaxed) as u32,
        tid: r.tid.load(Ordering::Relaxed) as u32,
        gpr: std::array::from_fn(|i| r.gpr[i].load(Ordering::Relaxed)),
        stack: std::array::from_fn(|i| r.stack[i].load(Ordering::Relaxed)),
    };
    // Overwritten while we copied: drop it.
    (r.seq.load(Ordering::Acquire) == want).then_some(hit)
}

struct State {
    armed: bool,
    armed_addr: usize,
    busy_logged: bool,
    tail: u32,
    writes: u32,
    lines: u32,
    seen: BTreeSet<u64>,
    counts: BTreeMap<u64, u32>,
}

static STATE: Mutex<State> = Mutex::new(State {
    armed: false,
    armed_addr: 0,
    busy_logged: false,
    tail: 0,
    writes: 0,
    lines: 0,
    seen: BTreeSet::new(),
    counts: BTreeMap::new(),
});

/// `exe+0x…`, `module+0x…` or `0x…` for an address.
fn label(addr: u64, base: usize, size: usize) -> String {
    if in_exe(addr, base, size) {
        return format!("exe+0x{:X}", addr as usize - base);
    }
    match crate::steam::lobtrace::module_of(addr) {
        Some((name, off)) => format!("{name}+0x{off:X}"),
        None => format!("0x{addr:X}"),
    }
}

fn in_exe(v: u64, base: usize, size: usize) -> bool {
    let v = v as usize;
    v >= base && v - base < size
}

fn log_hit(st: &mut State, h: &Hit, base: usize, size: usize) {
    st.writes += 1;
    *st.counts.entry(h.rip).or_insert(0) += 1;
    let first = st.seen.insert(h.rip);
    // T-041 step 4: the watched dword is `+0x730..+0x733` (downs, "go down", "revive completed", spare). The game clears
    // `+0x731..+0x732` every frame, so only a RIP seen for the first time or a write that leaves a flag set gets a line.
    let flags_set = h.value & 0x00FF_FF00 != 0;
    if st.lines >= MAX_LINES || !(first || flags_set) {
        return;
    }
    st.lines += 1;
    tracing::info!(
        "T-041 FLAG WRITE #{}: rip={} downs={} go_down={} revive_done={} (+0x733={}) tid={}{}",
        st.writes,
        label(h.rip, base, size),
        h.value & 0xFF,
        (h.value >> 8) & 0xFF,
        (h.value >> 16) & 0xFF,
        (h.value >> 24) & 0xFF,
        h.tid,
        if st.lines == MAX_LINES {
            " (line limit reached: only counting from now on)"
        } else {
            ""
        }
    );
    if first {
        let regs: Vec<String> = GPR_NAMES
            .iter()
            .zip(h.gpr.iter())
            .map(|(n, v)| format!("{n}=0x{v:X}"))
            .collect();
        tracing::info!("T-041 FLAG WRITE regs: {}", regs.join(" "));
        let cands: Vec<String> = h
            .stack
            .iter()
            .enumerate()
            .filter(|(_, v)| in_exe(**v, base, size))
            .map(|(i, v)| format!("[rsp+0x{:X}]=exe+0x{:X}", i * 8, *v as usize - base))
            .collect();
        tracing::info!(
            "T-041 FLAG WRITE stack (return-address candidates): {}",
            if cands.is_empty() {
                "none in the exe".to_string()
            } else {
                cands.join(" ")
            }
        );
    }
}

fn drain(st: &mut State, base: usize, size: usize) {
    let head = HEAD.load(Ordering::Acquire);
    if head.wrapping_sub(st.tail) as usize > RING {
        st.tail = head.wrapping_sub(RING as u32);
    }
    while st.tail != head {
        if let Some(h) = read_rec(st.tail) {
            log_hit(st, &h, base, size);
        }
        st.tail = st.tail.wrapping_add(1);
    }
}

fn disarm(st: &mut State, base: usize, size: usize) {
    RECORD.store(false, Ordering::Relaxed);
    hwbp::clear(SLOT);
    drain(st, base, size);
    let mut per: Vec<(u64, u32)> = st.counts.iter().map(|(k, v)| (*k, *v)).collect();
    per.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let list: Vec<String> = per
        .iter()
        .map(|(rip, n)| format!("{} ×{n}", label(*rip, base, size)))
        .collect();
    tracing::info!(
        "T-041 bar_trace: disarmed after {} writes{}{}",
        st.writes,
        if list.is_empty() { "" } else { ": " },
        list.join(", ")
    );
    st.armed = false;
    st.armed_addr = 0;
    st.writes = 0;
    st.lines = 0;
    st.seen.clear();
    st.counts.clear();
}

/// Main loop, right after the combat tick: arm / disarm the watch from LOCAL's state, then log what the VEH captured.
pub(crate) fn tick(combat: &CombatCtl, base: usize, size: usize) {
    let mut st = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    let want = if enabled() && TARGET.load(Ordering::Relaxed) != 0 {
        Some(TARGET.load(Ordering::Relaxed))
    } else if enabled() && RUNES.load(Ordering::Relaxed) {
        combat
            .local_state()
            .and_then(|(a, _)| crate::game::mem::read::<usize>(a + layout::PLAYER_GAMEDATA))
            .filter(|g| crate::game::mem::is_ptr(*g))
            .map(|g| g + RUNE_OFF)
    } else if enabled() {
        combat
            .local_state()
            .filter(|(_, s)| *s == 1 || *s == 2)
            .map(|(a, _)| a + layout::PLAYER_DOWNS)
            .filter(|a| a.is_multiple_of(4))
    } else {
        None
    };
    match want {
        Some(addr) => {
            if !st.armed {
                if !hwbp::is_free(SLOT) {
                    if !st.busy_logged {
                        st.busy_logged = true;
                        tracing::info!("T-041 bar_trace: slot 3 busy (labels), not armed");
                    }
                    return;
                }
                st.busy_logged = false;
                st.tail = HEAD.load(Ordering::Acquire);
                st.armed = true;
            }
            if st.armed_addr != addr {
                ADDR.store(addr, Ordering::Relaxed);
                RECORD.store(true, Ordering::Relaxed);
                hwbp::set_write(SLOT, addr);
                st.armed_addr = addr;
                tracing::info!(
                    "T-041 bar_trace: armed write watch on 0x{addr:X} (LOCAL +0x730..+0x733: downs, go-down and revive-completed flags)"
                );
            }
            drain(&mut st, base, size);
        }
        None => {
            if st.armed {
                disarm(&mut st, base, size);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_range_check() {
        assert!(in_exe(0x1010, 0x1000, 0x100));
        assert!(!in_exe(0xFFF, 0x1000, 0x100));
        assert!(!in_exe(0x1100, 0x1000, 0x100));
    }
}
