//! T-026: rosters from the per-player ids and the "rivals only" PvPvE plan. Pure logic, no game memory.

use std::collections::{HashMap, HashSet};

use crate::config::Scheme;
use crate::overlay::shared::Relation;
use crate::rules::teams::{assign_teams, PlayerId};

// ---------------------------------------------------------------------------------------------------
// T-026: rosters and the "rivals only" plan (pure logic, no game memory)
// ---------------------------------------------------------------------------------------------------

/// What the rosters say about the players of the current ChrSet (keyed by PlayerIns address).
#[derive(Debug, Default)]
pub(crate) struct RosterView {
    pub(crate) relation: HashMap<usize, Relation>,
    pub(crate) team_no: HashMap<usize, u8>,
    pub(crate) ids: HashMap<usize, Option<u64>>,
    /// Set when the ids were unusable and the view fell back to "everyone except LOCAL is a rival".
    pub(crate) warning: Option<String>,
    /// `teams: team 0 = [0x6001FFFFFFFF(LOCAL)], team 1 = [0x6002FFFFFFFF]` (or the warning text).
    pub(crate) summary: String,
    /// Sorted (id, is_local) pairs: the view is re-logged only when this changes.
    pub(crate) key: Vec<(Option<u64>, bool)>,
}

/// An id of 0 is treated as unreadable: it is what an unfilled object would hold (every observed id is
/// non-zero, TECHNICAL_NOTES §4); failing open on it is the conservative choice.
pub(super) fn usable_id(id: Option<u64>) -> Option<u64> {
    id.filter(|&v| v != 0)
}

/// Build the view for `players` (address, id read from ChrIns+0x78) with `local` the LOCAL PlayerIns.
/// Rosters come from `assign_teams` over the ids, so every client computes the same ones. If any id is
/// unreadable or two players share an id, nothing can be trusted: everyone except LOCAL is a rival
/// (fail open to free-for-all) and `warning` says why.
pub(crate) fn build_view(
    players: &[(usize, Option<u64>)],
    local: Option<usize>,
    team_size: usize,
) -> RosterView {
    let mut view = RosterView::default();
    for &(a, id) in players {
        view.ids.insert(a, usable_id(id));
    }
    let mut key: Vec<(Option<u64>, bool)> = players
        .iter()
        .map(|&(a, id)| (usable_id(id), Some(a) == local))
        .collect();
    key.sort();
    view.key = key;
    let readable: Vec<u64> = players
        .iter()
        .filter_map(|&(_, id)| usable_id(id))
        .collect();
    let mut dedup = readable.clone();
    dedup.sort_unstable();
    dedup.dedup();
    let problem = if readable.len() != players.len() {
        Some("a player id (ChrIns+0x78) is unreadable".to_string())
    } else if dedup.len() != readable.len() {
        Some("two players share the same id".to_string())
    } else {
        None
    };
    if let Some(p) = problem {
        for &(a, _) in players {
            if local.is_some() {
                let rel = if Some(a) == local {
                    Relation::Local
                } else {
                    Relation::Rival
                };
                view.relation.insert(a, rel);
            }
        }
        let msg = format!("{p}; treating everyone except LOCAL as a rival (free-for-all)");
        view.summary = format!("teams: WARNING {msg}");
        view.warning = Some(msg);
        return view;
    }
    let ids: Vec<PlayerId> = readable.iter().map(|&v| PlayerId(v)).collect();
    let rosters = assign_teams(&ids, team_size);
    let local_id = local
        .and_then(|l| players.iter().find(|(a, _)| *a == l))
        .and_then(|&(_, id)| usable_id(id))
        .map(PlayerId);
    for &(a, id) in players {
        let Some(pid) = usable_id(id).map(PlayerId) else {
            continue;
        };
        if let Some(t) = rosters.team_of(pid) {
            view.team_no.insert(a, t.0);
        }
        if let Some(l) = local_id {
            let rel = if pid == l {
                Relation::Local
            } else if rosters.are_allies(l, pid) {
                Relation::Ally
            } else {
                Relation::Rival
            };
            view.relation.insert(a, rel);
        }
    }
    let parts: Vec<String> = rosters
        .teams()
        .map(|(t, members)| {
            let m: Vec<String> = members
                .iter()
                .map(|p| {
                    if Some(*p) == local_id {
                        format!("0x{:X}(LOCAL)", p.0)
                    } else {
                        format!("0x{:X}", p.0)
                    }
                })
                .collect();
            format!("team {} = [{}]", t.0, m.join(", "))
        })
        .collect();
    view.summary = format!("teams: {}", parts.join(", "));
    view
}

/// One PvPvE pass, decided: which PlayerIns get `pvpve_team` and which previously written ones are restored.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PvpvePlan {
    pub(crate) writes: Vec<usize>,
    /// Previously written players that are no longer rivals; the executor restores each one's captured
    /// original if its value is still `pvpve_team`, and forgets it either way.
    pub(crate) restores: Vec<usize>,
}

/// `players` = (address, live team_type). Scheme "all": every player at 2 is written (T-020, unchanged).
/// Scheme "rivals": only rivals of the local player at 2 are written; LOCAL, allies and players with no
/// known relation (no local player) are never written.
pub(crate) fn plan_pvpve(
    scheme: Scheme,
    view: &RosterView,
    players: &[(usize, Option<u8>)],
    written: &HashSet<usize>,
) -> PvpvePlan {
    let mut plan = PvpvePlan::default();
    for &(a, cur) in players {
        let wants = cur.is_some_and(pvpve_wants_write);
        match scheme {
            Scheme::All => {
                if wants {
                    plan.writes.push(a);
                }
            }
            Scheme::Rivals => {
                let rival = view.relation.get(&a) == Some(&Relation::Rival);
                if rival && wants {
                    plan.writes.push(a);
                } else if !rival && written.contains(&a) {
                    plan.restores.push(a);
                }
            }
        }
    }
    plan
}

/// T-020: true if the PvPvE mode should overwrite this live team_type (only the expedition default).
pub(crate) fn pvpve_wants_write(current: u8) -> bool {
    current == PVPVE_FROM
}

/// The team_type players have during an expedition [VERIFIED: TC-004 runs, FS-0]; the PvPvE mode only replaces this value.
pub(super) const PVPVE_FROM: u8 = 2;
