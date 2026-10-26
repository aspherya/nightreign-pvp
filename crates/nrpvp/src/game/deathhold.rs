//! M5-9 (both builds): hold the comeback of a dead LOCAL player and let it go later. Game 1.03.3 / exe 1.3.3.0; the reading and the
//! solo proof are in `research/M5-8-death.md` §R2 and MEMORY_MAP §17.
//!
//! The comeback is a chain of timed callbacks on the game's Lua event sequencer. Its first step after the death wait is
//! `exe+0x5C4810(ctx, seq)` ([`P_START`], `void`), which queues the fade, the "begin respawn" step and the respawn at the nearest
//! respawn point. Breakpoints only, no patched code:
//! - **Hold.** While the night match has [`set_armed`] and LOCAL is dead (HP 0, state not 1) an execute breakpoint on the start
//!   step returns at once (RIP = return address, RSP + 8), keeping `(rcx, rdx)`. Nothing after it is queued: LOCAL stays dead.
//! - **Release.** [`request_release`] (the morning) makes a gate breakpoint call the held step on a game thread: the gate's rcx,
//!   rdx, r8, r9 and rsp are saved, RSP - 16 with the gate's own address as the return address, RIP = the held step; when the step
//!   returns into the gate the registers are put back (RF set) and the gate runs on. The game then plays its own comeback. Gate A
//!   is `exe+0x5BCE10` (the Lua event proxy's update, the sequencer's own thread, `[HYPOTHESIS]`); when it has not fired
//!   [`GATE_B_AFTER_MS`] after the request, gate B `exe+0x675480` (a per-frame character update; worked in the solo run 4) is armed
//!   as well. The first to fire makes the call. The thread of the gate and of the last start-step hit are logged.
//!
//! The VEH records into atomics only (no logging, allocation or locks); the main loop logs ([`tick`]). A held step is dropped (never
//! called) when LOCAL goes away (another character address, or none for [`LOCAL_GONE_MS`]) or when the hold is disarmed with no
//! release pending. Dev console `death_hold [0|1]` (`deathtrace`) drives the same module by hand. Log lines start with `M5-9 deathhold`.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, Once, PoisonError};
use std::time::Instant;

use windows::Win32::Foundation::STATUS_SINGLE_STEP;
use windows::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};
use windows::Win32::System::Threading::GetCurrentThreadId;

use super::gamemem::RealMem;
use super::views::ChrIns;
use crate::dr::{EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH, TARGETS};
use crate::hwbp;

const P_START: usize = 0;
const P_GATE_A: usize = 1;
const P_GATE_B: usize = 2;
const POINTS: usize = 3;
/// (RVA, first 6 bytes in exe 1.3.3.0, name). MEMORY_MAP §17 and research/M5-8-death.md §R2 (exe 1.3.3.0 = game 1.03.3). Fixed RVAs
/// with a byte check, as `reviveblock` does: no signature scan yet (a patched build fails the check and stays passive).
const TABLE: [(usize, [u8; 6], &str); POINTS] = [
    // The comeback's first step after the death wait.
    (
        0x5C_4810,
        [0x48, 0x89, 0x5C, 0x24, 0x08, 0x48],
        "respawn sequence start",
    ),
    // CSLuaEventProxy update (the sequencer's own thread; HYPOTHESIS until the first live release logs both threads).
    (
        0x5B_CE10,
        [0x40, 0x55, 0x53, 0x56, 0x57, 0x41],
        "gate A, Lua event proxy update",
    ),
    // Per-frame character update; the release gate of the solo run 4 (thread of the character, not of the steps).
    (
        0x67_5480,
        [0x48, 0x89, 0x5C, 0x24, 0x10, 0x57],
        "gate B, character update",
    ),
];
/// Gate B is armed this long after the release was asked if gate A has not fired (card M5-9 step 3).
pub(crate) const GATE_B_AFTER_MS: u128 = 2_000;
/// A release asked and not done after this long is given up (logged once; the held step is dropped).
pub(crate) const RELEASE_GIVE_UP_MS: u128 = 20_000;
/// LOCAL missing from the main loop this long = an area load or the match end: a held step is dropped.
pub(crate) const LOCAL_GONE_MS: u128 = 3_000;
const STACK_QWORDS: usize = 1;

/// Set by the night match (Solo, nightfall to the morning).
static ARMED: AtomicBool = AtomicBool::new(false);
/// Set by the dev console `death_hold 1` (same effect as armed).
static MANUAL: AtomicBool = AtomicBool::new(false);
/// Breakpoint addresses (0 = not resolved); never reset, so a late hit after a disarm is still recognised.
static ADDRS: [AtomicUsize; POINTS] = [const { AtomicUsize::new(0) }; POINTS];
static START_HITS: AtomicU64 = AtomicU64::new(0);
static START_TID: AtomicU32 = AtomicU32::new(0);
/// The held start step's arguments (valid while `HELD_CTX` != 0).
static HELD_CTX: AtomicU64 = AtomicU64::new(0);
static HELD_SEQ: AtomicU64 = AtomicU64::new(0);
/// How many times a step was held.
static HELD: AtomicU64 = AtomicU64::new(0);
/// Release asked: a gate calls the held step on its next entry.
static PENDING: AtomicBool = AtomicBool::new(false);
/// A gate's call is in flight: its saved registers.
static IN_CALL: AtomicBool = AtomicBool::new(false);
static CALL_TID: AtomicU32 = AtomicU32::new(0);
static CALL_GATE: AtomicUsize = AtomicUsize::new(0);
static CALL_RSP: AtomicU64 = AtomicU64::new(0);
static CALL_REGS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static CALLS_DONE: AtomicU64 = AtomicU64::new(0);
static VEH_ONCE: Once = Once::new();

struct State {
    slots: [Option<usize>; POINTS],
    bad: [bool; POINTS],
    busy_logged: bool,
    pending_since: Option<Instant>,
    seen_held: u64,
    seen_calls: u64,
    held_for: Option<usize>,
    local_gone: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    slots: [None; POINTS],
    bad: [false; POINTS],
    busy_logged: false,
    pending_since: None,
    seen_held: 0,
    seen_calls: 0,
    held_for: None,
    local_gone: None,
});

/// The night match arms or disarms the hold (main loop). Disarming does not cancel a release already asked.
pub(crate) fn set_armed(on: bool) {
    if ARMED.swap(on, Ordering::Relaxed) != on {
        tracing::info!(
            "M5-9 deathhold: hold {}",
            if on { "ARMED" } else { "disarmed" }
        );
    }
}

/// Dev console `death_hold 1|0` (pipe thread): the manual switch; `0` with a held step also asks for the release.
pub(crate) fn set_manual(on: bool) {
    MANUAL.store(on, Ordering::Relaxed);
    tracing::info!(
        "M5-9 deathhold: manual hold {}",
        if on { "ON" } else { "OFF" }
    );
    if !on {
        request_release();
    }
}

/// Ask for the held step to be called (the morning). False when nothing is held (a hold not yet hit is simply disarmed by the
/// caller; the game's own comeback then runs).
pub(crate) fn request_release() -> bool {
    if HELD_CTX.load(Ordering::Relaxed) == 0 {
        return false;
    }
    if !PENDING.swap(true, Ordering::Relaxed) {
        tracing::info!("M5-9 deathhold: release asked; gate A exe+0x{:X} first, gate B exe+0x{:X} after {GATE_B_AFTER_MS} ms", TABLE[P_GATE_A].0, TABLE[P_GATE_B].0);
    }
    true
}

/// True while a step is held (the player is held dead).
pub(crate) fn held() -> bool {
    HELD_CTX.load(Ordering::Relaxed) != 0
}

/// A short phrase for `night_status`: `"death held"`, `"death released"` or `None`.
pub(crate) fn status_short() -> Option<&'static str> {
    if held() {
        Some(if PENDING.load(Ordering::Relaxed) {
            "death held (release pending)"
        } else {
            "death held"
        })
    } else if CALLS_DONE.load(Ordering::Relaxed) > 0 {
        Some("death released")
    } else {
        None
    }
}

/// Console `death_hold` with no argument.
pub(crate) fn status() -> String {
    format!(
        "death_hold {} (night match {}); held {} (ctx 0x{:X}), release pending {}, released {}; start-step hits {} (last thread {})",
        u8::from(MANUAL.load(Ordering::Relaxed)),
        if ARMED.load(Ordering::Relaxed) { "armed" } else { "not armed" },
        HELD.load(Ordering::Relaxed),
        HELD_CTX.load(Ordering::Relaxed),
        u8::from(PENDING.load(Ordering::Relaxed)),
        CALLS_DONE.load(Ordering::Relaxed),
        START_HITS.load(Ordering::Relaxed),
        START_TID.load(Ordering::Relaxed)
    )
}

fn armed() -> bool {
    ARMED.load(Ordering::Relaxed) || MANUAL.load(Ordering::Relaxed)
}

fn ensure_veh() {
    VEH_ONCE.call_once(|| {
        // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature and returns CONTINUE_SEARCH for anything that is not one of
        // our breakpoint addresses.
        if unsafe { AddVectoredExceptionHandler(1, Some(veh)) }.is_null() {
            tracing::warn!("M5-9 deathhold: AddVectoredExceptionHandler failed");
        }
    });
}

unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS; pointers are null-checked. Every address is the first instruction of a
    // function (exe 1.3.3.0, byte-checked before arming), so `[rsp]` is the caller's return address; the stack read and the one
    // stack write are checked against the TEB stack bounds. The hold does what the step's own `ret` does (it returns nothing). The
    // gate call keeps the Windows x64 calling convention: the stack stays 16-byte aligned at the callee's entry (RSP - 16, return
    // address at the new RSP), and the gate's own argument registers and RSP are put back before the gate runs. Nothing here
    // allocates, logs or locks.
    unsafe {
        if info.is_null() {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let rec = (*info).ExceptionRecord;
        let ctx = (*info).ContextRecord;
        if rec.is_null() || ctx.is_null() || (*rec).ExceptionCode != STATUS_SINGLE_STEP {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let rip = (*ctx).Rip as usize;
        let Some(i) = (0..POINTS).find(|&i| {
            let a = ADDRS[i].load(Ordering::Relaxed);
            a != 0 && a == rip
        }) else {
            return EXCEPTION_CONTINUE_SEARCH;
        };
        let tid = GetCurrentThreadId();
        let rsp = (*ctx).Rsp as usize;
        let (lo, hi) = stack_bounds();
        let stack_ok = rsp.is_multiple_of(8)
            && lo != 0
            && rsp >= lo + 0x100
            && rsp.checked_add(STACK_QWORDS * 8).is_some_and(|e| e <= hi);
        if i != P_START {
            // Phase 2: the held step returned into this gate: put the gate's own arguments back.
            if IN_CALL.load(Ordering::Acquire)
                && i == CALL_GATE.load(Ordering::Relaxed)
                && tid == CALL_TID.load(Ordering::Relaxed)
                && rsp as u64 == CALL_RSP.load(Ordering::Relaxed) - 8
            {
                (*ctx).Rcx = CALL_REGS[0].load(Ordering::Relaxed);
                (*ctx).Rdx = CALL_REGS[1].load(Ordering::Relaxed);
                (*ctx).R8 = CALL_REGS[2].load(Ordering::Relaxed);
                (*ctx).R9 = CALL_REGS[3].load(Ordering::Relaxed);
                (*ctx).Rsp = CALL_RSP.load(Ordering::Relaxed);
                IN_CALL.store(false, Ordering::Release);
                CALLS_DONE.fetch_add(1, Ordering::Relaxed);
                (*ctx).EFlags |= EFLAGS_RF;
                return EXCEPTION_CONTINUE_EXECUTION;
            }
            // Phase 1: call the held step on this thread, then come back here.
            let held = HELD_CTX.load(Ordering::Relaxed);
            let start = ADDRS[P_START].load(Ordering::Relaxed);
            if PENDING.load(Ordering::Relaxed)
                && !IN_CALL.load(Ordering::Acquire)
                && held != 0
                && start != 0
                && stack_ok
            {
                PENDING.store(false, Ordering::Relaxed);
                CALL_TID.store(tid, Ordering::Relaxed);
                CALL_GATE.store(i, Ordering::Relaxed);
                CALL_RSP.store(rsp as u64, Ordering::Relaxed);
                CALL_REGS[0].store((*ctx).Rcx, Ordering::Relaxed);
                CALL_REGS[1].store((*ctx).Rdx, Ordering::Relaxed);
                CALL_REGS[2].store((*ctx).R8, Ordering::Relaxed);
                CALL_REGS[3].store((*ctx).R9, Ordering::Relaxed);
                IN_CALL.store(true, Ordering::Release);
                let new_rsp = rsp - 16;
                *(new_rsp as *mut u64) = rip as u64;
                (*ctx).Rsp = new_rsp as u64;
                (*ctx).Rcx = held;
                (*ctx).Rdx = HELD_SEQ.load(Ordering::Relaxed);
                (*ctx).Rip = start as u64;
                HELD_CTX.store(0, Ordering::Relaxed);
                return EXCEPTION_CONTINUE_EXECUTION;
            }
            (*ctx).EFlags |= EFLAGS_RF;
            return EXCEPTION_CONTINUE_EXECUTION;
        }
        START_HITS.fetch_add(1, Ordering::Relaxed);
        START_TID.store(tid, Ordering::Relaxed);
        if armed() && !IN_CALL.load(Ordering::Acquire) && stack_ok {
            if HELD_CTX.load(Ordering::Relaxed) == 0 {
                HELD_SEQ.store((*ctx).Rdx, Ordering::Relaxed);
                HELD_CTX.store((*ctx).Rcx, Ordering::Relaxed);
                HELD.fetch_add(1, Ordering::Relaxed);
            }
            (*ctx).Rip = *(rsp as *const u64);
            (*ctx).Rsp = (rsp + 8) as u64;
            return EXCEPTION_CONTINUE_EXECUTION;
        }
        (*ctx).EFlags |= EFLAGS_RF;
    }
    EXCEPTION_CONTINUE_EXECUTION
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

/// LOCAL is dead: HP 0 and not in the downed state (MEMORY_MAP §5, §6).
pub(crate) fn is_dead(hp: Option<u32>, state: Option<u8>) -> bool {
    hp == Some(0) && state != Some(1)
}

fn free_slot(taken: &[Option<usize>; POINTS]) -> Option<usize> {
    (0..TARGETS).find(|s| hwbp::is_free(*s) && !taken.contains(&Some(*s)))
}

/// Which breakpoints are wanted. Pure (tested): `dead` LOCAL is dead, `armed` the hold is on, `pending` a release is asked and has
/// waited `waited_ms`, `in_call` a gate's call is in flight through that gate (index).
pub(crate) fn wanted(
    dead: bool,
    armed: bool,
    pending: bool,
    waited_ms: u128,
    in_call: Option<usize>,
) -> [bool; POINTS] {
    // The start breakpoint is not needed once a release is asked (the held step is already captured; a call through a gate runs
    // the step without it). Freeing its slot keeps the peak at one slot for the hold and one for the gate: with puppets
    // (dev: three slots) or the menu breakpoints in use there is no second free slot (2026-10-24, scenario night1-local-dies).
    [
        dead && armed && !pending && in_call.is_none(),
        // Never free the gate whose call is in flight: the return into it must find the breakpoint.
        pending || in_call == Some(P_GATE_A),
        (pending && waited_ms >= GATE_B_AFTER_MS) || in_call == Some(P_GATE_B),
    ]
}

/// Main loop, every tick (both builds): arm and disarm the breakpoints, drop a stale held step, log what the VEH did.
pub(crate) fn tick(local: Option<usize>, base: usize) {
    let mem = RealMem;
    let mut st = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    let held_now = HELD_CTX.load(Ordering::Relaxed) != 0;
    let pending = PENDING.load(Ordering::Relaxed);
    let in_call = IN_CALL
        .load(Ordering::Acquire)
        .then(|| CALL_GATE.load(Ordering::Relaxed));
    let dead = local.is_some_and(|c| {
        let chr = ChrIns::new(&mem, c);
        is_dead(chr.hp(), chr.downed_state())
    });
    // A new hold belongs to the LOCAL of this tick (a stale `held_for` of an earlier hold would drop it at once: Nucleus run 2026-10-25,
    // "LOCAL is another character now" right after the hold).
    if HELD.load(Ordering::Relaxed) != st.seen_held {
        st.held_for = local;
    }
    // A held step belongs to one character in one area: drop it when LOCAL changes or is gone for a while.
    match local {
        Some(c) => {
            st.local_gone = None;
            if held_now && st.held_for.is_some_and(|h| h != c) && in_call.is_none() {
                drop_held("LOCAL is another character now");
            }
        }
        None => {
            let since = *st.local_gone.get_or_insert_with(Instant::now);
            if held_now && in_call.is_none() && since.elapsed().as_millis() >= LOCAL_GONE_MS {
                drop_held("LOCAL is gone (area load or match end)");
            }
        }
    }
    if held_now && !armed() && !pending && in_call.is_none() {
        drop_held("the hold was disarmed with no release asked");
    }
    // The release gives up.
    if pending {
        let since = *st.pending_since.get_or_insert_with(Instant::now);
        if since.elapsed().as_millis() >= RELEASE_GIVE_UP_MS {
            PENDING.store(false, Ordering::Relaxed);
            drop_held("no gate fired in time");
            tracing::warn!(
                "M5-9 deathhold: the release did not happen; LOCAL stays dead until the area ends"
            );
        }
    } else {
        st.pending_since = None;
    }
    let pending = PENDING.load(Ordering::Relaxed);
    let waited = st.pending_since.map_or(0, |s| s.elapsed().as_millis());
    let want = wanted(dead, armed(), pending, waited, in_call);
    for i in 0..POINTS {
        match (want[i], st.slots[i]) {
            (true, None) if !st.bad[i] => {
                let (rva, bytes, name) = TABLE[i];
                if super::mem::read::<[u8; 6]>(base + rva) != Some(bytes) {
                    st.bad[i] = true;
                    tracing::warn!(
                        "M5-9 deathhold: {name} (exe+0x{rva:X}) bytes differ from exe 1.3.3.0; not armed"
                    );
                    continue;
                }
                let Some(s) = free_slot(&st.slots) else {
                    if !st.busy_logged {
                        st.busy_logged = true;
                        tracing::info!("M5-9 deathhold: no free hardware slot for {name}");
                    }
                    continue;
                };
                ensure_veh();
                ADDRS[i].store(base + rva, Ordering::Relaxed);
                hwbp::set(s, base + rva);
                st.slots[i] = Some(s);
                st.busy_logged = false;
                tracing::info!("M5-9 deathhold: armed slot {s} on exe+0x{rva:X} ({name})");
            }
            (false, Some(s)) => {
                hwbp::clear(s);
                st.slots[i] = None;
                tracing::info!(
                    "M5-9 deathhold: disarmed slot {s} (exe+0x{:X}, {})",
                    TABLE[i].0,
                    TABLE[i].2
                );
            }
            _ => {}
        }
    }
    let held_count = HELD.load(Ordering::Relaxed);
    if held_count != st.seen_held {
        st.seen_held = held_count;
        st.held_for = local;
        tracing::info!(
            "M5-9 deathhold: start step HELD (ctx 0x{:X}, seq 0x{:X}, thread {}): LOCAL stays dead until the release",
            HELD_CTX.load(Ordering::Relaxed),
            HELD_SEQ.load(Ordering::Relaxed),
            START_TID.load(Ordering::Relaxed)
        );
    }
    let calls = CALLS_DONE.load(Ordering::Relaxed);
    if calls != st.seen_calls {
        st.seen_calls = calls;
        let g = CALL_GATE.load(Ordering::Relaxed).min(POINTS - 1);
        tracing::info!(
            "M5-9 deathhold: RELEASED: the held start step ran through gate exe+0x{:X} ({}) on thread {} (last start-step hit on thread {}); the game's comeback follows",
            TABLE[g].0,
            TABLE[g].2,
            CALL_TID.load(Ordering::Relaxed),
            START_TID.load(Ordering::Relaxed)
        );
    }
}

/// Forget the held step without calling it.
fn drop_held(why: &str) {
    if HELD_CTX.swap(0, Ordering::Relaxed) != 0 {
        PENDING.store(false, Ordering::Relaxed);
        tracing::info!("M5-9 deathhold: held step dropped ({why}); it is never called");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_means_hp_zero_and_not_downed() {
        assert!(is_dead(Some(0), Some(0)));
        assert!(is_dead(Some(0), Some(2)));
        assert!(!is_dead(Some(0), Some(1)), "downed is not dead");
        assert!(!is_dead(Some(5), Some(0)));
        assert!(!is_dead(None, Some(0)));
    }

    #[test]
    fn the_start_step_is_armed_only_while_armed_and_dead() {
        assert_eq!(
            wanted(true, true, false, 0, None),
            [true, false, false],
            "dead and armed: only the start step"
        );
        assert_eq!(wanted(false, true, false, 0, None), [false; 3]);
        assert_eq!(wanted(true, false, false, 0, None), [false; 3]);
    }

    #[test]
    fn a_release_arms_gate_a_then_gate_b_after_two_seconds() {
        assert_eq!(wanted(false, false, true, 0, None), [false, true, false]);
        assert_eq!(
            wanted(false, false, true, GATE_B_AFTER_MS - 1, None),
            [false, true, false]
        );
        assert_eq!(
            wanted(false, false, true, GATE_B_AFTER_MS, None),
            [false, true, true]
        );
        // Still dead and armed with the release pending: the start slot is freed for the gate.
        assert_eq!(wanted(true, true, true, 0, None), [false, true, false]);
    }

    #[test]
    fn the_gate_in_flight_is_never_freed() {
        // Gate B made the call: it stays until the return; gate A may go.
        let w = wanted(false, false, false, 3_000, Some(P_GATE_B));
        assert!(w[P_GATE_B] && !w[P_GATE_A]);
        // Gate A made the call: it stays.
        let w = wanted(false, false, false, 0, Some(P_GATE_A));
        assert!(w[P_GATE_A] && !w[P_GATE_B]);
        // No start breakpoint during the call: one slot only.
        assert!(!w[P_START]);
    }

    #[test]
    fn the_table_names_three_distinct_points() {
        for (i, a) in TABLE.iter().enumerate() {
            assert!(TABLE.iter().skip(i + 1).all(|b| b.0 != a.0));
        }
        assert_eq!(TABLE[P_START].0, 0x5C_4810);
        assert_eq!(TABLE[P_GATE_A].0, 0x5B_CE10);
        assert_eq!(TABLE[P_GATE_B].0, 0x67_5480);
    }

    #[test]
    fn nothing_held_means_nothing_to_release() {
        assert!(!request_release());
        assert!(status().contains("death_hold"));
    }
}
