//! The player list: WorldChrMan's inline ChrSet, PlayerIns recognition by RTTI vtable, the LOCAL decision (first valid entry,
//! T-020) and the change log (T-008/T-015). Read-only. Layout: MEMORY_MAP §2-3.

use std::collections::HashSet;

use super::*;
use crate::game::mem::{is_ptr, va_rva, RegionCache};
use crate::game::rtti::Rtti;
use crate::overlay::shared::{PlayerRow, Role};

// Offsets: game::layout (R2), MEMORY_MAP §2-3. The local names below are the ones this module has always used.
pub(crate) use crate::game::layout::CHRSET_ENTRY_SIZE as ENTRY_SIZE;
pub(crate) use crate::game::layout::CHR_PLAYER_ID as CHR_P2P_HANDLE;
pub(crate) use crate::game::layout::CHR_TYPE as CHR_TYPE70;
/// The T-015 evidence name for `PlayerIns+0x5A0`, which is the PlayerGameData pointer (see `layout::PLAYER_GAMEDATA`).
pub(crate) use crate::game::layout::PLAYER_GAMEDATA as PLAYER_SESSION_HOLDER;
pub(crate) use crate::game::layout::{
    CHRSET_CAPACITY, CHRSET_ENTRIES, CHR_ID, CHR_NET_AUTHORITY, CHR_TEAM, PLAYER_SESSION_ENTRY,
    PLAYER_VTABLE_RVA, WCM_CHRSET,
};
/// Largest ChrSet capacity we accept (6 expected).
pub(super) const MAX_CAPACITY: u32 = 16;

/// ChrSet change lines are coalesced to at most one burst per this long (a state byte in the entry can blink).
pub(super) const RESOLVE_LOG_MIN_GAP_MS: u64 = 5000;

// ---------------------------------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------------------------------

pub(super) fn le32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(off..off.checked_add(4)?)?.try_into().ok()?,
    ))
}

pub(super) fn le64(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        b.get(off..off.checked_add(8)?)?.try_into().ok()?,
    ))
}

pub(super) fn hex16(raw: &[u8; 16]) -> String {
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
    pub(super) fn text(&self) -> String {
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

    pub(super) fn sorted(&self) -> Vec<usize> {
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
pub(super) type ResolveKey = (Vec<[u8; ENTRY_SIZE]>, Vec<usize>);

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
    pub(super) fn key(&self) -> ResolveKey {
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
    pub(super) fn index_of(&self, addr: usize) -> Option<usize> {
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
    pub(super) fn rows(&self, team: &mut dyn FnMut(usize) -> Option<u8>) -> Vec<PlayerRow> {
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
pub(super) fn is_player_obj(rc: &mut RegionCache, vts: &PlayerVtables, p: usize) -> bool {
    is_ptr(p) && rc.read::<usize>(p).is_some_and(|vt| vts.contains(vt))
}

/// The evidence signals of one PlayerIns (log only since T-020). ER offsets shifted by NR's +8, see the consts;
/// reads are VirtualQuery-guarded and an unreadable value is recorded as `None`.
pub(super) fn read_signals(
    rc: &mut RegionCache,
    p: usize,
    entry: &[u8; ENTRY_SIZE],
) -> Vec<Signal> {
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
    pub(super) last: Option<ResolveKey>,
    pub(super) last_logged_ms: u64,
    pub(super) coalesced: u32,
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
