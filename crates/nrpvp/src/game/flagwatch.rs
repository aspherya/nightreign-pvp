//! UI-1 step 0 (dev only, `ui_flag_watch = true`): log every event flag that changes, in every flag group that has storage, sampled
//! every [`PERIOD`]. Finds the flags the stock expedition menu sets when the player presses start or cancels matchmaking (the UI-0
//! probe sampled a short list once per second and saw nothing).
//!
//! Read-only, own thread. The `CSEventFlagMan` slot is `exe+0x3C115A8` (game 1.03.3; TC-004 singleton log, UI-0 step 5) and the ER
//! layout of `probe2.rs` holds on NR (UI-0 step 5, all structural checks pass). Bit order inside a block: byte = n / 8, bit = 7 - n % 8
//! (fsrs `EventFlag`). A flag that flips more than [`NOISY_FLIPS`] times in [`NOISY_WINDOW`] is muted (logged once). Log lines start with
//! `ui_flag_watch:`.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use super::mem::RegionCache;
use super::probe2::{
    all_groups, block_addr, checks_ok, parse_layout, structural_checks, EF_HEADER_LEN, NODE_IS_NIL,
    NODE_LEN,
};

use super::flags::RVA_FLAGMAN_SLOT;
const PERIOD: Duration = Duration::from_millis(50);
/// Groups and their block addresses are re-resolved this often (blocks can be reallocated on area loads).
const RESOLVE_EVERY: Duration = Duration::from_secs(2);
const NOISY_WINDOW: Duration = Duration::from_secs(10);
const NOISY_FLIPS: u32 = 20;
const MAX_GROUPS: usize = 4096;

/// Flags whose bit differs between `old` and `new` (same length), as `(flag, now_on)`, in flag order.
pub(crate) fn diff_bits(old: &[u8], new: &[u8], group: u32, divisor: u32) -> Vec<(u32, bool)> {
    let mut out = Vec::new();
    for (i, (a, b)) in old.iter().zip(new).enumerate() {
        let x = a ^ b;
        if x == 0 {
            continue;
        }
        for bit in (0..8u32).rev() {
            if x & (1 << bit) != 0 {
                let n = i as u32 * 8 + (7 - bit);
                out.push((group * divisor + n, b & (1 << bit) != 0));
            }
        }
    }
    out
}

/// Mutes flags that flip too often.
#[derive(Default)]
pub(crate) struct Noise {
    flips: HashMap<u32, (Instant, u32)>,
    muted: HashSet<u32>,
}

impl Noise {
    /// `true` = log this change; `Some(true)` in the second value = the flag was just muted (log that once).
    pub(crate) fn admit(&mut self, flag: u32, now: Instant) -> (bool, bool) {
        if self.muted.contains(&flag) {
            return (false, false);
        }
        let e = self.flips.entry(flag).or_insert((now, 0));
        if now.duration_since(e.0) > NOISY_WINDOW {
            *e = (now, 0);
        }
        e.1 += 1;
        if e.1 > NOISY_FLIPS {
            self.muted.insert(flag);
            return (false, true);
        }
        (true, false)
    }
}

/// Start the watcher thread (developer mode with `ui_flag_watch = true`). `base`: the image base of `nightreign.exe`.
pub(crate) fn spawn(base: usize) {
    let r = std::thread::Builder::new()
        .name("nrpvp-flagwatch".into())
        .spawn(move || {
            if std::panic::catch_unwind(|| run(base)).is_err() {
                tracing::error!("ui_flag_watch: watcher thread panicked; stopped");
            }
        });
    if r.is_err() {
        tracing::warn!("ui_flag_watch: could not spawn the watcher thread");
    }
}

struct Group {
    key: u32,
    addr: usize,
    last: Vec<u8>,
}

fn resolve(rc: &mut RegionCache, inst: usize) -> Option<(u32, Vec<Group>)> {
    let hdr = rc.read_prefix(inst, EF_HEADER_LEN);
    let mut l = parse_layout(&hdr)?;
    l.blocks_readable = rc.readable_len(l.blocks, 1) > 0;
    l.head_readable = rc.readable_len(l.head, NODE_LEN) >= NODE_LEN;
    l.head_is_nil = rc.read::<u8>(l.head.wrapping_add(NODE_IS_NIL));
    if !checks_ok(&structural_checks(&l)) {
        return None;
    }
    let nodes = all_groups(rc, l.head, MAX_GROUPS).ok()?;
    let groups = nodes
        .iter()
        .filter_map(|n| {
            let addr = block_addr(n.mode, n.loc, l.blocks, l.holder_size)?;
            let last = rc.read_prefix(addr, l.holder_size as usize);
            (last.len() == l.holder_size as usize).then_some(Group {
                key: n.key,
                addr,
                last,
            })
        })
        .collect();
    Some((l.divisor, groups))
}

fn run(base: usize) {
    let slot = base + RVA_FLAGMAN_SLOT;
    tracing::info!(
        "ui_flag_watch: started (every {} ms, all flag groups with storage, slot 0x{slot:X})",
        PERIOD.as_millis()
    );
    let mut inst = 0usize;
    let mut divisor = 0u32;
    let mut groups: Vec<Group> = Vec::new();
    let mut next_resolve = Instant::now();
    let mut noise = Noise::default();
    let mut changes: u64 = 0;
    loop {
        std::thread::sleep(PERIOD);
        let mut rc = RegionCache::direct();
        let cur = rc.read::<usize>(slot).unwrap_or(0);
        let now = Instant::now();
        if cur == 0 {
            if inst != 0 {
                tracing::info!(
                    "ui_flag_watch: flag manager gone ({changes} changes logged so far)"
                );
                inst = 0;
                groups.clear();
            }
            continue;
        }
        if cur != inst || now >= next_resolve {
            let fresh = cur != inst;
            match resolve(&mut rc, cur) {
                Some((d, g)) => {
                    if fresh {
                        tracing::info!("ui_flag_watch: flag manager 0x{cur:X}: watching {} groups (divisor {d})", g.len());
                        groups = g;
                    } else {
                        // Keep the previous bytes of groups whose block did not move; take new ones silently.
                        let old: HashMap<u32, Group> =
                            groups.drain(..).map(|x| (x.key, x)).collect();
                        let mut moved = 0;
                        groups = g
                            .into_iter()
                            .map(|n| match old.get(&n.key) {
                                Some(o) if o.addr == n.addr => Group {
                                    last: o.last.clone(),
                                    ..n
                                },
                                _ => {
                                    moved += 1;
                                    n
                                }
                            })
                            .collect();
                        if moved > 0 {
                            tracing::info!("ui_flag_watch: {moved} group(s) new or moved; re-baselined without logging");
                        }
                    }
                    divisor = d;
                    inst = cur;
                }
                None => {
                    if fresh {
                        tracing::warn!(
                            "ui_flag_watch: flag manager 0x{cur:X}: layout checks failed; retrying"
                        );
                    }
                }
            }
            next_resolve = now + RESOLVE_EVERY;
        }
        for g in groups.iter_mut() {
            let b = rc.read_prefix(g.addr, g.last.len());
            if b.len() != g.last.len() || b == g.last {
                continue;
            }
            for (flag, on) in diff_bits(&g.last, &b, g.key, divisor) {
                match noise.admit(flag, now) {
                    (true, _) => {
                        changes += 1;
                        tracing::info!(
                            "ui_flag_watch: flag {flag}: {}",
                            if on { "off -> ON" } else { "ON -> off" }
                        );
                    }
                    (false, true) => {
                        tracing::info!("ui_flag_watch: flag {flag} flips too often; muted")
                    }
                    _ => {}
                }
            }
            g.last = b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_bits_uses_the_fsrs_bit_order() {
        // byte 0 bit 7 = n 0; byte 1 bit 0 = n 15.
        let old = [0u8, 0, 0];
        let new = [0x80u8, 0x01, 0];
        assert_eq!(
            diff_bits(&old, &new, 10_007, 1000),
            vec![(10_007_000, true), (10_007_015, true)]
        );
        assert_eq!(
            diff_bits(&new, &old, 10_007, 1000),
            vec![(10_007_000, false), (10_007_015, false)]
        );
        assert!(diff_bits(&new, &new, 1, 1000).is_empty());
    }

    #[test]
    fn noisy_flags_are_muted_once() {
        let mut n = Noise::default();
        let t = Instant::now();
        for _ in 0..NOISY_FLIPS {
            assert_eq!(n.admit(5, t), (true, false));
        }
        assert_eq!(n.admit(5, t), (false, true));
        assert_eq!(n.admit(5, t), (false, false));
        assert_eq!(n.admit(6, t), (true, false));
    }
}
