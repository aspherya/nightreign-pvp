//! T-032 (a): MARK dumps (dev mode only, read-only).
//!
//! On every MARK the main loop starts one job on the discovery worker (the existing job gate; if the worker is busy the dump is
//! skipped with a log line). For every ChrSet player (LOCAL and REMOTE) the job logs `PlayerIns+0x000..0x1000` and the first
//! 0x200 bytes of each one-level pointee (8-aligned pointers in that range to readable memory outside the exe image, at most 64:
//! the same selection as the T-031 position scan, `probe_scan::collect_pointees`), as hex lines of 32 bytes each, under one header
//! line per region. Opus diffs the dumps of marks taken while the rival stands and while the rival is downed to find the
//! downed marker. Nothing here is interpreted; the RTTI class names are only labels.
//!
//! Strictly read-only: guarded reads through [`RegionCache`] (one cache per job), no writes, no calls into game functions.

use std::fmt::Write as _;
use std::time::Instant;

use super::mem::RegionCache;
use super::probe_pos::Target;
use super::probe_scan::{collect_pointees, Pointee, MAX_POINTEES, PLAYER_LEN, POINTEE_LEN};
use super::rtti::Rtti;

/// Bytes per hex line.
pub(crate) const LINE_BYTES: usize = 32;
/// ChrIns per-player id [VERIFIED: FS-0/FS-1 2026-10-10, TECHNICAL_NOTES section 4].
const CHR_PLAYER_ID: usize = 0x78;

/// Which region a header describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Region {
    /// The PlayerIns itself.
    Player,
    /// An object a PlayerIns pointer leads to: pointer offset, address.
    Pointee { poff: u16, addr: usize },
}

/// `vtable=exe+0x2C0A898 class=CS::Foo`, or `vtable=none class=?` when the first qword is not an exe address. `class` = the RTTI
/// name when the vtable is known to RTTI (`None` prints `?`).
pub(crate) fn vtable_fields(
    first_qword: Option<usize>,
    base: usize,
    size: usize,
    class: Option<&str>,
) -> String {
    match first_qword.filter(|&v| v >= base && v - base < size) {
        Some(v) => format!("vtable=exe+0x{:X} class={}", v - base, class.unwrap_or("?")),
        None => "vtable=none class=?".to_string(),
    }
}

/// `T-032 DUMP mark=#3 who=REMOTE[1] id=0x6002 region=PlayerIns vtable=... addr=0x.. len=0x1000`, or for a pointee
/// `... region=[PlayerIns+0x5F0] vtable=exe+0x... class=... addr=0x.. len=0x200`.
pub(crate) fn header(
    mark: u32,
    who: &str,
    id: Option<u64>,
    region: &Region,
    vtable: &str,
    addr: usize,
    len: usize,
) -> String {
    let id = id.map_or("?".to_string(), |v| format!("0x{v:X}"));
    let region = match region {
        Region::Player => "PlayerIns".to_string(),
        Region::Pointee { poff, .. } => format!("[PlayerIns+0x{poff:X}]"),
    };
    format!(
        "T-032 DUMP mark=#{mark} who={who} id={id} region={region} {vtable} addr=0x{addr:X} len=0x{len:X}"
    )
}

/// Hex lines of [`LINE_BYTES`] bytes each: `T-032 DUMP +0x0040: 00 11 .. ff`. The offset is relative to the region start. A short
/// last line is not padded.
pub(crate) fn hex_lines(bytes: &[u8]) -> Vec<String> {
    bytes
        .chunks(LINE_BYTES)
        .enumerate()
        .map(|(i, chunk)| {
            let mut s = format!("T-032 DUMP +0x{:04X}:", i * LINE_BYTES);
            for b in chunk {
                let _ = write!(s, " {b:02X}");
            }
            s
        })
        .collect()
}

fn first_qword(bytes: &[u8]) -> Option<usize> {
    let raw: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
    Some(u64::from_le_bytes(raw) as usize)
}

/// Who a dump belongs to.
pub(crate) struct Subject<'a> {
    pub mark: u32,
    pub who: &'a str,
    pub id: Option<u64>,
    pub addr: usize,
}

/// The whole dump of one player as log lines (headers and hex), from already-read bytes. `class_of` maps a vtable VA to its RTTI
/// class name; `base`/`size` are the exe image.
pub(crate) fn player_lines(
    s: &Subject<'_>,
    player: &[u8],
    pointees: &[Pointee],
    image: (usize, usize),
    class_of: &dyn Fn(usize) -> Option<String>,
) -> Vec<String> {
    let (base, size) = image;
    let mut out = Vec::new();
    let vt = |bytes: &[u8]| {
        let q = first_qword(bytes);
        let class = q.and_then(class_of);
        vtable_fields(q, base, size, class.as_deref())
    };
    out.push(header(
        s.mark,
        s.who,
        s.id,
        &Region::Player,
        &vt(player),
        s.addr,
        player.len(),
    ));
    out.extend(hex_lines(player));
    for p in pointees {
        out.push(header(
            s.mark,
            s.who,
            s.id,
            &Region::Pointee {
                poff: p.poff,
                addr: p.addr,
            },
            &vt(&p.bytes),
            p.addr,
            p.bytes.len(),
        ));
        out.extend(hex_lines(&p.bytes));
    }
    out
}

/// The worker job: dump every target. `targets` come from a fresh quiet resolve on the main thread.
pub(crate) fn run(rtti: &Rtti, mark: u32, targets: &[Target], base: usize, size: usize) {
    let t0 = Instant::now();
    tracing::info!(
        "T-032 DUMP start mark=#{mark}: {} player(s), PlayerIns+0..0x{PLAYER_LEN:X} and up to {MAX_POINTEES} pointees of 0x{POINTEE_LEN:X} bytes each",
        targets.len()
    );
    let class_of = |vt: usize| rtti.class_of_vtable(vt).map(|c| rtti.name(c));
    let mut rc = RegionCache::new();
    let mut lines = 0usize;
    for t in targets {
        let who = t.label();
        let player = rc.read_prefix(t.addr, PLAYER_LEN);
        if player.is_empty() {
            tracing::warn!(
                "T-032 DUMP mark=#{mark} who={who}: PlayerIns 0x{:X} is unreadable",
                t.addr
            );
            continue;
        }
        let id = rc.read::<u64>(t.addr + CHR_PLAYER_ID);
        let pointees = collect_pointees(&mut rc, &player, base, size);
        let subject = Subject {
            mark,
            who: &who,
            id,
            addr: t.addr,
        };
        for l in player_lines(&subject, &player, &pointees, (base, size), &class_of) {
            lines += 1;
            tracing::info!("{l}");
        }
    }
    tracing::info!(
        "T-032 DUMP done mark=#{mark}: {lines} line(s) in {} ms",
        t0.elapsed().as_millis()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMG: usize = 0x7FF6_0000_0000;
    const IMG_SIZE: usize = 0x800_0000;

    #[test]
    fn hex_lines_are_32_bytes_with_the_offset_and_a_short_tail() {
        let bytes: Vec<u8> = (0..70u8).collect();
        let l = hex_lines(&bytes);
        assert_eq!(l.len(), 3);
        assert!(l[0].starts_with("T-032 DUMP +0x0000: 00 01 02"), "{}", l[0]);
        assert!(l[0].ends_with(" 1E 1F"), "{}", l[0]);
        // "T-032 DUMP +0x0000:" has 2 spaces, then one space per byte.
        assert_eq!(l[0].matches(' ').count(), 2 + 32);
        assert!(l[1].starts_with("T-032 DUMP +0x0020: 20 21"), "{}", l[1]);
        assert_eq!(l[2], "T-032 DUMP +0x0040: 40 41 42 43 44 45");
        assert!(hex_lines(&[]).is_empty());
        // A whole PlayerIns is 128 lines.
        assert_eq!(hex_lines(&vec![0u8; PLAYER_LEN]).len(), 0x1000 / 32);
    }

    #[test]
    fn headers_name_the_region_the_vtable_and_the_class() {
        let h = header(
            3,
            "REMOTE[1]",
            Some(0x6002),
            &Region::Player,
            &vtable_fields(Some(IMG + 0x2C307F8), IMG, IMG_SIZE, Some("CS::PlayerIns")),
            0x1234_5000,
            0x1000,
        );
        assert_eq!(
            h,
            "T-032 DUMP mark=#3 who=REMOTE[1] id=0x6002 region=PlayerIns vtable=exe+0x2C307F8 class=CS::PlayerIns addr=0x12345000 len=0x1000"
        );
        let h = header(
            4,
            "LOCAL[0]",
            None,
            &Region::Pointee {
                poff: 0x5F0,
                addr: 0x2000_0000,
            },
            &vtable_fields(Some(IMG + 0x2C0A898), IMG, IMG_SIZE, None),
            0x2000_0000,
            0x200,
        );
        assert_eq!(
            h,
            "T-032 DUMP mark=#4 who=LOCAL[0] id=? region=[PlayerIns+0x5F0] vtable=exe+0x2C0A898 class=? addr=0x20000000 len=0x200"
        );
        // A first qword outside the image (or missing) is "none".
        assert_eq!(
            vtable_fields(Some(0x1234), IMG, IMG_SIZE, Some("x")),
            "vtable=none class=?"
        );
        assert_eq!(
            vtable_fields(None, IMG, IMG_SIZE, None),
            "vtable=none class=?"
        );
    }

    #[test]
    fn a_player_dump_is_header_hex_then_each_pointee() {
        let mut player = vec![0u8; PLAYER_LEN];
        player[..8].copy_from_slice(&((IMG + 0x2C307F8) as u64).to_le_bytes());
        let mut pt = vec![0u8; POINTEE_LEN];
        pt[..8].copy_from_slice(&((IMG + 0x2C8FD20) as u64).to_le_bytes());
        let pointees = vec![Pointee {
            poff: 0x6B0,
            addr: 0x3000_0000,
            bytes: pt,
        }];
        let class_of = |vt: usize| (vt == IMG + 0x2C8FD20).then(|| "CS::Thing".to_string());
        let subject = Subject {
            mark: 7,
            who: "LOCAL[0]",
            id: Some(0x6001),
            addr: 0x1000_0000,
        };
        let lines = player_lines(&subject, &player, &pointees, (IMG, IMG_SIZE), &class_of);
        assert_eq!(lines.len(), 2 + 128 + 16);
        assert!(
            lines[0].contains("region=PlayerIns vtable=exe+0x2C307F8 class=?"),
            "{}",
            lines[0]
        );
        assert!(
            lines[1].starts_with("T-032 DUMP +0x0000: F8 07 C3"),
            "{}",
            lines[1]
        );
        assert!(
            lines[129].contains("region=[PlayerIns+0x6B0] vtable=exe+0x2C8FD20 class=CS::Thing"),
            "{}",
            lines[129]
        );
        assert!(lines.iter().all(|l| l.starts_with("T-032 DUMP")));
    }
}
