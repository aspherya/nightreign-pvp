//! Hardware debug registers (DR0..DR3): the primitives every execute-breakpoint feature shares. R1 moved them here from the
//! dev-only lobby tracer (`steam::lobtrace`) so player features (`hwbp`, `steam::quietinvite`, `game::menuflow`) no longer depend
//! on dev code. Interop only: no byte of the game or of any DLL is written; a thread is suspended only for
//! `GetThreadContext` / `SetThreadContext`, and nothing allocates or logs while it is suspended.

use std::collections::HashMap;
use std::mem::size_of;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::{CONTEXT, CONTEXT_DEBUG_REGISTERS_AMD64};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::Threading::{
    OpenThread, THREAD_GET_CONTEXT, THREAD_SET_CONTEXT, THREAD_SUSPEND_RESUME,
};

/// Number of debug registers with an address (DR0..DR3).
pub(crate) const TARGETS: usize = 4;
/// EFlags resume flag: suppresses the instruction breakpoint for exactly one instruction (Intel SDM vol. 3, 17.3.1.1).
pub(crate) const EFLAGS_RF: u32 = 0x1_0000;
pub(crate) const EXCEPTION_CONTINUE_EXECUTION: i32 = -1;
pub(crate) const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

/// Bit `WRITE_SHIFT + i` of a slot mask: slot `i` is a 4-byte write watch (data breakpoint) instead of an execute breakpoint.
pub(crate) const WRITE_SHIFT: u32 = 4;

/// DR7 for the slots in `mask`: bits 0..3 enable slot `i` (local-enable bit `2*i`); an enabled slot is an execute breakpoint (RW and
/// LEN fields 00) unless bit `4+i` is set, which makes it a 4-byte write watch (RW = 01 at bit `16+4i`, LEN = 11 at bit `18+4i`)
/// (Intel SDM vol. 3, 17.2.4).
pub(crate) fn dr7_for(mask: u8) -> u64 {
    (0..TARGETS as u32)
        .filter(|i| mask & (1 << i) != 0)
        .fold(0u64, |acc, i| {
            let mut v = 1u64 << (2 * i);
            if mask & (1 << (WRITE_SHIFT + i)) != 0 {
                v |= (0b01u64 << (16 + 4 * i)) | (0b11u64 << (18 + 4 * i));
            }
            acc | v
        })
}

/// Decode the vtable displacement out of a flat-API thunk: `mov rax,[rcx]` (48 8B 01) followed by `jmp qword ptr [rax+disp]` as
/// `48 FF 60 d8`, `FF 60 d8`, `48 FF A0 d32` or `FF A0 d32`. `None` for anything else (the caller logs the raw bytes).
pub(crate) fn decode_vtable_disp(b: &[u8]) -> Option<u32> {
    if b.len() < 3 || b[..3] != [0x48, 0x8B, 0x01] {
        return None;
    }
    let rest = &b[3..];
    let rest = if rest.first() == Some(&0x48) {
        &rest[1..]
    } else {
        rest
    };
    match rest {
        [0xFF, 0x60, d, ..] => Some(u32::from(*d)),
        [0xFF, 0xA0, a, b2, c, d, ..] => Some(u32::from_le_bytes([*a, *b2, *c, *d])),
        _ => None,
    }
}

// Raw kernel32 declarations: no `windows::core::Error` (which may allocate / touch COM) inside the suspend window.
#[link(name = "kernel32")]
extern "system" {
    fn SuspendThread(h: HANDLE) -> u32;
    fn ResumeThread(h: HANDLE) -> u32;
    fn GetThreadContext(h: HANDLE, c: *mut CONTEXT) -> i32;
    fn SetThreadContext(h: HANDLE, c: *const CONTEXT) -> i32;
}

#[repr(C, align(16))]
pub(crate) struct AlignedContext(pub(crate) CONTEXT);

/// Write `addrs` / `mask` into the debug registers of thread `tid`. No allocation while the thread is suspended.
fn set_debug_regs(tid: u32, addrs: &[u64; TARGETS], mask: u8) -> Result<(), &'static str> {
    // SAFETY: OpenThread has no memory preconditions; the returned handle is closed below.
    let h = match unsafe {
        OpenThread(
            THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_SET_CONTEXT,
            false,
            tid,
        )
    } {
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
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else {
            return false;
        };
        let mut e = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
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
    fn dr7_write_slots_set_rw_and_len() {
        // slot 3 write watch: L3 = bit 6, RW3 (bits 28-29) = 01, LEN3 (bits 30-31) = 11
        let v = dr7_for(0b1000 | (0b1000 << WRITE_SHIFT));
        assert_eq!(v, (1 << 6) | (0b01 << 28) | (0b11 << 30));
        // slot 0 write + slot 2 execute
        let v = dr7_for(0b0101 | (1 << WRITE_SHIFT));
        assert_eq!(v, 0b01_0001 | (0b01 << 16) | (0b11 << 18));
        // a write flag without the enable bit does nothing
        assert_eq!(dr7_for(1 << (WRITE_SHIFT + 1)), 0);
    }

    #[test]
    fn thunk_decoder_reads_the_vtable_displacement() {
        // mov rax,[rcx]; jmp qword ptr [rax+0x70]  (both REX.W and plain forms)
        assert_eq!(
            decode_vtable_disp(&[0x48, 0x8B, 0x01, 0x48, 0xFF, 0x60, 0x70, 0xCC]),
            Some(0x70)
        );
        assert_eq!(
            decode_vtable_disp(&[0x48, 0x8B, 0x01, 0xFF, 0x60, 0x08]),
            Some(8)
        );
        // disp32 forms
        assert_eq!(
            decode_vtable_disp(&[0x48, 0x8B, 0x01, 0x48, 0xFF, 0xA0, 0x40, 0x01, 0x00, 0x00]),
            Some(0x140)
        );
        assert_eq!(
            decode_vtable_disp(&[0x48, 0x8B, 0x01, 0xFF, 0xA0, 0x00, 0x02, 0x00, 0x00]),
            Some(0x200)
        );
        // other shapes and short input
        assert_eq!(decode_vtable_disp(&[0x48, 0x89, 0x5C, 0x24, 0x08]), None);
        assert_eq!(
            decode_vtable_disp(&[0x48, 0x8B, 0x01, 0x48, 0xFF, 0xE0]),
            None
        );
        assert_eq!(decode_vtable_disp(&[0x48, 0x8B]), None);
        assert_eq!(
            decode_vtable_disp(&[0x48, 0x8B, 0x01, 0xFF, 0xA0, 0x00]),
            None
        );
    }
}
