//! OFF-5: an offline Solo match with rival bots. As soon as an offline expedition is loaded, this feature asks for `offline_bots`
//! (default 5) of the game's own NPC bots and keeps them hostile. The game itself holds an armed bot table until the player has
//! landed after the hawk crossing, so the bots arrive at the landing (OFF-5 run 2: armed during the ride, spawned at the landing).
//! Engine facts: MEMORY_MAP §15, TECHNICAL_NOTES §4d, TESTING §3 OFF-4 / OFF-5.
//!
//! The game's bot manager holds only 2 bots, so the director works in rounds ("detach and re-arm"):
//! 1. set our flag for a bot table (our `common.emevd` event runs `2003[97]`; the manager spawns the table's 2 Nightfarers);
//! 2. wait until the manager's 2 slots hold new handles; keep as many as are still wanted;
//! 3. make the manager forget the kept bots (their slot handle becomes -1; data only, see `BotMan::forget_slot`); a slot that holds
//!    a bot not wanted is left alone and removed by `2003[98]` at the end;
//! 4. next round with another table, until the count is reached; then `2003[98]` (removes only what is still in the slots).
//!
//! Every tick, each kept bot that is found (by handle, `World::bot_chrs`) and is not on the wanted team gets it: the rival value
//! (`pvpve_team`, 29: attacks the player, the other bots and monsters) while hostile (the default), or the game's own bot value
//! [`ALLY_TEAM`] (2: fights on the player's side) after the dev console's `bots_hostile 0`. If the manager ever puts more new bots in its slots than asked, they are counted
//! like a round's spawn; nothing grows past the wanted count. Offline mode only; a new expedition starts a new set.
//! Log lines start with `bot_match:`.

use std::time::Instant;

use super::mode::GameMode;
use super::{BotWrite, Ctx, Feature};
use crate::game::views::Place;
use crate::game::world::{World, MAX_WATCHED};

/// A round that sees no new bots this long gives up (and the director moves on). Long on purpose: the game spawns an armed table
/// only once the host is on the ground (OFF-5 run 1: armed during the hawk ride, nothing for 15 s; the next table spawned at the
/// landing), so the first round may wait out the rest of the ride.
pub(crate) const ROUND_TIMEOUT_MS: u64 = 120_000;
/// Pause between forgetting a round's bots and arming the next table.
pub(crate) const ROUND_GAP_MS: u64 = 1_000;
/// The bot tables of the rounds, in order (`NPCBotTableParam`: 10 = heroes 1 + 2, 30 = 3 + 4, 50 = 5 + 6, all level 1), with the
/// flags of their events in `gamedata/emevd-bots.json`.
pub(crate) const ROUND_TABLES: [(i32, u32); 3] =
    [(10, 10_007_300), (30, 10_007_303), (50, 10_007_304)];
/// Flag of the `2003[98]` event (removes what is still in the manager's slots).
pub(crate) const FLAG_CLEAR: u32 = super::bots::FLAG_CLEAR;
/// The most bots a match asks for (5 rivals for a 6-player Solo match).
pub(crate) const MAX_BOTS: usize = 5;
/// The team the game gives its bots (`NpcParam 600040000`): allies of the player.
pub(crate) const ALLY_TEAM: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Not in an offline expedition.
    Idle,
    /// Round `n` armed at `at`.
    Spawning { n: usize, at: u64 },
    /// Round `n` done at `at`; the next round starts after [`ROUND_GAP_MS`].
    Gap { n: usize, at: u64 },
    /// All rounds done (or given up).
    Done,
}

/// One kept bot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Kept {
    handle: u64,
    /// Character set it was last found in (search hint).
    set: Option<usize>,
    /// Logged once it was found.
    seen: bool,
    /// The team last written (0 = none yet), so each change is logged once.
    applied: u8,
    /// Ticks in a row it was not found (the world searches every set for it, so a missing bot is looked for less often).
    misses: u32,
    /// Seen before and missing for [`GONE_AFTER`] ticks: dead or removed; no longer watched.
    gone: bool,
}

impl Kept {
    fn new(handle: u64) -> Self {
        Kept {
            handle,
            set: None,
            seen: false,
            applied: 0,
            misses: 0,
            gone: false,
        }
    }
}

/// Ticks (50 ms) a bot that was seen may be missing before it counts as gone (3 s).
const GONE_AFTER: u32 = 60;
/// A bot not found yet is searched for every this many ticks (1 s).
const RETRY_EVERY: u32 = 20;

pub(crate) struct BotMatch {
    /// Bots wanted (`offline_bots`, 0..=5; 0 = off).
    want: usize,
    /// Team value written on kept bots while hostile (`pvpve_team`).
    rival_team: u8,
    /// Hostile (default) or friendly bots (dev console `bots_hostile`); kept across expeditions.
    hostile: bool,
    stage: Stage,
    kept: Vec<Kept>,
    /// Handles seen in the manager's slots and dealt with (kept or left for the clear), so they are not counted twice.
    handled: Vec<u64>,
    epoch: Instant,
}

const EMPTY: u64 = u64::MAX;

impl BotMatch {
    pub(crate) fn new(want: usize, rival_team: u8) -> Self {
        BotMatch {
            want: want.min(MAX_BOTS),
            rival_team,
            hostile: true,
            stage: Stage::Idle,
            kept: Vec::new(),
            handled: Vec::new(),
            epoch: Instant::now(),
        }
    }

    /// Handles the world should look for (kept bots), with their last set.
    pub(crate) fn watched(&self) -> Vec<(u64, Option<usize>)> {
        self.kept
            .iter()
            .filter(|k| !k.gone && (k.misses == 0 || k.misses % RETRY_EVERY == 0))
            .take(MAX_WATCHED)
            .map(|k| (k.handle, k.set))
            .collect()
    }

    /// One step (pure apart from logging; unit tested with a made-up clock).
    pub(crate) fn step(&mut self, now: u64, mode: GameMode, world: &World, ctx: &mut Ctx) {
        let active = self.want > 0 && mode == GameMode::Offline && world.in_expedition();
        if !active {
            let over = world.wcm_instance == 0 || world.place == Some(Place::Roundtable);
            if self.stage != Stage::Idle && over {
                // Load screen, title or back in the Roundtable: the expedition is over; its bots are gone with it.
                tracing::info!(
                    "bot_match: expedition over; {} bot(s) were kept",
                    self.kept.len()
                );
                self.reset();
            }
            return;
        }
        self.keep_rivals(world, ctx);
        let slots = world
            .bots
            .map(|b| [b.heads[0], b.heads[1]])
            .unwrap_or([EMPTY; 2]);
        match self.stage {
            Stage::Idle => {
                tracing::info!(
                    "bot_match: offline expedition: {} bot(s); the game brings them at the landing",
                    self.want
                );
                self.arm(0, now, ctx);
            }
            Stage::Spawning { n, at } => {
                let fresh: Vec<(usize, u64)> = slots
                    .iter()
                    .enumerate()
                    .filter(|(_, h)| **h != EMPTY && **h != 0 && !self.handled.contains(h))
                    .map(|(i, h)| (i, *h))
                    .collect();
                let both = slots.iter().all(|h| *h != EMPTY && *h != 0);
                if both && !fresh.is_empty() {
                    for (slot, h) in fresh {
                        self.handled.push(h);
                        if self.kept.len() < self.want {
                            self.kept.push(Kept::new(h));
                            ctx.bot_writes.push(BotWrite::ForgetSlot(slot));
                            tracing::info!(
                                "bot_match: round {}: kept bot 0x{h:X} (slot {slot}); {} of {}",
                                n + 1,
                                self.kept.len(),
                                self.want
                            );
                        } else {
                            tracing::info!("bot_match: round {}: bot 0x{h:X} (slot {slot}) not needed; the clear removes it", n + 1);
                        }
                    }
                    self.stage = Stage::Gap { n, at: now };
                } else if now.saturating_sub(at) >= ROUND_TIMEOUT_MS {
                    tracing::warn!(
                        "bot_match: round {}: no new bots after 120 s (slots {:X?}); moving on",
                        n + 1,
                        slots
                    );
                    self.stage = Stage::Gap { n, at: now };
                }
            }
            Stage::Gap { n, at } => {
                if now.saturating_sub(at) < ROUND_GAP_MS {
                    return;
                }
                if self.kept.len() < self.want && n + 1 < ROUND_TABLES.len() {
                    self.arm(n + 1, now, ctx);
                } else {
                    ctx.flags.push((FLAG_CLEAR, true));
                    tracing::info!(
                        "bot_match: done: {} of {} bot(s) kept; clearing the manager",
                        self.kept.len(),
                        self.want
                    );
                    self.stage = Stage::Done;
                }
            }
            Stage::Done => {}
        }
    }

    fn arm(&mut self, n: usize, now: u64, ctx: &mut Ctx) {
        let (table, flag) = ROUND_TABLES[n];
        ctx.flags.push((flag, true));
        tracing::info!("bot_match: round {}: table {table} (flag {flag})", n + 1);
        self.stage = Stage::Spawning { n, at: now };
    }

    /// Hostile bots (rival team) or friendly ones (the game's ally team). Takes effect on the next tick, for the bots already here too.
    pub(crate) fn set_hostile(&mut self, on: bool) {
        self.hostile = on;
        tracing::info!(
            "bot_match: bots are now {} (team {})",
            if on { "hostile" } else { "friendly" },
            self.target_team()
        );
    }

    fn target_team(&self) -> u8 {
        if self.hostile {
            self.rival_team
        } else {
            ALLY_TEAM
        }
    }

    /// Keep every found bot on the wanted team; remember where it was found.
    fn keep_rivals(&mut self, world: &World, ctx: &mut Ctx) {
        let target = self.target_team();
        for k in &mut self.kept {
            if k.gone {
                continue;
            }
            let Some(b) = world.bot_chr(k.handle) else {
                k.misses += 1;
                if k.seen && k.misses >= GONE_AFTER {
                    k.gone = true;
                    tracing::info!("bot_match: bot 0x{:X} is gone (dead or removed)", k.handle);
                }
                continue;
            };
            k.misses = 0;
            k.set = Some(b.set);
            if !k.seen {
                k.seen = true;
                tracing::info!(
                    "bot_match: bot 0x{:X} is ChrIns 0x{:X} (set {}, npc id {}, team {})",
                    k.handle,
                    b.chr,
                    b.set,
                    b.npc_id,
                    b.team
                );
            }
            if b.team != target {
                ctx.bot_writes.push(BotWrite::Team {
                    chr: b.chr,
                    handle: k.handle,
                    team: target,
                });
                if k.applied != target {
                    k.applied = target;
                    tracing::info!(
                        "bot_match: bot 0x{:X}: team {} -> {}",
                        k.handle,
                        b.team,
                        target
                    );
                }
            }
        }
    }

    fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.kept.clear();
        self.handled.clear();
    }

    /// One line for the console.
    pub(crate) fn status(&self) -> String {
        format!(
            "bot_match: {:?}, {} of {} kept, {} ({} alive)",
            self.stage,
            self.kept.len(),
            self.want,
            if self.hostile { "hostile" } else { "friendly" },
            self.kept.iter().filter(|k| !k.gone).count()
        )
    }
}

impl Feature for BotMatch {
    fn name(&self) -> &'static str {
        "bot_match"
    }

    fn tick(&mut self, world: &World, ctx: &mut Ctx) {
        let now = self.epoch.elapsed().as_millis() as u64;
        self.step(now, super::mode::current(), world, ctx);
        super::bots::set_match_status(self.status());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::views::BotManState;
    use crate::game::world::BotChr;

    fn world(slots: [u64; 2]) -> World {
        let mut heads = [0u64; 8];
        heads[..2].copy_from_slice(&slots);
        World {
            wcm_instance: 0x5_0000,
            local: Some(0x10_0000),
            place: Some(Place::Expedition),
            bots: Some(BotManState {
                table: 10,
                armed: false,
                bots: 2,
                heads,
            }),
            ..World::default()
        }
    }

    const E: u64 = EMPTY;

    /// Drive the director through the rounds; the manager fills both slots with new handles after each arm.
    #[test]
    fn five_bots_in_three_rounds() {
        let mut d = BotMatch::new(5, 29);
        let mut ctx = Ctx::default();
        let mut t = 0;
        let mut go = |d: &mut BotMatch, w: &World, ctx: &mut Ctx, dt: u64| {
            t += dt;
            d.step(t, GameMode::Offline, w, ctx);
        };
        // The expedition is loaded: round 1 is armed at once (the game waits for the landing itself).
        go(&mut d, &world([E, E]), &mut ctx, 0);
        assert_eq!(ctx.flags, vec![(10_007_300, true)]);
        // Nothing yet during the ride.
        let mut ctx = Ctx::default();
        go(&mut d, &world([E, E]), &mut ctx, 50_000);
        assert_eq!(ctx, Ctx::default());
        // Round 1 spawns 2: both kept and forgotten.
        let mut ctx = Ctx::default();
        go(&mut d, &world([0xA0, 0xA1]), &mut ctx, 50);
        assert_eq!(
            ctx.bot_writes,
            vec![BotWrite::ForgetSlot(0), BotWrite::ForgetSlot(1)]
        );
        // Gap, then round 2.
        let mut ctx = Ctx::default();
        go(&mut d, &world([E, E]), &mut ctx, ROUND_GAP_MS);
        assert_eq!(ctx.flags, vec![(10_007_303, true)]);
        go(&mut d, &world([0xB0, 0xB1]), &mut ctx, 50);
        go(&mut d, &world([E, E]), &mut ctx, ROUND_GAP_MS);
        // Round 3: only one more is wanted; slot 1 stays for the clear.
        let mut ctx = Ctx::default();
        go(&mut d, &world([0xC0, 0xC1]), &mut ctx, 50);
        assert_eq!(ctx.bot_writes, vec![BotWrite::ForgetSlot(0)]);
        let mut ctx = Ctx::default();
        go(&mut d, &world([E, 0xC1]), &mut ctx, ROUND_GAP_MS);
        assert_eq!(ctx.flags, vec![(FLAG_CLEAR, true)]);
        assert_eq!(d.kept.len(), 5);
        assert_eq!(d.stage, Stage::Done);
    }

    #[test]
    fn found_bots_are_made_rivals_and_kept_so() {
        let mut d = BotMatch::new(1, 29);
        d.stage = Stage::Done;
        d.kept.push(Kept::new(0xA0));
        let mut w = world([E, E]);
        w.bot_chrs[0] = Some(BotChr {
            handle: 0xA0,
            chr: 0x7000,
            set: 14,
            team: 2,
            npc_id: 0,
        });
        let mut ctx = Ctx::default();
        d.step(0, GameMode::Offline, &w, &mut ctx);
        assert_eq!(
            ctx.bot_writes,
            vec![BotWrite::Team {
                chr: 0x7000,
                handle: 0xA0,
                team: 29
            }]
        );
        assert_eq!(d.watched(), vec![(0xA0, Some(14))]);
        // Already a rival: nothing to write.
        w.bot_chrs[0].as_mut().expect("bot").team = 29;
        let mut ctx = Ctx::default();
        d.step(50, GameMode::Offline, &w, &mut ctx);
        assert!(ctx.bot_writes.is_empty());
    }

    #[test]
    fn nothing_happens_outside_offline_or_when_off() {
        for (want, mode) in [(5, GameMode::BattleRoyale), (0, GameMode::Offline)] {
            let mut d = BotMatch::new(want, 29);
            let mut ctx = Ctx::default();
            d.step(0, mode, &world([E, E]), &mut ctx);
            d.step(ROUND_TIMEOUT_MS, mode, &world([E, E]), &mut ctx);
            assert_eq!(ctx, Ctx::default());
            assert_eq!(d.stage, Stage::Idle);
        }
    }

    #[test]
    fn back_in_the_roundtable_ends_the_set() {
        let mut d = BotMatch::new(5, 29);
        d.stage = Stage::Done;
        d.kept.push(Kept::new(0xA0));
        let mut w = world([E, E]);
        w.place = Some(crate::game::views::Place::Roundtable);
        let mut ctx = Ctx::default();
        d.step(0, GameMode::Offline, &w, &mut ctx);
        assert_eq!(d.stage, Stage::Idle);
        assert!(d.kept.is_empty());
    }

    #[test]
    fn a_round_without_bots_moves_on_and_the_end_clears() {
        let mut d = BotMatch::new(5, 29);
        d.stage = Stage::Spawning { n: 2, at: 0 };
        let mut ctx = Ctx::default();
        d.step(
            ROUND_TIMEOUT_MS,
            GameMode::Offline,
            &world([E, E]),
            &mut ctx,
        );
        d.step(
            ROUND_TIMEOUT_MS + ROUND_GAP_MS,
            GameMode::Offline,
            &world([E, E]),
            &mut ctx,
        );
        assert_eq!(ctx.flags, vec![(FLAG_CLEAR, true)]);
        assert_eq!(d.stage, Stage::Done);
    }

    #[test]
    fn a_new_expedition_starts_a_new_set() {
        let mut d = BotMatch::new(5, 29);
        d.stage = Stage::Done;
        d.kept.push(Kept {
            seen: true,
            applied: 29,
            ..Kept::new(0xA0)
        });
        let mut ctx = Ctx::default();
        d.step(0, GameMode::Offline, &World::default(), &mut ctx);
        assert_eq!(d.stage, Stage::Idle);
        assert!(d.watched().is_empty());
    }

    #[test]
    fn a_dead_bot_stops_being_watched() {
        let mut d = BotMatch::new(1, 29);
        d.stage = Stage::Done;
        d.kept.push(Kept {
            seen: true,
            ..Kept::new(0xA0)
        });
        let w = world([E, E]);
        let mut ctx = Ctx::default();
        for t in 0..u64::from(GONE_AFTER) {
            assert!(!d.watched().is_empty() || t % u64::from(RETRY_EVERY) != 0);
            d.step(t * 50, GameMode::Offline, &w, &mut ctx);
        }
        assert!(d.kept[0].gone);
        assert!(d.watched().is_empty());
    }

    #[test]
    fn friendly_bots_go_back_to_the_ally_team_and_hostile_again() {
        let mut d = BotMatch::new(1, 29);
        d.stage = Stage::Done;
        d.kept.push(Kept::new(0xA0));
        let mut w = world([E, E]);
        w.bot_chrs[0] = Some(BotChr {
            handle: 0xA0,
            chr: 0x7000,
            set: 14,
            team: 29,
            npc_id: 0,
        });
        d.set_hostile(false);
        let mut ctx = Ctx::default();
        d.step(0, GameMode::Offline, &w, &mut ctx);
        let team = |t| BotWrite::Team {
            chr: 0x7000,
            handle: 0xA0,
            team: t,
        };
        assert_eq!(ctx.bot_writes, vec![team(ALLY_TEAM)]);
        w.bot_chrs[0].as_mut().expect("bot").team = ALLY_TEAM;
        d.set_hostile(true);
        let mut ctx = Ctx::default();
        d.step(50, GameMode::Offline, &w, &mut ctx);
        assert_eq!(ctx.bot_writes, vec![team(29)]);
    }

    /// Every round table has its event in gamedata/emevd-bots.json.
    #[test]
    fn gamedata_has_every_round_table() {
        let events = include_str!("../../../../gamedata/emevd-bots.json");
        for (table, flag) in ROUND_TABLES {
            let want = format!("\"flag\": {flag}, \"bots\": {table} }}");
            assert!(events.contains(&want), "missing: {want}");
        }
    }
}
