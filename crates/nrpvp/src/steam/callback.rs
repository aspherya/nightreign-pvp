//! MM-1: hand-built, listen-only `CCallbackBase` objects for `SteamAPI_RegisterCallback`.
//!
//! The handler runs on whichever thread pumps Steam callbacks (the game's / Seamless Co-op's), so it does the minimum: copy the
//! struct bytes into a bounded queue and return. No logging, no Steam calls, no growth (the queue's capacity is reserved up front
//! and a full queue drops the event and counts it). A panic is caught, never unwound into Steam.
//!
//! `CCallbackBase` layout (x64), `steam_api_common.h` in the SDK bundled by steamworks-rs
//! (https://github.com/Noxime/steamworks-rs/blob/master/steamworks-sys/lib/steam/public/steam/steam_api_common.h, fetched 2026-10-12):
//! `vptr` (8 bytes) then `uint8 m_nCallbackFlags` then `int m_iCallback`; virtuals declared in the order
//! `Run(void*)`, `Run(void*, bool, SteamAPICall_t)`, `GetCallbackSizeBytes()`; no virtual destructor.
//! MSVC's slot order for the two overloaded `Run`s is NOT confirmed by any source I could read (UNKNOWN). The design does not
//! depend on it: slots 0 and 1 hold the same function, which reads only the first two register arguments (`this`, `pvParam`) and
//! so is correct for either overload on x64 (the extra `bIOFailure`/`hSteamAPICall` arguments are ignored). Slot 2 is
//! `GetCallbackSizeBytes`. Only the slot-2 assumption (size function after both `Run`s) comes from declaration order.
//! `SteamAPI_RegisterCallback` sets `k_ECallbackFlagsRegistered` (0x01) in `m_nCallbackFlags` (comment at steam_api_internal.h
//! line 162); we read that bit back as the evidence that registration took effect.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::panic::catch_unwind;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use super::FnRegisterCallback;

/// Largest payload we keep: `GameRichPresenceJoinRequested_t` = 8 + 256 bytes (isteamfriends.h, pack 8). Longer payloads are truncated.
pub(crate) const MAX_PAYLOAD: usize = 264;
/// Queue bound; the pump thread never allocates because the deque is created with this capacity.
const QUEUE_CAP: usize = 64;
/// `k_ECallbackFlagsRegistered`, steam_api_common.h.
const FLAG_REGISTERED: u8 = 0x01;

/// One raw callback delivery.
#[derive(Clone)]
pub(crate) struct RawEvent {
    pub id: i32,
    /// Bytes of `data` that are valid.
    pub len: usize,
    pub data: [u8; MAX_PAYLOAD],
}

struct Queue {
    items: Mutex<VecDeque<RawEvent>>,
    dropped: AtomicU32,
}

static QUEUE: OnceLock<Queue> = OnceLock::new();

fn queue() -> &'static Queue {
    QUEUE.get_or_init(|| Queue {
        items: Mutex::new(VecDeque::with_capacity(QUEUE_CAP)),
        dropped: AtomicU32::new(0),
    })
}

/// Take everything queued so far (our own thread only).
pub(crate) fn drain() -> Vec<RawEvent> {
    let q = queue();
    let mut g = q.items.lock().unwrap_or_else(|e| e.into_inner());
    g.drain(..).collect()
}

/// Events dropped because the queue was full.
pub(crate) fn dropped() -> u32 {
    queue().dropped.load(Ordering::Relaxed)
}

/// Instance layout: the `CCallbackBase` fields first (vptr, flags, `m_iCallback`), then our own bookkeeping.
#[repr(C)]
struct CbObject {
    vtable: *const usize,
    flags: u8,
    #[allow(dead_code)]
    icallback: i32,
    id: i32,
    size: u32,
}

/// Slot 0 and slot 1 (both `Run` overloads). Only `this` and `pvParam` are read.
unsafe extern "system" fn run(this: *mut CbObject, pv: *const c_void) {
    let _ = catch_unwind(|| {
        if this.is_null() || pv.is_null() {
            return;
        }
        // SAFETY: `this` is the leaked CbObject we registered (never freed), `pv` is Steam's callback payload of at least
        // `size` bytes for this callback id (struct sizes from the SDK headers, see probe.rs).
        let (id, size) = unsafe { ((*this).id, (*this).size as usize) };
        let n = size.min(MAX_PAYLOAD);
        let mut ev = RawEvent { id, len: n, data: [0; MAX_PAYLOAD] };
        // SAFETY: see above; `n <= MAX_PAYLOAD` and `n <= size`.
        unsafe { std::ptr::copy_nonoverlapping(pv as *const u8, ev.data.as_mut_ptr(), n) };
        let q = queue();
        // try_lock: never block Steam's thread behind our drain.
        match q.items.try_lock() {
            Ok(mut g) if g.len() < QUEUE_CAP => g.push_back(ev),
            _ => {
                q.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

/// Slot 2: `GetCallbackSizeBytes()`.
unsafe extern "system" fn get_size(this: *mut CbObject) -> i32 {
    if this.is_null() {
        return 0;
    }
    // SAFETY: our own leaked CbObject.
    unsafe { (*this).size as i32 }
}

/// Create and register one listen-only callback object per `(callback id, struct size)`. Returns `(id, registered)` where
/// `registered` is true when Steam set the registered flag on our object. Objects and the vtable are leaked on purpose:
/// Steam keeps the pointers, and the DLL never unloads.
///
/// # Safety
/// `register` must be the real `SteamAPI_RegisterCallback` export of the loaded Steam DLL.
pub(crate) unsafe fn register_all(
    register: FnRegisterCallback,
    callbacks: &[(i32, usize)],
) -> Vec<(i32, bool)> {
    queue(); // create before the first delivery can arrive
    let vtable: &'static [usize; 3] =
        Box::leak(Box::new([run as *const () as usize, run as *const () as usize, get_size as *const () as usize]));
    let mut out = Vec::new();
    for &(id, size) in callbacks {
        let obj: &'static mut CbObject = Box::leak(Box::new(CbObject {
            vtable: vtable.as_ptr(),
            flags: 0,
            icallback: 0,
            id,
            size: size as u32,
        }));
        let p = obj as *mut CbObject;
        // SAFETY: `p` points to a leaked, properly laid out CCallbackBase-compatible object (see module doc).
        unsafe { register(p as *mut c_void, id) };
        // SAFETY: same object; Steam may have written the flags on this thread during the call.
        let flags = unsafe { std::ptr::read_volatile(std::ptr::addr_of!((*p).flags)) };
        out.push((id, flags & FLAG_REGISTERED != 0));
    }
    out
}
