//! R3 (second half, OFF-1): feature modules. Each feature implements [`Feature`]; [`Features`] is the one list the main loop ticks,
//! after it has read the tick's [`World`]. A feature decides from the `World` and asks for side effects through [`Ctx`] (commands
//! for the matchmaking worker, writes of our event flags), which the loop carries out after every feature ran; so a feature's decisions are testable
//! without the game or Steam (ARCHITECTURE §4 "Feature modules", §6).
//!
//! New behaviour comes as a new `Feature` here, not as another field on an existing controller. The older controllers (`TeamCtl`,
//! `CombatCtl`) are still ticked by `game::watch` directly; they move here when a card touches them.

pub(crate) mod bots;
pub(crate) mod br_match;
pub(crate) mod clock;
pub(crate) mod effects;
pub(crate) mod hooks;
pub(crate) mod killfeed;
pub(crate) mod mode;
pub(crate) mod nights;

#[cfg(test)]
mod golden_timelines;
#[cfg(test)]
mod sim_tests;
#[cfg(test)]
mod testkit;

use crate::game::world::World;

pub(crate) use effects::Act;

/// R7-5: where in the main loop's pass a feature is ticked. The order is behaviour: the world is read before `combat.tick`, so the
/// `Rules` features see the fighters of the previous tick; `AfterCombat` is for the features that need the current tick's rivals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// After the world is read, before the team and combat controllers tick (today's only place).
    Rules,
    /// After `combat.tick` and [`World::after_combat`]: the hook modules (`hooks`), which need the current tick's rivals.
    AfterCombat,
}

/// One feature of the mod, ticked once per main-loop pass (50 ms in a world, 500 ms outside one). `Any` lets the console find a
/// feature by type ([`Features::get_mut`]).
pub(crate) trait Feature: std::any::Any {
    /// Short name for logs.
    fn name(&self) -> &'static str;
    /// Where the main loop ticks this feature.
    fn phase(&self) -> Phase {
        Phase::Rules
    }
    /// Decide from this tick's world; ask for side effects through `ctx`. Never blocks.
    fn tick(&mut self, world: &World, ctx: &mut Ctx);
}

/// Side effects the features asked for this tick; the main loop carries them out ([`Ctx::flush`]).
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Ctx {
    /// Commands for the matchmaking worker (sent with `crate::mm::send`, which never calls Steam on this thread).
    pub mm: Vec<crate::mm::Cmd>,
    /// OFF-4: our event flags to set or clear (group 10007 only; `crate::game::flags::set` refuses others).
    pub flags: Vec<(u32, bool)>,
    /// M5: a time for the play-area clock (the game's set-time mailbox, written in [`Ctx::flush`]).
    pub clock: Option<crate::game::views::ClockTime>,
    /// The match rules' requests on LOCAL, the clock and the game's flags (carried out first in [`Ctx::flush`], see `effects`).
    pub acts: Vec<Act>,
    /// BOT-1: writes on the bot manager and on bot characters (each re-checked against the live memory in [`Ctx::flush`]).
    pub bot_writes: Vec<BotWrite>,
    /// BOT-KF: a bot's death for this game's killfeed (bots live on this game only, so nothing goes on the bus).
    pub kills: Vec<killfeed::KillEvent>,
}

/// A write the bot feature asks for (BOT-1; the OFF-5 writes, git `72f7689`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BotWrite {
    /// Make the bot manager forget the bot in this slot (`BotMan::forget_slot`): the bot stays, the slot is free for the next table.
    ForgetSlot(usize),
    /// Write `team` on this bot character, only if its handle field still reads `handle`.
    Team { chr: usize, handle: u64, team: u8 },
    /// BOT-2 hunt: bring this far bot to a living enemy about 100 m from LOCAL (`game::botspread::hunt`).
    Hunt { chr: usize, handle: u64 },
    /// Move this bot once to an enemy camp far from LOCAL, in direction `sector` of `sectors` (`game::botspread`).
    Spread {
        chr: usize,
        handle: u64,
        sector: usize,
        sectors: usize,
    },
}

/// Where the main loop's bot writes go.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Targets {
    /// Exe base (event flags).
    pub base: usize,
    /// `WorldPlayAreaManager` instance, 0 = none.
    pub play_area: usize,
    /// R7-4: the local PlayerIns (the target of the match rules' [`Act`]s), `None` = no local player.
    pub local: Option<usize>,
    /// BOT-1: `CSNpcBotMan` instance, 0 = none.
    pub bot_man: usize,
    /// BOT-1: WorldChrMan instance, 0 = none (the spread walks its character sets).
    pub wcm: usize,
}

impl Ctx {
    /// Carry out the requests (main loop, after every feature ticked): the acts, then the matchmaking commands, our flags and the
    /// clock time.
    pub(crate) fn flush(&mut self, t: Targets, mem: &dyn crate::game::gamemem::GameMem) {
        let base = t.base;
        effects::run_acts(std::mem::take(&mut self.acts), t, mem);
        for c in self.mm.drain(..) {
            if !crate::mm::send(c) {
                tracing::info!("features: {c:?} not sent to matchmaking (queue off or full)");
            }
        }
        for ev in self.kills.drain(..) {
            killfeed::local_event(ev);
        }
        for w in self.bot_writes.drain(..) {
            let r = match w {
                BotWrite::Spread {
                    chr,
                    handle,
                    sector,
                    sectors,
                } => match t.local.filter(|_| t.wcm != 0) {
                    Some(local) => crate::game::botspread::spread(
                        t.wcm, local, base, chr, handle, sector, sectors,
                    )
                    .map(|l| tracing::info!("bots: {l}")),
                    None => Err("no world or no local player"),
                },
                BotWrite::Hunt { chr, handle } => match t.local.filter(|_| t.wcm != 0) {
                    Some(local) => crate::game::botspread::hunt(t.wcm, local, base, chr, handle)
                        .map(|l| tracing::info!("bots: {l}")),
                    None => Err("no world or no local player"),
                },
                BotWrite::ForgetSlot(slot) if t.bot_man != 0 => {
                    crate::game::views::BotMan::new(mem, t.bot_man).forget_slot(slot)
                }
                BotWrite::ForgetSlot(_) => Err("no bot manager"),
                BotWrite::Team { chr, handle, team } => {
                    let c = crate::game::views::ChrIns::new(mem, chr);
                    if c.handle() == Some(handle) {
                        c.set_team(team)
                    } else {
                        Err("the character is gone (handle changed)")
                    }
                }
            };
            if let Err(e) = r {
                tracing::warn!("features: {w:?} failed: {e}");
            }
        }
        for (flag, on) in self.flags.drain(..) {
            match crate::game::flags::set(base, flag, on) {
                Ok(prev) => tracing::info!(
                    "features: flag {flag} {} (was {})",
                    on_off(on),
                    on_off(prev)
                ),
                Err(e) => tracing::warn!("features: flag {flag} {} failed: {e}", on_off(on)),
            }
        }
        if let Some(time) = self.clock.take() {
            let r = if t.play_area == 0 {
                Err("no play-area manager")
            } else {
                crate::game::views::PlayArea::new(mem, t.play_area).request_time(time)
            };
            match r {
                Ok(()) => tracing::info!("features: clock {time} handed to the game"),
                Err(e) => tracing::warn!("features: clock {time} failed: {e}"),
            }
        }
    }
}

fn on_off(on: bool) -> &'static str {
    if on {
        "ON"
    } else {
        "OFF"
    }
}

/// Every feature, in tick order. Built once at startup from the config.
pub(crate) struct Features(Vec<Box<dyn Feature>>);

impl Features {
    /// Wrap an assembled list (`crate::modes::assemble` builds it, in tick order).
    pub(crate) fn from_list(list: Vec<Box<dyn Feature>>) -> Self {
        Features(list)
    }

    /// Tick the features of one phase, in list order.
    pub(crate) fn tick(&mut self, phase: Phase, world: &World, ctx: &mut Ctx) {
        for f in self.0.iter_mut().filter(|f| f.phase() == phase) {
            f.tick(world, ctx);
        }
    }

    /// Names of the features, for the startup log.
    pub(crate) fn names(&self) -> Vec<&'static str> {
        self.0.iter().map(|f| f.name()).collect()
    }

    /// The feature of type `T`, for the console's commands. A miss is logged (it means the list was assembled without it) and
    /// the command does nothing.
    pub(crate) fn get_mut<T: Feature>(&mut self) -> Option<&mut T> {
        let found = self
            .0
            .iter_mut()
            .find_map(|f| (f.as_mut() as &mut dyn std::any::Any).downcast_mut::<T>());
        if found.is_none() {
            tracing::warn!(
                "features: no {} in the list; the request is dropped",
                std::any::type_name::<T>()
            );
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn the_list_has_the_same_features_in_the_same_order() {
        let mut f = crate::modes::assemble(&Config::default());
        assert_eq!(
            f.names(),
            [
                "mode",
                "bots",
                "clock",
                "nights",
                "night_match",
                "multiplay",
                "deathhold",
                "menugate",
                "hudfilter",
                "runeshare",
                "defeatdrop",
                "orbclaim",
                "maphide",
                "toastfilter",
                "hawk",
                "killfeed"
            ]
        );
        let phases: Vec<Phase> = f.0.iter().map(|f| f.phase()).collect();
        assert!(phases[..6].iter().all(|p| *p == Phase::Rules));
        assert!(phases[6..].iter().all(|p| *p == Phase::AfterCombat));
        assert!(f.get_mut::<mode::ModeFeature>().is_some());
        assert!(f.get_mut::<bots::BotsFeature>().is_some());
        assert!(f.get_mut::<clock::ClockFeature>().is_some());
        assert!(f.get_mut::<nights::NightsFeature>().is_some());
        assert!(f.get_mut::<br_match::MatchFeature>().is_some());
        // A list without the feature: a miss, not a panic.
        let mut empty = Features(Vec::new());
        assert!(empty.get_mut::<clock::ClockFeature>().is_none());
    }
}
