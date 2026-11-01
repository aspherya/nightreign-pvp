//! OFF-4 (bot spike): start and stop the game's own NPC bots (AI Nightfarers) and log what the bot manager does. Kept as the engine
//! of the orchestrator's puppet players (`game/puppet.rs`, `tools/`); it is not a game feature (decided 2026-10-24). Research and the
//! engine facts: `archive/research/OFF-3-bots.md`, TECHNICAL_NOTES §4d, MEMORY_MAP §15.
//!
//! No game code is called from here. Our events in `event/common.emevd.dcx` (`gamedata/emevd-bots.json`, built by
//! `scripts/build-gamedata.ps1`) wait for our flags and run the game's instructions: `2003[97](table)` arms `CSNpcBotMan` with an
//! `NPCBotTableParam` table (the manager then spawns its Nightfarers), `2003[98]` removes them. This feature only sets the flag
//! (through [`Ctx::flags`]) and, every tick, compares the manager's state in the [`World`] with the last one and logs a change.
//!
//! Offline mode only: the game itself refuses `2003[97]` when its solo gate counts more than one, and bots in a matchmade match are
//! out of scope. Requests come from the dev console (`bots`, `bots_clear`) and the orchestrator's puppets.
//! Log lines start with `bots:`.

use std::sync::{Mutex, PoisonError};

use super::mode::GameMode;
use super::{Ctx, Feature};
use crate::game::views::{BotManState, BOT_HEADS};
use crate::game::world::World;

/// A request for the bot manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BotsCmd {
    /// Arm this `NPCBotTableParam` table (`2003[97]`).
    Spawn(i32),
    /// Remove every bot (`2003[98]`).
    Clear,
}

/// Tables our events know, with their flags (`gamedata/emevd-bots.json`; a unit test checks both files). 10 / 30 / 50: 2
/// Nightfarers at level 1 (heroes 1 + 2, 3 + 4, 5 + 6); 10000: 2 of the 8 base Nightfarers at level 6 (the manager holds 2).
pub(crate) const TABLES: [(i32, u32); 4] = [
    (10, 10_007_300),
    (30, 10_007_303),
    (50, 10_007_304),
    (10_000, 10_007_301),
];
/// Flag of the `2003[98]` event.
pub(crate) const FLAG_CLEAR: u32 = 10_007_302;

/// The flag that carries out `cmd`, or why it is refused (pure; unit tested).
pub(crate) fn plan(cmd: BotsCmd, mode: GameMode, world: &World) -> Result<u32, String> {
    if mode != GameMode::Offline {
        return Err("bots need the offline mode (mode offline)".into());
    }
    if world.wcm_instance == 0 {
        return Err("no world loaded".into());
    }
    match cmd {
        BotsCmd::Spawn(t) => TABLES
            .iter()
            .find(|(id, _)| *id == t)
            .map(|(_, f)| *f)
            .ok_or_else(|| {
                format!(
                    "no event for table {t}; tables: {}",
                    TABLES.map(|(id, _)| id.to_string()).join(", ")
                )
            }),
        BotsCmd::Clear => Ok(FLAG_CLEAR),
    }
}

/// The manager's state in words (pure).
fn describe(s: Option<&BotManState>) -> String {
    match s {
        None => "no bot manager".into(),
        Some(s) => {
            let mut l = format!(
                "table {}, {}, {} bot(s)",
                s.table,
                if s.armed { "armed" } else { "idle" },
                s.bots
            );
            let shown = s.bots.min(BOT_HEADS);
            if shown > 0 {
                let heads: Vec<String> = s.heads[..shown]
                    .iter()
                    .map(|h| format!("0x{h:X}"))
                    .collect();
                l.push_str(&format!(" [{}]", heads.join(", ")));
            }
            l
        }
    }
}

/// Last request result and the manager's state, for the console (written by the main loop, read by the pipe thread).
static STATUS: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));

/// `bots: <last request>; manager: <state>` (console).
pub(crate) fn status_line() -> String {
    let s = STATUS.lock().unwrap_or_else(PoisonError::into_inner);
    format!(
        "bots: {}; manager: {}",
        if s.0.is_empty() {
            "no request yet"
        } else {
            &s.0
        },
        if s.1.is_empty() { "not read yet" } else { &s.1 }
    )
}

fn set_status(request: Option<String>, manager: Option<String>) {
    let mut s = STATUS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(r) = request {
        s.0 = r;
    }
    if let Some(m) = manager {
        s.1 = m;
    }
}

#[derive(Default)]
pub(crate) struct BotsFeature {
    /// The manager's state last logged (`None` before the first read).
    last: Option<Option<BotManState>>,
}

impl BotsFeature {
    /// Ask for `cmd`: sets our flag when allowed (the flag is written after every feature ticked).
    pub(crate) fn request(
        &mut self,
        cmd: BotsCmd,
        src: &str,
        mode: GameMode,
        world: &World,
        ctx: &mut Ctx,
    ) {
        let line = match plan(cmd, mode, world) {
            Ok(flag) => {
                ctx.flags.push((flag, true));
                format!("{cmd:?} asked by {src} (flag {flag})")
            }
            Err(e) => format!("{cmd:?} refused: {e}"),
        };
        tracing::info!("bots: {line}");
        set_status(Some(line), None);
    }
}

impl Feature for BotsFeature {
    fn name(&self) -> &'static str {
        "bots"
    }

    fn tick(&mut self, world: &World, _ctx: &mut Ctx) {
        if self.last == Some(world.bots) {
            return;
        }
        let line = describe(world.bots.as_ref());
        // The first read and the manager's own changes are evidence (OFF-4); a vanished manager is an area load.
        tracing::info!("bots: CSNpcBotMan {line}");
        set_status(None, Some(line));
        self.last = Some(world.bots);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live() -> World {
        World {
            wcm_instance: 0x5_0000,
            ..World::default()
        }
    }

    #[test]
    fn requests_need_offline_mode_and_a_world() {
        assert!(plan(BotsCmd::Spawn(10), GameMode::BattleRoyale, &live())
            .unwrap_err()
            .contains("offline"));
        assert!(plan(BotsCmd::Clear, GameMode::Offline, &World::default())
            .unwrap_err()
            .contains("no world"));
    }

    #[test]
    fn known_tables_map_to_their_flags() {
        assert_eq!(
            plan(BotsCmd::Spawn(10), GameMode::Offline, &live()),
            Ok(10_007_300)
        );
        assert_eq!(
            plan(BotsCmd::Spawn(10_000), GameMode::Offline, &live()),
            Ok(10_007_301)
        );
        assert_eq!(
            plan(BotsCmd::Clear, GameMode::Offline, &live()),
            Ok(FLAG_CLEAR)
        );
        assert!(plan(BotsCmd::Spawn(20), GameMode::Offline, &live())
            .unwrap_err()
            .contains("tables: 10, 30, 50, 10000"));
    }

    #[test]
    fn a_request_queues_the_flag_only_when_allowed() {
        let mut f = BotsFeature::default();
        let mut ctx = Ctx::default();
        f.request(
            BotsCmd::Spawn(10),
            "test",
            GameMode::Offline,
            &live(),
            &mut ctx,
        );
        assert_eq!(ctx.flags, vec![(10_007_300, true)]);
        let mut ctx = Ctx::default();
        f.request(
            BotsCmd::Clear,
            "test",
            GameMode::BattleRoyale,
            &live(),
            &mut ctx,
        );
        assert!(ctx.flags.is_empty());
    }

    #[test]
    fn manager_state_reads_as_one_line() {
        assert_eq!(describe(None), "no bot manager");
        let s = BotManState {
            table: 10,
            armed: true,
            bots: 2,
            heads: [0xA, 0xB, 0, 0, 0, 0, 0, 0],
        };
        assert_eq!(describe(Some(&s)), "table 10, armed, 2 bot(s) [0xA, 0xB]");
    }

    /// Every table and the clear flag have their event in gamedata/emevd-bots.json (plain-text check: one event per line).
    #[test]
    fn gamedata_has_every_bot_event() {
        let events = include_str!("../../../../gamedata/emevd-bots.json");
        for (table, flag) in TABLES {
            let want = format!("\"flag\": {flag}, \"bots\": {table} }}");
            assert!(events.contains(&want), "missing: {want}");
        }
        let want = format!("\"flag\": {FLAG_CLEAR}, \"bots_clear\": true }}");
        assert!(events.contains(&want), "missing: {want}");
        assert!(events.contains("\"file\": \"event/common.emevd.dcx\""));
    }
}
