//! BOT-1: fill an expedition with hostile bots (BOTS.md §2a). The OFF-5 director (git `72f7689`, `features/bot_match.rs`), back as a
//! feature. As soon as an expedition is loaded it asks for `want` of the game's own NPC bots and keeps them on the rival team. The game
//! holds an armed table until the host has landed after the hawk crossing, so the bots arrive at the landing (OFF-5 run 2).
//!
//! The game's bot manager holds only 2 bots, so the director works in rounds ("detach and re-arm", MEMORY_MAP §15):
//! 1. set our flag for a bot table (our `common.emevd` event runs `2003[97]`; the manager spawns the table's 2 Nightfarers);
//! 2. wait until the manager's 2 slots hold new handles; keep as many as are still wanted;
//! 3. make the manager forget the kept bots (their slot handle becomes -1); a slot that holds a bot not wanted is left alone and removed
//!    by `2003[98]` at the end;
//! 4. next round with the next table, until the count is reached; then `2003[98]` (removes only what is still in the slots).
//!
//! Every tick each kept bot found in the bot set (`World::bot_chrs`) that is not on the rival team gets it (29: attacks the player,
//! the other bots and monsters; OFF-5 run 2). The roster (alive, HP, position) is what the later cards read (BOT-2 rules, BOT-3
//! brain). No hardware breakpoint, no game call: flags and two kinds of data writes ([`BotWrite`]). Offline mode only in BOT-1.
//! Log lines start with `bots:`.

use super::super::mode::GameMode;
use super::super::{BotWrite, Ctx};
use crate::game::views::Place;
use crate::game::world::World;

/// A round that sees no new bots this long gives up (and the director moves on). Long on purpose: the first round may wait out the
/// rest of the hawk ride (the game spawns an armed table only once the host is on the ground).
pub(crate) const ROUND_TIMEOUT_MS: u64 = 120_000;
/// Pause between forgetting a round's bots and arming the next table.
pub(crate) const ROUND_GAP_MS: u64 = 1_000;
/// The bot tables of the rounds, in order and then again (`NPCBotTableParam`: 10 = heroes 1 + 2, 30 = 3 + 4, 50 = 5 + 6, all level
/// 1), with the flags of their events in `gamedata/emevd-bots.json`.
pub(crate) const ROUND_TABLES: [(i32, u32); 3] =
    [(10, 10_007_300), (30, 10_007_303), (50, 10_007_304)];
/// A director never arms more rounds than this in one expedition (a guard against a manager that never spawns what we keep; mob
/// deaths arm a round each).
pub(crate) const MAX_ROUNDS: usize = 40;
/// BOT-2: a bot killed by a mob is replaced this long after its death (a player's mob death respawns in about 14 s, RW-2).
pub(crate) const RESPAWN_MS: u64 = 14_000;
/// The most bots a match asks for (5 rivals for a 6-player Solo match).
pub(crate) const MAX_BOTS: usize = 5;
/// Ticks (50 ms) a kept bot that was seen may be missing from the set before it counts as gone (3 s).
const GONE_AFTER: u32 = 60;

const EMPTY: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    /// Not in an expedition (or off).
    Idle,
    /// Round `n` armed at `at`.
    Spawning { n: usize, at: u64 },
    /// Round `n` done at `at`; the next round starts after [`ROUND_GAP_MS`].
    Gap { n: usize, at: u64 },
    /// All rounds done (or given up).
    Done,
}

/// BOT-2: who killed a bot (its last attacker at the moment its HP read 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Death {
    /// LOCAL or another rival bot: out of the match for good.
    Rival,
    /// An enemy (any other valid handle): replaced by a new bot after [`RESPAWN_MS`], like a player's mob death (RW-2).
    Mob,
    /// No attacker (the tide, a fall): out for good.
    Env,
}

/// The cause of a bot's death from its last attacker (pure; unit tested).
pub(crate) fn death_cause(attacker: u64, local: Option<u64>, rival_bots: &[u64]) -> Death {
    if attacker == 0 || attacker == u64::MAX {
        Death::Env
    } else if Some(attacker) == local || rival_bots.contains(&attacker) {
        Death::Rival
    } else {
        Death::Mob
    }
}

/// One kept bot and what the roster knows about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Kept {
    pub handle: u64,
    /// Found in the set at least once (logged once).
    pub seen: bool,
    /// The team last written (0 = none yet), so each change is logged once.
    applied: u8,
    /// Ticks in a row it was not found in the set.
    misses: u32,
    /// Seen before and missing for [`GONE_AFTER`] ticks: removed from the world.
    pub gone: bool,
    /// HP read 0 (logged once; whether a dead bot leaves the set is evidence for BOT-2).
    pub dead: bool,
    pub hp: u32,
    pub max_hp: u32,
    pub pos: Option<[f32; 3]>,
    /// Sent to a camp once (`BotWrite::Spread`).
    spread: bool,
    /// BOT-2: how it died and when (ms), once its HP read 0.
    pub death: Option<(Death, u64)>,
    /// BOT-2: killed by a mob and its replacement was asked for: no longer one of the wanted bots.
    pub replaced: bool,
}

impl Kept {
    fn new(handle: u64) -> Self {
        Kept {
            handle,
            seen: false,
            applied: 0,
            misses: 0,
            gone: false,
            dead: false,
            hp: 0,
            max_hp: 0,
            pos: None,
            spread: false,
            death: None,
            replaced: false,
        }
    }
}

pub(crate) struct BotFill {
    /// Bots wanted (`bots_fill`, 0..=5; 0 = off).
    want: usize,
    /// Team value written on kept bots (`pvpve_team`).
    rival_team: u8,
    pub(crate) stage: Stage,
    pub(crate) kept: Vec<Kept>,
    /// Handles seen in the manager's slots and dealt with (kept or left for the clear), so they are not counted twice.
    handled: Vec<u64>,
    /// Bots spawned but not needed (the clear removes them about a second later); each is still sent away from LOCAL once, so no bot
    /// is ever seen at LOCAL's side. `true` = sent.
    extra: Vec<(u64, bool)>,
    /// Rounds armed in this expedition.
    rounds: usize,
    /// Send each kept bot to a far camp once (`bots_spread`, on by default).
    pub(crate) spread: bool,
    /// LOCAL's world position at the last step (the roster's distances).
    me: Option<[f32; 3]>,
    /// BOT-5: this expedition is a match against bots (Bots On at Commence): active in any mode, with [`MAX_BOTS`] bots.
    bots_match: bool,
    /// The count from the config / console, put back when a bots match ends.
    cfg_want: usize,
    /// An expedition this director was active in has just ended (read once by [`BotFill::take_over`]).
    over: bool,
}

impl BotFill {
    pub(crate) fn new(want: usize, rival_team: u8) -> Self {
        BotFill {
            want: want.min(MAX_BOTS),
            rival_team,
            stage: Stage::Idle,
            kept: Vec::new(),
            handled: Vec::new(),
            extra: Vec::new(),
            rounds: 0,
            spread: true,
            me: None,
            bots_match: false,
            cfg_want: want.min(MAX_BOTS),
            over: false,
        }
    }

    pub(crate) fn want(&self) -> usize {
        self.want
    }

    /// Kept bots that still play for the fill count (not replaced after a mob death).
    fn active(&self) -> usize {
        self.kept.iter().filter(|k| !k.replaced).count()
    }

    /// BOT-5: whether the current (or next) expedition is a match against bots.
    pub(crate) fn set_bots_match(&mut self, on: bool) {
        self.bots_match = on;
    }

    /// BOT-5: true once after an expedition this director was active in ended.
    pub(crate) fn take_over(&mut self) -> bool {
        std::mem::take(&mut self.over)
    }

    /// Change the wanted count (console `bots_fill`). In an expedition that is done, a higher count arms more rounds.
    pub(crate) fn set_want(&mut self, n: usize, now: u64) {
        self.want = n.min(MAX_BOTS);
        self.cfg_want = self.want;
        if self.stage == Stage::Done && self.active() < self.want && self.rounds < MAX_ROUNDS {
            self.stage = Stage::Gap {
                n: self.rounds - 1,
                at: now.saturating_sub(ROUND_GAP_MS),
            };
        }
        tracing::info!("bots: fill count is now {}", self.want);
    }

    /// One step (pure apart from logging; unit tested with a made-up clock).
    pub(crate) fn step(&mut self, now: u64, mode: GameMode, world: &World, ctx: &mut Ctx) {
        let active = (mode == GameMode::Offline || self.bots_match) && world.in_expedition();
        if active && self.bots_match && self.want < MAX_BOTS && self.stage == Stage::Idle {
            tracing::info!("bots: match against bots: {MAX_BOTS} bots");
            self.want = MAX_BOTS;
        }
        if !active {
            let over = world.wcm_instance == 0 || world.place == Some(Place::Roundtable);
            if self.stage != Stage::Idle && over {
                // Load screen, title or back in the Roundtable: the expedition is over; its bots are gone with it.
                tracing::info!(
                    "bots: expedition over; {} bot(s) were kept",
                    self.kept.len()
                );
                self.reset();
            }
            return;
        }
        self.me = world.local_pos.map(|(p, _)| p);
        self.keep_rivals(now, world, ctx);
        self.respawn_due(now);
        if self.want == 0 && self.stage == Stage::Idle {
            return;
        }
        let slots = world
            .bots
            .map(|b| [b.heads[0], b.heads[1]])
            .unwrap_or([EMPTY; 2]);
        match self.stage {
            Stage::Idle => {
                tracing::info!(
                    "bots: expedition: filling with {} bot(s); the game brings them at the landing",
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
                        if self.active() < self.want {
                            self.kept.push(Kept::new(h));
                            ctx.bot_writes.push(BotWrite::ForgetSlot(slot));
                            tracing::info!(
                                "bots: round {}: kept bot 0x{h:X} (slot {slot}); {} of {}",
                                n + 1,
                                self.active(),
                                self.want
                            );
                        } else {
                            self.extra.push((h, false));
                            tracing::info!(
                                "bots: round {}: bot 0x{h:X} (slot {slot}) not needed; sent away, the clear removes it",
                                n + 1
                            );
                        }
                    }
                    self.stage = Stage::Gap { n, at: now };
                } else if now.saturating_sub(at) >= ROUND_TIMEOUT_MS {
                    tracing::warn!(
                        "bots: round {}: no new bots after {} s (slots {:X?}); moving on",
                        n + 1,
                        ROUND_TIMEOUT_MS / 1000,
                        slots
                    );
                    self.stage = Stage::Gap { n, at: now };
                }
            }
            Stage::Gap { n, at } => {
                if now.saturating_sub(at) < ROUND_GAP_MS {
                    return;
                }
                if self.active() < self.want && self.rounds < MAX_ROUNDS {
                    self.arm(n + 1, now, ctx);
                } else {
                    ctx.flags.push((super::FLAG_CLEAR, true));
                    tracing::info!(
                        "bots: fill done: {} of {} bot(s) kept; clearing the manager",
                        self.active(),
                        self.want
                    );
                    self.stage = Stage::Done;
                }
            }
            Stage::Done => {}
        }
    }

    fn arm(&mut self, n: usize, now: u64, ctx: &mut Ctx) {
        let (table, flag) = ROUND_TABLES[n % ROUND_TABLES.len()];
        ctx.flags.push((flag, true));
        self.rounds += 1;
        tracing::info!("bots: round {}: table {table} (flag {flag})", n + 1);
        self.stage = Stage::Spawning { n, at: now };
    }

    /// Keep every found bot on the rival team and refresh the roster.
    fn keep_rivals(&mut self, now: u64, world: &World, ctx: &mut Ctx) {
        let ground = world.local_pos.is_some_and(|(_, g)| g);
        for (h, sent) in &mut self.extra {
            if *sent || !self.spread || !ground {
                continue;
            }
            if let Some(b) = world.bot(*h).filter(|b| b.pos.is_some()) {
                *sent = true;
                ctx.bot_writes.push(BotWrite::Spread {
                    chr: b.chr,
                    handle: *h,
                    sector: 0,
                    sectors: 1,
                });
            }
        }
        let target = self.rival_team;
        let rivals: Vec<u64> = world.rival_bots().map(|b| b.handle).collect();
        let sectors = self.want.max(self.kept.len());
        let spread_on = self.spread;
        for (i, k) in self.kept.iter_mut().enumerate() {
            if k.gone {
                continue;
            }
            let Some(b) = world.bot(k.handle) else {
                k.misses += 1;
                if k.seen && k.misses >= GONE_AFTER {
                    k.gone = true;
                    tracing::info!(
                        "bots: bot 0x{:X} left the bot set (dead: {})",
                        k.handle,
                        k.dead
                    );
                }
                continue;
            };
            k.misses = 0;
            k.hp = b.hp;
            k.max_hp = b.max_hp;
            k.pos = b.pos.or(k.pos);
            if !k.seen {
                k.seen = true;
                if b.npc_id != crate::game::layout::BOT_NPC_PARAM {
                    tracing::warn!(
                        "bots: bot 0x{:X} has npc id {}, not the bot tables' {}",
                        k.handle,
                        b.npc_id,
                        crate::game::layout::BOT_NPC_PARAM
                    );
                }
                tracing::info!(
                    "bots: bot 0x{:X} is ChrIns 0x{:X} (npc id {}, team {}, hp {}/{})",
                    k.handle,
                    b.chr,
                    b.npc_id,
                    b.team,
                    b.hp,
                    b.max_hp
                );
            }
            if b.hp == 0 && b.max_hp > 0 && !k.dead {
                k.dead = true;
                let cause = death_cause(b.attacker, world.local_handle, &rivals);
                k.death = Some((cause, now));
                let next = if cause == Death::Mob {
                    format!("; a new bot replaces it in {} s", RESPAWN_MS / 1000)
                } else {
                    "; out of the match".to_string()
                };
                tracing::info!(
                    "bots: bot 0x{:X} is dead (hp 0; last attacker 0x{:X} = {cause:?}){next}",
                    k.handle,
                    b.attacker
                );
            } else if b.hp > 0 && k.dead {
                k.dead = false;
                tracing::info!("bots: bot 0x{:X} is alive again (hp {})", k.handle, b.hp);
            }
            if spread_on
                && !k.spread
                && b.pos.is_some()
                && world.local_pos.is_some_and(|(_, ground)| ground)
            {
                k.spread = true;
                ctx.bot_writes.push(BotWrite::Spread {
                    chr: b.chr,
                    handle: k.handle,
                    sector: i,
                    sectors,
                });
            }
            if b.team != target {
                ctx.bot_writes.push(BotWrite::Team {
                    chr: b.chr,
                    handle: k.handle,
                    team: target,
                });
                if k.applied != target {
                    k.applied = target;
                    tracing::info!("bots: bot 0x{:X}: team {} -> {}", k.handle, b.team, target);
                }
            }
        }
    }

    /// BOT-2: a bot killed by a mob [`RESPAWN_MS`] ago stops counting; a done fill arms one more round for its replacement.
    fn respawn_due(&mut self, now: u64) {
        let mut due = 0;
        for k in &mut self.kept {
            if let Some((Death::Mob, at)) = k.death {
                if !k.replaced && now.saturating_sub(at) >= RESPAWN_MS {
                    k.replaced = true;
                    due += 1;
                }
            }
        }
        if due > 0 && self.stage == Stage::Done && self.rounds < MAX_ROUNDS {
            tracing::info!("bots: {due} bot(s) killed by mobs: arming a replacement round");
            self.stage = Stage::Gap {
                n: self.rounds - 1,
                at: now.saturating_sub(ROUND_GAP_MS),
            };
        }
    }

    fn reset(&mut self) {
        self.over = true;
        self.want = self.cfg_want;
        self.stage = Stage::Idle;
        self.kept.clear();
        self.handled.clear();
        self.extra.clear();
        self.rounds = 0;
    }

    /// Bots kept and still in the world with HP above 0.
    pub(crate) fn alive(&self) -> usize {
        self.kept
            .iter()
            .filter(|k| k.seen && !k.gone && !k.dead && !k.replaced)
            .count()
    }

    /// The roster for the console: a summary line, then one line per kept bot.
    pub(crate) fn roster(&self) -> String {
        let mut out = format!(
            "bots: {:?}, {} of {} kept, {} alive",
            self.stage,
            self.active(),
            self.want,
            self.alive()
        );
        for (i, k) in self.kept.iter().enumerate() {
            let state = match (k.gone, k.seen, k.death) {
                _ if k.replaced => "dead (mob), replaced",
                (true, _, _) => "gone",
                (_, false, _) => "not found yet",
                (_, _, Some((Death::Mob, _))) => "dead (mob), respawning",
                (_, _, Some((Death::Rival, _))) => "dead (rival), out",
                (_, _, Some((Death::Env, _))) => "dead (tide or fall), out",
                _ if k.dead => "dead",
                _ => "alive",
            };
            let dist = match (k.pos, self.me) {
                (Some(p), Some(m)) => format!(
                    ", {:.0} m away",
                    ((p[0] - m[0]).powi(2) + (p[2] - m[2]).powi(2)).sqrt()
                ),
                _ => String::new(),
            };
            let pos = k
                .pos
                .map(|p| format!(" at ({:.0}, {:.0}, {:.0}){dist}", p[0], p[1], p[2]))
                .unwrap_or_default();
            let team = if k.applied == 0 {
                "-".to_string()
            } else {
                k.applied.to_string()
            };
            out.push_str(&format!(
                "\n  b{} 0x{:X}: {state}, hp {}/{}, team {team}{pos}",
                i + 1,
                k.handle,
                k.hp,
                k.max_hp
            ));
        }
        out
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
        let mut d = BotFill::new(5, 29);
        let mut t = 0;
        let mut go = |d: &mut BotFill, w: &World, ctx: &mut Ctx, dt: u64| {
            t += dt;
            d.step(t, GameMode::Offline, w, ctx);
        };
        // The expedition is loaded: round 1 is armed at once (the game waits for the landing itself).
        let mut ctx = Ctx::default();
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
        assert_eq!(ctx.flags, vec![(super::super::FLAG_CLEAR, true)]);
        assert_eq!(d.kept.len(), 5);
        assert_eq!(d.stage, Stage::Done);
        // The same count again changes nothing.
        d.set_want(5, t);
        assert_eq!(d.stage, Stage::Done);
    }

    #[test]
    fn found_bots_are_made_rivals_and_the_roster_follows_them() {
        let mut d = BotFill::new(1, 29);
        d.stage = Stage::Done;
        d.kept.push(Kept::new(0xA0));
        let mut w = world([E, E]);
        w.bot_chrs.push(BotChr {
            handle: 0xA0,
            chr: 0x7000,
            team: 2,
            npc_id: 600_040_000,
            hp: 700,
            max_hp: 720,
            pos: Some([1.0, 2.0, 3.0]),
            attacker: 0,
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
        assert_eq!(d.alive(), 1);
        let r = d.roster();
        assert!(
            r.contains("b1 0xA0: alive, hp 700/720, team 29 at (1, 2, 3)"),
            "{r}"
        );
        // Already a rival: nothing to write. HP 0: dead.
        w.bot_chrs[0].team = 29;
        w.bot_chrs[0].hp = 0;
        let mut ctx = Ctx::default();
        d.step(50, GameMode::Offline, &w, &mut ctx);
        assert!(ctx.bot_writes.is_empty());
        assert_eq!(d.alive(), 0);
        assert!(d.roster().contains("dead"));
        // Missing from the set for 3 s: gone.
        w.bot_chrs.clear();
        for i in 0..u64::from(GONE_AFTER) {
            d.step(100 + i * 50, GameMode::Offline, &w, &mut ctx);
        }
        assert!(d.kept[0].gone);
    }

    #[test]
    fn a_higher_count_after_the_fill_arms_another_round() {
        let mut d = BotFill::new(1, 29);
        let mut ctx = Ctx::default();
        d.step(0, GameMode::Offline, &world([E, E]), &mut ctx);
        d.step(50, GameMode::Offline, &world([0xA0, 0xA1]), &mut ctx);
        d.step(
            50 + ROUND_GAP_MS,
            GameMode::Offline,
            &world([E, 0xA1]),
            &mut ctx,
        );
        assert_eq!(d.stage, Stage::Done);
        d.set_want(3, 2_000);
        let mut ctx = Ctx::default();
        d.step(2_000, GameMode::Offline, &world([E, E]), &mut ctx);
        assert_eq!(ctx.flags, vec![(10_007_303, true)], "round 2 = table 30");
    }

    #[test]
    fn the_cause_of_a_bots_death_comes_from_its_last_attacker() {
        assert_eq!(death_cause(0, Some(0xF0), &[]), Death::Env);
        assert_eq!(death_cause(u64::MAX, None, &[]), Death::Env);
        assert_eq!(death_cause(0xF0, Some(0xF0), &[]), Death::Rival);
        assert_eq!(death_cause(0xA1, Some(0xF0), &[0xA0, 0xA1]), Death::Rival);
        assert_eq!(
            death_cause(0x3200_0000_1100_0004, Some(0xF0), &[0xA0]),
            Death::Mob
        );
    }

    #[test]
    fn a_bot_killed_by_a_mob_is_replaced_after_the_respawn_delay() {
        let mut d = BotFill::new(1, 29);
        let mut ctx = Ctx::default();
        d.step(0, GameMode::Offline, &world([E, E]), &mut ctx);
        d.step(50, GameMode::Offline, &world([0xA0, 0xA1]), &mut ctx);
        d.step(
            50 + ROUND_GAP_MS,
            GameMode::Offline,
            &world([E, 0xA1]),
            &mut ctx,
        );
        assert_eq!(d.stage, Stage::Done);
        let mut w = world([E, E]);
        let bot = |hp, attacker| BotChr {
            handle: 0xA0,
            chr: 0x7000,
            team: 29,
            npc_id: 600_040_000,
            hp,
            max_hp: 240,
            pos: None,
            attacker,
        };
        w.bot_chrs = vec![bot(700, 0)];
        d.step(2_000, GameMode::Offline, &w, &mut ctx);
        // A camp mob kills it.
        w.bot_chrs = vec![bot(0, 0x3200_0000_1100_0004)];
        d.step(3_000, GameMode::Offline, &w, &mut ctx);
        assert!(d.roster().contains("respawning"), "{}", d.roster());
        let mut ctx = Ctx::default();
        d.step(3_000 + RESPAWN_MS - 1, GameMode::Offline, &w, &mut ctx);
        assert!(ctx.flags.is_empty(), "not yet");
        d.step(3_000 + RESPAWN_MS, GameMode::Offline, &w, &mut ctx);
        d.step(3_050 + RESPAWN_MS, GameMode::Offline, &w, &mut ctx);
        assert_eq!(ctx.flags, vec![(10_007_303, true)], "a replacement round");
        assert_eq!(d.active(), 0);
        // Killed by LOCAL instead: out, no replacement.
        let mut d = BotFill::new(1, 29);
        d.stage = Stage::Done;
        d.rounds = 1;
        d.kept.push(Kept::new(0xA0));
        w.local_handle = Some(0xF0);
        w.bot_chrs = vec![bot(0, 0xF0)];
        let mut ctx = Ctx::default();
        d.step(0, GameMode::Offline, &w, &mut ctx);
        d.step(RESPAWN_MS * 2, GameMode::Offline, &w, &mut ctx);
        assert!(ctx.flags.is_empty());
        assert!(d.roster().contains("dead (rival), out"), "{}", d.roster());
    }

    #[test]
    fn nothing_happens_outside_offline_or_when_off() {
        for (want, mode) in [(5, GameMode::BattleRoyale), (0, GameMode::Offline)] {
            let mut d = BotFill::new(want, 29);
            let mut ctx = Ctx::default();
            d.step(0, mode, &world([E, E]), &mut ctx);
            d.step(ROUND_TIMEOUT_MS, mode, &world([E, E]), &mut ctx);
            assert_eq!(ctx, Ctx::default());
            assert_eq!(d.stage, Stage::Idle);
        }
    }

    #[test]
    fn a_bots_match_fills_five_in_any_mode_and_ends_with_the_expedition() {
        let mut d = BotFill::new(0, 29);
        d.set_bots_match(true);
        let mut ctx = Ctx::default();
        d.step(0, GameMode::BattleRoyale, &world([E, E]), &mut ctx);
        assert_eq!(ctx.flags, vec![(10_007_300, true)]);
        assert_eq!(d.want(), MAX_BOTS);
        let mut w = world([E, E]);
        w.place = Some(Place::Roundtable);
        d.step(50, GameMode::BattleRoyale, &w, &mut ctx);
        assert!(d.take_over());
        assert!(!d.take_over(), "once");
        assert_eq!(d.want(), 0, "the config count is back");
    }

    #[test]
    fn the_roundtable_ends_the_expedition() {
        let mut d = BotFill::new(2, 29);
        let mut ctx = Ctx::default();
        d.step(0, GameMode::Offline, &world([E, E]), &mut ctx);
        let mut w = world([E, E]);
        w.place = Some(Place::Roundtable);
        d.step(50, GameMode::Offline, &w, &mut ctx);
        assert_eq!(d.stage, Stage::Idle);
    }
}
