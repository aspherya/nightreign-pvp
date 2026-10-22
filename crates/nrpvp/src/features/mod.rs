//! R3 (second half, OFF-1): feature modules. Each feature implements [`Feature`]; [`Features`] is the one list the main loop ticks,
//! after it has read the tick's [`World`]. A feature decides from the `World` and asks for side effects through [`Ctx`] (today:
//! commands for the matchmaking worker), which the loop carries out after every feature ran; so a feature's decisions are testable
//! without the game or Steam (ARCHITECTURE §4 "Feature modules", §6).
//!
//! New behaviour comes as a new `Feature` here, not as another field on an existing controller. The older controllers (`TeamCtl`,
//! `CombatCtl`, the hawk swap) are still ticked by `game::watch` directly; they move here when a card touches them.

pub(crate) mod mode;

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
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Ctx {
    /// Commands for the matchmaking worker (sent with `crate::mm::send`, which never calls Steam on this thread).
    pub mm: Vec<crate::mm::Cmd>,
}

impl Ctx {
    /// Carry out the requests (main loop, after every feature ticked).
    pub(crate) fn flush(&mut self) {
        for c in self.mm.drain(..) {
            if !crate::mm::send(c) {
                tracing::info!("features: {c:?} not sent to matchmaking (queue off or full)");
            }
        }
    }
}

/// Every feature, in tick order. Built once at startup from the config.
pub(crate) struct Features {
    pub mode: mode::ModeFeature,
}

impl Features {
    pub(crate) fn new(cfg: &Config) -> Self {
        Features {
            mode: mode::ModeFeature::new(cfg.game_mode(), "nrpvp.toml"),
        }
    }

    fn all(&mut self) -> [&mut dyn Feature; 1] {
        [&mut self.mode]
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
