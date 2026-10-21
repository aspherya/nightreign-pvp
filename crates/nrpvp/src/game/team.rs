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
//! T-015 (FS-1 tooling): every valid PlayerIns can be read and written, local or remote. T-020: the LOCAL one is
//! the first valid PlayerIns in ChrSet entry order ([`decide_local`]), VERIFIED on every client in FS-0 and FS-1
//! (TECHNICAL_NOTES §4); the other signals below are logged as evidence only.
//!
//! T-020 PvPvE mode: every `pvpve_interval_ms`, every ChrSet player whose team_type is the expedition default (2)
//! gets `pvpve_team` written, using the same gated 1-byte write as the hotkeys. Nothing else is ever overwritten.
//!
//! The write (hotkeys, overlay, persist) is the only memory write in the project. It is a single u8 store,
//! only at `PlayerIns`+0x74, only after the object's vtable was re-read and matched a PlayerIns-derived
//! class from RTTI ([`write_team_type`]). No hooks.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use super::mem::{self, is_ptr, va_rva, RegionCache};
use super::poswrite::{self, PosTest, RealMem};
use super::rtti::Rtti;
use crate::config::{Config, Scheme};
use crate::mark::Marker;
use crate::overlay::shared::{PlayerRow, PvpveState, Relation, Role, Snapshot};
use crate::rules::teams::{assign_teams, PlayerId};

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
// ---- Local/remote evidence, log only (T-015; the rule is entry[0], T-020): ER offsets from .claude/re/er-reference.md plus NR's
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

/// ChrSet change lines are coalesced to at most one burst per this long (a state byte in the entry can blink).
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

/// One observed value logged as evidence about a PlayerIns. The names carry the ER offset the idea came from.
/// Since T-020 these are log-only: the local player is decided by the entry[0] rule, not by a signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Signal {
    pub name: &'static str,
    /// `None` = unreadable.
    pub value: Option<u64>,
    /// Shown as hex in the log.
    pub hex: bool,
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

/// A valid PlayerIns of the ChrSet with its evidence signals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlayerProbe {
    pub addr: usize,
    /// ChrSet entry index of its first entry.
    pub index: usize,
    pub signals: Vec<Signal>,
}

/// How the local player was decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Basis {
    /// No PlayerIns at all.
    NoPlayers,
    /// The first valid PlayerIns in ChrSet entry order.
    FirstEntry,
}

impl Basis {
    /// Short text for the overlay and the logs.
    pub(crate) fn label(&self) -> String {
        match self {
            Basis::NoPlayers => "no PlayerIns".to_string(),
            Basis::FirstEntry => "first PlayerIns in the ChrSet (entry[0] rule)".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Decision {
    pub local: Option<usize>,
    pub basis: Basis,
}

/// The local PlayerIns is the first valid PlayerIns in ChrSet entry order; every other one is REMOTE.
/// [VERIFIED: FS-0 (Nucleus) and FS-1 (two PCs, real Steam), 2026-10-10, 2 players; see TECHNICAL_NOTES §4.
/// HYPOTHESIS beyond 2 players.]
pub(crate) fn decide_local(probes: &[PlayerProbe]) -> Decision {
    match probes.first() {
        None => Decision {
            local: None,
            basis: Basis::NoPlayers,
        },
        Some(p) => Decision {
            local: Some(p.addr),
            basis: Basis::FirstEntry,
        },
    }
}

/// `LOCAL` or `REMOTE[i]` (`i` = ChrSet entry index), the `who=` field of every write line.
pub(crate) fn who_label(role: Role, index: usize) -> String {
    match role {
        Role::Local => "LOCAL".to_string(),
        Role::Remote => format!("REMOTE[{index}]"),
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
        f.id, f.v70, f.team
    );
    if let Some(p) = prev {
        let _ = write!(s, " (was id={} +0x70={} team_type={})", p.id, p.v70, p.team);
    }
    s
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

/// What makes two resolutions "the same" for logging: the raw ChrSet entry bytes and the player set. The evidence
/// signals are deliberately NOT part of it: `session_entry_ptr` changes on every read (FS-1 logs), so including it
/// re-logged the whole block every 5 s with identical entry bytes.
type ResolveKey = (Vec<[u8; ENTRY_SIZE]>, Vec<usize>);

pub(crate) struct Resolution {
    pub chrset: usize,
    pub capacity: u32,
    pub entries_ptr: usize,
    pub entries: Vec<Entry>,
    /// Distinct valid PlayerIns of the ChrSet, entry order.
    pub players: Vec<usize>,
    /// The same players with their evidence signals (log only).
    pub probes: Vec<PlayerProbe>,
    pub local: Option<usize>,
    pub basis: Basis,
}

impl Resolution {
    fn key(&self) -> ResolveKey {
        (
            self.entries.iter().map(|e| e.raw.raw).collect(),
            self.players.clone(),
        )
    }

    /// Why the local player is what it is, for logs.
    pub(crate) fn reasoning(&self) -> String {
        format!(
            "{} valid PlayerIns in the ChrSet; local = {} ({})",
            self.players.len(),
            self.local
                .map_or("NONE".to_string(), |p| format!("0x{p:X}")),
            self.basis.label()
        )
    }

    /// ChrSet entry index of the first entry holding `addr`.
    fn index_of(&self, addr: usize) -> Option<usize> {
        self.probes.iter().find(|p| p.addr == addr).map(|p| p.index)
    }

    /// Local or remote role of a PlayerIns of this ChrSet.
    pub(crate) fn role_of(&self, addr: usize) -> Role {
        if self.local == Some(addr) {
            Role::Local
        } else {
            Role::Remote
        }
    }

    /// `LOCAL` / `REMOTE[i]` for the log; `None` if `addr` is not a PlayerIns of the ChrSet.
    pub(crate) fn who(&self, addr: usize) -> Option<String> {
        let index = self.index_of(addr)?;
        Some(who_label(self.role_of(addr), index))
    }

    /// Overlay rows, one per valid PlayerIns. `team` reads the live team_type of an address.
    fn rows(&self, team: &mut dyn FnMut(usize) -> Option<u8>) -> Vec<PlayerRow> {
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
                },
                team: team(p.addr),
                state8: e.raw.raw[8],
                state9: e.raw.raw[9],
                original: None,
                persist: None,
                reverts_last_sec: 0,
                reverts_total: 0,
                id: None,
                team_no: None,
                relation: None,
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

/// The evidence signals of one PlayerIns (log only since T-020). ER offsets shifted by NR's +8, see the consts;
/// reads are VirtualQuery-guarded and an unreadable value is recorded as `None`.
fn read_signals(rc: &mut RegionCache, p: usize, entry: &[u8; ENTRY_SIZE]) -> Vec<Signal> {
    let sig = |name, value, hex| Signal { name, value, hex };
    let holder = rc.read::<u64>(p + PLAYER_SESSION_HOLDER);
    let sess_entry = rc.read::<u64>(p + PLAYER_SESSION_ENTRY);
    vec![
        sig("entry+8", Some(u64::from(entry[8])), false),
        sig("entry+9[ER update_type]", Some(u64::from(entry[9])), false),
        sig(
            "chr_type@+0x70[ER +0x68]",
            rc.read::<u32>(p + CHR_TYPE70).map(u64::from),
            false,
        ),
        sig(
            "net_authority@+0x1EC[ER +0x1E4]",
            rc.read::<u32>(p + CHR_NET_AUTHORITY).map(u64::from),
            false,
        ),
        sig(
            "session_holder_set@+0x5A0[ER +0x598]",
            holder.map(|v| u64::from(v != 0)),
            false,
        ),
        sig(
            "session_entry_set@+0x6C0[ER +0x6B8]",
            sess_entry.map(|v| u64::from(v != 0)),
            false,
        ),
        // ChrIns+0x78 is a per-player id, identical on all clients [VERIFIED: FS-0/FS-1 2026-10-10].
        sig(
            "p2p_handle@+0x78[ER +0x70]",
            rc.read::<u64>(p + CHR_P2P_HANDLE),
            true,
        ),
        sig("session_holder_ptr", holder, true),
        // Changes on every read (an obfuscated value); never part of the "did the ChrSet change" comparison.
        sig("session_entry_ptr", sess_entry, true),
    ]
}

/// Resolve the ChrSet and decide the local PlayerIns (entry[0] rule) from a WorldChrMan instance. Re-run whenever needed: WorldChrMan is re-created on area change.
/// Read-only.
pub(crate) fn resolve(wcm: usize, vts: &PlayerVtables) -> Result<Resolution, ResolveError> {
    let mut rc = RegionCache::direct();
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

    let Decision { local, basis } = decide_local(&probes);
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
        // Evidence only (T-015): the local player is the first PlayerIns, VERIFIED FS-0/FS-1 2026-10-10.
        for p in &res.probes {
            let sigs: Vec<String> = p.signals.iter().map(Signal::text).collect();
            out.push(format!(
                "team:   evidence PlayerIns 0x{:X} entry[{}] raw={} [evidence only; ER offsets + NR shift]: {}",
                p.addr,
                p.index,
                res.entries
                    .get(p.index)
                    .map_or("?".to_string(), |e| hex16(&e.raw.raw)),
                sigs.join(" ")
            ));
        }
        if let Some(p) = res.probes.first() {
            if p.index != 0 {
                out.push(format!(
                    "team: WARNING: the first valid PlayerIns is at entry[{}], not entry[0]; the entry[0] rule was verified with the local player in entry[0]",
                    p.index
                ));
            }
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
// T-026: rosters and the "rivals only" plan (pure logic, no game memory)
// ---------------------------------------------------------------------------------------------------

/// What the rosters say about the players of the current ChrSet (keyed by PlayerIns address).
#[derive(Debug, Default)]
pub(crate) struct RosterView {
    pub(crate) relation: HashMap<usize, Relation>,
    pub(crate) team_no: HashMap<usize, u8>,
    pub(crate) ids: HashMap<usize, Option<u64>>,
    /// Set when the ids were unusable and the view fell back to "everyone except LOCAL is a rival".
    pub(crate) warning: Option<String>,
    /// `teams: team 0 = [0x6001FFFFFFFF(LOCAL)], team 1 = [0x6002FFFFFFFF]` (or the warning text).
    pub(crate) summary: String,
    /// Sorted (id, is_local) pairs: the view is re-logged only when this changes.
    pub(crate) key: Vec<(Option<u64>, bool)>,
}

/// An id of 0 is treated as unreadable: it is what an unfilled object would hold (every observed id is
/// non-zero, TECHNICAL_NOTES §4); failing open on it is the conservative choice.
fn usable_id(id: Option<u64>) -> Option<u64> {
    id.filter(|&v| v != 0)
}

/// Build the view for `players` (address, id read from ChrIns+0x78) with `local` the LOCAL PlayerIns.
/// Rosters come from `assign_teams` over the ids, so every client computes the same ones. If any id is
/// unreadable or two players share an id, nothing can be trusted: everyone except LOCAL is a rival
/// (fail open to free-for-all) and `warning` says why.
pub(crate) fn build_view(
    players: &[(usize, Option<u64>)],
    local: Option<usize>,
    team_size: usize,
) -> RosterView {
    let mut view = RosterView::default();
    for &(a, id) in players {
        view.ids.insert(a, usable_id(id));
    }
    let mut key: Vec<(Option<u64>, bool)> = players
        .iter()
        .map(|&(a, id)| (usable_id(id), Some(a) == local))
        .collect();
    key.sort();
    view.key = key;
    let readable: Vec<u64> = players
        .iter()
        .filter_map(|&(_, id)| usable_id(id))
        .collect();
    let mut dedup = readable.clone();
    dedup.sort_unstable();
    dedup.dedup();
    let problem = if readable.len() != players.len() {
        Some("a player id (ChrIns+0x78) is unreadable".to_string())
    } else if dedup.len() != readable.len() {
        Some("two players share the same id".to_string())
    } else {
        None
    };
    if let Some(p) = problem {
        for &(a, _) in players {
            if local.is_some() {
                let rel = if Some(a) == local {
                    Relation::Local
                } else {
                    Relation::Rival
                };
                view.relation.insert(a, rel);
            }
        }
        let msg = format!("{p}; treating everyone except LOCAL as a rival (free-for-all)");
        view.summary = format!("teams: WARNING {msg}");
        view.warning = Some(msg);
        return view;
    }
    let ids: Vec<PlayerId> = readable.iter().map(|&v| PlayerId(v)).collect();
    let rosters = assign_teams(&ids, team_size);
    let local_id = local
        .and_then(|l| players.iter().find(|(a, _)| *a == l))
        .and_then(|&(_, id)| usable_id(id))
        .map(PlayerId);
    for &(a, id) in players {
        let Some(pid) = usable_id(id).map(PlayerId) else {
            continue;
        };
        if let Some(t) = rosters.team_of(pid) {
            view.team_no.insert(a, t.0);
        }
        if let Some(l) = local_id {
            let rel = if pid == l {
                Relation::Local
            } else if rosters.are_allies(l, pid) {
                Relation::Ally
            } else {
                Relation::Rival
            };
            view.relation.insert(a, rel);
        }
    }
    let parts: Vec<String> = rosters
        .teams()
        .map(|(t, members)| {
            let m: Vec<String> = members
                .iter()
                .map(|p| {
                    if Some(*p) == local_id {
                        format!("0x{:X}(LOCAL)", p.0)
                    } else {
                        format!("0x{:X}", p.0)
                    }
                })
                .collect();
            format!("team {} = [{}]", t.0, m.join(", "))
        })
        .collect();
    view.summary = format!("teams: {}", parts.join(", "));
    view
}

/// One PvPvE pass, decided: which PlayerIns get `pvpve_team` and which previously written ones are restored.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PvpvePlan {
    pub(crate) writes: Vec<usize>,
    /// Previously written players that are no longer rivals; the executor restores each one's captured
    /// original if its value is still `pvpve_team`, and forgets it either way.
    pub(crate) restores: Vec<usize>,
}

/// `players` = (address, live team_type). Scheme "all": every player at 2 is written (T-020, unchanged).
/// Scheme "rivals": only rivals of the local player at 2 are written; LOCAL, allies and players with no
/// known relation (no local player) are never written.
pub(crate) fn plan_pvpve(
    scheme: Scheme,
    view: &RosterView,
    players: &[(usize, Option<u8>)],
    written: &HashSet<usize>,
) -> PvpvePlan {
    let mut plan = PvpvePlan::default();
    for &(a, cur) in players {
        let wants = cur.is_some_and(pvpve_wants_write);
        match scheme {
            Scheme::All => {
                if wants {
                    plan.writes.push(a);
                }
            }
            Scheme::Rivals => {
                let rival = view.relation.get(&a) == Some(&Relation::Rival);
                if rival && wants {
                    plan.writes.push(a);
                } else if !rival && written.contains(&a) {
                    plan.restores.push(a);
                }
            }
        }
    }
    plan
}

// ---------------------------------------------------------------------------------------------------
// Controller driven from the main loop
// ---------------------------------------------------------------------------------------------------

/// T-020: true if the PvPvE mode should overwrite this live team_type (only the expedition default).
pub(crate) fn pvpve_wants_write(current: u8) -> bool {
    current == PVPVE_FROM
}

/// Persist state of one player (T-015: persist is per player).
struct PersistEntry {
    /// The value re-applied every tick.
    value: u8,
    tracker: PersistTracker,
    /// The first successful application was logged.
    first_logged: bool,
}

/// The team_type players have during an expedition [VERIFIED: TC-004 runs, FS-0]; the PvPvE mode only replaces this value.
const PVPVE_FROM: u8 = 2;

/// After this many automatic writes only every [`PVPVE_LOG_EVERY`]th one is logged, so a game that keeps flipping the
/// value back cannot flood the log (the overlay counter and the heartbeat still count all of them).
const PVPVE_LOG_ALL_UP_TO: u64 = 200;
const PVPVE_LOG_EVERY: u64 = 50;
/// T-030 (player build): the first writes logged one by one, then a summary line this often.
const PLAYER_LOG_FIRST: u64 = 20;
const PLAYER_SUMMARY_EVERY: Duration = Duration::from_secs(60);

pub(crate) struct TeamCtl {
    /// T-020: `pvpve` from the config. When false the mode is OFF and cannot be switched on at runtime.
    pvpve_enabled: bool,
    /// T-030: developer mode. Off = player build: the PvPvE mode cannot be paused or resumed and the per-player watcher
    /// stays quiet; automatic writes are logged for the first [`PLAYER_LOG_FIRST`] then summarised once a minute.
    dev: bool,
    /// T-030 (player build): when the last once-a-minute write summary was logged, and the write count then.
    last_summary: Option<Instant>,
    /// Running (true) or paused by F12 / the overlay checkbox (false). Only meaningful when `pvpve_enabled`.
    pvpve_running: bool,
    pvpve_scheme: Scheme,
    team_size: usize,
    /// T-026 (rivals scheme): players this client wrote `pvpve_team` to and has not restored yet.
    pvpve_written: HashSet<usize>,
    /// T-026: key of the roster view last logged (log once per change).
    last_roster_key: Option<Vec<(Option<u64>, bool)>>,
    pvpve_team: u8,
    pvpve_every: Duration,
    next_pvpve: Instant,
    /// Successful automatic writes this session.
    pvpve_writes: u64,
    /// Rate limit for the write-failure line (reuses the persist tracker.s limiter).
    pvpve_fail_log: PersistTracker,
    /// Valid PlayerIns count seen by the last resolve of the watcher / PvPvE tick (None = never resolved).
    last_player_count: Option<usize>,
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
    marker: Marker,
    next_persist: Instant,
    /// T-013: last quiet resolve for the overlay snapshot (refreshed at most every [`LIVE_REFRESH`]).
    live: LiveView,
    next_live: Instant,
    /// T-032 (dev only): snap-back test candidates, selection and saved values.
    pos: PosTest,
    /// T-032: pending follow-up value logs after an F6 write: (due, label, PlayerIns).
    pos_follow: Vec<(Instant, &'static str, usize)>,
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
    /// T-026: why the rosters fell back to free-for-all (empty when they did not).
    roster_warning: String,
}

impl TeamCtl {
    pub(crate) fn new(cfg: &Config, base: usize, size: usize) -> Self {
        let cycler = Cycler::new(&cfg.team_candidates);
        let (i, n) = cycler.position();
        tracing::info!(
            "T-008: selected team candidate = {} ({i}/{n}); controls: developer console (team_set, team_restore, pvp)",
            cycler.current()
        );
        if cfg.pvpve {
            tracing::info!(
                "T-020: PvPvE mode ON{}: every {} ms, players with team_type {PVPVE_FROM} get {}; T-026 scheme = {}, team_size = {}",
                if cfg.dev_mode {
                    " (console: pvp 0 pauses, pvp 1 resumes)"
                } else {
                    " (player build: always on)"
                },
                cfg.pvpve_interval_ms,
                cfg.pvpve_team,
                cfg.scheme().label(),
                cfg.team_size_usize()
            );
        } else {
            tracing::info!("T-020: PvPvE mode is OFF (set pvpve = true in nrpvp.toml to enable)");
        }
        if cfg.dev_mode {
            let (_, warnings) = poswrite::parse_list(&cfg.pos_candidates);
            for w in warnings {
                tracing::warn!("T-032: {w}");
            }
            tracing::info!(
                "T-032 POS: snap-back test (dev only, LOCAL player only, 12 bytes): {} candidate(s); console pos_next / pos_save / pos_go",
                poswrite::parse_list(&cfg.pos_candidates).0.len()
            );
        }
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
            pvpve_enabled: cfg.pvpve,
            dev: cfg.dev_mode,
            last_summary: None,
            pvpve_running: cfg.pvpve,
            pvpve_scheme: cfg.scheme(),
            team_size: cfg.team_size_usize(),
            pvpve_written: HashSet::new(),
            last_roster_key: None,
            pvpve_team: cfg.pvpve_team,
            pvpve_every: Duration::from_millis(cfg.pvpve_interval_ms),
            next_pvpve: now,
            pvpve_writes: 0,
            pvpve_fail_log: PersistTracker::new(),
            last_player_count: None,
            log: ResolveLog::default(),
            watch: WatchState::default(),
            next_watch: now,
            last_err: None,
            originals: HashMap::new(),
            last_written: HashMap::new(),
            persist: BTreeMap::new(),
            marker: Marker::default(),
            next_persist: now,
            live: LiveView::default(),
            next_live: now,
            pos: PosTest::new(&poswrite::parse_list(&cfg.pos_candidates).0, base, size),
            pos_follow: Vec::new(),
        }
    }

    /// The team size the rules use right now: `team_size` from the config, or the queue match's override (MM-2) while a match
    /// is MATCH READY. With `mm_queue` off the override is never set, so this is exactly `self.team_size`.
    fn team_size_now(&self) -> usize {
        crate::mm::effective_team_size(self.team_size)
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
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
        // values, so everything keyed by address is forgotten: originals, written values and persist entries.
        // The PvPvE running/paused state and its write counter belong to the session and are kept.
        if !self.originals.is_empty() || !self.persist.is_empty() {
            tracing::info!(
                "T-008: WorldChrMan instance changed; forgetting {} captured original team value(s) and {} persist entr(y/ies)",
                self.originals.len(),
                self.persist.len()
            );
        }
        self.originals.clear();
        self.last_written.clear();
        self.persist.clear();
        self.pvpve_written.clear();
        // T-032: saved positions belong to the old PlayerIns objects.
        self.pos.forget();
        self.pos_follow.clear();
        self.last_roster_key = None;
        self.last_player_count = None;
        self.next_watch = Instant::now();
        self.next_pvpve = Instant::now();
    }

    /// Resolve and log errors only when they change (the watcher calls this every second).
    fn resolve_logged(&mut self, quiet_repeat: bool) -> Option<Resolution> {
        match resolve(self.inst, &self.vts) {
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
                    "T-008: {src} ignored: no local player ({}); use a row button",
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

    /// T-032 (dev only): select the next snap-back position candidate (console `pos_next`).
    pub(crate) fn pos_cycle(&mut self) {
        if !self.dev {
            return;
        }
        let line = self.pos.cycle();
        tracing::info!("{line}");
    }

    /// The local PlayerIns, freshly resolved (ChrSet entry[0] rule, PlayerIns vtable check). Quiet resolve; logs only failures.
    fn fresh_local(&mut self, src: &str) -> Option<usize> {
        if self.inst == 0 || self.vts.is_empty() {
            tracing::info!(
                "T-032 POS: {src} ignored, WorldChrMan or the PlayerIns vtables are not known yet"
            );
            return None;
        }
        let res = match resolve(self.inst, &self.vts) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("T-032 POS: {src} ignored, could not resolve the players: {e}");
                return None;
            }
        };
        let Some(local) = res.local else {
            tracing::warn!(
                "T-032 POS: {src} ignored: no local player ({})",
                res.reasoning()
            );
            return None;
        };
        // Re-check the vtable of the object right now (resolve() already required it; this guards the window since).
        let vt_ok =
            is_ptr(local) && mem::read::<usize>(local).is_some_and(|vt| self.vts.contains(vt));
        if !vt_ok {
            tracing::warn!(
                "T-032 POS: {src} ignored: 0x{local:X} no longer has a PlayerIns vtable"
            );
            return None;
        }
        Some(local)
    }

    /// T-032 (dev only): save the LOCAL player's current 12 bytes of every candidate (console `pos_save`).
    pub(crate) fn pos_save(&mut self, src: &str) {
        if !self.dev {
            return;
        }
        let Some(local) = self.fresh_local(src) else {
            return;
        };
        tracing::info!("T-032 POS: {src} save for LOCAL PlayerIns 0x{local:X}");
        for l in self.pos.save(&RealMem, local) {
            tracing::info!("{l}");
        }
    }

    /// T-032 (dev only): write the saved 12 bytes into the SELECTED candidate of the LOCAL player (console `pos_go`), then schedule the
    /// +100 ms / +500 ms / +2 s value logs.
    pub(crate) fn pos_write(&mut self, src: &str) {
        if !self.dev {
            return;
        }
        let Some(local) = self.fresh_local(src) else {
            return;
        };
        tracing::info!("{}", self.pos.values_line(&RealMem, local, "before write"));
        match self.pos.write(&mut RealMem, local) {
            Ok(r) => {
                tracing::info!("{} ({src})", r.line());
                let now = Instant::now();
                self.pos_follow = poswrite::FOLLOW_MS
                    .iter()
                    .zip(["+100ms", "+500ms", "+2s"])
                    .map(|(&ms, label)| (now + Duration::from_millis(ms), label, local))
                    .collect();
            }
            Err(e) => tracing::warn!("T-032 POS: {src} write refused: {e}"),
        }
    }

    /// T-032: log every candidate's value for the pending follow-ups that are due. Read-only.
    fn pos_follow_tick(&mut self) {
        if self.pos_follow.is_empty() {
            return;
        }
        let now = Instant::now();
        let (due, rest): (Vec<_>, Vec<_>) = self
            .pos_follow
            .drain(..)
            .partition(|(when, _, _)| now >= *when);
        self.pos_follow = rest;
        for (_, label, player) in due {
            tracing::info!("{}", self.pos.values_line(&RealMem, player, label));
        }
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

    /// The gated write plus its bookkeeping (original capture, last-written value, persist-value follow-up), without
    /// the log line. `Ok((write, first, original))`: `first` = this was the first write to `addr`, `original` = the
    /// value captured before it.
    fn apply_write(
        &mut self,
        addr: usize,
        who: &str,
        v: u8,
    ) -> Result<(WriteOk, bool, u8), String> {
        let w = write_team_type(addr, v, &self.vts)?;
        let first = !self.originals.contains_key(&addr);
        let orig = *self.originals.entry(addr).or_insert(w.old);
        self.last_written.insert(addr, v);
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
        Ok((w, first, orig))
    }

    /// The write itself plus its log line. `who` is the `LOCAL` / `REMOTE[i]` label. Returns true on success.
    fn write_to(&mut self, addr: usize, who: &str, v: u8, src: &str) -> bool {
        match self.apply_write(addr, who, v) {
            Ok((w, first, orig)) => {
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
                true
            }
            Err(e) => {
                tracing::warn!("T-008: {src} write who={who} to PlayerIns 0x{addr:X} failed: {e}");
                false
            }
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

    /// T-020: current state of the PvPvE mode.
    pub(crate) fn pvpve_state(&self) -> PvpveState {
        match (self.pvpve_enabled, self.pvpve_running) {
            (false, _) => PvpveState::Off,
            (true, true) => PvpveState::On,
            (true, false) => PvpveState::Paused,
        }
    }

    /// Resume (`on`) or pause the PvPvE mode (overlay checkbox, F11/F12). Does nothing, and says so, when the mode is
    /// off in the config.
    pub(crate) fn set_pvpve(&mut self, on: bool, src: &str) {
        // T-030: no pause/resume in the player build, whatever asks for it.
        if !self.dev {
            return;
        }
        if !self.pvpve_enabled {
            tracing::info!(
                "T-020: {src}: PvPvE mode is OFF in the config (pvpve = false); ignored"
            );
            return;
        }
        if self.pvpve_running == on {
            return;
        }
        self.pvpve_running = on;
        self.next_pvpve = Instant::now();
        tracing::info!(
            "T-020: {src}: PvPvE mode {} ({} automatic write(s) so far)",
            if on { "resumed" } else { "paused" },
            self.pvpve_writes
        );
        if !on {
            self.restore_written(src);
        }
    }

    /// T-026: put back the captured original of every player the rivals tick wrote (pause, roster change).
    /// A player whose value is no longer `pvpve_team` was changed by someone else and is left alone.
    fn restore_written(&mut self, src: &str) {
        let addrs: Vec<usize> = self.pvpve_written.drain().collect();
        for addr in addrs {
            self.restore_written_one(addr, src);
        }
    }

    fn restore_written_one(&mut self, addr: usize, src: &str) {
        self.pvpve_written.remove(&addr);
        let Some(&orig) = self.originals.get(&addr) else {
            return;
        };
        // Guarded read; offset VERIFIED on NR 1.3.3.0 (TECHNICAL_NOTES §4).
        if mem::read::<u8>(addr + CHR_TEAM) != Some(self.pvpve_team) {
            return;
        }
        match write_team_type(addr, orig, &self.vts) {
            Ok(w) => tracing::info!(
                "T-026: {src}: restored PlayerIns 0x{addr:X} +0x74: {} -> {orig} (readback {})",
                w.old,
                w.readback
            ),
            Err(e) => tracing::warn!("T-026: {src}: restore of PlayerIns 0x{addr:X} failed: {e}"),
        }
    }

    /// Automatic writes this session.
    #[cfg(test)]
    pub(crate) fn pvpve_writes(&self) -> u64 {
        self.pvpve_writes
    }

    /// `ON (team 29, 12 writes)` for the heartbeat line.
    pub(crate) fn pvpve_summary(&self) -> String {
        match self.pvpve_state() {
            PvpveState::Off => "OFF".to_string(),
            s => format!(
                "{} (team {}, {} writes, scheme {}, team_size {})",
                s.label(),
                self.pvpve_team,
                self.pvpve_writes,
                self.pvpve_scheme.label(),
                self.team_size_now()
            ),
        }
    }

    /// Players seen by the last resolve, for the heartbeat line.
    pub(crate) fn player_count_text(&self) -> String {
        self.last_player_count
            .map_or("?".to_string(), |n| n.to_string())
    }

    /// T-015: `MARK #n: <text>` (console `mark`). Needs no game memory.
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
        s.pvpve = self.pvpve_state();
        s.pvpve_team = self.pvpve_team;
        s.pvpve_writes = self.pvpve_writes;
        s.pvpve_scheme = self.pvpve_scheme.label().to_string();
        s.team_size = u8::try_from(self.team_size_now()).unwrap_or(u8::MAX);
        s.roster_warning = self.live.roster_warning.clone();
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

    /// T-033/T-035 (both modes): every PlayerIns of the ChrSet with its id and relation to LOCAL, from the same quiet resolve that
    /// feeds the overlay (refreshed at most every [`LIVE_REFRESH`]). Read-only.
    pub(crate) fn combat_subjects(&mut self) -> Vec<super::combat_state::Subject> {
        if Instant::now() >= self.next_live {
            self.next_live = Instant::now() + LIVE_REFRESH;
            self.refresh_live();
        }
        self.live
            .rows
            .iter()
            .map(|r| super::combat_state::Subject {
                addr: r.addr,
                label: if r.role == Role::Local {
                    format!("LOCAL[{}]", r.index)
                } else {
                    format!("REMOTE[{}]", r.index)
                },
                local: r.role == Role::Local,
                id: r.id,
                relation: r.relation,
                team_no: r.team_no,
            })
            .collect()
    }

    /// T-026: read every player's id and build the roster view (quiet).
    fn view_for(&self, res: &Resolution) -> RosterView {
        let players: Vec<(usize, Option<u64>)> = res
            .players
            .iter()
            // Guarded read (VirtualQuery). ChrIns+0x78 is a per-player id identical on all clients:
            // VERIFIED on NR 1.3.3.0 by FS-0/FS-1 for 2 players, see TECHNICAL_NOTES §4.
            .map(|&p| (p, mem::read::<u64>(p + CHR_P2P_HANDLE)))
            .collect();
        build_view(&players, res.local, self.team_size_now())
    }

    /// T-026: log the rosters once per change of the player set.
    fn note_roster(&mut self, view: &RosterView) {
        if self.last_roster_key.as_ref() == Some(&view.key) {
            return;
        }
        self.last_roster_key = Some(view.key.clone());
        if view.warning.is_some() {
            tracing::warn!("{}", view.summary);
        } else {
            tracing::info!("{}", view.summary);
        }
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
        match resolve(self.inst, &self.vts) {
            Ok(r) => {
                // Read-only, VirtualQuery-guarded (mem::read). Offset VERIFIED on NR 1.3.3.0 by
                // T-007a; see TECHNICAL_NOTES section 4.
                self.live.rows = r.rows(&mut |a| mem::read::<u8>(a + CHR_TEAM));
                let view = self.view_for(&r);
                self.note_roster(&view);
                for row in &mut self.live.rows {
                    row.id = view.ids.get(&row.addr).copied().flatten();
                    row.team_no = view.team_no.get(&row.addr).copied();
                    row.relation = view.relation.get(&row.addr).copied();
                }
                self.live.roster_warning = view.warning.clone().unwrap_or_default();
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
        if self.dev {
            self.pos_follow_tick();
        }
        if self.pvpve_enabled && self.pvpve_running && now >= self.next_pvpve {
            self.next_pvpve = now + self.pvpve_every;
            self.pvpve_tick();
        }
    }

    fn watch_tick(&mut self) {
        let Some(res) = self.resolve_logged(true) else {
            return;
        };
        self.last_player_count = Some(res.players.len());
        let now_ms = self.now_ms();
        let lines = self.log.lines(&res, now_ms, (self.base, self.size));
        if self.dev {
            for l in lines {
                tracing::info!("T-008: {l}");
            }
        }
        let mut rc = RegionCache::direct();
        for &p in &res.players {
            let Some(f) = read_fields(&mut rc, p) else {
                continue;
            };
            if let Some(prev) = self.watch.update(p, f) {
                if self.dev {
                    let who = res.who(p).unwrap_or_else(|| "?".to_string());
                    tracing::info!("T-008: {}", watch_line(p, &who, f, prev));
                }
            }
        }
        // MM-3: tell the matchmaking worker whether the local player is in the Roundtable (team_type 1) or an expedition (2)
        // (TECHNICAL_NOTES.md §4, VERIFIED TC-004 run 1, NR 1.3.3.0). Read-only; a plain atomic store, only while mm is enabled.
        if crate::mm::enabled() {
            if let Some(f) = res.local.and_then(|l| read_fields(&mut rc, l)) {
                crate::mm::note_local_team(f.team);
            }
            // MM-3f: the ChrSet player count (a real Seamless Co-op join shows as 2+ at the Roundtable, S9 5f).
            crate::mm::note_local_players(res.players.len());
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

    /// T-020/T-026: one PvPvE pass. Scheme "all": every ChrSet player whose live team_type is [`PVPVE_FROM`] gets
    /// `pvpve_team`. Scheme "rivals": only rivals of the local player do, and players this tick wrote that are no longer
    /// rivals (roster change) are restored. Every other value (Roundtable 1, anything a manual experiment set) is left
    /// alone. Quiet resolve: resolve errors are logged (deduplicated) by the watcher. Same gated 1-byte write as the
    /// hotkeys.
    fn pvpve_tick(&mut self) {
        let Ok(res) = resolve(self.inst, &self.vts) else {
            return;
        };
        self.last_player_count = Some(res.players.len());
        let view = self.view_for(&res);
        self.note_roster(&view);
        let live: Vec<(usize, Option<u8>)> = res
            .players
            .iter()
            // Read-only, VirtualQuery-guarded. Offset VERIFIED on NR 1.3.3.0 (TECHNICAL_NOTES §4).
            .map(|&p| (p, mem::read::<u8>(p + CHR_TEAM)))
            .collect();
        let plan = plan_pvpve(self.pvpve_scheme, &view, &live, &self.pvpve_written);
        for addr in plan.restores {
            self.restore_written_one(addr, "roster change");
        }
        for p in plan.writes {
            let who = res.who(p).unwrap_or_else(|| "?".to_string());
            let team = self.pvpve_team;
            match self.apply_write(p, &who, team) {
                Ok((w, _first, _orig)) => {
                    if self.pvpve_scheme == Scheme::Rivals {
                        self.pvpve_written.insert(p);
                    }
                    self.pvpve_writes += 1;
                    let n = self.pvpve_writes;
                    if self.should_log_write(n) {
                        tracing::info!(
                            "T-020: pvpve team write who={who} PlayerIns 0x{p:X} +0x74: {} -> {team} (readback {}); write #{n}",
                            w.old,
                            w.readback
                        );
                    }
                }
                Err(e) => {
                    let now_ms = self.now_ms();
                    if self.pvpve_fail_log.should_log_failure(now_ms) {
                        tracing::warn!(
                            "T-020: pvpve team write who={who} to PlayerIns 0x{p:X} failed: {e}"
                        );
                    }
                }
            }
        }
    }

    /// Whether automatic write number `n` gets its own log line. Dev: the first [`PVPVE_LOG_ALL_UP_TO`], then every
    /// [`PVPVE_LOG_EVERY`]th. Player build (T-030): the first [`PLAYER_LOG_FIRST`], then one summary line a minute.
    fn should_log_write(&mut self, n: u64) -> bool {
        if self.dev {
            return n <= PVPVE_LOG_ALL_UP_TO || n.is_multiple_of(PVPVE_LOG_EVERY);
        }
        if n <= PLAYER_LOG_FIRST {
            return true;
        }
        let due = self
            .last_summary
            .is_none_or(|t| t.elapsed() >= PLAYER_SUMMARY_EVERY);
        if due {
            self.last_summary = Some(Instant::now());
            tracing::info!("T-020: pvpve summary: {n} automatic team write(s) so far");
        }
        false
    }

    fn persist_tick(&mut self) {
        let now_ms = self.now_ms();
        // Quiet resolve: errors are already logged (deduplicated) by the watcher.
        let Ok(res) = resolve(self.inst, &self.vts) else {
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
            line.contains("who=REMOTE[2]")
                && line.contains("team_type=6")
                && line.contains("was id=1")
        );
        assert!(!watch_line(0x10, "LOCAL", b, None).contains("was"));
    }

    // ---- T-020: the entry[0] local-player rule on synthetic multi-player data ----

    /// A probe at the given ChrSet entry index with arbitrary evidence values.
    fn probe(addr: usize, index: usize, vals: &[u64]) -> PlayerProbe {
        const NAMES: [&str; 3] = ["sigA", "sigB", "sigC"];
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
                })
                .collect(),
        }
    }

    #[test]
    fn local_is_the_first_valid_playerins_whatever_the_evidence_says() {
        let d = decide_local(&[]);
        assert_eq!((d.local, &d.basis), (None, &Basis::NoPlayers));

        // One player: local.
        let d = decide_local(&[probe(0x100, 3, &[1, 2])]);
        assert_eq!((d.local, &d.basis), (Some(0x100), &Basis::FirstEntry));

        // Two, three and four players: always the first in entry order; the signals are irrelevant (here the
        // third player is the "odd one out" on sigA, which the old heuristic would have picked).
        let ps = [
            probe(0xA, 0, &[5, 7]),
            probe(0xB, 1, &[5, 7]),
            probe(0xC, 2, &[9, 7]),
            probe(0xD, 3, &[5, 7]),
        ];
        for n in 2..=4 {
            let d = decide_local(&ps[..n]);
            assert_eq!((d.local, &d.basis), (Some(0xA), &Basis::FirstEntry), "{n}");
        }
        // Index gaps do not matter: the first valid player is local even if it is not at entry 0.
        let d = decide_local(&[probe(0xB, 2, &[1]), probe(0xA, 4, &[1])]);
        assert_eq!(d.local, Some(0xB));
        assert_eq!(
            Basis::FirstEntry.label(),
            "first PlayerIns in the ChrSet (entry[0] rule)"
        );
    }

    #[test]
    fn who_labels() {
        assert_eq!(who_label(Role::Local, 2), "LOCAL");
        assert_eq!(who_label(Role::Remote, 3), "REMOTE[3]");
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
        let r = resolve(wcm.as_ptr() as usize, &vts()).expect("resolves");
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
        assert_eq!(r.basis, Basis::FirstEntry);
        assert_eq!(r.who(pa as usize).as_deref(), Some("LOCAL"));
        assert_eq!(r.who(enemy.as_ptr() as usize), None);

        // The log prints every raw entry and the evidence for the player the first time, nothing when
        // unchanged.
        let mut lg = ResolveLog::default();
        let lines = lg.lines(&r, 0, (0, 0));
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.starts_with("team:   entry["))
                .count(),
            6
        );
        let ev: Vec<&String> = lines.iter().filter(|l| l.contains("evidence")).collect();
        assert_eq!(ev.len(), 1);
        assert!(ev[0].contains("evidence only") && ev[0].contains("net_authority@+0x1EC"));
        assert!(
            ev[0].contains("04 04"),
            "the raw entry bytes are in the evidence"
        );
        assert!(lines.iter().any(|l| l.contains("local player = ")));
        assert!(
            !lines.iter().any(|l| l.contains("1C820")),
            "the old +0x1C820 rule and its log lines are gone"
        );
        assert!(lg.lines(&r, 10, (0, 0)).is_empty());
    }

    #[test]
    fn resolve_with_several_players_uses_entry_order_and_labels_rows() {
        // Three PlayerIns in entries 1..=3 (entry 0 empty); the third has a different chr_type (+0x70), which
        // the old heuristic would have picked. The rule is the first valid PlayerIns, whatever the evidence says.
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
        let entries = entries_with_state(&[0, pa as u64, pb as u64, pc as u64], &[]);
        let (wcm, _e) = fake_wcm(4, &entries);
        let w = wcm.as_ptr() as usize;

        let r = resolve(w, &vts()).expect("resolves");
        assert_eq!(r.players, vec![pa, pb, pc]);
        assert_eq!(r.local, Some(pa), "the first valid PlayerIns is local");
        assert_eq!(r.basis, Basis::FirstEntry);
        assert_eq!(r.who(pa).as_deref(), Some("LOCAL"));
        assert_eq!(r.who(pb).as_deref(), Some("REMOTE[2]"));
        assert_eq!(r.who(pc).as_deref(), Some("REMOTE[3]"));
        assert_eq!(r.role_of(pb), Role::Remote);
        let rows = r.rows(&mut |addr| Some(unsafe { *((addr + CHR_TEAM) as *const u8) }));
        assert_eq!(
            rows.iter().map(|x| (x.index, x.role)).collect::<Vec<_>>(),
            vec![(1, Role::Local), (2, Role::Remote), (3, Role::Remote)]
        );
        assert_eq!(
            (rows[0].state8, rows[0].state9, rows[0].team),
            (4, 4, Some(2))
        );
        assert_eq!(rows[0].basis, Basis::FirstEntry.label());
        assert_eq!(rows[1].basis, "not the local player");

        // With the local player in entry[0] (as in the real game) there is no warning; with an empty entry[0]
        // (above) there is one.
        let entries0 = entries_with_state(&[pa as u64, pb as u64, pc as u64], &[]);
        let (wcm0, _e0) = fake_wcm(3, &entries0);
        let r0 = resolve(wcm0.as_ptr() as usize, &vts()).expect("resolves");
        assert_eq!(r0.local, Some(pa));
        assert_eq!(r0.who(pa).as_deref(), Some("LOCAL"));
        assert_eq!(r0.who(pc).as_deref(), Some("REMOTE[2]"));
        let mut lg = ResolveLog::default();
        assert!(!lg
            .lines(&r0, 0, (0, 0))
            .iter()
            .any(|l| l.contains("WARNING")));
        let mut lg = ResolveLog::default();
        assert!(lg
            .lines(&r, 0, (0, 0))
            .iter()
            .any(|l| l.contains("WARNING") && l.contains("entry[1]")));

        // Permuting the entries moves the LOCAL role with them.
        let swapped = entries_with_state(&[pb as u64, pa as u64], &[]);
        let (wcm2, _e2) = fake_wcm(2, &swapped);
        let r2 = resolve(wcm2.as_ptr() as usize, &vts()).expect("resolves");
        assert_eq!(r2.local, Some(pb));
        assert_eq!(r2.who(pa).as_deref(), Some("REMOTE[1]"));
    }

    #[test]
    fn resolve_refuses_bad_capacity_and_pointer() {
        let (wcm, _e) = fake_wcm(0, &[0u8; 0x100]);
        assert_eq!(
            resolve(wcm.as_ptr() as usize, &vts()).err(),
            Some(ResolveError::BadCapacity(0))
        );
        let (wcm, _e) = fake_wcm(17, &[0u8; 0x100]);
        assert_eq!(
            resolve(wcm.as_ptr() as usize, &vts()).err(),
            Some(ResolveError::BadCapacity(17))
        );
        let mut wcm = vec![0u8; 0x1D000];
        wcm[WCM_CHRSET + CHRSET_CAPACITY] = 6; // entries pointer left null
        assert_eq!(
            resolve(wcm.as_ptr() as usize, &vts()).err(),
            Some(ResolveError::BadEntriesPtr(0))
        );
        assert_eq!(
            resolve(0, &vts()).err(),
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
        let r1 = resolve(wcm.as_ptr() as usize, &vts()).expect("ok");
        assert!(!lg.lines(&r1, 0, (0, 0)).is_empty());
        assert!(lg.lines(&r1, 1000, (0, 0)).is_empty());
        // The entry state byte +9 blinks 04 -> 02 (seen in TC-004): within the gap it is coalesced...
        let mut r2 = resolve(wcm.as_ptr() as usize, &vts()).expect("ok");
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

    /// Three fake PlayerIns (team 2) in entries 1..=3 of a 4-slot ChrSet, in the order objs[2], objs[0], objs[1].
    /// The first valid entry (objs[2]) is LOCAL.
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
            // Entry order: empty, objs[2], objs[0], objs[1]. The first valid entry (objs[2]) is LOCAL.
            let ptrs: Vec<u64> = [None, Some(2), Some(0), Some(1)]
                .iter()
                .map(|i| i.map_or(0, |i: usize| objs[i].as_ptr() as u64))
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
            self.ctl_with(&Config::default())
        }

        fn ctl_with(&self, cfg: &Config) -> TeamCtl {
            let mut ctl = TeamCtl::new(cfg, 0, 0);
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
        assert_eq!(ctl.cycler.current(), 6);
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
    fn snapshot_marks_the_first_entry_local_and_a_new_instance_forgets_the_rest() {
        let f = Fake::new();
        let mut ctl = f.ctl();
        let c = f.addr(2);
        let mut snap = Snapshot::default();
        ctl.fill_snapshot(&mut snap);
        // `c` sits in the first valid entry, so it is LOCAL (the entry[0] rule), whatever its evidence says.
        assert_eq!(snap.local_player, Some(c));
        assert_eq!(snap.local_basis, Basis::FirstEntry.label());
        assert_eq!(snap.players.len(), 3);
        assert_eq!(
            snap.players.iter().map(|p| p.role).collect::<Vec<_>>(),
            vec![Role::Local, Role::Remote, Role::Remote]
        );
        assert_eq!(snap.players[0].addr, c);
        // The global buttons write to the local player only.
        ctl.set_and_write(29, "overlay");
        assert_eq!((f.team(0), f.team(1), f.team(2)), (2, 2, 29));
        // A new WorldChrMan instance forgets everything keyed by address.
        ctl.set_instance(f.wcm.as_ptr() as usize + 8);
        assert!(ctl.originals.is_empty());
    }

    // ---- T-020: the PvPvE mode ----

    fn pvpve_cfg(team: u8) -> Config {
        Config {
            dev_mode: true,
            pvpve: true,
            pvpve_team: team,
            pvpve_scheme: "all".to_string(),
            ..Config::default()
        }
    }

    #[test]
    fn pvpve_rewrites_only_team_2_and_counts_the_writes() {
        assert!(pvpve_wants_write(2));
        for v in [0u8, 1, 3, 6, 13, 29, 30, 255] {
            assert!(!pvpve_wants_write(v), "{v}");
        }
        let f = Fake::new();
        let mut ctl = f.ctl_with(&pvpve_cfg(29));
        assert_eq!(ctl.pvpve_state(), PvpveState::On);
        // Players: 0 = Roundtable (1), 1 = expedition default (2), 2 = already 29 (e.g. set by hand).
        f_set(&f, 0, 1);
        f_set(&f, 2, 29);
        ctl.pvpve_tick();
        assert_eq!((f.team(0), f.team(1), f.team(2)), (1, 29, 29));
        assert_eq!(ctl.pvpve_writes(), 1);
        // A second pass has nothing to do.
        ctl.pvpve_tick();
        assert_eq!(ctl.pvpve_writes(), 1);
        // The expedition starts for the Roundtable player too: 1 -> 2 (the game) -> 29 (us).
        f_set(&f, 0, 2);
        ctl.pvpve_tick();
        assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 29, 29));
        assert_eq!(ctl.pvpve_writes(), 2);
        // The original (2) was captured, so F12 can restore it.
        assert_eq!(ctl.originals.get(&f.addr(0)), Some(&2));
        // A manual experiment value (6) is never fought.
        f_set(&f, 1, 6);
        ctl.pvpve_tick();
        assert_eq!(f.team(1), 6);
        assert_eq!(ctl.pvpve_writes(), 2);
        // Other team values work too.
        let g = Fake::new();
        let mut ctl13 = g.ctl_with(&pvpve_cfg(13));
        ctl13.pvpve_tick();
        assert_eq!((g.team(0), g.team(1), g.team(2)), (13, 13, 13));
        assert_eq!(ctl13.pvpve_writes(), 3);
        assert_eq!(ctl13.player_count_text(), "3");
    }

    #[test]
    fn pvpve_runs_from_tick_on_its_own_interval_and_never_when_off() {
        let f = Fake::new();
        let mut cfg = pvpve_cfg(29);
        cfg.pvpve_interval_ms = 5000;
        let mut ctl = f.ctl_with(&cfg);
        ctl.tick();
        assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 29, 29));
        // Not due again for 5 s: a value reset to 2 is left alone by an immediate second tick.
        f_set(&f, 1, 2);
        ctl.tick();
        assert_eq!(f.team(1), 2);
        ctl.next_pvpve = Instant::now();
        ctl.tick();
        assert_eq!(f.team(1), 29);

        // Mode off (the default config): nothing is ever written; the checkbox and F11 cannot turn it on.
        let g = Fake::new();
        let mut off = g.ctl();
        assert_eq!(off.pvpve_state(), PvpveState::Off);
        off.tick();
        off.set_pvpve(true, "console");
        assert_eq!(off.pvpve_state(), PvpveState::Off);
        off.next_pvpve = Instant::now();
        off.tick();
        assert_eq!(off.pvpve_writes(), 0);
        assert_eq!((g.team(0), g.team(1), g.team(2)), (2, 2, 2));
    }

    #[test]
    fn pause_and_resume_from_the_console() {
        let f = Fake::new();
        let mut ctl = f.ctl_with(&pvpve_cfg(29));
        ctl.pvpve_tick();
        assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 29, 29));

        // `pvp 0`: pauses; the console then restores every player with a captured original (2), like the old F12. The mode must
        // not write the value again while paused.
        ctl.set_pvpve(false, "console");
        for i in 0..3 {
            ctl.restore_for(f.addr(i), "console");
        }
        assert_eq!(ctl.pvpve_state(), PvpveState::Paused);
        assert_eq!((f.team(0), f.team(1), f.team(2)), (2, 2, 2));
        ctl.next_pvpve = Instant::now();
        ctl.tick();
        assert_eq!((f.team(0), f.team(1), f.team(2)), (2, 2, 2));
        let writes_when_paused = ctl.pvpve_writes();

        // `pvp 1` resumes it, and the next tick applies again.
        ctl.set_pvpve(true, "console");
        assert_eq!(ctl.pvpve_state(), PvpveState::On);
        ctl.tick();
        assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 29, 29));
        assert_eq!(ctl.pvpve_writes(), writes_when_paused + 3);

        // `pvp 0` pauses it again.
        ctl.set_pvpve(false, "console");
        f_set(&f, 0, 2);
        ctl.next_pvpve = Instant::now();
        ctl.tick();
        assert_eq!(f.team(0), 2);

        // Resumed, the next tick writes its own value to everyone again.
        ctl.set_pvpve(true, "console");
        assert_eq!(ctl.pvpve_state(), PvpveState::On);
        ctl.tick();
        assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 29, 29));

        // The state survives a WorldChrMan change; a paused mode stays paused.
        ctl.set_pvpve(false, "console");
        ctl.set_instance(0);
        assert_eq!(ctl.pvpve_state(), PvpveState::Paused);
        let mut snap = Snapshot::default();
        ctl.fill_snapshot(&mut snap);
        assert_eq!(
            (snap.pvpve, snap.pvpve_team, snap.pvpve_writes),
            (PvpveState::Paused, 29, ctl.pvpve_writes())
        );
        assert!(ctl.pvpve_summary().starts_with("PAUSED (team 29,"));
    }

    #[test]
    fn the_deprecated_alias_drives_the_same_mode() {
        let (cfg, _w) =
            crate::config::parse("dev_mode = true\nauto_team_all = 29\npvpve_scheme = \"all\"\n")
                .expect("parses");
        let f = Fake::new();
        let mut ctl = f.ctl_with(&cfg);
        assert_eq!(ctl.pvpve_state(), PvpveState::On);
        ctl.pvpve_tick();
        assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 29, 29));
    }

    #[test]
    fn player_mode_cannot_pause_or_resume() {
        // A player-mode config (parse forces pvpve on, rivals).
        let (cfg, _w) =
            crate::config::parse("pvpve = false\npvpve_scheme = \"all\"\n").expect("ok");
        let f = Fake::new();
        let mut ctl = f.ctl_with(&cfg);
        assert_eq!(ctl.pvpve_state(), PvpveState::On);
        ctl.set_pvpve(false, "console");
        assert_eq!(ctl.pvpve_state(), PvpveState::On);
    }

    #[test]
    fn pvpve_write_failures_count_as_nothing() {
        // Without vtables the gate refuses every write: no panic, no count, memory untouched.
        let f = Fake::new();
        let mut ctl = f.ctl_with(&pvpve_cfg(29));
        ctl.vts = PlayerVtables::default();
        ctl.pvpve_tick();
        assert_eq!(ctl.pvpve_writes(), 0);
        assert_eq!((f.team(0), f.team(1), f.team(2)), (2, 2, 2));
    }

    #[test]
    fn the_resolve_log_ignores_changing_evidence_but_not_changing_entries() {
        let f = Fake::new();
        let r1 = resolve(f.wcm.as_ptr() as usize, &vts()).expect("ok");
        let mut lg = ResolveLog::default();
        assert!(!lg.lines(&r1, 0, (0, 0)).is_empty());
        // Same entry bytes and players, but the evidence (e.g. an obfuscated pointer that differs on every read)
        // changes: nothing is logged, not even after the coalescing gap.
        let mut r2 = resolve(f.wcm.as_ptr() as usize, &vts()).expect("ok");
        for p in &mut r2.probes {
            for s in &mut p.signals {
                s.value = s.value.map(|v| v ^ 0x1234_5678);
            }
        }
        assert!(lg.lines(&r2, 1000, (0, 0)).is_empty());
        assert!(lg.lines(&r2, 60_000, (0, 0)).is_empty());
        // A different player set is a real change.
        let mut r3 = resolve(f.wcm.as_ptr() as usize, &vts()).expect("ok");
        r3.players.pop();
        assert!(!lg.lines(&r3, 120_000, (0, 0)).is_empty());
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
        // Persist on for the local player (nothing written to it): the selected candidate.
        ctl.set_persist_for(c, true, "console");
        assert_eq!(ctl.persist.get(&c).map(|p| p.value), Some(6));
        assert_eq!(ctl.persist.len(), 2);
        assert!(!ctl.persist.contains_key(&b));
        assert_eq!(
            ctl.poll_interval(Duration::from_millis(500)),
            ctl.persist_every
        );

        // A tick applies each player's own value to that player alone.
        f_set(&f, 0, 2); // the game "reverts" a
        ctl.persist_tick();
        assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 2, 6));
        // The game reverts c: counted for c only, re-applied.
        f_set(&f, 2, 2);
        ctl.persist_tick();
        assert_eq!(f.team(2), 6);
        assert_eq!(ctl.persist[&c].tracker.total_reverts(), 1);
        assert_eq!(
            ctl.persist[&a].tracker.total_reverts(),
            0,
            "a: only its first application so far"
        );
        let mut snap = Snapshot::default();
        ctl.fill_snapshot(&mut snap);
        let by_addr = |x: usize| {
            snap.players
                .iter()
                .find(|p| p.addr == x)
                .cloned()
                .expect("row")
        };
        assert_eq!(by_addr(a).persist, Some(29));
        assert_eq!(by_addr(b).persist, None);
        assert_eq!(by_addr(c).persist, Some(6));
        assert!(
            snap.persist,
            "the global checkbox shows the local player's persist"
        );
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
        assert_eq!(
            ctl.poll_interval(Duration::from_millis(500)),
            Duration::from_millis(500)
        );
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
        assert_eq!(snap.local_basis, Basis::FirstEntry.label());

        // Overlay button: select 29 and write it. The byte changes, the original is captured.
        ctl.set_and_write(29, "overlay");
        assert_eq!(player[CHR_TEAM], 29);
        assert_eq!(ctl.cycler.current(), 29);
        ctl.next_live = Instant::now();
        ctl.fill_snapshot(&mut snap);
        assert_eq!(
            (snap.team_type, snap.original, snap.selected),
            (Some(29), Some(2), 29)
        );
        assert_eq!(snap.players[0].original, Some(2));

        // A value that is not a candidate is refused and nothing is written.
        ctl.set_and_write(99, "overlay");
        assert_eq!((player[CHR_TEAM], ctl.cycler.current()), (29, 29));

        // Persist toggle shows up in the snapshot; the revert window is only reported while persisting.
        ctl.set_persist_for(pa as usize, true, "console");
        ctl.fill_snapshot(&mut snap);
        assert!(snap.persist);
        ctl.set_persist_for(pa as usize, false, "console");
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

    // ---- T-026: rosters, the rivals scheme ----

    fn pid(n: u64) -> u64 {
        ((0x6000 + n) << 32) | 0xFFFF_FFFF
    }

    type Synth = (Vec<(usize, Option<u64>)>, RosterView);

    /// Synthetic players: address 0x1000 * (i + 1), id = pid(ordinals[i]); local = index `local`.
    fn synth(ordinals: &[u64], local: usize, size: usize) -> Synth {
        let players: Vec<(usize, Option<u64>)> = ordinals
            .iter()
            .enumerate()
            .map(|(i, &n)| (0x1000 * (i + 1), Some(pid(n))))
            .collect();
        let view = build_view(&players, Some(players[local].0), size);
        (players, view)
    }

    /// Plan for synthetic players that all sit at team_type 2 and were never written.
    fn plan_all_at_2(
        scheme: Scheme,
        players: &[(usize, Option<u64>)],
        view: &RosterView,
    ) -> PvpvePlan {
        let live: Vec<(usize, Option<u8>)> = players.iter().map(|&(a, _)| (a, Some(2))).collect();
        plan_pvpve(scheme, view, &live, &HashSet::new())
    }

    #[test]
    fn rivals_scheme_writes_only_rivals_for_every_size_and_count() {
        for n in [2usize, 3, 4, 6] {
            for size in 1..=3usize {
                for local in 0..n {
                    let ords: Vec<u64> = (1..=n as u64).collect();
                    let (players, view) = synth(&ords, local, size);
                    let plan = plan_all_at_2(Scheme::Rivals, &players, &view);
                    // Ids are sorted ordinals, chunked by size.
                    let local_team = local / size;
                    let expect: Vec<usize> = (0..n)
                        .filter(|i| i / size != local_team)
                        .map(|i| players[i].0)
                        .collect();
                    assert_eq!(plan.writes, expect, "n={n} size={size} local={local}");
                    assert!(!plan.writes.contains(&players[local].0));
                    assert!(plan.restores.is_empty());
                    for (i, &(addr, _)) in players.iter().enumerate() {
                        let want = if i == local {
                            Relation::Local
                        } else if i / size == local_team {
                            Relation::Ally
                        } else {
                            Relation::Rival
                        };
                        assert_eq!(view.relation[&addr], want);
                        assert_eq!(usize::from(view.team_no[&players[i].0]), i / size);
                    }
                }
            }
        }
    }

    #[test]
    fn rivals_scheme_only_writes_value_2_and_never_allies() {
        // Teams [1,2] [3,4]; local = id 1.
        let (players, view) = synth(&[1, 2, 3, 4], 0, 2);
        let live = vec![
            (players[0].0, Some(2)), // local
            (players[1].0, Some(2)), // ally
            (players[2].0, Some(1)), // rival with the Roundtable value
            (players[3].0, Some(2)), // rival
        ];
        let plan = plan_pvpve(Scheme::Rivals, &view, &live, &HashSet::new());
        assert_eq!(plan.writes, vec![players[3].0]);
        let live = vec![(players[2].0, None), (players[3].0, Some(29))];
        let plan = plan_pvpve(Scheme::Rivals, &view, &live, &HashSet::new());
        assert!(plan.writes.is_empty());
    }

    #[test]
    fn mm_team_size_override_changes_the_rosters_only_when_set() {
        // MM-2: the size passed to `build_view` comes from `mm::pick_team_size(configured, override)`.
        let configured = 1;
        let (players, _) = synth(&[1, 2, 3, 4], 0, configured);
        let at = |ov: usize| {
            let size = crate::mm::pick_team_size(configured, ov);
            build_view(&players, Some(players[0].0), size)
        };
        // No override (queue off, or not MATCH READY): free-for-all as configured.
        assert_eq!(at(0).relation[&players[1].0], Relation::Rival);
        // Duo match: the lowest two ids are allies, the rest rivals.
        let duo = at(2);
        assert_eq!(duo.relation[&players[1].0], Relation::Ally);
        assert_eq!(duo.relation[&players[2].0], Relation::Rival);
        // Solo match over a configured team_size of 2 forces free-for-all.
        let (players2, _) = synth(&[1, 2, 3, 4], 0, 2);
        let solo = build_view(
            &players2,
            Some(players2[0].0),
            crate::mm::pick_team_size(2, 1),
        );
        assert_eq!(solo.relation[&players2[1].0], Relation::Rival);
    }
    #[test]
    fn a_rival_turning_ally_is_planned_for_restore() {
        let (players, view) = synth(&[1, 2, 3], 0, 1);
        let written: HashSet<usize> = [players[1].0, players[2].0].into_iter().collect();
        let live: Vec<_> = players.iter().map(|&(a, _)| (a, Some(29))).collect();
        let plan = plan_pvpve(Scheme::Rivals, &view, &live, &written);
        assert!(plan.restores.is_empty() && plan.writes.is_empty());
        // Same players with team_size 2: id 2 is now an ally of LOCAL.
        let (_, view2) = synth(&[1, 2, 3], 0, 2);
        let plan = plan_pvpve(Scheme::Rivals, &view2, &live, &written);
        assert_eq!(plan.restores, vec![players[1].0]);
        assert!(plan.writes.is_empty());
    }

    #[test]
    fn scheme_all_is_unchanged_and_ignores_the_rosters() {
        let (players, view) = synth(&[1, 2, 3, 4], 0, 2);
        let plan = plan_all_at_2(Scheme::All, &players, &view);
        assert_eq!(plan.writes, players.iter().map(|p| p.0).collect::<Vec<_>>());
        let live = vec![(players[0].0, Some(1)), (players[1].0, Some(2))];
        let written: HashSet<usize> = [players[0].0].into_iter().collect();
        let plan = plan_pvpve(Scheme::All, &view, &live, &written);
        assert_eq!(plan.writes, vec![players[1].0]);
        assert!(plan.restores.is_empty());
    }

    #[test]
    fn unreadable_or_duplicate_ids_fail_open_to_free_for_all() {
        let players = vec![
            (0x1000, Some(pid(1))),
            (0x2000, None),
            (0x3000, Some(pid(3))),
        ];
        let view = build_view(&players, Some(0x1000), 2);
        assert!(view.warning.is_some());
        assert!(view.summary.starts_with("teams: WARNING"));
        assert_eq!(view.relation[&0x1000], Relation::Local);
        assert_eq!(view.relation[&0x2000], Relation::Rival);
        assert_eq!(view.relation[&0x3000], Relation::Rival);
        assert!(view.team_no.is_empty());
        let plan = plan_all_at_2(Scheme::Rivals, &players, &view);
        assert_eq!(plan.writes, vec![0x2000, 0x3000]);

        let dup = vec![(0x1000, Some(pid(1))), (0x2000, Some(pid(1)))];
        let view = build_view(&dup, Some(0x1000), 1);
        assert!(view.warning.as_deref().unwrap_or("").contains("same id"));
        assert_eq!(view.relation[&0x2000], Relation::Rival);

        // An id of 0 counts as unreadable.
        let zero = vec![(0x1000, Some(pid(1))), (0x2000, Some(0))];
        assert!(build_view(&zero, Some(0x1000), 1).warning.is_some());

        // No local player: nobody is written in the rivals scheme.
        let none = build_view(&dup, None, 1);
        assert!(plan_all_at_2(Scheme::Rivals, &dup, &none).writes.is_empty());
    }

    #[test]
    fn roster_summary_matches_the_log_format_and_key_tracks_changes() {
        let players = vec![
            (0x1000, Some(0x6001_FFFF_FFFFu64)),
            (0x2000, Some(0x6002_FFFF_FFFF)),
        ];
        let v = build_view(&players, Some(0x1000), 1);
        assert_eq!(
            v.summary,
            "teams: team 0 = [0x6001FFFFFFFF(LOCAL)], team 1 = [0x6002FFFFFFFF]"
        );
        let v2 = build_view(&players, Some(0x1000), 2);
        assert_eq!(
            v2.summary,
            "teams: team 0 = [0x6001FFFFFFFF(LOCAL), 0x6002FFFFFFFF]"
        );
        assert_eq!(v.key, build_view(&players, Some(0x1000), 1).key);
        assert_ne!(v.key, build_view(&players, Some(0x2000), 1).key);
        assert_eq!(
            crate::overlay::shared::short_id(Some(0x6001_FFFF_FFFF)),
            "0x6001.."
        );
    }

    /// Set the per-player id of fake object `i`.
    fn set_id(f: &mut Fake, i: usize, ord: u64) {
        f.objs[i][CHR_P2P_HANDLE..CHR_P2P_HANDLE + 8].copy_from_slice(&pid(ord).to_le_bytes());
    }

    fn rivals_cfg(size: u64) -> Config {
        Config {
            team_size: size,
            pvpve_scheme: "rivals".to_string(),
            ..pvpve_cfg(29)
        }
    }

    #[test]
    fn rivals_tick_on_fake_memory_spares_local_and_allies_and_restores() {
        // Entry order: objs[2] (LOCAL), objs[0], objs[1].
        let mut f = Fake::new();
        set_id(&mut f, 2, 1);
        set_id(&mut f, 0, 4);
        set_id(&mut f, 1, 2);
        // Teams [1,2] [4]: objs[1] is an ally, objs[0] a rival.
        let mut ctl = f.ctl_with(&rivals_cfg(2));
        ctl.pvpve_tick();
        assert_eq!((f.team(2), f.team(0), f.team(1)), (2, 29, 2));
        assert_eq!(ctl.pvpve_writes(), 1);
        // Id of objs[1] becomes 7: teams [1,4] [7]; objs[0] is an ally now, objs[1] a rival.
        set_id(&mut f, 1, 7);
        ctl.pvpve_tick();
        assert_eq!((f.team(2), f.team(0), f.team(1)), (2, 2, 29));
        // Pausing restores what the tick wrote.
        ctl.set_pvpve(false, "test");
        assert_eq!((f.team(2), f.team(0), f.team(1)), (2, 2, 2));
        assert!(ctl.pvpve_written.is_empty());
        // Resuming writes again.
        ctl.set_pvpve(true, "test");
        ctl.pvpve_tick();
        assert_eq!(f.team(1), 29);
    }

    #[test]
    fn scheme_all_tick_on_fake_memory_writes_everyone_with_value_2() {
        let mut f = Fake::new();
        set_id(&mut f, 2, 1);
        set_id(&mut f, 0, 2);
        set_id(&mut f, 1, 3);
        let mut cfg = rivals_cfg(3);
        cfg.pvpve_scheme = "all".to_string();
        let mut ctl = f.ctl_with(&cfg);
        ctl.pvpve_tick();
        assert_eq!((f.team(2), f.team(0), f.team(1)), (29, 29, 29));
        assert_eq!(ctl.pvpve_writes(), 3);
        assert!(ctl.pvpve_written.is_empty());
    }

    #[test]
    fn unreadable_ids_on_fake_memory_write_every_remote_and_not_local() {
        // All ids are zero, which counts as unreadable.
        let f = Fake::new();
        let mut ctl = f.ctl_with(&rivals_cfg(2));
        ctl.pvpve_tick();
        assert_eq!((f.team(2), f.team(0), f.team(1)), (2, 29, 29));
    }

    // ---- T-032: inert downed switch, MARK events, snap-back test on fake memory ----

    fn dev_cfg() -> Config {
        Config {
            dev_mode: true,
            ..Config::default()
        }
    }

    fn put3(f: &mut Fake, i: usize, off: usize, v: [f32; 3]) {
        for (k, x) in v.iter().enumerate() {
            f.objs[i][off + k * 4..off + k * 4 + 4].copy_from_slice(&x.to_le_bytes());
        }
    }

    fn get3(f: &Fake, i: usize, off: usize) -> [f32; 3] {
        let b = &f.objs[i][off..off + 12];
        let g = |k: usize| f32::from_le_bytes([b[k], b[k + 1], b[k + 2], b[k + 3]]);
        [g(0), g(4), g(8)]
    }

    #[test]
    fn snap_back_writes_only_the_selected_candidate_of_the_local_player() {
        let mut f = Fake::new();
        // LOCAL is objs[2]; give every object a position at 0x6C0 and 0x6D0.
        for i in 0..3 {
            put3(&mut f, i, 0x6C0, [10.0 + i as f32, 0.0, 0.0]);
            put3(&mut f, i, 0x6D0, [20.0 + i as f32, 0.0, 0.0]);
        }
        let mut ctl = f.ctl_with(&dev_cfg());
        // Nothing saved yet: F6 does nothing.
        ctl.pos_write("F6");
        assert_eq!(get3(&f, 2, 0x6C0), [12.0, 0.0, 0.0]);
        ctl.pos_save("F7");
        assert!(ctl.pos.has_saved());
        for i in 0..3 {
            put3(&mut f, i, 0x6C0, [100.0 + i as f32, 1.0, 1.0]);
            put3(&mut f, i, 0x6D0, [200.0 + i as f32, 1.0, 1.0]);
        }
        // Candidate 1 (PlayerIns+0x6C0) is written on the LOCAL player only; 0x6D0 is not.
        ctl.pos_write("F6");
        assert_eq!(get3(&f, 2, 0x6C0), [12.0, 0.0, 0.0]);
        assert_eq!(get3(&f, 2, 0x6D0), [202.0, 1.0, 1.0]);
        for i in 0..2 {
            assert_eq!(get3(&f, i, 0x6C0), [100.0 + i as f32, 1.0, 1.0]);
        }
        assert_eq!(ctl.pos_follow.len(), 3);
        // F9 selects the second candidate; F6 writes it.
        ctl.pos_cycle();
        ctl.pos_write("F6");
        assert_eq!(get3(&f, 2, 0x6D0), [22.0, 0.0, 0.0]);
        // The follow-ups only read.
        for e in ctl.pos_follow.iter_mut() {
            e.0 = Instant::now();
        }
        ctl.pos_follow_tick();
        assert!(ctl.pos_follow.is_empty());
        // A new WorldChrMan instance forgets the saved values and the pending follow-ups.
        ctl.set_instance(0);
        assert!(!ctl.pos.has_saved());
        assert!(ctl.pos_follow.is_empty());
    }

    #[test]
    fn snap_back_is_refused_without_vtables_and_absent_in_the_player_build() {
        let mut f = Fake::new();
        put3(&mut f, 2, 0x6C0, [1.0, 2.0, 3.0]);
        // Player build: every call is a no-op.
        let mut p = f.ctl_with(&Config::default());
        p.pos_save("F7");
        put3(&mut f, 2, 0x6C0, [9.0, 9.0, 9.0]);
        p.pos_write("F6");
        p.pos_cycle();
        assert_eq!(get3(&f, 2, 0x6C0), [9.0, 9.0, 9.0]);
        assert!(!p.pos.has_saved());
        assert!(p.pos_follow.is_empty());
        // Dev build, but the PlayerIns vtable check fails (no vtables known): nothing is saved or written.
        let mut d = f.ctl_with(&dev_cfg());
        d.vts = PlayerVtables::default();
        d.pos_save("F7");
        assert!(!d.pos.has_saved());
        // The object's vtable changed after the resolve: refused by the re-check.
        let mut d = f.ctl_with(&dev_cfg());
        d.pos_save("F7");
        assert!(d.pos.has_saved());
        f.objs[2][..8].copy_from_slice(&(VT_ENEMY as u64).to_le_bytes());
        d.pos_write("F6");
        assert_eq!(get3(&f, 2, 0x6C0), [9.0, 9.0, 9.0]);
    }
}
