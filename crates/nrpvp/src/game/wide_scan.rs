//! T-034/T-035: downed-timer scan (DEV MODE ONLY, read-only, needs `probes = true`).
//!
//! T-034's version took 520-920 ms per pass on the main loop (FS-0 run 9) and found nothing. T-035 rewrites the sampler:
//! - one guarded bulk copy per region ([`BulkMem::read_region`], backed by `mem::RegionCache`), never per word;
//! - regions: `PlayerIns+0..0x1000`, its one- and two-level pointees (at most [`MAX_REGIONS`] regions of 0x200 bytes), and the
//!   first 0x1000 bytes of a short list of singletons ([`SHORT_LIST`]) plus their one-level pointees (at most
//!   [`SING_MAX_REGIONS`] regions of 0x200 bytes);
//! - the sampling runs on its own thread ([`ScanThread`]) every [`SAMPLE_MS`] ms, so the main loop is never delayed. The main
//!   loop only sends [`ScanMsg`]s (start, marker, stop);
//! - a pass that takes more than [`SLOW_PASS_MS`] ms makes the next ones cheaper ([`Budget::back_off`]: first the two-level
//!   pointees are dropped, then the caps are halved) and says so in the log.
//!
//! Every word is read both as f32 and as u32. Nothing is kept per pass except the values that CHANGED (run-length encoding per
//! location). At the end of the window (LOCAL left the downed state) two rankings are printed, both prefixed
//! `T-035 TIMER UNVERIFIED` (nothing here is verified; the HUMAN compares the candidates with what the game showed). At least
//! [`MIN_SAMPLES`] samples (about 3 s) are needed:
//! (i) steady counters that start moving within [`START_S`] s of the DOWN, ranked by how close the total change over the
//!     window is to the window length in seconds, or to the window length in frames at 30 or 60 fps (the bleed-out clock lasts
//!     as long as the window when nobody revives the player);
//! (ii) values that were constant and then changed in steps that coincide (+-300 ms) with a marker: a change of LOCAL's
//!     last-attacker field or HP, or the moment the downed flag cleared (revive-meter candidates).
//!
//! Memory access is guarded (`RegionCache`, VirtualQuery). No writes, no calls into game functions.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::discover::is_ptr;
use super::mem::RegionCache;
use super::probe_scan::{in_image, PLAYER_LEN, POINTEE_LEN};

pub(crate) const TAG: &str = "T-035 TIMER UNVERIFIED";
/// Sample period of one pass.
pub(crate) const SAMPLE_MS: u64 = 100;
/// A pass slower than this makes the next passes cheaper.
pub(crate) const SLOW_PASS_MS: u64 = 40;
/// Pointee regions (levels 1 and 2 together) read from the PlayerIns per pass.
pub(crate) const MAX_REGIONS: usize = 128;
/// Level-1 pointees of the PlayerIns (the position scan's selection size).
const MAX_L1: usize = 64;
/// Pointee regions read from the singletons per pass (shared by the whole short list).
pub(crate) const SING_MAX_REGIONS: usize = 64;
/// The caps are never halved below this.
const MIN_REGIONS: usize = 8;
/// Bytes read from the start of each short-list singleton instance.
pub(crate) const SING_LEN: usize = 0x1000;
/// A PlayerIns read shorter than this is not a PlayerIns any more.
const MIN_PLAYER_BYTES: usize = 0x400;
/// The singletons sampled (names as printed by the singleton map). Exact match.
pub(crate) const SHORT_LIST: [&str; 7] = [
    "CSTeamSessionDataMan",
    "CSTeamSessionManager",
    "CSRespawnPointMan",
    "BloodStainMan",
    "CSDeathBoxInsMan",
    "CSRebornBuddyMan",
    "WorldChrMan",
];
/// Samples per window (120 s at 100 ms).
pub(crate) const MAX_SAMPLES: usize = 1200;
/// Recorded value changes over all locations.
const CHANGE_CAP: usize = 4_000_000;
/// Distinct locations.
const TRACK_CAP: usize = 3_000_000;
/// A series needs this many samples (about 3 s).
pub(crate) const MIN_SAMPLES: usize = 30;
/// Fraction of steps that must go in the trend direction.
const MIN_MONOTONE: f64 = 0.8;
/// Largest coefficient of variation of the per-second rate.
const MAX_RATE_CV: f64 = 0.35;
/// Entries per ranking.
pub(crate) const TOP: usize = 15;
/// Marker match tolerance for the step ranking.
pub(crate) const MATCH_MS: u64 = 300;
/// A steady counter must start moving this early (seconds after the DOWN).
pub(crate) const START_S: f64 = 0.3;
/// Step candidates change at most this often.
const MAX_STEPS: usize = 12;
/// A step candidate is constant for at least this many samples before its first change.
const MIN_QUIET: u16 = 2;
/// Steps listed per candidate.
const STEPS_SHOWN: usize = 6;
/// Units per second a counter may use: seconds, or frames at 30 / 60 fps.
const WINDOW_SCALES: [f64; 3] = [1.0, 30.0, 60.0];

// ---------------------------------------------------------------------------------------------------
// Locations
// ---------------------------------------------------------------------------------------------------

/// Where a word lives. `kind` 0 = `PlayerIns+c`; 1 = `[PlayerIns+a]+c`; 2 = `[[PlayerIns+a]+b]+c`; 3 = `Singleton<names[a]>+c`;
/// 4 = `[Singleton<names[a]>+b]+c`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct WLoc {
    kind: u8,
    a: u16,
    b: u16,
    c: u16,
}

impl WLoc {
    pub(crate) fn player(off: u16) -> Self {
        WLoc {
            kind: 0,
            a: 0,
            b: 0,
            c: off,
        }
    }
    pub(crate) fn level1(poff: u16, off: u16) -> Self {
        WLoc {
            kind: 1,
            a: poff,
            b: 0,
            c: off,
        }
    }
    pub(crate) fn level2(poff: u16, off1: u16, off: u16) -> Self {
        WLoc {
            kind: 2,
            a: poff,
            b: off1,
            c: off,
        }
    }
    pub(crate) fn singleton(idx: u16, off: u16) -> Self {
        WLoc {
            kind: 3,
            a: idx,
            b: 0,
            c: off,
        }
    }
    pub(crate) fn singleton1(idx: u16, poff: u16, off: u16) -> Self {
        WLoc {
            kind: 4,
            a: idx,
            b: poff,
            c: off,
        }
    }

    fn pack(self) -> u64 {
        (u64::from(self.kind) << 48)
            | (u64::from(self.a) << 32)
            | (u64::from(self.b) << 16)
            | u64::from(self.c)
    }

    fn unpack(k: u64) -> Self {
        WLoc {
            kind: (k >> 48) as u8,
            a: (k >> 32) as u16,
            b: (k >> 16) as u16,
            c: k as u16,
        }
    }

    pub(crate) fn text(&self, names: &[String]) -> String {
        let name = |i: u16| match names.get(usize::from(i)) {
            Some(n) => format!("Singleton<{n}>"),
            None => format!("Singleton#{i}"),
        };
        match self.kind {
            0 => format!("PlayerIns+0x{:X}", self.c),
            1 => format!("[PlayerIns+0x{:X}]+0x{:X}", self.a, self.c),
            2 => format!("[[PlayerIns+0x{:X}]+0x{:X}]+0x{:X}", self.a, self.b, self.c),
            3 => format!("{}+0x{:X}", name(self.a), self.c),
            _ => format!("[{}+0x{:X}]+0x{:X}", name(self.a), self.b, self.c),
        }
    }
}

/// Cheap hasher for the packed location keys (the keys have zero low bits, so the finaliser mixes them).
#[derive(Default, Clone, Copy)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        let mut z = self.0;
        z ^= z >> 30;
        z = z.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z ^= z >> 27;
        z = z.wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(8) ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = v;
    }
}

type Tracks = HashMap<u64, Track, BuildHasherDefault<KeyHasher>>;

// ---------------------------------------------------------------------------------------------------
// Series analysis
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValKind {
    F32,
    U32,
}

impl ValKind {
    fn name(self) -> &'static str {
        match self {
            ValKind::F32 => "f32",
            ValKind::U32 => "u32",
        }
    }
    fn fmt(self, bits: u32) -> String {
        match self {
            ValKind::F32 => format!("{:.3}", f32::from_bits(bits)),
            ValKind::U32 => bits.to_string(),
        }
    }
    fn delta(self, from: u32, to: u32) -> String {
        match self {
            ValKind::F32 => format!("{:+.3}", f32::from_bits(to) - f32::from_bits(from)),
            ValKind::U32 => format!("{:+}", i64::from(to) - i64::from(from)),
        }
    }
}

/// Result of analysing one series.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Analysis {
    pub first: f64,
    pub last: f64,
    /// Mean units per second over the steps that follow the trend.
    pub rate: f64,
    /// Fraction of steps that follow the trend.
    pub monotone: f64,
    /// Coefficient of variation of the per-second rate.
    pub cv: f64,
    pub samples: usize,
}

/// Does `v` (sampled at `t` seconds) move strictly in one direction in at least 80% of its steps at a near constant rate?
pub(crate) fn analyse_series(t: &[f64], v: &[f64]) -> Option<Analysis> {
    let n = v.len().min(t.len());
    if n < MIN_SAMPLES {
        return None;
    }
    let total = v[n - 1] - v[0];
    if total == 0.0 || !total.is_finite() {
        return None;
    }
    let sign = total.signum();
    let mut steps = 0usize;
    let mut with = 0usize;
    let mut rates = Vec::new();
    for i in 1..n {
        let dt = t[i] - t[i - 1];
        if dt <= 0.0 {
            continue;
        }
        steps += 1;
        let d = v[i] - v[i - 1];
        if d * sign > 0.0 {
            with += 1;
            rates.push(d / dt);
        }
    }
    if steps == 0 || rates.is_empty() {
        return None;
    }
    let monotone = with as f64 / steps as f64;
    if monotone < MIN_MONOTONE {
        return None;
    }
    let mean = rates.iter().sum::<f64>() / rates.len() as f64;
    let var = rates.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / rates.len() as f64;
    let cv = var.sqrt() / mean.abs();
    if !cv.is_finite() || cv > MAX_RATE_CV {
        return None;
    }
    Some(Analysis {
        first: v[0],
        last: v[n - 1],
        rate: mean,
        monotone,
        cv,
        samples: n,
    })
}

/// A float reading that is a plausible value (small integers read as f32 are denormals and are left to the u32 reading).
fn float_sane(x: f64) -> bool {
    x.is_finite() && x.abs() < 1.0e9 && (x == 0.0 || x.abs() >= 1.0e-6)
}

/// Steady-rate analysis of raw words, as f32 when every value is a sane float, else as u32.
pub(crate) fn steady(ts: &[f64], bits: &[u32]) -> Option<(ValKind, Analysis)> {
    let f: Vec<f64> = bits.iter().map(|&b| f64::from(f32::from_bits(b))).collect();
    if f.iter().all(|&x| float_sane(x)) {
        if let Some(a) = analyse_series(ts, &f) {
            return Some((ValKind::F32, a));
        }
    }
    let u: Vec<f64> = bits.iter().map(|&b| f64::from(b)).collect();
    analyse_series(ts, &u).map(|a| (ValKind::U32, a))
}

/// How close `|total|` is to the window length `window_s`, counting in seconds, or in frames at 30 or 60 fps. Returns the
/// best (scale, relative error); `None` when the window is empty.
pub(crate) fn window_match(total: f64, window_s: f64) -> Option<(f64, f64)> {
    if window_s.is_nan() || window_s <= 0.0 || !total.is_finite() {
        return None;
    }
    WINDOW_SCALES
        .iter()
        .map(|&s| (s, ((total.abs() / s) - window_s).abs() / window_s))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// The moving part of a series: from the sample before the first change to the last change. `None` for a constant series.
pub(crate) fn moving_range(series: &[u32]) -> Option<(usize, usize)> {
    let first = *series.first()?;
    let first_change = series.iter().position(|&v| v != first)?;
    let last_change = (1..series.len())
        .rev()
        .find(|&i| series[i] != series[i - 1])?;
    Some((first_change.saturating_sub(1), last_change))
}

// ---------------------------------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------------------------------

/// A moment LOCAL's state changed (used by the step ranking).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Marker {
    pub t_ms: u64,
    pub what: &'static str,
}

/// What a pass reports about itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PassInfo {
    pub bytes: usize,
    /// PlayerIns pointee regions read (both levels).
    pub regions: usize,
    /// Singleton pointee regions read.
    pub sing_regions: usize,
    pub sing_live: usize,
    pub sing_sampled: usize,
    pub elapsed_ms: u64,
}

/// One location: first value and the changes after it.
struct Track {
    first_idx: u16,
    last_idx: u16,
    first: u32,
    last: u32,
    changes: Vec<(u16, u32)>,
}

impl Track {
    /// The value at every sample index `first_idx..=last_idx` (a missing sample repeats the previous value).
    fn series(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(usize::from(self.last_idx - self.first_idx) + 1);
        let mut cur = self.first;
        let mut ci = 0;
        for i in self.first_idx..=self.last_idx {
            while ci < self.changes.len() && self.changes[ci].0 <= i {
                cur = self.changes[ci].1;
                ci += 1;
            }
            out.push(cur);
        }
        out
    }
}

/// Samples of one downed window.
#[derive(Default)]
pub(crate) struct WideScan {
    active: bool,
    start_ms: u64,
    next_ms: u64,
    times_ms: Vec<u64>,
    tracks: Tracks,
    changes: usize,
    change_capped: bool,
    track_capped: bool,
    sample_capped: bool,
    cur_idx: u16,
    pass_sum_ms: u64,
    pass_max_ms: u64,
    passes: u64,
}

impl WideScan {
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    pub(crate) fn begin(&mut self, now_ms: u64) {
        *self = WideScan {
            active: true,
            start_ms: now_ms,
            next_ms: now_ms,
            ..WideScan::default()
        };
    }

    pub(crate) fn due(&self, now_ms: u64) -> bool {
        self.active && now_ms >= self.next_ms && !self.sample_capped
    }

    /// When the next pass is due (0 when inactive).
    pub(crate) fn next_due_ms(&self) -> u64 {
        self.next_ms
    }

    /// Start a pass; false when the sample cap was reached.
    pub(crate) fn begin_pass(&mut self, now_ms: u64) -> bool {
        if self.times_ms.len() >= MAX_SAMPLES {
            self.sample_capped = true;
            return false;
        }
        self.next_ms = now_ms + SAMPLE_MS;
        self.cur_idx = u16::try_from(self.times_ms.len()).unwrap_or(u16::MAX);
        self.times_ms.push(now_ms);
        true
    }

    pub(crate) fn record(&mut self, loc: WLoc, bits: u32) {
        let idx = self.cur_idx;
        let WideScan {
            tracks,
            changes,
            change_capped,
            track_capped,
            ..
        } = self;
        match tracks.entry(loc.pack()) {
            Entry::Occupied(e) => {
                let t = e.into_mut();
                t.last_idx = idx;
                if t.last != bits {
                    if *changes < CHANGE_CAP {
                        t.changes.push((idx, bits));
                        *changes += 1;
                    } else {
                        *change_capped = true;
                    }
                    t.last = bits;
                }
            }
            Entry::Vacant(v) => {
                if !*track_capped {
                    v.insert(Track {
                        first_idx: idx,
                        last_idx: idx,
                        first: bits,
                        last: bits,
                        changes: Vec::new(),
                    });
                }
            }
        }
        if self.tracks.len() >= TRACK_CAP {
            self.track_capped = true;
        }
    }

    pub(crate) fn end_pass(&mut self, info: &PassInfo) {
        self.passes += 1;
        self.pass_sum_ms += info.elapsed_ms;
        self.pass_max_ms = self.pass_max_ms.max(info.elapsed_ms);
    }

    /// End the window and rank. `names` are the singleton names (index = `WLoc::singleton` index).
    pub(crate) fn finish(
        &mut self,
        now_ms: u64,
        markers: &[Marker],
        names: &[String],
    ) -> Vec<String> {
        let started = Instant::now();
        self.active = false;
        let n = self.times_ms.len();
        let window_s = now_ms.saturating_sub(self.start_ms) as f64 / 1000.0;
        let mut lines = vec![format!(
            "{TAG}: LOCAL was downed for {window_s:.1} s; {n} samples, {} locations tracked, {} value changes; passes {}: avg {} ms, max {} ms{}{}{}",
            self.tracks.len(),
            self.changes,
            self.passes,
            self.pass_sum_ms.checked_div(self.passes).unwrap_or(0),
            self.pass_max_ms,
            if self.sample_capped { " (sample cap reached)" } else { "" },
            if self.change_capped { " (change cap reached)" } else { "" },
            if self.track_capped { " (location cap reached)" } else { "" },
        )];
        if n < MIN_SAMPLES {
            lines.push(format!(
                "{TAG}: window too short ({n} < {MIN_SAMPLES} samples), no ranking"
            ));
            *self = WideScan::default();
            return lines;
        }
        let t_s: Vec<f64> = self
            .times_ms
            .iter()
            .map(|&m| m.saturating_sub(self.start_ms) as f64 / 1000.0)
            .collect();
        lines.extend(self.rank_steady(&t_s, window_s, names));
        lines.extend(self.rank_steps(markers, names));
        lines.push(format!(
            "{TAG}: ranking took {} ms",
            started.elapsed().as_millis()
        ));
        *self = WideScan::default();
        lines
    }

    /// (i) steady counters that start moving within [`START_S`] of the DOWN, ranked by how close their total change is to the
    /// window length (in seconds, or in frames at 30 / 60 fps).
    fn rank_steady(&self, t_s: &[f64], window_s: f64, names: &[String]) -> Vec<String> {
        struct Hit {
            loc: WLoc,
            kind: ValKind,
            a: Analysis,
            scale: f64,
            err: f64,
            start_s: f64,
            moved_s: f64,
        }
        let mut steady_total = 0usize;
        let mut hits: Vec<Hit> = Vec::new();
        for (&k, tr) in &self.tracks {
            // Cheap pre-filter: a steady counter changes at (almost) every sample.
            if tr.changes.len() + 1 < MIN_SAMPLES * 4 / 5 {
                continue;
            }
            let f = usize::from(tr.first_idx);
            let l = usize::from(tr.last_idx);
            if l >= t_s.len() || f > l {
                continue;
            }
            let bits = tr.series();
            let Some((s, e)) = moving_range(&bits) else {
                continue;
            };
            let (si, ei) = (f + s, f + e);
            let (Some(&t_start), Some(&t_end)) = (t_s.get(si), t_s.get(ei)) else {
                continue;
            };
            if t_start > START_S || e + 1 - s < MIN_SAMPLES {
                continue;
            }
            let Some((kind, a)) = steady(&t_s[si..=ei], &bits[s..=e]) else {
                continue;
            };
            steady_total += 1;
            let Some((scale, err)) = window_match(a.last - a.first, window_s) else {
                continue;
            };
            hits.push(Hit {
                loc: WLoc::unpack(k),
                kind,
                a,
                scale,
                err,
                start_s: t_start,
                moved_s: t_end - t_start,
            });
        }
        hits.sort_by(|x, y| {
            x.err
                .total_cmp(&y.err)
                .then(y.a.monotone.total_cmp(&x.a.monotone))
                .then(x.a.cv.total_cmp(&y.a.cv))
                .then(x.loc.cmp(&y.loc))
        });
        let mut lines = vec![format!(
            "{TAG}: (i) {steady_total} steady counter(s) started moving within {START_S} s of the DOWN; ranked by |total change| vs the {window_s:.1} s window (1x = seconds, 30x / 60x = frames); top {TOP}:"
        )];
        for (i, h) in hits.iter().take(TOP).enumerate() {
            lines.push(format!(
                "{TAG}: (i) #{} {} {} first={} last={} rate={:+.3}/s started={:.1}s moved={:.1}s window={window_s:.1}s match={}x err={:.0}% monotone={:.0}% cv={:.2} samples={}",
                i + 1,
                h.loc.text(names),
                h.kind.name(),
                fmt_f(h.kind, h.a.first),
                fmt_f(h.kind, h.a.last),
                h.a.rate,
                h.start_s,
                h.moved_s,
                h.scale,
                h.err * 100.0,
                h.a.monotone * 100.0,
                h.a.cv,
                h.a.samples
            ));
        }
        lines
    }

    /// (ii) constant values that changed in steps coinciding with a marker.
    fn rank_steps(&self, markers: &[Marker], names: &[String]) -> Vec<String> {
        struct Step {
            t_ms: u64,
            from: u32,
            to: u32,
            marker: Option<&'static str>,
        }
        struct Cand {
            loc: WLoc,
            kind: ValKind,
            first: u32,
            last: u32,
            matched: usize,
            unmatched: usize,
            steps: Vec<Step>,
        }
        let mut cands: Vec<Cand> = Vec::new();
        for (&k, tr) in &self.tracks {
            if tr.changes.is_empty()
                || tr.changes.len() > MAX_STEPS
                || tr.changes[0].0 < tr.first_idx.saturating_add(MIN_QUIET)
            {
                continue;
            }
            let mut steps = Vec::new();
            let mut prev = tr.first;
            for &(idx, to) in &tr.changes {
                let Some(&t_ms) = self.times_ms.get(usize::from(idx)) else {
                    continue;
                };
                let marker = markers
                    .iter()
                    .find(|m| m.t_ms.abs_diff(t_ms) <= MATCH_MS)
                    .map(|m| m.what);
                steps.push(Step {
                    t_ms,
                    from: prev,
                    to,
                    marker,
                });
                prev = to;
            }
            let matched = steps.iter().filter(|s| s.marker.is_some()).count();
            if matched == 0 {
                continue;
            }
            let all_float = std::iter::once(tr.first)
                .chain(tr.changes.iter().map(|c| c.1))
                .all(|b| float_sane(f64::from(f32::from_bits(b))));
            cands.push(Cand {
                loc: WLoc::unpack(k),
                kind: if all_float {
                    ValKind::F32
                } else {
                    ValKind::U32
                },
                first: tr.first,
                last: tr.last,
                matched,
                unmatched: steps.len() - matched,
                steps,
            });
        }
        cands.sort_by(|a, b| {
            b.matched
                .cmp(&a.matched)
                .then(a.unmatched.cmp(&b.unmatched))
                .then(a.loc.cmp(&b.loc))
        });
        let mut lines = vec![format!(
            "{TAG}: (ii) {} constant-then-stepping value(s) with a step within {MATCH_MS} ms of a marker ({} marker(s)); top {TOP}:",
            cands.len(),
            markers.len()
        )];
        for (i, c) in cands.iter().take(TOP).enumerate() {
            let shown: Vec<String> = c
                .steps
                .iter()
                .take(STEPS_SHOWN)
                .map(|s| {
                    format!(
                        "t={:.2}s {}{}",
                        s.t_ms.saturating_sub(self.start_ms) as f64 / 1000.0,
                        c.kind.delta(s.from, s.to),
                        s.marker.map_or(String::new(), |m| format!(" [{m}]"))
                    )
                })
                .collect();
            lines.push(format!(
                "{TAG}: (ii) #{} {} {} first={} last={} steps={} matched={} unmatched={}: {}{}",
                i + 1,
                c.loc.text(names),
                c.kind.name(),
                c.kind.fmt(c.first),
                c.kind.fmt(c.last),
                c.steps.len(),
                c.matched,
                c.unmatched,
                shown.join(", "),
                if c.steps.len() > STEPS_SHOWN {
                    ", ..."
                } else {
                    ""
                }
            ));
        }
        lines
    }
}

fn fmt_f(kind: ValKind, v: f64) -> String {
    match kind {
        ValKind::F32 => format!("{v:.3}"),
        ValKind::U32 => format!("{v:.0}"),
    }
}

// ---------------------------------------------------------------------------------------------------
// Bulk sampler
// ---------------------------------------------------------------------------------------------------

/// Guarded bulk reads. One call = one copy of one region.
pub(crate) trait BulkMem {
    /// The readable prefix of `[addr, addr+len)`: empty when `addr` is unreadable, shorter than `len` when the region ends.
    fn read_region(&mut self, addr: usize, len: usize) -> Vec<u8>;
}

impl BulkMem for RegionCache {
    fn read_region(&mut self, addr: usize, len: usize) -> Vec<u8> {
        // One VirtualQuery per page run (cached for the pass) and one memcpy.
        self.read_prefix(addr, len)
    }
}

/// A singleton of the short list: its name and the address of its static pointer slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Single {
    pub name: String,
    pub slot: usize,
}

/// Which of [`SHORT_LIST`] exist in `all` (name, slot), in short-list order, and which are missing.
pub(crate) fn pick_short_list<'a>(
    all: impl Iterator<Item = (&'a str, usize)> + Clone,
) -> (Vec<Single>, Vec<&'static str>) {
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for name in SHORT_LIST {
        match all.clone().find(|(n, _)| *n == name) {
            Some((n, slot)) => found.push(Single {
                name: n.to_string(),
                slot,
            }),
            None => missing.push(name),
        }
    }
    (found, missing)
}

/// How much one pass reads. Starts full; [`Budget::back_off`] makes it cheaper after a slow pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Budget {
    /// Read the pointees of the PlayerIns pointees too.
    pub two_level: bool,
    /// PlayerIns pointee regions per pass (both levels).
    pub max_regions: usize,
    /// Singleton pointee regions per pass.
    pub sing_regions: usize,
}

impl Budget {
    pub(crate) const FULL: Budget = Budget {
        two_level: true,
        max_regions: MAX_REGIONS,
        sing_regions: SING_MAX_REGIONS,
    };

    /// One step cheaper: first drop the two-level pointees, then halve both caps (never below [`MIN_REGIONS`]). Returns what
    /// was done, or `None` when nothing is left to cut.
    pub(crate) fn back_off(&mut self) -> Option<String> {
        if self.two_level {
            self.two_level = false;
            return Some("two-level pointees dropped".to_string());
        }
        if self.max_regions > MIN_REGIONS || self.sing_regions > MIN_REGIONS {
            self.max_regions = (self.max_regions / 2).max(MIN_REGIONS);
            self.sing_regions = (self.sing_regions / 2).max(MIN_REGIONS);
            return Some(format!(
                "region caps halved to {} (PlayerIns) / {} (singletons)",
                self.max_regions, self.sing_regions
            ));
        }
        None
    }
}

/// What one pass reads.
pub(crate) struct PassReq<'a> {
    pub player: usize,
    pub singles: &'a [Single],
    pub budget: Budget,
}

fn emit(bytes: &[u8], sink: &mut dyn FnMut(WLoc, u32), mk: &dyn Fn(u16) -> WLoc) {
    for (i, c) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let Ok(off) = u16::try_from(i * 4) else { break };
        sink(mk(off), u32::from_le_bytes(*c));
    }
}

/// The 8-aligned pointers in `bytes` that could be heap objects: `is_ptr` and outside the exe image. `(offset, target)`.
fn pointers_of(bytes: &[u8], base: usize, size: usize) -> impl Iterator<Item = (u16, usize)> + '_ {
    (0..bytes.len().saturating_sub(7))
        .step_by(8)
        .filter_map(move |off| {
            let raw: [u8; 8] = bytes.get(off..off + 8)?.try_into().ok()?;
            let q = u64::from_le_bytes(raw) as usize;
            if !is_ptr(q) || in_image(q, base, size) {
                return None;
            }
            Some((u16::try_from(off).ok()?, q))
        })
}

/// One pass: the PlayerIns at `req.player`, its pointees (one copy per region), and the short-list singletons with their
/// one-level pointees. Every word goes to `sink`. Guarded reads only; `elapsed_ms` is measured here.
pub(crate) fn sample_pass(
    mem: &mut dyn BulkMem,
    req: &PassReq<'_>,
    base: usize,
    size: usize,
    sink: &mut dyn FnMut(WLoc, u32),
) -> PassInfo {
    let started = Instant::now();
    let b = req.budget;
    let mut info = PassInfo::default();
    let mut seen: HashSet<usize> = HashSet::new();
    seen.insert(req.player);
    let player = mem.read_region(req.player, PLAYER_LEN);
    if player.len() >= MIN_PLAYER_BYTES {
        emit(&player, sink, &WLoc::player);
        info.bytes += player.len();
        let mut l1: Vec<(u16, Vec<u8>)> = Vec::new();
        for (off, q) in pointers_of(&player, base, size) {
            if l1.len() >= b.max_regions.min(MAX_L1) {
                break;
            }
            if !seen.insert(q) {
                continue;
            }
            let bytes = mem.read_region(q, POINTEE_LEN);
            if bytes.len() < 12 {
                continue;
            }
            emit(&bytes, sink, &|c| WLoc::level1(off, c));
            info.bytes += bytes.len();
            l1.push((off, bytes));
        }
        info.regions = l1.len();
        if b.two_level {
            'outer: for (poff, bytes) in &l1 {
                for (off, q) in pointers_of(bytes, base, size) {
                    if info.regions >= b.max_regions {
                        break 'outer;
                    }
                    if !seen.insert(q) {
                        continue;
                    }
                    let bytes2 = mem.read_region(q, POINTEE_LEN);
                    if bytes2.len() < 12 {
                        continue;
                    }
                    emit(&bytes2, sink, &|c| WLoc::level2(*poff, off, c));
                    info.bytes += bytes2.len();
                    info.regions += 1;
                }
            }
        }
    }
    // The short-list singletons: slot -> instance (8-byte read), first SING_LEN bytes, then a fair share of the shared
    // pointee-region cap each.
    let share = (b.sing_regions / req.singles.len().max(1)).max(1);
    for (i, s) in req.singles.iter().enumerate() {
        let Ok(idx) = u16::try_from(i) else { break };
        let raw: Option<[u8; 8]> = mem.read_region(s.slot, 8).try_into().ok();
        let Some(inst) = raw
            .map(|r| u64::from_le_bytes(r) as usize)
            .filter(|&p| is_ptr(p))
        else {
            continue;
        };
        info.sing_live += 1;
        let bytes = mem.read_region(inst, SING_LEN);
        if bytes.is_empty() {
            continue;
        }
        emit(&bytes, sink, &|c| WLoc::singleton(idx, c));
        info.bytes += bytes.len();
        info.sing_sampled += 1;
        seen.insert(inst);
        let mut own = 0usize;
        for (off, q) in pointers_of(&bytes, base, size) {
            if own >= share || info.sing_regions >= b.sing_regions {
                break;
            }
            if !seen.insert(q) {
                continue;
            }
            let bytes2 = mem.read_region(q, POINTEE_LEN);
            if bytes2.len() < 12 {
                continue;
            }
            emit(&bytes2, sink, &|c| WLoc::singleton1(idx, off, c));
            info.bytes += bytes2.len();
            info.sing_regions += 1;
            own += 1;
        }
    }
    info.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    info
}

// ---------------------------------------------------------------------------------------------------
// Worker (pure state machine) and thread
// ---------------------------------------------------------------------------------------------------

/// Messages from the main loop to the sampling thread.
#[derive(Clone, Debug)]
pub(crate) enum ScanMsg {
    /// LOCAL went down: start a window now.
    Start {
        at_ms: u64,
        player: usize,
        singles: Vec<Single>,
    },
    /// LOCAL's attacker field or HP changed (or the downed flag cleared).
    Marker(Marker),
    /// LOCAL left the downed state: end the window and print the rankings.
    Stop { at_ms: u64 },
}

/// Where the main loop sends [`ScanMsg`]s.
pub(crate) trait ScanSink {
    fn send(&mut self, msg: ScanMsg);
}

/// Runs one pass for the worker.
pub(crate) trait PassRunner {
    fn run(&mut self, req: &PassReq<'_>, sink: &mut dyn FnMut(WLoc, u32)) -> PassInfo;
}

impl<F> PassRunner for F
where
    F: FnMut(&PassReq<'_>, &mut dyn FnMut(WLoc, u32)) -> PassInfo,
{
    fn run(&mut self, req: &PassReq<'_>, sink: &mut dyn FnMut(WLoc, u32)) -> PassInfo {
        self(req, sink)
    }
}

/// The real runner: a fresh `RegionCache` per pass over the live process memory.
pub(crate) struct RealRunner {
    pub base: usize,
    pub size: usize,
}

impl PassRunner for RealRunner {
    fn run(&mut self, req: &PassReq<'_>, sink: &mut dyn FnMut(WLoc, u32)) -> PassInfo {
        let mut rc = RegionCache::new();
        sample_pass(&mut rc, req, self.base, self.size, sink)
    }
}

/// The sampling state machine, driven by explicit clock values (testable without a thread).
pub(crate) struct ScanWorker {
    runner: Box<dyn PassRunner>,
    scan: WideScan,
    budget: Budget,
    player: usize,
    singles: Vec<Single>,
    names: Vec<String>,
    markers: Vec<Marker>,
    passes_logged: u32,
}

impl ScanWorker {
    pub(crate) fn new(runner: Box<dyn PassRunner>) -> Self {
        ScanWorker {
            runner,
            scan: WideScan::default(),
            budget: Budget::FULL,
            player: 0,
            singles: Vec::new(),
            names: Vec::new(),
            markers: Vec::new(),
            passes_logged: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn is_active(&self) -> bool {
        self.scan.is_active()
    }

    #[cfg(test)]
    pub(crate) fn budget(&self) -> Budget {
        self.budget
    }

    /// When the next pass is due; `None` while idle.
    pub(crate) fn next_due_ms(&self) -> Option<u64> {
        self.scan.is_active().then(|| self.scan.next_due_ms())
    }

    /// Drop the current window without ranking (after a panic).
    pub(crate) fn abort(&mut self) {
        self.scan = WideScan::default();
        self.markers.clear();
    }

    pub(crate) fn handle(&mut self, msg: ScanMsg) -> Vec<String> {
        match msg {
            ScanMsg::Start {
                at_ms,
                player,
                singles,
            } => {
                self.scan.begin(at_ms);
                self.budget = Budget::FULL;
                self.player = player;
                self.names = singles.iter().map(|s| s.name.clone()).collect();
                let missing: Vec<&str> = SHORT_LIST
                    .iter()
                    .copied()
                    .filter(|n| !self.names.iter().any(|x| x == n))
                    .collect();
                let line = format!(
                    "{TAG}: LOCAL is downed; sampling every {SAMPLE_MS} ms on a dedicated thread (PlayerIns 0x{PLAYER_LEN:X} + <= {MAX_REGIONS} pointee regions of 0x{POINTEE_LEN:X}; {} short-list singleton(s) of 0x{SING_LEN:X} + <= {SING_MAX_REGIONS} pointee regions; slow-pass limit {SLOW_PASS_MS} ms); singletons: {}{}",
                    self.names.len(),
                    if self.names.is_empty() { "none".to_string() } else { self.names.join(", ") },
                    if missing.is_empty() { String::new() } else { format!("; not in the singleton map: {}", missing.join(", ")) }
                );
                self.singles = singles;
                self.markers.clear();
                self.passes_logged = 0;
                vec![line]
            }
            ScanMsg::Marker(m) => {
                if self.scan.is_active() {
                    self.markers.push(m);
                }
                Vec::new()
            }
            ScanMsg::Stop { at_ms } => {
                if !self.scan.is_active() {
                    return Vec::new();
                }
                self.markers.push(Marker {
                    t_ms: at_ms,
                    what: "downed-clear",
                });
                let mut lines = vec![format!(
                    "{TAG}: LOCAL left the downed state ({} marker(s) recorded; final budget: {})",
                    self.markers.len(),
                    budget_text(&self.budget)
                )];
                lines.extend(self.scan.finish(at_ms, &self.markers, &self.names));
                self.markers.clear();
                lines
            }
        }
    }

    /// Run a pass if one is due.
    pub(crate) fn tick(&mut self, now_ms: u64) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.scan.due(now_ms) || !self.scan.begin_pass(now_ms) {
            return lines;
        }
        let req = PassReq {
            player: self.player,
            singles: &self.singles,
            budget: self.budget,
        };
        let scan = &mut self.scan;
        let info = self.runner.run(&req, &mut |l, b| scan.record(l, b));
        self.scan.end_pass(&info);
        self.passes_logged += 1;
        let slow = info.elapsed_ms > SLOW_PASS_MS;
        if self.passes_logged <= 3 || slow {
            lines.push(format!(
                "{TAG}: pass #{} took {} ms: {} KB, {}+{} pointee regions, {}/{} short-list singletons sampled ({})",
                self.passes_logged,
                info.elapsed_ms,
                info.bytes / 1024,
                info.regions,
                info.sing_regions,
                info.sing_sampled,
                info.sing_live,
                budget_text(&self.budget)
            ));
        }
        if slow {
            match self.budget.back_off() {
                Some(what) => lines.push(format!(
                    "{TAG}: pass took {} ms (> {SLOW_PASS_MS} ms); {what}",
                    info.elapsed_ms
                )),
                None => lines.push(format!(
                    "{TAG}: pass took {} ms (> {SLOW_PASS_MS} ms) and the budget is already at its floor",
                    info.elapsed_ms
                )),
            }
        }
        lines
    }
}

fn budget_text(b: &Budget) -> String {
    format!(
        "{} levels, caps {}/{}",
        if b.two_level { 2 } else { 1 },
        b.max_regions,
        b.sing_regions
    )
}

/// Handle to the sampling thread.
pub(crate) struct ScanThread {
    tx: mpsc::Sender<ScanMsg>,
    dead_logged: bool,
}

impl ScanThread {
    /// Spawn the thread. `epoch` is the clock the main loop's `now_ms` counts from. `None` (logged) if it cannot be spawned.
    pub(crate) fn spawn(base: usize, size: usize, epoch: Instant) -> Option<Self> {
        let (tx, rx) = mpsc::channel::<ScanMsg>();
        let r = std::thread::Builder::new()
            .name("nrpvp-timerscan".into())
            .spawn(move || {
                let mut w = ScanWorker::new(Box::new(RealRunner { base, size }));
                let now = || u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
                loop {
                    let got = match w.next_due_ms() {
                        None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
                        Some(due) => rx.recv_timeout(Duration::from_millis(due.saturating_sub(now()))),
                    };
                    let mut msgs = Vec::new();
                    match got {
                        Ok(m) => {
                            msgs.push(m);
                            while let Ok(m) = rx.try_recv() {
                                msgs.push(m);
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                    for m in msgs {
                        let lines = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            w.handle(m)
                        }));
                        match lines {
                            Ok(lines) => lines.iter().for_each(|l| tracing::info!("{l}")),
                            Err(_) => {
                                tracing::error!("{TAG}: scan thread panicked while handling a message; window dropped");
                                w.abort();
                            }
                        }
                    }
                    let t = now();
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.tick(t))) {
                        Ok(lines) => lines.iter().for_each(|l| tracing::info!("{l}")),
                        Err(_) => {
                            tracing::error!("{TAG}: scan thread panicked during a pass; window dropped");
                            w.abort();
                        }
                    }
                }
            });
        match r {
            Ok(_) => Some(ScanThread {
                tx,
                dead_logged: false,
            }),
            Err(e) => {
                tracing::error!("{TAG}: could not spawn the scan thread: {e}");
                None
            }
        }
    }
}

impl ScanSink for ScanThread {
    fn send(&mut self, msg: ScanMsg) {
        if self.tx.send(msg).is_err() && !self.dead_logged {
            self.dead_logged = true;
            tracing::error!("{TAG}: the scan thread is gone; no more windows will be sampled");
        }
    }
}

/// Used when no scan thread exists (probes off, or the spawn failed).
pub(crate) struct NullSink;

impl ScanSink for NullSink {
    fn send(&mut self, _msg: ScanMsg) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    type Gen = Box<dyn Fn(usize) -> u32>;

    fn drive(n: usize, locs: &[(WLoc, Gen)], start: u64) -> WideScan {
        let mut s = WideScan::default();
        s.begin(start);
        for i in 0..n {
            let t = start + i as u64 * SAMPLE_MS;
            assert!(s.due(t));
            assert!(s.begin_pass(t));
            for (l, f) in locs {
                s.record(*l, f(i));
            }
            s.end_pass(&PassInfo {
                elapsed_ms: 5,
                ..PassInfo::default()
            });
            assert!(!s.due(t + 50));
        }
        s
    }

    #[test]
    fn locations_roundtrip_and_print() {
        let names = vec!["WorldChrMan".to_string()];
        for l in [
            WLoc::player(0x710),
            WLoc::level1(0x5A8, 0x1C8),
            WLoc::level2(0x5A8, 0x40, 0x8),
            WLoc::singleton(0, 0x1F00),
            WLoc::singleton1(0, 0x88, 0x10),
        ] {
            assert_eq!(WLoc::unpack(l.pack()), l);
        }
        assert_eq!(WLoc::player(0x710).text(&names), "PlayerIns+0x710");
        assert_eq!(
            WLoc::level1(0x5A8, 0x1C8).text(&names),
            "[PlayerIns+0x5A8]+0x1C8"
        );
        assert_eq!(
            WLoc::level2(0x5A8, 0x40, 0x8).text(&names),
            "[[PlayerIns+0x5A8]+0x40]+0x8"
        );
        assert_eq!(
            WLoc::singleton(0, 0x20).text(&names),
            "Singleton<WorldChrMan>+0x20"
        );
        assert_eq!(WLoc::singleton(7, 0x20).text(&names), "Singleton#7+0x20");
        assert_eq!(
            WLoc::singleton1(0, 0x88, 0x10).text(&names),
            "[Singleton<WorldChrMan>+0x88]+0x10"
        );
    }

    #[test]
    fn series_expands_run_length_changes() {
        let mut s = WideScan::default();
        s.begin(0);
        let l = WLoc::player(0);
        for (i, v) in [5u32, 5, 5, 9, 9, 7].iter().enumerate() {
            assert!(s.begin_pass(i as u64 * SAMPLE_MS));
            s.record(l, *v);
        }
        let tr = s.tracks.get(&l.pack()).expect("tracked");
        assert_eq!(tr.series(), vec![5, 5, 5, 9, 9, 7]);
        assert_eq!(tr.changes, vec![(3, 9), (5, 7)]);
    }

    #[test]
    fn moving_range_trims_the_constant_ends() {
        assert_eq!(moving_range(&[]), None);
        assert_eq!(moving_range(&[4, 4, 4]), None);
        assert_eq!(moving_range(&[0, 0, 0, 1, 2, 3, 3, 3]), Some((2, 5)));
        assert_eq!(moving_range(&[1, 2, 3]), Some((0, 2)));
        assert_eq!(moving_range(&[5, 6]), Some((0, 1)));
    }

    #[test]
    fn window_match_scores_seconds_and_frames() {
        // 15 s window.
        let (s, e) = window_match(-15.0, 15.0).expect("match");
        assert!((s - 1.0).abs() < 1e-9 && e < 1e-9);
        let (s, e) = window_match(900.0, 15.0).expect("60 fps");
        assert!((s - 60.0).abs() < 1e-9 && e < 1e-9);
        let (s, _) = window_match(450.0, 15.0).expect("30 fps");
        assert!((s - 30.0).abs() < 1e-9);
        // Half the window is a 50% error.
        let (_, e) = window_match(7.5, 15.0).expect("half");
        assert!((e - 0.5).abs() < 1e-9);
        assert!(window_match(5.0, 0.0).is_none());
        assert!(window_match(f64::NAN, 5.0).is_none());
    }

    #[test]
    fn steady_ranking_orders_by_closeness_to_the_window_length() {
        let cd15 = WLoc::player(0x10); // f32 countdown 15 -> 0 over the 15 s window: error 0
        let cd7 = WLoc::player(0x14); // f32 countdown at -0.5/s: total 7.5, 50% off
        let frames = WLoc::level1(0x5A8, 0x20); // u32 frame counter, 900 frames at 60 fps counting down
        let up0 = WLoc::level2(0x1, 0x2, 0x3); // f32 count-up from 0 at +1/s
        let konst = WLoc::player(0x18);
        let late = WLoc::singleton(0, 0x40); // starts moving after 1 s: too late
        let delayed = WLoc::singleton(0, 0x44); // constant for 2 samples, then a countdown: start 0.2 s, still accepted
        let locs: Vec<(WLoc, Gen)> = vec![
            (cd15, Box::new(|i| (15.0f32 - i as f32 * 0.1).to_bits())),
            (cd7, Box::new(|i| (7.5f32 - i as f32 * 0.05).to_bits())),
            (frames, Box::new(|i| 900 - (i as u32) * 6)),
            (up0, Box::new(|i| (i as f32 * 0.1).to_bits())),
            (konst, Box::new(|_| 77)),
            (
                late,
                Box::new(|i| (60.0f32 - i.saturating_sub(10) as f32 * 0.1).to_bits()),
            ),
            (
                delayed,
                Box::new(|i| (15.0f32 - i.saturating_sub(2) as f32 * 0.1).to_bits()),
            ),
        ];
        // 150 samples = 15 s.
        let mut s = drive(150, &locs, 10_000);
        let lines = s.finish(10_000 + 15_000, &[], &["WorldChrMan".to_string()]);
        let top: Vec<&String> = lines.iter().filter(|l| l.contains("(i) #")).collect();
        assert!(
            lines[0].contains("downed for 15.0 s; 150 samples") && lines[0].contains(TAG),
            "{lines:?}"
        );
        let pos = |needle: &str| top.iter().position(|l| l.contains(needle));
        assert!(pos("PlayerIns+0x10 f32").is_some(), "{lines:?}");
        assert!(pos("[PlayerIns+0x5A8]+0x20 u32").is_some(), "{lines:?}");
        assert!(pos("[[PlayerIns+0x1]+0x2]+0x3 f32").is_some(), "{lines:?}");
        assert!(pos("PlayerIns+0x14 f32").is_some(), "{lines:?}");
        assert!(
            pos("Singleton<WorldChrMan>+0x44 f32").is_some(),
            "{lines:?}"
        );
        assert!(pos("PlayerIns+0x18 ").is_none(), "constant: {lines:?}");
        assert!(
            pos("Singleton<WorldChrMan>+0x40 ").is_none(),
            "started late: {lines:?}"
        );
        // The half-window counter is ranked after the exact ones.
        let half = pos("PlayerIns+0x14 f32").expect("listed");
        for exact in ["PlayerIns+0x10 f32", "[PlayerIns+0x5A8]+0x20 u32"] {
            assert!(pos(exact).expect("listed") < half, "{lines:?}");
        }
        assert!(
            top[0].contains("err=0%") || top[0].contains("err=1%"),
            "{lines:?}"
        );
        assert!(
            top[pos("[PlayerIns+0x5A8]+0x20 u32").expect("listed")].contains("match=60x"),
            "{lines:?}"
        );
        assert!(!s.is_active());
    }

    #[test]
    fn a_window_shorter_than_thirty_samples_is_not_ranked() {
        let locs: Vec<(WLoc, Gen)> = vec![(
            WLoc::player(0x10),
            Box::new(|i| (15.0f32 - i as f32 * 0.1).to_bits()),
        )];
        let mut s = drive(29, &locs, 0);
        let lines = s.finish(2900, &[], &[]);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("window too short (29 < 30")),
            "{lines:?}"
        );
        assert!(lines.iter().all(|l| !l.contains("(i)")), "{lines:?}");
        let mut s = drive(30, &locs, 0);
        let lines = s.finish(3000, &[], &[]);
        assert!(
            lines.iter().any(|l| l.contains("(i) 1 steady")),
            "{lines:?}"
        );
    }

    #[test]
    fn step_ranking_needs_a_marker_and_orders_by_matches() {
        let hit2 = WLoc::player(0x100); // steps at 3.0 s and 6.0 s: both markers
        let hit1 = WLoc::level1(0x40, 0x8); // steps at 3.0 s (marker) and 8.0 s (none)
        let miss = WLoc::player(0x104); // steps at 5.0 s only: no marker near
        let noisy = WLoc::player(0x108); // changes every sample: not constant-then-stepping
        let early = WLoc::player(0x10C); // changes at sample 1: not quiet long enough
        let locs: Vec<(WLoc, Gen)> = vec![
            (
                hit2,
                Box::new(|i| 100 + if i >= 30 { 5 } else { 0 } + if i >= 60 { 5 } else { 0 }),
            ),
            (
                hit1,
                Box::new(|i| {
                    1.0f32.to_bits() + if i >= 30 { 10 } else { 0 } + if i >= 80 { 10 } else { 0 }
                }),
            ),
            (miss, Box::new(|i| if i >= 50 { 9 } else { 4 })),
            (noisy, Box::new(|i| i as u32 * 3)),
            (early, Box::new(|i| if i >= 1 { 8 } else { 2 })),
        ];
        let mut s = drive(100, &locs, 1000);
        let markers = [
            Marker {
                t_ms: 1000 + 3000 + 100,
                what: "attacker",
            },
            Marker {
                t_ms: 1000 + 6000 - 200,
                what: "hp",
            },
        ];
        let lines = s.finish(1000 + 100 * SAMPLE_MS, &markers, &[]);
        let top: Vec<&String> = lines.iter().filter(|l| l.contains("(ii) #")).collect();
        assert_eq!(top.len(), 2, "{lines:?}");
        assert!(
            top[0].contains("PlayerIns+0x100") && top[0].contains("matched=2 unmatched=0"),
            "{lines:?}"
        );
        assert!(
            top[0].contains("[attacker]") && top[0].contains("[hp]"),
            "{lines:?}"
        );
        assert!(
            top[1].contains("[PlayerIns+0x40]+0x8") && top[1].contains("matched=1 unmatched=1"),
            "{lines:?}"
        );
        assert!(
            top.iter()
                .all(|l| !l.contains("0x104") && !l.contains("0x108") && !l.contains("0x10C")),
            "{lines:?}"
        );
        // Without markers nothing is a candidate.
        let mut s = drive(100, &locs, 1000);
        let lines = s.finish(1000 + 100 * SAMPLE_MS, &[], &[]);
        assert!(
            lines.iter().any(|l| l.contains("(ii) 0 constant")),
            "{lines:?}"
        );
    }

    #[test]
    fn analysis_accepts_countdowns_and_rejects_noise() {
        let series = |f: &dyn Fn(f64) -> f64, n: usize, dt: f64| {
            let t: Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
            let v = t.iter().map(|&x| f(x)).collect::<Vec<_>>();
            (t, v)
        };
        let (t, v) = series(&|x| 60.0 - x, 40, 0.1);
        let a = analyse_series(&t, &v).expect("countdown");
        assert!((a.rate + 1.0).abs() < 1e-9 && a.monotone == 1.0, "{a:?}");
        let (t, v) = series(&|_| 7.0, 40, 0.1);
        assert!(analyse_series(&t, &v).is_none());
        let (t, v) = series(&|x| 60.0 - x, 29, 0.1);
        assert!(analyse_series(&t, &v).is_none(), "29 < 30 samples");
        let (t, v) = series(&|x| (x * 10.0).sin(), 40, 0.1);
        assert!(analyse_series(&t, &v).is_none());
    }

    #[test]
    fn too_few_samples_and_caps_are_reported() {
        let mut s = WideScan::default();
        s.begin(0);
        assert!(s.begin_pass(0));
        let l = s.finish(300, &[], &[]);
        assert!(l.iter().any(|x| x.contains("window too short")), "{l:?}");
        // Sample cap.
        let mut s = WideScan::default();
        s.begin(0);
        for i in 0..MAX_SAMPLES {
            assert!(s.begin_pass(i as u64 * SAMPLE_MS));
        }
        assert!(!s.begin_pass(1_000_000));
        assert!(!s.due(2_000_000));
    }

    // ---- bulk sampler on fake memory ----

    const BASE: usize = 0x7FF0_0000_0000;
    const SIZE: usize = 0x100_0000;
    const PLAYER: usize = 0x1000_0000;

    /// Fake process memory: a set of regions; `read_region` returns the readable prefix and counts every call.
    #[derive(Default)]
    struct FakeMem {
        regions: Vec<(usize, Vec<u8>)>,
        calls: Vec<(usize, usize)>,
    }

    impl FakeMem {
        fn add(&mut self, addr: usize, bytes: Vec<u8>) {
            self.regions.push((addr, bytes));
        }
        fn put_u64(&mut self, addr: usize, off: usize, v: u64) {
            let r = self
                .regions
                .iter_mut()
                .find(|r| r.0 == addr)
                .expect("region");
            r.1[off..off + 8].copy_from_slice(&v.to_le_bytes());
        }
    }

    impl BulkMem for FakeMem {
        fn read_region(&mut self, addr: usize, len: usize) -> Vec<u8> {
            self.calls.push((addr, len));
            for (start, bytes) in &self.regions {
                if addr >= *start && addr < start + bytes.len() {
                    let off = addr - start;
                    let n = len.min(bytes.len() - off);
                    return bytes[off..off + n].to_vec();
                }
            }
            Vec::new()
        }
    }

    /// PlayerIns at PLAYER with pointers at +0x20 -> P1, +0x28 -> P2 (+ a duplicate at +0x30 -> P1, and one into the exe image
    /// at +0x38); P1 holds a pointer to L2 at +0x40. Singleton slot S0 -> instance I0 which points to Q at +0x10.
    fn world() -> (FakeMem, Vec<Single>) {
        let (p1, p2, l2, i0, q, slot) = (
            0x2000_0000usize,
            0x2000_1000,
            0x2000_2000,
            0x3000_0000,
            0x3000_2000,
            BASE + 0x1000,
        );
        let mut m = FakeMem::default();
        m.add(PLAYER, vec![0u8; PLAYER_LEN]);
        m.put_u64(PLAYER, 0x20, p1 as u64);
        m.put_u64(PLAYER, 0x28, p2 as u64);
        m.put_u64(PLAYER, 0x30, p1 as u64);
        m.put_u64(PLAYER, 0x38, (BASE + 0x500) as u64);
        let mut v = vec![0u8; POINTEE_LEN];
        v[4..8].copy_from_slice(&15.0f32.to_le_bytes());
        m.add(p1, v);
        m.put_u64(p1, 0x40, l2 as u64);
        m.add(p2, vec![1u8; POINTEE_LEN]);
        m.add(l2, vec![2u8; POINTEE_LEN]);
        m.add(slot, (i0 as u64).to_le_bytes().to_vec());
        m.add(i0, vec![0u8; SING_LEN]);
        m.put_u64(i0, 0x10, q as u64);
        m.add(q, vec![3u8; POINTEE_LEN]);
        let singles = vec![
            Single {
                name: "WorldChrMan".into(),
                slot,
            },
            Single {
                name: "BloodStainMan".into(),
                slot: BASE + 0x2000, // unreadable slot
            },
        ];
        (m, singles)
    }

    fn run_pass(
        m: &mut FakeMem,
        singles: &[Single],
        budget: Budget,
    ) -> (PassInfo, Vec<(WLoc, u32)>) {
        let req = PassReq {
            player: PLAYER,
            singles,
            budget,
        };
        let mut words = Vec::new();
        let info = sample_pass(m, &req, BASE, SIZE, &mut |l, b| words.push((l, b)));
        (info, words)
    }

    #[test]
    fn the_sampler_copies_each_region_once_and_labels_every_word() {
        let (mut m, singles) = world();
        let (info, words) = run_pass(&mut m, &singles, Budget::FULL);
        // PlayerIns + P1, P2 (the duplicate and the exe pointer are skipped) + L2 = 4 regions; singleton I0 + Q.
        assert_eq!(info.regions, 3, "{info:?}");
        assert_eq!(info.sing_regions, 1, "{info:?}");
        assert_eq!((info.sing_live, info.sing_sampled), (1, 1));
        assert_eq!(
            info.bytes,
            PLAYER_LEN + 3 * POINTEE_LEN + SING_LEN + POINTEE_LEN
        );
        assert_eq!(words.len(), info.bytes / 4);
        // One bulk call per region: player, P1, P2, L2, slot0, I0, Q, slot1 (the unreadable slot is asked once and skipped).
        assert_eq!(m.calls.len(), 8, "{:?}", m.calls);
        assert!(m.calls.iter().all(|c| c.1 >= 8));
        let has = |l: WLoc, v: u32| words.contains(&(l, v));
        assert!(has(WLoc::level1(0x20, 4), 15.0f32.to_bits()));
        assert!(has(WLoc::level1(0x28, 4), 0x0101_0101));
        assert!(has(WLoc::level2(0x20, 0x40, 8), 0x0202_0202));
        assert!(has(WLoc::singleton1(0, 0x10, 0x100), 0x0303_0303));
        assert!(words.iter().any(|(l, _)| *l == WLoc::singleton(0, 0xFFC)));
        // The duplicate pointer at +0x30 produced no second copy of P1.
        assert_eq!(
            words
                .iter()
                .filter(|(l, _)| *l == WLoc::level1(0x30, 0))
                .count(),
            0
        );
    }

    #[test]
    fn dropping_the_second_level_and_the_caps_limit_the_regions() {
        let (mut m, singles) = world();
        let mut b = Budget::FULL;
        b.two_level = false;
        let (info, words) = run_pass(&mut m, &singles, b);
        assert_eq!(info.regions, 2);
        assert!(words.iter().all(|(l, _)| l.kind != 2));
        // Cap of 1 PlayerIns region and 0 -> at least one singleton region per singleton.
        let (mut m, singles) = world();
        let b = Budget {
            two_level: true,
            max_regions: 1,
            sing_regions: 1,
        };
        let (info, _) = run_pass(&mut m, &singles, b);
        assert_eq!(info.regions, 1);
        assert!(info.sing_regions <= 1);
        // An unreadable PlayerIns gives no words and no pointee reads.
        let (mut m, singles) = world();
        let req = PassReq {
            player: 0x5000_0000,
            singles: &singles,
            budget: Budget::FULL,
        };
        let mut n = 0;
        let info = sample_pass(&mut m, &req, BASE, SIZE, &mut |_, _| n += 1);
        assert_eq!(info.regions, 0);
        assert_eq!(info.sing_sampled, 1, "singletons are independent");
        assert_eq!(n, (SING_LEN + POINTEE_LEN) / 4);
    }

    #[test]
    fn the_short_list_is_picked_by_exact_name() {
        let all = [
            ("WorldChrMan", 0x10usize),
            ("BloodStainMan", 0x20),
            ("OtherMan", 0x30),
            ("CSRespawnPointManX", 0x40),
        ];
        let (found, missing) = pick_short_list(all.iter().map(|(n, s)| (*n, *s)));
        let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        // Short-list order, not map order.
        assert_eq!(names, ["BloodStainMan", "WorldChrMan"]);
        assert_eq!(found[1].slot, 0x10);
        assert!(missing.contains(&"CSRespawnPointMan") && missing.len() == 5);
    }

    #[test]
    fn slow_passes_back_off_in_order_and_stop_at_the_floor() {
        let mut b = Budget::FULL;
        let mut steps = Vec::new();
        while let Some(s) = b.back_off() {
            steps.push(s);
            assert!(steps.len() < 20);
        }
        assert!(steps[0].contains("two-level pointees dropped"), "{steps:?}");
        assert!(!b.two_level);
        assert!(steps[1].contains("halved to 64 (PlayerIns) / 32 (singletons)"));
        // 128 -> 64 -> 32 -> 16 -> 8: four halvings of the first cap, after the drop.
        assert_eq!(steps.len(), 1 + 4, "{steps:?}");
        assert_eq!((b.max_regions, b.sing_regions), (MIN_REGIONS, MIN_REGIONS));
        assert!(b.back_off().is_none());
    }

    fn worker(passes: std::rc::Rc<std::cell::RefCell<Vec<Budget>>>, elapsed: u64) -> ScanWorker {
        ScanWorker::new(Box::new(
            move |req: &PassReq<'_>, sink: &mut dyn FnMut(WLoc, u32)| {
                passes.borrow_mut().push(req.budget);
                sink(WLoc::player(0), 1);
                PassInfo {
                    elapsed_ms: elapsed,
                    bytes: 4,
                    ..PassInfo::default()
                }
            },
        ))
    }

    fn start(w: &mut ScanWorker, at: u64) -> Vec<String> {
        w.handle(ScanMsg::Start {
            at_ms: at,
            player: PLAYER,
            singles: vec![Single {
                name: "WorldChrMan".into(),
                slot: 1,
            }],
        })
    }

    #[test]
    fn the_worker_idles_until_started_and_ticks_every_100_ms() {
        let passes = Default::default();
        let mut w = worker(std::rc::Rc::clone(&passes), 3);
        assert!(w.tick(0).is_empty() && w.next_due_ms().is_none());
        let l = start(&mut w, 1000);
        assert!(
            l[0].contains("sampling every 100 ms") && l[0].contains("WorldChrMan"),
            "{l:?}"
        );
        assert!(l[0].contains("not in the singleton map: CSTeamSessionDataMan"));
        assert_eq!(w.next_due_ms(), Some(1000));
        w.tick(1000);
        assert_eq!(w.next_due_ms(), Some(1100));
        w.tick(1050); // not due
        w.tick(1100);
        assert_eq!(passes.borrow().len(), 2);
        // Stop ranks: too short for a ranking, with the downed-clear marker counted.
        let l = w.handle(ScanMsg::Stop { at_ms: 1200 });
        assert!(l[0].contains("left the downed state (1 marker(s)"), "{l:?}");
        assert!(l.iter().any(|x| x.contains("window too short")));
        assert!(!w.is_active());
        w.tick(5000);
        assert_eq!(passes.borrow().len(), 2);
        // A Stop or Marker while idle is harmless.
        assert!(w.handle(ScanMsg::Stop { at_ms: 9 }).is_empty());
        assert!(w
            .handle(ScanMsg::Marker(Marker {
                t_ms: 1,
                what: "hp"
            }))
            .is_empty());
    }

    #[test]
    fn a_slow_pass_is_logged_and_the_next_passes_get_cheaper() {
        let passes: std::rc::Rc<std::cell::RefCell<Vec<Budget>>> = Default::default();
        let mut w = worker(std::rc::Rc::clone(&passes), SLOW_PASS_MS + 1);
        start(&mut w, 0);
        let mut lines = Vec::new();
        for i in 0..8u64 {
            lines.extend(w.tick(i * 100));
        }
        let seen = passes.borrow();
        assert!(seen[0].two_level && seen[0].max_regions == MAX_REGIONS);
        assert!(!seen[1].two_level && seen[1].max_regions == MAX_REGIONS);
        assert_eq!(seen[2].max_regions, 64);
        assert_eq!(seen[3].max_regions, 32);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("two-level pointees dropped") && l.contains("> 40 ms")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("halved to 64")),
            "{lines:?}"
        );
        assert!(lines.iter().any(|l| l.contains("already at its floor")));
        drop(seen);
        // A new window starts with the full budget again.
        start(&mut w, 10_000);
        assert_eq!(w.budget(), Budget::FULL);
        // Fast passes never back off.
        let passes: std::rc::Rc<std::cell::RefCell<Vec<Budget>>> = Default::default();
        let mut w = worker(std::rc::Rc::clone(&passes), SLOW_PASS_MS);
        start(&mut w, 0);
        for i in 0..5u64 {
            w.tick(i * 100);
        }
        assert!(passes.borrow().iter().all(|b| *b == Budget::FULL));
    }
}
