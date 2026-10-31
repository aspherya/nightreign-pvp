//! NET-2: the only part of the data channel a feature touches (NETWORKING.md §4). Two mailboxes between the game loop (or any
//! feature thread) and the net worker, so a feature never sees the link, Steam ids or timing:
//!
//! ```ignore
//! // sender side, e.g. when the boss dies:
//! net::bus::publish(net::bus::topic::BOSS_VERDICT, boss_key, last_hitter_team);
//! // receiver side, once per pass of the feature's own loop:
//! for r in net::bus::take(net::bus::topic::BOSS_VERDICT) { /* r.fact.key, r.fact.value, r.from */ }
//! ```
//!
//! Semantics are those of [`super::facts`]: delivered exactly once, resent until acked, first value per key wins. A fact is delivered
//! to the *other* machines only; the publisher already knows its own value. To add a new kind of message, add a `topic` constant here
//! and document what `key` and `value` mean for it; the wire format and the worker do not change.

use std::sync::Mutex;

use super::facts::{Fact, Received};

/// Topic numbers. One owner per number; write down what key and value mean.
pub(crate) mod topic {
    /// RW-5: "the boss `key` was won by team `value`". `key` must be the same number on every machine for the same boss and must
    /// be published by one machine only (the one that saw the kill), see NETWORKING.md §4.
    #[allow(dead_code)] // first user: RW-5 (console `net_verdict` is the test aid until then)
    pub(crate) const BOSS_VERDICT: u16 = 1;
    /// Dev tests (console `net_publish`).
    #[allow(dead_code)] // used by the dev console and tests
    pub(crate) const TEST: u16 = 0xF000;
}

static OUT: Mutex<Vec<Fact>> = Mutex::new(Vec::new());
static IN: Mutex<Vec<Received>> = Mutex::new(Vec::new());
/// More than this waiting means nobody reads them; the oldest go.
const IN_CAP: usize = 1024;
const OUT_CAP: usize = 256;

/// Publish a fact to every other player on the channel. Returns `false` when it was dropped (queue full). Safe to call from any
/// thread at any time, including before the channel is up: the fact waits.
#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn publish(topic: u16, key: u32, value: u32) -> bool {
    let Ok(mut g) = OUT.lock() else { return false };
    if g.len() >= OUT_CAP {
        return false;
    }
    g.push(Fact { topic, key, value });
    true
}

/// The facts of `topic` that arrived since the last call (each fact is returned once, in arrival order).
#[allow(dead_code)] // first reader: RW-5
pub(crate) fn take(topic: u16) -> Vec<Received> {
    let Ok(mut g) = IN.lock() else {
        return Vec::new();
    };
    let (mine, rest): (Vec<_>, Vec<_>) = g.drain(..).partition(|r| r.fact.topic == topic);
    *g = rest;
    mine
}

/// Worker side: facts the features published since the last call.
pub(super) fn drain_out() -> Vec<Fact> {
    OUT.lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default()
}

/// Worker side: hand received facts to the features.
pub(super) fn deliver(items: Vec<Received>) {
    if let Ok(mut g) = IN.lock() {
        g.extend(items);
        let n = g.len();
        if n > IN_CAP {
            g.drain(..n - IN_CAP);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The mailboxes are process-wide statics: one test, so parallel tests cannot see each other's items.
    #[test]
    fn mailboxes_round_trip_and_topics_are_separate() {
        assert!(publish(topic::TEST, 1, 2));
        assert_eq!(
            drain_out(),
            vec![Fact {
                topic: topic::TEST,
                key: 1,
                value: 2
            }]
        );
        assert!(drain_out().is_empty());
        let mk = |t, k| Received {
            from: 9,
            fact: Fact {
                topic: t,
                key: k,
                value: 0,
            },
        };
        deliver(vec![mk(1, 1), mk(2, 2), mk(1, 3)]);
        let a = take(1);
        assert_eq!(a.iter().map(|r| r.fact.key).collect::<Vec<_>>(), [1, 3]);
        assert!(take(1).is_empty());
        assert_eq!(take(2).len(), 1);
        for k in 0..(IN_CAP as u32 + 10) {
            deliver(vec![mk(5, k)]);
        }
        let all = take(5);
        assert_eq!(all.len(), IN_CAP);
        assert_eq!(all[0].fact.key, 10, "the oldest were dropped");
    }
}
