//! T-020/T-026 PvPvE mode: mark rivals hostile every `pvpve_interval_ms`, pause/resume (dev), roster changes.

use std::time::Instant;

use super::*;
use crate::config::Scheme;
use crate::game::mem::{self};
use crate::overlay::shared::PvpveState;

impl TeamCtl {
    /// T-020: current state of the PvPvE mode.
    pub(crate) fn pvpve_state(&self) -> PvpveState {
        match (self.pvpve_enabled, self.pvpve_running) {
            (false, _) => PvpveState::Off,
            (true, true) => PvpveState::On,
            (true, false) => PvpveState::Paused,
        }
    }

    /// Resume (`on`) or pause the PvPvE mode (overlay checkbox, F11/F12). Does nothing, and says so, when the mode is
    /// off in the config.
    pub(crate) fn set_pvpve(&mut self, on: bool, src: &str) {
        // T-030: no pause/resume in the player build, whatever asks for it.
        if !self.dev {
            return;
        }
        if !self.pvpve_enabled {
            tracing::info!(
                "T-020: {src}: PvPvE mode is OFF in the config (pvpve = false); ignored"
            );
            return;
        }
        if self.pvpve_running == on {
            return;
        }
        self.pvpve_running = on;
        self.next_pvpve = Instant::now();
        tracing::info!(
            "T-020: {src}: PvPvE mode {} ({} automatic write(s) so far)",
            if on { "resumed" } else { "paused" },
            self.pvpve_writes
        );
        if !on {
            self.restore_written(src);
        }
    }

    /// T-026: put back the captured original of every player the rivals tick wrote (pause, roster change).
    /// A player whose value is no longer `pvpve_team` was changed by someone else and is left alone.
    pub(super) fn restore_written(&mut self, src: &str) {
        let addrs: Vec<usize> = self.pvpve_written.drain().collect();
        for addr in addrs {
            self.restore_written_one(addr, src);
        }
    }

    pub(super) fn restore_written_one(&mut self, addr: usize, src: &str) {
        self.pvpve_written.remove(&addr);
        let Some(&orig) = self.originals.get(&addr) else {
            return;
        };
        // Guarded read; offset VERIFIED on NR 1.3.3.0 (TECHNICAL_NOTES §4).
        if mem::read::<u8>(addr + CHR_TEAM) != Some(self.pvpve_team) {
            return;
        }
        match write_team_type(addr, orig, &self.vts) {
            Ok(w) => tracing::info!(
                "T-026: {src}: restored PlayerIns 0x{addr:X} +0x74: {} -> {orig} (readback {})",
                w.old,
                w.readback
            ),
            Err(e) => tracing::warn!("T-026: {src}: restore of PlayerIns 0x{addr:X} failed: {e}"),
        }
    }

    /// Automatic writes this session.
    #[cfg(test)]
    pub(crate) fn pvpve_writes(&self) -> u64 {
        self.pvpve_writes
    }

    /// `ON (team 29, 12 writes)` for the heartbeat line.
    pub(crate) fn pvpve_summary(&self) -> String {
        match self.pvpve_state() {
            PvpveState::Off => "OFF".to_string(),
            s => format!(
                "{} (team {}, {} writes, scheme {}, team_size {})",
                s.label(),
                self.pvpve_team,
                self.pvpve_writes,
                self.pvpve_scheme.label(),
                self.team_size_now()
            ),
        }
    }

    /// T-026: read every player's id and build the roster view (quiet).
    pub(super) fn view_for(&self, res: &Resolution) -> RosterView {
        let players: Vec<(usize, Option<u64>)> = res
            .players
            .iter()
            // Guarded read (VirtualQuery). ChrIns+0x78 is a per-player id identical on all clients:
            // VERIFIED on NR 1.3.3.0 by FS-0/FS-1 for 2 players, see TECHNICAL_NOTES §4.
            .map(|&p| (p, mem::read::<u64>(p + CHR_P2P_HANDLE)))
            .collect();
        // MM-FILL part B: tell the queue worker our own session id (it publishes it as `nrpvp_pid`) and take the roster's teams from it.
        let local_id = res
            .local
            .and_then(|l| players.iter().find(|(a, _)| *a == l))
            .and_then(|&(_, id)| super::roster::usable_id(id));
        crate::mm::set_local_pid(local_id);
        let map = crate::mm::roster_teams();
        build_view(&players, res.local, self.team_size_now(), map.as_ref())
    }

    /// T-026: log the rosters once per change of the player set.
    pub(super) fn note_roster(&mut self, view: &RosterView) {
        if self.last_roster_key.as_ref() == Some(&view.key) && self.last_roster_src == view.source {
            return;
        }
        self.last_roster_key = Some(view.key.clone());
        self.last_roster_src = view.source;
        if view.warning.is_some() {
            tracing::warn!("{}", view.summary);
        } else {
            tracing::info!("{}", view.summary);
        }
    }

    /// T-020/T-026: one PvPvE pass. Scheme "all": every ChrSet player whose live team_type is [`PVPVE_FROM`] gets
    /// `pvpve_team`. Scheme "rivals": only rivals of the local player do, and players this tick wrote that are no longer
    /// rivals (roster change) are restored. Every other value (Roundtable 1, anything a manual experiment set) is left
    /// alone. Quiet resolve: resolve errors are logged (deduplicated) by the watcher. Same gated 1-byte write as the
    /// hotkeys.
    pub(super) fn pvpve_tick(&mut self) {
        let Ok(res) = resolve(self.inst, &self.vts) else {
            return;
        };
        self.last_player_count = Some(res.players.len());
        let view = self.view_for(&res);
        self.note_roster(&view);
        let live: Vec<(usize, Option<u8>)> = res
            .players
            .iter()
            // Read-only, VirtualQuery-guarded. Offset VERIFIED on NR 1.3.3.0 (TECHNICAL_NOTES §4).
            .map(|&p| (p, mem::read::<u8>(p + CHR_TEAM)))
            .collect();
        let plan = plan_pvpve(self.pvpve_scheme, &view, &live, &self.pvpve_written);
        for addr in plan.restores {
            self.restore_written_one(addr, "roster change");
        }
        for p in plan.writes {
            let who = res.who(p).unwrap_or_else(|| "?".to_string());
            let team = self.pvpve_team;
            match self.apply_write(p, &who, team) {
                Ok((w, _first, _orig)) => {
                    if self.pvpve_scheme == Scheme::Rivals {
                        self.pvpve_written.insert(p);
                    }
                    self.pvpve_writes += 1;
                    let n = self.pvpve_writes;
                    if self.should_log_write(n) {
                        tracing::info!(
                            "T-020: pvpve team write who={who} PlayerIns 0x{p:X} +0x74: {} -> {team} (readback {}); write #{n}",
                            w.old,
                            w.readback
                        );
                    }
                }
                Err(e) => {
                    let now_ms = self.now_ms();
                    if self.pvpve_fail_log.should_log_failure(now_ms) {
                        tracing::warn!(
                            "T-020: pvpve team write who={who} to PlayerIns 0x{p:X} failed: {e}"
                        );
                    }
                }
            }
        }
    }

    /// Whether automatic write number `n` gets its own log line. Dev: the first [`PVPVE_LOG_ALL_UP_TO`], then every
    /// [`PVPVE_LOG_EVERY`]th. Player build (T-030): the first [`PLAYER_LOG_FIRST`], then one summary line a minute.
    pub(super) fn should_log_write(&mut self, n: u64) -> bool {
        if self.dev {
            return n <= PVPVE_LOG_ALL_UP_TO || n.is_multiple_of(PVPVE_LOG_EVERY);
        }
        if n <= PLAYER_LOG_FIRST {
            return true;
        }
        let due = self
            .last_summary
            .is_none_or(|t| t.elapsed() >= PLAYER_SUMMARY_EVERY);
        if due {
            self.last_summary = Some(Instant::now());
            tracing::info!("T-020: pvpve summary: {n} automatic team write(s) so far");
        }
        false
    }
}
