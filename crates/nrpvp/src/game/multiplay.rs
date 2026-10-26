//! RW-10: enemy strength follows the match type, not the session size. `MultiPlayCorrectionParam` (regulation 1.03.3, 85 rows) gives each
//! enemy kind a SpEffect for "1 co-op client" (two players) and "2 clients" (three players), and none for a third: the game picks one
//! by how many players are in the session. A Battle Royale session has 2 to 6 players however many are on each team, so the game's pick is wrong
//! for every team size (Solo should meet solo enemies, Duo the two-player ones, Trio the three-player ones; Leonardo, 2026-10-24).
//!
//! The mod rewrites the three SpEffect fields of every row in memory (the param file stays as the game loaded it), once the table is
//! found: Solo = no SpEffect, Duo = every client slot holds the two-player SpEffect, Trio = the three-player one. The table is found by
//! scanning the process for the 85-row data block (vanilla values, byte for byte) on a background thread; nothing is written unless a
//! whole block matches. Writes are 12 bytes at a time through `mem::write_bytes`. Log lines start with `multiplay:`.
//! Param semantics: Paramdex NR `MultiPlayCorrectionParam` (`client1SpEffectId`, `client2SpEffectId`, `client3SpEffectId`).
//! `[HYPOTHESIS]` until an expedition shows enemy health matching the team size.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::{mem, memscan};

/// Rows of the table: `(row id, client 1 SpEffect, client 2 SpEffect)`; client 3 is -1 in every row (game 1.03.3).
pub(crate) const ROWS: [(i32, i32, i32); 85] = [
    (0, -1, -1),
    (7700, 7800, 7900),
    (7701, 7801, 7901),
    (7702, 7802, 7902),
    (7703, 7803, 7903),
    (7704, 7804, 7904),
    (7705, 7805, 7905),
    (7706, 7806, 7906),
    (7707, 7807, 7907),
    (7708, 7803, 7903),
    (7709, 7809, 7909),
    (7710, 7810, 7910),
    (7711, 7811, 7911),
    (7712, 7812, 7912),
    (7713, 7813, 7913),
    (7714, 7814, 7914),
    (7715, 7815, 7915),
    (7716, 7816, 7916),
    (7717, 7817, 7917),
    (7718, 7813, 7913),
    (7719, 7819, 7919),
    (7720, 7820, 7920),
    (7721, 7821, 7921),
    (7722, 7822, 7922),
    (7723, 7823, 7923),
    (7724, 7824, 7924),
    (7725, 7825, 7925),
    (7726, 7826, 7926),
    (7727, 7827, 7927),
    (7728, 7823, 7923),
    (7729, 7829, 7929),
    (7730, 7830, 7930),
    (7731, 7831, 7931),
    (7732, 7832, 7932),
    (7733, 7833, 7933),
    (7734, 7834, 7934),
    (7735, 7835, 7935),
    (7736, 7836, 7936),
    (7737, 7837, 7937),
    (7738, 7833, 7933),
    (7739, 7839, 7939),
    (7740, 7840, 7940),
    (7741, 7841, 7941),
    (7742, 7842, 7942),
    (7743, 7843, 7943),
    (7744, 7844, 7944),
    (7745, 7845, 7945),
    (7746, 7846, 7946),
    (7747, 7847, 7947),
    (7748, 7843, 7943),
    (7749, 7849, 7949),
    (7750, 7850, 7950),
    (7751, 7851, 7951),
    (7752, 7852, 7952),
    (7753, 7853, 7953),
    (7754, 7854, 7954),
    (7755, 7855, 7955),
    (7756, 7856, 7956),
    (7757, 7857, 7957),
    (7758, 7853, 7953),
    (7759, 7859, 7959),
    (7760, 7860, 7960),
    (7761, 7861, 7961),
    (7762, 7862, 7962),
    (7763, 7863, 7963),
    (7764, 7864, 7964),
    (7765, 7865, 7965),
    (7766, 7866, 7966),
    (7767, 7867, 7967),
    (7768, 7863, 7963),
    (7769, 7869, 7969),
    (7770, 45696, 45697),
    (7771, 45696, 45697),
    (7772, 45696, 45697),
    (7773, 45696, 45697),
    (7774, 45696, 45697),
    (7775, 45696, 45697),
    (7776, 45696, 45697),
    (7777, 45696, 45697),
    (7778, 45696, 45697),
    (7779, 45696, 45697),
    (98810, 98811, 98812),
    (98815, 98816, 98817),
    (98818, 98818, 98819),
    (98822, 98822, 98823),
];
/// Bytes of one row's data (`MULTI_PLAY_CORRECTION_PARAM_ST`): flags and padding (4), three SpEffect ids (12), the override byte and
/// padding (16).
const ROW_SIZE: usize = 32;
/// Offset of the first SpEffect id inside a row's data.
const FIELDS_AT: usize = 4;
/// How often the table is checked once it is found.
const CHECK_EVERY: Duration = Duration::from_secs(2);

/// The three SpEffect ids a row should hold for a team size (`team_size` 1..=3; anything else leaves the vanilla values).
pub(crate) fn wanted(row: usize, team_size: usize) -> [i32; 3] {
    let (_, c1, c2) = ROWS[row];
    match team_size {
        1 => [-1, -1, -1],
        2 => [c1, c1, c1],
        3 => [c2, c2, c2],
        _ => [c1, c2, -1],
    }
}

fn row_values(block: &[u8], i: usize) -> [i32; 3] {
    let at = i * ROW_SIZE + FIELDS_AT;
    [0, 4, 8].map(|o| {
        i32::from_le_bytes(
            block
                .get(at + o..at + o + 4)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 4]),
        )
    })
}

/// True when `block` (85 rows of data) holds the vanilla values, or the values this module wrote for `team_size`.
pub(crate) fn block_matches(block: &[u8], team_size: Option<usize>) -> bool {
    if block.len() < ROWS.len() * ROW_SIZE {
        return false;
    }
    (0..ROWS.len()).all(|i| {
        let got = row_values(block, i);
        got == wanted(i, 0) || team_size.is_some_and(|t| got == wanted(i, t))
    })
}

#[derive(Default)]
struct State {
    /// Start of the row data (row 0) of every copy found.
    copies: Vec<usize>,
    last_check: Option<Instant>,
    applied: Option<usize>,
    /// Scans that found nothing (the table may not be loaded yet): another one starts after [`RESCAN_AFTER`], up to [`MAX_SCANS`].
    empty_scans: u32,
    scanned_at: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    copies: Vec::new(),
    last_check: None,
    applied: None,
    empty_scans: 0,
    scanned_at: None,
});
const RESCAN_AFTER: Duration = Duration::from_secs(60);
const MAX_SCANS: u32 = 4;
static SCANNING: AtomicBool = AtomicBool::new(false);
static SCANNED: AtomicBool = AtomicBool::new(false);

/// Main loop, every tick: start the scan once, then keep the rows right for `team_size`.
pub(crate) fn tick(team_size: usize) {
    if !SCANNED.load(Ordering::Relaxed) {
        if !SCANNING.swap(true, Ordering::Relaxed) {
            let _ = std::thread::Builder::new()
                .name("nrpvp-multiplay-scan".into())
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
        let mut block = vec![0u8; ROWS.len() * ROW_SIZE];
        if !mem::copy_out(base, &mut block) || !block_matches(&block, Some(team_size)) {
            continue;
        }
        for i in 0..ROWS.len() {
            let w = wanted(i, team_size);
            if row_values(&block, i) == w {
                continue;
            }
            let mut bytes = [0u8; 12];
            for (k, v) in w.iter().enumerate() {
                bytes[k * 4..k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            match mem::write_bytes(base + i * ROW_SIZE + FIELDS_AT, &bytes) {
                Ok(()) => changed += 1,
                Err(e) => {
                    tracing::warn!("multiplay: write in row {i} failed: {e}");
                    return;
                }
            }
        }
    }
    let copies = st.copies.len();
    if changed > 0 || st.applied != Some(team_size) {
        st.applied = Some(team_size);
        tracing::info!(
            "multiplay: enemy scaling rows set for team size {team_size} ({changed} row(s) rewritten, {copies} copy(ies))"
        );
    }
}

/// The vanilla 12 bytes of row 7700 (client 1 = 7800, client 2 = 7900, client 3 = -1): where the scan looks first.
fn needle() -> [u8; 12] {
    let w = [7800i32, 7900, -1];
    let mut b = [0u8; 12];
    for (k, v) in w.iter().enumerate() {
        b[k * 4..k * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    b
}

fn scan() {
    let n = needle();
    // Row 7700 is the second row: its data starts FIELDS_AT before the needle, row 0 one row earlier.
    let back = FIELDS_AT + ROW_SIZE;
    let r = memscan::scan(&n, ROWS.len() * ROW_SIZE, usize::MAX);
    // Keep only whole 85-row blocks of vanilla values (also drops any stray byte match).
    let mut copies = Vec::new();
    for h in r.hits {
        let Some(s) = h.checked_sub(back) else {
            continue;
        };
        let mut block = vec![0u8; ROWS.len() * ROW_SIZE];
        if mem::copy_out(s, &mut block) && block_matches(&block, None) {
            copies.push(s);
        }
    }
    tracing::info!(
        "multiplay: scanned {} regions ({} MiB) in {:.1} s; {} copy(ies) of MultiPlayCorrectionParam: {}",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn vanilla_block() -> Vec<u8> {
        let mut b = vec![0u8; ROWS.len() * ROW_SIZE];
        for (i, (_, c1, c2)) in ROWS.iter().enumerate() {
            let at = i * ROW_SIZE + FIELDS_AT;
            b[at..at + 4].copy_from_slice(&c1.to_le_bytes());
            b[at + 4..at + 8].copy_from_slice(&c2.to_le_bytes());
            b[at + 8..at + 12].copy_from_slice(&(-1i32).to_le_bytes());
        }
        b
    }

    #[test]
    fn the_table_matches_the_regulation_dump() {
        assert_eq!(ROWS.len(), 85);
        assert_eq!(ROWS[0], (0, -1, -1));
        assert_eq!(ROWS[1], (7700, 7800, 7900));
        assert_eq!(ROWS[84], (98822, 98822, 98823));
    }

    #[test]
    fn solo_removes_every_client_effect_duo_and_trio_use_one_value_for_all_slots() {
        for (i, (_, c1, c2)) in ROWS.iter().enumerate() {
            assert_eq!(wanted(i, 1), [-1, -1, -1]);
            assert_eq!(wanted(i, 2), [*c1, *c1, *c1]);
            assert_eq!(wanted(i, 3), [*c2, *c2, *c2]);
            assert_eq!(wanted(i, 0), [*c1, *c2, -1], "vanilla");
        }
    }

    #[test]
    fn a_block_is_recognised_as_vanilla_or_as_ours_and_nothing_else() {
        let mut b = vanilla_block();
        assert!(block_matches(&b, None));
        for i in 0..ROWS.len() {
            let at = i * ROW_SIZE + FIELDS_AT;
            b[at..at + 12].copy_from_slice(&[0xFF; 12]);
        }
        assert!(!block_matches(&b, None));
        assert!(block_matches(&b, Some(1)));
        assert!(!block_matches(&b, Some(2)));
        b[FIELDS_AT] = 0x12;
        assert!(!block_matches(&b, Some(1)), "one odd row refuses the block");
        assert!(!block_matches(&b[..100], None));
    }

    #[test]
    fn the_needle_is_found_inside_a_buffer() {
        let b = vanilla_block();
        let at = ROW_SIZE + FIELDS_AT;
        let mut hay = vec![0xAAu8; 5000];
        hay[1000..1000 + b.len()].copy_from_slice(&b);
        assert_eq!(memscan::find_all(&hay, &needle()), vec![1000 + at]);
    }
}
