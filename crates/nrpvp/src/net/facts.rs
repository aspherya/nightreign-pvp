//! NET-2: the generic piece features build on (NETWORKING.md §4): **facts**. A fact is `(topic, key) -> value`, three plain numbers a
//! feature chooses ("boss 4012 was won by team 1"). `Facts` makes sure that
//!
//! - a published fact reaches every roster peer, also one that was not connected yet or joins later (it is resent every
//!   [`RESEND`] to each peer that has not answered with an `Ack`; kept for [`KEEP`] or until [`Facts::reset`]),
//! - a received fact is handed to the feature once, however often it arrives,
//! - all machines end with the same value even if two of them published the same key with different values: **the lowest publisher
//!   Steam id wins**. The loser sees the winner's fact arrive (a later fact for a key supersedes the earlier one) and stops resending
//!   its own. Prefer designs where one machine publishes each key anyway.
//!
//! Keys are remembered until `reset` (the worker resets when the lobby is left, i.e. between matches). No Steam and no threads in
//! here: [`super::chan::Channel`] feeds it messages and asks what to send, tests drive it with fake time.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use super::link::SteamId;

/// Resend a pending fact to peers that have not acked it this often.
const RESEND: Duration = Duration::from_millis(500);
/// A published fact is kept (and offered to late joiners) this long; a match is shorter.
const KEEP: Duration = Duration::from_secs(3600);
/// Bounds, so a buggy feature or a hostile roster peer cannot make us grow without limit.
const MAX_PENDING: usize = 256;
const MAX_SETTLED: usize = 4096;

/// What a feature publishes and receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Fact {
    pub(crate) topic: u16,
    pub(crate) key: u32,
    pub(crate) value: u32,
}

/// A fact that arrived from a roster peer and was new (or superseded our value) here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Received {
    pub(crate) from: SteamId,
    pub(crate) fact: Fact,
}

/// What `on_fact` did. Only `Refused` must not be acked (the sender should keep trying).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    New(Received),
    /// Already known (same value) or lost against a lower publisher id.
    Known,
    /// A limit was reached; nothing was recorded.
    Refused,
}

struct Pending {
    fact: Fact,
    acked: BTreeSet<SteamId>,
    since: Instant,
    last_send: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct Facts {
    pending: Vec<Pending>,
    /// Per `(topic, key)`: who published the winning value and the value, whether we published it or received it.
    settled: BTreeMap<(u16, u32), (SteamId, u32)>,
    pub(crate) published: u64,
    pub(crate) received: u64,
    pub(crate) acked: u64,
    pub(crate) duplicates: u64,
    pub(crate) conflicts: u64,
    pub(crate) superseded: u64,
    pub(crate) refused: u64,
    last: Option<Fact>,
}

impl Facts {
    /// Forget every key and every pending fact (a new match).
    pub(crate) fn reset(&mut self) {
        self.pending.clear();
        self.settled.clear();
        self.last = None;
    }

    /// Publish a fact as `own`. `false` when it was not accepted: the key is already settled (a second publish of the same key
    /// changes nothing, even with a different value) or a limit is reached.
    pub(crate) fn publish(&mut self, fact: Fact, own: SteamId, now: Instant) -> bool {
        if self.settled.contains_key(&(fact.topic, fact.key))
            || self.settled.len() >= MAX_SETTLED
            || self.pending.len() >= MAX_PENDING
        {
            self.refused += 1;
            return false;
        }
        self.settled
            .insert((fact.topic, fact.key), (own, fact.value));
        self.pending.push(Pending {
            fact,
            acked: BTreeSet::new(),
            since: now,
            last_send: None,
        });
        self.published += 1;
        true
    }

    /// A fact arrived from `from` (already known to be a roster peer, and `from` published it).
    pub(crate) fn on_fact(&mut self, from: SteamId, fact: Fact) -> Outcome {
        let k = (fact.topic, fact.key);
        match self.settled.get(&k).copied() {
            Some((_, v)) if v == fact.value => {
                self.duplicates += 1;
                Outcome::Known
            }
            Some((origin, _)) if from < origin => {
                // The lower publisher id wins: our value (published by us or received from a higher id) is replaced, and
                // our own pending copy stops being sent.
                self.settled.insert(k, (from, fact.value));
                self.pending
                    .retain(|p| !(p.fact.topic == fact.topic && p.fact.key == fact.key));
                self.superseded += 1;
                self.last = Some(fact);
                Outcome::New(Received { from, fact })
            }
            Some(_) => {
                self.conflicts += 1;
                Outcome::Known
            }
            None if self.settled.len() >= MAX_SETTLED => {
                self.refused += 1;
                Outcome::Refused
            }
            None => {
                self.settled.insert(k, (from, fact.value));
                self.received += 1;
                self.last = Some(fact);
                Outcome::New(Received { from, fact })
            }
        }
    }

    /// `from` has recorded `(topic, key)`.
    pub(crate) fn on_ack(&mut self, from: SteamId, topic: u16, key: u32) {
        for p in &mut self.pending {
            if p.fact.topic == topic && p.fact.key == key && p.acked.insert(from) {
                self.acked += 1;
            }
        }
    }

    /// What to (re)send now: each pending fact to every roster peer that has not acked it, at most every [`RESEND`]. Facts older
    /// than [`KEEP`] are forgotten; fully acked facts are kept, so a peer who joins later still gets them.
    pub(crate) fn due(&mut self, roster: &BTreeSet<SteamId>, now: Instant) -> Vec<(SteamId, Fact)> {
        self.pending.retain(|p| now.duration_since(p.since) < KEEP);
        let mut out = Vec::new();
        for p in &mut self.pending {
            if p.last_send.is_some_and(|t| now.duration_since(t) < RESEND) {
                continue;
            }
            let todo: Vec<_> = roster.iter().filter(|r| !p.acked.contains(r)).collect();
            if todo.is_empty() {
                continue;
            }
            p.last_send = Some(now);
            out.extend(todo.into_iter().map(|&r| (r, p.fact)));
        }
        out
    }

    /// Published facts some roster peer has not acked yet.
    pub(crate) fn unacked(&self, roster: &BTreeSet<SteamId>) -> usize {
        self.pending
            .iter()
            .filter(|p| roster.iter().any(|r| !p.acked.contains(r)))
            .count()
    }

    pub(crate) fn last(&self) -> Option<Fact> {
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(topic: u16, key: u32, value: u32) -> Fact {
        Fact { topic, key, value }
    }
    fn roster(ids: &[SteamId]) -> BTreeSet<SteamId> {
        ids.iter().copied().collect()
    }
    fn is_new(o: Outcome) -> bool {
        matches!(o, Outcome::New(_))
    }

    #[test]
    fn a_received_fact_is_delivered_once_however_often_it_arrives() {
        let mut a = Facts::default();
        assert!(is_new(a.on_fact(7, f(1, 40, 2))));
        assert_eq!(a.on_fact(7, f(1, 40, 2)), Outcome::Known);
        assert_eq!(a.on_fact(7, f(1, 40, 2)), Outcome::Known);
        assert_eq!((a.received, a.duplicates, a.conflicts), (1, 2, 0));
    }

    #[test]
    fn first_value_wins_unless_the_other_publisher_has_the_lower_id() {
        let mut a = Facts::default();
        assert!(is_new(a.on_fact(7, f(1, 40, 2))));
        // Higher id, other value: ignored.
        assert_eq!(a.on_fact(8, f(1, 40, 3)), Outcome::Known);
        assert_eq!(a.conflicts, 1);
        // Lower id, other value: supersedes and is delivered.
        assert!(is_new(a.on_fact(5, f(1, 40, 4))));
        assert_eq!(a.superseded, 1);
        assert_eq!(
            a.on_fact(7, f(1, 40, 2)),
            Outcome::Known,
            "the old value is now a conflict"
        );
        // Our own publish also settles the key; a second publish of it is refused.
        let t = Instant::now();
        assert!(a.publish(f(1, 41, 5), 9, t));
        assert!(!a.publish(f(1, 41, 6), 9, t));
        // Topics are separate namespaces.
        assert!(a.publish(f(2, 41, 6), 9, t));
    }

    #[test]
    fn two_machines_that_publish_the_same_key_end_with_the_lowest_id_value() {
        // Machine 1 (id 1) and machine 2 (id 2) both publish key 40 with different values, then exchange the facts.
        let (mut m1, mut m2) = (Facts::default(), Facts::default());
        let t = Instant::now();
        assert!(m1.publish(f(1, 40, 10), 1, t));
        assert!(m2.publish(f(1, 40, 20), 2, t));
        assert_eq!(
            m1.on_fact(2, f(1, 40, 20)),
            Outcome::Known,
            "id 2 loses against our id 1"
        );
        assert!(
            is_new(m2.on_fact(1, f(1, 40, 10))),
            "id 1 wins at machine 2"
        );
        // The loser stopped resending its own value.
        assert!(m2.due(&roster(&[1]), t).is_empty());
        assert_eq!(m1.due(&roster(&[2]), t), vec![(2, f(1, 40, 10))]);
    }

    #[test]
    fn a_refused_fact_is_reported_so_it_is_not_acked() {
        let mut a = Facts::default();
        for k in 0..MAX_SETTLED as u32 {
            assert!(is_new(a.on_fact(7, f(1, k, 0))));
        }
        assert_eq!(a.on_fact(7, f(1, 99_999, 0)), Outcome::Refused);
        assert_eq!(a.refused, 1);
        // A known key is still answered normally at the limit.
        assert_eq!(a.on_fact(7, f(1, 3, 0)), Outcome::Known);
    }

    #[test]
    fn a_fact_is_resent_until_every_peer_acked_and_then_stays_for_late_joiners() {
        let mut a = Facts::default();
        let t0 = Instant::now();
        let r = roster(&[10, 11]);
        assert!(a.publish(f(1, 40, 2), 1, t0));
        assert_eq!(a.due(&r, t0).len(), 2);
        assert!(a.due(&r, t0 + Duration::from_millis(100)).is_empty());
        a.on_ack(10, 1, 40);
        assert_eq!(
            a.due(&r, t0 + Duration::from_millis(600)),
            vec![(11, f(1, 40, 2))]
        );
        a.on_ack(11, 1, 40);
        a.on_ack(11, 1, 40); // a repeated ack is counted once
        assert_eq!(a.acked, 2);
        assert_eq!(a.unacked(&r), 0);
        assert!(a.due(&r, t0 + Duration::from_secs(2)).is_empty());
        // A peer who joins later still gets it.
        let r3 = roster(&[10, 11, 12]);
        assert_eq!(a.unacked(&r3), 1);
        assert_eq!(
            a.due(&r3, t0 + Duration::from_secs(3)),
            vec![(12, f(1, 40, 2))]
        );
    }

    #[test]
    fn a_fact_waits_with_nobody_on_the_roster_and_expires_after_keep() {
        let mut a = Facts::default();
        let t0 = Instant::now();
        assert!(a.publish(f(1, 40, 2), 1, t0));
        assert!(a.due(&roster(&[]), t0).is_empty());
        assert_eq!(a.unacked(&roster(&[])), 0);
        let d = a.due(&roster(&[10]), t0 + Duration::from_secs(5));
        assert_eq!(d, vec![(10, f(1, 40, 2))]);
        assert!(a
            .due(&roster(&[10]), t0 + KEEP + Duration::from_secs(1))
            .is_empty());
    }

    #[test]
    fn reset_makes_keys_usable_again_for_the_next_match() {
        let mut a = Facts::default();
        let t = Instant::now();
        assert!(a.publish(f(1, 4012, 1), 1, t));
        assert!(is_new(a.on_fact(5, f(1, 7, 3))));
        assert!(!a.publish(f(1, 4012, 2), 1, t));
        a.reset();
        assert!(a.publish(f(1, 4012, 2), 1, t), "same key, next match");
        assert!(is_new(a.on_fact(5, f(1, 7, 3))));
        assert!(a.last().is_some());
    }

    #[test]
    fn publish_limit_holds() {
        let mut a = Facts::default();
        let t = Instant::now();
        for k in 0..MAX_PENDING as u32 {
            assert!(a.publish(f(1, k, 0), 1, t));
        }
        assert!(!a.publish(f(1, 99_999, 0), 1, t));
        assert_eq!(a.refused, 1);
    }
}
