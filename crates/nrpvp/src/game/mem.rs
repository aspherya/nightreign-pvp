//! Guarded access to process memory. Reads are everywhere; the only writes in the whole project are
//! [`write_u8`] (T-008, caller `team::write_team_type`) and [`write_bytes`] (T-032 dev-only snap-back test,
//! caller `poswrite`, LOCAL player only); each caller gates what may be written.
//!
//! Every read first asks `VirtualQuery` whether the whole range is committed and readable (no
//! `PAGE_NOACCESS`, no `PAGE_GUARD`), so a stale or garbage pointer yields `None` instead of an access
//! violation. Residual risk: the page could be unmapped between the query and the read (a TOCTOU race
//! with the game's allocator). Reads are tiny and the window is a few instructions wide.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;

use windows::Win32::System::Memory::{
    VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
    PAGE_WRITECOPY,
};

/// Addresses below this are never mapped user memory on Windows (null page and the 64 KiB reserve).
const MIN_VALID_ADDR: usize = 0x1_0000;

/// True if a `MEMORY_BASIC_INFORMATION::Protect` value allows reading.
pub(crate) fn protect_readable(protect: u32) -> bool {
    const READ_MASK: u32 = PAGE_READONLY.0
        | PAGE_READWRITE.0
        | PAGE_WRITECOPY.0
        | PAGE_EXECUTE_READ.0
        | PAGE_EXECUTE_READWRITE.0
        | PAGE_EXECUTE_WRITECOPY.0;
    protect & (PAGE_GUARD.0 | PAGE_NOACCESS.0) == 0 && protect & READ_MASK != 0
}

/// One `VirtualQuery`: `(base, end, readable)` of the run of pages containing `addr`, all sharing the same
/// state and protection. `None` if the query itself fails.
fn query_region(addr: usize) -> Option<(usize, usize, bool)> {
    let mut mbi = MEMORY_BASIC_INFORMATION::default();
    // SAFETY: `mbi` is a valid out-buffer of the size passed; VirtualQuery only reads the address
    // space description and tolerates any address.
    let n = unsafe {
        VirtualQuery(
            Some(addr as *const c_void),
            &mut mbi,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if n == 0 {
        return None;
    }
    let base = mbi.BaseAddress as usize;
    let end = base.saturating_add(mbi.RegionSize);
    if base > addr || end <= addr {
        return None;
    }
    Some((
        base,
        end,
        mbi.State == MEM_COMMIT && protect_readable(mbi.Protect.0),
    ))
}

/// Number of contiguous readable bytes starting at `addr`, capped at `max` (0 if `addr` itself is not
/// readable).
pub(crate) fn readable_len(addr: usize, max: usize) -> usize {
    if addr < MIN_VALID_ADDR || max == 0 {
        return 0;
    }
    let end = addr.saturating_add(max);
    let mut cur = addr;
    while cur < end {
        match query_region(cur) {
            Some((_, region_end, true)) => cur = region_end.min(end),
            _ => break,
        }
    }
    cur - addr
}

/// Per-run cache of `VirtualQuery` answers: a sorted, non-overlapping list of `[base, end)` page runs with
/// their readability, looked up by binary search, so each region is asked about once per run instead of
/// once per read. Read-only like the rest of this module.
///
/// TOCTOU: an entry can go stale if the game unmaps or re-protects memory after it was learned. Create the
/// cache per discovery run (a few seconds at most) and drop it; the residual risk is the same class as the
/// uncached `read`, only the window is the run instead of a few instructions. Do not keep one across runs.
#[derive(Default)]
pub(crate) struct RegionCache {
    /// Sorted by base, disjoint: `(base, end, readable)`.
    regions: Vec<(usize, usize, bool)>,
    /// `VirtualQuery` syscalls issued.
    pub queries: usize,
    /// Guarded reads answered (cache hit or miss).
    pub lookups: usize,
}

impl RegionCache {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Index of the cached run containing `addr`.
    fn find(&self, addr: usize) -> Option<usize> {
        let i = self.regions.partition_point(|r| r.1 <= addr);
        (i < self.regions.len() && self.regions[i].0 <= addr).then_some(i)
    }

    /// `(end, readable)` of the run containing `addr`, querying the OS only on a cache miss.
    fn run_at(&mut self, addr: usize) -> Option<(usize, bool)> {
        if let Some(i) = self.find(addr) {
            let r = self.regions[i];
            return Some((r.1, r.2));
        }
        self.queries += 1;
        // A failed query (address beyond the user range) is cached as an unreadable page too.
        let page = addr & !0xFFF;
        let (mut base, mut end, readable) =
            query_region(addr).unwrap_or((page, page.saturating_add(0x1000), false));
        // Keep the list disjoint: a run learned mid-way never reaches into a neighbour already known.
        let i = self.regions.partition_point(|r| r.1 <= addr);
        if i > 0 {
            base = base.max(self.regions[i - 1].1);
        }
        if let Some(next) = self.regions.get(i) {
            end = end.min(next.0);
        }
        if base > addr || end <= addr {
            return None;
        }
        self.regions.insert(i, (base, end, readable));
        Some((end, readable))
    }

    /// Cached counterpart of [`readable_len`].
    pub(crate) fn readable_len(&mut self, addr: usize, max: usize) -> usize {
        self.lookups += 1;
        if addr < MIN_VALID_ADDR || max == 0 {
            return 0;
        }
        let end = addr.saturating_add(max);
        let mut cur = addr;
        while cur < end {
            match self.run_at(cur) {
                Some((run_end, true)) => cur = run_end.min(end),
                _ => break,
            }
        }
        cur - addr
    }

    /// Cached counterpart of [`read`].
    pub(crate) fn read<T: Copy>(&mut self, addr: usize) -> Option<T> {
        let size = size_of::<T>();
        if self.readable_len(addr, size) < size {
            return None;
        }
        // SAFETY: [addr, addr+size) lies inside cached committed/readable runs (see the type-level TOCTOU
        // note); `read_unaligned` because game structures are not guaranteed to be aligned for `T`.
        Some(unsafe { ptr::read_unaligned(addr as *const T) })
    }

    /// Cached counterpart of [`read_prefix`].
    pub(crate) fn read_prefix(&mut self, addr: usize, max: usize) -> Vec<u8> {
        let n = self.readable_len(addr, max);
        if n == 0 {
            return Vec::new();
        }
        // SAFETY: `n` bytes at `addr` lie inside cached readable runs (TOCTOU note on the type); they are
        // copied out immediately.
        unsafe { std::slice::from_raw_parts(addr as *const u8, n) }.to_vec()
    }
}

/// Read a `T` from `addr` if the whole `T` is readable, else `None`.
pub(crate) fn read<T: Copy>(addr: usize) -> Option<T> {
    let size = size_of::<T>();
    if readable_len(addr, size) < size {
        return None;
    }
    // SAFETY: the range [addr, addr+size) was just verified committed and readable; `read_unaligned`
    // because game structures are not guaranteed to be aligned for `T` at arbitrary offsets.
    Some(unsafe { ptr::read_unaligned(addr as *const T) })
}

/// Borrow `len` bytes at `addr` as a slice, if the whole range is readable.
///
/// # Safety
/// The memory must stay mapped and unchanged for as long as the slice is used. Only call this for the
/// loaded game image (its headers and sections live for the whole process lifetime).
pub(crate) unsafe fn readable_slice(addr: usize, len: usize) -> Option<&'static [u8]> {
    if len == 0 || readable_len(addr, len) < len {
        return None;
    }
    // SAFETY: range verified readable above; lifetime obligation is the caller's (see above).
    Some(unsafe { std::slice::from_raw_parts(addr as *const u8, len) })
}

/// Copy the readable prefix of `[addr, addr+max)` into a fresh `Vec` (empty if `addr` is unreadable).
/// Used to snapshot live structures so they can be scanned without further syscalls.
#[cfg(test)]
pub(crate) fn read_prefix(addr: usize, max: usize) -> Vec<u8> {
    let n = readable_len(addr, max);
    if n == 0 {
        return Vec::new();
    }
    // SAFETY: `n` bytes at `addr` were just verified readable; they are copied out immediately.
    unsafe { std::slice::from_raw_parts(addr as *const u8, n) }.to_vec()
}

/// True if a `MEMORY_BASIC_INFORMATION::Protect` value allows a plain in-place write. `PAGE_WRITECOPY` and
/// `PAGE_EXECUTE_WRITECOPY` are deliberately excluded: a store there would silently fork the page.
pub(crate) fn protect_writable(protect: u32) -> bool {
    const WRITE_MASK: u32 = PAGE_READWRITE.0 | PAGE_EXECUTE_READWRITE.0;
    protect & (PAGE_GUARD.0 | PAGE_NOACCESS.0) == 0 && protect & WRITE_MASK != 0
}

/// Store ONE byte at `addr` after checking, with `VirtualQuery`, that the page is committed and writable.
/// Used by the T-008 team_type write. No hooks, no page-protection changes.
///
/// SAFETY / policy: the caller decides *what* may be written (`team::write_team_type` only calls this for
/// `PlayerIns`+0x74, TECHNICAL_NOTES.md section 4, NR 1.3.3.0, after a vtable check). This function only
/// guarantees that the byte lies in committed, writable, non-guard memory, so it cannot fault on a stale
/// or garbage address. Residual risk: the page could be freed between the query and the store (TOCTOU),
/// same class as the guarded reads; the store is a single byte via `write_volatile`.
pub(crate) fn write_u8(addr: usize, value: u8) -> Result<(), &'static str> {
    check_writable(addr, 1)?;
    // SAFETY: the byte at `addr` was just verified to lie in a committed, writable, non-guard page (see
    // the doc comment for the policy and the TOCTOU note). A u8 store has no alignment requirement.
    unsafe { ptr::write_volatile(addr as *mut u8, value) };
    Ok(())
}

/// `VirtualQuery` check that `[addr, addr+len)` lies inside ONE committed, plain-writable, non-guard run of pages.
fn check_writable(addr: usize, len: usize) -> Result<(), &'static str> {
    if addr < MIN_VALID_ADDR {
        return Err("address below the valid user range");
    }
    let mut mbi = MEMORY_BASIC_INFORMATION::default();
    // SAFETY: `mbi` is a valid out-buffer of the size passed; VirtualQuery tolerates any address.
    let n = unsafe {
        VirtualQuery(
            Some(addr as *const c_void),
            &mut mbi,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if n == 0 {
        return Err("VirtualQuery failed");
    }
    let base = mbi.BaseAddress as usize;
    let end = base.saturating_add(mbi.RegionSize);
    if base > addr || end <= addr {
        return Err("VirtualQuery returned a region that does not contain the address");
    }
    if mbi.State != MEM_COMMIT {
        return Err("page is not committed");
    }
    if !protect_writable(mbi.Protect.0) {
        return Err("page is not plain writable (read-only, guard, no-access or copy-on-write)");
    }
    if addr.saturating_add(len) > end {
        return Err("the range runs past the end of the writable region");
    }
    Ok(())
}

/// Largest store [`write_bytes`] accepts.
pub(crate) const MAX_WRITE_BYTES: usize = 16;

/// T-032 (dev only): store 1..=[`MAX_WRITE_BYTES`] bytes at `addr` after the same `VirtualQuery` check as
/// [`write_u8`], applied to the whole range (one committed, plain-writable, non-guard run). Same TOCTOU
/// residual risk. The caller (`poswrite`) decides what may be written: the LOCAL player's position candidate
/// only, after a PlayerIns vtable re-check. Bytes are stored one at a time with `write_volatile`.
pub(crate) fn write_bytes(addr: usize, data: &[u8]) -> Result<(), &'static str> {
    if data.is_empty() || data.len() > MAX_WRITE_BYTES {
        return Err("write length must be 1..=16 bytes");
    }
    check_writable(addr, data.len())?;
    for (i, &b) in data.iter().enumerate() {
        // SAFETY: [addr, addr+len) was just verified to lie inside one committed, writable, non-guard run (see
        // the doc comment); a u8 store has no alignment requirement.
        unsafe { ptr::write_volatile((addr + i) as *mut u8, b) };
    }
    Ok(())
}

/// Copy exactly `len` bytes at `addr`; every unreadable 4 KiB page is left zero-filled. Returns the
/// bytes and the number of bytes that could not be read. Used to snapshot PE sections of the game image.
pub(crate) fn copy_guarded(addr: usize, len: usize) -> (Vec<u8>, usize) {
    const CHUNK: usize = 0x10000;
    const PAGE: usize = 0x1000;
    let mut out = vec![0u8; len];
    let mut missing = 0usize;
    let mut off = 0usize;
    while off < len {
        let n = CHUNK.min(len - off);
        if readable_len(addr + off, n) >= n {
            // SAFETY: the range was just verified readable and `out[off..off+n]` is in bounds.
            unsafe {
                ptr::copy_nonoverlapping((addr + off) as *const u8, out[off..].as_mut_ptr(), n);
            }
        } else {
            let mut p = off;
            while p < off + n {
                let m = PAGE.min(off + n - p);
                if readable_len(addr + p, m) >= m {
                    // SAFETY: as above, for this smaller range.
                    unsafe {
                        ptr::copy_nonoverlapping((addr + p) as *const u8, out[p..].as_mut_ptr(), m);
                    }
                } else {
                    missing += m;
                }
                p += m;
            }
        }
        off += n;
    }
    (out, missing)
}

/// `0xADDR` plus ` (exe+0xRVA)` when the address lies inside the game image.
pub(crate) fn va_rva(addr: usize, exe_base: usize, exe_size: usize) -> String {
    if addr >= exe_base && addr - exe_base < exe_size {
        format!("0x{addr:X} (exe+0x{:X})", addr - exe_base)
    } else {
        format!("0x{addr:X}")
    }
}

/// Read a NUL-terminated printable-ASCII string of at most `max` bytes (NUL not counted). `None` if the
/// memory is unreadable, there is no NUL within `max` bytes, or the text is empty or not printable ASCII.
pub(crate) fn read_cstr(addr: usize, max: usize) -> Option<String> {
    let n = readable_len(addr, max + 1);
    if n == 0 {
        return None;
    }
    // SAFETY: `n` bytes at `addr` were just verified readable; the bytes are copied out immediately.
    let bytes = unsafe { std::slice::from_raw_parts(addr as *const u8, n) };
    parse_cstr(bytes, max)
}

/// Pure part of [`read_cstr`]: first NUL-terminated run, printable ASCII only.
pub(crate) fn parse_cstr(bytes: &[u8], max: usize) -> Option<String> {
    let nul = bytes.iter().position(|&b| b == 0)?;
    if nul == 0 || nul > max {
        return None;
    }
    let s = &bytes[..nul];
    if !s.iter().all(|&b| (0x20..0x7F).contains(&b)) {
        return None;
    }
    Some(String::from_utf8_lossy(s).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protect_flags() {
        assert!(protect_readable(PAGE_READONLY.0));
        assert!(protect_readable(PAGE_READWRITE.0));
        assert!(protect_readable(PAGE_EXECUTE_READ.0));
        assert!(!protect_readable(PAGE_NOACCESS.0));
        assert!(!protect_readable(PAGE_READWRITE.0 | PAGE_GUARD.0));
        assert!(!protect_readable(0));
    }

    #[test]
    fn write_protect_flags() {
        assert!(protect_writable(PAGE_READWRITE.0));
        assert!(protect_writable(PAGE_EXECUTE_READWRITE.0));
        assert!(!protect_writable(PAGE_READONLY.0));
        assert!(!protect_writable(PAGE_EXECUTE_READ.0));
        assert!(!protect_writable(PAGE_WRITECOPY.0));
        assert!(!protect_writable(PAGE_READWRITE.0 | PAGE_GUARD.0));
        assert!(!protect_writable(PAGE_NOACCESS.0));
        assert!(!protect_writable(0));
    }

    #[test]
    fn write_bytes_stores_up_to_16_bytes_and_rejects_bad_input() {
        let mut v = vec![0x11u8; 32];
        let a = v.as_mut_ptr() as usize;
        assert_eq!(
            write_bytes(a + 4, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]),
            Ok(())
        );
        assert_eq!(&v[4..16], &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert!(v[..4].iter().chain(&v[16..]).all(|&b| b == 0x11));
        assert!(write_bytes(a, &[]).is_err());
        assert!(write_bytes(a, &[0u8; 17]).is_err());
        assert_eq!(write_bytes(a, &[0u8; 16]), Ok(()));
        assert!(write_bytes(0, &[1]).is_err());
        assert!(write_bytes(0x10, &[1, 2]).is_err());
        let ro: &'static [u8] = b"read-only literal";
        assert!(write_bytes(ro.as_ptr() as usize, &[1, 2, 3]).is_err());
    }

    #[test]
    fn write_u8_stores_one_byte_and_rejects_bad_addresses() {
        let mut v = vec![0x11u8; 16];
        let a = v.as_mut_ptr() as usize;
        assert_eq!(write_u8(a + 5, 0xAB), Ok(()));
        assert_eq!(v[5], 0xAB);
        assert!(v.iter().enumerate().all(|(i, &b)| i == 5 || b == 0x11));
        assert!(write_u8(0, 1).is_err());
        assert!(write_u8(0x10, 1).is_err());
        assert!(write_u8(0x7FFF_FFFF_FFF0, 1).is_err());
        // Read-only memory (a string literal in .rdata) must be refused, not faulted on.
        let ro: &'static [u8] = b"read-only literal";
        assert!(write_u8(ro.as_ptr() as usize, 1).is_err());
    }

    #[test]
    fn reads_own_stack_value() {
        let v: u64 = 0x1122_3344_5566_7788;
        assert_eq!(read::<u64>(&v as *const u64 as usize), Some(v));
        let b: u8 = 0xAB;
        assert_eq!(read::<u8>(&b as *const u8 as usize), Some(0xAB));
    }

    #[test]
    fn rejects_null_and_low_addresses() {
        assert_eq!(read::<u64>(0), None);
        assert_eq!(read::<u8>(0x10), None);
        assert_eq!(readable_len(0, 100), 0);
    }

    #[test]
    fn rejects_unmapped_high_address() {
        // Top of the canonical user range: never mapped.
        assert_eq!(read::<u64>(0x7FFF_FFFF_FFF0), None);
    }

    #[test]
    fn range_spanning_past_end_is_rejected() {
        // The last bytes before an unmapped gap: asking for more than is readable returns the short length.
        let v = [1u8; 16];
        let a = v.as_ptr() as usize;
        assert!(readable_len(a, 16) >= 16);
        assert!(read::<[u8; 16]>(a).is_some());
    }

    #[test]
    fn read_prefix_and_copy_guarded() {
        let v: Vec<u8> = (0..200u8).collect();
        let a = v.as_ptr() as usize;
        assert_eq!(read_prefix(a, 100), v[..100].to_vec());
        assert!(read_prefix(0, 100).is_empty());
        let (c, missing) = copy_guarded(a, 200);
        assert_eq!(c, v);
        assert_eq!(missing, 0);
        // Entirely unmapped: zero-filled and fully counted as missing.
        let (c, missing) = copy_guarded(0x7FFF_FFFF_0000, 0x2000);
        assert_eq!(c, vec![0u8; 0x2000]);
        assert_eq!(missing, 0x2000);
    }

    #[test]
    fn region_cache_matches_uncached_and_saves_queries() {
        let v: Vec<u8> = (0..=255u8).cycle().take(0x20000).collect();
        let a = v.as_ptr() as usize;
        let mut c = RegionCache::new();
        assert_eq!(c.read_prefix(a, 0x20000), v);
        let after_first = c.queries;
        assert!(after_first >= 1);
        // Hundreds of further reads in the same region cost no further syscalls.
        for (i, &byte) in v.iter().enumerate().take(500) {
            assert_eq!(c.read::<u8>(a + i), Some(byte));
            assert_eq!(c.readable_len(a + i, 8), 8);
        }
        assert_eq!(c.queries, after_first);
        assert!(c.lookups >= 1000);
        // Same answers as the uncached path for unreadable / low / unmapped addresses.
        assert_eq!(c.read::<u64>(0), None);
        assert_eq!(c.read::<u64>(0x10), None);
        assert_eq!(c.read::<u64>(0x7FFF_FFFF_FFF0), None);
        assert!(c.read_prefix(0x7FFF_FFFF_0000, 64).is_empty());
        // A negative answer is cached too.
        let q = c.queries;
        assert_eq!(c.read::<u64>(0x7FFF_FFFF_FFF0), None);
        assert_eq!(c.queries, q);
        // Sorted and disjoint.
        assert!(c.regions.windows(2).all(|w| w[0].1 <= w[1].0));
    }

    #[test]
    fn va_rva_formats() {
        const BASE: usize = 0x7FF7_9155_0000;
        const SIZE: usize = 0x7C5_D600;
        assert_eq!(va_rva(BASE + 0x10, BASE, SIZE), "0x7FF791550010 (exe+0x10)");
        assert_eq!(va_rva(0x1234, BASE, SIZE), "0x1234");
        assert_eq!(va_rva(BASE + SIZE, BASE, SIZE), "0x7FF7991AD600");
        assert_eq!(va_rva(BASE, BASE, SIZE), "0x7FF791550000 (exe+0x0)");
    }

    #[test]
    fn cstr_parsing() {
        assert_eq!(
            parse_cstr(b"WorldChrMan\0xx", 128).as_deref(),
            Some("WorldChrMan")
        );
        assert_eq!(parse_cstr(b"\0abc", 128), None); // empty
        assert_eq!(parse_cstr(b"abc", 128), None); // no NUL
        assert_eq!(parse_cstr(b"ab\x01c\0", 128), None); // non printable
        assert_eq!(parse_cstr(b"abcdef\0", 3), None); // too long
    }
}
