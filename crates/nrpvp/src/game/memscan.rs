//! A scan of the whole process for a byte pattern (writable and readable heap, mapped data; never the loaded images). Used by
//! [`super::multiplay`] to find the enemy-scaling table, and by the dev console `memscan` to find where the game keeps a string it
//! shows (the password fields of the Matchmaking Settings menu, RW-6). It runs on its own thread, reads 8 MiB at a time through
//! `ReadProcessMemory` (a failing page is skipped, never a fault) and sleeps between chunks, so it costs the game a few percent of one
//! core for about a minute (9-12 GiB of address space).

use std::ffi::c_void;
use std::mem::size_of;
use std::time::{Duration, Instant};

use windows::Win32::System::Memory::{
    VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_IMAGE,
};

use super::mem;

/// Bytes read per `ReadProcessMemory`.
const CHUNK: usize = 8 << 20;

/// What a scan saw.
#[derive(Debug, Default)]
pub(crate) struct Scan {
    /// Start addresses of every match (sorted, no duplicates), capped at `max_hits`.
    pub hits: Vec<usize>,
    pub regions: usize,
    pub bytes: usize,
    pub secs: f32,
}

/// Offsets of `needle` in `hay` (a scan on the first byte).
pub(crate) fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if needle.is_empty() || hay.len() < needle.len() {
        return out;
    }
    let first = needle[0];
    let last = hay.len() - needle.len();
    let mut i = 0;
    while i <= last {
        match hay[i..=last].iter().position(|b| *b == first) {
            None => break,
            Some(p) => {
                i += p;
                if &hay[i..i + needle.len()] == needle {
                    out.push(i);
                }
                i += 1;
            }
        }
    }
    out
}

/// Offsets of `needle` in `hay` that are multiples of `align` (1, or 4 for data the game stores as `s32` fields: one `u32` compare per
/// 4 bytes instead of a byte scan; several GiB per second against about one).
pub(crate) fn find_all_aligned(hay: &[u8], needle: &[u8], align: usize) -> Vec<usize> {
    if align != 4 || needle.len() < 4 {
        return find_all(hay, needle);
    }
    let head = u32::from_le_bytes([needle[0], needle[1], needle[2], needle[3]]);
    let mut out = Vec::new();
    for (i, w) in hay.as_chunks::<4>().0.iter().enumerate() {
        if u32::from_le_bytes(*w) == head && hay[i * 4..].starts_with(needle) {
            out.push(i * 4);
        }
    }
    out
}

/// `text` as UTF-16LE bytes (what the game's strings are).
pub(crate) fn utf16_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Scan the process for `needle`. `min_region` skips regions smaller than that. Our own read buffers are never reported (the heap
/// reuses them, so a match inside one is a copy of a copy).
pub(crate) fn scan(needle: &[u8], min_region: usize, max_hits: usize) -> Scan {
    scan_aligned(needle, min_region, max_hits, 1)
}

/// [`scan`] for a pattern that starts at a multiple of `align` (1 or 4) in memory; one read buffer is reused for every chunk.
pub(crate) fn scan_aligned(
    needle: &[u8],
    min_region: usize,
    max_hits: usize,
    align: usize,
) -> Scan {
    let started = Instant::now();
    let mut out = Scan::default();
    let mut buf = vec![0u8; CHUNK];
    let own: Vec<(usize, usize)> = vec![(buf.as_ptr() as usize, CHUNK)];
    let mut found: Vec<usize> = Vec::new();
    let mut addr: usize = 0x1_0000;
    loop {
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: `mbi` is a valid out-buffer of the size passed; VirtualQuery tolerates any address.
        let got = unsafe {
            VirtualQuery(
                Some(addr as *const c_void),
                &mut mbi,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if got == 0 {
            break;
        }
        let base = mbi.BaseAddress as usize;
        let end = base.saturating_add(mbi.RegionSize);
        if end <= addr {
            break;
        }
        if mbi.State == MEM_COMMIT
            && mbi.Type != MEM_IMAGE
            && mem::protect_readable(mbi.Protect.0)
            && mbi.RegionSize >= min_region.max(needle.len())
        {
            out.regions += 1;
            let mut p = base;
            while p < end {
                let len = CHUNK.min(end - p);
                if mem::copy_out(p, &mut buf[..len]) {
                    out.bytes += len;
                    found.extend(
                        find_all_aligned(&buf[..len], needle, align)
                            .into_iter()
                            .map(|o| p + o),
                    );
                }
                // Overlap the next chunk a little so a match across a chunk border is seen.
                p += if end - p > CHUNK { CHUNK - 64 } else { len };
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        addr = end;
    }
    found.sort_unstable();
    found.dedup();
    found.retain(|s| !own.iter().any(|(a, l)| *s >= *a && *s < a + l));
    found.truncate(max_hits);
    out.hits = found;
    out.secs = started.elapsed().as_secs_f32();
    out
}

static LAST: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Console `memscan` with no argument: the last result (or that one is running).
pub(crate) fn last_result() -> String {
    if RUNNING.load(std::sync::atomic::Ordering::Relaxed) {
        return "memscan: a scan is running".into();
    }
    let s = LAST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if s.is_empty() {
        "memscan: nothing scanned yet".into()
    } else {
        s.clone()
    }
}

/// Console `memscan <pattern>`: start a scan on its own thread; the hits are logged and kept for [`last_result`].
pub(crate) fn start_console_scan(needle: Vec<u8>) -> Result<String, String> {
    if RUNNING.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return Err("a scan is already running".into());
    }
    std::thread::Builder::new()
        .name("nrpvp-memscan".into())
        .spawn(move || {
            let r = scan(&needle, 0, 200);
            let mut text = format!(
                "memscan: {} byte pattern: {} hit(s) in {} regions ({} MiB, {:.1} s)",
                needle.len(),
                r.hits.len(),
                r.regions,
                r.bytes >> 20,
                r.secs
            );
            tracing::info!("{text}");
            for h in &r.hits {
                let around = h.saturating_sub(16);
                let mut b = vec![0u8; 96];
                let got = mem::copy_out(around, &mut b);
                let hex: String = if got {
                    b.iter()
                        .map(|x| format!("{x:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    "unreadable".into()
                };
                let line = format!("memscan: hit {h:#X} (from {around:#X}): {hex}");
                tracing::info!("{line}");
                text.push('\n');
                text.push_str(&line);
            }
            *LAST
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = text;
            RUNNING.store(false, std::sync::atomic::Ordering::Relaxed);
        })
        .map_err(|e| e.to_string())?;
    Ok("memscan: started; results in the log (`memscan` shows them)".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needles_are_found_at_every_offset() {
        let mut hay = vec![0xAAu8; 100];
        hay[10..14].copy_from_slice(&[1, 2, 3, 4]);
        hay[50..54].copy_from_slice(&[1, 2, 3, 4]);
        hay[98..100].copy_from_slice(&[1, 2]);
        assert_eq!(find_all(&hay, &[1, 2, 3, 4]), vec![10, 50]);
        assert!(find_all(&hay[..3], &[1, 2, 3, 4]).is_empty());
        assert!(find_all(&hay, &[]).is_empty());
    }

    #[test]
    fn text_becomes_utf16_little_endian() {
        assert_eq!(utf16_bytes("Ab"), vec![0x41, 0, 0x62, 0]);
    }

    #[test]
    fn aligned_search_finds_only_multiples_of_four() {
        let mut hay = vec![0xAAu8; 64];
        hay[8..12].copy_from_slice(&[1, 2, 3, 4]); // aligned
        hay[21..25].copy_from_slice(&[1, 2, 3, 4]); // not aligned: skipped
        hay[40..46].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(find_all_aligned(&hay, &[1, 2, 3, 4], 4), vec![8, 40]);
        assert_eq!(find_all_aligned(&hay, &[1, 2, 3, 4, 5, 6], 4), vec![40]);
        assert_eq!(
            find_all_aligned(&hay, &[1, 2, 3, 4], 1),
            find_all(&hay, &[1, 2, 3, 4]),
            "align 1 is the byte scan"
        );
    }
}
