//! MSVC x64 RTTI recovery over a snapshot of the game image's data sections (T-007a step 1).
//!
//! Pure logic over byte snapshots (`Region`), so it is unit-tested on synthetic images. Layouts (x64 MSVC,
//! all "pointers" are image-relative u32 RVAs):
//! - TypeDescriptor: `{ pVFTable u64, spare u64, char name[] }`, name starts with `.?AV` (class) or `.?AU`
//!   (struct). The name is therefore at TD+0x10.
//! - CompleteObjectLocator (signature 1 on x64): `{ sig u32, offset u32, cdOffset u32, pTypeDescriptor u32,
//!   pClassDescriptor u32, pSelf u32 }`.
//! - ClassHierarchyDescriptor: `{ sig u32, attributes u32, numBaseClasses u32, pBaseClassArray u32 }`; each
//!   array element is the RVA of a BaseClassDescriptor whose first u32 is a TypeDescriptor RVA.
//! - vtable: the qword just before it holds the VA of the COL (so vtable = address of that qword + 8).
//!
//! Source: MSVC ABI as documented by the task card (T-007a); the layouts are exercised by the unit tests
//! below, the NR 1.3.3.0 image itself is validated by the HUMAN run.

use std::collections::HashMap;

/// TypeDescriptor raw names we need (x64 MSVC decoration). [T-006 verified these strings exist in NR 1.3.3.0]
pub(crate) const CHR_INS: &str = ".?AVChrIns@CS@@";
pub(crate) const PLAYER_INS: &str = ".?AVPlayerIns@CS@@";
pub(crate) const WORLD_CHR_MAN: &str = ".?AVWorldChrManImp@CS@@";

/// A snapshot of one PE section: `bytes[i]` is the byte at image RVA `rva + i`.
pub(crate) struct Region {
    pub rva: u32,
    pub bytes: Vec<u8>,
}

fn le32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(off..off.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn le64(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        b.get(off..off.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// u32 at image RVA `rva`, looked up across all regions.
fn rd32(regions: &[Region], rva: u32) -> Option<u32> {
    regions.iter().find_map(|r| {
        let off = rva.checked_sub(r.rva)? as usize;
        le32(&r.bytes, off)
    })
}

/// One class as seen through a primary (offset 0) CompleteObjectLocator.
#[derive(Clone, Debug)]
pub(crate) struct Class {
    /// Raw decorated TypeDescriptor name, e.g. `.?AVPlayerIns@CS@@`.
    pub name: String,
    pub td_rva: u32,
    pub col_rva: u32,
    /// RVAs of every vtable whose COL is this one (usually exactly one).
    pub vtables: Vec<u32>,
    /// TypeDescriptor RVAs of the class's BaseClassArray (includes the class itself, per MSVC).
    pub bases: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stats {
    /// Plausible TypeDescriptors found (name starts `.?AV`/`.?AU`, ends `@@`).
    pub type_descriptors: usize,
    /// COLs (signature 1, self-referential) pointing at a found TypeDescriptor, any offset.
    pub cols_all: usize,
    /// Of those, with offset 0 (the primary vtable).
    pub cols_primary: usize,
    /// Primary classes for which at least one vtable was located.
    pub classes_with_vtable: usize,
}

pub(crate) struct Rtti {
    pub classes: Vec<Class>,
    pub stats: Stats,
    by_vtable: HashMap<usize, usize>,
    chr: Vec<bool>,
    player: Vec<bool>,
    chrset: Vec<bool>,
}

impl Rtti {
    /// Build the lookup tables from classified classes (also used by tests to fake an RTTI).
    pub(crate) fn from_classes(base: usize, classes: Vec<Class>, stats: Stats) -> Self {
        let tds_named = |name: &str| -> Vec<u32> {
            classes
                .iter()
                .filter(|c| c.name == name)
                .map(|c| c.td_rva)
                .collect()
        };
        let chr_tds = tds_named(CHR_INS);
        let player_tds = tds_named(PLAYER_INS);
        let derives = |c: &Class, tds: &[u32]| {
            !tds.is_empty() && (tds.contains(&c.td_rva) || c.bases.iter().any(|b| tds.contains(b)))
        };
        let chr: Vec<bool> = classes.iter().map(|c| derives(c, &chr_tds)).collect();
        let player: Vec<bool> = classes.iter().map(|c| derives(c, &player_tds)).collect();
        let chrset: Vec<bool> = classes.iter().map(|c| c.name.contains("ChrSet")).collect();
        let mut by_vtable = HashMap::new();
        for (i, c) in classes.iter().enumerate() {
            for &vt in &c.vtables {
                by_vtable.entry(base + vt as usize).or_insert(i);
            }
        }
        Rtti {
            classes,
            stats,
            by_vtable,
            chr,
            player,
            chrset,
        }
    }

    /// Class whose primary vtable is exactly at VA `vt`.
    pub(crate) fn class_of_vtable(&self, vt: usize) -> Option<usize> {
        self.by_vtable.get(&vt).copied()
    }

    /// T-032: every known vtable VA with its readable class name (labels for the dev dumps and the position scan).
    pub(crate) fn vtable_names(&self) -> HashMap<usize, String> {
        self.by_vtable
            .iter()
            .map(|(&vt, &c)| (vt, self.name(c)))
            .collect()
    }

    /// Derives from (or is) `ChrIns`.
    pub(crate) fn is_chr(&self, class: usize) -> bool {
        self.chr.get(class).copied().unwrap_or(false)
    }

    /// Derives from (or is) `PlayerIns`.
    pub(crate) fn is_player(&self, class: usize) -> bool {
        self.player.get(class).copied().unwrap_or(false)
    }

    /// Name contains `ChrSet`.
    pub(crate) fn is_chrset(&self, class: usize) -> bool {
        self.chrset.get(class).copied().unwrap_or(false)
    }

    pub(crate) fn chr_class_count(&self) -> usize {
        self.chr.iter().filter(|&&b| b).count()
    }

    /// Readable name, e.g. `CS::PlayerIns`.
    pub(crate) fn name(&self, class: usize) -> String {
        self.classes
            .get(class)
            .map(|c| pretty_name(&c.name))
            .unwrap_or_else(|| "?".to_string())
    }

    /// Readable name of a class by its TypeDescriptor RVA (for base-class lists).
    pub(crate) fn name_of_td(&self, td: u32) -> String {
        self.classes
            .iter()
            .find(|c| c.td_rva == td)
            .map(|c| pretty_name(&c.name))
            .unwrap_or_else(|| format!("td@0x{td:X}"))
    }

    /// Indices of classes with the exact raw TypeDescriptor name.
    pub(crate) fn find(&self, raw: &str) -> Vec<usize> {
        self.classes
            .iter()
            .enumerate()
            .filter(|(_, c)| c.name == raw)
            .map(|(i, _)| i)
            .collect()
    }
}

/// `.?AVPlayerIns@CS@@` -> `CS::PlayerIns`. Template and otherwise unusual names are returned undecorated
/// of the `.?AV`/`@@` frame but otherwise as is.
pub(crate) fn pretty_name(raw: &str) -> String {
    let inner = raw
        .strip_prefix(".?AV")
        .or_else(|| raw.strip_prefix(".?AU"))
        .unwrap_or(raw);
    let inner = inner.strip_suffix("@@").unwrap_or(inner);
    if inner.contains('?') {
        return inner.to_string();
    }
    let mut parts: Vec<&str> = inner.split('@').collect();
    parts.reverse();
    parts.join("::")
}

/// Parse `bytes` as a TypeDescriptor name: printable ASCII, NUL-terminated within 512 bytes, `.?AV`/`.?AU`
/// prefix, `@@` suffix.
fn parse_td_name(bytes: &[u8]) -> Option<String> {
    let window = &bytes[..bytes.len().min(513)];
    let nul = window.iter().position(|&b| b == 0)?;
    let s = &window[..nul];
    if s.len() < 8 || !s.iter().all(|&b| (0x21..0x7F).contains(&b)) {
        return None;
    }
    if !(s.starts_with(b".?AV") || s.starts_with(b".?AU")) || !s.ends_with(b"@@") {
        return None;
    }
    Some(String::from_utf8_lossy(s).into_owned())
}

/// Every plausible TypeDescriptor: TD RVA -> raw name. The name sits at TD+0x10 and TDs are 8-aligned.
fn find_type_descriptors(regions: &[Region]) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    for r in regions {
        let b = &r.bytes;
        let mut i = 0usize;
        while i + 4 <= b.len() {
            let Some(p) = b[i..].iter().position(|&c| c == b'.') else {
                break;
            };
            i += p;
            if i + 4 > b.len() {
                break;
            }
            if b[i + 1] == b'?' && b[i + 2] == b'A' && (b[i + 3] == b'V' || b[i + 3] == b'U') {
                let name_rva = r.rva as u64 + i as u64;
                if i >= 0x10 && name_rva.is_multiple_of(8) {
                    if let Some(name) = parse_td_name(&b[i..]) {
                        out.insert((name_rva - 0x10) as u32, name);
                    }
                }
            }
            i += 1;
        }
    }
    out
}

struct Col {
    rva: u32,
    offset: u32,
    td: u32,
    chd: u32,
}

/// COLs: signature 1 at +0, `pSelf` (+0x14) equal to the COL's own RVA, `pTypeDescriptor` (+0x0C) a TD we
/// found. 4-aligned.
fn find_cols(regions: &[Region], tds: &HashMap<u32, String>) -> Vec<Col> {
    let mut out = Vec::new();
    for r in regions {
        let b = &r.bytes;
        let mut i = ((4 - (r.rva % 4)) % 4) as usize;
        while i + 0x18 <= b.len() {
            if le32(b, i) == Some(1) {
                let rva = r.rva.wrapping_add(i as u32);
                if le32(b, i + 0x14) == Some(rva) {
                    if let (Some(td), Some(offset), Some(chd)) =
                        (le32(b, i + 0x0C), le32(b, i + 4), le32(b, i + 0x10))
                    {
                        if tds.contains_key(&td) {
                            out.push(Col {
                                rva,
                                offset,
                                td,
                                chd,
                            });
                        }
                    }
                }
            }
            i += 4;
        }
    }
    out
}

/// BaseClassArray -> TypeDescriptor RVAs. Empty if anything is out of range.
fn parse_bases(regions: &[Region], chd: u32) -> Vec<u32> {
    let (Some(n), Some(arr)) = (
        chd.checked_add(8).and_then(|a| rd32(regions, a)),
        chd.checked_add(0xC).and_then(|a| rd32(regions, a)),
    ) else {
        return Vec::new();
    };
    if n == 0 || n > 256 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n as usize);
    for k in 0..n {
        let Some(bcd) = arr.checked_add(k * 4).and_then(|a| rd32(regions, a)) else {
            break;
        };
        match rd32(regions, bcd) {
            Some(td) => out.push(td),
            None => break,
        }
    }
    out
}

/// Recover all classes with a primary vtable from the data-section snapshots. `image_base` is the exe's
/// load address (vtable slots hold absolute VAs of COLs).
pub(crate) fn analyze(regions: &[Region], image_base: usize) -> Rtti {
    let tds = find_type_descriptors(regions);
    let cols = find_cols(regions, &tds);
    let mut stats = Stats {
        type_descriptors: tds.len(),
        cols_all: cols.len(),
        ..Stats::default()
    };

    // Primary COLs -> classes.
    let mut classes: Vec<Class> = Vec::new();
    let mut col_to_class: HashMap<u64, usize> = HashMap::new();
    for c in cols.iter().filter(|c| c.offset == 0) {
        let Some(name) = tds.get(&c.td) else { continue };
        col_to_class.insert(image_base as u64 + c.rva as u64, classes.len());
        classes.push(Class {
            name: name.clone(),
            td_rva: c.td,
            col_rva: c.rva,
            vtables: Vec::new(),
            bases: parse_bases(regions, c.chd),
        });
    }
    stats.cols_primary = classes.len();

    // vtables: an 8-aligned qword equal to a primary COL's VA; the vtable starts 8 bytes later.
    for r in regions {
        let b = &r.bytes;
        let mut i = ((8 - (r.rva % 8)) % 8) as usize;
        while i + 8 <= b.len() {
            if let Some(v) = le64(b, i) {
                if v >= image_base as u64 {
                    if let Some(&ci) = col_to_class.get(&v) {
                        let vt = (r.rva as u64 + i as u64 + 8) as u32;
                        classes[ci].vtables.push(vt);
                    }
                }
            }
            i += 8;
        }
    }
    stats.classes_with_vtable = classes.iter().filter(|c| !c.vtables.is_empty()).count();
    Rtti::from_classes(image_base, classes, stats)
}

/// Diagnostics for the "found nothing" case: a few TD names plus counts. (Caller logs.)
pub(crate) fn sample_type_descriptors(regions: &[Region], max: usize) -> Vec<String> {
    let mut v: Vec<(u32, String)> = find_type_descriptors(regions).into_iter().collect();
    v.sort();
    v.into_iter()
        .take(max)
        .map(|(rva, n)| format!("td rva=0x{rva:X} {n}"))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const BASE: usize = 0x1_4000_0000;

    fn put32(b: &mut [u8], off: usize, v: u32) {
        b[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put64(b: &mut [u8], off: usize, v: u64) {
        b[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }

    /// Builds a synthetic `.data` (TypeDescriptors at rva 0x1000..) and `.rdata` (COL/CHD/BCD/vtables at
    /// rva 0x4000..).
    struct Img {
        data: Vec<u8>,
        rdata: Vec<u8>,
        next_td: usize,
        next_col: usize,
        next_misc: usize,
        next_vt: usize,
    }
    const DATA_RVA: u32 = 0x1000;
    const RDATA_RVA: u32 = 0x4000;

    impl Img {
        fn new() -> Self {
            Img {
                data: vec![0; 0x1000],
                rdata: vec![0; 0x4000],
                next_td: 0x20,
                next_col: 0x000,
                next_misc: 0x1000,
                next_vt: 0x2000,
            }
        }
        fn td(&mut self, name: &str) -> u32 {
            let off = self.next_td;
            self.next_td += 0x10 + (name.len() + 1).div_ceil(16) * 16;
            self.data[off + 0x10..off + 0x10 + name.len()].copy_from_slice(name.as_bytes());
            DATA_RVA + off as u32
        }
        /// Adds BCD + base array + CHD; returns the CHD RVA.
        fn chd(&mut self, base_tds: &[u32]) -> u32 {
            let mut bcds = Vec::new();
            for &td in base_tds {
                let o = self.next_misc;
                self.next_misc += 0x30;
                put32(&mut self.rdata, o, td);
                bcds.push(RDATA_RVA + o as u32);
            }
            let arr = self.next_misc;
            self.next_misc += 0x40;
            for (k, b) in bcds.iter().enumerate() {
                put32(&mut self.rdata, arr + k * 4, *b);
            }
            let chd = self.next_misc;
            self.next_misc += 0x20;
            put32(&mut self.rdata, chd + 8, base_tds.len() as u32);
            put32(&mut self.rdata, chd + 0xC, RDATA_RVA + arr as u32);
            RDATA_RVA + chd as u32
        }
        /// Adds a COL (+ optional vtable); returns (col_rva, vtable_rva).
        fn col(&mut self, td: u32, chd: u32, offset: u32, sig: u32, self_ok: bool) -> (u32, u32) {
            let o = self.next_col;
            self.next_col += 0x20;
            let rva = RDATA_RVA + o as u32;
            put32(&mut self.rdata, o, sig);
            put32(&mut self.rdata, o + 4, offset);
            put32(&mut self.rdata, o + 0xC, td);
            put32(&mut self.rdata, o + 0x10, chd);
            put32(
                &mut self.rdata,
                o + 0x14,
                if self_ok { rva } else { rva + 4 },
            );
            let v = self.next_vt;
            self.next_vt += 0x40;
            put64(&mut self.rdata, v, BASE as u64 + rva as u64);
            (rva, RDATA_RVA + v as u32 + 8)
        }
        fn regions(self) -> Vec<Region> {
            vec![
                Region {
                    rva: DATA_RVA,
                    bytes: self.data,
                },
                Region {
                    rva: RDATA_RVA,
                    bytes: self.rdata,
                },
            ]
        }
    }

    #[test]
    fn pretty_names() {
        assert_eq!(pretty_name(".?AVPlayerIns@CS@@"), "CS::PlayerIns");
        assert_eq!(pretty_name(".?AUFoo@@"), "Foo");
        assert_eq!(
            pretty_name(".?AV?$ChrSet@VPlayerIns@CS@@@CS@@"),
            "?$ChrSet@VPlayerIns@CS@@@CS"
        );
    }

    #[test]
    fn td_name_parsing() {
        assert_eq!(
            parse_td_name(b".?AVChrIns@CS@@\0junk").as_deref(),
            Some(".?AVChrIns@CS@@")
        );
        assert_eq!(parse_td_name(b".?AVChrIns@CS@\0"), None); // no @@
        assert_eq!(parse_td_name(b".?AVChrIns@CS@@"), None); // no NUL
        assert_eq!(parse_td_name(b"abc@@\0"), None);
        assert_eq!(parse_td_name(b".?AVCh\x01rIns@CS@@\0"), None); // non printable
    }

    #[test]
    fn finds_player_derived_from_chr() {
        let mut img = Img::new();
        let td_chr = img.td(CHR_INS);
        let td_player = img.td(PLAYER_INS);
        let td_enemy = img.td(".?AVEnemyIns@CS@@");
        let td_other = img.td(".?AVWorldChrManImp@CS@@");
        let td_fi = img.td(".?AVFieldIns@CS@@");

        let chd_chr = img.chd(&[td_chr, td_fi]);
        let chd_player = img.chd(&[td_player, td_chr, td_fi]);
        let chd_enemy = img.chd(&[td_enemy, td_chr, td_fi]);
        let chd_other = img.chd(&[td_other]);

        let (_, vt_chr) = img.col(td_chr, chd_chr, 0, 1, true);
        let (_, vt_player) = img.col(td_player, chd_player, 0, 1, true);
        let (_, vt_enemy) = img.col(td_enemy, chd_enemy, 0, 1, true);
        let (_, vt_other) = img.col(td_other, chd_other, 0, 1, true);

        let r = analyze(&img.regions(), BASE);
        assert_eq!(r.stats.type_descriptors, 5);
        assert_eq!(r.stats.cols_primary, 4);
        assert_eq!(r.stats.classes_with_vtable, 4);

        let ci = |vt: u32| r.class_of_vtable(BASE + vt as usize).expect("class");
        assert_eq!(r.name(ci(vt_player)), "CS::PlayerIns");
        assert!(r.is_chr(ci(vt_chr)));
        assert!(r.is_chr(ci(vt_player)));
        assert!(r.is_chr(ci(vt_enemy)));
        assert!(!r.is_chr(ci(vt_other)));
        assert!(r.is_player(ci(vt_player)));
        assert!(!r.is_player(ci(vt_enemy)));
        assert!(!r.is_player(ci(vt_chr)));
        assert_eq!(r.chr_class_count(), 3);
        assert_eq!(r.class_of_vtable(BASE + vt_player as usize + 8), None);
        assert_eq!(r.find(CHR_INS).len(), 1);
        assert_eq!(
            r.name_of_td(td_fi),
            "td@0x".to_string() + &format!("{td_fi:X}")
        );
    }

    #[test]
    fn rejects_bad_cols() {
        let mut img = Img::new();
        let td_chr = img.td(CHR_INS);
        let td_a = img.td(".?AVA@CS@@");
        let td_b = img.td(".?AVB@CS@@");
        let td_c = img.td(".?AVC@CS@@");
        let chd = img.chd(&[td_chr]);
        img.col(td_chr, chd, 0, 1, true); // good
        img.col(td_a, chd, 0, 0, true); // wrong signature (32-bit style)
        img.col(td_b, chd, 0, 1, false); // pSelf mismatch
        img.col(td_c, chd, 8, 1, true); // non-primary (offset 8): counted, but not a class
        let r = analyze(&img.regions(), BASE);
        assert_eq!(r.stats.cols_all, 2);
        assert_eq!(r.stats.cols_primary, 1);
        assert_eq!(r.classes.len(), 1);
        assert_eq!(r.classes[0].name, CHR_INS);
    }

    #[test]
    fn vtable_without_col_reference_is_ignored_and_empty_image_is_ok() {
        let r = analyze(&[], BASE);
        assert_eq!(r.stats, Stats::default());
        assert!(r.classes.is_empty());
        assert_eq!(r.chr_class_count(), 0);
        assert!(!r.is_chr(0));

        // TD present but no COL: nothing found.
        let mut img = Img::new();
        img.td(CHR_INS);
        let r = analyze(&img.regions(), BASE);
        assert_eq!(r.stats.type_descriptors, 1);
        assert_eq!(r.stats.cols_all, 0);
    }

    #[test]
    fn garbage_hierarchy_is_tolerated() {
        let mut img = Img::new();
        let td_chr = img.td(CHR_INS);
        // CHD RVA points outside every region; another has an absurd base count.
        let (_, vt1) = img.col(td_chr, 0xDEAD_0000, 0, 1, true);
        let td_x = img.td(".?AVX@CS@@");
        let chd_big = img.chd(&[td_x]);
        // Corrupt numBaseClasses.
        let off = (chd_big - RDATA_RVA) as usize + 8;
        put32(&mut img.rdata, off, 100_000);
        let (_, vt2) = img.col(td_x, chd_big, 0, 1, true);
        let r = analyze(&img.regions(), BASE);
        assert_eq!(r.stats.cols_primary, 2);
        let c1 = r.class_of_vtable(BASE + vt1 as usize).expect("c1");
        let c2 = r.class_of_vtable(BASE + vt2 as usize).expect("c2");
        assert!(r.classes[c1].bases.is_empty());
        assert!(r.classes[c2].bases.is_empty());
        // ChrIns itself is still recognised through its own TD.
        assert!(r.is_chr(c1));
        assert!(!r.is_chr(c2));
    }

    #[test]
    fn chrset_detection_by_name() {
        let mut img = Img::new();
        let td = img.td(".?AV?$ChrSet@VPlayerIns@CS@@@CS@@");
        let chd = img.chd(&[td]);
        let (_, vt) = img.col(td, chd, 0, 1, true);
        let r = analyze(&img.regions(), BASE);
        let c = r.class_of_vtable(BASE + vt as usize).expect("class");
        assert!(r.is_chrset(c));
        assert!(!r.is_chr(c));
    }

    #[test]
    fn multiple_vtables_per_col_are_all_recorded() {
        let mut img = Img::new();
        let td = img.td(CHR_INS);
        let chd = img.chd(&[td]);
        let (col, vt1) = img.col(td, chd, 0, 1, true);
        // A second vtable slot referencing the same COL.
        let v2 = img.next_vt;
        img.next_vt += 0x40;
        put64(&mut img.rdata, v2, BASE as u64 + col as u64);
        let vt2 = RDATA_RVA + v2 as u32 + 8;
        let r = analyze(&img.regions(), BASE);
        assert_eq!(r.classes.len(), 1);
        assert_eq!(r.classes[0].vtables, vec![vt1, vt2]);
        assert_eq!(r.class_of_vtable(BASE + vt2 as usize), Some(0));
    }
}
