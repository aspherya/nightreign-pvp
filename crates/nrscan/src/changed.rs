//! `nrscan bar --changed` (T-038): find words that change when (and only when) a hit lands.
//!
//! Phase 1 (snapshot): raw bytes of the near regions at the DOWN. Every later full sample compares against the previous one
//! (a changed-word bitmap per chunk). Samples that precede any hit mark their changed words as *noisy*. The first sample(s)
//! after a hit seed the candidate list: changed and not noisy. Phase 2: the candidates (addresses only) are re-read every
//! interval and scored on how often they changed in hit groups vs. non-hit samples.
//!
//! Hit groups: a hit that lands while a sample is being read may show up in that sample or the next one, so the two samples
//! form one group; a word "responds" to a group if it changed in either of them. Read-only.

#![allow(
    clippy::too_many_arguments,
    clippy::chunks_exact_to_as_chunks,
    clippy::needless_range_loop
)]

use crate::mem::{read_u32, read_u64};
use crate::scan::{chunk_jobs, reread, threads, Cand, Say};
use crate::winmem::{Proc, Region};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ------------------------------------------------------------------ pure correlation logic

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    NonHit,
    Group(u16),
}

/// Assigns each sample (read interval `[s, e]`, seconds) a role from the hit timestamps.
#[derive(Default, Debug)]
pub struct RoleTracker {
    pub gid: u16,
    follow: bool,
}

impl RoleTracker {
    /// `pending`: hit times not yet consumed (ascending); those <= `e` are consumed.
    pub fn next(&mut self, s: f64, e: f64, pending: &mut Vec<f64>) -> Role {
        let n = pending.iter().take_while(|t| **t <= e).count();
        let taken: Vec<f64> = pending.drain(..n).collect();
        if !taken.is_empty() {
            let mid = taken.iter().any(|t| *t >= s);
            if !self.follow {
                self.gid += 1;
            }
            self.follow = mid;
            Role::Group(self.gid)
        } else if self.follow {
            self.follow = false;
            Role::Group(self.gid)
        } else {
            Role::NonHit
        }
    }

    /// True while the current group may still continue into the next sample.
    pub fn group_open(&self) -> bool {
        self.follow
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Corr {
    pub groups_changed: u16,
    counted_gid: u16,
    pub nonhit_changed: u16,
}

impl Corr {
    pub fn feed(&mut self, role: Role, changed: bool) {
        if !changed {
            return;
        }
        match role {
            Role::NonHit => self.nonhit_changed += 1,
            Role::Group(g) => {
                if self.counted_gid != g {
                    self.counted_gid = g;
                    self.groups_changed += 1;
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Totals {
    pub groups: u16,
    pub nonhit: u16,
}

pub const MIN_HIT_RATIO: f64 = 0.6;
pub const MAX_NONHIT_RATIO: f64 = 0.1;

/// Final criterion: changed on >= 60% of the hit groups and <= 10% of the non-hit samples.
pub fn passes(c: &Corr, t: &Totals) -> bool {
    t.groups > 0
        && c.groups_changed as f64 >= MIN_HIT_RATIO * t.groups as f64
        && c.nonhit_changed as f64 <= MAX_NONHIT_RATIO * t.nonhit as f64
}

/// Loose running prune so the candidate list stays small.
fn keep_running(c: &Corr, t: &Totals) -> bool {
    !(t.nonhit >= 3 && c.nonhit_changed as f64 > MAX_NONHIT_RATIO * t.nonhit as f64 + 2.0)
        && !(t.groups >= 3 && (c.groups_changed as f64) < 0.4 * t.groups as f64)
}

// ------------------------------------------------------------------ hit sources

#[derive(Default)]
pub struct HitFeed {
    pub times: Mutex<Vec<f64>>,
    pub notes: Mutex<Vec<String>>,
}

impl HitFeed {
    pub fn push(&self, t: f64, note: String) {
        if let Ok(mut v) = self.times.lock() {
            v.push(t);
        }
        if let Ok(mut n) = self.notes.lock() {
            if n.len() < 300 {
                n.push(format!("{t:.2}s {note}"));
            }
        }
    }
}

/// Polls `PlayerIns+0x198` (current attacker handle: all-ones -> attacker handle on a hit, TESTING run 7), `+0x1A0` (last
/// attacker) and optionally the authoritative HP `[PlayerIns+0xB78]+0x70` of every PlayerIns in the log (NR 1.3.3.0).
pub fn spawn_mem_hits(
    pid: u32,
    players: Vec<u64>,
    with_hp: bool,
    t0: Instant,
    feed: Arc<HitFeed>,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let Ok(proc) = Proc::open(pid) else { return };
        let read = |p: u64| -> Option<(u64, u64, u32)> {
            // SAFETY-equivalent: all reads go through ReadProcessMemory and fail soft.
            let a = read_u64(&proc, p + 0x198)?;
            let b = read_u64(&proc, p + 0x1A0)?;
            let hp = if with_hp {
                read_u64(&proc, p + 0xB78)
                    .filter(|x| *x > 0x1_0000)
                    .and_then(|x| read_u32(&proc, x + 0x70))
                    .unwrap_or(0)
            } else {
                0
            };
            Some((a, b, hp))
        };
        let mut prev: Vec<Option<(u64, u64, u32)>> = players.iter().map(|p| read(*p)).collect();
        let mut last_hit = -1.0f64;
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(15));
            let now = t0.elapsed().as_secs_f64();
            for (i, p) in players.iter().enumerate() {
                let cur = read(*p);
                if let (Some(o), Some(c)) = (prev[i], cur) {
                    let hit198 = c.0 != o.0 && c.0 != u64::MAX;
                    let hit1a0 = c.1 != o.1 && c.1 != u64::MAX;
                    let hithp = with_hp && c.2 != o.2;
                    if (hit198 || hit1a0 || hithp) && now - last_hit > 0.15 {
                        last_hit = now;
                        feed.push(
                            now,
                            format!(
                                "memory hit on PlayerIns {p:#X}: +0x198 {:#X}->{:#X}, +0x1A0 {:#X}->{:#X}, hp {}->{}",
                                o.0, c.0, o.1, c.1, o.2, c.2
                            ),
                        );
                    }
                }
                prev[i] = cur;
            }
        }
    })
}

pub fn spawn_keyboard_hits(t0: Instant, feed: Arc<HitFeed>) {
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match std::io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => feed.push(t0.elapsed().as_secs_f64(), "keyboard hit".into()),
            }
        }
    });
}

// ------------------------------------------------------------------ snapshot machinery

struct Snap {
    base: u64,
    len: usize,
    data: Vec<u8>,
    ok: Vec<bool>,
    changed: Vec<u64>,
    noisy: Vec<u64>,
    acc: Vec<u64>,
}

fn read_chunk(proc: &Proc, base: u64, len: usize, buf: &mut Vec<u8>, ok: &mut Vec<bool>) {
    buf.resize(len, 0);
    let pages = len.div_ceil(4096);
    ok.clear();
    ok.resize(pages, false);
    if proc.read(base, &mut buf[..len]) {
        ok.fill(true);
        return;
    }
    for (pi, page) in buf[..len].chunks_mut(4096).enumerate() {
        ok[pi] = proc.read(base + (pi as u64) * 4096, page);
    }
}

fn par_snaps<F>(snaps: &mut [Snap], f: F)
where
    F: Fn(&mut Snap, &mut Vec<u8>, &mut Vec<bool>) + Sync,
{
    let queue = Mutex::new(snaps.iter_mut());
    std::thread::scope(|sc| {
        for _ in 0..threads() {
            sc.spawn(|| {
                let mut buf: Vec<u8> = Vec::new();
                let mut ok: Vec<bool> = Vec::new();
                loop {
                    let next = queue.lock().ok().and_then(|mut q| q.next());
                    let Some(s) = next else { break };
                    f(s, &mut buf, &mut ok);
                }
            });
        }
    });
}

fn take_snapshot(proc: &Proc, regions: &[Region]) -> Vec<Snap> {
    let mut snaps: Vec<Snap> = chunk_jobs(regions)
        .into_iter()
        .map(|(base, len)| {
            let words = len / 4;
            let bm = words.div_ceil(64);
            Snap {
                base,
                len,
                data: Vec::new(),
                ok: Vec::new(),
                changed: vec![0; bm],
                noisy: vec![0; bm],
                acc: vec![0; bm],
            }
        })
        .collect();
    par_snaps(&mut snaps, |s, _, _| {
        let mut data = Vec::new();
        let mut ok = Vec::new();
        read_chunk(proc, s.base, s.len, &mut data, &mut ok);
        s.data = data;
        s.ok = ok;
    });
    snaps
}

/// Read the regions again, fill `changed` bitmaps, and replace the stored bytes with the fresh ones.
fn compare_sample(proc: &Proc, snaps: &mut [Snap]) {
    par_snaps(snaps, |s, buf, ok| {
        read_chunk(proc, s.base, s.len, buf, ok);
        s.changed.iter_mut().for_each(|w| *w = 0);
        for p in 0..s.ok.len() {
            if !(s.ok[p] && ok[p]) {
                continue;
            }
            let (a, b) = (p * 4096, ((p + 1) * 4096).min(s.len));
            if s.data[a..b] == buf[a..b] {
                continue;
            }
            for (k, (o, n)) in s.data[a..b]
                .chunks_exact(4)
                .zip(buf[a..b].chunks_exact(4))
                .enumerate()
            {
                if o != n {
                    let w = (a / 4) + k;
                    s.changed[w / 64] |= 1u64 << (w % 64);
                }
            }
        }
        std::mem::swap(&mut s.data, buf);
        std::mem::swap(&mut s.ok, ok);
    });
}

fn bit_addrs(snaps: &[Snap], pick: impl Fn(&Snap, usize) -> u64) -> Vec<u64> {
    let mut out = Vec::new();
    for s in snaps {
        for wi in 0..s.changed.len() {
            let mut w = pick(s, wi);
            while w != 0 {
                let b = w.trailing_zeros() as usize;
                w &= w - 1;
                let word = wi * 64 + b;
                if word < s.len / 4 {
                    out.push(s.base + (word as u64) * 4);
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------------ driver

pub struct Row {
    pub addr: u64,
    pub corr: Corr,
    pub hist: Vec<u32>,
}

pub struct ChangedOut {
    pub rows: Vec<Row>,
    pub totals: Totals,
    pub times: Vec<f32>,
    pub roles: Vec<(f32, Role)>,
    pub snapshot_bytes: u64,
    pub seeded: usize,
    pub note: Option<String>,
}

struct Cand2 {
    addr: u64,
    corr: Corr,
    hist: Vec<u32>,
}

fn sleep_stop(ms: u64, stop: &mut dyn FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        if stop() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    stop()
}

fn drain_feed(feed: &HitFeed, pend: &mut Vec<f64>) {
    if let Ok(mut v) = feed.times.lock() {
        pend.append(&mut v);
    }
    pend.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}

pub const SEED_CAP: usize = 8_000_000;

pub fn changed_scan(
    proc: &Proc,
    regions: &[Region],
    interval_ms: u64,
    t0: Instant,
    feed: &HitFeed,
    stop: &mut dyn FnMut() -> bool,
    say: Say,
) -> ChangedOut {
    let mut out = ChangedOut {
        rows: Vec::new(),
        totals: Totals::default(),
        times: Vec::new(),
        roles: Vec::new(),
        snapshot_bytes: 0,
        seeded: 0,
        note: None,
    };
    let st = Instant::now();
    let mut snaps = take_snapshot(proc, regions);
    out.snapshot_bytes = snaps.iter().map(|s| s.len as u64).sum();
    say(&format!(
        "snapshot: {} MB in {:.2}s",
        out.snapshot_bytes >> 20,
        st.elapsed().as_secs_f64()
    ));
    let mut tracker = RoleTracker::default();
    let mut pend: Vec<f64> = Vec::new();
    let mut sample = 0usize;
    // ---- phase 1: full-memory samples until the first hit group has closed
    let seeds: Vec<u64> = loop {
        if sleep_stop(interval_ms, stop) {
            out.note = Some("stopped before any hit group completed".into());
            return out;
        }
        sample += 1;
        let s = t0.elapsed().as_secs_f64();
        compare_sample(proc, &mut snaps);
        let e = t0.elapsed().as_secs_f64();
        drain_feed(feed, &mut pend);
        let role = tracker.next(s, e, &mut pend);
        out.roles.push((e as f32, role));
        let nchanged: u32 = snaps
            .iter()
            .map(|s| s.changed.iter().map(|w| w.count_ones()).sum::<u32>())
            .sum();
        match role {
            Role::NonHit => {
                out.totals.nonhit += 1;
                for s in &mut snaps {
                    for (n, c) in s.noisy.iter_mut().zip(&s.changed) {
                        *n |= *c;
                    }
                }
                say(&format!(
                    "  sample {sample} (non-hit, {:.1}s): {nchanged} word(s) changed -> marked noisy",
                    e
                ));
            }
            Role::Group(g) => {
                out.totals.groups = g;
                for s in &mut snaps {
                    for (a, c) in s.acc.iter_mut().zip(&s.changed) {
                        *a |= *c;
                    }
                }
                say(&format!(
                    "  sample {sample} (HIT group {g}, {:.1}s): {nchanged} word(s) changed",
                    e
                ));
                if !tracker.group_open() {
                    let n: usize = snaps
                        .iter()
                        .map(|s| {
                            s.acc
                                .iter()
                                .zip(&s.noisy)
                                .map(|(a, n)| (a & !n).count_ones() as usize)
                                .sum::<usize>()
                        })
                        .sum();
                    say(&format!("  seeding: {n} changed, not-noisy word(s)"));
                    if n > SEED_CAP {
                        out.note = Some(format!("{n} seed candidates exceeds the cap {SEED_CAP}: more pre-hit samples (wait longer before the first hit), a smaller --near-mb, or a larger --interval-ms needed"));
                        return out;
                    }
                    break bit_addrs(&snaps, |s, wi| s.acc[wi] & !s.noisy[wi]);
                }
            }
        }
    };
    drop(snaps);
    out.seeded = seeds.len();
    if seeds.is_empty() {
        out.note = Some("no word changed in the first hit group".into());
        return out;
    }
    // ---- phase 2: track the seeds
    let blocks: Vec<Vec<Cand>> = to_blocks(seeds.iter().map(|a| Cand {
        addr: *a,
        bits: 0,
        t: 0.0,
    }));
    let first: Vec<Vec<(u32, f32)>> = reread(proc, &blocks, t0, |_, _, v| v.to_vec());
    let mut cands: Vec<Cand2> = Vec::new();
    let g = out.totals.groups;
    let mut t_first = 0.0f32;
    for (a, (bits, t)) in seeds.iter().zip(first.into_iter().flatten()) {
        if t.is_nan() {
            continue;
        }
        t_first = t;
        cands.push(Cand2 {
            addr: *a,
            corr: Corr {
                groups_changed: 1,
                counted_gid: g,
                nonhit_changed: 0,
            },
            hist: vec![bits],
        });
    }
    out.times.push(t_first);
    say(&format!("tracking {} candidate(s)", cands.len()));
    loop {
        if sleep_stop(interval_ms, stop) {
            break;
        }
        sample += 1;
        let blocks = to_blocks(cands.iter().map(|c| Cand {
            addr: c.addr,
            bits: 0,
            t: 0.0,
        }));
        let s = t0.elapsed().as_secs_f64();
        let vals: Vec<Vec<(u32, f32)>> = reread(proc, &blocks, t0, |_, _, v| v.to_vec());
        let e = t0.elapsed().as_secs_f64();
        drain_feed(feed, &mut pend);
        let role = tracker.next(s, e, &mut pend);
        match role {
            Role::NonHit => out.totals.nonhit += 1,
            Role::Group(g) => out.totals.groups = g,
        }
        out.roles.push((e as f32, role));
        out.times.push(e as f32);
        for (c, (bits, t)) in cands.iter_mut().zip(vals.into_iter().flatten()) {
            let last = *c.hist.last().unwrap_or(&0);
            let cur = if t.is_nan() { last } else { bits };
            c.corr.feed(role, cur != last);
            c.hist.push(cur);
        }
        let tot = out.totals;
        cands.retain(|c| keep_running(&c.corr, &tot));
        say(&format!(
            "  sample {sample} ({}, {:.1}s): {} candidate(s) left; hit groups {}, non-hit samples {}",
            match role {
                Role::NonHit => "non-hit".to_string(),
                Role::Group(g) => format!("HIT group {g}"),
            },
            e,
            cands.len(),
            tot.groups,
            tot.nonhit
        ));
        if cands.is_empty() {
            break;
        }
    }
    let tot = out.totals;
    out.rows = cands
        .into_iter()
        .filter(|c| passes(&c.corr, &tot))
        .map(|c| Row {
            addr: c.addr,
            corr: c.corr,
            hist: c.hist,
        })
        .collect();
    out.rows.sort_by(|a, b| {
        b.corr
            .groups_changed
            .cmp(&a.corr.groups_changed)
            .then(a.corr.nonhit_changed.cmp(&b.corr.nonhit_changed))
            .then(a.addr.cmp(&b.addr))
    });
    out
}

fn to_blocks(items: impl Iterator<Item = Cand>) -> Vec<Vec<Cand>> {
    let mut blocks: Vec<Vec<Cand>> = Vec::new();
    for c in items {
        match blocks.last_mut() {
            Some(b) if b.len() < 50_000 => b.push(c),
            _ => blocks.push(vec![c]),
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn roles_from_hit_times() {
        let mut t = RoleTracker::default();
        let mut p = vec![];
        assert_eq!(t.next(0.0, 0.5, &mut p), Role::NonHit);
        // hit at 0.9: before the next read starts (s = 1.0) -> clean single-sample group
        let mut p = vec![0.9];
        assert_eq!(t.next(1.0, 1.5, &mut p), Role::Group(1));
        assert!(!t.group_open());
        assert_eq!(t.next(1.6, 2.0, &mut p), Role::NonHit);
        // hit at 2.3 while the read [2.1, 2.7] is running: that sample and the next form group 2
        let mut p = vec![2.3];
        assert_eq!(t.next(2.1, 2.7, &mut p), Role::Group(2));
        assert!(t.group_open());
        assert_eq!(t.next(2.8, 3.3, &mut p), Role::Group(2));
        assert!(!t.group_open());
        assert_eq!(t.next(3.4, 3.9, &mut p), Role::NonHit);
        // a later hit stays pending until its sample
        let mut p = vec![5.0];
        assert_eq!(t.next(3.9, 4.4, &mut p), Role::NonHit);
        assert_eq!(p, vec![5.0]);
    }

    /// Synthetic series: roles per sample and per-word "changed" flags.
    #[test]
    fn correlation_on_synthetic_series() {
        use Role::*;
        let roles = [
            NonHit,
            NonHit,
            Group(1),
            NonHit,
            Group(2),
            Group(2),
            NonHit,
            NonHit,
            Group(3),
            NonHit,
        ];
        let series: [(&str, [bool; 10]); 4] = [
            // responds to every group, never otherwise
            (
                "bar",
                [
                    false, false, true, false, false, true, false, false, true, false,
                ],
            ),
            // changes all the time
            ("noise", [true; 10]),
            // responds to 1 of 3 groups only
            (
                "rare",
                [
                    false, false, true, false, false, false, false, false, false, false,
                ],
            ),
            // responds to groups but also twice in 7 non-hit samples (> 10%)
            (
                "leaky",
                [
                    true, false, true, false, true, false, true, false, true, false,
                ],
            ),
        ];
        let mut tot = Totals::default();
        for r in roles {
            match r {
                NonHit => tot.nonhit += 1,
                Group(g) => tot.groups = g,
            }
        }
        assert_eq!((tot.groups, tot.nonhit), (3, 6));
        let mut res = Vec::new();
        for (name, ch) in series {
            let mut c = Corr::default();
            for (r, x) in roles.iter().zip(ch) {
                c.feed(*r, x);
            }
            res.push((name, passes(&c, &tot), c.groups_changed, c.nonhit_changed));
        }
        assert_eq!(res[0], ("bar", true, 3, 0));
        assert!(!res[1].1);
        assert_eq!(res[2], ("rare", false, 1, 0));
        assert!(!res[3].1);
    }

    /// End to end in this process: a word that changes only at "hits" and a word that changes constantly.
    #[test]
    fn finds_hit_word_in_own_process() {
        let cell: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));
        let noise: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));
        let addr = cell as *const AtomicU32 as u64;
        let t0 = Instant::now();
        let feed = Arc::new(HitFeed::default());
        let run = Arc::new(AtomicBool::new(true));
        let (f2, r2) = (feed.clone(), run.clone());
        let h = std::thread::spawn(move || {
            let mut n = 0u32;
            let mut next_hit = Instant::now() + Duration::from_millis(1100);
            while r2.load(Ordering::Relaxed) {
                n += 1;
                noise.store(n, Ordering::Relaxed);
                if Instant::now() >= next_hit {
                    cell.fetch_add(1, Ordering::Relaxed);
                    f2.push(t0.elapsed().as_secs_f64(), "test hit".into());
                    next_hit += Duration::from_millis(900);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let proc = Proc::open(std::process::id()).expect("open self");
        let all = proc.regions();
        let regions = crate::filters::clip_regions(&all, &[(addr - (1 << 20), addr + (1 << 20))]);
        let quiet: &(dyn Fn(&str) + Sync) = &|s: &str| println!("{s}");
        let out = changed_scan(
            &proc,
            &regions,
            300,
            t0,
            &feed,
            &mut || t0.elapsed() > Duration::from_secs(7),
            quiet,
        );
        run.store(false, Ordering::Relaxed);
        let _ = h.join();
        println!("totals {:?} rows {}", out.totals, out.rows.len());
        assert!(out.note.is_none(), "{:?}", out.note);
        assert!(
            out.rows.iter().any(|r| r.addr == addr),
            "hit word not found"
        );
        assert!(!out
            .rows
            .iter()
            .any(|r| r.addr == noise as *const AtomicU32 as u64));
    }
}
