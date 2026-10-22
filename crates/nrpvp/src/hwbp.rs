//! One owner for the four hardware debug registers (DR0..DR3) of every thread, so several features can hold an execute breakpoint at
//! the same time (MM-3f quiet invite: slots 0 and 1; UI-1 Commence handler: slot 2; UI-1 Number of Players labels: slot 3). Each feature keeps its own vectored exception
//! handler that matches only its own addresses and returns CONTINUE_SEARCH for anything else; this module only decides what the debug
//! registers hold.
//!
//! A feature calls [`set`] / [`clear`] for its slot; one thread ([`spawn`], started on first use) rewrites the debug registers of every
//! thread whenever the wanted set changes, and at least every [`APPLY_EVERY`] for new threads, through `dr::apply_all`
//! (suspend, GetThreadContext, SetThreadContext, resume; nothing allocates while a thread is suspended). The dev tracers that take all
//! four registers (`mm_lobby_trace`, `ui_text_trace`, `ui_menu_trace`, `ui_c1_proto`) do not use this module; the config switches the
//! features that do off while one of them is on.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::dr::{apply_all, TARGETS, WRITE_SHIFT};

/// Slot of MM-3f's flat `ActivateGameOverlayInviteDialog` export.
pub(crate) const SLOT_QUIET_FLAT: usize = 0;
/// Slot of MM-3f's vtable target.
pub(crate) const SLOT_QUIET_VTABLE: usize = 1;
/// Slot of the UI-1 Commence handler.
pub(crate) const SLOT_COMMENCE: usize = 2;
/// Slot of the UI-1 Number of Players label swap.
pub(crate) const SLOT_LABELS: usize = 3;

const TICK: Duration = Duration::from_millis(100);
const APPLY_EVERY: Duration = Duration::from_secs(2);

static SLOTS: [AtomicUsize; TARGETS] = [const { AtomicUsize::new(0) }; TARGETS];
/// Bit `i` set: slot `i` is a 4-byte write watch ([`set_write`]).
static WRITE: AtomicU8 = AtomicU8::new(0);
static DIRTY: AtomicBool = AtomicBool::new(false);
static START: Once = Once::new();

/// The addresses and mask of a slot table (0 = slot unused). Bits 0..3 enable a slot; `writes` (bit `i` = slot `i`) turns the enabled
/// slot into a write watch (mask bit `4+i`, see `dr::dr7_for`).
pub(crate) fn wanted(slots: &[usize; TARGETS], writes: u8) -> ([u64; TARGETS], u8) {
    let mut addrs = [0u64; TARGETS];
    let mut mask = 0u8;
    for (i, a) in slots.iter().enumerate() {
        if *a != 0 {
            addrs[i] = *a as u64;
            mask |= 1 << i;
            if writes & (1 << i) != 0 {
                mask |= 1 << (WRITE_SHIFT as usize + i);
            }
        }
    }
    (addrs, mask)
}

/// Put an execute breakpoint on `addr` in `slot` (on every thread, within ~100 ms).
pub(crate) fn set(slot: usize, addr: usize) {
    ensure_thread();
    if WRITE.fetch_and(!(1 << slot), Ordering::Relaxed) & (1 << slot) != 0 {
        DIRTY.store(true, Ordering::Relaxed);
    }
    if SLOTS[slot].swap(addr, Ordering::Relaxed) != addr {
        DIRTY.store(true, Ordering::Relaxed);
    }
}

/// Put a 4-byte write watch on `addr` (4-aligned) in `slot` (dev tracing only).
#[cfg(feature = "dev")]
pub(crate) fn set_write(slot: usize, addr: usize) {
    ensure_thread();
    if WRITE.fetch_or(1 << slot, Ordering::Relaxed) & (1 << slot) == 0 {
        DIRTY.store(true, Ordering::Relaxed);
    }
    if SLOTS[slot].swap(addr, Ordering::Relaxed) != addr {
        DIRTY.store(true, Ordering::Relaxed);
    }
}

/// Free `slot`.
pub(crate) fn clear(slot: usize) {
    WRITE.fetch_and(!(1 << slot), Ordering::Relaxed);
    if SLOTS[slot].swap(0, Ordering::Relaxed) != 0 {
        DIRTY.store(true, Ordering::Relaxed);
    }
}

/// Nobody holds `slot` right now.
pub(crate) fn is_free(slot: usize) -> bool {
    SLOTS[slot].load(Ordering::Relaxed) == 0
}

fn ensure_thread() {
    START.call_once(|| {
        let r = std::thread::Builder::new()
            .name("nrpvp-hwbp".into())
            .spawn(|| {
                if std::panic::catch_unwind(run).is_err() {
                    tracing::error!(
                        "hwbp: arming thread panicked; breakpoints are no longer updated"
                    );
                }
            });
        if r.is_err() {
            tracing::warn!("hwbp: could not spawn the arming thread");
        }
    });
}

fn run() {
    // SAFETY: GetCurrentThreadId has no preconditions.
    let own = unsafe { GetCurrentThreadId() };
    let pid = std::process::id();
    let mut applied: HashMap<u32, u8> = HashMap::new();
    let mut scratch: Vec<u32> = Vec::with_capacity(2048);
    let mut last: Option<([u64; TARGETS], u8)> = None;
    let mut next_apply = Instant::now();
    tracing::info!("hwbp: arming thread started (slots: 0/1 quiet invite, 2 Commence, 3 labels)");
    loop {
        std::thread::sleep(TICK);
        let snap: [usize; TARGETS] = std::array::from_fn(|i| SLOTS[i].load(Ordering::Relaxed));
        let (addrs, mask) = wanted(&snap, WRITE.load(Ordering::Relaxed));
        let changed = DIRTY.swap(false, Ordering::Relaxed) || last != Some((addrs, mask));
        let now = Instant::now();
        if !changed && now < next_apply {
            continue;
        }
        if changed {
            applied.clear(); // every thread gets the new set
        }
        let (written, failed, first) =
            apply_all(pid, own, &addrs, mask, changed, &mut applied, &mut scratch);
        if changed {
            tracing::info!(
                "hwbp: debug registers now {:?} (mask {mask:#010b}; write watch on slot(s) {:?}): {written} thread(s) written, {failed} failed{}",
                addrs.iter().map(|a| format!("0x{a:X}")).collect::<Vec<_>>(),
                (0..TARGETS).filter(|i| mask & (1 << (WRITE_SHIFT as usize + i)) != 0).collect::<Vec<_>>(),
                first.map_or(String::new(), |(t, e)| format!("; first: thread {t}: {e}"))
            );
        }
        last = Some((addrs, mask));
        next_apply = now + APPLY_EVERY;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wanted_builds_mask_from_used_slots() {
        assert_eq!(wanted(&[0, 0, 0, 0], 0), ([0; 4], 0));
        assert_eq!(wanted(&[0x10, 0, 0x30, 0], 0), ([0x10, 0, 0x30, 0], 0b0101));
    }

    #[test]
    fn wanted_marks_write_watches_only_on_used_slots() {
        assert_eq!(
            wanted(&[0x10, 0, 0, 0x40], 0b1000),
            ([0x10, 0, 0, 0x40], 0b1000_1001)
        );
        // a write flag on an unused slot is ignored
        assert_eq!(wanted(&[0, 0, 0, 0], 0b1000), ([0; 4], 0));
    }
}
