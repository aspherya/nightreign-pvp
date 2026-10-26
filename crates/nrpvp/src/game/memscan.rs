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

/// `text` as UTF-16LE bytes (what the game's strings are).
pub(crate) fn utf16_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Scan the process for `needle`. `min_region` skips regions smaller than that. Our own read buffers are never reported (the heap
/// reuses them, so a match inside one is a copy of a copy).
pub(crate) fn scan(needle: &[u8], min_region: usize, max_hits: usize) -> Scan {
    let started = Instant::now();
    let mut out = Scan::default();
    let mut own: Vec<(usize, usize)> = Vec::new();
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
                let mut buf = vec![0u8; len];
                own.push((buf.as_ptr() as usize, len));
                if mem::copy_out(p, &mut buf) {
                    out.bytes += len;
                    found.extend(find_all(&buf, needle).into_iter().map(|o| p + o));
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
}
