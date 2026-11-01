//! M5-3: nights with no boss and no minions (`archive/research/M5-no-boss-nights.md`). The work is done by the game-data package:
//! `gamedata/emevd-nights-func.json` gives every common event that brings a night boss or its minions an empty body, and
//! `gamedata/emevd-nights.json` adds two events to `common.emevd` that end night 1 / night 2 by setting the game's own "night done"
//! flags (7502 / 7507), so the game's morning follows as after a boss.
//!
//! Those events wait for our flag [`FLAG_NIGHT_END`]: the night match sets it in the host's game when one team is left standing
//! (M5-5, `night_match.rs`), the console `night_end` here; a 10 min timer in the event is the fallback so a night never hangs. The
//! events run only in the host's game. Log lines start with `nights:`.

use std::sync::{Mutex, PoisonError};

use super::{Ctx, Feature};
use crate::game::world::World;

/// Our flag the night-end events wait for besides their fallback timer (group 10007; `gamedata/emevd-nights.json`).
pub(crate) const FLAG_NIGHT_END: u32 = 10_007_400;

/// Whether a night can be ended now (pure; unit tested).
pub(crate) fn plan(world: &World) -> Result<u32, String> {
    if !world.in_expedition() {
        return Err("not in an expedition".into());
    }
    Ok(FLAG_NIGHT_END)
}

static STATUS: Mutex<String> = Mutex::new(String::new());

/// `nights: <last request>` (console).
pub(crate) fn status_line() -> String {
    let s = STATUS.lock().unwrap_or_else(PoisonError::into_inner);
    format!(
        "nights: {}",
        if s.is_empty() { "no request yet" } else { &s }
    )
}

#[derive(Default)]
pub(crate) struct NightsFeature;

impl NightsFeature {
    /// End the current night now (the event does it only during a night, in the host's game).
    pub(crate) fn request_end(&mut self, src: &str, world: &World, ctx: &mut Ctx) {
        let line = match plan(world) {
            Ok(flag) => {
                ctx.flags.push((flag, true));
                format!("night end asked by {src} (flag {flag}; acts only at night, in the host's game)")
            }
            Err(e) => format!("night end refused: {e}"),
        };
        tracing::info!("nights: {line}");
        *STATUS.lock().unwrap_or_else(PoisonError::into_inner) = line;
    }
}

impl Feature for NightsFeature {
    fn name(&self) -> &'static str {
        "nights"
    }

    /// Nothing to decide per tick yet: the night runs in the event scripts.
    fn tick(&mut self, _world: &World, _ctx: &mut Ctx) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::views::Place;

    #[test]
    fn night_end_needs_an_expedition_and_sets_our_flag() {
        let mut f = NightsFeature;
        let mut ctx = Ctx::default();
        f.request_end("test", &World::default(), &mut ctx);
        assert!(ctx.flags.is_empty());
        let w = World {
            wcm_instance: 0x5_0000,
            local: Some(0x10_0000),
            place: Some(Place::Expedition),
            ..World::default()
        };
        f.request_end("test", &w, &mut ctx);
        assert_eq!(ctx.flags, vec![(FLAG_NIGHT_END, true)]);
    }

    /// Both night-end events wait for our flag, and the boss/minion events are replaced (plain-text check: one entry per line).
    #[test]
    fn gamedata_has_the_night_events() {
        let nights = include_str!("../../../../gamedata/emevd-nights.json");
        for n in [1, 2] {
            let want = format!("\"night_end\": {n}, \"secs\": 600, \"flag\": {FLAG_NIGHT_END} }}");
            assert!(nights.contains(&want), "missing: {want}");
        }
        assert!(nights.contains("\"file\": \"event/common.emevd.dcx\""));
        let func = include_str!("../../../../gamedata/emevd-nights-func.json");
        assert!(func.contains("\"file\": \"event/common_func.emevd.dcx\""));
        for id in [
            90_055_000, 90_055_001, 90_065_251, 90_065_252, 90_065_253, 90_035_000, 90_015_442,
        ] {
            assert!(
                func.contains(&format!("\"id\": {id},")),
                "not replaced: {id}"
            );
        }
    }
}
