//! FD4 / DLRF singleton discovery for NR, scanning EVERY executable section.
//!
//! Technique and pattern code: `from-singleton` v3.0.1 (Dasaav-dsv, MIT/Apache-2.0),
//! `src/find.rs` (`derived_singletons`, `fd4_singletons`, `FD4SingletonPartialResult::finish`) and
//! `src/lib.rs` (`map`), as summarised in `.claude/re/er-reference.md` section 1.2. We reuse the crate's
//! public instruction-pattern iterators (`derived_singleton_pat_iter`, `fd4_singleton_pat_iter`) and
//! replace only its driver, because the stock driver scans just the first section named `.text`
//! (see `image.rs`) and NR has two.
//!
//! Deliberate differences from the stock driver (all logged where relevant):
//! * every executable section is scanned, not only the first `.text`;
//! * the `call` displacement to the DLRF name-getter is sign-extended (the stock code adds it as
//!   `u32 as usize`, so a backward call is mis-resolved and that candidate is skipped);
//! * the name-getter is chosen by majority vote over all FD4 candidates, not "the first one that
//!   points into .text";
//! * candidates whose call target is not the chosen getter are dropped.
//!
//! The scan itself is read-only. Resolving FD4 names CALLS the game's name-getter function (like the
//! stock `finish()` does), so it is only done after FD4 singletons are observed non-null, i.e. after
//! DLRF reflection exists.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::ffi::c_char;
use std::ops::Range;

use from_singleton::find::{derived_singleton_pat_iter, fd4_singleton_pat_iter};

use super::image::{in_ranges, Section};
use super::mem;

/// Max length of a singleton name we accept.
const MAX_NAME: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `FD4Singleton`: name comes from the DLRF reflection object via the name-getter.
    Fd4,
    /// `FD4DerivedSingleton`: name is a C string referenced by the call site.
    Derived,
}

/// One discovered singleton: its static pointer slot (absolute address) and name.
#[derive(Clone, Debug)]
pub(crate) struct Found {
    pub name: String,
    pub kind: Kind,
    /// Absolute address of the static pointer slot (`*slot` is the instance, null until constructed).
    pub slot: usize,
}

/// Per-section scan statistics: pattern hits that passed the data-range filter vs all hits.
#[derive(Clone, Debug, Default)]
pub(crate) struct SectionStats {
    pub name: String,
    pub rva: u32,
    pub scanned_len: usize,
    pub derived_hits: usize,
    pub derived_accepted: usize,
    pub fd4_hits: usize,
    pub fd4_accepted: usize,
}

/// FD4 candidate: slot RVA -> (reflection object RVA, name-getter RVA seen at that call site).
type Fd4Map = BTreeMap<u32, (u32, u32)>;
/// Derived candidate: slot RVA -> name string RVA.
type DerivedMap = BTreeMap<u32, u32>;

/// Raw accumulator filled by [`scan_text`].
#[derive(Default)]
pub(crate) struct RawScan {
    pub fd4: Fd4Map,
    pub derived: DerivedMap,
}

/// Final scan result.
pub(crate) struct Scan {
    /// FD4 candidates (slot RVA, reflection RVA) that use the chosen name-getter.
    pub fd4: Vec<(u32, u32)>,
    /// Chosen name-getter RVA and how many candidates voted for it / total FD4 candidates.
    pub getter: Option<(u32, usize, usize)>,
    pub derived: Vec<(u32, u32)>,
}

/// `rva(end of slice) + le32(slice)`, i.e. the target of a rip-relative displacement whose 4 bytes are
/// `disp` (the displacement is the last field of the instruction, so "end of disp" = next instruction).
fn target_rva(disp: &[u8], image_base: usize) -> Option<u32> {
    let bytes: [u8; 4] = disp.try_into().ok()?;
    let end = (disp.as_ptr() as usize).checked_add(disp.len())?;
    let end_rva = end.checked_sub(image_base)? as u32;
    Some(end_rva.wrapping_add(u32::from_le_bytes(bytes)))
}

/// Run both pattern iterators over one section's bytes and accumulate candidates.
///
/// `text` must be a slice of the loaded image starting at `image_base + section rva`, so the
/// displacement arithmetic (pointer based, like the stock driver) yields image RVAs.
pub(crate) fn scan_text(
    text: &[u8],
    image_base: usize,
    data_ranges: &[Range<u32>],
    out: &mut RawScan,
) -> SectionStats {
    let mut stats = SectionStats {
        scanned_len: text.len(),
        ..Default::default()
    };

    for [addr_d, name_d] in derived_singleton_pat_iter(text) {
        stats.derived_hits += 1;
        let (Some(slot), Some(name)) = (
            target_rva(addr_d, image_base),
            target_rva(name_d, image_base),
        ) else {
            continue;
        };
        // Slot must be in .data and pointer aligned (stock: find.rs#L67-L70 and `is_aligned`).
        if slot % 8 != 0 || !in_ranges(data_ranges, slot) {
            continue;
        }
        // First sighting wins, like the stock driver's dedupe by slot address.
        if let Entry::Vacant(e) = out.derived.entry(slot) {
            e.insert(name);
            stats.derived_accepted += 1;
        }
    }

    for [addr_d, refl_d, fn_d] in fd4_singleton_pat_iter(text) {
        stats.fd4_hits += 1;
        let (Some(slot), Some(refl), Some(getter)) = (
            target_rva(addr_d, image_base),
            target_rva(refl_d, image_base),
            target_rva(fn_d, image_base),
        ) else {
            continue;
        };
        if slot % 8 != 0 || !in_ranges(data_ranges, slot) {
            continue;
        }
        if let Entry::Vacant(e) = out.fd4.entry(slot) {
            e.insert((refl, getter));
            stats.fd4_accepted += 1;
        }
    }
    stats
}

/// Most frequent value and its count; ties go to the lowest value (deterministic).
pub(crate) fn majority<I: IntoIterator<Item = u32>>(vals: I) -> Option<(u32, usize)> {
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for v in vals {
        *counts.entry(v).or_default() += 1;
    }
    // BTreeMap iterates ascending; `max_by_key` returns the LAST max, so reverse for lowest-wins.
    counts.into_iter().rev().max_by_key(|&(_, c)| c)
}

/// Turn the raw accumulator into a [`Scan`]: pick the name-getter by majority vote (it must lie in an
/// executable section) and keep only the FD4 candidates that call it.
pub(crate) fn finalize(raw: RawScan, exec_ranges: &[Range<u32>]) -> Scan {
    let total = raw.fd4.len();
    let getter = majority(raw.fd4.values().map(|&(_, g)| g))
        .filter(|&(g, _)| in_ranges(exec_ranges, g))
        .map(|(g, c)| (g, c, total));
    let fd4 = match getter {
        Some((g, _, _)) => raw
            .fd4
            .iter()
            .filter(|(_, &(_, gg))| gg == g)
            .map(|(&slot, &(refl, _))| (slot, refl))
            .collect(),
        None => Vec::new(),
    };
    Scan {
        fd4,
        getter,
        derived: raw.derived.into_iter().collect(),
    }
}

/// Resolve `FD4DerivedSingleton` names (plain C strings in the image). Safe to do at any time after the
/// image is decrypted.
pub(crate) fn resolve_derived(scan: &Scan, image_base: usize) -> (Vec<Found>, usize) {
    let mut out = Vec::new();
    let mut failed = 0;
    for &(slot, name) in &scan.derived {
        match mem::read_cstr(image_base + name as usize, MAX_NAME) {
            Some(n) => out.push(Found {
                name: n,
                kind: Kind::Derived,
                slot: image_base + slot as usize,
            }),
            None => failed += 1,
        }
    }
    (out, failed)
}

/// Resolve `FD4Singleton` names by calling the game's DLRF name-getter on each reflection object
/// (same call the stock `FD4SingletonPartialResult::finish` makes).
///
/// # Safety
/// DLRF reflection must be initialised (the caller gates on FD4 slots being non-null) and
/// `getter_rva` must be the real name-getter. Both are heuristics inherited from from-singleton and
/// are UNVERIFIED on NR 1.3.3.0 until the HUMAN run.
pub(crate) unsafe fn resolve_fd4(scan: &Scan, image_base: usize) -> (Vec<Found>, usize) {
    let Some((getter_rva, _, _)) = scan.getter else {
        return (Vec::new(), scan.fd4.len());
    };
    // SAFETY: caller contract; the address is inside an executable section of the loaded image.
    let getter: unsafe extern "C" fn(*const u8) -> *const c_char =
        unsafe { std::mem::transmute(image_base + getter_rva as usize) };
    let mut out = Vec::new();
    let mut failed = 0;
    for &(slot, refl) in &scan.fd4 {
        // SAFETY: `refl` is an address inside the image (a static reflection object); the getter only
        // reads it. Its return value is validated through `read_cstr` before use.
        let p = unsafe { getter((image_base + refl as usize) as *const u8) };
        match mem::read_cstr(p as usize, MAX_NAME) {
            Some(n) => out.push(Found {
                name: n,
                kind: Kind::Fd4,
                slot: image_base + slot as usize,
            }),
            None => failed += 1,
        }
    }
    (out, failed)
}

/// Number of FD4 candidates whose static slot currently holds a non-null instance.
pub(crate) fn count_nonnull_fd4(scan: &Scan, image_base: usize) -> usize {
    scan.fd4
        .iter()
        .filter(|&&(slot, _)| matches!(mem::read::<usize>(image_base + slot as usize), Some(p) if p != 0))
        .count()
}

/// Convenience for the orchestrator: scan every given section of the loaded image.
///
/// # Safety
/// `image_base` must be the base of the loaded game image and `sections` its real section table.
pub(crate) unsafe fn scan_image(
    image_base: usize,
    image_size: usize,
    exec: &[&Section],
    data_ranges: &[Range<u32>],
) -> (RawScan, Vec<SectionStats>) {
    let mut raw = RawScan::default();
    let mut stats = Vec::new();
    for s in exec {
        let want = (s.vsize as usize).min(image_size.saturating_sub(s.rva as usize));
        let addr = image_base + s.rva as usize;
        let have = mem::readable_len(addr, want);
        let mut st = SectionStats {
            name: s.name.clone(),
            rva: s.rva,
            ..Default::default()
        };
        if have > 0 {
            // SAFETY: range verified readable; the game image stays mapped for the whole process.
            if let Some(text) = unsafe { mem::readable_slice(addr, have) } {
                let r = scan_text(text, image_base, data_ranges, &mut raw);
                st = SectionStats {
                    name: s.name.clone(),
                    rva: s.rva,
                    ..r
                };
            }
        }
        stats.push(st);
    }
    (raw, stats)
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)] // single-element slices of ranges are intended here
mod tests {
    use super::*;

    const LEA_RCX: [u8; 3] = [0x48, 0x8D, 0x0D];
    const LEA_R8: [u8; 3] = [0x4C, 0x8D, 0x05];
    const LEA_R9: [u8; 3] = [0x4C, 0x8D, 0x0D];

    /// Write `opcode` + rip-relative disp32 (7-byte lea / mov) at `pos` pointing at `target` (image RVA ==
    /// buffer offset in these tests).
    fn put_rip(buf: &mut [u8], pos: usize, opcode: [u8; 3], target: usize) {
        buf[pos..pos + 3].copy_from_slice(&opcode);
        let disp = (target as i64 - (pos as i64 + 7)) as i32;
        buf[pos + 3..pos + 7].copy_from_slice(&disp.to_le_bytes());
    }

    fn put_call(buf: &mut [u8], pos: usize, target: usize) {
        buf[pos] = 0xE8;
        let disp = (target as i64 - (pos as i64 + 5)) as i32;
        buf[pos + 1..pos + 5].copy_from_slice(&disp.to_le_bytes());
    }

    /// `mov rax,[rip+slot]; test rax,rax; jne +0x10` at `p`; returns the offset after the jne.
    fn put_prologue(buf: &mut [u8], p: usize, slot: usize) -> usize {
        put_rip(buf, p, [0x48, 0x8B, 0x05], slot);
        buf[p + 7..p + 10].copy_from_slice(&[0x48, 0x85, 0xC0]);
        buf[p + 10..p + 12].copy_from_slice(&[0x75, 0x10]);
        p + 12
    }

    const DATA: Range<u32> = 0x2000..0x2800;

    /// Call-site shape for FD4DerivedSingleton (see candidates_iter / derived_singleton_pat_iter).
    fn put_derived(buf: &mut [u8], p: usize, slot: usize, name: usize) {
        let w = put_prologue(buf, p, slot);
        put_rip(buf, w, LEA_RCX, 0x2600);
        put_rip(buf, w + 7, LEA_R8, 0x2610);
        buf[w + 14] = 0xBA; // mov edx, 0x30
        buf[w + 15..w + 19].copy_from_slice(&0x30u32.to_le_bytes());
        put_rip(buf, w + 19, LEA_R9, name);
        buf[w + 26] = 0xE8; // call (ends the forward window); disp irrelevant
    }

    /// Call-site shape for FD4Singleton (see fd4_singleton_pat_iter).
    fn put_fd4(buf: &mut [u8], p: usize, slot: usize, refl: usize, getter: usize) {
        let w = put_prologue(buf, p, slot);
        put_rip(buf, w, LEA_RCX, refl);
        put_call(buf, w + 7, getter);
        let win = w + 12;
        put_rip(buf, win, LEA_RCX, 0x2620);
        buf[win + 7..win + 10].copy_from_slice(&[0x4C, 0x8B, 0xC8]); // mov r9, rax
        buf[win + 10] = 0xBA;
        buf[win + 11..win + 15].copy_from_slice(&0x30u32.to_le_bytes());
        put_rip(buf, win + 15, LEA_R8, 0x2630);
        buf[win + 22] = 0xE8;
    }

    fn image() -> Vec<u8> {
        vec![0u8; 0x3000]
    }

    #[test]
    fn finds_derived_singleton() {
        let mut buf = image();
        put_derived(&mut buf, 0x100, 0x2008, 0x2400);
        let base = buf.as_ptr() as usize;
        let mut raw = RawScan::default();
        let st = scan_text(&buf[0x100..0x400], base, &[DATA], &mut raw);
        assert_eq!(raw.derived.get(&0x2008), Some(&0x2400));
        assert_eq!(st.derived_accepted, 1);
        assert!(raw.fd4.is_empty());
    }

    #[test]
    fn finds_fd4_singleton_and_getter() {
        let mut buf = image();
        put_fd4(&mut buf, 0x100, 0x2010, 0x2500, 0x300);
        put_fd4(&mut buf, 0x200, 0x2018, 0x2508, 0x300);
        // One outlier call site with a different "getter".
        put_fd4(&mut buf, 0x280, 0x2020, 0x2510, 0x340);
        let base = buf.as_ptr() as usize;
        let mut raw = RawScan::default();
        let st = scan_text(&buf[0x100..0x400], base, &[DATA], &mut raw);
        assert_eq!(st.fd4_accepted, 3);
        assert_eq!(raw.fd4.get(&0x2010), Some(&(0x2500, 0x300)));
        let scan = finalize(raw, &[0x100..0x400]);
        assert_eq!(scan.getter, Some((0x300, 2, 3)));
        assert_eq!(scan.fd4, vec![(0x2010, 0x2500), (0x2018, 0x2508)]);
    }

    #[test]
    fn handles_backward_getter_call() {
        // Getter located BEFORE the call site: negative displacement (stock driver mis-resolves this).
        let mut buf = image();
        put_fd4(&mut buf, 0x200, 0x2010, 0x2500, 0x120);
        let base = buf.as_ptr() as usize;
        let mut raw = RawScan::default();
        scan_text(&buf[0x100..0x400], base, &[DATA], &mut raw);
        assert_eq!(raw.fd4.get(&0x2010), Some(&(0x2500, 0x120)));
    }

    #[test]
    fn second_section_is_scanned_independently() {
        // Same technique over a second slice of the same image, as for NR's second `.text`.
        let mut buf = image();
        put_derived(&mut buf, 0x100, 0x2008, 0x2400);
        put_derived(&mut buf, 0x800, 0x2040, 0x2440);
        let base = buf.as_ptr() as usize;
        let mut raw = RawScan::default();
        scan_text(&buf[0x100..0x400], base, &[DATA], &mut raw);
        assert_eq!(raw.derived.len(), 1);
        scan_text(&buf[0x700..0xA00], base, &[DATA], &mut raw);
        assert_eq!(raw.derived.len(), 2);
        assert_eq!(raw.derived.get(&0x2040), Some(&0x2440));
    }

    #[test]
    fn slots_outside_data_are_rejected() {
        let mut buf = image();
        put_derived(&mut buf, 0x100, 0x2900, 0x2400); // slot beyond DATA
        put_derived(&mut buf, 0x200, 0x2009, 0x2400); // unaligned
        let base = buf.as_ptr() as usize;
        let mut raw = RawScan::default();
        let st = scan_text(&buf[0x100..0x400], base, &[DATA], &mut raw);
        assert!(raw.derived.is_empty());
        assert_eq!(st.derived_hits, 2);
        assert_eq!(st.derived_accepted, 0);
    }

    #[test]
    fn getter_must_be_executable() {
        let mut buf = image();
        put_fd4(&mut buf, 0x100, 0x2010, 0x2500, 0x300);
        let base = buf.as_ptr() as usize;
        let mut raw = RawScan::default();
        scan_text(&buf[0x100..0x400], base, &[DATA], &mut raw);
        let scan = finalize(raw, &[0x1000..0x2000]);
        assert!(scan.getter.is_none());
        assert!(scan.fd4.is_empty());
    }

    #[test]
    fn majority_vote() {
        assert_eq!(majority([]), None);
        assert_eq!(majority([5, 5, 7]), Some((5, 2)));
        assert_eq!(majority([9, 3, 3, 9]), Some((3, 2))); // tie -> lowest
        assert_eq!(majority([1]), Some((1, 1)));
    }

    #[test]
    fn target_arithmetic_wraps() {
        let mut b = [0u8; 16];
        b[4..8].copy_from_slice(&(-8i32).to_le_bytes());
        // Use a standalone buffer as the image so end rva = 8.
        let base_b = b.as_ptr() as usize;
        assert_eq!(target_rva(&b[4..8], base_b), Some(0));
        assert_eq!(target_rva(&b[4..7], base_b), None);
    }
}
