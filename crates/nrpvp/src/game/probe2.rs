//! T-027: read-only probes for the match timeline (world clock, event flags) and the local player's
//! `PlayerGameData` (level, runes, HP).
//!
//! EVERYTHING here is an ER-layout HYPOTHESIS for NR 1.3.3.0 and every logged value is labelled
//! `UNVERIFIED`. Strictly read-only: guarded reads through [`RegionCache`] (one cache per tick), no writes, no
//! calls into game functions, no hooks. A wrong layout must be VISIBLE, so the structural checks of the event
//! flag manager are logged (each one passes or fails) and the flag lookup is skipped when they fail.
//!
//! Sources (all ER, fsrs = `vswarte/fromsoftware-rs` at 59fbd3b, see `.claude/re/er-reference.md` section 0):
//! * `CSEventFlagMan` / `CSFD4VirtualMemoryFlag` / `FlagBlockDescriptor` / `FlagBlock`: fsrs `cs/event_flag.rs`.
//!   Lookup: group = flag / divisor; the group's descriptor lives in a `DLMap<u32, FlagBlockDescriptor>` (an MSVC
//!   red-black tree); descriptor mode 1 = block at `flag_blocks + holder_offset * holder_size`, mode 2 = external
//!   block pointer; inside the block, byte = (flag % divisor) / 8, bit = 7 - (flag % divisor) % 8.
//!   Offsets below are DERIVED BY HAND from the field order and sizes in that file (not from a compiled
//!   `offset_of!`); the tree node layout is from fsrs `shared/stl/src/rb_tree.rs` (`RbNode`) and `pair.rs`.
//! * `WorldAreaTime`: fsrs `cs/world_area_time.rs` (`clock: DLDateTime` at +0, `DLDateTime::date` is a packed
//!   bitfield, fsrs `dlut/date_time.rs`).
//! * `PlayerGameData`: fsrs `cs/player_game_data.rs`; offsets derived by hand from the field order, they agree
//!   with `chr_type` +0x98 and `character_name` +0x9C in er-reference.md section 5.2.
//! * The PlayerIns pointer to `PlayerGameData` is ER +0x580 (er-reference.md section 3.1); NR's ChrIns head is
//!   ER shifted by +8 (TECHNICAL_NOTES.md section 4), so +0x588 is the guess (S2-inventory-drops.md). The
//!   variants +0/+8/+0x10 around ER's +0x580 are all probed.

use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};

use super::discover::{hex_lines, is_ptr};
use super::mem::{va_rva, RegionCache};
use super::probe_down::DownWatch;
use super::probe_pos::{PosProbe, Target};
use super::probe_scan::PosScan;
use super::singletons::Found;
use crate::config::Config;

const TAG: &str = "T-027 PROBE UNVERIFIED";

const SING_CLOCK: &str = "WorldAreaTime";
const SING_FLAGS: &str = "CSEventFlagMan";
const SING_AREA: &str = "WorldPlayAreaManager";

// ---- CSEventFlagMan (ER offsets, DERIVED BY HAND from fsrs@59fbd3b cs/event_flag.rs; HYPOTHESIS for NR 1.3.3.0) ----
/// `CSFD4VirtualMemoryFlag.event_flag_divisor` (u32). Struct start = `CSEventFlagMan` start.
const EF_DIVISOR: usize = 0x1C;
/// `event_flag_holder_size` (u32): bytes per flag block (ER: 125).
const EF_HOLDER_SIZE: usize = 0x20;
/// `event_flag_holder_count` (u32).
const EF_HOLDER_COUNT: usize = 0x24;
/// `flag_blocks` (pointer to the block array).
const EF_BLOCKS: usize = 0x28;
/// `flag_block_descriptors: DLMap` = `{allocator @0, head @8, size @0x10}` (comparator is a ZST).
const EF_MAP_HEAD: usize = 0x38;
const EF_MAP_SIZE: usize = 0x40;
/// `CSEventFlagMan.world_type` (u32), right after the 0x78-byte `CSFD4VirtualMemoryFlag`.
const EF_WORLD_TYPE: usize = 0x78;
/// Bytes read of the manager header.
pub(crate) const EF_HEADER_LEN: usize = 0x80;

// ---- MSVC std::map node (fsrs shared/stl rb_tree.rs `RbNode<Pair<u32, FlagBlockDescriptor>>`) ----
const NODE_LEFT: usize = 0x00;
const NODE_PARENT: usize = 0x08;
const NODE_RIGHT: usize = 0x10;
pub(crate) const NODE_IS_NIL: usize = 0x19;
/// `Pair.first` (the group id, u32).
const NODE_KEY: usize = 0x20;
/// `FlagBlockDescriptor.location_mode` (u32).
const NODE_MODE: usize = 0x28;
/// The 8-byte location union (holder offset u32, or an external block pointer).
const NODE_LOC: usize = 0x30;
pub(crate) const NODE_LEN: usize = 0x38;
/// Depth guard for the tree walk (a red-black tree of 10^6 nodes is under 40 deep).
const TREE_MAX_DEPTH: usize = 128;

// ---- WorldAreaTime (ER, fsrs world_area_time.rs; HYPOTHESIS for NR) ----
const CLOCK_DATE: usize = 0x08;
const CLOCK_PREV_DATE: usize = 0x18;
const CLOCK_TARGET: usize = 0x28;
const CLOCK_MULT: usize = 0x38;
const CLOCK_READ_LEN: usize = 0x40;
const CLOCK_DUMP_LEN: usize = 0x100;
/// How long the 0x100-byte change tracker observes a new `WorldAreaTime` instance.
const CLOCK_TRACK: Duration = Duration::from_secs(10);
/// A clock line is logged at least this often even when nothing special happened.
const CLOCK_PERIODIC_MS: u64 = 10_000;
/// A forward step of more than this many clock minutes between two samples counts as a jump.
const CLOCK_MAX_STEP_MIN: u32 = 120;

// ---- PlayerGameData (ER, fsrs player_game_data.rs; offsets derived by hand) ----
/// ER `PlayerIns.player_game_data` is +0x580; the probed variants are +0/+8/+0x10 around it.
pub(crate) const PGD_PTR_OFFSETS: [usize; 3] = [0x580, 0x588, 0x590];
const PGD_HP: usize = 0x10;
const PGD_MAX_HP: usize = 0x14;
const PGD_LEVEL: usize = 0x68;
const PGD_RUNES: usize = 0x6C;
/// `character_name`: 17 UTF-16 code units.
const PGD_NAME: usize = 0x9C;
const PGD_NAME_UNITS: usize = 17;
/// Bytes of a candidate `PlayerGameData` hex-dumped once.
const PGD_DUMP_LEN: usize = 0xC0;
/// At most this many distinct `PlayerGameData` dumps per session.
const PGD_MAX_DUMPS: usize = 8;

/// Flags watched (S3-match-flow.md): game clear/over, the timeline chain 7500..=7523, clock-stop 2200/2201.
/// UI-0 step 5: 6001 "always ON" / 6000 "always OFF" (a check of the layout; ESD t600001000 comments), the lobby flags
/// 10002700..=10002707 (10002700 "For lobby menu: Matching menu", S7 1.1), 10003045 (blocks the expedition button in t600001000) and
/// 69990 "lobby multiplayer start" (S7 1.1).
pub(crate) fn watched_flags() -> Vec<u32> {
    let mut v = vec![7000, 7001];
    v.extend(7500..=7523);
    v.extend([2200, 2201]);
    v.extend([6000, 6001]);
    v.extend(10_002_700..=10_002_707);
    v.extend([10_003_045, 69_990]);
    // UI-0 step 6: our first flag (group 10007: has storage, no ESD/EMEVD reference in game 1.03.3), set by the table list.
    v.push(10_007_000);
    v
}

/// UI-0 step 5: every node of the descriptor map in key order (in-order walk of the MSVC tree), at most `limit` nodes.
pub(crate) fn all_groups(rc: &mut RegionCache, head: usize, limit: usize) -> Result<Vec<Node>, &'static str> {
    let head_node = read_node(rc, head).ok_or("map head unreadable")?;
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut cur = head_node.parent;
    let mut steps = 0usize;
    loop {
        while cur != head {
            let n = read_node(rc, cur).ok_or("tree node unreadable")?;
            if n.is_nil {
                break;
            }
            if stack.len() >= TREE_MAX_DEPTH {
                return Err("tree walk exceeded the depth guard");
            }
            stack.push(cur);
            cur = n.left;
        }
        let Some(top) = stack.pop() else { break };
        let n = read_node(rc, top).ok_or("tree node unreadable")?;
        out.push(n);
        steps += 1;
        if steps >= limit {
            break;
        }
        cur = n.right;
    }
    Ok(out)
}

/// `[1, 2, 3, 7, 9, 10]` -> `"1..3 7 9..10"` (sorted input).
pub(crate) fn group_ranges(keys: &[u32]) -> String {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < keys.len() {
        let mut j = i;
        while j + 1 < keys.len() && keys[j + 1] == keys[j].wrapping_add(1) {
            j += 1;
        }
        parts.push(if i == j { keys[i].to_string() } else { format!("{}..{}", keys[i], keys[j]) });
        i = j + 1;
    }
    parts.join(" ")
}

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

fn in_image(addr: usize, base: usize, size: usize) -> bool {
    addr >= base && addr - base < size
}

// ---------------------------------------------------------------------------------------------------
// Event flags
// ---------------------------------------------------------------------------------------------------

/// `(group, byte, bit)` of a flag id, or `None` for a zero divisor (fsrs `EventFlag::{group,byte,bit}` with the
/// divisor taken from the manager instead of the hardcoded 1000).
pub(crate) fn flag_split(flag: u32, divisor: u32) -> Option<(u32, u32, u32)> {
    if divisor == 0 {
        return None;
    }
    let rem = flag % divisor;
    Some((flag / divisor, rem / 8, 7 - (rem % 8)))
}

/// Address of a flag block from its descriptor (fsrs `FlagBlockDescriptor::resolve`). `None` for an unknown
/// mode (fsrs: no block, flag reads as off) or an implausible pointer.
pub(crate) fn block_addr(mode: u32, loc: u64, blocks: usize, holder_size: u32) -> Option<usize> {
    match mode {
        1 => {
            if !is_ptr(blocks) {
                return None;
            }
            // `holder_offset` is the low u32 of the union; blocks are `holder_size` bytes each.
            let idx = (loc & 0xFFFF_FFFF) as usize;
            blocks.checked_add(idx.checked_mul(holder_size as usize)?)
        }
        2 => {
            let p = usize::try_from(loc).ok()?;
            is_ptr(p).then_some(p)
        }
        _ => None,
    }
}

/// The manager header, parsed. Readability flags are filled in by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EfLayout {
    pub divisor: u32,
    pub holder_size: u32,
    pub holder_count: u32,
    pub blocks: usize,
    pub head: usize,
    pub map_size: u64,
    pub world_type: u32,
    pub blocks_readable: bool,
    pub head_readable: bool,
    /// `is_nil` byte of the map's head node (the MSVC sentinel has it set to 1).
    pub head_is_nil: Option<u8>,
}

/// Parse the header bytes (at least [`EF_HEADER_LEN`]); readability fields start false/None.
pub(crate) fn parse_layout(b: &[u8]) -> Option<EfLayout> {
    Some(EfLayout {
        divisor: le32(b, EF_DIVISOR)?,
        holder_size: le32(b, EF_HOLDER_SIZE)?,
        holder_count: le32(b, EF_HOLDER_COUNT)?,
        blocks: usize::try_from(le64(b, EF_BLOCKS)?).ok()?,
        head: usize::try_from(le64(b, EF_MAP_HEAD)?).ok()?,
        map_size: le64(b, EF_MAP_SIZE)?,
        world_type: le32(b, EF_WORLD_TYPE)?,
        blocks_readable: false,
        head_readable: false,
        head_is_nil: None,
    })
}

/// One structural check of the layout hypothesis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

/// The structural checks. ER values: divisor 1000, holder size 125 (= ceil(1000 / 8)).
pub(crate) fn structural_checks(l: &EfLayout) -> Vec<Check> {
    let c = |name, ok, detail: String| Check { name, ok, detail };
    let min_holder = l.divisor.div_ceil(8);
    vec![
        c(
            "divisor",
            (100..=100_000).contains(&l.divisor),
            format!("{} (ER: 1000)", l.divisor),
        ),
        c(
            "holder_size",
            l.divisor != 0 && (min_holder..=4096).contains(&l.holder_size),
            format!(
                "{} (ER: 125; must be {min_holder}..=4096 for this divisor)",
                l.holder_size
            ),
        ),
        c(
            "holder_count",
            (1..=1_000_000).contains(&l.holder_count),
            format!("{}", l.holder_count),
        ),
        c(
            "flag_blocks pointer",
            is_ptr(l.blocks) && l.blocks_readable,
            format!("0x{:X} readable={}", l.blocks, l.blocks_readable),
        ),
        c(
            "descriptor map head",
            is_ptr(l.head) && l.head_readable && l.head_is_nil == Some(1),
            format!(
                "0x{:X} readable={} is_nil={:?}",
                l.head, l.head_readable, l.head_is_nil
            ),
        ),
        c(
            "descriptor map size",
            (1..=1_000_000).contains(&l.map_size),
            format!("{}", l.map_size),
        ),
    ]
}

/// `true` if every check passed.
pub(crate) fn checks_ok(checks: &[Check]) -> bool {
    checks.iter().all(|c| c.ok)
}

/// A tree node, parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    pub left: usize,
    pub parent: usize,
    pub right: usize,
    pub is_nil: bool,
    pub key: u32,
    pub mode: u32,
    pub loc: u64,
}

pub(crate) fn parse_node(b: &[u8]) -> Option<Node> {
    Some(Node {
        left: usize::try_from(le64(b, NODE_LEFT)?).ok()?,
        parent: usize::try_from(le64(b, NODE_PARENT)?).ok()?,
        right: usize::try_from(le64(b, NODE_RIGHT)?).ok()?,
        is_nil: *b.get(NODE_IS_NIL)? != 0,
        key: le32(b, NODE_KEY)?,
        mode: le32(b, NODE_MODE)?,
        loc: le64(b, NODE_LOC)?,
    })
}

fn read_node(rc: &mut RegionCache, addr: usize) -> Option<Node> {
    if !is_ptr(addr) {
        return None;
    }
    let b = rc.read_prefix(addr, NODE_LEN);
    if b.len() < NODE_LEN {
        return None;
    }
    parse_node(&b)
}

/// Why a lookup produced no value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlagState {
    On,
    Off,
    /// The flag's group has no block, or the descriptor mode is unknown: fsrs reads this as "off".
    NoBlock,
    /// A guarded read failed or a pointer was implausible; the reason is for the log.
    Unreadable(&'static str),
}

impl FlagState {
    fn label(self) -> String {
        match self {
            FlagState::On => "ON".to_string(),
            FlagState::Off => "off".to_string(),
            FlagState::NoBlock => "no-block".to_string(),
            FlagState::Unreadable(why) => format!("unreadable ({why})"),
        }
    }
}

/// Binary search of the map for `group` (MSVC `std::map`: `head.parent` is the root, children that are absent
/// point at `head`, nodes have an `is_nil` byte). `Ok(None)` = group not in the map.
fn find_group(rc: &mut RegionCache, head: usize, group: u32) -> Result<Option<Node>, &'static str> {
    let head_node = read_node(rc, head).ok_or("map head unreadable")?;
    let mut cur = head_node.parent;
    for _ in 0..TREE_MAX_DEPTH {
        if cur == head {
            return Ok(None);
        }
        let n = read_node(rc, cur).ok_or("tree node unreadable")?;
        if n.is_nil {
            return Ok(None);
        }
        cur = match group.cmp(&n.key) {
            std::cmp::Ordering::Less => n.left,
            std::cmp::Ordering::Greater => n.right,
            std::cmp::Ordering::Equal => return Ok(Some(n)),
        };
    }
    Err("tree walk exceeded the depth guard")
}

/// Look one flag up with the ER algorithm. The caller has already passed the structural checks.
pub(crate) fn lookup_flag(rc: &mut RegionCache, l: &EfLayout, flag: u32) -> FlagState {
    let Some((group, byte, bit)) = flag_split(flag, l.divisor) else {
        return FlagState::Unreadable("divisor is zero");
    };
    if byte >= l.holder_size {
        return FlagState::Unreadable("byte index is outside the holder");
    }
    let d = match find_group(rc, l.head, group) {
        Err(e) => return FlagState::Unreadable(e),
        Ok(None) => return FlagState::NoBlock,
        Ok(Some(d)) => d,
    };
    let Some(block) = block_addr(d.mode, d.loc, l.blocks, l.holder_size) else {
        return FlagState::NoBlock;
    };
    match rc.read::<u8>(block + byte as usize) {
        Some(b) if (b >> bit) & 1 == 1 => FlagState::On,
        Some(_) => FlagState::Off,
        None => FlagState::Unreadable("flag byte unreadable"),
    }
}

/// Remembers the last state of every watched flag and describes what changed.
#[derive(Default)]
pub(crate) struct FlagTracker {
    last: BTreeMap<u32, FlagState>,
}

impl FlagTracker {
    pub(crate) fn reset(&mut self) {
        self.last.clear();
    }

    /// Log lines for this reading: one summary line the first time, then one line per changed flag.
    pub(crate) fn update(&mut self, readings: &[(u32, FlagState)]) -> Vec<String> {
        let mut out = Vec::new();
        if self.last.is_empty() {
            let on: Vec<u32> = readings
                .iter()
                .filter(|r| r.1 == FlagState::On)
                .map(|r| r.0)
                .collect();
            let count = |f: fn(&FlagState) -> bool| readings.iter().filter(|r| f(&r.1)).count();
            out.push(format!(
                "initial flags: ON={on:?}, off={}, no-block={}, unreadable={} (of {})",
                count(|s| *s == FlagState::Off),
                count(|s| *s == FlagState::NoBlock),
                count(|s| matches!(s, FlagState::Unreadable(_))),
                readings.len()
            ));
        } else {
            for (flag, st) in readings {
                if self.last.get(flag) != Some(st) {
                    let from = self.last.get(flag).map_or("?".to_string(), |s| s.label());
                    out.push(format!("flag {flag}: {from} -> {}", st.label()));
                }
            }
        }
        for (flag, st) in readings {
            self.last.insert(*flag, *st);
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------------
// World clock
// ---------------------------------------------------------------------------------------------------

/// The packed date of fsrs `DLDateTime::date` (bit ranges from fsrs `dlut/date_time.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Packed {
    pub year: u16,
    pub millisecond: u16,
    pub month: u8,
    pub day_of_week: u8,
    pub day: u8,
    pub hours: u8,
    pub minutes: u8,
    pub seconds: u8,
    pub utc: bool,
}

pub(crate) fn decode_packed(v: u64) -> Packed {
    Packed {
        year: (v & 0xFFF) as u16,
        millisecond: ((v >> 12) & 0x3FF) as u16,
        month: ((v >> 22) & 0xF) as u8,
        day_of_week: ((v >> 26) & 0x7) as u8,
        day: ((v >> 29) & 0x1F) as u8,
        hours: ((v >> 34) & 0x1F) as u8,
        minutes: ((v >> 39) & 0x3F) as u8,
        seconds: ((v >> 45) & 0x3F) as u8,
        utc: (v >> 51) & 1 == 1,
    }
}

impl Packed {
    /// Hours, minutes and seconds are in range.
    pub(crate) fn plausible(&self) -> bool {
        self.hours < 24 && self.minutes < 60 && self.seconds < 60
    }

    pub(crate) fn minute_of_day(&self) -> u32 {
        u32::from(self.hours) * 60 + u32::from(self.minutes)
    }

    fn hms(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hours, self.minutes, self.seconds)
    }
}

/// One sample of the ER-layout `WorldAreaTime` fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClockSample {
    pub raw_date: u64,
    pub date: Packed,
    pub raw_prev: u64,
    pub prev: Packed,
    pub target: (u32, u32, u32),
    pub mult: f32,
}

pub(crate) fn parse_clock(b: &[u8]) -> Option<ClockSample> {
    let raw_date = le64(b, CLOCK_DATE)?;
    let raw_prev = le64(b, CLOCK_PREV_DATE)?;
    Some(ClockSample {
        raw_date,
        date: decode_packed(raw_date),
        raw_prev,
        prev: decode_packed(raw_prev),
        target: (
            le32(b, CLOCK_TARGET)?,
            le32(b, CLOCK_TARGET + 4)?,
            le32(b, CLOCK_TARGET + 8)?,
        ),
        mult: f32::from_bits(le32(b, CLOCK_MULT)?),
    })
}

impl ClockSample {
    /// The hint decodes to a believable time of day and speed.
    pub(crate) fn plausible(&self) -> bool {
        self.date.plausible() && self.mult.is_finite() && (0.0..=1000.0).contains(&self.mult)
    }

    fn line(&self) -> String {
        format!(
            "clock hint +0x08 packed=0x{:016X} -> {} (ms {}, utc {}, ymd {}-{}-{}) plausible={}; prev(+0x18) {}; target(+0x28) {}:{}:{}; mult(+0x38)={}",
            self.raw_date,
            self.date.hms(),
            self.date.millisecond,
            self.date.utc,
            self.date.year,
            self.date.month,
            self.date.day,
            self.plausible(),
            self.prev.hms(),
            self.target.0,
            self.target.1,
            self.target.2,
            self.mult,
        )
    }
}

/// Why this clock sample deserves a log line (the clock moves every sample, so most are not logged).
pub(crate) fn clock_log_reason(
    prev: Option<&ClockSample>,
    cur: &ClockSample,
    ms_since_log: u64,
) -> Option<&'static str> {
    let Some(p) = prev else {
        return Some("first sample");
    };
    if cur.target != p.target || cur.mult.to_bits() != p.mult.to_bits() {
        return Some("target or speed changed");
    }
    if cur.plausible() && p.plausible() {
        let delta = (cur.date.minute_of_day() + 1440 - p.date.minute_of_day()) % 1440;
        if delta > CLOCK_MAX_STEP_MIN {
            return Some("clock jumped (backwards or far forwards)");
        }
        if cur.date.hours != p.date.hours {
            return Some("hour changed");
        }
    } else if cur.plausible() != p.plausible() {
        return Some("plausibility changed");
    }
    (ms_since_log >= CLOCK_PERIODIC_MS).then_some("periodic")
}

/// Which bytes of a byte window change over repeated samples (a cheap way to find the clock if the hint is wrong).
pub(crate) struct ByteTracker {
    first: Vec<u8>,
    last: Vec<u8>,
    changes: Vec<u32>,
    samples: u32,
}

impl ByteTracker {
    pub(crate) fn new(first: &[u8]) -> Self {
        ByteTracker {
            first: first.to_vec(),
            last: first.to_vec(),
            changes: vec![0; first.len()],
            samples: 1,
        }
    }

    pub(crate) fn feed(&mut self, cur: &[u8]) {
        if cur.len() != self.last.len() {
            return;
        }
        for (i, (&a, &b)) in self.last.iter().zip(cur).enumerate() {
            if a != b {
                self.changes[i] += 1;
            }
        }
        self.last = cur.to_vec();
        self.samples += 1;
    }

    /// One line per 8-byte group with a change: first and last value, byte-change count.
    pub(crate) fn report(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (g, (f, l)) in self.first.chunks(8).zip(self.last.chunks(8)).enumerate() {
            let n: u32 = self.changes[g * 8..g * 8 + f.len()].iter().sum();
            if n > 0 {
                let val = |c: &[u8]| {
                    let mut a = [0u8; 8];
                    a[..c.len()].copy_from_slice(c);
                    u64::from_le_bytes(a)
                };
                out.push(format!(
                    "+0x{:03X}: 0x{:016X} -> 0x{:016X} ({n} byte changes over {} samples)",
                    g * 8,
                    val(f),
                    val(l),
                    self.samples
                ));
            }
        }
        if out.is_empty() {
            out.push(format!("no byte changed over {} samples", self.samples));
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------------
// PlayerGameData
// ---------------------------------------------------------------------------------------------------

/// The plausibility filters of the card: level 1..=15, runes < 10^7, hp <= max hp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Plausibility {
    pub level: bool,
    pub runes: bool,
    pub hp: bool,
}

impl Plausibility {
    pub(crate) fn all(&self) -> bool {
        self.level && self.runes && self.hp
    }

    fn label(&self) -> String {
        let f = |ok| if ok { "ok" } else { "NO" };
        format!(
            "level:{} runes:{} hp:{}",
            f(self.level),
            f(self.runes),
            f(self.hp)
        )
    }
}

pub(crate) fn assess(
    level: Option<u32>,
    runes: Option<u32>,
    hp: Option<u32>,
    max_hp: Option<u32>,
) -> Plausibility {
    Plausibility {
        level: level.is_some_and(|l| (1..=15).contains(&l)),
        runes: runes.is_some_and(|r| r < 10_000_000),
        hp: matches!((hp, max_hp), (Some(h), Some(m)) if m > 0 && m <= 100_000 && h <= m),
    }
}

/// UTF-16 name, up to the first NUL; `None` if empty or it holds control characters / unpaired surrogates.
pub(crate) fn decode_name(units: &[u16]) -> Option<String> {
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    if end == 0 {
        return None;
    }
    let s = String::from_utf16(&units[..end]).ok()?;
    (!s.chars().any(char::is_control)).then_some(s)
}

/// One probed pointer variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PgdCandidate {
    /// Offset from the PlayerIns where the pointer was read.
    pub ptr_off: usize,
    pub ptr: Option<usize>,
    /// First qword of the target (a vtable pointer into the exe for a real `PlayerGameData`).
    pub vtable: Option<usize>,
    pub vtable_in_image: bool,
    pub level: Option<u32>,
    pub runes: Option<u32>,
    pub hp: Option<u32>,
    pub max_hp: Option<u32>,
    pub name: Option<String>,
    pub plausibility: Plausibility,
}

impl PgdCandidate {
    fn line(&self, base: usize, size: usize) -> String {
        let n = |v: Option<u32>| v.map_or("?".to_string(), |v| v.to_string());
        let ptr = match self.ptr {
            None => "unreadable".to_string(),
            Some(p) => va_rva(p, base, size),
        };
        format!(
            "PlayerIns+0x{:X} -> {ptr}; vtable_in_exe={} level={} runes={} hp={}/{} name={:?} => {} {}",
            self.ptr_off,
            self.vtable_in_image,
            n(self.level),
            n(self.runes),
            n(self.hp),
            n(self.max_hp),
            self.name.as_deref().unwrap_or("?"),
            self.plausibility.label(),
            if self.plausibility.all() && self.vtable_in_image {
                "LOOKS PLAUSIBLE"
            } else {
                "not plausible"
            }
        )
    }

    /// Short form for the overlay.
    fn overlay_line(&self) -> String {
        let n = |v: Option<u32>| v.map_or("?".to_string(), |v| v.to_string());
        format!(
            "+0x{:X}: lvl {} runes {} hp {}/{} {}",
            self.ptr_off,
            n(self.level),
            n(self.runes),
            n(self.hp),
            n(self.max_hp),
            if self.plausibility.all() && self.vtable_in_image {
                "[plausible]"
            } else {
                "[no]"
            }
        )
    }
}

/// Read every pointer variant of `player` (a PlayerIns address). All reads are guarded.
pub(crate) fn read_pgd_candidates(
    rc: &mut RegionCache,
    player: usize,
    base: usize,
    size: usize,
) -> Vec<PgdCandidate> {
    PGD_PTR_OFFSETS
        .iter()
        .map(|&off| {
            let ptr = rc.read::<usize>(player + off).filter(|&p| is_ptr(p));
            let vtable = ptr.and_then(|p| rc.read::<usize>(p));
            let (level, runes, hp, max_hp, name) = match ptr {
                Some(p) => {
                    let name = rc.read_prefix(p + PGD_NAME, PGD_NAME_UNITS * 2);
                    let units: Vec<u16> = name
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_le_bytes(*c))
                        .collect();
                    (
                        rc.read::<u32>(p + PGD_LEVEL),
                        rc.read::<u32>(p + PGD_RUNES),
                        rc.read::<u32>(p + PGD_HP),
                        rc.read::<u32>(p + PGD_MAX_HP),
                        (units.len() == PGD_NAME_UNITS)
                            .then(|| decode_name(&units))
                            .flatten(),
                    )
                }
                None => (None, None, None, None, None),
            };
            PgdCandidate {
                ptr_off: off,
                ptr,
                vtable,
                vtable_in_image: vtable.is_some_and(|v| in_image(v, base, size)),
                level,
                runes,
                hp,
                max_hp,
                name,
                plausibility: assess(level, runes, hp, max_hp),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------------------------------

/// State of a singleton as the probe saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SingState {
    NotInMap,
    Null,
    Live(usize),
}

pub(crate) struct Probes {
    enabled: bool,
    interval: Duration,
    base: usize,
    size: usize,
    last_tick: Option<Instant>,
    sing: BTreeMap<&'static str, SingState>,
    view: Vec<String>,
    // position (T-028 read-only part)
    pos: PosProbe,
    // T-031: downed-state log and position scan
    down: DownWatch,
    scan: PosScan,
    // clock
    clock_inst: usize,
    clock_prev: Option<ClockSample>,
    clock_last_log: Instant,
    clock_tracker: Option<(ByteTracker, Instant)>,
    // event flags
    flag_inst: usize,
    flag_tracker: FlagTracker,
    flag_checks_ok: Option<bool>,
    flag_watch: Vec<u32>,
    // player data
    pgd_last: Option<(usize, Vec<PgdCandidate>)>,
    pgd_dumped: HashSet<usize>,
}

impl Probes {
    pub(crate) fn new(cfg: &Config, base: usize, size: usize) -> Self {
        if cfg.probes {
            tracing::info!(
                "{TAG}: probes ENABLED (interval {} ms): world clock, event flags, PlayerGameData candidates, player positions (T-028), downed-state log and position scan (T-031). ER-layout hypotheses, read-only; nothing logged under this tag is verified",
                cfg.probe_interval_ms
            );
        }
        Probes {
            enabled: cfg.probes,
            interval: Duration::from_millis(cfg.probe_interval_ms),
            base,
            size,
            last_tick: None,
            sing: BTreeMap::new(),
            view: Vec::new(),
            pos: PosProbe::new(base, size),
            down: DownWatch::new(),
            scan: PosScan::new(base, size),
            clock_inst: 0,
            clock_prev: None,
            clock_last_log: Instant::now(),
            clock_tracker: None,
            flag_inst: 0,
            flag_tracker: FlagTracker::default(),
            flag_checks_ok: None,
            flag_watch: watched_flags(),
            pgd_last: None,
            pgd_dumped: HashSet::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    /// Overlay lines of the latest tick: positions first, then clock, flags and player data.
    pub(crate) fn lines(&self) -> Vec<String> {
        let mut v = self.pos.lines().to_vec();
        v.extend(self.view.iter().cloned());
        v
    }

    /// T-032: RTTI class names (vtable VA -> name) for the position scan report.
    pub(crate) fn set_class_names(
        &mut self,
        names: std::sync::Arc<std::collections::HashMap<usize, String>>,
    ) {
        self.scan.set_class_names(names);
    }

    /// T-032: a MARK was logged: the downed-state tracker runs its 2 s before/after window around it.
    pub(crate) fn on_mark(&mut self, mark: u32) {
        if self.enabled {
            self.down.request_mark(mark);
        }
    }

    /// The slow probes (clock, flags, PlayerGameData) are due.
    fn slow_due(&self) -> bool {
        self.enabled && self.last_tick.is_none_or(|t| t.elapsed() >= self.interval)
    }

    /// One probe pass; call it every main-loop iteration (each part gates itself). `all` is the singleton map,
    /// `wcm` the WorldChrMan instance (0 = not live), `targets` every PlayerIns of the ChrSet (local flagged).
    pub(crate) fn tick(&mut self, all: &[&Found], wcm: usize, targets: &[Target]) {
        if !self.enabled {
            return;
        }
        self.pos.tick(wcm, targets);
        self.down.tick(wcm, targets);
        self.scan.tick(wcm, targets);
        if !self.slow_due() {
            return;
        }
        self.last_tick = Some(Instant::now());
        let local = targets.iter().find(|t| t.local).map(|t| t.addr);
        let mut rc = RegionCache::new();
        let mut view = Vec::new();
        let clock = self.singleton(&mut rc, all, SING_CLOCK);
        self.clock_probe(&mut rc, clock, &mut view);
        let flags = self.singleton(&mut rc, all, SING_FLAGS);
        self.flag_probe(&mut rc, flags, &mut view);
        let _ = self.singleton(&mut rc, all, SING_AREA);
        self.player_probe(&mut rc, local, &mut view);
        self.view = view;
    }

    /// Resolve a singleton by name and log when its state changes. Returns the live instance, if any.
    fn singleton(
        &mut self,
        rc: &mut RegionCache,
        all: &[&Found],
        name: &'static str,
    ) -> Option<usize> {
        let st = match all.iter().find(|f| f.name == name) {
            None => SingState::NotInMap,
            Some(f) => match rc.read::<usize>(f.slot) {
                Some(p) if p != 0 => SingState::Live(p),
                _ => SingState::Null,
            },
        };
        if self.sing.insert(name, st) != Some(st) {
            let what = match st {
                SingState::NotInMap => "NOT in the singleton map".to_string(),
                SingState::Null => "in the map, instance is null".to_string(),
                SingState::Live(p) => format!("live at {}", va_rva(p, self.base, self.size)),
            };
            tracing::info!("{TAG}: singleton {name}: {what}");
        }
        match st {
            SingState::Live(p) => Some(p),
            _ => None,
        }
    }

    fn clock_probe(&mut self, rc: &mut RegionCache, inst: Option<usize>, view: &mut Vec<String>) {
        let Some(inst) = inst else {
            self.clock_inst = 0;
            self.clock_prev = None;
            self.clock_tracker = None;
            view.push("clock: WorldAreaTime not live".to_string());
            return;
        };
        let window = rc.read_prefix(inst, CLOCK_DUMP_LEN);
        if inst != self.clock_inst {
            self.clock_inst = inst;
            self.clock_prev = None;
            self.clock_tracker = None;
            if window.len() < CLOCK_DUMP_LEN {
                tracing::warn!(
                    "{TAG}: WorldAreaTime {}: only 0x{:X} of 0x{CLOCK_DUMP_LEN:X} bytes readable",
                    va_rva(inst, self.base, self.size),
                    window.len()
                );
            } else {
                tracing::info!(
                    "{TAG}: WorldAreaTime instance {} first 0x{CLOCK_DUMP_LEN:X} bytes (once; the bytes that change over the next {} s follow):",
                    va_rva(inst, self.base, self.size),
                    CLOCK_TRACK.as_secs()
                );
                for l in hex_lines(&window) {
                    tracing::info!("{TAG}:   {l}");
                }
                self.clock_tracker = Some((ByteTracker::new(&window), Instant::now()));
            }
        } else if let Some((tr, since)) = self.clock_tracker.as_mut() {
            tr.feed(&window);
            if since.elapsed() >= CLOCK_TRACK {
                tracing::info!(
                    "{TAG}: WorldAreaTime bytes that changed over {} s (qword granularity):",
                    CLOCK_TRACK.as_secs()
                );
                for l in tr.report() {
                    tracing::info!("{TAG}:   {l}");
                }
                self.clock_tracker = None;
            }
        }

        let Some(cur) = parse_clock(&window[..window.len().min(CLOCK_READ_LEN)]) else {
            view.push("clock: unreadable".to_string());
            return;
        };
        let since = u64::try_from(self.clock_last_log.elapsed().as_millis()).unwrap_or(u64::MAX);
        if let Some(why) = clock_log_reason(self.clock_prev.as_ref(), &cur, since) {
            tracing::info!("{TAG}: {} [{why}]", cur.line());
            self.clock_last_log = Instant::now();
        }
        view.push(format!(
            "clock hint: {} mult {} {}",
            cur.date.hms(),
            cur.mult,
            if cur.plausible() {
                "[plausible]"
            } else {
                "[no]"
            }
        ));
        self.clock_prev = Some(cur);
    }

    fn flag_probe(&mut self, rc: &mut RegionCache, inst: Option<usize>, view: &mut Vec<String>) {
        let Some(inst) = inst else {
            self.flag_inst = 0;
            self.flag_checks_ok = None;
            self.flag_tracker.reset();
            view.push("flags: CSEventFlagMan not live".to_string());
            return;
        };
        let new_inst = inst != self.flag_inst;
        if new_inst {
            self.flag_inst = inst;
            self.flag_checks_ok = None;
            self.flag_tracker.reset();
            tracing::info!(
                "{TAG}: CSEventFlagMan instance {}: header (first 0x{EF_HEADER_LEN:X} bytes, once):",
                va_rva(inst, self.base, self.size)
            );
            for l in hex_lines(&rc.read_prefix(inst, EF_HEADER_LEN)) {
                tracing::info!("{TAG}:   {l}");
            }
        }
        let hdr = rc.read_prefix(inst, EF_HEADER_LEN);
        let Some(mut layout) = parse_layout(&hdr) else {
            if self.flag_checks_ok != Some(false) {
                tracing::warn!("{TAG}: CSEventFlagMan header unreadable; flag probe skipped");
            }
            self.flag_checks_ok = Some(false);
            view.push("flags: header unreadable".to_string());
            return;
        };
        layout.blocks_readable = rc.readable_len(layout.blocks, 1) > 0;
        layout.head_readable = rc.readable_len(layout.head, NODE_LEN) >= NODE_LEN;
        layout.head_is_nil = rc.read::<u8>(layout.head.wrapping_add(NODE_IS_NIL));
        let checks = structural_checks(&layout);
        let ok = checks_ok(&checks);
        if self.flag_checks_ok != Some(ok) {
            tracing::info!(
                "{TAG}: CSEventFlagMan ER-layout structural checks (world_type {}): {}",
                layout.world_type,
                if ok { "ALL PASS" } else { "FAILED" }
            );
            for c in &checks {
                tracing::info!(
                    "{TAG}:   [{}] {}: {}",
                    if c.ok { "pass" } else { "FAIL" },
                    c.name,
                    c.detail
                );
            }
            self.flag_checks_ok = Some(ok);
            // UI-0 step 5: which groups have storage (a flag in a group without a block cannot be set), once per instance.
            if ok {
                match all_groups(rc, layout.head, 200_000).map(|v| v.iter().map(|n| n.key).collect::<Vec<u32>>()) {
                    Ok(g) => tracing::info!(
                        "{TAG}: flag groups with a descriptor ({} of map size {}; flag = group * {} + n): {}",
                        g.len(),
                        layout.map_size,
                        layout.divisor,
                        group_ranges(&g)
                    ),
                    Err(e) => tracing::warn!("{TAG}: flag group census failed: {e}"),
                }
            }
        }
        if !ok {
            let failed: Vec<&str> = checks.iter().filter(|c| !c.ok).map(|c| c.name).collect();
            view.push(format!("flags: layout FAILED ({})", failed.join(", ")));
            return;
        }
        let readings: Vec<(u32, FlagState)> = self
            .flag_watch
            .iter()
            .map(|&f| (f, lookup_flag(rc, &layout, f)))
            .collect();
        for l in self.flag_tracker.update(&readings) {
            tracing::info!("{TAG}: {l}");
        }
        let on: Vec<String> = readings
            .iter()
            .filter(|r| r.1 == FlagState::On)
            .map(|r| r.0.to_string())
            .collect();
        let bad = readings
            .iter()
            .filter(|r| matches!(r.1, FlagState::Unreadable(_) | FlagState::NoBlock))
            .count();
        view.push(format!(
            "flags: layout ok; ON: {}; {bad}/{} not readable",
            if on.is_empty() {
                "none".to_string()
            } else {
                on.join(" ")
            },
            readings.len()
        ));
    }

    fn player_probe(&mut self, rc: &mut RegionCache, local: Option<usize>, view: &mut Vec<String>) {
        let Some(player) = local else {
            view.push("player data: no local PlayerIns".to_string());
            return;
        };
        let cands = read_pgd_candidates(rc, player, self.base, self.size);
        // Dump each plausible-looking PlayerGameData object once (first qword in the exe = a vtable).
        for c in &cands {
            let Some(p) = c.ptr else { continue };
            if c.vtable_in_image
                && self.pgd_dumped.len() < PGD_MAX_DUMPS
                && self.pgd_dumped.insert(p)
            {
                tracing::info!(
                    "{TAG}: candidate PlayerGameData at {} (via PlayerIns+0x{:X}), first 0x{PGD_DUMP_LEN:X} bytes (once):",
                    va_rva(p, self.base, self.size),
                    c.ptr_off
                );
                for l in hex_lines(&rc.read_prefix(p, PGD_DUMP_LEN)) {
                    tracing::info!("{TAG}:   {l}");
                }
            }
        }
        let changed = self
            .pgd_last
            .as_ref()
            .is_none_or(|(pl, old)| *pl != player || *old != cands);
        if changed {
            tracing::info!(
                "{TAG}: PlayerGameData candidates for local PlayerIns {}:",
                va_rva(player, self.base, self.size)
            );
            for c in &cands {
                tracing::info!("{TAG}:   {}", c.line(self.base, self.size));
            }
            let good: Vec<String> = cands
                .iter()
                .filter(|c| c.plausibility.all() && c.vtable_in_image)
                .map(|c| format!("+0x{:X}", c.ptr_off))
                .collect();
            tracing::info!(
                "{TAG}:   plausible pointer offsets: {}",
                if good.is_empty() {
                    "none".to_string()
                } else {
                    good.join(", ")
                }
            );
        }
        view.push(
            match cands
                .iter()
                .find(|c| c.plausibility.all() && c.vtable_in_image)
            {
                Some(c) => {
                    let n = |v: Option<u32>| v.map_or("?".to_string(), |v| v.to_string());
                    format!(
                        "PlayerData (via +0x{:X}): level {} runes {} HP {}/{}",
                        c.ptr_off,
                        n(c.level),
                        n(c.runes),
                        n(c.hp),
                        n(c.max_hp)
                    )
                }
                None => "PlayerData: no plausible candidate".to_string(),
            },
        );
        view.push("PlayerGameData candidates (PlayerIns pointer offset):".to_string());
        view.extend(cands.iter().map(|c| format!("  {}", c.overlay_line())));
        if changed {
            self.pgd_last = Some((player, cands));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_ranges_compress_runs() {
        assert_eq!(group_ranges(&[1, 2, 3, 7, 9, 10]), "1..3 7 9..10");
        assert_eq!(group_ranges(&[]), "");
        assert_eq!(group_ranges(&[5]), "5");
    }

    #[test]
    fn watched_flags_include_the_lobby_set() {
        let w = watched_flags();
        for f in [6000, 6001, 10_002_700, 10_002_707, 10_003_045, 69_990, 10_007_000] {
            assert!(w.contains(&f), "{f}");
        }
    }

    // ---- a synthetic CSEventFlagMan in heap memory (addresses are real, so RegionCache works) ----

    struct Arena {
        buf: Vec<u8>,
        base: usize,
    }

    impl Arena {
        fn new() -> Self {
            let buf = vec![0u8; 0x2000];
            let p = buf.as_ptr() as usize;
            let base = (p + 15) & !15;
            Arena { buf, base }
        }
        fn addr(&self, off: usize) -> usize {
            self.base + off
        }
        fn idx(&self, off: usize) -> usize {
            self.base - self.buf.as_ptr() as usize + off
        }
        fn put(&mut self, off: usize, bytes: &[u8]) {
            let i = self.idx(off);
            self.buf[i..i + bytes.len()].copy_from_slice(bytes);
        }
        fn put64(&mut self, off: usize, v: u64) {
            self.put(off, &v.to_le_bytes());
        }
        fn put32(&mut self, off: usize, v: u32) {
            self.put(off, &v.to_le_bytes());
        }
        #[allow(clippy::too_many_arguments)]
        fn node(
            &mut self,
            off: usize,
            left: usize,
            parent: usize,
            right: usize,
            nil: bool,
            key: u32,
            mode: u32,
            loc: u64,
        ) {
            self.put64(off + NODE_LEFT, left as u64);
            self.put64(off + NODE_PARENT, parent as u64);
            self.put64(off + NODE_RIGHT, right as u64);
            self.put(off + NODE_IS_NIL, &[u8::from(nil)]);
            self.put32(off + NODE_KEY, key);
            self.put32(off + NODE_MODE, mode);
            self.put64(off + NODE_LOC, loc);
        }
    }

    const HEAD: usize = 0x100;
    const N3: usize = 0x200;
    const N1: usize = 0x240;
    const N7: usize = 0x280;
    const N5: usize = 0x2C0;
    const BLOCKS: usize = 0x400;
    const EXT: usize = 0x800;

    /// Groups 1, 3 (root), 5, 7. 1/3/7 use mode 1 (offsets 0/1/2), 5 uses mode 2 (external block).
    fn arena() -> (Arena, EfLayout) {
        let mut a = Arena::new();
        let head = a.addr(HEAD);
        let (n3, n1, n7, n5) = (a.addr(N3), a.addr(N1), a.addr(N7), a.addr(N5));
        a.node(HEAD, n1, n3, n7, true, 0, 0, 0);
        a.node(N3, n1, head, n7, false, 3, 1, 1);
        a.node(N1, head, n3, head, false, 1, 1, 0);
        a.node(N7, n5, n3, head, false, 7, 1, 2);
        a.node(N5, head, n7, head, false, 5, 2, a.addr(EXT) as u64);
        // Group 7, flag 7502: rem 502 -> byte 62, bit 7 - 6 = 1. Flag 7000: byte 0 bit 7. Flag 7001: byte 0 bit 6.
        let g7 = BLOCKS + 2 * 125;
        a.put(g7 + 62, &[0b0000_0010]);
        a.put(g7, &[0b1000_0000]);
        // Group 5, flag 5010: rem 10 -> byte 1, bit 7 - 2 = 5, in the external block.
        a.put(EXT + 1, &[0b0010_0000]);
        let l = EfLayout {
            divisor: 1000,
            holder_size: 125,
            holder_count: 4,
            blocks: a.addr(BLOCKS),
            head,
            map_size: 4,
            world_type: 0,
            blocks_readable: true,
            head_readable: true,
            head_is_nil: Some(1),
        };
        (a, l)
    }

    #[test]
    fn flag_arithmetic_matches_the_fsrs_algorithm() {
        assert_eq!(flag_split(7502, 1000), Some((7, 62, 1)));
        assert_eq!(flag_split(7000, 1000), Some((7, 0, 7)));
        assert_eq!(flag_split(7001, 1000), Some((7, 0, 6)));
        assert_eq!(flag_split(2200, 1000), Some((2, 25, 7)));
        assert_eq!(flag_split(2201, 1000), Some((2, 25, 6)));
        assert_eq!(flag_split(999, 1000), Some((0, 124, 0)));
        assert_eq!(flag_split(5, 0), None);
        // Every watched flag lands inside a 125-byte holder for divisor 1000.
        for f in watched_flags() {
            let (_, byte, bit) = flag_split(f, 1000).expect("split");
            assert!(byte < 125 && bit < 8, "{f}");
        }
    }

    #[test]
    fn block_addresses_by_mode() {
        assert_eq!(block_addr(1, 2, 0x10000, 125), Some(0x10000 + 250));
        assert_eq!(
            block_addr(1, 0x1_0000_0002, 0x10000, 125),
            Some(0x10000 + 250)
        );
        assert_eq!(block_addr(1, 2, 0, 125), None); // null block array
        assert_eq!(
            block_addr(2, 0x7FF0_0000_1000, 0, 125),
            Some(0x7FF0_0000_1000)
        );
        assert_eq!(block_addr(2, 0, 0x10000, 125), None);
        assert_eq!(block_addr(2, 0x1003, 0x10000, 125), None); // unaligned
        assert_eq!(block_addr(0, 1, 0x10000, 125), None);
        assert_eq!(block_addr(3, 1, 0x10000, 125), None);
    }

    #[test]
    fn lookup_walks_the_tree_and_reads_bits() {
        let (_a, l) = arena();
        let mut rc = RegionCache::new();
        assert_eq!(lookup_flag(&mut rc, &l, 7502), FlagState::On);
        assert_eq!(lookup_flag(&mut rc, &l, 7000), FlagState::On);
        assert_eq!(lookup_flag(&mut rc, &l, 7001), FlagState::Off);
        assert_eq!(lookup_flag(&mut rc, &l, 7503), FlagState::Off);
        assert_eq!(lookup_flag(&mut rc, &l, 7501), FlagState::Off);
        // Mode 2 (external block), a group deeper in the tree.
        assert_eq!(lookup_flag(&mut rc, &l, 5010), FlagState::On);
        assert_eq!(lookup_flag(&mut rc, &l, 5011), FlagState::Off);
        // Groups 1 and 3 exist but are empty; groups 2 and 9 are absent.
        assert_eq!(lookup_flag(&mut rc, &l, 1000), FlagState::Off);
        assert_eq!(lookup_flag(&mut rc, &l, 3999), FlagState::Off);
        assert_eq!(lookup_flag(&mut rc, &l, 2200), FlagState::NoBlock);
        assert_eq!(lookup_flag(&mut rc, &l, 9000), FlagState::NoBlock);
        // Byte index beyond the holder.
        let small = EfLayout {
            holder_size: 10,
            ..l.clone()
        };
        assert!(matches!(
            lookup_flag(&mut rc, &small, 7502),
            FlagState::Unreadable(_)
        ));
        // Zero divisor.
        let zero = EfLayout {
            divisor: 0,
            ..l.clone()
        };
        assert!(matches!(
            lookup_flag(&mut rc, &zero, 7502),
            FlagState::Unreadable(_)
        ));
    }

    #[test]
    fn garbage_trees_do_not_hang_or_fault() {
        let (mut a, l) = arena();
        // A cycle: node 7 points left at itself.
        let n7 = a.addr(N7);
        a.node(N7, n7, a.addr(N3), a.addr(HEAD), false, 7, 1, 2);
        let mut rc = RegionCache::new();
        // Group 6 < 7 walks left forever -> depth guard.
        assert!(matches!(
            lookup_flag(&mut rc, &l, 6000),
            FlagState::Unreadable(_)
        ));
        // Wild head pointer.
        let wild = EfLayout {
            head: 0x1234_5678,
            ..l.clone()
        };
        assert!(matches!(
            lookup_flag(&mut rc, &wild, 7502),
            FlagState::Unreadable(_)
        ));
    }

    #[test]
    fn structural_checks_pass_and_fail_visibly() {
        let (_a, l) = arena();
        let checks = structural_checks(&l);
        assert_eq!(checks.len(), 6);
        assert!(checks_ok(&checks), "{checks:?}");
        let bad = EfLayout {
            divisor: 0,
            holder_size: 0,
            blocks: 0,
            head_is_nil: Some(0),
            map_size: 0,
            ..l.clone()
        };
        let failed: Vec<&str> = structural_checks(&bad)
            .iter()
            .filter(|c| !c.ok)
            .map(|c| c.name)
            .collect();
        assert_eq!(
            failed,
            [
                "divisor",
                "holder_size",
                "flag_blocks pointer",
                "descriptor map head",
                "descriptor map size"
            ]
        );
        let small_holder = EfLayout {
            holder_size: 100, // < ceil(1000 / 8)
            ..l
        };
        assert!(!checks_ok(&structural_checks(&small_holder)));
    }

    #[test]
    fn layout_parses_from_the_header_bytes() {
        let mut b = vec![0u8; EF_HEADER_LEN];
        b[EF_DIVISOR..EF_DIVISOR + 4].copy_from_slice(&1000u32.to_le_bytes());
        b[EF_HOLDER_SIZE..EF_HOLDER_SIZE + 4].copy_from_slice(&125u32.to_le_bytes());
        b[EF_HOLDER_COUNT..EF_HOLDER_COUNT + 4].copy_from_slice(&7u32.to_le_bytes());
        b[EF_BLOCKS..EF_BLOCKS + 8].copy_from_slice(&0x1111_0000u64.to_le_bytes());
        b[EF_MAP_HEAD..EF_MAP_HEAD + 8].copy_from_slice(&0x2222_0000u64.to_le_bytes());
        b[EF_MAP_SIZE..EF_MAP_SIZE + 8].copy_from_slice(&9u64.to_le_bytes());
        b[EF_WORLD_TYPE..EF_WORLD_TYPE + 4].copy_from_slice(&3u32.to_le_bytes());
        let l = parse_layout(&b).expect("parse");
        assert_eq!(
            (
                l.divisor,
                l.holder_size,
                l.holder_count,
                l.map_size,
                l.world_type
            ),
            (1000, 125, 7, 9, 3)
        );
        assert_eq!((l.blocks, l.head), (0x1111_0000, 0x2222_0000));
        assert!(parse_layout(&b[..0x70]).is_none());
    }

    #[test]
    fn flag_tracker_logs_a_summary_then_only_changes() {
        let mut t = FlagTracker::default();
        let first = [
            (7000, FlagState::Off),
            (7500, FlagState::On),
            (7501, FlagState::NoBlock),
            (2200, FlagState::Unreadable("x")),
        ];
        let l = t.update(&first);
        assert_eq!(l.len(), 1);
        assert!(
            l[0].contains("ON=[7500]") && l[0].contains("off=1"),
            "{l:?}"
        );
        assert!(t.update(&first).is_empty());
        let second = [
            (7000, FlagState::Off),
            (7500, FlagState::On),
            (7501, FlagState::On),
            (2200, FlagState::Unreadable("x")),
        ];
        let l = t.update(&second);
        assert_eq!(l, vec!["flag 7501: no-block -> ON".to_string()]);
        t.reset();
        assert_eq!(t.update(&second).len(), 1, "a reset starts with a summary");
    }

    // ---- clock ----

    fn packed(h: u64, m: u64, s: u64) -> u64 {
        (2026u64) | (1 << 22) | (1 << 29) | (h << 34) | (m << 39) | (s << 45)
    }

    fn clock_bytes(date: u64, target: (u32, u32, u32), mult: f32) -> Vec<u8> {
        let mut b = vec![0u8; CLOCK_READ_LEN];
        b[CLOCK_DATE..CLOCK_DATE + 8].copy_from_slice(&date.to_le_bytes());
        b[CLOCK_TARGET..CLOCK_TARGET + 4].copy_from_slice(&target.0.to_le_bytes());
        b[CLOCK_TARGET + 4..CLOCK_TARGET + 8].copy_from_slice(&target.1.to_le_bytes());
        b[CLOCK_TARGET + 8..CLOCK_TARGET + 12].copy_from_slice(&target.2.to_le_bytes());
        b[CLOCK_MULT..CLOCK_MULT + 4].copy_from_slice(&mult.to_le_bytes());
        b
    }

    #[test]
    fn packed_date_decodes_like_the_fsrs_bitfield() {
        let p = decode_packed(packed(23, 59, 58) | (1 << 51) | (5 << 26) | (500 << 12));
        assert_eq!(
            (p.year, p.month, p.day, p.hours, p.minutes, p.seconds),
            (2026, 1, 1, 23, 59, 58)
        );
        assert_eq!((p.millisecond, p.day_of_week, p.utc), (500, 5, true));
        assert!(p.plausible());
        assert!(!decode_packed(packed(31, 0, 0)).plausible());
        assert!(!decode_packed(packed(1, 63, 0)).plausible());
        assert!(!decode_packed(0xFFFF_FFFF_FFFF_FFFF).plausible());
    }

    #[test]
    fn clock_log_reasons() {
        let s = |h, m, sec, mult| {
            parse_clock(&clock_bytes(packed(h, m, sec), (0, 0, 0), mult)).expect("parse")
        };
        let a = s(7, 0, 0, 1.0);
        assert_eq!(clock_log_reason(None, &a, 0), Some("first sample"));
        // Same hour, small step, recent log: nothing.
        assert_eq!(clock_log_reason(Some(&a), &s(7, 1, 0, 1.0), 1000), None);
        assert_eq!(
            clock_log_reason(Some(&a), &s(8, 0, 5, 1.0), 1000),
            Some("hour changed")
        );
        assert_eq!(
            clock_log_reason(Some(&a), &s(7, 1, 0, 2.0), 1000),
            Some("target or speed changed")
        );
        // Backwards jump (the day restart) and a far-forward jump.
        let late = s(23, 50, 0, 1.0);
        let early = s(7, 0, 0, 1.0);
        assert!(clock_log_reason(Some(&late), &early, 1000).is_some_and(|r| r.contains("jump")));
        assert!(clock_log_reason(Some(&a), &s(7, 30, 0, 1.0), 0).is_none());
        assert!(clock_log_reason(Some(&a), &s(10, 0, 0, 1.0), 0).is_some());
        // Periodic.
        assert_eq!(
            clock_log_reason(Some(&a), &s(7, 1, 0, 1.0), CLOCK_PERIODIC_MS),
            Some("periodic")
        );
        // Target change.
        let t = parse_clock(&clock_bytes(packed(7, 0, 0), (7, 0, 0), 1.0)).expect("parse");
        assert_eq!(
            clock_log_reason(Some(&a), &t, 0),
            Some("target or speed changed")
        );
        // A garbage sample is not plausible.
        let junk = parse_clock(&clock_bytes(u64::MAX, (0, 0, 0), f32::NAN)).expect("parse");
        assert!(!junk.plausible());
    }

    #[test]
    fn byte_tracker_reports_changed_qwords_only() {
        let mut first = vec![0u8; 32];
        first[8] = 1;
        let mut t = ByteTracker::new(&first);
        let mut next = first.clone();
        next[8] = 2;
        next[20] = 9;
        t.feed(&next);
        next[8] = 3;
        t.feed(&next);
        t.feed(&next[..16]); // wrong length: ignored
        let r = t.report();
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(
            r[0].starts_with("+0x008:")
                && r[0].contains("0x0000000000000001 -> 0x0000000000000003")
        );
        assert!(r[0].contains("2 byte changes over 3 samples"), "{r:?}");
        assert!(r[1].starts_with("+0x010:"), "{r:?}");
        assert_eq!(
            ByteTracker::new(&first).report(),
            vec!["no byte changed over 1 samples".to_string()]
        );
    }

    // ---- player data ----

    #[test]
    fn plausibility_filters() {
        let ok = assess(Some(7), Some(12_345), Some(300), Some(450));
        assert!(ok.all());
        assert!(assess(Some(1), Some(0), Some(1), Some(1)).all());
        assert!(assess(Some(15), Some(9_999_999), Some(450), Some(450)).all());
        assert!(!assess(Some(0), Some(0), Some(1), Some(1)).level);
        assert!(!assess(Some(16), Some(0), Some(1), Some(1)).level);
        assert!(!assess(Some(7), Some(10_000_000), Some(1), Some(1)).runes);
        assert!(!assess(Some(7), Some(0), Some(500), Some(450)).hp);
        assert!(!assess(Some(7), Some(0), Some(0), Some(0)).hp);
        assert!(!assess(Some(7), Some(0), Some(1), Some(1_000_000)).hp);
        assert!(!assess(None, None, None, None).all());
        assert!(!assess(Some(7), Some(1), None, Some(5)).hp);
    }

    #[test]
    fn names_decode_or_are_rejected() {
        let mut u: Vec<u16> = "Wylder".encode_utf16().collect();
        u.resize(PGD_NAME_UNITS, 0);
        assert_eq!(decode_name(&u).as_deref(), Some("Wylder"));
        assert_eq!(decode_name(&[0; PGD_NAME_UNITS]), None);
        assert_eq!(decode_name(&[0xD800, 0, 0]), None); // lone surrogate
        assert_eq!(decode_name(&[0x41, 0x07, 0]), None); // control char
    }

    #[test]
    fn candidates_are_read_from_synthetic_player_memory() {
        let mut a = Arena::new();
        let (player, pgd, bogus) = (0x000, 0x400, 0x800);
        let base_img = 0x7FF6_0000_0000usize;
        // Variant +0x588 points at a believable PlayerGameData; +0x580 at an object whose numbers are garbage;
        // +0x590 is null.
        a.put64(player + 0x588, a.addr(pgd) as u64);
        a.put64(player + 0x580, a.addr(bogus) as u64);
        a.put64(pgd, (base_img + 0x2C0_0000) as u64);
        a.put32(pgd + PGD_HP, 300);
        a.put32(pgd + PGD_MAX_HP, 450);
        a.put32(pgd + PGD_LEVEL, 9);
        a.put32(pgd + PGD_RUNES, 4321);
        for (i, u) in "Wylder".encode_utf16().enumerate() {
            a.put(pgd + PGD_NAME + i * 2, &u.to_le_bytes());
        }
        a.put64(bogus, 0x1234);
        a.put32(bogus + PGD_LEVEL, 0xFFFF_FFFF);
        a.put32(bogus + PGD_HP, 99);
        let mut rc = RegionCache::new();
        let c = read_pgd_candidates(&mut rc, a.addr(player), base_img, 0x800_0000);
        assert_eq!(c.len(), 3);
        assert_eq!(
            c.iter().map(|c| c.ptr_off).collect::<Vec<_>>(),
            [0x580, 0x588, 0x590]
        );
        assert!(c[0].ptr.is_some() && !c[0].vtable_in_image && !c[0].plausibility.all());
        assert!(c[1].plausibility.all() && c[1].vtable_in_image);
        assert_eq!(
            (c[1].level, c[1].runes, c[1].hp, c[1].max_hp),
            (Some(9), Some(4321), Some(300), Some(450))
        );
        assert_eq!(c[1].name.as_deref(), Some("Wylder"));
        assert_eq!(c[2].ptr, None);
        assert!(!c[2].plausibility.all());
        assert!(c[1].line(base_img, 0x800_0000).contains("LOOKS PLAUSIBLE"));
        assert!(c[0].line(base_img, 0x800_0000).contains("not plausible"));
        assert!(c[2].overlay_line().contains("[no]"));
        // An unreadable player address yields three empty candidates, not a fault.
        let none = read_pgd_candidates(&mut rc, 0x10, base_img, 0x800_0000);
        assert!(none.iter().all(|c| c.ptr.is_none()));
    }

    #[test]
    fn probes_are_inert_when_disabled() {
        let cfg = Config::default();
        assert!(!cfg.probes);
        let mut p = Probes::new(&cfg, 0, 0);
        assert!(!p.enabled() && !p.slow_due());
        p.tick(
            &[],
            0x1000,
            &[Target {
                index: 0,
                addr: 0x1234,
                entry_addr: 0,
                local: true,
            }],
        );
        assert!(p.lines().is_empty());
    }

    #[test]
    fn enabled_probes_survive_missing_singletons_and_a_wild_player() {
        let cfg = Config {
            probes: true,
            ..Config::default()
        };
        let mut p = Probes::new(&cfg, 0x1_0000_0000, 0x1000);
        assert!(p.slow_due());
        p.tick(
            &[],
            0x1000,
            &[Target {
                index: 0,
                addr: 0x20,
                entry_addr: 0,
                local: true,
            }],
        );
        assert!(!p.lines().is_empty());
        assert!(p.lines().iter().any(|l| l.contains("not live")));
        assert!(!p.slow_due(), "interval not elapsed yet");
    }
}
