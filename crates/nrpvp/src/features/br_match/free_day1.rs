//! RW-4: slow the clock on Day 1, end Day 1 at [`DAY1_END`], put the speed back on Day 2.

use super::{before, Act, MatchBook, MatchOut, MatchRule, Tick};
use crate::game::views::ClockTime;

/// The game's own flag "night 1 done" (the host sets it; the game's morning follows).
pub(crate) const NIGHT1_DONE: u32 = 7502;
/// The free Day 1 starts at 07:00 and the host ends it when the clock reads this (well before the first circle at 12:30).
pub(crate) const DAY1_END: ClockTime = ClockTime { h: 11, m: 50, s: 0 };
/// Day 2 counts as begun for the clock speed once the clock is back before this (the game's morning reset) ...
const DAY1_END_HOUR_GUARD: ClockTime = ClockTime { h: 10, m: 0, s: 0 };
/// ... or once it reaches this (the reset never came; the first circle starts at 12:30).
const SLOW_UNTIL: ClockTime = ClockTime { h: 12, m: 20, s: 0 };
/// Game seconds from the morning to [`DAY1_END`].
pub(crate) const DAY1_SPAN_SECS: f32 = 4.0 * 3600.0 + 50.0 * 60.0;
/// The clock speed the game normally runs at (seen in every log) when none was read.
pub(crate) const VANILLA_SPEED: f32 = 72.9;

/// RW-4's state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Day1 {
    /// The speed to put back (what the clock ran at before we slowed it).
    orig_speed: Option<f32>,
    slowed: bool,
    end_sent: bool,
}

/// The rule: the free first day.
#[derive(Debug, Default)]
pub(crate) struct FreeDay1 {
    day1: Day1,
}

impl MatchRule for FreeDay1 {
    fn step(&mut self, t: &Tick, book: &mut MatchBook, out: &mut MatchOut) {
        let (world, nf, host) = (t.world, &t.nf, t.host);
        let Some(c) = world.clock else { return };
        if book.day1_secs == 0 {
            return;
        }
        if nf.day2 {
            // The game raises the Day-2 flag at once but resets the clock to the morning ~20 s later: the slow speed stays until then
            // (a fast clock would run on towards the first circle at 12:30), and goes back at the latest at SLOW_UNTIL.
            let morning_back = before(c.time, DAY1_END_HOUR_GUARD) || !before(c.time, SLOW_UNTIL);
            if self.day1.slowed && morning_back {
                self.day1.slowed = false;
                let back = self.day1.orig_speed.unwrap_or(VANILLA_SPEED);
                out.acts.push(Act::ClockSpeed(back));
                out.lines.push(format!(
                    "night_match: Day 2 has begun; clock speed back to {back:.1}"
                ));
            }
            return;
        }
        if nf.night1_done || book.match_over {
            return;
        }
        let target = DAY1_SPAN_SECS / book.day1_secs as f32;
        if self.day1.orig_speed.is_none() && c.speed > target * 1.5 {
            self.day1.orig_speed = Some(c.speed);
        }
        if (c.speed - target).abs() > 0.05 {
            out.acts.push(Act::ClockSpeed(target));
            if !self.day1.slowed {
                out.lines.push(format!(
                    "night_match: free Day 1: clock speed {:.1} -> {target:.1} ({} s until {DAY1_END})",
                    c.speed, book.day1_secs
                ));
            }
        }
        self.day1.slowed = true;
        if host && !self.day1.end_sent && !before(c.time, DAY1_END) {
            self.day1.end_sent = true;
            out.acts.push(Act::GameFlag(NIGHT1_DONE, true));
            out.lines.push(format!(
                "night_match: Day 1 is over at {}: night-1-done flag {NIGHT1_DONE} set, Day 2 follows",
                c.time
            ));
        }
    }

    fn reset(&mut self) {
        *self = FreeDay1::default();
    }

    fn describe(&self, parts: &mut super::StatusParts) {
        parts.free_day1 = self.day1.slowed;
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::super::Match;
    use super::*;
    use crate::game::world::NightFlags;

    #[test]
    fn day_one_is_slowed_ended_by_the_host_and_restored_on_day_two() {
        let mut c = core();
        let mut w = world(&[up(ME, true)], NightFlags::default());
        w.clock = Some(clock_at(7, 0, 72.9));
        let (acts, _, lines) = run(&mut c, 0, &w);
        let target = DAY1_SPAN_SECS / 1020.0;
        assert_eq!(acts, vec![Act::ClockSpeed(target)], "{lines:?}");
        // Slowed already: nothing more until the end.
        let mut w = world(&[up(ME, true)], NightFlags::default());
        w.clock = Some(clock_at(9, 0, target));
        let (acts, _, _) = run(&mut c, 50, &w);
        assert!(acts.is_empty());
        // The host ends Day 1 once, at DAY1_END.
        w.clock = Some(clock_at(11, 50, target));
        let (acts, _, _) = run(&mut c, 100, &w);
        assert_eq!(acts, vec![Act::GameFlag(NIGHT1_DONE, true)]);
        let (acts, _, _) = run(&mut c, 150, &w);
        assert!(acts.is_empty());
        // Day 2 flag up but the clock not reset yet: the speed stays slow.
        let mut early = world(
            &[up(ME, true)],
            NightFlags {
                day2: true,
                ..NightFlags::default()
            },
        );
        early.clock = Some(clock_at(11, 52, target));
        let (acts, _, _) = run(&mut c, 160, &early);
        assert!(acts.is_empty(), "{acts:?}");
        // The reset never came: back to normal at 12:20.
        early.clock = Some(clock_at(12, 20, target));
        let (acts, _, _) = run(&mut c, 170, &early);
        assert_eq!(acts, vec![Act::ClockSpeed(72.9)]);
        // Day 2: the original speed returns.
        let mut c = core();
        c.rule_mut::<FreeDay1>().day1.slowed = true;
        c.rule_mut::<FreeDay1>().day1.orig_speed = Some(72.9);
        let mut d2 = world(
            &[up(ME, true)],
            NightFlags {
                day2: true,
                ..NightFlags::default()
            },
        );
        d2.clock = Some(clock_at(7, 1, target));
        let (acts, _, _) = run(&mut c, 200, &d2);
        assert_eq!(acts, vec![Act::ClockSpeed(72.9)]);
    }

    #[test]
    fn a_client_never_ends_day_one_and_day1_secs_zero_leaves_the_clock_alone() {
        let mut c = core();
        let mut w = world(&[up(P2, true)], NightFlags::default());
        w.clock = Some(clock_at(11, 55, DAY1_SPAN_SECS / 1020.0));
        let (acts, _, _) = run(&mut c, 0, &w);
        assert!(acts.is_empty());
        let mut off = Match::battle_royale(0, 20, false);
        let (acts, _, _) = run(&mut off, 0, &world(&[up(ME, true)], NightFlags::default()));
        assert!(acts.is_empty());
    }
}
