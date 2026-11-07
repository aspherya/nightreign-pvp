//! BOT-1 spread (BOTS.md §2b, lever 4 used once): the game spawns every bot next to the host, so each kept bot is moved once to an
//! enemy camp far from LOCAL, each bot in its own direction. There it fights the camp (farming) instead of LOCAL. The camp is found by
//! walking WorldChrMan's character sets (`[WorldChrMan+0x170B8 + i*8]`, i = 0..=0x84, MEMORY_MAP §15) for living enemies (team 6) and
//! reading their world position like a player's (`spawn::world_position_of`). Whether the bot's own logic (follow the host, logic
//! 30000) walks or warps it back is what the `bots-match` scenario watches (distance in `bots_list`).
//! Log lines start with `bots:`.

use super::layout::{CHRSET_CAPACITY, CHRSET_ENTRIES, CHRSET_ENTRY_SIZE, CHR_HANDLE, CHR_TEAM};
use super::mem;

/// WorldChrMan -> character-set pointers (OFF-5 static reading of the handle lookup `exe+0x518F30`, game 1.03.3).
const WCM_CHRSETS: usize = 0x170B8;
const WCM_CHRSET_COUNT: usize = 0x85;
const MAX_CAPACITY: u32 = 0x2000;
/// The enemy team value (MEMORY_MAP §4; the small-base invaders and camp mobs read 6, BOT-0 lab).
const ENEMY_TEAM: u8 = 6;
/// A camp this far from LOCAL (horizontal metres) is a candidate; the closest to [`IDEAL`] in the bot's direction wins.
pub(crate) const MIN_DIST: f32 = 120.0;
pub(crate) const MAX_DIST: f32 = 380.0;
pub(crate) const IDEAL: f32 = 220.0;
/// No more than this height difference (cliffs, caves).
const MAX_DY: f32 = 60.0;
/// The bot is put this far above the enemy and this far beside it, so it lands on the ground next to the camp.
const LIFT: f32 = 2.0;
const BESIDE: f32 = 3.0;
/// At most this many enemies are looked at (one physics read each).
const MAX_SCAN: usize = 2500;
/// Two bots never get camps closer than this to each other (Leonardo 2026-11-06: bots start in separate places).
pub(crate) const APART: f32 = 80.0;
/// The camps given out lately (the first spread of an expedition takes a few seconds); forgotten when no bot was sent for this long.
const USED_TTL: std::time::Duration = std::time::Duration::from_secs(120);
static USED: std::sync::Mutex<(Vec<[f32; 3]>, Option<std::time::Instant>)> =
    std::sync::Mutex::new((Vec::new(), None));

fn hdist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Sector of `p` around `c` among `n` equal sectors (0..n).
fn sector_of(c: [f32; 3], p: [f32; 3], n: usize) -> usize {
    let a = (p[2] - c[2])
        .atan2(p[0] - c[0])
        .rem_euclid(std::f32::consts::TAU);
    ((a / std::f32::consts::TAU * n as f32) as usize).min(n.saturating_sub(1))
}

/// The camp for bot `sector` of `sectors` (pure; unit tested): in range, at least [`APART`] from every camp in `used`, and in its sector,
/// closest to [`IDEAL`]; else any such camp in range.
pub(crate) fn pick(
    local: [f32; 3],
    cands: &[[f32; 3]],
    sector: usize,
    sectors: usize,
    used: &[[f32; 3]],
) -> Option<[f32; 3]> {
    let n = sectors.max(1);
    let score = |p: &[f32; 3]| (hdist(local, *p) - IDEAL).abs();
    let ok = |p: &&[f32; 3]| {
        let d = hdist(local, **p);
        (MIN_DIST..=MAX_DIST).contains(&d)
            && (p[1] - local[1]).abs() <= MAX_DY
            && used.iter().all(|u| hdist(*u, **p) >= APART)
    };
    let best = |it: &mut dyn Iterator<Item = &[f32; 3]>| {
        it.min_by(|a, b| score(a).total_cmp(&score(b))).copied()
    };
    best(
        &mut cands
            .iter()
            .filter(ok)
            .filter(|p| sector_of(local, **p, n) == sector % n),
    )
    .or_else(|| best(&mut cands.iter().filter(ok)))
}

/// World positions of living enemies (team 6) in every character set.
fn enemy_positions(wcm: usize, local: usize, base: usize) -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    let mut looked = 0;
    for i in 0..WCM_CHRSET_COUNT {
        let Some(set) = mem::read::<usize>(wcm + WCM_CHRSETS + i * 8).filter(|&p| mem::is_ptr(p))
        else {
            continue;
        };
        let Some(cap) =
            mem::read::<u32>(set + CHRSET_CAPACITY).filter(|&c| c > 0 && c <= MAX_CAPACITY)
        else {
            continue;
        };
        let Some(entries) = mem::read::<usize>(set + CHRSET_ENTRIES).filter(|&p| mem::is_ptr(p))
        else {
            continue;
        };
        for k in 0..cap as usize {
            if looked >= MAX_SCAN {
                return out;
            }
            let Some(chr) =
                mem::read::<usize>(entries + k * CHRSET_ENTRY_SIZE).filter(|&c| mem::is_ptr(c))
            else {
                continue;
            };
            if mem::read::<u64>(chr + CHR_HANDLE).is_none()
                || mem::read::<u8>(chr + CHR_TEAM) != Some(ENEMY_TEAM)
            {
                continue;
            }
            let alive = super::views::ChrIns::new(&super::gamemem::RealMem, chr)
                .hp()
                .is_some_and(|h| h > 0);
            looked += 1;
            if !alive {
                continue;
            }
            if let Some(p) = super::spawn::world_position_of(chr, local, base) {
                out.push(p);
            }
        }
    }
    out
}

/// Move bot `chr` (still `handle`) to a camp in direction `sector` of `sectors`. Main loop only (reads many characters).
pub(crate) fn spread(
    wcm: usize,
    local: usize,
    base: usize,
    chr: usize,
    handle: u64,
    sector: usize,
    sectors: usize,
) -> Result<String, &'static str> {
    if mem::read::<u64>(chr + CHR_HANDLE) != Some(handle) {
        return Err("the bot is gone (handle changed)");
    }
    let me = super::spawn::world_position(local, base)
        .ok_or("LOCAL position unreadable")?
        .0;
    let camps = enemy_positions(wcm, local, base);
    let mut used = USED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if used.1.is_none_or(|t| t.elapsed() > USED_TTL) {
        used.0.clear();
    }
    // Apart from the others if possible; a crowded map still gets a camp.
    let camp = pick(me, &camps, sector, sectors, &used.0)
        .or_else(|| pick(me, &camps, sector, sectors, &[]))
        .ok_or("no enemy camp in range")?;
    used.0.push(camp);
    used.1 = Some(std::time::Instant::now());
    drop(used);
    // Beside the enemy, on the side facing LOCAL, a little higher.
    let d = hdist(me, camp).max(1.0);
    let to = [
        camp[0] + (me[0] - camp[0]) / d * BESIDE,
        camp[1] + LIFT,
        camp[2] + (me[2] - camp[2]) / d * BESIDE,
    ];
    let from = super::spawn::teleport_other(chr, local, base, to)?;
    Ok(format!(
        "bot 0x{handle:X} moved from ({:.0}, {:.0}, {:.0}) to a camp {:.0} m away, sector {} of {} ({} living enemies seen)",
        from[0],
        from[1],
        from[2],
        hdist(me, to),
        sector + 1,
        sectors,
        camps.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_camp_in_the_bots_direction_and_range_is_picked() {
        let me = [0.0, 0.0, 0.0];
        let cands = [
            [50.0, 0.0, 0.0],     // too close
            [230.0, 0.0, 0.0],    // east, sector 0 of 4
            [0.0, 0.0, 200.0],    // north (+z), sector 1 of 4
            [-210.0, 5.0, 0.0],   // west, sector 2
            [0.0, 200.0, -220.0], // south but 200 m up: refused
        ];
        assert_eq!(pick(me, &cands, 0, 4, &[]), Some([230.0, 0.0, 0.0]));
        assert_eq!(pick(me, &cands, 1, 4, &[]), Some([0.0, 0.0, 200.0]));
        assert_eq!(pick(me, &cands, 2, 4, &[]), Some([-210.0, 5.0, 0.0]));
        // Nothing in sector 3: the best in range anywhere (closest to 220 m).
        assert_eq!(pick(me, &cands, 3, 4, &[]), Some([230.0, 0.0, 0.0]));
        // ...unless that camp is taken: then the next best that is far enough from the taken ones.
        assert_eq!(
            pick(me, &cands, 3, 4, &[[230.0, 0.0, 0.0]]),
            Some([-210.0, 5.0, 0.0])
        );
        assert_eq!(
            pick(me, &cands, 3, 4, &[[230.0, 0.0, 30.0]]),
            Some([-210.0, 5.0, 0.0]),
            "within 80 m of a taken camp"
        );
        assert_eq!(pick(me, &[[10.0, 0.0, 0.0]], 0, 1, &[]), None);
    }
}
