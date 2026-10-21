//! The read side: the quiet resolve every 100 ms that feeds the console snapshot and the downed rule's subjects.

use std::time::Instant;

use super::*;
use crate::game::mem::{self};
use crate::overlay::shared::{Role, Snapshot};

impl TeamCtl {
    /// Fill the team part of the overlay snapshot (T-013, T-015). Runs on the main loop thread: the only
    /// place that reads game memory for the overlay. Read-only; refreshes the resolve at most every
    /// [`LIVE_REFRESH`].
    pub(crate) fn fill_snapshot(&mut self, s: &mut Snapshot) {
        if Instant::now() >= self.next_live {
            self.next_live = Instant::now() + LIVE_REFRESH;
            self.refresh_live();
        }
        s.vtables_known = !self.vts.is_empty();
        s.local_player = self.live.local;
        s.team_type = self.live.team;
        s.note = self.live.note.clone();
        s.local_basis = self.live.basis.clone();
        s.original = self
            .live
            .local
            .and_then(|l| self.originals.get(&l).copied());
        s.selected = self.cycler.current();
        s.candidates = self.cycler.list().to_vec();
        s.persist = self
            .live
            .local
            .is_some_and(|l| self.persist.contains_key(&l));
        s.reverts_last_sec = self
            .persist
            .values()
            .map(|p| p.tracker.reverts_last_window())
            .sum();
        s.reverts_total = self
            .persist
            .values()
            .map(|p| p.tracker.total_reverts())
            .sum();
        s.marks = self.marker.count();
        s.pvpve = self.pvpve_state();
        s.pvpve_team = self.pvpve_team;
        s.pvpve_writes = self.pvpve_writes;
        s.pvpve_scheme = self.pvpve_scheme.label().to_string();
        s.team_size = u8::try_from(self.team_size_now()).unwrap_or(u8::MAX);
        s.roster_warning = self.live.roster_warning.clone();
        s.players = self
            .live
            .rows
            .iter()
            .map(|r| {
                let mut r = r.clone();
                r.original = self.originals.get(&r.addr).copied();
                if let Some(p) = self.persist.get(&r.addr) {
                    r.persist = Some(p.value);
                    r.reverts_last_sec = p.tracker.reverts_last_window();
                    r.reverts_total = p.tracker.total_reverts();
                }
                r
            })
            .collect();
    }

    /// T-033/T-035 (both modes): every PlayerIns of the ChrSet with its id and relation to LOCAL, from the same quiet resolve that
    /// feeds the overlay (refreshed at most every [`LIVE_REFRESH`]). Read-only.
    pub(crate) fn combat_subjects(&mut self) -> Vec<crate::game::combat_state::Subject> {
        if Instant::now() >= self.next_live {
            self.next_live = Instant::now() + LIVE_REFRESH;
            self.refresh_live();
        }
        self.live
            .rows
            .iter()
            .map(|r| crate::game::combat_state::Subject {
                addr: r.addr,
                label: if r.role == Role::Local {
                    format!("LOCAL[{}]", r.index)
                } else {
                    format!("REMOTE[{}]", r.index)
                },
                local: r.role == Role::Local,
                id: r.id,
                relation: r.relation,
                team_no: r.team_no,
            })
            .collect()
    }

    /// Quiet resolve of the players and their live team_type (no logging; the watcher logs changes).
    pub(super) fn refresh_live(&mut self) {
        self.live = LiveView::default();
        if self.inst == 0 {
            self.live.note = "WorldChrMan is not live".to_string();
            return;
        }
        if self.vts.is_empty() {
            self.live.note = "PlayerIns vtables unknown (RTTI not recovered yet)".to_string();
            return;
        }
        match resolve(self.inst, &self.vts) {
            Ok(r) => {
                // Read-only, VirtualQuery-guarded (mem::read). Offset VERIFIED on NR 1.3.3.0 by
                // T-007a; see TECHNICAL_NOTES section 4.
                self.live.rows = r.rows(&mut |a| mem::read::<u8>(a + CHR_TEAM));
                let view = self.view_for(&r);
                self.note_roster(&view);
                for row in &mut self.live.rows {
                    row.id = view.ids.get(&row.addr).copied().flatten();
                    row.team_no = view.team_no.get(&row.addr).copied();
                    row.relation = view.relation.get(&row.addr).copied();
                }
                self.live.roster_warning = view.warning.clone().unwrap_or_default();
                match r.local {
                    Some(l) => {
                        self.live.local = Some(l);
                        self.live.team = mem::read::<u8>(l + CHR_TEAM);
                        self.live.basis = r.basis.label();
                    }
                    None => self.live.note = r.reasoning(),
                }
            }
            Err(e) => self.live.note = e.to_string(),
        }
    }
}
