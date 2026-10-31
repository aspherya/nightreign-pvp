//! NET-2: the generic piece features build on (NETWORKING.md §4): **facts**. A fact is `(topic, key) -> value`, three plain numbers a
//! feature chooses ("boss 4012 was won by team 1"). `Facts` makes sure that
//!
//! - a published fact reaches every roster peer even if the peer was not connected yet (it is resent until the peer answers with an
//!   `Ack`, then forgotten; at most [`KEEP`] in any case),
//! - a received fact is handed to the feature **exactly once**, however often it arrives,
//! - the first value recorded for a `(topic, key)` wins on each machine; a later different value is counted as a conflict and ignored.
//!   Design keys so that one machine publishes each key (for example the one that saw the boss die), or accept first-wins.
//!
//! No Steam and no threads in here: [`super::chan::Channel`] feeds it messages and asks what to send, tests drive it with fake time.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use super::link::SteamId;

/// Resend a pending fact to peers that have not acked it this often.
const RESEND: Duration = Duration::from_millis(500);
/// A fact nobody acked is dropped after this long (bounds traffic when a peer is gone for good).
const KEEP: Duration = Duration::from_secs(120);
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

/// A fact that arrived from a roster peer and was new here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Received {
    pub(crate) from: SteamId,
    pub(crate) fact: Fact,
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
    /// First value recorded per `(topic, key)`, whether we published it or received it.
    settled: BTreeMap<(u16, u32), u32>,
    pub(crate) published: u64,
    pub(crate) received: u64,
    pub(crate) acked: u64,
    pub(crate) duplicates: u64,
    pub(crate) conflicts: u64,
    pub(crate) refused: u64,
    last: Option<Fact>,
}

impl Facts {
    /// Publish a fact. `false` when it was not accepted: the key is already settled (a second publish of the same key changes
    /// nothing, even with a different value) or a limit is reached.
    pub(crate) fn publish(&mut self, fact: Fact, now: Instant) -> bool {
        if self.settled.contains_key(&(fact.topic, fact.key))
            || self.settled.len() >= MAX_SETTLED
            || self.pending.len() >= MAX_PENDING
        {
            self.refused += 1;
            return false;
        }
        self.settled.insert((fact.topic, fact.key), fact.value);
        self.pending.push(Pending {
            fact,
            acked: BTreeSet::new(),
            since: now,
            last_send: None,
        });
        self.published += 1;
        true
    }

    /// A fact arrived from `from` (already known to be a roster peer). Returns it when it is new; the caller answers with an `Ack`
    /// in every case.
    pub(crate) fn on_fact(&mut self, from: SteamId, fact: Fact) -> Option<Received> {
        match self.settled.get(&(fact.topic, fact.key)) {
            Some(&v) if v == fact.value => {
                self.duplicates += 1;
                None
            }
            Some(_) => {
                self.conflicts += 1;
                None
            }
            None if self.settled.len() >= MAX_SETTLED => {
                self.refused += 1;
                None
            }
            None => {
                self.settled.insert((fact.topic, fact.key), fact.value);
                self.received += 1;
                self.last = Some(fact);
                Some(Received { from, fact })
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

    /// What to (re)send now: each pending fact to every roster peer that has not acked it, at most every [`RESEND`]. Forgets facts
    /// that every current peer acked (when there is at least one) and facts older than [`KEEP`].
    pub(crate) fn due(&mut self, roster: &BTreeSet<SteamId>, now: Instant) -> Vec<(SteamId, Fact)> {
        self.pending.retain(|p| {
            now.duration_since(p.since) < KEEP
                && (roster.is_empty() || !roster.iter().all(|r| p.acked.contains(r)))
        });
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

    pub(crate) fn pending(&self) -> usize {
        self.pending.len()
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

    #[test]
    fn a_received_fact_is_delivered_once_however_often_it_arrives() {
        let mut a = Facts::default();
        assert!(a.on_fact(7, f(1, 40, 2)).is_some());
        assert!(a.on_fact(7, f(1, 40, 2)).is_none());
        assert!(a.on_fact(7, f(1, 40, 2)).is_none());
        assert_eq!((a.received, a.duplicates, a.conflicts), (1, 2, 0));
    }

    #[test]
    fn first_value_wins_and_a_different_later_one_is_a_conflict() {
        let mut a = Facts::default();
        assert!(a.on_fact(7, f(1, 40, 2)).is_some());
        assert!(a.on_fact(8, f(1, 40, 3)).is_none());
        assert_eq!(a.conflicts, 1);
        // Our own publish also settles the key.
        let t = Instant::now();
        assert!(a.publish(f(1, 41, 5), t));
        assert!(
            !a.publish(f(1, 41, 6), t),
            "second publish of a key is refused"
        );
        assert!(a.on_fact(7, f(1, 41, 9)).is_none());
        assert_eq!(a.conflicts, 2);
        // Topics are separate namespaces.
        assert!(a.publish(f(2, 41, 6), t));
    }

    #[test]
    fn a_fact_is_resent_until_every_peer_acked_then_forgotten() {
        let mut a = Facts::default();
        let t0 = Instant::now();
        let r = roster(&[10, 11]);
        assert!(a.publish(f(1, 40, 2), t0));
        let d = a.due(&r, t0);
        assert_eq!(d.len(), 2);
        // Too soon for a resend.
        assert!(a.due(&r, t0 + Duration::from_millis(100)).is_empty());
        // One peer acks: only the other one is resent.
        a.on_ack(10, 1, 40);
        let d = a.due(&r, t0 + Duration::from_millis(600));
        assert_eq!(d, vec![(11, f(1, 40, 2))]);
        a.on_ack(11, 1, 40);
        a.on_ack(11, 1, 40); // a repeated ack is counted once
        assert_eq!(a.acked, 2);
        assert!(a.due(&r, t0 + Duration::from_secs(2)).is_empty());
        assert_eq!(a.pending(), 0);
    }

    #[test]
    fn a_fact_waits_for_a_peer_who_joins_later_and_expires_if_nobody_comes() {
        let mut a = Facts::default();
        let t0 = Instant::now();
        assert!(a.publish(f(1, 40, 2), t0));
        assert!(a.due(&roster(&[]), t0).is_empty());
        assert_eq!(a.pending(), 1, "kept while nobody is on the roster");
        let d = a.due(&roster(&[10]), t0 + Duration::from_secs(5));
        assert_eq!(d, vec![(10, f(1, 40, 2))]);
        assert!(a
            .due(&roster(&[10]), t0 + KEEP + Duration::from_secs(1))
            .is_empty());
        assert_eq!(a.pending(), 0);
    }

    #[test]
    fn limits_hold() {
        let mut a = Facts::default();
        let t = Instant::now();
        for k in 0..MAX_PENDING as u32 {
            assert!(a.publish(f(1, k, 0), t));
        }
        assert!(!a.publish(f(1, 99_999, 0), t));
        assert_eq!(a.refused, 1);
    }
}
