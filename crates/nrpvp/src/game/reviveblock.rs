//! T-041: stop a rival's revive before the game starts the get-up. Always on in both builds; the dev console `revive_block [0|1]`
//! can switch it off for a comparison run.
//!
//! What the T-041 trace and the disassembly showed (game 1.03.3 / exe 1.3.3.0, MEMORY_MAP section on the downed player):
//! - `exe+0x674EB0` runs every frame for a downed player (state byte `+0x710` = 1) on the owner's game. At `exe+0x675159` it reads
//!   the byte `PlayerIns+0x732` ("revive completed"); when it is not 0 it calls `exe+0x66F3B0(chr, 2)`, which starts the get-up
//!   (state 2). The revive bar (`+0x714`) is not what decides: a get-up started with the bar at 40 (T-041 trace, 22:42:36).
//! - `exe+0x66DEC0` applies a reviver's hit to the bar and, when the bar reaches 0, stores the reviver's handle in `+0x728`.
//!
//! Here: an execute breakpoint on `exe+0x675159` (the `cmp byte [rbx+0x732], 0`). In the VEH, if `rbx` is LOCAL, the flag is set and
//! the reviver (`+0x728`, else the last attacker `+0x1A0`) is a RIVAL, RIP goes to the game's own no-event exit
//! (`exe+0x67539F`, which clears the flags): the revive event is dropped and LOCAL stays downed.
//! Teammates are untouched. Armed only while LOCAL is downed, in a free hardware slot. No code is patched.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, Once, PoisonError};

use windows::Win32::Foundation::STATUS_SINGLE_STEP;
use windows::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};

use super::combat_state::CombatCtl;
use crate::dr::{EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH, TARGETS};
use crate::hwbp;

/// `cmp byte ptr [rbx+0x732], 0` in the per-frame downed function `exe+0x674EB0` (exe 1.3.3.0).
pub(crate) const RVA_REVIVE_CHECK: usize = 0x675159;
const REVIVE_CHECK_BYTES: [u8; 7] = [0x80, 0xBB, 0x32, 0x07, 0x00, 0x00, 0x00];
/// `mov word ptr [rbx+0x731], 0`: where the per-frame function goes when there is no revive event (`je` at `exe+0x675153`).
pub(crate) const RVA_NO_EVENT: usize = 0x67539F;
const NO_EVENT_BYTES: [u8; 9] = [0x66, 0xC7, 0x83, 0x31, 0x07, 0x00, 0x00, 0x00, 0x00];
/// u8 "revive completed" flag read by the check above.
pub(crate) const PLAYER_REVIVE_DONE: usize = 0x732;
/// u64 handle of the player whose hit emptied the bar (`exe+0x66E028`); all-ones when none.
pub(crate) const PLAYER_REVIVER: usize = 0x728;
const MAX_RIVALS: usize = 8;

static ENABLED: AtomicBool = AtomicBool::new(true);
/// The breakpoint address (0 = not resolved); never reset, so a late hit after a disarm is still ours.
static ADDR: AtomicUsize = AtomicUsize::new(0);
/// `exe+RVA_NO_EVENT` (0 = not resolved: nothing is blocked).
static SKIP: AtomicUsize = AtomicUsize::new(0);
static LOCAL: AtomicUsize = AtomicUsize::new(0);
static RIVALS: [AtomicU64; MAX_RIVALS] = [const { AtomicU64::new(0) }; MAX_RIVALS];
static BLOCKS: AtomicU32 = AtomicU32::new(0);
static ALLOWED: AtomicU32 = AtomicU32::new(0);
static LAST_HANDLE: AtomicU64 = AtomicU64::new(0);
static VEH_ONCE: Once = Once::new();

struct State {
    slot: Option<usize>,
    seen_blocks: u32,
    seen_allowed: u32,
    /// BLOCKS / ALLOWED when the slot was armed: one line per kind per down, the totals at the disarm.
    base_blocks: u32,
    base_allowed: u32,
    busy_logged: bool,
    bytes_bad: bool,
}
static STATE: Mutex<State> = Mutex::new(State {
    slot: None,
    seen_blocks: 0,
    seen_allowed: 0,
    base_blocks: 0,
    base_allowed: 0,
    busy_logged: false,
    bytes_bad: false,
});

pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

#[cfg(feature = "dev")]
pub(crate) fn blocks() -> u32 {
    BLOCKS.load(Ordering::Relaxed)
}

/// The VEH is registered once, right before the first arm.
fn ensure_veh() {
    VEH_ONCE.call_once(|| {
        // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature and returns CONTINUE_SEARCH for anything that is not our
        // breakpoint address.
        if unsafe { AddVectoredExceptionHandler(1, Some(veh)) }.is_null() {
            tracing::warn!("T-041 revive_block: AddVectoredExceptionHandler failed");
        }
    });
}

/// Dev console `revive_block 0|1` (on by default).
#[cfg(feature = "dev")]
pub(crate) fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    tracing::info!("T-041 revive_block: {}", if on { "ON" } else { "OFF" });
}

unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS; pointers are null-checked. `rbx` at this instruction is the PlayerIns the
    // game itself is about to read `+0x732` from, and it is only touched when it equals LOCAL (published by the main loop). The
    // only change is RIP, to the game's own no-event exit of the same function (same frame and registers). Nothing here allocates, logs or locks.
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
        if addr == 0 || (*ctx).Rip as usize != addr {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let chr = (*ctx).Rbx as usize;
        if chr != 0 && chr == LOCAL.load(Ordering::Relaxed) {
            let flag = (chr + PLAYER_REVIVE_DONE) as *const u8;
            if *flag != 0 {
                let reviver = *((chr + PLAYER_REVIVER) as *const u64);
                let last = *((chr + super::layout::PLAYER_LAST_ATTACKER) as *const u64);
                let rival = |h: u64| {
                    h != 0 && h != u64::MAX && RIVALS.iter().any(|r| r.load(Ordering::Relaxed) == h)
                };
                let who = if reviver != u64::MAX && reviver != 0 {
                    reviver
                } else {
                    last
                };
                let skip = SKIP.load(Ordering::Relaxed);
                if rival(who) && skip != 0 {
                    // Leave the event block the way the game does when there is no event (`je 0x67539F` at `exe+0x675153`, same
                    // registers): the flags are cleared there and LOCAL stays downed. Clearing only `+0x732` sent the game into the
                    // "neither flag" branch, which stands the player up at once (T-041 Nucleus run, 23:01:10).
                    (*ctx).Rip = skip as u64;
                    LAST_HANDLE.store(who, Ordering::Relaxed);
                    BLOCKS.fetch_add(1, Ordering::Relaxed);
                } else {
                    LAST_HANDLE.store(who, Ordering::Relaxed);
                    ALLOWED.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        (*ctx).EFlags |= EFLAGS_RF;
    }
    EXCEPTION_CONTINUE_EXECUTION
}

fn free_slot() -> Option<usize> {
    (0..TARGETS).find(|&s| hwbp::is_free(s))
}

/// Main loop, right after the combat tick: publish LOCAL and the rival handles, arm while LOCAL is downed, log what the VEH did.
pub(crate) fn tick(combat: &CombatCtl, base: usize) {
    let mut st = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    let local = combat.local_state();
    let want = enabled() && local.is_some_and(|(_, s)| s == 1);
    if want {
        let (addr, _) = local.unwrap_or_default();
        LOCAL.store(addr, Ordering::Relaxed);
        let handles = combat.rival_handles();
        for (i, r) in RIVALS.iter().enumerate() {
            r.store(handles.get(i).copied().unwrap_or(0), Ordering::Relaxed);
        }
        if st.slot.is_none() && !st.bytes_bad {
            let target = base + RVA_REVIVE_CHECK;
            if super::mem::read::<[u8; 7]>(target) != Some(REVIVE_CHECK_BYTES)
                || super::mem::read::<[u8; 9]>(base + RVA_NO_EVENT) != Some(NO_EVENT_BYTES)
            {
                st.bytes_bad = true;
                tracing::warn!("T-041 revive_block: code bytes at exe+0x{RVA_REVIVE_CHECK:X} / exe+0x{RVA_NO_EVENT:X} differ from game 1.03.3; not armed");
            } else if let Some(s) = free_slot() {
                ensure_veh();
                SKIP.store(base + RVA_NO_EVENT, Ordering::Relaxed);
                ADDR.store(target, Ordering::Relaxed);
                hwbp::set(s, target);
                st.slot = Some(s);
                st.base_blocks = BLOCKS.load(Ordering::Relaxed);
                st.base_allowed = ALLOWED.load(Ordering::Relaxed);
                st.busy_logged = false;
                tracing::info!(
                    "T-041 revive_block: armed on exe+0x{RVA_REVIVE_CHECK:X} (slot {s}); LOCAL 0x{addr:X}, {} rival handle(s)",
                    handles.len()
                );
            } else if !st.busy_logged {
                st.busy_logged = true;
                tracing::info!("T-041 revive_block: no free hardware slot, not armed");
            }
        }
    } else if let Some(s) = st.slot.take() {
        hwbp::clear(s);
        tracing::info!(
            "T-041 revive_block: disarmed (slot {s}); this down: {} rival revive frame(s) dropped, {} other revive(s) allowed",
            BLOCKS.load(Ordering::Relaxed).wrapping_sub(st.base_blocks),
            ALLOWED.load(Ordering::Relaxed).wrapping_sub(st.base_allowed)
        );
    }
    let (b, a) = (
        BLOCKS.load(Ordering::Relaxed),
        ALLOWED.load(Ordering::Relaxed),
    );
    // One line for the first block / allow of a down (the event repeats every frame for the length of a get-up).
    if b != st.seen_blocks {
        let first = st.seen_blocks == st.base_blocks;
        st.seen_blocks = b;
        if first {
            tracing::info!(
            "T-041 revive_block: BLOCKED a rival's revive of LOCAL (reviver handle 0x{:X}); {b} block(s) so far",
            LAST_HANDLE.load(Ordering::Relaxed)
        );
        }
    }
    if a != st.seen_allowed {
        let first = st.seen_allowed == st.base_allowed;
        st.seen_allowed = a;
        if first {
            tracing::info!(
            "T-041 revive_block: allowed a revive of LOCAL by a non-rival (handle 0x{:X}); {a} so far",
            LAST_HANDLE.load(Ordering::Relaxed)
        );
        }
    }
}
