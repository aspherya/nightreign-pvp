//! T-028/T-032 snap-back test (dev only, LOCAL player only): select, save and write a position candidate (console pos_next,
//! pos_save, pos_go), then log every candidate at +100 ms / +500 ms / +2 s.

use std::time::{Duration, Instant};

use super::*;
use crate::game::mem::{self, is_ptr};
use crate::game::poswrite::{self, RealMem};

impl TeamCtl {
    /// T-032 (dev only): select the next snap-back position candidate (console `pos_next`).
    pub(crate) fn pos_cycle(&mut self) {
        if !self.dev {
            return;
        }
        let line = self.pos.cycle();
        tracing::info!("{line}");
    }

    /// The local PlayerIns, freshly resolved (ChrSet entry[0] rule, PlayerIns vtable check). Quiet resolve; logs only failures.
    pub(super) fn fresh_local(&mut self, src: &str) -> Option<usize> {
        if self.inst == 0 || self.vts.is_empty() {
            tracing::info!(
                "T-032 POS: {src} ignored, WorldChrMan or the PlayerIns vtables are not known yet"
            );
            return None;
        }
        let res = match resolve(self.inst, &self.vts) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("T-032 POS: {src} ignored, could not resolve the players: {e}");
                return None;
            }
        };
        let Some(local) = res.local else {
            tracing::warn!(
                "T-032 POS: {src} ignored: no local player ({})",
                res.reasoning()
            );
            return None;
        };
        // Re-check the vtable of the object right now (resolve() already required it; this guards the window since).
        let vt_ok =
            is_ptr(local) && mem::read::<usize>(local).is_some_and(|vt| self.vts.contains(vt));
        if !vt_ok {
            tracing::warn!(
                "T-032 POS: {src} ignored: 0x{local:X} no longer has a PlayerIns vtable"
            );
            return None;
        }
        Some(local)
    }

    /// T-032 (dev only): save the LOCAL player's current 12 bytes of every candidate (console `pos_save`).
    pub(crate) fn pos_save(&mut self, src: &str) {
        if !self.dev {
            return;
        }
        let Some(local) = self.fresh_local(src) else {
            return;
        };
        tracing::info!("T-032 POS: {src} save for LOCAL PlayerIns 0x{local:X}");
        for l in self.pos.save(&RealMem, local) {
            tracing::info!("{l}");
        }
    }

    /// T-032 (dev only): write the saved 12 bytes into the SELECTED candidate of the LOCAL player (console `pos_go`), then schedule the
    /// +100 ms / +500 ms / +2 s value logs.
    pub(crate) fn pos_write(&mut self, src: &str) {
        if !self.dev {
            return;
        }
        let Some(local) = self.fresh_local(src) else {
            return;
        };
        tracing::info!("{}", self.pos.values_line(&RealMem, local, "before write"));
        match self.pos.write(&mut RealMem, local) {
            Ok(r) => {
                tracing::info!("{} ({src})", r.line());
                let now = Instant::now();
                self.pos_follow = poswrite::FOLLOW_MS
                    .iter()
                    .zip(["+100ms", "+500ms", "+2s"])
                    .map(|(&ms, label)| (now + Duration::from_millis(ms), label, local))
                    .collect();
            }
            Err(e) => tracing::warn!("T-032 POS: {src} write refused: {e}"),
        }
    }

    /// T-032: log every candidate's value for the pending follow-ups that are due. Read-only.
    pub(super) fn pos_follow_tick(&mut self) {
        if self.pos_follow.is_empty() {
            return;
        }
        let now = Instant::now();
        let (due, rest): (Vec<_>, Vec<_>) = self
            .pos_follow
            .drain(..)
            .partition(|(when, _, _)| now >= *when);
        self.pos_follow = rest;
        for (_, label, player) in due {
            tracing::info!("{}", self.pos.values_line(&RealMem, player, label));
        }
    }
}
