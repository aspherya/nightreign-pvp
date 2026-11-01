//! The match rules (Leonardo's notes after the first real match, 2026-10-24; DECISIONS "After the first real match", TASKS RW-*).
//! Runs in every game; each game decides from the same ChrSet copies and acts only on its own player. Replaces the elimination
//! rounds of M5-5 / M5-6 / M5-9 (git history).
//!
//! - **Defeat for good (RW-1).** When LOCAL dies (HP 0 and not in the downed state) the game decides who did it. A rival's hit
//!   in the last [`RIVAL_WINDOW_MS`] (the hit log of `game::combat_state`: the last-attacker handle is valid on the victim's own
//!   game), or the tide (a hit with no attacker, from the circle's first closing on Day 2), is a **defeat**: the comeback's start
//!   step is held ([`NightAct::DeathHold`], `game::deathhold`) so LOCAL stays dead; after `defeat_exit_secs` the defeat screen
//!   plays ([`FLAG_LOSE`], then the Roundtable) unless the match ends first. There is no spectator camera yet (M5-7).
//! - **A mob's or a fall's death is the game's own (RW-2).** Nothing is held; the comeback respawns LOCAL with the items; the runes
//!   wait where it died. Respawning far from the other players (RW-7) moves LOCAL to the farthest place of a pool recorded on Day 1.
//! - **The match ends when one team is left (RW-1).** At any time of any day. A player is *out* when dead for [`OUT_DEAD_MS`]
//!   (a mob's death respawns sooner; a defeated player never does) or gone from the player list for [`GONE_MS`]; LOCAL is out at
//!   once when defeated. The last team standing wins: its games set [`FLAG_WIN`] (banner 4000, then 7000), every other game
//!   [`FLAG_LOSE`] (banner 4010, then 7001). Only when at least two teams were seen in this expedition.
//! - **A free first day (RW-4).** Day 1 and Night 1 run `day1_secs` of real time with the clock slowed so the Night's Tide never
//!   starts (the first circle begins at 12:30): the clock is held below [`DAY1_END`]; there the host sets the game's "night 1
//!   done" flag 7502 (the game's own morning, Day 2 at 07:00, follows) and the speed is put back once Day 2 has begun. Day 2 is
//!   vanilla: tide, then the night inside the final circle.
//!
//! When our event file is not loaded (no me3), a flag of ours stays ON; after [`FALLBACK_MS`] this game writes the game's flag
//! itself (7000 / 7001). Log lines start with `night_match:`.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use super::{Ctx, Feature};
use crate::game::combat_state::{Fighter, HitKind};
use crate::game::views::ClockTime;
use crate::game::world::{NightFlags, World};

/// This game's player won (event 99100803: banner 4000, 10 s, 7000).
pub(crate) const FLAG_WIN: u32 = 10_007_411;
/// This game's player lost (event 99100804: banner 4010, 10 s, 7001).
pub(crate) const FLAG_LOSE: u32 = 10_007_412;
/// Dev: LOCAL gets SpEffect 102130 (die at once) while this is ON (event 99100805; `research/M5-8-death.md` §R1). Set by hand
/// (console `flag`) to test a defeat in offline mode, where the offline revive effect would otherwise bring LOCAL back.
pub(crate) const FLAG_DIE: u32 = 10_007_420;
/// The game's own flags written when our event file is missing.
pub(crate) const NIGHT1_DONE: u32 = 7502;
pub(crate) const GAME_CLEAR: u32 = 7000;
pub(crate) const GAME_OVER: u32 = 7001;

/// A rival's hit this recent when LOCAL dies = killed by a player (a full bleed-out is 15-17 s after the hit that downed LOCAL and every
/// hit while down counts; the margin covers a rival who walked away).
pub(crate) const RIVAL_WINDOW_MS: u64 = 30_000;
/// A hit with no attacker this recent when LOCAL dies, from the tide's first closing, = killed by the tide.
pub(crate) const ENV_WINDOW_MS: u64 = 10_000;
/// The first circle of Day 2 starts closing here (`PlayAreaCreateCommonParam` row 0, MEMORY_MAP §16).
pub(crate) const TIDE_FROM: ClockTime = ClockTime { h: 12, m: 30, s: 0 };
/// Dead this long (HP 0, not downed) = out of the match. A mob's death respawns in about 14 s (defeat-mob run 2026-10-24: HP 0 to standing
/// 14.2 s), so this must stay well above that or another game would call a respawning player out; a defeated player is held for good.
pub(crate) const OUT_DEAD_MS: u64 = 30_000;
/// Missing from the player list this long after being seen = out (left the match).
pub(crate) const GONE_MS: u64 = 6_000;
/// One team must be the only one standing this long before the match is decided (one stray reading never ends it).
pub(crate) const DECIDE_MS: u64 = 1_000;
/// Our flag still ON this long after we set it = our event file is not loaded: write the game's flag ourselves.
pub(crate) const FALLBACK_MS: u64 = 1_500;
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
/// RW-1: HP 0 without the downed state must last this long before it counts as a death (ms).
pub(crate) const DEAD_DEBOUNCE_MS: u64 = 800;
/// RW-1: the match rules switch the Esc menu gate (`game::menugate`) on for a defeated, held player: Esc then asks the game to open the
/// pause menu (the game's own check refuses at HP 0, MEMORY_MAP §19). Unverified in the game until the Nucleus test.
pub(crate) const ESC_GATE_AUTO: bool = true;
/// RW-1: the defeated player's Esc menu is unlocked this long after the defeat (ms), when the death banner is over.
pub(crate) const MENU_AFTER_MS: u64 = 9_000;
/// RW-1: how long a defeated body waits for the hold before it follows anyway (ms).
pub(crate) const SPECTATE_WAIT_MS: u64 = 12_000;
/// RW-1: the spectating body is moved every tick and this far above the watched player's feet (metres). Zero: a body above the floor
/// falls between two moves and the camera bobs up and down at the move rate (user, 2026-10-25).
pub(crate) const SPECTATE_HEIGHT: f32 = 0.0;
/// RW-7: LOCAL stands this long after a mob-death respawn before it is moved.
pub(crate) const RESPAWN_MOVE_DELAY_MS: u64 = 3_000;
/// RW-7: no move when LOCAL already is at least this far from every other living player (metres).
pub(crate) const FAR_ENOUGH: f32 = 300.0;
/// Spots are taken while the day-1 clock is before this (players have landed and spread).
const PLACE_EVERY_MS: u64 = 5_000;
/// A place counts only after LOCAL moved slower than [`PLACE_MAX_SPEED`] this long: the start hawk flies at ~17 m/s with the ground
/// byte set (MEMORY_MAP §8), so ride positions in the air must never enter the pool (S1 solo run 2026-10-23: a y 210 pick).
const PLACE_SETTLE_MS: u64 = 2_000;
/// Metres per second (horizontal) between two ticks; a sprint is about 7.
const PLACE_MAX_SPEED: f32 = 9.0;
/// A sampled position is kept only when it is at least this far from every kept one (metres).
pub(crate) const PLACE_MIN_APART: f32 = 100.0;
/// The pool keeps this many positions (the oldest goes first).
pub(crate) const PLACE_POOL: usize = 64;
/// The host's session player id has this in its upper half (`0x6001FFFFFFFF`; FS-0, run 21, M5-4 runs 1-2).
pub(crate) const HOST_ID_HI: u64 = 0x6001;

/// What the match rules ask the main loop to do (besides our flags, which go through [`Ctx::flags`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum NightAct {
    /// Move LOCAL to this world position.
    Teleport([f32; 3]),
    /// RW-1: move the defeated, invisible body next to the watched player (10 times a second, so the move is not logged).
    Follow([f32; 3]),
    /// Write one of the game's flags (`game::flags::MATCH_WRITABLE`).
    GameFlag(u32, bool),
    /// Arm or disarm the hold of a dead LOCAL's comeback (`game::deathhold::set_armed`).
    DeathHold(bool),
    /// RW-4: the play-area clock's speed (game seconds per real second).
    ClockSpeed(f32),
    /// RW-1: let a defeated, held player's Esc menu open (`game::menugate`).
    MenuGate(bool),
    /// RW-3: LOCAL was defeated by a rival: its gear drops where it stands (`game::defeatdrop`).
    DropItems,
}

/// Who defeated LOCAL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cause {
    Rival,
    Tide,
}

impl Cause {
    fn text(self) -> &'static str {
        match self {
            Cause::Rival => "a rival",
            Cause::Tide => "the tide",
        }
    }
}

/// How the match ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Decision {
    /// The last team standing (`None`: everyone was out at once).
    pub winner: Option<u64>,
    pub at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Defeat {
    cause: Cause,
    at_ms: u64,
    /// The defeat screen was asked for.
    exit_sent: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Seen {
    team: u64,
    dead_since: Option<u64>,
    gone_since: Option<u64>,
}

impl Seen {
    fn out(&self, now_ms: u64) -> bool {
        self.dead_since
            .is_some_and(|t| now_ms.saturating_sub(t) >= OUT_DEAD_MS)
            || self
                .gone_since
                .is_some_and(|t| now_ms.saturating_sub(t) >= GONE_MS)
    }
}

/// RW-4's state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Day1 {
    /// The speed to put back (what the clock ran at before we slowed it).
    orig_speed: Option<f32>,
    slowed: bool,
    end_sent: bool,
}

/// The pure decision core (unit tested).
#[derive(Debug, Default)]
pub(crate) struct NightCore {
    in_expedition: bool,
    seen: BTreeMap<u64, Seen>,
    max_teams: usize,
    lone_since: Option<u64>,
    decided: Option<Decision>,
    match_over: bool,
    /// LOCAL has been dead since (HP 0, not downed), and whether this death was already judged.
    dead_since: Option<u64>,
    judged: bool,
    defeat: Option<Defeat>,
    hold_on: bool,
    /// RW-1: the Esc menu gate is open (what [`NightAct::MenuGate`] last said).
    menu_on: bool,
    /// RW-7: a mob's death was judged; the respawn is moved once LOCAL has stood [`RESPAWN_MOVE_DELAY_MS`].
    respawn_pending: bool,
    up_since: Option<u64>,
    mob_deaths: u32,
    /// Dev console `night_count 0`: the match never ends by count.
    count_off: bool,
    /// Since when each of our flags (win, lose) has been ON (the fallback).
    stuck: [Option<u64>; 2],
    day1: Day1,
    /// `day1_secs` from the config (0 = off) and `defeat_exit_secs` in ms (0 = never).
    day1_secs: u32,
    exit_ms: u64,
    /// `spectate` from the config.
    spectate: bool,
    /// RW-1: the player being watched (session id) and when the body was last moved.
    spec_target: Option<u64>,
    /// Day-1 ground positions, at least [`PLACE_MIN_APART`] apart (ring of [`PLACE_POOL`]).
    places: VecDeque<[f32; 3]>,
    last_place_ms: Option<u64>,
    /// The previous tick's ground position (the speed check) and since when LOCAL has been slow.
    place_prev: Option<(u64, [f32; 3])>,
    slow_since: Option<u64>,
    status: String,
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// Add `p` to the pool when it is at least [`PLACE_MIN_APART`] from every kept position; true when kept.
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

/// RW-7: the pool place farthest from every position in `others` (the smallest distance to any of them is the largest);
/// `None` for an empty pool. With no `others` any place is as good: the first.
pub(crate) fn farthest_place(
    pool: &VecDeque<[f32; 3]>,
    others: &[[f32; 3]],
) -> Option<([f32; 3], f32)> {
    pool.iter()
        .map(|p| {
            let d = others
                .iter()
                .map(|o| dist(*p, *o))
                .fold(f32::INFINITY, f32::min);
            (*p, d)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

fn before(t: ClockTime, limit: ClockTime) -> bool {
    (t.h, t.m, t.s) < (limit.h, limit.m, limit.s)
}

impl NightCore {
    fn new(day1_secs: u32, exit_secs: u32, spectate: bool) -> Self {
        NightCore {
            day1_secs,
            spectate,
            exit_ms: u64::from(exit_secs) * 1000,
            ..NightCore::default()
        }
    }

    /// One tick. Our flags go to `flags`, everything else is returned.
    pub(crate) fn step(
        &mut self,
        now_ms: u64,
        world: &World,
        flags: &mut Vec<(u32, bool)>,
    ) -> (Vec<NightAct>, Vec<String>) {
        let mut acts = Vec::new();
        let mut lines = Vec::new();
        let fighters: Vec<(usize, Fighter)> = world
            .fighters
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.map(|f| (i, f)))
            .collect();
        let local = fighters.iter().map(|(_, f)| *f).find(|f| f.local);
        if !world.in_expedition() {
            if self.in_expedition {
                // Back in the Roundtable (or a load): forget the expedition, put the rules back.
                let (day1_secs, exit_ms, spectate) = (self.day1_secs, self.exit_ms, self.spectate);
                let count_off = self.count_off;
                *self = NightCore {
                    count_off,
                    day1_secs,
                    exit_ms,
                    spectate,
                    ..NightCore::default()
                };
                acts.push(NightAct::DeathHold(false));
                acts.push(NightAct::MenuGate(false));
                if world.night.is_some_and(|n| n.our_die) {
                    flags.push((FLAG_DIE, false));
                }
                lines.push("night_match: out of the expedition; rules reset".into());
            }
            return (acts, lines);
        }
        // A tick without the flags or without LOCAL in the player list decides nothing and forgets nothing.
        let (Some(nf), Some(local)) = (world.night, local) else {
            return (acts, lines);
        };
        self.in_expedition = true;
        self.record_place(now_ms, world, &nf, local);
        let host = local.id >> 32 == HOST_ID_HI;

        self.local_step(now_ms, world, &nf, local, flags, &mut acts, &mut lines);
        self.count_step(now_ms, &fighters, local, host, flags, &mut lines);
        self.spectate_step(
            now_ms, world, &fighters, local, flags, &mut acts, &mut lines,
        );
        self.respawn_step(now_ms, world, &nf, &fighters, local, &mut acts, &mut lines);
        self.day1_step(world, &nf, host, &mut acts, &mut lines);
        self.fallback(now_ms, &nf, flags, &mut acts, &mut lines);
        self.status = self.describe(now_ms, &fighters, &nf);
        (acts, lines)
    }

    /// RW-1 / RW-2: judge LOCAL's death, hold a defeated player, send the defeat screen.
    #[allow(clippy::too_many_arguments)]
    fn local_step(
        &mut self,
        now_ms: u64,
        world: &World,
        nf: &NightFlags,
        local: Fighter,
        flags: &mut Vec<(u32, bool)>,
        acts: &mut Vec<NightAct>,
        lines: &mut Vec<String>,
    ) {
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
            match Self::judge(world, nf) {
                Some(cause) => {
                    self.defeat = Some(Defeat {
                        cause,
                        at_ms: now_ms,
                        exit_sent: false,
                    });
                    if cause == Cause::Rival {
                        acts.push(NightAct::DropItems);
                    }
                    lines.push(format!(
                        "night_match: LOCAL defeated by {}: stays dead until the match ends{}",
                        cause.text(),
                        if self.exit_ms == 0 {
                            String::new()
                        } else {
                            format!(", the defeat screen follows in {} s", self.exit_ms / 1000)
                        }
                    ));
                }
                None => {
                    self.mob_deaths += 1;
                    self.respawn_pending = true;
                    lines.push(format!(
                        "night_match: LOCAL died to an enemy or a fall: the game's own respawn, items kept (last hit {:?}, last rival hit {:?} ms ago)",
                        world.local_hits.last, world.local_hits.rival_age_ms
                    ));
                }
            }
        }
        // A defeated LOCAL that stands again (a dev release, an area load) is not defeated any more.
        if self.defeat.is_some() && !dead && local.state == 0 && local.hp > 0 {
            lines.push("night_match: LOCAL is up again; the defeat is cleared".into());
            self.defeat = None;
        }
        let hold = self.defeat.is_some();
        if hold != self.hold_on {
            self.hold_on = hold;
            acts.push(NightAct::DeathHold(hold));
        }
        // The Esc menu: from MENU_AFTER_MS after the defeat, once the hold hides the body (the "YOU DIED" banner has been and gone).
        let menu = ESC_GATE_AUTO
            && self.defeat.is_some_and(|d| {
                world.death_held && now_ms.saturating_sub(d.at_ms) >= MENU_AFTER_MS
            });
        if menu != self.menu_on {
            self.menu_on = menu;
            acts.push(NightAct::MenuGate(menu));
            lines.push(format!(
                "night_match: the Esc menu is {} for the defeated player",
                if menu { "unlocked" } else { "left to the game" }
            ));
        }
        if let Some(d) = self.defeat.as_mut() {
            if !d.exit_sent
                && !self.match_over
                && self.exit_ms != 0
                && now_ms.saturating_sub(d.at_ms) >= self.exit_ms
            {
                d.exit_sent = true;
                flags.push((FLAG_LOSE, true));
                lines.push(
                    "night_match: the defeat screen is asked for (back to the Roundtable)".into(),
                );
            }
        }
    }

    /// RW-1: a defeated player watches the match: the (invisible, held) dead body is moved next to a player who is still in, so the
    /// follow camera shows them; F10 leaves (the defeat screen). Needs `spectate` and a defeat; nothing happens once the match is over.
    #[allow(clippy::too_many_arguments)]
    fn spectate_step(
        &mut self,
        now_ms: u64,
        world: &World,
        fighters: &[(usize, Fighter)],
        local: Fighter,
        flags: &mut Vec<(u32, bool)>,
        acts: &mut Vec<NightAct>,
        lines: &mut Vec<String>,
    ) {
        let Some(d) = self.defeat.as_mut() else {
            self.spec_target = None;
            return;
        };
        if world.leave_pressed && !d.exit_sent && !self.match_over {
            d.exit_sent = true;
            flags.push((FLAG_LOSE, true));
            lines.push(
                "night_match: the leave key was pressed: the defeat screen is asked for".into(),
            );
            return;
        }
        // Only a dead body is moved: a downed or standing player (a stale defeat) is never dragged about.
        let dead_now = local.hp == 0 && local.state != 1;
        if !self.spectate || self.match_over || d.exit_sent || !dead_now {
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
                && !self.seen.get(&f.id).is_some_and(|s| s.out(now_ms))
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
                lines.push(format!(
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
                lines.push("night_match: nobody left to watch".into());
            }
            return;
        };
        let Some(p) = world.positions.get(*i).copied().flatten() else {
            return;
        };
        if self.spec_target != Some(f.id) {
            self.spec_target = Some(f.id);
            lines.push(format!("night_match: spectating player {:#X}", f.id));
        }
        // Next to the watched player (the held body is invisible to the others); the follow camera goes with it.
        acts.push(NightAct::Follow([p[0], p[1] + SPECTATE_HEIGHT, p[2]]));
    }

    /// Who killed LOCAL, or `None` for an enemy, a fall or anything else the game handles itself.
    fn judge(world: &World, nf: &NightFlags) -> Option<Cause> {
        let hits = world.local_hits;
        if hits.rival_age_ms.is_some_and(|a| a <= RIVAL_WINDOW_MS) {
            return Some(Cause::Rival);
        }
        let tide = nf.day2 && world.clock.is_some_and(|c| !before(c.time, TIDE_FROM));
        match hits.last {
            Some((HitKind::Env, age)) if tide && age <= ENV_WINDOW_MS => Some(Cause::Tide),
            _ => None,
        }
    }

    /// RW-1: who is out, and the match decision.
    fn count_step(
        &mut self,
        now_ms: u64,
        fighters: &[(usize, Fighter)],
        local: Fighter,
        host: bool,
        flags: &mut Vec<(u32, bool)>,
        lines: &mut Vec<String>,
    ) {
        for (_, f) in fighters {
            let s = self.seen.entry(f.id).or_default();
            s.team = f.team;
            s.gone_since = None;
            if f.hp == 0 && f.state != 1 {
                s.dead_since.get_or_insert(now_ms);
            } else {
                s.dead_since = None;
            }
        }
        for (id, s) in self.seen.iter_mut() {
            if !fighters.iter().any(|(_, f)| f.id == *id) {
                s.gone_since.get_or_insert(now_ms);
            }
        }
        let mut teams: Vec<u64> = self.seen.values().map(|s| s.team).collect();
        teams.sort_unstable();
        teams.dedup();
        self.max_teams = self.max_teams.max(teams.len());
        if self.match_over || self.count_off || self.max_teams < 2 {
            return;
        }
        let local_out = self.defeat.is_some();
        let standing: Vec<u64> = teams
            .iter()
            .copied()
            .filter(|t| {
                self.seen.iter().any(|(id, s)| {
                    s.team == *t && !s.out(now_ms) && !(local_out && *id == local.id)
                })
            })
            .collect();
        if standing.len() > 1 {
            self.lone_since = None;
            return;
        }
        let since = *self.lone_since.get_or_insert(now_ms);
        if now_ms.saturating_sub(since) < DECIDE_MS {
            return;
        }
        let winner = standing.first().copied();
        self.decided = Some(Decision {
            winner,
            at_ms: now_ms,
        });
        self.match_over = true;
        let won = winner.is_some() && winner == Some(local.team);
        lines.push(format!(
            "night_match: match decided: {} (this game's player {}{})",
            winner.map_or("nobody left standing".into(), |w| format!(
                "team {w:#X} is the last one standing"
            )),
            if won { "won" } else { "lost" },
            if host { ", host" } else { "" }
        ));
        let already_lost = self.defeat.is_some_and(|d| d.exit_sent);
        if won {
            flags.push((FLAG_WIN, true));
        } else if !already_lost {
            flags.push((FLAG_LOSE, true));
        }
    }

    /// RW-7: after a mob's death, LOCAL is moved to the pool place farthest from the other living players.
    #[allow(clippy::too_many_arguments)]
    fn respawn_step(
        &mut self,
        now_ms: u64,
        world: &World,
        nf: &NightFlags,
        fighters: &[(usize, Fighter)],
        local: Fighter,
        acts: &mut Vec<NightAct>,
        lines: &mut Vec<String>,
    ) {
        if !self.respawn_pending {
            return;
        }
        if !(local.state == 0 && local.hp > 0) {
            self.up_since = None;
            return;
        }
        let since = *self.up_since.get_or_insert(now_ms);
        if now_ms.saturating_sub(since) < RESPAWN_MOVE_DELAY_MS {
            return;
        }
        self.respawn_pending = false;
        self.up_since = None;
        // After the first circle of Day 2 starts closing, a Day-1 place may lie outside it: the game's own respawn point
        // (inside the circle) stays.
        let circle_open = nf.day2
            && world
                .clock
                .is_none_or(|c| !before(c.time, TIDE_FROM) || c.time.h >= 20);
        if circle_open {
            lines.push("night_match: respawn left to the game (the circle is closing)".into());
            return;
        }
        let others: Vec<[f32; 3]> = fighters
            .iter()
            .filter(|(_, f)| !f.local && !self.seen.get(&f.id).is_some_and(|s| s.out(now_ms)))
            .filter_map(|(i, _)| world.positions.get(*i).copied().flatten())
            .collect();
        let here = world.local_pos.map(|p| p.0);
        let here_far = here.map(|h| {
            others
                .iter()
                .map(|o| dist(h, *o))
                .fold(f32::INFINITY, f32::min)
        });
        if here_far.is_some_and(|d| d >= FAR_ENOUGH) {
            lines.push(format!(
                "night_match: respawned {:.0} m from the nearest player; no move",
                here_far.unwrap_or(0.0)
            ));
            return;
        }
        match farthest_place(&self.places, &others) {
            Some((p, d)) if here_far.is_none_or(|h| d > h) => {
                acts.push(NightAct::Teleport(p));
                lines.push(format!(
                    "night_match: respawn moved to ({:.1}, {:.1}, {:.1}), {d:.0} m from the nearest player (was {})",
                    p[0],
                    p[1],
                    p[2],
                    here_far.map_or("?".into(), |h| format!("{h:.0} m"))
                ));
            }
            _ => lines.push(
                "night_match: respawn stays where the game put it (no better place recorded)"
                    .into(),
            ),
        }
    }

    /// RW-4: slow the clock on Day 1, end Day 1 at [`DAY1_END`], put the speed back on Day 2.
    fn day1_step(
        &mut self,
        world: &World,
        nf: &NightFlags,
        host: bool,
        acts: &mut Vec<NightAct>,
        lines: &mut Vec<String>,
    ) {
        let Some(c) = world.clock else { return };
        if self.day1_secs == 0 {
            return;
        }
        if nf.day2 {
            // The game raises the Day-2 flag at once but resets the clock to the morning ~20 s later: the slow speed stays until then
            // (a fast clock would run on towards the first circle at 12:30), and goes back at the latest at SLOW_UNTIL.
            let morning_back = before(c.time, DAY1_END_HOUR_GUARD) || !before(c.time, SLOW_UNTIL);
            if self.day1.slowed && morning_back {
                self.day1.slowed = false;
                let back = self.day1.orig_speed.unwrap_or(VANILLA_SPEED);
                acts.push(NightAct::ClockSpeed(back));
                lines.push(format!(
                    "night_match: Day 2 has begun; clock speed back to {back:.1}"
                ));
            }
            return;
        }
        if nf.night1_done || self.match_over {
            return;
        }
        let target = DAY1_SPAN_SECS / self.day1_secs as f32;
        if self.day1.orig_speed.is_none() && c.speed > target * 1.5 {
            self.day1.orig_speed = Some(c.speed);
        }
        if (c.speed - target).abs() > 0.05 {
            acts.push(NightAct::ClockSpeed(target));
            if !self.day1.slowed {
                lines.push(format!(
                    "night_match: free Day 1: clock speed {:.1} -> {target:.1} ({} s until {DAY1_END})",
                    c.speed, self.day1_secs
                ));
            }
        }
        self.day1.slowed = true;
        if host && !self.day1.end_sent && !before(c.time, DAY1_END) {
            self.day1.end_sent = true;
            acts.push(NightAct::GameFlag(NIGHT1_DONE, true));
            lines.push(format!(
                "night_match: Day 1 is over at {}: night-1-done flag {NIGHT1_DONE} set, Day 2 follows",
                c.time
            ));
        }
    }

    /// Day-1 ground positions for RW-7's respawn.
    fn record_place(&mut self, now_ms: u64, world: &World, nf: &NightFlags, local: Fighter) {
        let Some((pos, true)) = world.local_pos else {
            return;
        };
        if nf.day2 || nf.night || nf.night1_done || local.state != 0 || local.hp == 0 {
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

    /// Our flag still ON after [`FALLBACK_MS`] = our event file is not loaded: write the game's flag instead.
    fn fallback(
        &mut self,
        now_ms: u64,
        nf: &NightFlags,
        flags: &mut Vec<(u32, bool)>,
        acts: &mut Vec<NightAct>,
        lines: &mut Vec<String>,
    ) {
        let cases = [
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

    fn describe(&mut self, now_ms: u64, fighters: &[(usize, Fighter)], nf: &NightFlags) -> String {
        let out = self.seen.values().filter(|s| s.out(now_ms)).count();
        let phase = if self.match_over {
            "match over".to_string()
        } else if nf.night {
            format!("night {}", if nf.day2 { 2 } else { 1 })
        } else if self.day1.slowed && !nf.day2 {
            "free day 1".to_string()
        } else {
            format!("day {}", if nf.day2 { 2 } else { 1 })
        };
        let who = match self.decided {
            Some(d) => format!(
                ", decided (winner {})",
                d.winner.map_or("none".into(), |w| format!("{w:#X}"))
            ),
            None => String::new(),
        };
        format!(
            "{phase}{who}; {} player(s), {} team(s) seen, {out} out; LOCAL {}; mob deaths {}; places {}",
            fighters.len(),
            self.max_teams,
            match self.defeat {
                Some(d) => format!(
                    "defeated by {}{}",
                    d.cause.text(),
                    self.spec_target
                        .map_or(String::new(), |t| format!(", watching {t:#X}"))
                ),
                None => "in".into(),
            },
            self.mob_deaths,
            self.places.len()
        )
    }
}

static STATUS: Mutex<String> = Mutex::new(String::new());
/// Dev console `night_count 0|1` (on by default; resets at launch).
static COUNT: AtomicBool = AtomicBool::new(true);
/// Dev console `spectate 0|1`: overrides the config value live (`None` = the config's).
/// The config's `spectate`, kept for the console.
static SPECTATE_CFG: AtomicBool = AtomicBool::new(true);
static SPECTATE_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);

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

fn spectate_now(configured: bool) -> bool {
    match SPECTATE_OVERRIDE.load(Ordering::Relaxed) {
        0 => false,
        1 => true,
        _ => configured,
    }
}

/// Console `night_count [0|1]`: switch the end-by-count (`Some`) and report it.
pub(crate) fn count_cmd(on: Option<bool>) -> String {
    if let Some(on) = on {
        COUNT.store(on, Ordering::Relaxed);
        tracing::info!("night_match: count {}", if on { "ON" } else { "OFF" });
    }
    if COUNT.load(Ordering::Relaxed) {
        "night_count 1: the match ends when one team is left standing".into()
    } else {
        "night_count 0: the match never ends by count".into()
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
    spectate_cfg: bool,
    started: Instant,
}

impl NightMatch {
    pub(crate) fn new(cfg: &crate::config::Config) -> Self {
        SPECTATE_CFG.store(cfg.spectate, Ordering::Relaxed);
        NightMatch {
            core: NightCore::new(cfg.day1_secs, cfg.defeat_exit_secs, cfg.spectate),
            spectate_cfg: cfg.spectate,
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
        self.core.spectate = spectate_now(self.spectate_cfg);
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
    use crate::game::combat_state::LocalHits;
    use crate::game::views::{ClockState, Place};
    use crate::game::world::MAX_FIGHTERS;

    const ME: u64 = 0x6001_FFFF_FFFF;
    const P2: u64 = 0x6002_FFFF_FFFF;
    const P3: u64 = 0x6003_FFFF_FFFF;

    fn fighter(id: u64, local: bool, state: u8, hp: u32) -> Fighter {
        Fighter {
            addr: 0x10_0000 + (id >> 32) as usize,
            id,
            team: id,
            local,
            state,
            hp,
            clock: 0.0,
        }
    }

    fn up(id: u64, local: bool) -> Fighter {
        fighter(id, local, 0, 240)
    }

    fn dead(id: u64, local: bool) -> Fighter {
        fighter(id, local, 0, 0)
    }

    fn clock_at(h: u8, m: u8, speed: f32) -> ClockState {
        ClockState {
            time: ClockTime { h, m, s: 0 },
            speed,
            running: true,
            mailbox_free: true,
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
            clock: Some(clock_at(9, 0, DAY1_SPAN_SECS / 1020.0)),
            ..World::default()
        }
    }

    fn core() -> NightCore {
        NightCore::new(1020, 20, false)
    }

    fn run(
        c: &mut NightCore,
        ms: u64,
        w: &World,
    ) -> (Vec<NightAct>, Vec<(u32, bool)>, Vec<String>) {
        let mut flags = Vec::new();
        let (acts, lines) = c.step(ms, w, &mut flags);
        (acts, flags, lines)
    }

    /// The dead world twice, [`DEAD_DEBOUNCE_MS`] apart (HP 0 must last that long to count as a death); both results merged.
    fn run_dead(
        c: &mut NightCore,
        ms: u64,
        w: &World,
    ) -> (Vec<NightAct>, Vec<(u32, bool)>, Vec<String>) {
        let (mut a, mut f, mut l) = run(c, ms, w);
        let (a2, f2, l2) = run(c, ms + DEAD_DEBOUNCE_MS, w);
        a.extend(a2);
        f.extend(f2);
        l.extend(l2);
        (a, f, l)
    }

    fn rival_hit(w: &mut World, age: u64) {
        w.local_hits = LocalHits {
            last: Some((HitKind::Rival, age)),
            rival_age_ms: Some(age),
        };
    }

    #[test]
    fn a_rival_kill_is_a_defeat_that_holds_the_body_and_sends_the_defeat_screen_after_the_delay() {
        let mut c = core();
        let nf = NightFlags::default();
        let mut w = world(&[up(ME, true), up(P2, false), up(P3, false)], nf);
        run(&mut c, 0, &w);
        w = world(&[dead(ME, true), up(P2, false), up(P3, false)], nf);
        rival_hit(&mut w, 2_000);
        let (acts, flags, lines) = run_dead(&mut c, 100, &w);
        assert!(acts.contains(&NightAct::DeathHold(true)));
        assert_eq!(
            acts.iter().filter(|a| **a == NightAct::DropItems).count(),
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
    fn a_defeated_player_watches_the_nearest_living_player_and_f10_leaves() {
        let nf = NightFlags::default();
        let mut c = NightCore::new(1020, 0, true);
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
            acts2.contains(&NightAct::Follow([50.0, 12.0 + SPECTATE_HEIGHT, 5.0])),
            "the nearer player P3 is watched: {acts2:?}"
        );
        // It stays on the same player while that one is in, even when the other comes closer.
        w.positions[1] = Some([5.0, 0.0, 0.0]);
        let (acts3, _, _) = run(&mut c, 1_200, &w);
        assert!(acts3.contains(&NightAct::Follow([50.0, 12.0 + SPECTATE_HEIGHT, 5.0])));
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
    fn the_body_only_follows_once_the_hold_hides_it_or_after_the_wait() {
        let nf = NightFlags::default();
        let mut c = NightCore::new(1020, 0, true);
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
            !acts.iter().any(|a| matches!(a, NightAct::Follow(_))),
            "not yet: the death animation still shows"
        );
        w.death_held = true;
        let (acts, _, _) = run(&mut c, 3_100, &w);
        assert!(acts.iter().any(|a| matches!(a, NightAct::Follow(_))));
        // No hold at all: it follows after the wait.
        let mut c2 = NightCore::new(1020, 0, true);
        run(
            &mut c2,
            0,
            &world(&[up(ME, true), up(P2, false), up(P3, false)], nf),
        );
        w.death_held = false;
        run_dead(&mut c2, 100, &w);
        let (acts, _, _) = run(&mut c2, 100 + SPECTATE_WAIT_MS + 1_000, &w);
        assert!(acts.iter().any(|a| matches!(a, NightAct::Follow(_))));
    }

    #[test]
    fn the_spectator_switches_to_the_next_and_previous_player_and_wraps() {
        const P4: u64 = 0x6004_FFFF_FFFF;
        let nf = NightFlags::default();
        let mut c = NightCore::new(1020, 0, true);
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
            acts.contains(&NightAct::Follow([10.0, SPECTATE_HEIGHT, 0.0])),
            "the nearest first: {acts:?}"
        );
        let mut at = |next: bool, prev: bool, t: u64| {
            w.spec_next = next;
            w.spec_prev = prev;
            let (acts, _, _) = run(&mut c, t, &w);
            acts.iter()
                .find_map(|a| match a {
                    NightAct::Follow(p) => Some(p[0]),
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
    #[allow(clippy::assertions_on_constants)]
    fn the_defeated_players_esc_menu_gate_is_on() {
        assert!(ESC_GATE_AUTO);
    }

    #[test]
    fn the_defeated_players_esc_menu_opens_after_the_banner_once_the_body_is_held() {
        let nf = NightFlags::default();
        let mut c = NightCore::new(1020, 0, true);
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
            !acts.contains(&NightAct::MenuGate(true)),
            "not while the body is not held"
        );
        w.death_held = true;
        let (acts, _, lines) = run(&mut c, 20_100, &w);
        assert!(acts.contains(&NightAct::MenuGate(true)), "{lines:?}");
        let (acts, _, _) = run(&mut c, 20_200, &w);
        assert!(!acts.contains(&NightAct::MenuGate(true)), "said once");
    }

    #[test]
    fn nothing_is_watched_without_spectate_or_without_a_defeat() {
        let nf = NightFlags::default();
        let mut c = NightCore::new(1020, 0, false);
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
        assert!(!acts.iter().any(|a| matches!(a, NightAct::Follow(_))));
        let mut c2 = NightCore::new(1020, 0, true);
        let mut alive = world(&[up(ME, true), up(P2, false), up(P3, false)], nf);
        alive.positions[1] = Some([50.0, 12.0, 5.0]);
        run(&mut c2, 0, &alive);
        let (acts, _, _) = run(&mut c2, 300, &alive);
        assert!(!acts.iter().any(|a| matches!(a, NightAct::Follow(_))));
    }

    #[test]
    fn in_a_two_player_match_the_defeat_ends_the_match_at_once_for_both() {
        let nf = NightFlags::default();
        // The loser's game.
        let mut c = core();
        run(&mut c, 0, &world(&[up(ME, true), up(P2, false)], nf));
        let mut w = world(&[dead(ME, true), up(P2, false)], nf);
        rival_hit(&mut w, 100);
        run_dead(&mut c, 100, &w);
        let (_, flags, lines) = run(&mut c, 2_100, &w);
        assert_eq!(flags, vec![(FLAG_LOSE, true)], "{lines:?}");
        let (_, flags, _) = run(&mut c, 30_000, &w);
        assert!(flags.is_empty());
        // The winner's game sees the loser dead and out only after OUT_DEAD_MS.
        let mut c = core();
        run(&mut c, 0, &world(&[up(P2, true), up(ME, false)], nf));
        let w = world(&[up(P2, true), dead(ME, false)], nf);
        run(&mut c, 100, &w);
        let (_, flags, _) = run(&mut c, 100 + OUT_DEAD_MS, &w);
        assert!(flags.is_empty());
        let (_, flags, _) = run(&mut c, 1_200 + OUT_DEAD_MS, &w);
        assert_eq!(flags, vec![(FLAG_WIN, true)]);
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
        };
        let (acts, flags, lines) = run_dead(&mut c, 100, &w);
        assert!(!acts.contains(&NightAct::DeathHold(true)));
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
        assert!(!acts.contains(&NightAct::DeathHold(true)));
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
        };
        let mut c = core();
        let mut w = world(&[up(ME, true), up(P2, false)], nf);
        w.clock = Some(clock_at(14, 0, 72.9));
        run(&mut c, 0, &w);
        let mut dead_w = world(&[dead(ME, true), up(P2, false)], nf);
        dead_w.clock = Some(clock_at(14, 0, 72.9));
        dead_w.local_hits = env(1_000);
        let (acts, _, lines) = run_dead(&mut c, 100, &dead_w);
        assert!(acts.contains(&NightAct::DeathHold(true)), "{lines:?}");
        assert!(
            !acts.contains(&NightAct::DropItems),
            "the tide drops no items (Leonardo 2026-10-31: rival kills only)"
        );
        assert!(lines.iter().any(|l| l.contains("defeated by the tide")));
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
            assert!(!acts.contains(&NightAct::DeathHold(true)));
        }
    }

    #[test]
    fn the_match_ends_at_any_time_when_one_team_is_left_and_the_winner_gets_the_win_flag() {
        let mut c = core();
        let all = world(
            &[up(ME, true), up(P2, false), up(P3, false)],
            NightFlags::default(),
        );
        run(&mut c, 0, &all);
        // P2 and P3 die for good (dead for OUT_DEAD_MS).
        let w = world(
            &[up(ME, true), dead(P2, false), dead(P3, false)],
            NightFlags::default(),
        );
        run(&mut c, 1_000, &w);
        let (_, flags, _) = run(&mut c, 1_000 + OUT_DEAD_MS - 1, &w);
        assert!(flags.is_empty(), "not out yet");
        run(&mut c, 1_000 + OUT_DEAD_MS, &w);
        let (_, flags, lines) = run(&mut c, 2_000 + OUT_DEAD_MS, &w);
        assert_eq!(flags, vec![(FLAG_WIN, true)], "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("match decided")));
        // Decided once.
        let (_, flags, _) = run(&mut c, 3_000 + OUT_DEAD_MS, &w);
        assert!(flags.is_empty());
    }

    #[test]
    fn a_downed_player_is_still_in_and_one_team_alone_is_not_enough_before_a_rival_was_seen() {
        let mut c = core();
        let solo = world(&[up(ME, true)], NightFlags::default());
        for t in [0, 2_000, 60_000] {
            let (_, flags, _) = run(&mut c, t, &solo);
            assert!(flags.is_empty(), "never a match with one team seen");
        }
        let two = world(
            &[up(ME, true), fighter(P2, false, 1, 0)],
            NightFlags::default(),
        );
        for t in [61_000, 70_000, 90_000] {
            let (_, flags, _) = run(&mut c, t, &two);
            assert!(flags.is_empty(), "a downed player can still be revived");
        }
    }

    #[test]
    fn a_player_who_leaves_the_match_is_out_after_a_few_seconds() {
        let mut c = core();
        run(
            &mut c,
            0,
            &world(&[up(ME, true), up(P2, false)], NightFlags::default()),
        );
        let alone = world(&[up(ME, true)], NightFlags::default());
        let (_, flags, _) = run(&mut c, 1_000, &alone);
        assert!(flags.is_empty());
        let (_, flags, _) = run(&mut c, 1_000 + GONE_MS, &alone);
        assert!(flags.is_empty(), "the lone team must stand DECIDE_MS");
        let (_, flags, _) = run(&mut c, 2_100 + GONE_MS, &alone);
        assert_eq!(flags, vec![(FLAG_WIN, true)]);
    }

    #[test]
    fn the_count_can_be_switched_off() {
        let mut c = core();
        c.count_off = true;
        let w = world(&[up(P2, true), dead(ME, false)], NightFlags::default());
        run(&mut c, 0, &w);
        let (_, flags, _) = run(&mut c, 40_000, &w);
        assert!(flags.is_empty());
        c.count_off = false;
        run(&mut c, 41_000, &w);
        let (_, flags, _) = run(&mut c, 42_100, &w);
        assert_eq!(flags, vec![(FLAG_WIN, true)]);
    }

    #[test]
    fn leaving_the_expedition_resets_everything_and_releases_the_hold() {
        let mut c = core();
        let mut w = world(&[up(ME, true), up(P2, false)], NightFlags::default());
        run(&mut c, 0, &w);
        w = world(&[dead(ME, true), up(P2, false)], NightFlags::default());
        rival_hit(&mut w, 1);
        run(&mut c, 100, &w);
        let mut rt = w;
        rt.place = Some(Place::Roundtable);
        let (acts, _, _) = run(&mut c, 200, &rt);
        assert_eq!(
            acts,
            vec![NightAct::DeathHold(false), NightAct::MenuGate(false)]
        );
        assert!(c.defeat.is_none() && c.seen.is_empty());
    }

    #[test]
    fn day_one_is_slowed_ended_by_the_host_and_restored_on_day_two() {
        let mut c = core();
        let mut w = world(&[up(ME, true)], NightFlags::default());
        w.clock = Some(clock_at(7, 0, 72.9));
        let (acts, _, lines) = run(&mut c, 0, &w);
        let target = DAY1_SPAN_SECS / 1020.0;
        assert_eq!(acts, vec![NightAct::ClockSpeed(target)], "{lines:?}");
        // Slowed already: nothing more until the end.
        let mut w = world(&[up(ME, true)], NightFlags::default());
        w.clock = Some(clock_at(9, 0, target));
        let (acts, _, _) = run(&mut c, 50, &w);
        assert!(acts.is_empty());
        // The host ends Day 1 once, at DAY1_END.
        w.clock = Some(clock_at(11, 50, target));
        let (acts, _, _) = run(&mut c, 100, &w);
        assert_eq!(acts, vec![NightAct::GameFlag(NIGHT1_DONE, true)]);
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
        assert_eq!(acts, vec![NightAct::ClockSpeed(72.9)]);
        // Day 2: the original speed returns.
        let mut c = core();
        c.day1.slowed = true;
        c.day1.orig_speed = Some(72.9);
        let mut d2 = world(
            &[up(ME, true)],
            NightFlags {
                day2: true,
                ..NightFlags::default()
            },
        );
        d2.clock = Some(clock_at(7, 1, target));
        let (acts, _, _) = run(&mut c, 200, &d2);
        assert_eq!(acts, vec![NightAct::ClockSpeed(72.9)]);
    }

    #[test]
    fn a_client_never_ends_day_one_and_day1_secs_zero_leaves_the_clock_alone() {
        let mut c = core();
        let mut w = world(&[up(P2, true)], NightFlags::default());
        w.clock = Some(clock_at(11, 55, DAY1_SPAN_SECS / 1020.0));
        let (acts, _, _) = run(&mut c, 0, &w);
        assert!(acts.is_empty());
        let mut off = NightCore::new(0, 20, false);
        let (acts, _, _) = run(&mut off, 0, &world(&[up(ME, true)], NightFlags::default()));
        assert!(acts.is_empty());
    }

    #[test]
    fn a_mob_death_respawn_moves_to_the_farthest_recorded_place() {
        let mut c = core();
        c.places.push_back([0.0, 0.0, 0.0]);
        c.places.push_back([1000.0, 0.0, 0.0]);
        let w0 = world(&[up(ME, true), up(P2, false)], NightFlags::default());
        run(&mut c, 0, &w0);
        let mut w = world(&[dead(ME, true), up(P2, false)], NightFlags::default());
        w.local_hits = LocalHits {
            last: Some((HitKind::Mob, 10)),
            rival_age_ms: None,
        };
        run_dead(&mut c, 100, &w);
        // Standing again next to the other player (P2 at x=10): the far place is x=1000.
        let mut back = world(&[up(ME, true), up(P2, false)], NightFlags::default());
        back.local_pos = Some(([20.0, 0.0, 0.0], true));
        back.positions[1] = Some([10.0, 0.0, 0.0]);
        run(&mut c, 12_000, &back);
        let (acts, _, lines) = run(&mut c, 12_000 + RESPAWN_MOVE_DELAY_MS, &back);
        assert_eq!(
            acts,
            vec![NightAct::Teleport([1000.0, 0.0, 0.0])],
            "{lines:?}"
        );
    }

    #[test]
    fn a_respawn_already_far_from_everyone_stays() {
        let mut c = core();
        c.places.push_back([1000.0, 0.0, 0.0]);
        let w0 = world(&[up(ME, true), up(P2, false)], NightFlags::default());
        run(&mut c, 0, &w0);
        let mut w = world(&[dead(ME, true), up(P2, false)], NightFlags::default());
        w.local_hits = LocalHits::default();
        run_dead(&mut c, 100, &w);
        let mut back = world(&[up(ME, true), up(P2, false)], NightFlags::default());
        back.local_pos = Some(([0.0, 0.0, 0.0], true));
        back.positions[1] = Some([900.0, 0.0, 0.0]);
        run(&mut c, 12_000, &back);
        let (acts, _, _) = run(&mut c, 15_100, &back);
        assert!(acts.is_empty());
    }

    #[test]
    fn places_are_kept_apart_and_the_pool_is_a_ring() {
        let mut pool = VecDeque::new();
        assert!(keep_place(&mut pool, [0.0, 0.0, 0.0]));
        assert!(!keep_place(&mut pool, [50.0, 0.0, 0.0]));
        assert!(keep_place(&mut pool, [150.0, 0.0, 0.0]));
        for i in 0..100 {
            keep_place(&mut pool, [1000.0 + 200.0 * i as f32, 0.0, 0.0]);
        }
        assert_eq!(pool.len(), PLACE_POOL);
    }

    #[test]
    fn the_farthest_place_maximises_the_distance_to_the_nearest_player() {
        let mut pool = VecDeque::new();
        pool.push_back([0.0, 0.0, 0.0]);
        pool.push_back([500.0, 0.0, 0.0]);
        pool.push_back([1000.0, 0.0, 0.0]);
        let others = [[0.0, 0.0, 10.0], [1000.0, 0.0, 10.0]];
        let (p, d) = farthest_place(&pool, &others).expect("a place");
        assert_eq!(p, [500.0, 0.0, 0.0]);
        assert!((d - 500.0).abs() < 1.0);
        assert!(farthest_place(&VecDeque::new(), &others).is_none());
    }

    #[test]
    fn a_stuck_win_flag_is_written_to_the_game_after_the_fallback_delay() {
        let mut c = core();
        let nf = NightFlags {
            our_win: true,
            ..NightFlags::default()
        };
        let w = world(&[up(ME, true)], nf);
        run(&mut c, 0, &w);
        let (acts, flags, _) = run(&mut c, FALLBACK_MS, &w);
        assert_eq!(flags, vec![(FLAG_WIN, false)]);
        assert!(acts.contains(&NightAct::GameFlag(GAME_CLEAR, true)));
    }

    #[test]
    fn our_flag_ids_match_the_game_data_file() {
        let nights = include_str!("../../../../gamedata/emevd-nights.json");
        for (id, flag) in [
            (99_100_803, FLAG_WIN),
            (99_100_804, FLAG_LOSE),
            (99_100_805, FLAG_DIE),
        ] {
            assert!(
                nights.contains(&format!("\"id\": {id},"))
                    && nights.contains(&format!("\"flag\": {flag} ")),
                "event {id} / flag {flag}"
            );
        }
    }
}
