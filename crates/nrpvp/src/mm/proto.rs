//! MM-2: the data protocol of the queue lobby (pure text handling, unit-tested).
//!
//! Queue lobby data (written by the owner only): `nrpvp=1`, `mode`, `kind` (ARENA-1: `br` / `arena`), `proto`, `game`, `sc`, `need` (players for a match),
//! later `merge_to` (this lobby dissolves: everyone searches again), `fit` (MM-FILL: the biggest party the lobby can still take),
//! `bounce` (MM-FILL: party ids that no longer pack and must leave), and at match time `host`, `roster`, `state=go`.
//! Per-member data on the queue lobby: `nrpvp_party` (party leader's Steam id, = own id for Solo) and `nrpvp_size`.
//! Member value on Seamless Co-op's lobby (ours only): `nrpvp_q` = `<queue lobby id>:<mode>:<party size>`.

use super::Mode;
use crate::steam::queue::SearchFilters;

/// Protocol version tag (`proto=4`; 3 = MM-FILL: roster entries carry the team; 4 = ARENA-1: the `kind` tag).
pub(crate) const PROTO: u32 = 4;
pub(crate) const K_MODE: &str = "mode";
/// ARENA-1: which mode the match is for, `br` or `arena`; part of the search filter, so the two queues never meet.
pub(crate) const K_KIND: &str = "kind";
pub(crate) const KIND_BR: &str = "br";
pub(crate) const KIND_ARENA: &str = "arena";
pub(crate) const K_PROTO: &str = "proto";
pub(crate) const K_GAME: &str = "game";
pub(crate) const K_SC: &str = "sc";
pub(crate) const K_NEED: &str = "need";
/// RW-6: `public`, or `p-<hash of the private match code>`: only players with the same room meet.
pub(crate) const K_ROOM: &str = "room";
pub(crate) const K_STATE: &str = "state";
pub(crate) const K_HOST: &str = "host";
pub(crate) const K_ROSTER: &str = "roster";
pub(crate) const K_MERGE: &str = "merge_to";
/// MM-FILL (owner writes): the biggest party size the lobby can still take and still pack into teams. Missing = the team size.
pub(crate) const K_FIT: &str = "fit";
/// MM-FILL (owner writes): comma separated party ids whose tickets no longer pack; those parties leave and search again.
pub(crate) const K_BOUNCE: &str = "bounce";
/// Value of `bounce` once nobody is bounced (an empty value is not written to Steam).
pub(crate) const BOUNCE_NONE: &str = "none";
/// MM-3 (host writes): the Seamless Co-op lobby id, `present=<n>/<total>`, and the Steam ids the host invited (comma separated).
pub(crate) const K_SC_LOBBY: &str = crate::steam::queue::KEY_PUBLISHED_SC;
pub(crate) const K_PRESENT: &str = "present";
pub(crate) const K_INVITED: &str = "invited";
pub(crate) const STATE_GO: &str = "go";
pub(crate) const M_PARTY: &str = "nrpvp_party";
pub(crate) const M_SIZE: &str = "nrpvp_size";

/// The version tags a lobby must carry for us to use it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tags {
    pub mode: Mode,
    /// ARENA-1: [`KIND_BR`] or [`KIND_ARENA`].
    pub kind: &'static str,
    pub game: String,
    pub sc: String,
    pub need: u32,
    /// RW-6: [`room_tag`] of the private code (`public` when none).
    pub room: String,
}

/// RW-6: the room tag for a private match code: `public` for none, else `p-` and a 64-bit FNV-1a hash of the (normalised) code in
/// hex, so the code itself never appears in the lobby data.
pub(crate) fn room_tag(code: &str) -> String {
    if code.is_empty() {
        return "public".to_string();
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in code.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("p-{h:016x}")
}

/// Data the owner writes on a new queue lobby (the tag `nrpvp=1` first).
pub(crate) fn tag_pairs(t: &Tags) -> Vec<(String, String)> {
    vec![
        (crate::steam::queue::KEY_TAG.to_string(), "1".to_string()),
        (K_MODE.to_string(), t.mode.label().to_string()),
        (K_KIND.to_string(), t.kind.to_string()),
        (K_PROTO.to_string(), PROTO.to_string()),
        (K_GAME.to_string(), t.game.clone()),
        (K_SC.to_string(), t.sc.clone()),
        (K_NEED.to_string(), t.need.to_string()),
        (K_ROOM.to_string(), t.room.clone()),
    ]
}

/// Search filters: every tag must match; `slots` free slots are required.
pub(crate) fn search_filters(t: &Tags, slots: u32) -> SearchFilters {
    SearchFilters {
        strings: tag_pairs(t),
        slots: i32::try_from(slots).unwrap_or(1),
        max_results: 50,
    }
}

/// One roster line entry: a player, the party they queued with and the team (0-based) the queue owner packed that party into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RosterEntry {
    pub id: u64,
    pub party: u64,
    pub team: u8,
}

/// `id:party:team,...` sorted by id, so every client writes and prints the same text.
pub(crate) fn encode_roster(entries: &[RosterEntry]) -> String {
    let mut e = entries.to_vec();
    e.sort();
    e.iter()
        .map(|r| format!("{}:{}:{}", r.id, r.party, r.team))
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn parse_roster(s: &str) -> Vec<RosterEntry> {
    s.split(',')
        .filter_map(|p| {
            let mut it = p.split(':');
            let (a, b, c) = (it.next()?, it.next()?, it.next()?);
            it.next().is_none().then_some(())?;
            Some(RosterEntry {
                id: a.trim().parse().ok()?,
                party: b.trim().parse().ok()?,
                team: c.trim().parse().ok()?,
            })
        })
        .collect()
}

/// A member's declared ticket (from member data), `None` while it is not published yet.
pub(crate) fn parse_ticket(party: &str, size: &str) -> Option<(u64, u32)> {
    let p: u64 = party.trim().parse().ok().filter(|&p| p != 0)?;
    let s: u32 = size.trim().parse().ok().filter(|&s| s != 0)?;
    Some((p, s))
}

/// What the members of a queue lobby add up to.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TicketReport {
    /// Players that belong to a complete ticket.
    pub complete_players: u32,
    /// Parties that are still missing members (party id).
    pub incomplete_parties: Vec<u64>,
    /// Members whose ticket is not published yet.
    pub unknown: u32,
    /// The roster of the complete tickets, sorted (team 0: the owner fills the teams in with `pack`).
    pub roster: Vec<RosterEntry>,
    /// Every published party with its declared size, complete or not, sorted by party id (MM-FILL: room is reserved for a party
    /// whose members are still arriving).
    pub parties: Vec<(u64, u32)>,
}

impl TicketReport {
    /// `(party id, size)` of the complete tickets only.
    pub(crate) fn complete_tickets(&self) -> Vec<(u64, u32)> {
        let mut v: Vec<(u64, u32)> = Vec::new();
        for r in &self.roster {
            match v.iter_mut().find(|t| t.0 == r.party) {
                Some(t) => t.1 += 1,
                None => v.push((r.party, 1)),
            }
        }
        v
    }
}

/// Group members by party. A ticket is complete when as many members carry its party id as its declared size.
pub(crate) fn tickets(members: &[(u64, Option<(u64, u32)>)]) -> TicketReport {
    let mut r = TicketReport::default();
    let mut parties: Vec<u64> = members.iter().filter_map(|m| m.1.map(|t| t.0)).collect();
    parties.sort_unstable();
    parties.dedup();
    for p in parties {
        let of_party: Vec<&(u64, Option<(u64, u32)>)> = members
            .iter()
            .filter(|m| m.1.map(|t| t.0) == Some(p))
            .collect();
        let size = of_party
            .iter()
            .filter_map(|m| m.1.map(|t| t.1))
            .max()
            .unwrap_or(1);
        r.parties.push((p, size));
        if of_party.len() as u32 >= size {
            r.complete_players += of_party.len() as u32;
            r.roster.extend(of_party.iter().map(|m| RosterEntry {
                id: m.0,
                party: p,
                team: 0,
            }));
        } else {
            r.incomplete_parties.push(p);
        }
    }
    r.unknown = members.iter().filter(|m| m.1.is_none()).count() as u32;
    r.roster.sort();
    r
}

/// `present=<n>/<total>`.
pub(crate) fn encode_present(n: usize, total: usize) -> String {
    format!("{n}/{total}")
}

pub(crate) fn parse_present(s: &str) -> Option<(usize, usize)> {
    let (a, b) = s.trim().split_once('/')?;
    let (n, t): (usize, usize) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (t != 0 && n <= t).then_some((n, t))
}

/// Comma separated Steam ids (`invited=`), sorted so the text is stable.
pub(crate) fn encode_ids(ids: &[u64]) -> String {
    let mut v = ids.to_vec();
    v.sort_unstable();
    v.iter().map(u64::to_string).collect::<Vec<_>>().join(",")
}

pub(crate) fn parse_ids(s: &str) -> Vec<u64> {
    s.split(',').filter_map(|p| p.trim().parse().ok()).collect()
}

/// The value a party leader publishes on Seamless Co-op's lobby.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QValue {
    pub lobby: u64,
    pub mode: Mode,
    pub size: u32,
}

pub(crate) fn encode_q(q: &QValue) -> String {
    format!("{}:{}:{}", q.lobby, q.mode.label(), q.size)
}

/// `None` for an empty value (cleared) or anything malformed.
pub(crate) fn parse_q(s: &str) -> Option<QValue> {
    let mut it = s.trim().split(':');
    let lobby: u64 = it.next()?.parse().ok().filter(|&l| l != 0)?;
    let mode = Mode::parse(it.next()?)?;
    let size: u32 = it.next()?.parse().ok().filter(|&s| s != 0)?;
    it.next().is_none().then_some(QValue { lobby, mode, size })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags() -> Tags {
        Tags {
            mode: Mode::Solo,
            kind: KIND_BR,
            game: "1.3.3.0".into(),
            sc: "1.1.3".into(),
            need: 2,
            room: "public".into(),
        }
    }

    #[test]
    fn tags_cover_the_required_keys_and_filters_match_them() {
        let t = tag_pairs(&tags());
        let keys: Vec<&str> = t.iter().map(|p| p.0.as_str()).collect();
        assert_eq!(
            keys,
            vec!["nrpvp", "mode", "kind", "proto", "game", "sc", "need", "room"]
        );
        assert_eq!(t[0].1, "1");
        assert_eq!(t[1].1, "solo");
        assert_eq!(t[2].1, "br");
        assert_eq!(t[3].1, "4");
        assert_eq!(t[7].1, "public");
        let f = search_filters(&tags(), 1);
        assert_eq!(f.strings, t);
        assert_eq!(f.slots, 1);
    }

    #[test]
    fn roster_round_trips_and_is_sorted() {
        let r = [
            RosterEntry {
                id: 30,
                party: 30,
                team: 1,
            },
            RosterEntry {
                id: 10,
                party: 10,
                team: 0,
            },
            RosterEntry {
                id: 20,
                party: 10,
                team: 0,
            },
        ];
        let text = encode_roster(&r);
        assert_eq!(text, "10:10:0,20:10:0,30:30:1");
        assert_eq!(parse_roster(&text).len(), 3);
        assert_eq!(encode_roster(&parse_roster(&text)), text);
        assert!(parse_roster("").is_empty());
        assert_eq!(
            parse_roster("1:2:0,junk,3:x:1,4:5:2,6:7,8:9:1:2"),
            vec![
                RosterEntry {
                    id: 1,
                    party: 2,
                    team: 0
                },
                RosterEntry {
                    id: 4,
                    party: 5,
                    team: 2
                }
            ]
        );
    }

    #[test]
    fn solo_tickets_are_complete_at_once() {
        let m = [(1, Some((1, 1))), (2, Some((2, 1)))];
        let r = tickets(&m);
        assert_eq!(r.complete_players, 2);
        assert!(r.incomplete_parties.is_empty() && r.unknown == 0);
        assert_eq!(encode_roster(&r.roster), "1:1:0,2:2:0");
        assert_eq!(r.parties, vec![(1, 1), (2, 1)]);
    }

    #[test]
    fn a_duo_ticket_needs_both_members() {
        let one = [(1, Some((1, 2)))];
        let r = tickets(&one);
        assert_eq!(
            (r.complete_players, r.incomplete_parties.clone()),
            (0, vec![1])
        );
        // The room for the missing member stays reserved.
        assert_eq!(r.parties, vec![(1, 2)]);
        let both = [(1, Some((1, 2))), (2, Some((1, 2)))];
        assert_eq!(tickets(&both).complete_players, 2);
        assert_eq!(tickets(&both).complete_tickets(), vec![(1, 2)]);
        // Two duos plus an unpublished member.
        let m = [
            (1, Some((1, 2))),
            (2, Some((1, 2))),
            (3, Some((3, 2))),
            (4, Some((3, 2))),
            (5, None),
        ];
        let r = tickets(&m);
        assert_eq!((r.complete_players, r.unknown), (4, 1));
    }

    #[test]
    fn ticket_text_is_validated() {
        assert_eq!(parse_ticket("123", "2"), Some((123, 2)));
        assert_eq!(parse_ticket("", "2"), None);
        assert_eq!(parse_ticket("0", "2"), None);
        assert_eq!(parse_ticket("5", "0"), None);
        assert_eq!(parse_ticket("5", "x"), None);
    }

    #[test]
    fn present_and_id_lists_round_trip() {
        assert_eq!(encode_present(1, 2), "1/2");
        assert_eq!(parse_present("2/2"), Some((2, 2)));
        for bad in ["", "3/2", "0/0", "a/b", "1"] {
            assert_eq!(parse_present(bad), None, "{bad:?}");
        }
        assert_eq!(encode_ids(&[9, 3]), "3,9");
        assert_eq!(parse_ids("3, 9,x"), vec![3, 9]);
        assert!(parse_ids("").is_empty());
    }

    #[test]
    fn queue_value_round_trips_and_rejects_junk() {
        let q = QValue {
            lobby: 109775241234567890,
            mode: Mode::Duo,
            size: 2,
        };
        assert_eq!(encode_q(&q), "109775241234567890:duo:2");
        assert_eq!(parse_q(&encode_q(&q)), Some(q));
        for bad in [
            "",
            "0:duo:2",
            "5:trio:2",
            "5:duo:0",
            "5:duo",
            "5:duo:2:x",
            "x:solo:1",
        ] {
            assert_eq!(parse_q(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_private_code_makes_its_own_room_and_never_shows_in_the_lobby() {
        assert_eq!(room_tag(""), "public");
        let a = room_tag("night-owls");
        assert!(a.starts_with("p-") && a.len() == 18 && !a.contains("night"));
        assert_eq!(a, room_tag("night-owls"));
        assert_ne!(a, room_tag("night-owl"));
        assert_ne!(a, "public");
    }
}
