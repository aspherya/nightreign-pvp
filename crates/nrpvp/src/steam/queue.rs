//! MM-2: the write side of the Steam lobby API, limited to OUR queue lobby (S9 §6, TASKS.md MM-2).
//!
//! Rules enforced here, in types, not by convention:
//! - Every call that writes lobby data, changes joinable, or leaves a lobby takes a [`QueueLobby`]. That type can only be built
//!   for a lobby our mod created itself ([`QueueLobby::created`], private to this module and reached only from the result of our
//!   own `CreateLobby` call) or for a lobby that, once readable, carries the tag `nrpvp=1` and does NOT carry `nrsc_lobby_type`
//!   ([`QueueLobby::verify`]). Seamless Co-op's lobby is therefore unrepresentable as a `QueueLobby`.
//! - The only thing we ever write on Seamless Co-op's lobby is OUR OWN member data under the key `nrpvp_q`
//!   ([`QueueSteam::set_sc_queue_value`], which takes a [`ScLobby`] and has no key parameter).
//! - `LeaveLobby` is additionally refused at call time if the lobby carries `nrsc_lobby_type`.
//! - We never call `SteamAPI_Init*`, `SteamAPI_ManualDispatch_*`, `SteamAPI_RunCallbacks` or `SteamAPI_Shutdown`.
//!
//! Signatures, enum values and struct layouts, all from the SDK headers bundled by steamworks-rs (fetched 2026-10-12), base
//! `https://github.com/Noxime/steamworks-rs/blob/master/steamworks-sys/lib/steam/public/steam/`:
//! - `steam_api_flat.h`: `SteamAPI_ISteamMatchmaking_{RequestLobbyList, AddRequestLobbyListStringFilter,
//!   AddRequestLobbyListFilterSlotsAvailable, AddRequestLobbyListDistanceFilter, AddRequestLobbyListResultCountFilter,
//!   GetLobbyByIndex, CreateLobby, JoinLobby, LeaveLobby, GetLobbyData, SetLobbyData, GetLobbyMemberData, SetLobbyMemberData,
//!   SetLobbyJoinable}`, `SteamAPI_ISteamUtils_{IsAPICallCompleted, GetAPICallFailureReason, GetAPICallResult}`,
//!   `SteamAPI_ISteamFriends_GetFriendPersonaName`. `CSteamID` crosses as `uint64`, `SteamAPICall_t` is `uint64`
//!   (`steamtypes.h`), `ELobbyType`/`ELobbyComparison`/`ELobbyDistanceFilter`/`ESteamAPICallFailure` are 32-bit enums.
//! - `isteammatchmaking.h`: `ELobbyType` Invisible = 3; `ELobbyComparison` Equal = 0; `ELobbyDistanceFilter` Worldwide = 3;
//!   `LobbyMatchList_t` = id 510, `uint32` (4 bytes); `LobbyCreated_t` = id 513 (`EResult` @0, `uint64` @8, 16 bytes);
//!   `LobbyEnter_t` = id 504 (`uint64` @0, `uint32` @8, `bool` @12, `uint32` @16, 24 bytes), pack 8 on Windows.
//! - `steamclientpublic.h`: `EResult` OK = 1; `EChatRoomEnterResponse` Success = 1.
//! - `isteamutils.h`: `ESteamAPICallFailure`: None = -1, SteamGone = 0, NetworkFailure = 1, InvalidHandle = 2, MismatchedCallback = 3.

use std::ffi::{c_char, c_int, c_void, CStr, CString};

use super::{export, Flat, LobbyApi};

/// Lobby data / member data keys. `nrsc_*` are Seamless Co-op's (read only); `nrpvp*` are ours.
pub(crate) const KEY_SC_TYPE: &str = "nrsc_lobby_type";
pub(crate) const KEY_SC_VERSION: &str = "nrsc_lobby_version";
/// Our member key on SC's lobby: the queue lobby the party leader sits in (the only write we make on SC's lobby).
pub(crate) const KEY_SC_MEMBER: &str = "nrpvp_q";
pub(crate) const KEY_TAG: &str = "nrpvp";
/// MM-3: queue lobby data (written by the host/owner) naming the host's Seamless Co-op lobby.
pub(crate) const KEY_PUBLISHED_SC: &str = "sc_lobby";

/// `ELobbyType::k_ELobbyTypeInvisible` (isteammatchmaking.h).
const LOBBY_TYPE_INVISIBLE: c_int = 3;
/// `ELobbyComparison::k_ELobbyComparisonEqual`.
const COMPARISON_EQUAL: c_int = 0;
/// `ELobbyDistanceFilter::k_ELobbyDistanceFilterWorldwide`.
const DISTANCE_WORLDWIDE: c_int = 3;
pub(crate) const CB_LOBBY_MATCH_LIST: i32 = 510;
pub(crate) const CB_LOBBY_CREATED: i32 = 513;
pub(crate) const CB_LOBBY_ENTER: i32 = 504;
const EVALUE_OK: i32 = 1;
const ENTER_SUCCESS: u32 = 1;
/// `k_cubChatMetadataMax` (isteamfriends.h) bounds a lobby data value.
const VALUE_BUF: usize = 8192;

/// Why a lobby cannot be treated as a queue lobby.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    /// Carries `nrsc_lobby_type`: it is Seamless Co-op's.
    IsScLobby,
    /// No `nrpvp=1` tag (or id 0, or data unreadable).
    NotOurs,
}

/// Text lookups on a lobby (data and per-member data) in addition to the MM-1 getters.
pub(crate) trait LobbyText: LobbyApi {
    /// `GetLobbyData`; empty when absent or unreadable.
    fn lobby_data(&self, lobby: u64, key: &str) -> String;
    /// `GetLobbyMemberData`; empty when absent or unreadable.
    fn member_data(&self, lobby: u64, member: u64, key: &str) -> String;
}

/// A lobby our mod created or verified as a queue lobby. The only argument type the write functions accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueueLobby(u64);

impl QueueLobby {
    pub(crate) fn id(&self) -> u64 {
        self.0
    }

    /// Private on purpose: used only for the lobby id returned by OUR OWN `CreateLobby` call result.
    fn created(id: u64) -> QueueLobby {
        QueueLobby(id)
    }

    /// A lobby is a queue lobby only if it carries `nrpvp=1` and does not carry `nrsc_lobby_type`.
    pub(crate) fn verify(api: &dyn LobbyText, id: u64) -> Result<QueueLobby, Refused> {
        if id == 0 {
            return Err(Refused::NotOurs);
        }
        if !api.lobby_data(id, KEY_SC_TYPE).is_empty() {
            return Err(Refused::IsScLobby);
        }
        if api.lobby_data(id, KEY_TAG) != "1" {
            return Err(Refused::NotOurs);
        }
        Ok(QueueLobby(id))
    }

    /// Test-only constructor for the state machine tests (they use a fake Steam).
    #[cfg(test)]
    pub(crate) fn for_test(id: u64) -> QueueLobby {
        QueueLobby(id)
    }
}

/// A lobby we may call `JoinLobby` on: a verified search result, or the id our own party leader published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinTarget(u64);

impl JoinTarget {
    pub(crate) fn id(&self) -> u64 {
        self.0
    }

    /// A search result whose data (delivered with the list) shows our tag and no SC marker.
    pub(crate) fn candidate(api: &dyn LobbyText, id: u64) -> Result<JoinTarget, Refused> {
        QueueLobby::verify(api, id).map(|q| JoinTarget(q.0))
    }

    /// The queue lobby id our own party leader's mod published in its `nrpvp_q` member data. Its data is not readable before
    /// joining, so it cannot be verified yet; [`QueueLobby::verify`] runs on the lobby after the join, and a lobby that fails
    /// it is never written to or left.
    pub(crate) fn party_hint(id: u64) -> Option<JoinTarget> {
        (id != 0).then_some(JoinTarget(id))
    }

    /// After `JoinLobby` succeeded: verify the lobby (its data is readable now) and turn it into a [`QueueLobby`].
    pub(crate) fn entered(&self, api: &dyn LobbyText) -> Result<QueueLobby, Refused> {
        QueueLobby::verify(api, self.0)
    }
}

/// Seamless Co-op's lobby (carries `nrsc_lobby_type`). Only usable for reading and for our own `nrpvp_q` member value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScLobby(u64);

impl ScLobby {
    pub(crate) fn id(&self) -> u64 {
        self.0
    }
    pub(crate) fn verify(api: &dyn LobbyText, id: u64) -> Option<ScLobby> {
        (id != 0 && !api.lobby_data(id, KEY_SC_TYPE).is_empty()).then_some(ScLobby(id))
    }
    #[cfg(test)]
    pub(crate) fn for_test(id: u64) -> ScLobby {
        ScLobby(id)
    }
}

/// MM-3: the lobby id the HOST of our own verified queue lobby published as `sc_lobby`. The only handle `JoinLobby` (method c) and the
/// `steam://joinlobby` link (method a) accept for Seamless Co-op's lobby. It cannot be built from a bare number: [`Self::from_queue`]
/// needs a verified [`QueueLobby`] whose owner is the host and whose `host` data names the same host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishedScLobby {
    lobby: u64,
    host: u64,
}

impl PublishedScLobby {
    pub(crate) fn id(&self) -> u64 {
        self.lobby
    }
    pub(crate) fn host(&self) -> u64 {
        self.host
    }
    /// Read `sc_lobby` from `queue`. `Ok(None)` while the host has not published one yet; `Err` when the lobby does not look like
    /// the host's (owner or `host` data differ, or the value is the queue lobby itself).
    pub(crate) fn from_queue(
        api: &dyn LobbyText,
        queue: &QueueLobby,
        host: u64,
    ) -> Result<Option<PublishedScLobby>, String> {
        let raw = api.lobby_data(queue.id(), KEY_PUBLISHED_SC);
        if raw.trim().is_empty() {
            return Ok(None);
        }
        let lobby: u64 = raw
            .trim()
            .parse()
            .map_err(|_| format!("sc_lobby value {raw:?} is not a number"))?;
        if lobby == 0 || lobby == queue.id() || host == 0 {
            return Err(format!("sc_lobby value {raw:?} is not usable"));
        }
        let data_host = api.lobby_data(queue.id(), crate::mm::proto::K_HOST);
        if data_host.trim() != host.to_string() {
            return Err(format!(
                "queue lobby host data {data_host:?} is not the roster host {host}"
            ));
        }
        let owner = api.owner(queue.id());
        if owner != host {
            return Err(format!(
                "queue lobby owner {owner} is not the host {host}; not trusting its sc_lobby"
            ));
        }
        Ok(Some(PublishedScLobby { lobby, host }))
    }
}

/// `ELobbyType` values (isteammatchmaking.h): Private = 0, FriendsOnly = 1, Public = 2, Invisible = 3, PrivateUnique = 4. Only the
/// four the dev buttons use are representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScType {
    Public,
    Friends,
    Invisible,
    Private,
}

impl ScType {
    /// MM-3d `mm_sc_create_type`: `"public"` | `"friends"` | `"private"` (case-insensitive). Invisible is not offered there.
    pub(crate) fn parse_create(s: &str) -> Option<ScType> {
        match s.trim().to_ascii_lowercase().as_str() {
            "public" => Some(ScType::Public),
            "friends" => Some(ScType::Friends),
            "private" => Some(ScType::Private),
            _ => None,
        }
    }
    /// The config spelling (inverse of [`Self::parse_create`]); `"public"` for the types the config cannot name.
    pub(crate) fn create_name(self) -> &'static str {
        match self {
            ScType::Friends => "friends",
            ScType::Private => "private",
            _ => "public",
        }
    }
    pub(crate) fn raw(self) -> c_int {
        match self {
            ScType::Private => 0,
            ScType::Friends => 1,
            ScType::Public => 2,
            ScType::Invisible => 3,
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            ScType::Friends => "FriendsOnly (1)",
            ScType::Public => "Public (2)",
            ScType::Invisible => "Invisible (3)",
            ScType::Private => "Private (0)",
        }
    }
}

/// Which call result a handle belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallKind {
    MatchList,
    Created,
    Enter,
}

/// A decoded call result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallData {
    MatchList {
        count: u32,
    },
    /// `lobby` is `Some` only when Steam said OK and the id is non-zero.
    Created {
        result: i32,
        lobby: Option<QueueLobby>,
    },
    Enter {
        lobby: u64,
        response: u32,
        locked: bool,
    },
}

impl CallData {
    pub(crate) fn enter_ok(&self) -> bool {
        matches!(self, CallData::Enter { response, .. } if *response == ENTER_SUCCESS)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallPoll {
    Pending,
    Done(CallData),
    /// `invalid_handle`: Steam no longer knows the handle (the result may already have been consumed by the game's callback pump).
    Failed {
        reason: String,
        invalid_handle: bool,
    },
}

/// Lobby search request (all filters are "equal"; distance is always worldwide).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchFilters {
    pub strings: Vec<(String, String)>,
    pub slots: i32,
    pub max_results: i32,
}

/// Everything the queue state machine needs from Steam. Implemented by [`RealSteam`] and by a fake in the tests.
pub(crate) trait QueueSteam: LobbyText {
    fn own_id(&self) -> u64;
    /// Display name (`GetFriendPersonaName`), or the decimal id when unavailable.
    fn persona(&self, id: u64) -> String;
    fn begin_search(&mut self, f: &SearchFilters) -> Result<u64, String>;
    fn begin_create(&mut self, max_members: i32) -> Result<u64, String>;
    fn begin_join(&mut self, target: &JoinTarget) -> Result<u64, String>;
    fn poll(&mut self, handle: u64, kind: CallKind) -> CallPoll;
    fn lobby_by_index(&self, index: i32) -> u64;
    // Writes: queue lobby only.
    fn set_data(&mut self, lobby: &QueueLobby, key: &str, value: &str) -> bool;
    fn set_joinable(&mut self, lobby: &QueueLobby, joinable: bool) -> bool;
    fn set_my_member_data(&mut self, lobby: &QueueLobby, key: &str, value: &str);
    fn leave(&mut self, lobby: &QueueLobby);
    /// Our own member value `nrpvp_q` on Seamless Co-op's lobby (empty string clears it). The only write we make there.
    fn set_sc_queue_value(&mut self, sc: &ScLobby, value: &str);

    // ---- MM-3: join methods. The ONLY other things ever done to Seamless Co-op's lobby (besides reads and `nrpvp_q` above). ----
    /// `ISteamUtils::GetAppID()`, 0 when unavailable.
    fn app_id(&self) -> u32;
    /// `ISteamUtils::IsOverlayEnabled()`, false when unavailable.
    fn overlay_enabled(&self) -> bool;
    /// Method (b): `ISteamMatchmaking::InviteUserToLobby(sc, invitee)`. The returned bool is Steam's ("sent", not "accepted").
    fn invite_to_sc(&mut self, sc: &ScLobby, invitee: u64) -> bool;
    /// Method (d): `ISteamMatchmaking::SetLobbyType(sc, ty)`. Dev button only, never automatic.
    fn set_sc_type(&mut self, sc: &ScLobby, ty: ScType) -> bool;
    /// Method (c): `JoinLobby(published sc lobby)`. Returns the call handle.
    fn begin_join_sc(&mut self, sc: &PublishedScLobby) -> Result<u64, String>;
    /// Method (a): hand `url` (a `steam://` URL built by `mm::join`) to the shell.
    fn open_url(&mut self, url: &str) -> Result<(), String>;

    // ---- MM-3d: the one lobby creation that is not a queue lobby. ----
    /// `ISteamMatchmaking::CreateLobby(ty, max_members)` through the same interface the game uses, to see whether Seamless Co-op adopts
    /// the result. Returns the call handle; the result is polled with [`CallKind::Created`] and yields a [`QueueLobby`] (the lobby is
    /// ours, so `leave` may be called on it, but `leave` refuses at call time while it carries `nrsc_lobby_type`).
    fn begin_create_sc(&mut self, ty: ScType, max_members: i32) -> Result<u64, String>;
}

// ---- flat function types (steam_api_flat.h) ----
type FnRequestLobbyList = unsafe extern "system" fn(*mut c_void) -> u64;
type FnStringFilter = unsafe extern "system" fn(*mut c_void, *const c_char, *const c_char, c_int);
type FnIntArg = unsafe extern "system" fn(*mut c_void, c_int);
type FnLobbyByIndex = unsafe extern "system" fn(*mut c_void, c_int) -> u64;
type FnCreateLobby = unsafe extern "system" fn(*mut c_void, c_int, c_int) -> u64;
type FnJoinLobby = unsafe extern "system" fn(*mut c_void, u64) -> u64;
type FnLeaveLobby = unsafe extern "system" fn(*mut c_void, u64);
type FnGetLobbyData = unsafe extern "system" fn(*mut c_void, u64, *const c_char) -> *const c_char;
type FnSetLobbyData =
    unsafe extern "system" fn(*mut c_void, u64, *const c_char, *const c_char) -> bool;
type FnGetMemberData =
    unsafe extern "system" fn(*mut c_void, u64, u64, *const c_char) -> *const c_char;
type FnSetMemberData = unsafe extern "system" fn(*mut c_void, u64, *const c_char, *const c_char);
type FnSetJoinable = unsafe extern "system" fn(*mut c_void, u64, bool) -> bool;
type FnIsCompleted = unsafe extern "system" fn(*mut c_void, u64, *mut bool) -> bool;
type FnFailureReason = unsafe extern "system" fn(*mut c_void, u64) -> c_int;
type FnGetResult =
    unsafe extern "system" fn(*mut c_void, u64, *mut c_void, c_int, c_int, *mut bool) -> bool;
type FnFriendName = unsafe extern "system" fn(*mut c_void, u64) -> *const c_char;

macro_rules! fns {
    ($sname:ident { $($field:ident : $ty:ty = $name:literal),* $(,)? }) => {
        #[derive(Default)]
        struct $sname { $($field: Option<$ty>,)* }
        impl $sname {
            /// Resolve every export; the names that were not found are pushed on `missing`.
            fn resolve(missing: &mut Vec<&'static str>) -> $sname {
                $sname { $($field: {
                    let a = export(concat!($name, "\0"));
                    if a.is_none() { missing.push($name); }
                    // SAFETY: the export has the signature declared in steam_api_flat.h for exactly this name; the address
                    // came from GetProcAddress for that name. `usize` -> fn pointer is the same size on x64.
                    a.map(|a| unsafe { std::mem::transmute::<usize, $ty>(a) })
                },)* }
            }
        }
    };
}

fns! { Fns {
    request_list: FnRequestLobbyList = "SteamAPI_ISteamMatchmaking_RequestLobbyList",
    string_filter: FnStringFilter = "SteamAPI_ISteamMatchmaking_AddRequestLobbyListStringFilter",
    slots_filter: FnIntArg = "SteamAPI_ISteamMatchmaking_AddRequestLobbyListFilterSlotsAvailable",
    distance_filter: FnIntArg = "SteamAPI_ISteamMatchmaking_AddRequestLobbyListDistanceFilter",
    count_filter: FnIntArg = "SteamAPI_ISteamMatchmaking_AddRequestLobbyListResultCountFilter",
    by_index: FnLobbyByIndex = "SteamAPI_ISteamMatchmaking_GetLobbyByIndex",
    create: FnCreateLobby = "SteamAPI_ISteamMatchmaking_CreateLobby",
    join: FnJoinLobby = "SteamAPI_ISteamMatchmaking_JoinLobby",
    leave: FnLeaveLobby = "SteamAPI_ISteamMatchmaking_LeaveLobby",
    get_data: FnGetLobbyData = "SteamAPI_ISteamMatchmaking_GetLobbyData",
    set_data: FnSetLobbyData = "SteamAPI_ISteamMatchmaking_SetLobbyData",
    get_member_data: FnGetMemberData = "SteamAPI_ISteamMatchmaking_GetLobbyMemberData",
    set_member_data: FnSetMemberData = "SteamAPI_ISteamMatchmaking_SetLobbyMemberData",
    set_joinable: FnSetJoinable = "SteamAPI_ISteamMatchmaking_SetLobbyJoinable",
    is_completed: FnIsCompleted = "SteamAPI_ISteamUtils_IsAPICallCompleted",
    failure_reason: FnFailureReason = "SteamAPI_ISteamUtils_GetAPICallFailureReason",
    get_result: FnGetResult = "SteamAPI_ISteamUtils_GetAPICallResult",
    friend_name: FnFriendName = "SteamAPI_ISteamFriends_GetFriendPersonaName",
} }

// MM-3 (join methods). Kept apart from `Fns` so that a missing export here never changes MM-2 behaviour.
// Signatures from steam_api_flat.h (steamworks-rs bundle, fetched 2026-10-12): `bool SteamAPI_ISteamMatchmaking_InviteUserToLobby(
// self, uint64_steamid lobby, uint64_steamid invitee)`, `bool SteamAPI_ISteamMatchmaking_SetLobbyType(self, uint64_steamid, ELobbyType)`,
// `uint32 SteamAPI_ISteamUtils_GetAppID(self)`, `bool SteamAPI_ISteamUtils_IsOverlayEnabled(self)`. All four exports were found in the
// game's steam_api64.dll (checked 2026-10-12 by scanning the file for the names).
type FnInvite = unsafe extern "system" fn(*mut c_void, u64, u64) -> bool;
type FnSetType = unsafe extern "system" fn(*mut c_void, u64, c_int) -> bool;
type FnAppId = unsafe extern "system" fn(*mut c_void) -> u32;
type FnOverlay = unsafe extern "system" fn(*mut c_void) -> bool;

fns! { JoinFns {
    invite: FnInvite = "SteamAPI_ISteamMatchmaking_InviteUserToLobby",
    set_type: FnSetType = "SteamAPI_ISteamMatchmaking_SetLobbyType",
    app_id: FnAppId = "SteamAPI_ISteamUtils_GetAppID",
    overlay: FnOverlay = "SteamAPI_ISteamUtils_IsOverlayEnabled",
} }

/// The real thing: the flat exports of the already-initialised `steam_api64.dll`.
pub(crate) struct RealSteam {
    flat: Flat,
    f: Fns,
    j: JoinFns,
    mm: usize,
    utils: usize,
    friends: usize,
    own: u64,
    /// Export names that did not resolve (the queue refuses to start when any is missing).
    pub missing: Vec<&'static str>,
    /// MM-3 exports that did not resolve (only the join methods that need them are refused).
    pub join_missing: Vec<&'static str>,
}

fn cs(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| format!("string {s:?} contains a NUL"))
}

/// Copy a `const char*` returned by Steam right away (it is only valid until the next call).
///
/// # Safety
/// `p` is null or a NUL-terminated string returned by Steam.
unsafe fn copy_cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: caller contract.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

impl RealSteam {
    /// Resolve exports and accessors. `None` if the matchmaking accessor is unavailable.
    pub(crate) fn new() -> Option<RealSteam> {
        let (mut flat, _log) = Flat::resolve();
        let mm = Flat::call_accessor(flat.matchmaking).unwrap_or(0);
        let utils = Flat::call_accessor(flat.utils).unwrap_or(0);
        let friends = Flat::call_accessor(flat.friends).unwrap_or(0);
        let user = Flat::call_accessor(flat.user).unwrap_or(0);
        if !flat.bind_matchmaking(mm) {
            return None;
        }
        let own = flat.own_steam_id(user).unwrap_or(0);
        let mut missing = Vec::new();
        let f = Fns::resolve(&mut missing);
        let mut join_missing = Vec::new();
        let j = JoinFns::resolve(&mut join_missing);
        if utils == 0 {
            missing.push("SteamAPI_SteamUtils_v010 (returned NULL)");
        }
        Some(RealSteam {
            flat,
            f,
            j,
            mm,
            utils,
            friends,
            own,
            missing,
            join_missing,
        })
    }

    fn mmp(&self) -> *mut c_void {
        self.mm as *mut c_void
    }

    fn miss(name: &str) -> String {
        format!("export {name} missing")
    }
}

impl LobbyApi for RealSteam {
    fn owner(&self, lobby: u64) -> u64 {
        self.flat.owner(lobby)
    }
    fn member_count(&self, lobby: u64) -> i32 {
        self.flat.member_count(lobby)
    }
    fn member_limit(&self, lobby: u64) -> i32 {
        self.flat.member_limit(lobby)
    }
    fn member_by_index(&self, lobby: u64, index: i32) -> u64 {
        self.flat.member_by_index(lobby, index)
    }
    fn data_count(&self, lobby: u64) -> i32 {
        self.flat.data_count(lobby)
    }
    fn data_by_index(&self, lobby: u64, index: i32) -> Option<(String, String)> {
        self.flat.data_by_index(lobby, index)
    }
}

impl LobbyText for RealSteam {
    fn lobby_data(&self, lobby: u64, key: &str) -> String {
        let (Some(f), Ok(k)) = (self.f.get_data, cs(key)) else {
            return String::new();
        };
        if self.mm == 0 {
            return String::new();
        }
        // SAFETY: non-null ISteamMatchmaking*; `GetLobbyData(self, uint64, const char*)` returns a Steam-owned C string (or ""),
        // copied immediately. `k` is NUL-terminated and outlives the call.
        unsafe { copy_cstr(f(self.mmp(), lobby, k.as_ptr())) }
    }
    fn member_data(&self, lobby: u64, member: u64, key: &str) -> String {
        let (Some(f), Ok(k)) = (self.f.get_member_data, cs(key)) else {
            return String::new();
        };
        if self.mm == 0 {
            return String::new();
        }
        // SAFETY: as `lobby_data`; `GetLobbyMemberData(self, uint64, uint64, const char*)`.
        unsafe { copy_cstr(f(self.mmp(), lobby, member, k.as_ptr())) }
    }
}

impl QueueSteam for RealSteam {
    fn own_id(&self) -> u64 {
        self.own
    }

    fn persona(&self, id: u64) -> String {
        if let (Some(f), true) = (self.f.friend_name, self.friends != 0) {
            // SAFETY: non-null ISteamFriends*; `GetFriendPersonaName(self, uint64)` returns a Steam-owned C string, copied now.
            let s = unsafe { copy_cstr(f(self.friends as *mut c_void, id)) };
            if !s.is_empty() && s != "[unknown]" {
                return s;
            }
        }
        id.to_string()
    }

    fn begin_search(&mut self, fl: &SearchFilters) -> Result<u64, String> {
        let (Some(sf), Some(slots), Some(dist), Some(cnt), Some(req)) = (
            self.f.string_filter,
            self.f.slots_filter,
            self.f.distance_filter,
            self.f.count_filter,
            self.f.request_list,
        ) else {
            return Err(Self::miss("AddRequestLobbyList*/RequestLobbyList"));
        };
        let p = self.mmp();
        for (k, v) in &fl.strings {
            let (ck, cv) = (cs(k)?, cs(v)?);
            // SAFETY: non-null ISteamMatchmaking*; signature `(self, const char*, const char*, ELobbyComparison)` per
            // steam_api_flat.h; both strings are NUL-terminated and outlive the call.
            unsafe { sf(p, ck.as_ptr(), cv.as_ptr(), COMPARISON_EQUAL) };
        }
        // SAFETY: same interface pointer; each takes one int (steam_api_flat.h).
        unsafe {
            slots(p, fl.slots);
            dist(p, DISTANCE_WORLDWIDE);
            cnt(p, fl.max_results);
        }
        // SAFETY: `RequestLobbyList(self)` returns a SteamAPICall_t (uint64).
        let h = unsafe { req(p) };
        tracing::info!(
            "mm_queue: Steam RequestLobbyList filters={:?} slots_available={} max_results={} distance=worldwide -> handle {h}",
            fl.strings,
            fl.slots,
            fl.max_results
        );
        if h == 0 {
            Err("RequestLobbyList returned handle 0".into())
        } else {
            Ok(h)
        }
    }

    fn begin_create(&mut self, max_members: i32) -> Result<u64, String> {
        let Some(f) = self.f.create else {
            return Err(Self::miss("SteamAPI_ISteamMatchmaking_CreateLobby"));
        };
        // SAFETY: non-null ISteamMatchmaking*; `CreateLobby(self, ELobbyType, int)` -> SteamAPICall_t.
        let h = unsafe { f(self.mmp(), LOBBY_TYPE_INVISIBLE, max_members) };
        tracing::info!("mm_queue: Steam CreateLobby(Invisible, {max_members}) -> handle {h}");
        if h == 0 {
            Err("CreateLobby returned handle 0".into())
        } else {
            Ok(h)
        }
    }

    fn begin_join(&mut self, t: &JoinTarget) -> Result<u64, String> {
        let Some(f) = self.f.join else {
            return Err(Self::miss("SteamAPI_ISteamMatchmaking_JoinLobby"));
        };
        // SAFETY: non-null ISteamMatchmaking*; `JoinLobby(self, uint64)` -> SteamAPICall_t. The target is a verified
        // queue-lobby candidate or our party leader's published id (see `JoinTarget`).
        let h = unsafe { f(self.mmp(), t.0) };
        tracing::info!(
            "mm_queue: Steam JoinLobby({} (0x{:X})) -> handle {h}",
            t.0,
            t.0
        );
        if h == 0 {
            Err("JoinLobby returned handle 0".into())
        } else {
            Ok(h)
        }
    }

    fn poll(&mut self, handle: u64, kind: CallKind) -> CallPoll {
        let (Some(done), Some(why), Some(get)) = (
            self.f.is_completed,
            self.f.failure_reason,
            self.f.get_result,
        ) else {
            return CallPoll::Failed {
                reason: Self::miss("ISteamUtils call-result functions"),
                invalid_handle: false,
            };
        };
        if self.utils == 0 {
            return CallPoll::Failed {
                reason: "ISteamUtils is NULL".into(),
                invalid_handle: false,
            };
        }
        let u = self.utils as *mut c_void;
        let mut failed = false;
        // SAFETY: non-null ISteamUtils*; `IsAPICallCompleted(self, SteamAPICall_t, bool*)`; `failed` is a valid local.
        let completed = unsafe { done(u, handle, &mut failed) };
        if !completed {
            return CallPoll::Pending;
        }
        let fail = |reason: i32| CallPoll::Failed {
            reason: format!("ESteamAPICallFailure {reason}"),
            invalid_handle: reason == 2,
        };
        if failed {
            // SAFETY: `GetAPICallFailureReason(self, SteamAPICall_t)` -> enum (int).
            return fail(unsafe { why(u, handle) });
        }
        let (id, size) = match kind {
            CallKind::MatchList => (CB_LOBBY_MATCH_LIST, 4usize),
            CallKind::Created => (CB_LOBBY_CREATED, 16),
            CallKind::Enter => (CB_LOBBY_ENTER, 24),
        };
        let mut buf = [0u8; 32];
        let mut failed2 = false;
        // SAFETY: `GetAPICallResult(self, SteamAPICall_t, void*, int, int, bool*)`; `buf` holds `size` (<= 32) writable bytes;
        // `failed2` is a valid local.
        let ok = unsafe {
            get(
                u,
                handle,
                buf.as_mut_ptr() as *mut c_void,
                size as c_int,
                id,
                &mut failed2,
            )
        };
        if !ok || failed2 {
            // SAFETY: as above.
            return fail(unsafe { why(u, handle) });
        }
        let u32_at = |o: usize| u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
        let u64_at = |o: usize| {
            let mut a = [0u8; 8];
            a.copy_from_slice(&buf[o..o + 8]);
            u64::from_le_bytes(a)
        };
        CallPoll::Done(match kind {
            CallKind::MatchList => CallData::MatchList { count: u32_at(0) },
            CallKind::Created => {
                let (result, lobby) = (u32_at(0) as i32, u64_at(8));
                CallData::Created {
                    result,
                    lobby: (result == EVALUE_OK && lobby != 0).then(|| QueueLobby::created(lobby)),
                }
            }
            CallKind::Enter => CallData::Enter {
                lobby: u64_at(0),
                response: u32_at(16),
                locked: buf[12] != 0,
            },
        })
    }

    fn lobby_by_index(&self, index: i32) -> u64 {
        match (self.f.by_index, self.mm) {
            // SAFETY: non-null ISteamMatchmaking*; `GetLobbyByIndex(self, int)` -> uint64 (invalid id if out of range).
            (Some(f), p) if p != 0 => unsafe { f(p as *mut c_void, index) },
            _ => 0,
        }
    }

    fn set_data(&mut self, lobby: &QueueLobby, key: &str, value: &str) -> bool {
        let (Some(f), Ok(k), Ok(v)) = (self.f.set_data, cs(key), cs(value)) else {
            tracing::warn!("mm_queue: Steam SetLobbyData({}, {key:?}) not callable (export missing or NUL in text)", lobby.0);
            return false;
        };
        if value.len() >= VALUE_BUF {
            return false;
        }
        // SAFETY: non-null ISteamMatchmaking*; `SetLobbyData(self, uint64, const char*, const char*)` -> bool. `lobby` is a
        // QueueLobby (never SC's). Strings are NUL-terminated and outlive the call.
        let ok = unsafe { f(self.mmp(), lobby.0, k.as_ptr(), v.as_ptr()) };
        tracing::info!(
            "mm_queue: Steam SetLobbyData({}, {key:?}, {value:?}) -> {ok}",
            lobby.0
        );
        ok
    }

    fn set_joinable(&mut self, lobby: &QueueLobby, joinable: bool) -> bool {
        let Some(f) = self.f.set_joinable else {
            return false;
        };
        // SAFETY: as `set_data`; `SetLobbyJoinable(self, uint64, bool)` -> bool.
        let ok = unsafe { f(self.mmp(), lobby.0, joinable) };
        tracing::info!(
            "mm_queue: Steam SetLobbyJoinable({}, {joinable}) -> {ok}",
            lobby.0
        );
        ok
    }

    fn set_my_member_data(&mut self, lobby: &QueueLobby, key: &str, value: &str) {
        let (Some(f), Ok(k), Ok(v)) = (self.f.set_member_data, cs(key), cs(value)) else {
            return;
        };
        // SAFETY: as `set_data`; `SetLobbyMemberData(self, uint64, const char*, const char*)` (sets OUR member entry).
        unsafe { f(self.mmp(), lobby.0, k.as_ptr(), v.as_ptr()) };
        tracing::info!(
            "mm_queue: Steam SetLobbyMemberData({}, {key:?}, {value:?})",
            lobby.0
        );
    }

    fn leave(&mut self, lobby: &QueueLobby) {
        // Belt and braces: even a QueueLobby is refused if the lobby carries Seamless Co-op's marker.
        if !self.lobby_data(lobby.0, KEY_SC_TYPE).is_empty() {
            tracing::error!(
                "mm_queue: REFUSED LeaveLobby({}): it carries {KEY_SC_TYPE}",
                lobby.0
            );
            return;
        }
        let Some(f) = self.f.leave else { return };
        // SAFETY: non-null ISteamMatchmaking*; `LeaveLobby(self, uint64)`. Only ever our queue lobby.
        unsafe { f(self.mmp(), lobby.0) };
        tracing::info!("mm_queue: Steam LeaveLobby({} (0x{:X}))", lobby.0, lobby.0);
    }

    fn set_sc_queue_value(&mut self, sc: &ScLobby, value: &str) {
        let (Some(f), Ok(k), Ok(v)) = (self.f.set_member_data, cs(KEY_SC_MEMBER), cs(value)) else {
            return;
        };
        // SAFETY: as `set_my_member_data`. The key is the fixed constant `nrpvp_q`: this is the only write on SC's lobby.
        unsafe { f(self.mmp(), sc.0, k.as_ptr(), v.as_ptr()) };
        tracing::info!(
            "mm_queue: Steam SetLobbyMemberData(SC lobby {}, {KEY_SC_MEMBER:?}, {value:?})",
            sc.0
        );
    }

    fn app_id(&self) -> u32 {
        match (self.j.app_id, self.utils) {
            // SAFETY: non-null ISteamUtils*; `uint32 GetAppID(self)` (steam_api_flat.h).
            (Some(f), u) if u != 0 => unsafe { f(u as *mut c_void) },
            _ => 0,
        }
    }

    fn overlay_enabled(&self) -> bool {
        match (self.j.overlay, self.utils) {
            // SAFETY: non-null ISteamUtils*; `bool IsOverlayEnabled(self)` (steam_api_flat.h).
            (Some(f), u) if u != 0 => unsafe { f(u as *mut c_void) },
            _ => false,
        }
    }

    fn invite_to_sc(&mut self, sc: &ScLobby, invitee: u64) -> bool {
        let Some(f) = self.j.invite else {
            tracing::warn!("mm_join: Steam InviteUserToLobby not callable: export missing");
            return false;
        };
        // SAFETY: non-null ISteamMatchmaking*; `bool InviteUserToLobby(self, uint64 lobby, uint64 invitee)`. `sc` is a verified
        // Seamless Co-op lobby we are a member of; an invite changes nothing in its data.
        let ok = unsafe { f(self.mmp(), sc.0, invitee) };
        tracing::info!(
            "mm_join: Steam InviteUserToLobby(SC lobby {} (0x{:X}), invitee {}) -> {ok}",
            sc.0,
            sc.0,
            invitee
        );
        ok
    }

    fn set_sc_type(&mut self, sc: &ScLobby, ty: ScType) -> bool {
        let Some(f) = self.j.set_type else {
            tracing::warn!("mm_join: Steam SetLobbyType not callable: export missing");
            return false;
        };
        // SAFETY: non-null ISteamMatchmaking*; `bool SetLobbyType(self, uint64 lobby, ELobbyType)`; ELobbyType is a 32-bit enum.
        // Reached only from a dev button, for a verified Seamless Co-op lobby we own (the caller checks).
        let ok = unsafe { f(self.mmp(), sc.0, ty.raw()) };
        tracing::info!(
            "mm_join: Steam SetLobbyType(SC lobby {} (0x{:X}), {}) -> {ok}",
            sc.0,
            sc.0,
            ty.label()
        );
        ok
    }

    fn begin_join_sc(&mut self, sc: &PublishedScLobby) -> Result<u64, String> {
        let Some(f) = self.f.join else {
            return Err(Self::miss("SteamAPI_ISteamMatchmaking_JoinLobby"));
        };
        // SAFETY: non-null ISteamMatchmaking*; `JoinLobby(self, uint64)` -> SteamAPICall_t. The id came from the host of our own
        // verified queue lobby (`PublishedScLobby`).
        let h = unsafe { f(self.mmp(), sc.lobby) };
        tracing::info!(
            "mm_join: Steam JoinLobby(SC lobby {} (0x{:X}), published by host {}) -> handle {h}",
            sc.lobby,
            sc.lobby,
            sc.host
        );
        if h == 0 {
            Err("JoinLobby returned handle 0".into())
        } else {
            Ok(h)
        }
    }

    fn begin_create_sc(&mut self, ty: ScType, max_members: i32) -> Result<u64, String> {
        let Some(f) = self.f.create else {
            return Err(Self::miss("SteamAPI_ISteamMatchmaking_CreateLobby"));
        };
        // SAFETY: non-null ISteamMatchmaking*; `CreateLobby(self, ELobbyType, int)` -> SteamAPICall_t (steam_api_flat.h), the same
        // call as `begin_create` with another lobby type (S9 §5d: the game itself calls CreateLobby(2, 6) through this interface).
        let h = unsafe { f(self.mmp(), ty.raw(), max_members) };
        tracing::info!(
            "mm_sc_create: Steam CreateLobby({}, {max_members}) -> handle {h}",
            ty.label()
        );
        if h == 0 {
            Err("CreateLobby returned handle 0".into())
        } else {
            Ok(h)
        }
    }

    fn open_url(&mut self, url: &str) -> Result<(), String> {
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call; "open" is a static wide string; the other parameters are null.
        // ShellExecuteW returns an HINSTANCE that is > 32 on success (documented Win32 contract).
        let r = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(wide.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            )
        };
        let code = r.0 as isize;
        tracing::info!(
            "mm_join: ShellExecuteW(open, {url:?}) -> {code} (> 32 = handed to the shell)"
        );
        if code > 32 {
            Ok(())
        } else {
            Err(format!("ShellExecuteW returned {code}"))
        }
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A fake Steam for the unit tests (queue state machine and the lobby-type rules).
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    pub(crate) struct FakeLobby {
        pub owner: u64,
        pub limit: i32,
        pub members: Vec<u64>,
        pub data: HashMap<String, String>,
        pub member_data: HashMap<(u64, String), String>,
    }

    /// One recorded write.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Write {
        SetData(u64, String, String),
        Joinable(u64, bool),
        MemberData(u64, String, String),
        Leave(u64),
        ScValue(u64, String),
        Join(u64),
        Create(i32),
        Search(SearchFilters),
        Invite(u64, u64),
        SetType(u64, ScType),
        JoinSc(u64),
        OpenUrl(String),
        CreateSc(ScType, i32),
    }

    #[derive(Default)]
    pub(crate) struct FakeSteam {
        pub own: u64,
        pub lobbies: HashMap<u64, FakeLobby>,
        pub writes: Vec<Write>,
        pub next_handle: u64,
        pub next_lobby: u64,
        /// handle -> result once "completed".
        pub results: HashMap<u64, CallPoll>,
        pub auto_search: Option<Vec<u64>>,
        pub search_results: Vec<u64>,
        pub create_fails: bool,
        pub app_id: u32,
        pub overlay: bool,
        pub invite_ok: bool,
        pub url_ok: bool,
        /// MM-3d: a lobby made by `begin_create_sc` carries `nrsc_lobby_type` from the start (Seamless Co-op adopted it).
        pub adopt_created: bool,
    }

    impl FakeSteam {
        pub(crate) fn new(own: u64) -> FakeSteam {
            FakeSteam {
                own,
                next_handle: 100,
                next_lobby: 1000,
                app_id: 2622380,
                overlay: true,
                invite_ok: true,
                url_ok: true,
                ..Default::default()
            }
        }
        pub(crate) fn add_lobby(
            &mut self,
            id: u64,
            owner: u64,
            limit: i32,
            members: &[u64],
            data: &[(&str, &str)],
        ) {
            let l = self.lobbies.entry(id).or_default();
            l.owner = owner;
            l.limit = limit;
            l.members = members.to_vec();
            for (k, v) in data {
                l.data.insert((*k).into(), (*v).into());
            }
        }
        pub(crate) fn set_member(&mut self, lobby: u64, member: u64, key: &str, v: &str) {
            if let Some(l) = self.lobbies.get_mut(&lobby) {
                l.member_data.insert((member, key.into()), v.into());
            }
        }
        pub(crate) fn writes_to(&self, lobby: u64) -> Vec<&Write> {
            self.writes
                .iter()
                .filter(|w| match w {
                    Write::SetData(l, ..)
                    | Write::Joinable(l, _)
                    | Write::MemberData(l, ..)
                    | Write::Leave(l)
                    | Write::ScValue(l, _)
                    | Write::Invite(l, _)
                    | Write::SetType(l, _)
                    | Write::JoinSc(l) => *l == lobby,
                    _ => false,
                })
                .collect()
        }
    }

    impl LobbyApi for FakeSteam {
        fn owner(&self, l: u64) -> u64 {
            self.lobbies.get(&l).map_or(0, |x| x.owner)
        }
        fn member_count(&self, l: u64) -> i32 {
            self.lobbies.get(&l).map_or(0, |x| x.members.len() as i32)
        }
        fn member_limit(&self, l: u64) -> i32 {
            self.lobbies.get(&l).map_or(0, |x| x.limit)
        }
        fn member_by_index(&self, l: u64, i: i32) -> u64 {
            self.lobbies
                .get(&l)
                .and_then(|x| x.members.get(i as usize).copied())
                .unwrap_or(0)
        }
        fn data_count(&self, l: u64) -> i32 {
            self.lobbies.get(&l).map_or(0, |x| x.data.len() as i32)
        }
        fn data_by_index(&self, l: u64, i: i32) -> Option<(String, String)> {
            let x = self.lobbies.get(&l)?;
            let mut keys: Vec<_> = x.data.iter().collect();
            keys.sort();
            keys.get(i as usize)
                .map(|(k, v)| ((*k).clone(), (*v).clone()))
        }
    }

    impl LobbyText for FakeSteam {
        fn lobby_data(&self, l: u64, k: &str) -> String {
            self.lobbies
                .get(&l)
                .and_then(|x| x.data.get(k).cloned())
                .unwrap_or_default()
        }
        fn member_data(&self, l: u64, m: u64, k: &str) -> String {
            self.lobbies
                .get(&l)
                .and_then(|x| x.member_data.get(&(m, k.to_string())).cloned())
                .unwrap_or_default()
        }
    }

    impl QueueSteam for FakeSteam {
        fn own_id(&self) -> u64 {
            self.own
        }
        fn persona(&self, id: u64) -> String {
            format!("P{id}")
        }
        fn begin_search(&mut self, f: &SearchFilters) -> Result<u64, String> {
            self.writes.push(Write::Search(f.clone()));
            self.next_handle += 1;
            let h = self.next_handle;
            let ids = self.auto_search.clone().unwrap_or_default();
            self.search_results = ids.clone();
            self.results.insert(
                h,
                CallPoll::Done(CallData::MatchList {
                    count: ids.len() as u32,
                }),
            );
            Ok(h)
        }
        fn begin_create(&mut self, max: i32) -> Result<u64, String> {
            self.writes.push(Write::Create(max));
            self.next_handle += 1;
            let h = self.next_handle;
            if self.create_fails {
                self.results.insert(
                    h,
                    CallPoll::Done(CallData::Created {
                        result: 2,
                        lobby: None,
                    }),
                );
            } else {
                self.next_lobby += 1;
                let id = self.next_lobby;
                let own = self.own;
                self.add_lobby(id, own, max, &[own], &[]);
                self.results.insert(
                    h,
                    CallPoll::Done(CallData::Created {
                        result: 1,
                        lobby: Some(QueueLobby::for_test(id)),
                    }),
                );
            }
            Ok(h)
        }
        fn begin_join(&mut self, t: &JoinTarget) -> Result<u64, String> {
            self.writes.push(Write::Join(t.id()));
            self.next_handle += 1;
            let h = self.next_handle;
            let own = self.own;
            let (ok, id) = match self.lobbies.get_mut(&t.id()) {
                Some(l) if (l.members.len() as i32) < l.limit => {
                    l.members.push(own);
                    (true, t.id())
                }
                _ => (false, t.id()),
            };
            self.results.insert(
                h,
                CallPoll::Done(CallData::Enter {
                    lobby: id,
                    response: if ok { 1 } else { 4 },
                    locked: false,
                }),
            );
            Ok(h)
        }
        fn poll(&mut self, h: u64, _k: CallKind) -> CallPoll {
            self.results.remove(&h).unwrap_or(CallPoll::Pending)
        }
        fn lobby_by_index(&self, i: i32) -> u64 {
            self.search_results.get(i as usize).copied().unwrap_or(0)
        }
        fn set_data(&mut self, l: &QueueLobby, k: &str, v: &str) -> bool {
            self.writes.push(Write::SetData(l.id(), k.into(), v.into()));
            if let Some(x) = self.lobbies.get_mut(&l.id()) {
                x.data.insert(k.into(), v.into());
            }
            true
        }
        fn set_joinable(&mut self, l: &QueueLobby, j: bool) -> bool {
            self.writes.push(Write::Joinable(l.id(), j));
            true
        }
        fn set_my_member_data(&mut self, l: &QueueLobby, k: &str, v: &str) {
            self.writes
                .push(Write::MemberData(l.id(), k.into(), v.into()));
            let own = self.own;
            self.set_member(l.id(), own, k, v);
        }
        fn leave(&mut self, l: &QueueLobby) {
            self.writes.push(Write::Leave(l.id()));
            let own = self.own;
            if let Some(x) = self.lobbies.get_mut(&l.id()) {
                x.members.retain(|m| *m != own);
                if x.owner == own {
                    x.owner = x.members.first().copied().unwrap_or(0);
                }
            }
        }
        fn set_sc_queue_value(&mut self, sc: &ScLobby, v: &str) {
            self.writes.push(Write::ScValue(sc.id(), v.into()));
            let own = self.own;
            self.set_member(sc.id(), own, KEY_SC_MEMBER, v);
        }
        fn app_id(&self) -> u32 {
            self.app_id
        }
        fn overlay_enabled(&self) -> bool {
            self.overlay
        }
        fn invite_to_sc(&mut self, sc: &ScLobby, invitee: u64) -> bool {
            self.writes.push(Write::Invite(sc.id(), invitee));
            self.invite_ok
        }
        fn set_sc_type(&mut self, sc: &ScLobby, ty: ScType) -> bool {
            self.writes.push(Write::SetType(sc.id(), ty));
            true
        }
        fn begin_join_sc(&mut self, sc: &PublishedScLobby) -> Result<u64, String> {
            self.writes.push(Write::JoinSc(sc.id()));
            self.next_handle += 1;
            let h = self.next_handle;
            let own = self.own;
            let ok = match self.lobbies.get_mut(&sc.id()) {
                Some(l) if (l.members.len() as i32) < l.limit => {
                    l.members.push(own);
                    true
                }
                _ => false,
            };
            self.results.insert(
                h,
                CallPoll::Done(CallData::Enter {
                    lobby: sc.id(),
                    response: if ok { 1 } else { 15 },
                    locked: true,
                }),
            );
            Ok(h)
        }
        fn begin_create_sc(&mut self, ty: ScType, max: i32) -> Result<u64, String> {
            self.writes.push(Write::CreateSc(ty, max));
            self.next_handle += 1;
            let h = self.next_handle;
            if self.create_fails {
                self.results.insert(
                    h,
                    CallPoll::Done(CallData::Created {
                        result: 2,
                        lobby: None,
                    }),
                );
            } else {
                self.next_lobby += 1;
                let id = self.next_lobby;
                let own = self.own;
                self.add_lobby(id, own, max, &[own], &[]);
                if self.adopt_created {
                    self.add_lobby(
                        id,
                        own,
                        max,
                        &[own],
                        &[(KEY_SC_TYPE, "nrsc_ynx3_seamless_master_lobby")],
                    );
                }
                self.results.insert(
                    h,
                    CallPoll::Done(CallData::Created {
                        result: 1,
                        lobby: Some(QueueLobby::for_test(id)),
                    }),
                );
            }
            Ok(h)
        }
        fn open_url(&mut self, url: &str) -> Result<(), String> {
            self.writes.push(Write::OpenUrl(url.to_string()));
            if self.url_ok {
                Ok(())
            } else {
                Err("fake refusal".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    #[test]
    fn published_sc_lobby_needs_the_host_to_own_and_name_it() {
        let mut s = FakeSteam::new(2);
        s.add_lobby(40, 1, 2, &[1, 2], &[(KEY_TAG, "1"), ("host", "1")]);
        let q = QueueLobby::verify(&s, 40).expect("queue");
        // Nothing published yet.
        assert_eq!(PublishedScLobby::from_queue(&s, &q, 1), Ok(None));
        s.lobbies
            .get_mut(&40)
            .expect("q")
            .data
            .insert(KEY_PUBLISHED_SC.into(), "5555".into());
        let p = PublishedScLobby::from_queue(&s, &q, 1)
            .expect("ok")
            .expect("some");
        assert_eq!((p.id(), p.host()), (5555, 1));
        // The roster host differs from the owner: refused. Same for a `host` value that disagrees.
        assert!(PublishedScLobby::from_queue(&s, &q, 2).is_err());
        s.lobbies
            .get_mut(&40)
            .expect("q")
            .data
            .insert("host".into(), "9".into());
        assert!(PublishedScLobby::from_queue(&s, &q, 1).is_err());
        s.lobbies
            .get_mut(&40)
            .expect("q")
            .data
            .insert("host".into(), "1".into());
        // Garbage, zero and the queue lobby itself are not lobby ids to join.
        for bad in ["abc", "0", "40"] {
            s.lobbies
                .get_mut(&40)
                .expect("q")
                .data
                .insert(KEY_PUBLISHED_SC.into(), bad.into());
            assert!(PublishedScLobby::from_queue(&s, &q, 1).is_err(), "{bad}");
        }
    }

    #[test]
    fn lobby_type_values_match_the_sdk_header() {
        // isteammatchmaking.h ELobbyType: Private 0, FriendsOnly 1, Public 2, Invisible 3, PrivateUnique 4.
        assert_eq!(
            (
                ScType::Private.raw(),
                ScType::Friends.raw(),
                ScType::Public.raw(),
                ScType::Invisible.raw()
            ),
            (0, 1, 2, 3)
        );
    }

    #[test]
    fn create_type_names_round_trip() {
        for t in [ScType::Public, ScType::Friends, ScType::Private] {
            assert_eq!(ScType::parse_create(t.create_name()), Some(t));
        }
        assert_eq!(ScType::parse_create(" PUBLIC "), Some(ScType::Public));
        assert_eq!(ScType::parse_create("invisible"), None);
        assert_eq!(ScType::parse_create(""), None);
    }
    #[test]
    fn verify_refuses_seamless_coop_lobbies_even_when_tagged() {
        let mut s = FakeSteam::new(1);
        s.add_lobby(
            10,
            1,
            6,
            &[1],
            &[(KEY_SC_TYPE, "nrsc_ynx3_seamless_master_lobby")],
        );
        s.add_lobby(11, 1, 6, &[1], &[(KEY_SC_TYPE, "x"), (KEY_TAG, "1")]);
        s.add_lobby(12, 1, 6, &[1], &[("inviteParam", "0")]);
        s.add_lobby(13, 1, 2, &[1], &[(KEY_TAG, "1")]);
        assert_eq!(QueueLobby::verify(&s, 10), Err(Refused::IsScLobby));
        assert_eq!(QueueLobby::verify(&s, 11), Err(Refused::IsScLobby));
        assert_eq!(QueueLobby::verify(&s, 12), Err(Refused::NotOurs));
        assert_eq!(QueueLobby::verify(&s, 0), Err(Refused::NotOurs));
        assert_eq!(QueueLobby::verify(&s, 99), Err(Refused::NotOurs));
        assert_eq!(QueueLobby::verify(&s, 13).map(|q| q.id()), Ok(13));
        assert!(JoinTarget::candidate(&s, 10).is_err());
        assert!(JoinTarget::candidate(&s, 13).is_ok());
        assert!(JoinTarget::party_hint(0).is_none());
        // A party hint joins an unverified lobby, but `entered` still refuses it afterwards.
        let t = JoinTarget::party_hint(10).expect("hint");
        assert_eq!(t.entered(&s), Err(Refused::IsScLobby));
        assert!(ScLobby::verify(&s, 10).is_some() && ScLobby::verify(&s, 13).is_none());
    }
}
