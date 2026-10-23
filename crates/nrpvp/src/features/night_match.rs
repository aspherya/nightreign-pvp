//! M5-5 / M5-6 / M6-1: the nights are elimination rounds (Leonardo's rules, TASKS M5-D, PROJECT_SCOPE §2). Runs in every game;
//! each game decides from the same ChrSet copies, acts only on its own player, and only the host's game ends a night.
//!
//! - **Knocked, then eliminated.** At night a downed player has the normal 15 s bleed-out (the vanilla night freeze, `common` 1302
//!   with SpEffect 102120, is emptied by our event file); rivals' hits take seconds off (T-039 / T-041). When the clock reaches
//!   [`ELIM_CLOCK`] the player is **eliminated**: this game holds its clock at [`HOLD_CLOCK`] so the game never kills the player
//!   (no ~7 s comeback can start), and nobody can revive them ([`ReviveMode::Nobody`]). The rival-hit penalty stops at
//!   [`NIGHT_CLOCK_FLOOR`] at night, so the 50 ms loop sees the clock before the game reaches 0.
//! - **The night ends when one team is left standing** (not downed, HP above 0), counted from every player's copy, not from the
//!   game's alive flags 7005-7007 (3 slots, network-shared). Not while a revive item is pending (7010-7012). Only when at least two
//!   teams were there when the night began (solo and offline runs never end a night by count). The host's game sets our flag
//!   [`FLAG_NIGHT_END`]; our event sets 7502 network-wide and the game's morning follows (`gamedata/emevd-nights.json`).
//! - **Only the winner is healed**: the winner's own game sets [`FLAG_HEAL`] (our per-game event: Bonfire-like Recovery).
//! - **Morning**: every player still down gets up (the game's own get-up, `PlayerIns+0x732`), and every player who lost the night
//!   moves to their own spot of the first morning (where they stood before 12:00 of day 1), so the losers come back spread out.
//! - **Night 2 ends the match**: the winner's game sets [`FLAG_WIN`] (banner 4000, then 7000), every other game [`FLAG_LOSE`]
//!   (banner 4010, then 7001). Each game reads its own result (M5-4 run 2, Nucleus).
//!
//! - **Solo: a kill is a real death (M5-9).** With `team_size` 1, at night LOCAL gets our flag [`FLAG_DIE`] ON while it stands (state 0,
//!   HP above 0) without a revive item: our event gives it SpEffect 102130, so 0 HP is a death, not a knock (`research/M5-8-death.md`).
//!   [`NightAct::DeathHold`] arms `game::deathhold` from nightfall to the morning, so a dead LOCAL stays dead (the comeback's start
//!   step is held). Night 1's morning calls the held step ([`NightAct::ReleaseDeath`]), the game respawns LOCAL, and LOCAL is moved
//!   to a random place of a pool recorded on day 1 ([`NightCore::places`]). Night 2's dead are never released. Duo / Trio: unchanged.
//!
//! When our event file is not loaded (Nucleus: no me3), a flag of ours stays ON; after [`FALLBACK_MS`] this game writes the
//! game's flag itself (7502 in the host, 7000 / 7001 in each game: `game::flags::set_match_flag`). The heal has no fallback.
//! Not built: spectating another player (the camera, a later card). Log lines start with `night_match:`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use super::{Ctx, Feature};
use crate::game::combat_state::Fighter;
use crate::game::reviveblock::ReviveMode;
use crate::game::views::ClockTime;
use crate::game::world::{NightFlags, World};

/// The host's game sets it when one team is left (`gamedata/emevd-nights.json`; same as `nights::FLAG_NIGHT_END`).
pub(crate) const FLAG_NIGHT_END: u32 = super::nights::FLAG_NIGHT_END;
/// The night's winner's game: heal the local player (our per-game event 99100802).
pub(crate) const FLAG_HEAL: u32 = 10_007_410;
/// Night 2: this game's player won (event 99100803: banner 4000, 10 s, 7000).
pub(crate) const FLAG_WIN: u32 = 10_007_411;
/// Night 2: this game's player lost (event 99100804: banner 4010, 10 s, 7001).
pub(crate) const FLAG_LOSE: u32 = 10_007_412;
/// M5-9: LOCAL gets SpEffect 102130 (die at once) while this is ON (event 99100805; `research/M5-8-death.md` §R1).
pub(crate) const FLAG_DIE: u32 = 10_007_420;
/// The game's own flags written when our event file is missing.
pub(crate) const NIGHT1_DONE: u32 = 7502;
pub(crate) const GAME_CLEAR: u32 = 7000;
pub(crate) const GAME_OVER: u32 = 7001;

/// Bleed-out seconds at which a knocked player is eliminated (the loop runs every 50 ms; the game kills at 0).
pub(crate) const ELIM_CLOCK: f32 = 1.0;
/// Where an eliminated player's clock is held (the vanilla night freeze holds 15: `BossArenaNearDeathDuration`).
pub(crate) const HOLD_CLOCK: f32 = 15.0;
/// The rival-hit penalty's lowest clock at night (below [`ELIM_CLOCK`], so the next tick eliminates).
pub(crate) const NIGHT_CLOCK_FLOOR: f32 = 0.9;
/// Down this long at night = eliminated even if the clock never ran down (a full bleed-out is about 15 s).
pub(crate) const KNOCK_BACKUP_MS: u64 = 16_000;
/// One team must be the only one standing this long before the night is decided (one stray reading never ends a night).
pub(crate) const DECIDE_MS: u64 = 1_000;
/// Our flag still ON this long after we set it = our event file is not loaded: write the game's flag ourselves.
pub(crate) const FALLBACK_MS: u64 = 1_500;
/// The morning get-up is asked for at most this long.
pub(crate) const GETUP_MS: u64 = 8_000;
/// Rule v1 stays off this long around our own get-up.
pub(crate) const GETUP_EXEMPT_MS: u64 = 12_000;
/// Spots are taken while the day-1 clock is before this (players have landed and spread; the circle has not pulled them in).
pub(crate) const SPOT_BEFORE: ClockTime = ClockTime { h: 12, m: 0, s: 0 };
/// How often the spot is refreshed.
const SPOT_EVERY_MS: u64 = 2_000;
/// M5-9: how long a dead LOCAL is waited for after the release (the comeback takes about 5 s; the gate fallback adds 2 s).
pub(crate) const DEAD_WAIT_MS: u64 = 40_000;
/// A morning move waits while the clock still reads this hour or later (the night clock holds at 23:59:59 until the Day-2 reset):
/// S1 solo run 2026-10-23, a move before the reset put LOCAL outside the closed circle and the storm killed them.
pub(crate) const MOVE_AFTER_HOUR: u8 = 20;
/// The longest a morning move waits for the Day-2 clock.
pub(crate) const MOVE_WAIT_MS: u64 = 90_000;
/// M5-9: a day-1 position is sampled this often (while LOCAL is on the ground).
const PLACE_EVERY_MS: u64 = 5_000;
/// A place counts only after LOCAL moved slower than [`PLACE_MAX_SPEED`] this long: the start hawk flies at ~17 m/s with the ground
/// byte set (MEMORY_MAP §8), so ride positions in the air must never enter the pool (S1 solo run 2026-10-23: a y 210 pick).
const PLACE_SETTLE_MS: u64 = 2_000;
/// Metres per second (horizontal) between two ticks; a sprint is about 7.
const PLACE_MAX_SPEED: f32 = 9.0;
/// M5-9: a sampled position is kept only when it is at least this far from every kept one (metres).
pub(crate) const PLACE_MIN_APART: f32 = 100.0;
/// M5-9: the pool keeps this many positions (the oldest goes first).
pub(crate) const PLACE_POOL: usize = 64;
/// M5-9: the morning prefers a place at least this far from where LOCAL respawned (metres).
pub(crate) const PLACE_FAR_FROM_RESPAWN: f32 = 150.0;
/// The host's session player id has this in its upper half (`0x6001FFFFFFFF`; FS-0, run 21, M5-4 runs 1-2).
pub(crate) const HOST_ID_HI: u64 = 0x6001;

/// What the night match asks the main loop to do (besides our flags, which go through [`Ctx::flags`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum NightAct {
    /// Hold LOCAL's bleed-out clock at this value (LOCAL downed only).
    HoldClock(f32),
    /// Ask the game to stand LOCAL up (LOCAL downed only).
    GetUp,
    /// Keep rule v1 off for this long (our own get-up).
    ExemptGetup(u64),
    /// Who may revive LOCAL.
    Revive(ReviveMode),
    /// The rival-hit penalty floor (`None` = the day value).
    ClockFloor(Option<f32>),
    /// Move LOCAL to this world position.
    Teleport([f32; 3]),
    /// Write one of the game's flags (`game::flags::MATCH_WRITABLE`).
    GameFlag(u32, bool),
    /// M5-9: arm or disarm the hold of a dead LOCAL's comeback (`game::deathhold::set_armed`).
    DeathHold(bool),
    /// M5-9: call the held comeback step now (`game::deathhold::request_release`); nothing happens when none is held.
    ReleaseDeath,
}

/// How a night ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Decision {
    /// The last team standing (`None`: everyone went down at once).
    pub winner: Option<u64>,
    pub at_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct NightRun {
    n: u8,
    teams_at_start: usize,
    lone_since: Option<u64>,
    decided: Option<Decision>,
    eliminated: bool,
    /// LOCAL lost this night (eliminated, or not on the winning team).
    out: bool,
    /// Since when LOCAL is down (the backup elimination timer).
    knocked_since: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
struct Morning {
    since_ms: u64,
    out: bool,
    getup_asked: bool,
    moved: bool,
    /// M5-9: LOCAL was dead (HP 0) at the morning and the release was asked.
    released: bool,
}

/// What the morning step reads besides LOCAL.
struct MorningIn<'a> {
    local: Option<Fighter>,
    spot: Option<[f32; 3]>,
    solo: bool,
    /// LOCAL's world position now.
    pos: Option<[f32; 3]>,
    places: &'a VecDeque<[f32; 3]>,
    /// The play-area clock still reads the night (the game resets it to the Day-2 morning ~20 s after 7503; until then the circle is
    /// closed and the storm hurts anyone outside it). No clock read = not waiting.
    clock_night: bool,
}

/// The pure decision core (unit tested).
#[derive(Debug, Default)]
pub(crate) struct NightCore {
    night: Option<NightRun>,
    morning: Option<Morning>,
    spot: Option<[f32; 3]>,
    last_spot_ms: Option<u64>,
    match_over: bool,
    in_expedition: bool,
    status: String,
    /// Dev console `night_count 0`: the night never ends by count (two-window tests of the elimination).
    count_off: bool,
    /// Since when each of our flags (night end, win, lose) has been ON (the fallback).
    stuck: [Option<u64>; 3],
    /// M5-9: what the die-at-once flag was last wanted to be (`None` = not decided yet).
    die_want: Option<bool>,
    /// M5-9: the death hold is armed (what [`NightAct::DeathHold`] last said).
    hold_on: bool,
    /// M5-9: day-1 ground positions, at least [`PLACE_MIN_APART`] apart (ring of [`PLACE_POOL`]).
    places: VecDeque<[f32; 3]>,
    last_place_ms: Option<u64>,
    /// The previous tick's ground position (the speed check) and since when LOCAL has been slow.
    place_prev: Option<(u64, [f32; 3])>,
    slow_since: Option<u64>,
    /// M5-9: xorshift state for the random place (0 = not seeded; the feature seeds it from the clock).
    rng: u64,
}

/// M5-9: the next value of an xorshift64 generator, below `n` (`n` > 0).
pub(crate) fn next_below(state: &mut u64, n: usize) -> usize {
    let mut x = if *state == 0 {
        0x9E37_79B9_7F4A_7C15
    } else {
        *state
    };
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    usize::try_from(x % (n.max(1) as u64)).unwrap_or(0)
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// M5-9: add `p` to the pool when it is at least [`PLACE_MIN_APART`] from every kept position; true when kept.
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

/// M5-9: a random place from the pool: one at least [`PLACE_FAR_FROM_RESPAWN`] from `from` when there is one, else any, else `None`.
/// `pick(n)` returns an index below `n`.
pub(crate) fn pick_place(
    pool: &VecDeque<[f32; 3]>,
    from: Option<[f32; 3]>,
    mut pick: impl FnMut(usize) -> usize,
) -> Option<[f32; 3]> {
    let far: Vec<[f32; 3]> = from
        .map(|f| {
            pool.iter()
                .copied()
                .filter(|p| dist(*p, f) >= PLACE_FAR_FROM_RESPAWN)
                .collect()
        })
        .unwrap_or_default();
    let list: Vec<[f32; 3]> = if far.is_empty() {
        pool.iter().copied().collect()
    } else {
        far
    };
    if list.is_empty() {
        return None;
    }
    list.get(pick(list.len()) % list.len()).copied()
}

fn teams_of<'a>(fs: impl Iterator<Item = &'a Fighter>) -> Vec<u64> {
    let mut t: Vec<u64> = fs.map(|f| f.team).collect();
    t.sort_unstable();
    t.dedup();
    t
}

fn before(t: ClockTime, limit: ClockTime) -> bool {
    (t.h, t.m, t.s) < (limit.h, limit.m, limit.s)
}

impl NightCore {
    /// One tick. Our flags go to `flags`, everything else is returned.
    pub(crate) fn step(
        &mut self,
        now_ms: u64,
        world: &World,
        flags: &mut Vec<(u32, bool)>,
    ) -> (Vec<NightAct>, Vec<String>) {
        let mut acts = Vec::new();
        let mut lines = Vec::new();
        let fighters: Vec<Fighter> = world.fighters.iter().flatten().copied().collect();
        let local = fighters.iter().find(|f| f.local).copied();
        if !world.in_expedition() {
            if self.in_expedition {
                // Back in the Roundtable (or a load): forget the expedition, put the rules back.
                let die_on = self.die_want == Some(true);
                let rng = self.rng;
                *self = NightCore::default();
                self.rng = rng;
                acts.push(NightAct::ClockFloor(None));
                acts.push(NightAct::Revive(ReviveMode::Rivals));
                acts.push(NightAct::DeathHold(false));
                if die_on {
                    flags.push((FLAG_DIE, false));
                }
            }
            return (acts, lines);
        }
        // A tick without the flags or without LOCAL in the player list decides nothing and forgets nothing.
        let (Some(nf), Some(_)) = (world.night, local) else {
            return (acts, lines);
        };
        self.in_expedition = true;
        self.record_spot(now_ms, world, &nf, local);
        self.record_place(now_ms, world, &nf, local);

        let n = if nf.day2 { 2 } else { 1 };
        let done = if n == 1 {
            nf.night1_done
        } else {
            nf.night2_done
        };
        if nf.night && !done && !self.match_over && self.night.as_ref().map(|r| r.n) != Some(n) {
            let teams = teams_of(fighters.iter()).len();
            lines.push(format!(
                "night_match: night {n} begins: {} player(s), {teams} team(s){}",
                fighters.len(),
                if teams >= 2 {
                    ""
                } else {
                    " (fewer than 2: the night does not end by count)"
                }
            ));
            self.night = Some(NightRun {
                n,
                teams_at_start: teams,
                lone_since: None,
                decided: None,
                eliminated: false,
                out: false,
                knocked_since: None,
            });
            acts.push(NightAct::ClockFloor(Some(NIGHT_CLOCK_FLOOR)));
        }
        let host = local.is_some_and(|l| l.id >> 32 == HOST_ID_HI);
        if let Some(run) = self.night.as_mut() {
            let count = !self.count_off;
            Self::night_step(
                run, now_ms, &nf, &fighters, local, host, count, flags, &mut acts, &mut lines,
            );
            if run.n == 2 && run.decided.is_some() {
                self.match_over = true;
            }
        }
        self.fallback(now_ms, &nf, host, flags, &mut acts, &mut lines);
        // The morning after night 1 (7502 in the host, 7503 everywhere).
        if let Some(run) = self.night.as_ref().filter(|r| r.n == 1 && nf.night1_done) {
            let out = run.out || run.eliminated;
            lines.push(format!(
                "night_match: night 1 is over; morning: get up if down{}",
                if out {
                    ", then back to my own spot"
                } else {
                    ""
                }
            ));
            self.morning = Some(Morning {
                since_ms: now_ms,
                out,
                getup_asked: false,
                moved: false,
                released: false,
            });
            self.night = None;
            acts.push(NightAct::ClockFloor(None));
            acts.push(NightAct::Revive(ReviveMode::Anyone));
            acts.push(NightAct::ExemptGetup(GETUP_EXEMPT_MS));
        }
        if let Some(m) = self.morning.as_mut() {
            let inp = MorningIn {
                local,
                spot: self.spot,
                solo: world.solo(),
                pos: world.local_pos.map(|p| p.0),
                places: &self.places,
                clock_night: world.clock.is_some_and(|c| c.time.h >= MOVE_AFTER_HOUR),
            };
            if Self::morning_step(m, now_ms, &inp, &mut self.rng, &mut acts, &mut lines) {
                self.morning = None;
                acts.push(NightAct::Revive(ReviveMode::Rivals));
            }
        }
        self.solo_death(world, &nf, local, flags, &mut acts, &mut lines);
        self.status = self.describe(&fighters, &nf);
        (acts, lines)
    }

    fn record_spot(&mut self, now_ms: u64, world: &World, nf: &NightFlags, local: Option<Fighter>) {
        let day1_morning = !nf.day2
            && !nf.night
            && !nf.night1_done
            && world.clock.is_some_and(|c| before(c.time, SPOT_BEFORE));
        let (Some(l), Some((pos, true))) = (local, world.local_pos) else {
            return;
        };
        if !day1_morning || l.state != 0 || l.hp == 0 {
            return;
        }
        if self
            .last_spot_ms
            .is_some_and(|t| now_ms.saturating_sub(t) < SPOT_EVERY_MS)
        {
            return;
        }
        self.last_spot_ms = Some(now_ms);
        self.spot = Some(pos);
    }

    /// M5-9: day-1 ground positions for the morning's random place.
    fn record_place(
        &mut self,
        now_ms: u64,
        world: &World,
        nf: &NightFlags,
        local: Option<Fighter>,
    ) {
        let (Some(l), Some((pos, true))) = (local, world.local_pos) else {
            return;
        };
        if nf.day2 || nf.night || nf.night1_done || l.state != 0 || l.hp == 0 {
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

    /// M5-9: the die-at-once flag and the hold of a dead LOCAL's comeback (Solo only).
    fn solo_death(
        &mut self,
        world: &World,
        nf: &NightFlags,
        local: Option<Fighter>,
        flags: &mut Vec<(u32, bool)>,
        acts: &mut Vec<NightAct>,
        lines: &mut Vec<String>,
    ) {
        let solo = world.solo();
        let live_night = self.night.as_ref().filter(|r| r.decided.is_none());
        let (want, why) = if !solo {
            (false, "not Solo")
        } else if !nf.night || live_night.is_none() {
            (false, "not a night in play")
        } else if world.local_revive_item {
            (false, "LOCAL holds a revive item")
        } else if !local.is_some_and(|l| l.state == 0 && l.hp > 0) {
            (false, "LOCAL is down or dead")
        } else {
            (true, "Solo night, LOCAL up, no revive item")
        };
        if self.die_want != Some(want) {
            if self.die_want.is_some() || want {
                lines.push(format!(
                    "night_match: die-at-once {} ({why})",
                    if want { "ON" } else { "OFF" }
                ));
            }
            self.die_want = Some(want);
        }
        // Written only when the wanted value differs from the flag (read back every tick).
        if nf.our_die != want {
            flags.push((FLAG_DIE, want));
        }
        let hold = solo && self.night.is_some();
        if hold != self.hold_on {
            self.hold_on = hold;
            acts.push(NightAct::DeathHold(hold));
        }
    }

    /// Our flag still ON after [`FALLBACK_MS`] = our event file is not loaded (Nucleus): write the game's flag instead. The night
    /// end only in the host's game during night 1 (our event acts only then); the endings any time.
    fn fallback(
        &mut self,
        now_ms: u64,
        nf: &NightFlags,
        host: bool,
        flags: &mut Vec<(u32, bool)>,
        acts: &mut Vec<NightAct>,
        lines: &mut Vec<String>,
    ) {
        let cases = [
            (
                nf.our_night_end && nf.night && !nf.day2 && host,
                FLAG_NIGHT_END,
                NIGHT1_DONE,
            ),
            (nf.our_win, FLAG_WIN, GAME_CLEAR),
            (nf.our_lose, FLAG_LOSE, GAME_OVER),
        ];
        for (slot, (on, flag, game)) in self.stuck.iter_mut().zip(cases) {
            if !on {
                *slot = None;
                continue;
            }
            let since = *slot.get_or_insert(now_ms);
            if now_ms.saturating_sub(since) >= FALLBACK_MS {
                *slot = None;
                lines.push(format!(
                    "night_match: our flag {flag} still ON after {FALLBACK_MS} ms (event file not loaded?): writing the game's {game}"
                ));
                flags.push((flag, false));
                acts.push(NightAct::GameFlag(game, true));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn night_step(
        run: &mut NightRun,
        now_ms: u64,
        nf: &NightFlags,
        fighters: &[Fighter],
        local: Option<Fighter>,
        host: bool,
        count: bool,
        flags: &mut Vec<(u32, bool)>,
        acts: &mut Vec<NightAct>,
        lines: &mut Vec<String>,
    ) {
        // Elimination: LOCAL's own clock, before the game reaches 0; or down longer than a full bleed-out (the backup, e.g. when
        // the vanilla night freeze holds the clock: Nucleus runs without our event file).
        match local.filter(|l| l.state == 1) {
            Some(l) => {
                let since = *run.knocked_since.get_or_insert(now_ms);
                let down_ms = now_ms.saturating_sub(since);
                if !run.eliminated
                    && run.decided.is_none()
                    && (l.clock <= ELIM_CLOCK || down_ms >= KNOCK_BACKUP_MS)
                {
                    run.eliminated = true;
                    run.out = true;
                    lines.push(format!(
                        "night_match: LOCAL eliminated (bleed-out {:.2} s left, down {:.1} s): held down until the morning, nobody can revive",
                        l.clock,
                        down_ms as f32 / 1000.0
                    ));
                    acts.push(NightAct::Revive(ReviveMode::Nobody));
                }
            }
            None => run.knocked_since = None,
        }
        // The count.
        if count && run.decided.is_none() && run.teams_at_start >= 2 {
            let standing = teams_of(fighters.iter().filter(|f| f.standing()));
            if nf.reviving || standing.len() > 1 {
                run.lone_since = None;
            } else {
                let since = *run.lone_since.get_or_insert(now_ms);
                if now_ms.saturating_sub(since) >= DECIDE_MS {
                    let winner = standing.first().copied();
                    run.decided = Some(Decision {
                        winner,
                        at_ms: now_ms,
                    });
                    let mine = local.map(|l| l.team);
                    let won = winner.is_some() && winner == mine;
                    run.out = run.out || !won;
                    lines.push(format!(
                        "night_match: night {} decided: {} (this game's player {}{})",
                        run.n,
                        winner.map_or("nobody left standing".into(), |w| format!(
                            "team {w:#X} is the last one standing"
                        )),
                        if won { "won" } else { "lost" },
                        if host { ", host" } else { "" }
                    ));
                    if won && local.is_some_and(|l| l.standing()) {
                        flags.push((FLAG_HEAL, true));
                    }
                    if run.n == 1 {
                        if host {
                            flags.push((FLAG_NIGHT_END, true));
                        }
                    } else {
                        flags.push((if won { FLAG_WIN } else { FLAG_LOSE }, true));
                    }
                }
            }
        }
        // Held down: eliminated, or still knocked once the night is decided (nobody may bleed out and respawn before the morning).
        if local.is_some_and(|l| l.state == 1) && (run.eliminated || run.decided.is_some()) {
            acts.push(NightAct::HoldClock(HOLD_CLOCK));
        }
    }

    /// True when the morning work is over.
    fn morning_step(
        m: &mut Morning,
        now_ms: u64,
        inp: &MorningIn,
        rng: &mut u64,
        acts: &mut Vec<NightAct>,
        lines: &mut Vec<String>,
    ) -> bool {
        let age = now_ms.saturating_sub(m.since_ms);
        let Some(l) = inp.local else {
            return age >= GETUP_MS;
        };
        if l.state == 1 {
            if age >= GETUP_MS {
                lines.push("night_match: morning: LOCAL is still down after the get-up requests; giving up".into());
                return true;
            }
            if !m.getup_asked {
                m.getup_asked = true;
                lines.push("night_match: morning: asking the game to stand LOCAL up".into());
            }
            acts.push(NightAct::GetUp);
            return false;
        }
        // M5-9: a Solo player who died at night (held dead): let the game's comeback run, then move LOCAL to a random place.
        if inp.solo && l.hp == 0 {
            if !m.released {
                m.released = true;
                lines.push(
                    "night_match: morning: LOCAL is dead; releasing the held comeback".into(),
                );
                acts.push(NightAct::ReleaseDeath);
            }
            if age >= DEAD_WAIT_MS {
                lines.push(
                    "night_match: morning: LOCAL did not come back in time; giving up".into(),
                );
                return true;
            }
            return false;
        }
        if m.released && !m.moved {
            if l.state != 0 {
                return age >= DEAD_WAIT_MS;
            }
            if inp.clock_night {
                return age >= MOVE_WAIT_MS;
            }
            m.moved = true;
            let place = pick_place(inp.places, inp.pos, |n| next_below(rng, n)).or(inp.spot);
            match place {
                Some(p) => {
                    acts.push(NightAct::Teleport(p));
                    lines.push(format!(
                        "night_match: morning: LOCAL is back; moved to a random day-1 place ({:.1}, {:.1}, {:.1}), {} kept",
                        p[0], p[1], p[2], inp.places.len()
                    ));
                }
                None => lines.push(
                    "night_match: morning: LOCAL is back but no place was recorded; stays here"
                        .into(),
                ),
            }
            return true;
        }
        if !m.out || m.moved {
            return true;
        }
        if l.state != 0 || l.hp == 0 {
            // Still getting up: wait (bounded).
            return age >= GETUP_MS + 5_000;
        }
        if inp.clock_night {
            return age >= MOVE_WAIT_MS;
        }
        m.moved = true;
        match inp.spot {
            Some(p) => {
                acts.push(NightAct::Teleport(p));
                lines.push(format!(
                    "night_match: morning: LOCAL lost the night; back to its own spot ({:.1}, {:.1}, {:.1})",
                    p[0], p[1], p[2]
                ));
            }
            None => lines.push(
                "night_match: morning: LOCAL lost the night but no spot was recorded; stays here"
                    .into(),
            ),
        }
        true
    }

    fn describe(&self, fighters: &[Fighter], nf: &NightFlags) -> String {
        let standing = teams_of(fighters.iter().filter(|f| f.standing())).len();
        let teams = teams_of(fighters.iter()).len();
        let phase = if self.match_over {
            "match over".to_string()
        } else if let Some(r) = &self.night {
            format!(
                "night {}{}{}",
                r.n,
                if r.eliminated {
                    ", LOCAL eliminated"
                } else {
                    ""
                },
                match r.decided {
                    Some(d) => format!(
                        ", decided (winner {})",
                        d.winner.map_or("none".into(), |w| format!("{w:#X}"))
                    ),
                    None => String::new(),
                }
            )
        } else if self.morning.is_some() {
            "morning".to_string()
        } else {
            format!("day {}", if nf.day2 { 2 } else { 1 })
        };
        format!(
            "{phase}; {} player(s), {teams} team(s), {standing} standing; die-at-once {}; spot {}",
            fighters.len(),
            if self.die_want == Some(true) {
                "on"
            } else {
                "off"
            },
            self.spot.map_or("none".into(), |p| format!(
                "({:.0}, {:.0}, {:.0})",
                p[0], p[1], p[2]
            ))
        )
    }
}

static STATUS: Mutex<String> = Mutex::new(String::new());
/// Dev console `night_count 0|1` (on by default; resets at launch).
static COUNT: AtomicBool = AtomicBool::new(true);

/// Console `night_count [0|1]`: switch the end-by-count (`Some`) and report it.
pub(crate) fn count_cmd(on: Option<bool>) -> String {
    if let Some(on) = on {
        COUNT.store(on, Ordering::Relaxed);
        tracing::info!("night_match: count {}", if on { "ON" } else { "OFF" });
    }
    if COUNT.load(Ordering::Relaxed) {
        "night_count 1: a night ends when one team is left standing".into()
    } else {
        "night_count 0: a night never ends by count (console night_end ends it)".into()
    }
}

/// `night_match: <phase>; …` (console `night_status`).
pub(crate) fn status_line() -> String {
    let s = STATUS.lock().unwrap_or_else(PoisonError::into_inner);
    format!(
        "night_match: {}{}",
        if s.is_empty() {
            "not in an expedition"
        } else {
            &s
        },
        crate::game::deathhold::status_short().map_or(String::new(), |d| format!("; {d}"))
    )
}

pub(crate) struct NightMatch {
    core: NightCore,
    started: Instant,
}

impl Default for NightMatch {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        NightMatch {
            core: NightCore {
                rng: seed | 1,
                ..NightCore::default()
            },
            started: Instant::now(),
        }
    }
}

impl Feature for NightMatch {
    fn name(&self) -> &'static str {
        "night_match"
    }

    fn tick(&mut self, world: &World, ctx: &mut Ctx) {
        let now_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.core.count_off = !COUNT.load(Ordering::Relaxed);
        let (acts, lines) = self.core.step(now_ms, world, &mut ctx.flags);
        for l in lines {
            tracing::info!("{l}");
        }
        ctx.night.extend(acts);
        let mut s = STATUS.lock().unwrap_or_else(PoisonError::into_inner);
        if *s != self.core.status {
            s.clone_from(&self.core.status);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::views::{ClockState, Place};
    use crate::game::world::MAX_FIGHTERS;

    const ME: u64 = 0x6001_FFFF_FFFF;
    const P2: u64 = 0x6002_FFFF_FFFF;
    const P3: u64 = 0x6003_FFFF_FFFF;

    fn fighter(id: u64, local: bool, state: u8, hp: u32, clock: f32) -> Fighter {
        Fighter {
            addr: 0x10_0000 + (id >> 32) as usize,
            id,
            team: id,
            local,
            state,
            hp,
            clock,
        }
    }

    fn world(fs: &[Fighter], nf: NightFlags) -> World {
        let mut fighters = [None; MAX_FIGHTERS];
        for (o, f) in fighters.iter_mut().zip(fs) {
            *o = Some(*f);
        }
        World {
            wcm_instance: 0x5_0000,
            local: Some(0x10_0000),
            place: Some(Place::Expedition),
            night: Some(nf),
            fighters,
            local_pos: Some(([10.0, 20.0, 30.0], true)),
            clock: Some(ClockState {
                time: ClockTime { h: 9, m: 0, s: 0 },
                speed: 72.0,
                running: true,
                mailbox_free: true,
            }),
            ..World::default()
        }
    }

    fn night() -> NightFlags {
        NightFlags {
            night: true,
            ..NightFlags::default()
        }
    }

    fn run(core: &mut NightCore, ms: u64, w: &World) -> (Vec<NightAct>, Vec<(u32, bool)>) {
        let mut flags = Vec::new();
        let (acts, _) = core.step(ms, w, &mut flags);
        (acts, flags)
    }

    #[test]
    fn the_host_ends_night_one_when_one_player_is_left() {
        let mut c = NightCore::default();
        let up = [
            fighter(ME, true, 0, 240, 0.0),
            fighter(P2, false, 0, 240, 0.0),
        ];
        let (acts, _) = run(&mut c, 0, &world(&up, night()));
        assert!(acts.contains(&NightAct::ClockFloor(Some(NIGHT_CLOCK_FLOOR))));
        let knocked = [
            fighter(ME, true, 0, 240, 0.0),
            fighter(P2, false, 1, 0, 14.0),
        ];
        let (_, flags) = run(&mut c, 100, &world(&knocked, night()));
        assert!(flags.is_empty(), "one reading is not enough");
        let (_, flags) = run(&mut c, 1_200, &world(&knocked, night()));
        assert_eq!(flags, vec![(FLAG_HEAL, true), (FLAG_NIGHT_END, true)]);
        // Decided once.
        let (_, flags) = run(&mut c, 1_300, &world(&knocked, night()));
        assert!(flags.is_empty());
    }

    #[test]
    fn a_client_that_lost_neither_heals_nor_ends_the_night_and_holds_its_knocked_player() {
        let mut c = NightCore::default();
        let me = |state, hp| fighter(P2, true, state, hp, 13.0);
        run(
            &mut c,
            0,
            &world(&[me(0, 240), fighter(ME, false, 0, 240, 0.0)], night()),
        );
        let w = world(&[me(1, 0), fighter(ME, false, 0, 240, 0.0)], night());
        run(&mut c, 100, &w);
        let (acts, flags) = run(&mut c, 1_200, &w);
        assert!(flags.is_empty());
        assert!(acts.contains(&NightAct::HoldClock(HOLD_CLOCK)));
    }

    #[test]
    fn a_knocked_player_is_eliminated_at_the_end_of_the_bleed_out_and_held() {
        let mut c = NightCore::default();
        let three = |me_state, me_clock| {
            [
                fighter(
                    ME,
                    true,
                    me_state,
                    if me_state == 1 { 0 } else { 240 },
                    me_clock,
                ),
                fighter(P2, false, 0, 240, 0.0),
                fighter(P3, false, 0, 240, 0.0),
            ]
        };
        run(&mut c, 0, &world(&three(0, 0.0), night()));
        let (acts, _) = run(&mut c, 100, &world(&three(1, 5.0), night()));
        assert!(
            !acts.contains(&NightAct::HoldClock(HOLD_CLOCK)),
            "knocked, not out yet"
        );
        let (acts, _) = run(&mut c, 200, &world(&three(1, 0.95), night()));
        assert!(acts.contains(&NightAct::Revive(ReviveMode::Nobody)));
        assert!(acts.contains(&NightAct::HoldClock(HOLD_CLOCK)));
        // Held every tick, even once the clock is back at 15.
        let (acts, flags) = run(&mut c, 300, &world(&three(1, 15.0), night()));
        assert!(acts.contains(&NightAct::HoldClock(HOLD_CLOCK)));
        assert!(flags.is_empty(), "two rivals still stand");
    }

    #[test]
    fn no_decision_while_a_revive_item_is_pending_or_with_one_team() {
        let mut c = NightCore::default();
        let up = [
            fighter(ME, true, 0, 240, 0.0),
            fighter(P2, false, 0, 240, 0.0),
        ];
        run(&mut c, 0, &world(&up, night()));
        let knocked = [
            fighter(ME, true, 0, 240, 0.0),
            fighter(P2, false, 1, 0, 14.0),
        ];
        let reviving = NightFlags {
            reviving: true,
            ..night()
        };
        run(&mut c, 100, &world(&knocked, reviving));
        let (_, flags) = run(&mut c, 5_000, &world(&knocked, reviving));
        assert!(flags.is_empty());

        let mut solo = NightCore::default();
        let me = [fighter(ME, true, 1, 0, 3.0)];
        run(&mut solo, 0, &world(&me, night()));
        let (_, flags) = run(&mut solo, 5_000, &world(&me, night()));
        assert!(flags.is_empty(), "a solo night never ends by count");
    }

    #[test]
    fn teammates_count_as_one_team() {
        let mut c = NightCore::default();
        let mut a = fighter(ME, true, 0, 240, 0.0);
        let mut b = fighter(P2, false, 1, 0, 10.0);
        let mut r = fighter(P3, false, 0, 240, 0.0);
        a.team = 1;
        b.team = 1;
        r.team = 2;
        run(&mut c, 0, &world(&[a, b, r], night()));
        let (_, flags) = run(&mut c, 2_000, &world(&[a, b, r], night()));
        assert!(flags.is_empty(), "team 1 still has a player up");
        r.state = 1;
        r.hp = 0;
        run(&mut c, 2_100, &world(&[a, b, r], night()));
        let (_, flags) = run(&mut c, 3_200, &world(&[a, b, r], night()));
        assert_eq!(flags, vec![(FLAG_HEAL, true), (FLAG_NIGHT_END, true)]);
    }

    #[test]
    fn night_two_gives_each_game_its_own_ending() {
        let n2 = NightFlags {
            night: true,
            day2: true,
            ..NightFlags::default()
        };
        let mut win = NightCore::default();
        let mut lose = NightCore::default();
        let host_view = |p2_state| {
            [
                fighter(ME, true, 0, 240, 0.0),
                fighter(
                    P2,
                    false,
                    p2_state,
                    if p2_state == 1 { 0 } else { 240 },
                    9.0,
                ),
            ]
        };
        let client_view = |p2_state| {
            [
                fighter(P2, true, p2_state, if p2_state == 1 { 0 } else { 240 }, 9.0),
                fighter(ME, false, 0, 240, 0.0),
            ]
        };
        run(&mut win, 0, &world(&host_view(0), n2));
        run(&mut lose, 0, &world(&client_view(0), n2));
        run(&mut win, 100, &world(&host_view(1), n2));
        run(&mut lose, 100, &world(&client_view(1), n2));
        let (_, fw) = run(&mut win, 1_200, &world(&host_view(1), n2));
        let (_, fl) = run(&mut lose, 1_200, &world(&client_view(1), n2));
        assert_eq!(fw, vec![(FLAG_HEAL, true), (FLAG_WIN, true)]);
        assert_eq!(fl, vec![(FLAG_LOSE, true)]);
        assert!(
            !fw.contains(&(FLAG_NIGHT_END, true)),
            "night 2 ends the match, not the night"
        );
    }

    #[test]
    fn without_our_event_file_the_game_flag_is_written() {
        let mut c = NightCore::default();
        let up = [
            fighter(ME, true, 0, 240, 0.0),
            fighter(P2, false, 0, 240, 0.0),
        ];
        run(&mut c, 0, &world(&up, night()));
        let knocked = [
            fighter(ME, true, 0, 240, 0.0),
            fighter(P2, false, 1, 0, 14.0),
        ];
        run(&mut c, 100, &world(&knocked, night()));
        run(&mut c, 1_200, &world(&knocked, night()));
        let stuck = NightFlags {
            our_night_end: true,
            ..night()
        };
        let (acts, _) = run(&mut c, 1_300, &world(&knocked, stuck));
        assert!(!acts.iter().any(|a| matches!(a, NightAct::GameFlag(..))));
        let (acts, flags) = run(&mut c, 2_800, &world(&knocked, stuck));
        assert!(acts.contains(&NightAct::GameFlag(NIGHT1_DONE, true)));
        assert_eq!(flags, vec![(FLAG_NIGHT_END, false)]);
        // With the event file the flag is already OFF: nothing more.
        let mut c = NightCore::default();
        run(&mut c, 0, &world(&up, night()));
        run(&mut c, 100, &world(&knocked, night()));
        run(&mut c, 1_200, &world(&knocked, night()));
        let (acts, _) = run(&mut c, 2_800, &world(&knocked, night()));
        assert!(!acts.iter().any(|a| matches!(a, NightAct::GameFlag(..))));
    }

    #[test]
    fn the_fallback_never_ends_a_night_by_day_or_from_a_client() {
        let mut c = NightCore::default();
        let up = [
            fighter(ME, true, 0, 240, 0.0),
            fighter(P2, false, 0, 240, 0.0),
        ];
        let by_day = NightFlags {
            our_night_end: true,
            ..NightFlags::default()
        };
        run(&mut c, 0, &world(&up, by_day));
        let (acts, _) = run(&mut c, 5_000, &world(&up, by_day));
        assert!(acts.is_empty());
        let mut client = NightCore::default();
        let view = [
            fighter(P2, true, 0, 240, 0.0),
            fighter(ME, false, 0, 240, 0.0),
        ];
        let stuck = NightFlags {
            our_night_end: true,
            ..night()
        };
        run(&mut client, 0, &world(&view, stuck));
        let (acts, _) = run(&mut client, 5_000, &world(&view, stuck));
        assert!(!acts.iter().any(|a| matches!(a, NightAct::GameFlag(..))));
        // The endings fall back in any game (console `flag 10007411 1` in Nucleus).
        let win = NightFlags {
            our_win: true,
            ..NightFlags::default()
        };
        run(&mut client, 6_000, &world(&view, win));
        let (acts, _) = run(&mut client, 7_600, &world(&view, win));
        assert!(acts.contains(&NightAct::GameFlag(GAME_CLEAR, true)));
    }

    #[test]
    fn with_the_count_off_a_knocked_player_is_eliminated_by_the_backup_timer() {
        let frozen = |state: u8| {
            [
                fighter(ME, true, state, if state == 1 { 0 } else { 240 }, 15.0),
                fighter(P2, false, 0, 240, 0.0),
            ]
        };
        let mut c = NightCore {
            count_off: true,
            ..NightCore::default()
        };
        run(&mut c, 0, &world(&frozen(0), night()));
        run(&mut c, 1_000, &world(&frozen(1), night()));
        let (acts, flags) = run(&mut c, 16_900, &world(&frozen(1), night()));
        assert!(flags.is_empty(), "no decision with the count off");
        assert!(
            !acts.contains(&NightAct::Revive(ReviveMode::Nobody)),
            "15.9 s down: not yet"
        );
        let (acts, _) = run(&mut c, 17_000, &world(&frozen(1), night()));
        assert!(acts.contains(&NightAct::Revive(ReviveMode::Nobody)));
        assert!(acts.contains(&NightAct::HoldClock(HOLD_CLOCK)));
        // A get-up in between restarts the timer.
        let mut c = NightCore {
            count_off: true,
            ..NightCore::default()
        };
        run(&mut c, 0, &world(&frozen(1), night()));
        run(&mut c, 10_000, &world(&frozen(0), night()));
        run(&mut c, 11_000, &world(&frozen(1), night()));
        let (acts, _) = run(&mut c, 20_000, &world(&frozen(1), night()));
        assert!(!acts.contains(&NightAct::Revive(ReviveMode::Nobody)));
    }

    #[test]
    fn the_morning_stands_the_loser_up_and_moves_it_to_its_spot() {
        let mut c = NightCore::default();
        let day = NightFlags::default();
        let me_up = fighter(P2, true, 0, 240, 0.0);
        let rival = fighter(ME, false, 0, 240, 0.0);
        // Day 1 before noon: the spot is taken.
        run(&mut c, 0, &world(&[me_up, rival], day));
        let mut w = world(&[me_up, rival], night());
        w.local_pos = Some(([500.0, 0.0, 500.0], true));
        run(&mut c, 10_000, &w);
        let knocked = fighter(P2, true, 1, 0, 12.0);
        run(&mut c, 10_100, &world(&[knocked, rival], night()));
        run(&mut c, 11_200, &world(&[knocked, rival], night()));
        // The host ended the night: 7503 reached this game.
        let morning = NightFlags {
            night1_done: true,
            ..NightFlags::default()
        };
        let (acts, _) = run(&mut c, 11_500, &world(&[knocked, rival], morning));
        assert!(acts.contains(&NightAct::Revive(ReviveMode::Anyone)));
        assert!(acts.contains(&NightAct::ExemptGetup(GETUP_EXEMPT_MS)));
        assert!(acts.contains(&NightAct::GetUp));
        let getting_up = fighter(P2, true, 2, 120, 0.0);
        let (acts, _) = run(&mut c, 12_000, &world(&[getting_up, rival], morning));
        assert!(acts.is_empty());
        let up = fighter(P2, true, 0, 120, 0.0);
        let (acts, _) = run(&mut c, 15_000, &world(&[up, rival], morning));
        assert!(
            acts.contains(&NightAct::Teleport([10.0, 20.0, 30.0])),
            "the day-1 spot, not the night position"
        );
        assert!(acts.contains(&NightAct::Revive(ReviveMode::Rivals)));
    }

    #[test]
    fn the_winner_is_not_moved_and_the_roundtable_resets_everything() {
        let mut c = NightCore::default();
        let up = [
            fighter(ME, true, 0, 240, 0.0),
            fighter(P2, false, 0, 240, 0.0),
        ];
        run(&mut c, 0, &world(&up, NightFlags::default()));
        run(&mut c, 100, &world(&up, night()));
        let knocked = [
            fighter(ME, true, 0, 240, 0.0),
            fighter(P2, false, 1, 0, 14.0),
        ];
        run(&mut c, 200, &world(&knocked, night()));
        run(&mut c, 1_300, &world(&knocked, night()));
        let morning = NightFlags {
            night1_done: true,
            ..NightFlags::default()
        };
        let (acts, _) = run(&mut c, 1_400, &world(&knocked, morning));
        assert!(!acts
            .iter()
            .any(|a| matches!(a, NightAct::Teleport(_) | NightAct::GetUp)));
        let mut rt = world(&up, NightFlags::default());
        rt.place = Some(Place::Roundtable);
        let (acts, _) = run(&mut c, 2_000, &rt);
        assert!(acts.contains(&NightAct::ClockFloor(None)));
        assert!(c.night.is_none() && c.spot.is_none());
    }

    #[test]
    fn a_tick_without_flags_or_without_local_keeps_the_night() {
        let mut c = NightCore::default();
        let up = [
            fighter(ME, true, 0, 240, 0.0),
            fighter(P2, false, 0, 240, 0.0),
        ];
        run(&mut c, 0, &world(&up, night()));
        let mut no_flags = world(&up, night());
        no_flags.night = None;
        run(&mut c, 100, &no_flags);
        assert!(
            c.night.is_some(),
            "an unreadable flag tick is not the Roundtable"
        );
        // LOCAL missing from the list (a ChrSet hiccup): nobody is counted, nothing is decided.
        let only_rival = [fighter(P2, false, 0, 240, 0.0)];
        run(&mut c, 200, &world(&only_rival, night()));
        let (_, flags) = run(&mut c, 2_000, &world(&only_rival, night()));
        assert!(flags.is_empty());
        assert!(c.night.as_ref().is_some_and(|r| r.decided.is_none()));
    }

    #[test]
    fn the_spot_is_only_taken_on_day_one_before_noon() {
        let mut c = NightCore::default();
        let me = [fighter(ME, true, 0, 240, 0.0)];
        let mut w = world(&me, NightFlags::default());
        w.clock = Some(ClockState {
            time: ClockTime { h: 13, m: 0, s: 0 },
            speed: 72.0,
            running: true,
            mailbox_free: true,
        });
        run(&mut c, 0, &w);
        assert!(c.spot.is_none());
        w.clock = None;
        run(&mut c, 0, &w);
        assert!(c.spot.is_none());
        let w = world(&me, NightFlags::default());
        run(&mut c, 0, &w);
        assert_eq!(c.spot, Some([10.0, 20.0, 30.0]));
    }

    fn solo_world(fs: &[Fighter], nf: NightFlags) -> World {
        World {
            team_size: 1,
            ..world(fs, nf)
        }
    }

    fn at(w: &World, pos: [f32; 3]) -> World {
        World {
            local_pos: Some((pos, true)),
            ..*w
        }
    }

    #[test]
    fn solo_night_gives_the_die_at_once_flag_only_when_it_changes() {
        let mut c = NightCore::default();
        let me = [fighter(ME, true, 0, 240, 0.0)];
        // By day: nothing wanted, nothing written.
        let (acts, flags) = run(&mut c, 0, &solo_world(&me, NightFlags::default()));
        assert!(flags.is_empty() && !acts.contains(&NightAct::DeathHold(true)));
        // Nightfall: ON, the hold is armed.
        let (acts, flags) = run(&mut c, 100, &solo_world(&me, night()));
        assert_eq!(flags, vec![(FLAG_DIE, true)]);
        assert!(acts.contains(&NightAct::DeathHold(true)));
        // The flag reads back ON: no second write, no second arm.
        let on = NightFlags {
            our_die: true,
            ..night()
        };
        let (acts, flags) = run(&mut c, 150, &solo_world(&me, on));
        assert!(flags.is_empty() && !acts.contains(&NightAct::DeathHold(true)));
        // LOCAL dead (HP 0, state 0): OFF again, the hold stays armed.
        let dead = [fighter(ME, true, 0, 0, 0.0)];
        let (acts, flags) = run(&mut c, 200, &solo_world(&dead, on));
        assert_eq!(flags, vec![(FLAG_DIE, false)]);
        assert!(!acts.contains(&NightAct::DeathHold(false)));
    }

    #[test]
    fn the_die_at_once_flag_stays_off_in_duo_with_a_revive_item_by_day_and_after_the_decision() {
        let me = fighter(ME, true, 0, 240, 0.0);
        let rival = fighter(P2, false, 0, 240, 0.0);
        // Duo / Trio (team size 2, 3) and an unfilled team size.
        for size in [0, 2, 3] {
            let mut c = NightCore::default();
            let w = World {
                team_size: size,
                ..world(&[me], night())
            };
            let (acts, flags) = run(&mut c, 0, &w);
            assert!(flags.is_empty(), "team size {size}");
            assert!(
                !acts.contains(&NightAct::DeathHold(true)),
                "team size {size}"
            );
        }
        // A revive item at work.
        let mut c = NightCore::default();
        let w = World {
            local_revive_item: true,
            ..solo_world(&[me], night())
        };
        assert!(run(&mut c, 0, &w).1.is_empty());
        // Knocked.
        let mut c = NightCore::default();
        let knocked = [fighter(ME, true, 1, 0, 9.0)];
        assert!(run(&mut c, 0, &solo_world(&knocked, night())).1.is_empty());
        // Decided: a flag that was ON goes OFF.
        let mut c = NightCore::default();
        let two = [me, rival];
        let on = NightFlags {
            our_die: true,
            ..night()
        };
        run(&mut c, 0, &solo_world(&two, on));
        let down = [me, fighter(P2, false, 1, 0, 9.0)];
        run(&mut c, 100, &solo_world(&down, on));
        let (_, flags) = run(&mut c, 1_200, &solo_world(&down, on));
        assert!(flags.contains(&(FLAG_DIE, false)), "{flags:?}");
        assert!(flags.contains(&(FLAG_NIGHT_END, true)));
    }

    #[test]
    fn leaving_the_expedition_clears_the_flag_and_the_hold() {
        let mut c = NightCore::default();
        let me = [fighter(ME, true, 0, 240, 0.0)];
        run(&mut c, 0, &solo_world(&me, night()));
        let mut rt = solo_world(&me, NightFlags::default());
        rt.place = Some(Place::Roundtable);
        let (acts, flags) = run(&mut c, 100, &rt);
        assert!(acts.contains(&NightAct::DeathHold(false)));
        assert_eq!(flags, vec![(FLAG_DIE, false)]);
    }

    #[test]
    fn the_hold_is_armed_from_nightfall_to_the_morning_in_solo_only() {
        let mut c = NightCore::default();
        let me = [fighter(ME, true, 0, 240, 0.0)];
        let (acts, _) = run(&mut c, 0, &solo_world(&me, night()));
        assert!(acts.contains(&NightAct::DeathHold(true)));
        let morning = NightFlags {
            night1_done: true,
            ..NightFlags::default()
        };
        let (acts, _) = run(&mut c, 1_000, &solo_world(&me, morning));
        assert!(acts.contains(&NightAct::DeathHold(false)));
        assert!(
            !acts.contains(&NightAct::ReleaseDeath),
            "alive: nothing to release"
        );
        // Night 2: armed again.
        let n2 = NightFlags {
            night: true,
            day2: true,
            ..NightFlags::default()
        };
        let (acts, _) = run(&mut c, 2_000, &solo_world(&me, n2));
        assert!(acts.contains(&NightAct::DeathHold(true)));
    }

    #[test]
    fn a_dead_solo_player_is_released_once_at_the_morning_and_moved_to_a_far_day_place() {
        let mut c = NightCore {
            rng: 7,
            ..NightCore::default()
        };
        let up = [fighter(ME, true, 0, 240, 0.0)];
        let day = solo_world(&up, NightFlags::default());
        // Day 1: positions 0, 120, 240, 360 m apart are kept (each held still long enough); one 10 m from the last is not.
        for (i, x) in [0.0, 120.0, 240.0, 360.0, 370.0].iter().enumerate() {
            for dt in [0, 1_000, 3_500] {
                run(&mut c, i as u64 * 6_000 + dt, &at(&day, [*x, 0.0, 0.0]));
            }
        }
        assert_eq!(c.places.len(), 4);
        run(&mut c, 30_000, &solo_world(&up, night()));
        let dead = [fighter(ME, true, 0, 0, 0.0)];
        let on = NightFlags {
            our_die: true,
            ..night()
        };
        run(&mut c, 31_000, &solo_world(&dead, on));
        let morning = NightFlags {
            night1_done: true,
            ..NightFlags::default()
        };
        // First morning tick: the release, once; the player is still dead: wait.
        let w = at(&solo_world(&dead, morning), [350.0, 0.0, 0.0]);
        let (acts, _) = run(&mut c, 32_000, &w);
        assert_eq!(
            acts.iter()
                .filter(|a| **a == NightAct::ReleaseDeath)
                .count(),
            1
        );
        assert!(acts.contains(&NightAct::DeathHold(false)));
        let (acts, _) = run(&mut c, 33_000, &w);
        assert!(
            acts.is_empty(),
            "release asked once, then waiting: {acts:?}"
        );
        // Respawning (state 2): still waiting.
        let rs = at(
            &solo_world(&[fighter(ME, true, 2, 240, 0.0)], morning),
            [350.0, 0.0, 0.0],
        );
        assert!(run(&mut c, 36_000, &rs).0.is_empty());
        // Back (state 0, HP up) at x = 350: the place is one of the pool at least 150 m away (0, 120, 240 are; 360 is not).
        let back = at(&solo_world(&up, morning), [350.0, 0.0, 0.0]);
        let (acts, _) = run(&mut c, 38_000, &back);
        let Some(NightAct::Teleport(p)) = acts.iter().find(|a| matches!(a, NightAct::Teleport(_)))
        else {
            panic!("no teleport: {acts:?}");
        };
        assert!(
            [0.0, 120.0, 240.0].contains(&p[0]),
            "a far place, got {p:?}"
        );
        assert!(acts.contains(&NightAct::Revive(ReviveMode::Rivals)));
        // Once.
        let (acts, _) = run(&mut c, 39_000, &back);
        assert!(!acts.iter().any(|a| matches!(a, NightAct::Teleport(_))));
    }

    #[test]
    fn a_solo_player_who_never_died_is_not_released_or_moved() {
        let mut c = NightCore::default();
        let up = [fighter(ME, true, 0, 240, 0.0)];
        run(&mut c, 0, &solo_world(&up, NightFlags::default()));
        run(&mut c, 100, &solo_world(&up, night()));
        let morning = NightFlags {
            night1_done: true,
            ..NightFlags::default()
        };
        let (acts, _) = run(&mut c, 1_000, &solo_world(&up, morning));
        assert!(!acts
            .iter()
            .any(|a| matches!(a, NightAct::ReleaseDeath | NightAct::Teleport(_))));
    }

    #[test]
    fn night_two_deaths_are_never_released() {
        let mut c = NightCore::default();
        let up = [fighter(ME, true, 0, 240, 0.0)];
        let n2 = NightFlags {
            night: true,
            day2: true,
            ..NightFlags::default()
        };
        run(&mut c, 0, &solo_world(&up, n2));
        let dead = [fighter(ME, true, 0, 0, 0.0)];
        for t in [1_000, 20_000, 60_000] {
            let (acts, _) = run(&mut c, t, &solo_world(&dead, n2));
            assert!(!acts.contains(&NightAct::ReleaseDeath));
            assert!(!acts.contains(&NightAct::DeathHold(false)));
        }
    }

    #[test]
    fn a_comeback_that_never_happens_is_given_up_after_the_wait() {
        let mut c = NightCore::default();
        let up = [fighter(ME, true, 0, 240, 0.0)];
        run(&mut c, 0, &solo_world(&up, night()));
        let dead = [fighter(ME, true, 0, 0, 0.0)];
        let morning = NightFlags {
            night1_done: true,
            ..NightFlags::default()
        };
        run(&mut c, 1_000, &solo_world(&dead, morning));
        run(&mut c, 1_000 + DEAD_WAIT_MS, &solo_world(&dead, morning));
        assert!(c.morning.is_none(), "the morning work ended");
    }

    #[test]
    fn a_morning_move_waits_for_the_day_2_clock() {
        let mut c = NightCore {
            rng: 3,
            ..NightCore::default()
        };
        let up = [fighter(ME, true, 0, 240, 0.0)];
        let day = solo_world(&up, NightFlags::default());
        for (i, x) in [0.0, 200.0].iter().enumerate() {
            for dt in [0, 1_000, 3_500] {
                run(&mut c, i as u64 * 6_000 + dt, &at(&day, [*x, 0.0, 0.0]));
            }
        }
        run(&mut c, 20_000, &solo_world(&up, night()));
        let dead = [fighter(ME, true, 0, 0, 0.0)];
        run(&mut c, 21_000, &solo_world(&dead, night()));
        let morning = NightFlags {
            night1_done: true,
            ..NightFlags::default()
        };
        let with_clock = |w: World, h: u8| World {
            clock: Some(ClockState {
                time: ClockTime { h, m: 59, s: 59 },
                ..w.clock.unwrap()
            }),
            ..w
        };
        run(&mut c, 22_000, &with_clock(solo_world(&dead, morning), 23));
        // Back, but the clock still reads the night: no move yet.
        let (acts, _) = run(&mut c, 26_000, &with_clock(solo_world(&up, morning), 23));
        assert!(
            !acts.iter().any(|a| matches!(a, NightAct::Teleport(_))),
            "{acts:?}"
        );
        // The Day-2 reset: now the move.
        let (acts, _) = run(&mut c, 45_000, &with_clock(solo_world(&up, morning), 7));
        assert!(
            acts.iter().any(|a| matches!(a, NightAct::Teleport(_))),
            "{acts:?}"
        );
    }

    #[test]
    fn hawk_ride_positions_never_enter_the_pool() {
        let mut c = NightCore::default();
        let up = [fighter(ME, true, 0, 240, 0.0)];
        let day = solo_world(&up, NightFlags::default());
        // The start hawk: 17 m/s with the ground byte set, for 20 s: nothing kept.
        for k in 0..400u64 {
            run(&mut c, k * 50, &at(&day, [k as f32 * 0.85, 200.0, 0.0]));
        }
        assert!(c.places.is_empty(), "{:?}", c.places);
        // Landed and walking (4 m/s): kept once settled for 2 s.
        for k in 0..60u64 {
            run(
                &mut c,
                20_000 + k * 50,
                &at(&day, [400.0 + k as f32 * 0.2, 30.0, 0.0]),
            );
        }
        assert_eq!(c.places.len(), 1);
        assert!(c.places[0][1] < 31.0);
    }

    #[test]
    fn the_pool_keeps_places_apart_and_forgets_the_oldest() {
        let mut pool = VecDeque::new();
        assert!(keep_place(&mut pool, [0.0, 0.0, 0.0]));
        assert!(!keep_place(&mut pool, [99.0, 0.0, 0.0]), "too close");
        assert!(keep_place(&mut pool, [100.0, 0.0, 0.0]));
        for i in 2..(PLACE_POOL as u32 + 10) {
            keep_place(&mut pool, [i as f32 * 200.0, 0.0, 0.0]);
        }
        assert_eq!(pool.len(), PLACE_POOL);
        assert!(!pool.contains(&[0.0, 0.0, 0.0]), "the oldest went first");
    }

    #[test]
    fn the_random_place_prefers_far_ones_then_any_then_none() {
        let pool: VecDeque<[f32; 3]> =
            [[0.0, 0.0, 0.0], [200.0, 0.0, 0.0], [400.0, 0.0, 0.0]].into();
        // From x = 10: 0 is near; the far ones are 200 and 400; pick(n) = n - 1 takes the last.
        assert_eq!(
            pick_place(&pool, Some([10.0, 0.0, 0.0]), |n| n - 1),
            Some([400.0, 0.0, 0.0])
        );
        assert_eq!(
            pick_place(&pool, Some([10.0, 0.0, 0.0]), |_| 0),
            Some([200.0, 0.0, 0.0])
        );
        // Everything near: any place.
        let near: VecDeque<[f32; 3]> = [[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]].into();
        assert_eq!(
            pick_place(&near, Some([50.0, 0.0, 0.0]), |_| 1),
            Some([100.0, 0.0, 0.0])
        );
        // Unknown respawn position: any place. Empty pool: none.
        assert_eq!(pick_place(&pool, None, |_| 1), Some([200.0, 0.0, 0.0]));
        assert_eq!(pick_place(&VecDeque::new(), None, |_| 0), None);
        // The generator stays in range and moves.
        let mut s = 1;
        let a = next_below(&mut s, 3);
        let b = next_below(&mut s, 3);
        assert!(a < 3 && b < 3 && s != 1);
    }

    #[test]
    fn our_flag_ids_match_the_event_file() {
        let nights = include_str!("../../../../gamedata/emevd-nights.json");
        for (flag, what) in [
            (FLAG_HEAL, "\"heal\": true"),
            (FLAG_WIN, "\"ending\": \"win\", \"banner\": 4000"),
            (FLAG_LOSE, "\"ending\": \"lose\", \"banner\": 4010"),
            (FLAG_DIE, "\"speffect\": 102130"),
        ] {
            assert!(
                nights
                    .lines()
                    .any(|l| l.contains(what) && l.contains(&format!("\"flag\": {flag} "))),
                "missing: {what} with flag {flag}"
            );
        }
        for id in [1180, 1302] {
            assert!(
                nights.contains(&format!("{{ \"id\": {id}, \"body\": \"end\" }}")),
                "not replaced: {id}"
            );
        }
        let func = include_str!("../../../../gamedata/emevd-nights-func.json");
        assert!(
            func.contains("{ \"id\": 90005910, \"body\": \"end\" }"),
            "the Day 3 warp stays"
        );
    }
}
