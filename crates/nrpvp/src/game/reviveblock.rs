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
//!
//! M5-6: [`set_mode`] widens or lifts the block: [`ReviveMode::Nobody`] (LOCAL is eliminated at night: no reviver at all, teammates
//! included, neither on the bar nor on the revive event) and [`ReviveMode::Anyone`] (our own morning get-up writes the revive event
//! itself; the last attacker may be a rival, so the rival block must not drop it).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
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
/// `movss dword ptr [rdi+0x714], xmm1` in `exe+0x66DEC0` (a reviver's hit on the downed player's bar; rdi = victim, rsi = reviver).
pub(crate) const RVA_HIT_STORE: usize = 0x66DFC4;
const HIT_STORE_BYTES: [u8; 8] = [0xF3, 0x0F, 0x11, 0x8F, 0x14, 0x07, 0x00, 0x00];
const MAX_RIVALS: usize = 8;

static ENABLED: AtomicBool = AtomicBool::new(true);
/// [`ReviveMode`] as a byte (the VEH reads it).
static MODE: AtomicU8 = AtomicU8::new(0);

/// M5-6: who may revive LOCAL. Only `Rivals` is used since the match rules changed (RW-1); the other modes stay for a spectator rule.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ReviveMode {
    /// T-041: everyone but a rival (the normal rule).
    #[default]
    Rivals,
    /// Nobody (LOCAL eliminated at night).
    Nobody,
    /// Everyone (our own get-up at the morning).
    Anyone,
}

/// M5-6 (main loop): set who may revive LOCAL; logs a change.
#[allow(dead_code)]
pub(crate) fn set_mode(m: ReviveMode) {
    let v = match m {
        ReviveMode::Rivals => 0,
        ReviveMode::Nobody => 1,
        ReviveMode::Anyone => 2,
    };
    if MODE.swap(v, Ordering::Relaxed) != v {
        tracing::info!("T-041 revive_block: mode {m:?}");
    }
}
/// The breakpoint address (0 = not resolved); never reset, so a late hit after a disarm is still ours.
static ADDR: AtomicUsize = AtomicUsize::new(0);
/// `exe+RVA_NO_EVENT` (0 = not resolved: nothing is blocked).
static SKIP: AtomicUsize = AtomicUsize::new(0);
static LOCAL: AtomicUsize = AtomicUsize::new(0);
static RIVALS: [AtomicU64; MAX_RIVALS] = [const { AtomicU64::new(0) }; MAX_RIVALS];
static BLOCKS: AtomicU32 = AtomicU32::new(0);
static ALLOWED: AtomicU32 = AtomicU32::new(0);
static LAST_HANDLE: AtomicU64 = AtomicU64::new(0);
/// `exe+RVA_HIT_STORE` (0 = not resolved); never reset.
static HIT_ADDR: AtomicUsize = AtomicUsize::new(0);
/// Rival hits whose bar decrease was cancelled (the main loop applies the T-039 clock penalty for each).
static RIVAL_HITS: AtomicU32 = AtomicU32::new(0);
static VEH_ONCE: Once = Once::new();

struct State {
    slot: Option<usize>,
    hit_slot: Option<usize>,
    seen_hits: u32,
    /// Ticks left in which the game's pause write after a cancelled rival hit is cleared again (it lands after our trap).
    clear_pause_ticks: u8,
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
    hit_slot: None,
    seen_hits: 0,
    clear_pause_ticks: 0,
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
        let rip = (*ctx).Rip as usize;
        let hit_addr = HIT_ADDR.load(Ordering::Relaxed);
        if hit_addr != 0 && rip == hit_addr {
            // A reviver's hit is about to store the new bar (xmm1) into `[rdi+0x714]`. For LOCAL and a RIVAL reviver (`[rsi+8]` is
            // its handle, the same field `exe+0x66E01B` copies into `+0x728`), store the current bar instead: the bar never drops,
            // so the player script never sees it at 0 and never starts the revival animation (handoff T-041). Only xmm1's low
            // 32 bits change; the store itself then runs as normal.
            let victim = (*ctx).Rdi as usize;
            let reviver = (*ctx).Rsi as usize;
            if victim != 0 && victim == LOCAL.load(Ordering::Relaxed) && reviver > 0x10000 {
                let h = *((reviver + super::layout::CHR_HANDLE) as *const u64);
                let rival = h != 0
                    && h != u64::MAX
                    && RIVALS.iter().any(|r| r.load(Ordering::Relaxed) == h);
                let mode = MODE.load(Ordering::Relaxed);
                // M5-6: an eliminated LOCAL (mode 1) keeps its bar against everyone; the rival penalty stays a mode-0 rule.
                if (mode == 0 && rival) || mode == 1 {
                    let bar = *((victim + super::layout::PLAYER_REVIVE_BAR) as *const u32);
                    let x = &mut (*ctx).Anonymous.Anonymous.Xmm1;
                    x.Low = (x.Low & 0xFFFF_FFFF_0000_0000) | u64::from(bar);
                    LAST_HANDLE.store(h, Ordering::Relaxed);
                    if mode == 0 {
                        RIVAL_HITS.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            (*ctx).EFlags |= EFLAGS_RF;
            return EXCEPTION_CONTINUE_EXECUTION;
        }
        let addr = ADDR.load(Ordering::Relaxed);
        if addr == 0 || rip != addr {
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
                let block = match MODE.load(Ordering::Relaxed) {
                    1 => true,
                    2 => false,
                    _ => rival(who),
                };
                if block && skip != 0 {
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
pub(crate) fn tick(combat: &mut CombatCtl, base: usize) {
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
        if st.hit_slot.is_none() && !st.bytes_bad {
            let target = base + RVA_HIT_STORE;
            if super::mem::read::<[u8; 8]>(target) != Some(HIT_STORE_BYTES) {
                st.bytes_bad = true;
                tracing::warn!("T-041 hit hook: code bytes at exe+0x{RVA_HIT_STORE:X} differ from game 1.03.3; not armed");
            } else if let Some(s) = free_slot() {
                ensure_veh();
                HIT_ADDR.store(target, Ordering::Relaxed);
                hwbp::set(s, target);
                st.hit_slot = Some(s);
                tracing::info!("T-041 hit hook: armed on exe+0x{RVA_HIT_STORE:X} (slot {s}): a rival's hit no longer lowers LOCAL's bar");
            }
        }
    } else if let Some(s) = st.hit_slot.take() {
        hwbp::clear(s);
        tracing::info!("T-041 hit hook: disarmed (slot {s})");
    }
    // The T-039 penalty for each cancelled rival hit (the poll rule sees no bar drop any more).
    let hits = RIVAL_HITS.load(Ordering::Relaxed);
    if hits != st.seen_hits {
        let n = hits.wrapping_sub(st.seen_hits);
        st.seen_hits = hits;
        combat.penalize_rival_hits(n);
        st.clear_pause_ticks = 4;
    } else if st.clear_pause_ticks > 0 {
        st.clear_pause_ticks -= 1;
        combat.clear_pause();
    }
    if !want {
        if let Some(s) = st.slot.take() {
            hwbp::clear(s);
            tracing::info!(
            "T-041 revive_block: disarmed (slot {s}); this down: {} rival revive frame(s) dropped, {} other revive(s) allowed",
            BLOCKS.load(Ordering::Relaxed).wrapping_sub(st.base_blocks),
            ALLOWED.load(Ordering::Relaxed).wrapping_sub(st.base_allowed)
        );
        }
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
            "T-041 revive_block: BLOCKED a revive of LOCAL (rival, or anyone while eliminated; reviver handle 0x{:X}); {b} block(s) so far",
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
