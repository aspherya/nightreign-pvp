//! NET-2: where the channel's roster comes from. The Steam thread (`steam/probe.rs`) already sees every lobby event; once a second it
//! asks [`scan`] which Seamless Co-op lobbies (data key `nrsc_lobby_type`) we are a member of and publishes their members here. The net
//! worker reads them with [`lobby_members`]. We only ever read lobby getters; nothing is written to any lobby.

use std::collections::BTreeSet;
use std::sync::Mutex;

use crate::steam::LobbyApi;

const KEY_SC_TYPE: &str = "nrsc_lobby_type";
/// Same cap the probe uses for member loops (a garbage count must not spin the thread).
const MAX_MEMBERS: i32 = 64;

static MEMBERS: Mutex<BTreeSet<u64>> = Mutex::new(BTreeSet::new());

/// Result of one [`scan`].
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Scan {
    /// Members (including ourselves) of every Seamless Co-op lobby we are a member of.
    pub(crate) members: BTreeSet<u64>,
    /// The lobbies of the input that we are still a member of (the caller keeps only these, so left lobbies do not pile up).
    pub(crate) joined: Vec<u64>,
}

/// Look at the lobbies in `lobbies` (getters only).
pub(crate) fn scan(api: &dyn LobbyApi, own: u64, lobbies: &[u64]) -> Scan {
    let mut out = Scan::default();
    if own == 0 {
        return out;
    }
    for &lobby in lobbies {
        let n = api.member_count(lobby);
        if n <= 0 {
            continue;
        }
        let ids: Vec<u64> = (0..n.clamp(0, MAX_MEMBERS))
            .map(|i| api.member_by_index(lobby, i))
            .collect();
        if !ids.contains(&own) {
            continue;
        }
        out.joined.push(lobby);
        let is_sc = (0..api.data_count(lobby).clamp(0, 256))
            .filter_map(|i| api.data_by_index(lobby, i))
            .any(|(k, _)| k == KEY_SC_TYPE);
        if is_sc {
            out.members.extend(ids.into_iter().filter(|&i| i != 0));
        }
    }
    out
}

/// Probe thread: publish the current members. Returns `true` when they changed.
pub(crate) fn publish_members(ids: BTreeSet<u64>) -> bool {
    match MEMBERS.lock() {
        Ok(mut g) if *g != ids => {
            *g = ids;
            true
        }
        _ => false,
    }
}

/// Net worker: the last published members.
pub(crate) fn lobby_members() -> BTreeSet<u64> {
    MEMBERS.lock().map(|g| g.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Fake {
        members: HashMap<u64, Vec<u64>>,
        keys: HashMap<u64, Vec<&'static str>>,
    }
    impl LobbyApi for Fake {
        fn owner(&self, _: u64) -> u64 {
            0
        }
        fn member_count(&self, l: u64) -> i32 {
            self.members.get(&l).map_or(0, |m| m.len() as i32)
        }
        fn member_limit(&self, _: u64) -> i32 {
            6
        }
        fn member_by_index(&self, l: u64, i: i32) -> u64 {
            self.members[&l][i as usize]
        }
        fn data_count(&self, l: u64) -> i32 {
            self.keys.get(&l).map_or(0, |k| k.len() as i32)
        }
        fn data_by_index(&self, l: u64, i: i32) -> Option<(String, String)> {
            Some((self.keys[&l][i as usize].to_string(), String::new()))
        }
    }

    fn fake() -> Fake {
        Fake {
            members: HashMap::from([
                (100, vec![1, 2, 3]), // SC lobby, we (1) are in it
                (200, vec![1, 9]),    // our queue lobby: not SC
                (300, vec![5, 6]),    // an SC lobby we are not in
                (400, vec![0]),       // a lobby we left still lists a zero
            ]),
            keys: HashMap::from([
                (100, vec!["nrsc_lobby_type", "x"]),
                (200, vec!["nrpvp"]),
                (300, vec!["nrsc_lobby_type"]),
                (400, vec!["nrsc_lobby_type"]),
            ]),
        }
    }

    #[test]
    fn only_sc_lobbies_we_are_in_count() {
        let f = fake();
        let got = scan(&f, 1, &[100, 200, 300, 400, 999]);
        assert_eq!(got.members, BTreeSet::from([1, 2, 3]));
        assert_eq!(
            got.joined,
            vec![100, 200],
            "left or foreign lobbies are not kept"
        );
        assert_eq!(
            scan(&f, 0, &[100]),
            Scan::default(),
            "unknown own id: nothing"
        );
        assert_eq!(
            scan(&f, 7, &[100]),
            Scan::default(),
            "not a member: nothing"
        );
    }

    #[test]
    fn publish_reports_changes_only() {
        assert!(publish_members(BTreeSet::from([11, 12])));
        assert!(!publish_members(BTreeSet::from([11, 12])));
        assert_eq!(lobby_members(), BTreeSet::from([11, 12]));
        assert!(publish_members(BTreeSet::new()));
    }
}
