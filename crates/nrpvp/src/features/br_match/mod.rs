//! The Battle Royale match rules (Leonardo's notes after the first real match, 2026-10-24; DECISIONS "After the first real match",
//! TASKS RW-*). Runs in every game; each game decides from the same ChrSet copies and acts only on its own player. Replaces the
//! elimination rounds of M5-5 / M5-6 / M5-9 (git history).
//!
//! R7-8 (P2-8): the former `night_match.rs` is split by rule. A [`Match`] owns one [`MatchBook`] (the facts the rules share) and an
//! ordered list of [`MatchRule`]s; each tick it calls them in list order, so the order is behaviour:
//!
//! 1. [`defeat::Defeat`] (RW-1 / RW-2): judge LOCAL's death, hold a defeated player, send the defeat screen.
//! 2. [`kill_report::KillReport`] (KF-1): LOCAL went down by a rival or is out: tell the killfeed.
//! 3. [`elimination::Elimination`] (RW-1): who is out, and the match decision.
//! 4. [`spectate::Spectate`] (RW-1): a defeated player watches a living one; F10 leaves.
//! 5. [`respawn_far::RespawnFar`] (RW-7): records Day-1 places and moves a mob-death respawn far from the others.
//! 6. [`free_day1::FreeDay1`] (RW-4): slow the clock on Day 1, end it, put the speed back on Day 2.
//! 7. [`grace_lock::GraceLock`] (GRACE-1): flag 7525 ON while LOCAL is in a PvP fight, so no Site of Grace works.
//! 8. [`fallback::FlagFallback`]: write the game's flag when our event file is not loaded.
//!
//! Then [`status::render`] makes the `night_status` text. Who writes a [`MatchBook`] field is named on the field.
//!
//! - **Defeat for good (RW-1).** When LOCAL dies (HP 0 and not in the downed state) the game decides who did it. A rival's hit
//!   in the last [`defeat::RIVAL_WINDOW_MS`] (the hit log of `game::combat_state`: the last-attacker handle is valid on the victim's own
//!   game), or the tide (a hit with no attacker, from the circle's first closing on Day 2), is a **defeat**: the comeback's start
//!   step is held ([`Act::DeathHold`], `game::deathhold`) so LOCAL stays dead; after `defeat_exit_secs` the defeat screen
//!   plays ([`FLAG_LOSE`], then the Roundtable) unless the match ends first. A defeated player may spectate instead (M5-7: the held body follows a living player; `spectate`).
//! - **A mob's or a fall's death is the game's own (RW-2).** Nothing is held; the comeback respawns LOCAL with the items; the runes
//!   wait where it died. Respawning far from the other players (RW-7) moves LOCAL to the farthest place of a pool recorded on Day 1.
//! - **The match ends when one team is left (RW-1).** At any time of any day. A player is *out* when dead for
//!   [`elimination::OUT_DEAD_MS`] (a mob's death respawns sooner; a defeated player never does) or gone from the player list for
//!   [`elimination::GONE_MS`]; LOCAL is out at once when defeated. The last team standing wins: its games set [`FLAG_WIN`] (banner
//!   4000, then 7000), every other game [`FLAG_LOSE`] (banner 4010, then 7001). Only when at least two teams were seen in this expedition.
//! - **A free first day (RW-4).** Day 1 and Night 1 run `day1_secs` of real time with the clock slowed so the Night's Tide never
//!   starts (the first circle begins at 12:30): the clock is held below [`free_day1::DAY1_END`]; there the host sets the game's "night 1
//!   done" flag 7502 (the game's own morning, Day 2 at 07:00, follows) and the speed is put back once Day 2 has begun. Day 2 is
//!   vanilla: tide, then the night inside the final circle.
//!
//! When our event file is not loaded (no me3), a flag of ours stays ON; after [`fallback::FALLBACK_MS`] this game writes the game's flag
//! itself (7000 / 7001). Log lines start with `night_match:`.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::Instant;

use super::{Act, Ctx, Feature};
use crate::game::combat_state::Fighter;
use crate::game::views::ClockTime;
use crate::game::world::{NightFlags, World};

pub(crate) mod defeat;
pub(crate) mod elimination;
pub(crate) mod fallback;
pub(crate) mod free_day1;
pub(crate) mod grace_lock;
pub(crate) mod kill_report;
pub(crate) mod respawn_far;
pub(crate) mod spectate;
pub(crate) mod status;
#[cfg(test)]
mod testutil;

use defeat::DefeatInfo;
use elimination::{Decision, Seen};
use status::StatusParts;

/// This game's player won (event 99100803: banner 4000, 10 s, 7000).
pub(crate) const FLAG_WIN: u32 = 10_007_411;
/// This game's player lost (event 99100804: banner 4010, 10 s, 7001).
pub(crate) const FLAG_LOSE: u32 = 10_007_412;
/// UI-4: shows the spectator hint (event 99100806: blinking message 99100600), raised while a defeated player watches.
pub(crate) const FLAG_SPECTATE_HINT: u32 = 10_007_413;
/// Dev: LOCAL gets SpEffect 102130 (die at once) while this is ON (event 99100805; `archive/research/M5-8-death.md` §R1). Set by hand
/// (console `flag`) to test a defeat in offline mode, where the offline revive effect would otherwise bring LOCAL back.
pub(crate) const FLAG_DIE: u32 = 10_007_420;
/// The first circle of Day 2 starts closing here (`PlayAreaCreateCommonParam` row 0, MEMORY_MAP §16).
pub(crate) const TIDE_FROM: ClockTime = ClockTime { h: 12, m: 30, s: 0 };
/// The host's session player id has this in its upper half (`0x6001FFFFFFFF`; FS-0, run 21, M5-4 runs 1-2).
pub(crate) const HOST_ID_HI: u64 = 0x6001;

/// Is this game the host? The session role says so, and it follows the game's own host migration (HOST_MIGRATION.md): a promoted client
/// keeps its id. Without a session (offline, solo) the id decides.
fn is_host(role: Option<u32>, local_id: u64) -> bool {
    use crate::game::layout::{SESSION_ROLE_CLIENT, SESSION_ROLE_HOST};
    match role {
        Some(SESSION_ROLE_HOST) => true,
        Some(SESSION_ROLE_CLIENT) => false,
        _ => local_id >> 32 == HOST_ID_HI,
    }
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

fn before(t: ClockTime, limit: ClockTime) -> bool {
    (t.h, t.m, t.s) < (limit.h, limit.m, limit.s)
}

/// What the rules share: the facts more than one rule reads. State only one rule uses lives in that rule. Single writer per field
/// (the comment on each says who); the exceptions are named there.
#[derive(Debug, Default)]
pub(crate) struct MatchBook {
    /// Dev console `night_count 0`: the match never ends by count. Writer: [`MatchFeature::tick`] (kept across a reset).
    pub(crate) count_off: bool,
    /// `spectate` from the config, or the console's override. Writer: [`MatchFeature::tick`] (kept across a reset).
    pub(crate) spectate: bool,
    /// `day1_secs` from the config (0 = off). Writer: construction (kept across a reset).
    day1_secs: u32,
    /// `defeat_exit_secs` in ms (0 = never). Writer: construction (kept across a reset).
    exit_ms: u64,
    /// `grace_combat_secs` in ms (0 = the grace lock is off). Writer: [`MatchFeature::new`] (kept across a reset).
    grace_ms: u64,
    /// Writer: [`Match::step`].
    in_expedition: bool,
    /// Every player seen in this expedition. Writer: [`elimination::Elimination`].
    seen: BTreeMap<u64, Seen>,
    /// Writer: [`elimination::Elimination`].
    max_teams: usize,
    /// Writer: [`elimination::Elimination`].
    lone_since: Option<u64>,
    /// Writer: [`elimination::Elimination`].
    decided: Option<Decision>,
    /// Writer: [`elimination::Elimination`].
    match_over: bool,
    /// LOCAL is defeated for good. Writer: [`defeat::Defeat`]; the one exception is `exit_sent`, which [`spectate::Spectate`] also
    /// sets when F10 asks for the defeat screen (both mean "the defeat screen was asked for").
    defeat: Option<DefeatInfo>,
    /// Mob deaths judged. Writer: [`defeat::Defeat`].
    mob_deaths: u32,
    /// RW-7: a mob's death was judged; the respawn is moved once LOCAL has stood long enough. Writer: [`defeat::Defeat`] sets it,
    /// [`respawn_far::RespawnFar`] clears it (a hand-over, not a shared write).
    respawn_pending: bool,
}

impl MatchBook {
    pub(crate) fn new(day1_secs: u32, exit_secs: u32, spectate: bool) -> Self {
        MatchBook {
            day1_secs,
            spectate,
            exit_ms: u64::from(exit_secs) * 1000,
            ..MatchBook::default()
        }
    }

    /// Back in the Roundtable (or a load): forget the expedition, keep the config values and the console's `night_count`.
    fn reset(&mut self) {
        let (day1_secs, exit_ms, spectate) = (self.day1_secs, self.exit_ms, self.spectate);
        let (count_off, grace_ms) = (self.count_off, self.grace_ms);
        *self = MatchBook {
            count_off,
            grace_ms,
            day1_secs,
            exit_ms,
            spectate,
            ..MatchBook::default()
        };
    }
}

/// One tick's input, the same for every rule. Built by [`Match::step`] once LOCAL and the flags are known.
pub(crate) struct Tick<'a> {
    pub now_ms: u64,
    pub world: &'a World,
    pub nf: NightFlags,
    pub local: Fighter,
    /// Every fighter in the player list with its slot in `world.positions`.
    pub fighters: &'a [(usize, Fighter)],
    /// BOT-2: the rival bots as fighters (no player-list slot; each its own team). Only the elimination counts them.
    pub bots: &'a [Fighter],
    /// This game is the host's (session id).
    pub host: bool,
}

/// What the rules asked for this tick. Our flags go to the caller's list, everything else is returned.
#[derive(Debug, Default)]
pub(crate) struct MatchOut {
    pub acts: Vec<Act>,
    pub flags: Vec<(u32, bool)>,
    pub lines: Vec<String>,
}

/// One rule of the match. The rules of a [`Match`] run in list order every tick LOCAL and the flags are known.
pub(crate) trait MatchRule: std::any::Any {
    fn step(&mut self, t: &Tick, book: &mut MatchBook, out: &mut MatchOut);
    /// Leaving the expedition: forget what this rule learned (the [`MatchBook`] is reset by the caller).
    fn reset(&mut self) {}
    /// Leaving the expedition, before [`reset`](MatchRule::reset): undo what the rule switched on in the game.
    fn leave(&mut self, _out: &mut MatchOut) {}
    /// Add this rule's part of the `night_status` line (see [`status`]).
    fn describe(&self, _parts: &mut StatusParts) {}
}

/// The decision core (unit tested): a book and its rules.
pub(crate) struct Match {
    pub(crate) book: MatchBook,
    rules: Vec<Box<dyn MatchRule>>,
    pub(crate) status: String,
}

impl Match {
    pub(crate) fn new(book: MatchBook, rules: Vec<Box<dyn MatchRule>>) -> Self {
        Match {
            book,
            rules,
            status: String::new(),
        }
    }

    /// The Battle Royale rules, in the order the old `NightCore::step` ran them.
    pub(crate) fn battle_royale(day1_secs: u32, exit_secs: u32, spectate: bool) -> Self {
        Match::new(
            MatchBook::new(day1_secs, exit_secs, spectate),
            vec![
                Box::<defeat::Defeat>::default(),
                Box::<kill_report::KillReport>::default(),
                Box::<elimination::Elimination>::default(),
                Box::<spectate::Spectate>::default(),
                Box::<respawn_far::RespawnFar>::default(),
                Box::<free_day1::FreeDay1>::default(),
                Box::<grace_lock::GraceLock>::default(),
                Box::<fallback::FlagFallback>::default(),
            ],
        )
    }

    #[cfg(test)]
    pub(crate) fn rule_mut<T: MatchRule>(&mut self) -> &mut T {
        self.rules
            .iter_mut()
            .find_map(|r| (r.as_mut() as &mut dyn std::any::Any).downcast_mut::<T>())
            .expect("the rule is in the list")
    }

    /// One tick. Our flags go to `flags`, everything else is returned.
    pub(crate) fn step(
        &mut self,
        now_ms: u64,
        world: &World,
        flags: &mut Vec<(u32, bool)>,
    ) -> (Vec<Act>, Vec<String>) {
        let mut out = MatchOut::default();
        let fighters: Vec<(usize, Fighter)> = world
            .fighters
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.map(|f| (i, f)))
            .collect();
        let local = fighters.iter().map(|(_, f)| *f).find(|f| f.local);
        if !world.in_expedition() {
            if self.book.in_expedition {
                // Back in the Roundtable (or a load): forget the expedition, put the rules back.
                self.book.reset();
                self.status.clear();
                for r in &mut self.rules {
                    r.leave(&mut out);
                    r.reset();
                }
                out.acts.push(Act::DeathHold(false));
                out.acts.push(Act::MenuGate(false));
                out.lines
                    .push("night_match: out of the expedition; rules reset".into());
            }
            return (out.acts, out.lines);
        }
        // A tick without the flags or without LOCAL in the player list decides nothing and forgets nothing.
        let (Some(nf), Some(local)) = (world.night, local) else {
            return (out.acts, out.lines);
        };
        // FS7-6: the die-at-once flag is a test aid only (no rule of the player build sets it), but our flag group is saved with the
        // character: a test run on the same account left it ON and the player died with no downed state in a real match. Clear it as
        // the expedition starts (the night flags are read only inside one, so the end of an expedition cannot see it). A scenario that
        // wants it sets it after this first tick.
        if !self.book.in_expedition && nf.our_die {
            flags.push((FLAG_DIE, false));
            out.lines.push(
                "night_match: die-at-once flag 10007420 was ON from the save; switched off".into(),
            );
        }
        self.book.in_expedition = true;
        let bots: Vec<Fighter> = world.rival_bots().map(bot_fighter).collect();
        let t = Tick {
            now_ms,
            world,
            nf,
            local,
            fighters: &fighters,
            bots: &bots,
            host: is_host(world.session_role, local.id),
        };
        for r in &mut self.rules {
            r.step(&t, &mut self.book, &mut out);
        }
        let mut parts = StatusParts::default();
        for r in &self.rules {
            r.describe(&mut parts);
        }
        self.status = status::render(now_ms, fighters.len(), &nf, &self.book, &parts);
        flags.append(&mut out.flags);
        (out.acts, out.lines)
    }
}

/// BOT-2: a rival bot as the elimination sees it: its handle is its id and its own team (bit 62 set, so it never equals a player's
/// session id or a roster team key, `team_key`). A bot is never downed (solo play kills at 0 HP).
pub(crate) fn bot_fighter(b: &crate::game::world::BotChr) -> Fighter {
    Fighter {
        addr: b.chr,
        id: b.handle,
        team: (1 << 62) | (b.handle & 0xFFFF_FFFF),
        local: false,
        state: 0,
        hp: b.hp,
        clock: 0.0,
    }
}

/// The match rules as a feature of the main loop (`Rules` phase).
pub(crate) struct MatchFeature {
    core: Match,
    spectate_cfg: bool,
    started: Instant,
}

impl MatchFeature {
    pub(crate) fn new(cfg: &crate::config::Config) -> Self {
        spectate::set_cfg(cfg.spectate);
        let mut core = Match::battle_royale(cfg.day1_secs, cfg.defeat_exit_secs, cfg.spectate);
        core.book.grace_ms = u64::from(cfg.grace_combat_secs) * 1000;
        MatchFeature {
            core,
            spectate_cfg: cfg.spectate,
            started: Instant::now(),
        }
    }
}

impl Feature for MatchFeature {
    fn name(&self) -> &'static str {
        "night_match"
    }

    fn tick(&mut self, world: &World, ctx: &mut Ctx) {
        let now_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.core.book.count_off = !elimination::COUNT.load(Ordering::Relaxed);
        self.core.book.spectate = spectate::spectate_now(self.spectate_cfg);
        let (acts, lines) = self.core.step(now_ms, world, &mut ctx.flags);
        for l in lines {
            tracing::info!("{l}");
        }
        ctx.acts.extend(acts);
        status::publish(&self.core.status);
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;
    use crate::game::views::Place;

    #[test]
    fn the_host_is_the_session_role_and_survives_a_migration() {
        use crate::game::layout::{SESSION_ROLE_CLIENT, SESSION_ROLE_HOST};
        // The game promoted this client: its id stays 0x6003, the role is what changed.
        assert!(is_host(Some(SESSION_ROLE_HOST), P3));
        // An old host's id on a game that is a client now.
        assert!(!is_host(Some(SESSION_ROLE_CLIENT), ME));
        // No session (offline, solo): the id decides.
        assert!(is_host(Some(0), ME));
        assert!(is_host(None, ME));
        assert!(!is_host(None, P2));
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
        assert_eq!(acts, vec![Act::DeathHold(false), Act::MenuGate(false)]);
        assert!(c.book.defeat.is_none() && c.book.seen.is_empty());
    }

    #[test]
    fn our_flag_ids_match_the_game_data_file() {
        let nights = include_str!("../../../../../gamedata/emevd-nights.json");
        for (id, flag) in [
            (99_100_803, FLAG_WIN),
            (99_100_804, FLAG_LOSE),
            (99_100_805, FLAG_DIE),
            (99_100_806, FLAG_SPECTATE_HINT),
        ] {
            assert!(
                nights.contains(&format!("\"id\": {id},"))
                    && nights.contains(&format!("\"flag\": {flag} ")),
                "event {id} / flag {flag}"
            );
        }
    }
}
