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

use crate::game::layout::{DATA_MODULE_HP as DM_HP_OFF, DATA_MODULE_MAX_HP as DM_MAX_HP_OFF};

/// Dev console `god 0|1` (off at launch; only the console sets it).
pub(crate) static GOD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Dev console `hp0` (2026-10-24, M5-9 tests): the next tick writes 0 to LOCAL's HP, see [`zero_hp`].
pub(crate) static KILL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Dev console `hp0`: put the character at 0 HP through the data module field the T-040 re-down and `god` write (MEMORY_MAP §5), gated
/// like them (HP > 0 and equal to PlayerGameData's). `Ok(old hp)`. By day or in a session this downs the player; at night with
/// SpEffect 102130 on (M5-9) it is a real death.
pub(crate) fn zero_hp(mem: &mut dyn CombatMem, addr: usize) -> Result<u32, String> {
    let (field, hp) = data_module_hp(mem, addr)?;
    mem.write(field, &0u32.to_le_bytes())
        .map_err(|e| e.to_string())?;
    Ok(hp)
}

/// Dev console `rival_hit` (RW-1 tests): the next tick stamps a rival hit on LOCAL, as if a rival's blow had just landed.
pub(crate) static FAKE_RIVAL_HIT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// RW-1: who dealt the last HP-reducing hit to LOCAL, as far as this game can tell (the last-attacker handle is valid on the
/// victim's own game only, see the module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HitKind {
    /// A player this game marks as a rival.
    Rival,
    /// A teammate or LOCAL itself.
    Friendly,
    /// An enemy character (a handle that is not a player).
    Mob,
    /// No attacker handle at all: the tide, a fall, poison.
    Env,
}

/// RW-1: LOCAL's recent hits, ages in ms (the controller's own clock).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LocalHits {
    /// The last HP-reducing hit.
    pub last: Option<(HitKind, u64)>,
    /// The last hit (or downed-state hit) a rival dealt.
    pub rival_age_ms: Option<u64>,
}

/// M5-5: the team key of a player: its roster team when the rosters are known, else its own session id (free-for-all).
pub(crate) fn team_key(team_no: Option<u8>, id: u64) -> u64 {
    team_no.map_or(id, |t| (1 << 63) | u64::from(t))
}

/// M5-5: one ChrSet player as the night match sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Fighter {
    pub addr: usize,
    /// Session player id (`ChrIns+0x78`): the same on every game.
    pub id: u64,
    /// [`team_key`]: equal for teammates on every game.
    pub team: u64,
    pub local: bool,
    /// `PlayerIns+0x710`: 0 up, 1 downed, 2 getting up.
    pub state: u8,
    pub hp: u32,
    /// Bleed-out seconds left (`PlayerIns+0x718`; meaningful while downed, and fresh only for LOCAL).
    pub clock: f32,
}

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
    /// RW-1: LOCAL's HP at the previous tick, its last hit and its last rival hit (ms on `started`).
    hit_prev_hp: Option<u32>,
    last_hit: Option<(HitKind, u64)>,
    last_rival_ms: Option<u64>,
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
            hit_prev_hp: None,
            last_hit: None,
            last_rival_ms: None,
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// RW-1: LOCAL's recent hits (read-only).
    pub(crate) fn local_hits(&self) -> LocalHits {
        let now = self.now_ms();
        LocalHits {
            last: self.last_hit.map(|(k, t)| (k, now.saturating_sub(t))),
            rival_age_ms: self.last_rival_ms.map(|t| now.saturating_sub(t)),
        }
    }

    /// RW-1: stamp a rival hit now (the downed rule saw one, or the console faked one).
    fn stamp_rival(&mut self) {
        let now = self.now_ms();
        if self
            .last_rival_ms
            .is_none_or(|t| now.saturating_sub(t) >= 2_000)
        {
            tracing::info!("T-033 hit: a rival's hit on LOCAL is recorded (RW-1)");
        }
        self.last_rival_ms = Some(now);
        self.last_hit = Some((HitKind::Rival, now));
    }

    /// RW-1: watch LOCAL's HP; a drop is a hit, classified by the last-attacker handle (read right away, while it is fresh).
    fn watch_hits(&mut self) {
        let Some(addr) = self.subjects.iter().find(|s| s.local).map(|s| s.addr) else {
            self.hit_prev_hp = None;
            return;
        };
        if !self.mem.is_player(addr) {
            self.hit_prev_hp = None;
            return;
        }
        let r = read_state(&self.mem, addr);
        let (Some(hp), prev) = (r.hp, self.hit_prev_hp) else {
            return;
        };
        self.hit_prev_hp = Some(hp);
        if prev.is_none_or(|p| hp >= p) {
            return;
        }
        let infos = player_infos(&self.mem, &self.subjects);
        let kind = match classify(r.attacker_raw, &infos) {
            Attacker {
                kind: AttackerKind::Rival,
                ..
            } => HitKind::Rival,
            Attacker {
                kind: AttackerKind::Ally | AttackerKind::SelfHit,
                ..
            } => HitKind::Friendly,
            Attacker {
                kind: AttackerKind::Other,
                handle: Some(_),
                ..
            } => HitKind::Mob,
            Attacker {
                kind: AttackerKind::Other,
                handle: None,
                ..
            } => HitKind::Env,
        };
        if kind == HitKind::Rival {
            self.stamp_rival();
        } else {
            let now = self.now_ms();
            self.last_hit = Some((kind, now));
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

    /// T-041 hit hook: the T-039 penalty for `n` rival hits whose bar decrease was cancelled inside the game (the bar did not move,
    /// so `downed_step` sees no hit): the bleed-out clock loses `rival_hit_seconds` per hit and the pause timer is cleared. LOCAL
    /// in state 1 only; same gates as `restore_after_rival_hit`.
    pub(crate) fn penalize_rival_hits(&mut self, n: u32) {
        let Some((addr, 1)) = self.local_state() else {
            return;
        };
        let (Some(bar), Some(clock), Some(pause)) = (
            read_f32(&self.mem, addr + OFF_REVIVE_BAR),
            read_f32(&self.mem, addr + OFF_BLEED_CLOCK),
            read_f32(&self.mem, addr + OFF_PAUSE),
        ) else {
            return;
        };
        if !(clock.is_finite() && (0.0..=60.0).contains(&clock)) {
            return;
        }
        self.stamp_rival();
        let new_clock = (clock - self.core.hit_seconds * n as f32).max(self.core.clock_floor);
        let ok = self
            .mem
            .write(addr + OFF_BLEED_CLOCK, &new_clock.to_le_bytes())
            .and_then(|()| self.mem.write(addr + OFF_PAUSE, &0f32.to_le_bytes()));
        self.core.fired += n;
        tracing::info!(
            "rule: rival hit on downed LOCAL cancelled in the game (T-041 hook, {n} hit(s)): bar stays {bar}, clock {clock} -> {new_clock}, pause {pause} -> 0{}",
            if ok.is_ok() { "" } else { " (write refused)" }
        );
    }

    /// T-041 hit hook: clear LOCAL's pause timer again (the game writes it right after the cancelled hit's store).
    pub(crate) fn clear_pause(&mut self) {
        if let Some((addr, 1)) = self.local_state() {
            if read_f32(&self.mem, addr + OFF_PAUSE).is_some_and(|p| p > 0.0) {
                let _ = self.mem.write(addr + OFF_PAUSE, &0f32.to_le_bytes());
            }
        }
    }

    /// RW-9 markers: the `PlayerIns` addresses of the players this game marks as RIVAL (their map id names their map markers).
    pub(crate) fn rival_addrs(&self) -> Vec<usize> {
        if !self.rtti_ready {
            return Vec::new();
        }
        self.subjects
            .iter()
            .filter(|s| !s.local && s.relation == Some(crate::overlay::shared::Relation::Rival))
            .filter(|s| self.mem.is_player(s.addr))
            .map(|s| s.addr)
            .collect()
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

    /// M5-5: every ChrSet player as the night match needs it (read-only): state byte, HP, bleed-out clock, team key.
    pub(crate) fn fighters(&self) -> Vec<Fighter> {
        if !self.rtti_ready {
            return Vec::new();
        }
        self.subjects
            .iter()
            .filter(|s| self.mem.is_player(s.addr))
            .filter_map(|s| {
                let r = read_state(&self.mem, s.addr);
                Some(Fighter {
                    addr: s.addr,
                    id: s.id?,
                    team: team_key(s.team_no, s.id?),
                    local: s.local,
                    state: self.mem.read_u8(s.addr + OFF_DOWNED)?,
                    hp: r.hp?,
                    clock: read_f32(&self.mem, s.addr + OFF_BLEED_CLOCK).unwrap_or(0.0),
                })
            })
            .collect()
    }

    /// Dev console `god 1` (test aid, 2026-10-23: monsters kept downing the knocking player in two-window night tests): LOCAL,
    /// while up, is put back to full HP every tick through the character data module (the field T-040 writes; MEMORY_MAP §5).
    fn god_refill(&mut self) {
        let Some((addr, 0)) = self.local_state() else {
            return;
        };
        if let Ok((field, hp)) = data_module_hp(&self.mem, addr) {
            let max = self
                .mem
                .read_u32(field - DM_HP_OFF + DM_MAX_HP_OFF)
                .unwrap_or(0);
            if hp < max {
                let _ = self.mem.write(field, &max.to_le_bytes());
            }
        }
    }

    pub(crate) fn tick(&mut self, subjects: Vec<Subject>) {
        self.subjects = subjects;
        if !self.rtti_ready {
            return;
        }
        if KILL.swap(false, std::sync::atomic::Ordering::Relaxed) {
            match self.local_state() {
                Some((addr, 0)) => match zero_hp(&mut self.mem, addr) {
                    Ok(hp) => tracing::info!("console hp0: LOCAL HP {hp} -> 0"),
                    Err(e) => tracing::warn!("console hp0: refused ({e})"),
                },
                other => tracing::warn!("console hp0: LOCAL is not up ({other:?})"),
            }
        } else if GOD.load(std::sync::atomic::Ordering::Relaxed) {
            self.god_refill();
        }
        let now_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let fired = self.core.fired;
        for l in self.core.step(now_ms, &self.subjects, &mut self.mem) {
            tracing::info!("{l}");
        }
        // RW-1: a rival-related rule write while LOCAL is downed (a hit on the bleeding player) is a rival hit too.
        if self.core.fired != fired && self.local_state().is_some_and(|(_, s)| s == 1) {
            self.stamp_rival();
        }
        self.watch_hits();
        if FAKE_RIVAL_HIT.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.stamp_rival();
            tracing::info!("console rival_hit: a rival hit on LOCAL is recorded now");
        }
    }
}
