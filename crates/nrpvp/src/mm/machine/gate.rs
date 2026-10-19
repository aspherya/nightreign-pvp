//! MM-3: the gate. After `MatchReady`, every roster member who is not yet in the host's Seamless Co-op (SC) lobby gets in.
//!
//! What it does, per member (all on the Steam worker thread, driven by `Machine::step`):
//! - Host (queue lobby owner): publishes `sc_lobby=<id>` as soon as it is a member of an SC lobby (else says in plain words what to do),
//!   publishes `present=<n>/<total>` and `invited=<ids>`, and sends `InviteUserToLobby` once per absent member (method b) when `mm_join_auto`
//!   asks for it.
//! - Joiner: reads `sc_lobby` from the queue lobby (only as a [`PublishedScLobby`], which proves the host wrote it), and with
//!   `mm_join_auto` = `invite+link` / `link` opens `steam://joinlobby/...` by itself once (method a).
//! - Everybody: "present" = member of the host's SC lobby. When every roster member is a member, the queue lobby is left (the match,
//!   roster and team-size override are kept) and the status says ALL PRESENT.
//! - The match ends on Leave, when the local player is back at the Roundtable after an expedition (`join::Phase`, 5 s debounce), or when,
//!   after an expedition, the player has been in no Seamless Co-op lobby of the match for 15 s outside the Expedition phase (MM-3a).
//!
//! Only these things are ever done to SC's lobby: reads, our member value `nrpvp_q` (MM-2), `InviteUserToLobby`, `JoinLobby` (method c),
//! and `SetLobbyType` from a dev button (method d). No `SetLobbyData`, `LeaveLobby`, `SetLobbyJoinable`, kick or owner change exists
//! for it: the types in `steam::queue` do not allow them.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use super::{idt, Machine, Pending, Ready, State};
use crate::mm::join::{self, JoinCmd, Phase};
use crate::mm::proto;
use crate::steam::queue::{CallData, CallKind, CallPoll, PublishedScLobby, QueueLobby, QueueSteam, ScLobby, ScType, KEY_SC_TYPE};

const GATE_EVAL: Duration = Duration::from_millis(500);
const SUMMARY_EVERY: Duration = Duration::from_secs(10);
const INVITE_RETRY: Duration = Duration::from_secs(5);
const ROUNDTABLE_DEBOUNCE: Duration = Duration::from_secs(5);
const DIRECT_TIMEOUT: Duration = Duration::from_secs(20);
/// MM-3a: after an expedition, this long outside the Expedition phase with no Seamless Co-op lobby of the match ends the match.
const SC_GONE_END: Duration = Duration::from_secs(15);

/// MM-3e: how long our created lobby may take to carry `nrsc_lobby_type` before it is left and the host is asked to open Invite Members.
const HOST_ADOPT_WITHIN: Duration = Duration::from_secs(10);
/// MM-3e: give up waiting for the `LobbyCreated_t` call result after this long.
const HOST_CREATE_RESULT: Duration = Duration::from_secs(15);
/// MM-3e: no roster member entered this long after `sc_lobby` was published -> the host is asked to invite them by hand.
const NO_ARRIVAL: Duration = Duration::from_secs(45);
/// MM-3e: the lobby the game makes for Invite Members is `CreateLobby(Public, 6)` (S9 §5d); MM-3d showed Seamless Co-op adopts the same call.
const HOST_CREATE_MAX: i32 = crate::mm::sccreate::MAX_MEMBERS;

/// MM-3e: the host-side creation of Seamless Co-op's lobby, once per match (never a second one).
pub(super) enum HostCreate {
    /// Not started yet (also: switched off, or waiting for the safety guard).
    Todo,
    /// `CreateLobby` sent, `LobbyCreated_t` pending.
    Creating { handle: u64, started: Instant },
    /// Our lobby exists; waiting for `nrsc_lobby_type` to appear on it.
    Watching { lobby: QueueLobby, since: Instant },
    /// Seamless Co-op adopted it (it carries `nrsc_lobby_type`). No call is ever made on it by this module.
    Adopted(u64),
    /// Nothing (more) to do: finished, failed, left, or an SC lobby existed already.
    Done,
}

/// Inputs the worker refreshes every tick (the machine itself never reads game memory or modules).
pub(crate) struct Env {
    /// The Seamless Co-op lobby we are a member of right now (verified: carries `nrsc_lobby_type`).
    pub sc: Option<ScLobby>,
    /// The MM-2 safety guard (SC loaded, no EasyAntiCheat).
    pub guard: Result<(), String>,
    pub phase: Option<Phase>,
    /// MM-3f: how many players the local ChrSet holds right now (0 = not read yet). A real Seamless Co-op join shows as `players >= 2`
    /// at the Roundtable (S9 5f: FS-MM1 2, FS-MM2 stayed 1 while the Steam lobby had both).
    pub players: usize,
}

impl Default for Env {
    fn default() -> Env {
        Env { sc: None, guard: Err("safety guard not evaluated yet".into()), phase: None, players: 0 }
    }
}

/// MM-3f: the Steam lobby has everybody; ALL PRESENT also needs the local ChrSet to hold at least the roster size.
pub(crate) fn everyone_loaded(steam_all: bool, players: usize, total: usize) -> bool {
    steam_all && total > 0 && players >= total
}

/// Per-match state of the gate.
pub(crate) struct Gate {
    published: Option<PublishedScLobby>,
    invited: BTreeSet<u64>,
    invite_retry_at: Option<Instant>,
    pub_sc: Option<u64>,
    pub_present: String,
    pub_invited: String,
    link_tried: Option<u64>,
    all_present: Option<usize>,
    saw_expedition: bool,
    roundtable_since: Option<Instant>,
    /// MM-3a: since when (after an expedition, outside the Expedition phase) the player has been in no SC lobby of this match.
    sc_gone_since: Option<Instant>,
    direct: Option<Pending>,
    next_eval: Instant,
    next_log: Instant,
    last_log: String,
    warned_publish: Option<String>,
    /// Host: the last SC lobby id it was a member of (kept for the "known" flag after the lobby disappears).
    sc_last: Option<u64>,
    /// What the overlay shows.
    line: String,
    sc_owner: bool,
    /// MM-3e: the host's creation of the SC lobby.
    create: HostCreate,
    /// MM-3e: when the host first published `sc_lobby` (starts the no-arrival timer).
    pub_at: Option<Instant>,
    /// MM-3e: the no-arrival fallback was logged.
    no_arrival_logged: bool,
}

impl Gate {
    pub(crate) fn new(now: Instant) -> Gate {
        Gate {
            published: None,
            invited: BTreeSet::new(),
            invite_retry_at: None,
            pub_sc: None,
            pub_present: String::new(),
            pub_invited: String::new(),
            link_tried: None,
            all_present: None,
            saw_expedition: false,
            roundtable_since: None,
            sc_gone_since: None,
            direct: None,
            next_eval: now,
            next_log: now,
            last_log: String::new(),
            warned_publish: None,
            sc_last: None,
            line: String::new(),
            sc_owner: false,
            create: HostCreate::Todo,
            pub_at: None,
            no_arrival_logged: false,
        }
    }

    /// MM-3e: the SC lobby this match is bound to: the one the host published (joiner), else the one we created, else the last one
    /// the host was a member of. The worker keeps its Seamless Co-op lobby detection on this one.
    pub(crate) fn preferred_sc(&self) -> Option<u64> {
        self.published.as_ref().map(PublishedScLobby::id).or(match &self.create {
            HostCreate::Watching { lobby, .. } => Some(lobby.id()),
            HostCreate::Adopted(id) => Some(*id),
            _ => None,
        }).or(self.sc_last)
    }

    /// MM-3e: the host's creation is under way or done and its lobby has not shown up as `env.sc` yet.
    fn creating(&self) -> bool {
        matches!(self.create, HostCreate::Creating { .. } | HostCreate::Watching { .. } | HostCreate::Adopted(_))
    }

    /// MM-3e: hand the creation state to the caller (match ended), leaving `Done` behind.
    pub(super) fn take_create(&mut self) -> HostCreate {
        std::mem::replace(&mut self.create, HostCreate::Done)
    }

    /// The host's SC lobby id when known (the host: the one it is in; a joiner: the one the host published).
    pub(crate) fn sc_known(&self) -> bool {
        self.published.is_some() || self.sc_last.is_some()
    }
    /// MM-3f: no ALL PRESENT yet and no expedition: the window in which the host's Invite Members is still expected.
    pub(crate) fn quiet_window_open(&self) -> bool {
        self.all_present.is_none() && !self.saw_expedition
    }
    pub(crate) fn sc_owner(&self) -> bool {
        self.sc_owner
    }
    pub(crate) fn line(&self) -> &str {
        &self.line
    }
}

fn members_of(api: &dyn QueueSteam, lobby: u64) -> Vec<u64> {
    let n = api.member_count(lobby);
    (0..n.clamp(0, 64)).map(|i| api.member_by_index(lobby, i)).filter(|&m| m != 0).collect()
}

impl Machine {
    /// End a ready match: leave the queue lobby if we are still in it, clear our `nrpvp_q`, back to Idle.
    fn end_match(&mut self, api: &mut dyn QueueSteam, mut r: Box<Ready>, why: &str, now: Instant) -> State {
        tracing::info!("mm_join: match ends: {why}");
        self.ignore_q = Some(r.queue_id);
        let c = r.gate.take_create();
        self.abandon_create(api, c, now);
        match &r.lobby {
            Some(q) => Self::leave_lobby(api, q, &r.ticket),
            None => Self::clear_sc(api, &r.ticket),
        }
        self.event_list = None;
        self.event_enter = None;
        State::Idle
    }

    /// MM-3e: the match ended while our `CreateLobby` was pending or its lobby was not adopted: do not leave a stray lobby behind.
    /// An adopted lobby is Seamless Co-op's: no call (the types would refuse the leave anyway).
    pub(super) fn abandon_create(&mut self, api: &mut dyn QueueSteam, c: HostCreate, now: Instant) {
        match c {
            HostCreate::Creating { handle, .. } => {
                tracing::info!("mm_host_create: match ended while CreateLobby was pending; leaving that lobby once it arrives");
                self.orphans.push(super::Orphan { kind: super::OrphanKind::Create, handle, since: now });
            }
            HostCreate::Watching { lobby, .. } => {
                if api.lobby_data(lobby.id(), KEY_SC_TYPE).is_empty() {
                    tracing::info!("mm_host_create: match ended; leaving our not-adopted lobby {}", idt(lobby.id()));
                    api.leave(&lobby);
                }
            }
            HostCreate::Todo | HostCreate::Adopted(_) | HostCreate::Done => {}
        }
    }

    /// MM-3e: the host creates Seamless Co-op's lobby once, the way the game does for Invite Members (S9 §5d, MM-3d: adopted).
    fn host_create_step(&mut self, api: &mut dyn QueueSteam, g: &mut Gate, is_host: bool, env_sc: Option<u64>, now: Instant) {
        match std::mem::replace(&mut g.create, HostCreate::Done) {
            HostCreate::Todo => {
                g.create = HostCreate::Todo;
                if !self.cfg.host_create || !is_host || g.saw_expedition {
                    return;
                }
                if let Some(id) = env_sc {
                    tracing::info!("mm_host_create: skipped: Seamless Co-op lobby {} exists already; using it, creating nothing", idt(id));
                    g.create = HostCreate::Done;
                    return;
                }
                if self.env.guard.is_err() {
                    return; // not evaluated yet, or the safety guard fails: the prompt stays
                }
                match api.begin_create_sc(ScType::Public, HOST_CREATE_MAX) {
                    Ok(handle) => {
                        tracing::info!(
                            "mm_host_create: MATCH READY, host without a Seamless Co-op lobby: CreateLobby({}, {HOST_CREATE_MAX}) started, handle {handle}",
                            ScType::Public.label()
                        );
                        g.create = HostCreate::Creating { handle, started: now };
                    }
                    Err(e) => {
                        tracing::warn!("mm_host_create: CreateLobby not started: {e}; falling back to the Invite Members prompt");
                        g.create = HostCreate::Done;
                    }
                }
            }
            HostCreate::Creating { handle, started } => match api.poll(handle, CallKind::Created) {
                CallPoll::Done(CallData::Created { lobby: Some(q), result }) => {
                    tracing::info!("mm_host_create: LobbyCreated result={result} lobby={}", idt(q.id()));
                    g.create = HostCreate::Watching { lobby: q, since: now };
                    self.host_create_step(api, g, is_host, env_sc, now);
                }
                CallPoll::Pending if now.saturating_duration_since(started) < HOST_CREATE_RESULT => {
                    g.create = HostCreate::Creating { handle, started };
                }
                CallPoll::Pending => tracing::warn!(
                    "mm_host_create: no LobbyCreated result after {} s; falling back to the Invite Members prompt (a lobby may exist that is not tracked)",
                    HOST_CREATE_RESULT.as_secs()
                ),
                CallPoll::Done(CallData::Created { result, lobby: None }) => {
                    tracing::warn!("mm_host_create: CreateLobby failed: EResult {result} (1 = OK); falling back to the Invite Members prompt");
                }
                CallPoll::Done(other) => tracing::warn!("mm_host_create: unexpected call result {other:?}; falling back to the Invite Members prompt"),
                CallPoll::Failed { reason, invalid_handle } => tracing::warn!(
                    "mm_host_create: CreateLobby call result failed: {reason} (invalid handle: {invalid_handle}); falling back to the Invite Members prompt"
                ),
            },
            HostCreate::Watching { lobby, since } => {
                let id = lobby.id();
                let waited = now.saturating_duration_since(since);
                if !api.lobby_data(id, KEY_SC_TYPE).is_empty() {
                    tracing::info!(
                        "mm_host_create: ADOPTED by Seamless Co-op: lobby {} carries {KEY_SC_TYPE} after {:.1} s; left alone from now on (env.sc={env_sc:?})",
                        idt(id),
                        waited.as_secs_f32()
                    );
                    g.create = HostCreate::Adopted(id);
                } else if waited >= HOST_ADOPT_WITHIN {
                    tracing::warn!(
                        "mm_host_create: not adopted within {} s: lobby {} never carried {KEY_SC_TYPE}; LeaveLobby, falling back to the Invite Members prompt",
                        HOST_ADOPT_WITHIN.as_secs(),
                        idt(id)
                    );
                    api.leave(&lobby);
                } else {
                    g.create = HostCreate::Watching { lobby, since };
                }
            }
            HostCreate::Adopted(id) => g.create = HostCreate::Adopted(id),
            HostCreate::Done => {}
        }
    }

    /// One gate pass for a ready match (called from `step` when `mm_join` is on).
    pub(super) fn gate_step(&mut self, api: &mut dyn QueueSteam, mut r: Box<Ready>, now: Instant) -> State {
        if now < r.gate.next_eval && !self.dirty {
            return State::MatchReady(r);
        }
        r.gate.next_eval = now + GATE_EVAL;
        let own = api.own_id();
        let host = r.host;
        let is_host = host != 0 && own == host;
        let total = r.roster.len();

        // 0. Phase: the match ends when the player is back at the Roundtable after an expedition.
        match self.env.phase {
            Some(Phase::Expedition) => {
                if !r.gate.saw_expedition {
                    tracing::info!("mm_join: local player is in an expedition (team_type 2)");
                }
                r.gate.saw_expedition = true;
                r.gate.roundtable_since = None;
            }
            Some(Phase::Roundtable) if r.gate.saw_expedition => {
                let since = *r.gate.roundtable_since.get_or_insert(now);
                if now.saturating_duration_since(since) >= ROUNDTABLE_DEBOUNCE {
                    return self.end_match(api, r, "back at the Roundtable after the expedition (team_type 1)", now);
                }
            }
            _ => {}
        }

        // 1. The queue lobby: still in it?
        if let Some(q) = r.lobby.clone() {
            let ids = members_of(api, q.id());
            if ids.is_empty() || !ids.contains(&own) {
                tracing::warn!("mm_join: queue lobby {} is gone or we are no longer in it; carrying on without it", idt(q.id()));
                r.lobby = None;
            }
        }

        // 2. Joiners read what the host published.
        if let (Some(q), false) = (r.lobby.clone(), is_host) {
            match PublishedScLobby::from_queue(api, &q, host) {
                Ok(Some(p)) => {
                    if r.gate.published.as_ref() != Some(&p) {
                        tracing::info!("mm_join: the host published sc_lobby={} (host {})", idt(p.id()), idt(p.host()));
                        r.gate.link_tried = None;
                        r.gate.published = Some(p);
                    }
                }
                Ok(None) => {}
                // MM-3a: a verified value exists already (the owner moved after the host left): keep it, nothing above debug.
                Err(e) if r.gate.published.is_some() => tracing::debug!("mm_join: keeping the earlier sc_lobby: {e}"),
                Err(e) => {
                    if r.gate.warned_publish.as_deref() != Some(e.as_str()) {
                        tracing::warn!("mm_join: not using the published sc_lobby: {e}");
                        r.gate.warned_publish = Some(e);
                    }
                }
            }
        }

        // 3. Where is the host's lobby, who is in it?
        let env_sc = self.env.sc.clone();
        let target: Option<u64> = if is_host {
            env_sc.as_ref().map(ScLobby::id)
        } else {
            r.gate.published.as_ref().map(PublishedScLobby::id)
        };
        if is_host {
            if let Some(sc) = &env_sc {
                r.gate.sc_last = Some(sc.id());
            }
        }
        r.gate.sc_owner = env_sc.as_ref().is_some_and(|sc| api.owner(sc.id()) == own);
        let members: Vec<u64> = match (target, &env_sc) {
            (Some(t), Some(sc)) if sc.id() == t => members_of(api, t),
            _ => Vec::new(),
        };
        let i_am_in = members.contains(&own);

        // 3b. MM-3a: after the expedition Seamless Co-op's lobby is gone. Not in the Expedition phase and in no lobby of this
        // match (S9 5c: FS-MM1 stayed on "IN EXPEDITION" with lobby A gone) for SC_GONE_END -> the match is over.
        let in_match_sc = match (target, &env_sc) {
            (Some(t), Some(sc)) => sc.id() == t,
            _ => false,
        };
        if r.gate.saw_expedition && self.env.phase != Some(Phase::Expedition) && !in_match_sc {
            let since = *r.gate.sc_gone_since.get_or_insert(now);
            if now.saturating_duration_since(since) >= SC_GONE_END {
                return self.end_match(api, r, "expedition over: Seamless Co-op lobby gone", now);
            }
        } else {
            r.gate.sc_gone_since = None;
        }
        // 3c. MM-3e: the host's mod creates the SC lobby itself (once) when the game has not made one.
        self.host_create_step(api, &mut r.gate, is_host, env_sc.as_ref().map(ScLobby::id), now);
        let present: Vec<u64> = r.roster.iter().map(|e| e.id).filter(|id| members.contains(id)).collect();
        let absent: Vec<u64> = r.roster.iter().map(|e| e.id).filter(|id| !members.contains(id)).collect();
        let steam_all = i_am_in && total > 0 && absent.is_empty();
        // MM-3f: Steam membership alone is not enough (S9 5f): the ChrSet must hold the whole roster too.
        let local_all = everyone_loaded(steam_all, self.env.players, total);

        // 4. Host: publish, invite.
        if is_host {
            if let (Some(q), Some(sc)) = (r.lobby.clone(), env_sc.clone()) {
                if r.gate.pub_sc != Some(sc.id()) {
                    api.set_data(&q, proto::K_SC_LOBBY, &sc.id().to_string());
                    tracing::info!("mm_join: host publishes sc_lobby={}", idt(sc.id()));
                    r.gate.pub_sc = Some(sc.id());
                    r.gate.pub_at = Some(now);
                }
                let pres = proto::encode_present(present.len(), total);
                if r.gate.pub_present != pres {
                    api.set_data(&q, proto::K_PRESENT, &pres);
                    r.gate.pub_present = pres;
                }
            }
            if let Some(sc) = env_sc.clone() {
                if r.gate.all_present.is_none() && self.cfg.join_auto.invites() && self.env.guard.is_ok() {
                    let due = r.gate.invite_retry_at.is_none_or(|t| now >= t);
                    let todo: Vec<u64> = absent.iter().copied().filter(|id| *id != own && !r.gate.invited.contains(id)).collect();
                    if due && !todo.is_empty() {
                        let all_ok = Self::send_invites(api, &sc, &todo, "automatic", &mut r.gate);
                        r.gate.invite_retry_at = (!all_ok).then(|| now + INVITE_RETRY);
                    }
                }
            }
            if let Some(q) = r.lobby.clone() {
                let inv = proto::encode_ids(&r.gate.invited.iter().copied().collect::<Vec<_>>());
                if r.gate.pub_invited != inv {
                    api.set_data(&q, proto::K_INVITED, &inv);
                    r.gate.pub_invited = inv;
                }
            }
        }

        // 5. Joiner: automatic link (method a), once per published lobby.
        if !is_host && self.cfg.join_auto.links() && !i_am_in && r.gate.all_present.is_none() {
            if let Some(p) = r.gate.published.clone() {
                if r.gate.link_tried != Some(p.id()) {
                    r.gate.link_tried = Some(p.id());
                    self.run_link(api, &p, "automatic");
                }
            }
        }

        // 6. Direct JoinLobby (method c) result.
        if let Some(pd) = r.gate.direct {
            match api.poll(pd.handle, CallKind::Enter) {
                CallPoll::Done(CallData::Enter { lobby, response, locked }) => {
                    tracing::info!(
                        "mm_join: method (c) JoinLobby result: lobby={} EChatRoomEnterResponse={response} (1 = success) locked={locked}",
                        idt(lobby)
                    );
                    r.gate.direct = None;
                }
                CallPoll::Done(other) => {
                    tracing::info!("mm_join: method (c) JoinLobby returned {other:?}");
                    r.gate.direct = None;
                }
                CallPoll::Failed { reason, invalid_handle } => {
                    tracing::warn!("mm_join: method (c) JoinLobby call failed: {reason} (invalid handle: {invalid_handle})");
                    r.gate.direct = None;
                }
                CallPoll::Pending if now.saturating_duration_since(pd.since) >= DIRECT_TIMEOUT => {
                    tracing::warn!("mm_join: method (c) JoinLobby: no result after {} s", DIRECT_TIMEOUT.as_secs());
                    r.gate.direct = None;
                }
                CallPoll::Pending => {}
            }
        }

        // 7. Everybody here: leave the queue lobby; the match stays.
        if local_all && r.gate.all_present.is_none() {
            r.gate.all_present = Some(total);
            tracing::info!("mm_join: ALL PRESENT {total}/{total} in SC lobby {} (members {:?})", idt(target.unwrap_or(0)), members);
            if let Some(q) = r.lobby.take() {
                // Deliberately NOT `leave_lobby`: the party leader's `nrpvp_q` stays until the match ends, so followers that have not
                // noticed ALL PRESENT yet do not read its removal as "the leader's queue ended".
                api.leave(&q);
                tracing::info!("mm_join: left queue lobby {}; the match, roster and team size override stay", idt(q.id()));
            }
        }

        // 8. Status and evidence.
        let names = |ids: &[u64], api: &dyn QueueSteam| ids.iter().map(|i| api.persona(*i)).collect::<Vec<_>>().join(", ");
        let others_absent: Vec<u64> = absent.iter().copied().filter(|i| *i != own).collect();
        // The roster members other than ourselves (for the "load in" line, where nobody is absent from the Steam lobby).
        let others_absent_roster: Vec<u64> = r.roster.iter().map(|e| e.id).filter(|i| *i != own).collect();
        let pub_present = r.lobby.as_ref().map(|q| api.lobby_data(q.id(), proto::K_PRESENT)).and_then(|s| proto::parse_present(&s));
        let shown_present = if target.is_some() && !members.is_empty() { Some((present.len(), total)) } else { pub_present };
        let count = shown_present.map_or(String::new(), |(n, t)| format!(" ({n}/{t} present)"));
        // MM-3e: our created lobby was adopted, we published it, and nobody of the roster entered within 45 s: ask for the manual invite
        // (the lobby stays ours to keep).
        let no_arrival = is_host
            && matches!(r.gate.create, HostCreate::Adopted(_))
            && r.gate.all_present.is_none()
            && !others_absent.is_empty()
            && present.iter().all(|i| *i == own)
            && r.gate.pub_at.is_some_and(|t| now.saturating_duration_since(t) >= NO_ARRIVAL);
        if no_arrival && !r.gate.no_arrival_logged {
            r.gate.no_arrival_logged = true;
            tracing::warn!(
                "mm_host_create: no roster member entered our lobby within {} s of publishing sc_lobby; asking the host to open Invite Members (the lobby is kept)",
                NO_ARRIVAL.as_secs()
            );
        }
        r.gate.line = if r.gate.saw_expedition {
            "IN EXPEDITION - match active (Leave, or going back to the Roundtable, ends it)".to_string()
        } else if let Some(n) = r.gate.all_present {
            if is_host {
                format!("ALL PRESENT {n}/{n} - host: start the expedition")
            } else {
                "ALL PRESENT - waiting for the host to start".to_string()
            }
        } else if host == 0 {
            "MATCH READY - the roster names no host".to_string()
        } else if steam_all && r.gate.all_present.is_none() {
            // MM-3f: in the Steam lobby with everyone, but the game has not brought them all into the world yet.
            if is_host {
                format!("in the lobby {total}/{total}, waiting for {} to load in", names(&others_absent_roster, api))
            } else {
                "in the host's lobby, loading in...".to_string()
            }
        } else if is_host && env_sc.is_none() && r.gate.creating() {
            "creating the Seamless Co-op lobby...".to_string()
        } else if no_arrival {
            format!("{} did not arrive: open Invite Members and invite them", names(&others_absent, api))
        } else if is_host && env_sc.is_none() && self.cfg.quiet_invite {
            "MATCH READY - press Matchmaking Settings > Invite Members (no Steam window will open)".to_string()
        } else if is_host && env_sc.is_none() {
            format!(
                "host has no Seamless Co-op lobby yet: open Matchmaking Settings > Invite Members and invite {}",
                if others_absent.is_empty() { "the other players".to_string() } else { names(&others_absent, api) }
            )
        } else if is_host {
            format!("MATCH READY - waiting for {} to enter your Seamless Co-op lobby{count}", names(&others_absent, api))
        } else if r.gate.published.is_none() {
            format!("MATCH READY - waiting for the host's Seamless Co-op lobby{count}")
        } else if !i_am_in {
            let invited_me =
                r.lobby.as_ref().is_some_and(|q| proto::parse_ids(&api.lobby_data(q.id(), proto::K_INVITED)).contains(&own));
            if self.cfg.join_auto.links() && r.gate.link_tried.is_some() {
                // MM-3a: the automatic link replaces the accept click; an invite may still pop up.
                format!("joining the host's lobby... (if a Steam invite pops up, accept it){count}")
            } else if invited_me {
                format!("host invited you: accept the Steam invite (Shift+Tab){count}")
            } else if r.gate.link_tried.is_some() {
                format!("opened the join link: waiting to enter the host's lobby{count}")
            } else {
                format!("host's lobby is ready: waiting for the invite{count}")
            }
        } else {
            format!("MATCH READY - in the host's lobby, waiting for the others{count}")
        };

        let phase = self.env.phase.map_or("?", |p| if p == Phase::Expedition { "expedition" } else { "roundtable" });
        let summary = format!(
            "host={} is_host={is_host} sc_target={:?} i_am_in={i_am_in} sc_members={:?} roster={:?} present={}/{total} queue_lobby={:?} phase={phase} invited={:?}",
            idt(host),
            target,
            members,
            r.roster.iter().map(|e| e.id).collect::<Vec<_>>(),
            present.len(),
            r.lobby.as_ref().map(QueueLobby::id),
            r.gate.invited
        );
        if summary != r.gate.last_log || now >= r.gate.next_log {
            tracing::info!("mm_join: gate: {summary}");
            r.gate.last_log = summary;
            r.gate.next_log = now + SUMMARY_EVERY;
        }
        State::MatchReady(r)
    }

    /// Method (b) for each id. `true` when Steam accepted all of them.
    fn send_invites(api: &mut dyn QueueSteam, sc: &ScLobby, ids: &[u64], why: &str, g: &mut Gate) -> bool {
        let mut all = true;
        for &id in ids {
            let ok = api.invite_to_sc(sc, id);
            tracing::info!(
                "mm_join: method (b) InviteUserToLobby ({why}): sc_lobby={} joiner={} ({}) -> {ok}",
                idt(sc.id()),
                idt(id),
                api.persona(id)
            );
            if ok {
                g.invited.insert(id);
            } else {
                all = false;
            }
        }
        all
    }

    /// Method (a): the guarded `steam://joinlobby` link.
    fn run_link(&mut self, api: &mut dyn QueueSteam, p: &PublishedScLobby, why: &str) {
        let app = api.app_id();
        let overlay = api.overlay_enabled();
        let url = join::link_url(app, p.id(), p.host());
        match join::link_guard(app, overlay, &self.env.guard) {
            Err(e) => tracing::warn!(
                "mm_join: method (a) link ({why}) REFUSED: {e}; inputs: app_id={app} overlay_enabled={overlay} sc_lobby={} host={} url={url}",
                idt(p.id()),
                idt(p.host())
            ),
            Ok(()) => {
                let r = api.open_url(&url);
                tracing::info!(
                    "mm_join: method (a) link ({why}): app_id={app} overlay_enabled={overlay} sc_lobby={} host={} url={url} -> {r:?}",
                    idt(p.id()),
                    idt(p.host())
                );
            }
        }
    }

    /// A dev button. Only meaningful while the match is ready and `mm_join` is on.
    pub(crate) fn join_cmd(&mut self, api: &mut dyn QueueSteam, c: JoinCmd, now: Instant) {
        let own = api.own_id();
        let env_sc = self.env.sc.clone();
        let guard = self.env.guard.clone();
        if !self.cfg.join {
            tracing::info!("mm_join: {c:?} ignored: mm_join is off");
            return;
        }
        let (host, published) = match &self.state {
            State::MatchReady(r) => (r.host, r.gate.published.clone()),
            _ => {
                tracing::info!("mm_join: {c:?} ignored: no ready match");
                return;
            }
        };
        if let Err(e) = &guard {
            tracing::warn!("mm_join: {c:?} REFUSED: safety guard: {e}");
            return;
        }
        let is_host = host != 0 && own == host;
        match c {
            JoinCmd::Invite => {
                let Some(sc) = env_sc else {
                    tracing::info!("mm_join: method (b) invite: no Seamless Co-op lobby of ours is known");
                    return;
                };
                let members = members_of(api, sc.id());
                let State::MatchReady(r) = &mut self.state else { return };
                let absent: Vec<u64> = r.roster.iter().map(|e| e.id).filter(|i| *i != own && !members.contains(i)).collect();
                tracing::info!(
                    "mm_join: method (b) button: sc_lobby={} members={:?} absent roster members={:?} is_host={is_host}",
                    idt(sc.id()),
                    members,
                    absent
                );
                r.gate.invited.clear();
                let ok = Self::send_invites(api, &sc, &absent, "button", &mut r.gate);
                r.gate.invite_retry_at = (!ok).then(|| now + INVITE_RETRY);
            }
            JoinCmd::Link | JoinCmd::Direct => {
                if is_host {
                    tracing::info!("mm_join: {c:?} ignored: we are the host and already own the lobby");
                    return;
                }
                let Some(p) = published else {
                    tracing::info!("mm_join: {c:?}: the host has not published sc_lobby (or we cannot verify it)");
                    return;
                };
                if c == JoinCmd::Link {
                    if let State::MatchReady(r) = &mut self.state {
                        r.gate.link_tried = Some(p.id());
                    }
                    self.run_link(api, &p, "button");
                } else {
                    match api.begin_join_sc(&p) {
                        Ok(handle) => {
                            tracing::info!(
                                "mm_join: method (c) button: JoinLobby sc_lobby={} host={} -> handle {handle}",
                                idt(p.id()),
                                idt(p.host())
                            );
                            if let State::MatchReady(r) = &mut self.state {
                                r.gate.direct = Some(Pending { handle, since: now });
                            }
                        }
                        Err(e) => tracing::warn!("mm_join: method (c) button: JoinLobby not started: {e}"),
                    }
                }
            }
            JoinCmd::UnlockPublic | JoinCmd::UnlockInvisible | JoinCmd::RelockPrivate => {
                let ty = match c {
                    JoinCmd::UnlockPublic => ScType::Public,
                    JoinCmd::UnlockInvisible => ScType::Invisible,
                    _ => ScType::Private,
                };
                let Some(sc) = env_sc else {
                    tracing::info!("mm_join: method (d) {}: no Seamless Co-op lobby of ours is known", ty.label());
                    return;
                };
                let owner = api.owner(sc.id());
                if owner != own {
                    tracing::info!(
                        "mm_join: method (d) {}: refused, we do not own SC lobby {} (owner {})",
                        ty.label(),
                        idt(sc.id()),
                        idt(owner)
                    );
                    return;
                }
                let ok = api.set_sc_type(&sc, ty);
                if ty == ScType::Private {
                    tracing::info!(
                        "mm_join: method (d) relock: SetLobbyType(SC lobby {}, Private) -> {ok}; the ORIGINAL type of Seamless Co-op's lobby is UNKNOWN (it reported locked=true), so this is not guaranteed to restore it",
                        idt(sc.id())
                    );
                } else {
                    tracing::info!("mm_join: method (d) unlock: SetLobbyType(SC lobby {}, {}) -> {ok}", idt(sc.id()), ty.label());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{MachineCfg, Party, Ticket};
    use super::*;
    use crate::mm::join::JoinAuto;
    use crate::mm::proto::RosterEntry;
    use crate::mm::Mode;
    use crate::steam::queue::fake::{FakeSteam, Write};
    use crate::steam::queue::{LobbyText, KEY_SC_TYPE};

    const HOST: u64 = 7001;
    const JOINER: u64 = 7002;
    const Q: u64 = 77;
    const SC: u64 = 5555;

    fn machine(join: bool, auto: JoinAuto) -> Machine {
        Machine::new(MachineCfg { need: 2, game: "1.3.3.0".into(), sc: "1.1.3".into(), join, join_auto: auto, host_create: false, quiet_invite: false })
    }

    fn ready(m: &mut Machine, own: u64, t: Instant) {
        m.state = State::MatchReady(Box::new(Ready {
            ticket: Ticket { mode: Mode::Solo, party: Party { id: own, size: 1, leader: true, sc: None } },
            queue_id: Q,
            lobby: Some(QueueLobby::for_test(Q)),
            roster: vec![RosterEntry { id: HOST, party: HOST }, RosterEntry { id: JOINER, party: JOINER }],
            host_name: "h".into(),
            host: HOST,
            gate: Gate::new(t),
        }));
    }

    fn world(own: u64) -> FakeSteam {
        let mut s = FakeSteam::new(own);
        s.add_lobby(Q, HOST, 2, &[HOST, JOINER], &[("nrpvp", "1"), ("host", "7001"), ("state", "go")]);
        s
    }

    fn add_sc(s: &mut FakeSteam, members: &[u64]) {
        s.add_lobby(SC, HOST, 6, members, &[(KEY_SC_TYPE, "x")]);
    }

    fn sc() -> Option<ScLobby> {
        Some(ScLobby::for_test(SC))
    }

    fn tick(m: &mut Machine, s: &mut FakeSteam, t: &mut Instant, n: usize) {
        for _ in 0..n {
            *t += Duration::from_millis(600);
            m.tick(s, *t);
        }
    }

    fn line(m: &Machine) -> String {
        m.status().line
    }

    fn sc_writes(s: &FakeSteam) -> Vec<&Write> {
        s.writes_to(SC)
    }

    #[test]
    fn a_host_without_an_sc_lobby_says_what_to_do_and_keeps_waiting() {
        let mut m = machine(true, JoinAuto::Invite);
        let mut s = world(HOST);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 3);
        assert_eq!(
            line(&m),
            "host has no Seamless Co-op lobby yet: open Matchmaking Settings > Invite Members and invite P7002"
        );
        assert!(s.lobby_data(Q, "sc_lobby").is_empty());
        assert!(!s.writes.iter().any(|w| matches!(w, Write::Invite(..))));
        assert!(m.status().join_ui && !m.status().sc_known);
        // The moment an SC lobby appears the host publishes it and invites.
        add_sc(&mut s, &[HOST]);
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        assert_eq!(s.lobby_data(Q, "sc_lobby"), SC.to_string());
        assert_eq!(s.lobby_data(Q, "present"), "1/2");
        assert!(m.status().sc_known && m.status().sc_owner);
        assert!(line(&m).starts_with("MATCH READY - waiting for P7002"), "{}", line(&m));
    }

    fn quiet_machine() -> Machine {
        Machine::new(MachineCfg {
            need: 2,
            game: "1.3.3.0".into(),
            sc: "1.1.3".into(),
            join: true,
            join_auto: JoinAuto::Invite,
            host_create: false,
            quiet_invite: true,
        })
    }

    #[test]
    fn quiet_invite_is_wanted_only_for_the_host_of_a_ready_match_without_an_sc_lobby() {
        let t = Instant::now();
        // Not in a match: no.
        let mut m = quiet_machine();
        assert!(!m.quiet_invite_wanted(HOST));
        // Host, MATCH READY, no SC lobby yet: yes.
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        assert!(m.quiet_invite_wanted(HOST));
        // The same match seen by the joiner: no (Duo / Squad invites and the joiner's own menus are untouched).
        assert!(!m.quiet_invite_wanted(JOINER));
        // The game's lobby exists: no.
        m.set_env(sc(), Ok(()), None, 2);
        assert!(!m.quiet_invite_wanted(HOST));
        // Switched off in the config: no.
        let mut off = machine(true, JoinAuto::Invite);
        ready(&mut off, HOST, t);
        off.set_env(None, Ok(()), None, 2);
        assert!(!off.quiet_invite_wanted(HOST));
        // mm_join off: no.
        let mut nojoin = Machine::new(MachineCfg {
            need: 2,
            game: "1.3.3.0".into(),
            sc: "1.1.3".into(),
            join: false,
            join_auto: JoinAuto::Invite,
            host_create: false,
            quiet_invite: true,
        });
        ready(&mut nojoin, HOST, t);
        assert!(!nojoin.quiet_invite_wanted(HOST));
    }

    #[test]
    fn quiet_invite_stops_being_wanted_once_everybody_is_present_or_the_expedition_started() {
        let mut m = quiet_machine();
        let mut s = world(HOST);
        add_sc(&mut s, &[HOST, JOINER]);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), Some(Phase::Expedition), 2);
        tick(&mut m, &mut s, &mut t, 2);
        assert!(!m.quiet_invite_wanted(HOST), "expedition started");
        let mut m = quiet_machine();
        ready(&mut m, HOST, t);
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        m.set_env(None, Ok(()), None, 2);
        assert!(!m.quiet_invite_wanted(HOST), "ALL PRESENT reached");
    }

    #[test]
    fn the_host_line_asks_for_invite_members_without_a_steam_window_when_quiet_invite_is_on() {
        let mut m = quiet_machine();
        let mut s = world(HOST);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 3);
        assert_eq!(line(&m), "MATCH READY - press Matchmaking Settings > Invite Members (no Steam window will open)");
        // Then the game's lobby appears and the gate runs as before.
        add_sc(&mut s, &[HOST]);
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        assert_eq!(s.lobby_data(Q, "sc_lobby"), SC.to_string());
        assert!(line(&m).starts_with("MATCH READY - waiting for P7002"), "{}", line(&m));
    }

    #[test]
    fn everyone_loaded_needs_the_steam_lobby_and_the_chrset_count() {
        assert!(everyone_loaded(true, 2, 2));
        assert!(everyone_loaded(true, 3, 2));
        assert!(!everyone_loaded(true, 1, 2), "FS-MM2: Steam had both, the ChrSet stayed at 1");
        assert!(!everyone_loaded(true, 0, 2), "never read");
        assert!(!everyone_loaded(false, 2, 2));
        assert!(!everyone_loaded(true, 0, 0));
    }

    #[test]
    fn all_present_waits_for_the_chrset_to_hold_the_roster() {
        // Host.
        let mut m = machine(true, JoinAuto::Invite);
        let mut s = world(HOST);
        add_sc(&mut s, &[HOST, JOINER]);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(sc(), Ok(()), None, 1);
        tick(&mut m, &mut s, &mut t, 4);
        assert_eq!(line(&m), "in the lobby 2/2, waiting for P7002 to load in");
        assert!(!s.writes.contains(&Write::Leave(Q)), "the queue lobby is kept until everybody is loaded");
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        assert_eq!(line(&m), "ALL PRESENT 2/2 - host: start the expedition");
        assert!(s.writes.contains(&Write::Leave(Q)));
        // Joiner.
        let mut m = machine(true, JoinAuto::Invite);
        let mut s = world(JOINER);
        add_sc(&mut s, &[HOST, JOINER]);
        s.lobbies.get_mut(&Q).expect("q").data.insert("sc_lobby".into(), SC.to_string());
        let mut t = Instant::now();
        ready(&mut m, JOINER, t);
        m.set_env(sc(), Ok(()), None, 1);
        tick(&mut m, &mut s, &mut t, 4);
        assert_eq!(line(&m), "in the host's lobby, loading in...");
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        assert_eq!(line(&m), "ALL PRESENT - waiting for the host to start");
    }

    #[test]
    fn the_host_invites_each_absent_member_once_and_then_everybody_leaves_the_queue_lobby() {
        let mut m = machine(true, JoinAuto::Invite);
        let mut s = world(HOST);
        add_sc(&mut s, &[HOST]);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 6);
        let invites: Vec<_> = s.writes.iter().filter(|w| matches!(w, Write::Invite(..))).collect();
        assert_eq!(invites, vec![&Write::Invite(SC, JOINER)], "one invite, once");
        assert_eq!(s.lobby_data(Q, "invited"), JOINER.to_string());
        // The joiner arrives in the host lobby.
        s.lobbies.get_mut(&SC).expect("sc").members.push(JOINER);
        tick(&mut m, &mut s, &mut t, 2);
        assert_eq!(line(&m), "ALL PRESENT 2/2 - host: start the expedition");
        assert!(s.writes.contains(&Write::Leave(Q)), "left the queue lobby");
        assert_eq!(m.team_override(), Some(1), "the team size override stays");
        assert!(matches!(m.state, State::MatchReady(_)));
        // The only thing done to SC lobby was the invite.
        assert_eq!(sc_writes(&s), vec![&Write::Invite(SC, JOINER)]);
    }

    #[test]
    fn other_auto_modes_do_not_make_the_host_invite() {
        for auto in [JoinAuto::None, JoinAuto::Link] {
            let mut m = machine(true, auto);
            let mut s = world(HOST);
            add_sc(&mut s, &[HOST]);
            let mut t = Instant::now();
            ready(&mut m, HOST, t);
            m.set_env(sc(), Ok(()), None, 2);
            tick(&mut m, &mut s, &mut t, 4);
            assert!(sc_writes(&s).is_empty(), "{auto:?}: {:?}", s.writes);
        }
        // `invite+link` does invite.
        let mut m = machine(true, JoinAuto::InviteLink);
        let mut s = world(HOST);
        add_sc(&mut s, &[HOST]);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 3);
        assert_eq!(sc_writes(&s), vec![&Write::Invite(SC, JOINER)]);
    }

    #[test]
    fn a_failed_invite_is_retried_later_not_every_tick() {
        let mut m = machine(true, JoinAuto::Invite);
        let mut s = world(HOST);
        s.invite_ok = false;
        add_sc(&mut s, &[HOST]);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 6); // 3.6 s
        assert_eq!(s.writes.iter().filter(|w| matches!(w, Write::Invite(..))).count(), 1);
        tick(&mut m, &mut s, &mut t, 6); // past the 5 s retry delay
        assert!(s.writes.iter().filter(|w| matches!(w, Write::Invite(..))).count() >= 2);
    }

    #[test]
    fn a_member_who_is_already_in_the_host_lobby_needs_no_join_action() {
        // Scenario 2: both are already in the same SC lobby. The joiner view.
        let mut m = machine(true, JoinAuto::InviteLink);
        let mut s = world(JOINER);
        add_sc(&mut s, &[HOST, JOINER]);
        s.lobbies.get_mut(&Q).expect("q").data.insert("sc_lobby".into(), SC.to_string());
        let mut t = Instant::now();
        ready(&mut m, JOINER, t);
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 3);
        assert_eq!(line(&m), "ALL PRESENT - waiting for the host to start");
        assert!(s.writes.contains(&Write::Leave(Q)));
        assert!(!s.writes.iter().any(|w| matches!(w, Write::OpenUrl(_) | Write::JoinSc(_) | Write::Invite(..) | Write::SetType(..))));
        assert_eq!(m.team_override(), Some(1));
        // ... and the host view: nothing to invite.
        let mut mh = machine(true, JoinAuto::InviteLink);
        let mut sh = world(HOST);
        add_sc(&mut sh, &[HOST, JOINER]);
        let mut th = Instant::now();
        ready(&mut mh, HOST, th);
        mh.set_env(sc(), Ok(()), None, 2);
        tick(&mut mh, &mut sh, &mut th, 3);
        assert_eq!(line(&mh), "ALL PRESENT 2/2 - host: start the expedition");
        assert!(sc_writes(&sh).is_empty(), "{:?}", sh.writes);
    }

    #[test]
    fn a_joiner_sees_the_invite_message_and_the_automatic_link_runs_once() {
        let mut m = machine(true, JoinAuto::InviteLink);
        let mut s = world(JOINER);
        let mut t = Instant::now();
        ready(&mut m, JOINER, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        assert_eq!(line(&m), "MATCH READY - waiting for the host's Seamless Co-op lobby");
        // The host publishes its lobby and the list of invited players.
        let q = s.lobbies.get_mut(&Q).expect("q");
        q.data.insert("sc_lobby".into(), SC.to_string());
        q.data.insert("invited".into(), JOINER.to_string());
        tick(&mut m, &mut s, &mut t, 4);
        let urls: Vec<_> = s.writes.iter().filter(|w| matches!(w, Write::OpenUrl(_))).collect();
        assert_eq!(urls, vec![&Write::OpenUrl("steam://joinlobby/2622380/5555/7001".to_string())], "once");
        // MM-3a: with the automatic link on, no accept click is asked for.
        assert_eq!(line(&m), "joining the host's lobby... (if a Steam invite pops up, accept it)");
        assert!(m.status().sc_known && !m.status().sc_owner);
        assert!(sc_writes(&s).is_empty());
    }

    #[test]
    fn a_joiner_without_links_is_still_told_to_accept_the_invite() {
        let mut m = machine(true, JoinAuto::Invite);
        let mut s = world(JOINER);
        let q = s.lobbies.get_mut(&Q).expect("q");
        q.data.insert("sc_lobby".into(), SC.to_string());
        q.data.insert("invited".into(), JOINER.to_string());
        let mut t = Instant::now();
        ready(&mut m, JOINER, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 3);
        assert_eq!(line(&m), "host invited you: accept the Steam invite (Shift+Tab)");
    }

    #[test]
    fn a_verified_sc_lobby_is_kept_when_the_queue_lobby_owner_moves_later() {
        let mut m = machine(true, JoinAuto::Invite);
        let mut s = world(JOINER);
        s.lobbies.get_mut(&Q).expect("q").data.insert("sc_lobby".into(), SC.to_string());
        let mut t = Instant::now();
        ready(&mut m, JOINER, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        assert!(m.status().sc_known);
        // The host leaves the queue lobby: Steam hands ownership to the joiner. The earlier value stays.
        s.lobbies.get_mut(&Q).expect("q").owner = JOINER;
        tick(&mut m, &mut s, &mut t, 3);
        assert!(m.status().sc_known);
        if let State::MatchReady(r) = &m.state {
            assert_eq!(r.gate.published.as_ref().map(PublishedScLobby::id), Some(SC));
            assert!(r.gate.warned_publish.is_none(), "no warning for the moved owner");
        } else {
            panic!("match should still be ready");
        }
    }

    #[test]
    fn the_match_ends_15_s_after_the_expedition_when_the_sc_lobby_is_gone() {
        let mut m = machine(true, JoinAuto::None);
        let mut s = world(HOST);
        add_sc(&mut s, &[HOST, JOINER]);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(sc(), Ok(()), Some(Phase::Expedition), 2);
        tick(&mut m, &mut s, &mut t, 3);
        // Lobby A is gone, the phase is unknown (loading / title): still the match for a while, then over.
        s.lobbies.remove(&SC);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 20); // 12 s
        assert!(matches!(m.state, State::MatchReady(_)));
        tick(&mut m, &mut s, &mut t, 8); // 16.8 s
        assert!(m.is_idle());
        assert_eq!(m.team_override(), None);
    }

    #[test]
    fn a_flickering_sc_read_during_the_expedition_never_ends_the_match() {
        let mut m = machine(true, JoinAuto::None);
        let mut s = world(HOST);
        add_sc(&mut s, &[HOST, JOINER]);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        for i in 0..100 {
            m.set_env(if i % 3 == 0 { None } else { sc() }, Ok(()), Some(Phase::Expedition), 2);
            tick(&mut m, &mut s, &mut t, 1);
        }
        // Even a long stretch with no SC lobby at all, while the phase says Expedition.
        m.set_env(None, Ok(()), Some(Phase::Expedition), 2);
        tick(&mut m, &mut s, &mut t, 60);
        assert!(matches!(m.state, State::MatchReady(_)));
        // And the clock restarts when the lobby is seen again outside the expedition.
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 20); // 12 s
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 1);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 20); // 12 s again
        assert!(matches!(m.state, State::MatchReady(_)));
    }

    #[test]
    fn no_sc_lobby_does_not_end_the_match_before_any_expedition() {
        let mut m = machine(true, JoinAuto::None);
        let mut s = world(HOST);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 100); // 60 s
        assert!(matches!(m.state, State::MatchReady(_)));
    }

    #[test]
    fn with_auto_invite_only_the_joiner_never_opens_a_link() {
        let mut m = machine(true, JoinAuto::Invite);
        let mut s = world(JOINER);
        s.lobbies.get_mut(&Q).expect("q").data.insert("sc_lobby".into(), SC.to_string());
        let mut t = Instant::now();
        ready(&mut m, JOINER, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 4);
        assert!(!s.writes.iter().any(|w| matches!(w, Write::OpenUrl(_))));
        assert_eq!(line(&m), "host's lobby is ready: waiting for the invite");
    }

    #[test]
    fn the_link_is_refused_unless_app_id_overlay_and_guard_all_pass() {
        let cases: [(u32, bool, Result<(), String>); 3] =
            [(480, true, Ok(())), (2622380, false, Ok(())), (2622380, true, Err("no nrsc".into()))];
        for (app, overlay, guard) in cases {
            let mut m = machine(true, JoinAuto::Link);
            let mut s = world(JOINER);
            s.app_id = app;
            s.overlay = overlay;
            s.lobbies.get_mut(&Q).expect("q").data.insert("sc_lobby".into(), SC.to_string());
            let mut t = Instant::now();
            ready(&mut m, JOINER, t);
            m.set_env(None, guard.clone(), None, 2);
            tick(&mut m, &mut s, &mut t, 4);
            assert!(!s.writes.iter().any(|w| matches!(w, Write::OpenUrl(_))), "{app} {overlay} {guard:?}");
        }
    }

    #[test]
    fn an_sc_lobby_published_by_someone_who_is_not_the_host_is_not_trusted() {
        let mut m = machine(true, JoinAuto::InviteLink);
        let mut s = world(JOINER);
        let q = s.lobbies.get_mut(&Q).expect("q");
        q.data.insert("sc_lobby".into(), SC.to_string());
        q.owner = 9999; // ownership moved to somebody else
        let mut t = Instant::now();
        ready(&mut m, JOINER, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 3);
        assert!(!s.writes.iter().any(|w| matches!(w, Write::OpenUrl(_))));
        assert!(!m.status().sc_known);
        // `sc_lobby` equal to the queue lobby itself is rejected too.
        let q = s.lobbies.get_mut(&Q).expect("q");
        q.owner = HOST;
        q.data.insert("sc_lobby".into(), Q.to_string());
        tick(&mut m, &mut s, &mut t, 2);
        assert!(!m.status().sc_known);
    }

    #[test]
    fn dev_buttons_run_one_method_each_and_respect_ownership() {
        // Joiner: (a) and (c) use the published lobby only.
        let mut m = machine(true, JoinAuto::None);
        let mut s = world(JOINER);
        add_sc(&mut s, &[HOST]);
        s.lobbies.get_mut(&Q).expect("q").data.insert("sc_lobby".into(), SC.to_string());
        let mut t = Instant::now();
        ready(&mut m, JOINER, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        assert!(sc_writes(&s).is_empty(), "auto none does nothing");
        m.join_cmd(&mut s, JoinCmd::Link, t);
        assert!(s.writes.contains(&Write::OpenUrl("steam://joinlobby/2622380/5555/7001".into())));
        m.join_cmd(&mut s, JoinCmd::Direct, t);
        assert!(s.writes.contains(&Write::JoinSc(SC)));
        tick(&mut m, &mut s, &mut t, 2); // polls the call result
        // (d) from a non-owner: refused.
        m.set_env(sc(), Ok(()), None, 2);
        m.join_cmd(&mut s, JoinCmd::UnlockPublic, t);
        assert!(!s.writes.iter().any(|w| matches!(w, Write::SetType(..))));
        // Host: (d) and relock work, (a)/(c) are ignored, (b) invites absent members only.
        let mut mh = machine(true, JoinAuto::None);
        let mut sh = world(HOST);
        add_sc(&mut sh, &[HOST]);
        let mut th = Instant::now();
        ready(&mut mh, HOST, th);
        mh.set_env(sc(), Ok(()), None, 2);
        tick(&mut mh, &mut sh, &mut th, 2);
        mh.join_cmd(&mut sh, JoinCmd::Link, th);
        mh.join_cmd(&mut sh, JoinCmd::Direct, th);
        assert!(!sh.writes.iter().any(|w| matches!(w, Write::OpenUrl(_) | Write::JoinSc(_))));
        mh.join_cmd(&mut sh, JoinCmd::UnlockPublic, th);
        mh.join_cmd(&mut sh, JoinCmd::UnlockInvisible, th);
        mh.join_cmd(&mut sh, JoinCmd::RelockPrivate, th);
        mh.join_cmd(&mut sh, JoinCmd::Invite, th);
        mh.join_cmd(&mut sh, JoinCmd::Invite, th); // re-invite
        assert_eq!(
            sc_writes(&sh),
            vec![
                &Write::SetType(SC, ScType::Public),
                &Write::SetType(SC, ScType::Invisible),
                &Write::SetType(SC, ScType::Private),
                &Write::Invite(SC, JOINER),
                &Write::Invite(SC, JOINER),
            ]
        );
    }

    #[test]
    fn buttons_are_refused_when_the_guard_fails_or_no_match_is_ready() {
        let mut m = machine(true, JoinAuto::None);
        let mut s = world(HOST);
        add_sc(&mut s, &[HOST]);
        let t = Instant::now();
        m.set_env(sc(), Ok(()), None, 2);
        m.join_cmd(&mut s, JoinCmd::Invite, t); // idle
        ready(&mut m, HOST, t);
        m.set_env(sc(), Err("EasyAntiCheat".into()), None, 2);
        m.join_cmd(&mut s, JoinCmd::Invite, t);
        m.join_cmd(&mut s, JoinCmd::UnlockPublic, t);
        assert!(sc_writes(&s).is_empty());
        // mm_join off: ignored too.
        let mut off = machine(false, JoinAuto::None);
        ready(&mut off, HOST, t);
        off.set_env(sc(), Ok(()), None, 2);
        off.join_cmd(&mut s, JoinCmd::Invite, t);
        assert!(sc_writes(&s).is_empty());
    }

    #[test]
    fn the_match_ends_back_at_the_roundtable_after_an_expedition_but_not_before() {
        let mut m = machine(true, JoinAuto::None);
        let mut s = world(HOST);
        add_sc(&mut s, &[HOST, JOINER]);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        // Roundtable before any expedition: the match stays.
        m.set_env(sc(), Ok(()), Some(Phase::Roundtable), 2);
        tick(&mut m, &mut s, &mut t, 20);
        assert!(matches!(m.state, State::MatchReady(_)));
        assert_eq!(m.team_override(), Some(1));
        // In the expedition: still the match, status says so.
        m.set_env(sc(), Ok(()), Some(Phase::Expedition), 2);
        tick(&mut m, &mut s, &mut t, 3);
        assert!(line(&m).starts_with("IN EXPEDITION"), "{}", line(&m));
        assert_eq!(m.team_override(), Some(1));
        // Loading screens (phase unknown) change nothing.
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 20);
        assert!(matches!(m.state, State::MatchReady(_)));
        // Back at the Roundtable: ends after the debounce.
        m.set_env(None, Ok(()), Some(Phase::Roundtable), 2);
        tick(&mut m, &mut s, &mut t, 3);
        assert!(matches!(m.state, State::MatchReady(_)), "debounce");
        tick(&mut m, &mut s, &mut t, 10);
        assert!(m.is_idle());
        assert_eq!(m.team_override(), None);
    }

    #[test]
    fn leave_keeps_working_before_and_after_the_queue_lobby_was_left() {
        let mut m = machine(true, JoinAuto::Invite);
        let mut s = world(HOST);
        add_sc(&mut s, &[HOST]);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        m.leave(&mut s, "test", t);
        assert!(m.is_idle() && s.writes.contains(&Write::Leave(Q)));
        // After ALL PRESENT: no second LeaveLobby.
        let mut m = machine(true, JoinAuto::Invite);
        let mut s = world(HOST);
        add_sc(&mut s, &[HOST, JOINER]);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 3);
        m.leave(&mut s, "test", t);
        assert!(m.is_idle());
        assert_eq!(s.writes.iter().filter(|w| matches!(w, Write::Leave(_))).count(), 1);
        assert_eq!(m.team_override(), None);
    }

    #[test]
    fn a_party_follower_keeps_the_match_after_the_queue_lobby_is_left() {
        let mut m = machine(true, JoinAuto::Invite);
        let mut s = world(JOINER);
        add_sc(&mut s, &[HOST, JOINER]);
        s.lobbies.get_mut(&Q).expect("q").data.insert("sc_lobby".into(), SC.to_string());
        let mut t = Instant::now();
        ready(&mut m, JOINER, t);
        if let State::MatchReady(r) = &mut m.state {
            r.ticket.party = Party { id: HOST, size: 2, leader: false, sc: sc() };
        }
        m.set_env(sc(), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 3);
        assert!(line(&m).starts_with("ALL PRESENT"));
        // The leader value is empty: the follower still keeps the match (the queue lobby is gone, nothing to follow).
        let scl = ScLobby::for_test(SC);
        m.sync_party(&mut s, &scl, t);
        assert!(matches!(m.state, State::MatchReady(_)));
        // After the match ends the stale value is not followed again.
        s.set_member(SC, HOST, "nrpvp_q", &format!("{Q}:solo:1"));
        m.leave(&mut s, "test", t);
        m.sync_party(&mut s, &scl, t + Duration::from_secs(10));
        assert!(m.is_idle(), "stale nrpvp_q for the finished match is ignored");
    }

    #[test]
    fn with_mm_join_off_a_ready_match_does_exactly_what_mm2_did() {
        let mut m = machine(false, JoinAuto::InviteLink);
        let mut s = world(HOST);
        add_sc(&mut s, &[HOST]);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(sc(), Ok(()), Some(Phase::Expedition), 2);
        let before = s.writes.len();
        tick(&mut m, &mut s, &mut t, 30);
        assert_eq!(s.writes.len(), before, "no write of any kind");
        assert_eq!(line(&m), "MATCH READY host=h (2 players, team size 1)");
        assert!(!m.status().join_ui);
        assert_eq!(m.team_override(), Some(1));
        assert!(!m.wants_sc());
    }

    #[test]
    fn nothing_but_invite_ever_touches_sc_lobby_in_the_automatic_flow() {
        // Full automatic flow, host side, every auto mode: the SC lobby only ever sees Invite.
        for auto in [JoinAuto::Invite, JoinAuto::InviteLink, JoinAuto::Link, JoinAuto::None] {
            let mut m = machine(true, auto);
            let mut s = world(HOST);
            add_sc(&mut s, &[HOST]);
            let mut t = Instant::now();
            ready(&mut m, HOST, t);
            m.set_env(sc(), Ok(()), None, 2);
            tick(&mut m, &mut s, &mut t, 4);
            s.lobbies.get_mut(&SC).expect("sc").members.push(JOINER);
            tick(&mut m, &mut s, &mut t, 4);
            for w in sc_writes(&s) {
                assert!(matches!(w, Write::Invite(..)), "{auto:?}: forbidden automatic write on SC lobby: {w:?}");
            }
        }
    }
}

/// MM-3e: the host's mod creates Seamless Co-op's lobby at MATCH READY.
#[cfg(test)]
mod host_create_tests {
    use super::super::{MachineCfg, Party, Ticket};
    use super::*;
    use crate::mm::join::JoinAuto;
    use crate::mm::proto::RosterEntry;
    use crate::mm::Mode;
    use crate::steam::queue::fake::{FakeSteam, Write};
    use crate::steam::queue::LobbyText;

    const HOST: u64 = 7001;
    const JOINER: u64 = 7002;
    const Q: u64 = 77;
    const SC: u64 = 5555;

    fn machine(host_create: bool) -> Machine {
        Machine::new(MachineCfg {
            need: 2,
            game: "1.3.3.0".into(),
            sc: "1.1.3".into(),
            join: true,
            join_auto: JoinAuto::InviteLink,
            host_create,
            quiet_invite: false,
        })
    }

    fn ready(m: &mut Machine, own: u64, t: Instant) {
        m.state = State::MatchReady(Box::new(Ready {
            ticket: Ticket { mode: Mode::Solo, party: Party { id: own, size: 1, leader: true, sc: None } },
            queue_id: Q,
            lobby: Some(QueueLobby::for_test(Q)),
            roster: vec![RosterEntry { id: HOST, party: HOST }, RosterEntry { id: JOINER, party: JOINER }],
            host_name: "h".into(),
            host: HOST,
            gate: Gate::new(t),
        }));
    }

    fn world(own: u64) -> FakeSteam {
        let mut s = FakeSteam::new(own);
        s.add_lobby(Q, HOST, 2, &[HOST, JOINER], &[("nrpvp", "1"), ("host", "7001"), ("state", "go")]);
        s
    }

    fn tick(m: &mut Machine, s: &mut FakeSteam, t: &mut Instant, n: usize) {
        for _ in 0..n {
            *t += Duration::from_millis(600);
            m.tick(s, *t);
        }
    }

    fn creates(s: &FakeSteam) -> usize {
        s.writes.iter().filter(|w| matches!(w, Write::CreateSc(..))).count()
    }

    const CREATING: &str = "creating the Seamless Co-op lobby...";
    const PROMPT: &str = "host has no Seamless Co-op lobby yet: open Matchmaking Settings > Invite Members and invite P7002";

    #[test]
    fn automatic_path_creates_once_publishes_invites_and_reaches_all_present() {
        let mut m = machine(true);
        let mut s = world(HOST);
        s.adopt_created = true;
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 1);
        assert_eq!(s.writes.iter().find(|w| matches!(w, Write::CreateSc(..))), Some(&Write::CreateSc(ScType::Public, 6)));
        assert_eq!(m.status().line, CREATING);
        tick(&mut m, &mut s, &mut t, 2);
        assert_eq!(m.status().line, CREATING, "still creating until the worker reports the lobby as env.sc");
        let lobby = s.next_lobby;
        // The worker now reports the adopted lobby as env.sc.
        m.set_env(Some(ScLobby::for_test(lobby)), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 3);
        assert_eq!(s.lobby_data(Q, "sc_lobby"), lobby.to_string());
        assert!(m.status().line.starts_with("MATCH READY - waiting for P7002 to enter your Seamless Co-op lobby"), "{}", m.status().line);
        assert_eq!(s.writes_to(lobby), vec![&Write::Invite(lobby, JOINER)], "only the invite is ever done to the adopted lobby");
        s.lobbies.get_mut(&lobby).expect("sc").members.push(JOINER);
        tick(&mut m, &mut s, &mut t, 3);
        assert_eq!(m.status().line, "ALL PRESENT 2/2 - host: start the expedition");
        assert_eq!(creates(&s), 1);
        assert_eq!(s.writes_to(lobby), vec![&Write::Invite(lobby, JOINER)]);
    }

    #[test]
    fn a_lobby_that_is_not_adopted_in_10_s_is_left_and_the_prompt_comes_back_and_no_second_one_is_made() {
        let mut m = machine(true);
        let mut s = world(HOST);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 10); // ~6 s
        let lobby = s.next_lobby;
        assert_eq!(m.status().line, CREATING);
        assert!(s.writes_to(lobby).is_empty(), "no call on it while it is watched: {:?}", s.writes);
        tick(&mut m, &mut s, &mut t, 10); // ~12 s
        assert_eq!(s.writes_to(lobby), vec![&Write::Leave(lobby)], "LeaveLobby on our own lobby only");
        assert_eq!(m.status().line, PROMPT);
        tick(&mut m, &mut s, &mut t, 100);
        assert_eq!(creates(&s), 1, "never a second lobby in the same match");
        assert_eq!(s.writes.iter().filter(|w| matches!(w, Write::Leave(l) if *l == lobby)).count(), 1);
        assert_eq!(m.status().line, PROMPT);
        // The game's own lobby appearing later is used normally.
        s.add_lobby(SC, HOST, 6, &[HOST], &[(KEY_SC_TYPE, "x")]);
        m.set_env(Some(ScLobby::for_test(SC)), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 3);
        assert_eq!(s.lobby_data(Q, "sc_lobby"), SC.to_string());
        assert_eq!(creates(&s), 1);
    }

    #[test]
    fn no_arrival_in_45_s_changes_the_host_line_and_keeps_the_lobby() {
        let mut m = machine(true);
        let mut s = world(HOST);
        s.adopt_created = true;
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        let lobby = s.next_lobby;
        m.set_env(Some(ScLobby::for_test(lobby)), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 60); // ~36 s after publishing
        assert!(m.status().line.starts_with("MATCH READY - waiting for P7002"), "{}", m.status().line);
        tick(&mut m, &mut s, &mut t, 30); // ~54 s
        assert_eq!(m.status().line, "P7002 did not arrive: open Invite Members and invite them");
        assert!(!s.writes.iter().any(|w| matches!(w, Write::Leave(l) if *l == lobby)), "our lobby is kept");
        assert_eq!(creates(&s), 1);
        // Once they do arrive, everything is normal again.
        s.lobbies.get_mut(&lobby).expect("sc").members.push(JOINER);
        tick(&mut m, &mut s, &mut t, 3);
        assert_eq!(m.status().line, "ALL PRESENT 2/2 - host: start the expedition");
    }

    #[test]
    fn nothing_is_created_when_a_lobby_exists_when_off_for_joiners_or_while_the_guard_fails() {
        // The game's lobby exists already: used, nothing created, and still nothing later when it vanishes.
        let mut m = machine(true);
        let mut s = world(HOST);
        s.add_lobby(SC, HOST, 6, &[HOST], &[(KEY_SC_TYPE, "x")]);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(Some(ScLobby::for_test(SC)), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 4);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 10);
        assert_eq!(creates(&s), 0);
        // Switch off.
        let mut m = machine(false);
        let mut s = world(HOST);
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 10);
        assert_eq!(creates(&s), 0);
        assert_eq!(m.status().line, PROMPT);
        // A joiner never creates.
        let mut m = machine(true);
        let mut s = world(JOINER);
        ready(&mut m, JOINER, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 10);
        assert_eq!(creates(&s), 0);
        // The safety guard fails: no create; it starts once the guard is fine.
        let mut m = machine(true);
        let mut s = world(HOST);
        ready(&mut m, HOST, t);
        m.set_env(None, Err("EasyAntiCheat".into()), None, 2);
        tick(&mut m, &mut s, &mut t, 5);
        assert_eq!(creates(&s), 0);
        assert_eq!(m.status().line, PROMPT);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        assert_eq!(creates(&s), 1);
    }

    #[test]
    fn a_failed_create_falls_back_to_the_prompt_without_retrying() {
        let mut m = machine(true);
        let mut s = world(HOST);
        s.create_fails = true;
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 30);
        assert_eq!(creates(&s), 1);
        assert!(!s.writes.iter().any(|w| matches!(w, Write::Leave(_))));
        assert_eq!(m.status().line, PROMPT);
    }

    #[test]
    fn a_second_sc_lobby_does_not_take_over_env_sc() {
        const SC2: u64 = 6666;
        // Host: our created and adopted lobby stays the match's lobby while the host opens Invite Members anyway.
        let mut m = machine(true);
        let mut s = world(HOST);
        s.adopt_created = true;
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        let mine = s.next_lobby;
        m.set_env(Some(ScLobby::for_test(mine)), Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 2);
        s.add_lobby(SC2, HOST, 6, &[HOST], &[(KEY_SC_TYPE, "y")]);
        assert_eq!(m.preferred_sc_alive(&s), Some(mine), "the worker ignores {SC2} and keeps env.sc on ours");
        tick(&mut m, &mut s, &mut t, 3);
        assert_eq!(s.lobby_data(Q, "sc_lobby"), mine.to_string(), "the published lobby does not change");
        assert!(s.writes_to(SC2).is_empty());
        // Joiner: bound to the lobby the host published.
        let mut mj = machine(true);
        let mut sj = world(JOINER);
        sj.add_lobby(SC, HOST, 6, &[HOST, JOINER], &[(KEY_SC_TYPE, "x")]);
        sj.add_lobby(SC2, JOINER, 6, &[JOINER], &[(KEY_SC_TYPE, "y")]);
        sj.lobbies.get_mut(&Q).expect("q").data.insert("sc_lobby".into(), SC.to_string());
        ready(&mut mj, JOINER, t);
        mj.set_env(Some(ScLobby::for_test(SC)), Ok(()), None, 2);
        tick(&mut mj, &mut sj, &mut t, 2);
        assert_eq!(mj.preferred_sc_alive(&sj), Some(SC));
        // Once the match's lobby is gone (we are not a member) the preference lapses.
        sj.lobbies.get_mut(&SC).expect("sc").members.retain(|m| *m != JOINER);
        assert_eq!(mj.preferred_sc_alive(&sj), None);
        // Idle machine: no preference.
        assert_eq!(machine(true).preferred_sc_alive(&sj), None);
    }

    #[test]
    fn ending_the_match_does_not_leave_a_stray_or_touch_an_adopted_lobby() {
        // While the create result is pending: the lobby that arrives later is left.
        let mut m = machine(true);
        let mut s = world(HOST);
        let mut t = Instant::now();
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        m.tick(&mut s, t + Duration::from_millis(600)); // CreateLobby sent
        let lobby = s.next_lobby;
        m.leave(&mut s, "test", t + Duration::from_secs(1));
        m.tick(&mut s, t + Duration::from_secs(2));
        assert!(s.writes.contains(&Write::Leave(lobby)));
        // Watching, not adopted: left at once.
        let mut m = machine(true);
        let mut s = world(HOST);
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 3);
        let lobby = s.next_lobby;
        m.leave(&mut s, "test", t);
        assert!(s.writes.contains(&Write::Leave(lobby)));
        // Adopted: no call on it.
        let mut m = machine(true);
        let mut s = world(HOST);
        s.adopt_created = true;
        ready(&mut m, HOST, t);
        m.set_env(None, Ok(()), None, 2);
        tick(&mut m, &mut s, &mut t, 3);
        let lobby = s.next_lobby;
        m.leave(&mut s, "test", t);
        assert!(s.writes_to(lobby).is_empty(), "{:?}", s.writes);
    }
}
