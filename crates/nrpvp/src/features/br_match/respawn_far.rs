//! RW-7: after a mob's death, LOCAL is moved to the pool place farthest from the other living players. The pool is recorded on Day 1.
//! FS7-5: every such respawn then starts in the air on a hawk ride above that place ([`Act::HawkRespawn`]).

use std::collections::VecDeque;

use super::{before, dist, Act, MatchBook, MatchOut, MatchRule, StatusParts, Tick, TIDE_FROM};

/// RW-7: LOCAL stands this long after a mob-death respawn before it is moved.
pub(crate) const RESPAWN_MOVE_DELAY_MS: u64 = 3_000;
/// RW-7: no move when LOCAL already is at least this far from every other living player (metres).
pub(crate) const FAR_ENOUGH: f32 = 300.0;
/// Spots are taken while the day-1 clock is before this (players have landed and spread).
const PLACE_EVERY_MS: u64 = 5_000;
/// A place counts only after LOCAL moved slower than [`PLACE_MAX_SPEED`] this long: the start hawk flies at ~17 m/s with the ground
/// byte set (MEMORY_MAP §8), so ride positions in the air must never enter the pool (S1 solo run 2026-10-23: a y 210 pick).
const PLACE_SETTLE_MS: u64 = 2_000;
/// Metres per second (horizontal) between two ticks; a sprint is about 7.
const PLACE_MAX_SPEED: f32 = 9.0;
/// A sampled position is kept only when it is at least this far from every kept one (metres).
pub(crate) const PLACE_MIN_APART: f32 = 100.0;
/// The pool keeps this many positions (the oldest goes first).
pub(crate) const PLACE_POOL: usize = 64;

/// Add `p` to the pool when it is at least [`PLACE_MIN_APART`] from every kept position; true when kept.
pub(crate) fn keep_place(pool: &mut VecDeque<[f32; 3]>, p: [f32; 3]) -> bool {
    if pool.iter().any(|q| dist(*q, p) < PLACE_MIN_APART) {
        return false;
    }
    pool.push_back(p);
    while pool.len() > PLACE_POOL {
        pool.pop_front();
    }
    true
}

/// RW-7: the pool place farthest from every position in `others` (the smallest distance to any of them is the largest);
/// `None` for an empty pool. With no `others` any place is as good: the first.
pub(crate) fn farthest_place(
    pool: &VecDeque<[f32; 3]>,
    others: &[[f32; 3]],
) -> Option<([f32; 3], f32)> {
    pool.iter()
        .map(|p| {
            let d = others
                .iter()
                .map(|o| dist(*p, *o))
                .fold(f32::INFINITY, f32::min);
            (*p, d)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// The rule: record Day-1 places, move a mob-death respawn.
#[derive(Debug, Default)]
pub(crate) struct RespawnFar {
    /// RW-7: Day-1 ground positions, at least [`PLACE_MIN_APART`] apart (ring of [`PLACE_POOL`]).
    places: VecDeque<[f32; 3]>,
    last_place_ms: Option<u64>,
    /// The previous tick's ground position (the speed check) and since when LOCAL has been slow.
    place_prev: Option<(u64, [f32; 3])>,
    slow_since: Option<u64>,
    /// RW-7: since when LOCAL has stood after a mob's death was judged.
    up_since: Option<u64>,
}

impl RespawnFar {
    /// Day-1 ground positions for RW-7's respawn.
    fn record_place(&mut self, t: &Tick) {
        let (now_ms, world, nf, local) = (t.now_ms, t.world, &t.nf, t.local);
        let Some((pos, true)) = world.local_pos else {
            return;
        };
        if nf.day2 || nf.night || nf.night1_done || local.state != 0 || local.hp == 0 {
            self.place_prev = None;
            self.slow_since = None;
            return;
        }
        let slow = self.place_prev.is_some_and(|(t, p)| {
            let dt = now_ms.saturating_sub(t) as f32 / 1000.0;
            let d = ((pos[0] - p[0]).powi(2) + (pos[2] - p[2]).powi(2)).sqrt();
            dt > 0.0 && d / dt <= PLACE_MAX_SPEED
        });
        self.place_prev = Some((now_ms, pos));
        if !slow {
            self.slow_since = None;
            return;
        }
        let since = *self.slow_since.get_or_insert(now_ms);
        if now_ms.saturating_sub(since) < PLACE_SETTLE_MS {
            return;
        }
        if self
            .last_place_ms
            .is_some_and(|t| now_ms.saturating_sub(t) < PLACE_EVERY_MS)
        {
            return;
        }
        self.last_place_ms = Some(now_ms);
        keep_place(&mut self.places, pos);
    }
}

impl MatchRule for RespawnFar {
    fn step(&mut self, t: &Tick, book: &mut MatchBook, out: &mut MatchOut) {
        // Moved here from the head of the old `NightCore::step`: it reads no book state, and nothing before the respawn below reads
        // `places`, so the order is equivalent.
        self.record_place(t);
        let (now_ms, world, nf, fighters, local) = (t.now_ms, t.world, &t.nf, t.fighters, t.local);
        if !book.respawn_pending {
            return;
        }
        if !(local.state == 0 && local.hp > 0) {
            self.up_since = None;
            return;
        }
        let since = *self.up_since.get_or_insert(now_ms);
        if now_ms.saturating_sub(since) < RESPAWN_MOVE_DELAY_MS {
            return;
        }
        book.respawn_pending = false;
        self.up_since = None;
        // After the first circle of Day 2 starts closing, a Day-1 place may lie outside it: the game's own respawn point
        // (inside the circle) stays.
        let circle_open = nf.day2
            && world
                .clock
                .is_none_or(|c| !before(c.time, TIDE_FROM) || c.time.h >= 20);
        if circle_open {
            out.acts.push(Act::HawkRespawn(None));
            out.lines.push(
                "night_match: respawn left to the game (the circle is closing); on the hawk from there"
                    .into(),
            );
            return;
        }
        let others: Vec<[f32; 3]> = fighters
            .iter()
            .filter(|(_, f)| !f.local && !book.seen.get(&f.id).is_some_and(|s| s.out(now_ms)))
            .filter_map(|(i, _)| world.positions.get(*i).copied().flatten())
            .collect();
        let here = world.local_pos.map(|p| p.0);
        let here_far = here.map(|h| {
            others
                .iter()
                .map(|o| dist(h, *o))
                .fold(f32::INFINITY, f32::min)
        });
        if here_far.is_some_and(|d| d >= FAR_ENOUGH) {
            out.acts.push(Act::HawkRespawn(None));
            out.lines.push(format!(
                "night_match: respawned {:.0} m from the nearest player; no move",
                here_far.unwrap_or(0.0)
            ));
            return;
        }
        match farthest_place(&self.places, &others) {
            Some((p, d)) if here_far.is_none_or(|h| d > h) => {
                out.acts.push(Act::HawkRespawn(Some(p)));
                out.lines.push(format!(
                    "night_match: respawn moved to ({:.1}, {:.1}, {:.1}), {d:.0} m from the nearest player (was {})",
                    p[0],
                    p[1],
                    p[2],
                    here_far.map_or("?".into(), |h| format!("{h:.0} m"))
                ));
            }
            _ => {
                out.acts.push(Act::HawkRespawn(None));
                out.lines.push(
                    "night_match: respawn stays where the game put it (no better place recorded)"
                        .into(),
                );
            }
        }
    }

    fn reset(&mut self) {
        *self = RespawnFar::default();
    }

    fn describe(&self, parts: &mut StatusParts) {
        parts.places = self.places.len();
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;
    use crate::game::combat_state::{HitKind, LocalHits};
    use crate::game::world::NightFlags;

    #[test]
    fn a_mob_death_respawn_moves_to_the_farthest_recorded_place() {
        let mut c = core();
        c.rule_mut::<RespawnFar>().places.push_back([0.0, 0.0, 0.0]);
        c.rule_mut::<RespawnFar>()
            .places
            .push_back([1000.0, 0.0, 0.0]);
        let w0 = world(&[up(ME, true), up(P2, false)], NightFlags::default());
        run(&mut c, 0, &w0);
        let mut w = world(&[dead(ME, true), up(P2, false)], NightFlags::default());
        w.local_hits = LocalHits {
            last: Some((HitKind::Mob, 10)),
            rival_age_ms: None,
            rival_id: None,
        };
        run_dead(&mut c, 100, &w);
        // Standing again next to the other player (P2 at x=10): the far place is x=1000.
        let mut back = world(&[up(ME, true), up(P2, false)], NightFlags::default());
        back.local_pos = Some(([20.0, 0.0, 0.0], true));
        back.positions[1] = Some([10.0, 0.0, 0.0]);
        run(&mut c, 12_000, &back);
        let (acts, _, lines) = run(&mut c, 12_000 + RESPAWN_MOVE_DELAY_MS, &back);
        assert_eq!(
            acts,
            vec![Act::HawkRespawn(Some([1000.0, 0.0, 0.0]))],
            "{lines:?}"
        );
    }

    #[test]
    fn a_respawn_already_far_from_everyone_stays() {
        let mut c = core();
        c.rule_mut::<RespawnFar>()
            .places
            .push_back([1000.0, 0.0, 0.0]);
        let w0 = world(&[up(ME, true), up(P2, false)], NightFlags::default());
        run(&mut c, 0, &w0);
        let mut w = world(&[dead(ME, true), up(P2, false)], NightFlags::default());
        w.local_hits = LocalHits::default();
        run_dead(&mut c, 100, &w);
        let mut back = world(&[up(ME, true), up(P2, false)], NightFlags::default());
        back.local_pos = Some(([0.0, 0.0, 0.0], true));
        back.positions[1] = Some([900.0, 0.0, 0.0]);
        run(&mut c, 12_000, &back);
        let (acts, _, _) = run(&mut c, 15_100, &back);
        assert_eq!(
            acts,
            vec![Act::HawkRespawn(None)],
            "FS7-5: no move, but on the hawk"
        );
    }

    #[test]
    fn places_are_kept_apart_and_the_pool_is_a_ring() {
        let mut pool = VecDeque::new();
        assert!(keep_place(&mut pool, [0.0, 0.0, 0.0]));
        assert!(!keep_place(&mut pool, [50.0, 0.0, 0.0]));
        assert!(keep_place(&mut pool, [150.0, 0.0, 0.0]));
        for i in 0..100 {
            keep_place(&mut pool, [1000.0 + 200.0 * i as f32, 0.0, 0.0]);
        }
        assert_eq!(pool.len(), PLACE_POOL);
    }

    #[test]
    fn the_farthest_place_maximises_the_distance_to_the_nearest_player() {
        let mut pool = VecDeque::new();
        pool.push_back([0.0, 0.0, 0.0]);
        pool.push_back([500.0, 0.0, 0.0]);
        pool.push_back([1000.0, 0.0, 0.0]);
        let others = [[0.0, 0.0, 10.0], [1000.0, 0.0, 10.0]];
        let (p, d) = farthest_place(&pool, &others).expect("a place");
        assert_eq!(p, [500.0, 0.0, 0.0]);
        assert!((d - 500.0).abs() < 1.0);
        assert!(farthest_place(&VecDeque::new(), &others).is_none());
    }
}
