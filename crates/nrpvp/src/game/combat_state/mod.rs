//! T-033/T-034/T-035/T-039: combat-state tracker, the rival revive rule and the downed rule.
//!
//! Facts used (all "VERIFIED on NR 1.3.3.0 by FS-0 runs 7-9, TECHNICAL_NOTES section 4", identical on every client unless noted):
//! - `PlayerIns+0x710` u8 == 1 while downed, 0 otherwise;
//! - `PlayerIns+0x730` = number of downs;
//! - `PlayerIns+0x1A0` u64 = handle of the last attacker, valid on the victim's OWN client only (0xFFFF_FFFF_FFFF_FFFF or some
//!   other value on remote copies);
//! - a character's handle is the u64 at `ChrIns+0x08` (seen 0xFFFFFFFF10C00000 host, 0xFFFFFFFF10C00001 joiner);
//! - `PlayerIns+0x5A0` -> object whose RTTI class is `CS::PlayerGameData` (vtable exe+0x2BE6E28); HP = u32 at `+0x10` (240 full,
//!   0 downed, 120 just revived). FS-0 run 8: that field is a MIRROR (writing 0 had no effect). The same pattern is at
//!   `PlayerIns+0xBA0` (u32, mirror) and `[PlayerIns+0xB78]+0x70` (u32): FS-0 run 9 found THAT one authoritative. Writing 0 to it
//!   on the owner's client right after a revive downs the player again at once. Leaving the downed state with hp 0 = bled out,
//!   with hp > 0 = revived. The bleed-out lasts about 15-17 s and restarts on every down.
//!
//! 1. [`Tracker`] (both modes): every 100 ms, per ChrSet player: downed flag, down count, HP, own handle; for LOCAL also the last
//!    attacker, classified against the other players' handles and the rosters (ALLY / RIVAL / SELF / OTHER). Only transitions are
//!    produced. Dev mode logs all of them; the player build logs nothing but the rule line below.
//! 2. **Rule v1** (both modes, always on unless dev mode sets `rival_revive = "vanilla"`): on LOCAL's UP transition (downed 1 -> 0
//!    with HP > 0, not a respawn) whose last attacker is a RIVAL, write u32 0 to `[PlayerIns+0xB78]+0x70` once, so LOCAL is downed
//!    again instead of revived ([`Core::apply_rule`], the only store in this module). Gates right before the write: PlayerIns
//!    vtable, plausible `[PlayerIns+0xB78]`, a current value > 0 that equals the mirror at `PlayerIns+0xBA0` or the PlayerGameData
//!    HP. Never for remote players, never for ALLY/SELF/OTHER.
//!
//! Memory access goes through the guarded [`CombatMem`] trait; the real implementation uses `mem::read` (VirtualQuery-guarded)
//! and `mem::write_bytes` (the single store: [`Core::apply_rule`]).
//!
//! R3 (2026-10-20): [`memory`] (the guarded memory trait), [`tracker`] (per-player downed/HP/attacker transitions), [`rules`]
//! (rule v1 and the T-039 downed rule in [`Core`]); this file holds [`CombatCtl`], the main loop's handle. Offsets: `game::layout`.

use super::rtti::Rtti;
use crate::config::Config;

mod memory;
mod rules;
#[cfg(test)]
mod tests;
mod tracker;

pub(crate) use memory::*;
pub(crate) use rules::*;
pub(crate) use tracker::*;

// Offsets: game::layout (R2), MEMORY_MAP §5-7. The local names below are the ones this module has always used.
pub(crate) use crate::game::layout::GAMEDATA_HP as GD_HP;
pub(crate) use crate::game::layout::PLAYER_BLEED_CLOCK as OFF_BLEED_CLOCK;
pub(crate) use crate::game::layout::PLAYER_CUR_ATTACKER as OFF_CUR_ATTACKER;
pub(crate) use crate::game::layout::PLAYER_DOWNED as OFF_DOWNED;
pub(crate) use crate::game::layout::PLAYER_DOWNS as OFF_DOWNS;
pub(crate) use crate::game::layout::PLAYER_GAMEDATA as OFF_GAMEDATA;
pub(crate) use crate::game::layout::PLAYER_LAST_ATTACKER as OFF_LAST_ATTACKER;
pub(crate) use crate::game::layout::PLAYER_PAUSE as OFF_PAUSE;
pub(crate) use crate::game::layout::PLAYER_REVIVE_BAR as OFF_REVIVE_BAR;
pub(crate) use crate::game::layout::{CHR_HANDLE, GAMEDATA_CLASS};
/// A bar drop larger than this between two ticks is a hit.
pub(crate) const BAR_HIT_EPS: f32 = 0.01;
/// Lowest clock value the rule writes.
pub(crate) const CLOCK_FLOOR: f32 = 0.05;
/// Default / clamp range of `rival_hit_seconds`.
pub(crate) const DEFAULT_RIVAL_HIT_SECONDS: f32 = 3.0;
pub(crate) const RIVAL_HIT_SECONDS_RANGE: (f32, f32) = (0.5, 15.0);

/// Tracker period.
pub(crate) const STATE_MS: u64 = 100;

// ---------------------------------------------------------------------------------------------------
// Controller used by the main loop
// ---------------------------------------------------------------------------------------------------

pub(crate) struct CombatCtl {
    core: Core,
    mem: RealCombatMem,
    rtti_ready: bool,
    base: usize,
    started: std::time::Instant,
    subjects: Vec<Subject>,
}

impl CombatCtl {
    /// Both modes.
    pub(crate) fn new(cfg: &Config, base: usize) -> Self {
        let started = std::time::Instant::now();
        let mut core = Core::new(cfg.dev_mode, cfg.revive_policy());
        core.set_hit_seconds(cfg.rival_hit_seconds);
        tracing::info!(
            "T-035: combat-state tracker on (100 ms); T-039 downed rule on (50 ms, rival_hit_seconds {}); rival revive rule: {}{}",
            cfg.rival_hit_seconds,
            core.policy().label(),
            if cfg.dev_mode {
                ""
            } else {
                " (player build: always on)"
            }
        );
        CombatCtl {
            core,
            mem: RealCombatMem::default(),
            rtti_ready: false,
            base,
            started,
            subjects: Vec::new(),
        }
    }

    pub(crate) fn set_rtti(&mut self, rtti: &Rtti) {
        if self.mem.set_rtti(rtti, self.base) {
            self.rtti_ready = true;
            tracing::info!(
                "T-033: RTTI ready: {} PlayerIns vtable(s), {} {GAMEDATA_CLASS} vtable(s)",
                self.mem.player_vts.len(),
                self.mem.gd_vts.len()
            );
        }
    }

    /// T-040 (console `revive_fail`): the next `n` re-down tries fail without reading memory.
    pub(crate) fn set_forced_fails(&mut self, n: u32) {
        self.core.set_forced_fails(n);
    }

    pub(crate) fn forced_fails(&self) -> u32 {
        self.core.forced_fails()
    }

    /// T-040 follow-up (console `early_redown`): the get-up re-down experiment.
    pub(crate) fn set_early_redown(&mut self, on: bool) {
        self.core.early_redown = on;
        tracing::info!(
            "rule: early re-down (during the get-up) {}",
            if on { "ON" } else { "OFF" }
        );
    }

    pub(crate) fn early_redown(&self) -> bool {
        self.core.early_redown
    }

    /// T-041: the character handles (`ChrIns` handle field) of the players this game marks as RIVAL, read-only.
    pub(crate) fn rival_handles(&self) -> Vec<u64> {
        if !self.rtti_ready {
            return Vec::new();
        }
        self.subjects
            .iter()
            .filter(|s| !s.local && s.relation == Some(crate::overlay::shared::Relation::Rival))
            .filter(|s| self.mem.is_player(s.addr))
            .filter_map(|s| self.mem.read_u64(s.addr + CHR_HANDLE))
            .filter(|&h| valid_handle(h))
            .collect()
    }

    /// T-041: LOCAL's PlayerIns address and its state byte (`PlayerIns+0x710`), read-only.
    pub(crate) fn local_state(&self) -> Option<(usize, u8)> {
        if !self.rtti_ready {
            return None;
        }
        let s = self.subjects.iter().find(|s| s.local)?;
        if !self.mem.is_player(s.addr) {
            return None;
        }
        Some((s.addr, self.mem.read_u8(s.addr + OFF_DOWNED)?))
    }

    pub(crate) fn tick(&mut self, subjects: Vec<Subject>) {
        self.subjects = subjects;
        if !self.rtti_ready {
            return;
        }
        let now_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        for l in self.core.step(now_ms, &self.subjects, &mut self.mem) {
            tracing::info!("{l}");
        }
    }
}
