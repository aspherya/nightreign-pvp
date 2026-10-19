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

/// MM-4: the world state in which the player has left the Roundtable: an expedition, or the world was unloaded.
pub(crate) fn away_from_roundtable(w: super::World) -> bool {
    matches!(w, super::World::Gone | super::World::Present(Some(super::join::Phase::Expedition)))
}

/// MM-4: why a queue request is refused for this world state (`None` = fine).
pub(crate) fn world_refusal(w: super::World) -> Option<&'static str> {
    match w {
        super::World::Never | super::World::Gone => Some("load into the Roundtable first"),
        super::World::Present(Some(super::join::Phase::Expedition)) => Some("go back to the Roundtable first"),
        super::World::Present(_) => None,
    }
}

const SYNC_EVERY: Duration = Duration::from_secs(1);
const GUARD_CACHE: Duration = Duration::from_secs(5);

/// What the startup code hands the worker.
pub(crate) struct Setup {
    /// Queue lobby size (`mm_max_players`).
    pub need: u32,
    /// MM-4: `mm_min_players`, `mm_fill_wait_s`.
    pub min: u32,
    pub fill_wait: Duration,
    /// MM-4: the player build (status lines in player words, quieter probe).
    pub friendly: bool,
    /// MM-4: no local-player read for this long means the world is unloaded (a few watcher periods).
    pub world_stale: Duration,
    /// MM-3: `mm_join` (dev only) and `mm_join_auto`.
    pub join: bool,
    pub join_auto: super::join::JoinAuto,
    /// MM-3e: `mm_host_create`.
    pub host_create: bool,
    /// MM-3f: `mm_quiet_invite`.
    pub quiet_invite: bool,
    /// MM-3d: `Some(type)` when `mm_sc_create` is on (dev only).
    pub sc_create: Option<crate::steam::queue::ScType>,
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
    /// MM-3d: the "create SC lobby (test)" button's state, `Some` only with `mm_sc_create`.
    sc_create: Option<super::sccreate::ScCreate>,
    /// MM-3e: the second SC lobby already logged as ignored.
    ignored_sc: Option<u64>,
    /// MM-3f: `mm_quiet_invite` is on.
    quiet: bool,
    /// MM-3f: the dev button's test window ends at this time.
    quiet_test_until: Option<Instant>,
    /// MM-3f: when the raw "wanted" condition was last true (for the linger after the SC lobby appears).
    quiet_raw_at: Option<Instant>,
    quiet_last_want: bool,
    /// MM-3f: the suppressed counter when the current test window started.
    quiet_seen_sup: u32,
    /// MM-4: see [`Setup::world_stale`].
    world_stale: Duration,
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
            "mm_queue: worker started: own Steam id {}, lobby size (mm_max_players)={}, mm_min_players={}, mm_fill_wait_s={}, player build={}, game tag {:?}, sc tag {:?} (pinned until an SC lobby is seen)",
            idt(steam.own_id()),
            s.need,
            s.min,
            s.fill_wait.as_secs(),
            s.friendly,
            s.game,
            SC_PINNED_VERSION
        );
        let machine = Machine::new(MachineCfg {
            need: s.need,
            min: s.min,
            fill_wait: s.fill_wait,
            friendly: s.friendly,
            game: s.game,
            sc: SC_PINNED_VERSION.to_string(),
            join: s.join,
            join_auto: s.join_auto,
            host_create: s.host_create && s.join,
            quiet_invite: s.quiet_invite && s.join,
        });
        if s.join {
            tracing::info!(
                "mm_join: enabled, mm_join_auto={:?}; these join exports did not resolve (their methods are refused): {:?}",
                s.join_auto.label(),
                steam.join_missing
            );
            if s.host_create {
                tracing::info!("mm_host_create: enabled: the host's mod creates the Seamless Co-op lobby at MATCH READY");
            }
        }
        if let Some(ty) = s.sc_create {
            tracing::info!("mm_sc_create: enabled, type {}; dev button \"create SC lobby (test)\" in the Queue block", ty.label());
        }
        if s.quiet_invite && s.join {
            tracing::info!("mm_quiet_invite: enabled: the Steam invite window is skipped while we are host at MATCH READY without a Seamless Co-op lobby (or for 60 s after the dev button)");
        }
        Some(Worker {
            world_stale: s.world_stale,
            quiet: s.quiet_invite && s.join,
            quiet_test_until: None,
            quiet_raw_at: None,
            quiet_last_want: false,
            quiet_seen_sup: 0,
            ignored_sc: None,
            sc_create: s.sc_create.map(super::sccreate::ScCreate::new),
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
        if let Some(c) = self.sc_create.as_mut() {
            c.on_event(ev);
        }
        let lobby = match ev {
            Event::LobbyEnter { lobby, .. } | Event::LobbyDataUpdate { lobby, .. } | Event::LobbyChatUpdate { lobby, .. } => *lobby,
            _ => 0,
        };
        if lobby == 0 {
            return;
        }
        // Seamless Co-op's lobby carries `nrsc_lobby_type`. Remember it (read only) and its version.
        if ScLobby::verify(&self.steam, lobby).is_some() {
            // MM-3e: a ready match is bound to one SC lobby (the one the gate published or created). While that one is alive a
            // second SC lobby (e.g. the host opened Invite Members anyway) is ignored.
            if let Some(p) = self.machine.preferred_sc_alive(&self.steam) {
                if p != lobby {
                    if self.ignored_sc != Some(lobby) {
                        tracing::info!("mm_host_create: second SC lobby ignored: {} (env.sc stays on {})", idt(lobby), idt(p));
                        self.ignored_sc = Some(lobby);
                    }
                    return;
                }
            }
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
        // MM-3e: stay on the lobby the match is bound to, whatever was seen last.
        if let Some(p) = self.machine.preferred_sc_alive(&self.steam) {
            if self.sc_seen != Some(p) {
                tracing::info!("mm_queue: Seamless Co-op lobby follows the match's lobby: {}", idt(p));
                self.sc_seen = Some(p);
            }
        }
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
        self.machine.set_env(sc, guard, super::local_phase(), super::local_players());
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
            Cmd::ScCreate => {
                if self.sc_create.is_none() {
                    tracing::info!("mm_sc_create: button ignored: mm_sc_create is off");
                    return;
                }
                let guard = self.guard(now);
                let in_sc = self.current_sc().map(|(sc, _, _)| sc.id());
                if let Some(c) = self.sc_create.as_mut() {
                    c.press(&mut self.steam, &guard, in_sc, now);
                }
            }
            Cmd::QuietTest => {
                if !self.quiet {
                    tracing::info!("mm_quiet_invite: button ignored: mm_quiet_invite is off");
                    return;
                }
                if !crate::steam::quietinvite::ready() {
                    tracing::warn!("mm_quiet_invite: button pressed but the hook is not installed (Steam not ready or the install failed, see above)");
                }
                self.quiet_test_until = Some(now + crate::steam::quietinvite::TEST_WINDOW);
                self.quiet_seen_sup = crate::steam::quietinvite::suppressed();
                tracing::info!(
                    "mm_quiet_invite: test armed for {} s: open Matchmaking Settings > Invite Members now; no Steam window should appear",
                    crate::steam::quietinvite::TEST_WINDOW.as_secs()
                );
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
                // MM-4: a queue needs a loaded world (the auto-leave watches it); at the title screen there is nothing to queue from.
                if let Some(e) = world_refusal(super::world_state(self.world_stale)) {
                    self.refuse(e.to_string());
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

    /// MM-3f: decide whether the invite window is suppressed right now and tell the arming thread.
    fn quiet_tick(&mut self, now: Instant) {
        use crate::steam::quietinvite as qi;
        if !self.quiet {
            return;
        }
        let test_active = self.quiet_test_until.is_some_and(|t| now < t);
        let raw = qi::arm_wanted(self.machine.quiet_invite_wanted(self.steam.own_id()), test_active);
        if raw {
            self.quiet_raw_at = Some(now);
        }
        let since = self.quiet_raw_at.map(|t| now.saturating_duration_since(t));
        let want = qi::keep_armed(raw, since, qi::LINGER);
        if want != self.quiet_last_want {
            tracing::info!(
                "mm_quiet_invite: {} (host waiting for the game's lobby or test window: {raw}, lingering: {})",
                if want { "wanted: arming" } else { "not wanted: disarming" },
                want && !raw
            );
            self.quiet_last_want = want;
        }
        qi::set_wanted(want);
    }

    /// MM-3f: the status line for the overlay.
    fn quiet_line(&self, now: Instant) -> String {
        use crate::steam::quietinvite as qi;
        let sup = qi::suppressed();
        if !qi::ready() {
            return "quiet invite: hook not installed yet (waiting for Steam)".to_string();
        }
        if let Some(t) = self.quiet_test_until.filter(|t| now < *t) {
            let left = t.saturating_duration_since(now).as_secs();
            return if sup > self.quiet_seen_sup {
                format!("quiet invite: SUPPRESSED {} Steam window(s) ({left} s left)", sup - self.quiet_seen_sup)
            } else {
                format!("quiet invite: test armed, {left} s left: press Matchmaking Settings > Invite Members")
            };
        }
        if sup > 0 {
            return format!("quiet invite: {sup} Steam window(s) suppressed this session");
        }
        if qi::armed() {
            "quiet invite: armed (host at MATCH READY)".to_string()
        } else {
            String::new()
        }
    }

    /// One pass: commands, the machine, party following, override, status. Called every ~100 ms.
    pub(crate) fn tick(&mut self, now: Instant) {
        for c in super::take_cmds() {
            tracing::info!("mm_queue: command {c:?}");
            self.command(c, now);
        }
        self.refresh_env(now, false);
        // MM-4 step 4: leave the queue by itself when the player leaves the Roundtable (not while MATCH READY).
        let away = away_from_roundtable(super::world_state(self.world_stale));
        self.machine.world_tick(&mut self.steam, away, now);
        self.machine.tick(&mut self.steam, now);
        self.quiet_tick(now);
        if self.sc_create.as_ref().is_some_and(|c| c.running()) {
            // What the worker's own SC-lobby detection reports right now (env.sc), for the MM-3d log lines.
            let reports = self.current_sc().map(|(sc, _, _)| sc.id());
            if let Some(c) = self.sc_create.as_mut() {
                c.tick(&mut self.steam, now, reports);
            }
        }
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
        super::set_phase(self.machine.phase_code());
        let mut st = self.machine.status();
        if let Some(c) = &self.sc_create {
            st.sc_create_ui = true;
            st.sc_create_line = c.line().to_string();
        }
        if self.quiet {
            st.quiet_ui = true;
            st.quiet_line = self.quiet_line(now);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mm::join::Phase;
    use crate::mm::World;

    #[test]
    fn away_means_an_expedition_or_an_unloaded_world_never_a_title_screen_or_unknown_team() {
        assert!(!away_from_roundtable(World::Present(Some(Phase::Roundtable))));
        assert!(away_from_roundtable(World::Present(Some(Phase::Expedition))));
        assert!(away_from_roundtable(World::Gone));
        assert!(!away_from_roundtable(World::Present(None)), "a team_type that tells nothing is not a reason to leave");
        assert!(!away_from_roundtable(World::Never));
    }

    #[test]
    fn a_queue_request_needs_a_loaded_world() {
        assert!(world_refusal(World::Never).is_some());
        assert!(world_refusal(World::Gone).is_some());
        assert!(world_refusal(World::Present(Some(Phase::Expedition))).is_some());
        assert!(world_refusal(World::Present(Some(Phase::Roundtable))).is_none());
        assert!(world_refusal(World::Present(None)).is_none());
    }
}
