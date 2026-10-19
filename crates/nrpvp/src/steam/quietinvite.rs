//! MM-3f: skip the Steam overlay invite window that the game opens after Matchmaking Settings > Invite Members, but only while this
//! player is the host of a ready match that has no Seamless Co-op lobby yet (`mm_quiet_invite`, developer mode).
//!
//! The game's own session code makes the Seamless Co-op (SC) lobby (S9 5d, 5f); the mod needs that lobby but not the Steam window
//! the game opens right after it (`ISteamFriends::ActivateGameOverlayInviteDialog(lobby)`, 300 ms after `LobbyCreated_t`).
//!
//! No code byte of the game, `steam_api64.dll`, `steamclient64.dll` or `nrsc.dll` is written. Mechanism (same family as `lobtrace`):
//! hardware EXECUTE breakpoints (DR0, DR1) on every thread, caught by a vectored exception handler (VEH).
//! - DR0: the flat export `SteamAPI_ISteamFriends_ActivateGameOverlayInviteDialog` (`steam_api64.dll`).
//! - DR1: the code behind its vtable slot (`steamclient64.dll`, decoded at run time from the flat export's `mov rax,[rcx]; jmp [rax+disp]`
//!   thunk and read from the live `ISteamFriends` object). The MM-3c trace shows the game calls THE VTABLE TARGET, not the flat
//!   export (`HIT 1/3 vtable ISteamFriends::ActivateGameOverlayInviteDialog`, DR2 never hit; S9 5d), so DR1 is the one that matters;
//!   DR0 covers a caller that uses the flat API.
//! - In the VEH, when the instruction pointer is one of those two addresses, `[RSP]` is the return address of the call. Only when it
//!   lies inside `nightreign.exe`'s image: `Rip = [Rsp]`, `Rsp += 8` (the function returns `void`, so nothing else is needed), RF set,
//!   resume. Any other caller (Seamless Co-op, Steam itself, the overlay) runs the call unchanged.
//!
//! The handler is allocation-free and lock-free: atomics only, no logging. The worker thread below logs from the counters.
//! Armed only while [`set_wanted`]`(true)`; otherwise every debug register is cleared on every thread. `mm_quiet_invite` and
//! `mm_lobby_trace` both own the debug registers of every thread, so the config turns the former off when the latter is on.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::STATUS_SINGLE_STEP;
use windows::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};
use windows::Win32::System::Threading::GetCurrentThreadId;

use super::lobtrace::{apply_all, EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH, TARGETS};
use super::Flat;

const FLAT_NAME: &str = "SteamAPI_ISteamFriends_ActivateGameOverlayInviteDialog\0";
const TICK: Duration = Duration::from_millis(100);
/// While armed, new threads are armed at least this often.
const APPLY_EVERY: Duration = Duration::from_secs(2);
const WAIT_TOTAL: Duration = Duration::from_secs(300);
const WAIT_STEP: Duration = Duration::from_millis(500);
/// The window stays suppressed this long after the SC lobby appeared: the game opens it about 300 ms after `LobbyCreated_t`
/// (S9 5d) and the worker only notices the lobby on its next 500 ms refresh.
pub(crate) const LINGER: Duration = Duration::from_secs(5);
/// Length of the dev button's test window.
pub(crate) const TEST_WINDOW: Duration = Duration::from_secs(60);

// Shared with the VEH: plain atomics, nothing that allocates or locks.
static ADDR: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
static EXE_BASE: AtomicUsize = AtomicUsize::new(0);
static EXE_SIZE: AtomicUsize = AtomicUsize::new(0);
/// The handler skips calls only while this is set.
static ARMED: AtomicBool = AtomicBool::new(false);
/// What the worker wants (set by [`set_wanted`], applied by the arming thread).
static WANT: AtomicBool = AtomicBool::new(false);
/// The VEH is installed and at least one address is armed-able.
static READY: AtomicBool = AtomicBool::new(false);
static SUPPRESSED: AtomicU32 = AtomicU32::new(0);
static PASSED: AtomicU32 = AtomicU32::new(0);
static LAST_LOBBY: AtomicU64 = AtomicU64::new(0);
static LAST_RET: AtomicU64 = AtomicU64::new(0);
static LAST_PASS_RET: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------------------------------------------------------
// Pure parts (unit tested)
// ---------------------------------------------------------------------------------------------------------------------------

/// Whether `ret` lies inside the image `[base, base + size)`. An unknown image (base 0 or size 0) matches nothing.
pub(crate) fn return_in_image(ret: u64, base: usize, size: usize) -> bool {
    base != 0 && size != 0 && ret >= base as u64 && ret < (base as u64).saturating_add(size as u64)
}

/// What the handler does with a hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Return to the caller without running the function.
    Skip,
    /// Let the call run.
    Run,
}

/// The return-address filter: only a caller inside `nightreign.exe` is skipped.
pub(crate) fn decide(ret: u64, base: usize, size: usize) -> Action {
    if return_in_image(ret, base, size) {
        Action::Skip
    } else {
        Action::Run
    }
}

/// Whether the suppression is wanted right now: the dev test window is open, or we are the host of a ready match that has no
/// Seamless Co-op lobby yet (`host_waiting_for_lobby`, see `Machine::quiet_invite_wanted`).
pub(crate) fn arm_wanted(host_waiting_for_lobby: bool, test_active: bool) -> bool {
    test_active || host_waiting_for_lobby
}

/// `raw` with a linger: stays true for `linger` after `raw` was last true (`since_raw` = time since then).
pub(crate) fn keep_armed(raw: bool, since_raw: Option<Duration>, linger: Duration) -> bool {
    raw || since_raw.is_some_and(|d| d < linger)
}

/// What a skipped hit does to the context: `(new Rip, new Rsp)` for a function entered with the return address at `rsp`.
pub(crate) fn skip_registers(ret: u64, rsp: u64) -> (u64, u64) {
    (ret, rsp.wrapping_add(8))
}

// ---------------------------------------------------------------------------------------------------------------------------
// The vectored exception handler
// ---------------------------------------------------------------------------------------------------------------------------

/// Runs on the faulting thread with an arbitrary lock state: no allocation, no locks, no logging.
unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS; every pointer is null-checked before use. The one memory read of game
    // state is `[Rsp]` at the entry of an armed function: at a function's first instruction Rsp points at the return address on
    // the thread's own stack (x64 calling convention; TECHNICAL_NOTES has no entry for this, evidence is the MM-3c trace in S9 5d).
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
        if rip == 0 || (ADDR[0].load(Ordering::Relaxed) != rip && ADDR[1].load(Ordering::Relaxed) != rip) {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let rsp = (*ctx).Rsp;
        if ARMED.load(Ordering::Relaxed) && rsp != 0 && rsp.is_multiple_of(8) {
            let ret = *(rsp as *const u64);
            match decide(ret, EXE_BASE.load(Ordering::Relaxed), EXE_SIZE.load(Ordering::Relaxed)) {
                Action::Skip => {
                    LAST_LOBBY.store((*ctx).Rdx, Ordering::Relaxed);
                    LAST_RET.store(ret, Ordering::Relaxed);
                    SUPPRESSED.fetch_add(1, Ordering::Relaxed);
                    let (nrip, nrsp) = skip_registers(ret, rsp);
                    (*ctx).Rip = nrip;
                    (*ctx).Rsp = nrsp;
                }
                Action::Run => {
                    LAST_PASS_RET.store(ret, Ordering::Relaxed);
                    PASSED.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        // RF suppresses the instruction breakpoint for exactly one instruction (Intel SDM vol. 3, 17.3.1.1); harmless after a skip.
        (*ctx).EFlags |= EFLAGS_RF;
    }
    EXCEPTION_CONTINUE_EXECUTION
}

// ---------------------------------------------------------------------------------------------------------------------------
// Control surface for the worker
// ---------------------------------------------------------------------------------------------------------------------------

/// Worker: arm (`true`) or disarm (`false`) the suppression. Cheap; the arming thread applies it within ~100 ms.
pub(crate) fn set_wanted(v: bool) {
    WANT.store(v, Ordering::Relaxed);
}

/// The hook is installed (the dev button and the host line only make sense then).
pub(crate) fn ready() -> bool {
    READY.load(Ordering::Relaxed)
}

/// Debug registers are currently set (the VEH skips matching calls).
pub(crate) fn armed() -> bool {
    ARMED.load(Ordering::Relaxed)
}

/// How many invite windows were skipped so far.
pub(crate) fn suppressed() -> u32 {
    SUPPRESSED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------------------------------------------------------
// The arming thread
// ---------------------------------------------------------------------------------------------------------------------------

/// Start the arming thread. Call only in developer mode with `mm_quiet_invite = true`. `base` / `size`: the image of `nightreign.exe`.
pub(crate) fn spawn(base: usize, size: usize) {
    EXE_BASE.store(base, Ordering::Relaxed);
    EXE_SIZE.store(size, Ordering::Relaxed);
    let r = std::thread::Builder::new().name("nrpvp-quietinvite".into()).spawn(|| {
        if std::panic::catch_unwind(run).is_err() {
            tracing::error!("mm_quiet_invite: arming thread panicked; the invite window is no longer suppressed");
            ARMED.store(false, Ordering::Relaxed);
            READY.store(false, Ordering::Relaxed);
        }
    });
    if r.is_err() {
        tracing::warn!("mm_quiet_invite: could not spawn the arming thread");
    }
}

/// The code behind the flat export's vtable slot, read from the live `ISteamFriends` object (`iface`).
fn vtable_target(flat: usize, iface: usize) -> Option<usize> {
    let bytes = crate::game::mem::read::<[u8; 24]>(flat)?;
    let disp = super::lobtrace::decode_vtable_disp(&bytes)?;
    if iface == 0 {
        return None;
    }
    // [iface] = vtable pointer, [vtable + disp] = the function (S9 5d: slot 33, target in steamclient64.dll).
    let t = crate::game::mem::read::<u64>(iface).and_then(|vt| crate::game::mem::read::<u64>(vt as usize + disp as usize))?;
    (t != 0 && t as usize != flat).then_some(t as usize)
}

fn run() {
    let start = Instant::now();
    let (flat, _resolved) = Flat::resolve();
    loop {
        if super::steam_dll_loaded() && flat.hsteam_user() != 0 {
            break;
        }
        if start.elapsed() >= WAIT_TOTAL {
            tracing::warn!("mm_quiet_invite: gave up after {} s waiting for Steam; nothing installed", WAIT_TOTAL.as_secs());
            return;
        }
        std::thread::sleep(WAIT_STEP);
    }
    let Some(flat_addr) = super::export(FLAT_NAME) else {
        tracing::warn!("mm_quiet_invite: export SteamAPI_ISteamFriends_ActivateGameOverlayInviteDialog missing; nothing installed");
        return;
    };
    let iface = Flat::call_accessor(flat.friends).unwrap_or(0);
    let vt = vtable_target(flat_addr, iface);
    let mut addrs = [0u64; TARGETS];
    let mut mask = 0u8;
    addrs[0] = flat_addr as u64;
    ADDR[0].store(flat_addr, Ordering::Relaxed);
    mask |= 1;
    tracing::info!("mm_quiet_invite: DR0 = 0x{flat_addr:X} (flat ActivateGameOverlayInviteDialog)");
    match vt {
        Some(t) => {
            addrs[1] = t as u64;
            ADDR[1].store(t, Ordering::Relaxed);
            mask |= 2;
            tracing::info!("mm_quiet_invite: DR1 = 0x{t:X} (vtable ActivateGameOverlayInviteDialog; the game calls this one, S9 5d)");
        }
        None => tracing::warn!(
            "mm_quiet_invite: the vtable target of ActivateGameOverlayInviteDialog could not be resolved; only the flat export is armed, \
             and the game calls the vtable target (S9 5d), so the window will NOT be suppressed"
        ),
    }
    // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature; first = 1 so it sees our single-steps before other handlers. It returns
    // CONTINUE_SEARCH for everything that is not a hit on our two addresses.
    let handle = unsafe { AddVectoredExceptionHandler(1, Some(veh)) };
    if handle.is_null() {
        tracing::warn!("mm_quiet_invite: AddVectoredExceptionHandler failed; nothing installed");
        return;
    }
    READY.store(true, Ordering::Relaxed);
    tracing::info!(
        "mm_quiet_invite: ready (waited {:.1} s); returns from nightreign.exe+[0x0, 0x{:X}) are skipped, hardware breakpoints, no code is patched",
        start.elapsed().as_secs_f32(),
        EXE_SIZE.load(Ordering::Relaxed)
    );
    // SAFETY: GetCurrentThreadId has no preconditions.
    let own = unsafe { GetCurrentThreadId() };
    let pid = std::process::id();
    let mut applied: HashMap<u32, u8> = HashMap::new();
    let mut scratch: Vec<u32> = Vec::with_capacity(2048);
    let mut is_armed = false;
    let mut next_apply = Instant::now();
    let (mut seen_sup, mut seen_pass) = (0u32, 0u32);
    loop {
        std::thread::sleep(TICK);
        let want = WANT.load(Ordering::Relaxed);
        let now = Instant::now();
        if want && (!is_armed || now >= next_apply) {
            ARMED.store(true, Ordering::Relaxed);
            let (written, failed, first) = apply_all(pid, own, &addrs, mask, false, &mut applied, &mut scratch);
            if !is_armed || written > 0 || failed > 0 {
                tracing::info!(
                    "mm_quiet_invite: ARMED ({written} thread(s) written, {} armed in total, {failed} failed{})",
                    applied.len(),
                    first.map_or(String::new(), |(t, e)| format!("; first: thread {t}: {e}"))
                );
            }
            is_armed = true;
            next_apply = now + APPLY_EVERY;
        } else if !want && is_armed {
            let (written, failed, first) = apply_all(pid, own, &addrs, 0, false, &mut applied, &mut scratch);
            applied.clear();
            ARMED.store(false, Ordering::Relaxed);
            is_armed = false;
            tracing::info!(
                "mm_quiet_invite: disarmed (debug registers cleared on {written} thread(s), {failed} failed{})",
                first.map_or(String::new(), |(t, e)| format!("; first: thread {t}: {e}"))
            );
        }
        // Log from the counters (never inside the handler).
        let (sup, pass) = (SUPPRESSED.load(Ordering::Relaxed), PASSED.load(Ordering::Relaxed));
        if sup != seen_sup {
            tracing::info!(
                "mm_quiet_invite: invite dialog suppressed (lobby {}) - call from nightreign.exe+0x{:X}, {} suppressed so far",
                crate::mm::machine::idt(LAST_LOBBY.load(Ordering::Relaxed)),
                LAST_RET.load(Ordering::Relaxed).wrapping_sub(EXE_BASE.load(Ordering::Relaxed) as u64),
                sup
            );
            seen_sup = sup;
        }
        if pass != seen_pass {
            tracing::info!(
                "mm_quiet_invite: invite dialog call left alone: return address 0x{:X} is not in nightreign.exe ({pass} so far)",
                LAST_PASS_RET.load(Ordering::Relaxed)
            );
            seen_pass = pass;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Diagnostics::Debug::{CONTEXT, EXCEPTION_RECORD};

    #[test]
    fn return_address_filter_is_exactly_the_exe_image() {
        let (base, size) = (0x7FF6_0000_0000usize, 0x400_0000usize);
        assert!(return_in_image(base as u64, base, size));
        assert!(return_in_image((base + size - 1) as u64, base, size));
        assert!(!return_in_image((base + size) as u64, base, size));
        assert!(!return_in_image((base - 1) as u64, base, size));
        // steamclient64.dll / nrsc.dll style addresses are outside.
        assert!(!return_in_image(0x7FFD_1F0A_2070, base, size));
        // An unknown image matches nothing.
        assert!(!return_in_image(0, 0, 0));
        assert!(!return_in_image(1, 0, 10));
        assert!(!return_in_image(base as u64, base, 0));
        assert_eq!(decide((base + 0x258_568B) as u64, base, size), Action::Skip);
        assert_eq!(decide(0x7FFD_1F0A_2070, base, size), Action::Run);
        // Near the top of the address space: no overflow (saturating end).
        assert!(return_in_image(u64::MAX - 1, usize::MAX - 5, 100));
    }

    #[test]
    fn arming_decision_and_linger() {
        // Host waiting for the game's lobby: on. Otherwise off (Duo / Squad invites work as normal).
        assert!(arm_wanted(true, false));
        assert!(!arm_wanted(false, false));
        // The dev test window arms it regardless of the match state.
        assert!(arm_wanted(false, true));
        assert!(arm_wanted(true, true));
        // Linger: stays armed for 5 s after the lobby appeared, then drops.
        let l = LINGER;
        assert!(keep_armed(true, None, l));
        assert!(keep_armed(false, Some(Duration::from_secs(0)), l));
        assert!(keep_armed(false, Some(Duration::from_millis(4900)), l));
        assert!(!keep_armed(false, Some(l), l));
        assert!(!keep_armed(false, None, l));
    }

    #[test]
    fn a_skip_returns_to_the_caller_and_pops_the_return_address() {
        assert_eq!(skip_registers(0x7FF6_0000_1234, 0x1000), (0x7FF6_0000_1234, 0x1008));
    }

    #[test]
    fn handler_ignores_null_and_foreign_exceptions() {
        // SAFETY: null is explicitly handled.
        assert_eq!(unsafe { veh(std::ptr::null_mut()) }, EXCEPTION_CONTINUE_SEARCH);
    }

    #[repr(C, align(16))]
    struct Ctx(CONTEXT);

    /// The one test that touches the shared statics: the handler skips an in-image caller, leaves a foreign one alone, and ignores
    /// a disarmed hit.
    #[test]
    fn handler_skips_only_armed_calls_from_the_exe() {
        const BASE: usize = 0x7FF6_0000_0000;
        const SIZE: usize = 0x400_0000;
        const ENTRY: usize = 0x7FFD_1F0A_2070;
        EXE_BASE.store(BASE, Ordering::Relaxed);
        EXE_SIZE.store(SIZE, Ordering::Relaxed);
        ADDR[1].store(ENTRY, Ordering::Relaxed);
        let fire = |ret: u64, armed: bool| -> (u64, u64, u32, i32) {
            ARMED.store(armed, Ordering::Relaxed);
            let stack: [u64; 4] = [ret, 0, 0, 0];
            let mut c: Ctx = Ctx(unsafe { std::mem::zeroed() });
            c.0.Rip = ENTRY as u64;
            c.0.Rsp = stack.as_ptr() as u64;
            c.0.Rdx = 0x0186_0000_0000_0042;
            let mut r: EXCEPTION_RECORD = unsafe { std::mem::zeroed() };
            r.ExceptionCode = STATUS_SINGLE_STEP;
            let mut p = EXCEPTION_POINTERS { ExceptionRecord: &mut r, ContextRecord: &mut c.0 };
            // SAFETY: `p` points at a valid record and a 16-byte-aligned CONTEXT; Rsp points at `stack`, which outlives the call.
            let code = unsafe { veh(&mut p) };
            (c.0.Rip, c.0.Rsp.wrapping_sub(stack.as_ptr() as u64), c.0.EFlags, code)
        };
        let before = SUPPRESSED.load(Ordering::Relaxed);
        // In-image caller, armed: Rip = return address, Rsp popped by 8, RF set, counted, lobby id recorded.
        let ret = (BASE + 0x258_568B) as u64;
        let (rip, rsp_delta, eflags, code) = fire(ret, true);
        assert_eq!((rip, rsp_delta, code), (ret, 8, EXCEPTION_CONTINUE_EXECUTION));
        assert_ne!(eflags & EFLAGS_RF, 0);
        assert_eq!(SUPPRESSED.load(Ordering::Relaxed), before + 1);
        assert_eq!(LAST_LOBBY.load(Ordering::Relaxed), 0x0186_0000_0000_0042);
        // Foreign caller (inside steamclient64.dll's range): runs unchanged, RF set, not counted as suppressed.
        let (rip, rsp_delta, eflags, code) = fire(0x7FFD_0000_1000, true);
        assert_eq!((rip, rsp_delta, code), (ENTRY as u64, 0, EXCEPTION_CONTINUE_EXECUTION));
        assert_ne!(eflags & EFLAGS_RF, 0);
        assert_eq!(SUPPRESSED.load(Ordering::Relaxed), before + 1);
        // Disarmed (stray hit while the registers are being cleared): resumes normally.
        let (rip, rsp_delta, _, code) = fire(ret, false);
        assert_eq!((rip, rsp_delta, code), (ENTRY as u64, 0, EXCEPTION_CONTINUE_EXECUTION));
        assert_eq!(SUPPRESSED.load(Ordering::Relaxed), before + 1);
        // A single-step at some other address is not ours.
        ADDR[1].store(0, Ordering::Relaxed);
        let (_, _, _, code) = fire(ret, true);
        assert_eq!(code, EXCEPTION_CONTINUE_SEARCH);
        ARMED.store(false, Ordering::Relaxed);
    }
}
