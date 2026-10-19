//! MM-3c: what creates Seamless Co-op's lobby A? A dev-only call-stack tracer (`mm_lobby_trace = true`).
//!
//! Interop only: no byte of `nrsc.dll`, `steam_api64.dll`, `steamclient64.dll` or the game is written. The tracer arms up to four
//! hardware EXECUTE breakpoints (debug registers DR0..DR3) on every thread of the process and catches the resulting single-step
//! exceptions in a vectored exception handler (VEH). Each hit is logged with the thread id, the first four argument registers and a
//! call stack of `module+offset` frames. Every log line starts with `mm_lobby_trace:`.
//!
//! Armed addresses (see `run`):
//! - DR0 the flat export `SteamAPI_ISteamMatchmaking_CreateLobby` (steam_api64.dll; card MM-3c step 1, `steam_api_flat.h`).
//! - DR1 the code that export jumps to through the `ISteamMatchmaking` vtable. The slot is NOT hard-coded: the SDK headers
//!   (`isteammatchmaking.h`) were not found on this machine (steamworks-rs is not a dependency, nothing under `~/.cargo/registry`),
//!   so the slot index is decoded at run time from the first bytes of the flat export (`mov rax,[rcx]; jmp [rax+disp]`, see
//!   [`decode_vtable_disp`]) and the target is read from the live object. If the export is not that shape, DR1 stays unused and the
//!   first 24 bytes of the export are logged so the shape can be read by hand. A direct vtable call (what the game's C++ interface
//!   code does) reaches DR1 and never DR0, which is why both are armed.
//! - DR2 / DR3 the same for `SteamAPI_ISteamFriends_ActivateGameOverlayInviteDialog` (`ISteamFriends`).
//!
//! The VEH is allocation-free and lock-free: it matches the faulting `Rip` against the armed addresses, bumps an atomic hit counter,
//! copies the CONTEXT and up to 8 KiB of the stack into a preallocated slot of a static ring (claimed by compare-exchange), sets RF in
//! EFlags and resumes. Everything else (unwinding with `RtlLookupFunctionEntry` + `RtlVirtualUnwind`, module lookup, formatting,
//! logging) happens on the tracer thread from that copy. For the unwind the copied stack is placed in a scratch buffer and the
//! registers that point into the original stack are rebased into the buffer, so no lock-taking unwinder code ever runs inside the
//! exception handler.
//!
//! Safety of arming: a thread is suspended only for `GetThreadContext` / `SetThreadContext` and nothing in that window allocates or
//! logs (a suspended thread may hold the heap lock). After 3 hits per address that address is disarmed on every thread; after all
//! are disarmed, or after 10 minutes, every debug register is cleared on every thread and the handler is removed.

use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HMODULE, STATUS_SINGLE_STEP};
use windows::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, RemoveVectoredExceptionHandler, RtlLookupFunctionEntry, RtlVirtualUnwind, CONTEXT,
    CONTEXT_DEBUG_REGISTERS_AMD64, EXCEPTION_POINTERS, UNW_FLAG_NHANDLER,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::System::Threading::{
    GetCurrentThreadId, GetCurrentThreadStackLimits, OpenThread, THREAD_GET_CONTEXT, THREAD_SET_CONTEXT,
    THREAD_SUSPEND_RESUME,
};

use super::Flat;

/// Number of debug registers with an address (DR0..DR3).
pub(crate) const TARGETS: usize = 4;
/// Hits recorded per address before it is disarmed.
const MAX_HITS: u32 = 3;
/// The tracer disarms itself after this long.
const TRACE_DURATION: Duration = Duration::from_secs(600);
/// New threads are armed at least this often.
const APPLY_EVERY: Duration = Duration::from_secs(2);
/// Drain period of the ring.
const TICK: Duration = Duration::from_millis(100);
/// How long we wait for the game to initialise Steam.
const WAIT_TOTAL: Duration = Duration::from_secs(300);
const WAIT_STEP: Duration = Duration::from_millis(500);
const RING_SLOTS: usize = 16;
const STACK_QWORDS: usize = 1024;
const STACK_BYTES: usize = STACK_QWORDS * 8;
/// Zero padding behind the copied stack in the unwind scratch buffer, so a frame that reaches past the copy reads zeros, not
/// whatever lies behind the buffer (128 KiB).
const PAD_QWORDS: usize = 16 * 1024;
const MAX_FRAMES: usize = 32;
/// EFlags resume flag: suppresses the instruction breakpoint for exactly one instruction (Intel SDM vol. 3, 17.3.1.1).
pub(crate) const EFLAGS_RF: u32 = 0x1_0000;
pub(crate) const EXCEPTION_CONTINUE_EXECUTION: i32 = -1;
pub(crate) const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

const LABELS: [&str; TARGETS] = [
    "flat ISteamMatchmaking::CreateLobby",
    "vtable ISteamMatchmaking::CreateLobby",
    "flat ISteamFriends::ActivateGameOverlayInviteDialog",
    "vtable ISteamFriends::ActivateGameOverlayInviteDialog",
];

// Shared with the VEH: plain atomics and a static ring, nothing that allocates or locks.
static ADDRS: [AtomicUsize; TARGETS] = [AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0)];
static HITS: [AtomicU32; TARGETS] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
static OWN_TID: AtomicU32 = AtomicU32::new(0);
static SEQ: AtomicU32 = AtomicU32::new(0);
static DROPPED: AtomicU32 = AtomicU32::new(0);
static STATE: [AtomicU32; RING_SLOTS] = [const { AtomicU32::new(0) }; RING_SLOTS];

const SLOT_FREE: u32 = 0;
const SLOT_WRITING: u32 = 1;
const SLOT_READY: u32 = 2;

/// One captured hit. Plain data so a zeroed value is valid.
#[repr(C, align(16))]
struct SlotData {
    seq: u32,
    tid: u32,
    target: u32,
    hit_no: u32,
    /// Valid bytes in `stack` (the stack from the faulting Rsp upwards).
    stack_len: usize,
    ctx: CONTEXT,
    stack: [u64; STACK_QWORDS],
}

struct Ring(UnsafeCell<[SlotData; RING_SLOTS]>);
// SAFETY: a slot is written only by the thread that moved its STATE FREE -> WRITING (compare-exchange) and read only by the single
// tracer thread after it observed READY (acquire); the tracer sets it back to FREE after it is done. No two threads touch a slot
// at the same time.
unsafe impl Sync for Ring {}
// SAFETY: SlotData is integers, arrays of integers and CONTEXT (integers and a union of integer/float arrays): all-zero is valid.
static RING: Ring = Ring(UnsafeCell::new(unsafe { std::mem::zeroed() }));

/// Called by the Steam thread (`probe.rs`) so our own queue-lobby `CreateLobby` hits can be labelled.
pub(crate) fn note_steam_thread() {
    // SAFETY: GetCurrentThreadId has no preconditions.
    OWN_TID.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------------------------------------------------------
// Pure parts (unit tested)
// ---------------------------------------------------------------------------------------------------------------------------

/// DR7 for execute breakpoints: local-enable bit `2*i` for every set bit `i` of `mask`; RW and LEN fields stay 00 (execute, 1 byte)
/// (Intel SDM vol. 3, 17.2.4).
pub(crate) fn dr7_for(mask: u8) -> u64 {
    (0..TARGETS as u32).filter(|i| mask & (1 << i) != 0).fold(0u64, |acc, i| acc | (1u64 << (2 * i)))
}

/// The mask after removing every slot that reached `max` hits.
pub(crate) fn mask_after_hits(mask: u8, hits: &[u32; TARGETS], max: u32) -> u8 {
    let mut m = mask;
    for (i, h) in hits.iter().enumerate() {
        if *h >= max {
            m &= !(1 << i);
        }
    }
    m
}

/// Whether hit number `hit_no` (1-based) is still recorded.
pub(crate) fn should_record(hit_no: u32, max: u32) -> bool {
    hit_no <= max
}

/// Whether the tracer is done.
pub(crate) fn should_finish(mask: u8, elapsed: Duration) -> bool {
    mask == 0 || elapsed >= TRACE_DURATION
}

/// Decode the vtable displacement out of a flat-API thunk: `mov rax,[rcx]` (48 8B 01) followed by `jmp qword ptr [rax+disp]` as
/// `48 FF 60 d8`, `FF 60 d8`, `48 FF A0 d32` or `FF A0 d32`. `None` for anything else (the caller logs the raw bytes).
pub(crate) fn decode_vtable_disp(b: &[u8]) -> Option<u32> {
    if b.len() < 3 || b[..3] != [0x48, 0x8B, 0x01] {
        return None;
    }
    let rest = &b[3..];
    let rest = if rest.first() == Some(&0x48) { &rest[1..] } else { rest };
    match rest {
        [0xFF, 0x60, d, ..] => Some(u32::from(*d)),
        [0xFF, 0xA0, a, b2, c, d, ..] => Some(u32::from_le_bytes([*a, *b2, *c, *d])),
        _ => None,
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}

/// File name part of a module path.
pub(crate) fn short_module_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

pub(crate) fn thread_label(tid: u32, own: u32) -> &'static str {
    if own != 0 && tid == own {
        "OUR Steam thread (queue lobby)"
    } else {
        "game / Seamless Co-op thread"
    }
}

pub(crate) fn format_frame(i: usize, addr: u64, module: Option<(&str, u64)>) -> String {
    match module {
        Some((name, off)) => format!("mm_lobby_trace:   #{i} {name}+0x{off:X} (0x{addr:X})"),
        None => format!("mm_lobby_trace:   #{i} 0x{addr:X} (no module)"),
    }
}

/// What the first arguments mean for target `t` (x64 calling convention: rcx = this).
pub(crate) fn arg_note(t: usize, rdx: u64, r8: u64) -> String {
    match t {
        0 | 1 => format!(
            "this, ELobbyType={rdx} ({}), cMaxMembers={r8}",
            match rdx {
                0 => "Private",
                1 => "FriendsOnly",
                2 => "Public",
                3 => "Invisible",
                4 => "PrivateUnique",
                _ => "?",
            }
        ),
        _ => format!("this, steamIDLobby={rdx} (0x{rdx:X})"),
    }
}

pub(crate) struct HitInfo {
    pub target: usize,
    pub hit_no: u32,
    pub tid: u32,
    pub own_tid: u32,
    pub rip: u64,
    pub regs: [u64; 4],
}

/// The first line of a hit (frames follow, see [`format_frame`]).
pub(crate) fn format_hit(h: &HitInfo) -> String {
    format!(
        "mm_lobby_trace: HIT {}/{MAX_HITS} {} at 0x{:X} thread={} ({}) rcx=0x{:X} rdx=0x{:X} r8=0x{:X} r9=0x{:X} [{}]",
        h.hit_no,
        LABELS.get(h.target).copied().unwrap_or("?"),
        h.rip,
        h.tid,
        thread_label(h.tid, h.own_tid),
        h.regs[0],
        h.regs[1],
        h.regs[2],
        h.regs[3],
        arg_note(h.target, h.regs[1], h.regs[2])
    )
}

/// Map `v` into the scratch copy if it points into the original stack range `[lo, lo + len)`.
pub(crate) fn rebase(v: u64, lo: u64, len: u64, copy_base: u64) -> u64 {
    if v >= lo && v < lo.saturating_add(len) {
        copy_base + (v - lo)
    } else {
        v
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// The vectored exception handler
// ---------------------------------------------------------------------------------------------------------------------------

/// MM-3c. Runs on the faulting thread with an arbitrary lock state: no allocation, no locks, no logging. See the module doc.
unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS; each pointer is null-checked before use (MM-3c, no game offsets involved).
    unsafe {
        if info.is_null() {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let rec = (*info).ExceptionRecord;
        let ctx = (*info).ContextRecord;
        if rec.is_null() || ctx.is_null() || (*rec).ExceptionCode != STATUS_SINGLE_STEP {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let rip = (*ctx).Rip as usize;
        let Some(target) = (0..TARGETS).find(|i| rip != 0 && ADDRS[*i].load(Ordering::Relaxed) == rip) else {
            return EXCEPTION_CONTINUE_SEARCH;
        };
        let hit_no = HITS[target].fetch_add(1, Ordering::Relaxed).saturating_add(1);
        if should_record(hit_no, MAX_HITS) {
            record(target, hit_no, &*ctx);
        }
        (*ctx).EFlags |= EFLAGS_RF;
    }
    EXCEPTION_CONTINUE_EXECUTION
}

/// Copy one hit into a free ring slot (or count it as dropped).
///
/// # Safety
/// Must run on the thread that owns `ctx` (the stack bounds are read from the current thread).
unsafe fn record(target: usize, hit_no: u32, ctx: &CONTEXT) {
    #[allow(clippy::needless_range_loop)] // the index selects both STATE[i] and the ring slot i
    for i in 0..RING_SLOTS {
        if STATE[i].compare_exchange(SLOT_FREE, SLOT_WRITING, Ordering::Acquire, Ordering::Relaxed).is_err() {
            continue;
        }
        // SAFETY: we own slot `i` (state WRITING); see `impl Sync for Ring`.
        let slot = unsafe { &mut *RING.0.get().cast::<SlotData>().add(i) };
        slot.seq = SEQ.fetch_add(1, Ordering::Relaxed);
        slot.tid = unsafe { GetCurrentThreadId() };
        slot.target = target as u32;
        slot.hit_no = hit_no;
        slot.ctx = *ctx;
        let (mut lo, mut hi) = (0usize, 0usize);
        // SAFETY: out-pointers are valid locals; the call only reads the current thread's TEB.
        unsafe { GetCurrentThreadStackLimits(&mut lo, &mut hi) };
        let rsp = ctx.Rsp as usize;
        // Only bytes between Rsp and the stack's high limit are copied: committed and in use (null/range checked).
        let n = if rsp >= lo && rsp < hi { (hi - rsp).min(STACK_BYTES) } else { 0 };
        slot.stack_len = n;
        if n != 0 {
            // SAFETY: `[rsp, rsp+n)` lies inside the current thread's stack (checked above); the destination holds STACK_BYTES.
            unsafe { std::ptr::copy_nonoverlapping(rsp as *const u8, slot.stack.as_mut_ptr().cast::<u8>(), n) };
        }
        STATE[i].store(SLOT_READY, Ordering::Release);
        return;
    }
    DROPPED.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------------------------------------------------------
// Arming (tracer thread)
// ---------------------------------------------------------------------------------------------------------------------------

// Raw kernel32 declarations: no `windows::core::Error` (which may allocate / touch COM) inside the suspend window.
#[link(name = "kernel32")]
extern "system" {
    fn SuspendThread(h: HANDLE) -> u32;
    fn ResumeThread(h: HANDLE) -> u32;
    fn GetThreadContext(h: HANDLE, c: *mut CONTEXT) -> i32;
    fn SetThreadContext(h: HANDLE, c: *const CONTEXT) -> i32;
}

#[repr(C, align(16))]
struct AlignedContext(CONTEXT);

/// Write `addrs` / `mask` into the debug registers of thread `tid`. No allocation while the thread is suspended.
fn set_debug_regs(tid: u32, addrs: &[u64; TARGETS], mask: u8) -> Result<(), &'static str> {
    // SAFETY: OpenThread has no memory preconditions; the returned handle is closed below.
    let h = match unsafe { OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_SET_CONTEXT, false, tid) } {
        Ok(h) => h,
        Err(_) => return Err("OpenThread failed"),
    };
    let mut res: Result<(), &'static str> = Ok(());
    // SAFETY: valid thread handle with the three access rights above; `c` is a 16-byte-aligned CONTEXT as GetThreadContext
    // requires. Between SuspendThread and ResumeThread only these raw calls run (no allocation, no logging, no panics).
    unsafe {
        if SuspendThread(h) == u32::MAX {
            res = Err("SuspendThread failed");
        } else {
            let mut c = AlignedContext(std::mem::zeroed());
            c.0.ContextFlags = CONTEXT_DEBUG_REGISTERS_AMD64;
            if GetThreadContext(h, &mut c.0) == 0 {
                res = Err("GetThreadContext failed");
            } else {
                c.0.Dr0 = if mask & 1 != 0 { addrs[0] } else { 0 };
                c.0.Dr1 = if mask & 2 != 0 { addrs[1] } else { 0 };
                c.0.Dr2 = if mask & 4 != 0 { addrs[2] } else { 0 };
                c.0.Dr3 = if mask & 8 != 0 { addrs[3] } else { 0 };
                c.0.Dr6 = 0;
                c.0.Dr7 = dr7_for(mask);
                if SetThreadContext(h, &c.0) == 0 {
                    res = Err("SetThreadContext failed");
                }
            }
            ResumeThread(h);
        }
        let _ = CloseHandle(h);
    }
    res
}

/// Ids of all threads of this process into `out` (cleared first). False if the snapshot failed.
fn thread_ids(pid: u32, out: &mut Vec<u32>) -> bool {
    out.clear();
    // SAFETY: Toolhelp snapshot of threads; the entry struct is initialised with its size as the API requires.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else { return false };
        let mut e = THREADENTRY32 { dwSize: size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut ok = Thread32First(snap, &mut e).is_ok();
        while ok {
            if e.th32OwnerProcessID == pid {
                out.push(e.th32ThreadID);
            }
            ok = Thread32Next(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    true
}

/// Apply `mask` to every thread except `own`. `applied` remembers the mask each thread already has (so only new threads and
/// changed masks cost a suspend). With `force` every thread is written (disarm). Returns `(written, failed, first error)`.
pub(crate) fn apply_all(
    pid: u32,
    own: u32,
    addrs: &[u64; TARGETS],
    mask: u8,
    force: bool,
    applied: &mut HashMap<u32, u8>,
    scratch: &mut Vec<u32>,
) -> (usize, usize, Option<(u32, &'static str)>) {
    if !thread_ids(pid, scratch) {
        return (0, 0, Some((0, "thread snapshot failed")));
    }
    applied.retain(|t, _| scratch.contains(t));
    let (mut written, mut failed, mut first) = (0usize, 0usize, None);
    for &tid in scratch.iter() {
        if tid == own || (!force && applied.get(&tid) == Some(&mask)) {
            continue;
        }
        match set_debug_regs(tid, addrs, mask) {
            Ok(()) => {
                written += 1;
                applied.insert(tid, mask);
            }
            Err(e) => {
                failed += 1;
                first.get_or_insert((tid, e));
            }
        }
    }
    (written, failed, first)
}

// ---------------------------------------------------------------------------------------------------------------------------
// Logging side
// ---------------------------------------------------------------------------------------------------------------------------

/// `(file name, offset in module)` of an address.
pub(crate) fn module_of(addr: u64) -> Option<(String, u64)> {
    let mut h = HMODULE::default();
    // SAFETY: FROM_ADDRESS treats the pointer argument as an address inside a module and does not dereference it; UNCHANGED_REFCOUNT
    // leaves the module's reference count alone.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(addr as *const u16),
            &mut h,
        )
        .ok()?;
        let mut buf = [0u16; 520];
        let n = GetModuleFileNameW(Some(h), &mut buf) as usize;
        if n == 0 || n > buf.len() {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..n]);
        Some((short_module_name(&path).to_string(), addr.wrapping_sub(h.0 as u64)))
    }
}

/// Unwind the captured stack: `[Rip, return addresses...]`, at most [`MAX_FRAMES`]. Runs on the tracer thread.
fn unwind(d: &SlotData) -> Vec<u64> {
    let mut buf = vec![0u64; STACK_QWORDS + PAD_QWORDS];
    buf[..STACK_QWORDS].copy_from_slice(&d.stack);
    let copy_base = buf.as_ptr() as u64;
    let copy_end = copy_base + d.stack_len as u64;
    let (orig_lo, len) = (d.ctx.Rsp, d.stack_len as u64);
    let mut c = AlignedContext(d.ctx);
    c.0.Rsp = copy_base;
    let mut out = vec![c.0.Rip];
    for _ in 1..MAX_FRAMES {
        // Registers that point into the original stack must point into the copy for the frame-register based unwinds.
        macro_rules! rb {
            ($($r:ident),*) => {$( c.0.$r = rebase(c.0.$r, orig_lo, len, copy_base); )*};
        }
        rb!(Rax, Rcx, Rdx, Rbx, Rbp, Rsi, Rdi, R8, R9, R10, R11, R12, R13, R14, R15);
        let pc = c.0.Rip;
        let mut image_base = 0u64;
        // SAFETY: RtlLookupFunctionEntry only reads PE exception tables of loaded images; a null result means a leaf function.
        let fe = unsafe { RtlLookupFunctionEntry(pc, &mut image_base, None) };
        if fe.is_null() {
            // Leaf frame: the return address is at [Rsp].
            if c.0.Rsp < copy_base || c.0.Rsp + 8 > copy_end {
                break;
            }
            // SAFETY: Rsp..Rsp+8 is inside the copied stack (checked above).
            c.0.Rip = unsafe { *(c.0.Rsp as *const u64) };
            c.0.Rsp += 8;
        } else {
            let mut handler_data: *mut c_void = std::ptr::null_mut();
            let mut establisher = 0u64;
            // SAFETY: `fe` came from RtlLookupFunctionEntry for `pc`; `c` is a full 16-byte-aligned CONTEXT whose Rsp and
            // frame registers point into `buf`, which has PAD_QWORDS of zero padding behind the copied bytes. The unwinder
            // reads the (static) unwind info of the image and the stack copy only.
            unsafe {
                RtlVirtualUnwind(UNW_FLAG_NHANDLER, image_base, pc, fe, &mut c.0, &mut handler_data, &mut establisher, None);
            }
        }
        if c.0.Rip == 0 || c.0.Rsp < copy_base || c.0.Rsp > copy_end {
            break;
        }
        out.push(c.0.Rip);
    }
    out
}

/// Drain READY slots in capture order and log them.
fn drain_ring() -> usize {
    let mut ready: Vec<(u32, usize)> = Vec::new();
    for (i, st) in STATE.iter().enumerate() {
        if st.load(Ordering::Acquire) == SLOT_READY {
            // SAFETY: READY slot, observed with acquire: the writer is done and nobody else reads it (single tracer thread).
            let seq = unsafe { (*RING.0.get().cast::<SlotData>().add(i)).seq };
            ready.push((seq, i));
        }
    }
    ready.sort_unstable();
    for (_, i) in &ready {
        // SAFETY: as above; the slot is released to the writers only by the store below.
        let d = unsafe { &*RING.0.get().cast::<SlotData>().add(*i) };
        let info = HitInfo {
            target: d.target as usize,
            hit_no: d.hit_no,
            tid: d.tid,
            own_tid: OWN_TID.load(Ordering::Relaxed),
            rip: d.ctx.Rip,
            regs: [d.ctx.Rcx, d.ctx.Rdx, d.ctx.R8, d.ctx.R9],
        };
        tracing::info!("{}", format_hit(&info));
        let frames = unwind(d);
        for (n, a) in frames.iter().enumerate() {
            let m = module_of(*a);
            tracing::info!("{}", format_frame(n, *a, m.as_ref().map(|(s, o)| (s.as_str(), *o))));
        }
        if d.stack_len == 0 {
            tracing::warn!("mm_lobby_trace: no stack bytes were captured for this hit (Rsp outside the thread's stack limits)");
        }
        STATE[*i].store(SLOT_FREE, Ordering::Release);
    }
    ready.len()
}

// ---------------------------------------------------------------------------------------------------------------------------
// The tracer thread
// ---------------------------------------------------------------------------------------------------------------------------

/// Start the tracer thread. Call only in developer mode with `mm_lobby_trace = true`.
pub(crate) fn spawn() {
    let r = std::thread::Builder::new().name("nrpvp-lobtrace".into()).spawn(|| {
        if catch_unwind(AssertUnwindSafe(run)).is_err() {
            tracing::error!("mm_lobby_trace: tracer thread panicked");
        }
    });
    if r.is_err() {
        tracing::warn!("mm_lobby_trace: could not spawn the tracer thread");
    }
}

struct ExportProbe {
    flat: Option<usize>,
    vtable_target: Option<usize>,
}

/// Resolve one flat export and, from its first bytes and the live interface object, the code behind its vtable slot.
fn probe_export(name: &'static str, iface: usize) -> ExportProbe {
    let flat = super::export(name);
    let Some(addr) = flat else {
        tracing::warn!("mm_lobby_trace: export {} missing in steam_api64.dll", name.trim_end_matches('\0'));
        return ExportProbe { flat: None, vtable_target: None };
    };
    let bytes = crate::game::mem::read::<[u8; 24]>(addr);
    let n = name.trim_end_matches('\0');
    match &bytes {
        Some(b) => tracing::info!("mm_lobby_trace: {n} @ 0x{addr:X}, first 24 bytes: {}", hex(b)),
        None => tracing::warn!("mm_lobby_trace: {n} @ 0x{addr:X}: first bytes unreadable"),
    }
    let mut vtable_target = None;
    match bytes.as_ref().and_then(|b| decode_vtable_disp(b)) {
        Some(disp) if iface != 0 => {
            // [iface] = vtable pointer, [vtable + disp] = the function (SDK vtable layout: one 8-byte pointer per virtual method).
            let target = crate::game::mem::read::<u64>(iface)
                .and_then(|vt| crate::game::mem::read::<u64>(vt as usize + disp as usize))
                .filter(|t| *t != 0);
            match target {
                Some(t) if t as usize != addr => {
                    tracing::info!(
                        "mm_lobby_trace: {n}: thunk jumps through vtable displacement 0x{disp:X} (slot {}, 8 bytes each); live target 0x{t:X}",
                        disp / 8
                    );
                    vtable_target = Some(t as usize);
                }
                _ => tracing::warn!("mm_lobby_trace: {n}: could not read the vtable target (disp 0x{disp:X}); only the flat export is armed"),
            }
        }
        Some(_) => tracing::warn!("mm_lobby_trace: {n}: interface pointer is null; only the flat export is armed"),
        None => tracing::warn!("mm_lobby_trace: {n}: not the 'mov rax,[rcx]; jmp [rax+disp]' shape; read the bytes above by hand; only the flat export is armed"),
    }
    ExportProbe { flat, vtable_target }
}

fn run() {
    tracing::info!(
        "mm_lobby_trace: enabled (dev only; hardware execute breakpoints on every thread, {MAX_HITS} hits per address or {} min, no code is patched)",
        TRACE_DURATION.as_secs() / 60
    );
    let start = Instant::now();
    let (flat, _resolved) = Flat::resolve();
    loop {
        if super::steam_dll_loaded() && flat.hsteam_user() != 0 {
            tracing::info!("mm_lobby_trace: Steam is initialised after {:.1} s", start.elapsed().as_secs_f32());
            break;
        }
        if start.elapsed() >= WAIT_TOTAL {
            tracing::warn!("mm_lobby_trace: gave up after {} s waiting for Steam; nothing armed", WAIT_TOTAL.as_secs());
            return;
        }
        std::thread::sleep(WAIT_STEP);
    }
    let mm = Flat::call_accessor(flat.matchmaking).unwrap_or(0);
    let fr = Flat::call_accessor(flat.friends).unwrap_or(0);
    let create = probe_export("SteamAPI_ISteamMatchmaking_CreateLobby\0", mm);
    let invite = probe_export("SteamAPI_ISteamFriends_ActivateGameOverlayInviteDialog\0", fr);
    let slots = [create.flat, create.vtable_target, invite.flat, invite.vtable_target];
    let mut mask = 0u8;
    let mut addrs = [0u64; TARGETS];
    for (i, a) in slots.iter().enumerate() {
        if let Some(a) = a {
            addrs[i] = *a as u64;
            ADDRS[i].store(*a, Ordering::Relaxed);
            mask |= 1 << i;
            tracing::info!("mm_lobby_trace: DR{i} = 0x{a:X} ({})", LABELS[i]);
        }
    }
    if mask == 0 {
        tracing::warn!("mm_lobby_trace: no address to arm; stopping");
        return;
    }
    // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature; first = 1 puts it before the others so a hit is handled
    // before another handler sees a single-step it does not expect. It returns CONTINUE_SEARCH for everything but our hits.
    let handle = unsafe { AddVectoredExceptionHandler(1, Some(veh)) };
    if handle.is_null() {
        tracing::warn!("mm_lobby_trace: AddVectoredExceptionHandler failed; nothing armed");
        return;
    }
    // SAFETY: GetCurrentThreadId has no preconditions.
    let own = unsafe { GetCurrentThreadId() };
    let pid = std::process::id();
    let mut applied: HashMap<u32, u8> = HashMap::new();
    let mut scratch: Vec<u32> = Vec::with_capacity(2048);
    let t0 = Instant::now();
    let mut next_apply = t0;
    let mut mask_changed = true;
    loop {
        let now = Instant::now();
        if now >= next_apply || mask_changed {
            let (written, failed, first) = apply_all(pid, own, &addrs, mask, false, &mut applied, &mut scratch);
            if written > 0 || failed > 0 {
                tracing::info!(
                    "mm_lobby_trace: armed {written} thread(s) (mask 0x{mask:X}, {} armed in total), {failed} failed{}",
                    applied.len(),
                    first.map_or(String::new(), |(t, e)| format!(" (first: thread {t}: {e})"))
                );
            }
            next_apply = now + APPLY_EVERY;
            mask_changed = false;
        }
        std::thread::sleep(TICK);
        drain_ring();
        let hits = [
            HITS[0].load(Ordering::Relaxed),
            HITS[1].load(Ordering::Relaxed),
            HITS[2].load(Ordering::Relaxed),
            HITS[3].load(Ordering::Relaxed),
        ];
        let new_mask = mask_after_hits(mask, &hits, MAX_HITS);
        if new_mask != mask {
            tracing::info!("mm_lobby_trace: DR mask 0x{mask:X} -> 0x{new_mask:X} ({MAX_HITS} hits reached on the removed address(es); hits {hits:?})");
            mask = new_mask;
            mask_changed = true;
        }
        if should_finish(mask, t0.elapsed()) {
            break;
        }
    }
    // Disarm: clear every debug register on every thread, then let in-flight hits drain and remove the handler.
    let (written, failed, first) = apply_all(pid, own, &addrs, 0, true, &mut applied, &mut scratch);
    tracing::info!(
        "mm_lobby_trace: disarmed after {:.0} s: debug registers cleared on {written} thread(s), {failed} failed{}; hits per address {:?}",
        t0.elapsed().as_secs_f32(),
        first.map_or(String::new(), |(t, e)| format!(" (first: thread {t}: {e})")),
        [HITS[0].load(Ordering::Relaxed), HITS[1].load(Ordering::Relaxed), HITS[2].load(Ordering::Relaxed), HITS[3].load(Ordering::Relaxed)]
    );
    std::thread::sleep(Duration::from_millis(500));
    drain_ring();
    // SAFETY: `handle` is the value AddVectoredExceptionHandler returned above.
    let removed = unsafe { RemoveVectoredExceptionHandler(handle) };
    let dropped = DROPPED.load(Ordering::Relaxed);
    tracing::info!("mm_lobby_trace: handler removed ({removed}); hits dropped because the ring was full: {dropped}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dr7_enables_exactly_the_masked_slots() {
        assert_eq!(dr7_for(0), 0);
        assert_eq!(dr7_for(0b0001), 0b01);
        assert_eq!(dr7_for(0b0010), 0b0100);
        assert_eq!(dr7_for(0b0101), 0b01_0001);
        assert_eq!(dr7_for(0b1111), 0b0101_0101);
        // RW / LEN fields (bits 16..32) stay zero = execute breakpoints.
        assert_eq!(dr7_for(0xF) >> 16, 0);
    }

    #[test]
    fn hit_counting_disarms_after_three() {
        assert!(should_record(1, 3) && should_record(3, 3) && !should_record(4, 3));
        assert_eq!(mask_after_hits(0b1111, &[3, 0, 2, 5], 3), 0b0110);
        assert_eq!(mask_after_hits(0b0101, &[0, 9, 0, 0], 3), 0b0101);
        assert_eq!(mask_after_hits(0b0011, &[3, 3, 0, 0], 3), 0);
    }

    #[test]
    fn finish_when_nothing_armed_or_time_is_up() {
        assert!(!should_finish(1, Duration::from_secs(599)));
        assert!(should_finish(0, Duration::from_secs(1)));
        assert!(should_finish(1, Duration::from_secs(600)));
    }

    #[test]
    fn thunk_decoder_reads_the_vtable_displacement() {
        // mov rax,[rcx]; jmp qword ptr [rax+0x70]  (both REX.W and plain forms)
        assert_eq!(decode_vtable_disp(&[0x48, 0x8B, 0x01, 0x48, 0xFF, 0x60, 0x70, 0xCC]), Some(0x70));
        assert_eq!(decode_vtable_disp(&[0x48, 0x8B, 0x01, 0xFF, 0x60, 0x08]), Some(8));
        // disp32 forms
        assert_eq!(decode_vtable_disp(&[0x48, 0x8B, 0x01, 0x48, 0xFF, 0xA0, 0x40, 0x01, 0x00, 0x00]), Some(0x140));
        assert_eq!(decode_vtable_disp(&[0x48, 0x8B, 0x01, 0xFF, 0xA0, 0x00, 0x02, 0x00, 0x00]), Some(0x200));
        // other shapes and short input
        assert_eq!(decode_vtable_disp(&[0x48, 0x89, 0x5C, 0x24, 0x08]), None);
        assert_eq!(decode_vtable_disp(&[0x48, 0x8B, 0x01, 0x48, 0xFF, 0xE0]), None);
        assert_eq!(decode_vtable_disp(&[0x48, 0x8B]), None);
        assert_eq!(decode_vtable_disp(&[0x48, 0x8B, 0x01, 0xFF, 0xA0, 0x00]), None);
    }

    #[test]
    fn frames_and_hits_are_formatted() {
        assert_eq!(format_frame(2, 0x7FF0_0000_1234, Some(("nrsc.dll", 0x1234))), "mm_lobby_trace:   #2 nrsc.dll+0x1234 (0x7FF000001234)");
        assert_eq!(format_frame(0, 0x10, None), "mm_lobby_trace:   #0 0x10 (no module)");
        assert_eq!(short_module_name("Z:\\Game\\SeamlessCoop\\nrsc.dll"), "nrsc.dll");
        assert_eq!(short_module_name("plain.dll"), "plain.dll");
        let h = HitInfo { target: 0, hit_no: 1, tid: 77, own_tid: 77, rip: 0xABC, regs: [0x10, 3, 6, 0] };
        let l = format_hit(&h);
        assert!(l.starts_with("mm_lobby_trace: HIT 1/3 flat ISteamMatchmaking::CreateLobby at 0xABC thread=77 (OUR Steam thread"), "{l}");
        assert!(l.contains("rdx=0x3") && l.contains("ELobbyType=3 (Invisible), cMaxMembers=6"), "{l}");
        let h = HitInfo { target: 3, hit_no: 2, tid: 5, own_tid: 77, rip: 1, regs: [0, 0x0018_6000_0000_0001, 0, 0] };
        let l = format_hit(&h);
        assert!(l.contains("game / Seamless Co-op thread") && l.contains("steamIDLobby="), "{l}");
        assert_eq!(thread_label(5, 0), "game / Seamless Co-op thread");
    }

    #[test]
    fn rebase_maps_only_pointers_into_the_original_stack() {
        assert_eq!(rebase(0x1000, 0x1000, 0x100, 0x9000), 0x9000);
        assert_eq!(rebase(0x10FF, 0x1000, 0x100, 0x9000), 0x90FF);
        assert_eq!(rebase(0x1100, 0x1000, 0x100, 0x9000), 0x1100);
        assert_eq!(rebase(0xFFF, 0x1000, 0x100, 0x9000), 0xFFF);
        assert_eq!(rebase(5, u64::MAX - 1, 100, 0x9000), 5);
    }

    #[test]
    fn handler_ignores_foreign_exceptions() {
        // Nothing is armed in the test process: a null pointer and a pointer with no matching address both continue the search.
        // SAFETY: null is explicitly handled by the handler.
        assert_eq!(unsafe { veh(std::ptr::null_mut()) }, EXCEPTION_CONTINUE_SEARCH);
    }

    #[test]
    fn unwinder_walks_a_real_stack() {
        // Capture this thread's own context and stack through the same path the handler uses, then unwind it.
        // SAFETY: RtlCaptureContext fills an aligned CONTEXT; the stack limits describe this thread.
        unsafe {
            use windows::Win32::System::Diagnostics::Debug::RtlCaptureContext;
            let mut c = AlignedContext(std::mem::zeroed());
            RtlCaptureContext(&mut c.0);
            let (mut lo, mut hi) = (0usize, 0usize);
            GetCurrentThreadStackLimits(&mut lo, &mut hi);
            let rsp = c.0.Rsp as usize;
            assert!(rsp >= lo && rsp < hi);
            let n = (hi - rsp).min(STACK_BYTES);
            // Zeroed on the heap: a ~9 KiB stack temporary would make this test's own frame bigger than the 8 KiB copy.
            let mut uninit = Box::<SlotData>::new_uninit();
            std::ptr::write_bytes(uninit.as_mut_ptr(), 0, 1);
            let mut slot: Box<SlotData> = uninit.assume_init();
            slot.ctx = c.0;
            slot.stack_len = n;
            std::ptr::copy_nonoverlapping(rsp as *const u8, slot.stack.as_mut_ptr().cast::<u8>(), n);
            let frames = unwind(&slot);
            assert!(frames.len() >= 3, "{frames:x?}");
            assert_eq!(frames[0], c.0.Rip);
            // Every frame must lie inside some loaded module.
            assert!(frames.iter().take(3).all(|a| module_of(*a).is_some()), "{frames:x?}");
        }
    }
}
