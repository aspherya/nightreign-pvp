//! R3 (second half, OFF-1): feature modules. Each feature implements [`Feature`]; [`Features`] is the one list the main loop ticks,
//! after it has read the tick's [`World`]. A feature decides from the `World` and asks for side effects through [`Ctx`] (commands
//! for the matchmaking worker, writes of our event flags), which the loop carries out after every feature ran; so a feature's decisions are testable
//! without the game or Steam (ARCHITECTURE §4 "Feature modules", §6).
//!
//! New behaviour comes as a new `Feature` here, not as another field on an existing controller. The older controllers (`TeamCtl`,
//! `CombatCtl`, the hawk swap) are still ticked by `game::watch` directly; they move here when a card touches them.

pub(crate) mod bots;
pub(crate) mod clock;
pub(crate) mod mode;
pub(crate) mod night_match;
pub(crate) mod nights;

use crate::config::Config;
use crate::game::world::World;

/// One feature of the mod, ticked once per main-loop pass (50 ms in a world, 500 ms outside one).
pub(crate) trait Feature {
    /// Short name for logs.
    fn name(&self) -> &'static str;
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
    /// The match rules' requests on LOCAL, the clock and the game's flags (carried out by the main loop, which owns them).
    pub night: Vec<night_match::NightAct>,
}

/// Where the main loop's bot writes go.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Targets {
    /// Exe base (event flags).
    pub base: usize,
    /// `WorldPlayAreaManager` instance, 0 = none.
    pub play_area: usize,
}

impl Ctx {
    /// Carry out the requests (main loop, after every feature ticked).
    pub(crate) fn flush(&mut self, t: Targets, mem: &dyn crate::game::gamemem::GameMem) {
        let base = t.base;
        for c in self.mm.drain(..) {
            if !crate::mm::send(c) {
                tracing::info!("features: {c:?} not sent to matchmaking (queue off or full)");
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
pub(crate) struct Features {
    pub mode: mode::ModeFeature,
    pub bots: bots::BotsFeature,
    pub clock: clock::ClockFeature,
    pub nights: nights::NightsFeature,
    pub night_match: night_match::NightMatch,
}

impl Features {
    pub(crate) fn new(cfg: &Config) -> Self {
        Features {
            mode: mode::ModeFeature::new(cfg.game_mode(), "nrpvp.toml"),
            bots: bots::BotsFeature::default(),
            clock: clock::ClockFeature::default(),
            nights: nights::NightsFeature,
            night_match: night_match::NightMatch::new(cfg),
        }
    }

    fn all(&mut self) -> [&mut dyn Feature; 5] {
        [
            &mut self.mode,
            &mut self.bots,
            &mut self.clock,
            &mut self.nights,
            &mut self.night_match,
        ]
    }

    pub(crate) fn tick(&mut self, world: &World, ctx: &mut Ctx) {
        for f in self.all() {
            f.tick(world, ctx);
        }
    }

    /// Names of the features, for the startup log.
    pub(crate) fn names(&mut self) -> Vec<&'static str> {
        self.all().iter().map(|f| f.name()).collect()
    }
}
