//! Team values of the players in the ChrSet (T-008, T-015, T-020, T-026): the controller the main loop drives.
//!
//! Split by R3 (2026-10-20) into one file per job:
//! - [`chrset`]: read the player list (WorldChrMan ChrSet), recognise PlayerIns by RTTI vtable, decide who is LOCAL (entry[0]).
//! - [`write`]: the one gated team_type write, persist re-application, the candidate list, the per-player watcher lines.
//! - [`roster`]: pure rules glue: rosters from the per-player ids and the "rivals only" plan (no game memory).
//! - this file: [`TeamCtl`], which ties them together each tick (PvPvE marking, persist, console commands, the snapshot).
//!
//! The write is a single u8 store at `PlayerIns+0x74`, only after the object's vtable was re-read and matched a PlayerIns-derived
//! class from RTTI ([`write_team_type`]). No hooks. Offsets: `game::layout` (MEMORY_MAP §2-4).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use super::mem::{va_rva, RegionCache};
use super::poswrite::{self, PosTest};
use super::rtti::Rtti;
use crate::config::{Config, Scheme};
use crate::mark::Marker;
use crate::overlay::shared::PlayerRow;

mod chrset;
mod manual;
mod postest;
mod pvpve;
mod roster;
#[cfg(test)]
mod tests;
mod view;
mod write;

pub(crate) use chrset::*;
pub(crate) use roster::*;
pub(crate) use write::*;

/// How often the overlay snapshot re-reads the players (T-013).
const LIVE_REFRESH: Duration = Duration::from_millis(100);

// ---------------------------------------------------------------------------------------------------
// Controller driven from the main loop
// ---------------------------------------------------------------------------------------------------

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
}
