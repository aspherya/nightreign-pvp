//! UI-1 option 1 (`br_menu = true`): Battle Royale matchmaking from the stock expedition menu. Commence Expedition starts our queue
//! (with the Number of Players option as the mode) and the game's own dialog says so; Commence again stops it; once the match is
//! ready, Commence works as normal so the host starts the expedition.
//!
//! No code byte is written (game 1.03.3, exe 1.3.3.0; UI-1 card in TASKS.md):
//! - **Commence** (hardware execute breakpoint, `crate::hwbp` slot 2) at `exe+0x9EEBFB`, the first instruction after the prologue of the
//!   Commence check sequence `0x9EEBE0` (`r14` = the menu; settings `[r14+0xFC8]`; Number of Players byte `+0x49`: 2 one player,
//!   1 two players, 0 three players, from the Commence label code `0x9EF3F0`). To show a message the handler sets
//!   `RIP = exe+0x9EEC1D`, the game's own branch that shows an OK dialog with CL_Dialogues text 506040 and returns normally (proven by
//!   the C1 prototype, `c1proto.rs`).
//! - **Text**: entry 506040's string-offset slot in the CL_Dialogues FMG points at one of our UTF-16 texts (`fmg + offset`, see
//!   `c1proto.rs`). The handler picks the text by storing `text - fmg` into that slot just before the dialog is built. 506040 ("not
//!   fully installed") is not otherwise shown on an installed game.
//! - **Labels** (hardware execute breakpoint, `crate::hwbp` slot 3) at `exe+0x7AC800`, the CL_MenuText getter (`edx` = text id,
//!   category 200 with its `_dlc01` twin 504 asked first). Seamless Co-op's `LookupEntry` hook replaces 130409 / 130417 / 130408 (One /
//!   Two / Three players) with "Solo Mission" / "Up to 6 players"; the handler swaps those ids for ours (99100100..=99100102 "Solo" /
//!   "Duo" / "Trio", CL_MenuText_dlc01 entries from the game-data package, `scripts/build-gamedata.ps1`) before the lookup runs, so
//!   SC's hook never sees them. Armed only once our first id is found in the live CL_MenuText_dlc01 FMG (no package = SC's labels).
//! - The handler never takes a lock: it records a request in an atomic; this module's thread hands it to the matchmaking worker
//!   (`crate::mm::send`), the same path as the overlay's queue buttons.
//!
//! Log lines start with `br_menu:`.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::STATUS_SINGLE_STEP;
use windows::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};

use super::c1proto::{dialogues_fmg, offset_slot, AFTER_PROLOGUE_BYTES, NOTICE_BRANCH_BYTES, NOTICE_ID, RVA_AFTER_PROLOGUE, RVA_NOTICE_BRANCH};
use crate::mm::{PHASE_MATCHED, PHASE_SEARCHING};
use crate::steam::lobtrace::{EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH};

/// The CL_MenuText getter `getter(out, id)` (game 1.03.3): `mov [rsp+8],rcx; push rbx; sub rsp,30h`.
pub(crate) const RVA_MENUTEXT_GETTER: usize = 0x7A_C800;
pub(crate) const MENUTEXT_GETTER_BYTES: [u8; 10] = [0x48, 0x89, 0x4C, 0x24, 0x08, 0x53, 0x48, 0x83, 0xEC, 0x30];
/// CL_MenuText_dlc01 (FMG file id in menu_dlc01.msgbnd).
const CAT_MENUTEXT_DLC: u32 = 504;
/// Our label ids (gamedata/fmg-entries.json).
pub(crate) const LABEL_SOLO: u32 = 99_100_100;
pub(crate) const LABEL_DUO: u32 = 99_100_101;
pub(crate) const LABEL_TRIO: u32 = 99_100_102;

/// The id the Number of Players list should ask for instead of the game's (pure; unit tested).
pub(crate) fn label_swap(id: u32) -> Option<u32> {
    match id {
        130_409 => Some(LABEL_SOLO),
        130_417 => Some(LABEL_DUO),
        130_408 => Some(LABEL_TRIO),
        _ => None,
    }
}

const MENU_SETTINGS_OFF: usize = 0xFC8;
const PLAYERS_BYTE: usize = 0x49;
/// Number of Players byte values (`0x9EF3F0`: 2 → 130304 "Commence Solo Expedition", 1 → 130305 "Two-Player", else three).
pub(crate) const PLAYERS_ONE: u8 = 2;
pub(crate) const PLAYERS_TWO: u8 = 1;
#[cfg_attr(not(test), allow(dead_code))] // the third value; `decide` treats it as "not one player"
pub(crate) const PLAYERS_THREE: u8 = 0;

/// Our dialog texts (index into [`TEXTS`]).
pub(crate) const T_SEARCHING: usize = 0;
pub(crate) const T_STOPPED: usize = 1;
pub(crate) const T_SOON: usize = 2;
pub(crate) const T_OFF: usize = 3;
const N_TEXTS: usize = 4;
const TEXT_UNITS: usize = 256;

/// Requests from the handler to the thread.
pub(crate) const REQ_NONE: u8 = 0;
pub(crate) const REQ_QUEUE_SOLO: u8 = 1;
pub(crate) const REQ_LEAVE: u8 = 2;

/// What a Commence press does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Let the game run Commence unchanged.
    Pass,
    /// Match ready: set the Number of Players byte to `PLAYERS_TWO` (a multiplayer start), then let Commence run.
    PassMultiplayer,
    /// Show our dialog `text` instead of Commence and hand `request` to the thread.
    Dialog { text: usize, request: u8 },
}

/// The decision for a Commence press (pure; unit tested).
pub(crate) fn decide(players: u8, phase: u8, mm_on: bool) -> Decision {
    if !mm_on {
        return Decision::Dialog { text: T_OFF, request: REQ_NONE };
    }
    match phase {
        PHASE_MATCHED => {
            if players == PLAYERS_ONE {
                Decision::PassMultiplayer
            } else {
                Decision::Pass
            }
        }
        PHASE_SEARCHING => Decision::Dialog { text: T_STOPPED, request: REQ_LEAVE },
        _ => match players {
            PLAYERS_ONE => Decision::Dialog { text: T_SEARCHING, request: REQ_QUEUE_SOLO },
            _ => Decision::Dialog { text: T_SOON, request: REQ_NONE },
        },
    }
}

pub(crate) fn texts() -> [String; N_TEXTS] {
    [
        "Battle Royale: searching for players (Solo).\nYou will be told when the match is found.\nPress Commence Expedition again to stop searching.".to_string(),
        "Battle Royale: stopped searching for players.".to_string(),
        "Duo and Trio Battle Royale are coming soon.\nChoose Solo in Matchmaking Settings > Number of Players.".to_string(),
        "Battle Royale matchmaking is turned off (matchmaking = false).".to_string(),
    ]
}

struct Texts(UnsafeCell<[[u16; TEXT_UNITS]; N_TEXTS]>);
// SAFETY: written once by this module's thread before the handler can use them (publication through `READY`, release/acquire);
// read-only afterwards (the game reads them as strings).
unsafe impl Sync for Texts {}
static TEXTS: Texts = Texts(UnsafeCell::new([[0; TEXT_UNITS]; N_TEXTS]));

static ADDR: AtomicUsize = AtomicUsize::new(0);
static LABEL_ADDR: AtomicUsize = AtomicUsize::new(0);
static LABEL_SWAPS: AtomicU32 = AtomicU32::new(0);
static TARGET: AtomicUsize = AtomicUsize::new(0);
/// The CL_Dialogues FMG and the address of entry 506040's offset slot (0 = not resolved: Commence is left alone).
static FMG: AtomicUsize = AtomicUsize::new(0);
static SLOT: AtomicUsize = AtomicUsize::new(0);
static REQUEST: AtomicU8 = AtomicU8::new(REQ_NONE);
static PRESSES: AtomicU32 = AtomicU32::new(0);
/// Last press: players byte, phase and decision code (for the log).
static LAST: AtomicU64 = AtomicU64::new(0);

fn text_addr(i: usize) -> usize {
    // SAFETY: index < N_TEXTS; only the address is taken.
    unsafe { (*TEXTS.0.get())[i].as_ptr() as usize }
}

fn decision_code(d: Decision) -> u64 {
    match d {
        Decision::Pass => 1,
        Decision::PassMultiplayer => 2,
        Decision::Dialog { text, request } => 0x100 | ((text as u64) << 4) | u64::from(request),
    }
}

unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS; pointers are null-checked. The reads go through the menu object the game is
    // using in this very function (r14 was just set from rcx) and its settings object, which the next instructions read too; the one
    // write to the settings byte is the same byte the game's own Number of Players list writes; the FMG slot address was validated
    // (committed, writable, 8-aligned) by the thread.
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
        if rip != 0 && rip == LABEL_ADDR.load(Ordering::Relaxed) {
            if let Some(ours) = label_swap((*ctx).Rdx as u32) {
                (*ctx).Rdx = u64::from(ours);
                LABEL_SWAPS.fetch_add(1, Ordering::Relaxed);
            }
            (*ctx).EFlags |= EFLAGS_RF;
            return EXCEPTION_CONTINUE_EXECUTION;
        }
        if rip == 0 || rip != ADDR.load(Ordering::Relaxed) {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let menu = (*ctx).R14 as usize;
        let settings = if menu > 0x10000 { *((menu + MENU_SETTINGS_OFF) as *const usize) } else { 0 };
        let (fmg, slot) = (FMG.load(Ordering::Acquire), SLOT.load(Ordering::Acquire));
        if settings > 0x10000 && fmg != 0 && slot != 0 {
            let pbyte = (settings + PLAYERS_BYTE) as *mut u8;
            let players = *pbyte;
            let phase = crate::mm::phase();
            let d = decide(players, phase, crate::mm::enabled());
            PRESSES.fetch_add(1, Ordering::Relaxed);
            LAST.store((u64::from(players) << 32) | (u64::from(phase) << 24) | decision_code(d), Ordering::Relaxed);
            match d {
                Decision::Pass => {}
                Decision::PassMultiplayer => *pbyte = PLAYERS_TWO,
                Decision::Dialog { text, request } => {
                    *(slot as *mut u64) = (text_addr(text) as u64).wrapping_sub(fmg as u64);
                    if request != REQ_NONE {
                        REQUEST.store(request, Ordering::Release);
                    }
                    (*ctx).Rip = TARGET.load(Ordering::Relaxed) as u64;
                }
            }
        }
        (*ctx).EFlags |= EFLAGS_RF;
    }
    EXCEPTION_CONTINUE_EXECUTION
}

/// Start the thread (`br_menu = true`, developer mode with `mm_queue` for now).
pub(crate) fn spawn(base: usize) {
    let r = std::thread::Builder::new().name("nrpvp-brmenu".into()).spawn(move || {
        if std::panic::catch_unwind(|| run(base)).is_err() {
            crate::hwbp::clear(crate::hwbp::SLOT_COMMENCE);
            tracing::error!("br_menu: thread panicked; Commence is back to normal");
        }
    });
    if r.is_err() {
        tracing::warn!("br_menu: could not spawn the thread");
    }
}

fn run(base: usize) {
    let check = |rva: usize, want: &[u8]| super::mem::read::<[u8; 13]>(base + rva).is_some_and(|b| b[..want.len()] == *want);
    if !check(RVA_AFTER_PROLOGUE, &AFTER_PROLOGUE_BYTES) || !check(RVA_NOTICE_BRANCH, &NOTICE_BRANCH_BYTES) {
        tracing::warn!("br_menu: code bytes differ from game 1.03.3; Commence left alone");
        return;
    }
    let all = texts();
    for (i, t) in all.iter().enumerate() {
        let units: Vec<u16> = t.encode_utf16().take(TEXT_UNITS - 1).collect();
        // SAFETY: nothing reads TEXTS before FMG/SLOT are published below (Release), and only this thread writes it.
        let dst = unsafe { &mut (*TEXTS.0.get())[i] };
        dst[..units.len()].copy_from_slice(&units);
        dst[units.len()] = 0;
    }
    ADDR.store(base + RVA_AFTER_PROLOGUE, Ordering::Relaxed);
    TARGET.store(base + RVA_NOTICE_BRANCH, Ordering::Relaxed);
    // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature; it returns CONTINUE_SEARCH for anything not on our address.
    if unsafe { AddVectoredExceptionHandler(1, Some(veh)) }.is_null() {
        tracing::warn!("br_menu: AddVectoredExceptionHandler failed; Commence left alone");
        return;
    }
    crate::hwbp::set(crate::hwbp::SLOT_COMMENCE, base + RVA_AFTER_PROLOGUE);
    tracing::info!("br_menu: Commence handler armed (hwbp slot 2); waiting for the CL_Dialogues FMG");
    let labels_ok = check(RVA_MENUTEXT_GETTER, &MENUTEXT_GETTER_BYTES);
    if !labels_ok {
        tracing::warn!("br_menu: menu text getter bytes differ from game 1.03.3; Number of Players labels stay as they are");
    }
    let mut labels_armed = false;
    let mut seen_swaps = 0u32;
    let mut next_resolve = Instant::now();
    let mut seen = 0u32;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let now = Instant::now();
        let resolve_tick = now >= next_resolve;
        if resolve_tick {
            next_resolve = now + Duration::from_secs(2);
            match dialogues_fmg(base).and_then(|f| offset_slot(f, NOTICE_ID).map(|(s, _)| (f, s))) {
                Some((f, s)) if f != FMG.load(Ordering::Relaxed) => {
                    if s % 8 == 0 && super::mem::read::<u64>(s).is_some() {
                        SLOT.store(s, Ordering::Release);
                        FMG.store(f, Ordering::Release);
                        tracing::info!("br_menu: CL_Dialogues FMG 0x{f:X}, entry {NOTICE_ID} slot 0x{s:X}: Commence is now Battle Royale");
                    }
                }
                Some(_) => {}
                None => {
                    if FMG.swap(0, Ordering::AcqRel) != 0 {
                        SLOT.store(0, Ordering::Release);
                        tracing::info!("br_menu: CL_Dialogues FMG gone; Commence left alone until it is back");
                    }
                }
            }
        }
        // Labels: arm once our package's ids are in the live CL_MenuText_dlc01 (else the list would show nothing).
        if labels_ok && !labels_armed && resolve_tick {
            if let Some(f) = super::c1proto::fmg_for(base, CAT_MENUTEXT_DLC) {
                if offset_slot(f, LABEL_SOLO).is_some() && offset_slot(f, LABEL_TRIO).is_some() {
                    LABEL_ADDR.store(base + RVA_MENUTEXT_GETTER, Ordering::Relaxed);
                    crate::hwbp::set(crate::hwbp::SLOT_LABELS, base + RVA_MENUTEXT_GETTER);
                    labels_armed = true;
                    tracing::info!("br_menu: our labels found in CL_MenuText_dlc01 0x{f:X}: Number of Players shows Solo / Duo / Trio (hwbp slot 3)");
                }
            }
        }
        let sw = LABEL_SWAPS.load(Ordering::Relaxed);
        if sw != seen_swaps && (seen_swaps == 0 || sw / 100 != seen_swaps / 100) {
            tracing::info!("br_menu: Number of Players labels swapped {sw} time(s) so far");
            seen_swaps = sw;
        }
        match REQUEST.swap(REQ_NONE, Ordering::AcqRel) {
            REQ_QUEUE_SOLO => {
                let ok = crate::mm::send(crate::mm::Cmd::Queue(crate::mm::Mode::Solo));
                tracing::info!("br_menu: Commence -> queue Solo ({})", if ok { "sent" } else { "NOT sent: queue off or full" });
            }
            REQ_LEAVE => {
                let ok = crate::mm::send(crate::mm::Cmd::Leave);
                tracing::info!("br_menu: Commence -> stop searching ({})", if ok { "sent" } else { "NOT sent" });
            }
            _ => {}
        }
        let p = PRESSES.load(Ordering::Relaxed);
        if p != seen {
            let l = LAST.load(Ordering::Relaxed);
            tracing::info!(
                "br_menu: Commence pressed (#{p}): Number of Players byte {}, queue phase {}, decision code 0x{:X}",
                l >> 32,
                (l >> 24) & 0xFF,
                l & 0xFF_FFFF
            );
            seen = p;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mm::PHASE_IDLE;

    #[test]
    fn idle_solo_queues_and_duo_trio_are_coming_soon() {
        assert_eq!(decide(PLAYERS_ONE, PHASE_IDLE, true), Decision::Dialog { text: T_SEARCHING, request: REQ_QUEUE_SOLO });
        assert_eq!(decide(PLAYERS_TWO, PHASE_IDLE, true), Decision::Dialog { text: T_SOON, request: REQ_NONE });
        assert_eq!(decide(PLAYERS_THREE, PHASE_IDLE, true), Decision::Dialog { text: T_SOON, request: REQ_NONE });
    }

    #[test]
    fn searching_commence_stops() {
        for p in [PLAYERS_ONE, PLAYERS_TWO, PLAYERS_THREE] {
            assert_eq!(decide(p, PHASE_SEARCHING, true), Decision::Dialog { text: T_STOPPED, request: REQ_LEAVE });
        }
    }

    #[test]
    fn matched_commence_runs_as_multiplayer() {
        assert_eq!(decide(PLAYERS_ONE, PHASE_MATCHED, true), Decision::PassMultiplayer);
        assert_eq!(decide(PLAYERS_TWO, PHASE_MATCHED, true), Decision::Pass);
    }

    #[test]
    fn matchmaking_off_says_so() {
        assert_eq!(decide(PLAYERS_ONE, PHASE_IDLE, false), Decision::Dialog { text: T_OFF, request: REQ_NONE });
    }

    #[test]
    fn labels_swap_only_the_three_number_of_players_ids() {
        assert_eq!(label_swap(130_409), Some(LABEL_SOLO));
        assert_eq!(label_swap(130_417), Some(LABEL_DUO));
        assert_eq!(label_swap(130_408), Some(LABEL_TRIO));
        assert_eq!(label_swap(130_200), None);
        assert_eq!(label_swap(130_407), None);
    }

    #[test]
    fn texts_fit_the_buffers() {
        for t in texts() {
            assert!(t.encode_utf16().count() < TEXT_UNITS);
        }
    }
}
