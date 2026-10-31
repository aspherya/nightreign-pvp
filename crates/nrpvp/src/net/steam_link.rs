//! NET-1: [`PeerLink`] over `ISteamNetworkingMessages` of the Steam DLL the game already loaded (NETWORKING.md §3 T1).
//!
//! Rules (S4 §2.2/§2.4): never initialise, pump or shut Steam down (we resolve flat exports by name, like `steam/mod.rs`); never call
//! `CloseSessionWithUser` (it would cut Seamless Co-op's traffic with the same peer); one private channel number; polling only, no
//! callbacks (our heartbeat to every roster peer opens the session in both directions, a send accepts the peer's session).
//!
//! Layouts are from the Steamworks SDK headers bundled by steamworks-rs (`steamnetworkingtypes.h`, `steam_api_flat.h`), checked by
//! offset arithmetic: `SteamNetworkingIdentity` = type i32, size i32, 128-byte union (the Steam id at +8), 136 bytes in all;
//! `SteamNetworkingMessage_t`: `m_pData` +0, `m_cbSize` +8, `m_identityPeer` +16 (so the sender's Steam id is at +24).

use std::ffi::c_void;

use super::link::{PeerLink, SteamId};
use super::msg::{self, Msg};
use crate::steam::export;

/// Our private channel number (S4 §2.2: pick an unusual one and log it).
pub(crate) const CHANNEL: i32 = 0x4E52;
/// `k_nSteamNetworkingSend_Reliable` (8) | `k_nSteamNetworkingSend_AutoRestartBrokenSession` (32): a session that broke (peer started late,
/// network blip) restarts on the next send, so we never need `CloseSessionWithUser` (which would also cut Seamless Co-op's traffic).
const SEND_RELIABLE: i32 = 8 | 32;
/// `k_EResultOK`.
const RESULT_OK: i32 = 1;
const BATCH: usize = 16;

type FnAccessor = unsafe extern "system" fn() -> *mut c_void;
type FnSend =
    unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void, u32, i32, i32) -> i32;
type FnRecv = unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void, i32) -> i32;
type FnSetId = unsafe extern "system" fn(*mut c_void, u64);
type FnRelease = unsafe extern "system" fn(*mut c_void);
type FnUserId = unsafe extern "system" fn(*mut c_void) -> u64;

pub(crate) struct SteamLink {
    iface: usize,
    send: FnSend,
    recv: FnRecv,
    set_id: FnSetId,
    release: FnRelease,
    own: SteamId,
    dropped: u64,
}

macro_rules! resolve {
    ($name:literal) => {{
        let a = export(concat!($name, "\0")).ok_or(concat!("missing export ", $name))?;
        // SAFETY: the export has the signature documented in steam_api_flat.h for this name (see the aliases above); the
        // address came from GetProcAddress for exactly that name.
        unsafe { std::mem::transmute::<usize, _>(a) }
    }};
}

impl SteamLink {
    /// Resolve the exports and the interface. Call once Steam is initialised (`SteamAPI_GetHSteamUser() != 0`).
    pub(crate) fn open() -> Result<SteamLink, String> {
        let accessor: FnAccessor = resolve!("SteamAPI_SteamNetworkingMessages_SteamAPI_v002");
        let send: FnSend = resolve!("SteamAPI_ISteamNetworkingMessages_SendMessageToUser");
        let recv: FnRecv = resolve!("SteamAPI_ISteamNetworkingMessages_ReceiveMessagesOnChannel");
        let set_id: FnSetId = resolve!("SteamAPI_SteamNetworkingIdentity_SetSteamID64");
        let release: FnRelease = resolve!("SteamAPI_SteamNetworkingMessage_t_Release");
        let user_acc: FnAccessor = resolve!("SteamAPI_SteamUser_v021");
        let user_id: FnUserId = resolve!("SteamAPI_ISteamUser_GetSteamID");
        // SAFETY: no-argument accessors returning interface pointers or null; ISteamUser::GetSteamID takes that pointer.
        let (iface, own) = unsafe {
            let user = user_acc();
            (
                accessor() as usize,
                if user.is_null() { 0 } else { user_id(user) },
            )
        };
        if iface == 0 {
            return Err("SteamNetworkingMessages accessor returned NULL".into());
        }
        Ok(SteamLink {
            iface,
            send,
            recv,
            set_id,
            release,
            own,
            dropped: 0,
        })
    }
}

impl PeerLink for SteamLink {
    fn own_id(&self) -> SteamId {
        self.own
    }

    fn send(&mut self, to: SteamId, m: &Msg) -> Result<(), String> {
        let bytes = msg::encode(m);
        let mut identity = [0u64; 17]; // 136 bytes, 8-aligned
                                       // SAFETY: `identity` is a zeroed buffer of sizeof(SteamNetworkingIdentity) (136) and lives for both calls;
                                       // `iface` is the non-null ISteamNetworkingMessages*; the data pointer and length describe `bytes`, which outlives the call.
        let r = unsafe {
            (self.set_id)(identity.as_mut_ptr().cast(), to);
            (self.send)(
                self.iface as *mut c_void,
                identity.as_ptr().cast(),
                bytes.as_ptr().cast(),
                bytes.len() as u32,
                SEND_RELIABLE,
                CHANNEL,
            )
        };
        if r == RESULT_OK {
            Ok(())
        } else {
            Err(format!("EResult {r}"))
        }
    }

    fn poll(&mut self, out: &mut Vec<(SteamId, Msg)>) {
        loop {
            let mut ptrs = [std::ptr::null_mut::<c_void>(); BATCH];
            // SAFETY: `iface` is the interface pointer; `ptrs` has room for BATCH message pointers, which Steam fills.
            let n = unsafe {
                (self.recv)(
                    self.iface as *mut c_void,
                    CHANNEL,
                    ptrs.as_mut_ptr(),
                    BATCH as i32,
                )
            };
            if n <= 0 {
                return;
            }
            for &p in ptrs.iter().take((n as usize).min(BATCH)) {
                if p.is_null() {
                    continue;
                }
                // SAFETY: `p` is a SteamNetworkingMessage_t* returned by Steam, valid until Release. Offsets per the module doc:
                // pData +0 (pointer), cbSize +8 (i32), sender Steam id +24 (u64). The bytes are copied before Release, and
                // only when cbSize is within the protocol cap, so no length from the wire is trusted for an allocation.
                unsafe {
                    let base = p as *const u8;
                    let data = *(base as *const *const u8);
                    let size = *(base.add(8) as *const i32);
                    let from = *(base.add(24) as *const u64);
                    if !data.is_null() && size > 0 && (size as usize) <= msg::MAX_LEN {
                        let slice = std::slice::from_raw_parts(data, size as usize);
                        match msg::decode(slice) {
                            Ok(m) => out.push((from, m)),
                            Err(_) => self.dropped += 1,
                        }
                    } else {
                        self.dropped += 1;
                    }
                    (self.release)(p);
                }
            }
            if (n as usize) < BATCH {
                return;
            }
        }
    }

    fn dropped(&self) -> u64 {
        self.dropped
    }
}
