//! GRACE-1: no Site of Grace during PvP combat. While LOCAL is engaged with a rival, game flag 7525 ("all bonfires disabled", MEMORY_MAP
//! §26, game 1.03.3) is ON on this game: a grace gives no HP and no flasks. The flag is per game, so every game locks its own player.
//!
//! **Engaged** when, within the last `book.grace_ms` (config `grace_combat_secs`, default [`DEFAULT_GRACE_COMBAT_SECS`](crate::config)):
//! a rival hit LOCAL (`world.local_hits.rival_age_ms`), or LOCAL probably hit a rival: a rival's HP fell on this game's copy while it was
//! within [`ENGAGE_RANGE_M`] of LOCAL (a teammate's hit counts too, accepted).
//! **Ends early** when no living rival is within [`RELEASE_RANGE_M`] (the opponent died or is far away); only a newer hit engages again.
//! **Never engaged** when LOCAL is defeated, the match is decided or no rival is in the list. Always OFF when leaving the expedition.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{dist, Act, MatchBook, MatchOut, MatchRule, StatusParts, Tick};

/// The game's event flag "all bonfires disabled" (MEMORY_MAP §26).
pub(crate) const GRACE_OFF_FLAG: u32 = 7525;
/// A rival whose HP falls this close to LOCAL was probably hit by LOCAL.
pub(crate) const ENGAGE_RANGE_M: f32 = 30.0;
/// With no living rival this close the fight is over.
pub(crate) const RELEASE_RANGE_M: f32 = 100.0;
/// After an early release a hit this close to the release (tick jitter) is still the old one.
const SETTLE_SLACK_MS: u64 = 300;
/// "No living rival near" must hold this long before the fight counts as over (one bad read of the player list, as when the game
/// reshuffles it, must not end it; seen in `mi-grace-local`, 2026-11-03).
const RELEASE_HOLD_MS: u64 = 1500;

/// The lock is ON (console `grace_lock`; the main loop writes, the pipe thread reads).
static LOCK_ON: AtomicBool = AtomicBool::new(false);

/// Console `grace_lock`.
pub(crate) fn status_line() -> String {
    format!(
        "grace_lock: {} (flag {GRACE_OFF_FLAG})",
        if LOCK_ON.load(Ordering::Relaxed) {
            "ON, Sites of Grace are off"
        } else {
            "off"
        }
    )
}

/// The rule: the grace lock.
#[derive(Debug, Default)]
pub(crate) struct GraceLock {
    on: bool,
    /// Match clock (ms) of the last fight sign, and what it was.
    last_fight: Option<(u64, String)>,
    /// A fight released early at this time; hits up to then are settled.
    settled_at: Option<u64>,
    /// Each rival's HP at the last tick.
    hp: BTreeMap<u64, u32>,
    /// Since when no living rival has been near.
    quiet_since: Option<u64>,
}

impl GraceLock {
    fn fight(&mut self, at: u64, why: String) {
        if self.last_fight.as_ref().is_none_or(|(t, _)| at >= *t) {
            self.last_fight = Some((at, why));
        }
    }

    /// Switch the lock; the act and the log line only on a change.
    fn turn(&mut self, on: bool, why: &str, out: &mut MatchOut) {
        if on == self.on {
            return;
        }
        self.on = on;
        LOCK_ON.store(on, Ordering::Relaxed);
        out.acts.push(Act::GameFlag(GRACE_OFF_FLAG, on));
        out.lines.push(format!(
            "grace_lock: {} ({why})",
            if on { "ON" } else { "OFF" }
        ));
    }
}

impl MatchRule for GraceLock {
    fn step(&mut self, t: &Tick, book: &mut MatchBook, out: &mut MatchOut) {
        let (now, window) = (t.now_ms, book.grace_ms);
        if window == 0 {
            self.turn(false, "grace_combat_secs is 0", out);
            return;
        }
        let rivals: Vec<(usize, crate::game::combat_state::Fighter)> = t
            .fighters
            .iter()
            .filter(|(_, f)| !f.local && f.team != t.local.team)
            .copied()
            .collect();
        let pos = |i: usize| t.world.positions.get(i).copied().flatten();
        let here = t.world.local_pos.map(|p| p.0);
        // LOCAL hit a rival: its HP fell on this copy while it stood near.
        let seen: BTreeMap<u64, u32> = rivals.iter().map(|(_, f)| (f.id, f.hp)).collect();
        for (i, f) in &rivals {
            let Some(&before) = self.hp.get(&f.id) else {
                continue;
            };
            if f.hp < before {
                if let (Some(h), Some(p)) = (here, pos(*i)) {
                    let d = dist(h, p);
                    if d <= ENGAGE_RANGE_M {
                        self.fight(
                            now,
                            format!("I hit a rival: HP {before} -> {} at {d:.0} m", f.hp),
                        );
                    }
                }
            }
        }
        self.hp = seen;
        // A rival hit LOCAL.
        if let Some(age) = t.world.local_hits.rival_age_ms {
            let at = now.saturating_sub(age);
            let fresh = self
                .settled_at
                .is_none_or(|s| at > s.saturating_add(SETTLE_SLACK_MS));
            if age <= window && fresh {
                self.fight(at, "a rival hit me".into());
            }
        }
        // Never engaged.
        let never = if book.defeat.is_some() {
            Some("LOCAL is defeated")
        } else if book.match_over {
            Some("the match is decided")
        } else if rivals.is_empty() {
            Some("no rival seen")
        } else {
            None
        };
        if let Some(why) = never {
            self.last_fight = None;
            self.turn(false, why, out);
            return;
        }
        let Some((at, why)) = self.last_fight.clone() else {
            self.turn(false, "no fight", out);
            return;
        };
        if now.saturating_sub(at) > window {
            self.last_fight = None;
            self.turn(false, &format!("{} s without a fight", window / 1000), out);
            return;
        }
        // Living = HP above 0 (a downed rival is at 0 HP and the fight is over). A rival with an unknown place may be near: no early end then.
        let living: Vec<(usize, u32)> = rivals
            .iter()
            .filter(|(_, f)| f.hp > 0)
            .map(|(i, f)| (*i, f.hp))
            .collect();
        let near = living.iter().any(|(i, _)| match (here, pos(*i)) {
            (Some(h), Some(p)) => dist(h, p) <= RELEASE_RANGE_M,
            _ => true,
        });
        if !near {
            let since = *self.quiet_since.get_or_insert(now);
            if now.saturating_sub(since) >= RELEASE_HOLD_MS {
                self.last_fight = None;
                self.quiet_since = None;
                self.settled_at = Some(now);
                let nearest = rivals
                    .iter()
                    .filter_map(|(i, f)| Some((f.hp, dist(here?, pos(*i)?))))
                    .map(|(hp, d)| format!("HP {hp} at {d:.0} m"))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.turn(
                    false,
                    &format!(
                        "no living rival within {RELEASE_RANGE_M:.0} m; {} rival(s), {} living; seen: [{nearest}]",
                        rivals.len(),
                        living.len()
                    ),
                    out,
                );
            }
            return;
        }
        self.quiet_since = None;
        self.turn(true, &why, out);
    }

    fn leave(&mut self, out: &mut MatchOut) {
        self.turn(false, "left the expedition", out);
    }

    fn reset(&mut self) {
        *self = GraceLock::default();
        LOCK_ON.store(false, Ordering::Relaxed);
    }

    fn describe(&self, parts: &mut StatusParts) {
        parts.grace_lock = self.on;
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::super::Match;
    use super::*;
    use crate::game::world::NightFlags;

    fn nf() -> NightFlags {
        NightFlags::default()
    }

    fn on(c: &mut Match, ms: u64, w: &crate::game::world::World) -> Vec<Act> {
        run(c, ms, w).0
    }

    fn lock() -> Match {
        let mut c = core();
        c.book.grace_ms = 15_000;
        c
    }

    /// LOCAL and one rival 10 m away.
    fn duel(rival_hp: u32) -> crate::game::world::World {
        let mut w = world(&[up(ME, true), fighter(P2, false, 0, rival_hp)], nf());
        w.positions[0] = Some([10.0, 20.0, 30.0]);
        w.positions[1] = Some([20.0, 20.0, 30.0]);
        w
    }

    const ON: Act = Act::GameFlag(GRACE_OFF_FLAG, true);
    const OFF: Act = Act::GameFlag(GRACE_OFF_FLAG, false);

    #[test]
    fn a_rival_hit_locks_and_the_timer_releases() {
        let mut c = lock();
        assert!(on(&mut c, 0, &duel(240)).is_empty());
        let mut w = duel(240);
        rival_hit(&mut w, 100);
        assert_eq!(on(&mut c, 1000, &w), vec![ON]);
        // Still engaged: nothing more (a change only).
        let mut w2 = duel(240);
        rival_hit(&mut w2, 5100);
        assert!(on(&mut c, 6000, &w2).is_empty());
        // The hit is 15.1 s old.
        let mut old = duel(240);
        rival_hit(&mut old, 15_100);
        assert_eq!(on(&mut c, 16_000, &old), vec![OFF]);
        assert!(on(&mut c, 17_000, &old).is_empty());
    }

    #[test]
    fn my_hit_on_a_near_rival_locks_a_far_one_does_not() {
        let mut c = lock();
        on(&mut c, 0, &duel(240));
        assert_eq!(on(&mut c, 100, &duel(200)), vec![ON]);
        let mut c = lock();
        let far = |hp| {
            let mut w = duel(hp);
            w.positions[1] = Some([60.0, 20.0, 30.0]);
            w
        };
        on(&mut c, 0, &far(240));
        assert!(on(&mut c, 100, &far(200)).is_empty());
    }

    #[test]
    fn the_rival_dying_ends_it_early_and_an_old_hit_does_not_bring_it_back() {
        let mut c = lock();
        on(&mut c, 0, &duel(240));
        let mut w = duel(240);
        rival_hit(&mut w, 100);
        assert_eq!(on(&mut c, 1000, &w), vec![ON]);
        // The rival is dead and no other is near: off at once.
        let mut dead = duel(0);
        rival_hit(&mut dead, 1100);
        assert!(on(&mut c, 2000, &dead).is_empty(), "held for a moment");
        assert_eq!(on(&mut c, 3600, &dead), vec![OFF]);
        let mut later = duel(0);
        rival_hit(&mut later, 2100);
        assert!(on(&mut c, 4000, &later).is_empty());
        // A new hit after the release engages again (the rival is alive again for the test).
        let mut again = duel(240);
        rival_hit(&mut again, 100);
        assert_eq!(on(&mut c, 5000, &again), vec![ON]);
    }

    #[test]
    fn one_bad_tick_does_not_release() {
        let mut c = lock();
        on(&mut c, 0, &duel(240));
        let mut w = duel(240);
        rival_hit(&mut w, 100);
        assert_eq!(on(&mut c, 1000, &w), vec![ON]);
        let mut glitch = duel(0);
        rival_hit(&mut glitch, 1100);
        assert!(on(&mut c, 2000, &glitch).is_empty());
        let mut back = duel(240);
        rival_hit(&mut back, 1200);
        assert!(on(&mut c, 2100, &back).is_empty());
        // Still locked (a later bad stretch must hold again from its own start).
        assert!(on(&mut c, 3700, &glitch).is_empty());
    }

    #[test]
    fn a_rival_that_ran_off_ends_it_but_a_second_near_one_keeps_it() {
        let mut c = lock();
        let mut w = duel(240);
        rival_hit(&mut w, 100);
        w.positions[1] = Some([500.0, 20.0, 30.0]);
        assert!(on(&mut c, 1000, &w).is_empty(), "the only rival is far");
        let mut c = lock();
        let mut w = world(&[up(ME, true), up(P2, false), up(P3, false)], nf());
        w.positions[0] = Some([0.0; 3]);
        w.positions[1] = Some([500.0, 0.0, 0.0]);
        w.positions[2] = Some([90.0, 0.0, 0.0]);
        rival_hit(&mut w, 100);
        assert_eq!(on(&mut c, 1000, &w), vec![ON]);
    }

    #[test]
    fn no_fight_no_rival_defeat_and_a_decided_match_never_lock() {
        let mut c = lock();
        assert!(on(&mut c, 0, &duel(240)).is_empty());
        assert!(on(&mut c, 20_000, &duel(240)).is_empty());
        // Alone: a stale hit still locks nothing.
        let mut alone = world(&[up(ME, true)], nf());
        rival_hit(&mut alone, 10);
        assert!(on(&mut lock(), 0, &alone).is_empty());
        // Defeated for good (HP 0 after a rival hit): the lock goes off.
        let mut c = lock();
        let mut w = duel(240);
        rival_hit(&mut w, 10);
        assert_eq!(on(&mut c, 0, &w), vec![ON]);
        let mut dead_w = world(&[dead(ME, true), up(P2, false)], nf());
        dead_w.positions[0] = Some([10.0, 20.0, 30.0]);
        dead_w.positions[1] = Some([20.0, 20.0, 30.0]);
        rival_hit(&mut dead_w, 10);
        let (acts, _, _) = run_dead(&mut c, 100, &dead_w);
        assert!(acts.contains(&OFF), "{acts:?}");
    }

    #[test]
    fn locked_then_the_match_is_decided_or_local_is_defeated_releases() {
        let mut c = lock();
        let mut w = duel(240);
        rival_hit(&mut w, 100);
        assert_eq!(on(&mut c, 1000, &w), vec![ON]);
        c.book.match_over = true;
        assert_eq!(on(&mut c, 1100, &w), vec![OFF]);
    }

    #[test]
    fn leaving_the_expedition_puts_the_flag_back_and_off_means_off() {
        let mut c = lock();
        let mut w = duel(240);
        rival_hit(&mut w, 100);
        assert_eq!(on(&mut c, 1000, &w), vec![ON]);
        let mut rt = w;
        rt.place = Some(crate::game::views::Place::Roundtable);
        assert!(on(&mut c, 2000, &rt).contains(&OFF));
        // Never on: nothing extra on leaving.
        let mut c = lock();
        on(&mut c, 0, &duel(240));
        let mut rt = duel(240);
        rt.place = Some(crate::game::views::Place::Roundtable);
        assert!(!on(&mut c, 100, &rt).contains(&OFF));
        // grace_combat_secs 0: the rule is off.
        let mut c = core();
        let mut w = duel(240);
        rival_hit(&mut w, 100);
        assert!(on(&mut c, 1000, &w).is_empty());
    }
}
