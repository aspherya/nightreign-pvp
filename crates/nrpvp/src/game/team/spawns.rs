//! F3 separate team spawns: the [`TeamCtl`] glue. The drop detection and the move live in `game::spawn`; this file gives it the
//! LOCAL player, whether it is in an expedition, and (only at the hawk drop) the team numbers from the roster.

use super::*;
use crate::game::spawn::SpawnCtl;

impl TeamCtl {
    /// F3: one spawn pass: resolve the LOCAL player (quiet), check it is in an expedition, and let [`SpawnCtl`] decide.
    /// The roster is only built at the hawk drop.
    pub(super) fn spawn_tick(&mut self, sp: &mut SpawnCtl) {
        let res = resolve(self.inst, &self.vts).ok();
        let local = res.as_ref().and_then(|r| r.local);
        let in_expedition = local.is_some_and(|l| {
            read_fields(&mut RegionCache::direct(), l).is_some_and(|f| f.team == PVPVE_FROM)
        });
        sp.tick(self.inst, local, in_expedition, || {
            let res = res.as_ref()?;
            let view = self.view_for(res);
            if let Some(w) = &view.warning {
                tracing::warn!("T-028 SPAWN: roster not usable: {w}");
                return None;
            }
            tracing::info!("T-028 SPAWN: {}", view.summary);
            let mine = *view.team_no.get(&local?)?;
            Some((mine, view.team_no.values().copied().collect()))
        });
    }
}
