//! OFF-1: the game mode. `BattleRoyale` (default): Commence in the stock expedition menu starts our matchmaking queue (`menuflow`).
//! `Offline`: Commence with Solo runs the game's own solo expedition at once (no queue, no second player), for solo play and solo
//! testing; everything else (game-data package, PvP rules, hawk crossing) is the same in both modes.
//!
//! Chosen by `mode = "br" | "offline"` in `nrpvp.toml` and, in the developer build, the console command `mode [br|offline]`. A
//! change asked for during an expedition waits until the player is back in the Roundtable (or no world is loaded), so a running
//! match is never left. Switching to Offline leaves the queue (searching or a ready match). Every change logs `mode: <mode>`.
//!
//! The current mode is published in an atomic: the Commence exception handler (`menuflow`) reads it without a lock.

use std::sync::atomic::{AtomicU8, Ordering};

use super::{Ctx, Feature};
use crate::game::world::World;
use crate::mm::PHASE_IDLE;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GameMode {
    #[default]
    BattleRoyale,
    Offline,
}

impl GameMode {
    /// The word used in `nrpvp.toml`, the console and the log.
    pub(crate) fn label(self) -> &'static str {
        match self {
            GameMode::BattleRoyale => "br",
            GameMode::Offline => "offline",
        }
    }

    /// `br` / `offline` (case and surrounding spaces ignored).
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "br" => Some(GameMode::BattleRoyale),
            "offline" => Some(GameMode::Offline),
            _ => None,
        }
    }

    fn code(self) -> u8 {
        match self {
            GameMode::BattleRoyale => 0,
            GameMode::Offline => 1,
        }
    }

    fn from_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(GameMode::BattleRoyale),
            1 => Some(GameMode::Offline),
            _ => None,
        }
    }
}

const NO_PENDING: u8 = 0xFF;
static CURRENT: AtomicU8 = AtomicU8::new(0);
static PENDING: AtomicU8 = AtomicU8::new(NO_PENDING);

/// The mode in force (lock-free; also read by the Commence exception handler).
pub(crate) fn current() -> GameMode {
    GameMode::from_code(CURRENT.load(Ordering::Relaxed)).unwrap_or_default()
}

/// The status line (`status` and `mode` in the console).
pub(crate) fn status_line() -> String {
    line(
        current(),
        GameMode::from_code(PENDING.load(Ordering::Relaxed)),
    )
}

fn line(current: GameMode, pending: Option<GameMode>) -> String {
    match pending {
        Some(p) if p != current => format!(
            "mode: {} ({} once back in the Roundtable)",
            current.label(),
            p.label()
        ),
        _ => format!("mode: {}", current.label()),
    }
}

pub(crate) struct ModeFeature {
    current: GameMode,
    /// A change waiting for the end of the expedition, and who asked for it.
    pending: Option<(GameMode, String)>,
}

impl ModeFeature {
    pub(crate) fn new(start: GameMode, src: &str) -> Self {
        let f = ModeFeature {
            current: start,
            pending: None,
        };
        f.publish();
        tracing::info!("mode: {} (from {src})", start.label());
        f
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn current(&self) -> GameMode {
        self.current
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn pending(&self) -> Option<GameMode> {
        self.pending.as_ref().map(|(m, _)| *m)
    }

    /// Switch to `want` now, or once the expedition is over.
    pub(crate) fn request(&mut self, want: GameMode, src: &str, world: &World, ctx: &mut Ctx) {
        if want == self.current {
            if self.pending.take().is_some() {
                tracing::info!(
                    "mode: {} (the waiting change was cancelled by {src})",
                    want.label()
                );
            } else {
                tracing::info!("mode: {} (unchanged, asked by {src})", want.label());
            }
            self.publish();
            return;
        }
        if world.in_expedition() {
            tracing::info!(
                "mode: {} stays until the expedition ends; then {} (asked by {src})",
                self.current.label(),
                want.label()
            );
            self.pending = Some((want, src.to_string()));
            self.publish();
            return;
        }
        self.apply(want, src, world, ctx);
    }

    fn apply(&mut self, want: GameMode, src: &str, world: &World, ctx: &mut Ctx) {
        self.current = want;
        self.pending = None;
        self.publish();
        tracing::info!("mode: {} (from {src})", want.label());
        if want == GameMode::Offline && world.mm_phase != PHASE_IDLE {
            tracing::info!("mode: offline leaves the Battle Royale queue");
            ctx.mm.push(crate::mm::Cmd::Leave);
        }
    }

    fn publish(&self) {
        CURRENT.store(self.current.code(), Ordering::Relaxed);
        PENDING.store(
            self.pending.as_ref().map_or(NO_PENDING, |(m, _)| m.code()),
            Ordering::Relaxed,
        );
    }
}

impl Feature for ModeFeature {
    fn name(&self) -> &'static str {
        "mode"
    }

    fn tick(&mut self, world: &World, ctx: &mut Ctx) {
        if world.in_expedition() {
            return;
        }
        if let Some((want, src)) = self.pending.take() {
            self.apply(want, &format!("{src}, after the expedition"), world, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::views::Place;
    use crate::mm::{Cmd, PHASE_MATCHED, PHASE_SEARCHING};

    fn at(place: Option<Place>, phase: u8) -> World {
        World {
            wcm_instance: 0x5_0000,
            local: Some(0x10_0000),
            place,
            mm_phase: phase,
            ..World::default()
        }
    }

    #[test]
    fn parse_and_label_round_trip() {
        for m in [GameMode::BattleRoyale, GameMode::Offline] {
            assert_eq!(GameMode::parse(m.label()), Some(m));
            assert_eq!(GameMode::from_code(m.code()), Some(m));
        }
        assert_eq!(GameMode::parse(" Offline "), Some(GameMode::Offline));
        assert_eq!(GameMode::parse("BR"), Some(GameMode::BattleRoyale));
        assert_eq!(GameMode::parse("solo"), None);
        assert_eq!(GameMode::default(), GameMode::BattleRoyale);
    }

    #[test]
    fn switch_in_the_roundtable_applies_at_once() {
        let mut f = ModeFeature::new(GameMode::BattleRoyale, "test");
        let mut ctx = Ctx::default();
        let w = at(Some(Place::Roundtable), PHASE_IDLE);
        f.request(GameMode::Offline, "console", &w, &mut ctx);
        assert_eq!(f.current(), GameMode::Offline);
        assert_eq!(f.pending(), None);
        assert!(ctx.mm.is_empty(), "nothing to leave");
        f.request(GameMode::BattleRoyale, "console", &w, &mut ctx);
        assert_eq!(f.current(), GameMode::BattleRoyale);
    }

    #[test]
    fn switch_with_no_world_applies_at_once() {
        let mut f = ModeFeature::new(GameMode::BattleRoyale, "test");
        let mut ctx = Ctx::default();
        f.request(GameMode::Offline, "console", &World::default(), &mut ctx);
        assert_eq!(f.current(), GameMode::Offline);
    }

    #[test]
    fn offline_leaves_the_queue_when_searching_or_matched() {
        for phase in [PHASE_SEARCHING, PHASE_MATCHED] {
            let mut f = ModeFeature::new(GameMode::BattleRoyale, "test");
            let mut ctx = Ctx::default();
            f.request(
                GameMode::Offline,
                "console",
                &at(Some(Place::Roundtable), phase),
                &mut ctx,
            );
            assert_eq!(ctx.mm, vec![Cmd::Leave]);
        }
        let mut f = ModeFeature::new(GameMode::Offline, "test");
        let mut ctx = Ctx::default();
        f.request(
            GameMode::BattleRoyale,
            "console",
            &at(Some(Place::Roundtable), PHASE_SEARCHING),
            &mut ctx,
        );
        assert!(ctx.mm.is_empty(), "going back to br never leaves");
    }

    #[test]
    fn switch_during_an_expedition_waits_for_the_roundtable() {
        let mut f = ModeFeature::new(GameMode::BattleRoyale, "test");
        let mut ctx = Ctx::default();
        let exp = at(Some(Place::Expedition), PHASE_MATCHED);
        f.request(GameMode::Offline, "console", &exp, &mut ctx);
        assert_eq!(f.current(), GameMode::BattleRoyale);
        assert_eq!(f.pending(), Some(GameMode::Offline));
        f.tick(&exp, &mut ctx);
        assert_eq!(
            f.current(),
            GameMode::BattleRoyale,
            "still on the expedition"
        );
        assert!(ctx.mm.is_empty(), "a running match is never left");
        // Load screen (no world), then the Roundtable.
        f.tick(&World::default(), &mut ctx);
        assert_eq!(f.current(), GameMode::Offline);
        assert_eq!(f.pending(), None);
    }

    #[test]
    fn asking_for_the_current_mode_cancels_a_waiting_change() {
        let mut f = ModeFeature::new(GameMode::BattleRoyale, "test");
        let mut ctx = Ctx::default();
        let exp = at(Some(Place::Expedition), PHASE_IDLE);
        f.request(GameMode::Offline, "console", &exp, &mut ctx);
        f.request(GameMode::BattleRoyale, "console", &exp, &mut ctx);
        assert_eq!(f.pending(), None);
        f.tick(&World::default(), &mut ctx);
        assert_eq!(f.current(), GameMode::BattleRoyale);
    }

    #[test]
    fn status_line_names_a_waiting_change() {
        assert_eq!(line(GameMode::BattleRoyale, None), "mode: br");
        assert_eq!(
            line(GameMode::BattleRoyale, Some(GameMode::BattleRoyale)),
            "mode: br"
        );
        assert_eq!(
            line(GameMode::BattleRoyale, Some(GameMode::Offline)),
            "mode: br (offline once back in the Roundtable)"
        );
    }
}
