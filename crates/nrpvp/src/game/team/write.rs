//! The one team_type write (gated on the PlayerIns vtable), persist re-application (T-015), the candidate list and the
//! per-player watcher lines (T-008).

use std::collections::HashMap;

use super::*;
use crate::game::mem::{self, is_ptr, RegionCache};

/// Persist summary period.
pub(super) const PERSIST_SUMMARY_MS: u64 = 1000;

/// The configured team-type candidates and which one is selected (cycle hotkey / overlay buttons).
pub(crate) struct Cycler {
    pub(super) list: Vec<u8>,
    pub(super) idx: usize,
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
    pub(super) armed: bool,
    pub(super) last_target: usize,
    pub(super) last_selected: u8,
    pub(super) last_seen: u8,
    pub(super) window_reverts: u32,
    /// Reverts counted in the last completed one-second window (shown by the overlay).
    pub(super) last_window_reverts: u32,
    pub(super) total_reverts: u64,
    pub(super) window_start_ms: u64,
    pub(super) last_fail_log_ms: Option<u64>,
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
    pub(super) seen: HashMap<usize, Fields>,
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

pub(super) fn watch_line(addr: usize, who: &str, f: Fields, prev: Option<Fields>) -> String {
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

pub(super) fn read_fields(rc: &mut RegionCache, p: usize) -> Option<Fields> {
    Some(Fields {
        id: rc.read::<u32>(p + CHR_ID)?,
        v70: rc.read::<u32>(p + CHR_TYPE70)?,
        team: rc.read::<u8>(p + CHR_TEAM)?,
    })
}

/// Persist state of one player (T-015: persist is per player).
pub(super) struct PersistEntry {
    /// The value re-applied every tick.
    pub(super) value: u8,
    pub(super) tracker: PersistTracker,
    /// The first successful application was logged.
    pub(super) first_logged: bool,
}
