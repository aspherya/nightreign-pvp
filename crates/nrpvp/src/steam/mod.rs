//! MM-1: runtime access to Steam's flat C API, in the Steam DLL the game (and Seamless Co-op) already loaded.
//!
//! We never link `steam_api64.dll` and never initialise Steam: every function is resolved by name with
//! `GetModuleHandleW("steam_api64.dll")` + `GetProcAddress`. This module only RESOLVES and CALLS GETTERS; the exports that
//! initialise, pump or shut Steam down (`SteamAPI_Init*`, `SteamAPI_ManualDispatch_*`, `SteamAPI_RunCallbacks`,
//! `SteamAPI_Shutdown`) are deliberately not declared anywhere. The setters (`SetLobbyData`, `JoinLobby`, `CreateLobby`, ...)
//! of MM-2 live only in `queue.rs`, behind types that restrict them to our own queue lobby.
//!
//! Signatures below are from the Steamworks SDK 1.6x public headers as bundled by steamworks-rs
//! (`https://github.com/Noxime/steamworks-rs/tree/master/steamworks-sys/lib/steam/public/steam/`, fetched 2026-10-12):
//! `steam_api_flat.h` for the `SteamAPI_ISteam*` functions and the accessors, `steam_api_internal.h` for `SteamAPI_GetHSteamUser`
//! and `SteamAPI_RegisterCallback`. On x64 Windows there is one calling convention, so `extern "system"` == `S_CALLTYPE`.
//! A `CSteamID` crosses the flat API as a plain `uint64` (`uint64_steamid` in `steam_api_flat.h`).

pub(crate) mod callback;
#[cfg(feature = "dev")]
pub(crate) mod lobtrace;
pub(crate) mod probe;
pub(crate) mod queue;
pub(crate) mod quietinvite;

use std::ffi::{c_char, c_int, c_void, CStr};

use windows::core::{w, PCSTR};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

/// Name of the Steam client library the game loads (`<Game>\steam_api64.dll`, TECHNICAL_NOTES / S4 §2.1).
pub(crate) const STEAM_DLL: windows::core::PCWSTR = w!("steam_api64.dll");

/// Read-only view of the lobby getters we use. The probe logic is written against this trait so it can be tested without Steam.
pub(crate) trait LobbyApi {
    fn owner(&self, lobby: u64) -> u64;
    fn member_count(&self, lobby: u64) -> i32;
    fn member_limit(&self, lobby: u64) -> i32;
    fn member_by_index(&self, lobby: u64, index: i32) -> u64;
    fn data_count(&self, lobby: u64) -> i32;
    /// `(key, value)` of lobby data entry `index`, `None` if Steam returned false.
    fn data_by_index(&self, lobby: u64, index: i32) -> Option<(String, String)>;
}

// `steam_api_flat.h` signatures (`ISteamMatchmaking*` is the accessor's returned interface pointer).
type FnAccessor = unsafe extern "system" fn() -> *mut c_void;
type FnGetHSteamUser = unsafe extern "system" fn() -> i32;
type FnUserGetSteamId = unsafe extern "system" fn(*mut c_void) -> u64;
type FnLobbyU64 = unsafe extern "system" fn(*mut c_void, u64) -> u64;
type FnLobbyInt = unsafe extern "system" fn(*mut c_void, u64) -> c_int;
type FnLobbyMemberByIndex = unsafe extern "system" fn(*mut c_void, u64, c_int) -> u64;
type FnLobbyDataByIndex = unsafe extern "system" fn(
    *mut c_void,
    u64,
    c_int,
    *mut c_char,
    c_int,
    *mut c_char,
    c_int,
) -> bool;
/// `steam_api_internal.h`: `void SteamAPI_RegisterCallback( class CCallbackBase *pCallback, int iCallback )`.
pub(crate) type FnRegisterCallback = unsafe extern "system" fn(*mut c_void, c_int);

/// Buffer sizes for lobby data: key max `k_nMaxLobbyKeyLength` = 255 (isteammatchmaking.h) + NUL; value max
/// `k_cubChatMetadataMax` = 8192 (isteamfriends.h).
const KEY_BUF: usize = 256;
const VALUE_BUF: usize = 8192;

/// Result of resolving one export, for the log.
pub(crate) struct Resolved {
    pub name: &'static str,
    pub found: bool,
}

/// The flat functions we use. Every field is optional: a missing export is logged and the matching step skipped.
#[derive(Default)]
pub(crate) struct Flat {
    pub get_hsteam_user: Option<FnGetHSteamUser>,
    pub register_callback: Option<FnRegisterCallback>,
    pub matchmaking: Option<FnAccessor>,
    pub utils: Option<FnAccessor>,
    pub user: Option<FnAccessor>,
    pub friends: Option<FnAccessor>,
    user_get_steam_id: Option<FnUserGetSteamId>,
    get_owner: Option<FnLobbyU64>,
    get_member_count: Option<FnLobbyInt>,
    get_member_limit: Option<FnLobbyInt>,
    get_member_by_index: Option<FnLobbyMemberByIndex>,
    get_data_count: Option<FnLobbyInt>,
    get_data_by_index: Option<FnLobbyDataByIndex>,
    /// Interface pointer of `ISteamMatchmaking`, filled by [`Flat::bind_matchmaking`].
    mm_ptr: usize,
}

/// `GetModuleHandleW` + `GetProcAddress` for one NUL-terminated export name. `None` if the DLL is not loaded or lacks it.
pub(crate) fn export(name: &'static str) -> Option<usize> {
    debug_assert!(name.ends_with('\0'));
    // SAFETY: GetModuleHandleW does not load or free anything and takes a valid static wide string; the module stays loaded
    // for the life of the process (steam_api64.dll is never unloaded by the game). GetProcAddress gets a NUL-terminated name.
    unsafe {
        let module = GetModuleHandleW(STEAM_DLL).ok()?;
        GetProcAddress(module, PCSTR(name.as_ptr())).map(|f| f as usize)
    }
}

/// Whether `steam_api64.dll` is loaded in this process (no `GetProcAddress`).
pub(crate) fn steam_dll_loaded() -> bool {
    // SAFETY: see `export`.
    unsafe { GetModuleHandleW(STEAM_DLL).is_ok() }
}

macro_rules! resolve {
    ($log:expr, $name:literal) => {{
        let addr = export(concat!($name, "\0"));
        $log.push(Resolved { name: $name, found: addr.is_some() });
        // SAFETY: the export has the signature documented in steam_api_flat.h / steam_api_internal.h for this name; the
        // address came from GetProcAddress for exactly that name. `usize` -> fn pointer is the same size on x64.
        addr.map(|a| unsafe { std::mem::transmute::<usize, _>(a) })
    }};
}

impl Flat {
    /// Resolve every export we use (once Steam's DLL is loaded). Returns the table plus a per-export result for logging.
    pub(crate) fn resolve() -> (Flat, Vec<Resolved>) {
        let mut log = Vec::new();
        let flat = Flat {
            get_hsteam_user: resolve!(log, "SteamAPI_GetHSteamUser"),
            register_callback: resolve!(log, "SteamAPI_RegisterCallback"),
            matchmaking: resolve!(log, "SteamAPI_SteamMatchmaking_v009"),
            utils: resolve!(log, "SteamAPI_SteamUtils_v010"),
            user: resolve!(log, "SteamAPI_SteamUser_v021"),
            friends: resolve!(log, "SteamAPI_SteamFriends_v017"),
            user_get_steam_id: resolve!(log, "SteamAPI_ISteamUser_GetSteamID"),
            get_owner: resolve!(log, "SteamAPI_ISteamMatchmaking_GetLobbyOwner"),
            get_member_count: resolve!(log, "SteamAPI_ISteamMatchmaking_GetNumLobbyMembers"),
            get_member_limit: resolve!(log, "SteamAPI_ISteamMatchmaking_GetLobbyMemberLimit"),
            get_member_by_index: resolve!(log, "SteamAPI_ISteamMatchmaking_GetLobbyMemberByIndex"),
            get_data_count: resolve!(log, "SteamAPI_ISteamMatchmaking_GetLobbyDataCount"),
            get_data_by_index: resolve!(log, "SteamAPI_ISteamMatchmaking_GetLobbyDataByIndex"),
            mm_ptr: 0,
        };
        (flat, log)
    }

    /// `SteamAPI_GetHSteamUser()`; 0 means Steam is not initialised yet (or the export is missing).
    pub(crate) fn hsteam_user(&self) -> i32 {
        match self.get_hsteam_user {
            // SAFETY: no-argument getter (steam_api_internal.h); reads Steam's global user handle.
            Some(f) => unsafe { f() },
            None => 0,
        }
    }

    /// Call an interface accessor; the pointer is returned as a number for logging (0 = null).
    pub(crate) fn call_accessor(f: Option<FnAccessor>) -> Option<usize> {
        // SAFETY: no-argument accessor (steam_api_flat.h) returning an interface pointer or null; we never dereference it.
        f.map(|f| unsafe { f() } as usize)
    }

    /// Our own Steam ID via `SteamAPI_ISteamUser_GetSteamID(user)`; `None` if unavailable.
    pub(crate) fn own_steam_id(&self, user_ptr: usize) -> Option<u64> {
        let f = self.user_get_steam_id?;
        if user_ptr == 0 {
            return None;
        }
        // SAFETY: `user_ptr` is the non-null ISteamUser* returned by SteamAPI_SteamUser_v021 (steam_api_flat.h); the getter
        // takes only that pointer and returns the uint64 Steam ID.
        Some(unsafe { f(user_ptr as *mut c_void) })
    }

    /// Remember the ISteamMatchmaking pointer used by the [`LobbyApi`] getters. Returns false if it is null.
    pub(crate) fn bind_matchmaking(&mut self, ptr: usize) -> bool {
        self.mm_ptr = ptr;
        ptr != 0
    }
}

impl LobbyApi for Flat {
    fn owner(&self, lobby: u64) -> u64 {
        match (self.get_owner, self.mm_ptr) {
            // SAFETY: non-null ISteamMatchmaking* from the accessor; `GetLobbyOwner(self, uint64)` per steam_api_flat.h.
            (Some(f), p) if p != 0 => unsafe { f(p as *mut c_void, lobby) },
            _ => 0,
        }
    }
    fn member_count(&self, lobby: u64) -> i32 {
        match (self.get_member_count, self.mm_ptr) {
            // SAFETY: as `owner`; `GetNumLobbyMembers` returns int.
            (Some(f), p) if p != 0 => unsafe { f(p as *mut c_void, lobby) },
            _ => -1,
        }
    }
    fn member_limit(&self, lobby: u64) -> i32 {
        match (self.get_member_limit, self.mm_ptr) {
            // SAFETY: as `owner`; `GetLobbyMemberLimit` returns int (0 if not in the lobby).
            (Some(f), p) if p != 0 => unsafe { f(p as *mut c_void, lobby) },
            _ => -1,
        }
    }
    fn member_by_index(&self, lobby: u64, index: i32) -> u64 {
        match (self.get_member_by_index, self.mm_ptr) {
            // SAFETY: as `owner`; `GetLobbyMemberByIndex(self, uint64, int)` returns the member's uint64 Steam ID.
            (Some(f), p) if p != 0 => unsafe { f(p as *mut c_void, lobby, index) },
            _ => 0,
        }
    }
    fn data_count(&self, lobby: u64) -> i32 {
        match (self.get_data_count, self.mm_ptr) {
            // SAFETY: as `owner`; `GetLobbyDataCount` returns int.
            (Some(f), p) if p != 0 => unsafe { f(p as *mut c_void, lobby) },
            _ => -1,
        }
    }
    fn data_by_index(&self, lobby: u64, index: i32) -> Option<(String, String)> {
        let (f, p) = (self.get_data_by_index?, self.mm_ptr);
        if p == 0 {
            return None;
        }
        let mut key = vec![0u8; KEY_BUF];
        let mut value = vec![0u8; VALUE_BUF];
        // SAFETY: as `owner`; the signature is `bool GetLobbyDataByIndex(self, uint64, int, char*, int, char*, int)`
        // (steam_api_flat.h). Both buffers are zero-filled, live for the call, and their sizes are passed; Steam NUL-terminates.
        let ok = unsafe {
            f(
                p as *mut c_void,
                lobby,
                index,
                key.as_mut_ptr() as *mut c_char,
                KEY_BUF as c_int,
                value.as_mut_ptr() as *mut c_char,
                VALUE_BUF as c_int,
            )
        };
        if !ok {
            return None;
        }
        Some((cstr_lossy(&key), cstr_lossy(&value)))
    }
}

/// Text up to the first NUL (or the whole buffer), lossy UTF-8.
pub(crate) fn cstr_lossy(buf: &[u8]) -> String {
    match CStr::from_bytes_until_nul(buf) {
        Ok(c) => c.to_string_lossy().into_owned(),
        Err(_) => String::from_utf8_lossy(buf).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cstr_lossy_stops_at_nul_and_tolerates_missing_nul() {
        assert_eq!(cstr_lossy(b"abc\0def"), "abc");
        assert_eq!(cstr_lossy(b"abc"), "abc");
        assert_eq!(cstr_lossy(b"\0"), "");
    }

    #[test]
    fn unresolved_table_returns_inert_values() {
        let f = Flat::default();
        assert_eq!(f.hsteam_user(), 0);
        assert_eq!(f.owner(1), 0);
        assert_eq!(f.member_count(1), -1);
        assert!(f.data_by_index(1, 0).is_none());
        assert!(f.own_steam_id(0).is_none());
    }
}
