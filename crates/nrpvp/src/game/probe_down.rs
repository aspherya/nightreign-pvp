//! T-031 (a): downed-state detection log (dev probes, read-only).
//!
//! Every [`SAMPLE_MS`] each ChrSet player is sampled: the raw 16-byte ChrSet entry, `team_type` (ChrIns+0x74) and the first
//! [`WATCH_LEN`] bytes of the PlayerIns. A line is written only when the entry bytes or team_type change (the first sample of a
//! player counts as a change); it carries the byte-level changes seen in the [`CONTEXT_MS`] before it, and the following
//! [`CONTEXT_MS`] log every sample's changes. Bytes that change in more than half of the samples (timers, positions) are dropped
//! from those lists, and at most [`MAX_OFFSETS`] offsets are printed per sample.
//!
//! Everything is a HYPOTHESIS aimed at finding which field marks "downed" (FS-0 run 5: entry byte +9 went 04 -> 02 -> 04 for
//! ~5 s for one player). Nothing logged under `T-031 DOWN UNVERIFIED` is verified. Strictly read-only (guarded reads through
//! [`RegionCache`]).

use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::mem::RegionCache;
use super::probe_pos::Target;
use super::team::{CHR_TEAM, ENTRY_SIZE};

const TAG: &str = "T-031 DOWN UNVERIFIED";
/// PlayerIns bytes tracked.
pub(crate) const WATCH_LEN: usize = 0x400;
/// Sampling period.
pub(crate) const SAMPLE_MS: u64 = 100;
/// Context window around a trigger, each side.
pub(crate) const CONTEXT_MS: u64 = 2000;
/// At most this many changed offsets are printed per sample.
pub(crate) const MAX_OFFSETS: usize = 24;
/// An offset can only be called noisy after this many samples.
pub(crate) const NOISE_MIN_SAMPLES: u32 = 20;
/// ChrIns per-player id [VERIFIED: FS-0/FS-1 2026-10-10, TECHNICAL_NOTES section 4].
const CHR_PLAYER_ID: usize = 0x78;

/// One byte that differs between two consecutive samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ByteChange {
    pub off: u16,
    pub old: u8,
    pub new: u8,
}

/// Change detector over a fixed-size byte window with a per-offset change counter.
#[derive(Debug, Default)]
pub(crate) struct ChangeTracker {
    prev: Vec<u8>,
    counts: Vec<u32>,
    /// Samples compared (the first sample only seeds `prev`).
    samples: u32,
}

impl ChangeTracker {
    /// Compare `cur` with the previous sample. The first call (or a length change) seeds and returns nothing.
    pub(crate) fn feed(&mut self, cur: &[u8]) -> Vec<ByteChange> {
        if self.prev.len() != cur.len() {
            self.prev = cur.to_vec();
            self.counts = vec![0; cur.len()];
            self.samples = 0;
            return Vec::new();
        }
        self.samples = self.samples.saturating_add(1);
        let mut out = Vec::new();
        for (i, (&p, &c)) in self.prev.iter().zip(cur).enumerate() {
            if p != c {
                self.counts[i] = self.counts[i].saturating_add(1);
                out.push(ByteChange {
                    off: u16::try_from(i).unwrap_or(u16::MAX),
                    old: p,
                    new: c,
                });
            }
        }
        self.prev.copy_from_slice(cur);
        out
    }

    /// True if the offset changed in more than half of the samples (judged only after [`NOISE_MIN_SAMPLES`]).
    pub(crate) fn noisy(&self, off: u16) -> bool {
        self.samples >= NOISE_MIN_SAMPLES
            && self
                .counts
                .get(usize::from(off))
                .is_some_and(|&c| u64::from(c) * 2 > u64::from(self.samples))
    }

    /// `changes` without the noisy offsets.
    pub(crate) fn keep(&self, changes: &[ByteChange]) -> Vec<ByteChange> {
        changes
            .iter()
            .copied()
            .filter(|c| !self.noisy(c.off))
            .collect()
    }
}

/// `+0x1A4:00>01 +0x1A5:10>11 (+3 more)`, at most [`MAX_OFFSETS`] offsets.
pub(crate) fn fmt_changes(ch: &[ByteChange]) -> String {
    let mut s = String::new();
    for c in ch.iter().take(MAX_OFFSETS) {
        if !s.is_empty() {
            s.push(' ');
        }
        let _ = write!(s, "+0x{:X}:{:02X}>{:02X}", c.off, c.old, c.new);
    }
    if ch.len() > MAX_OFFSETS {
        let _ = write!(s, " (+{} more)", ch.len() - MAX_OFFSETS);
    }
    s
}

fn hex16(raw: &[u8; ENTRY_SIZE]) -> String {
    let mut s = String::with_capacity(48);
    for (i, b) in raw.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{b:02X}");
    }
    s
}

/// What one sample of one player read.
#[derive(Clone, Debug, Default)]
pub(crate) struct DownSample {
    /// The first [`WATCH_LEN`] bytes of the PlayerIns (None = unreadable).
    pub bytes: Option<Vec<u8>>,
    pub entry: Option<[u8; ENTRY_SIZE]>,
    pub team: Option<u8>,
}

/// Per-player state of the log.
#[derive(Debug, Default)]
pub(crate) struct PlayerDown {
    tracker: ChangeTracker,
    /// (time ms, raw changes) of the last [`CONTEXT_MS`].
    history: VecDeque<(u64, Vec<ByteChange>)>,
    last_entry: Option<[u8; ENTRY_SIZE]>,
    last_team: Option<u8>,
    seen: bool,
    /// T-032: a MARK asked for a context window at the next sample.
    forced: Option<u32>,
    /// (trigger time, window end) while the "after" lines are being written.
    post: Option<(u64, u64)>,
}

impl PlayerDown {
    /// T-032: make the next sample a trigger (the 2 s before/after change tracker runs around a MARK too).
    pub(crate) fn force(&mut self, mark: u32) {
        self.forced = Some(mark);
    }

    /// Process one sample taken at `now_ms`; returns the log lines (empty when nothing is to be said).
    /// `label` = `LOCAL[0]`, `stamp` = `id=0x.. t=+..ms ts=..`.
    pub(crate) fn step(
        &mut self,
        now_ms: u64,
        label: &str,
        stamp: &str,
        s: &DownSample,
    ) -> Vec<String> {
        let changes = s
            .bytes
            .as_deref()
            .map(|b| self.tracker.feed(b))
            .unwrap_or_default();
        if !changes.is_empty() {
            self.history.push_back((now_ms, changes.clone()));
        }
        while self
            .history
            .front()
            .is_some_and(|(t, _)| t.saturating_add(CONTEXT_MS) < now_ms)
        {
            self.history.pop_front();
        }
        let entry_changed = s.entry.is_some() && s.entry != self.last_entry;
        let team_changed = s.team.is_some() && s.team != self.last_team;
        let forced = self.forced.take();
        let trigger = !self.seen || entry_changed || team_changed || forced.is_some();
        let mut lines = Vec::new();
        if trigger {
            let mut main = format!("{TAG} {label} {stamp}");
            if let Some(n) = forced {
                let _ = write!(main, " MARK #{n}");
            }
            match (self.last_entry, s.entry) {
                (None, Some(e)) => {
                    let _ = write!(main, " first sample: entry=[{}]", hex16(&e));
                }
                (Some(old), Some(e)) if old != e => {
                    let diffs: Vec<String> = (0..ENTRY_SIZE)
                        .filter(|&i| old[i] != e[i])
                        .map(|i| format!("+{i} {:02X}->{:02X}", old[i], e[i]))
                        .collect();
                    let _ = write!(
                        main,
                        " entry=[{}] (was [{}]) {}",
                        hex16(&e),
                        hex16(&old),
                        diffs.join(" ")
                    );
                }
                (_, Some(e)) => {
                    let _ = write!(main, " entry=[{}] (unchanged)", hex16(&e));
                }
                (_, None) => main.push_str(" entry=[unreadable]"),
            }
            let team = |t: Option<u8>| t.map_or("?".to_string(), |t| t.to_string());
            let _ = write!(main, " team={}", team(s.team));
            if team_changed && self.last_team.is_some() {
                let _ = write!(main, " (was {})", team(self.last_team));
            }
            lines.push(main);
            for (t, ch) in &self.history {
                let kept = self.tracker.keep(ch);
                if !kept.is_empty() {
                    lines.push(format!(
                        "{TAG} {label}   before t-{}ms {}",
                        now_ms.saturating_sub(*t),
                        fmt_changes(&kept)
                    ));
                }
            }
            self.post = Some((now_ms, now_ms.saturating_add(CONTEXT_MS)));
        } else if let Some((from, until)) = self.post {
            if now_ms > until {
                self.post = None;
            } else {
                let kept = self.tracker.keep(&changes);
                if !kept.is_empty() {
                    lines.push(format!(
                        "{TAG} {label}   after +{}ms {}",
                        now_ms.saturating_sub(from),
                        fmt_changes(&kept)
                    ));
                }
            }
        }
        self.seen = true;
        if s.entry.is_some() {
            self.last_entry = s.entry;
        }
        if s.team.is_some() {
            self.last_team = s.team;
        }
        lines
    }
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Orchestrator: schedule and per-player state. Reads game memory; everything else is in [`PlayerDown`].
pub(crate) struct DownWatch {
    wcm: usize,
    epoch: Option<Instant>,
    last: Option<Instant>,
    players: HashMap<usize, PlayerDown>,
    /// T-032: a MARK number to apply to every player at the next sample.
    pending_mark: Option<u32>,
}

impl DownWatch {
    pub(crate) fn new() -> Self {
        DownWatch {
            wcm: 0,
            epoch: None,
            last: None,
            players: HashMap::new(),
            pending_mark: None,
        }
    }

    /// T-032: a MARK was logged; the next sample of every player starts a context window.
    pub(crate) fn request_mark(&mut self, mark: u32) {
        self.pending_mark = Some(mark);
    }

    /// One pass; cheap when no sample is due. `wcm` = WorldChrMan instance (0 = not live).
    pub(crate) fn tick(&mut self, wcm: usize, targets: &[Target]) {
        if wcm == 0 {
            self.pending_mark = None;
            self.wcm = 0;
            self.epoch = None;
            self.last = None;
            self.players.clear();
            return;
        }
        if wcm != self.wcm {
            self.wcm = wcm;
            self.epoch = Some(Instant::now());
            self.last = None;
            self.players.clear();
            tracing::info!(
                "{TAG}: new WorldChrMan 0x{wcm:X}: sampling every {SAMPLE_MS} ms: entry bytes, team_type, PlayerIns+0x0..0x{WATCH_LEN:X}; lines only when the entry or team_type changes, with +-{CONTEXT_MS} ms of byte changes"
            );
        }
        let Some(epoch) = self.epoch else { return };
        if self
            .last
            .is_some_and(|t| t.elapsed().as_millis() < u128::from(SAMPLE_MS))
        {
            return;
        }
        self.last = Some(Instant::now());
        let now_ms = u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.players
            .retain(|a, _| targets.iter().any(|t| t.addr == *a));
        let mut rc = RegionCache::new();
        for t in targets {
            let bytes = rc.read_prefix(t.addr, WATCH_LEN);
            let entry = (t.entry_addr != 0)
                .then(|| rc.read_prefix(t.entry_addr, ENTRY_SIZE))
                .and_then(|v| <[u8; ENTRY_SIZE]>::try_from(v).ok());
            let sample = DownSample {
                bytes: (bytes.len() == WATCH_LEN).then_some(bytes),
                entry,
                // ChrIns+0x74 team_type: VERIFIED on NR 1.3.3.0 (TECHNICAL_NOTES section 4), guarded read.
                team: rc.read::<u8>(t.addr + CHR_TEAM),
            };
            let id = rc.read::<u64>(t.addr + CHR_PLAYER_ID);
            let stamp = format!(
                "id={} t=+{now_ms}ms ts={}",
                id.map_or("?".to_string(), |v| format!("0x{v:X}")),
                unix_ms()
            );
            let pending = self.pending_mark;
            let pd = self.players.entry(t.addr).or_default();
            if let Some(n) = pending {
                pd.force(n);
            }
            let lines = pd.step(now_ms, &t.label(), &stamp, &sample);
            for l in lines {
                tracing::info!("{l}");
            }
        }
        self.pending_mark = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const E_UP: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 4, 0, 0, 0, 0, 0, 0];
    const E_DOWN: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 0, 0, 0, 0, 0, 0];

    fn buf() -> Vec<u8> {
        vec![0; WATCH_LEN]
    }

    fn sample(bytes: &[u8], entry: [u8; 16], team: u8) -> DownSample {
        DownSample {
            bytes: Some(bytes.to_vec()),
            entry: Some(entry),
            team: Some(team),
        }
    }

    #[test]
    fn tracker_reports_changed_bytes_and_seeds_on_the_first_sample() {
        let mut t = ChangeTracker::default();
        let mut b = buf();
        assert!(t.feed(&b).is_empty());
        assert!(t.feed(&b).is_empty());
        b[0x10] = 5;
        b[0x11] = 6;
        let c = t.feed(&b);
        assert_eq!(
            c,
            vec![
                ByteChange {
                    off: 0x10,
                    old: 0,
                    new: 5
                },
                ByteChange {
                    off: 0x11,
                    old: 0,
                    new: 6
                }
            ]
        );
        // Unchanged afterwards.
        assert!(t.feed(&b).is_empty());
        // A length change re-seeds.
        assert!(t.feed(&b[..0x20]).is_empty());
    }

    #[test]
    fn offsets_changing_in_more_than_half_of_the_samples_are_dropped() {
        let mut t = ChangeTracker::default();
        let mut b = buf();
        t.feed(&b);
        let mut kept_last = Vec::new();
        for i in 0..40u32 {
            // Offset 0x40 flips every sample (a timer); offset 0x80 changes three times in total.
            b[0x40] = b[0x40].wrapping_add(1);
            if matches!(i, 5 | 17 | 33) {
                b[0x80] ^= 1;
            }
            let c = t.feed(&b);
            kept_last = t.keep(&c);
            if i < NOISE_MIN_SAMPLES - 1 {
                // Not judged yet: the timer is still listed.
                assert!(kept_last.iter().any(|c| c.off == 0x40), "sample {i}");
            }
        }
        assert!(t.noisy(0x40));
        assert!(!t.noisy(0x80));
        // The last sample had only the timer flipping.
        assert!(kept_last.is_empty());
        let c = {
            b[0x80] ^= 1;
            t.feed(&b)
        };
        let kept = t.keep(&c);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].off, 0x80);
    }

    #[test]
    fn at_most_24_offsets_are_printed() {
        let ch: Vec<ByteChange> = (0..30u16)
            .map(|i| ByteChange {
                off: i,
                old: 0,
                new: 1,
            })
            .collect();
        let s = fmt_changes(&ch);
        assert_eq!(s.matches("+0x").count(), MAX_OFFSETS);
        assert!(s.ends_with("(+6 more)"), "{s}");
        assert_eq!(fmt_changes(&ch[..2]), "+0x0:00>01 +0x1:00>01");
    }

    #[test]
    fn it_logs_on_the_first_sample_then_only_on_entry_or_team_changes() {
        let mut p = PlayerDown::default();
        let b = buf();
        let l = p.step(0, "LOCAL[0]", "id=1", &sample(&b, E_UP, 29));
        assert_eq!(l.len(), 1);
        assert!(l[0].contains("T-031 DOWN UNVERIFIED LOCAL[0]") && l[0].contains("first sample"));
        // Quiet while nothing changes, even if PlayerIns bytes change outside the 2 s after a trigger.
        let mut b2 = b.clone();
        for t in 1..60u64 {
            b2[0x100] = t as u8;
            let l = p.step(t * 100, "LOCAL[0]", "id=1", &sample(&b2, E_UP, 29));
            // Within 2 s of the first sample the byte changes are listed ("after"), later nothing.
            if t * 100 > CONTEXT_MS {
                assert!(l.is_empty(), "t={t}: {l:?}");
            }
        }
    }

    #[test]
    fn a_mark_forces_a_context_window_without_any_entry_change() {
        let mut p = PlayerDown::default();
        let mut b = buf();
        p.step(0, "REMOTE[1]", "s", &sample(&b, E_UP, 29));
        for t in 1..=30u64 {
            p.step(t * 100, "REMOTE[1]", "s", &sample(&b, E_UP, 29));
        }
        // A byte changes quietly, then a MARK arrives: the next sample logs a line naming the mark, with the "before" change.
        b[0x1A4] = 7;
        assert!(p
            .step(3100, "REMOTE[1]", "s", &sample(&b, E_UP, 29))
            .is_empty());
        p.force(4);
        let l = p.step(3200, "REMOTE[1]", "s", &sample(&b, E_UP, 29));
        assert!(
            l[0].contains("MARK #4") && l[0].contains("(unchanged)"),
            "{l:?}"
        );
        assert!(
            l.iter()
                .any(|x| x.contains("before") && x.contains("+0x1A4:00>07")),
            "{l:?}"
        );
        // The window then follows with "after" lines; the mark is consumed.
        b[0x1A5] = 1;
        let l = p.step(3300, "REMOTE[1]", "s", &sample(&b, E_UP, 29));
        assert!(
            l.iter()
                .any(|x| x.contains("after") && x.contains("+0x1A5:00>01")),
            "{l:?}"
        );
        assert!(!l.iter().any(|x| x.contains("MARK")));
    }

    #[test]
    fn a_downed_transition_is_logged_with_before_and_after_context() {
        let mut p = PlayerDown::default();
        let mut b = buf();
        p.step(0, "REMOTE[1]", "s", &sample(&b, E_UP, 29));
        // 2.5 s of calm, then a change at +0x1A4 one sample before the entry flips, then the flip.
        for t in 1..=25u64 {
            p.step(t * 100, "REMOTE[1]", "s", &sample(&b, E_UP, 29));
        }
        b[0x1A4] = 7;
        let l = p.step(2600, "REMOTE[1]", "s", &sample(&b, E_UP, 29));
        assert!(l.is_empty(), "{l:?}");
        b[0x1A5] = 1;
        let l = p.step(2700, "REMOTE[1]", "s", &sample(&b, E_DOWN, 29));
        assert!(l[0].contains("+9 04->02"), "{}", l[0]);
        assert!(l[0].contains("team=29"));
        assert!(l.iter().any(|x| x.contains("before t-100ms +0x1A4:00>07")));
        assert!(l.iter().any(|x| x.contains("before t-0ms +0x1A5:00>01")));
        // After the trigger the next byte changes are listed with their offset in time.
        b[0x1B0] = 9;
        let l = p.step(2900, "REMOTE[1]", "s", &sample(&b, E_DOWN, 29));
        assert_eq!(l.len(), 1);
        assert!(l[0].contains("after +200ms +0x1B0:00>09"), "{}", l[0]);
        // The window ends after 2 s.
        b[0x1B1] = 9;
        let l = p.step(4600, "REMOTE[1]", "s", &sample(&b, E_DOWN, 29));
        assert_eq!(l.len(), 1);
        b[0x1B2] = 9;
        assert!(p
            .step(4800, "REMOTE[1]", "s", &sample(&b, E_DOWN, 29))
            .is_empty());
        // Back up.
        let l = p.step(5000, "REMOTE[1]", "s", &sample(&b, E_UP, 29));
        assert!(l[0].contains("+9 02->04"), "{}", l[0]);
    }

    #[test]
    fn a_team_type_change_alone_triggers_and_an_unreadable_sample_does_not() {
        let mut p = PlayerDown::default();
        let b = buf();
        p.step(0, "L", "s", &sample(&b, E_UP, 2));
        let l = p.step(100, "L", "s", &sample(&b, E_UP, 29));
        assert_eq!(l.len(), 1);
        assert!(l[0].contains("team=29 (was 2)"), "{}", l[0]);
        // Everything unreadable for a sample: no trigger, no panic.
        let none = DownSample::default();
        assert!(p.step(200, "L", "s", &none).is_empty());
        // And the same values again afterwards: still quiet.
        assert!(p.step(300, "L", "s", &sample(&b, E_UP, 29)).is_empty());
    }

    #[test]
    fn the_watcher_survives_garbage_targets_and_resets_with_the_world() {
        let mut w = DownWatch::new();
        w.tick(0, &[]);
        let t = [Target {
            index: 0,
            addr: 0x20,
            entry_addr: 0x30,
            local: true,
        }];
        w.tick(0x1000_0000, &t);
        assert_eq!(w.players.len(), 1);
        w.tick(0x2000_0000, &[]);
        assert!(w.players.is_empty());
        w.tick(0, &t);
        assert!(w.players.is_empty());
    }

    #[test]
    fn the_watcher_reads_a_synthetic_player() {
        let mut mem = vec![0u8; 0x800];
        let base = (mem.as_ptr() as usize + 15) & !15;
        let off = base - mem.as_ptr() as usize;
        mem[off + 0x74] = 29;
        mem[off + 0x400 + 9] = 4;
        let mut w = DownWatch::new();
        let t = [Target {
            index: 3,
            addr: base,
            entry_addr: base + 0x400,
            local: false,
        }];
        w.tick(0x1000_0000, &t);
        let pd = w.players.get(&base).expect("player state");
        assert_eq!(pd.last_entry.map(|e| e[9]), Some(4));
        assert_eq!(pd.last_team, Some(29));
        drop(mem);
    }
}
