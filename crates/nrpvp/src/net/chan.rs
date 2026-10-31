//! NET-1: the logic of the data channel, independent of Steam: a roster of peers, a heartbeat, ping/pong with round-trip times and the
//! counters `net_status` prints. It only talks to a [`PeerLink`] and is driven by `tick` (the worker thread calls it ~20 times a second).

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use super::facts::{Fact, Facts, Received};
use super::link::{PeerLink, SteamId};
use super::msg::Msg;

const HEARTBEAT: Duration = Duration::from_secs(1);
/// Pings still waiting for their pong are forgotten after this long.
const PING_KEEP: Duration = Duration::from_secs(30);

pub(crate) struct Channel<L: PeerLink> {
    link: L,
    build: u16,
    roster: BTreeSet<SteamId>,
    last_hello: Option<Instant>,
    next_seq: u32,
    waiting: BTreeMap<(SteamId, u32), Instant>,
    /// Pings still to send: (peer, how many left, next due).
    burst: Vec<(SteamId, u32, Instant)>,
    gap: Duration,
    facts: Facts,
    /// Facts that arrived and were new; the owner takes them with [`Channel::take_received`].
    inbox: Vec<Received>,
    pub(crate) sent: u64,
    pub(crate) received: u64,
    pub(crate) pongs: u64,
    pub(crate) send_errors: u64,
    pub(crate) from_strangers: u64,
    last_rtt_ms: Option<f32>,
    last_error: Option<String>,
}

impl<L: PeerLink> Channel<L> {
    pub(crate) fn new(link: L, build: u16) -> Self {
        Channel {
            link,
            build,
            roster: BTreeSet::new(),
            last_hello: None,
            next_seq: 1,
            waiting: BTreeMap::new(),
            burst: Vec::new(),
            gap: Duration::from_millis(100),
            facts: Facts::default(),
            inbox: Vec::new(),
            sent: 0,
            received: 0,
            pongs: 0,
            send_errors: 0,
            from_strangers: 0,
            last_rtt_ms: None,
            last_error: None,
        }
    }

    pub(crate) fn own_id(&self) -> SteamId {
        self.link.own_id()
    }

    pub(crate) fn roster(&self) -> &BTreeSet<SteamId> {
        &self.roster
    }

    #[cfg(test)]
    pub(crate) fn add_peer(&mut self, id: SteamId) -> bool {
        id != 0 && id != self.link.own_id() && self.roster.insert(id)
    }

    /// Replace the roster (own id and zero are dropped). Pending facts stay; they go to whoever is on the roster when they are due.
    pub(crate) fn set_roster(&mut self, ids: &BTreeSet<SteamId>) {
        let own = self.link.own_id();
        self.roster = ids
            .iter()
            .copied()
            .filter(|&i| i != 0 && i != own)
            .collect();
    }

    /// Publish a fact to the roster (see [`Facts`]). `false` when it was refused.
    pub(crate) fn publish(&mut self, fact: Fact, now: Instant) -> bool {
        self.facts.publish(fact, now)
    }

    /// Facts received since the last call, each one once.
    pub(crate) fn take_received(&mut self) -> Vec<Received> {
        std::mem::take(&mut self.inbox)
    }

    /// Queue `count` pings to every roster peer, one every `gap`.
    pub(crate) fn ping_all(&mut self, count: u32, gap: Duration, now: Instant) {
        for &p in &self.roster {
            self.burst.push((p, count, now));
        }
        self.gap = gap;
    }

    fn send(&mut self, to: SteamId, m: &Msg) {
        match self.link.send(to, m) {
            Ok(()) => self.sent += 1,
            Err(e) => {
                self.send_errors += 1;
                self.last_error = Some(e);
            }
        }
    }

    pub(crate) fn tick(&mut self, now: Instant) {
        // Heartbeat: opens the Steam session in both directions without any callback.
        if self
            .last_hello
            .is_none_or(|t| now.duration_since(t) >= HEARTBEAT)
        {
            self.last_hello = Some(now);
            let peers: Vec<_> = self.roster.iter().copied().collect();
            for p in peers {
                self.send(p, &Msg::Hello { build: self.build });
            }
        }
        // Facts: first sends and resends.
        for (to, f) in self.facts.due(&self.roster, now) {
            self.send(
                to,
                &Msg::Fact {
                    topic: f.topic,
                    key: f.key,
                    value: f.value,
                },
            );
        }
        // Ping bursts.
        let mut burst = std::mem::take(&mut self.burst);
        for b in &mut burst {
            while b.1 > 0 && now >= b.2 {
                let seq = self.next_seq;
                self.next_seq = self.next_seq.wrapping_add(1);
                self.waiting.insert((b.0, seq), now);
                self.send(b.0, &Msg::Ping { seq });
                b.1 -= 1;
                b.2 += self.gap;
            }
        }
        burst.retain(|b| b.1 > 0);
        self.burst = burst;
        self.waiting
            .retain(|_, t| now.duration_since(*t) < PING_KEEP);
        // Incoming.
        let mut inbox = Vec::new();
        self.link.poll(&mut inbox);
        for (from, m) in inbox {
            if !self.roster.contains(&from) {
                self.from_strangers += 1;
                continue;
            }
            self.received += 1;
            match m {
                Msg::Hello { .. } => {}
                Msg::Ping { seq } => self.send(from, &Msg::Pong { seq }),
                Msg::Pong { seq } => {
                    if let Some(t) = self.waiting.remove(&(from, seq)) {
                        self.pongs += 1;
                        self.last_rtt_ms = Some(now.duration_since(t).as_secs_f32() * 1000.0);
                    }
                }
                Msg::Fact { topic, key, value } => {
                    if let Some(r) = self.facts.on_fact(from, Fact { topic, key, value }) {
                        self.inbox.push(r);
                    }
                    self.send(from, &Msg::Ack { topic, key });
                }
                Msg::Ack { topic, key } => self.facts.on_ack(from, topic, key),
            }
        }
    }

    pub(crate) fn status(&self) -> String {
        format!(
            "net: id={} peers={:?} sent={} recv={} pongs={} send_errors={} strangers={} undecodable={} last_rtt={} facts_out={} facts_acked={} facts_in={} fact_dups={} fact_conflicts={} fact_pending={}{}{}",
            self.link.own_id(),
            self.roster,
            self.sent,
            self.received,
            self.pongs,
            self.send_errors,
            self.from_strangers,
            self.link.dropped(),
            self.last_rtt_ms
                .map_or("-".to_string(), |r| format!("{r:.1}ms")),
            self.facts.published,
            self.facts.acked,
            self.facts.received,
            self.facts.duplicates,
            self.facts.conflicts,
            self.facts.pending(),
            self.facts.last().map_or(String::new(), |f| format!(
                " last_fact={}:{}:{}",
                f.topic, f.key, f.value
            )),
            self.last_error
                .as_ref()
                .map_or(String::new(), |e| format!(" last_error={e}")),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::link::fake::FakeLink;
    use super::*;

    #[test]
    fn ping_pong_between_two_channels_and_strangers_are_ignored() {
        let (la, lb) = FakeLink::pair(1, 2);
        let mut a = Channel::new(la, 1);
        let mut b = Channel::new(lb, 1);
        assert!(a.add_peer(2));
        assert!(b.add_peer(1));
        assert!(!a.add_peer(1), "own id is not a peer");
        assert!(!a.add_peer(0));
        let t0 = Instant::now();
        a.ping_all(5, Duration::from_millis(10), t0);
        for i in 0..20u64 {
            let now = t0 + Duration::from_millis(10 * i);
            a.tick(now);
            b.tick(now);
        }
        assert_eq!(a.pongs, 5, "{}", a.status());
        assert!(b.received >= 5);
        // b does not know peer 3: its message is counted and ignored.
        let (l3, lb2) = FakeLink::pair(3, 2);
        let mut c = Channel::new(l3, 1);
        let mut b2 = Channel::new(lb2, 1);
        c.add_peer(2);
        c.tick(t0);
        b2.tick(t0);
        assert_eq!(b2.received, 0);
        assert_eq!(b2.from_strangers, 1);
    }

    #[test]
    fn a_pong_nobody_asked_for_is_not_counted() {
        let (la, mut lb) = FakeLink::pair(1, 2);
        let mut a = Channel::new(la, 1);
        a.add_peer(2);
        lb.send(1, &Msg::Pong { seq: 99 }).unwrap();
        a.tick(Instant::now());
        assert_eq!(a.pongs, 0);
        assert_eq!(a.received, 1);
    }

    #[test]
    fn a_fact_crosses_once_is_acked_and_resends_stop() {
        let (la, lb) = FakeLink::pair(1, 2);
        let mut a = Channel::new(la, 1);
        let mut b = Channel::new(lb, 1);
        a.set_roster(&[2, 1, 0].into_iter().collect());
        b.set_roster(&[1].into_iter().collect());
        let t0 = Instant::now();
        let fact = Fact {
            topic: 1,
            key: 40,
            value: 2,
        };
        assert!(a.publish(fact, t0));
        assert!(!a.publish(fact, t0), "same key again is refused");
        for i in 0..40u64 {
            let now = t0 + Duration::from_millis(100 * i);
            a.tick(now);
            b.tick(now);
        }
        let got = b.take_received();
        assert_eq!(got.len(), 1, "{}", b.status());
        assert_eq!((got[0].from, got[0].fact), (1, fact));
        assert!(b.take_received().is_empty());
        assert!(a.status().contains("facts_acked=1"), "{}", a.status());
        assert!(a.status().contains("fact_pending=0"), "{}", a.status());
        assert!(b.status().contains("last_fact=1:40:2"));
    }

    #[test]
    fn a_fact_from_a_stranger_is_not_delivered() {
        let (la, lb) = FakeLink::pair(1, 2);
        let mut a = Channel::new(la, 1);
        let mut b = Channel::new(lb, 1);
        a.set_roster(&[2].into_iter().collect());
        // b has an empty roster: it hears nobody.
        a.publish(
            Fact {
                topic: 1,
                key: 1,
                value: 1,
            },
            Instant::now(),
        );
        a.tick(Instant::now());
        b.tick(Instant::now());
        assert!(b.take_received().is_empty());
        assert_eq!(b.from_strangers, 2, "the hello and the fact");
    }
}
