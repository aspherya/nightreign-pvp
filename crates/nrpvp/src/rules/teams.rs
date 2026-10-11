//! Deterministic team assignment (T-023, pure-logic part).
//!
//! Every client computes the same rosters from the same set of player ids with no messages: sort the
//! ids, then chunk by `team_size`. Input order never matters.
//!
//! The id is the per-player value read from `ChrIns+0x78` (observed 0x6001FFFFFFFF, 0x6002FFFFFFFF; a
//! small ordinal in the high part). It is treated as an opaque `u64`; only its ordering is relied on.

use std::collections::BTreeSet;

/// Per-player id (opaque; only `Ord` is relied on).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerId(pub u64);

/// Team index (0-based, in roster order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TeamId(pub u8);

/// Team rosters. `teams[i]` is team `TeamId(i)`, members sorted by id.
///
/// A roster produced by [`assign_teams_stable`] may contain empty teams (their ids stay reserved so
/// other teams keep theirs); the query methods skip them where it matters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rosters {
    team_size: usize,
    teams: Vec<Vec<PlayerId>>,
}

fn to_team_id(index: usize) -> TeamId {
    // More than 255 teams is impossible in practice (the game caps lobbies at a handful of players).
    TeamId(u8::try_from(index).unwrap_or(u8::MAX))
}

fn normalized(players: &[PlayerId]) -> Vec<PlayerId> {
    let mut v = players.to_vec();
    v.sort();
    v.dedup();
    v
}

/// Assign players to teams: sort by id, chunk by `team_size` (a `team_size` of 0 is treated as 1).
/// The last team may be short. Duplicate ids are collapsed.
///
/// Identical on every client regardless of input order.
///
/// CONSEQUENCE of recomputing after a join or leave: a player can change team when someone with a
/// lower id leaves (everyone after them shifts down), and a late joiner with a low id shifts everyone
/// above them up. Use [`assign_teams_stable`] once a match is running.
pub fn assign_teams(players: &[PlayerId], team_size: usize) -> Rosters {
    let team_size = team_size.max(1);
    let sorted = normalized(players);
    Rosters {
        team_size,
        teams: sorted.chunks(team_size).map(<[PlayerId]>::to_vec).collect(),
    }
}

/// Keep every existing assignment from `previous` for players that are still present, drop leavers,
/// and place new players (in id order) in the lowest-numbered team with room, creating a new team
/// when all are full. Team ids never change.
///
/// Result depends on `previous`, so it is identical across clients only if every client saw the same
/// join/leave history (ASSUMPTION: the lobby's membership changes are observed in the same order, or
/// at least one client's roster is authoritative). Callers that need hard agreement should freeze the
/// rosters at match start and never recompute.
#[allow(dead_code)] // not used by T-026 (rosters are recomputed); kept for the match phase
pub fn assign_teams_stable(previous: &Rosters, players: &[PlayerId]) -> Rosters {
    let present: BTreeSet<PlayerId> = players.iter().copied().collect();
    let mut teams: Vec<Vec<PlayerId>> = previous
        .teams
        .iter()
        .map(|t| t.iter().copied().filter(|p| present.contains(p)).collect())
        .collect();
    let known: BTreeSet<PlayerId> = previous.teams.iter().flatten().copied().collect();
    for p in present.iter().filter(|p| !known.contains(p)) {
        match teams.iter_mut().find(|t| t.len() < previous.team_size) {
            Some(t) => {
                t.push(*p);
                t.sort();
            }
            None => teams.push(vec![*p]),
        }
    }
    Rosters {
        team_size: previous.team_size,
        teams,
    }
}

impl Rosters {
    #[allow(dead_code)] // API for later cards
    pub fn team_size(&self) -> usize {
        self.team_size
    }

    /// Number of non-empty teams.
    #[allow(dead_code)] // API for later cards
    pub fn team_count(&self) -> usize {
        self.teams.iter().filter(|t| !t.is_empty()).count()
    }

    #[allow(dead_code)] // API for later cards
    pub fn player_count(&self) -> usize {
        self.teams.iter().map(Vec::len).sum()
    }

    /// Non-empty teams in id order, with their members (sorted by id).
    pub fn teams(&self) -> impl Iterator<Item = (TeamId, &[PlayerId])> + '_ {
        self.teams
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.is_empty())
            .map(|(i, t)| (to_team_id(i), t.as_slice()))
    }

    /// Members of a team (empty slice if the team does not exist).
    #[allow(dead_code)] // API for later cards
    pub fn members(&self, team: TeamId) -> &[PlayerId] {
        self.teams
            .get(usize::from(team.0))
            .map_or(&[], Vec::as_slice)
    }

    pub fn team_of(&self, player: PlayerId) -> Option<TeamId> {
        self.teams
            .iter()
            .position(|t| t.contains(&player))
            .map(to_team_id)
    }

    /// The other members of `player`'s team (empty if unknown).
    #[allow(dead_code)] // API for later cards
    pub fn teammates(&self, player: PlayerId) -> Vec<PlayerId> {
        match self.team_of(player) {
            Some(t) => self
                .members(t)
                .iter()
                .copied()
                .filter(|p| *p != player)
                .collect(),
            None => Vec::new(),
        }
    }

    /// True when both players are known, distinct, and on the same team. A player is not their own
    /// ally and unknown players are nobody's ally.
    pub fn are_allies(&self, a: PlayerId, b: PlayerId) -> bool {
        a != b && matches!((self.team_of(a), self.team_of(b)), (Some(x), Some(y)) if x == y)
    }

    /// True when both players are known and on different teams. Unknown players are nobody's rival.
    #[allow(dead_code)] // API for later cards (T-026 treats "known and not an ally" as a rival)
    pub fn are_rivals(&self, a: PlayerId, b: PlayerId) -> bool {
        matches!((self.team_of(a), self.team_of(b)), (Some(x), Some(y)) if x != y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ids shaped like the observed ones: ordinal in the high part, 0xFFFFFFFF low.
    fn pid(n: u64) -> PlayerId {
        PlayerId(((0x6000 + n) << 32) | 0xFFFF_FFFF)
    }

    fn players(n: u64) -> Vec<PlayerId> {
        (1..=n).map(pid).collect()
    }

    /// Deterministic shuffle (LCG), no external crates.
    fn shuffled(v: &[PlayerId], seed: u64) -> Vec<PlayerId> {
        let mut v = v.to_vec();
        let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        for i in (1..v.len()).rev() {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            v.swap(i, ((s >> 33) as usize) % (i + 1));
        }
        v
    }

    #[test]
    fn observed_ids_two_players() {
        let a = PlayerId(0x6001FFFFFFFF);
        let b = PlayerId(0x6002FFFFFFFF);
        let r = assign_teams(&[b, a], 1);
        assert_eq!(r.team_of(a), Some(TeamId(0)));
        assert_eq!(r.team_of(b), Some(TeamId(1)));
        assert!(r.are_rivals(a, b));
        let r = assign_teams(&[b, a], 2);
        assert!(r.are_allies(a, b));
    }

    #[test]
    fn permutation_invariance_and_shape() {
        for n in 1..=18u64 {
            for size in 1..=3usize {
                let base = players(n);
                let expected = assign_teams(&base, size);
                for seed in 0..20 {
                    assert_eq!(assign_teams(&shuffled(&base, seed), size), expected);
                }
                assert_eq!(expected.player_count(), n as usize);
                assert_eq!(expected.team_count(), (n as usize).div_ceil(size));
                let sizes: Vec<usize> = expected.teams().map(|(_, m)| m.len()).collect();
                for (i, len) in sizes.iter().enumerate() {
                    if i + 1 < sizes.len() {
                        assert_eq!(*len, size);
                    } else {
                        assert!(*len >= 1 && *len <= size);
                    }
                }
                // every player in exactly one team, members sorted
                for p in &base {
                    assert!(expected.team_of(*p).is_some());
                }
                for (_, m) in expected.teams() {
                    assert!(m.windows(2).all(|w| w[0] < w[1]));
                }
            }
        }
    }

    #[test]
    fn duplicates_and_zero_size() {
        let r = assign_teams(&[pid(1), pid(1), pid(2)], 0);
        assert_eq!(r.team_size(), 1);
        assert_eq!(r.player_count(), 2);
        assert_eq!(r.team_count(), 2);
    }

    #[test]
    fn empty_input() {
        let r = assign_teams(&[], 3);
        assert_eq!(r.team_count(), 0);
        assert_eq!(r.team_of(pid(1)), None);
    }

    #[test]
    fn ally_rival_queries() {
        let r = assign_teams(&players(6), 3);
        let (a, b, c, d) = (pid(1), pid(2), pid(3), pid(4));
        assert!(r.are_allies(a, b));
        assert!(!r.are_allies(a, a));
        assert!(!r.are_rivals(a, a));
        assert!(r.are_rivals(c, d));
        assert!(!r.are_allies(a, d));
        assert_eq!(r.teammates(a), vec![b, c]);
        assert!(!r.are_allies(a, pid(99)));
        assert!(!r.are_rivals(a, pid(99)));
        assert!(r.teammates(pid(99)).is_empty());
    }

    #[test]
    fn recompute_can_move_players_but_stable_does_not() {
        let all = players(6);
        let before = assign_teams(&all, 3);
        let without_lowest: Vec<_> = all[1..].to_vec();
        // Plain recompute shifts everyone: pid(4) moves from team 1 to team 0.
        let recomputed = assign_teams(&without_lowest, 3);
        assert_eq!(before.team_of(pid(4)), Some(TeamId(1)));
        assert_eq!(recomputed.team_of(pid(4)), Some(TeamId(0)));
        // Stable keeps everyone where they were.
        let stable = assign_teams_stable(&before, &without_lowest);
        for p in &without_lowest {
            assert_eq!(stable.team_of(*p), before.team_of(*p));
        }
        assert_eq!(stable.team_of(pid(1)), None);
    }

    #[test]
    fn stable_fills_gaps_then_new_team() {
        let before = assign_teams(&players(6), 3);
        // pid(2) leaves, then a low-id newcomer arrives: takes the gap in team 0.
        let mut now: Vec<_> = players(6).into_iter().filter(|p| *p != pid(2)).collect();
        let r1 = assign_teams_stable(&before, &now);
        now.push(pid(0));
        let r2 = assign_teams_stable(&r1, &now);
        assert_eq!(r2.team_of(pid(0)), Some(TeamId(0)));
        for p in players(6).into_iter().filter(|p| *p != pid(2)) {
            assert_eq!(r2.team_of(p), before.team_of(p));
        }
        // All full: another newcomer opens team 2.
        now.push(pid(50));
        let r3 = assign_teams_stable(&r2, &now);
        assert_eq!(r3.team_of(pid(50)), Some(TeamId(2)));
        assert_eq!(r3.team_count(), 3);
    }

    #[test]
    fn stable_is_order_independent_and_idempotent() {
        for n in 1..=18u64 {
            for size in 1..=3usize {
                let base = assign_teams(&players(n), size);
                let newcomers: Vec<_> = (30..34).map(pid).collect();
                let mut cur: Vec<_> = players(n).into_iter().skip(1).collect();
                cur.extend(newcomers);
                let a = assign_teams_stable(&base, &cur);
                for seed in 0..10 {
                    assert_eq!(assign_teams_stable(&base, &shuffled(&cur, seed)), a);
                }
                assert_eq!(assign_teams_stable(&a, &cur), a);
                for (_, m) in a.teams() {
                    assert!(m.len() <= size);
                }
            }
        }
    }

    #[test]
    fn stable_empty_teams_keep_ids() {
        let before = assign_teams(&players(6), 3);
        let r = assign_teams_stable(&before, &players(6)[3..]);
        assert_eq!(r.team_count(), 1);
        assert_eq!(r.team_of(pid(4)), Some(TeamId(1)));
        assert_eq!(r.teams().next().map(|(t, _)| t), Some(TeamId(1)));
    }
}
