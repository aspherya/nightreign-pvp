//! UI-1 A/B/C investigation (dev only, `ui_menu_trace = true`): what the stock expedition menu does when the player changes Number of
//! Players, presses Commence, and cancels matchmaking, read from the live game with hardware breakpoints (no code is patched).
//!
//! Addresses (game 1.03.3, exe 1.3.3.0; found by `.claude/experiments/UI-0/imm_xrefs.py`, UI-1 card):
//! - DR0 `exe+0x9EEBE0`: the Commence check sequence (shows 506040 / 506070 / 506024 / 506023 "offline" dialogs); `rcx` = menu, settings
//!   object at `[rcx+0xFC8]`, Number of Players byte at settings `+0x49` (`cmp byte [rax+49h], 2`).
//! - DR1 `exe+0x9FB020`: builds the Number of Players list (text ids 130408/130417/130409); settings at `[rbx+0x3E08]`.
//! - DR2 `exe+0x9EE750`: "Cancel matchmaking?" (506012) / "Matchmaking has been canceled" (506011).
//! - DR3 `exe+0x27F4D20` `LookupEntry`, recorded only for the matchmaking / offline text ids in [`WATCH_IDS`], to find the code of the
//!   matchmaking screen from the stack (the ids are computed, not constants in the code).
//!
//! Each recorded hit copies the four argument registers and [`STACK_QWORDS`] qwords of the stack; the trace thread logs them, lists the
//! stack values inside `nightreign.exe` (return-address candidates), dumps the settings object candidates and then polls their first
//! [`SETTINGS_LEN`] bytes every [`POLL`] and logs every change. Log lines start with `ui_menu_trace:`. It owns the debug registers, so
//! the config turns `ui_text_trace`, `mm_quiet_invite` and `mm_lobby_trace` off while it is on.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::STATUS_SINGLE_STEP;
use windows::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::steam::lobtrace::{
    apply_all, EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH, TARGETS,
};

/// (rva, label). Game 1.03.3, exe 1.3.3.0 (see the module doc).
pub(crate) const TARGETS_RVA: [(usize, &str); TARGETS] = [
    (0x9E_EBE0, "Commence checks"),
    (0x9F_B020, "Number of Players list"),
    (0x9E_E750, "Cancel matchmaking"),
    (0x27F_4D20, "LookupEntry (matchmaking ids)"),
];
/// Text ids recorded at DR3: Matchmaking... 336900..=336905, canceled 506011, cancel? 506012, failed 506020..=506022, offline 506023.
pub(crate) const WATCH_IDS: [u32; 11] = [
    336_900, 336_901, 336_902, 336_903, 336_904, 336_905, 506_011, 506_012, 506_020, 506_021,
    506_023,
];
/// Offsets of a settings-object pointer inside the `rcx` / `rbx` object (UI-1 static reading).
const SETTINGS_PTR_OFFS: [usize; 2] = [0xFC8, 0x3E08];
const SETTINGS_LEN: usize = 0x80;
const STACK_QWORDS: usize = 48;
const MAX_LOGGED_PER_TARGET: u32 = 40;
const RING: usize = 64;
const POLL: Duration = Duration::from_millis(100);
const APPLY_EVERY: Duration = Duration::from_secs(2);
const TRACE_DURATION: Duration = Duration::from_secs(30 * 60);

static ADDR: [AtomicUsize; TARGETS] = [const { AtomicUsize::new(0) }; TARGETS];
static HITS: [AtomicU32; TARGETS] = [const { AtomicU32::new(0) }; TARGETS];
static DROPPED: AtomicU32 = AtomicU32::new(0);
static STATE: [AtomicU32; RING] = [const { AtomicU32::new(0) }; RING];
const FREE: u32 = 0;
const WRITING: u32 = 1;
const READY: u32 = 2;

#[derive(Clone, Copy)]
#[repr(C)]
struct Rec {
    target: u32,
    hit_no: u32,
    tid: u32,
    regs: [u64; 5], // rcx rdx r8 r9 rbx
    stack: [u64; STACK_QWORDS],
}

struct Ring(std::cell::UnsafeCell<[Rec; RING]>);
// SAFETY: a slot is written only by the thread that moved its STATE FREE -> WRITING and read only by the trace thread after READY.
unsafe impl Sync for Ring {}
// SAFETY: Rec is plain integers; all-zero is valid.
static RECS: Ring = Ring(std::cell::UnsafeCell::new(unsafe { std::mem::zeroed() }));

/// Whether a hit on `target` with text id `r9` is recorded.
pub(crate) fn wanted(target: usize, r9: u64) -> bool {
    target != 3 || WATCH_IDS.contains(&(r9 as u32))
}

/// Stack values inside `[base, base+size)`, as image offsets, in stack order, at most `max`.
pub(crate) fn image_offsets(stack: &[u64], base: usize, size: usize, max: usize) -> Vec<u64> {
    stack
        .iter()
        .filter(|v| (**v as usize) >= base && (**v as usize) < base + size)
        .map(|v| *v - base as u64)
        .take(max)
        .collect()
}

/// `(offset, old, new)` for every byte that differs.
pub(crate) fn byte_changes(old: &[u8], new: &[u8]) -> Vec<(usize, u8, u8)> {
    old.iter()
        .zip(new)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, (a, b))| (i, *a, *b))
        .collect()
}

unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS; pointers are null-checked. Atomics and a static ring only.
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
        let Some(t) = (0..TARGETS).find(|i| rip != 0 && ADDR[*i].load(Ordering::Relaxed) == rip)
        else {
            return EXCEPTION_CONTINUE_SEARCH;
        };
        if wanted(t, (*ctx).R9) {
            let n = HITS[t].fetch_add(1, Ordering::Relaxed) + 1;
            if n <= MAX_LOGGED_PER_TARGET {
                record(t, n, &*ctx);
            }
        }
        (*ctx).EFlags |= EFLAGS_RF;
    }
    EXCEPTION_CONTINUE_EXECUTION
}

/// # Safety
/// Runs on the faulting thread; reads only its own stack between Rsp and the stack's high limit.
unsafe fn record(t: usize, n: u32, ctx: &windows::Win32::System::Diagnostics::Debug::CONTEXT) {
    #[allow(clippy::needless_range_loop)] // i selects both STATE[i] and the ring slot i
    for i in 0..RING {
        if STATE[i]
            .compare_exchange(FREE, WRITING, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            continue;
        }
        // SAFETY: slot i is ours (WRITING).
        let r = unsafe { &mut *RECS.0.get().cast::<Rec>().add(i) };
        r.target = t as u32;
        r.hit_no = n;
        r.tid = unsafe { GetCurrentThreadId() };
        r.regs = [ctx.Rcx, ctx.Rdx, ctx.R8, ctx.R9, ctx.Rbx];
        let (mut lo, mut hi) = (0usize, 0usize);
        // SAFETY: out-pointers are locals; reads the current thread's TEB only.
        unsafe { windows::Win32::System::Threading::GetCurrentThreadStackLimits(&mut lo, &mut hi) };
        let rsp = ctx.Rsp as usize;
        let avail = if rsp >= lo && rsp < hi {
            (hi - rsp) / 8
        } else {
            0
        };
        let k = avail.min(STACK_QWORDS);
        r.stack = [0; STACK_QWORDS];
        if k > 0 {
            // SAFETY: [rsp, rsp + k*8) lies in the current thread's stack (checked above).
            unsafe { std::ptr::copy_nonoverlapping(rsp as *const u64, r.stack.as_mut_ptr(), k) };
        }
        STATE[i].store(READY, Ordering::Release);
        return;
    }
    DROPPED.fetch_add(1, Ordering::Relaxed);
}

/// Start the trace thread (developer mode with `ui_menu_trace = true`).
pub(crate) fn spawn(base: usize, size: usize) {
    let r = std::thread::Builder::new()
        .name("nrpvp-menutrace".into())
        .spawn(move || {
            if std::panic::catch_unwind(|| run(base, size)).is_err() {
                tracing::error!("ui_menu_trace: trace thread panicked");
            }
        });
    if r.is_err() {
        tracing::warn!("ui_menu_trace: could not spawn the trace thread");
    }
}

fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn run(base: usize, size: usize) {
    let mut addrs = [0u64; TARGETS];
    for (i, (rva, label)) in TARGETS_RVA.iter().enumerate() {
        let a = base + rva;
        addrs[i] = a as u64;
        ADDR[i].store(a, Ordering::Relaxed);
        let first = super::mem::read::<[u8; 8]>(a).map_or("unreadable".to_string(), |b| hex(&b));
        tracing::info!("ui_menu_trace: DR{i} = exe+0x{rva:X} ({label}); first bytes {first}");
    }
    // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature and returns CONTINUE_SEARCH for anything not on our addresses.
    let handle = unsafe { AddVectoredExceptionHandler(1, Some(veh)) };
    if handle.is_null() {
        tracing::warn!("ui_menu_trace: AddVectoredExceptionHandler failed; nothing armed");
        return;
    }
    let own = unsafe { GetCurrentThreadId() };
    let pid = std::process::id();
    let mut applied: HashMap<u32, u8> = HashMap::new();
    let mut scratch: Vec<u32> = Vec::with_capacity(2048);
    let start = Instant::now();
    let mut next_apply = start;
    // settings pointer -> last bytes
    let mut watched: HashMap<usize, Vec<u8>> = HashMap::new();
    tracing::info!(
        "ui_menu_trace: armed for {} min (hardware breakpoints, no code is patched)",
        TRACE_DURATION.as_secs() / 60
    );
    loop {
        std::thread::sleep(POLL);
        let now = Instant::now();
        if now.duration_since(start) >= TRACE_DURATION {
            break;
        }
        if now >= next_apply {
            let _ = apply_all(pid, own, &addrs, 0b1111, false, &mut applied, &mut scratch);
            next_apply = now + APPLY_EVERY;
        }
        #[allow(clippy::needless_range_loop)] // i selects both STATE[i] and the ring slot i
        for i in 0..RING {
            if STATE[i].load(Ordering::Acquire) != READY {
                continue;
            }
            // SAFETY: READY slot; only this thread reads it, then frees it.
            let r = unsafe { *RECS.0.get().cast::<Rec>().add(i) };
            STATE[i].store(FREE, Ordering::Release);
            let t = r.target as usize;
            let frames: Vec<String> = image_offsets(&r.stack, base, size, 12)
                .iter()
                .map(|o| format!("exe+0x{o:X}"))
                .collect();
            tracing::info!(
                "ui_menu_trace: HIT {} #{} thread {} rcx=0x{:X} rdx=0x{:X} r8=0x{:X} r9=0x{:X} ({}) rbx=0x{:X}; stack exe values: {}",
                TARGETS_RVA[t].1,
                r.hit_no,
                r.tid,
                r.regs[0],
                r.regs[1],
                r.regs[2],
                r.regs[3],
                r.regs[3] as u32,
                r.regs[4],
                frames.join(" ")
            );
            if t <= 2 {
                for (who, obj) in [("rcx", r.regs[0] as usize), ("rbx", r.regs[4] as usize)] {
                    for off in SETTINGS_PTR_OFFS {
                        let Some(p) = super::mem::read::<usize>(obj.wrapping_add(off)) else {
                            continue;
                        };
                        if p < 0x10000 {
                            continue;
                        }
                        let b = read_bytes(p, SETTINGS_LEN);
                        if b.len() < 0x50 {
                            continue;
                        }
                        if let std::collections::hash_map::Entry::Vacant(e) = watched.entry(p) {
                            tracing::info!(
                                "ui_menu_trace: settings candidate [{who}+0x{off:X}] = 0x{p:X}; +0x49 = {} ; first 0x{:X} bytes: {}",
                                b[0x49],
                                b.len(),
                                hex(&b)
                            );
                            e.insert(b);
                        }
                    }
                }
            }
        }
        for (p, last) in watched.iter_mut() {
            let b = read_bytes(*p, SETTINGS_LEN);
            if b.len() != last.len() || b == *last {
                continue;
            }
            let ch: Vec<String> = byte_changes(last, &b)
                .iter()
                .map(|(o, a, n)| format!("+0x{o:X}: {a} -> {n}"))
                .collect();
            tracing::info!("ui_menu_trace: settings 0x{p:X} changed: {}", ch.join(", "));
            *last = b;
        }
    }
    let (written, failed, _) = apply_all(pid, own, &addrs, 0, true, &mut applied, &mut scratch);
    tracing::info!(
        "ui_menu_trace: finished; hits {:?}, dropped {}; debug registers cleared on {written} thread(s) ({failed} failed)",
        HITS.iter().map(|h| h.load(Ordering::Relaxed)).collect::<Vec<_>>(),
        DROPPED.load(Ordering::Relaxed)
    );
}

fn read_bytes(addr: usize, len: usize) -> Vec<u8> {
    super::mem::RegionCache::direct().read_prefix(addr, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_matchmaking_ids_are_recorded_at_the_lookup() {
        assert!(wanted(0, 0) && wanted(1, 5) && wanted(2, 0));
        assert!(wanted(3, 336_900) && wanted(3, 506_023));
        assert!(!wanted(3, 130_409));
    }

    #[test]
    fn image_offsets_keep_order_and_limit() {
        let st = [0x10, 0x1000_0010, 5, 0x1000_0020, 0x1000_0030];
        assert_eq!(image_offsets(&st, 0x1000_0000, 0x100, 2), vec![0x10, 0x20]);
    }

    #[test]
    fn byte_changes_lists_differences() {
        assert_eq!(byte_changes(&[1, 2, 3], &[1, 9, 3]), vec![(1, 2, 9)]);
    }
}
