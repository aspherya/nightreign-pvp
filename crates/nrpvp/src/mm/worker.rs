//! MM-2: the queue worker. It runs inside the Steam thread (`steam::probe`), never on the game loop: real Steam, the safety
//! guard, commands from the hub (overlay / hotkeys), Seamless Co-op party tracking, and publishing status plus the team override.

use std::time::{Duration, Instant};

use super::machine::{build_party, idt, Machine, MachineCfg};
use super::{guard, Cmd, Status};
use crate::steam::probe::Event;
use crate::steam::queue::{LobbyText, QueueSteam, RealSteam, ScLobby, KEY_SC_VERSION};
use crate::steam::LobbyApi;

/// The Seamless Co-op version the mod is built against (SC 1.1.3, TECHNICAL_NOTES §1). Used for the `sc=` tag until an SC lobby has
/// been seen and its `nrsc_lobby_version` data can be read.
pub(crate) const SC_PINNED_VERSION: &str = "1.1.3";
/// Fallback for the `game=` tag when the executable's FileVersion could not be read.
// TODO MM-4: derive a proper build id (FileVersion is what the startup banner logs; revisit if it proves unstable).
pub(crate) const GAME_UNKNOWN: &str = "unknown";

const SYNC_EVERY: Duration = Duration::from_secs(1);
const GUARD_CACHE: Duration = Duration::from_secs(5);

/// What the startup code hands the worker.
pub(crate) struct Setup {
    /// `mm_match_players`.
    pub need: u32,
    /// MM-3: `mm_join` (dev only) and `mm_join_auto`.
    pub join: bool,
    pub join_auto: super::join::JoinAuto,
    /// The game build string (the exe FileVersion the banner logs), or [`GAME_UNKNOWN`].
    pub game: String,
}

pub(crate) struct Worker {
    steam: RealSteam,
    machine: Machine,
    sc_seen: Option<u64>,
    refusal: Option<String>,
    last_status: Option<Status>,
    last_override: Option<usize>,
    next_sync: Instant,
    next_env: Instant,
    guard_cache: Option<(Instant, Result<(), String>)>,
    exports_ok: bool,
}

impl Worker {
    /// Called on the Steam thread once Steam is initialised and the listener is registered.
    pub(crate) fn start(s: Setup) -> Option<Worker> {
        let Some(steam) = RealSteam::new() else {
            tracing::warn!("mm_queue: ISteamMatchmaking is unavailable; the queue cannot run");
            return None;
        };
        let exports_ok = steam.missing.is_empty();
        if !exports_ok {
            tracing::warn!("mm_queue: these Steam exports did not resolve, queuing is refused: {:?}", steam.missing);
        }
        tracing::info!(
            "mm_queue: worker started: own Steam id {}, mm_match_players={}, game tag {:?}, sc tag {:?} (pinned until an SC lobby is seen)",
            idt(steam.own_id()),
            s.need,
            s.game,
            SC_PINNED_VERSION
        );
        let machine = Machine::new(MachineCfg {
            need: s.need,
            game: s.game,
            sc: SC_PINNED_VERSION.to_string(),
            join: s.join,
            join_auto: s.join_auto,
        });
        if s.join {
            tracing::info!(
                "mm_join: enabled, mm_join_auto={:?}; these join exports did not resolve (their methods are refused): {:?}",
                s.join_auto.label(),
                steam.join_missing
            );
        }
        Some(Worker {
            steam,
            machine,
            sc_seen: None,
            refusal: None,
            last_status: None,
            last_override: None,
            next_sync: Instant::now(),
            next_env: Instant::now(),
            guard_cache: None,
            exports_ok,
        })
    }

    pub(crate) fn on_event(&mut self, ev: &Event) {
        self.machine.on_event(ev);
        let lobby = match ev {
            Event::LobbyEnter { lobby, .. } | Event::LobbyDataUpdate { lobby, .. } | Event::LobbyChatUpdate { lobby, .. } => *lobby,
            _ => 0,
        };
        if lobby == 0 {
            return;
        }
        // Seamless Co-op's lobby carries `nrsc_lobby_type`. Remember it (read only) and its version.
        if ScLobby::verify(&self.steam, lobby).is_some() {
            if self.sc_seen != Some(lobby) {
                tracing::info!("mm_queue: Seamless Co-op lobby seen: {}", idt(lobby));
                self.sc_seen = Some(lobby);
            }
            let v = self.steam.lobby_data(lobby, KEY_SC_VERSION);
            if !v.is_empty() {
                self.machine.set_sc_version(&v);
            }
        }
    }

    /// The Seamless Co-op lobby the local player is currently a member of, if any.
    fn current_sc(&mut self) -> Option<(ScLobby, u64, u32)> {
        let id = self.sc_seen?;
        let sc = ScLobby::verify(&self.steam, id);
        let n = self.steam.member_count(id);
        let own = self.steam.own_id();
        let member = (0..n.clamp(0, 64)).any(|i| self.steam.member_by_index(id, i) == own);
        match sc {
            Some(sc) if n > 0 && member => Some((sc, self.steam.owner(id), n as u32)),
            _ => {
                tracing::info!("mm_queue: Seamless Co-op lobby {} is gone or we are not in it", idt(id));
                self.sc_seen = None;
                None
            }
        }
    }

    fn guard(&mut self, now: Instant) -> Result<(), String> {
        if let Some((t, r)) = &self.guard_cache {
            if now.saturating_duration_since(*t) < GUARD_CACHE {
                return r.clone();
            }
        }
        let r = if !self.exports_ok {
            Err("required Steam exports are missing (see the log)".to_string())
        } else if self.steam.own_id() == 0 {
            Err("own Steam id is unavailable".to_string())
        } else {
            guard::check_process()
        };
        self.guard_cache = Some((now, r.clone()));
        r
    }

    /// MM-3: give the machine the Seamless Co-op lobby we are in, the safety guard result and the local phase (cheap; throttled
    /// to every 500 ms unless `force`). Only while a match is ready and `mm_join` is on.
    fn refresh_env(&mut self, now: Instant, force: bool) {
        if !self.machine.wants_sc() {
            return;
        }
        if !force && now < self.next_env {
            return;
        }
        self.next_env = now + Duration::from_millis(500);
        let sc = self.current_sc().map(|(sc, _, _)| sc);
        let guard = self.guard(now);
        self.machine.set_env(sc, guard, super::local_phase());
    }

    fn refuse(&mut self, why: String) {
        tracing::warn!("mm_queue: REFUSED: {why}");
        self.refusal = Some(why);
    }

    fn command(&mut self, c: Cmd, now: Instant) {
        match c {
            Cmd::Join(j) => {
                // Refresh the inputs the button needs, then run it (the machine checks the guard and the state).
                self.refresh_env(now, true);
                tracing::info!("mm_join: button {j:?}");
                self.machine.join_cmd(&mut self.steam, j, now);
            }
            Cmd::Leave => {
                self.refusal = None;
                self.machine.leave(&mut self.steam, "user pressed Leave", now);
            }
            Cmd::Queue(mode) => {
                if !self.machine.is_idle() {
                    tracing::info!("mm_queue: Queue {} ignored: not idle", mode.label());
                    return;
                }
                self.refusal = None;
                if let Err(e) = self.guard(now) {
                    self.refuse(e);
                    return;
                }
                let own = self.steam.own_id();
                let sc = self.current_sc();
                match build_party(mode, own, sc) {
                    Ok(party) => self.machine.queue(&mut self.steam, mode, party, now),
                    Err(e) => self.refuse(e),
                }
            }
        }
    }

    /// One pass: commands, the machine, party following, override, status. Called every ~100 ms.
    pub(crate) fn tick(&mut self, now: Instant) {
        for c in super::take_cmds() {
            tracing::info!("mm_queue: command {c:?}");
            self.command(c, now);
        }
        self.refresh_env(now, false);
        self.machine.tick(&mut self.steam, now);
        if now >= self.next_sync {
            self.next_sync = now + SYNC_EVERY;
            if self.sc_seen.is_some() && self.guard(now).is_ok() {
                if let Some((sc, _, _)) = self.current_sc() {
                    self.machine.sync_party(&mut self.steam, &sc, now);
                }
            }
        }
        let ov = self.machine.team_override();
        if ov != self.last_override {
            tracing::info!("mm_queue: team size override {:?} -> {:?}", self.last_override, ov);
            super::set_team_override(ov);
            self.last_override = ov;
        }
        let mut st = self.machine.status();
        if let (Some(r), true) = (&self.refusal, self.machine.is_idle()) {
            st.line = format!("Queue: REFUSED - {r}");
        }
        if self.last_status.as_ref() != Some(&st) {
            tracing::info!("mm_queue: status: {}", st.line);
            super::publish(st.clone());
            self.last_status = Some(st);
        }
    }
}
