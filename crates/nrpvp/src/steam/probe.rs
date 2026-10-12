//! MM-1: the Steam access probe (S9 U1, U2). Read/listen-only: resolves Steam's flat API in the DLL the game already
//! initialised, registers listen-only lobby callbacks, and logs every lobby event plus a lobby summary. Never calls a Steam
//! setter, never initialises/pumps/shuts Steam down. Every log line starts with `mm_probe:`.
//!
//! Struct ids and layouts, all from the Steamworks SDK headers bundled by steamworks-rs (fetched 2026-10-12):
//! base `https://github.com/Noxime/steamworks-rs/blob/master/steamworks-sys/lib/steam/public/steam/`
//! - `k_iSteamMatchmakingCallbacks = 500`, `k_iSteamFriendsCallbacks = 300`: `steam_api_internal.h` lines 247 and 249.
//! - Windows packing is `#pragma pack(push, 8)`: `steamclientpublic.h` defines `VALVE_CALLBACK_PACK_LARGE` on every platform
//!   except Linux/macOS/FreeBSD, and `isteammatchmaking.h` / `isteamfriends.h` pick `pack(8)` for it.
//! - `LobbyEnter_t` (+4), `LobbyDataUpdate_t` (+5), `LobbyChatUpdate_t` (+6), `LobbyCreated_t` (+13): `isteammatchmaking.h`.
//! - `GameLobbyJoinRequested_t` (+33), `GameRichPresenceJoinRequested_t` (+37, `k_cchMaxRichPresenceValueLength` = 256): `isteamfriends.h`.
//!   `CSteamID` is an 8-byte class (one uint64) with 8-byte alignment, so with pack(8) it sits at offset 0/8 as a plain u64.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use super::callback::{self, RawEvent};
use super::{steam_dll_loaded, Flat, LobbyApi};

const MATCHMAKING_BASE: i32 = 500;
const FRIENDS_BASE: i32 = 300;
pub(crate) const ID_LOBBY_ENTER: i32 = MATCHMAKING_BASE + 4;
/// MM-3: `LobbyInvite_t` = `k_iSteamMatchmakingCallbacks + 3` (isteammatchmaking.h): `uint64 m_ulSteamIDUser` (who invited) @0,
/// `uint64 m_ulSteamIDLobby` @8, `uint64 m_ulGameID` @16 = 24 bytes (all 8-byte fields, so pack 4 and pack 8 agree).
pub(crate) const ID_LOBBY_INVITE: i32 = MATCHMAKING_BASE + 3;
pub(crate) const ID_LOBBY_DATA_UPDATE: i32 = MATCHMAKING_BASE + 5;
pub(crate) const ID_LOBBY_CHAT_UPDATE: i32 = MATCHMAKING_BASE + 6;
pub(crate) const ID_LOBBY_MATCH_LIST: i32 = MATCHMAKING_BASE + 10;
pub(crate) const ID_LOBBY_CREATED: i32 = MATCHMAKING_BASE + 13;
pub(crate) const ID_GAME_LOBBY_JOIN_REQUESTED: i32 = FRIENDS_BASE + 33;
pub(crate) const ID_GAME_RICH_PRESENCE_JOIN_REQUESTED: i32 = FRIENDS_BASE + 37;

/// `(callback id, sizeof(struct) with pack(8), name)`. Sizes: LobbyEnter_t u64@0 u32@8 bool@12 u32@16 -> 24;
/// LobbyDataUpdate_t u64@0 u64@8 u8@16 -> 24; LobbyChatUpdate_t 3*u64 + u32@24 -> 32; LobbyCreated_t int@0 u64@8 -> 16;
/// GameLobbyJoinRequested_t 2*CSteamID -> 16; GameRichPresenceJoinRequested_t CSteamID + char[256] -> 264.
pub(crate) const CALLBACKS: [(i32, usize, &str); 8] = [
    (ID_LOBBY_INVITE, 24, "LobbyInvite_t"),
    (ID_LOBBY_ENTER, 24, "LobbyEnter_t"),
    (ID_LOBBY_DATA_UPDATE, 24, "LobbyDataUpdate_t"),
    (ID_LOBBY_CHAT_UPDATE, 32, "LobbyChatUpdate_t"),
    (ID_LOBBY_MATCH_LIST, 4, "LobbyMatchList_t"),
    (ID_LOBBY_CREATED, 16, "LobbyCreated_t"),
    (ID_GAME_LOBBY_JOIN_REQUESTED, 16, "GameLobbyJoinRequested_t"),
    (ID_GAME_RICH_PRESENCE_JOIN_REQUESTED, 264, "GameRichPresenceJoinRequested_t"),
];

/// How long we wait for the game to initialise Steam before giving up.
const WAIT_TOTAL: Duration = Duration::from_secs(300);
const WAIT_STEP: Duration = Duration::from_millis(500);
/// Drain period of the event queue.
const DRAIN_STEP: Duration = Duration::from_millis(100);
/// Minimum gap between two summaries of the same lobby.
const SUMMARY_GAP: Duration = Duration::from_secs(1);

/// A decoded callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    LobbyEnter { lobby: u64, permissions: u32, locked: bool, response: u32 },
    LobbyDataUpdate { lobby: u64, member: u64, success: bool },
    LobbyChatUpdate { lobby: u64, user_changed: u64, making_change: u64, state_change: u32 },
    LobbyMatchList { count: u32 },
    LobbyCreated { result: i32, lobby: u64 },
    LobbyInvite { user: u64, lobby: u64, game: u64 },
    GameLobbyJoinRequested { lobby: u64, friend: u64 },
    GameRichPresenceJoinRequested { friend: u64, connect: String },
}

fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(off..off + 8)?.try_into().ok()?))
}
fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

/// Decode the raw bytes of callback `id` (layouts: see the module doc). `None` for an unknown id or a short buffer.
pub(crate) fn decode(id: i32, b: &[u8]) -> Option<Event> {
    let need = CALLBACKS.iter().find(|c| c.0 == id)?.1;
    if b.len() < need {
        return None;
    }
    Some(match id {
        ID_LOBBY_INVITE => Event::LobbyInvite { user: u64_at(b, 0)?, lobby: u64_at(b, 8)?, game: u64_at(b, 16)? },
        ID_LOBBY_ENTER => Event::LobbyEnter {
            lobby: u64_at(b, 0)?,
            permissions: u32_at(b, 8)?,
            locked: *b.get(12)? != 0,
            response: u32_at(b, 16)?,
        },
        ID_LOBBY_DATA_UPDATE => Event::LobbyDataUpdate {
            lobby: u64_at(b, 0)?,
            member: u64_at(b, 8)?,
            success: *b.get(16)? != 0,
        },
        ID_LOBBY_CHAT_UPDATE => Event::LobbyChatUpdate {
            lobby: u64_at(b, 0)?,
            user_changed: u64_at(b, 8)?,
            making_change: u64_at(b, 16)?,
            state_change: u32_at(b, 24)?,
        },
        ID_LOBBY_MATCH_LIST => Event::LobbyMatchList { count: u32_at(b, 0)? },
        ID_LOBBY_CREATED => Event::LobbyCreated { result: u32_at(b, 0)? as i32, lobby: u64_at(b, 8)? },
        ID_GAME_LOBBY_JOIN_REQUESTED => {
            Event::GameLobbyJoinRequested { lobby: u64_at(b, 0)?, friend: u64_at(b, 8)? }
        }
        ID_GAME_RICH_PRESENCE_JOIN_REQUESTED => Event::GameRichPresenceJoinRequested {
            friend: u64_at(b, 0)?,
            connect: super::cstr_lossy(b.get(8..264)?),
        },
        _ => return None,
    })
}

impl Event {
    /// The lobby this event is about, if any (0 = none).
    fn lobby(&self) -> u64 {
        match self {
            Event::LobbyEnter { lobby, .. }
            | Event::LobbyDataUpdate { lobby, .. }
            | Event::LobbyChatUpdate { lobby, .. }
            | Event::LobbyCreated { lobby, .. }
            | Event::LobbyInvite { lobby, .. }
            | Event::GameLobbyJoinRequested { lobby, .. } => *lobby,
            Event::LobbyMatchList { .. } | Event::GameRichPresenceJoinRequested { .. } => 0,
        }
    }
    /// Events after which the lobby summary is worth repeating.
    fn refreshes_summary(&self) -> bool {
        matches!(self, Event::LobbyDataUpdate { .. } | Event::LobbyChatUpdate { .. } | Event::LobbyEnter { .. })
    }
}

/// `1234 (0x4D2)`
fn id_text(v: u64) -> String {
    format!("{v} (0x{v:X})")
}

/// One log line for an event.
pub(crate) fn describe(ev: &Event) -> String {
    match ev {
        Event::LobbyInvite { user, lobby, game } => format!(
            "mm_probe: LobbyInvite_t from user={} lobby={} game_id={game} (0x{game:X})",
            id_text(*user),
            id_text(*lobby)
        ),
        Event::LobbyEnter { lobby, permissions, locked, response } => format!(
            "mm_probe: LobbyEnter_t lobby={} permissions=0x{permissions:X} locked={locked} response={response} (1 = success)",
            id_text(*lobby)
        ),
        Event::LobbyDataUpdate { lobby, member, success } => format!(
            "mm_probe: LobbyDataUpdate_t lobby={} member={} ({}) success={success}",
            id_text(*lobby),
            id_text(*member),
            if member == lobby { "the lobby itself" } else { "a member" }
        ),
        Event::LobbyChatUpdate { lobby, user_changed, making_change, state_change } => format!(
            "mm_probe: LobbyChatUpdate_t lobby={} user_changed={} making_change={} state_change=0x{state_change:X}",
            id_text(*lobby),
            id_text(*user_changed),
            id_text(*making_change)
        ),
        Event::LobbyMatchList { count } => format!("mm_probe: LobbyMatchList_t lobbies_matching={count}"),
        Event::LobbyCreated { result, lobby } => format!(
            "mm_probe: LobbyCreated_t result={result} (1 = OK) lobby={}",
            id_text(*lobby)
        ),
        Event::GameLobbyJoinRequested { lobby, friend } => format!(
            "mm_probe: GameLobbyJoinRequested_t lobby={} friend={}",
            id_text(*lobby),
            id_text(*friend)
        ),
        Event::GameRichPresenceJoinRequested { friend, connect } => format!(
            "mm_probe: GameRichPresenceJoinRequested_t friend={} connect={connect:?}",
            id_text(*friend)
        ),
    }
}

/// Lobby summary lines: owner, size, limit, members, every data key/value. Uses getters only.
pub(crate) fn summarize(api: &dyn LobbyApi, lobby: u64) -> Vec<String> {
    let mut out = Vec::new();
    let members = api.member_count(lobby);
    out.push(format!(
        "mm_probe: lobby {} summary: owner={} members={members} limit={}",
        id_text(lobby),
        id_text(api.owner(lobby)),
        api.member_limit(lobby)
    ));
    // Cap the loops so a garbage count cannot spin the thread.
    for i in 0..members.clamp(0, 64) {
        out.push(format!("mm_probe: lobby {lobby} member[{i}]={}", id_text(api.member_by_index(lobby, i))));
    }
    let n = api.data_count(lobby);
    out.push(format!("mm_probe: lobby {lobby} data entries={n}"));
    for i in 0..n.clamp(0, 256) {
        match api.data_by_index(lobby, i) {
            Some((k, v)) => out.push(format!("mm_probe: lobby {lobby} data[{i}] {k:?} = {v:?}")),
            None => out.push(format!("mm_probe: lobby {lobby} data[{i}] unreadable")),
        }
    }
    out
}

/// Remembers which lobbies were seen and when each was last summarised.
#[derive(Default)]
pub(crate) struct Tracker {
    last_summary: HashMap<u64, Instant>,
}

impl Tracker {
    /// Every lobby id seen in an event so far.
    pub(crate) fn known(&self) -> Vec<u64> {
        self.last_summary.keys().copied().collect()
    }

    /// Lines to log for one event: its description, plus a summary for a new lobby or (rate-limited) a refreshed one.
    pub(crate) fn handle(&mut self, api: &dyn LobbyApi, ev: &Event, now: Instant) -> Vec<String> {
        let mut out = vec![describe(ev)];
        let lobby = ev.lobby();
        if lobby == 0 {
            return out;
        }
        let due = match self.last_summary.get(&lobby) {
            None => true,
            Some(t) => ev.refreshes_summary() && now.saturating_duration_since(*t) >= SUMMARY_GAP,
        };
        if due {
            if !self.last_summary.contains_key(&lobby) {
                out.push(format!("mm_probe: new lobby seen: {}", id_text(lobby)));
            }
            self.last_summary.insert(lobby, now);
            out.extend(summarize(api, lobby));
        }
        out
    }
}

/// MM-3 evidence: how often membership is checked, and the longest gap between two lines for the same lobby.
const WATCH_STEP: Duration = Duration::from_secs(1);
const WATCH_PERIOD: Duration = Duration::from_secs(10);

/// What kind of lobby this is, from its data keys: Seamless Co-op's lobby A (`nrsc_lobby_type`), our queue lobby (`nrpvp`), the game's
/// session lobby B (only `inviteParam`, MM-1 finding in S9 §5a), or something else.
pub(crate) fn classify(api: &dyn LobbyApi, lobby: u64) -> &'static str {
    let n = api.data_count(lobby).clamp(0, 256);
    let keys: Vec<String> = (0..n).filter_map(|i| api.data_by_index(lobby, i)).map(|(k, _)| k).collect();
    if keys.iter().any(|k| k == "nrsc_lobby_type") {
        "SC lobby (A)"
    } else if keys.iter().any(|k| k == "nrpvp") {
        "queue lobby"
    } else if !keys.is_empty() && keys.iter().all(|k| k == "inviteParam") {
        "game lobby (B, inviteParam only)"
    } else if keys.is_empty() {
        "no readable data"
    } else {
        "other lobby"
    }
}

/// Logs one line per known lobby whenever its owner/limit/members change and at least every `period` while we are a member.
#[derive(Default)]
pub(crate) struct LobbyWatch {
    last: HashMap<u64, (String, Instant)>,
}

impl LobbyWatch {
    pub(crate) fn tick(&mut self, api: &dyn LobbyApi, known: &[u64], now: Instant, period: Duration) -> Vec<String> {
        let mut out = Vec::new();
        for &lobby in known {
            let n = api.member_count(lobby);
            let (sig, member) = if n > 0 {
                let ids: Vec<u64> = (0..n.clamp(0, 64)).map(|i| api.member_by_index(lobby, i)).collect();
                (format!("owner={} limit={} members={ids:?}", api.owner(lobby), api.member_limit(lobby)), true)
            } else {
                ("members=none (we are not in it, or it is gone)".to_string(), false)
            };
            let due = match self.last.get(&lobby) {
                None => true,
                Some((s, t)) => *s != sig || (member && now.saturating_duration_since(*t) >= period),
            };
            if due {
                out.push(format!("mm_join: lobby {} [{}] {sig}", id_text(lobby), classify(api, lobby)));
                self.last.insert(lobby, (sig, now));
            }
        }
        out
    }
}

/// Start the Steam thread (one thread, one listener registration). Call only in developer mode, when `mm_probe` or `mm_queue` is
/// on; nothing here runs otherwise. `queue` is `Some` when `mm_queue` is on: the MM-2 worker then runs inside this loop.
pub(crate) fn spawn(queue: Option<crate::mm::worker::Setup>, evidence: bool) {
    let r = std::thread::Builder::new().name("nrpvp-mm-probe".into()).spawn(move || {
        if catch_unwind(AssertUnwindSafe(|| run(queue, evidence))).is_err() {
            tracing::error!("mm_probe: probe thread panicked");
        }
    });
    if r.is_err() {
        tracing::warn!("mm_probe: could not spawn the probe thread");
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "NO" }
}

fn ptr_text(p: Option<usize>) -> String {
    match p {
        None => "export missing".to_string(),
        Some(0) => "returned NULL".to_string(),
        Some(v) => format!("non-null (0x{v:X})"),
    }
}

fn run(queue: Option<crate::mm::worker::Setup>, evidence: bool) {
    tracing::info!(
        "mm_probe: enabled (listen-only; waits up to {} s for the game to initialise Steam)",
        WAIT_TOTAL.as_secs()
    );
    let start = Instant::now();
    let mut flat: Option<Flat> = None;
    loop {
        if flat.is_none() && steam_dll_loaded() {
            let (f, resolved) = Flat::resolve();
            tracing::info!("mm_probe: steam_api64.dll loaded after {:.1} s", start.elapsed().as_secs_f32());
            for r in &resolved {
                tracing::info!("mm_probe: export {} resolved: {}", r.name, yes_no(r.found));
            }
            flat = Some(f);
        }
        if let Some(f) = &flat {
            if f.hsteam_user() != 0 {
                tracing::info!(
                    "mm_probe: SteamAPI_GetHSteamUser() = {} after {:.1} s: Steam is initialised",
                    f.hsteam_user(),
                    start.elapsed().as_secs_f32()
                );
                break;
            }
        }
        if start.elapsed() >= WAIT_TOTAL {
            tracing::warn!(
                "mm_probe: gave up after {} s: steam_api64.dll loaded = {}, HSteamUser non-zero = false",
                WAIT_TOTAL.as_secs(),
                flat.is_some()
            );
            return;
        }
        std::thread::sleep(WAIT_STEP);
    }
    let Some(mut flat) = flat else { return };

    // (a) accessors and own Steam ID.
    let mm = Flat::call_accessor(flat.matchmaking);
    let user = Flat::call_accessor(flat.user);
    tracing::info!("mm_probe: SteamAPI_SteamMatchmaking_v009(): {}", ptr_text(mm));
    tracing::info!("mm_probe: SteamAPI_SteamUtils_v010(): {}", ptr_text(Flat::call_accessor(flat.utils)));
    tracing::info!("mm_probe: SteamAPI_SteamUser_v021(): {}", ptr_text(user));
    tracing::info!("mm_probe: SteamAPI_SteamFriends_v017(): {}", ptr_text(Flat::call_accessor(flat.friends)));
    match flat.own_steam_id(user.unwrap_or(0)) {
        Some(id) => tracing::info!("mm_probe: own Steam ID = {}", id_text(id)),
        None => tracing::warn!("mm_probe: own Steam ID unavailable (user accessor null or GetSteamID export missing)"),
    }
    let mm_ok = flat.bind_matchmaking(mm.unwrap_or(0));

    // (b) listen-only callbacks.
    match flat.register_callback {
        Some(reg) => {
            let wanted: Vec<(i32, usize)> = CALLBACKS.iter().map(|c| (c.0, c.1)).collect();
            // SAFETY: `reg` is the SteamAPI_RegisterCallback export of the loaded steam_api64.dll.
            let results = unsafe { callback::register_all(reg, &wanted) };
            for (id, ok) in results {
                let name = CALLBACKS.iter().find(|c| c.0 == id).map_or("?", |c| c.2);
                tracing::info!("mm_probe: RegisterCallback {name} (id {id}): registered flag set = {}", yes_no(ok));
            }
        }
        None => tracing::warn!("mm_probe: SteamAPI_RegisterCallback missing; no events will be seen"),
    }
    if !mm_ok {
        tracing::warn!("mm_probe: ISteamMatchmaking is null; events are logged but lobby summaries are skipped");
    }

    // MM-2: the queue worker shares this thread, this registration and this event stream.
    let mut worker = queue.and_then(crate::mm::worker::Worker::start);

    // (c/d) drain our queue forever.
    let mut tracker = Tracker::default();
    let mut watch = LobbyWatch::default();
    let mut next_watch = Instant::now();
    let mut dropped_seen = 0u32;
    loop {
        for raw in callback::drain() {
            let RawEvent { id, len, data } = raw;
            match decode(id, &data[..len]) {
                Some(ev) => {
                    // Without matchmaking, log the event only.
                    let lines = if mm_ok { tracker.handle(&flat, &ev, Instant::now()) } else { vec![describe(&ev)] };
                    for l in lines {
                        tracing::info!("{l}");
                    }
                    if let Some(w) = worker.as_mut() {
                        w.on_event(&ev);
                    }
                }
                None => tracing::warn!("mm_probe: could not decode callback id {id} ({len} bytes)"),
            }
        }
        let d = callback::dropped();
        if d != dropped_seen {
            tracing::warn!("mm_probe: {} callback(s) dropped because the queue was full", d - dropped_seen);
            dropped_seen = d;
        }
        if let Some(w) = worker.as_mut() {
            w.tick(Instant::now());
        }
        // MM-3 evidence: which lobby holds whom, on change and at least every 10 s (SC's lobby A, the game's lobby B, our queue lobby).
        if evidence && mm_ok {
            let now = Instant::now();
            if now >= next_watch {
                next_watch = now + WATCH_STEP;
                for l in watch.tick(&flat, &tracker.known(), now, WATCH_PERIOD) {
                    tracing::info!("{l}");
                }
            }
        }
        std::thread::sleep(DRAIN_STEP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u64(b: &mut [u8], off: usize, v: u64) {
        b[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }
    fn put_u32(b: &mut [u8], off: usize, v: u32) {
        b[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    #[test]
    fn callback_ids_and_sizes_match_the_sdk() {
        let ids: Vec<i32> = CALLBACKS.iter().map(|c| c.0).collect();
        assert_eq!(ids, vec![503, 504, 505, 506, 510, 513, 333, 337]);
        let sizes: Vec<usize> = CALLBACKS.iter().map(|c| c.1).collect();
        assert_eq!(sizes, vec![24, 24, 24, 32, 4, 16, 16, 264]);
    }

    #[test]
    fn decodes_lobby_enter_with_pack8_padding() {
        let mut b = [0u8; 24];
        put_u64(&mut b, 0, 109775241234567890);
        put_u32(&mut b, 8, 0);
        b[12] = 1;
        put_u32(&mut b, 16, 1);
        assert_eq!(
            decode(ID_LOBBY_ENTER, &b),
            Some(Event::LobbyEnter { lobby: 109775241234567890, permissions: 0, locked: true, response: 1 })
        );
    }

    #[test]
    fn decodes_the_other_lobby_structs() {
        let mut b = [0u8; 24];
        put_u64(&mut b, 0, 7);
        put_u64(&mut b, 8, 9);
        b[16] = 1;
        assert_eq!(
            decode(ID_LOBBY_DATA_UPDATE, &b),
            Some(Event::LobbyDataUpdate { lobby: 7, member: 9, success: true })
        );
        let mut c = [0u8; 32];
        put_u64(&mut c, 0, 1);
        put_u64(&mut c, 8, 2);
        put_u64(&mut c, 16, 3);
        put_u32(&mut c, 24, 0x2);
        assert_eq!(
            decode(ID_LOBBY_CHAT_UPDATE, &c),
            Some(Event::LobbyChatUpdate { lobby: 1, user_changed: 2, making_change: 3, state_change: 2 })
        );
        let mut d = [0u8; 16];
        put_u32(&mut d, 0, 1);
        put_u64(&mut d, 8, 55);
        assert_eq!(decode(ID_LOBBY_CREATED, &d), Some(Event::LobbyCreated { result: 1, lobby: 55 }));
        let mut e = [0u8; 16];
        put_u64(&mut e, 0, 11);
        put_u64(&mut e, 8, 12);
        assert_eq!(
            decode(ID_GAME_LOBBY_JOIN_REQUESTED, &e),
            Some(Event::GameLobbyJoinRequested { lobby: 11, friend: 12 })
        );
    }

    #[test]
    fn decodes_rich_presence_connect_string() {
        let mut b = [0u8; 264];
        put_u64(&mut b, 0, 42);
        b[8..8 + 5].copy_from_slice(b"hello");
        assert_eq!(
            decode(ID_GAME_RICH_PRESENCE_JOIN_REQUESTED, &b),
            Some(Event::GameRichPresenceJoinRequested { friend: 42, connect: "hello".into() })
        );
    }

    #[test]
    fn short_buffers_and_unknown_ids_are_rejected() {
        assert_eq!(decode(ID_LOBBY_ENTER, &[0u8; 23]), None);
        assert_eq!(decode(ID_LOBBY_CREATED, &[0u8; 15]), None);
        assert_eq!(decode(9999, &[0u8; 300]), None);
    }

    struct Fake;
    impl LobbyApi for Fake {
        fn owner(&self, _: u64) -> u64 {
            100
        }
        fn member_count(&self, _: u64) -> i32 {
            2
        }
        fn member_limit(&self, _: u64) -> i32 {
            4
        }
        fn member_by_index(&self, _: u64, i: i32) -> u64 {
            200 + i as u64
        }
        fn data_count(&self, _: u64) -> i32 {
            2
        }
        fn data_by_index(&self, _: u64, i: i32) -> Option<(String, String)> {
            (i == 0).then(|| ("k".to_string(), "v".to_string()))
        }
    }

    #[test]
    fn summary_lists_owner_members_and_data() {
        let s = summarize(&Fake, 77);
        assert!(s[0].contains("owner=100 (0x64)") && s[0].contains("members=2") && s[0].contains("limit=4"), "{s:?}");
        assert!(s.iter().any(|l| l.contains("member[1]=201")));
        assert!(s.iter().any(|l| l.contains("data[0] \"k\" = \"v\"")));
        assert!(s.iter().any(|l| l.contains("data[1] unreadable")));
        assert!(s.iter().all(|l| l.starts_with("mm_probe:")));
    }

    #[test]
    fn tracker_summarises_new_lobbies_and_rate_limits_updates() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let upd = Event::LobbyDataUpdate { lobby: 5, member: 5, success: true };
        // First sight: event line + "new lobby" + summary.
        let l = t.handle(&Fake, &upd, t0);
        assert!(l.iter().any(|x| x.contains("new lobby seen")) && l.len() > 2, "{l:?}");
        // Within a second: only the event line.
        assert_eq!(t.handle(&Fake, &upd, t0 + Duration::from_millis(500)).len(), 1);
        // After a second: summary again, but not announced as new.
        let l = t.handle(&Fake, &upd, t0 + Duration::from_millis(1100));
        assert!(l.len() > 1 && !l.iter().any(|x| x.contains("new lobby seen")), "{l:?}");
        // A join request for a known lobby never re-summarises; an event without a lobby never summarises.
        let join = Event::GameLobbyJoinRequested { lobby: 5, friend: 1 };
        assert_eq!(t.handle(&Fake, &join, t0 + Duration::from_secs(10)).len(), 1);
        let rp = Event::GameRichPresenceJoinRequested { friend: 1, connect: String::new() };
        assert_eq!(t.handle(&Fake, &rp, t0).len(), 1);
    }


    #[test]
    fn decodes_lobby_invite() {
        let mut b = [0u8; 24];
        put_u64(&mut b, 0, 11);
        put_u64(&mut b, 8, 22);
        put_u64(&mut b, 16, 2622380);
        assert_eq!(
            decode(ID_LOBBY_INVITE, &b),
            Some(Event::LobbyInvite { user: 11, lobby: 22, game: 2622380 })
        );
        assert_eq!(decode(ID_LOBBY_INVITE, &b[..23]), None);
        assert!(describe(&decode(ID_LOBBY_INVITE, &b).expect("ev")).contains("LobbyInvite_t from user=11"));
    }

    /// Lobby with chosen members and data keys, for the watch tests.
    struct W {
        members: std::cell::RefCell<Vec<u64>>,
        keys: Vec<&'static str>,
    }
    impl LobbyApi for W {
        fn owner(&self, _: u64) -> u64 {
            1
        }
        fn member_count(&self, _: u64) -> i32 {
            self.members.borrow().len() as i32
        }
        fn member_limit(&self, _: u64) -> i32 {
            6
        }
        fn member_by_index(&self, _: u64, i: i32) -> u64 {
            self.members.borrow()[i as usize]
        }
        fn data_count(&self, _: u64) -> i32 {
            self.keys.len() as i32
        }
        fn data_by_index(&self, _: u64, i: i32) -> Option<(String, String)> {
            self.keys.get(i as usize).map(|k| ((*k).to_string(), "v".to_string()))
        }
    }

    #[test]
    fn lobbies_are_classified_by_their_data_keys() {
        let k = |keys: Vec<&'static str>| classify(&W { members: Default::default(), keys }, 1);
        assert_eq!(k(vec!["nrsc_lobby_type", "inviteParam"]), "SC lobby (A)");
        assert_eq!(k(vec!["nrpvp", "mode"]), "queue lobby");
        assert_eq!(k(vec!["inviteParam"]), "game lobby (B, inviteParam only)");
        assert_eq!(k(vec![]), "no readable data");
        assert_eq!(k(vec!["x"]), "other lobby");
    }

    #[test]
    fn lobby_watch_logs_on_change_and_every_period_while_a_member() {
        let w = W { members: std::cell::RefCell::new(vec![1, 2]), keys: vec!["nrsc_lobby_type"] };
        let mut lw = LobbyWatch::default();
        let t0 = Instant::now();
        let p = Duration::from_secs(10);
        let l = lw.tick(&w, &[9], t0, p);
        assert_eq!(l.len(), 1);
        assert!(l[0].starts_with("mm_join: lobby 9 (0x9) [SC lobby (A)] owner=1 limit=6 members=[1, 2]"), "{l:?}");
        assert!(lw.tick(&w, &[9], t0 + Duration::from_secs(3), p).is_empty(), "unchanged and early");
        w.members.borrow_mut().push(3);
        assert_eq!(lw.tick(&w, &[9], t0 + Duration::from_secs(4), p).len(), 1, "membership changed");
        assert_eq!(lw.tick(&w, &[9], t0 + Duration::from_secs(15), p).len(), 1, "periodic");
        // A lobby we are not in: one line when it changes to that, then silence.
        w.members.borrow_mut().clear();
        assert_eq!(lw.tick(&w, &[9], t0 + Duration::from_secs(16), p).len(), 1);
        assert!(lw.tick(&w, &[9], t0 + Duration::from_secs(60), p).is_empty());
    }
    #[test]
    fn queue_round_trip_through_the_callback_object() {
        // Exercise register_all with a fake RegisterCallback that sets the flag and then delivers via the vtable.
        use std::ffi::{c_int, c_void};
        unsafe extern "system" fn fake_register(obj: *mut c_void, _id: c_int) {
            // Mimic Steam: set the registered flag (offset 8), then call vtable slot 0 with a payload.
            let p = obj as *mut u8;
            unsafe {
                *p.add(8) |= 1;
                let vt = *(obj as *const *const usize);
                let run: unsafe extern "system" fn(*mut c_void, *const c_void) = std::mem::transmute(*vt);
                let payload = [0xABu8; 16];
                run(obj, payload.as_ptr() as *const c_void);
            }
        }
        let res = unsafe { callback::register_all(fake_register, &[(ID_LOBBY_CREATED, 16)]) };
        assert_eq!(res, vec![(ID_LOBBY_CREATED, true)]);
        let evs = callback::drain();
        assert!(evs.iter().any(|e| e.id == ID_LOBBY_CREATED && e.len == 16 && e.data[0] == 0xAB));
    }
}
