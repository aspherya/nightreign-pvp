//! N6: the headless twins of the single-game rules scenarios (`scenarios/*.toml`). Tests only.
//!
//! Each test drives the real [`Match`] (the core of the `night_match` feature, the same `step` the main loop calls) on a fake
//! clock with the commands of its scenario written as events, and asserts the same strings the scenario's `expect` / `wait` /
//! `expect_log` assert. They run in milliseconds in `cargo test`, so a rules change is checked before the 3-5 minute in-game run;
//! they do NOT replace it. What a twin cannot model is named in its doc comment: everything the game does (the comeback hold
//! itself, the respawn, the defeat screen and the end flow, the camera, the Esc menu opening) and the real wall-clock timings.
//! The `Act`s the rules ask for stand in for the game-side effects (`DeathHold(true)` is the `start step HELD` / `death held` of
//! the scenarios). The scenario -> test list is in `orchestrator/SCENARIOS.md` ("Headless twins").
//!
//! The scenario console commands map to events like this: `hp0` = LOCAL at 0 HP, `rival_hit` = `rival_hit(age)`, `puppet_hp n 0`
//! = that rival at 0 HP, `flag 10007420 1` = `NightFlags::our_die` (kept for fidelity), `sleep_ms` = the next tick's time.

use super::br_match::{Match, FLAG_LOSE, FLAG_SPECTATE_HINT, FLAG_WIN};
use super::testkit::*;
use super::Act;

/// LOCAL + 3 puppets (rivals, every player on its own team) at known positions. LOCAL's HP, the puppets' HP.
fn arena(me_hp: u32, puppets: [u32; 3]) -> WorldBuilder {
    WorldBuilder::new()
        .local(HOST, me_hp, 0)
        .local_pos(Some(([0.0, 0.0, 0.0], true)))
        .flags(|f| f.our_die = true)
        .other(P2, puppets[0], 0)
        .at([500.0, 10.0, 0.0])
        .other(P3, puppets[1], 0)
        .at([50.0, 12.0, 5.0])
        .other(P4, puppets[2], 0)
        .at([300.0, 0.0, 0.0])
}

/// One match core, a log of everything it said, and the acts and flags it asked for.
struct Twin {
    core: Match,
    log: Vec<String>,
    acts: Vec<Act>,
    flags: Vec<(u32, bool)>,
}

impl Twin {
    fn new(day1_secs: u32, exit_secs: u32, spectate: bool) -> Self {
        Twin {
            core: Match::battle_royale(day1_secs, exit_secs, spectate),
            log: Vec::new(),
            acts: Vec::new(),
            flags: Vec::new(),
        }
    }

    fn tick(&mut self, ms: u64, b: &WorldBuilder) {
        let w = b.build();
        let (acts, lines) = self.core.step(ms, &w, &mut self.flags);
        self.acts.extend(acts);
        self.log.extend(lines);
    }

    fn log_has(&self, s: &str) -> bool {
        self.log.iter().any(|l| l.contains(s))
    }

    fn acted(&self, a: Act) -> bool {
        self.acts.contains(&a)
    }

    /// `expect_log`: every string in `want` was logged, none of `absent` was.
    fn expect_log(&self, want: &[&str], absent: &[&str]) {
        for w in want {
            assert!(self.log_has(w), "log lacks {w:?}: {:#?}", self.log);
        }
        for a in absent {
            assert!(!self.log_has(a), "log has {a:?}: {:#?}", self.log);
        }
    }

    /// `expect` on `night_status`.
    fn expect_status(&self, want: &[&str], absent: &[&str]) {
        let status = &self.core.status;
        for w in want {
            assert!(status.contains(w), "status lacks {w:?}: {status}");
        }
        for a in absent {
            assert!(!status.contains(a), "status has {a:?}: {status}");
        }
    }
}

/// Twin of `scenarios/day1-free.toml` (`day1_secs = 120`): the slowed clock, "Day 1 is over" at 11:50 with 7502 set and no night,
/// the game's morning and the speed back. Not modelled: the real clock running 07:00 -> 11:50 in 120 s (the clock is a fake
/// reading here), the screenshots and the `clock` console reading.
#[test]
fn sim_day1_free() {
    let vanilla = 72.9;
    let slow = super::br_match::free_day1::DAY1_SPAN_SECS / 120.0;
    let mut t = Twin::new(120, 0, false);
    let at = |h, m, s, speed| arena(240, [240; 3]).clock(h, m, s, speed);
    t.tick(0, &at(7, 0, 0, vanilla));
    t.expect_status(&["free day 1"], &[]);
    t.tick(1_000, &at(7, 0, 1, slow));
    t.tick(2_000, &at(11, 50, 0, slow));
    t.tick(3_000, &at(11, 50, 1, slow));
    t.tick(4_000, &at(11, 50, 2, slow).flags(|f| f.night1_done = true));
    let d2 = |h, m, s, sp| {
        at(h, m, s, sp).flags(|f| {
            f.night1_done = true;
            f.day2 = true;
        })
    };
    t.tick(5_000, &d2(7, 0, 0, slow));
    t.tick(6_000, &d2(7, 0, 5, vanilla));
    t.expect_status(&["day 2"], &["free day 1"]);
    t.expect_log(
        &[
            "free Day 1: clock speed",
            "Day 1 is over",
            "Day 2 has begun; clock speed back",
        ],
        &["night 1 begins", "panicked at", "no free hardware slot"],
    );
    assert!(t.acted(Act::GameFlag(7502, true)));
}

/// Twin of `scenarios/defeat-mob.toml`: `hp0` with no rival hit is "died to an enemy", nothing is held, LOCAL is not defeated,
/// `mob deaths 1`. Not modelled: the game's own respawn (about 14 s) and what the game does with the body.
#[test]
fn sim_defeat_mob() {
    let mut t = Twin::new(1020, 0, true);
    t.tick(0, &arena(240, [240; 3]));
    let dead = arena(0, [240; 3]);
    t.tick(1_000, &dead);
    t.tick(2_000, &dead);
    t.expect_status(&["LOCAL in", "mob deaths 1"], &["defeated"]);
    t.expect_log(
        &["died to an enemy"],
        &["defeated by", "panicked at", "no free hardware slot"],
    );
    assert!(!t.acted(Act::DeathHold(true)), "a mob death holds nothing");
    assert!(!t.acted(Act::DropItems));
}

/// Twin of `scenarios/defeat-rival.toml` (`defeat_exit_secs = 15`): `rival_hit` then `hp0` is a defeat for good, the comeback hold
/// is asked for, the defeat screen (`FLAG_LOSE`) follows after `defeat_exit_secs`. Not modelled: the hold itself (`start step
/// HELD` and "death held" in `night_status` are game-side: here `Act::DeathHold(true)`), the banner, the Result / Reward / Next
/// flow and the return to the Roundtable.
#[test]
fn sim_defeat_rival() {
    let mut t = Twin::new(1020, 15, false);
    t.tick(0, &arena(240, [240; 3]));
    let dead = arena(0, [240; 3]).rival_hit(1_000);
    t.tick(1_000, &dead);
    t.tick(2_000, &dead); // judged here (HP 0 for 1000 ms >= the debounce)
    t.expect_status(
        &["defeated by a rival"],
        &["panic", "no free hardware slot"],
    );
    t.expect_log(
        &["defeated by a rival"],
        &["the defeat screen is asked for"],
    );
    assert!(t.acted(Act::DeathHold(true)));
    assert!(t.flags.is_empty());
    // 15 s after the defeat: not at 16999, at 17000.
    t.tick(16_999, &dead.clone().held());
    assert!(t.flags.is_empty());
    t.tick(17_000, &dead.clone().held());
    t.expect_log(
        &["defeated by a rival", "the defeat screen is asked for"],
        &["RELEASED", "panicked at", "no free hardware slot"],
    );
    assert_eq!(t.flags, vec![(FLAG_LOSE, true)]);
}

/// Twin of `scenarios/match-win.toml`: the three puppets at 0 HP, out after 30 s, LOCAL alone for 1 s on Day 1: `match decided`,
/// `won`, `FLAG_WIN`. Not modelled: the game's ending banner and the scenario's wall-clock wait (the rules use 30 s dead + 1 s alone).
#[test]
fn sim_match_win() {
    let mut t = Twin::new(1020, 20, false);
    t.tick(0, &arena(240, [240; 3]));
    let down = arena(240, [0; 3]);
    for ms in [1_000, 15_000, 30_999, 31_000, 31_999, 32_000, 33_000] {
        t.tick(ms, &down);
    }
    t.expect_log(
        &["match decided", "won"],
        &["panicked at", "no free hardware slot"],
    );
    t.expect_status(&["match over", "decided"], &[]);
    assert_eq!(t.flags, vec![(FLAG_WIN, true)]);
}

/// Twin of the rules part of `scenarios/match-win-end.toml` (same events as `match-win`, then the Roundtable). Not modelled: the
/// end flow the scenario adds (Result, Reward, Next presses, the return itself: all game UI); here the Roundtable is only a
/// `place` change that resets the rules.
#[test]
fn sim_match_win_end() {
    let mut t = Twin::new(1020, 20, false);
    t.tick(0, &arena(240, [240; 3]));
    let down = arena(240, [0; 3]);
    for ms in [1_000, 31_000, 32_000, 40_000] {
        t.tick(ms, &down);
    }
    t.expect_log(
        &["match decided", "won"],
        &["panicked at", "no free hardware slot"],
    );
    assert_eq!(t.flags, vec![(FLAG_WIN, true)]);
    t.tick(50_000, &arena(240, [240; 3]).roundtable());
    t.expect_log(&["out of the expedition; rules reset"], &[]);
}

/// Twin of `scenarios/match-loss.toml` (`defeat_exit_secs = 60`): two puppets at 0 HP, a rival hit, LOCAL at 0 HP: defeated, the
/// hold is asked for, the last puppet wins, `match decided ... lost`, and the defeat screen is NOT asked for afterwards. Not
/// modelled: the held start step in the game and the loss banner.
#[test]
fn sim_match_loss() {
    let mut t = Twin::new(1020, 60, false);
    t.tick(0, &arena(240, [240; 3]));
    t.tick(1_000, &arena(240, [0, 0, 240]));
    let dead = arena(0, [0, 0, 240]).rival_hit(1_000);
    t.tick(2_000, &dead);
    t.tick(2_800, &dead); // judged
    assert!(t.acted(Act::DeathHold(true)));
    // The two downed puppets are out after 30 s, the last one stands alone for 1 s; the defeat screen would be at 62800.
    for ms in [20_000, 32_800, 33_800, 34_000, 62_900, 70_000] {
        t.tick(ms, &dead.clone().held());
    }
    t.expect_log(
        &["defeated by a rival", "match decided", "lost"],
        &["the defeat screen is asked for", "RELEASED", "panicked at"],
    );
    assert_eq!(
        t.flags.iter().filter(|f| **f == (FLAG_LOSE, true)).count(),
        1,
        "the count sets FLAG_LOSE once: no second defeat screen"
    );
}

/// Twin of `scenarios/spectate.toml` (`defeat_exit_secs = 0`, spectate on): after `rival_hit` + `hp0` the held body follows the
/// nearest living puppet (`watching 0x..` in `night_status`, `Act::Follow`), F10 asks for the defeat screen. Not modelled: the
/// teleport itself, the follow camera (screenshot `watching2`) and the real exit from the expedition after F10.
#[test]
fn sim_spectate() {
    let mut t = Twin::new(1020, 0, true);
    t.tick(0, &arena(240, [240; 3]));
    let dead = arena(0, [240; 3]).rival_hit(1_000);
    t.tick(1_000, &dead);
    t.tick(1_900, &dead.clone().held());
    t.tick(3_000, &dead.clone().held());
    t.expect_status(&["defeated by a rival", "watching"], &["the defeat screen"]);
    assert!(t.acted(Act::DeathHold(true)));
    assert!(t.acts.iter().any(|a| matches!(a, Act::Follow(_))));
    t.expect_log(&["spectating player"], &[]);
    t.tick(4_000, &dead.clone().held().leave());
    t.expect_log(
        &["spectating player", "the leave key was pressed"],
        &["panicked at", "no free hardware slot"],
    );
    // The spectator hint (UI-4) comes first, when the watching starts; the leave key then asks for the defeat screen.
    assert_eq!(t.flags, vec![(FLAG_SPECTATE_HINT, true), (FLAG_LOSE, true)]);
}

/// Twin of the rules part of `scenarios/esc-defeated.toml` (`defeat_exit_secs = 0`): as `spectate`, and 9 s after the defeat the
/// rules unlock the Esc menu (`MenuGate(true)`, "the Esc menu is unlocked"). Not modelled: the menu actually opening (CSMenuMan
/// +0x48, the `asked the game to open the pause menu` line, the screenshots), which is `game::menugate`.
#[test]
fn sim_esc_defeated() {
    let mut t = Twin::new(1020, 0, true);
    t.tick(0, &arena(240, [240; 3]));
    let dead = arena(0, [240; 3]).rival_hit(1_000);
    t.tick(1_000, &dead);
    t.tick(1_900, &dead.clone().held()); // defeated here
    t.expect_status(&["defeated by a rival", "watching"], &["the defeat screen"]);
    t.tick(10_899, &dead.clone().held());
    assert!(!t.acted(Act::MenuGate(true)), "not before MENU_AFTER_MS");
    t.tick(10_900, &dead.clone().held());
    t.expect_log(&["the Esc menu is unlocked"], &["panicked at"]);
    assert!(t.acted(Act::MenuGate(true)));
}
