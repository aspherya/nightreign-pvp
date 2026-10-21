//! Manual team writes and restores (console team_set / team_restore) and persist re-application (team_persist).

use std::time::Instant;

use super::*;
use crate::game::mem::{self};

impl TeamCtl {
    /// Overlay candidate button: select `v` and write it to the local player. A value outside the
    /// configured list is refused.
    pub(crate) fn set_and_write(&mut self, v: u8, src: &str) {
        if self.cycler.current() != v {
            if !self.cycler.select(v) {
                tracing::warn!(
                    "T-008: {src}: {v} is not in team_candidates {:?}; ignored",
                    self.cycler.list()
                );
                return;
            }
            let (i, n) = self.cycler.position();
            tracing::info!("T-008: selected team candidate = {v} ({i}/{n})");
        }
        self.apply_selected(src);
    }

    /// Write the selected candidate to the local player. `src` is the trigger ("F5", "overlay", ...).
    pub(crate) fn apply_selected(&mut self, src: &str) {
        let Some((res, local)) = self.local_or_log(src) else {
            return;
        };
        let who = res.who(local).unwrap_or_else(|| "LOCAL".to_string());
        let v = self.cycler.current();
        self.write_to(local, &who, v, src);
    }

    /// T-015 row button: write `v` to this player's local copy (does not change the selected candidate).
    pub(crate) fn set_for(&mut self, addr: usize, v: u8, src: &str) {
        let Some((_res, who)) = self.target_or_log(addr, src) else {
            return;
        };
        self.write_to(addr, &who, v, src);
    }

    /// The gated write plus its bookkeeping (original capture, last-written value, persist-value follow-up), without
    /// the log line. `Ok((write, first, original))`: `first` = this was the first write to `addr`, `original` = the
    /// value captured before it.
    pub(super) fn apply_write(
        &mut self,
        addr: usize,
        who: &str,
        v: u8,
    ) -> Result<(WriteOk, bool, u8), String> {
        let w = write_team_type(addr, v, &self.vts)?;
        let first = !self.originals.contains_key(&addr);
        let orig = *self.originals.entry(addr).or_insert(w.old);
        self.last_written.insert(addr, v);
        if w.readback != v {
            tracing::warn!(
                "T-008: who={who} readback {} != written {v}; the game changed it immediately or the store did not stick",
                w.readback
            );
        }
        // A player in persist mode keeps the newest value written to it.
        if let Some(p) = self.persist.get_mut(&addr) {
            if p.value != v {
                tracing::info!(
                    "T-008: who={who} persist value for 0x{addr:X} is now {v} (was {})",
                    p.value
                );
                p.value = v;
            }
        }
        Ok((w, first, orig))
    }

    /// The write itself plus its log line. `who` is the `LOCAL` / `REMOTE[i]` label. Returns true on success.
    pub(super) fn write_to(&mut self, addr: usize, who: &str, v: u8, src: &str) -> bool {
        match self.apply_write(addr, who, v) {
            Ok((w, first, orig)) => {
                tracing::info!(
                    "T-008: {src} team write who={who} PlayerIns 0x{addr:X} +0x74: {} -> {v} (readback {}){}",
                    w.old,
                    w.readback,
                    if first {
                        format!("; captured original {orig}")
                    } else {
                        format!("; original is {orig}")
                    }
                );
                true
            }
            Err(e) => {
                tracing::warn!("T-008: {src} write who={who} to PlayerIns 0x{addr:X} failed: {e}");
                false
            }
        }
    }

    /// Restore the local player's captured original.
    pub(crate) fn restore(&mut self, src: &str) {
        let Some((res, local)) = self.local_or_log(src) else {
            return;
        };
        let who = res.who(local).unwrap_or_else(|| "LOCAL".to_string());
        self.restore_at(local, &who, src);
    }

    /// T-015 row button: restore this player's captured original.
    pub(crate) fn restore_for(&mut self, addr: usize, src: &str) {
        let Some((_res, who)) = self.target_or_log(addr, src) else {
            return;
        };
        self.restore_at(addr, &who, src);
    }

    pub(super) fn restore_at(&mut self, addr: usize, who: &str, src: &str) {
        let Some(&orig) = self.originals.get(&addr) else {
            tracing::info!(
                "T-008: {src} ignored: no original value captured for who={who} PlayerIns 0x{addr:X} (write a value first)"
            );
            return;
        };
        if self.persist.contains_key(&addr) {
            tracing::warn!(
                "T-008: persist is ON for who={who} and will re-apply its value right after this restore; turn persist off first"
            );
        }
        match write_team_type(addr, orig, &self.vts) {
            Ok(w) => tracing::info!(
                "T-008: {src} restore who={who} PlayerIns 0x{addr:X} +0x74: {} -> {orig} (readback {})",
                w.old,
                w.readback
            ),
            Err(e) => tracing::warn!(
                "T-008: {src} restore of who={who} PlayerIns 0x{addr:X} failed: {e}"
            ),
        }
    }

    /// T-015 row checkbox: persist on/off for this player. Turning it on re-applies the last value written
    /// to it (or the selected candidate if nothing was written).
    pub(crate) fn set_persist_for(&mut self, addr: usize, on: bool, src: &str) {
        let Some((_res, who)) = self.target_or_log(addr, src) else {
            return;
        };
        self.persist_at(addr, &who, on, None, src);
    }

    pub(super) fn persist_at(
        &mut self,
        addr: usize,
        who: &str,
        on: bool,
        value: Option<u8>,
        src: &str,
    ) {
        if on {
            let v = value
                .or_else(|| self.last_written.get(&addr).copied())
                .unwrap_or_else(|| self.cycler.current());
            let mut tracker = PersistTracker::new();
            tracker.reset(self.now_ms());
            self.persist.insert(
                addr,
                PersistEntry {
                    value: v,
                    tracker,
                    first_logged: false,
                },
            );
            self.next_persist = Instant::now();
            tracing::info!(
                "T-008: {src} persist ON who={who} PlayerIns 0x{addr:X}: re-applying team_type {v} every {} ms",
                self.persist_every.as_millis()
            );
        } else if let Some(p) = self.persist.remove(&addr) {
            tracing::info!(
                "T-008: {src} persist OFF who={who} PlayerIns 0x{addr:X} ({} revert(s) counted)",
                p.tracker.total_reverts()
            );
        }
    }

    pub(super) fn persist_tick(&mut self) {
        let now_ms = self.now_ms();
        // Quiet resolve: errors are already logged (deduplicated) by the watcher.
        let Ok(res) = resolve(self.inst, &self.vts) else {
            return;
        };
        for (&addr, entry) in self.persist.iter_mut() {
            // A persist entry whose PlayerIns left the ChrSet is idle (and is dropped with the instance).
            let Some(who) = res.who(addr) else {
                continue;
            };
            if let Some(s) = entry.tracker.summary(now_ms) {
                tracing::info!("T-008: who={who} PlayerIns 0x{addr:X} {s}");
            }
            let Some(seen) = mem::read::<u8>(addr + CHR_TEAM) else {
                continue;
            };
            let want = entry.value;
            if !entry.tracker.observe(addr, want, seen) {
                continue;
            }
            match write_team_type(addr, want, &self.vts) {
                Ok(w) => {
                    self.originals.entry(addr).or_insert(w.old);
                    self.last_written.insert(addr, want);
                    if !entry.first_logged {
                        entry.first_logged = true;
                        tracing::info!(
                            "T-008: persist first apply who={who} PlayerIns 0x{addr:X} +0x74: {} -> {want} (readback {}); later re-applies are counted in the persist summary",
                            w.old,
                            w.readback
                        );
                    }
                }
                Err(e) => {
                    entry.tracker.write_failed();
                    if entry.tracker.should_log_failure(now_ms) {
                        tracing::warn!(
                            "T-008: persist write who={who} to PlayerIns 0x{addr:X} failed: {e}"
                        );
                    }
                }
            }
        }
    }
}
