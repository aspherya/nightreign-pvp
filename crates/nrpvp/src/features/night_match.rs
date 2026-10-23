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
//! When our event file is not loaded (Nucleus: no me3), a flag of ours stays ON; after [`FALLBACK_MS`] this game writes the
//! game's flag itself (7502 in the host, 7000 / 7001 in each game: `game::flags::set_match_flag`). The heal has no fallback.
//! Not built: spectating another player (the camera, a later card). Log lines start with `night_match:`.

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
                *self = NightCore::default();
                acts.push(NightAct::ClockFloor(None));
                acts.push(NightAct::Revive(ReviveMode::Rivals));
            }
            return (acts, lines);
        }
        // A tick without the flags or without LOCAL in the player list decides nothing and forgets nothing.
        let (Some(nf), Some(_)) = (world.night, local) else {
            return (acts, lines);
        };
        self.in_expedition = true;
        self.record_spot(now_ms, world, &nf, local);

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
            });
            self.night = None;
            acts.push(NightAct::ClockFloor(None));
            acts.push(NightAct::Revive(ReviveMode::Anyone));
            acts.push(NightAct::ExemptGetup(GETUP_EXEMPT_MS));
        }
        if let Some(m) = self.morning.as_mut() {
            if Self::morning_step(m, now_ms, local, self.spot, &mut acts, &mut lines) {
                self.morning = None;
                acts.push(NightAct::Revive(ReviveMode::Rivals));
            }
        }
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
        local: Option<Fighter>,
        spot: Option<[f32; 3]>,
        acts: &mut Vec<NightAct>,
        lines: &mut Vec<String>,
    ) -> bool {
        let age = now_ms.saturating_sub(m.since_ms);
        let Some(l) = local else {
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
        if !m.out || m.moved {
            return true;
        }
        if l.state != 0 || l.hp == 0 {
            // Still getting up: wait (bounded).
            return age >= GETUP_MS + 5_000;
        }
        m.moved = true;
        match spot {
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
            "{phase}; {} player(s), {teams} team(s), {standing} standing; spot {}",
            fighters.len(),
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
        "night_match: {}",
        if s.is_empty() {
            "not in an expedition"
        } else {
            &s
        }
    )
}

pub(crate) struct NightMatch {
    core: NightCore,
    started: Instant,
}

impl Default for NightMatch {
    fn default() -> Self {
        NightMatch {
            core: NightCore::default(),
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

    #[test]
    fn our_flag_ids_match_the_event_file() {
        let nights = include_str!("../../../../gamedata/emevd-nights.json");
        for (flag, what) in [
            (FLAG_HEAL, "\"heal\": true"),
            (FLAG_WIN, "\"ending\": \"win\", \"banner\": 4000"),
            (FLAG_LOSE, "\"ending\": \"lose\", \"banner\": 4010"),
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
