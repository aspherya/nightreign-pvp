//! KF-1: what the killfeed hears from this game: LOCAL went down by a rival's hit, or LOCAL is out for good ([`super::defeat::Defeat`] judged it,
//! so the feed and the match rules can never disagree about who is out or who did it). Only this game can name the attacker.

use super::defeat::Cause;
use super::{Act, MatchBook, MatchOut, MatchRule, Tick};
use crate::features::killfeed::{short_id, KillEvent, KillKind};

/// HP reaches 0 up to 100 ms before the downed byte, and the hit log is written a tick after the drop: the attacker is read this long after
/// the downed state began.
const SETTLE_MS: u64 = 200;
/// A rival's hit this recent when LOCAL goes down is what put it down.
const DOWN_WINDOW_MS: u64 = 2_000;

/// The rule. After [`super::defeat::Defeat`] in the list.
#[derive(Debug, Default)]
pub(crate) struct KillReport {
    down_since: Option<u64>,
    down_judged: bool,
    defeat_sent: bool,
    /// SB: the mob deaths already reported (`MatchBook::mob_deaths`).
    mob_deaths_sent: u32,
}

impl MatchRule for KillReport {
    fn step(&mut self, t: &Tick, book: &mut MatchBook, out: &mut MatchOut) {
        let hits = t.world.local_hits;
        let victim = short_id(t.local.id);
        let killer = hits.rival_id.map(short_id);
        if t.local.state == 1 {
            let since = *self.down_since.get_or_insert(t.now_ms);
            if !self.down_judged && t.now_ms.saturating_sub(since) >= SETTLE_MS {
                self.down_judged = true;
                if hits.rival_age_ms.is_some_and(|a| a <= DOWN_WINDOW_MS) {
                    out.acts.push(Act::KillFeed(KillEvent {
                        kind: KillKind::Downed,
                        victim,
                        killer,
                    }));
                }
            }
        } else {
            self.down_since = None;
            self.down_judged = false;
        }
        // SB: a death to a mob or a fall (the game respawns LOCAL) is a death on the scoreboard; the feed shows no line for it.
        while self.mob_deaths_sent < book.mob_deaths {
            self.mob_deaths_sent += 1;
            out.acts.push(Act::KillFeed(KillEvent {
                kind: KillKind::Died,
                victim,
                killer: None,
            }));
        }
        match book.defeat {
            Some(d) if !self.defeat_sent => {
                self.defeat_sent = true;
                let (kind, killer) = match d.cause {
                    Cause::Rival => (KillKind::Killed, killer),
                    Cause::Tide => (KillKind::Tide, None),
                };
                out.acts.push(Act::KillFeed(KillEvent {
                    kind,
                    victim,
                    killer,
                }));
            }
            None => self.defeat_sent = false,
            _ => {}
        }
    }

    fn reset(&mut self) {
        *self = KillReport::default();
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;
    use crate::game::combat_state::{Fighter, HitKind, LocalHits};
    use crate::game::world::NightFlags;

    fn down(id: u64, local: bool) -> Fighter {
        Fighter {
            state: 1,
            ..up(id, local)
        }
    }

    fn by_rival(age: u64) -> LocalHits {
        LocalHits {
            last: Some((HitKind::Rival, age)),
            rival_age_ms: Some(age),
            rival_id: Some(P2),
        }
    }

    fn reports(acts: &[Act]) -> Vec<KillEvent> {
        acts.iter()
            .filter_map(|a| match a {
                Act::KillFeed(e) => Some(*e),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn going_down_by_a_rival_is_reported_once_after_the_settle_time() {
        let mut c = core();
        let nf = NightFlags::default();
        let mut w = world(&[down(ME, true), up(P2, false)], nf);
        w.local_hits = by_rival(50);
        let (acts, _, _) = run(&mut c, 0, &w);
        assert!(reports(&acts).is_empty(), "not settled yet");
        let (acts, _, _) = run(&mut c, 200, &w);
        assert_eq!(
            reports(&acts),
            [KillEvent {
                kind: KillKind::Downed,
                victim: short_id(ME),
                killer: Some(short_id(P2)),
            }]
        );
        let (acts, _, _) = run(&mut c, 400, &w);
        assert!(reports(&acts).is_empty(), "once");
    }

    #[test]
    fn going_down_by_a_mob_is_no_line() {
        let mut c = core();
        let mut w = world(&[down(ME, true), up(P2, false)], NightFlags::default());
        w.local_hits = LocalHits {
            last: Some((HitKind::Mob, 50)),
            rival_age_ms: None,
            rival_id: None,
        };
        run(&mut c, 0, &w);
        let (acts, _, _) = run(&mut c, 300, &w);
        assert!(reports(&acts).is_empty());
    }

    #[test]
    fn up_again_and_down_again_is_a_second_line() {
        let mut c = core();
        let nf = NightFlags::default();
        let mut w = world(&[down(ME, true), up(P2, false)], nf);
        w.local_hits = by_rival(50);
        run(&mut c, 0, &w);
        assert_eq!(reports(&run(&mut c, 200, &w).0).len(), 1);
        let mut u = world(&[up(ME, true), up(P2, false)], nf);
        u.local_hits = by_rival(100);
        run(&mut c, 300, &u);
        run(&mut c, 400, &w);
        assert_eq!(reports(&run(&mut c, 600, &w).0).len(), 1);
    }

    #[test]
    fn a_defeat_by_a_rival_names_the_last_one_who_hit() {
        let mut c = core();
        let nf = NightFlags::default();
        run(&mut c, 0, &world(&[up(ME, true), up(P2, false)], nf));
        let mut w = world(&[dead(ME, true), up(P2, false)], nf);
        w.local_hits = by_rival(1_000);
        let (acts, _, _) = run_dead(&mut c, 100, &w);
        assert_eq!(
            reports(&acts),
            [KillEvent {
                kind: KillKind::Killed,
                victim: short_id(ME),
                killer: Some(short_id(P2)),
            }]
        );
        let (acts, _, _) = run(&mut c, 5_000, &w);
        assert!(reports(&acts).is_empty(), "once");
    }

    #[test]
    fn a_mobs_kill_is_reported_as_a_death_only() {
        let mut c = core();
        let nf = NightFlags::default();
        run(&mut c, 0, &world(&[up(ME, true), up(P2, false)], nf));
        let mut w = world(&[dead(ME, true), up(P2, false)], nf);
        w.local_hits = LocalHits {
            last: Some((HitKind::Mob, 500)),
            rival_age_ms: None,
            rival_id: None,
        };
        let (acts, _, _) = run_dead(&mut c, 100, &w);
        assert_eq!(
            reports(&acts),
            [KillEvent {
                kind: KillKind::Died,
                victim: short_id(ME),
                killer: None,
            }],
            "a death on the scoreboard, no line on the feed"
        );
        let (acts, _, _) = run(&mut c, 5_000, &w);
        assert!(reports(&acts).is_empty(), "once");
    }
}
