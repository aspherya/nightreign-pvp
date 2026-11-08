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
    /// Where the teams came from: `0` ids sorted and chunked, `1` the queue roster (MM-FILL), `2` ids because the roster map is incomplete.
    /// The view is re-logged when this changes too.
    pub(crate) source: u8,
}

/// An id of 0 is treated as unreadable: it is what an unfilled object would hold (every observed id is
/// non-zero, TECHNICAL_NOTES §4); failing open on it is the conservative choice.
pub(super) fn usable_id(id: Option<u64>) -> Option<u64> {
    id.filter(|&v| v != 0)
}

/// Build the view for `players` (address, id read from ChrIns+0x78) with `local` the LOCAL PlayerIns.
/// Rosters come from the queue roster (`roster_teams`: session id -> team, MM-FILL) when it is there and covers EVERY player;
/// otherwise from `assign_teams` over the ids, so every client computes the same ones. If any id is
/// unreadable or two players share an id, nothing can be trusted: everyone except LOCAL is a rival
/// (fail open to free-for-all) and `warning` says why. Bots live in another ChrSet, not in `players`, so they never block the map.
pub(crate) fn build_view(
    players: &[(usize, Option<u64>)],
    local: Option<usize>,
    team_size: usize,
    roster_teams: Option<&HashMap<u64, u8>>,
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
    let local_id = local
        .and_then(|l| players.iter().find(|(a, _)| *a == l))
        .and_then(|&(_, id)| usable_id(id))
        .map(PlayerId);
    let mut tag = "ids";
    if let Some(map) = roster_teams {
        let known = readable.iter().filter(|id| map.contains_key(id)).count();
        if !readable.is_empty() && known == readable.len() {
            return roster_view(view, players, local_id, map);
        }
        // mi-duo-fill 2026-11-08: the ids change when the expedition loads and each game republishes its own about 1-4 s later.
        // Until then sorted-id chunking can split a team (3 teams of 2), so a player the map does not know yet is neutral (no
        // relation: never hostile, never written as a rival) instead of a guessed rival or ally.
        if !map.is_empty() {
            let mut v = roster_view(view, players, local_id, map);
            v.source = 2;
            v.summary = format!(
                "teams (roster map {known}/{}, others neutral): {}",
                readable.len(),
                v.summary.trim_start_matches("teams (roster): ")
            );
            return v;
        }
        view.source = 2;
        tag = "ids, roster map";
    }
    let ids: Vec<PlayerId> = readable.iter().map(|&v| PlayerId(v)).collect();
    let rosters = assign_teams(&ids, team_size);
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
    view.summary = match roster_teams {
        Some(map) => {
            let known = readable.iter().filter(|id| map.contains_key(id)).count();
            format!(
                "teams ({tag} {known}/{}): {}",
                readable.len(),
                parts.join(", ")
            )
        }
        None => format!("teams: {}", parts.join(", ")),
    };
    view
}

/// MM-FILL: the view when the queue roster maps every player: teams and relations from it.
fn roster_view(
    mut view: RosterView,
    players: &[(usize, Option<u64>)],
    local_id: Option<PlayerId>,
    map: &HashMap<u64, u8>,
) -> RosterView {
    view.source = 1;
    let local_team = local_id.and_then(|l| map.get(&l.0).copied());
    let mut teams: std::collections::BTreeMap<u8, Vec<u64>> = std::collections::BTreeMap::new();
    for &(a, id) in players {
        let Some(pid) = usable_id(id) else { continue };
        if local_id.is_some_and(|l| l.0 == pid) {
            view.relation.insert(a, Relation::Local);
        }
        let Some(&t) = map.get(&pid) else { continue };
        view.team_no.insert(a, t);
        teams.entry(t).or_default().push(pid);
        if let Some(l) = local_id {
            let rel = if pid == l.0 {
                Some(Relation::Local)
            } else {
                // LOCAL not mapped yet (partial map): nobody is a rival or an ally yet.
                local_team.map(|lt| {
                    if lt == t {
                        Relation::Ally
                    } else {
                        Relation::Rival
                    }
                })
            };
            if let Some(rel) = rel {
                view.relation.insert(a, rel);
            }
        }
    }
    let parts: Vec<String> = teams
        .iter_mut()
        .map(|(t, ids)| {
            ids.sort_unstable();
            let m: Vec<String> = ids
                .iter()
                .map(|p| {
                    if Some(*p) == local_id.map(|l| l.0) {
                        format!("0x{p:X}(LOCAL)")
                    } else {
                        format!("0x{p:X}")
                    }
                })
                .collect();
            format!("team {t} = [{}]", m.join(", "))
        })
        .collect();
    view.summary = format!("teams (roster): {}", parts.join(", "));
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
