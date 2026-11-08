//! MM-FILL (part A): packing queue tickets into teams. Pure logic, no Steam, no game memory.
//!
//! A ticket is `(party id, party size)`. A match has `teams` teams of `team_size` slots; `teams` is `ceil(need / team_size)`
//! ([`teams_for`]), so a `need` that is not a multiple of the team size gets one short team. A party is never split.
//!
//! The packing is deterministic: tickets are sorted by size (largest first), then by party id (lowest first); each goes into the
//! lowest-numbered team with room (first-fit decreasing). The result does not depend on the input order, so every client that sees
//! the same tickets computes the same teams. For party sizes 1..=3 (team size 1..=3) first-fit decreasing is exact.

/// Number of teams for a lobby of `need` players: `need / team_size`, rounded up (a short last team is allowed).
pub(crate) fn teams_for(need: u32, team_size: u32) -> u32 {
    need.div_ceil(team_size.max(1))
}

/// Team number (0-based) for every party, sorted by party id, or `None` when the tickets do not fit `teams` teams of `team_size`.
pub(crate) fn pack(tickets: &[(u64, u32)], team_size: u32, teams: u32) -> Option<Vec<(u64, u8)>> {
    let teams = usize::try_from(teams.min(255)).unwrap_or(255);
    let mut sorted: Vec<(u64, u32)> = tickets.to_vec();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut room = vec![team_size; teams];
    let mut out = Vec::with_capacity(sorted.len());
    for (party, size) in sorted {
        if size == 0 || size > team_size {
            return None;
        }
        let team = room.iter().position(|&r| r >= size)?;
        room[team] -= size;
        out.push((party, u8::try_from(team).unwrap_or(u8::MAX)));
    }
    out.sort_unstable();
    Some(out)
}

/// The largest party size (0..=`team_size`) that could still be added to `tickets` with the result packable and the total at most
/// `need`. A single can always fill leftover room later, so this is `1` whenever any room is left and no bigger party fits. `0` = nothing fits.
pub(crate) fn fit(tickets: &[(u64, u32)], team_size: u32, need: u32) -> u32 {
    let total: u32 = tickets.iter().map(|t| t.1).sum();
    let teams = teams_for(need, team_size);
    (1..=team_size)
        .rev()
        .find(|&s| {
            total + s <= need && {
                let mut with = tickets.to_vec();
                with.push((u64::MAX, s));
                pack(&with, team_size, teams).is_some()
            }
        })
        .unwrap_or(0)
}

/// Party ids to send away so the rest packs: while the tickets do not pack, the highest party id that is not in `fixed` goes.
/// Empty when everything packs.
pub(crate) fn bounce(
    tickets: &[(u64, u32)],
    team_size: u32,
    teams: u32,
    fixed: &[u64],
) -> Vec<u64> {
    let mut rest = tickets.to_vec();
    let mut out = Vec::new();
    while pack(&rest, team_size, teams).is_none() {
        let Some(victim) = rest
            .iter()
            .map(|t| t.0)
            .filter(|p| !fixed.contains(p))
            .max()
        else {
            break;
        };
        rest.retain(|t| t.0 != victim);
        out.push(victim);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn teams_of(p: &[(u64, u8)]) -> Vec<(u64, u8)> {
        p.to_vec()
    }

    #[test]
    fn teams_round_up() {
        assert_eq!(teams_for(6, 2), 3);
        assert_eq!(teams_for(6, 3), 2);
        assert_eq!(teams_for(5, 2), 3);
        assert_eq!(teams_for(6, 1), 6);
    }

    #[test]
    fn duo_one_one_two_two_packs_into_three_teams() {
        let p = pack(&[(10, 1), (20, 1), (30, 2), (40, 2)], 2, 3).expect("packs");
        // The pairs take the first two teams; the two singles share the third.
        assert_eq!(teams_of(&p), vec![(10, 2), (20, 2), (30, 0), (40, 1)]);
        assert_eq!(fit(&[(10, 1), (20, 1), (30, 2), (40, 2)], 2, 6), 0);
    }

    #[test]
    fn trio_two_one_three_packs() {
        let p = pack(&[(1, 2), (2, 1), (3, 3)], 3, 2).expect("packs");
        assert_eq!(teams_of(&p), vec![(1, 1), (2, 1), (3, 0)]);
        // Full: nothing fits.
        assert_eq!(fit(&[(1, 2), (2, 1), (3, 3)], 3, 6), 0);
    }

    #[test]
    fn trio_three_pairs_do_not_pack_and_two_pairs_only_take_singles() {
        assert_eq!(pack(&[(1, 2), (2, 2), (3, 2)], 3, 2), None);
        assert_eq!(fit(&[(1, 2), (2, 2)], 3, 6), 1);
        // A pair and a trio: another pair fits the leftover? 2 + 3 = 5 of 6, one slot left: a single.
        assert_eq!(fit(&[(1, 2), (2, 3)], 3, 6), 1);
        // Empty lobby: a whole team fits.
        assert_eq!(fit(&[], 3, 6), 3);
        // One single: a pair still fits.
        assert_eq!(fit(&[(1, 1)], 3, 6), 3);
        // Pair + single in team 0 (full), pair in team 1: one more slot, a single.
        assert_eq!(fit(&[(1, 2), (2, 1), (3, 2)], 3, 6), 1);
    }

    #[test]
    fn solo_behaves_as_before() {
        let p = pack(&[(5, 1), (3, 1), (9, 1)], 1, 3).expect("packs");
        assert_eq!(teams_of(&p), vec![(3, 0), (5, 1), (9, 2)]);
        assert_eq!(pack(&[(1, 1), (2, 1)], 1, 1), None);
        assert_eq!(fit(&[(1, 1), (2, 1)], 1, 6), 1);
        assert_eq!(fit(&[(1, 1), (2, 1)], 1, 2), 0);
        // An oversized party never packs.
        assert_eq!(pack(&[(1, 2)], 1, 6), None);
    }

    #[test]
    fn a_short_last_team_is_allowed_and_the_total_never_exceeds_need() {
        // need 5, Duo: 3 teams, the last one short.
        assert_eq!(teams_for(5, 2), 3);
        assert_eq!(fit(&[(1, 2), (2, 2)], 2, 5), 1);
        assert_eq!(fit(&[(1, 2), (2, 2), (3, 1)], 2, 5), 0);
    }

    #[test]
    fn the_result_does_not_depend_on_the_input_order() {
        let a = [(10, 1), (20, 2), (30, 1), (40, 2), (50, 1)];
        let mut b = a;
        b.reverse();
        let mut c = a;
        c.swap(0, 3);
        assert_eq!(pack(&a, 2, 3), pack(&b, 2, 3));
        assert_eq!(pack(&a, 2, 3), pack(&c, 2, 3));
        assert_eq!(fit(&a[..4], 2, 6), fit(&b[1..], 2, 6));
    }

    #[test]
    fn bounce_names_the_highest_party_that_breaks_the_packing() {
        let t = [(10, 2), (20, 2), (30, 2)];
        assert_eq!(bounce(&t, 3, 2, &[]), vec![30]);
        // The owner's party is never bounced.
        assert_eq!(bounce(&t, 3, 2, &[30]), vec![20]);
        // Everything packs: nothing to bounce.
        assert!(bounce(&[(10, 2), (20, 1)], 3, 2, &[]).is_empty());
    }
}
