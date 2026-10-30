//! NET-1: the logic of the data channel, independent of Steam: a roster of peers, a heartbeat, ping/pong with round-trip times and the
//! counters `net_status` prints. It only talks to a [`PeerLink`] and is driven by `tick` (the worker thread calls it ~20 times a second).

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

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
            sent: 0,
            received: 0,
            pongs: 0,
            send_errors: 0,
            from_strangers: 0,
            last_rtt_ms: None,
            last_error: None,
        }
    }

    pub(crate) fn add_peer(&mut self, id: SteamId) -> bool {
        id != 0 && id != self.link.own_id() && self.roster.insert(id)
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
            }
        }
    }

    pub(crate) fn status(&self) -> String {
        format!(
            "net: id={} peers={:?} sent={} recv={} pongs={} send_errors={} strangers={} undecodable={} last_rtt={}{}",
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
}
