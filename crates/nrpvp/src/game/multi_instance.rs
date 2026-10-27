//! Several game instances on one PC (orchestrator plan: `.claude/orchestrator/MULTI-INSTANCE.md` §12, §15). Two named objects stop a
//! second game process, and this process closes its own handle to each so the next instance can start:
//! - the engine's single-instance mutex `Global\SekiroMutex`, created early in WinMain (the UTF-16 string is in `nightreign.exe`
//!   `[VERIFIED: byte scan 2026-10-26]`); a second game that finds it taken exits at once. Nucleus Co-op closes it from outside.
//! - the section `Local\DEARXAN_SCHEDULED_AFTER_ARXAN` that me3's host creates through the `dearxan` crate (0.5.2, me3 0.11.0 and
//!   0.13.0). dearxan means it as a process-wide global, but `Local\` is the whole logon session: it holds a pointer into the first
//!   game's heap, and the second game's me3 host opens it, skips its own setup and pushes onto that foreign SLIST: access violation in
//!   ntdll `RtlInterlocked*SList` `[VERIFIED: 2026-10-26, the read address of instance 2's AV equals the pointer in instance 1's
//!   section]`. Closing the last handle removes the name; the mapped view (all dearxan uses after setup) stays valid.
//!
//! Dev builds only, config `allow_multi_instance = true` (or the console command `mutex`). Touches this process's own handle table only.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Memory::{CreateFileMappingW, PAGE_READWRITE};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};

const SYSTEM_EXTENDED_HANDLE_INFORMATION: u32 = 0x40;
const OBJECT_NAME_INFORMATION: u32 = 1;
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
const NAME: &str = "SekiroMutex";
/// Prefix of dearxan's `lazy_global!` sections (`DEARXAN_SCHEDULED_AFTER_ARXAN`, `DEARXAN_NEUTER_ARXAN_RESULT`).
const SECTION_PREFIX: &str = "DEARXAN_";

/// How many handles to the engine mutex / dearxan sections this process has closed so far.
static CLOSED: AtomicU32 = AtomicU32::new(0);
static CLOSED_SECTIONS: AtomicU32 = AtomicU32::new(0);

#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(class: u32, buf: *mut c_void, len: u32, ret: *mut u32) -> i32;
    fn NtQueryObject(h: *mut c_void, class: u32, buf: *mut c_void, len: u32, ret: *mut u32) -> i32;
}

/// `SYSTEM_HANDLE_TABLE_ENTRY_INFO_EX` (40 bytes on x64).
#[repr(C)]
#[derive(Clone, Copy)]
struct Entry {
    object: usize,
    pid: usize,
    handle: usize,
    access: u32,
    backtrace: u16,
    type_index: u16,
    attributes: u32,
    reserved: u32,
}

fn last_component(object_name: &str) -> &str {
    object_name.rsplit('\\').next().unwrap_or("")
}

/// Object names the engine's single-instance mutex can have: `\BaseNamedObjects\SekiroMutex` (`Global\`) or a per-session path.
pub(crate) fn name_matches(object_name: &str) -> bool {
    last_component(object_name).eq_ignore_ascii_case(NAME)
}

/// dearxan's session-wide sections, e.g. `\Sessions\1\BaseNamedObjects\DEARXAN_SCHEDULED_AFTER_ARXAN`.
pub(crate) fn section_matches(object_name: &str) -> bool {
    last_component(object_name).starts_with(SECTION_PREFIX)
}

fn own_handles() -> Vec<Entry> {
    let mut len: u32 = 1 << 20;
    loop {
        let mut buf = vec![0u8; len as usize];
        let mut ret = 0u32;
        // SAFETY: `buf` is `len` bytes; the call fills at most that and reports the needed size in `ret`.
        let st = unsafe {
            NtQuerySystemInformation(
                SYSTEM_EXTENDED_HANDLE_INFORMATION,
                buf.as_mut_ptr().cast(),
                len,
                &mut ret,
            )
        };
        if st == STATUS_INFO_LENGTH_MISMATCH {
            len = len.saturating_mul(2).max(ret.saturating_add(1 << 16));
            if len > 1 << 30 {
                return Vec::new();
            }
            continue;
        }
        if st < 0 {
            return Vec::new();
        }
        // SAFETY: the buffer holds a SYSTEM_HANDLE_INFORMATION_EX: a count, a reserved word, then `count` 40-byte entries.
        let count = unsafe { *(buf.as_ptr() as *const usize) };
        let max = (buf.len().saturating_sub(16)) / std::mem::size_of::<Entry>();
        let pid = unsafe { GetCurrentProcessId() } as usize;
        let mut out = Vec::new();
        for i in 0..count.min(max) {
            // SAFETY: `i < max`, so the entry lies inside `buf`; read unaligned because the Vec<u8> is not 8-aligned by contract.
            let e = unsafe {
                std::ptr::read_unaligned(
                    buf.as_ptr().add(16 + i * std::mem::size_of::<Entry>()) as *const Entry
                )
            };
            if e.pid == pid {
                out.push(e);
            }
        }
        return out;
    }
}

fn type_index_of(h: HANDLE) -> Option<u16> {
    let idx = own_handles()
        .into_iter()
        .find(|e| e.handle == h.0 as usize)
        .map(|e| e.type_index);
    // SAFETY: closing the probe handle the caller created.
    let _ = unsafe { CloseHandle(h) };
    idx
}

/// The `ObjectTypeIndex` of mutexes in this OS build: found by creating an unnamed mutex and looking up its own handle.
fn mutant_type_index() -> Option<u16> {
    // SAFETY: an unnamed mutex owned here, closed by `type_index_of`.
    type_index_of(unsafe { CreateMutexW(None, false, None) }.ok()?)
}

/// The `ObjectTypeIndex` of sections (file mappings): an unnamed 4 KB pagefile mapping, looked up the same way.
fn section_type_index() -> Option<u16> {
    // SAFETY: an unnamed pagefile-backed section owned here, closed by `type_index_of`.
    type_index_of(
        unsafe { CreateFileMappingW(INVALID_HANDLE_VALUE, None, PAGE_READWRITE, 0, 4096, None) }
            .ok()?,
    )
}

fn object_name(handle: usize) -> Option<String> {
    let mut buf = vec![0u8; 2048];
    let mut ret = 0u32;
    // SAFETY: a mutex or section handle of this process (the type was checked first, so no pipe hang); `buf` is 2048 bytes.
    let st = unsafe {
        NtQueryObject(
            handle as *mut c_void,
            OBJECT_NAME_INFORMATION,
            buf.as_mut_ptr().cast(),
            2048,
            &mut ret,
        )
    };
    if st < 0 {
        return None;
    }
    // UNICODE_STRING { u16 Length; u16 MaximumLength; (pad) ; PWSTR Buffer }
    let n = u16::from_le_bytes([buf[0], buf[1]]) as usize / 2;
    let p = usize::from_le_bytes(buf[8..16].try_into().ok()?);
    if n == 0 || p == 0 {
        return None;
    }
    // SAFETY: the kernel put `n` UTF-16 units at `p` inside `buf`.
    let s = unsafe { std::slice::from_raw_parts(p as *const u16, n) };
    Some(String::from_utf16_lossy(s))
}

/// One pass over this process's handles of type `ty`: close those whose name `matches`. Returns (names closed, or all names seen
/// with `list_all`; handles closed).
fn close_matching(
    ty: u16,
    matches: fn(&str) -> bool,
    list_all: bool,
    counter: &AtomicU32,
) -> (Vec<String>, u32) {
    let mut names = Vec::new();
    let mut closed = 0;
    for e in own_handles().into_iter().filter(|e| e.type_index == ty) {
        let Some(n) = object_name(e.handle) else {
            continue;
        };
        if matches(&n) {
            // SAFETY: closing this process's own handle to a named object that blocks a second game instance (module doc).
            if unsafe { CloseHandle(HANDLE(e.handle as *mut c_void)) }.is_ok() {
                closed += 1;
                counter.fetch_add(1, Ordering::SeqCst);
                tracing::info!("multi_instance: closed handle 0x{:X} to {n}", e.handle);
            }
            names.push(n);
        } else if list_all {
            names.push(n);
        }
    }
    (names, closed)
}

/// One pass: close every handle of this process to the engine mutex. Returns (names of all mutexes seen, handles closed).
pub(crate) fn close_once() -> (Vec<String>, u32) {
    let Some(ty) = mutant_type_index() else {
        return (Vec::new(), 0);
    };
    close_matching(ty, name_matches, true, &CLOSED)
}

/// One pass: close every handle of this process to a dearxan section. Returns (names closed, handles closed).
pub(crate) fn close_sections_once() -> (Vec<String>, u32) {
    let Some(ty) = section_type_index() else {
        return (Vec::new(), 0);
    };
    close_matching(ty, section_matches, false, &CLOSED_SECTIONS)
}

/// Poll for up to two minutes (the game creates the mutex in WinMain, before or just after this DLL loads) and close the mutex and
/// the dearxan sections once each. me3's host has already run dearxan's setup when this DLL loads (after the game's main starts).
pub(crate) fn spawn() {
    let _ = std::thread::Builder::new()
        .name("nrpvp-multi-instance".into())
        .spawn(|| {
            let t0 = Instant::now();
            let mut logged = false;
            let (mut mutex_done, mut sections_done) = (false, false);
            while t0.elapsed() < Duration::from_secs(120) {
                if !sections_done {
                    sections_done = close_sections_once().1 > 0;
                }
                if !mutex_done {
                    let (names, closed) = close_once();
                    if !logged && !names.is_empty() {
                        tracing::info!("multi_instance: named mutexes in this process: {names:?}");
                        logged = true;
                    }
                    mutex_done = closed > 0;
                }
                if mutex_done && sections_done {
                    tracing::info!("multi_instance: ready for another instance");
                    return;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            if !mutex_done {
                tracing::warn!("multi_instance: no handle to {NAME} found within 120 s");
            }
            if !sections_done {
                tracing::warn!("multi_instance: no {SECTION_PREFIX}* section found within 120 s");
            }
        });
}

/// Console `mutex`: one pass now, one line back.
pub(crate) fn command() -> String {
    let (names, closed) = close_once();
    let (sections, closed_s) = close_sections_once();
    format!(
        "closed {closed} handle(s) to {NAME} now ({} in total), {closed_s} to dearxan sections {sections:?} ({} in total); named mutexes seen: {names:?}",
        CLOSED.load(Ordering::SeqCst),
        CLOSED_SECTIONS.load(Ordering::SeqCst)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_engine_mutex_names() {
        assert!(name_matches(r"\BaseNamedObjects\SekiroMutex"));
        assert!(name_matches(r"\Sessions\1\BaseNamedObjects\sekiromutex"));
        assert!(!name_matches(r"\BaseNamedObjects\SekiroMutex2"));
        assert!(!name_matches(r"\BaseNamedObjects\Other"));
    }

    #[test]
    fn matches_the_dearxan_sections() {
        assert!(section_matches(
            r"\Sessions\1\BaseNamedObjects\DEARXAN_SCHEDULED_AFTER_ARXAN"
        ));
        assert!(section_matches(
            r"\Sessions\2\BaseNamedObjects\DEARXAN_NEUTER_ARXAN_RESULT"
        ));
        assert!(!section_matches(r"\Sessions\1\BaseNamedObjects\Other"));
    }

    #[test]
    fn a_pass_runs_and_finds_our_own_named_mutex() {
        // A real named mutex of this test process under the engine's name: the pass must find and close it.
        let name: Vec<u16> = "SekiroMutex\0".encode_utf16().collect();
        // SAFETY: a named mutex created and closed (by the pass itself) in this test process.
        let h = unsafe { CreateMutexW(None, false, windows::core::PCWSTR(name.as_ptr())) }
            .expect("mutex");
        let (names, closed) = close_once();
        assert_eq!(closed, 1, "names seen: {names:?}");
        assert!(names.iter().any(|n| name_matches(n)));
        let _ = h;
    }

    #[test]
    fn a_pass_closes_our_own_dearxan_section() {
        // A real named section under a dearxan name (test-only suffix): the pass must close it and free the name.
        let name: Vec<u16> = "Local\\DEARXAN_NRPVP_TEST\0".encode_utf16().collect();
        // SAFETY: a named pagefile section created here and closed by the pass itself.
        let h = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                32,
                windows::core::PCWSTR(name.as_ptr()),
            )
        }
        .expect("section");
        let (names, closed) = close_sections_once();
        assert!(closed >= 1, "names: {names:?}");
        assert!(names.iter().any(|n| n.ends_with("DEARXAN_NRPVP_TEST")));
        let _ = h;
    }
}
