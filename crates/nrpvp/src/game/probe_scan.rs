//! T-031 (c): position discovery by movement (dev probes, read-only).
//!
//! The ER physics chain failed on NR 1.3.3.0 (`PlayerIns+0x190` is readable, `[+0x190]+0x68` is not), so this probe finds the
//! position by brute force: every [`SAMPLE_MS`], for every ChrSet player, read `PlayerIns+0..0x1000` and, for each 8-aligned
//! pointer in that range that points to readable memory OUTSIDE the exe image, the first 0x200 bytes of the pointee (one level,
//! at most [`MAX_POINTEES`]). Every 4-aligned f32 triple (x, y, z consecutive, all finite, |v| < 100000) is tracked. Over a
//! [`WINDOW_MS`] window a triple is a candidate when it changed in at least 30% of the steps and no step was 15 units or more;
//! candidates are ranked by the number of distinct values (1 cm resolution, capped at 64) and then by smoothness (mean step /
//! max step), and the top [`TOP_N`] are logged per player together with the vtable RVA of the pointee that holds them.
//!
//! No ground truth is available to the probe: the HUMAN walks in a straight line for 20 s and compares the candidates of the two
//! windows. Everything logged under `T-031 POSSCAN UNVERIFIED` is a hypothesis. Strictly read-only: guarded reads through
//! [`RegionCache`] (one cache per pass), no writes, no calls into game functions.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::discover::is_ptr;
use super::mark_dump::vtable_fields;
use super::mem::{va_rva, RegionCache};
use super::probe_pos::Target;

const TAG: &str = "T-031 POSSCAN UNVERIFIED";
/// Bytes of the PlayerIns scanned (and searched for pointers).
pub(crate) const PLAYER_LEN: usize = 0x1000;
/// Bytes read from each pointee.
pub(crate) const POINTEE_LEN: usize = 0x200;
pub(crate) const MAX_POINTEES: usize = 64;
pub(crate) const SAMPLE_MS: u64 = 250;
pub(crate) const WINDOW_MS: u64 = 10_000;
pub(crate) const TOP_N: usize = 12;
/// |component| must stay below this to be a believable coordinate.
const MAX_ABS: f32 = 100_000.0;
/// A step of this many units or more disqualifies a triple (it jumped, so it is not a smooth position).
const MAX_STEP: f32 = 15.0;
/// A step above this (1 cm) counts as "changed".
const CHANGE_EPS: f32 = 0.01;
/// Triples whose components are all below this are treated as zero and are not tracked until they leave it.
const TINY: f32 = 1.0e-3;
/// A candidate needs at least this many samples in the window.
const MIN_SAMPLES: u32 = 8;
/// Distinct-value counter cap.
const DISTINCT_CAP: usize = 64;
/// `Loc::poff` of a triple inside the PlayerIns itself.
const NO_PTR: u16 = u16::MAX;

/// Where a triple lives: `poff` = PlayerIns offset of the pointer (`NO_PTR` = in the PlayerIns itself), `off` = offset in the
/// PlayerIns or in the pointee.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Loc {
    pub poff: u16,
    pub off: u16,
}

impl Loc {
    pub(crate) fn text(&self) -> String {
        if self.poff == NO_PTR {
            format!("PlayerIns+0x{:X}", self.off)
        } else {
            format!("[PlayerIns+0x{:X}]+0x{:X}", self.poff, self.off)
        }
    }
}

/// One pointee read from memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pointee {
    pub poff: u16,
    pub addr: usize,
    pub bytes: Vec<u8>,
}

pub(crate) fn in_image(addr: usize, base: usize, size: usize) -> bool {
    addr >= base && addr - base < size
}

/// The (up to [`MAX_POINTEES`]) distinct pointees of the 8-aligned pointers in `player`, in offset order: `is_ptr`, outside
/// the exe image, readable. Their first [`POINTEE_LEN`] bytes are copied.
pub(crate) fn collect_pointees(
    rc: &mut RegionCache,
    player: &[u8],
    base: usize,
    size: usize,
) -> Vec<Pointee> {
    let mut out: Vec<Pointee> = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();
    for off in (0..player.len().saturating_sub(7)).step_by(8) {
        if out.len() >= MAX_POINTEES {
            break;
        }
        let Some(raw) = player.get(off..off + 8) else {
            break;
        };
        let q = u64::from_le_bytes(raw.try_into().unwrap_or([0; 8])) as usize;
        if !is_ptr(q) || in_image(q, base, size) || !seen.insert(q) {
            continue;
        }
        let bytes = rc.read_prefix(q, POINTEE_LEN);
        if bytes.len() < 12 {
            continue;
        }
        let Ok(poff) = u16::try_from(off) else { break };
        out.push(Pointee {
            poff,
            addr: q,
            bytes,
        });
    }
    out
}

/// What a 12-byte read at one location is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tri {
    /// All components below [`TINY`] (zero, denormals, small integers read as floats).
    Small([f32; 3]),
    Valid([f32; 3]),
    /// A component is NaN/inf or at least [`MAX_ABS`].
    Invalid,
}

fn tri_at(buf: &[u8], off: usize) -> Option<Tri> {
    let b = buf.get(off..off + 12)?;
    let f = |i: usize| f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let v = [f(0), f(4), f(8)];
    if v.iter().any(|x| !x.is_finite() || x.abs() >= MAX_ABS) {
        return Some(Tri::Invalid);
    }
    if v.iter().all(|x| x.abs() < TINY) {
        return Some(Tri::Small(v));
    }
    Some(Tri::Valid(v))
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn quant(v: [f32; 3]) -> [i32; 3] {
    // 1 cm; the casts saturate, and the values are below 100000 so they fit comfortably.
    [
        (v[0] * 100.0).round() as i32,
        (v[1] * 100.0).round() as i32,
        (v[2] * 100.0).round() as i32,
    ]
}

/// Online statistics of one triple over the window.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LocStats {
    /// Samples seen since the triple was first tracked.
    pub n: u32,
    /// Steps larger than [`CHANGE_EPS`].
    pub changes: u32,
    /// Steps of [`MAX_STEP`] or more.
    pub big: u32,
    /// The triple was invalid (NaN/inf/huge) at least once.
    pub bad: bool,
    first_q: [i32; 3],
    last_q: [i32; 3],
    extra: Vec<[i32; 3]>,
    pub last: [f32; 3],
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub max_step: f32,
    sum_step: f32,
}

impl LocStats {
    fn new(v: [f32; 3]) -> Self {
        LocStats {
            n: 1,
            changes: 0,
            big: 0,
            bad: false,
            first_q: quant(v),
            last_q: quant(v),
            extra: Vec::new(),
            last: v,
            min: v,
            max: v,
            max_step: 0.0,
            sum_step: 0.0,
        }
    }

    fn update(&mut self, v: [f32; 3]) {
        let step = dist(self.last, v);
        self.n = self.n.saturating_add(1);
        if step > CHANGE_EPS {
            self.changes += 1;
            self.sum_step += step;
        }
        if step >= MAX_STEP {
            self.big += 1;
        }
        if step > self.max_step {
            self.max_step = step;
        }
        for (i, &x) in v.iter().enumerate() {
            self.min[i] = self.min[i].min(x);
            self.max[i] = self.max[i].max(x);
        }
        let q = quant(v);
        if q != self.last_q {
            self.last_q = q;
            if q != self.first_q && !self.extra.contains(&q) && self.extra.len() + 1 < DISTINCT_CAP
            {
                self.extra.push(q);
            }
        }
        self.last = v;
    }

    fn mark_bad(&mut self) {
        self.bad = true;
        self.n = self.n.saturating_add(1);
    }

    /// Distinct 1 cm values seen (capped at [`DISTINCT_CAP`]).
    pub(crate) fn distinct(&self) -> usize {
        1 + self.extra.len()
    }

    /// Mean changing step / largest step, 0..=1 (1 = perfectly even steps; 0 = never moved).
    pub(crate) fn smoothness(&self) -> f32 {
        if self.changes == 0 || self.max_step <= 0.0 {
            return 0.0;
        }
        (self.sum_step / self.changes as f32 / self.max_step).clamp(0.0, 1.0)
    }

    /// Axes whose range exceeds the change threshold.
    pub(crate) fn moving_axes(&self) -> usize {
        (0..3)
            .filter(|&i| self.max[i] - self.min[i] > CHANGE_EPS)
            .count()
    }

    pub(crate) fn first_axis_moves(&self) -> bool {
        self.max[0] - self.min[0] > CHANGE_EPS
    }

    /// Largest per-axis range.
    pub(crate) fn span(&self) -> f32 {
        (0..3)
            .map(|i| self.max[i] - self.min[i])
            .fold(0.0, f32::max)
    }

    /// Changed in at least 30% of the steps, no step of 15 units or more, never invalid, enough samples.
    pub(crate) fn qualifies(&self) -> bool {
        !self.bad
            && self.n >= MIN_SAMPLES
            && self.big == 0
            && u64::from(self.changes) * 10 >= u64::from(self.n - 1) * 3
    }
}

/// All the statistics of one player for the current window.
#[derive(Debug, Default)]
pub(crate) struct PlayerScan {
    stats: HashMap<Loc, LocStats>,
    /// Pointee address per PlayerIns pointer offset, from the last pass.
    ptr_at: HashMap<u16, usize>,
    /// Passes in the current window.
    pub samples: u32,
    pub pointees: usize,
}

impl PlayerScan {
    fn observe(&mut self, loc: Loc, buf: &[u8], off: usize) {
        let Some(t) = tri_at(buf, off) else { return };
        match (self.stats.get_mut(&loc), t) {
            (Some(s), Tri::Valid(v) | Tri::Small(v)) => s.update(v),
            (Some(s), Tri::Invalid) => s.mark_bad(),
            (None, Tri::Valid(v)) => {
                self.stats.insert(loc, LocStats::new(v));
            }
            (None, _) => {}
        }
    }

    /// Fold one pass in: the PlayerIns bytes and its pointees.
    pub(crate) fn feed(&mut self, player: &[u8], pointees: &[Pointee]) {
        self.samples = self.samples.saturating_add(1);
        self.pointees = pointees.len();
        // A pointer that moved or vanished invalidates what was learned through it.
        let current: HashMap<u16, usize> = pointees.iter().map(|p| (p.poff, p.addr)).collect();
        let stale: HashSet<u16> = self
            .ptr_at
            .iter()
            .filter(|(poff, addr)| current.get(poff) != Some(addr))
            .map(|(poff, _)| *poff)
            .collect();
        if !stale.is_empty() {
            self.stats.retain(|l, _| !stale.contains(&l.poff));
        }
        self.ptr_at = current;
        for off in (0..player.len().saturating_sub(11)).step_by(4) {
            self.observe(
                Loc {
                    poff: NO_PTR,
                    off: u16::try_from(off).unwrap_or(u16::MAX),
                },
                player,
                off,
            );
        }
        for p in pointees {
            for off in (0..p.bytes.len().saturating_sub(11)).step_by(4) {
                self.observe(
                    Loc {
                        poff: p.poff,
                        off: u16::try_from(off).unwrap_or(u16::MAX),
                    },
                    &p.bytes,
                    off,
                );
            }
        }
    }

    /// The qualifying candidates, best first, at most `top`; and how many qualified in total.
    pub(crate) fn rank(&self, top: usize) -> (Vec<(Loc, LocStats)>, usize) {
        let mut q: Vec<(Loc, &LocStats)> = self
            .stats
            .iter()
            .filter(|(_, s)| s.qualifies())
            .map(|(l, s)| (*l, s))
            .collect();
        let total = q.len();
        // Distinct values (more is better), then smoothness (permille, higher is better). Ties: more moving axes, then the
        // first axis moving (an (x,y,z) beats the shifted (w,x,y) that overlaps it), then location for a stable order.
        q.sort_by(|a, b| {
            let key = |s: &LocStats| {
                (
                    s.distinct(),
                    (s.smoothness() * 1000.0) as u32,
                    s.moving_axes(),
                    s.first_axis_moves(),
                )
            };
            key(b.1).cmp(&key(a.1)).then(a.0.cmp(&b.0))
        });
        // Overlapping triples of the same region show the same movement; keep the best of each overlap.
        let mut kept: Vec<(Loc, &LocStats)> = Vec::new();
        for (l, s) in q {
            if !kept
                .iter()
                .any(|(k, _)| k.poff == l.poff && k.off.abs_diff(l.off) < 12)
            {
                kept.push((l, s));
            }
        }
        let mut q = kept;
        q.truncate(top);
        (q.into_iter().map(|(l, s)| (l, s.clone())).collect(), total)
    }

    pub(crate) fn tracked(&self) -> usize {
        self.stats.len()
    }

    /// Pointee address for a pointer offset (from the last pass).
    pub(crate) fn pointee_addr(&self, poff: u16) -> Option<usize> {
        self.ptr_at.get(&poff).copied()
    }

    /// Start a new window (the pointer map is kept; it is rebuilt on every pass).
    pub(crate) fn reset_window(&mut self) {
        self.stats.clear();
        self.samples = 0;
    }
}

fn v3(v: [f32; 3]) -> String {
    format!("({:.2},{:.2},{:.2})", v[0], v[1], v[2])
}

/// One ranked candidate as a log line (without the pointee vtable, which needs memory).
pub(crate) fn candidate_line(rank: usize, loc: Loc, s: &LocStats) -> String {
    format!(
        "#{rank} {} cur={} min={} max={} distinct={}{} changes={}/{} max_step={:.2} smooth={:.2} span={:.2}",
        loc.text(),
        v3(s.last),
        v3(s.min),
        v3(s.max),
        s.distinct(),
        if s.distinct() >= DISTINCT_CAP { "+" } else { "" },
        s.changes,
        s.n.saturating_sub(1),
        s.max_step,
        s.smoothness(),
        s.span()
    )
}

/// Orchestrator: schedule, window, logging.
pub(crate) struct PosScan {
    base: usize,
    size: usize,
    wcm: usize,
    last_pass: Option<Instant>,
    window_start: Option<Instant>,
    players: HashMap<usize, PlayerScan>,
    /// Slowest pass in the current window.
    slowest: Duration,
    /// T-032: vtable VA -> RTTI class name (labels in the report); `None` until the RTTI is available.
    names: Option<Arc<HashMap<usize, String>>>,
}

impl PosScan {
    pub(crate) fn new(base: usize, size: usize) -> Self {
        PosScan {
            base,
            size,
            wcm: 0,
            last_pass: None,
            window_start: None,
            players: HashMap::new(),
            slowest: Duration::ZERO,
            names: None,
        }
    }

    /// T-032: give the report the RTTI class names (vtable VA -> name).
    pub(crate) fn set_class_names(&mut self, names: Arc<HashMap<usize, String>>) {
        self.names = Some(names);
    }

    /// One pass; cheap when no sample is due. Skipped entirely while WorldChrMan is not live (`wcm` = 0).
    pub(crate) fn tick(&mut self, wcm: usize, targets: &[Target]) {
        if wcm == 0 {
            self.wcm = 0;
            self.last_pass = None;
            self.window_start = None;
            self.players.clear();
            return;
        }
        if wcm != self.wcm {
            self.wcm = wcm;
            self.last_pass = None;
            self.window_start = Some(Instant::now());
            self.players.clear();
            self.slowest = Duration::ZERO;
            tracing::info!(
                "{TAG}: new WorldChrMan {}: scanning PlayerIns+0..0x{PLAYER_LEN:X} and up to {MAX_POINTEES} pointees (0x{POINTEE_LEN:X} bytes each) for f32 triples every {SAMPLE_MS} ms; top {TOP_N} per player every {} s. Walk in a straight line to make the position stand out",
                va_rva(wcm, self.base, self.size),
                WINDOW_MS / 1000
            );
        }
        if targets.is_empty()
            || self
                .last_pass
                .is_some_and(|t| t.elapsed().as_millis() < u128::from(SAMPLE_MS))
        {
            return;
        }
        let started = Instant::now();
        self.last_pass = Some(started);
        self.players
            .retain(|a, _| targets.iter().any(|t| t.addr == *a));
        let mut rc = RegionCache::new();
        for t in targets {
            let player = rc.read_prefix(t.addr, PLAYER_LEN);
            if player.len() < 0x400 {
                continue;
            }
            let pointees = collect_pointees(&mut rc, &player, self.base, self.size);
            self.players
                .entry(t.addr)
                .or_default()
                .feed(&player, &pointees);
        }
        self.slowest = self.slowest.max(started.elapsed());
        if self
            .window_start
            .is_some_and(|w| w.elapsed().as_millis() >= u128::from(WINDOW_MS))
        {
            self.report(&mut rc, targets);
            self.window_start = Some(Instant::now());
            self.slowest = Duration::ZERO;
        }
    }

    fn report(&mut self, rc: &mut RegionCache, targets: &[Target]) {
        for t in targets {
            let Some(ps) = self.players.get_mut(&t.addr) else {
                continue;
            };
            let label = t.label();
            let (top, qualifying) = ps.rank(TOP_N);
            tracing::info!(
                "{TAG} {label} window {} s: passes={} pointees={} tracked={} qualifying={} slowest_pass={} ms",
                WINDOW_MS / 1000,
                ps.samples,
                ps.pointees,
                ps.tracked(),
                qualifying,
                self.slowest.as_millis()
            );
            let mut vt_done: HashSet<u16> = HashSet::new();
            for (i, (loc, s)) in top.iter().enumerate() {
                tracing::info!("{TAG} {label} {}", candidate_line(i + 1, *loc, s));
                if loc.poff != NO_PTR && vt_done.insert(loc.poff) {
                    let vt = ps
                        .pointee_addr(loc.poff)
                        .and_then(|a| rc.read::<usize>(a).map(|v| (a, v)));
                    let text = match vt {
                        Some((a, v)) => format!(
                            "pointee 0x{a:X} (via PlayerIns+0x{:X}) {}",
                            loc.poff,
                            vtable_fields(
                                Some(v),
                                self.base,
                                self.size,
                                self.names
                                    .as_ref()
                                    .and_then(|n| n.get(&v))
                                    .map(String::as_str)
                            )
                        ),
                        None => format!("pointee via PlayerIns+0x{:X} is gone", loc.poff),
                    };
                    tracing::info!("{TAG} {label}   {text}");
                }
            }
            ps.reset_window();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMG: usize = 0x7FF6_0000_0000;
    const IMG_SIZE: usize = 0x800_0000;

    fn put3(buf: &mut [u8], off: usize, v: [f32; 3]) {
        for (i, x) in v.iter().enumerate() {
            buf[off + i * 4..off + i * 4 + 4].copy_from_slice(&x.to_le_bytes());
        }
    }

    fn player() -> Vec<u8> {
        vec![0u8; PLAYER_LEN]
    }

    #[test]
    fn triples_are_classified() {
        let mut b = vec![0u8; 24];
        assert_eq!(tri_at(&b, 0), Some(Tri::Small([0.0; 3])));
        put3(&mut b, 0, [1.0, 2.0, 3.0]);
        assert_eq!(tri_at(&b, 0), Some(Tri::Valid([1.0, 2.0, 3.0])));
        put3(&mut b, 0, [f32::NAN, 0.0, 0.0]);
        assert_eq!(tri_at(&b, 0), Some(Tri::Invalid));
        put3(&mut b, 0, [100_000.0, 1.0, 1.0]);
        assert_eq!(tri_at(&b, 0), Some(Tri::Invalid));
        put3(&mut b, 0, [99_999.0, 1.0, 1.0]);
        assert!(matches!(tri_at(&b, 0), Some(Tri::Valid(_))));
        // Denormals (small integers read as floats) are "small", and the read is bounds-checked.
        b[..12].copy_from_slice(&[5, 0, 0, 0, 7, 0, 0, 0, 9, 0, 0, 0]);
        assert!(matches!(tri_at(&b, 0), Some(Tri::Small(_))));
        assert_eq!(tri_at(&b, 16), None);
    }

    #[test]
    fn a_walking_position_ranks_first_and_jumpers_and_constants_do_not_qualify() {
        let mut ps = PlayerScan::default();
        let mut pos = [100.0f32, 20.0, -50.0];
        for i in 0..40u32 {
            let mut p = player();
            // Smooth walk in +x at 1.5 units per sample at PlayerIns+0x6C0.
            pos[0] += 1.5;
            put3(&mut p, 0x6C0, pos);
            // A constant triple.
            put3(&mut p, 0x100, [10.0, 20.0, 30.0]);
            // A counter that jumps by 100 per sample (disqualified by the step rule).
            put3(&mut p, 0x200, [100.0 * i as f32 + 1.0, 5.0, 5.0]);
            // Slow drift (2% of the steps change): not enough changes.
            put3(
                &mut p,
                0x300,
                [3.0 + if i == 20 { 0.5 } else { 0.0 }, 3.0, 3.0],
            );
            // A walker seen through a pointer is found too, with a faster, uneven gait.
            let mut pointee = vec![0u8; POINTEE_LEN];
            let x = 7.0 + i as f32 * 0.4 + if i % 2 == 0 { 0.3 } else { 0.0 };
            put3(&mut pointee, 0x68, [x, 1.0, 2.0]);
            let pt = Pointee {
                poff: 0x190,
                addr: 0x2000_0000,
                bytes: pointee,
            };
            ps.feed(&p, &[pt]);
        }
        let (top, total) = ps.rank(12);
        assert!(total >= 2, "{total}");
        // The first one in the list has the most distinct values; both walkers have 40.
        let locs: Vec<Loc> = top.iter().map(|(l, _)| *l).collect();
        assert!(locs.contains(&Loc {
            poff: NO_PTR,
            off: 0x6C0
        }));
        assert!(locs.contains(&Loc {
            poff: 0x190,
            off: 0x68
        }));
        assert!(!locs.iter().any(|l| l.poff == NO_PTR && l.off == 0x100));
        assert!(!locs.iter().any(|l| l.poff == NO_PTR && l.off == 0x200));
        assert!(!locs.iter().any(|l| l.poff == NO_PTR && l.off == 0x300));
        // The even gait (+1.5 every sample) is smoother than the alternating one, so it ranks first on equal distinct counts.
        assert_eq!(
            top[0].0,
            Loc {
                poff: NO_PTR,
                off: 0x6C0
            }
        );
        let s = &top[0].1;
        assert_eq!(s.distinct(), 40);
        assert!(s.smoothness() > 0.99, "{}", s.smoothness());
        assert!((s.span() - 58.5).abs() < 0.01, "{}", s.span());
        let line = candidate_line(1, top[0].0, s);
        assert!(
            line.contains("PlayerIns+0x6C0") && line.contains("distinct=40"),
            "{line}"
        );
        let line = candidate_line(2, top[1].0, &top[1].1);
        assert!(line.contains("[PlayerIns+0x190]+0x68"), "{line}");
    }

    #[test]
    fn ranking_prefers_more_distinct_values_then_smoothness() {
        let mk = |steps: &[f32]| {
            let mut s = LocStats::new([1.0, 1.0, 1.0]);
            let mut x = 1.0;
            for d in steps {
                x += d;
                s.update([x, 1.0, 1.0]);
            }
            s
        };
        let even = mk(&[1.0; 12]);
        let uneven = mk(&[0.1, 3.0, 0.1, 3.0, 0.1, 3.0, 0.1, 3.0, 0.1, 3.0, 0.1, 3.0]);
        let few = mk(&[1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0]);
        assert!(even.qualifies() && uneven.qualifies() && few.qualifies());
        assert_eq!(even.distinct(), 13);
        assert_eq!(few.distinct(), 2);
        assert!(even.smoothness() > uneven.smoothness());
        let mut ps = PlayerScan::default();
        ps.stats.insert(
            Loc {
                poff: NO_PTR,
                off: 0x30,
            },
            few,
        );
        ps.stats.insert(
            Loc {
                poff: NO_PTR,
                off: 0x20,
            },
            uneven,
        );
        ps.stats.insert(
            Loc {
                poff: NO_PTR,
                off: 0x10,
            },
            even,
        );
        let order: Vec<u16> = ps.rank(12).0.iter().map(|(l, _)| l.off).collect();
        assert_eq!(order, vec![0x10, 0x20, 0x30]);
        assert_eq!(ps.rank(2).0.len(), 2);
        assert_eq!(ps.rank(2).1, 3);
    }

    #[test]
    fn qualification_rules() {
        // Not enough samples.
        let mut s = LocStats::new([5.0, 5.0, 5.0]);
        for i in 1..6 {
            s.update([5.0 + i as f32, 5.0, 5.0]);
        }
        assert!(!s.qualifies());
        // A step of exactly 15 disqualifies, 14.9 does not.
        let mut a = LocStats::new([0.0, 0.0, 1.0]);
        let mut b = a.clone();
        let mut x = 0.0;
        for _ in 0..10 {
            x += 14.9;
            a.update([x, 0.0, 1.0]);
        }
        assert!(a.qualifies());
        x = 0.0;
        for _ in 0..10 {
            x += 15.0;
            b.update([x, 0.0, 1.0]);
        }
        assert!(!b.qualifies());
        // Exactly 30% of the steps changing is enough (3 of 10), 2 of 10 is not.
        let run = |changing: usize| {
            let mut s = LocStats::new([5.0, 5.0, 5.0]);
            let mut x = 5.0;
            for i in 0..10 {
                if i < changing {
                    x += 0.5;
                }
                s.update([x, 5.0, 5.0]);
            }
            s
        };
        assert!(run(3).qualifies());
        assert!(!run(2).qualifies());
        // Invalid once = never a candidate.
        let mut c = run(10);
        assert!(c.qualifies());
        c.mark_bad();
        assert!(!c.qualifies());
    }

    #[test]
    fn a_moved_pointer_forgets_what_was_learned_through_it() {
        let mut ps = PlayerScan::default();
        let mk = |addr: usize, i: usize| {
            let mut pointee = vec![0u8; POINTEE_LEN];
            put3(&mut pointee, 0x20, [1.0 + i as f32, 2.0, 3.0]);
            Pointee {
                poff: 0x40,
                addr,
                bytes: pointee,
            }
        };
        for i in 0..5 {
            ps.feed(&player(), &[mk(0x1000_0000, i)]);
        }
        assert!(ps.tracked() >= 1);
        assert_eq!(ps.pointee_addr(0x40), Some(0x1000_0000));
        // The pointer now leads elsewhere: the old statistics are dropped and a fresh one starts.
        ps.feed(&player(), &[mk(0x1100_0000, 0)]);
        assert!(ps.tracked() >= 1);
        assert!(ps.stats.values().all(|s| s.n == 1));
        // The pointer vanished.
        ps.feed(&player(), &[]);
        assert_eq!(ps.tracked(), 0);
        // A new window starts empty.
        ps.feed(&player(), &[mk(0x1100_0000, 1)]);
        ps.reset_window();
        assert_eq!((ps.tracked(), ps.samples), (0, 0));
    }

    #[test]
    fn pointees_are_collected_from_real_memory_with_the_filters() {
        // A heap arena the pointers can lead to.
        let mut arena = vec![0u8; 0x1000];
        let a0 = (arena.as_ptr() as usize + 7) & !7;
        let off0 = a0 - arena.as_ptr() as usize;
        put3(&mut arena, off0 + 0x20, [1.0, 2.0, 3.0]);
        let mut p = player();
        let put_ptr = |p: &mut Vec<u8>, off: usize, v: usize| {
            p[off..off + 8].copy_from_slice(&(v as u64).to_le_bytes());
        };
        put_ptr(&mut p, 0x00, IMG + 0x2C307F8); // vtable in the exe: skipped
        put_ptr(&mut p, 0x10, a0); // good
        put_ptr(&mut p, 0x18, a0); // duplicate: skipped
        put_ptr(&mut p, 0x20, 0x1234); // below the pointer range
        put_ptr(&mut p, 0x28, 0x7FFF_0000_0001); // misaligned
        put_ptr(&mut p, 0x30, 0x7000_0000_0000); // plausible but unmapped
        put_ptr(&mut p, 0x38, a0 + 8); // a second distinct pointee
        let mut rc = RegionCache::new();
        let found = collect_pointees(&mut rc, &p, IMG, IMG_SIZE);
        let at: Vec<(u16, usize)> = found.iter().map(|f| (f.poff, f.addr)).collect();
        assert_eq!(at, vec![(0x10, a0), (0x38, a0 + 8)]);
        assert!(found
            .iter()
            .all(|f| !f.bytes.is_empty() && f.bytes.len() <= POINTEE_LEN));
        let mut ps = PlayerScan::default();
        ps.feed(&p, &found);
        assert!(ps.stats.contains_key(&Loc {
            poff: 0x10,
            off: 0x20
        }));
        drop(arena);
    }

    #[test]
    fn at_most_64_pointees() {
        let arena = vec![0u8; 0x8000];
        let a0 = (arena.as_ptr() as usize + 7) & !7;
        let mut p = player();
        for i in 0..100usize {
            let off = i * 8;
            p[off..off + 8].copy_from_slice(&((a0 + i * 0x40) as u64).to_le_bytes());
        }
        let mut rc = RegionCache::new();
        let found = collect_pointees(&mut rc, &p, IMG, IMG_SIZE);
        assert_eq!(found.len(), MAX_POINTEES);
        drop(arena);
    }

    #[test]
    fn the_scanner_skips_while_there_is_no_world_and_survives_garbage_players() {
        let mut s = PosScan::new(IMG, IMG_SIZE);
        let t = [Target {
            index: 0,
            addr: 0x20,
            entry_addr: 0,
            local: true,
        }];
        s.tick(0, &t);
        assert!(s.players.is_empty());
        s.tick(0x1000_0000, &t);
        assert!(s.players.is_empty(), "an unreadable player is skipped");
        s.tick(0, &t);
        assert!(s.window_start.is_none());
    }

    #[test]
    fn an_unreadable_position_pointer_target_is_never_dereferenced_by_the_report() {
        // Reporting with a pointee address that is not mapped must not fault: the vtable read is guarded.
        let mut s = PosScan::new(IMG, IMG_SIZE);
        let mut ps = PlayerScan::default();
        for i in 0..10 {
            let mut pointee = vec![0u8; POINTEE_LEN];
            put3(&mut pointee, 0, [i as f32, 1.0, 1.0]);
            ps.feed(
                &player(),
                &[Pointee {
                    poff: 0x10,
                    addr: 0x7000_0000_0000,
                    bytes: pointee,
                }],
            );
        }
        s.players.insert(0x77, ps);
        let t = [Target {
            index: 1,
            addr: 0x77,
            entry_addr: 0,
            local: false,
        }];
        let mut rc = RegionCache::new();
        s.report(&mut rc, &t);
        assert_eq!(s.players[&0x77].samples, 0);
    }
}
