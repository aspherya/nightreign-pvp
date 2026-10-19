//! UI-1 C1 prototype (dev only, `ui_c1_proto = true`): pressing Commence Expedition with a multiplayer Number of Players option opens the
//! game's OWN dialog with our live text ("Searching for players n/6") instead of the offline error. Proves the two mechanisms C1 needs;
//! the queue is not started yet.
//!
//! No code byte is written. Two mechanisms (game 1.03.3, exe 1.3.3.0; UI-1 card, trace of 2026-10-19):
//! - **Redirect** (hardware execute breakpoint): at `exe+0x9EEBFB`, the first instruction after the prologue of the Commence check
//!   sequence `0x9EEBE0` (`r14` = the menu, settings `[r14+0xFC8]`, Number of Players byte `+0x49`, 2 = one player), when the byte is
//!   not 2 the handler sets `RIP = exe+0x9EEC1D`: the game's own branch that builds and shows the OK dialog with text 506040 ("Cannot
//!   embark … fully installed") and then leaves through the function's epilogue (`0x9EF0B4`). The caller ignores the result.
//! - **Live text** (one data write): FMG strings are `fmg + offsets[idx]` (`exe+0x27F5710`: `[[fmg+0x18] + idx*8] + fmg`). CL_Dialogues
//!   (category 204, no 506040 in CL_Dialogues_dlc01) entry 506040's offset slot is set to `buffer - fmg`, `buffer` being a UTF-16
//!   string owned by this DLL and rewritten every second. The old value is kept and put back when the prototype ends.
//!
//! Log lines start with `ui_c1_proto:`.

use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::STATUS_SINGLE_STEP;
use windows::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::steam::lobtrace::{apply_all, EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH, TARGETS};

/// First instruction after the prologue of the Commence checks: `mov rcx,[rip+3A3FCA6h]` (game 1.03.3).
pub(crate) const RVA_AFTER_PROLOGUE: usize = 0x9E_EBFB;
pub(crate) const AFTER_PROLOGUE_BYTES: [u8; 7] = [0x48, 0x8B, 0x0D, 0xA6, 0xFC, 0xA3, 0x03];
/// The 506040 notice-dialog branch: `lea rax,[rbp+50h]` … `mov edx,7B8B8h` (game 1.03.3).
pub(crate) const RVA_NOTICE_BRANCH: usize = 0x9E_EC1D;
pub(crate) const NOTICE_BRANCH_BYTES: [u8; 13] = [0x48, 0x8D, 0x45, 0x50, 0x48, 0x89, 0x45, 0x58, 0xBA, 0xB8, 0xB8, 0x07, 0x00];
/// `MsgRepository` slot (game 1.03.3; TC-004).
const RVA_MSGREPO: usize = 0x3C2_46A8;
const CAT_DIALOGUES: u32 = 204;
pub(crate) const NOTICE_ID: u32 = 506_040;
const MENU_SETTINGS_OFF: usize = 0xFC8;
const PLAYERS_BYTE: usize = 0x49;
const ONE_PLAYER: u8 = 2;
const BUF_UNITS: usize = 256;
const TICK: Duration = Duration::from_secs(1);
const APPLY_EVERY: Duration = Duration::from_secs(2);

static ADDR: AtomicUsize = AtomicUsize::new(0);
static TARGET: AtomicUsize = AtomicUsize::new(0);
static REDIRECTS: AtomicU32 = AtomicU32::new(0);
static PASSED: AtomicU32 = AtomicU32::new(0);
static LAST_VALUE: AtomicU32 = AtomicU32::new(u32::MAX);

struct Buf(UnsafeCell<[u16; BUF_UNITS]>);
// SAFETY: written only by the prototype thread (volatile stores), read by the game as a string; a torn read shows a mixed string for
// one frame at worst, never out-of-bounds (the last unit is always 0).
unsafe impl Sync for Buf {}
static BUF: Buf = Buf(UnsafeCell::new([0; BUF_UNITS]));

/// Index into the FMG's string-offset table for `id`, from the groups `(offset_index, first_id, last_id)` (SoulsFormats FMG; the same
/// search as `exe+0x27F5710`).
pub(crate) fn fmg_index(groups: &[(u32, u32, u32)], id: u32) -> Option<u32> {
    groups.iter().find(|g| id >= g.1 && id <= g.2).map(|g| g.0 + (id - g.1))
}

/// Whether a Commence with this Number of Players byte is redirected.
pub(crate) fn redirect(players: u8) -> bool {
    players != ONE_PLAYER
}

/// The text shown for search tick `n`.
pub(crate) fn text_for(n: u32) -> String {
    format!("Battle Royale (prototype)\nSearching for players {}/6 ({n} s)\nPress OK to stop searching", (n % 6) + 1)
}

unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS; pointers are null-checked. The two reads go through the menu object the game
    // is using in this very function (r14 was just set from rcx) and its settings object, which the next instructions also read.
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
        if rip == 0 || rip != ADDR.load(Ordering::Relaxed) {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let menu = (*ctx).R14 as usize;
        let settings = if menu > 0x10000 { *((menu + MENU_SETTINGS_OFF) as *const usize) } else { 0 };
        let players = if settings > 0x10000 { *((settings + PLAYERS_BYTE) as *const u8) } else { ONE_PLAYER };
        LAST_VALUE.store(u32::from(players), Ordering::Relaxed);
        if redirect(players) {
            REDIRECTS.fetch_add(1, Ordering::Relaxed);
            (*ctx).Rip = TARGET.load(Ordering::Relaxed) as u64;
        } else {
            PASSED.fetch_add(1, Ordering::Relaxed);
        }
        (*ctx).EFlags |= EFLAGS_RF;
    }
    EXCEPTION_CONTINUE_EXECUTION
}

/// Start the prototype thread (developer mode with `ui_c1_proto = true`).
pub(crate) fn spawn(base: usize) {
    let r = std::thread::Builder::new().name("nrpvp-c1proto".into()).spawn(move || {
        if std::panic::catch_unwind(|| run(base)).is_err() {
            tracing::error!("ui_c1_proto: thread panicked");
        }
    });
    if r.is_err() {
        tracing::warn!("ui_c1_proto: could not spawn the thread");
    }
}

fn r64(a: usize) -> Option<usize> {
    super::mem::read::<u64>(a).map(|v| v as usize)
}
fn r32(a: usize) -> Option<u32> {
    super::mem::read::<u32>(a)
}

/// The CL_Dialogues FMG (slot 0) from the repository table (`[[repo+8] + slot*8] + cat*8`, bounds at repo+0x10/+0x14).
fn dialogues_fmg(base: usize) -> Option<usize> {
    let repo = r64(base + RVA_MSGREPO).filter(|p| *p != 0)?;
    let table = r64(repo + 8)?;
    let cats = r32(repo + 0x14)?;
    if CAT_DIALOGUES >= cats {
        return None;
    }
    let row = r64(table).filter(|p| *p != 0)?;
    r64(row + CAT_DIALOGUES as usize * 8).filter(|p| *p != 0)
}

/// Address of the string-offset slot of `id` in `fmg`, and the FMG's group count (for the log).
fn offset_slot(fmg: usize, id: u32) -> Option<(usize, u32)> {
    let count = r32(fmg + 0xC)?;
    if count == 0 || count > 100_000 {
        return None;
    }
    let mut groups = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let g = fmg + 0x28 + i * 16;
        groups.push((r32(g)?, r32(g + 4)?, r32(g + 8)?));
    }
    let idx = fmg_index(&groups, id)?;
    let offsets = r64(fmg + 0x18)?;
    Some((offsets + idx as usize * 8, count))
}

fn write_text(s: &str) {
    let units: Vec<u16> = s.encode_utf16().take(BUF_UNITS - 1).collect();
    let p = BUF.0.get().cast::<u16>();
    for (i, u) in units.iter().enumerate() {
        // SAFETY: i < BUF_UNITS - 1; only this thread writes BUF.
        unsafe { p.add(i).write_volatile(*u) };
    }
    // SAFETY: units.len() <= BUF_UNITS - 1.
    unsafe { p.add(units.len()).write_volatile(0) };
}

fn run(base: usize) {
    let check = |rva: usize, want: &[u8]| super::mem::read::<[u8; 13]>(base + rva).is_some_and(|b| b[..want.len()] == *want);
    if !check(RVA_AFTER_PROLOGUE, &AFTER_PROLOGUE_BYTES) || !check(RVA_NOTICE_BRANCH, &NOTICE_BRANCH_BYTES) {
        tracing::warn!("ui_c1_proto: code bytes differ from game 1.03.3; not armed");
        return;
    }
    write_text(&text_for(0));
    ADDR.store(base + RVA_AFTER_PROLOGUE, Ordering::Relaxed);
    TARGET.store(base + RVA_NOTICE_BRANCH, Ordering::Relaxed);
    // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature; it returns CONTINUE_SEARCH for anything not on our address.
    let handle = unsafe { AddVectoredExceptionHandler(1, Some(veh)) };
    if handle.is_null() {
        tracing::warn!("ui_c1_proto: AddVectoredExceptionHandler failed");
        return;
    }
    let own = unsafe { GetCurrentThreadId() };
    let pid = std::process::id();
    let mut addrs = [0u64; TARGETS];
    addrs[0] = (base + RVA_AFTER_PROLOGUE) as u64;
    let mut applied: HashMap<u32, u8> = HashMap::new();
    let mut scratch = Vec::with_capacity(2048);
    let mut next_apply = Instant::now();
    let buf_addr = BUF.0.get() as usize;
    // (fmg, slot address, original value) currently redirected.
    let mut patched: Option<(usize, usize, u64)> = None;
    let (mut n, mut seen_red, mut seen_pass) = (0u32, 0u32, 0u32);
    tracing::info!(
        "ui_c1_proto: armed: Commence with a multiplayer Number of Players opens the game's 506040 dialog with our text (buffer 0x{buf_addr:X})"
    );
    loop {
        std::thread::sleep(TICK);
        let now = Instant::now();
        if now >= next_apply {
            let _ = apply_all(pid, own, &addrs, 0b1, false, &mut applied, &mut scratch);
            next_apply = now + APPLY_EVERY;
        }
        n = n.wrapping_add(1);
        write_text(&text_for(n));
        // Keep 506040 pointing at our buffer (the FMG can be reloaded, e.g. on a language change).
        if let Some(fmg) = dialogues_fmg(base) {
            if patched.is_none_or(|(f, _, _)| f != fmg) {
                match offset_slot(fmg, NOTICE_ID) {
                    Some((slot, groups)) => {
                        let old = super::mem::read::<u64>(slot).unwrap_or(0);
                        let new = (buf_addr as u64).wrapping_sub(fmg as u64);
                        match super::mem::write_u64_aligned(slot, new) {
                            Ok(()) => {
                                tracing::info!(
                                    "ui_c1_proto: CL_Dialogues FMG 0x{fmg:X} ({groups} groups): entry {NOTICE_ID} slot 0x{slot:X} offset 0x{old:X} -> 0x{new:X} (our text)"
                                );
                                patched = Some((fmg, slot, old));
                            }
                            Err(e) => tracing::warn!("ui_c1_proto: could not redirect entry {NOTICE_ID}: {e}"),
                        }
                    }
                    None => tracing::warn!("ui_c1_proto: entry {NOTICE_ID} not found in CL_Dialogues FMG 0x{fmg:X}"),
                }
            }
        }
        let (red, pass) = (REDIRECTS.load(Ordering::Relaxed), PASSED.load(Ordering::Relaxed));
        if red != seen_red || pass != seen_pass {
            tracing::info!(
                "ui_c1_proto: Commence pressed (Number of Players byte {}): {} redirected to our dialog, {} left alone (one player)",
                LAST_VALUE.load(Ordering::Relaxed),
                red,
                pass
            );
            (seen_red, seen_pass) = (red, pass);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmg_index_follows_the_groups() {
        let g = [(0, 10, 12), (3, 100, 101), (5, 506_040, 506_040)];
        assert_eq!(fmg_index(&g, 11), Some(1));
        assert_eq!(fmg_index(&g, 101), Some(4));
        assert_eq!(fmg_index(&g, 506_040), Some(5));
        assert_eq!(fmg_index(&g, 50), None);
    }

    #[test]
    fn only_multiplayer_options_are_redirected() {
        assert!(redirect(0) && redirect(1));
        assert!(!redirect(2));
    }

    #[test]
    fn text_counts_one_to_six() {
        assert!(text_for(0).contains("1/6"));
        assert!(text_for(5).contains("6/6"));
        assert!(text_for(6).contains("1/6"));
        assert!(text_for(42).contains("(42 s)"));
    }
}
