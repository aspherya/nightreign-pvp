//! MM-3d (dev only, `mm_sc_create = true`): does Seamless Co-op adopt a lobby the mod creates the game's way?
//!
//! S9 §5d: opening Invite Members makes the game call `ISteamMatchmaking::CreateLobby(Public, 6)` through SC's proxied interface, and
//! that lobby becomes SC's lobby A. One dev button makes the same call from the Steam worker thread, then watches the lobby for 20 s
//! and decides:
//! - it carries `nrsc_lobby_type` -> `ADOPTED by Seamless Co-op`: left completely alone (no call of any kind on it);
//! - it does not -> `not adopted`: it is ours alone, so `LeaveLobby` it.
//!
//! Rules, kept by types: nothing is ever written to the lobby (the only calls on it are the reads of the `LobbyApi` / `LobbyText`
//! getters and, for a lobby that does NOT carry `nrsc_lobby_type`, `QueueSteam::leave`, which takes the [`QueueLobby`] our own
//! `CreateLobby` call result yields and which refuses again at call time if the marker is present). There is no `set_*`
//! call anywhere in this file. Log prefix `mm_sc_create:`.

use std::time::{Duration, Instant};

use super::machine::idt;
use crate::steam::probe::Event;
use crate::steam::queue::{
    CallData, CallKind, CallPoll, QueueLobby, QueueSteam, ScType, KEY_SC_TYPE,
};

/// The member limit of the lobby the game itself creates (S9 §5d: `CreateLobby(2, 6)`).
pub(crate) const MAX_MEMBERS: i32 = 6;
/// How long the lobby is watched before the decision.
pub(crate) const WATCH: Duration = Duration::from_secs(20);
/// Sampling period while watching.
pub(crate) const STEP: Duration = Duration::from_secs(2);
/// Give up waiting for the `LobbyCreated_t` call result after this long.
pub(crate) const CREATE_TIMEOUT: Duration = Duration::from_secs(15);
/// Most `LobbyEnter_t` events remembered while the call result is pending.
const MAX_EARLY: usize = 16;

/// Why the button does nothing, or `None` when it may run. Pure, so the rules are tested.
pub(crate) fn refuse_reason(
    guard: &Result<(), String>,
    in_sc: Option<u64>,
    running: bool,
) -> Option<String> {
    if running {
        return Some("a create test is already running".to_string());
    }
    if let Err(e) = guard {
        return Some(format!("safety guard: {e}"));
    }
    if let Some(id) = in_sc {
        return Some(format!(
            "already in Seamless Co-op lobby {}: refusing to create another",
            idt(id)
        ));
    }
    None
}

/// The decision after the watch window. Pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The lobby carries `nrsc_lobby_type`.
    Adopted,
    /// It does not: the lobby is ours alone.
    NotAdopted,
}

pub(crate) fn verdict(carries_sc_type: bool) -> Verdict {
    if carries_sc_type {
        Verdict::Adopted
    } else {
        Verdict::NotAdopted
    }
}

enum Phase {
    Idle,
    Creating {
        handle: u64,
        started: Instant,
    },
    Watching {
        lobby: QueueLobby,
        since: Instant,
        next_sample: Instant,
        adopted_seen_at: Option<Duration>,
    },
}

/// The test state machine. One run at a time; after the verdict it is idle again.
pub(crate) struct ScCreate {
    ty: ScType,
    phase: Phase,
    /// `(lobby, locked)` from `LobbyEnter_t` events (they can arrive before the call result is polled).
    enters: Vec<(u64, bool)>,
    line: String,
}

impl ScCreate {
    pub(crate) fn new(ty: ScType) -> ScCreate {
        ScCreate {
            ty,
            phase: Phase::Idle,
            enters: Vec::new(),
            line: String::new(),
        }
    }

    pub(crate) fn running(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }

    /// One short status line for the overlay (empty until the button was pressed).
    pub(crate) fn line(&self) -> &str {
        &self.line
    }

    /// The button. `guard` is the MM-2 safety guard, `in_sc` the Seamless Co-op lobby the local player is in (if any).
    pub(crate) fn press(
        &mut self,
        api: &mut dyn QueueSteam,
        guard: &Result<(), String>,
        in_sc: Option<u64>,
        now: Instant,
    ) {
        if let Some(why) = refuse_reason(guard, in_sc, self.running()) {
            tracing::warn!("mm_sc_create: REFUSED: {why}");
            if !self.running() {
                self.line = format!("SC create test: REFUSED - {why}");
            }
            return;
        }
        self.enters.clear();
        match api.begin_create_sc(self.ty, MAX_MEMBERS) {
            Ok(handle) => {
                tracing::info!("mm_sc_create: pressed: CreateLobby({}, {MAX_MEMBERS}) started, handle {handle}", self.ty.label());
                self.line = format!(
                    "SC create test: creating a {} lobby...",
                    self.ty.create_name()
                );
                self.phase = Phase::Creating {
                    handle,
                    started: now,
                };
            }
            Err(e) => {
                tracing::warn!("mm_sc_create: CreateLobby not started: {e}");
                self.line = format!("SC create test: failed - {e}");
            }
        }
    }

    /// Every decoded Steam event (read only). Remembers `locked` for the lobby and logs a `LobbyCreated_t` seen while waiting.
    pub(crate) fn on_event(&mut self, ev: &Event) {
        if !self.running() {
            return;
        }
        match ev {
            Event::LobbyEnter {
                lobby,
                locked,
                response,
                ..
            } => {
                if self.enters.len() < MAX_EARLY {
                    self.enters.push((*lobby, *locked));
                }
                if let Phase::Creating { .. } = self.phase {
                    tracing::info!("mm_sc_create: event LobbyEnter lobby={} locked={locked} response={response}", idt(*lobby));
                }
            }
            Event::LobbyCreated { result, lobby }
                if matches!(self.phase, Phase::Creating { .. }) =>
            {
                tracing::info!(
                    "mm_sc_create: event LobbyCreated result={result} lobby={}",
                    idt(*lobby)
                );
            }
            _ => {}
        }
    }

    fn locked_of(&self, lobby: u64) -> Option<bool> {
        self.enters
            .iter()
            .rev()
            .find(|(l, _)| *l == lobby)
            .map(|(_, locked)| *locked)
    }

    /// Called every worker pass. `sc_reports` is the Seamless Co-op lobby the worker's detection (`env.sc`) currently reports.
    pub(crate) fn tick(&mut self, api: &mut dyn QueueSteam, now: Instant, sc_reports: Option<u64>) {
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Idle => {}
            Phase::Creating { handle, started } => match api.poll(handle, CallKind::Created) {
                CallPoll::Pending if now.saturating_duration_since(started) >= CREATE_TIMEOUT => {
                    tracing::warn!(
                        "mm_sc_create: no LobbyCreated result after {} s; giving up (a lobby may exist that this test does not track)",
                        CREATE_TIMEOUT.as_secs()
                    );
                    self.line = "SC create test: failed - no result from Steam".to_string();
                }
                CallPoll::Pending => self.phase = Phase::Creating { handle, started },
                CallPoll::Done(CallData::Created {
                    lobby: Some(q),
                    result,
                }) => {
                    tracing::info!(
                        "mm_sc_create: LobbyCreated result={result} lobby={}",
                        idt(q.id())
                    );
                    self.line = format!("SC create test: lobby {} created, watching 20 s", q.id());
                    self.phase = Phase::Watching {
                        lobby: q,
                        since: now,
                        next_sample: now,
                        adopted_seen_at: None,
                    };
                    self.watch(api, now, sc_reports);
                }
                CallPoll::Done(CallData::Created {
                    result,
                    lobby: None,
                }) => {
                    tracing::warn!("mm_sc_create: CreateLobby failed: result={result} (1 = OK)");
                    self.line = format!("SC create test: failed - EResult {result}");
                }
                CallPoll::Done(other) => {
                    tracing::warn!("mm_sc_create: unexpected call result {other:?}");
                    self.line = "SC create test: failed - unexpected result".to_string();
                }
                CallPoll::Failed {
                    reason,
                    invalid_handle,
                } => {
                    tracing::warn!(
                        "mm_sc_create: CreateLobby call result failed: {reason} (invalid handle: {invalid_handle}; if true the game's callback \
                         pump may have consumed it, then a lobby may exist that this test does not track - check the LobbyCreated event line)"
                    );
                    self.line = "SC create test: failed - call result unavailable".to_string();
                }
            },
            watching @ Phase::Watching { .. } => {
                self.phase = watching;
                self.watch(api, now, sc_reports);
            }
        }
    }

    /// One pass of the watch phase: sample every 2 s, decide after 20 s.
    fn watch(&mut self, api: &mut dyn QueueSteam, now: Instant, sc_reports: Option<u64>) {
        let Phase::Watching {
            lobby,
            since,
            next_sample,
            adopted_seen_at,
        } = &mut self.phase
        else {
            return;
        };
        let elapsed = now.saturating_duration_since(*since);
        let finished = elapsed >= WATCH;
        if now < *next_sample && !finished {
            return;
        }
        *next_sample = now + STEP;
        let id = lobby.id();
        let carries = !api.lobby_data(id, KEY_SC_TYPE).is_empty();
        if carries && adopted_seen_at.is_none() {
            *adopted_seen_at = Some(elapsed);
            tracing::info!(
                "mm_sc_create: nrsc_lobby_type appeared on lobby {} at +{:.1} s",
                idt(id),
                elapsed.as_secs_f32()
            );
        }
        let n = api.member_count(id).clamp(0, 64);
        let members: Vec<u64> = (0..n).map(|i| api.member_by_index(id, i)).collect();
        let mut data = Vec::new();
        for i in 0..api.data_count(id).clamp(0, 64) {
            if let Some((k, v)) = api.data_by_index(id, i) {
                data.push(format!("{k}={v:?}"));
            }
        }
        let locked = match self.enters.iter().rev().find(|(l, _)| *l == id) {
            Some((_, l)) => l.to_string(),
            None => "unknown".to_string(),
        };
        tracing::info!(
            "mm_sc_create: +{:.1}s lobby={} owner={} members={:?} limit={} locked={locked} data=[{}] env.sc={:?} nrsc_lobby_type={}",
            elapsed.as_secs_f32(),
            idt(id),
            api.owner(id),
            members,
            api.member_limit(id),
            data.join(", "),
            sc_reports,
            carries
        );
        if !finished {
            return;
        }
        let q = lobby.clone();
        let locked = self.locked_of(id);
        self.phase = Phase::Idle;
        match verdict(carries) {
            Verdict::Adopted => {
                tracing::info!(
                    "mm_sc_create: ADOPTED by Seamless Co-op: lobby {} carries {KEY_SC_TYPE}; left alone (no call made on it). locked={locked:?} env.sc={sc_reports:?}",
                    idt(id)
                );
                self.line = format!("SC create test: ADOPTED by Seamless Co-op (lobby {id})");
            }
            Verdict::NotAdopted => {
                tracing::info!("mm_sc_create: not adopted: lobby {} never carried {KEY_SC_TYPE} in 20 s; leaving it (LeaveLobby)", idt(id));
                api.leave(&q);
                self.line = format!("SC create test: not adopted (lobby {id} left)");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steam::queue::fake::{FakeSteam, Write};

    const OWN: u64 = 7;

    fn ok() -> Result<(), String> {
        Ok(())
    }

    /// Press, then tick until `secs` seconds after the press, every 100 ms like the worker.
    fn run(c: &mut ScCreate, s: &mut FakeSteam, t0: Instant, secs: f32, sc: Option<u64>) {
        let mut ms = 0u64;
        while (ms as f32) <= secs * 1000.0 {
            c.tick(s, t0 + Duration::from_millis(ms), sc);
            ms += 100;
        }
    }

    #[test]
    fn refuse_rules() {
        assert_eq!(refuse_reason(&ok(), None, false), None);
        assert!(refuse_reason(&Err("EAC".into()), None, false)
            .expect("r")
            .contains("safety guard: EAC"));
        assert!(refuse_reason(&ok(), Some(5), false)
            .expect("r")
            .contains("already in Seamless Co-op lobby"));
        assert!(refuse_reason(&ok(), None, true)
            .expect("r")
            .contains("already running"));
    }

    #[test]
    fn verdict_follows_the_marker() {
        assert_eq!(verdict(true), Verdict::Adopted);
        assert_eq!(verdict(false), Verdict::NotAdopted);
    }

    #[test]
    fn refused_press_makes_no_steam_call() {
        let t0 = Instant::now();
        for (g, sc) in [(Err("no nrsc".to_string()), None), (ok(), Some(9u64))] {
            let mut s = FakeSteam::new(OWN);
            let mut c = ScCreate::new(ScType::Public);
            c.press(&mut s, &g, sc, t0);
            assert!(s.writes.is_empty(), "{:?}", s.writes);
            assert!(!c.running());
            assert!(c.line().contains("REFUSED"), "{}", c.line());
        }
    }

    #[test]
    fn not_adopted_lobby_is_left_after_20_s() {
        let t0 = Instant::now();
        let mut s = FakeSteam::new(OWN);
        let mut c = ScCreate::new(ScType::Public);
        c.press(&mut s, &ok(), None, t0);
        assert_eq!(s.writes, vec![Write::CreateSc(ScType::Public, 6)]);
        run(&mut c, &mut s, t0, 19.0, None);
        assert!(c.running());
        assert!(
            !s.writes.iter().any(|w| matches!(w, Write::Leave(_))),
            "no leave before 20 s"
        );
        run(&mut c, &mut s, t0, 21.0, None);
        let lobby = s.next_lobby;
        assert_eq!(
            s.writes_to(lobby),
            vec![&Write::Leave(lobby)],
            "only a LeaveLobby on our own lobby"
        );
        assert!(!c.running());
        assert!(c.line().contains("not adopted"), "{}", c.line());
    }

    #[test]
    fn adopted_lobby_gets_no_call_at_all() {
        let t0 = Instant::now();
        let mut s = FakeSteam::new(OWN);
        s.adopt_created = true;
        let mut c = ScCreate::new(ScType::Public);
        c.press(&mut s, &ok(), None, t0);
        let lobby = s.next_lobby;
        run(&mut c, &mut s, t0, 22.0, Some(lobby));
        assert!(s.writes_to(lobby).is_empty(), "{:?}", s.writes);
        assert_eq!(s.writes, vec![Write::CreateSc(ScType::Public, 6)]);
        assert!(!c.running());
        assert!(c.line().contains("ADOPTED"), "{}", c.line());
    }

    #[test]
    fn marker_appearing_late_still_counts_and_blocks_the_leave() {
        let t0 = Instant::now();
        let mut s = FakeSteam::new(OWN);
        let mut c = ScCreate::new(ScType::Friends);
        c.press(&mut s, &ok(), None, t0);
        let lobby = s.next_lobby;
        run(&mut c, &mut s, t0, 10.0, None);
        s.lobbies
            .get_mut(&lobby)
            .expect("lobby")
            .data
            .insert(KEY_SC_TYPE.into(), "x".into());
        run(&mut c, &mut s, t0, 22.0, None);
        assert!(s.writes_to(lobby).is_empty());
        assert!(c.line().contains("ADOPTED"), "{}", c.line());
        assert_eq!(s.writes, vec![Write::CreateSc(ScType::Friends, 6)]);
    }

    #[test]
    fn a_failed_create_leaves_nothing_to_leave() {
        let t0 = Instant::now();
        let mut s = FakeSteam::new(OWN);
        s.create_fails = true;
        let mut c = ScCreate::new(ScType::Public);
        c.press(&mut s, &ok(), None, t0);
        run(&mut c, &mut s, t0, 1.0, None);
        assert!(!c.running());
        assert!(c.line().contains("failed"), "{}", c.line());
        assert_eq!(s.writes, vec![Write::CreateSc(ScType::Public, 6)]);
    }

    #[test]
    fn a_missing_call_result_times_out_without_a_leave() {
        let t0 = Instant::now();
        let mut s = FakeSteam::new(OWN);
        let mut c = ScCreate::new(ScType::Public);
        c.press(&mut s, &ok(), None, t0);
        s.results.clear();
        run(&mut c, &mut s, t0, 14.0, None);
        assert!(c.running());
        run(&mut c, &mut s, t0, 16.0, None);
        assert!(!c.running());
        assert_eq!(s.writes, vec![Write::CreateSc(ScType::Public, 6)]);
    }

    #[test]
    fn second_press_while_running_is_ignored_and_locked_comes_from_the_event() {
        let t0 = Instant::now();
        let mut s = FakeSteam::new(OWN);
        let mut c = ScCreate::new(ScType::Public);
        c.press(&mut s, &ok(), None, t0);
        let lobby = s.next_lobby;
        c.on_event(&Event::LobbyEnter {
            lobby,
            permissions: 0,
            locked: true,
            response: 1,
        });
        c.press(&mut s, &ok(), None, t0);
        assert_eq!(s.writes.len(), 1, "no second CreateLobby");
        c.tick(&mut s, t0, None);
        assert_eq!(c.locked_of(lobby), Some(true));
    }
}
