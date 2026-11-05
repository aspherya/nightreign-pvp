//! KF-1: a line of our own text as one of the game's toasts (the messages at the bottom right, `re/TOAST_KILLFEED.md`). Both builds.
//!
//! The game's toast function `exe+0x86833F(row, chr, kind, id, a5, a6)` shows the `UserDispLogParam` row `row`: with `chr` and `a5` null,
//! `kind` and `id` 0 and a row whose field `+0x10` is 0 it needs nothing else `[VERIFIED: static reading of exe 1.3.3.0]`. It allocates from
//! the calling thread's heap, so it must run on the game's own thread, as `game::itemdrop` found out: a call site the game runs every
//! frame (`exe+0x7B5DD4`, the menu update's call of the `CSFeMan` update) is pointed at [`hook_update`], which takes no hardware slot.
//!
//! The text: `gamedata/fmg-entries.json` reserves [`TEXT_IDS`] (long placeholders) in the menu text table. For one call the hook writes
//! the line over the placeholder of the next id and points row 6120 at that id, then puts the row back. Row 6120 is the "Ally is showing
//! location" toast, which is off in a match anyway (`game::toastfilter`, which owns the search for the table); the swap happens on the
//! game thread, the only one that shows toasts, so that toast can never get our text. Log lines start with `gametoast:`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{fmg, hudfilter, mem, toastfilter};

/// `(call site, its callee)`: the menu update's call of `CSFeMan`'s update (MEMORY_MAP §13).
const SITE_FE_UPDATE: (usize, usize) = (0x7B_5DD4, 0x7C_5710);
const RVA_TOAST: usize = 0x86_833F;
/// The toast function's first bytes (game 1.03.3, exe 1.3.3.0).
const TOAST_BYTES: [u8; 8] = [0x48, 0x48, 0x89, 0x5C, 0x24, 0x08, 0x48, 0x89];
/// The row the toast is shown from: see the module doc.
const ROW: u32 = 6120;
/// Row fields: the text id, and the icon (`<= 0` = none).
const ROW_TEXT: usize = 0;
const ROW_ICON: usize = 0x18;
/// Our placeholders in `gamedata/fmg-entries.json`, one per line on screen at a time: the game may read a text until its toast is gone.
pub(crate) const TEXT_IDS: [u32; 8] = [
    99_100_110, 99_100_111, 99_100_112, 99_100_113, 99_100_114, 99_100_115, 99_100_116, 99_100_117,
];
/// UTF-16 units of a placeholder, terminator included.
pub(crate) const TEXT_UNITS: usize = 120;
const PLACEHOLDER: u16 = b'_' as u16;
/// The categories of the game's text table.
const MAX_CATEGORY: u32 = 1024;
/// A line waits this long at most for the table scan of `toastfilter` (about 35 s after launch).
const GIVE_UP: Duration = Duration::from_secs(60);
/// Two toasts this far apart at least, so that they stack instead of one replacing the other.
const GAP: Duration = Duration::from_millis(400);

type FeUpdate = unsafe extern "system" fn(usize, f32, usize, usize) -> usize;
type Toast = unsafe extern "system" fn(u32, usize, u8, u32, usize, *const u8) -> usize;

static QUEUE: Mutex<VecDeque<(String, Instant)>> = Mutex::new(VecDeque::new());
static TEXTS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static LAST_SHOWN: Mutex<Option<Instant>> = Mutex::new(None);
static INSTALL_TRIED: AtomicBool = AtomicBool::new(false);
static WAITING: AtomicBool = AtomicBool::new(false);
static ORIG: AtomicUsize = AtomicUsize::new(0);
static BASE: AtomicUsize = AtomicUsize::new(0);
static NEXT_TEXT: AtomicUsize = AtomicUsize::new(0);
static SHOWN: AtomicU64 = AtomicU64::new(0);
static FAILED: AtomicU32 = AtomicU32::new(0);
/// The icon of a toast (`UserDispLogParam` field, 0 = none). Dev console `toast_icon`.
pub(crate) static ICON: AtomicI32 = AtomicI32::new(0);

/// The exe's base once the main loop has run (0 = not yet).
#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn exe_base() -> usize {
    BASE.load(Ordering::Relaxed)
}

/// Queue one line for the screen (any thread). The game's texts are markup: anything a player typed goes through [`clean`] first.
pub(crate) fn show(line: &str) {
    let mut q = QUEUE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    q.push_back((line.to_owned(), Instant::now()));
    WAITING.store(true, Ordering::Release);
}

/// A text a player typed, safe in a line (pure): one line, no markup, short enough for a placeholder.
pub(crate) fn clean(line: &str) -> String {
    line.chars()
        .map(|c| match c {
            '<' | '>' | '&' | '\n' | '\r' | '\t' => ' ',
            c => c,
        })
        .filter(|c| !c.is_control())
        .scan(0usize, |units, c| {
            *units += c.len_utf16();
            (*units < TEXT_UNITS).then_some(c)
        })
        .collect()
}

/// `line` as UTF-16 with its terminator, in the place of a placeholder (pure).
pub(crate) fn encode(line: &str) -> Vec<u16> {
    let mut w: Vec<u16> = line.encode_utf16().collect();
    w.truncate(TEXT_UNITS - 1);
    w.push(0);
    w
}

/// Main loop, every tick: install the hook once.
pub(crate) fn tick(base: usize) {
    if INSTALL_TRIED.swap(true, Ordering::Relaxed) {
        return;
    }
    BASE.store(base, Ordering::Relaxed);
    ORIG.store(base + SITE_FE_UPDATE.1, Ordering::Relaxed);
    if mem::read::<[u8; 8]>(base + RVA_TOAST) != Some(TOAST_BYTES) {
        tracing::warn!("gametoast: the toast function differs from game 1.03.3; no toasts of ours");
        return;
    }
    let Some(page) = hudfilter::alloc_near(base) else {
        tracing::warn!("gametoast: no page near the exe for the trampoline; no toasts of ours");
        return;
    };
    let code = hudfilter::trampoline(hook_update as FeUpdate as usize);
    // SAFETY: `page` is our own fresh RWX page of 0x1000 bytes.
    unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), page as *mut u8, code.len()) };
    match hudfilter::patch(base + SITE_FE_UPDATE.0, base + SITE_FE_UPDATE.1, page) {
        Ok(()) => tracing::info!(
            "gametoast: hooked at exe+0x{:X} (trampoline 0x{page:X})",
            SITE_FE_UPDATE.0
        ),
        Err(e) => tracing::warn!("gametoast: NOT hooked: {e}"),
    }
}

/// The hooked call: the game's thread, once a frame. The second integer register is not an argument of the callee (its second argument
/// is the float in `xmm1`).
unsafe extern "system" fn hook_update(fe: usize, dt: f32, a3: usize, a4: usize) -> usize {
    let _ = std::panic::catch_unwind(|| super::matchstart::pump(BASE.load(Ordering::Relaxed)));
    if WAITING.load(Ordering::Acquire) {
        let _ = std::panic::catch_unwind(pump);
    }
    // SAFETY: the address is the original callee of the patched site, checked by `hudfilter::patch`.
    let orig = unsafe { std::mem::transmute::<usize, FeUpdate>(ORIG.load(Ordering::Relaxed)) };
    // SAFETY: the game's own arguments, passed on unchanged.
    unsafe { orig(fe, dt, a3, a4) }
}

/// Game thread: show the next line once the table is known and the gap has passed. Never blocks.
fn pump() {
    let Ok(mut last) = LAST_SHOWN.try_lock() else {
        return;
    };
    if last.is_some_and(|t| t.elapsed() < GAP) {
        return;
    }
    let Ok(mut q) = QUEUE.try_lock() else {
        return;
    };
    let Some((line, at)) = q.front().cloned() else {
        WAITING.store(false, Ordering::Release);
        return;
    };
    let rows = toastfilter::rows_at(ROW);
    if rows.is_empty() {
        if at.elapsed() > GIVE_UP {
            q.pop_front();
            FAILED.fetch_add(1, Ordering::Relaxed);
            tracing::warn!("gametoast: no toast table found; line dropped: {line}");
        }
        return;
    }
    q.pop_front();
    drop(q);
    *last = Some(Instant::now());
    drop(last);
    match show_now(&line, &rows) {
        Ok(id) => {
            SHOWN.fetch_add(1, Ordering::Relaxed);
            tracing::info!("gametoast: shown (text {id}): {line}");
        }
        Err(e) => {
            FAILED.fetch_add(1, Ordering::Relaxed);
            tracing::warn!("gametoast: not shown: {e}: {line}");
        }
    }
}

/// The addresses of our placeholders in the text table (searched once, the table never moves).
fn texts(base: usize) -> Result<Vec<usize>, String> {
    let mut g = TEXTS.try_lock().map_err(|_| "text table busy")?;
    if g.is_empty() {
        for id in TEXT_IDS {
            let at = (0..MAX_CATEGORY)
                .filter_map(|c| fmg::fmg_for(base, c))
                .find_map(|f| fmg::text_at(f, id))
                .ok_or_else(|| format!("text {id} is not in the game's text table"))?;
            if mem::read::<u16>(at) != Some(PLACEHOLDER) {
                return Err(format!("text {id} is not our placeholder"));
            }
            g.push(at);
        }
    }
    Ok(g.clone())
}

/// Game thread.
fn show_now(line: &str, rows: &[usize]) -> Result<u32, String> {
    let base = BASE.load(Ordering::Relaxed);
    let texts = texts(base)?;
    let i = NEXT_TEXT.fetch_add(1, Ordering::Relaxed) % TEXT_IDS.len();
    let bytes: Vec<u8> = encode(line).iter().flat_map(|u| u.to_le_bytes()).collect();
    for (k, chunk) in bytes.chunks(16).enumerate() {
        mem::write_bytes(texts[i] + k * 16, chunk).map_err(|e| format!("text write: {e}"))?;
    }
    let id = TEXT_IDS[i];
    let icon = ICON.load(Ordering::Relaxed);
    let saved: Vec<Option<(i32, i32)>> = rows
        .iter()
        .map(|&r| {
            Some((
                mem::read::<i32>(r + ROW_TEXT)?,
                mem::read::<i32>(r + ROW_ICON)?,
            ))
        })
        .collect();
    for &r in rows {
        mem::write_bytes(r + ROW_TEXT, &(id as i32).to_le_bytes())
            .and_then(|()| mem::write_bytes(r + ROW_ICON, &icon.to_le_bytes()))
            .map_err(|e| format!("row write: {e}"))?;
    }
    // SAFETY: the address is the game's toast function, its first bytes were checked at install; this is the game's thread; a null `chr`
    // and `a5` and zero `kind` and `id` are paths of the function (module doc); `a6` is only read for a row whose `+0x10` is not 0.
    let toast = unsafe { std::mem::transmute::<usize, Toast>(base + RVA_TOAST) };
    let unused = [0u8; 16];
    // SAFETY: as above; `unused` outlives the call.
    unsafe { toast(ROW, 0, 0, 0, 0, unused.as_ptr()) };
    for (&r, s) in rows.iter().zip(&saved) {
        if let Some((text, ic)) = s {
            let _ = mem::write_bytes(r + ROW_TEXT, &text.to_le_bytes());
            let _ = mem::write_bytes(r + ROW_ICON, &ic.to_le_bytes());
        }
    }
    Ok(id)
}

/// The console's status line.
#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn describe() -> String {
    format!(
        "gametoast: {} shown, {} failed, icon {}",
        SHOWN.load(Ordering::Relaxed),
        FAILED.load(Ordering::Relaxed),
        ICON.load(Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_loses_markup_and_fits_a_placeholder() {
        assert_eq!(clean("A<b> killed B&C\n"), "A b  killed B C ");
        assert_eq!(clean(&"x".repeat(500)).len(), TEXT_UNITS - 1);
        let e = encode(&clean(&"x".repeat(500)));
        assert_eq!(e.len(), TEXT_UNITS);
        assert_eq!(e[TEXT_UNITS - 1], 0);
        // A character outside the basic plane takes two units: no half character at the end.
        let long = format!("{}\u{1F480}", "x".repeat(TEXT_UNITS - 2));
        assert_eq!(clean(&long), "x".repeat(TEXT_UNITS - 2));
    }

    #[test]
    fn the_placeholders_are_in_the_game_data_file_and_long_enough() {
        let entries = include_str!("../../../../gamedata/fmg-entries.json");
        for id in TEXT_IDS {
            let key = format!("\"{id}\": \"");
            let at = entries.find(&key).expect("the id is in the file") + key.len();
            let units = entries[at..].chars().take_while(|c| *c == '_').count();
            assert_eq!(units + 1, TEXT_UNITS, "text {id}");
        }
    }

    #[test]
    fn the_hooked_site_is_the_fe_update() {
        assert_eq!(SITE_FE_UPDATE, (0x7B_5DD4, 0x7C_5710));
        assert_eq!(
            hudfilter::rel32(SITE_FE_UPDATE.0, SITE_FE_UPDATE.1),
            Some(0xF937)
        );
    }
}
