//! T-008: debug controls for the team type of the players in the ChrSet. Resolves every PlayerIns from the
//! WorldChrMan ChrSet, watches them, and (on hotkeys / overlay buttons) writes `team_type`.
//!
//! Layout used here, all `VERIFIED on NR 1.3.3.0 by T-007a; see TECHNICAL_NOTES §4`:
//! - WorldChrMan+0x9A40: inline ChrSet. ChrSet+0x10 u32 capacity (6 observed, SC's cap), ChrSet+0x18 pointer
//!   to the entries; entries are 0x10 bytes, the first qword is a `ChrIns*`.
//! - ChrIns+0x68 u32 id, +0x70 u32 chr_type-like, **+0x74 u8 team_type** (the engine's hostility field,
//!   VERIFIED by TC-004 runs 2 to 4).
//! - WorldChrMan+0x1C820 is NOT a player pointer (it held UTF-16 text, TC-004 run 3). T-015 removed the old
//!   rule that used it.
//!
//! T-015 (FS-1 tooling): every valid PlayerIns can be read and written, local or remote. Which one is
//! LOCAL is decided by [`decide_local`]: the only PlayerIns, else a *suggestion* from an UNVERIFIED
//! signal that splits one-vs-rest, else UNKNOWN; the HUMAN can always override ("this is me").
//!
//! The write (hotkeys, overlay, persist) is the only memory write in the project. It is a single u8 store,
//! only at `PlayerIns`+0x74, only after the object's vtable was re-read and matched a PlayerIns-derived
//! class from RTTI ([`write_team_type`]). No hooks.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use super::discover::{self, is_ptr, Budgets};
use super::mem::{self, va_rva, RegionCache};
use super::rtti::Rtti;
use crate::config::Config;
use crate::mark::Marker;
use crate::overlay::shared::{PlayerRow, Role, Snapshot};

// ---- offsets: VERIFIED on NR 1.3.3.0 by T-007a; see TECHNICAL_NOTES §4 ----
/// WorldChrMan -> inline ChrSet.
pub(crate) const WCM_CHRSET: usize = 0x9A40;
/// ChrSet -> capacity (u32).
const CHRSET_CAPACITY: usize = 0x10;
/// ChrSet -> entries pointer.
const CHRSET_ENTRIES: usize = 0x18;
/// Size of one ChrSet entry; the first qword is the ChrIns pointer.
pub(crate) const ENTRY_SIZE: usize = 0x10;
/// ChrIns fields.
const CHR_ID: usize = 0x68;
const CHR_TYPE70: usize = 0x70;
/// ChrIns+0x74: team_type (u8).
pub(crate) const CHR_TEAM: usize = 0x74;
// ---- UNVERIFIED local/remote candidates (T-015): ER offsets from .claude/re/er-reference.md plus NR's
// verified +8 shift for the ChrIns head. Read-only evidence for the log; nothing here is a fact yet. ----
/// ER ChrIns+0x70 `p2p_entity_handle` (8 bytes) -> NR +0x78.
const CHR_P2P_HANDLE: usize = 0x78;
/// ER ChrIns+0x1E4 `network_authority` (u32) -> NR +0x1EC.
const CHR_NET_AUTHORITY: usize = 0x1EC;
/// ER PlayerIns+0x598 `player_session_holder` -> NR +0x5A0.
const PLAYER_SESSION_HOLDER: usize = 0x5A0;
/// ER PlayerIns+0x6B8 `session_manager_player_entry` -> NR +0x6C0.
const PLAYER_SESSION_ENTRY: usize = 0x6C0;
/// Largest ChrSet capacity we accept (6 expected).
const MAX_CAPACITY: u32 = 16;
/// PlayerIns vtable seen by T-007a (only used for a sanity log against the RTTI result).
const PLAYER_VTABLE_RVA: usize = 0x2C307F8;

/// ChrSet change lines are coalesced to at most one burst per this long.
const RESOLVE_LOG_MIN_GAP_MS: u64 = 5000;
/// Persist summary period.
const PERSIST_SUMMARY_MS: u64 = 1000;
/// How often the overlay snapshot re-reads the players (T-013).
const LIVE_REFRESH: Duration = Duration::from_millis(100);

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

fn hex16(raw: &[u8; 16]) -> String {
    let mut s = String::with_capacity(48);
    for (i, b) in raw.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{b:02X}");
    }
    s
}

/// ChrSet capacity accepted for resolution: 1..=16 (6 expected on NR 1.3.3.0).
pub(crate) fn capacity_ok(c: u32) -> bool {
    (1..=MAX_CAPACITY).contains(&c)
}

/// One raw ChrSet entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RawEntry {
    pub index: usize,
    pub raw: [u8; ENTRY_SIZE],
    /// First qword: the `ChrIns*` (0 = empty slot).
    pub ptr: usize,
}

/// Split the entries buffer into `capacity` 0x10-byte entries. Stops at the end of the buffer.
pub(crate) fn parse_entries(buf: &[u8], capacity: usize) -> Vec<RawEntry> {
    let mut out = Vec::new();
    for index in 0..capacity {
        let Some(chunk) = buf.get(index * ENTRY_SIZE..(index + 1) * ENTRY_SIZE) else {
            break;
        };
        let mut raw = [0u8; ENTRY_SIZE];
        raw.copy_from_slice(chunk);
        let ptr = le64(&raw, 0).unwrap_or(0) as usize;
        out.push(RawEntry { index, raw, ptr });
    }
    out
}

/// One observed value that might tell the local PlayerIns from a remote one. **UNVERIFIED**: the names
/// carry the ER offset the idea came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Signal {
    pub name: &'static str,
    /// `None` = unreadable.
    pub value: Option<u64>,
    /// Shown as hex in the log.
    pub hex: bool,
    /// May take part in the one-vs-rest suggestion. Log-only signals (raw handles and pointers) do not.
    pub decisive: bool,
}

impl Signal {
    fn text(&self) -> String {
        match (self.value, self.hex) {
            (None, _) => format!("{}=?", self.name),
            (Some(v), true) => format!("{}=0x{v:X}", self.name),
            (Some(v), false) => format!("{}={v}", self.name),
        }
    }
}

/// A valid PlayerIns of the ChrSet with its candidate signals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlayerProbe {
    pub addr: usize,
    /// ChrSet entry index of its first entry.
    pub index: usize,
    pub signals: Vec<Signal>,
}

/// If exactly one of the `vals` differs from all the others, and the others all agree, return that one.
/// Needs at least three players: with two, "one vs the rest" cannot say which of them is the odd one.
/// Any unreadable value disables the signal.
pub(crate) fn odd_one_out(vals: &[(usize, Option<u64>)]) -> Option<usize> {
    if vals.len() < 3 {
        return None;
    }
    let mut groups: Vec<(u64, Vec<usize>)> = Vec::new();
    for &(addr, v) in vals {
        let v = v?;
        match groups.iter_mut().find(|g| g.0 == v) {
            Some(g) => g.1.push(addr),
            None => groups.push((v, vec![addr])),
        }
    }
    if groups.len() != 2 {
        return None;
    }
    let singles: Vec<&(u64, Vec<usize>)> = groups.iter().filter(|g| g.1.len() == 1).collect();
    match singles.as_slice() {
        [one] => Some(one.1[0]),
        _ => None,
    }
}

/// How the local player was decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Basis {
    /// No PlayerIns at all.
    NoPlayers,
    /// Exactly one PlayerIns in the ChrSet.
    OnlyPlayer,
    /// The HUMAN pressed "this is me".
    Chosen,
    /// Suggestion from these UNVERIFIED signals.
    Heuristic(String),
    /// Several PlayerIns and no way to tell; the string says why.
    Unknown(String),
}

impl Basis {
    /// Short text for the overlay and the logs.
    pub(crate) fn label(&self) -> String {
        match self {
            Basis::NoPlayers => "no PlayerIns".to_string(),
            Basis::OnlyPlayer => "only PlayerIns in the ChrSet".to_string(),
            Basis::Chosen => "chosen by you (this is me)".to_string(),
            Basis::Heuristic(s) => format!("heuristic: {s}"),
            Basis::Unknown(why) => format!("UNKNOWN: {why}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Decision {
    pub local: Option<usize>,
    pub basis: Basis,
}

/// Decide which valid PlayerIns is "local" (T-015). In order:
/// 1. no PlayerIns: none;
/// 2. the HUMAN's "this is me" choice, if that address is still one of the players;
/// 3. exactly one PlayerIns: it is local;
/// 4. a *suggestion*: every decisive signal that splits the players exactly one-vs-rest (three or more
///    players) names its odd one out; if at least one does and they all name the same player, that player
///    is suggested, tagged with the signals;
/// 5. otherwise UNKNOWN and nobody is local.
pub(crate) fn decide_local(probes: &[PlayerProbe], chosen: Option<usize>) -> Decision {
    if probes.is_empty() {
        return Decision {
            local: None,
            basis: Basis::NoPlayers,
        };
    }
    if let Some(c) = chosen.filter(|c| probes.iter().any(|p| p.addr == *c)) {
        return Decision {
            local: Some(c),
            basis: Basis::Chosen,
        };
    }
    if let [p] = probes {
        return Decision {
            local: Some(p.addr),
            basis: Basis::OnlyPlayer,
        };
    }
    if probes.len() < 3 {
        return Decision {
            local: None,
            basis: Basis::Unknown(format!(
                "{} PlayerIns; a signal that differs cannot say which of two is local (use \"this is me\")",
                probes.len()
            )),
        };
    }
    // Signals are positional: probe[0].signals[k] is the same signal as probe[1].signals[k].
    let n_sig = probes.iter().map(|p| p.signals.len()).min().unwrap_or(0);
    let mut hits: Vec<(&'static str, usize)> = Vec::new();
    for k in 0..n_sig {
        if !probes[0].signals[k].decisive {
            continue;
        }
        let vals: Vec<(usize, Option<u64>)> = probes
            .iter()
            .map(|p| (p.addr, p.signals[k].value))
            .collect();
        if let Some(odd) = odd_one_out(&vals) {
            hits.push((probes[0].signals[k].name, odd));
        }
    }
    let Some(&(_, first)) = hits.first() else {
        return Decision {
            local: None,
            basis: Basis::Unknown(
                "no candidate signal splits the players one-vs-rest (use \"this is me\")"
                    .to_string(),
            ),
        };
    };
    if hits.iter().any(|&(_, a)| a != first) {
        let list: Vec<String> = hits
            .iter()
            .map(|&(n, a)| format!("{n} -> 0x{a:X}"))
            .collect();
        return Decision {
            local: None,
            basis: Basis::Unknown(format!(
                "candidate signals disagree ({}); use \"this is me\"",
                list.join(", ")
            )),
        };
    }
    let names: Vec<&str> = hits.iter().map(|&(n, _)| n).collect();
    Decision {
        local: Some(first),
        basis: Basis::Heuristic(names.join(", ")),
    }
}

/// `LOCAL`, `REMOTE[i]` or `UNKNOWN[i]` (`i` = ChrSet entry index), the `who=` field of every write line.
pub(crate) fn who_label(role: Role, index: usize) -> String {
    match role {
        Role::Local => "LOCAL".to_string(),
        Role::Remote => format!("REMOTE[{index}]"),
        Role::Unknown => format!("UNKNOWN[{index}]"),
    }
}

/// The configured team-type candidates and which one is selected (cycle hotkey / overlay buttons).
pub(crate) struct Cycler {
    list: Vec<u8>,
    idx: usize,
}

impl Cycler {
    pub(crate) fn new(list: &[u8]) -> Self {
        let list = if list.is_empty() {
            crate::config::DEFAULT_CANDIDATES.to_vec()
        } else {
            list.to_vec()
        };
        Cycler { list, idx: 0 }
    }

    pub(crate) fn current(&self) -> u8 {
        self.list[self.idx]
    }

    /// 1-based position and list length, for `selected team candidate = N (i/len)`.
    pub(crate) fn position(&self) -> (usize, usize) {
        (self.idx + 1, self.list.len())
    }

    pub(crate) fn next(&mut self) -> u8 {
        self.idx = (self.idx + 1) % self.list.len();
        self.current()
    }

    /// Select the first list entry equal to `v` (overlay buttons). `false`, and no change, if `v` is not
    /// one of the candidates.
    pub(crate) fn select(&mut self, v: u8) -> bool {
        match self.list.iter().position(|&c| c == v) {
            Some(i) => {
                self.idx = i;
                true
            }
            None => false,
        }
    }

    pub(crate) fn list(&self) -> &[u8] {
        &self.list
    }
}

/// F7 persist bookkeeping: counts how often the game changed the value back after we had set it.
#[derive(Default)]
pub(crate) struct PersistTracker {
    /// True once we know the value was (or is being) set to `selected` on `last_target`.
    armed: bool,
    last_target: usize,
    last_selected: u8,
    last_seen: u8,
    window_reverts: u32,
    /// Reverts counted in the last completed one-second window (shown by the overlay).
    last_window_reverts: u32,
    total_reverts: u64,
    window_start_ms: u64,
    last_fail_log_ms: Option<u64>,
}

impl PersistTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Start a fresh session (persist toggled on).
    pub(crate) fn reset(&mut self, now_ms: u64) {
        *self = PersistTracker {
            window_start_ms: now_ms,
            ..Self::default()
        };
    }

    /// Feed one observation of the live value. Returns true if the caller must rewrite it. A mismatch counts
    /// as a revert only if we had already established the selected value on this very object (a changed
    /// target or selection restarts that: the first mismatch is our own initial application).
    pub(crate) fn observe(&mut self, target: usize, selected: u8, seen: u8) -> bool {
        if target != self.last_target || selected != self.last_selected {
            self.armed = false;
            self.last_target = target;
            self.last_selected = selected;
        }
        self.last_seen = seen;
        if seen == selected {
            self.armed = true;
            return false;
        }
        if self.armed {
            self.window_reverts += 1;
            self.total_reverts += 1;
        }
        // We are about to rewrite it; if that fails the caller calls `write_failed`.
        self.armed = true;
        true
    }

    pub(crate) fn write_failed(&mut self) {
        self.armed = false;
    }

    /// Rate limit for write-failure log lines: true at most once per second.
    pub(crate) fn should_log_failure(&mut self, now_ms: u64) -> bool {
        let ok = self
            .last_fail_log_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= PERSIST_SUMMARY_MS);
        if ok {
            self.last_fail_log_ms = Some(now_ms);
        }
        ok
    }

    pub(crate) fn total_reverts(&self) -> u64 {
        self.total_reverts
    }

    /// Reverts in the last completed one-second window (0 if the last second was quiet).
    pub(crate) fn reverts_last_window(&self) -> u32 {
        self.last_window_reverts
    }

    /// Once per second: the summary line if the game reverted the value during that second, else `None`.
    pub(crate) fn summary(&mut self, now_ms: u64) -> Option<String> {
        if now_ms.saturating_sub(self.window_start_ms) < PERSIST_SUMMARY_MS {
            return None;
        }
        let n = self.window_reverts;
        self.last_window_reverts = n;
        self.window_reverts = 0;
        self.window_start_ms = now_ms;
        (n > 0).then(|| {
            format!(
                "persist: {n} reverts in last 1 s, last seen value {}",
                self.last_seen
            )
        })
    }
}

/// The fields the watcher logs for one PlayerIns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fields {
    pub id: u32,
    pub v70: u32,
    pub team: u8,
}

/// Remembers the last fields per PlayerIns address so only changes are logged.
#[derive(Default)]
pub(crate) struct WatchState {
    seen: HashMap<usize, Fields>,
}

impl WatchState {
    /// `Some(previous)` if `addr` is new (`previous` = None) or its fields changed; `None` if unchanged.
    pub(crate) fn update(&mut self, addr: usize, f: Fields) -> Option<Option<Fields>> {
        match self.seen.insert(addr, f) {
            Some(prev) if prev == f => None,
            prev => Some(prev),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.seen.clear();
    }
}

fn watch_line(addr: usize, who: &str, f: Fields, prev: Option<Fields>) -> String {
    let mut s = format!(
        "watch: PlayerIns 0x{addr:X} who={who} +0x68 id={} +0x70={} +0x74 team_type={}",
        f.id,
        f.v70,
        f.team
    );
    if let Some(p) = prev {
        let _ = write!(s, " (was id={} +0x70={} team_type={})", p.id, p.v70, p.team);
    }
    s
}

/// Histogram helpers for F8: overall counts per team value and per class.
pub(crate) fn histogram(
    items: &[(String, u8)],
) -> (BTreeMap<u8, usize>, BTreeMap<String, BTreeMap<u8, usize>>) {
    let mut all: BTreeMap<u8, usize> = BTreeMap::new();
    let mut per_class: BTreeMap<String, BTreeMap<u8, usize>> = BTreeMap::new();
    for (class, team) in items {
        *all.entry(*team).or_insert(0) += 1;
        *per_class
            .entry(class.clone())
            .or_default()
            .entry(*team)
            .or_insert(0) += 1;
    }
    (all, per_class)
}

/// `6 x45, 30 x4`, most frequent first (ties by value).
pub(crate) fn fmt_counts(m: &BTreeMap<u8, usize>) -> String {
    let mut v: Vec<(u8, usize)> = m.iter().map(|(&k, &n)| (k, n)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.iter()
        .map(|(k, n)| format!("{k} x{n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------------------------------
// PlayerIns identification and ChrSet resolution
// ---------------------------------------------------------------------------------------------------

/// Vtable addresses of PlayerIns and every PlayerIns-derived class, from RTTI.
#[derive(Default, Clone)]
pub(crate) struct PlayerVtables(HashSet<usize>);

impl PlayerVtables {
    pub(crate) fn from_rtti(rtti: &Rtti, base: usize) -> Self {
        let mut set = HashSet::new();
        for (i, c) in rtti.classes.iter().enumerate() {
            if rtti.is_player(i) {
                for &vt in &c.vtables {
                    set.insert(base + vt as usize);
                }
            }
        }
        PlayerVtables(set)
    }

    #[cfg(test)]
    pub(crate) fn from_set(set: HashSet<usize>) -> Self {
        PlayerVtables(set)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn contains(&self, vt: usize) -> bool {
        self.0.contains(&vt)
    }

    fn sorted(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.0.iter().copied().collect();
        v.sort_unstable();
        v
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    Empty,
    Player,
    /// Non-null first qword that is not a PlayerIns-derived object (or is unreadable).
    Other,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Entry {
    pub raw: RawEntry,
    pub kind: EntryKind,
}

/// What makes two resolutions "the same" for logging: raw entries, candidate signal values, chosen local.
type ResolveKey = (Vec<[u8; ENTRY_SIZE]>, Vec<Vec<Option<u64>>>, Option<usize>);

pub(crate) struct Resolution {
    pub chrset: usize,
    pub capacity: u32,
    pub entries_ptr: usize,
    pub entries: Vec<Entry>,
    /// Distinct valid PlayerIns of the ChrSet, entry order.
    pub players: Vec<usize>,
    /// The same players with their candidate signals (T-015 evidence).
    pub probes: Vec<PlayerProbe>,
    pub local: Option<usize>,
    pub basis: Basis,
}

impl Resolution {
    fn key(&self) -> ResolveKey {
        (
            self.entries.iter().map(|e| e.raw.raw).collect(),
            self.probes
                .iter()
                .map(|p| p.signals.iter().map(|s| s.value).collect())
                .collect(),
            self.local,
        )
    }

    /// Why the local player is what it is, for logs.
    pub(crate) fn reasoning(&self) -> String {
        format!(
            "{} valid PlayerIns in the ChrSet; local = {} ({})",
            self.players.len(),
            self.local.map_or("NONE".to_string(), |p| format!("0x{p:X}")),
            self.basis.label()
        )
    }

    /// ChrSet entry index of the first entry holding `addr`.
    fn index_of(&self, addr: usize) -> Option<usize> {
        self.probes.iter().find(|p| p.addr == addr).map(|p| p.index)
    }

    /// Local, remote or unknown role of a PlayerIns of this ChrSet.
    pub(crate) fn role_of(&self, addr: usize) -> Role {
        if self.local == Some(addr) {
            Role::Local
        } else if self.local.is_some() {
            Role::Remote
        } else {
            Role::Unknown
        }
    }

    /// `LOCAL` / `REMOTE[i]` / `UNKNOWN[i]` for the log; `None` if `addr` is not a PlayerIns of the ChrSet.
    pub(crate) fn who(&self, addr: usize) -> Option<String> {
        let index = self.index_of(addr)?;
        Some(who_label(self.role_of(addr), index))
    }

    /// Overlay rows, one per valid PlayerIns. `team` reads the live team_type of an address.
    fn rows(
        &self,
        chosen: Option<usize>,
        team: &mut dyn FnMut(usize) -> Option<u8>,
    ) -> Vec<PlayerRow> {
        let mut rows = Vec::new();
        for p in &self.probes {
            let Some(e) = self.entries.get(p.index) else {
                continue;
            };
            let role = self.role_of(p.addr);
            rows.push(PlayerRow {
                index: p.index,
                addr: p.addr,
                role,
                basis: match role {
                    Role::Local => self.basis.label(),
                    Role::Remote => "not the local player".to_string(),
                    Role::Unknown => self.basis.label(),
                },
                chosen: chosen == Some(p.addr),
                team: team(p.addr),
                state8: e.raw.raw[8],
                state9: e.raw.raw[9],
                original: None,
                persist: None,
                reverts_last_sec: 0,
                reverts_total: 0,
            });
        }
        rows
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ResolveError {
    HeaderUnreadable,
    BadCapacity(u32),
    BadEntriesPtr(usize),
    EntriesUnreadable,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::HeaderUnreadable => {
                write!(f, "WorldChrMan+0x9A40 ChrSet header is unreadable")
            }
            ResolveError::BadCapacity(c) => {
                write!(
                    f,
                    "ChrSet capacity {c} is outside 1..=16; refusing to use it"
                )
            }
            ResolveError::BadEntriesPtr(p) => write!(
                f,
                "ChrSet entries pointer 0x{p:X} is not a plausible pointer"
            ),
            ResolveError::EntriesUnreadable => write!(f, "ChrSet entries are unreadable"),
        }
    }
}

/// True if the object at `p` has a PlayerIns-derived vtable.
fn is_player_obj(rc: &mut RegionCache, vts: &PlayerVtables, p: usize) -> bool {
    is_ptr(p) && rc.read::<usize>(p).is_some_and(|vt| vts.contains(vt))
}

/// The candidate signals of one PlayerIns. **UNVERIFIED** (ER offsets shifted by NR's +8, see the consts);
/// reads are VirtualQuery-guarded and an unreadable value is recorded as `None`.
fn read_signals(rc: &mut RegionCache, p: usize, entry: &[u8; ENTRY_SIZE]) -> Vec<Signal> {
    let sig = |name, value, hex, decisive| Signal {
        name,
        value,
        hex,
        decisive,
    };
    let holder = rc.read::<u64>(p + PLAYER_SESSION_HOLDER);
    let sess_entry = rc.read::<u64>(p + PLAYER_SESSION_ENTRY);
    vec![
        sig("entry+8", Some(u64::from(entry[8])), false, true),
        sig("entry+9[ER update_type]", Some(u64::from(entry[9])), false, true),
        sig(
            "chr_type@+0x70[ER +0x68]",
            rc.read::<u32>(p + CHR_TYPE70).map(u64::from),
            false,
            true,
        ),
        sig(
            "net_authority@+0x1EC[ER +0x1E4]",
            rc.read::<u32>(p + CHR_NET_AUTHORITY).map(u64::from),
            false,
            true,
        ),
        sig(
            "session_holder_set@+0x5A0[ER +0x598]",
            holder.map(|v| u64::from(v != 0)),
            false,
            true,
        ),
        sig(
            "session_entry_set@+0x6C0[ER +0x6B8]",
            sess_entry.map(|v| u64::from(v != 0)),
            false,
            true,
        ),
        sig(
            "p2p_handle@+0x78[ER +0x70]",
            rc.read::<u64>(p + CHR_P2P_HANDLE),
            true,
            false,
        ),
        sig("session_holder_ptr", holder, true, false),
        sig("session_entry_ptr", sess_entry, true, false),
    ]
}

/// Resolve the ChrSet and decide the local PlayerIns from a WorldChrMan instance. `chosen` is the HUMAN's
/// "this is me" address, if any. Re-run whenever needed: WorldChrMan is re-created on area change.
/// Read-only.
pub(crate) fn resolve(
    wcm: usize,
    vts: &PlayerVtables,
    chosen: Option<usize>,
) -> Result<Resolution, ResolveError> {
    let mut rc = RegionCache::new();
    // SAFETY-ish: all reads are VirtualQuery-guarded; offsets VERIFIED on NR 1.3.3.0 by T-007a (§4).
    let chrset = wcm + WCM_CHRSET;
    let hdr = rc.read_prefix(chrset, CHRSET_ENTRIES + 8);
    if hdr.len() < CHRSET_ENTRIES + 8 {
        return Err(ResolveError::HeaderUnreadable);
    }
    let capacity = le32(&hdr, CHRSET_CAPACITY).ok_or(ResolveError::HeaderUnreadable)?;
    let entries_ptr = le64(&hdr, CHRSET_ENTRIES).ok_or(ResolveError::HeaderUnreadable)? as usize;
    if !capacity_ok(capacity) {
        return Err(ResolveError::BadCapacity(capacity));
    }
    if !is_ptr(entries_ptr) {
        return Err(ResolveError::BadEntriesPtr(entries_ptr));
    }
    let buf = rc.read_prefix(entries_ptr, capacity as usize * ENTRY_SIZE);
    let raws = parse_entries(&buf, capacity as usize);
    if raws.is_empty() {
        return Err(ResolveError::EntriesUnreadable);
    }

    let mut entries = Vec::with_capacity(raws.len());
    let mut players: Vec<usize> = Vec::new();
    let mut probes: Vec<PlayerProbe> = Vec::new();
    for raw in raws {
        let kind = if raw.ptr == 0 {
            EntryKind::Empty
        } else if is_player_obj(&mut rc, vts, raw.ptr) {
            if !players.contains(&raw.ptr) {
                players.push(raw.ptr);
                probes.push(PlayerProbe {
                    addr: raw.ptr,
                    index: raw.index,
                    signals: read_signals(&mut rc, raw.ptr, &raw.raw),
                });
            }
            EntryKind::Player
        } else {
            EntryKind::Other
        };
        entries.push(Entry { raw, kind });
    }

    let Decision { local, basis } = decide_local(&probes, chosen);
    Ok(Resolution {
        chrset,
        capacity,
        entries_ptr,
        entries,
        players,
        probes,
        local,
        basis,
    })
}

/// Logs the ChrSet entries the first time and when they change (coalesced to one burst per
/// [`RESOLVE_LOG_MIN_GAP_MS`] in case a status field in the entry changes every frame).
#[derive(Default)]
pub(crate) struct ResolveLog {
    last: Option<ResolveKey>,
    last_logged_ms: u64,
    coalesced: u32,
}

impl ResolveLog {
    pub(crate) fn reset(&mut self) {
        *self = ResolveLog::default();
    }

    /// Lines to log now (empty = nothing to say).
    pub(crate) fn lines(
        &mut self,
        res: &Resolution,
        now_ms: u64,
        exe: (usize, usize),
    ) -> Vec<String> {
        let key = res.key();
        let first = self.last.is_none();
        if self.last.as_ref() == Some(&key) {
            return Vec::new();
        }
        if !first && now_ms.saturating_sub(self.last_logged_ms) < RESOLVE_LOG_MIN_GAP_MS {
            self.coalesced += 1;
            return Vec::new();
        }
        self.last = Some(key);
        self.last_logged_ms = now_ms;
        let mut out = Vec::new();
        out.push(format!(
            "team: ChrSet {} (WorldChrMan+0x9A40) capacity={} entries={}{}{} [VERIFIED on NR 1.3.3.0 by T-007a; see TECHNICAL_NOTES §4]",
            va_rva(res.chrset, exe.0, exe.1),
            res.capacity,
            va_rva(res.entries_ptr, exe.0, exe.1),
            if first { " (first resolve)" } else { " (CHANGED)" },
            if self.coalesced > 0 {
                format!(" ({} earlier change(s) coalesced)", self.coalesced)
            } else {
                String::new()
            }
        ));
        self.coalesced = 0;
        for e in &res.entries {
            out.push(format!(
                "team:   entry[{}] raw={} ptr=0x{:X} {}",
                e.raw.index,
                hex16(&e.raw.raw),
                e.raw.ptr,
                match e.kind {
                    EntryKind::Empty => "empty",
                    EntryKind::Player => "PlayerIns",
                    EntryKind::Other => "not a PlayerIns",
                }
            ));
        }
        // T-015 evidence for telling local from remote. UNVERIFIED: ER offsets with NR's +8 shift.
        for p in &res.probes {
            let sigs: Vec<String> = p.signals.iter().map(Signal::text).collect();
            out.push(format!(
                "team:   evidence PlayerIns 0x{:X} entry[{}] raw={} [UNVERIFIED local/remote candidates, ER offsets + NR shift]: {}",
                p.addr,
                p.index,
                res.entries
                    .get(p.index)
                    .map_or("?".to_string(), |e| hex16(&e.raw.raw)),
                sigs.join(" ")
            ));
        }
        out.push(format!("team: local player = {}", res.reasoning()));
        out
    }
}

// ---------------------------------------------------------------------------------------------------
// The one write
// ---------------------------------------------------------------------------------------------------

pub(crate) struct WriteOk {
    pub old: u8,
    pub readback: u8,
}

/// Write `value` into `player`+0x74. Gates, all of which must pass:
/// 1. `player` is non-null and its vtable (re-read right now, uncached) is a PlayerIns-derived vtable from RTTI;
/// 2. the target is exactly `player + CHR_TEAM` (+0x74), one byte;
/// 3. [`mem::write_u8`] confirms the page is committed and writable.
///
/// Offsets: VERIFIED on NR 1.3.3.0 by T-007a; see TECHNICAL_NOTES §4. Read-back is returned for the log.
pub(crate) fn write_team_type(
    player: usize,
    value: u8,
    vts: &PlayerVtables,
) -> Result<WriteOk, String> {
    if !is_ptr(player) {
        return Err(format!("0x{player:X} is not a plausible object pointer"));
    }
    let vt = mem::read::<usize>(player).ok_or("object vtable is unreadable")?;
    if !vts.contains(vt) {
        return Err(format!(
            "object 0x{player:X} has vtable 0x{vt:X}, which is not a PlayerIns-derived class; refusing to write"
        ));
    }
    let addr = player + CHR_TEAM;
    let old = mem::read::<u8>(addr).ok_or("team_type byte is unreadable")?;
    mem::write_u8(addr, value).map_err(|e| format!("guarded write refused: {e}"))?;
    let readback = mem::read::<u8>(addr).ok_or("team_type byte unreadable after the write")?;
    Ok(WriteOk { old, readback })
}

fn read_fields(rc: &mut RegionCache, p: usize) -> Option<Fields> {
    Some(Fields {
        id: rc.read::<u32>(p + CHR_ID)?,
        v70: rc.read::<u32>(p + CHR_TYPE70)?,
        team: rc.read::<u8>(p + CHR_TEAM)?,
    })
}

// ---------------------------------------------------------------------------------------------------
// Controller driven from the main loop
// ---------------------------------------------------------------------------------------------------

/// Persist state of one player (T-015: persist is per player).
struct PersistEntry {
    /// The value re-applied every tick.
    value: u8,
    tracker: PersistTracker,
    /// The first successful application was logged.
    first_logged: bool,
}

pub(crate) struct TeamCtl {
    base: usize,
    size: usize,
    watch_every: Duration,
    persist_every: Duration,
    epoch: Instant,
    cycler: Cycler,
    vts: PlayerVtables,
    vts_logged: bool,
    inst: usize,
    announced: bool,
    log: ResolveLog,
    watch: WatchState,
    next_watch: Instant,
    last_err: Option<String>,
    /// First observed team value per PlayerIns address (captured at the first write).
    originals: HashMap<usize, u8>,
    /// Last value we wrote per PlayerIns address (what a new persist entry for it starts with).
    last_written: HashMap<usize, u8>,
    /// Persist set: PlayerIns address -> value (T-015).
    persist: BTreeMap<usize, PersistEntry>,
    /// The HUMAN's "this is me" PlayerIns, kept until the WorldChrMan instance changes (T-015).
    chosen_local: Option<usize>,
    marker: Marker,
    next_persist: Instant,
    /// T-013: last quiet resolve for the overlay snapshot (refreshed at most every [`LIVE_REFRESH`]).
    live: LiveView,
    next_live: Instant,
}

/// What the overlay shows about the players, from a quiet (non-logging) resolve.
#[derive(Default)]
struct LiveView {
    local: Option<usize>,
    team: Option<u8>,
    /// Why there is no local player (empty when there is one).
    note: String,
    /// How the local player was decided (empty when there is none).
    basis: String,
    rows: Vec<PlayerRow>,
}

impl TeamCtl {
    pub(crate) fn new(cfg: &Config, base: usize, size: usize) -> Self {
        let cycler = Cycler::new(&cfg.team_candidates);
        let (i, n) = cycler.position();
        tracing::info!(
            "T-008: selected team candidate = {} ({i}/{n}); controls: overlay buttons, hotkeys {}",
            cycler.current(),
            cfg.describe_bindings()
        );
        let now = Instant::now();
        TeamCtl {
            base,
            size,
            watch_every: Duration::from_millis(cfg.watch_interval_ms),
            persist_every: Duration::from_millis(cfg.persist_interval_ms),
            epoch: now,
            cycler,
            vts: PlayerVtables::default(),
            vts_logged: false,
            inst: 0,
            announced: false,
            log: ResolveLog::default(),
            watch: WatchState::default(),
            next_watch: now,
            last_err: None,
            originals: HashMap::new(),
            last_written: HashMap::new(),
            persist: BTreeMap::new(),
            chosen_local: None,
            marker: Marker::default(),
            next_persist: now,
            live: LiveView::default(),
            next_live: now,
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// True while the PlayerIns vtable set is still unknown (RTTI not recovered yet).
    pub(crate) fn needs_vts(&self) -> bool {
        self.vts.is_empty()
    }

    pub(crate) fn set_vts(&mut self, rtti: &Rtti) {
        self.vts = PlayerVtables::from_rtti(rtti, self.base);
        if !self.vts_logged && !self.vts.is_empty() {
            self.vts_logged = true;
            let list: Vec<String> = self
                .vts
                .sorted()
                .iter()
                .map(|&v| va_rva(v, self.base, self.size))
                .collect();
            let known = self.vts.contains(self.base + PLAYER_VTABLE_RVA);
            tracing::info!(
                "T-008: PlayerIns-derived vtables from RTTI: {}; exe+0x{PLAYER_VTABLE_RVA:X} (T-007a) among them: {known}",
                list.join(", ")
            );
        }
    }

    /// How long the main loop may sleep: shorter while persist mode needs a fast tick.
    pub(crate) fn poll_interval(&self, default: Duration) -> Duration {
        if self.persist.is_empty() {
            default
        } else {
            default.min(self.persist_every)
        }
    }

    /// Called every tick with the current WorldChrMan instance (0 = not live).
    pub(crate) fn set_instance(&mut self, inst: usize) {
        if inst == self.inst {
            return;
        }
        self.inst = inst;
        self.announced = false;
        self.log.reset();
        self.watch.clear();
        self.last_err = None;
        self.live = LiveView::default();
        self.next_live = Instant::now();
        // PlayerIns objects are re-created with the WorldChrMan; addresses could be reused for other
        // values, so everything keyed by address is forgotten: originals, written values, persist entries
        // and the "this is me" choice.
        if !self.originals.is_empty() || !self.persist.is_empty() || self.chosen_local.is_some() {
            tracing::info!(
                "T-008: WorldChrMan instance changed; forgetting {} captured original team value(s), {} persist entr(y/ies) and the \"this is me\" choice ({})",
                self.originals.len(),
                self.persist.len(),
                self.chosen_local
                    .map_or("none".to_string(), |a| format!("0x{a:X}"))
            );
        }
        self.originals.clear();
        self.last_written.clear();
        self.persist.clear();
        self.chosen_local = None;
        self.next_watch = Instant::now();
    }

    /// Resolve and log errors only when they change (the watcher calls this every second).
    fn resolve_logged(&mut self, quiet_repeat: bool) -> Option<Resolution> {
        match resolve(self.inst, &self.vts, self.chosen_local) {
            Ok(r) => {
                self.last_err = None;
                Some(r)
            }
            Err(e) => {
                let msg = e.to_string();
                if !quiet_repeat || self.last_err.as_deref() != Some(msg.as_str()) {
                    tracing::warn!("T-008: could not resolve the players: {msg}");
                }
                self.last_err = Some(msg);
                None
            }
        }
    }

    /// Resolve for an action (`src` = trigger) and log the ChrSet evidence if it changed.
    fn resolve_for(&mut self, src: &str) -> Option<Resolution> {
        if self.inst == 0 {
            tracing::info!("T-008: {src} ignored, WorldChrMan is not live");
            return None;
        }
        if self.vts.is_empty() {
            tracing::warn!("T-008: {src} ignored, no PlayerIns vtable known (RTTI not recovered)");
            return None;
        }
        let res = self.resolve_logged(false)?;
        for l in self.log.lines(&res, self.now_ms(), (self.base, self.size)) {
            tracing::info!("T-008: {l}");
        }
        Some(res)
    }

    /// Resolve for an action on the local player; logs and returns `None` if there is none.
    fn local_or_log(&mut self, src: &str) -> Option<(Resolution, usize)> {
        let res = self.resolve_for(src)?;
        match res.local {
            Some(p) => Some((res, p)),
            None => {
                tracing::warn!(
                    "T-008: {src} ignored: no local player ({}); use a row button or \"this is me\"",
                    res.reasoning()
                );
                None
            }
        }
    }

    /// Resolve for an action on `addr`; logs and returns `None` if it is not a PlayerIns of the ChrSet.
    fn target_or_log(&mut self, addr: usize, src: &str) -> Option<(Resolution, String)> {
        let res = self.resolve_for(src)?;
        match res.who(addr) {
            Some(who) => Some((res, who)),
            None => {
                tracing::warn!(
                    "T-008: {src} ignored: 0x{addr:X} is not a PlayerIns of the current ChrSet (stale row?)"
                );
                None
            }
        }
    }

    /// The candidate the write hotkey applies.
    pub(crate) fn selected(&self) -> u8 {
        self.cycler.current()
    }

    /// Cycle hotkey (unbound by default): select the next candidate.
    pub(crate) fn cycle(&mut self) {
        let v = self.cycler.next();
        let (i, n) = self.cycler.position();
        tracing::info!("T-008: selected team candidate = {v} ({i}/{n})");
    }

    /// Overlay candidate button: select `v` and write it to the local player. A value outside the
    /// configured list is refused.
    pub(crate) fn set_and_write(&mut self, v: u8, src: &str) {
        if self.cycler.current() != v {
            if !self.cycler.select(v) {
                tracing::warn!(
                    "T-008: {src}: {v} is not in team_candidates {:?}; ignored",
                    self.cycler.list()
                );
                return;
            }
            let (i, n) = self.cycler.position();
            tracing::info!("T-008: selected team candidate = {v} ({i}/{n})");
        }
        self.apply_selected(src);
    }

    /// Write the selected candidate to the local player. `src` is the trigger ("F5", "overlay", ...).
    pub(crate) fn apply_selected(&mut self, src: &str) {
        let Some((res, local)) = self.local_or_log(src) else {
            return;
        };
        let who = res.who(local).unwrap_or_else(|| "LOCAL".to_string());
        let v = self.cycler.current();
        self.write_to(local, &who, v, src);
    }

    /// T-015 row button: write `v` to this player's local copy (does not change the selected candidate).
    pub(crate) fn set_for(&mut self, addr: usize, v: u8, src: &str) {
        let Some((_res, who)) = self.target_or_log(addr, src) else {
            return;
        };
        self.write_to(addr, &who, v, src);
    }

    /// The write itself plus its log line, original capture and persist-value follow-up. `who` is the
    /// `LOCAL` / `REMOTE[i]` label.
    fn write_to(&mut self, addr: usize, who: &str, v: u8, src: &str) {
        match write_team_type(addr, v, &self.vts) {
            Ok(w) => {
                let first = !self.originals.contains_key(&addr);
                let orig = *self.originals.entry(addr).or_insert(w.old);
                self.last_written.insert(addr, v);
                tracing::info!(
                    "T-008: {src} team write who={who} PlayerIns 0x{addr:X} +0x74: {} -> {v} (readback {}){}",
                    w.old,
                    w.readback,
                    if first {
                        format!("; captured original {orig}")
                    } else {
                        format!("; original is {orig}")
                    }
                );
                if w.readback != v {
                    tracing::warn!(
                        "T-008: who={who} readback {} != written {v}; the game changed it immediately or the store did not stick",
                        w.readback
                    );
                }
                // A player in persist mode keeps the newest value written to it.
                if let Some(p) = self.persist.get_mut(&addr) {
                    if p.value != v {
                        tracing::info!(
                            "T-008: who={who} persist value for 0x{addr:X} is now {v} (was {})",
                            p.value
                        );
                        p.value = v;
                    }
                }
            }
            Err(e) => tracing::warn!(
                "T-008: {src} write who={who} to PlayerIns 0x{addr:X} failed: {e}"
            ),
        }
    }

    /// Restore the local player's captured original.
    pub(crate) fn restore(&mut self, src: &str) {
        let Some((res, local)) = self.local_or_log(src) else {
            return;
        };
        let who = res.who(local).unwrap_or_else(|| "LOCAL".to_string());
        self.restore_at(local, &who, src);
    }

    /// T-015 row button: restore this player's captured original.
    pub(crate) fn restore_for(&mut self, addr: usize, src: &str) {
        let Some((_res, who)) = self.target_or_log(addr, src) else {
            return;
        };
        self.restore_at(addr, &who, src);
    }

    fn restore_at(&mut self, addr: usize, who: &str, src: &str) {
        let Some(&orig) = self.originals.get(&addr) else {
            tracing::info!(
                "T-008: {src} ignored: no original value captured for who={who} PlayerIns 0x{addr:X} (write a value first)"
            );
            return;
        };
        if self.persist.contains_key(&addr) {
            tracing::warn!(
                "T-008: persist is ON for who={who} and will re-apply its value right after this restore; turn persist off first"
            );
        }
        match write_team_type(addr, orig, &self.vts) {
            Ok(w) => tracing::info!(
                "T-008: {src} restore who={who} PlayerIns 0x{addr:X} +0x74: {} -> {orig} (readback {})",
                w.old,
                w.readback
            ),
            Err(e) => tracing::warn!(
                "T-008: {src} restore of who={who} PlayerIns 0x{addr:X} failed: {e}"
            ),
        }
    }

    /// Persist toggle for the local player (F7 / the global checkbox): re-applies the selected candidate.
    pub(crate) fn toggle_persist(&mut self, src: &str) {
        let Some((res, local)) = self.local_or_log(src) else {
            return;
        };
        let who = res.who(local).unwrap_or_else(|| "LOCAL".to_string());
        let on = !self.persist.contains_key(&local);
        let v = self.cycler.current();
        self.persist_at(local, &who, on, Some(v), src);
    }

    /// T-015 row checkbox: persist on/off for this player. Turning it on re-applies the last value written
    /// to it (or the selected candidate if nothing was written).
    pub(crate) fn set_persist_for(&mut self, addr: usize, on: bool, src: &str) {
        let Some((_res, who)) = self.target_or_log(addr, src) else {
            return;
        };
        self.persist_at(addr, &who, on, None, src);
    }

    fn persist_at(&mut self, addr: usize, who: &str, on: bool, value: Option<u8>, src: &str) {
        if on {
            let v = value
                .or_else(|| self.last_written.get(&addr).copied())
                .unwrap_or_else(|| self.cycler.current());
            let mut tracker = PersistTracker::new();
            tracker.reset(self.now_ms());
            self.persist.insert(
                addr,
                PersistEntry {
                    value: v,
                    tracker,
                    first_logged: false,
                },
            );
            self.next_persist = Instant::now();
            tracing::info!(
                "T-008: {src} persist ON who={who} PlayerIns 0x{addr:X}: re-applying team_type {v} every {} ms",
                self.persist_every.as_millis()
            );
        } else if let Some(p) = self.persist.remove(&addr) {
            tracing::info!(
                "T-008: {src} persist OFF who={who} PlayerIns 0x{addr:X} ({} revert(s) counted)",
                p.tracker.total_reverts()
            );
        }
    }

    /// T-015: the HUMAN says `addr` is the local player. Kept until the WorldChrMan instance changes.
    pub(crate) fn set_local_override(&mut self, addr: usize, src: &str) {
        let Some((_res, who)) = self.target_or_log(addr, src) else {
            return;
        };
        self.chosen_local = Some(addr);
        self.next_live = Instant::now();
        tracing::info!(
            "T-008: {src}: you chose PlayerIns 0x{addr:X} (was {who}) as the local player for this session"
        );
    }

    pub(crate) fn clear_local_override(&mut self, src: &str) {
        if self.chosen_local.take().is_some() {
            self.next_live = Instant::now();
            tracing::info!("T-008: {src}: cleared the \"this is me\" choice");
        }
    }

    /// T-015: `MARK #n: <text>` (`None` = hotkey). Needs no game memory.
    pub(crate) fn mark(&mut self, text: Option<&str>) {
        let line = self.marker.next_line(text);
        tracing::info!("{line}");
    }

    /// Fill the team part of the overlay snapshot (T-013, T-015). Runs on the main loop thread: the only
    /// place that reads game memory for the overlay. Read-only; refreshes the resolve at most every
    /// [`LIVE_REFRESH`].
    pub(crate) fn fill_snapshot(&mut self, s: &mut Snapshot) {
        if Instant::now() >= self.next_live {
            self.next_live = Instant::now() + LIVE_REFRESH;
            self.refresh_live();
        }
        s.vtables_known = !self.vts.is_empty();
        s.local_player = self.live.local;
        s.team_type = self.live.team;
        s.note = self.live.note.clone();
        s.local_basis = self.live.basis.clone();
        s.original = self
            .live
            .local
            .and_then(|l| self.originals.get(&l).copied());
        s.selected = self.cycler.current();
        s.candidates = self.cycler.list().to_vec();
        s.persist = self
            .live
            .local
            .is_some_and(|l| self.persist.contains_key(&l));
        s.reverts_last_sec = self
            .persist
            .values()
            .map(|p| p.tracker.reverts_last_window())
            .sum();
        s.reverts_total = self
            .persist
            .values()
            .map(|p| p.tracker.total_reverts())
            .sum();
        s.marks = self.marker.count();
        s.players = self
            .live
            .rows
            .iter()
            .map(|r| {
                let mut r = r.clone();
                r.original = self.originals.get(&r.addr).copied();
                if let Some(p) = self.persist.get(&r.addr) {
                    r.persist = Some(p.value);
                    r.reverts_last_sec = p.tracker.reverts_last_window();
                    r.reverts_total = p.tracker.total_reverts();
                }
                r
            })
            .collect();
    }

    /// Quiet resolve of the players and their live team_type (no logging; the watcher logs changes).
    fn refresh_live(&mut self) {
        self.live = LiveView::default();
        if self.inst == 0 {
            self.live.note = "WorldChrMan is not live".to_string();
            return;
        }
        if self.vts.is_empty() {
            self.live.note = "PlayerIns vtables unknown (RTTI not recovered yet)".to_string();
            return;
        }
        match resolve(self.inst, &self.vts, self.chosen_local) {
            Ok(r) => {
                // Read-only, VirtualQuery-guarded (mem::read). Offset VERIFIED on NR 1.3.3.0 by
                // T-007a; see TECHNICAL_NOTES section 4.
                self.live.rows = r.rows(self.chosen_local, &mut |a| mem::read::<u8>(a + CHR_TEAM));
                match r.local {
                    Some(l) => {
                        self.live.local = Some(l);
                        self.live.team = mem::read::<u8>(l + CHR_TEAM);
                        self.live.basis = r.basis.label();
                    }
                    None => self.live.note = r.reasoning(),
                }
            }
            Err(e) => self.live.note = e.to_string(),
        }
    }

    /// Watcher and persist, called every main-loop tick while WorldChrMan is live.
    pub(crate) fn tick(&mut self) {
        if self.inst == 0 || self.vts.is_empty() {
            return;
        }
        let now = Instant::now();
        if now >= self.next_watch {
            self.next_watch = now + self.watch_every;
            self.watch_tick();
        }
        if !self.persist.is_empty() && now >= self.next_persist {
            self.next_persist = now + self.persist_every;
            self.persist_tick();
        }
    }

    fn watch_tick(&mut self) {
        let Some(res) = self.resolve_logged(true) else {
            return;
        };
        let now_ms = self.now_ms();
        for l in self.log.lines(&res, now_ms, (self.base, self.size)) {
            tracing::info!("T-008: {l}");
        }
        let mut rc = RegionCache::new();
        for &p in &res.players {
            let Some(f) = read_fields(&mut rc, p) else {
                continue;
            };
            if let Some(prev) = self.watch.update(p, f) {
                let who = res.who(p).unwrap_or_else(|| "?".to_string());
                tracing::info!("T-008: {}", watch_line(p, &who, f, prev));
            }
        }
        if !self.announced {
            if let Some(l) = res.local {
                if let Some(f) = read_fields(&mut rc, l) {
                    self.announced = true;
                    tracing::info!(
                        "T-008: new WorldChrMan instance {}: local player 0x{l:X} team_type = {} at start",
                        va_rva(self.inst, self.base, self.size),
                        f.team
                    );
                }
            }
        }
    }

    fn persist_tick(&mut self) {
        let now_ms = self.now_ms();
        // Quiet resolve: errors are already logged (deduplicated) by the watcher.
        let Ok(res) = resolve(self.inst, &self.vts, self.chosen_local) else {
            return;
        };
        for (&addr, entry) in self.persist.iter_mut() {
            // A persist entry whose PlayerIns left the ChrSet is idle (and is dropped with the instance).
            let Some(who) = res.who(addr) else {
                continue;
            };
            if let Some(s) = entry.tracker.summary(now_ms) {
                tracing::info!("T-008: who={who} PlayerIns 0x{addr:X} {s}");
            }
            let Some(seen) = mem::read::<u8>(addr + CHR_TEAM) else {
                continue;
            };
            let want = entry.value;
            if !entry.tracker.observe(addr, want, seen) {
                continue;
            }
            match write_team_type(addr, want, &self.vts) {
                Ok(w) => {
                    self.originals.entry(addr).or_insert(w.old);
                    self.last_written.insert(addr, want);
                    if !entry.first_logged {
                        entry.first_logged = true;
                        tracing::info!(
                            "T-008: persist first apply who={who} PlayerIns 0x{addr:X} +0x74: {} -> {want} (readback {}); later re-applies are counted in the persist summary",
                            w.old,
                            w.readback
                        );
                    }
                }
                Err(e) => {
                    entry.tracker.write_failed();
                    if entry.tracker.should_log_failure(now_ms) {
                        tracing::warn!(
                            "T-008: persist write who={who} to PlayerIns 0x{addr:X} failed: {e}"
                        );
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// F8: histogram (runs on the discovery worker thread)
// ---------------------------------------------------------------------------------------------------

/// Re-run the T-007a scan on `inst` and log the histogram of ChrIns+0x74 over every ChrIns it reaches (the
/// inline ChrSet, the big arrays, direct members) plus the ChrSet's PlayerIns and the local player value.
/// Read-only.
pub(crate) fn histogram_report(rtti: &Rtti, inst: usize, base: usize) {
    let t0 = Instant::now();
    let vts = PlayerVtables::from_rtti(rtti, base);
    if rtti.chr_class_count() == 0 {
        tracing::warn!(
            "T-008: F8: no ChrIns-derived classes known (RTTI not recovered); nothing to scan"
        );
        return;
    }
    let res = discover::scan_instance(
        inst,
        discover::SCAN_LEN,
        rtti,
        Budgets::default(),
        &mut |_| {},
    );
    let mut rc = RegionCache::new();

    // (address, class name), deduplicated by address.
    let mut seen: HashSet<usize> = HashSet::new();
    let mut objs: Vec<(usize, String)> = Vec::new();
    for o in &res.objects {
        if seen.insert(o.addr) {
            objs.push((o.addr, rtti.name(o.class)));
        }
    }
    let scanned = objs.len();
    let resolved = resolve(inst, &vts, None);
    if let Ok(r) = &resolved {
        for &p in &r.players {
            if seen.insert(p) {
                let class = rc
                    .read::<usize>(p)
                    .and_then(|vt| rtti.class_of_vtable(vt))
                    .map_or("PlayerIns?".to_string(), |c| rtti.name(c));
                objs.push((p, class));
            }
        }
    }

    let mut items: Vec<(String, u8)> = Vec::new();
    let mut unreadable = 0usize;
    for (addr, class) in &objs {
        match rc.read::<u8>(addr + CHR_TEAM) {
            Some(t) => items.push((class.clone(), t)),
            None => unreadable += 1,
        }
    }
    let (all, per_class) = histogram(&items);
    tracing::info!(
        "T-008: F8 histogram of ChrIns+0x74 team_type: {} ChrIns read ({} from the T-007a scan, {} added from the ChrSet, {} unreadable) in {} ms{}",
        items.len(),
        scanned,
        objs.len() - scanned,
        unreadable,
        t0.elapsed().as_millis(),
        if res.pass1.stopped_at.is_some() || res.pass2.stopped_at.is_some() {
            " [scan time budget exhausted: partial]"
        } else {
            ""
        }
    );
    tracing::info!("T-008: F8 all: {}", fmt_counts(&all));
    for (class, m) in &per_class {
        tracing::info!("T-008: F8   {class}: {}", fmt_counts(m));
    }
    match resolved {
        Ok(r) => match r.local {
            Some(l) => tracing::info!(
                "T-008: F8 local player 0x{l:X}: team_type = {}",
                mem::read::<u8>(l + CHR_TEAM).map_or("?".to_string(), |v| v.to_string())
            ),
            None => tracing::info!(
                "T-008: F8 local player not resolved: {}",
                r.reasoning()
            ),
        },
        Err(e) => tracing::info!("T-008: F8 local player not resolved: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_bytes(ptrs: &[u64]) -> Vec<u8> {
        let mut b = Vec::new();
        for (i, p) in ptrs.iter().enumerate() {
            b.extend_from_slice(&p.to_le_bytes());
            b.extend_from_slice(&(0xAA00 + i as u64).to_le_bytes());
        }
        b
    }

    #[test]
    fn capacity_bounds() {
        assert!(!capacity_ok(0));
        assert!(capacity_ok(1));
        assert!(capacity_ok(6));
        assert!(capacity_ok(16));
        assert!(!capacity_ok(17));
        assert!(!capacity_ok(0x1000));
    }

    #[test]
    fn parses_chrset_entries_from_a_synthetic_buffer() {
        let buf = entry_bytes(&[0x7FF0_0000_1000, 0, 0x7FF0_0000_2000]);
        let e = parse_entries(&buf, 3);
        assert_eq!(e.len(), 3);
        assert_eq!((e[0].index, e[0].ptr), (0, 0x7FF0_0000_1000));
        assert_eq!(e[1].ptr, 0);
        assert_eq!(e[2].ptr, 0x7FF0_0000_2000);
        // The second qword is kept verbatim in `raw`.
        assert_eq!(&e[2].raw[8..], &0xAA02u64.to_le_bytes());
        assert_eq!(
            hex16(&e[0].raw),
            "00 10 00 00 F0 7F 00 00 00 AA 00 00 00 00 00 00"
        );
        // A buffer shorter than capacity yields only the complete entries; a partial entry is dropped.
        assert_eq!(parse_entries(&buf[..40], 6).len(), 2);
        assert!(parse_entries(&[], 6).is_empty());
        assert_eq!(parse_entries(&buf, 0).len(), 0);
        // Capacity smaller than the buffer ignores the rest.
        assert_eq!(parse_entries(&buf, 2).len(), 2);
    }

    #[test]
    fn candidate_cycling_wraps_and_reports_position() {
        let mut c = Cycler::new(&[6, 29, 3]);
        assert_eq!((c.current(), c.position()), (6, (1, 3)));
        assert_eq!(c.next(), 29);
        assert_eq!(c.position(), (2, 3));
        assert_eq!(c.next(), 3);
        assert_eq!(c.next(), 6);
        assert_eq!(c.position(), (1, 3));
        // Single-element list stays put; empty list falls back to the defaults.
        let mut one = Cycler::new(&[13]);
        assert_eq!(one.next(), 13);
        let d = Cycler::new(&[]);
        assert_eq!(d.current(), 6);
        assert_eq!(d.position().1, 7);
    }

    #[test]
    fn persist_counts_reverts_not_the_initial_application() {
        let mut t = PersistTracker::new();
        t.reset(0);
        let me = 0x1000;
        // First tick: the value is still the original (30). We rewrite it; that is not a revert.
        assert!(t.observe(me, 6, 30));
        assert_eq!(t.total_reverts(), 0);
        // Stays at 6: nothing to do.
        assert!(!t.observe(me, 6, 6));
        assert!(!t.observe(me, 6, 6));
        // The game flips it back: counted, rewritten.
        assert!(t.observe(me, 6, 30));
        assert_eq!(t.total_reverts(), 1);
        assert!(!t.observe(me, 6, 6));
        assert!(t.observe(me, 6, 26));
        assert_eq!(t.total_reverts(), 2);
        // Summary only after a full second and only if something was reverted.
        assert_eq!(t.summary(500), None);
        assert_eq!(
            t.summary(1000).as_deref(),
            Some("persist: 2 reverts in last 1 s, last seen value 26")
        );
        // Window restarts: quiet second -> no line.
        assert_eq!(t.summary(1500), None);
        assert_eq!(t.summary(2000), None);
        assert!(!t.observe(me, 6, 6));
        assert!(t.observe(me, 6, 0));
        assert_eq!(t.summary(2500), None); // < 1 s since the last window start (2000)
        assert_eq!(
            t.summary(3000).as_deref(),
            Some("persist: 1 reverts in last 1 s, last seen value 0")
        );
        assert_eq!(t.total_reverts(), 3);
    }

    #[test]
    fn persist_rearms_on_new_target_or_selection_and_after_write_failure() {
        let mut t = PersistTracker::new();
        t.reset(0);
        assert!(!t.observe(0x1000, 6, 6)); // already matches: armed
                                           // Selection changes (F4): the mismatch is our own re-application, not a revert.
        assert!(t.observe(0x1000, 29, 6));
        assert_eq!(t.total_reverts(), 0);
        // New PlayerIns (area change): same.
        assert!(!t.observe(0x1000, 29, 29));
        assert!(t.observe(0x2000, 29, 30));
        assert_eq!(t.total_reverts(), 0);
        // A failed write must not make the next tick look like a game revert.
        assert!(!t.observe(0x2000, 29, 29));
        assert!(t.observe(0x2000, 29, 1)); // counted: 1
        t.write_failed();
        assert!(t.observe(0x2000, 29, 1)); // not counted again
        assert_eq!(t.total_reverts(), 1);
    }

    #[test]
    fn persist_failure_log_is_rate_limited() {
        let mut t = PersistTracker::new();
        assert!(t.should_log_failure(10));
        assert!(!t.should_log_failure(500));
        assert!(t.should_log_failure(1010));
    }

    #[test]
    fn watcher_reports_only_changes() {
        let mut w = WatchState::default();
        let a = Fields {
            id: 1,
            v70: 5,
            team: 30,
        };
        let b = Fields {
            id: 1,
            v70: 5,
            team: 6,
        };
        assert_eq!(w.update(0x10, a), Some(None)); // new
        assert_eq!(w.update(0x10, a), None);
        assert_eq!(w.update(0x10, b), Some(Some(a)));
        assert_eq!(w.update(0x20, a), Some(None)); // other address is independent
        w.clear();
        assert_eq!(w.update(0x10, b), Some(None));
        let line = watch_line(0x10, "REMOTE[2]", b, Some(a));
        assert!(
            line.contains("who=REMOTE[2]") && line.contains("team_type=6") && line.contains("was id=1")
        );
        assert!(!watch_line(0x10, "LOCAL", b, None).contains("was"));
    }

    #[test]
    fn histogram_counts_and_formats() {
        let items = vec![
            ("CS::EnemyIns".to_string(), 6u8),
            ("CS::EnemyIns".to_string(), 6),
            ("CS::EnemyIns".to_string(), 26),
            ("CS::PlayerIns".to_string(), 30),
            ("CS::PlayerIns".to_string(), 30),
        ];
        let (all, per) = histogram(&items);
        assert_eq!(fmt_counts(&all), "6 x2, 30 x2, 26 x1"); // count desc, then value asc
        assert_eq!(fmt_counts(&per["CS::EnemyIns"]), "6 x2, 26 x1");
        assert_eq!(fmt_counts(&per["CS::PlayerIns"]), "30 x2");
        assert_eq!(fmt_counts(&BTreeMap::new()), "");
    }

    // ---- T-015: the local-player decision on synthetic multi-player data ----

    /// A probe with the given decisive signal values (names are positional stand-ins).
    fn probe(addr: usize, index: usize, vals: &[u64]) -> PlayerProbe {
        const NAMES: [&str; 4] = ["sigA", "sigB", "sigC", "sigD"];
        PlayerProbe {
            addr,
            index,
            signals: vals
                .iter()
                .enumerate()
                .map(|(i, &v)| Signal {
                    name: NAMES[i],
                    value: Some(v),
                    hex: false,
                    decisive: true,
                })
                .collect(),
        }
    }

    #[test]
    fn odd_one_out_needs_three_players_and_an_exact_one_vs_rest_split() {
        let v = |a: &[(usize, u64)]| a.iter().map(|&(x, y)| (x, Some(y))).collect::<Vec<_>>();
        // Exactly one differs, the rest agree.
        assert_eq!(odd_one_out(&v(&[(1, 5), (2, 5), (3, 9)])), Some(3));
        assert_eq!(odd_one_out(&v(&[(1, 9), (2, 5), (3, 5), (4, 5)])), Some(1));
        // Two players: cannot tell which is the odd one.
        assert_eq!(odd_one_out(&v(&[(1, 5), (2, 9)])), None);
        // All equal, all distinct, or a 2-vs-2 split: no single odd one.
        assert_eq!(odd_one_out(&v(&[(1, 5), (2, 5), (3, 5)])), None);
        assert_eq!(odd_one_out(&v(&[(1, 1), (2, 2), (3, 3)])), None);
        assert_eq!(odd_one_out(&v(&[(1, 5), (2, 5), (3, 9), (4, 9)])), None);
        // Any unreadable value disables the signal.
        assert_eq!(
            odd_one_out(&[(1, Some(5)), (2, None), (3, Some(9))]),
            None
        );
        assert_eq!(odd_one_out(&[]), None);
    }

    #[test]
    fn local_decision_with_zero_one_and_two_players() {
        let d = decide_local(&[], None);
        assert_eq!((d.local, &d.basis), (None, &Basis::NoPlayers));

        // Exactly one PlayerIns is LOCAL, whatever its signals say.
        let d = decide_local(&[probe(0x100, 3, &[1, 2])], None);
        assert_eq!((d.local, &d.basis), (Some(0x100), &Basis::OnlyPlayer));

        // Two players: UNKNOWN, even if a signal differs; the HUMAN's choice resolves it.
        let two = [probe(0x100, 0, &[1]), probe(0x200, 1, &[2])];
        let d = decide_local(&two, None);
        assert_eq!(d.local, None);
        assert!(matches!(d.basis, Basis::Unknown(ref w) if w.contains("this is me")));
        let d = decide_local(&two, Some(0x200));
        assert_eq!((d.local, &d.basis), (Some(0x200), &Basis::Chosen));
    }

    #[test]
    fn local_decision_suggests_the_odd_one_out_of_three_or_more() {
        // sigA splits C from A and B; sigB is the same everywhere.
        let ps = [
            probe(0xA, 0, &[5, 7]),
            probe(0xB, 1, &[5, 7]),
            probe(0xC, 2, &[9, 7]),
        ];
        let d = decide_local(&ps, None);
        assert_eq!(d.local, Some(0xC));
        assert_eq!(d.basis, Basis::Heuristic("sigA".to_string()));
        assert_eq!(d.basis.label(), "heuristic: sigA");

        // Two signals pointing at the same player are both named.
        let ps = [
            probe(0xA, 0, &[5, 1]),
            probe(0xB, 1, &[5, 1]),
            probe(0xC, 2, &[9, 0]),
        ];
        let d = decide_local(&ps, None);
        assert_eq!(d.local, Some(0xC));
        assert_eq!(d.basis, Basis::Heuristic("sigA, sigB".to_string()));

        // Signals that point at different players disagree: no suggestion.
        let ps = [
            probe(0xA, 0, &[9, 1]),
            probe(0xB, 1, &[5, 0]),
            probe(0xC, 2, &[5, 1]),
        ];
        let d = decide_local(&ps, None);
        assert_eq!(d.local, None);
        assert!(matches!(d.basis, Basis::Unknown(ref w) if w.contains("disagree")));

        // No signal splits one-vs-rest (all distinct / all equal): UNKNOWN.
        let ps = [
            probe(0xA, 0, &[1, 4]),
            probe(0xB, 1, &[2, 4]),
            probe(0xC, 2, &[3, 4]),
        ];
        let d = decide_local(&ps, None);
        assert_eq!(d.local, None);
        assert!(matches!(d.basis, Basis::Unknown(ref w) if w.contains("no candidate signal")));
        // The HUMAN's choice beats any suggestion.
        let ps = [
            probe(0xA, 0, &[5]),
            probe(0xB, 1, &[5]),
            probe(0xC, 2, &[9]),
        ];
        let d = decide_local(&ps, Some(0xA));
        assert_eq!((d.local, &d.basis), (Some(0xA), &Basis::Chosen));
        // A stale choice (not one of the players) is ignored.
        let d = decide_local(&ps, Some(0xDEAD));
        assert_eq!(d.local, Some(0xC));
        // Log-only signals never decide.
        let mut ps = [
            probe(0xA, 0, &[5]),
            probe(0xB, 1, &[5]),
            probe(0xC, 2, &[9]),
        ];
        for p in &mut ps {
            p.signals[0].decisive = false;
        }
        assert_eq!(decide_local(&ps, None).local, None);
    }

    #[test]
    fn who_labels() {
        assert_eq!(who_label(Role::Local, 2), "LOCAL");
        assert_eq!(who_label(Role::Remote, 3), "REMOTE[3]");
        assert_eq!(who_label(Role::Unknown, 0), "UNKNOWN[0]");
    }

    // ---- resolve() and the gated write on fake memory ----

    const VT_PLAYER: usize = 0x7FF6_0000_0000;
    const VT_ENEMY: usize = 0x7FF6_0000_1000;

    /// A fake object: 0x800 bytes (so every candidate offset is inside it), first qword = vtable, byte
    /// 0x74 = team.
    fn fake_obj(vt: usize, team: u8) -> Vec<u8> {
        let mut v = vec![0u8; 0x800];
        v[..8].copy_from_slice(&(vt as u64).to_le_bytes());
        v[CHR_TEAM] = team;
        v[CHR_ID..CHR_ID + 4].copy_from_slice(&1234u32.to_le_bytes());
        v
    }

    fn vts() -> PlayerVtables {
        PlayerVtables::from_set([VT_PLAYER].into_iter().collect())
    }

    /// Entries buffer with explicit state bytes (+8, +9), padded so reads stay inside the allocation.
    fn entries_with_state(ptrs: &[u64], state: &[(u8, u8)]) -> Vec<u8> {
        let mut b = Vec::new();
        for (i, p) in ptrs.iter().enumerate() {
            let (s8, s9) = state.get(i).copied().unwrap_or((4, 4));
            b.extend_from_slice(&p.to_le_bytes());
            b.extend_from_slice(&[s8, s9, 0, 0, 0, 0, 0, 0]);
        }
        b.resize(0x100, 0);
        b
    }

    /// A fake WorldChrMan big enough for +0x9A40 with a ChrSet of the given capacity. The entries buffer
    /// is returned too and must stay alive.
    fn fake_wcm(capacity: u32, entries: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let mut w = vec![0u8; 0x1D000];
        w[WCM_CHRSET + CHRSET_CAPACITY..WCM_CHRSET + CHRSET_CAPACITY + 4]
            .copy_from_slice(&capacity.to_le_bytes());
        let e = entries.to_vec();
        w[WCM_CHRSET + CHRSET_ENTRIES..WCM_CHRSET + CHRSET_ENTRIES + 8]
            .copy_from_slice(&(e.as_ptr() as u64).to_le_bytes());
        (w, e)
    }

    #[test]
    fn resolve_finds_the_players_among_entries() {
        let player = fake_obj(VT_PLAYER, 30);
        let enemy = fake_obj(VT_ENEMY, 6);
        let pa = player.as_ptr() as u64;
        let entries = entries_with_state(&[0, pa, enemy.as_ptr() as u64, 0, 0, 0], &[]);
        let (wcm, _e) = fake_wcm(6, &entries);
        let r = resolve(wcm.as_ptr() as usize, &vts(), None).expect("resolves");
        assert_eq!(r.capacity, 6);
        assert_eq!(r.entries.len(), 6);
        let kinds: Vec<EntryKind> = r.entries.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                EntryKind::Empty,
                EntryKind::Player,
                EntryKind::Other,
                EntryKind::Empty,
                EntryKind::Empty,
                EntryKind::Empty
            ]
        );
        assert_eq!(r.players, vec![pa as usize]);
        // The only PlayerIns is LOCAL; its ChrSet entry index is 1.
        assert_eq!(r.local, Some(pa as usize));
        assert_eq!(r.basis, Basis::OnlyPlayer);
        assert_eq!(r.who(pa as usize).as_deref(), Some("LOCAL"));
        assert_eq!(r.who(enemy.as_ptr() as usize), None);

        // The log prints every raw entry and the evidence for the player the first time, nothing when
        // unchanged.
        let mut lg = ResolveLog::default();
        let lines = lg.lines(&r, 0, (0, 0));
        assert_eq!(lines.iter().filter(|l| l.contains("entry[")).count(), 6 + 1);
        let ev: Vec<&String> = lines.iter().filter(|l| l.contains("evidence")).collect();
        assert_eq!(ev.len(), 1);
        assert!(ev[0].contains("UNVERIFIED") && ev[0].contains("net_authority@+0x1EC"));
        assert!(ev[0].contains("04 04"), "the raw entry bytes are in the evidence");
        assert!(lines.iter().any(|l| l.contains("local player = ")));
        assert!(
            !lines.iter().any(|l| l.contains("1C820")),
            "the old +0x1C820 rule and its log lines are gone"
        );
        assert!(lg.lines(&r, 10, (0, 0)).is_empty());
    }

    #[test]
    fn resolve_with_several_players_suggests_chooses_and_labels_rows() {
        // Three PlayerIns; the third has a different chr_type (+0x70), the others share one.
        let mut a = fake_obj(VT_PLAYER, 2);
        let mut b = fake_obj(VT_PLAYER, 2);
        let mut c = fake_obj(VT_PLAYER, 2);
        for o in [&mut a, &mut b] {
            o[CHR_TYPE70..CHR_TYPE70 + 4].copy_from_slice(&5u32.to_le_bytes());
        }
        c[CHR_TYPE70..CHR_TYPE70 + 4].copy_from_slice(&13u32.to_le_bytes());
        let (pa, pb, pc) = (
            a.as_ptr() as usize,
            b.as_ptr() as usize,
            c.as_ptr() as usize,
        );
        // Entry 0 is empty so the ChrSet indices are 1, 2, 3.
        let entries = entries_with_state(&[0, pa as u64, pb as u64, pc as u64], &[]);
        let (wcm, _e) = fake_wcm(4, &entries);
        let w = wcm.as_ptr() as usize;

        let r = resolve(w, &vts(), None).expect("resolves");
        assert_eq!(r.players, vec![pa, pb, pc]);
        assert_eq!(r.local, Some(pc), "the odd one out is suggested");
        assert_eq!(
            r.basis,
            Basis::Heuristic("chr_type@+0x70[ER +0x68]".to_string())
        );
        assert_eq!(r.who(pc).as_deref(), Some("LOCAL"));
        assert_eq!(r.who(pa).as_deref(), Some("REMOTE[1]"));
        assert_eq!(r.who(pb).as_deref(), Some("REMOTE[2]"));
        assert_eq!(r.role_of(pa), Role::Remote);

        // The HUMAN overrides the suggestion.
        let r = resolve(w, &vts(), Some(pa)).expect("resolves");
        assert_eq!(r.local, Some(pa));
        assert_eq!(r.basis, Basis::Chosen);
        assert_eq!(r.who(pc).as_deref(), Some("REMOTE[3]"));
        let rows = r.rows(Some(pa), &mut |addr| {
            Some(unsafe { *((addr + CHR_TEAM) as *const u8) })
        });
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter().map(|x| (x.index, x.role)).collect::<Vec<_>>(),
            vec![(1, Role::Local), (2, Role::Remote), (3, Role::Remote)]
        );
        assert!(rows[0].chosen && !rows[1].chosen);
        assert_eq!((rows[0].state8, rows[0].state9, rows[0].team), (4, 4, Some(2)));

        // When nothing splits them the players are UNKNOWN, not guessed.
        let mut d = fake_obj(VT_PLAYER, 2);
        d[CHR_TYPE70] = 5;
        let (pa2, pb2, pd) = (pa, pb, d.as_ptr() as usize);
        // a and b equal (chr_type 5), d also 5: all equal on every signal.
        let entries = entries_with_state(&[pa2 as u64, pb2 as u64, pd as u64], &[]);
        let (wcm2, _e2) = fake_wcm(3, &entries);
        let r = resolve(wcm2.as_ptr() as usize, &vts(), None).expect("resolves");
        assert_eq!(r.local, None);
        assert!(matches!(r.basis, Basis::Unknown(_)));
        assert_eq!(r.who(pa2).as_deref(), Some("UNKNOWN[0]"));
        assert_eq!(r.role_of(pb2), Role::Unknown);
    }

    #[test]
    fn resolve_refuses_bad_capacity_and_pointer() {
        let (wcm, _e) = fake_wcm(0, &[0u8; 0x100]);
        assert_eq!(
            resolve(wcm.as_ptr() as usize, &vts(), None).err(),
            Some(ResolveError::BadCapacity(0))
        );
        let (wcm, _e) = fake_wcm(17, &[0u8; 0x100]);
        assert_eq!(
            resolve(wcm.as_ptr() as usize, &vts(), None).err(),
            Some(ResolveError::BadCapacity(17))
        );
        let mut wcm = vec![0u8; 0x1D000];
        wcm[WCM_CHRSET + CHRSET_CAPACITY] = 6; // entries pointer left null
        assert_eq!(
            resolve(wcm.as_ptr() as usize, &vts(), None).err(),
            Some(ResolveError::BadEntriesPtr(0))
        );
        assert_eq!(
            resolve(0, &vts(), None).err(),
            Some(ResolveError::HeaderUnreadable)
        );
    }

    #[test]
    fn resolve_log_reports_changes_and_coalesces_bursts() {
        let player = fake_obj(VT_PLAYER, 30);
        let pa = player.as_ptr() as u64;
        let entries = entries_with_state(&[pa, 0], &[]);
        let (wcm, _e) = fake_wcm(2, &entries);
        let mut lg = ResolveLog::default();
        let r1 = resolve(wcm.as_ptr() as usize, &vts(), None).expect("ok");
        assert!(!lg.lines(&r1, 0, (0, 0)).is_empty());
        assert!(lg.lines(&r1, 1000, (0, 0)).is_empty());
        // The entry state byte +9 blinks 04 -> 02 (seen in TC-004): within the gap it is coalesced...
        let mut r2 = resolve(wcm.as_ptr() as usize, &vts(), None).expect("ok");
        r2.entries[0].raw.raw[9] = 0x02;
        assert!(lg.lines(&r2, 2000, (0, 0)).is_empty());
        // ...and reported (with the count) once the gap has passed.
        let lines = lg.lines(&r2, 6000, (0, 0));
        assert!(lines[0].contains("CHANGED") && lines[0].contains("1 earlier change(s) coalesced"));
        assert!(lg.lines(&r2, 7000, (0, 0)).is_empty());
    }

    #[test]
    fn write_is_gated_on_the_playerins_vtable() {
        let mut player = fake_obj(VT_PLAYER, 30);
        let a = player.as_mut_ptr() as usize;
        let w = write_team_type(a, 6, &vts()).expect("writes");
        assert_eq!((w.old, w.readback), (30, 6));
        assert_eq!(player[CHR_TEAM], 6);
        // Only that one byte changed.
        let mut expect = fake_obj(VT_PLAYER, 6);
        assert_eq!(player, expect);
        expect[CHR_TEAM] = 30;

        // Wrong class (EnemyIns): refused, memory untouched.
        let mut enemy = fake_obj(VT_ENEMY, 6);
        let e = write_team_type(enemy.as_mut_ptr() as usize, 29, &vts());
        assert!(e.is_err());
        assert_eq!(enemy[CHR_TEAM], 6);
        // Empty vtable set (RTTI not recovered): everything refused.
        let none = PlayerVtables::default();
        assert!(write_team_type(a, 1, &none).is_err());
        assert_eq!(player[CHR_TEAM], 6);
        // Null / garbage / unmapped pointers.
        assert!(write_team_type(0, 1, &vts()).is_err());
        assert!(write_team_type(0x10, 1, &vts()).is_err());
        assert!(write_team_type(0x7FFF_FFFF_0000, 1, &vts()).is_err());
    }

    // ---- T-013: selection by value, revert window, and the overlay snapshot ----

    #[test]
    fn select_by_value_only_accepts_candidates() {
        let mut c = Cycler::new(&[6, 29, 3]);
        assert!(c.select(3));
        assert_eq!((c.current(), c.position()), (3, (3, 3)));
        assert!(!c.select(99));
        assert_eq!(c.current(), 3, "a refused select changes nothing");
        assert!(c.select(6));
        assert_eq!(c.position(), (1, 3));
        assert_eq!(c.list(), &[6, 29, 3]);
    }

    #[test]
    fn reverts_last_window_reports_the_last_completed_second() {
        let mut t = PersistTracker::new();
        t.reset(0);
        assert_eq!(t.reverts_last_window(), 0);
        assert!(!t.observe(0x1000, 6, 6)); // armed
        assert!(t.observe(0x1000, 6, 30)); // revert 1
        assert!(!t.observe(0x1000, 6, 6));
        assert!(t.observe(0x1000, 6, 30)); // revert 2
                                           // Nothing is reported until a full second has passed.
        assert_eq!(t.reverts_last_window(), 0);
        assert!(t.summary(1000).is_some());
        assert_eq!(t.reverts_last_window(), 2);
        // A quiet second resets it to 0.
        assert!(t.summary(2000).is_none());
        assert_eq!(t.reverts_last_window(), 0);
        assert_eq!(t.total_reverts(), 2);
    }

    // ---- T-015: per-player writes, per-player persist, MARK, "this is me" on fake memory ----

    /// Three fake PlayerIns (team 2) in entries 1..=3 of a 4-slot ChrSet; the third has a different
    /// chr_type so it is suggested as LOCAL.
    struct Fake {
        objs: Vec<Vec<u8>>,
        wcm: Vec<u8>,
        _entries: Vec<u8>,
    }

    impl Fake {
        fn new() -> Fake {
            let mut objs: Vec<Vec<u8>> = (0..3).map(|_| fake_obj(VT_PLAYER, 2)).collect();
            for (i, o) in objs.iter_mut().enumerate() {
                let t: u32 = if i == 2 { 13 } else { 5 };
                o[CHR_TYPE70..CHR_TYPE70 + 4].copy_from_slice(&t.to_le_bytes());
            }
            let ptrs: Vec<u64> = std::iter::once(0)
                .chain(objs.iter().map(|o| o.as_ptr() as u64))
                .collect();
            let entries = entries_with_state(&ptrs, &[]);
            let (wcm, e) = fake_wcm(4, &entries);
            Fake {
                objs,
                wcm,
                _entries: e,
            }
        }

        fn addr(&self, i: usize) -> usize {
            self.objs[i].as_ptr() as usize
        }

        fn team(&self, i: usize) -> u8 {
            self.objs[i][CHR_TEAM]
        }

        fn ctl(&self) -> TeamCtl {
            let mut ctl = TeamCtl::new(&Config::default(), 0, 0);
            ctl.set_instance(self.wcm.as_ptr() as usize);
            ctl.vts = vts();
            ctl
        }
    }

    #[test]
    fn writes_go_to_any_valid_playerins_and_are_gated() {
        let f = Fake::new();
        let mut ctl = f.ctl();
        let (a, b, c) = (f.addr(0), f.addr(1), f.addr(2));

        // A remote row button writes only that player's byte; the selected candidate is untouched.
        ctl.set_for(a, 29, "overlay");
        assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 2, 2));
        assert_eq!(ctl.selected(), 6);
        assert_eq!(ctl.originals.get(&a), Some(&2));
        // The global candidate button targets the local player (the suggested third one).
        ctl.set_and_write(3, "overlay");
        assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 2, 3));
        assert_eq!(ctl.originals.get(&c), Some(&2));
        // Restore per player.
        ctl.restore_for(a, "overlay");
        assert_eq!(f.team(0), 2);
        ctl.restore("overlay");
        assert_eq!(f.team(2), 2);
        // Restoring a player that was never written changes nothing.
        ctl.restore_for(b, "overlay");
        assert_eq!(f.team(1), 2);

        // A stale / foreign address is refused and nothing is written.
        let mut other = fake_obj(VT_PLAYER, 7);
        ctl.set_for(other.as_mut_ptr() as usize, 29, "overlay");
        assert_eq!(other[CHR_TEAM], 7);
        ctl.set_for(0, 29, "overlay");
        ctl.set_for(0x10, 29, "overlay");
        // Without vtables nothing is written either.
        ctl.vts = PlayerVtables::default();
        ctl.set_for(b, 29, "overlay");
        assert_eq!(f.team(1), 2);
    }

    #[test]
    fn this_is_me_is_kept_until_the_worldchrman_changes() {
        let f = Fake::new();
        let mut ctl = f.ctl();
        let (a, c) = (f.addr(0), f.addr(2));
        let mut snap = Snapshot::default();
        ctl.fill_snapshot(&mut snap);
        assert_eq!(snap.local_player, Some(c));
        assert!(snap.local_basis.starts_with("heuristic: "), "{}", snap.local_basis);
        assert_eq!(snap.players.len(), 3);
        assert_eq!(
            snap.players.iter().map(|p| p.role).collect::<Vec<_>>(),
            vec![Role::Remote, Role::Remote, Role::Local]
        );

        ctl.set_local_override(a, "overlay");
        ctl.next_live = Instant::now();
        ctl.fill_snapshot(&mut snap);
        assert_eq!(snap.local_player, Some(a));
        assert_eq!(snap.local_basis, Basis::Chosen.label());
        assert!(snap.players[0].chosen && snap.players[0].role == Role::Local);
        // The global buttons now write to the chosen player.
        ctl.set_and_write(29, "overlay");
        assert_eq!((f.team(0), f.team(2)), (29, 2));

        // An address that is not a player is not accepted as "me".
        ctl.set_local_override(0x1234, "overlay");
        assert_eq!(ctl.chosen_local, Some(a));
        ctl.clear_local_override("overlay");
        assert_eq!(ctl.chosen_local, None);
        ctl.set_local_override(a, "overlay");
        // A new WorldChrMan instance clears the choice (and everything else keyed by address).
        ctl.set_instance(f.wcm.as_ptr() as usize + 8);
        assert_eq!(ctl.chosen_local, None);
        assert!(ctl.originals.is_empty());
    }

    #[test]
    fn persist_is_per_player() {
        let f = Fake::new();
        let mut ctl = f.ctl();
        let (a, b, c) = (f.addr(0), f.addr(1), f.addr(2));

        // Persist on for a remote row: starts with the selected candidate (nothing written yet).
        ctl.set_persist_for(a, true, "overlay");
        assert_eq!(ctl.persist.get(&a).map(|p| p.value), Some(6));
        // Writing a value to a persisted player updates its persist value.
        ctl.set_for(a, 29, "overlay");
        assert_eq!(ctl.persist.get(&a).map(|p| p.value), Some(29));
        // The local (global) toggle persists the selected candidate for the local player only.
        ctl.toggle_persist("F7");
        assert_eq!(ctl.persist.get(&c).map(|p| p.value), Some(6));
        assert_eq!(ctl.persist.len(), 2);
        assert!(!ctl.persist.contains_key(&b));
        assert_eq!(ctl.poll_interval(Duration::from_millis(500)), ctl.persist_every);

        // A tick applies each player's own value to that player alone.
        f_set(&f, 0, 2); // the game "reverts" a
        ctl.persist_tick();
        assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 2, 6));
        // The game reverts c: counted for c only, re-applied.
        f_set(&f, 2, 2);
        ctl.persist_tick();
        assert_eq!(f.team(2), 6);
        assert_eq!(ctl.persist[&c].tracker.total_reverts(), 1);
        assert_eq!(ctl.persist[&a].tracker.total_reverts(), 0, "a: only its first application so far");
        let mut snap = Snapshot::default();
        ctl.fill_snapshot(&mut snap);
        let by_addr = |x: usize| snap.players.iter().find(|p| p.addr == x).cloned().expect("row");
        assert_eq!(by_addr(a).persist, Some(29));
        assert_eq!(by_addr(b).persist, None);
        assert_eq!(by_addr(c).persist, Some(6));
        assert!(snap.persist, "the global checkbox shows the local player's persist");
        assert_eq!(snap.reverts_total, 1);

        // Turning one off leaves the other running.
        ctl.set_persist_for(a, false, "overlay");
        assert_eq!(ctl.persist.len(), 1);
        f_set(&f, 0, 2);
        ctl.persist_tick();
        assert_eq!(f.team(0), 2, "a is no longer persisted");
        // A new WorldChrMan instance drops all persist entries.
        ctl.set_instance(0);
        assert!(ctl.persist.is_empty());
        assert_eq!(ctl.poll_interval(Duration::from_millis(500)), Duration::from_millis(500));
    }

    /// Set a fake player's team byte directly (the "game" changing it).
    fn f_set(f: &Fake, i: usize, v: u8) {
        let p = f.addr(i) + CHR_TEAM;
        // SAFETY: test-only; `p` is inside a Vec owned by `f` that outlives this call.
        unsafe { *(p as *mut u8) = v };
    }

    #[test]
    fn mark_counts_across_sources_and_instance_changes() {
        let f = Fake::new();
        let mut ctl = f.ctl();
        assert_eq!(ctl.marker.count(), 0);
        ctl.mark(Some("A hits B"));
        ctl.mark(None);
        ctl.set_instance(0); // the numbering survives a WorldChrMan change
        ctl.mark(Some(""));
        let mut snap = Snapshot::default();
        ctl.fill_snapshot(&mut snap);
        assert_eq!(snap.marks, 3);
    }

    #[test]
    fn snapshot_and_commands_on_fake_memory() {
        let mut player = fake_obj(VT_PLAYER, 2);
        let pa = player.as_mut_ptr() as u64;
        let entries = entries_with_state(&[pa, 0], &[]);
        let (wcm, _e) = fake_wcm(2, &entries);

        let cfg = Config::default();
        let mut ctl = TeamCtl::new(&cfg, 0, 0);
        let mut snap = Snapshot::default();

        // WorldChrMan not live / no vtables: nothing resolved, and the note says why.
        ctl.fill_snapshot(&mut snap);
        assert_eq!((snap.local_player, snap.team_type), (None, None));
        assert!(!snap.vtables_known);
        assert!(snap.note.contains("not live"));
        assert!(snap.players.is_empty());
        assert_eq!(snap.selected, 6);
        assert_eq!(snap.candidates, crate::config::DEFAULT_CANDIDATES.to_vec());

        ctl.set_instance(wcm.as_ptr() as usize);
        ctl.fill_snapshot(&mut snap); // still no vtables
        assert!(snap.note.contains("vtables"));
        ctl.vts = vts();
        ctl.next_live = Instant::now();
        ctl.fill_snapshot(&mut snap);
        assert!(snap.vtables_known);
        assert_eq!(snap.local_player, Some(pa as usize));
        assert_eq!(snap.team_type, Some(2));
        assert_eq!(snap.original, None);
        assert!(snap.note.is_empty());
        assert_eq!(snap.players.len(), 1, "a lone PlayerIns is one LOCAL row");
        assert_eq!(snap.players[0].role, Role::Local);
        assert_eq!(snap.local_basis, Basis::OnlyPlayer.label());

        // Overlay button: select 29 and write it. The byte changes, the original is captured.
        ctl.set_and_write(29, "overlay");
        assert_eq!(player[CHR_TEAM], 29);
        assert_eq!(ctl.selected(), 29);
        ctl.next_live = Instant::now();
        ctl.fill_snapshot(&mut snap);
        assert_eq!(
            (snap.team_type, snap.original, snap.selected),
            (Some(29), Some(2), 29)
        );
        assert_eq!(snap.players[0].original, Some(2));

        // A value that is not a candidate is refused and nothing is written.
        ctl.set_and_write(99, "overlay");
        assert_eq!((player[CHR_TEAM], ctl.selected()), (29, 29));

        // Persist toggle shows up in the snapshot; the revert window is only reported while persisting.
        ctl.toggle_persist("overlay");
        ctl.fill_snapshot(&mut snap);
        assert!(snap.persist);
        ctl.toggle_persist("overlay");
        ctl.fill_snapshot(&mut snap);
        assert!(!snap.persist);
        assert_eq!(snap.reverts_last_sec, 0);

        // Restore puts the captured original back.
        ctl.restore("overlay");
        assert_eq!(player[CHR_TEAM], 2);

        // A new WorldChrMan instance forgets the original and the live view.
        ctl.set_instance(0);
        ctl.fill_snapshot(&mut snap);
        assert_eq!((snap.local_player, snap.original), (None, None));
    }
}
