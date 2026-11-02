//! UI-5 research (dev only, `toast_trace = true`): who asks the game for the text of an in-game toast. The "Ally is showing location"
//! toast (a map pin placed by another player, with the icon of the nearest landmark) is text 337750 in CL_MenuText; its relatives are
//! 337700..337799 in steps of 10 (`re/TOAST_KILLFEED.md`). The id is computed at run time, so the code that creates the toast cannot be found
//! by searching for the number. Instead a hardware execute breakpoint on the CL_MenuText getter (`exe+0x7AC800`, the one `menuflow` uses
//! for labels) logs, for every lookup of an id in that range, the code addresses found on the stack at the call (`exe+0x...`): the first
//! ones are the callers.
//!
//! The handler is allocation-free: it copies the first stack words into a static ring and the thread logs them. It uses the Commence
//! debug register, so it excludes `br_menu`. Log lines start with `toast_trace:`.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use windows::Win32::Foundation::STATUS_SINGLE_STEP;
use windows::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};

use super::menuflow::{MENUTEXT_GETTER_BYTES, RVA_MENUTEXT_GETTER};
use crate::dr::{EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH};

/// The toast text family (CL_MenuText ids, game 1.03.3).
const FIRST_ID: u32 = 337_700;
const LAST_ID: u32 = 337_799;
/// Stack words kept per hit, and hits kept between two thread ticks.
const FRAMES: usize = 40;
const SLOTS: usize = 32;

static ADDR: AtomicUsize = AtomicUsize::new(0);
static NEXT: AtomicUsize = AtomicUsize::new(0);
static READY: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
static RING: [[AtomicU64; FRAMES + 2]; SLOTS] =
    [const { [const { AtomicU64::new(0) }; FRAMES + 2] }; SLOTS];

/// True for a text id of the toast family (pure).
pub(crate) fn is_toast_id(id: u32) -> bool {
    (FIRST_ID..=LAST_ID).contains(&id)
}

unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS; pointers are null-checked. The stack words are read from the stack of the
    // thread that is stopped at the getter's first instruction: `FRAMES` words above `rsp` belong to its callers' frames.
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
        let id = (*ctx).Rdx as u32;
        if is_toast_id(id) {
            let i = NEXT.fetch_add(1, Ordering::Relaxed) % SLOTS;
            READY[i].store(0, Ordering::Relaxed);
            RING[i][0].store(u64::from(id), Ordering::Relaxed);
            RING[i][1].store((*ctx).Rsp, Ordering::Relaxed);
            let sp = (*ctx).Rsp as *const u64;
            for k in 0..FRAMES {
                RING[i][2 + k].store(std::ptr::read_volatile(sp.add(k)), Ordering::Relaxed);
            }
            READY[i].store(1, Ordering::Release);
        }
        (*ctx).EFlags |= EFLAGS_RF;
    }
    EXCEPTION_CONTINUE_EXECUTION
}

/// Start the thread. `base` / `size`: the image of `nightreign.exe`.
pub(crate) fn spawn(base: usize, size: usize) {
    let r = std::thread::Builder::new()
        .name("nrpvp-toasttrace".into())
        .spawn(move || {
            if std::panic::catch_unwind(|| run(base, size)).is_err() {
                crate::hwbp::clear(crate::hwbp::SLOT_COMMENCE);
                tracing::error!("toast_trace: thread panicked; stopped");
            }
        });
    if r.is_err() {
        tracing::warn!("toast_trace: could not spawn the thread");
    }
}

fn run(base: usize, size: usize) {
    let ok = super::mem::read::<[u8; 13]>(base + RVA_MENUTEXT_GETTER)
        .is_some_and(|b| b[..MENUTEXT_GETTER_BYTES.len()] == MENUTEXT_GETTER_BYTES);
    if !ok {
        tracing::warn!("toast_trace: the CL_MenuText getter differs from game 1.03.3; not armed");
        return;
    }
    // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature; it returns CONTINUE_SEARCH for anything not on our address.
    if unsafe { AddVectoredExceptionHandler(1, Some(veh)) }.is_null() {
        tracing::warn!("toast_trace: AddVectoredExceptionHandler failed; not armed");
        return;
    }
    ADDR.store(base + RVA_MENUTEXT_GETTER, Ordering::Relaxed);
    crate::hwbp::set(crate::hwbp::SLOT_COMMENCE, base + RVA_MENUTEXT_GETTER);
    tracing::info!(
        "toast_trace: armed on the CL_MenuText getter (hwbp slot 2): text ids {FIRST_ID}..{LAST_ID}"
    );
    loop {
        std::thread::sleep(Duration::from_millis(200));
        for i in 0..SLOTS {
            if READY[i].swap(2, Ordering::Acquire) != 1 {
                continue;
            }
            let id = RING[i][0].load(Ordering::Relaxed);
            let rsp = RING[i][1].load(Ordering::Relaxed);
            let frames: Vec<String> = (0..FRAMES)
                .filter_map(|k| {
                    let v = RING[i][2 + k].load(Ordering::Relaxed) as usize;
                    (v >= base && v < base + size).then(|| format!("[{k}]exe+{:#X}", v - base))
                })
                .collect();
            tracing::info!(
                "toast_trace: text id {id} asked (rsp {rsp:#X}); code addresses on the stack: {}",
                frames.join(" ")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_toast_family_is_traced() {
        assert!(is_toast_id(337_750) && is_toast_id(337_700) && is_toast_id(337_799));
        assert!(!is_toast_id(337_699) && !is_toast_id(337_800) && !is_toast_id(130_304));
    }
}
