//! Minimal PE section-table parsing for the loaded game image, plus the (pure) section-selection policy
//! used by the singleton scan.
//!
//! Why this exists: `from-singleton` 3.0.1 picks `.text` and `.data` with pelite's `by_name`, which
//! returns the FIRST section with that name (pelite 0.10.0 `src/wrap/sections.rs#L93-L110`). NR's
//! `nightreign.exe` has TWO sections named `.text` (.claude/archive/er-reference.md section 1.3, [VERIFIED] PE headers
//! of NR 1.3.3.0), so the stock finder only ever scans the first one. We scan every executable section.

use std::ops::Range;

/// `IMAGE_SCN_CNT_CODE`
pub(crate) const SCN_CNT_CODE: u32 = 0x0000_0020;
/// `IMAGE_SCN_MEM_EXECUTE`
pub(crate) const SCN_MEM_EXECUTE: u32 = 0x2000_0000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Section {
    pub name: String,
    pub rva: u32,
    pub vsize: u32,
    pub characteristics: u32,
}

impl Section {
    pub(crate) fn is_executable(&self) -> bool {
        self.characteristics & (SCN_MEM_EXECUTE | SCN_CNT_CODE) != 0
    }

    pub(crate) fn range(&self) -> Range<u32> {
        self.rva..self.rva.saturating_add(self.vsize)
    }
}

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

/// Parse the section table from the first bytes of a PE image (DOS header, NT headers, section table).
/// Returns `None` if the headers are malformed or truncated.
pub(crate) fn parse_sections(hdr: &[u8]) -> Option<Vec<Section>> {
    if hdr.get(0..2)? != b"MZ" {
        return None;
    }
    let nt = u32_at(hdr, 0x3C)? as usize;
    if hdr.get(nt..nt + 4)? != b"PE\0\0" {
        return None;
    }
    let count = u16_at(hdr, nt + 6)? as usize;
    let opt_size = u16_at(hdr, nt + 20)? as usize;
    let table = nt.checked_add(24)?.checked_add(opt_size)?;
    if count == 0 || count > 96 {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let s = table.checked_add(i * 40)?;
        let raw_name = hdr.get(s..s + 8)?;
        let end = raw_name.iter().position(|&b| b == 0).unwrap_or(8);
        out.push(Section {
            name: String::from_utf8_lossy(&raw_name[..end]).into_owned(),
            vsize: u32_at(hdr, s + 8)?,
            rva: u32_at(hdr, s + 12)?,
            characteristics: u32_at(hdr, s + 36)?,
        });
    }
    Some(out)
}

/// Sections whose code we scan for singleton call sites: every executable one, in table order.
pub(crate) fn scan_sections(all: &[Section]) -> Vec<&Section> {
    all.iter().filter(|s| s.is_executable()).collect()
}

/// RVA ranges in which a singleton's static pointer slot may live: every section named `.data`
/// (from-singleton requires the slot to be inside `.data`; find.rs#L66-L70 in 3.0.1).
pub(crate) fn data_ranges(all: &[Section]) -> Vec<Range<u32>> {
    all.iter()
        .filter(|s| s.name == ".data")
        .map(Section::range)
        .collect()
}

/// Sections that can hold MSVC RTTI (TypeDescriptors live in `.data`, COLs/hierarchies/vtables in
/// `.rdata`): non-executable sections named `.rdata`, `.data` or `_RDATA`.
pub(crate) fn rtti_sections(all: &[Section]) -> Vec<&Section> {
    all.iter()
        .filter(|s| !s.is_executable() && matches!(s.name.as_str(), ".rdata" | ".data" | "_RDATA"))
        .collect()
}

/// True if `rva` falls inside any of `ranges`.
pub(crate) fn in_ranges(ranges: &[Range<u32>], rva: u32) -> bool {
    ranges.iter().any(|r| r.contains(&rva))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn put_u16(b: &mut [u8], off: usize, v: u16) {
        b[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put_u32(b: &mut [u8], off: usize, v: u32) {
        b[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// Build a header blob with the given (name, rva, vsize, characteristics) section table.
    pub(crate) fn make_headers(secs: &[(&str, u32, u32, u32)]) -> Vec<u8> {
        let mut b = vec![0u8; 0x1000];
        b[0..2].copy_from_slice(b"MZ");
        put_u32(&mut b, 0x3C, 0x80);
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        put_u16(&mut b, 0x80 + 6, secs.len() as u16);
        put_u16(&mut b, 0x80 + 20, 0xF0); // SizeOfOptionalHeader (PE32+)
        let table = 0x80 + 24 + 0xF0;
        for (i, (name, rva, vsize, ch)) in secs.iter().enumerate() {
            let s = table + i * 40;
            b[s..s + name.len()].copy_from_slice(name.as_bytes());
            put_u32(&mut b, s + 8, *vsize);
            put_u32(&mut b, s + 12, *rva);
            put_u32(&mut b, s + 36, *ch);
        }
        b
    }

    const CODE_X: u32 = 0x6000_0020; // CODE | EXECUTE | READ
    const DATA_RW: u32 = 0xC000_0040; // INITIALIZED_DATA | READ | WRITE
    const RDATA_R: u32 = 0x4000_0040;

    /// The section table of NR 1.3.3.0 as recorded in .claude/archive/er-reference.md section 1.3 (sizes where the doc
    /// gives them; remaining characteristics are plausible values, only the executable ones matter).
    pub(crate) fn nr_like() -> Vec<(&'static str, u32, u32, u32)> {
        vec![
            (".text", 0x1000, 0x2BB3C00, CODE_X),
            (".interpr", 0x2BB5000, 0xCE00, CODE_X),
            (".rdata", 0x2BC2000, 0x7C3000, RDATA_R),
            (".data", 0x3985000, 0xD87D0C, DATA_RW),
            (".pdata", 0x470D000, 0x100000, RDATA_R),
            ("_RDATA", 0x480D000, 0x1000, RDATA_R),
            (".rsrc", 0x480E000, 0x100000, RDATA_R),
            (".reloc", 0x490E000, 0x186000, 0x4200_0040),
            (".text", 0x4A94000, 0x31C9600, CODE_X),
        ]
    }

    #[test]
    fn parses_nr_like_table() {
        let secs = parse_sections(&make_headers(&nr_like())).expect("parse");
        assert_eq!(secs.len(), 9);
        assert_eq!(secs[0].name, ".text");
        assert_eq!(secs[8].name, ".text");
        assert_eq!(secs[8].rva, 0x4A94000);
        assert_eq!(secs[8].range().end, 0x4A94000 + 0x31C9600);
    }

    #[test]
    fn scans_both_text_sections_and_interpr() {
        let secs = parse_sections(&make_headers(&nr_like())).expect("parse");
        let scan: Vec<(&str, u32)> = scan_sections(&secs)
            .iter()
            .map(|s| (s.name.as_str(), s.rva))
            .collect();
        assert_eq!(
            scan,
            vec![
                (".text", 0x1000),
                (".interpr", 0x2BB5000),
                (".text", 0x4A94000)
            ]
        );
    }

    #[test]
    fn data_ranges_only_dot_data() {
        let secs = parse_sections(&make_headers(&nr_like())).expect("parse");
        let r = data_ranges(&secs);
        assert_eq!(r, vec![0x3985000..0x3985000 + 0xD87D0C]);
        assert!(in_ranges(&r, 0x3985000));
        assert!(!in_ranges(&r, 0x3985000 + 0xD87D0C));
        assert!(!in_ranges(&r, 0x1000));
    }

    #[test]
    fn rtti_sections_are_rdata_and_data() {
        let secs = parse_sections(&make_headers(&nr_like())).expect("parse");
        let names: Vec<&str> = rtti_sections(&secs)
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, vec![".rdata", ".data", "_RDATA"]);
    }

    #[test]
    fn rejects_malformed_headers() {
        assert!(parse_sections(&[]).is_none());
        assert!(parse_sections(b"MZ").is_none());
        let mut h = make_headers(&nr_like());
        h[0x80] = b'X';
        assert!(parse_sections(&h).is_none());
        let mut h = make_headers(&nr_like());
        put_u16(&mut h, 0x80 + 6, 0);
        assert!(parse_sections(&h).is_none());
        // Truncated inside the section table.
        let h = make_headers(&nr_like());
        assert!(parse_sections(&h[..0x80 + 24 + 0xF0 + 50]).is_none());
    }
}
