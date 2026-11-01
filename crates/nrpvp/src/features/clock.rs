//! M5 (dev console): the expedition's play-area clock. `clock` shows it; `clock night` / `clock circle` / `clock end` /
//! `clock <hh:mm[:ss]>` jump it, so a night or the final circle can be tested without waiting out the day.
//!
//! No game code is called and nothing is patched. The game's own clock update (`exe+0x698430`) reads a set-time mailbox on
//! `WorldPlayAreaManager` every frame; when it holds a valid hour, minute and second it sets the clock with its own setter, empties
//! the mailbox and, in a session, sends the time to the other players. This feature only fills that mailbox (through
//! [`Ctx::clock`], [`crate::game::views::PlayArea::request_time`]). Engine facts: MEMORY_MAP §16, `archive/research/M5-no-boss-nights.md`.
//!
//! The day runs 07:00 → 23:59:59; the game holds the clock at 23:59:59 for the night until "night boss defeated" (flags 7502 /
//! 7507) brings the next morning. Event scripts compare the same clock (`1[6]`/`1[7]`), so a jump fires what that time fires.
//! Log lines start with `clock:`.

use std::sync::{Mutex, PoisonError};

use super::{Ctx, Feature};
use crate::game::views::{ClockState, ClockTime};
use crate::game::world::World;

/// A console request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClockCmd {
    Show,
    Set(ClockTime),
}

/// Named times (PlayAreaCreateCommonParam row 0, 1.03.3: first circle 12:30 → 16:09, final circle 20:24 → 24:00; the night
/// events start at 23:00 when a player is at the boss area and at 23:59 for everyone; MEMORY_MAP §16).
pub(crate) const PRESETS: [(&str, ClockTime); 3] = [
    ("circle", ClockTime { h: 20, m: 20, s: 0 }),
    ("night", ClockTime { h: 22, m: 55, s: 0 }),
    (
        "end",
        ClockTime {
            h: 23,
            m: 58,
            s: 30,
        },
    ),
];

/// `night`, `circle`, `end`, `hh:mm` or `hh:mm:ss` (pure; unit tested).
pub(crate) fn parse(arg: &str) -> Result<ClockTime, String> {
    let a = arg.trim().to_ascii_lowercase();
    if let Some((_, t)) = PRESETS.iter().find(|(n, _)| *n == a) {
        return Ok(*t);
    }
    let parts: Vec<&str> = a.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return Err(format!(
            "expected hh:mm, hh:mm:ss or one of {}",
            preset_names()
        ));
    }
    let mut v = [0u8; 3];
    for (o, p) in v.iter_mut().zip(&parts) {
        *o = p.parse().map_err(|_| format!("{p:?} is not a number"))?;
    }
    ClockTime::new(v[0], v[1], v[2]).ok_or_else(|| "hours 0-23, minutes and seconds 0-59".into())
}

fn preset_names() -> String {
    PRESETS.map(|(n, _)| n).join(", ")
}

/// Whether the clock may be set now (pure; unit tested).
pub(crate) fn plan(world: &World) -> Result<(), String> {
    match world.clock {
        None => Err("no play-area clock (only in an expedition)".into()),
        Some(c) if !c.mailbox_free => Err("the game has not taken the last time yet".into()),
        Some(_) if !world.in_expedition() => Err("not in an expedition".into()),
        Some(_) => Ok(()),
    }
}

/// The clock reads `asked` or up to 5 game minutes past it (the clock runs about 73 game seconds per real second; pure).
fn applied(asked: ClockTime, now: ClockTime) -> bool {
    let secs = |t: ClockTime| i32::from(t.h) * 3600 + i32::from(t.m) * 60 + i32::from(t.s);
    (0..=300).contains(&(secs(now) - secs(asked)))
}

/// The clock in one line (pure).
pub(crate) fn describe(c: Option<&ClockState>) -> String {
    match c {
        None => "no play-area clock".into(),
        Some(c) => format!(
            "{} ({}, speed {:.2})",
            c.time,
            if c.running { "running" } else { "held" },
            c.speed
        ),
    }
}

/// Last request and the clock line, for the console (main loop writes, pipe thread reads).
static STATUS: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));

/// `clock: <now>; last: <request>` (console).
pub(crate) fn status_line() -> String {
    let s = STATUS.lock().unwrap_or_else(PoisonError::into_inner);
    format!(
        "clock: {}; last: {}",
        if s.1.is_empty() { "not read yet" } else { &s.1 },
        if s.0.is_empty() {
            "no request yet"
        } else {
            &s.0
        }
    )
}

fn set_status(request: Option<String>, now: Option<String>) {
    let mut s = STATUS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(r) = request {
        s.0 = r;
    }
    if let Some(n) = now {
        s.1 = n;
    }
}

#[derive(Default)]
pub(crate) struct ClockFeature {
    /// The time asked for, until the clock is seen there (then one evidence line is logged).
    asked: Option<ClockTime>,
    /// The last hour logged (one line per game hour, so a test log shows when things happened).
    last_hour: Option<u8>,
}

impl ClockFeature {
    pub(crate) fn request(&mut self, cmd: ClockCmd, src: &str, world: &World, ctx: &mut Ctx) {
        let now = describe(world.clock.as_ref());
        let line = match cmd {
            ClockCmd::Show => {
                set_status(None, Some(now));
                return;
            }
            ClockCmd::Set(t) => match plan(world) {
                Ok(()) => {
                    ctx.clock = Some(t);
                    self.asked = Some(t);
                    format!("{t} asked by {src} (was {now})")
                }
                Err(e) => format!("{t} refused: {e}"),
            },
        };
        tracing::info!("clock: {line}");
        set_status(Some(line), Some(now));
    }
}

impl Feature for ClockFeature {
    fn name(&self) -> &'static str {
        "clock"
    }

    fn tick(&mut self, world: &World, _ctx: &mut Ctx) {
        let Some(c) = world.clock else {
            self.last_hour = None;
            set_status(None, Some(describe(None)));
            return;
        };
        if let Some(t) = self.asked {
            // The game took the request once its mailbox is empty again and the clock reads the new time (give or take the
            // seconds it ran since). The first tick after a request runs before the mailbox is written, so it must not count.
            if c.mailbox_free && applied(t, c.time) {
                tracing::info!(
                    "clock: the game applied {t}; clock now {}",
                    describe(Some(&c))
                );
                self.asked = None;
            }
        }
        if self.last_hour != Some(c.time.h) {
            tracing::info!("clock: {}", describe(Some(&c)));
            self.last_hour = Some(c.time.h);
        }
        set_status(None, Some(describe(Some(&c))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::views::Place;

    fn clock(free: bool) -> ClockState {
        ClockState {
            time: ClockTime::new(10, 0, 0).unwrap(),
            speed: 1.0,
            running: true,
            mailbox_free: free,
        }
    }

    fn expedition(c: Option<ClockState>) -> World {
        World {
            wcm_instance: 0x5_0000,
            local: Some(0x10_0000),
            place: Some(Place::Expedition),
            clock: c,
            ..World::default()
        }
    }

    #[test]
    fn presets_and_times_parse() {
        assert_eq!(parse("night"), Ok(ClockTime::new(22, 55, 0).unwrap()));
        assert_eq!(parse(" Circle "), Ok(ClockTime::new(20, 20, 0).unwrap()));
        assert_eq!(parse("end"), Ok(ClockTime::new(23, 58, 30).unwrap()));
        assert_eq!(parse("7:05"), Ok(ClockTime::new(7, 5, 0).unwrap()));
        assert_eq!(parse("23:59:59"), Ok(ClockTime::new(23, 59, 59).unwrap()));
    }

    #[test]
    fn bad_times_are_refused() {
        assert!(parse("24:00").unwrap_err().contains("0-23"));
        assert!(parse("12:60").is_err());
        assert!(parse("noon").unwrap_err().contains("circle, night, end"));
        assert!(parse("1:2:3:4").is_err());
        assert!(parse("ab:00").unwrap_err().contains("not a number"));
    }

    #[test]
    fn setting_needs_an_expedition_clock_with_a_free_mailbox() {
        assert!(plan(&expedition(Some(clock(true)))).is_ok());
        assert!(plan(&expedition(None))
            .unwrap_err()
            .contains("only in an expedition"));
        assert!(plan(&expedition(Some(clock(false))))
            .unwrap_err()
            .contains("not taken"));
        let mut rt = expedition(Some(clock(true)));
        rt.place = Some(Place::Roundtable);
        assert!(plan(&rt).unwrap_err().contains("not in an expedition"));
    }

    #[test]
    fn a_set_request_queues_the_time_only_when_allowed() {
        let mut f = ClockFeature::default();
        let t = ClockTime::new(22, 55, 0).unwrap();
        let mut ctx = Ctx::default();
        f.request(
            ClockCmd::Set(t),
            "test",
            &expedition(Some(clock(true))),
            &mut ctx,
        );
        assert_eq!(ctx.clock, Some(t));
        assert!(status_line().contains("22:55:00 asked by test"));
        let mut ctx = Ctx::default();
        f.request(ClockCmd::Set(t), "test", &expedition(None), &mut ctx);
        assert_eq!(ctx.clock, None);
        let mut ctx = Ctx::default();
        f.request(
            ClockCmd::Show,
            "test",
            &expedition(Some(clock(true))),
            &mut ctx,
        );
        assert_eq!(ctx, Ctx::default());
    }

    #[test]
    fn a_jump_counts_as_applied_only_once_the_clock_is_there() {
        let t = |h, m, s| ClockTime::new(h, m, s).unwrap();
        // M5-2 run: asked 23:58:30 while the clock read 08:45:19 (the tick before the write).
        assert!(!applied(t(23, 58, 30), t(8, 45, 19)));
        assert!(applied(t(23, 58, 30), t(23, 58, 32)));
        assert!(
            applied(t(23, 58, 30), t(23, 59, 59)),
            "held at the night cap"
        );
        assert!(!applied(t(20, 20, 0), t(20, 26, 0)));
    }

    #[test]
    fn the_clock_reads_as_one_line() {
        assert_eq!(describe(None), "no play-area clock");
        let mut c = clock(true);
        c.running = false;
        assert_eq!(describe(Some(&c)), "10:00:00 (held, speed 1.00)");
    }
}
