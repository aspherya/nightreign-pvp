//! Pure filter logic (no Windows calls): elapsed-time matching, the revive-bar history filter, region windows.

use crate::winmem::Region;

pub const F32_SCALES: [f64; 3] = [1.0, 30.0, 60.0];
pub const U32_SCALES: [f64; 4] = [1.0, 30.0, 60.0, 1000.0];

/// Per-candidate tracking: direction of change (0 = unknown) and the bit mask of still-possible time scales.
#[derive(Clone, Copy, Debug)]
pub struct Track {
    pub dir: i8,
    pub mask: u8,
}

impl Track {
    pub fn new(n_scales: usize) -> Track {
        Track {
            dir: 0,
            mask: ((1u16 << n_scales) - 1) as u8,
        }
    }
}

/// Does the step prev -> cur match the elapsed time (x one of the still-possible scales), in the same direction as before?
/// On success the track is narrowed.
pub fn step_match(
    tr: &mut Track,
    scales: &[f64],
    prev: f64,
    prev_t: f64,
    cur: f64,
    cur_t: f64,
    tol: f64,
) -> bool {
    let d = cur - prev;
    let el = cur_t - prev_t;
    if el.is_nan() || el < 1e-4 || !d.is_finite() || d == 0.0 {
        return false;
    }
    let dir: i8 = if d > 0.0 { 1 } else { -1 };
    if tr.dir != 0 && tr.dir != dir {
        return false;
    }
    let mut m = 0u8;
    for (i, s) in scales.iter().enumerate() {
        if (tr.mask >> i) & 1 == 1 {
            let exp = el * s;
            if (d.abs() - exp).abs() <= tol * exp {
                m |= 1 << i;
            }
        }
    }
    if m == 0 {
        return false;
    }
    tr.mask = m;
    tr.dir = dir;
    true
}

pub const BAR_MAX: f64 = 300.0;
pub const BAR_TARGETS: [f64; 3] = [40.0, 90.0, 240.0];

/// Strip a trailing reset-to-0 (the bar clearing at the UP) from a history; returns (body, reset_seen).
pub fn strip_reset(vals: &[f64]) -> (&[f64], bool) {
    let n = vals.len();
    if n >= 2 && vals[n - 1] == 0.0 && vals[..n - 1].iter().any(|v| *v > 0.0) {
        (&vals[..n - 1], true)
    } else {
        (vals, false)
    }
}

/// Loose revive-bar filter: values stay in [0, 300]; the value changes on at most 60% of the samples (checked once there are
/// at least 5 transitions); a decrease is only allowed when it is slow (<= 35% of the peak so far); a single trailing reset to 0 is
/// allowed.
pub fn bar_history_ok(vals: &[f64]) -> bool {
    if vals
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0 || *v > BAR_MAX)
    {
        return false;
    }
    let (body, _) = strip_reset(vals);
    if body.len() < 2 {
        return true;
    }
    let transitions = body.len() - 1;
    let changes = body.windows(2).filter(|w| w[0] != w[1]).count();
    if transitions >= 5 && changes as f64 > 0.6 * transitions as f64 {
        return false;
    }
    let mut peak = body[0];
    for w in body.windows(2) {
        let (prev, cur) = (w[0], w[1]);
        if cur < prev && (prev - cur) > 0.35 * peak {
            return false;
        }
        peak = peak.max(cur);
    }
    true
}

/// (changes, final value before any reset, reset seen, nearest target within 10%)
pub fn bar_summary(vals: &[f64]) -> (usize, f64, bool, Option<f64>) {
    let (body, reset) = strip_reset(vals);
    let changes = body.windows(2).filter(|w| w[0] != w[1]).count();
    let fin = body.last().copied().unwrap_or(0.0);
    let near = BAR_TARGETS
        .iter()
        .copied()
        .find(|t| (fin - t).abs() <= 0.1 * t);
    (changes, fin, reset, near)
}

/// Merge [a - r, a + r] windows around the anchors into sorted disjoint half-open intervals.
pub fn merge_windows(anchors: &[u64], radius: u64) -> Vec<(u64, u64)> {
    let mut v: Vec<(u64, u64)> = anchors
        .iter()
        .map(|a| (a.saturating_sub(radius), a.saturating_add(radius)))
        .collect();
    v.sort_unstable();
    let mut out: Vec<(u64, u64)> = Vec::new();
    for (s, e) in v {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

pub fn in_windows(wins: &[(u64, u64)], addr: u64) -> bool {
    wins.binary_search_by(|w| {
        if addr < w.0 {
            std::cmp::Ordering::Greater
        } else if addr >= w.1 {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    })
    .is_ok()
}

/// Intersect regions with the (sorted, disjoint) windows, page-aligned.
pub fn clip_regions(regions: &[Region], wins: &[(u64, u64)]) -> Vec<Region> {
    let mut out = Vec::new();
    for r in regions {
        let (rs, re) = (r.base, r.base + r.size);
        for w in wins {
            let s = rs.max(w.0 & !0xFFF);
            let e = re.min((w.1 + 0xFFF) & !0xFFF);
            if s < e {
                out.push(Region {
                    base: s,
                    size: e - s,
                    protect: r.protect,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_matches_elapsed() {
        let mut tr = Track::new(3);
        // 1.5 s apart, value fell by 1.5 -> scale 1 matches
        assert!(step_match(&mut tr, &F32_SCALES, 14.0, 0.0, 12.5, 1.5, 0.25));
        assert_eq!(tr.dir, -1);
        assert_eq!(tr.mask, 0b001);
        assert!(step_match(&mut tr, &F32_SCALES, 12.5, 1.5, 11.0, 3.0, 0.25));
        // direction flip (count up) is rejected even if the magnitude matches
        assert!(!step_match(
            &mut tr,
            &F32_SCALES,
            11.0,
            3.0,
            12.5,
            4.5,
            0.25
        ));
    }

    #[test]
    fn frame_counter_scale_and_no_match() {
        let mut tr = Track::new(3);
        // 1.0 s, +60 -> only the x60 scale matches
        assert!(step_match(
            &mut tr,
            &F32_SCALES,
            100.0,
            0.0,
            160.0,
            1.0,
            0.25
        ));
        assert_eq!(tr.mask, 0b100);
        // then a x1 step no longer fits
        assert!(!step_match(
            &mut tr,
            &F32_SCALES,
            160.0,
            1.0,
            161.0,
            2.0,
            0.25
        ));
        let mut t2 = Track::new(3);
        assert!(!step_match(&mut t2, &F32_SCALES, 5.0, 0.0, 5.0, 1.0, 0.25)); // unchanged
        assert!(!step_match(&mut t2, &F32_SCALES, 5.0, 0.0, 9.0, 1.0, 0.25)); // 4 s in 1 s
    }

    #[test]
    fn bar_filter() {
        assert!(bar_history_ok(&[
            0.0, 0.0, 10.0, 10.0, 10.0, 25.0, 25.0, 24.0, 24.0, 24.0
        ]));
        assert!(bar_history_ok(&[0.0, 0.0, 0.0, 40.0, 40.0, 40.0, 0.0])); // trailing reset
        assert!(!bar_history_ok(&[0.0, 5.0, -1.0])); // negative
        assert!(!bar_history_ok(&[0.0, 5.0, 301.0]));
        assert!(!bar_history_ok(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0])); // changes every sample
        assert!(!bar_history_ok(&[0.0, 0.0, 40.0, 0.0, 0.0, 0.0, 0.0])); // reset mid-history, not trailing
        assert!(bar_history_ok(&[0.0; 7])); // constant zero is fine (filtered elsewhere)
        let (ch, fin, reset, near) = bar_summary(&[0.0, 0.0, 38.0, 38.0, 0.0]);
        assert_eq!((ch, fin, reset, near), (1, 38.0, true, Some(40.0)));
    }

    #[test]
    fn region_selection_and_windows() {
        let regions = vec![
            Region {
                base: 0x1000,
                size: 0x10000,
                protect: 4,
            },
            Region {
                base: 0x100000,
                size: 0x2000,
                protect: 4,
            },
        ];
        let wins = merge_windows(&[0x5000, 0x6000, 0x101000], 0x800);
        assert_eq!(wins, vec![(0x4800, 0x6800), (0x100800, 0x101800)]);
        let c = clip_regions(&regions, &wins);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].base, c[0].base + c[0].size), (0x4000, 0x7000));
        assert_eq!((c[1].base, c[1].base + c[1].size), (0x100000, 0x102000));
        assert!(
            in_windows(&wins, 0x5000) && !in_windows(&wins, 0x7000) && in_windows(&wins, 0x100900)
        );
    }

    #[test]
    fn scannable_filter() {
        use crate::winmem::scannable;
        // MEM_COMMIT=0x1000, MEM_PRIVATE=0x20000; PAGE_READWRITE=4, PAGE_GUARD=0x100, NOACCESS=1, READONLY=2
        assert!(scannable(0x1000, 0x20000, 4, 0x1000));
        assert!(!scannable(0x1000, 0x20000, 4 | 0x100, 0x1000));
        assert!(!scannable(0x1000, 0x20000, 1, 0x1000));
        assert!(!scannable(0x1000, 0x20000, 2, 0x1000));
        assert!(!scannable(0x1000, 0x1000000, 4, 0x1000)); // image
        assert!(!scannable(0x2000, 0x20000, 4, 0x1000)); // reserved
        assert!(!scannable(0x1000, 0x20000, 4, (64 << 30) + 1));
    }
}
