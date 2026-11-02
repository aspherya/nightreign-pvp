//! RW-1: a defeated player watches the match: the (invisible, held) dead body is moved next to a player who is still in, so the
//! follow camera shows them; F10 leaves (the defeat screen). Needs `spectate` and a defeat; nothing happens once the match is over.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use super::{dist, Act, MatchBook, MatchOut, MatchRule, Tick, FLAG_LOSE, FLAG_SPECTATE_HINT};
use crate::game::combat_state::Fighter;

/// RW-1: how long a defeated body waits for the hold before it follows anyway (ms).
pub(crate) const SPECTATE_WAIT_MS: u64 = 12_000;
/// RW-1: the spectating body is moved every tick and this far above the watched player's feet (metres). Zero: a body above the floor
/// falls between two moves and the camera bobs up and down at the move rate (user, 2026-10-25).
pub(crate) const SPECTATE_HEIGHT: f32 = 0.0;

/// UI-4: the spectator hint (controls, how to leave) is shown when the watching starts and again this often (ms), so it is not missed
/// and not a nuisance (about one blinking message per 45 s).
pub(crate) const HINT_EVERY_MS: u64 = 45_000;

/// Dev console `spectate 0|1`: overrides the config value live (`2` = the config's).
/// The config's `spectate`, kept for the console.
static SPECTATE_CFG: AtomicBool = AtomicBool::new(true);
static SPECTATE_OVERRIDE: AtomicU8 = AtomicU8::new(2);

pub(super) fn set_cfg(on: bool) {
    SPECTATE_CFG.store(on, Ordering::Relaxed);
}

/// The rule: a defeated body follows a living player.
#[derive(Debug, Default)]
pub(crate) struct Spectate {
    /// RW-1: the player being watched (session id).
    spec_target: Option<u64>,
    /// UI-4: when the spectator hint was last raised.
    hint_at_ms: Option<u64>,
}

impl MatchRule for Spectate {
    fn step(&mut self, t: &Tick, book: &mut MatchBook, out: &mut MatchOut) {
        let (now_ms, world, fighters, local) = (t.now_ms, t.world, t.fighters, t.local);
        let Some(d) = book.defeat.as_mut() else {
            self.spec_target = None;
            return;
        };
        if world.leave_pressed && !d.exit_sent && !book.match_over {
            d.exit_sent = true;
            out.flags.push((FLAG_LOSE, true));
            out.lines.push(
                "night_match: the leave key was pressed: the defeat screen is asked for".into(),
            );
            return;
        }
        // Only a dead body is moved: a downed or standing player (a stale defeat) is never dragged about.
        let dead_now = local.hp == 0 && local.state != 1;
        if !book.spectate || book.match_over || d.exit_sent || !dead_now {
            return;
        }
        // The body is hidden from the others once the comeback's start step is held (about 7 s after the death); before that the
        // death animation plays on their screens and a body jumping to the killer would be seen (user, 2026-10-25). If the hold never
        // comes, follow after SPECTATE_WAIT_MS anyway.
        if !world.death_held && now_ms.saturating_sub(d.at_ms) < SPECTATE_WAIT_MS {
            return;
        }
        let watchable = |id: u64, f: &Fighter| {
            f.id != local.id
                && f.id == id
                && f.hp > 0
                && !book.seen.get(&f.id).is_some_and(|s| s.out(now_ms))
        };
        let here = world.local_pos.map(|p| p.0);
        // Switch: the next / previous watchable player by session id (wraps).
        if world.spec_next || world.spec_prev {
            let mut ids: Vec<u64> = fighters
                .iter()
                .filter(|(_, f)| watchable(f.id, f))
                .map(|(_, f)| f.id)
                .collect();
            ids.sort_unstable();
            if !ids.is_empty() {
                let at = self
                    .spec_target
                    .and_then(|t| ids.iter().position(|i| *i == t));
                let n = ids.len();
                let to = match (at, world.spec_next) {
                    (Some(i), true) => (i + 1) % n,
                    (Some(i), false) => (i + n - 1) % n,
                    (None, _) => 0,
                };
                self.spec_target = Some(ids[to]);
                out.lines.push(format!(
                    "night_match: spectator switched to player {:#X}",
                    ids[to]
                ));
            }
        }
        let pick = self
            .spec_target
            .and_then(|id| fighters.iter().find(|(_, f)| watchable(id, f)))
            .or_else(|| {
                fighters
                    .iter()
                    .filter(|(i, f)| {
                        watchable(f.id, f) && world.positions.get(*i).copied().flatten().is_some()
                    })
                    .min_by(|(i, _), (j, _)| {
                        let dist_to = |k: &usize| {
                            let p = world
                                .positions
                                .get(*k)
                                .copied()
                                .flatten()
                                .unwrap_or([0.0; 3]);
                            here.map_or(0.0, |h| dist(h, p))
                        };
                        dist_to(i).total_cmp(&dist_to(j))
                    })
            });
        let Some((i, f)) = pick else {
            if self.spec_target.take().is_some() {
                out.lines.push("night_match: nobody left to watch".into());
            }
            return;
        };
        let Some(p) = world.positions.get(*i).copied().flatten() else {
            return;
        };
        if self.spec_target != Some(f.id) {
            self.spec_target = Some(f.id);
            out.lines
                .push(format!("night_match: spectating player {:#X}", f.id));
        }
        if self
            .hint_at_ms
            .is_none_or(|at| now_ms.saturating_sub(at) >= HINT_EVERY_MS)
        {
            self.hint_at_ms = Some(now_ms);
            out.flags.push((FLAG_SPECTATE_HINT, true));
        }
        // Next to the watched player (the held body is invisible to the others); the follow camera goes with it.
        out.acts
            .push(Act::Follow([p[0], p[1] + SPECTATE_HEIGHT, p[2]]));
    }

    fn reset(&mut self) {
        *self = Spectate::default();
    }

    fn describe(&self, parts: &mut super::StatusParts) {
        parts.spec_target = self.spec_target;
    }
}

/// Console `spectate [0|1]`: switch the body-moving spectator off or on (so the game's own behaviour of a dead player can be looked at) and report it.
pub(crate) fn spectate_cmd(on: Option<bool>) -> String {
    let configured = SPECTATE_CFG.load(Ordering::Relaxed);
    if let Some(on) = on {
        SPECTATE_OVERRIDE.store(u8::from(on), Ordering::Relaxed);
        tracing::info!("night_match: spectate {}", if on { "ON" } else { "OFF" });
    }
    let now = spectate_now(configured);
    format!(
        "spectate {}: a defeated player's body {}",
        u8::from(now),
        if now {
            "follows a living player"
        } else {
            "stays where it fell"
        }
    )
}

pub(super) fn spectate_now(configured: bool) -> bool {
    match SPECTATE_OVERRIDE.load(Ordering::Relaxed) {
        0 => false,
        1 => true,
        _ => configured,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::super::Match;
    use super::*;
    use crate::game::world::NightFlags;

    #[test]
    fn a_defeated_player_watches_the_nearest_living_player_and_f10_leaves() {
        let nf = NightFlags::default();
        let mut c = Match::battle_royale(1020, 0, true);
        run(
            &mut c,
            0,
            &world(&[up(ME, true), up(P2, false), up(P3, false)], nf),
        );
        let mut w = world(&[dead(ME, true), up(P2, false), up(P3, false)], nf);
        rival_hit(&mut w, 100);
        w.death_held = true;
        w.local_pos = Some(([0.0, 0.0, 0.0], true));
        w.positions[1] = Some([500.0, 10.0, 0.0]);
        w.positions[2] = Some([50.0, 12.0, 5.0]);
        let (acts, _, lines) = run_dead(&mut c, 100, &w);
        assert!(lines.iter().any(|l| l.contains("defeated by a rival")));
        let (acts2, _, lines2) = run(&mut c, 1_000, &w);
        let _ = acts;
        let _ = lines2;
        assert!(
            lines.iter().any(|l| l.contains("spectating player")),
            "{lines:?}"
        );
        assert!(
            acts2.contains(&Act::Follow([50.0, 12.0 + SPECTATE_HEIGHT, 5.0])),
            "the nearer player P3 is watched: {acts2:?}"
        );
        // It stays on the same player while that one is in, even when the other comes closer.
        w.positions[1] = Some([5.0, 0.0, 0.0]);
        let (acts3, _, _) = run(&mut c, 1_200, &w);
        assert!(acts3.contains(&Act::Follow([50.0, 12.0 + SPECTATE_HEIGHT, 5.0])));
        // F10 asks for the defeat screen once.
        w.leave_pressed = true;
        let (_, flags, _) = run(&mut c, 1_400, &w);
        assert_eq!(flags, vec![(FLAG_LOSE, true)]);
        w.leave_pressed = false;
        let (acts4, flags4, _) = run(&mut c, 1_600, &w);
        assert!(
            flags4.is_empty() && acts4.is_empty(),
            "no more moves after leaving"
        );
    }

    #[test]
    fn the_spectator_hint_comes_at_the_start_and_then_every_45_seconds() {
        let nf = NightFlags::default();
        let mut c = Match::battle_royale(1020, 0, true);
        run(
            &mut c,
            0,
            &world(&[up(ME, true), up(P2, false), up(P3, false)], nf),
        );
        let mut w = world(&[dead(ME, true), up(P2, false), up(P3, false)], nf);
        rival_hit(&mut w, 100);
        w.death_held = true;
        w.positions[1] = Some([50.0, 12.0, 5.0]);
        w.positions[2] = Some([500.0, 10.0, 0.0]);
        let count = |flags: &[(u32, bool)]| {
            flags
                .iter()
                .filter(|(f, on)| *f == FLAG_SPECTATE_HINT && *on)
                .count()
        };
        let (_, first, _) = run_dead(&mut c, 100, &w);
        assert_eq!(count(&first), 1, "when the watching starts");
        // One tick a second (a player not seen for a while counts as gone, so the time cannot jump).
        let mut hints_at = Vec::new();
        for k in 1..=(2 * HINT_EVERY_MS / 1_000 + 2) {
            let ms = 100 + k * 1_000;
            if count(&run(&mut c, ms, &w).1) > 0 {
                hints_at.push(ms);
            }
        }
        // `first` was the start of the watching; the next two are one interval apart each (the tick after the previous one is due).
        assert_eq!(hints_at.len(), 2, "{hints_at:?}");
        assert!(hints_at[0] <= 100 + HINT_EVERY_MS + 1_000, "{hints_at:?}");
        assert_eq!(hints_at[1] - hints_at[0], HINT_EVERY_MS, "{hints_at:?}");
    }

    #[test]
    fn the_body_only_follows_once_the_hold_hides_it_or_after_the_wait() {
        let nf = NightFlags::default();
        let mut c = Match::battle_royale(1020, 0, true);
        run(
            &mut c,
            0,
            &world(&[up(ME, true), up(P2, false), up(P3, false)], nf),
        );
        let mut w = world(&[dead(ME, true), up(P2, false), up(P3, false)], nf);
        rival_hit(&mut w, 100);
        w.positions[1] = Some([50.0, 12.0, 5.0]);
        run_dead(&mut c, 100, &w);
        let (acts, _, _) = run(&mut c, 3_000, &w);
        assert!(
            !acts.iter().any(|a| matches!(a, Act::Follow(_))),
            "not yet: the death animation still shows"
        );
        w.death_held = true;
        let (acts, _, _) = run(&mut c, 3_100, &w);
        assert!(acts.iter().any(|a| matches!(a, Act::Follow(_))));
        // No hold at all: it follows after the wait.
        let mut c2 = Match::battle_royale(1020, 0, true);
        run(
            &mut c2,
            0,
            &world(&[up(ME, true), up(P2, false), up(P3, false)], nf),
        );
        w.death_held = false;
        run_dead(&mut c2, 100, &w);
        let (acts, _, _) = run(&mut c2, 100 + SPECTATE_WAIT_MS + 1_000, &w);
        assert!(acts.iter().any(|a| matches!(a, Act::Follow(_))));
    }

    #[test]
    fn the_spectator_switches_to_the_next_and_previous_player_and_wraps() {
        const P4: u64 = 0x6004_FFFF_FFFF;
        let nf = NightFlags::default();
        let mut c = Match::battle_royale(1020, 0, true);
        let alive = [up(ME, true), up(P2, false), up(P3, false), up(P4, false)];
        run(&mut c, 0, &world(&alive, nf));
        let mut w = world(
            &[dead(ME, true), up(P2, false), up(P3, false), up(P4, false)],
            nf,
        );
        rival_hit(&mut w, 100);
        w.death_held = true;
        for (i, x) in [(1, 10.0), (2, 20.0), (3, 30.0)] {
            w.positions[i] = Some([x, 0.0, 0.0]);
        }
        w.local_pos = Some(([0.0, 0.0, 0.0], true));
        run_dead(&mut c, 100, &w);
        let (acts, _, _) = run(&mut c, 1_000, &w);
        assert!(
            acts.contains(&Act::Follow([10.0, SPECTATE_HEIGHT, 0.0])),
            "the nearest first: {acts:?}"
        );
        let mut at = |next: bool, prev: bool, t: u64| {
            w.spec_next = next;
            w.spec_prev = prev;
            let (acts, _, _) = run(&mut c, t, &w);
            acts.iter()
                .find_map(|a| match a {
                    Act::Follow(p) => Some(p[0]),
                    _ => None,
                })
                .unwrap_or(-1.0)
        };
        assert_eq!(at(true, false, 1_100), 20.0);
        assert_eq!(at(true, false, 1_200), 30.0);
        assert_eq!(at(true, false, 1_300), 10.0, "wraps");
        assert_eq!(at(false, true, 1_400), 30.0, "previous wraps back");
        assert_eq!(at(false, false, 1_500), 30.0, "stays");
    }

    #[test]
    fn nothing_is_watched_without_spectate_or_without_a_defeat() {
        let nf = NightFlags::default();
        let mut c = Match::battle_royale(1020, 0, false);
        run(
            &mut c,
            0,
            &world(&[up(ME, true), up(P2, false), up(P3, false)], nf),
        );
        let mut w = world(&[dead(ME, true), up(P2, false), up(P3, false)], nf);
        rival_hit(&mut w, 100);
        w.positions[1] = Some([50.0, 12.0, 5.0]);
        run_dead(&mut c, 100, &w);
        let (acts, _, _) = run(&mut c, 1_000, &w);
        assert!(!acts.iter().any(|a| matches!(a, Act::Follow(_))));
        let mut c2 = Match::battle_royale(1020, 0, true);
        let mut alive = world(&[up(ME, true), up(P2, false), up(P3, false)], nf);
        alive.positions[1] = Some([50.0, 12.0, 5.0]);
        run(&mut c2, 0, &alive);
        let (acts, _, _) = run(&mut c2, 300, &alive);
        assert!(!acts.iter().any(|a| matches!(a, Act::Follow(_))));
    }
}
