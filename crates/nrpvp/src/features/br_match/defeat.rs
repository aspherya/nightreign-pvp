//! RW-1 / RW-2: judge LOCAL's death, hold a defeated player, open the Esc menu for them, send the defeat screen.

use super::{before, Act, MatchBook, MatchOut, MatchRule, Tick, FLAG_LOSE, TIDE_FROM};
use crate::game::combat_state::HitKind;
use crate::game::world::{NightFlags, World};

/// A rival's hit this recent when LOCAL dies = killed by a player (a full bleed-out is 15-17 s after the hit that downed LOCAL and every
/// hit while down counts; the margin covers a rival who walked away).
pub(crate) const RIVAL_WINDOW_MS: u64 = 30_000;
/// A hit with no attacker this recent when LOCAL dies, from the tide's first closing, = killed by the tide.
pub(crate) const ENV_WINDOW_MS: u64 = 10_000;
/// LOCAL stood in the tide's rain this recent when it died = killed by the tide, whatever the last-attacker field says (it keeps the
/// handle of the last enemy that hit LOCAL, so a tide death after a fight read as a mob kill and LOCAL respawned: Leonardo's match of
/// 2026-11-07).
pub(crate) const RAIN_WINDOW_MS: u64 = 10_000;
/// RW-1: HP 0 without the downed state must last this long before it counts as a death (ms).
pub(crate) const DEAD_DEBOUNCE_MS: u64 = 800;
/// RW-1: the match rules switch the Esc menu gate (`game::menugate`) on for a defeated, held player: Esc then asks the game to open the
/// pause menu (the game's own check refuses at HP 0, MEMORY_MAP §19). Unverified in the game until the Nucleus test.
pub(crate) const ESC_GATE_AUTO: bool = true;
/// RW-1: the defeated player's Esc menu is unlocked this long after the defeat (ms), when the death banner is over.
pub(crate) const MENU_AFTER_MS: u64 = 9_000;

/// Who defeated LOCAL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cause {
    Rival,
    Tide,
}

impl Cause {
    pub(super) fn text(self) -> &'static str {
        match self {
            Cause::Rival => "a rival",
            Cause::Tide => "the tide",
        }
    }
}

/// LOCAL's defeat (the book's `defeat`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DefeatInfo {
    pub(super) cause: Cause,
    pub(super) at_ms: u64,
    /// The defeat screen was asked for.
    pub(super) exit_sent: bool,
}

/// The rule: LOCAL's own death.
#[derive(Debug, Default)]
pub(crate) struct Defeat {
    /// LOCAL has been dead since (HP 0, not downed), and whether this death was already judged.
    dead_since: Option<u64>,
    judged: bool,
    hold_on: bool,
    /// RW-1: the Esc menu gate is open (what [`Act::MenuGate`] last said).
    menu_on: bool,
    /// RW-1: the last tick LOCAL stood in the tide's rain (`World::local_in_tide`).
    last_rain_ms: Option<u64>,
}

/// Who killed LOCAL, or `None` for an enemy, a fall or anything else the game handles itself.
fn judge(world: &World, nf: &NightFlags, in_rain: bool) -> Option<Cause> {
    let hits = world.local_hits;
    if hits.rival_age_ms.is_some_and(|a| a <= RIVAL_WINDOW_MS) {
        return Some(Cause::Rival);
    }
    // The rain is the game's own word that LOCAL is outside the circle (any day: a vanilla Day 1 has a tide too).
    if in_rain {
        return Some(Cause::Tide);
    }
    let tide = nf.day2 && world.clock.is_some_and(|c| !before(c.time, TIDE_FROM));
    match hits.last {
        Some((HitKind::Env, age)) if tide && age <= ENV_WINDOW_MS => Some(Cause::Tide),
        _ => None,
    }
}

impl MatchRule for Defeat {
    fn step(&mut self, t: &Tick, book: &mut MatchBook, out: &mut MatchOut) {
        let (now_ms, world, local) = (t.now_ms, t.world, t.local);
        if world.local_in_tide {
            self.last_rain_ms = Some(now_ms);
        }
        let dead = local.hp == 0 && local.state != 1;
        if dead {
            self.dead_since.get_or_insert(now_ms);
        } else {
            self.dead_since = None;
            self.judged = false;
        }
        // HP reaches 0 up to 100 ms before the downed byte goes to 1 (Nucleus run 2026-10-25: a defeat was judged at the hit, while the
        // player was only downed): a real death is HP 0 with no downed state for this long.
        let settled = self
            .dead_since
            .is_some_and(|t| now_ms.saturating_sub(t) >= DEAD_DEBOUNCE_MS);
        if dead && settled && !self.judged {
            self.judged = true;
            let in_rain = self
                .last_rain_ms
                .is_some_and(|r| now_ms.saturating_sub(r) <= RAIN_WINDOW_MS);
            match judge(world, &t.nf, in_rain) {
                Some(cause) => {
                    book.defeat = Some(DefeatInfo {
                        cause,
                        at_ms: now_ms,
                        exit_sent: false,
                    });
                    if cause == Cause::Rival {
                        out.acts.push(Act::DropItems);
                    }
                    out.lines.push(format!(
                        "night_match: LOCAL defeated by {}: stays dead until the match ends{}",
                        cause.text(),
                        if book.exit_ms == 0 {
                            String::new()
                        } else {
                            format!(", the defeat screen follows in {} s", book.exit_ms / 1000)
                        }
                    ));
                }
                None => {
                    book.mob_deaths += 1;
                    book.respawn_pending = true;
                    out.lines.push(format!(
                        "night_match: LOCAL died to an enemy or a fall: the game's own respawn, items kept (last hit {:?}, last rival hit {:?} ms ago)",
                        world.local_hits.last, world.local_hits.rival_age_ms
                    ));
                }
            }
        }
        // A defeated LOCAL that stands again (a dev release, an area load) is not defeated any more.
        if book.defeat.is_some() && !dead && local.state == 0 && local.hp > 0 {
            out.lines
                .push("night_match: LOCAL is up again; the defeat is cleared".into());
            book.defeat = None;
        }
        let hold = book.defeat.is_some();
        if hold != self.hold_on {
            self.hold_on = hold;
            out.acts.push(Act::DeathHold(hold));
        }
        // The Esc menu: from MENU_AFTER_MS after the defeat, once the hold hides the body (the "YOU DIED" banner has been and gone).
        let menu = ESC_GATE_AUTO
            && book.defeat.is_some_and(|d| {
                world.death_held && now_ms.saturating_sub(d.at_ms) >= MENU_AFTER_MS
            });
        if menu != self.menu_on {
            self.menu_on = menu;
            out.acts.push(Act::MenuGate(menu));
            out.lines.push(format!(
                "night_match: the Esc menu is {} for the defeated player",
                if menu { "unlocked" } else { "left to the game" }
            ));
        }
        if let Some(d) = book.defeat.as_mut() {
            if !d.exit_sent
                && !book.match_over
                && book.exit_ms != 0
                && now_ms.saturating_sub(d.at_ms) >= book.exit_ms
            {
                d.exit_sent = true;
                out.flags.push((FLAG_LOSE, true));
                out.lines.push(
                    "night_match: the defeat screen is asked for (back to the Roundtable)".into(),
                );
            }
        }
    }

    fn reset(&mut self) {
        *self = Defeat::default();
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::super::Match;
    use super::*;
    use crate::game::combat_state::LocalHits;

    #[test]
    fn a_rival_kill_is_a_defeat_that_holds_the_body_and_sends_the_defeat_screen_after_the_delay() {
        let mut c = core();
        let nf = NightFlags::default();
        let mut w = world(&[up(ME, true), up(P2, false), up(P3, false)], nf);
        run(&mut c, 0, &w);
        w = world(&[dead(ME, true), up(P2, false), up(P3, false)], nf);
        rival_hit(&mut w, 2_000);
        let (acts, flags, lines) = run_dead(&mut c, 100, &w);
        assert!(acts.contains(&Act::DeathHold(true)));
        assert_eq!(
            acts.iter().filter(|a| **a == Act::DropItems).count(),
            1,
            "RW-3: the gear drops once, at the defeat"
        );
        assert!(flags.is_empty());
        assert!(lines.iter().any(|l| l.contains("defeated by a rival")));
        // The defeat screen follows after defeat_exit_secs, once.
        let (_, flags, _) = run(&mut c, 20_899, &w);
        assert!(flags.is_empty());
        let (_, flags, _) = run(&mut c, 20_900, &w);
        assert_eq!(flags, vec![(FLAG_LOSE, true)]);
        let (_, flags, _) = run(&mut c, 22_000, &w);
        assert!(flags.is_empty(), "no second defeat screen");
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn the_defeated_players_esc_menu_gate_is_on() {
        assert!(ESC_GATE_AUTO);
    }

    #[test]
    fn the_defeated_players_esc_menu_opens_after_the_banner_once_the_body_is_held() {
        let nf = NightFlags::default();
        let mut c = Match::battle_royale(1020, 0, true);
        run(
            &mut c,
            0,
            &world(&[up(ME, true), up(P2, false), up(P3, false)], nf),
        );
        let mut w = world(&[dead(ME, true), up(P2, false), up(P3, false)], nf);
        rival_hit(&mut w, 100);
        run_dead(&mut c, 100, &w);
        let (acts, _, _) = run(&mut c, 20_000, &w);
        assert!(
            !acts.contains(&Act::MenuGate(true)),
            "not while the body is not held"
        );
        w.death_held = true;
        let (acts, _, lines) = run(&mut c, 20_100, &w);
        assert!(acts.contains(&Act::MenuGate(true)), "{lines:?}");
        let (acts, _, _) = run(&mut c, 20_200, &w);
        assert!(!acts.contains(&Act::MenuGate(true)), "said once");
    }

    #[test]
    fn a_mob_death_is_vanilla_nothing_is_held_and_nobody_wins() {
        let mut c = core();
        let w0 = world(&[up(ME, true), up(P2, false)], NightFlags::default());
        run(&mut c, 0, &w0);
        let mut w = world(&[dead(ME, true), up(P2, false)], NightFlags::default());
        w.local_hits = LocalHits {
            last: Some((HitKind::Mob, 500)),
            rival_age_ms: None,
            rival_id: None,
        };
        let (acts, flags, lines) = run_dead(&mut c, 100, &w);
        assert!(!acts.contains(&Act::DeathHold(true)));
        assert!(flags.is_empty());
        assert!(lines.iter().any(|l| l.contains("died to an enemy")));
        // Respawned after 11 s: still in; the other player is not alone for the game.
        let (_, flags, _) = run(&mut c, 11_000, &w0);
        assert!(flags.is_empty());
    }

    #[test]
    fn a_rival_hit_long_ago_does_not_make_a_mob_death_a_defeat() {
        let mut c = core();
        let w0 = world(&[up(ME, true), up(P2, false)], NightFlags::default());
        run(&mut c, 0, &w0);
        let mut w = world(&[dead(ME, true), up(P2, false)], NightFlags::default());
        rival_hit(&mut w, RIVAL_WINDOW_MS + 1);
        let (acts, _, _) = run_dead(&mut c, 100, &w);
        assert!(!acts.contains(&Act::DeathHold(true)));
    }

    #[test]
    fn the_tide_kills_for_good_only_once_the_circle_closes_on_day_two() {
        let nf = NightFlags {
            day2: true,
            ..NightFlags::default()
        };
        let env = |age| LocalHits {
            last: Some((HitKind::Env, age)),
            rival_age_ms: None,
            rival_id: None,
        };
        let mut c = core();
        let mut w = world(&[up(ME, true), up(P2, false)], nf);
        w.clock = Some(clock_at(14, 0, 72.9));
        run(&mut c, 0, &w);
        let mut dead_w = world(&[dead(ME, true), up(P2, false)], nf);
        dead_w.clock = Some(clock_at(14, 0, 72.9));
        dead_w.local_hits = env(1_000);
        let (acts, _, lines) = run_dead(&mut c, 100, &dead_w);
        assert!(acts.contains(&Act::DeathHold(true)), "{lines:?}");
        assert!(
            !acts.contains(&Act::DropItems),
            "the tide drops no items (Leonardo 2026-10-31: rival kills only)"
        );
        assert!(lines.iter().any(|l| l.contains("defeated by the tide")));
        // A tide death after a fight: the last-attacker field still names the last enemy (a "mob" hit), but LOCAL stood in the rain.
        let mut c = core();
        let mut a = world(&[up(ME, true), up(P2, false)], nf);
        a.clock = Some(clock_at(23, 30, 72.9));
        a.local_in_tide = true;
        run(&mut c, 0, &a);
        let mut b = world(&[dead(ME, true), up(P2, false)], nf);
        b.clock = Some(clock_at(23, 30, 72.9));
        b.local_hits = LocalHits {
            last: Some((HitKind::Mob, 820)),
            rival_age_ms: None,
            rival_id: None,
        };
        let (acts, _, lines) = run_dead(&mut c, 100, &b);
        assert!(acts.contains(&Act::DeathHold(true)), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("defeated by the tide")));
        // Rain long before the death does not count.
        let mut c = core();
        run(&mut c, 0, &a);
        let (acts, _, _) = run_dead(&mut c, RAIN_WINDOW_MS + 1_000, &b);
        assert!(!acts.contains(&Act::DeathHold(true)));
        // The same death on Day 1, or before 12:30, is a fall: vanilla.
        for (day2, h) in [(false, 14), (true, 9)] {
            let nf = NightFlags {
                day2,
                ..NightFlags::default()
            };
            let mut c = core();
            let mut a = world(&[up(ME, true), up(P2, false)], nf);
            a.clock = Some(clock_at(h, 0, 72.9));
            run(&mut c, 0, &a);
            let mut b = world(&[dead(ME, true), up(P2, false)], nf);
            b.clock = Some(clock_at(h, 0, 72.9));
            b.local_hits = env(1_000);
            let (acts, _, _) = run_dead(&mut c, 100, &b);
            assert!(!acts.contains(&Act::DeathHold(true)));
        }
    }
}
