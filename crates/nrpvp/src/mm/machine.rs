//! MM-2: the queue state machine. Pure logic over the `QueueSteam` trait (a fake in the tests, `RealSteam` in the game).
//!
//! `Idle -> Searching -> (Creating | Joining) -> InQueue -> MatchReady`, and `leave()` from anywhere back to `Idle`.
//! Everything here runs on the Steam worker thread; nothing blocks (every Steam call result is polled on a later `tick`).
//!
//! Protocol summary (details in `proto.rs`, design in S9 §6/§7):
//! - Search for queue lobbies tagged `nrpvp=1, mode, proto, game, sc, need`; join the LOWEST lobby id with room for the whole
//!   ticket, else create one (Invisible, `need` members) and tag it.
//! - Duplicate lobbies: the owner of the higher id sets `merge_to`, leaves, and searches again; members of a dissolving lobby
//!   leave too. Everybody always picks the lowest eligible id, so the lobbies converge. The owner re-searches every few seconds.
//! - The lobby is READY when the owner sees `need` players in complete tickets: it closes the lobby (`SetLobbyJoinable(false)`)
//!   and writes `host`, `roster`, `state=go`. Every member then shows the same MATCH READY. With `mm_join` the MM-3 gate (`machine/gate.rs`)
//!   takes over from there and gets every roster member into the host's Seamless Co-op lobby.
//! - Party (Duo/Squad): the Seamless Co-op lobby owner is the leader. It queues for the party and publishes the queue lobby on
//!   SC's lobby as its own member value `nrpvp_q`; the other members' mods read it and join the same queue lobby.

use std::time::{Duration, Instant};

mod gate;

use self::gate::{Env, Gate};
use super::join::{JoinAuto, Phase};
use super::proto::{self, QValue, RosterEntry, Tags};
use super::Mode;
use crate::steam::probe::Event;
use crate::steam::queue::{
    CallData, CallKind, CallPoll, JoinTarget, QueueLobby, QueueSteam, ScLobby, KEY_SC_MEMBER, KEY_TAG,
};

const CALL_TIMEOUT: Duration = Duration::from_secs(20);
const RETRY_DELAY: Duration = Duration::from_secs(3);
const EVAL_EVERY: Duration = Duration::from_millis(500);
const HEARTBEAT_EVERY: Duration = Duration::from_secs(5);
/// A party that is still incomplete after this long leaves the lobby and searches again.
const TICKET_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_FAILS: u32 = 3;
const ORPHAN_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CANDIDATES: i32 = 50;

/// `1234 (0x4D2)`
pub(crate) fn idt(v: u64) -> String {
    format!("{v} (0x{v:X})")
}

/// Who queues. Solo: just us. Duo/Squad: the Seamless Co-op party; only its leader (the SC lobby owner) queues.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Party {
    /// The party leader's Steam id (our own id for Solo).
    pub id: u64,
    pub size: u32,
    pub leader: bool,
    pub sc: Option<ScLobby>,
}

/// Decide the party for a queue request. `sc` is `(the SC lobby we are in, its owner, its member count)` if there is one.
pub(crate) fn build_party(mode: Mode, own: u64, sc: Option<(ScLobby, u64, u32)>) -> Result<Party, String> {
    if mode == Mode::Solo {
        return Ok(Party { id: own, size: 1, leader: true, sc: None });
    }
    let want = mode.team_size() as u32;
    let Some((lobby, owner, members)) = sc else {
        return Err(format!(
            "{} needs a Seamless Co-op party: invite your party through Invite Members first (no SC lobby known)",
            mode.label()
        ));
    };
    if owner != own {
        return Err("only the party leader (owner of the Seamless Co-op lobby) queues; the leader's queue brings you along".to_string());
    }
    if members != want {
        return Err(format!("the party has {members} member(s); {} needs exactly {want}", mode.label()));
    }
    Ok(Party { id: own, size: want, leader: true, sc: Some(lobby) })
}

#[derive(Clone, Debug)]
struct Ticket {
    mode: Mode,
    party: Party,
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    handle: u64,
    since: Instant,
}

struct InQueue {
    ticket: Ticket,
    lobby: QueueLobby,
    next_eval: Instant,
    hb: Option<Pending>,
    next_hb: Instant,
    incomplete_since: Option<Instant>,
    members: u32,
}

struct Ready {
    ticket: Ticket,
    /// Id of the queue lobby the match came from (stays after the gate left it).
    queue_id: u64,
    /// `None` once the gate has left the queue lobby (MM-3); the match itself stays.
    lobby: Option<QueueLobby>,
    roster: Vec<RosterEntry>,
    host_name: String,
    /// The queue host (owner), 0 when the lobby named none.
    host: u64,
    gate: Gate,
}

enum State {
    Idle,
    Searching { ticket: Ticket, call: Option<Pending>, retry_at: Instant, fails: u32 },
    Creating { ticket: Ticket, call: Pending, fails: u32 },
    Joining { ticket: Ticket, target: JoinTarget, call: Pending },
    InQueue(Box<InQueue>),
    MatchReady(Box<Ready>),
}

impl State {
    fn name(&self) -> &'static str {
        match self {
            State::Idle => "Idle",
            State::Searching { .. } => "Searching",
            State::Creating { .. } => "Creating",
            State::Joining { .. } => "Joining",
            State::InQueue(_) => "InQueue",
            State::MatchReady(_) => "MatchReady",
        }
    }
}

enum OrphanKind {
    Create,
    Join(JoinTarget),
}

/// A create/join call that was still in flight when the user left: if it succeeds we leave that lobby again.
struct Orphan {
    kind: OrphanKind,
    handle: u64,
    since: Instant,
}

pub(crate) struct MachineCfg {
    /// Players for a match (`mm_match_players`, 2..=6).
    pub need: u32,
    pub game: String,
    pub sc: String,
    /// MM-3: `mm_join`. Off = exactly MM-2 (a ready match just stays ready).
    pub join: bool,
    /// MM-3: `mm_join_auto`.
    pub join_auto: JoinAuto,
    /// MM-3e: `mm_host_create` (needs `join`): the host's mod creates the Seamless Co-op lobby at MATCH READY.
    pub host_create: bool,
}

pub(crate) struct Machine {
    cfg: MachineCfg,
    state: State,
    orphans: Vec<Orphan>,
    last_error: Option<String>,
    event_list: Option<u32>,
    event_enter: Option<(u64, u32)>,
    dirty: bool,
    follow_block_until: Option<Instant>,
    /// MM-3: refreshed by the worker every tick.
    env: Env,
    /// MM-3: queue lobby id of the match that just ended; a party follower does not follow a stale `nrpvp_q` pointing at it.
    ignore_q: Option<u64>,
}

impl Machine {
    pub(crate) fn new(cfg: MachineCfg) -> Machine {
        Machine {
            cfg,
            state: State::Idle,
            orphans: Vec::new(),
            last_error: None,
            event_list: None,
            event_enter: None,
            dirty: false,
            follow_block_until: None,
            env: Env::default(),
            ignore_q: None,
        }
    }

    pub(crate) fn set_sc_version(&mut self, v: &str) {
        if self.cfg.sc != v {
            tracing::info!("mm_queue: sc tag is now {v:?} (nrsc_lobby_version seen on Seamless Co-op's lobby; was {:?})", self.cfg.sc);
            self.cfg.sc = v.to_string();
        }
    }

    pub(crate) fn is_idle(&self) -> bool {
        matches!(self.state, State::Idle)
    }

    /// Team size to force for the match: only while MATCH READY.
    pub(crate) fn team_override(&self) -> Option<usize> {
        match &self.state {
            State::MatchReady(r) => Some(r.ticket.mode.team_size()),
            _ => None,
        }
    }

    pub(crate) fn status(&self) -> super::Status {
        let need = self.cfg.need;
        let (line, active) = match &self.state {
            State::Idle => (
                match &self.last_error {
                    Some(e) => format!("Queue: idle ({e})"),
                    None => "Queue: idle".to_string(),
                },
                false,
            ),
            State::Searching { ticket, .. } => (format!("Queue: {} searching for a lobby", ticket.mode.label()), true),
            State::Creating { ticket, .. } => (format!("Queue: {} creating a lobby", ticket.mode.label()), true),
            State::Joining { ticket, target, .. } => {
                (format!("Queue: {} joining lobby {}", ticket.mode.label(), target.id()), true)
            }
            State::InQueue(q) => (format!("Queue: {} {}/{need} searching", q.ticket.mode.label(), q.members), true),
            State::MatchReady(r) if self.cfg.join && !r.gate.line().is_empty() => (r.gate.line().to_string(), true),
            State::MatchReady(r) => (
                format!(
                    "MATCH READY host={} ({} players, team size {})",
                    r.host_name,
                    r.roster.len(),
                    r.ticket.mode.team_size()
                ),
                true,
            ),
        };
        let (join_ui, sc_known, sc_owner) = match &self.state {
            State::MatchReady(r) if self.cfg.join => (true, r.gate.sc_known(), r.gate.sc_owner()),
            _ => (false, false, false),
        };
        super::Status { line, active, join_ui, sc_known, sc_owner, ..Default::default() }
    }

    /// MM-3: the worker refreshes these before every tick (Seamless Co-op lobby we are in, safety guard, local phase).
    pub(crate) fn set_env(&mut self, sc: Option<ScLobby>, guard: Result<(), String>, phase: Option<Phase>) {
        self.env = Env { sc, guard, phase };
    }

    /// MM-3: the worker only needs the (cheap but not free) Seamless Co-op lookup while a match is ready and `mm_join` is on.
    pub(crate) fn wants_sc(&self) -> bool {
        self.cfg.join && matches!(self.state, State::MatchReady(_))
    }

    /// MM-3e: the Seamless Co-op lobby this ready match is bound to (the one the gate published or created), but only while it is
    /// still alive: it carries `nrsc_lobby_type` and we are a member. The worker keeps `env.sc` on it and ignores any second SC lobby.
    pub(crate) fn preferred_sc_alive(&self, api: &dyn QueueSteam) -> Option<u64> {
        let State::MatchReady(r) = &self.state else { return None };
        if !self.cfg.join {
            return None;
        }
        let id = r.gate.preferred_sc()?;
        let own = api.own_id();
        let alive = ScLobby::verify(api, id).is_some() && (0..api.member_count(id).clamp(0, 64)).any(|i| api.member_by_index(id, i) == own);
        alive.then_some(id)
    }

    fn tags(&self, mode: Mode) -> Tags {
        Tags { mode, game: self.cfg.game.clone(), sc: self.cfg.sc.clone(), need: self.cfg.need }
    }

    fn goto(&mut self, next: State) {
        if self.state.name() != next.name() {
            tracing::info!("mm_queue: state {} -> {}", self.state.name(), next.name());
        }
        self.state = next;
    }

    fn fail_to_idle(&mut self, why: String) -> State {
        tracing::warn!("mm_queue: giving up: {why}");
        self.last_error = Some(why);
        State::Idle
    }

    // ---- user commands ----

    /// Start queuing. Ignored (logged) unless Idle.
    pub(crate) fn queue(&mut self, api: &mut dyn QueueSteam, mode: Mode, party: Party, now: Instant) {
        if !self.is_idle() {
            tracing::info!("mm_queue: queue {} ignored: already {}", mode.label(), self.state.name());
            return;
        }
        if party.size as usize != mode.team_size() {
            let e = format!("party of {} does not fit {}", party.size, mode.label());
            tracing::warn!("mm_queue: {e}");
            self.last_error = Some(e);
            return;
        }
        self.last_error = None;
        tracing::info!(
            "mm_queue: queue requested: mode={} party_leader={} size={} leader={} sc_lobby={:?} need={}",
            mode.label(),
            idt(party.id),
            party.size,
            party.leader,
            party.sc.as_ref().map(ScLobby::id),
            self.cfg.need
        );
        let st = self.start_search(api, Ticket { mode, party }, 0, now);
        self.goto(st);
    }

    /// Leave from any state. Only our queue lobby is ever left.
    pub(crate) fn leave(&mut self, api: &mut dyn QueueSteam, why: &str, now: Instant) {
        let old = std::mem::replace(&mut self.state, State::Idle);
        if matches!(old, State::Idle) {
            return;
        }
        let from = old.name();
        tracing::info!("mm_queue: leaving the queue from {from}: {why}");
        match old {
            State::Idle | State::Searching { .. } => {}
            State::Creating { call, .. } => {
                self.orphans.push(Orphan { kind: OrphanKind::Create, handle: call.handle, since: now });
            }
            State::Joining { target, call, .. } => {
                self.orphans.push(Orphan { kind: OrphanKind::Join(target), handle: call.handle, since: now });
            }
            State::InQueue(q) => {
                Self::leave_lobby(api, &q.lobby, &q.ticket);
            }
            State::MatchReady(mut r) => {
                self.ignore_q = Some(r.queue_id);
                let c = r.gate.take_create();
                self.abandon_create(api, c, now);
                match &r.lobby {
                    Some(q) => Self::leave_lobby(api, q, &r.ticket),
                    // The gate already left the queue lobby; only our `nrpvp_q` value is left to clear.
                    None => Self::clear_sc(api, &r.ticket),
                }
            }
        }
        self.event_list = None;
        self.event_enter = None;
        tracing::info!("mm_queue: state {from} -> Idle");
    }

    /// `LeaveLobby` on the queue lobby, plus clearing the party leader's `nrpvp_q` value.
    fn leave_lobby(api: &mut dyn QueueSteam, lobby: &QueueLobby, ticket: &Ticket) {
        api.leave(lobby);
        Self::clear_sc(api, ticket);
    }

    fn clear_sc(api: &mut dyn QueueSteam, ticket: &Ticket) {
        if ticket.party.leader {
            if let Some(sc) = &ticket.party.sc {
                api.set_sc_queue_value(sc, "");
            }
        }
    }

    // ---- events from the shared listener ----

    pub(crate) fn on_event(&mut self, ev: &Event) {
        match ev {
            Event::LobbyMatchList { count } => self.event_list = Some(*count),
            Event::LobbyEnter { lobby, response, .. } => {
                self.event_enter = Some((*lobby, *response));
                self.dirty = true;
            }
            Event::LobbyDataUpdate { .. } | Event::LobbyChatUpdate { .. } => self.dirty = true,
            _ => {}
        }
    }

    // ---- party sync (followers) ----

    /// Called regularly by the worker with the SC lobby we are in. Followers join the queue lobby their leader published in
    /// `nrpvp_q` and leave when it is cleared or moves.
    pub(crate) fn sync_party(&mut self, api: &mut dyn QueueSteam, sc: &ScLobby, now: Instant) {
        let own = api.own_id();
        let leader = api.owner(sc.id());
        if leader == 0 || leader == own {
            return;
        }
        let q = proto::parse_q(&api.member_data(sc.id(), leader, KEY_SC_MEMBER));
        let cur: Option<u64> = match &self.state {
            State::Joining { ticket, target, .. } if !ticket.party.leader && ticket.party.id == leader => Some(target.id()),
            State::InQueue(i) if !i.ticket.party.leader && i.ticket.party.id == leader => Some(i.lobby.id()),
            State::MatchReady(r) if !r.ticket.party.leader && r.ticket.party.id == leader => r.lobby.as_ref().map(QueueLobby::id),
            _ => None,
        };
        match (q, cur) {
            (None, Some(_)) => self.leave(api, "the party leader's queue ended", now),
            (Some(q), Some(c)) if c != q.lobby && matches!(self.state, State::InQueue(_)) => {
                self.leave(api, "the party leader moved to another queue lobby", now);
                self.follow(api, q, leader, sc, now);
            }
            (Some(q), None) if self.is_idle() => {
                if self.follow_block_until.is_some_and(|t| now < t) || self.ignore_q == Some(q.lobby) {
                    return;
                }
                self.follow(api, q, leader, sc, now);
            }
            _ => {}
        }
    }

    fn follow(&mut self, api: &mut dyn QueueSteam, q: QValue, leader: u64, sc: &ScLobby, now: Instant) {
        let Some(target) = JoinTarget::party_hint(q.lobby) else { return };
        tracing::info!(
            "mm_queue: following party leader {} into queue lobby {} (mode {}, party size {})",
            idt(leader),
            idt(q.lobby),
            q.mode.label(),
            q.size
        );
        let ticket = Ticket {
            mode: q.mode,
            party: Party { id: leader, size: q.size, leader: false, sc: Some(sc.clone()) },
        };
        match api.begin_join(&target) {
            Ok(handle) => {
                self.last_error = None;
                self.goto(State::Joining { ticket, target, call: Pending { handle, since: now } });
            }
            Err(e) => {
                self.last_error = Some(format!("follow failed: {e}"));
                self.follow_block_until = Some(now + RETRY_DELAY);
            }
        }
    }

    // ---- the tick ----

    /// Advance one step. The worker calls this every ~100 ms.
    pub(crate) fn tick(&mut self, api: &mut dyn QueueSteam, now: Instant) {
        self.poll_orphans(api, now);
        let old = std::mem::replace(&mut self.state, State::Idle);
        let before = old.name();
        let next = self.step(api, old, now);
        if next.name() != before {
            tracing::info!("mm_queue: state {before} -> {}", next.name());
        }
        self.state = next;
        self.dirty = false;
    }

    fn poll_orphans(&mut self, api: &mut dyn QueueSteam, now: Instant) {
        let mut keep = Vec::new();
        for o in std::mem::take(&mut self.orphans) {
            let kind = match o.kind {
                OrphanKind::Create => CallKind::Created,
                OrphanKind::Join(_) => CallKind::Enter,
            };
            match api.poll(o.handle, kind) {
                CallPoll::Pending if now.saturating_duration_since(o.since) < ORPHAN_TIMEOUT => keep.push(o),
                CallPoll::Done(CallData::Created { lobby: Some(q), .. }) => {
                    tracing::info!("mm_queue: abandoned CreateLobby finished; leaving lobby {}", idt(q.id()));
                    api.leave(&q);
                }
                CallPoll::Done(d @ CallData::Enter { .. }) if d.enter_ok() => {
                    if let OrphanKind::Join(t) = &o.kind {
                        match t.entered(api) {
                            Ok(q) => {
                                tracing::info!("mm_queue: abandoned JoinLobby finished; leaving lobby {}", idt(q.id()));
                                api.leave(&q);
                            }
                            Err(r) => tracing::warn!(
                                "mm_queue: abandoned JoinLobby entered lobby {} which is not a queue lobby ({r:?}); not touching it",
                                idt(t.id())
                            ),
                        }
                    }
                }
                _ => {}
            }
        }
        self.orphans = keep;
    }

    fn step(&mut self, api: &mut dyn QueueSteam, st: State, now: Instant) -> State {
        match st {
            State::Idle => State::Idle,
            State::MatchReady(r) if self.cfg.join => self.gate_step(api, r, now),
            State::MatchReady(r) => State::MatchReady(r),
            State::Searching { ticket, call, retry_at, fails } => match call {
                None if now >= retry_at => self.start_search(api, ticket, fails, now),
                None => State::Searching { ticket, call: None, retry_at, fails },
                Some(p) => {
                    let res = api.poll(p.handle, CallKind::MatchList);
                    let from_event = if matches!(res, CallPoll::Done(_)) { None } else { self.event_list.take() };
                    match (res, from_event) {
                        (CallPoll::Done(CallData::MatchList { count }), _) => {
                            self.event_list = None;
                            self.handle_results(api, ticket, fails, count, now)
                        }
                        (_, Some(count)) => {
                            tracing::info!("mm_queue: using the LobbyMatchList_t event ({count} lobbies) because the call result was not available");
                            self.handle_results(api, ticket, fails, count, now)
                        }
                        (CallPoll::Pending, None) if now.saturating_duration_since(p.since) < CALL_TIMEOUT => {
                            State::Searching { ticket, call, retry_at, fails }
                        }
                        (CallPoll::Pending, None) => self.retry_search(ticket, fails, "RequestLobbyList timed out".into(), now),
                        (CallPoll::Failed { reason, invalid_handle }, None) => {
                            let extra = if invalid_handle {
                                " (handle unknown to Steam: the result may have been consumed by the game's callback pump: UNKNOWN)"
                            } else {
                                ""
                            };
                            self.retry_search(ticket, fails, format!("RequestLobbyList failed: {reason}{extra}"), now)
                        }
                        (CallPoll::Done(other), None) => {
                            self.retry_search(ticket, fails, format!("RequestLobbyList returned {other:?}"), now)
                        }
                    }
                }
            },
            State::Creating { ticket, call, fails } => match api.poll(call.handle, CallKind::Created) {
                CallPoll::Done(CallData::Created { lobby: Some(q), result }) => {
                    tracing::info!("mm_queue: LobbyCreated result={result} lobby={}", idt(q.id()));
                    self.enter_queue(api, ticket, q, now)
                }
                CallPoll::Done(CallData::Created { result, lobby: None }) => {
                    self.retry_search(ticket, fails, format!("CreateLobby failed: EResult {result}"), now)
                }
                CallPoll::Pending if now.saturating_duration_since(call.since) < CALL_TIMEOUT => {
                    State::Creating { ticket, call, fails }
                }
                CallPoll::Pending => self.retry_search(ticket, fails, "CreateLobby timed out".into(), now),
                CallPoll::Failed { reason, invalid_handle } => {
                    let extra = if invalid_handle {
                        " (handle unknown to Steam: the result may have been consumed by the game's callback pump: UNKNOWN; a stray empty lobby may exist)"
                    } else {
                        ""
                    };
                    self.retry_search(ticket, fails, format!("CreateLobby call failed: {reason}{extra}"), now)
                }
                CallPoll::Done(other) => {
                    self.retry_search(ticket, fails, format!("CreateLobby returned {other:?}"), now)
                }
            },
            State::Joining { ticket, target, call } => {
                let res = api.poll(call.handle, CallKind::Enter);
                let from_event = match (&res, self.event_enter) {
                    (CallPoll::Done(_), _) => None,
                    (_, Some((l, r))) if l == target.id() => {
                        self.event_enter = None;
                        Some(r)
                    }
                    _ => None,
                };
                let response = match (&res, from_event) {
                    (CallPoll::Done(CallData::Enter { response, .. }), _) => Some(*response),
                    (_, Some(r)) => {
                        tracing::info!("mm_queue: using the LobbyEnter_t event because the call result was not available");
                        Some(r)
                    }
                    _ => None,
                };
                match (response, &res) {
                    (Some(1), _) => match target.entered(api) {
                        Ok(q) => {
                            tracing::info!("mm_queue: joined queue lobby {}", idt(q.id()));
                            self.enter_queue(api, ticket, q, now)
                        }
                        Err(r) => {
                            let e = format!(
                                "joined lobby {} but it is not a queue lobby ({r:?}); not touching it",
                                idt(target.id())
                            );
                            self.fail_to_idle(e)
                        }
                    },
                    (Some(r), _) => {
                        tracing::info!("mm_queue: JoinLobby({}) refused: EChatRoomEnterResponse {r} (4 = full, 2 = gone)", idt(target.id()));
                        self.after_join_refused(ticket, now)
                    }
                    (None, CallPoll::Pending) if now.saturating_duration_since(call.since) < CALL_TIMEOUT => {
                        State::Joining { ticket, target, call }
                    }
                    (None, CallPoll::Failed { reason, .. }) => {
                        tracing::warn!("mm_queue: JoinLobby call failed: {reason}");
                        self.after_join_refused(ticket, now)
                    }
                    (None, _) => {
                        tracing::warn!("mm_queue: JoinLobby timed out");
                        self.after_join_refused(ticket, now)
                    }
                }
            }
            State::InQueue(iq) => self.eval_queue(api, iq, now),
        }
    }

    fn after_join_refused(&mut self, ticket: Ticket, now: Instant) -> State {
        if ticket.party.leader {
            State::Searching { ticket, call: None, retry_at: now + Duration::from_secs(1), fails: 0 }
        } else {
            // A follower waits for its leader's next `nrpvp_q`.
            self.follow_block_until = Some(now + RETRY_DELAY);
            self.last_error = Some("could not join the party leader's queue lobby".to_string());
            State::Idle
        }
    }

    fn start_search(&mut self, api: &mut dyn QueueSteam, ticket: Ticket, fails: u32, now: Instant) -> State {
        let filters = proto::search_filters(&self.tags(ticket.mode), ticket.party.size);
        self.event_list = None;
        match api.begin_search(&filters) {
            Ok(handle) => State::Searching { ticket, call: Some(Pending { handle, since: now }), retry_at: now, fails },
            Err(e) => self.retry_search(ticket, fails, format!("RequestLobbyList not started: {e}"), now),
        }
    }

    fn retry_search(&mut self, ticket: Ticket, fails: u32, why: String, now: Instant) -> State {
        tracing::warn!("mm_queue: {why} (attempt {})", fails + 1);
        if fails + 1 >= MAX_FAILS {
            return self.fail_to_idle(why);
        }
        State::Searching { ticket, call: None, retry_at: now + RETRY_DELAY, fails: fails + 1 }
    }

    /// Why `id` cannot be used as a queue lobby for `tags` needing `slots` free slots, or `Ok`.
    fn eligibility(api: &dyn QueueSteam, id: u64, tags: &Tags, slots: u32) -> Result<(), String> {
        if JoinTarget::candidate(api, id).is_err() {
            return Err("no nrpvp=1 tag or it is Seamless Co-op's lobby".into());
        }
        let expect = [
            (proto::K_MODE, tags.mode.label().to_string()),
            (proto::K_PROTO, proto::PROTO.to_string()),
            (proto::K_GAME, tags.game.clone()),
            (proto::K_SC, tags.sc.clone()),
            (proto::K_NEED, tags.need.to_string()),
        ];
        for (k, v) in expect {
            let got = api.lobby_data(id, k);
            if got != v {
                return Err(format!("tag {k}={got:?}, want {v:?}"));
            }
        }
        if api.lobby_data(id, proto::K_STATE) == proto::STATE_GO {
            return Err("state=go".into());
        }
        if !api.lobby_data(id, proto::K_MERGE).is_empty() {
            return Err("dissolving (merge_to set)".into());
        }
        let members = api.member_count(id);
        if members < 0 || members as u32 + slots > tags.need {
            return Err(format!("no room: {members} member(s), need {} free", slots));
        }
        Ok(())
    }

    /// Lobby ids from the last search that are usable, lowest first.
    fn candidates(&self, api: &dyn QueueSteam, count: u32, tags: &Tags, slots: u32, skip: u64, below: Option<u64>) -> Vec<u64> {
        let mut ok = Vec::new();
        let n = i32::try_from(count).unwrap_or(MAX_CANDIDATES).min(MAX_CANDIDATES);
        for i in 0..n {
            let id = api.lobby_by_index(i);
            if id == 0 || id == skip {
                continue;
            }
            if below.is_some_and(|b| id >= b) {
                continue;
            }
            match Self::eligibility(api, id, tags, slots) {
                Ok(()) => {
                    tracing::info!(
                        "mm_queue: candidate lobby {}: owner={} members={} limit={}",
                        idt(id),
                        idt(api.owner(id)),
                        api.member_count(id),
                        api.member_limit(id)
                    );
                    ok.push(id);
                }
                Err(why) => tracing::info!("mm_queue: skipping lobby {}: {why}", idt(id)),
            }
        }
        ok.sort_unstable();
        ok
    }

    fn handle_results(&mut self, api: &mut dyn QueueSteam, ticket: Ticket, fails: u32, count: u32, now: Instant) -> State {
        tracing::info!("mm_queue: search returned {count} lobb{}", if count == 1 { "y" } else { "ies" });
        if !ticket.party.leader {
            // A follower never searches; it only follows. Should not happen.
            return self.fail_to_idle("internal: a party follower started a search".into());
        }
        let tags = self.tags(ticket.mode);
        let cands = self.candidates(api, count, &tags, ticket.party.size, 0, None);
        if let Some(&id) = cands.first() {
            if let Ok(target) = JoinTarget::candidate(api, id) {
                return match api.begin_join(&target) {
                    Ok(handle) => State::Joining { ticket, target, call: Pending { handle, since: now } },
                    Err(e) => self.retry_search(ticket, fails, format!("JoinLobby not started: {e}"), now),
                };
            }
        }
        match api.begin_create(i32::try_from(self.cfg.need).unwrap_or(2)) {
            Ok(handle) => State::Creating { ticket, call: Pending { handle, since: now }, fails },
            Err(e) => self.retry_search(ticket, fails, format!("CreateLobby not started: {e}"), now),
        }
    }

    /// We are in `lobby` (created or joined): publish our ticket and, as a party leader, the lobby for the party.
    fn enter_queue(&mut self, api: &mut dyn QueueSteam, ticket: Ticket, lobby: QueueLobby, now: Instant) -> State {
        api.set_my_member_data(&lobby, proto::M_PARTY, &ticket.party.id.to_string());
        api.set_my_member_data(&lobby, proto::M_SIZE, &ticket.party.size.to_string());
        if ticket.party.leader && ticket.mode != Mode::Solo {
            if let Some(sc) = &ticket.party.sc {
                let q = QValue { lobby: lobby.id(), mode: ticket.mode, size: ticket.party.size };
                api.set_sc_queue_value(sc, &proto::encode_q(&q));
            }
        }
        tracing::info!(
            "mm_queue: in queue lobby {} as {} (party leader {}, party size {})",
            idt(lobby.id()),
            if api.owner(lobby.id()) == api.own_id() { "owner" } else { "member" },
            idt(ticket.party.id),
            ticket.party.size
        );
        State::InQueue(Box::new(InQueue {
            ticket,
            lobby,
            next_eval: now,
            hb: None,
            next_hb: now + HEARTBEAT_EVERY,
            incomplete_since: None,
            members: 0,
        }))
    }

    /// Back to searching after losing the queue lobby. A follower goes idle instead and waits for its leader.
    fn lost_lobby(&mut self, api: &mut dyn QueueSteam, ticket: Ticket, now: Instant) -> State {
        Self::clear_sc(api, &ticket);
        if ticket.party.leader {
            State::Searching { ticket, call: None, retry_at: now + Duration::from_secs(1), fails: 0 }
        } else {
            self.follow_block_until = Some(now + RETRY_DELAY);
            State::Idle
        }
    }

    fn eval_queue(&mut self, api: &mut dyn QueueSteam, mut iq: Box<InQueue>, now: Instant) -> State {
        let id = iq.lobby.id();
        let own = api.own_id();
        // The heartbeat search runs on its own schedule, whatever the evaluation throttle says.
        if !(self.dirty || now >= iq.next_eval) && iq.hb.is_none() {
            return State::InQueue(iq);
        }
        iq.next_eval = now + EVAL_EVERY;

        // 1. Are we still in the lobby?
        let n = api.member_count(id);
        let member_ids: Vec<u64> = (0..n.clamp(0, 64)).map(|i| api.member_by_index(id, i)).collect();
        if n <= 0 || !member_ids.contains(&own) {
            tracing::warn!("mm_queue: queue lobby {} is gone or we are no longer in it", idt(id));
            let t = iq.ticket.clone();
            return self.lost_lobby(api, t, now);
        }
        iq.members = n as u32;
        let owner = api.owner(id) == own;

        // 2. The owner makes sure the tags are on the lobby (also after an ownership migration).
        if owner && api.lobby_data(id, KEY_TAG) != "1" {
            for (k, v) in proto::tag_pairs(&self.tags(iq.ticket.mode)) {
                api.set_data(&iq.lobby, &k, &v);
            }
        }

        // 3. The match started?
        if api.lobby_data(id, proto::K_STATE) == proto::STATE_GO {
            return self.become_ready(api, *iq, now);
        }

        // 4. The lobby is dissolving into a lower one: everybody leaves.
        let merge = api.lobby_data(id, proto::K_MERGE);
        if !merge.is_empty() {
            tracing::info!("mm_queue: lobby {} dissolves (merge_to={merge}); leaving", idt(id));
            api.leave(&iq.lobby);
            let t = iq.ticket.clone();
            return self.lost_lobby(api, t, now);
        }

        // 5. Tickets.
        let members: Vec<(u64, Option<(u64, u32)>)> = member_ids
            .iter()
            .map(|&m| (m, proto::parse_ticket(&api.member_data(id, m, proto::M_PARTY), &api.member_data(id, m, proto::M_SIZE))))
            .collect();
        let report = proto::tickets(&members);
        if report.incomplete_parties.contains(&iq.ticket.party.id) {
            let since = *iq.incomplete_since.get_or_insert(now);
            if now.saturating_duration_since(since) >= TICKET_TIMEOUT {
                tracing::warn!("mm_queue: our party is still incomplete after {} s; leaving lobby {}", TICKET_TIMEOUT.as_secs(), idt(id));
                api.leave(&iq.lobby);
                let t = iq.ticket.clone();
                return self.lost_lobby(api, t, now);
            }
        } else {
            iq.incomplete_since = None;
        }

        if owner {
            // 6a. Full of complete tickets: close and start.
            if report.complete_players >= self.cfg.need && report.incomplete_parties.is_empty() && report.unknown == 0 {
                api.set_joinable(&iq.lobby, false);
                api.set_data(&iq.lobby, proto::K_HOST, &own.to_string());
                api.set_data(&iq.lobby, proto::K_ROSTER, &proto::encode_roster(&report.roster));
                api.set_data(&iq.lobby, proto::K_STATE, proto::STATE_GO);
                return self.become_ready(api, *iq, now);
            }
            // 6b. Duplicate-lobby heartbeat.
            return self.heartbeat(api, iq, now);
        }
        State::InQueue(iq)
    }

    fn heartbeat(&mut self, api: &mut dyn QueueSteam, mut iq: Box<InQueue>, now: Instant) -> State {
        let id = iq.lobby.id();
        match iq.hb {
            None if now >= iq.next_hb => {
                iq.next_hb = now + HEARTBEAT_EVERY;
                let filters = proto::search_filters(&self.tags(iq.ticket.mode), iq.members);
                self.event_list = None;
                match api.begin_search(&filters) {
                    Ok(handle) => iq.hb = Some(Pending { handle, since: now }),
                    Err(e) => tracing::warn!("mm_queue: heartbeat search not started: {e}"),
                }
            }
            None => {}
            Some(p) => {
                let res = api.poll(p.handle, CallKind::MatchList);
                let count = match res {
                    CallPoll::Done(CallData::MatchList { count }) => Some(count),
                    CallPoll::Pending if now.saturating_duration_since(p.since) < CALL_TIMEOUT => None,
                    other => {
                        // Not done: the event may stand in for it.
                        if let Some(c) = self.event_list.take() {
                            Some(c)
                        } else {
                            tracing::warn!("mm_queue: heartbeat search ended without result: {other:?}");
                            iq.hb = None;
                            None
                        }
                    }
                };
                if let Some(count) = count {
                    iq.hb = None;
                    let tags = self.tags(iq.ticket.mode);
                    let lower = self.candidates(api, count, &tags, iq.members, id, Some(id));
                    if let Some(&target) = lower.first() {
                        tracing::info!(
                            "mm_queue: duplicate lobby: {} is lower than ours ({}); dissolving ours into it",
                            idt(target),
                            idt(id)
                        );
                        api.set_data(&iq.lobby, proto::K_MERGE, &target.to_string());
                        api.leave(&iq.lobby);
                        let t = iq.ticket.clone();
                        return self.lost_lobby(api, t, now);
                    }
                }
            }
        }
        State::InQueue(iq)
    }

    fn become_ready(&mut self, api: &mut dyn QueueSteam, iq: InQueue, now: Instant) -> State {
        let id = iq.lobby.id();
        let roster = proto::parse_roster(&api.lobby_data(id, proto::K_ROSTER));
        let host: u64 = api.lobby_data(id, proto::K_HOST).trim().parse().unwrap_or(0);
        let host_name = if host == 0 { "?".to_string() } else { api.persona(host) };
        tracing::info!(
            "mm_queue: MATCH READY lobby={} host={} ({host_name}) mode={} need={} roster={}",
            idt(id),
            idt(host),
            iq.ticket.mode.label(),
            self.cfg.need,
            proto::encode_roster(&roster)
        );
        for r in &roster {
            tracing::info!(
                "mm_queue: roster member {} ({}) party={}{}",
                idt(r.id),
                api.persona(r.id),
                idt(r.party),
                if r.id == api.own_id() { "  <- me" } else { "" }
            );
        }
        if !roster.iter().any(|r| r.id == api.own_id()) {
            tracing::warn!("mm_queue: our own Steam id is not in the roster");
        }
        tracing::info!(
            "mm_queue: team size override for this match = {} ({} mode)",
            iq.ticket.mode.team_size(),
            iq.ticket.mode.label()
        );
        let queue_id = iq.lobby.id();
        State::MatchReady(Box::new(Ready { ticket: iq.ticket, queue_id, lobby: Some(iq.lobby), roster, host_name, host, gate: Gate::new(now) }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steam::queue::fake::{FakeSteam, Write};
    use crate::steam::queue::{LobbyText, KEY_SC_TYPE};

    const ME: u64 = 7001;
    const OTHER: u64 = 7002;

    fn cfg() -> MachineCfg {
        MachineCfg { need: 2, game: "1.3.3.0".into(), sc: "1.1.3".into(), join: false, join_auto: JoinAuto::Invite, host_create: false }
    }

    fn tag_data() -> Vec<(&'static str, &'static str)> {
        vec![("nrpvp", "1"), ("mode", "solo"), ("proto", "1"), ("game", "1.3.3.0"), ("sc", "1.1.3"), ("need", "2")]
    }

    /// Run ticks (each 600 ms apart, so the evaluation throttle always passes) until `done` or 40 ticks.
    fn run(m: &mut Machine, s: &mut FakeSteam, t: &mut Instant, done: impl Fn(&Machine) -> bool) {
        for _ in 0..40 {
            if done(m) {
                return;
            }
            *t += Duration::from_millis(600);
            m.tick(s, *t);
        }
    }

    fn solo(me: u64) -> Party {
        Party { id: me, size: 1, leader: true, sc: None }
    }

    #[test]
    fn no_lobby_found_creates_one_and_tags_it() {
        let mut s = FakeSteam::new(ME);
        let mut m = Machine::new(cfg());
        let mut t = Instant::now();
        m.queue(&mut s, Mode::Solo, solo(ME), t);
        assert!(matches!(m.state, State::Searching { .. }));
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::InQueue(_)));
        let id = match &m.state {
            State::InQueue(q) => q.lobby.id(),
            _ => panic!("not in queue: {}", m.state.name()),
        };
        // One more evaluation writes the tags (we own it).
        m.tick(&mut s, t + Duration::from_secs(1));
        for (k, v) in tag_data() {
            assert_eq!(s.lobby_data(id, k), v, "{k}");
        }
        assert!(s.writes.contains(&Write::Create(2)), "created with `need` members");
        assert!(s.writes.contains(&Write::MemberData(id, "nrpvp_party".into(), ME.to_string())));
        assert!(s.writes.contains(&Write::MemberData(id, "nrpvp_size".into(), "1".into())));
        assert_eq!(m.status().line, "Queue: solo 1/2 searching");
        assert!(m.status().active);
        assert_eq!(m.team_override(), None);
    }

    #[test]
    fn joins_the_lowest_lobby_with_room_and_skips_unusable_ones() {
        let mut s = FakeSteam::new(ME);
        s.add_lobby(900, OTHER, 2, &[OTHER, 1], &tag_data()); // full
        s.add_lobby(800, OTHER, 2, &[OTHER], &[("nrpvp", "1"), ("mode", "duo"), ("proto", "1"), ("game", "1.3.3.0"), ("sc", "1.1.3"), ("need", "2")]); // wrong mode
        s.add_lobby(700, OTHER, 6, &[OTHER], &[(KEY_SC_TYPE, "x"), ("nrpvp", "1")]); // SC's
        let mut ok = tag_data();
        ok.push(("state", "go"));
        s.add_lobby(650, OTHER, 2, &[OTHER], &ok); // already going
        s.add_lobby(600, OTHER, 2, &[OTHER], &tag_data()); // usable
        s.add_lobby(500, OTHER, 2, &[OTHER], &{
            let mut d = tag_data();
            d.push(("merge_to", "400"));
            d
        }); // dissolving
        s.auto_search = Some(vec![900, 800, 700, 650, 600, 500]);
        let mut m = Machine::new(cfg());
        let mut t = Instant::now();
        m.queue(&mut s, Mode::Solo, solo(ME), t);
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::InQueue(_)));
        assert!(s.writes.contains(&Write::Join(600)), "{:?}", s.writes);
        assert!(!s.writes.iter().any(|w| matches!(w, Write::Create(_))));
        assert!(!s.writes.iter().any(|w| matches!(w, Write::Join(700) | Write::Join(900))));
        // The SC-tagged lobby got no write of any kind.
        assert!(s.writes_to(700).is_empty());
    }

    #[test]
    fn owner_starts_the_match_when_the_lobby_is_full_and_every_member_sees_the_same_roster() {
        let mut s = FakeSteam::new(ME);
        let mut m = Machine::new(cfg());
        let mut t = Instant::now();
        m.queue(&mut s, Mode::Solo, solo(ME), t);
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::InQueue(_)));
        let id = match &m.state {
            State::InQueue(q) => q.lobby.id(),
            _ => unreachable!(),
        };
        // The second player arrives and publishes a Solo ticket.
        s.lobbies.get_mut(&id).expect("lobby").members.push(OTHER);
        s.set_member(id, OTHER, "nrpvp_party", &OTHER.to_string());
        s.set_member(id, OTHER, "nrpvp_size", "1");
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::MatchReady(_)));
        assert_eq!(s.lobby_data(id, "state"), "go");
        assert_eq!(s.lobby_data(id, "host"), ME.to_string());
        let roster = s.lobby_data(id, "roster");
        assert_eq!(roster, format!("{ME}:{ME},{OTHER}:{OTHER}"));
        assert!(s.writes.contains(&Write::Joinable(id, false)));
        assert_eq!(m.team_override(), Some(1));
        assert!(m.status().line.starts_with("MATCH READY host=P7001"), "{}", m.status().line);

        // The other player's machine only reads `state=go` (it is not the owner) and gets the same roster.
        let mut s2 = FakeSteam::new(OTHER);
        s2.lobbies = std::mem::take(&mut s.lobbies);
        s2.lobbies.get_mut(&id).expect("lobby").owner = ME;
        let mut m2 = Machine::new(cfg());
        let mut t2 = Instant::now();
        m2.state = State::InQueue(Box::new(InQueue {
            ticket: Ticket { mode: Mode::Solo, party: solo(OTHER) },
            lobby: QueueLobby::for_test(id),
            next_eval: t2,
            hb: None,
            next_hb: t2,
            incomplete_since: None,
            members: 0,
        }));
        run(&mut m2, &mut s2, &mut t2, |m| matches!(m.state, State::MatchReady(_)));
        match (&m.state, &m2.state) {
            (State::MatchReady(a), State::MatchReady(b)) => {
                assert_eq!(a.roster, b.roster);
                assert_eq!(a.host_name, b.host_name);
            }
            _ => panic!("both must be ready"),
        }
        assert!(s2.writes.iter().all(|w| !matches!(w, Write::SetData(..) | Write::Joinable(..))), "a non-owner never writes lobby data");
    }

    #[test]
    fn leaving_a_ready_match_leaves_the_queue_lobby_and_clears_the_override() {
        let mut s = FakeSteam::new(ME);
        let mut m = Machine::new(cfg());
        let mut t = Instant::now();
        m.queue(&mut s, Mode::Solo, solo(ME), t);
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::InQueue(_)));
        let id = match &m.state {
            State::InQueue(q) => q.lobby.id(),
            _ => unreachable!(),
        };
        s.lobbies.get_mut(&id).expect("lobby").members.push(OTHER);
        s.set_member(id, OTHER, "nrpvp_party", &OTHER.to_string());
        s.set_member(id, OTHER, "nrpvp_size", "1");
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::MatchReady(_)));
        assert_eq!(m.team_override(), Some(1));
        m.leave(&mut s, "test", t);
        assert!(m.is_idle());
        assert_eq!(m.team_override(), None);
        assert!(s.writes.contains(&Write::Leave(id)));
        // Queue again works.
        m.queue(&mut s, Mode::Solo, solo(ME), t);
        assert!(matches!(m.state, State::Searching { .. }));
    }

    #[test]
    fn duplicate_lobbies_converge_on_the_lowest_id() {
        // Our lobby is created with id 1001. A lower lobby 500 exists with room.
        let mut s = FakeSteam::new(ME);
        let mut m = Machine::new(cfg());
        let mut t = Instant::now();
        m.queue(&mut s, Mode::Solo, solo(ME), t);
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::InQueue(_)));
        let mine = match &m.state {
            State::InQueue(q) => q.lobby.id(),
            _ => unreachable!(),
        };
        s.add_lobby(500, OTHER, 2, &[OTHER], &tag_data());
        s.auto_search = Some(vec![500, mine]);
        // Mark the other lobby's owner member data so it is not "unknown" (irrelevant for the merge).
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::Searching { .. }));
        assert_eq!(s.lobby_data(mine, "merge_to"), "500", "the higher lobby dissolves into the lower one");
        assert!(s.writes.contains(&Write::Leave(mine)));
        // Then the normal search joins 500.
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::InQueue(_)));
        assert!(s.writes.contains(&Write::Join(500)));
        match &m.state {
            State::InQueue(q) => assert_eq!(q.lobby.id(), 500),
            _ => panic!("expected InQueue"),
        }
    }

    #[test]
    fn the_lower_lobby_never_merges_upwards() {
        let mut s = FakeSteam::new(ME);
        let mut m = Machine::new(cfg());
        let mut t = Instant::now();
        m.queue(&mut s, Mode::Solo, solo(ME), t);
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::InQueue(_)));
        let mine = match &m.state {
            State::InQueue(q) => q.lobby.id(),
            _ => unreachable!(),
        };
        s.add_lobby(mine + 100, OTHER, 2, &[OTHER], &tag_data());
        s.auto_search = Some(vec![mine, mine + 100]);
        for _ in 0..30 {
            t += Duration::from_secs(1);
            m.tick(&mut s, t);
        }
        assert!(matches!(m.state, State::InQueue(_)));
        assert!(s.lobby_data(mine, "merge_to").is_empty());
        assert!(!s.writes.contains(&Write::Leave(mine)));
    }

    #[test]
    fn members_of_a_dissolving_lobby_leave_it() {
        let mut s = FakeSteam::new(OTHER);
        s.add_lobby(1500, ME, 2, &[ME, OTHER], &{
            let mut d = tag_data();
            d.push(("merge_to", "500"));
            d
        });
        let mut m = Machine::new(cfg());
        let t = Instant::now();
        m.state = State::InQueue(Box::new(InQueue {
            ticket: Ticket { mode: Mode::Solo, party: solo(OTHER) },
            lobby: QueueLobby::for_test(1500),
            next_eval: t,
            hb: None,
            next_hb: t,
            incomplete_since: None,
            members: 2,
        }));
        m.tick(&mut s, t);
        assert!(s.writes.contains(&Write::Leave(1500)));
        assert!(matches!(m.state, State::Searching { .. }));
    }

    #[test]
    fn create_failures_end_in_idle_with_the_reason_on_screen() {
        let mut s = FakeSteam::new(ME);
        s.create_fails = true;
        let mut m = Machine::new(cfg());
        let mut t = Instant::now();
        m.queue(&mut s, Mode::Solo, solo(ME), t);
        for _ in 0..40 {
            t += Duration::from_secs(4);
            m.tick(&mut s, t);
        }
        assert!(m.is_idle());
        assert!(m.status().line.contains("CreateLobby failed"), "{}", m.status().line);
        assert!(!m.status().active);
    }

    #[test]
    fn leaving_while_creating_leaves_the_stray_lobby_once_it_exists() {
        let mut s = FakeSteam::new(ME);
        let mut m = Machine::new(cfg());
        let mut t = Instant::now();
        m.queue(&mut s, Mode::Solo, solo(ME), t);
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::Creating { .. }));
        assert!(matches!(m.state, State::Creating { .. }));
        m.leave(&mut s, "user", t);
        assert!(m.is_idle());
        m.tick(&mut s, t + Duration::from_secs(1));
        let created = s.next_lobby;
        assert!(s.writes.contains(&Write::Leave(created)), "{:?}", s.writes);
        assert!(m.is_idle());
    }

    #[test]
    fn a_duo_leader_publishes_the_queue_lobby_only_as_its_own_member_value_on_sc_lobby() {
        const SC: u64 = 5555;
        let mut s = FakeSteam::new(ME);
        s.add_lobby(SC, ME, 6, &[ME, OTHER], &[(KEY_SC_TYPE, "nrsc_ynx3_seamless_master_lobby"), ("nrsc_lobby_version", "1.1.3")]);
        let sc = ScLobby::verify(&s, SC).expect("sc lobby");
        let party = build_party(Mode::Duo, ME, Some((sc, ME, 2))).expect("party");
        let mut c = cfg();
        c.need = 2;
        let mut m = Machine::new(c);
        let mut t = Instant::now();
        m.queue(&mut s, Mode::Duo, party, t);
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::InQueue(_)));
        let q = match &m.state {
            State::InQueue(q) => q.lobby.id(),
            _ => panic!("not queued"),
        };
        assert!(s.writes.contains(&Write::ScValue(SC, format!("{q}:duo:2"))));
        // Leaving clears it again.
        m.leave(&mut s, "test", t);
        assert_eq!(s.writes.last(), Some(&Write::ScValue(SC, String::new())));
        // The ONLY thing ever written on the SC lobby is the member value; never data, joinable, member data, or leave.
        for w in s.writes_to(SC) {
            assert!(matches!(w, Write::ScValue(..)), "forbidden write on SC's lobby: {w:?}");
        }
    }

    #[test]
    fn a_party_follower_joins_the_leaders_lobby_and_leaves_when_it_is_cleared() {
        const SC: u64 = 5555;
        const Q: u64 = 4242;
        let mut s = FakeSteam::new(OTHER);
        s.add_lobby(SC, ME, 6, &[ME, OTHER], &[(KEY_SC_TYPE, "x")]);
        let mut qdata = tag_data();
        qdata.retain(|(k, _)| *k != "mode");
        qdata.push(("mode", "duo"));
        s.add_lobby(Q, ME, 2, &[ME], &qdata);
        s.set_member(SC, ME, "nrpvp_q", &format!("{Q}:duo:2"));
        let sc = ScLobby::verify(&s, SC).expect("sc");
        let mut m = Machine::new(cfg());
        let mut t = Instant::now();
        m.sync_party(&mut s, &sc, t);
        assert!(matches!(m.state, State::Joining { .. }));
        run(&mut m, &mut s, &mut t, |m| matches!(m.state, State::InQueue(_)));
        assert!(s.writes.contains(&Write::Join(Q)));
        // The follower's ticket is the leader's party id.
        assert!(s.writes.contains(&Write::MemberData(Q, "nrpvp_party".into(), ME.to_string())));
        assert!(s.writes.contains(&Write::MemberData(Q, "nrpvp_size".into(), "2".into())));
        assert!(!s.writes.iter().any(|w| matches!(w, Write::Create(_))), "a follower never creates a lobby");
        // The leader clears its value: the follower leaves.
        s.set_member(SC, ME, "nrpvp_q", "");
        m.sync_party(&mut s, &sc, t);
        assert!(m.is_idle());
        assert!(s.writes.contains(&Write::Leave(Q)));
        // The follower never writes on SC's lobby.
        assert!(s.writes_to(SC).is_empty());
    }

    #[test]
    fn an_incomplete_duo_does_not_start_even_when_the_lobby_has_enough_members() {
        // Two members, but each declares a Duo with a different leader: neither ticket is complete.
        let mut s = FakeSteam::new(ME);
        s.add_lobby(77, ME, 2, &[ME, OTHER], &tag_data());
        s.set_member(77, ME, "nrpvp_party", &ME.to_string());
        s.set_member(77, ME, "nrpvp_size", "2");
        s.set_member(77, OTHER, "nrpvp_party", &OTHER.to_string());
        s.set_member(77, OTHER, "nrpvp_size", "2");
        let mut m = Machine::new(cfg());
        let t = Instant::now();
        m.state = State::InQueue(Box::new(InQueue {
            ticket: Ticket { mode: Mode::Solo, party: solo(ME) },
            lobby: QueueLobby::for_test(77),
            next_eval: t,
            hb: None,
            next_hb: t + Duration::from_secs(3600),
            incomplete_since: None,
            members: 0,
        }));
        m.tick(&mut s, t);
        assert!(matches!(m.state, State::InQueue(_)));
        assert!(s.lobby_data(77, "state").is_empty());
    }

    #[test]
    fn party_decisions() {
        let sc = ScLobby::for_test(1);
        assert_eq!(build_party(Mode::Solo, ME, None), Ok(Party { id: ME, size: 1, leader: true, sc: None }));
        assert!(build_party(Mode::Duo, ME, None).unwrap_err().contains("Seamless Co-op party"));
        assert!(build_party(Mode::Duo, ME, Some((sc.clone(), OTHER, 2))).unwrap_err().contains("party leader"));
        assert!(build_party(Mode::Duo, ME, Some((sc.clone(), ME, 3))).unwrap_err().contains("exactly 2"));
        assert!(build_party(Mode::Squad, ME, Some((sc.clone(), ME, 2))).unwrap_err().contains("exactly 3"));
        let p = build_party(Mode::Squad, ME, Some((sc.clone(), ME, 3))).expect("squad");
        assert_eq!((p.size, p.leader, p.sc), (3, true, Some(sc)));
    }

    #[test]
    fn queue_is_ignored_unless_idle_and_party_size_must_fit() {
        let mut s = FakeSteam::new(ME);
        let mut m = Machine::new(cfg());
        let t = Instant::now();
        m.queue(&mut s, Mode::Duo, solo(ME), t);
        assert!(m.is_idle() && m.status().line.contains("does not fit"));
        m.queue(&mut s, Mode::Solo, solo(ME), t);
        let searches = s.writes.iter().filter(|w| matches!(w, Write::Search(_))).count();
        m.queue(&mut s, Mode::Solo, solo(ME), t);
        assert_eq!(s.writes.iter().filter(|w| matches!(w, Write::Search(_))).count(), searches);
    }
}
