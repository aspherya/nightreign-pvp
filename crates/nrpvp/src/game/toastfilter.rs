//! UI-5: no "Ally is showing location" toast in a Battle Royale match. When another player places a map pin, every other game shows
//! a toast ("Ally is showing location", with the icon of the landmark nearest the pin), which tells a rival where someone is. The toasts
//! come from `UserDispLogParam` (regulation 1.03.3: rows of 32 bytes; the first field is the CL_MenuText id): row 6120 is text 337750
//! "Ally is showing location" and row 6130 is text 337720 "Ally is also showing location" (a pin moved or placed again). The function
//! that shows a toast (`exe+0x86833F`) does nothing when the row's text id is negative (`cmp dword [row], 0; jl <exit>` at `exe+0x86844F`)
//! `[VERIFIED: static reading of exe 1.3.3.0 and the two-instance trace of scenario mi-toast-trace, 2026-11-02]`.
//!
//! So the rule is a data switch, made in memory like [`super::multiplay`] (the param file stays as the game loaded it): while a match
//! runs (`in_match`) both text ids are -1, for every team size; in every other situation (a normal expedition, the Roundtable, offline)
//! they are the game's own. A teammate's toast goes too (Leonardo, 2026-11-02: simplest rule; the pin itself stays on the map; telling a
//! rival's pin from a teammate's at toast time was tried and failed, `re/TOAST_KILLFEED.md`). The table is found by scanning the process
//! for rows 6100 and 6110 (they do not change); nothing is written unless rows 6120 and 6130 hold the vanilla ids or our -1. Log lines
//! start with `toastfilter:`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::{mem, memscan};

/// Bytes of one `UserDispLogParam` row: 4 `s32`, 2 `u16`, 3 `s32`.
const ROW_SIZE: usize = 32;
/// The text id of the "Ally is showing location" row (6120) and of the "Ally is also showing location" row (6130).
const SHOWING: i32 = 337_750;
const ALSO_SHOWING: i32 = 337_720;
/// A text id the game never shows: the toast function skips the toast.
const OFF: i32 = -1;
/// Offsets of rows 6120 / 6130 from the start of row 6100.
const AT_6120: usize = 2 * ROW_SIZE;
const AT_6130: usize = 3 * ROW_SIZE;
const CHECK_EVERY: Duration = Duration::from_secs(1);
const RESCAN_AFTER: Duration = Duration::from_secs(30);
const MAX_SCANS: u32 = 6;

/// One row as the param stores it (field order of `UserDispLogParam`).
fn row(text: i32, kind: u16, f5: i32, f6: i32, f7: i32) -> [u8; ROW_SIZE] {
    let mut b = Vec::with_capacity(ROW_SIZE);
    for v in [text, -1, -1, -1] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(&kind.to_le_bytes());
    for v in [f5, f6, f7] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    let mut out = [0u8; ROW_SIZE];
    out.copy_from_slice(&b);
    out
}

/// Rows 6100 ("Location shown") and 6110 ("You also showed location"): the part of the table the scan looks for (64 bytes).
fn needle() -> Vec<u8> {
    let mut n = Vec::new();
    n.extend_from_slice(&row(337_740, 256, 0, 7320, -1));
    n.extend_from_slice(&row(337_770, 256, 0, 7305, 7305));
    n
}

/// The vanilla bytes of rows 6120 and 6130.
fn vanilla_tail() -> Vec<u8> {
    let mut n = Vec::new();
    n.extend_from_slice(&row(SHOWING, 256, 0, 7320, -1));
    n.extend_from_slice(&row(ALSO_SHOWING, 256, 0, 7305, 7305));
    n
}

/// What to write over the text id of a toast row, or `None` (pure). `current` is what the row holds, `vanilla` the game's id.
pub(crate) fn wanted(in_match: bool, current: i32, vanilla: i32) -> Option<i32> {
    let want = if in_match { OFF } else { vanilla };
    (current == vanilla || current == OFF)
        .then_some(want)
        .filter(|w| *w != current)
}

#[derive(Default)]
struct State {
    /// Start of row 6100 in every copy found.
    copies: Vec<usize>,
    last_check: Option<Instant>,
    suppressing: Option<bool>,
    empty_scans: u32,
    scanned_at: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    copies: Vec::new(),
    last_check: None,
    suppressing: None,
    empty_scans: 0,
    scanned_at: None,
});
static SCANNING: AtomicBool = AtomicBool::new(false);
static SCANNED: AtomicBool = AtomicBool::new(false);

/// Main loop, every tick: start the scan once, then keep the two toast rows right for the match.
pub(crate) fn tick(in_match: bool) {
    if !SCANNED.load(Ordering::Relaxed) {
        if !SCANNING.swap(true, Ordering::Relaxed) {
            let _ = std::thread::Builder::new()
                .name("nrpvp-toast-scan".into())
                .spawn(scan);
        }
        return;
    }
    let mut st = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    if st.copies.is_empty() {
        if st.empty_scans < MAX_SCANS && st.scanned_at.is_some_and(|t| t.elapsed() >= RESCAN_AFTER)
        {
            SCANNED.store(false, Ordering::Relaxed);
            SCANNING.store(false, Ordering::Relaxed);
        }
        return;
    }
    if st.last_check.is_some_and(|t| t.elapsed() < CHECK_EVERY) {
        return;
    }
    st.last_check = Some(Instant::now());
    let mut changed = 0usize;
    for &base in &st.copies {
        for (at, vanilla) in [(AT_6120, SHOWING), (AT_6130, ALSO_SHOWING)] {
            let Some(current) = mem::read::<i32>(base + at) else {
                continue;
            };
            if let Some(w) = wanted(in_match, current, vanilla) {
                match mem::write_bytes(base + at, &w.to_le_bytes()) {
                    Ok(()) => changed += 1,
                    Err(e) => {
                        tracing::warn!("toastfilter: write at {:#X} failed: {e}", base + at);
                        return;
                    }
                }
            }
        }
    }
    let suppressing = in_match;
    if changed > 0 || st.suppressing.is_none() {
        st.suppressing = Some(suppressing);
        tracing::info!(
            "toastfilter: ally location toasts {} ({changed} row(s) written, {} copy(ies))",
            if suppressing {
                "OFF (match)"
            } else {
                "the game's own"
            },
            st.copies.len()
        );
    }
}

fn scan() {
    let n = needle();
    let r = memscan::scan_aligned(&n, 4 * ROW_SIZE, usize::MAX, 4);
    let vanilla = vanilla_tail();
    let mut copies = Vec::new();
    for h in r.hits {
        // Rows 6120 / 6130 must hold the vanilla ids or our -1 (nothing else is touched).
        let mut tail = vec![0u8; 2 * ROW_SIZE];
        if !mem::copy_out(h + AT_6120, &mut tail) {
            continue;
        }
        let ours = |t: &[u8]| {
            let mut o = t.to_vec();
            o[..4].copy_from_slice(&OFF.to_le_bytes());
            o[ROW_SIZE..ROW_SIZE + 4].copy_from_slice(&OFF.to_le_bytes());
            o
        };
        if tail == vanilla || tail == ours(&vanilla) || tail_matches(&tail, &vanilla) {
            copies.push(h);
        }
    }
    tracing::info!(
        "toastfilter: scanned {} regions ({} MiB) in {:.1} s; {} copy(ies) of UserDispLogParam: {}",
        r.regions,
        r.bytes >> 20,
        r.secs,
        copies.len(),
        copies
            .iter()
            .map(|c| format!("{c:#X}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    {
        let mut st = STATE.lock().unwrap_or_else(PoisonError::into_inner);
        if copies.is_empty() {
            st.empty_scans += 1;
        }
        st.copies = copies;
        st.scanned_at = Some(Instant::now());
    }
    SCANNED.store(true, Ordering::Relaxed);
}

/// True when `tail` is the vanilla rows 6120 / 6130 except that each text id may be -1 (any mix).
fn tail_matches(tail: &[u8], vanilla: &[u8]) -> bool {
    tail.len() == vanilla.len()
        && [0, ROW_SIZE].iter().all(|&at| {
            let id =
                tail[at..at + 4] == OFF.to_le_bytes() || tail[at..at + 4] == vanilla[at..at + 4];
            id && tail[at + 4..at + ROW_SIZE] == vanilla[at + 4..at + ROW_SIZE]
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_32_bytes_and_the_needle_has_the_two_unchanging_rows() {
        assert_eq!(row(1, 256, 0, 0, 0).len(), ROW_SIZE);
        assert_eq!(needle().len(), 2 * ROW_SIZE);
        assert_eq!(vanilla_tail().len(), 2 * ROW_SIZE);
        assert_eq!(
            i32::from_le_bytes(needle()[..4].try_into().unwrap()),
            337_740
        );
        assert_eq!(
            i32::from_le_bytes(vanilla_tail()[..4].try_into().unwrap()),
            SHOWING
        );
    }

    #[test]
    fn a_match_turns_the_toasts_off_and_the_end_of_it_brings_them_back() {
        assert_eq!(wanted(true, SHOWING, SHOWING), Some(OFF));
        assert_eq!(wanted(true, ALSO_SHOWING, ALSO_SHOWING), Some(OFF));
        assert_eq!(wanted(true, OFF, SHOWING), None, "already off");
        assert_eq!(wanted(false, SHOWING, SHOWING), None, "outside a match");
        assert_eq!(wanted(false, OFF, SHOWING), Some(SHOWING));
        assert_eq!(wanted(false, OFF, ALSO_SHOWING), Some(ALSO_SHOWING));
    }

    #[test]
    fn a_row_that_holds_anything_else_is_never_touched() {
        assert_eq!(wanted(true, 12_345, SHOWING), None);
        assert_eq!(wanted(false, 12_345, SHOWING), None);
    }

    #[test]
    fn a_tail_is_ours_when_each_id_is_vanilla_or_minus_one() {
        let v = vanilla_tail();
        assert!(tail_matches(&v, &v));
        let mut a = v.clone();
        a[..4].copy_from_slice(&OFF.to_le_bytes());
        assert!(tail_matches(&a, &v));
        a[ROW_SIZE..ROW_SIZE + 4].copy_from_slice(&OFF.to_le_bytes());
        assert!(tail_matches(&a, &v));
        a[8] = 7;
        assert!(!tail_matches(&a, &v), "another field changed");
    }
}
