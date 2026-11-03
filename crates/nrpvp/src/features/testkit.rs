//! R7-1 (P2-1): the characterization harness for the match rules. Tests only (`#[cfg(test)]`).
//!
//! A [`WorldBuilder`] makes the tick's [`World`]; a [`Sim`] drives the real [`Match`] (the core of the `night_match` feature, the
//! thing `MatchFeature::tick` calls) tick by tick on a fake clock and writes what came out of every tick into a deterministic text
//! trace: the acts, the flag writes, the clock request and the matchmaking commands (the [`Ctx`] the main loop would flush), the log
//! lines, and the `status()` text. The trace is compared with a golden file under `features/golden/`; `UPDATE_GOLDEN=1 cargo test`
//! rewrites the goldens. The goldens record today's behaviour, warts included: a later card that changes behaviour on purpose
//! re-blesses them in the same PR and says so.
//!
//! `MatchFeature::tick` itself reads `Instant` and two process-wide statics (`COUNT`, `SPECTATE_OVERRIDE`); the harness calls the
//! same `Match::step` with a fake `now_ms` instead, and feeds `count_off` / `spectate` the way `tick` does.

use std::fmt::Write as _;
use std::path::PathBuf;

use super::br_match::Match;
use super::Act;
use super::Ctx;
use crate::game::combat_state::{Fighter, HitKind, LocalHits};
use crate::game::views::{ClockState, ClockTime, Place};
use crate::game::world::{NightFlags, World, MAX_FIGHTERS};

/// Session ids: the host has `0x6001` in its upper half (`br_match::HOST_ID_HI`).
pub(super) const HOST: u64 = 0x6001_FFFF_FFFF;
pub(super) const P2: u64 = 0x6002_FFFF_FFFF;
pub(super) const P3: u64 = 0x6003_FFFF_FFFF;
pub(super) const P4: u64 = 0x6004_FFFF_FFFF;
/// Team keys of a 2 v 2 (`game::combat_state::team_key` with a roster: `1 << 63 | team no`).
pub(super) const TEAM_A: u64 = (1 << 63) | 1;
pub(super) const TEAM_B: u64 = (1 << 63) | 2;
/// The Day-1 speed the rules aim for with `day1_secs = 1020`.
pub(super) const DAY1_1020_SPEED: f32 = super::br_match::free_day1::DAY1_SPAN_SECS / 1020.0;

/// Builds one tick's [`World`] in an expedition (09:00 on Day 1, LOCAL on the ground at (10, 20, 30)).
#[derive(Clone)]
pub(super) struct WorldBuilder {
    w: World,
    n: usize,
}

impl WorldBuilder {
    pub(super) fn new() -> Self {
        WorldBuilder {
            w: World {
                wcm_instance: 0x5_0000,
                local: Some(0x10_0000),
                place: Some(Place::Expedition),
                night: Some(NightFlags::default()),
                local_pos: Some(([10.0, 20.0, 30.0], true)),
                ..World::default()
            },
            n: 0,
        }
        .clock(9, 0, 0, DAY1_1020_SPEED)
    }

    fn push(mut self, id: u64, team: u64, local: bool, hp: u32, state: u8) -> Self {
        if self.n < MAX_FIGHTERS {
            self.w.fighters[self.n] = Some(Fighter {
                addr: 0x10_0000 + (id >> 32) as usize,
                id,
                team,
                local,
                state,
                hp,
                clock: 0.0,
            });
            self.n += 1;
        }
        self
    }

    /// LOCAL (free for all: its team is its own id).
    pub(super) fn local(self, id: u64, hp: u32, state: u8) -> Self {
        self.push(id, id, true, hp, state)
    }

    /// LOCAL on a team.
    pub(super) fn local_in(self, id: u64, team: u64, hp: u32, state: u8) -> Self {
        self.push(id, team, true, hp, state)
    }

    /// Another player (free for all).
    pub(super) fn other(self, id: u64, hp: u32, state: u8) -> Self {
        self.push(id, id, false, hp, state)
    }

    /// Another player on a team.
    pub(super) fn other_in(self, id: u64, team: u64, hp: u32, state: u8) -> Self {
        self.push(id, team, false, hp, state)
    }

    /// The world position of the fighter added last (the spectator and RW-7 read these).
    pub(super) fn at(mut self, p: [f32; 3]) -> Self {
        if self.n > 0 {
            self.w.positions[self.n - 1] = Some(p);
        }
        self
    }

    pub(super) fn local_pos(mut self, p: Option<([f32; 3], bool)>) -> Self {
        self.w.local_pos = p;
        self
    }

    pub(super) fn clock(mut self, h: u8, m: u8, s: u8, speed: f32) -> Self {
        self.w.clock = Some(ClockState {
            time: ClockTime { h, m, s },
            speed,
            running: true,
            mailbox_free: true,
        });
        self
    }

    pub(super) fn no_clock(mut self) -> Self {
        self.w.clock = None;
        self
    }

    pub(super) fn flags(mut self, f: impl FnOnce(&mut NightFlags)) -> Self {
        let mut nf = self.w.night.unwrap_or_default();
        f(&mut nf);
        self.w.night = Some(nf);
        self
    }

    pub(super) fn no_flags(mut self) -> Self {
        self.w.night = None;
        self
    }

    pub(super) fn rival_hit(mut self, age: u64) -> Self {
        self.w.local_hits = LocalHits {
            last: Some((HitKind::Rival, age)),
            rival_age_ms: Some(age),
            rival_id: None,
        };
        self
    }

    pub(super) fn env_hit(mut self, age: u64) -> Self {
        self.w.local_hits = LocalHits {
            last: Some((HitKind::Env, age)),
            rival_age_ms: None,
            rival_id: None,
        };
        self
    }

    pub(super) fn mob_hit(mut self, age: u64) -> Self {
        self.w.local_hits = LocalHits {
            last: Some((HitKind::Mob, age)),
            rival_age_ms: None,
            rival_id: None,
        };
        self
    }

    /// The comeback's start step is held (`deathhold`).
    pub(super) fn held(mut self) -> Self {
        self.w.death_held = true;
        self
    }

    pub(super) fn leave(mut self) -> Self {
        self.w.leave_pressed = true;
        self
    }

    pub(super) fn next(mut self) -> Self {
        self.w.spec_next = true;
        self
    }

    pub(super) fn prev(mut self) -> Self {
        self.w.spec_prev = true;
        self
    }

    pub(super) fn roundtable(mut self) -> Self {
        self.w.place = Some(Place::Roundtable);
        self
    }

    pub(super) fn build(&self) -> World {
        self.w.clone()
    }
}

/// Drives one [`Match`] on a fake clock and records the trace. One `Sim` per golden file; [`Sim::case`] starts a fresh core.
pub(super) struct Sim {
    core: Match,
    out: String,
    last_status: String,
}

impl Sim {
    pub(super) fn new(title: &str) -> Self {
        Sim {
            core: Match::battle_royale(0, 0, false),
            out: format!("# {title}\n# columns: t = fake clock in ms; in = the tick's input; then what came out\n"),
            last_status: String::new(),
        }
    }

    /// A new case with a fresh core: `day1_secs` (0 = off), `defeat_exit_secs`, `spectate` as in the config.
    pub(super) fn case(&mut self, title: &str, day1_secs: u32, exit_secs: u32, spectate: bool) {
        self.core = Match::battle_royale(day1_secs, exit_secs, spectate);
        self.last_status.clear();
        let _ = writeln!(
            self.out,
            "\n== {title} (day1_secs {day1_secs}, defeat_exit_secs {exit_secs}, spectate {spectate})"
        );
    }

    /// The dev console `night_count 0|1`.
    pub(super) fn count_off(&mut self, off: bool) {
        self.core.book.count_off = off;
        let _ = writeln!(self.out, "-- night_count {}", u8::from(!off));
    }

    pub(super) fn note(&mut self, text: &str) {
        let _ = writeln!(self.out, "-- {text}");
    }

    /// One tick at `ms`, as `MatchFeature::tick` does it: step, then the acts and flags land in a [`Ctx`] and the lines in the log.
    pub(super) fn tick(&mut self, ms: u64, b: &WorldBuilder) {
        let w = b.build();
        let mut ctx = Ctx::default();
        let (acts, lines) = self.core.step(ms, &w, &mut ctx.flags);
        ctx.acts.extend(acts);
        let _ = writeln!(self.out, "t={ms:06} in: {}", summary(&w));
        let mut quiet = true;
        let mut put = |label: &str, text: String| {
            quiet = false;
            let _ = writeln!(self.out, "          {label}: {text}");
        };
        if !ctx.acts.is_empty() {
            put("acts", fmt_acts(&ctx.acts));
        }
        if !ctx.flags.is_empty() {
            let f: Vec<String> = ctx
                .flags
                .iter()
                .map(|(f, on)| format!("{f}={}", if *on { "ON" } else { "OFF" }))
                .collect();
            put("flags", f.join(", "));
        }
        if let Some(c) = ctx.clock {
            put("clock", c.to_string());
        }
        if !ctx.mm.is_empty() {
            put("mm", format!("{:?}", ctx.mm));
        }
        for l in &lines {
            put("log", l.clone());
        }
        if self.core.status != self.last_status {
            put("status", self.core.status.clone());
            self.last_status.clone_from(&self.core.status);
        }
        if quiet {
            let _ = writeln!(self.out, "          .");
        }
    }

    pub(super) fn finish(self) -> String {
        self.out
    }
}

fn fmt_acts(acts: &[Act]) -> String {
    acts.iter()
        .map(|a| format!("{a:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One line describing the tick's input, so a golden can be read without the test next to it.
fn summary(w: &World) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !w.in_expedition() {
        parts.push(format!("place {:?}", w.place));
    }
    let fighters: Vec<String> = w
        .fighters
        .iter()
        .flatten()
        .map(|f| {
            format!(
                "{:X}{}{}:hp{}/s{}",
                f.id >> 32,
                if f.local { "*" } else { "" },
                if f.team == f.id {
                    String::new()
                } else {
                    format!("@{:X}", f.team & 0xFFFF)
                },
                f.hp,
                f.state
            )
        })
        .collect();
    parts.push(format!("[{}]", fighters.join(" ")));
    match w.clock {
        Some(c) => parts.push(format!("clock {} x{:.2}", c.time, c.speed)),
        None => parts.push("clock -".into()),
    }
    match w.night {
        None => parts.push("flags -".into()),
        Some(n) => {
            let on: Vec<&str> = [
                (n.night, "night"),
                (n.day2, "day2"),
                (n.night1_done, "n1done"),
                (n.our_win, "our_win"),
                (n.our_lose, "our_lose"),
                (n.our_die, "our_die"),
            ]
            .iter()
            .filter(|(b, _)| *b)
            .map(|(_, s)| *s)
            .collect();
            if !on.is_empty() {
                parts.push(on.join("+"));
            }
        }
    }
    if let Some((p, g)) = w.local_pos {
        parts.push(format!(
            "pos ({:.0},{:.0},{:.0}){}",
            p[0],
            p[1],
            p[2],
            if g { "" } else { " air" }
        ));
    }
    if w.local_hits != LocalHits::default() {
        parts.push(format!(
            "hits last {:?} rival_age {:?}",
            w.local_hits.last, w.local_hits.rival_age_ms
        ));
    }
    for (on, s) in [
        (w.death_held, "held"),
        (w.leave_pressed, "F10"),
        (w.spec_next, "next"),
        (w.spec_prev, "prev"),
    ] {
        if on {
            parts.push(s.into());
        }
    }
    let positions: Vec<String> = w
        .positions
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.map(|p| format!("#{i}({:.0},{:.0},{:.0})", p[0], p[1], p[2])))
        .collect();
    if !positions.is_empty() {
        parts.push(format!("at {}", positions.join(" ")));
    }
    parts.join(" | ")
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("features")
        .join("golden")
        .join(format!("{name}.txt"))
}

/// Compare `actual` with `features/golden/<name>.txt`; `UPDATE_GOLDEN=1` rewrites the file instead.
pub(super) fn assert_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().expect("golden dir")).expect("create golden dir");
        std::fs::write(&path, actual).expect("write golden");
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "golden {} missing ({e}); run the tests with UPDATE_GOLDEN=1 to create it",
            path.display()
        )
    });
    let want = want.replace("\r\n", "\n");
    if want != actual {
        let line = want
            .lines()
            .zip(actual.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| want.lines().count().min(actual.lines().count()));
        panic!(
            "golden {name} differs from the trace at line {} (UPDATE_GOLDEN=1 re-blesses, only for an intended change)\n  golden: {:?}\n  actual: {:?}",
            line + 1,
            want.lines().nth(line),
            actual.lines().nth(line)
        );
    }
}
