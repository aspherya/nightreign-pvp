//! T-007a steps 2 and 3: read-only discovery of ChrIns-derived objects reachable from the WorldChrMan
//! instance, plus hex dumps of what was found.
//!
//! Method (all reads are VirtualQuery-guarded via [`mem`], answered from a per-run [`mem::RegionCache`];
//! nothing is ever written). Every loop checks a wall-clock [`Clock`], so a run is bounded in time:
//! 1. Snapshot up to [`SCAN_LEN`] bytes of the WorldChrMan instance.
//! 2. Pass 1 (cheap, whole snapshot first): for every 8-aligned qword that is a user-space pointer, read the
//!    pointee's first qword (its vtable). A `ChrIns`-derived vtable is a direct member. Pointers to a
//!    `ChrSet`-named vtable and pointers to unknown objects (array candidates) are only *collected* here.
//! 3. Pass 2 (own budget): walk ChrSet candidates, then probe array candidates: a 256-byte prefix first
//!    (16 entries at stride 0x10, 32 at stride 0x8) and only if it already contains a `ChrIns` hit, the
//!    full array.
//! 4. Dump the first [`DUMP_LEN`] bytes of every PlayerIns found (capped) and a diverse set of other ChrIns.
//!
//! T-007a run 1 (2026-10-09) hung after "instance vtable" because the old single-pass design spent up to
//! ~64 KB of copying and hundreds of uncached VirtualQuery syscalls per unknown pointer, with no time bound.
//!
//! ChrSet shape used only to *recognise* candidates (`capacity` at +0x10, entries pointer at +0x18, 0x10-byte
//! entries whose first qword is the ChrIns pointer): ER layout, er-reference.md section 2, fsrs@59fbd3b
//! `cs/world_chr_man.rs`. It is a hint, not an assumption: everything is reported with the observed offsets.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::mem::{self, va_rva, RegionCache};
use super::rtti::Rtti;

/// How much of the WorldChrMan instance to scan (card: up to 0x40000 bytes).
pub(crate) const SCAN_LEN: usize = 0x40000;
/// Bytes dumped per object.
pub(crate) const DUMP_LEN: usize = 0x400;
/// Log-line budget per discovery run (card: <= ~1500).
pub(crate) const LINE_BUDGET: usize = 1500;
/// Lines one full dump needs at most: header + 64 hex + candidate summary + wrapped candidate list.
const DUMP_LINES_MAX: usize = 1 + DUMP_LEN / 16 + 1 + CAND_LINES_MAX;
const CAND_LINES_MAX: usize = 6;
const CAND_PER_LINE: usize = 20;
/// u8 values worth flagging inside dumps: the team-type enum candidates (TECHNICAL_NOTES.md section 4,
/// Smithbox TEAM_TYPE: Player, White/Black/Grey ghost, Enemy, Boss, Invader x2, Indiscriminate).
pub(crate) const TEAM_CANDIDATES: [u8; 9] = [1, 2, 3, 4, 6, 7, 13, 16, 29];

const MAX_PLAYER_DUMPS: usize = 4;
const MAX_OTHER_DUMPS: usize = 8;
/// Wall-clock budgets (card fix 2026-10-09): pass 1 (direct members, whole snapshot), pass 2 (ChrSet and
/// array probing), and the dumps/table. Worst case for a whole run is the sum, 2 s.
pub(crate) const PASS1_BUDGET: Duration = Duration::from_millis(500);
pub(crate) const PASS2_BUDGET: Duration = Duration::from_millis(1000);
pub(crate) const DUMP_BUDGET: Duration = Duration::from_millis(500);
/// Entries examined per candidate array when no capacity hint is available / the maximum with one.
const DEFAULT_ARRAY_ENTRIES: usize = 256;
const MAX_ARRAY_ENTRIES: usize = 4096;
/// Array prefix probe: this many 0x10-stride entries (twice as many at stride 8) are read first; the full
/// array is only read if the prefix already holds a ChrIns hit.
const PREFIX_ENTRIES: usize = 16;

const DIRECT_LINES_MAX: usize = 80;
const CHRSET_LINES_MAX: usize = 30;
const ARRAY_LINES_MAX: usize = 80;
const TABLE_LINES_MAX: usize = 100;
const FALLBACK_LINES_MAX: usize = 150;

// ---------------------------------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------------------------------

fn le32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(off..off.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn le64(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        b.get(off..off.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// Plausible user-space object pointer: above the null region, below the canonical user limit, 8-aligned.
pub(crate) fn is_ptr(v: usize) -> bool {
    (0x1_0000..0x7FFF_FFFF_FFFF).contains(&v) && v.is_multiple_of(8)
}

/// Hex dump lines, 16 bytes per line: `+0000  xx xx .. xx  xx .. xx  |ascii|`.
pub(crate) fn hex_lines(bytes: &[u8]) -> Vec<String> {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(i, chunk)| {
            let mut s = format!("+{:04X} ", i * 16);
            for (k, b) in chunk.iter().enumerate() {
                if k == 8 {
                    s.push(' ');
                }
                let _ = write!(s, " {b:02X}");
            }
            // Pad short final lines so the ASCII column lines up.
            for k in chunk.len()..16 {
                if k == 8 {
                    s.push(' ');
                }
                s.push_str("   ");
            }
            s.push_str("  |");
            s.extend(chunk.iter().map(|&b| {
                if (0x20..0x7F).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            }));
            s.push('|');
            s
        })
        .collect()
}

/// Offsets (and values) of every byte in `bytes` whose value is one of [`TEAM_CANDIDATES`].
pub(crate) fn team_candidates(bytes: &[u8]) -> Vec<(usize, u8)> {
    bytes
        .iter()
        .enumerate()
        .filter(|(_, b)| TEAM_CANDIDATES.contains(b))
        .map(|(i, &b)| (i, b))
        .collect()
}

/// Compact wrapped list `+06C=1 +070=4 ...`, at most `max_lines` lines of [`CAND_PER_LINE`] items.
pub(crate) fn candidate_lines(c: &[(usize, u8)], max_lines: usize) -> Vec<String> {
    let shown = (max_lines * CAND_PER_LINE).min(c.len());
    let mut lines: Vec<String> = c[..shown]
        .chunks(CAND_PER_LINE)
        .map(|ch| {
            ch.iter()
                .map(|(o, v)| format!("+{o:03X}={v}"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    if shown < c.len() {
        lines.push(format!("(+{} more candidates not shown)", c.len() - shown));
    }
    lines
}

/// Line-budgeted logger. Lines past the budget are counted and dropped.
pub(crate) struct Budget<F: FnMut(&str)> {
    left: usize,
    dropped: usize,
    sink: F,
}

impl<F: FnMut(&str)> Budget<F> {
    pub(crate) fn new(total: usize, sink: F) -> Self {
        Budget {
            left: total,
            dropped: 0,
            sink,
        }
    }
    pub(crate) fn line(&mut self, s: impl AsRef<str>) {
        if self.left > 0 {
            (self.sink)(s.as_ref());
            self.left -= 1;
        } else {
            self.dropped += 1;
        }
    }
    pub(crate) fn remaining(&self) -> usize {
        self.left
    }
}

/// Wall-clock deadline checked from inside loops. Sticky: once expired it stays expired.
pub(crate) struct Clock {
    start: Instant,
    limit: Duration,
    ticks: u32,
    expired: bool,
}

impl Clock {
    pub(crate) fn new(limit: Duration) -> Self {
        Clock {
            start: Instant::now(),
            limit,
            ticks: 0,
            expired: false,
        }
    }

    /// True once the budget is spent. Reads the system clock on the first call and then every 16th, which
    /// keeps the check negligible next to a guarded read (and so lets it sit in the innermost loops).
    pub(crate) fn expired(&mut self) -> bool {
        if self.expired {
            return true;
        }
        let t = self.ticks;
        self.ticks = t.wrapping_add(1);
        if t.is_multiple_of(16) && self.start.elapsed() >= self.limit {
            self.expired = true;
        }
        self.expired
    }

    pub(crate) fn is_expired(&self) -> bool {
        self.expired
    }

    pub(crate) fn ms(&self) -> u128 {
        self.start.elapsed().as_millis()
    }
}

/// One-at-a-time gate for the discovery worker thread. `try_acquire` fails while a run is in flight and
/// reports how long it has been running; the guard releases the gate on drop (also on panic or when the
/// thread could not be spawned and the closure holding the guard is dropped).
pub(crate) struct Gate {
    busy: AtomicBool,
    since_ms: AtomicU64,
    epoch: Instant,
}

pub(crate) struct GateGuard(Arc<Gate>);

impl Gate {
    pub(crate) fn new() -> Arc<Gate> {
        Arc::new(Gate {
            busy: AtomicBool::new(false),
            since_ms: AtomicU64::new(0),
            epoch: Instant::now(),
        })
    }

    /// `Ok(guard)` if the caller now owns the gate, `Err(running_ms)` if a run is already in flight.
    pub(crate) fn try_acquire(self: &Arc<Self>) -> Result<GateGuard, u64> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.since_ms.store(self.now_ms(), Ordering::SeqCst);
            Ok(GateGuard(Arc::clone(self)))
        } else {
            Err(self.running_ms())
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    pub(crate) fn running_ms(&self) -> u64 {
        self.now_ms()
            .saturating_sub(self.since_ms.load(Ordering::SeqCst))
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }
}

impl Drop for GateGuard {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------------------------------

/// A ChrIns-derived object found during a scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Obj {
    pub addr: usize,
    pub class: usize,
    /// Where it was found, e.g. `direct +0x10` or `array@+0x40 stride 0x10 [3]`.
    pub source: String,
}

#[derive(Clone, Debug)]
pub(crate) struct DirectHit {
    pub off: usize,
    pub addr: usize,
    pub class: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ArrayHit {
    pub off: usize,
    pub ptr: usize,
    pub stride: usize,
    pub scanned: usize,
    pub hits: usize,
    pub hist: Vec<(usize, usize)>,
    pub cap_hint: Option<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct ChrSetHit {
    /// Offset inside the WorldChrMan instance (of the ChrSet itself if inline, of the pointer if not).
    pub off: usize,
    pub via_ptr: bool,
    pub addr: usize,
    pub class: usize,
    pub capacity: u32,
    pub entries: usize,
    pub header_ok: bool,
    pub scanned: usize,
    pub hits: usize,
    pub hist: Vec<(usize, usize)>,
}

/// Wall-clock budgets of the two scan passes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Budgets {
    pub pass1: Duration,
    pub pass2: Duration,
}

impl Default for Budgets {
    fn default() -> Self {
        Budgets {
            pass1: PASS1_BUDGET,
            pass2: PASS2_BUDGET,
        }
    }
}

/// How one pass ended.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PassStats {
    pub ms: u128,
    /// Offset inside the instance being processed when the time budget ran out (`None` = completed).
    pub stopped_at: Option<usize>,
    pub done: usize,
    pub total: usize,
}

#[derive(Default)]
pub(crate) struct ScanResult {
    pub scanned_len: usize,
    pub pointers: usize,
    pub derefs: usize,
    pub snapshot_ms: u128,
    pub pass1: PassStats,
    pub pass2: PassStats,
    /// ChrSet candidates / array candidates collected by pass 1, and array candidates skipped as duplicates.
    pub chrset_cands: usize,
    pub array_cands: usize,
    pub array_dups: usize,
    /// Arrays whose 256-byte prefix already held a ChrIns hit (and were therefore read in full).
    pub array_extended: usize,
    /// `VirtualQuery` syscalls and guarded reads over the whole run.
    pub vq_calls: usize,
    pub region_lookups: usize,
    pub direct: Vec<DirectHit>,
    pub arrays: Vec<ArrayHit>,
    pub chrsets: Vec<ChrSetHit>,
    pub objects: Vec<Obj>,
}

fn hist_of(counts: &HashMap<usize, usize>) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = counts.iter().map(|(&k, &n)| (k, n)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

fn fmt_hist(rtti: &Rtti, h: &[(usize, usize)]) -> String {
    h.iter()
        .map(|&(c, n)| format!("{} x{n}", rtti.name(c)))
        .collect::<Vec<_>>()
        .join(", ")
}

struct Walk {
    scanned: usize,
    /// (entry index, object address, class)
    hits: Vec<(usize, usize, usize)>,
}

/// A pointer in the instance to a ChrSet-named object, or an inline ChrSet, to walk in pass 2.
struct ChrSetCand {
    off: usize,
    via_ptr: bool,
    addr: usize,
    class: usize,
}

/// A pointer in the instance to an unknown object that might be an array of ChrIns pointers.
struct ArrayCand {
    off: usize,
    ptr: usize,
    cap_hint: Option<u32>,
}

struct Scanner<'a> {
    rtti: &'a Rtti,
    regions: RegionCache,
    vt_cache: HashMap<usize, Option<usize>>,
    clock: Clock,
    derefs: usize,
    seen: HashSet<usize>,
    objects: Vec<Obj>,
}

impl<'a> Scanner<'a> {
    fn new(rtti: &'a Rtti, limit: Duration) -> Self {
        Scanner {
            rtti,
            regions: RegionCache::new(),
            vt_cache: HashMap::new(),
            clock: Clock::new(limit),
            derefs: 0,
            seen: HashSet::new(),
            objects: Vec::new(),
        }
    }

    fn start_pass(&mut self, limit: Duration) {
        self.clock = Clock::new(limit);
    }

    /// First qword of the object at `p` (its vtable pointer, if it is an object), guarded and cached.
    /// Callers bound the number of calls with [`Clock::expired`].
    fn vtable_of(&mut self, p: usize) -> Option<usize> {
        if let Some(c) = self.vt_cache.get(&p) {
            return *c;
        }
        self.derefs += 1;
        let v = self.regions.read::<usize>(p);
        self.vt_cache.insert(p, v);
        v
    }

    /// Class of the ChrIns-derived object at `v`, if `v` points to one.
    fn chr_class(&mut self, v: usize) -> Option<usize> {
        if !is_ptr(v) {
            return None;
        }
        let vt = self.vtable_of(v)?;
        let c = self.rtti.class_of_vtable(vt)?;
        self.rtti.is_chr(c).then_some(c)
    }

    fn add_object(&mut self, addr: usize, class: usize, source: String) {
        if self.seen.insert(addr) {
            self.objects.push(Obj {
                addr,
                class,
                source,
            });
        }
    }

    /// Examine up to `max_entries` entries of `stride` bytes in `buf`; entry = qword at the entry start.
    /// Stops early (partial result) when the pass clock expires.
    fn walk(&mut self, buf: &[u8], stride: usize, max_entries: usize) -> Walk {
        let n = (buf.len() / stride).min(max_entries);
        let mut hits = Vec::new();
        let mut scanned = 0;
        for i in 0..n {
            if self.clock.expired() {
                break;
            }
            scanned += 1;
            if let Some(v) = le64(buf, i * stride) {
                if let Some(c) = self.chr_class(v as usize) {
                    hits.push((i, v as usize, c));
                }
            }
        }
        Walk { scanned, hits }
    }

    /// Walk a ChrSet-shaped object (ER hint: capacity u32 @+0x10, entries ptr @+0x18, 0x10-byte entries).
    fn walk_chrset(&mut self, c: &ChrSetCand) -> ChrSetHit {
        let hdr = self.regions.read_prefix(c.addr, 0x20);
        let capacity = le32(&hdr, 0x10).unwrap_or(0);
        let entries = le64(&hdr, 0x18).unwrap_or(0) as usize;
        let mut hit = ChrSetHit {
            off: c.off,
            via_ptr: c.via_ptr,
            addr: c.addr,
            class: c.class,
            capacity,
            entries,
            header_ok: false,
            scanned: 0,
            hits: 0,
            hist: Vec::new(),
        };
        if capacity == 0 || capacity > 0x10000 || !is_ptr(entries) {
            return hit;
        }
        hit.header_ok = true;
        let n = (capacity as usize).min(MAX_ARRAY_ENTRIES);
        let buf = self.regions.read_prefix(entries, n * 16);
        let w = self.walk(&buf, 16, n);
        hit.scanned = w.scanned;
        hit.hits = w.hits.len();
        let mut counts = HashMap::new();
        for &(i, a, cl) in &w.hits {
            *counts.entry(cl).or_insert(0) += 1;
            self.add_object(a, cl, format!("chrset@+0x{:X} [{i}]", c.off));
        }
        hit.hist = hist_of(&counts);
        hit
    }

    /// Probe a candidate array at `ptr`. Reads a small prefix first (256 bytes: [`PREFIX_ENTRIES`] entries at
    /// stride 0x10, twice as many at stride 0x8) and only reads the whole array (`cap_hint` entries, else
    /// [`DEFAULT_ARRAY_ENTRIES`]) if that prefix already contains a ChrIns hit. Returns the winning stride and
    /// its walk, or `None` if nothing was found. `extended` is set when the full read happened.
    fn probe_array(
        &mut self,
        ptr: usize,
        cap_hint: Option<u32>,
        extended: &mut bool,
    ) -> Option<(usize, Walk)> {
        let limit = cap_hint
            .map(|c| (c as usize).min(MAX_ARRAY_ENTRIES))
            .unwrap_or(DEFAULT_ARRAY_ENTRIES);
        let head_len = (PREFIX_ENTRIES * 16).min(limit * 16);
        let head = self.regions.read_prefix(ptr, head_len);
        if head.len() < 8 {
            return None;
        }
        let h16 = self.walk(&head, 16, limit.min(PREFIX_ENTRIES));
        let h8 = self.walk(
            &head[..head.len().min(limit * 8)],
            8,
            limit.min(PREFIX_ENTRIES * 2),
        );
        if h16.hits.is_empty() && h8.hits.is_empty() {
            return None;
        }
        *extended = true;
        let abuf = if limit * 16 > head.len() {
            self.regions.read_prefix(ptr, limit * 16)
        } else {
            head
        };
        let w16 = self.walk(&abuf, 16, limit);
        let w8 = self.walk(&abuf[..abuf.len().min(limit * 8)], 8, limit);
        // Stride 8 only wins if it finds strictly more (a 0x10-stride array also scans clean at 8).
        let (stride, w) = if w8.hits.len() > w16.hits.len() {
            (8, w8)
        } else {
            (16, w16)
        };
        (!w.hits.is_empty()).then_some((stride, w))
    }
}

/// Scan `len` bytes of the instance at `inst`. Never panics, never writes. `progress` receives a line when
/// each phase starts (so a stall can be located from the log).
pub(crate) fn scan_instance(
    inst: usize,
    len: usize,
    rtti: &Rtti,
    budgets: Budgets,
    progress: &mut dyn FnMut(&str),
) -> ScanResult {
    let t_snap = Instant::now();
    let mut sc = Scanner::new(rtti, budgets.pass1);
    let buf = sc.regions.read_prefix(inst, len);
    let mut res = ScanResult {
        scanned_len: buf.len(),
        snapshot_ms: t_snap.elapsed().as_millis(),
        ..ScanResult::default()
    };
    progress(&format!(
        "scan: snapshot 0x{:X} bytes in {} ms; pass 1 (direct ChrIns members, budget {} ms) begins",
        buf.len(),
        res.snapshot_ms,
        budgets.pass1.as_millis()
    ));

    // ---- Pass 1: direct members over the WHOLE snapshot, before any array probing. ----
    let n_q = buf.len() / 8;
    let mut chrset_c: Vec<ChrSetCand> = Vec::new();
    let mut arr_c: Vec<ArrayCand> = Vec::new();
    sc.start_pass(budgets.pass1);
    res.pass1.total = n_q;
    for q in 0..n_q {
        let here = q * 8;
        if sc.clock.expired() {
            res.pass1.stopped_at = Some(here);
            break;
        }
        res.pass1.done += 1;
        let Some(v) = le64(&buf, here).map(|v| v as usize) else {
            break;
        };

        // Inline ChrSet: the qword is the vtable of a ChrSet-named class.
        if let Some(ci) = rtti.class_of_vtable(v) {
            if rtti.is_chrset(ci) {
                chrset_c.push(ChrSetCand {
                    off: here,
                    via_ptr: false,
                    addr: inst + here,
                    class: ci,
                });
            }
            continue;
        }
        if !is_ptr(v) {
            continue;
        }
        res.pointers += 1;
        let Some(vt) = sc.vtable_of(v) else { continue };

        match rtti.class_of_vtable(vt) {
            Some(ci) if rtti.is_chr(ci) => {
                res.direct.push(DirectHit {
                    off: here,
                    addr: v,
                    class: ci,
                });
                sc.add_object(v, ci, format!("direct +0x{here:X}"));
            }
            Some(ci) if rtti.is_chrset(ci) => chrset_c.push(ChrSetCand {
                off: here,
                via_ptr: true,
                addr: v,
                class: ci,
            }),
            Some(_) => {} // some other known C++ object: not an array
            None => {
                // Candidate array. ER hint: capacity u32 sits 8 bytes before the entries pointer.
                let cap_hint = here
                    .checked_sub(8)
                    .and_then(|o| le32(&buf, o))
                    .filter(|&c| (1..=0x10000).contains(&c));
                arr_c.push(ArrayCand {
                    off: here,
                    ptr: v,
                    cap_hint,
                });
            }
        }
    }
    res.pass1.ms = sc.clock.ms();
    // Probe likely candidates first: ER puts the capacity right before the entries pointer, so hinted
    // pointers are probed before bare ones (stable: offset order otherwise).
    arr_c.sort_by_key(|a| a.cap_hint.is_none());
    res.chrset_cands = chrset_c.len();
    res.array_cands = arr_c.len();

    // ---- Pass 2: ChrSet walks, then array probes, in its own budget. ----
    progress(&format!(
        "scan: pass 1 done in {} ms ({} direct); pass 2 (ChrSet/array probing, budget {} ms) begins with {} ChrSet and {} array candidates",
        res.pass1.ms,
        res.direct.len(),
        budgets.pass2.as_millis(),
        chrset_c.len(),
        arr_c.len()
    ));
    sc.start_pass(budgets.pass2);
    res.pass2.total = chrset_c.len() + arr_c.len();
    let mut last_off = 0usize;
    'pass2: {
        for c in &chrset_c {
            if sc.clock.expired() {
                res.pass2.stopped_at = Some(c.off);
                break 'pass2;
            }
            last_off = c.off;
            let h = sc.walk_chrset(c);
            res.chrsets.push(h);
            res.pass2.done += 1;
        }
        let mut seen_ptr: HashSet<usize> = HashSet::new();
        for a in &arr_c {
            if sc.clock.expired() {
                res.pass2.stopped_at = Some(a.off);
                break 'pass2;
            }
            last_off = a.off;
            res.pass2.done += 1;
            if !seen_ptr.insert(a.ptr) {
                res.array_dups += 1;
                continue;
            }
            let mut extended = false;
            let found = sc.probe_array(a.ptr, a.cap_hint, &mut extended);
            if extended {
                res.array_extended += 1;
            }
            let Some((stride, w)) = found else { continue };
            let mut counts = HashMap::new();
            for &(i, obj, c) in &w.hits {
                *counts.entry(c).or_insert(0) += 1;
                sc.add_object(
                    obj,
                    c,
                    format!("array@+0x{:X} stride 0x{stride:X} [{i}]", a.off),
                );
            }
            res.arrays.push(ArrayHit {
                off: a.off,
                ptr: a.ptr,
                stride,
                scanned: w.scanned,
                hits: w.hits.len(),
                hist: hist_of(&counts),
                cap_hint: a.cap_hint,
            });
        }
    }
    // The clock can also run out inside the last candidate's walks.
    if res.pass2.stopped_at.is_none() && sc.clock.is_expired() {
        res.pass2.stopped_at = Some(last_off);
    }
    res.pass2.ms = sc.clock.ms();

    res.arrays
        .sort_by(|a, b| b.hits.cmp(&a.hits).then(a.off.cmp(&b.off)));
    res.derefs = sc.derefs;
    res.vq_calls = sc.regions.queries;
    res.region_lookups = sc.regions.lookups;
    res.objects = sc.objects;
    res
}

/// Indices into `objs` to hex-dump: up to [`MAX_PLAYER_DUMPS`] player-derived objects, then up to
/// [`MAX_OTHER_DUMPS`] others, one per class first, then in discovery order.
pub(crate) fn select_dump_targets(objs: &[Obj], rtti: &Rtti) -> Vec<usize> {
    let mut out: Vec<usize> = objs
        .iter()
        .enumerate()
        .filter(|(_, o)| rtti.is_player(o.class))
        .map(|(i, _)| i)
        .take(MAX_PLAYER_DUMPS)
        .collect();
    let others: Vec<usize> = objs
        .iter()
        .enumerate()
        .filter(|(_, o)| !rtti.is_player(o.class))
        .map(|(i, _)| i)
        .collect();
    let mut picked: Vec<usize> = Vec::new();
    let mut classes: HashSet<usize> = HashSet::new();
    for &i in &others {
        if picked.len() < MAX_OTHER_DUMPS && classes.insert(objs[i].class) {
            picked.push(i);
        }
    }
    for &i in &others {
        if picked.len() < MAX_OTHER_DUMPS && !picked.contains(&i) {
            picked.push(i);
        }
    }
    picked.sort_unstable();
    out.extend(picked);
    out
}

/// Result of [`fallback_vtable_histogram`].
pub(crate) struct Fallback {
    /// (vtable RVA, count, first offset), most frequent first.
    pub hist: Vec<(usize, usize, usize)>,
    pub scanned_len: usize,
    /// Offset where the time budget ran out (`None` = whole block scanned).
    pub stopped_at: Option<usize>,
    pub ms: u128,
}

/// No ChrIns vtable known (RTTI failed): histogram of "pointer in the block whose pointee's first qword is a
/// pointer into the exe image" by that vtable's RVA, so the analyst can still spot object types by hand.
/// Bounded by `budget` of wall-clock time.
pub(crate) fn fallback_vtable_histogram(
    rtti: &Rtti,
    inst: usize,
    len: usize,
    exe_base: usize,
    exe_size: usize,
    budget: Duration,
) -> Fallback {
    let mut sc = Scanner::new(rtti, budget);
    let buf = sc.regions.read_prefix(inst, len);
    let mut map: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    let mut stopped_at = None;
    let mut off = 0usize;
    while off + 8 <= buf.len() {
        let here = off;
        off += 8;
        if sc.clock.expired() {
            stopped_at = Some(here);
            break;
        }
        let Some(v) = le64(&buf, here).map(|v| v as usize) else {
            break;
        };
        if !is_ptr(v) {
            continue;
        }
        if let Some(vt) = sc.vtable_of(v) {
            if vt >= exe_base && vt - exe_base < exe_size {
                let e = map.entry(vt - exe_base).or_insert((0, here));
                e.0 += 1;
            }
        }
    }
    let mut hist: Vec<(usize, usize, usize)> = map
        .into_iter()
        .map(|(rva, (n, first))| (rva, n, first))
        .collect();
    hist.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Fallback {
        hist,
        scanned_len: buf.len(),
        stopped_at,
        ms: sc.clock.ms(),
    }
}

// ---------------------------------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------------------------------

/// Startup report of the RTTI recovery: stats, the key classes, every ChrIns-derived class and every
/// ChrSet-named class, each with vtable RVAs (card step 1: "Log class -> vtable RVA").
pub(crate) fn log_rtti(rtti: &Rtti) {
    const MAX_CLASS_LINES: usize = 60;
    let mut b = Budget::new(200, |s: &str| tracing::info!("T-007a {s}"));
    let st = rtti.stats;
    b.line(format!(
        "RTTI: {} TypeDescriptors, {} COLs ({} primary/offset 0), {} classes with a vtable, {} ChrIns-derived",
        st.type_descriptors,
        st.cols_all,
        st.cols_primary,
        st.classes_with_vtable,
        rtti.chr_class_count()
    ));
    let vts = |c: usize| -> String {
        rtti.classes[c]
            .vtables
            .iter()
            .map(|&v| format!("exe+0x{v:X}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    for raw in [
        super::rtti::CHR_INS,
        super::rtti::PLAYER_INS,
        super::rtti::WORLD_CHR_MAN,
    ] {
        let found = rtti.find(raw);
        if found.is_empty() {
            b.line(format!(
                "  key class {raw}: NOT FOUND with a primary vtable"
            ));
        }
        for c in found {
            b.line(format!(
                "  key class {raw}: vtable {} col=exe+0x{:X} td=exe+0x{:X}",
                vts(c),
                rtti.classes[c].col_rva,
                rtti.classes[c].td_rva
            ));
        }
    }
    let chr: Vec<usize> = (0..rtti.classes.len())
        .filter(|&c| rtti.is_chr(c))
        .collect();
    b.line(format!(
        "ChrIns-derived classes ({}; class -> vtable RVA; bases):",
        chr.len()
    ));
    for &c in chr.iter().take(MAX_CLASS_LINES) {
        let bases: Vec<String> = rtti.classes[c]
            .bases
            .iter()
            .map(|&t| rtti.name_of_td(t))
            .collect();
        b.line(format!(
            "  {:<28} vtable {}  bases: {}",
            rtti.name(c),
            vts(c),
            bases.join(" > ")
        ));
    }
    let sets: Vec<usize> = (0..rtti.classes.len())
        .filter(|&c| rtti.is_chrset(c))
        .collect();
    b.line(format!("ChrSet-named classes ({}):", sets.len()));
    for &c in sets.iter().take(MAX_CLASS_LINES) {
        b.line(format!("  {}  vtable {}", rtti.name(c), vts(c)));
    }
}

fn dump_object<F: FnMut(&str)>(
    b: &mut Budget<F>,
    rc: &mut RegionCache,
    rtti: &Rtti,
    o: &Obj,
    exe_base: usize,
    exe_size: usize,
) {
    let bytes = rc.read_prefix(o.addr, DUMP_LEN);
    if bytes.is_empty() {
        b.line(format!(
            "=== DUMP {} @ 0x{:X}: unreadable ===",
            rtti.name(o.class),
            o.addr
        ));
        return;
    }
    let vt = le64(&bytes, 0).unwrap_or(0) as usize;
    b.line(format!(
        "=== DUMP {} @ 0x{:X} vtable={} found={} bytes=0x{:X}/0x{:X} ===",
        rtti.name(o.class),
        o.addr,
        if vt >= exe_base && vt - exe_base < exe_size {
            format!("exe+0x{:X}", vt - exe_base)
        } else {
            format!("0x{vt:X}")
        },
        o.source,
        bytes.len(),
        DUMP_LEN
    ));
    for l in hex_lines(&bytes) {
        b.line(l);
    }
    let cands = team_candidates(&bytes);
    b.line(format!(
        "  u8 values in {:?} at {} offsets (team-type candidates):",
        TEAM_CANDIDATES,
        cands.len()
    ));
    for l in candidate_lines(&cands, CAND_LINES_MAX) {
        b.line(format!("    {l}"));
    }
}

/// `TIME BUDGET EXHAUSTED` lines for the passes that ran out of time (empty if none did).
pub(crate) fn budget_messages(res: &ScanResult) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(off) = res.pass1.stopped_at {
        v.push(format!(
            "TIME BUDGET EXHAUSTED at +0x{off:X} after {} ms (pass 1, direct members: {}/{} qwords examined; partial results follow)",
            res.pass1.ms, res.pass1.done, res.pass1.total
        ));
    }
    if let Some(off) = res.pass2.stopped_at {
        v.push(format!(
            "TIME BUDGET EXHAUSTED at +0x{off:X} after {} ms (pass 2, ChrSet/array probing: {}/{} candidates processed; partial results follow)",
            res.pass2.ms, res.pass2.done, res.pass2.total
        ));
    }
    v
}

/// One full discovery pass: scan, report, dump. `reason` is e.g. `F9` or `auto +10 s`.
/// Runs on the discovery worker thread; bounded by [`PASS1_BUDGET`] + [`PASS2_BUDGET`] + [`DUMP_BUDGET`].
pub(crate) fn run(
    rtti: &Rtti,
    inst: usize,
    reason: &str,
    seq: u32,
    exe_base: usize,
    exe_size: usize,
) {
    let t_total = Instant::now();
    let mut b = Budget::new(LINE_BUDGET, |s: &str| tracing::info!("T-007a {s}"));
    b.line(format!(
        "##### discovery #{seq} ({reason}) WorldChrMan instance={} #####",
        va_rva(inst, exe_base, exe_size)
    ));

    let own_vt = mem::read::<usize>(inst);
    b.line(format!(
        "instance vtable: {}",
        match own_vt {
            None => "<unreadable>".to_string(),
            Some(vt) => match rtti.class_of_vtable(vt) {
                Some(c) => format!("{} ({})", va_rva(vt, exe_base, exe_size), rtti.name(c)),
                None => format!("{} (no RTTI class)", va_rva(vt, exe_base, exe_size)),
            },
        }
    ));

    if rtti.chr_class_count() == 0 {
        b.line(
            "FALLBACK: no ChrIns-derived vtable known (RTTI recovery failed); listing pointee vtables in the exe by RVA",
        );
        let fb = fallback_vtable_histogram(
            rtti,
            inst,
            SCAN_LEN,
            exe_base,
            exe_size,
            PASS1_BUDGET + PASS2_BUDGET,
        );
        b.line(format!(
            "scanned 0x{:X} bytes in {} ms; {} distinct pointee vtables",
            fb.scanned_len,
            fb.ms,
            fb.hist.len()
        ));
        if let Some(off) = fb.stopped_at {
            b.line(format!(
                "TIME BUDGET EXHAUSTED at +0x{off:X} after {} ms (fallback histogram; partial results follow)",
                fb.ms
            ));
        }
        for (rva, count, first) in fb.hist.iter().take(FALLBACK_LINES_MAX) {
            b.line(format!(
                "  vtable exe+0x{rva:X} x{count} (first at +0x{first:X})"
            ));
        }
        finish(&mut b);
        return;
    }

    let res = scan_instance(inst, SCAN_LEN, rtti, Budgets::default(), &mut |s: &str| {
        b.line(s)
    });
    b.line(format!(
        "scanned 0x{:X} bytes: {} pointers, {} derefs; {} direct ChrIns members, {} ChrSet-like, {} array candidates with hits, {} distinct ChrIns objects",
        res.scanned_len,
        res.pointers,
        res.derefs,
        res.direct.len(),
        res.chrsets.len(),
        res.arrays.len(),
        res.objects.len()
    ));
    b.line(format!(
        "timings: snapshot {} ms, pass 1 {} ms, pass 2 {} ms",
        res.snapshot_ms, res.pass1.ms, res.pass2.ms
    ));
    b.line(format!(
        "counters: pass 1 {}/{} qwords; pass 2 {}/{} candidates ({} ChrSet, {} array of which {} duplicate pointers skipped, {} prefix-positive and read in full); {} VirtualQuery calls for {} guarded reads",
        res.pass1.done,
        res.pass1.total,
        res.pass2.done,
        res.pass2.total,
        res.chrset_cands,
        res.array_cands,
        res.array_dups,
        res.array_extended,
        res.vq_calls,
        res.region_lookups
    ));
    for m in budget_messages(&res) {
        b.line(m);
    }

    b.line("-- direct members (offset -> ChrIns-derived object) --");
    for d in res.direct.iter().take(DIRECT_LINES_MAX) {
        b.line(format!(
            "  +0x{:X} -> 0x{:X} {}",
            d.off,
            d.addr,
            rtti.name(d.class)
        ));
    }
    if res.direct.len() > DIRECT_LINES_MAX {
        b.line(format!("  (+{} more)", res.direct.len() - DIRECT_LINES_MAX));
    }

    b.line("-- ChrSet-like objects (ER shape: capacity @+0x10, entries @+0x18, stride 0x10) --");
    for c in res.chrsets.iter().take(CHRSET_LINES_MAX) {
        b.line(format!(
            "  +0x{:X} {} {} @0x{:X} capacity={} entries=0x{:X} header_ok={} scanned={} hits={} [{}]",
            c.off,
            if c.via_ptr { "ptr->" } else { "inline" },
            rtti.name(c.class),
            c.addr,
            c.capacity,
            c.entries,
            c.header_ok,
            c.scanned,
            c.hits,
            fmt_hist(rtti, &c.hist)
        ));
    }

    b.line(
        "-- array candidates (offset holds a pointer to an array of ChrIns*), sorted by hits --",
    );
    for a in res.arrays.iter().take(ARRAY_LINES_MAX) {
        b.line(format!(
            "  +0x{:X} -> array 0x{:X} stride=0x{:X} hits={}/{} cap_hint={} [{}]",
            a.off,
            a.ptr,
            a.stride,
            a.hits,
            a.scanned,
            a.cap_hint.map_or("-".to_string(), |c| c.to_string()),
            fmt_hist(rtti, &a.hist)
        ));
    }
    if res.arrays.len() > ARRAY_LINES_MAX {
        b.line(format!("  (+{} more)", res.arrays.len() - ARRAY_LINES_MAX));
    }

    // Dumps phase: its own region cache (objects may have moved since the scan) and its own budget.
    let mut rc = RegionCache::new();
    let mut dump_clock = Clock::new(DUMP_BUDGET);

    // Per-object table. The +0x60/+0x64/+0x68/+0x6C columns are ER offsets shown only as a convenience.
    b.line(format!(
        "-- ChrIns objects ({}; columns are ER-layout hints, UNVERIFIED on NR: +0x60 u32, +0x64 u32, +0x68 i32, +0x6C u8) --",
        res.objects.len()
    ));
    for o in res.objects.iter().take(TABLE_LINES_MAX) {
        if dump_clock.expired() {
            b.line(format!(
                "TIME BUDGET EXHAUSTED after {} ms (object table; partial results follow)",
                dump_clock.ms()
            ));
            break;
        }
        let mut u32s = |off: usize| {
            rc.read::<u32>(o.addr + off)
                .map_or("?".to_string(), |v| v.to_string())
        };
        let (c60, c64) = (u32s(0x60), u32s(0x64));
        let c68 = rc
            .read::<i32>(o.addr + 0x68)
            .map_or("?".to_string(), |v| v.to_string());
        let c6c = rc
            .read::<u8>(o.addr + 0x6C)
            .map_or("?".to_string(), |v| v.to_string());
        b.line(format!(
            "  0x{:X} {:<18} {:<40} +60={c60} +64={c64} +68={c68} +6C={c6c}",
            o.addr,
            rtti.name(o.class),
            o.source,
        ));
    }
    if res.objects.len() > TABLE_LINES_MAX {
        b.line(format!("  (+{} more)", res.objects.len() - TABLE_LINES_MAX));
    }

    let targets = select_dump_targets(&res.objects, rtti);
    let players = targets
        .iter()
        .filter(|&&i| rtti.is_player(res.objects[i].class))
        .count();
    b.line(format!(
        "-- dumping {} objects ({} player, {} other) --",
        targets.len(),
        players,
        targets.len() - players
    ));
    for &i in &targets {
        if b.remaining() < DUMP_LINES_MAX {
            b.line("(line budget too low for another dump; stopping)");
            break;
        }
        if dump_clock.expired() {
            b.line(format!(
                "TIME BUDGET EXHAUSTED after {} ms (dumps; remaining objects not dumped)",
                dump_clock.ms()
            ));
            break;
        }
        dump_object(&mut b, &mut rc, rtti, &res.objects[i], exe_base, exe_size);
    }
    b.line(format!(
        "timings: snapshot {} ms, pass 1 {} ms, pass 2 {} ms, table+dumps {} ms, total {} ms",
        res.snapshot_ms,
        res.pass1.ms,
        res.pass2.ms,
        dump_clock.ms(),
        t_total.elapsed().as_millis()
    ));
    finish(&mut b);
}

fn finish<F: FnMut(&str)>(b: &mut Budget<F>) {
    let dropped = b.dropped;
    b.line(format!(
        "##### discovery end ({} lines left, {} dropped) #####",
        b.remaining(),
        dropped
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::rtti::{Class, Stats, CHR_INS, PLAYER_INS};

    const BASE: usize = 0x1_0000;
    const VT_PLAYER: usize = 0x20_0000;
    const VT_ENEMY: usize = 0x20_1000;
    const VT_CHRSET: usize = 0x20_2000;
    const VT_OTHER: usize = 0x20_3000;

    fn class(name: &str, td: u32, vt: usize, bases: &[u32]) -> Class {
        Class {
            name: name.to_string(),
            td_rva: td,
            col_rva: 0,
            vtables: vec![(vt - BASE) as u32],
            bases: bases.to_vec(),
        }
    }

    fn fake_rtti() -> Rtti {
        let classes = vec![
            class(CHR_INS, 1, 0x20_4000, &[1]),
            class(PLAYER_INS, 2, VT_PLAYER, &[2, 1]),
            class(".?AVEnemyIns@CS@@", 3, VT_ENEMY, &[3, 1]),
            class(".?AV?$ChrSet@VPlayerIns@CS@@@CS@@", 4, VT_CHRSET, &[4]),
            class(".?AVSomethingElse@CS@@", 5, VT_OTHER, &[5]),
        ];
        Rtti::from_classes(BASE, classes, Stats::default())
    }

    /// An 8-aligned fake object whose first qword is `vt`.
    fn obj(vt: usize) -> Vec<u64> {
        let mut v = vec![0u64; 0x80];
        v[0] = vt as u64;
        v
    }
    fn addr<T>(v: &[T]) -> usize {
        v.as_ptr() as usize
    }

    #[test]
    fn hex_line_format() {
        let bytes: Vec<u8> = (0x40..0x40 + 20u8).collect();
        let l = hex_lines(&bytes);
        assert_eq!(l.len(), 2);
        assert_eq!(
            l[0],
            "+0000  40 41 42 43 44 45 46 47  48 49 4A 4B 4C 4D 4E 4F  |@ABCDEFGHIJKLMNO|"
        );
        assert!(l[1].starts_with("+0010  50 51 52 53"));
        assert!(l[1].ends_with("|PQRS|"));
        // Column alignment: the ascii bar sits at the same column on both lines.
        assert_eq!(l[0].find('|'), l[1].find('|'));
        assert_eq!(
            hex_lines(&[0x00, 0x7F, 0x20])[0].rsplit('|').nth(1),
            Some(".. ")
        );
    }

    #[test]
    fn candidate_detection_and_wrapping() {
        let mut bytes = vec![0u8; 0x400];
        bytes[0x6C] = 1;
        bytes[0x70] = 29;
        bytes[0x71] = 5; // not a candidate
        bytes[0x72] = 13;
        let c = team_candidates(&bytes);
        assert_eq!(c, vec![(0x6C, 1), (0x70, 29), (0x72, 13)]);
        let lines = candidate_lines(&c, 6);
        assert_eq!(lines, vec!["+06C=1 +070=29 +072=13".to_string()]);

        let many: Vec<(usize, u8)> = (0..300).map(|i| (i, 1u8)).collect();
        let lines = candidate_lines(&many, 2);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].split(' ').count(), 20);
        assert_eq!(lines[2], "(+260 more candidates not shown)");
        assert!(candidate_lines(&[], 6).is_empty());
    }

    #[test]
    fn budget_caps_lines() {
        let mut got: Vec<String> = Vec::new();
        {
            let mut b = Budget::new(3, |s: &str| got.push(s.to_string()));
            for i in 0..10 {
                b.line(format!("l{i}"));
            }
            assert_eq!(b.remaining(), 0);
            assert_eq!(b.dropped, 7);
        }
        assert_eq!(got, vec!["l0", "l1", "l2"]);
    }

    #[test]
    fn pointer_plausibility() {
        assert!(is_ptr(0x1_0000));
        assert!(is_ptr(0x7FF7_1234_5678));
        assert!(!is_ptr(0));
        assert!(!is_ptr(0x1234)); // low
        assert!(!is_ptr(0x1_0004)); // unaligned
        assert!(!is_ptr(0xFFFF_FFFF_FFFF_0000)); // kernel range
    }

    #[test]
    fn scan_finds_direct_members_arrays_and_chrsets() {
        let rtti = fake_rtti();
        let player = obj(VT_PLAYER);
        let e1 = obj(VT_ENEMY);
        let e2 = obj(VT_ENEMY);
        let other = obj(VT_OTHER);

        // A 0x10-stride array of ChrIns*: [player, status][e1, status][null][e2, status]
        // Padded to 4 KiB so the scanner never reads into unrelated heap memory.
        let mut arr = vec![0u64; 0x200];
        arr[0] = addr(&player) as u64;
        arr[1] = 0x0002; // status bytes: not a pointer
        arr[2] = addr(&e1) as u64;
        arr[3] = 0x0002;
        arr[6] = addr(&e2) as u64;
        arr[7] = 0x0002;
        // A plain pointer array (stride 8): [e1, e2, e1]
        let mut parr = vec![0u64; 0x200];
        parr[0] = addr(&e1) as u64;
        parr[1] = addr(&e2) as u64;
        parr[2] = addr(&e1) as u64;
        // An inline ChrSet: vtable, index, capacity@+0x10, entries@+0x18.
        let mut set = [0u64; 8];
        set[0] = VT_CHRSET as u64;
        set[2] = 4; // capacity (low u32)
        set[3] = addr(&arr) as u64;

        let mut wcm = vec![0u64; 0x100];
        wcm[1] = addr(&player) as u64; // direct member at +0x08
        wcm[3] = addr(&other) as u64; // other object: ignored
        wcm[5] = 4; // cap hint (u32) right before ...
        wcm[6] = addr(&arr) as u64; // ... the array pointer at +0x30
        wcm[9] = addr(&parr) as u64; // pointer array (no hint) at +0x48
        wcm[10] = 0xDEAD_BEEF; // garbage
        for (i, w) in set.iter().enumerate() {
            wcm[0x20 + i] = *w; // inline ChrSet at +0x100
        }
        wcm[0x30] = 0x10; // unreadable low pointer-looking value

        let r = scan(addr(&wcm), wcm.len() * 8, &rtti);
        assert_eq!(r.scanned_len, 0x800);

        let direct: Vec<(usize, &str)> = r
            .direct
            .iter()
            .map(|d| (d.off, rtti.classes[d.class].name.as_str()))
            .collect();
        assert_eq!(direct, vec![(0x08, PLAYER_INS)]);

        // Array at +0x30 (stride 0x10, 3 hits) and pointer array at +0x48 (stride 8, 3 hits).
        let a16 = r
            .arrays
            .iter()
            .find(|a| a.off == 0x30)
            .expect("array +0x30");
        assert_eq!((a16.stride, a16.hits, a16.cap_hint), (16, 3, Some(4)));
        let a8 = r
            .arrays
            .iter()
            .find(|a| a.off == 0x48)
            .expect("array +0x48");
        assert_eq!((a8.stride, a8.hits), (8, 3));
        assert_eq!(fmt_hist(&rtti, &a8.hist), "CS::EnemyIns x3");
        assert_eq!(
            fmt_hist(&rtti, &a16.hist),
            "CS::EnemyIns x2, CS::PlayerIns x1"
        );

        let cs = r.chrsets.iter().find(|c| c.off == 0x100).expect("chrset");
        assert!(!cs.via_ptr && cs.header_ok);
        assert_eq!((cs.capacity, cs.hits), (4, 3));

        // Distinct objects: player, e1, e2 (deduplicated across all the paths).
        let mut addrs: Vec<usize> = r.objects.iter().map(|o| o.addr).collect();
        addrs.sort_unstable();
        let mut want = vec![addr(&player), addr(&e1), addr(&e2)];
        want.sort_unstable();
        assert_eq!(addrs, want);
    }

    /// Full default-budget scan with no progress sink.
    fn scan(inst: usize, len: usize, rtti: &Rtti) -> ScanResult {
        scan_instance(inst, len, rtti, Budgets::default(), &mut |_| {})
    }

    /// A pointer-filled fake "heap" (16 MiB): every qword points at another qword of the buffer, so any
    /// pointee's first qword is itself a pointer (unknown vtable) and every probe has real work to do.
    struct DenseWorld {
        heap: Vec<u64>,
        player: Vec<u64>,
        enemy: Vec<u64>,
        arr: Vec<u64>,
        arr2: Vec<u64>,
        wcm: Vec<u64>,
    }

    const DENSE_N: usize = 2 << 20;
    /// Offsets (bytes) of the planted real hits inside the dense snapshot.
    const HIT_DIRECT: usize = 0x7E00 * 8;
    const HIT_ARRAY_HINTED: usize = 0x4000 * 8;
    const HIT_ARRAY_PLAIN: usize = 0x5000 * 8;

    fn dense_world() -> DenseWorld {
        let mut heap = vec![0u64; DENSE_N];
        let hb = heap.as_ptr() as usize;
        for (i, w) in heap.iter_mut().enumerate() {
            *w = (hb + ((i * 37 + 11) % DENSE_N) * 8) as u64;
        }
        let player = obj(VT_PLAYER);
        let e1 = obj(VT_ENEMY);
        let mut arr = vec![0u64; 0x200];
        arr[0] = addr(&e1) as u64;
        arr[2] = addr(&player) as u64;
        let mut arr2 = vec![0u64; 0x200];
        arr2[0] = addr(&player) as u64;
        arr2[1] = addr(&e1) as u64;
        // 0x40000 bytes densely filled with distinct pointers into the heap.
        let mut wcm = vec![0u64; 0x8000];
        for (k, w) in wcm.iter_mut().enumerate() {
            *w = (hb + ((k * 61) % DENSE_N) * 8) as u64;
        }
        wcm[HIT_DIRECT / 8] = addr(&player) as u64;
        // ER-style: capacity right before the array pointer.
        wcm[HIT_ARRAY_HINTED / 8 - 1] = 4;
        wcm[HIT_ARRAY_HINTED / 8] = addr(&arr) as u64;
        wcm[HIT_ARRAY_PLAIN / 8] = addr(&arr2) as u64;
        DenseWorld {
            heap,
            player,
            enemy: e1,
            arr,
            arr2,
            wcm,
        }
    }

    /// The T-007a run-1 hang, reproduced synthetically. Measured on the pre-fix code with this exact world
    /// (16 MiB heap, 0x40000-byte snapshot of distinct heap pointers): 15.6 s in release / 18.7 s in debug,
    /// 600 000 derefs (the old budget), budget exhausted, and NO hit found, not even the cheap direct member at
    /// +0x7E00*8, because the single pass spent its budget on array probing first. New path: see the printed
    /// timings (a few hundred ms at most).
    #[test]
    fn dense_pointer_snapshot_completes_within_budget_and_finds_hits() {
        let rtti = fake_rtti();
        let w = dense_world();
        let t = Instant::now();
        let r = scan_instance(
            addr(&w.wcm),
            w.wcm.len() * 8,
            &rtti,
            Budgets::default(),
            &mut |_| {},
        );
        let el = t.elapsed();
        println!(
            "NEW PATH dense scan: {} ms total (snapshot {} ms, pass 1 {} ms [{}/{}], pass 2 {} ms [{}/{}]); derefs={}, array cands={} (extended {}), VirtualQuery calls={} for {} guarded reads; direct={}, arrays={}, stopped: p1={:?} p2={:?}",
            el.as_millis(),
            r.snapshot_ms,
            r.pass1.ms,
            r.pass1.done,
            r.pass1.total,
            r.pass2.ms,
            r.pass2.done,
            r.pass2.total,
            r.derefs,
            r.array_cands,
            r.array_extended,
            r.vq_calls,
            r.region_lookups,
            r.direct.len(),
            r.arrays.len(),
            r.pass1.stopped_at,
            r.pass2.stopped_at
        );
        // Bounded by the budgets (plus slack for one in-flight step and the snapshot).
        let b = Budgets::default();
        assert!(
            el < b.pass1 + b.pass2 + Duration::from_millis(500),
            "{el:?}"
        );
        // Pass 1 always completes the whole snapshot on this input and finds the cheap hit.
        assert_eq!(r.pass1.stopped_at, None);
        assert_eq!(r.pass1.done, 0x8000);
        assert_eq!(r.direct.len(), 1);
        assert_eq!(
            (r.direct[0].off, r.direct[0].addr),
            (HIT_DIRECT, addr(&w.player))
        );
        // The hinted array is probed first, so it is found even if pass 2 were cut short.
        let a = r.arrays.iter().find(|a| a.off == HIT_ARRAY_HINTED);
        assert_eq!(a.map(|a| (a.stride, a.hits)), Some((16, 2)));
        // The un-hinted one is found whenever pass 2 ran to completion.
        if r.pass2.stopped_at.is_none() {
            let a = r.arrays.iter().find(|a| a.off == HIT_ARRAY_PLAIN);
            assert_eq!(a.map(|a| a.hits), Some(2));
        }
        // Region cache: thousands of guarded reads, a handful of syscalls.
        assert!(r.region_lookups > 10_000, "{}", r.region_lookups);
        assert!(r.vq_calls * 20 < r.region_lookups, "{}", r.vq_calls);
        // Distinct objects include the player.
        assert!(r.objects.iter().any(|o| o.addr == addr(&w.player)));
        // Keep the backing buffers alive until here.
        assert!(
            w.heap.len() == DENSE_N
                && w.enemy.len() == 0x80
                && w.arr.len() == 0x200
                && w.arr2.len() == 0x200
        );
    }

    #[test]
    fn exhausted_pass2_still_reports_pass1_results_and_where_it_stopped() {
        let rtti = fake_rtti();
        let w = dense_world();
        let b = Budgets {
            pass1: PASS1_BUDGET,
            pass2: Duration::ZERO,
        };
        let r = scan_instance(addr(&w.wcm), w.wcm.len() * 8, &rtti, b, &mut |_| {});
        assert_eq!(r.pass1.stopped_at, None);
        assert_eq!(r.direct.len(), 1, "direct members come from pass 1");
        assert!(r.arrays.is_empty());
        assert!(r.pass2.stopped_at.is_some());
        let msgs = budget_messages(&r);
        assert_eq!(msgs.len(), 1);
        assert!(
            msgs[0].starts_with("TIME BUDGET EXHAUSTED at +0x"),
            "{}",
            msgs[0]
        );
        assert!(msgs[0].contains("pass 2"));
    }

    #[test]
    fn exhausted_pass1_stops_immediately_with_partial_results() {
        let rtti = fake_rtti();
        let w = dense_world();
        let b = Budgets {
            pass1: Duration::ZERO,
            pass2: PASS2_BUDGET,
        };
        let r = scan_instance(addr(&w.wcm), w.wcm.len() * 8, &rtti, b, &mut |_| {});
        assert_eq!(r.pass1.stopped_at, Some(0));
        assert_eq!(r.pass1.done, 0);
        assert!(r.direct.is_empty());
        let msgs = budget_messages(&r);
        assert!(
            msgs[0].contains("pass 1") && msgs[0].contains("+0x0 "),
            "{}",
            msgs[0]
        );
    }

    #[test]
    fn progress_reports_each_phase() {
        let rtti = fake_rtti();
        let wcm = [0u64; 16];
        let mut lines: Vec<String> = Vec::new();
        scan_instance(
            addr(&wcm),
            128,
            &rtti,
            Budgets::default(),
            &mut |s: &str| lines.push(s.to_string()),
        );
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("pass 1") && lines[1].contains("pass 2"));
    }

    #[test]
    fn array_prefix_gates_the_full_read() {
        // A candidate array with NO ChrIns in its first 16 entries is rejected after the 256-byte prefix:
        // it must not be reported and must not count as "extended".
        let rtti = fake_rtti();
        let e1 = obj(VT_ENEMY);
        let mut far = vec![0u64; 0x400];
        far[0x100] = addr(&e1) as u64; // entry 0x80 at stride 0x10: beyond the prefix
        let mut wcm = vec![0u64; 8];
        wcm[1] = addr(&far) as u64;
        let r = scan(addr(&wcm), 64, &rtti);
        assert_eq!(r.array_cands, 1);
        assert_eq!(r.array_extended, 0);
        assert!(r.arrays.is_empty());
        // With a hit inside the prefix the whole array is read and the far entry is found too.
        far[0] = addr(&e1) as u64;
        let r = scan(addr(&wcm), 64, &rtti);
        assert_eq!(r.array_extended, 1);
        assert_eq!(r.arrays.len(), 1);
        assert_eq!(r.arrays[0].hits, 2);
    }

    #[test]
    fn clock_expires_and_stays_expired() {
        let mut z = Clock::new(Duration::ZERO);
        assert!(z.expired());
        assert!(z.expired() && z.is_expired());
        let mut long = Clock::new(Duration::from_secs(3600));
        assert!((0..100).all(|_| !long.expired()));
        let mut short = Clock::new(Duration::from_millis(30));
        std::thread::sleep(Duration::from_millis(60));
        // Checks the wall clock on the first call, so a late first look sees the expiry.
        assert!(short.expired());
    }

    #[test]
    fn gate_allows_one_run_at_a_time() {
        let g = Gate::new();
        assert!(!g.is_busy());
        let guard = g.try_acquire().expect("first acquire");
        assert!(g.is_busy());
        std::thread::sleep(Duration::from_millis(15));
        let busy_ms = g.try_acquire().err().expect("second acquire must fail");
        assert!(busy_ms >= 10, "{busy_ms}");
        drop(guard);
        assert!(!g.is_busy());
        let _again = g.try_acquire().expect("free after drop");
        // A panicking holder releases the gate too.
        let g2 = Gate::new();
        let g3 = Arc::clone(&g2);
        let _ = std::thread::spawn(move || {
            let _guard = g3.try_acquire().expect("acquire");
            panic!("worker died");
        })
        .join();
        assert!(!g2.is_busy());
    }

    #[test]
    fn scan_tolerates_unreadable_and_empty() {
        let rtti = fake_rtti();
        let r = scan(0x10, 0x1000, &rtti);
        assert_eq!(r.scanned_len, 0);
        assert!(r.objects.is_empty());
        // Pointers into unmapped space are skipped without panicking.
        let wcm = [0x7FFF_FFFF_0000u64, 0x1_2345_6780, 0, 0];
        let r = scan(addr(&wcm), 32, &rtti);
        assert_eq!(r.scanned_len, 32);
        assert!(r.direct.is_empty() && r.arrays.is_empty() && r.chrsets.is_empty());
    }

    #[test]
    fn chrset_with_bad_header_is_reported_not_walked() {
        let rtti = fake_rtti();
        let mut set = [0u64; 8];
        set[0] = VT_CHRSET as u64;
        set[2] = 0; // capacity 0
        set[3] = 0x10;
        let r = scan(addr(&set), 64, &rtti);
        assert_eq!(r.chrsets.len(), 1);
        assert!(!r.chrsets[0].header_ok);
        assert_eq!(r.chrsets[0].hits, 0);
    }

    #[test]
    fn dump_target_selection() {
        let rtti = fake_rtti();
        let mk = |class: usize, n: usize| Obj {
            addr: 0x1000 * (n + 1),
            class,
            source: String::new(),
        };
        // classes: 1 = PlayerIns, 2 = EnemyIns
        let mut objs: Vec<Obj> = (0..6).map(|n| mk(1, n)).collect(); // 6 players
        objs.extend((6..20).map(|n| mk(2, n))); // 14 enemies
        objs.push(mk(0, 20)); // plain ChrIns
        let sel = select_dump_targets(&objs, &rtti);
        let players = sel.iter().filter(|&&i| objs[i].class == 1).count();
        let others = sel.len() - players;
        assert_eq!(players, MAX_PLAYER_DUMPS);
        assert_eq!(others, MAX_OTHER_DUMPS);
        // Class diversity first: the lone plain ChrIns (class 0) must be included despite being last.
        assert!(sel.contains(&20));
        // No duplicates.
        let mut s = sel.clone();
        s.sort_unstable();
        s.dedup();
        assert_eq!(s.len(), sel.len());
    }

    #[test]
    fn dump_object_output_shape() {
        let rtti = fake_rtti();
        let mut o = obj(VT_PLAYER);
        o[0xD] = 1 << 32; // the byte at +0x6C is 1
        let ob = Obj {
            addr: addr(&o),
            class: 1,
            source: "direct +0x8".into(),
        };
        let mut lines: Vec<String> = Vec::new();
        {
            let mut b = Budget::new(LINE_BUDGET, |s: &str| lines.push(s.to_string()));
            dump_object(&mut b, &mut RegionCache::new(), &rtti, &ob, BASE, 0x10_0000);
        }
        // header + 64 hex lines + summary + candidate line(s)
        assert!(lines[0].starts_with("=== DUMP CS::PlayerIns @ 0x"));
        assert!(lines[0].contains("bytes=0x400/0x400"));
        assert!(lines[1].starts_with("+0000 "));
        assert!(lines[64].starts_with("+03F0 "));
        assert!(lines[65].contains("team-type candidates"));
        assert!(lines[66].contains("+06C=1"));
        assert!(lines.len() <= DUMP_LINES_MAX);
    }

    #[test]
    fn fallback_histogram_counts_exe_vtables() {
        let rtti = fake_rtti();
        // Objects whose first qword points into a fake "exe" range [BASE, BASE+0x100000).
        let a = obj(BASE + 0x500);
        let b2 = obj(BASE + 0x500);
        let c = obj(BASE + 0x900);
        let d = obj(0x30_0000_0000); // outside the image
        let wcm = [
            addr(&a) as u64,
            addr(&b2) as u64,
            addr(&c) as u64,
            addr(&d) as u64,
        ];
        let f = fallback_vtable_histogram(
            &rtti,
            addr(&wcm),
            32,
            BASE,
            0x10_0000,
            Duration::from_secs(5),
        );
        assert_eq!(f.scanned_len, 32);
        assert_eq!(f.stopped_at, None);
        assert_eq!(f.hist, vec![(0x500, 2, 0), (0x900, 1, 16)]);
        // A zero budget stops at once and says where.
        let f = fallback_vtable_histogram(&rtti, addr(&wcm), 32, BASE, 0x10_0000, Duration::ZERO);
        assert_eq!(f.stopped_at, Some(0));
        assert!(f.hist.is_empty());
    }
}
