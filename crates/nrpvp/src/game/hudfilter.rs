//! RW-9 (both builds): rivals are not shown by the HUD. Game 1.03.3 / exe 1.3.3.0; the reading is in MEMORY_MAP §20 ("Rival filter").
//!
//! Three per-frame HUD producers look a player character up by handle or map id and skip the player when the lookup returns null
//! (each then writes its entry as "not shown"):
//!
//! | Site | Call | Shows | Our answer for a rival |
//! |---|---|---|---|
//! | `exe+0xC3C04D` | `exe+0x5197E0(chrman, map id)` in the map pin loop | the world-map pin and the compass mark (one list) | null, always |
//! | `exe+0x7DAAA4` | `exe+0x518F30(chrman, &handle)` in the party bar producer `exe+0x7DA8B0` | the party bar (portrait, name, HP) top left | null, always |
//! | `exe+0x7DA1F9` | `exe+0x518F30(chrman, &handle)` in the overhead tag producer `exe+0x7D9F80` | the name over the head and its off-screen arrow | null unless the rival is LOCAL's lock-on target (`PlayerIns+0x768`) |
//!
//! Each `call rel32` is pointed at a small trampoline (`mov rax, imm64; jmp rax`) in a page we allocate within ±2 GB of the exe, which
//! jumps to our hook; the hook calls the game's own function and returns null for a character whose handle is in the rival list. Only
//! the 4 displacement bytes of each call change (one store inside one cache line); the bytes are checked before (a site that does not
//! hold the expected call is left alone). Teammates are not rivals, so Duo / Trio keep their own team on the map, compass and bars;
//! enemies' HP bars from lock-on or hits are a different producer and stay vanilla (DECISIONS 2026-10-26). Log lines start with
//! `hudfilter:`.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};

/// `PlayerIns` -> the lock-on target's handle (u64, -1 = none). `[VERIFIED: static, MEMORY_MAP §20; live: see §20 "Rival filter"]`
pub(crate) const PLAYER_LOCK_TARGET: usize = 0x768;
/// `ChrIns` -> its handle (`layout::CHR_HANDLE`).
const CHR_HANDLE: usize = super::layout::CHR_HANDLE;

/// Site RVA, the callee RVA its `call rel32` must hold. `[VERIFIED: static disassembly of exe 1.3.3.0, MEMORY_MAP §20]`
const SITE_MAP: (usize, usize) = (0xC3_C04D, 0x51_97E0);
const SITE_PARTY: (usize, usize) = (0x7D_AAA4, 0x51_8F30);
const SITE_TAG: (usize, usize) = (0x7D_A1F9, 0x51_8F30);

const MAX_RIVALS: usize = 8;
static RIVALS: [AtomicU64; MAX_RIVALS] = [const { AtomicU64::new(0) }; MAX_RIVALS];
static RIVAL_COUNT: AtomicUsize = AtomicUsize::new(0);
static TARGET: AtomicU64 = AtomicU64::new(u64::MAX);
/// The filter acts (a match runs, or the console forces it).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Console override: 0 = automatic, 1 = forced on, 2 = forced off.
static OVERRIDE: AtomicU8 = AtomicU8::new(0);

/// The game's own functions (set before any site is patched).
static ORIG_BY_ID: AtomicUsize = AtomicUsize::new(0);
static ORIG_BY_HANDLE: AtomicUsize = AtomicUsize::new(0);

/// Per site: patched, game calls seen, rivals hidden.
struct Counters {
    patched: AtomicBool,
    calls: AtomicU64,
    hidden: AtomicU64,
    /// [`now_ms`] of the last rival hidden, and of the last rival shown because it is the lock-on target (name tags).
    last_hidden: AtomicU64,
    last_target: AtomicU64,
}
static MAP: Counters = Counters::new();
static PARTY: Counters = Counters::new();
static TAG: Counters = Counters::new();
static INSTALL_TRIED: AtomicBool = AtomicBool::new(false);

impl Counters {
    const fn new() -> Self {
        Self {
            patched: AtomicBool::new(false),
            calls: AtomicU64::new(0),
            hidden: AtomicU64::new(0),
            last_hidden: AtomicU64::new(0),
            last_target: AtomicU64::new(0),
        }
    }
    fn line(&self, name: &str) -> String {
        if self.patched.load(Ordering::Relaxed) {
            let recent = |a: &AtomicU64| {
                let t = a.load(Ordering::Relaxed);
                t != 0 && now_ms().saturating_sub(t) < 1000
            };
            format!(
                "{name} hooked{}{} ({} hidden / {} looked up)",
                if recent(&self.last_hidden) {
                    ", hiding a rival now"
                } else {
                    ", no rival hidden in the last second"
                },
                if recent(&self.last_target) {
                    ", showing the lock-on target"
                } else {
                    ""
                },
                self.hidden.load(Ordering::Relaxed),
                self.calls.load(Ordering::Relaxed)
            )
        } else {
            format!("{name} NOT hooked")
        }
    }
}

/// Is the map / compass site hooked (then `maphide` leaves the list alone)?
pub(crate) fn map_hooked() -> bool {
    MAP.patched.load(Ordering::Relaxed)
}

/// Console `rivalhud [0|1|auto]`.
pub(crate) fn set_override(v: Option<bool>) {
    OVERRIDE.store(
        match v {
            None => 0,
            Some(true) => 1,
            Some(false) => 2,
        },
        Ordering::Relaxed,
    );
}

fn wanted(in_match: bool) -> bool {
    match OVERRIDE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => in_match,
    }
}

/// What a hook answers for a looked-up character (pure, tested). `allow_target`: the lock-on target keeps it (name tags).
fn hide(active: bool, handle: u64, rivals: &[u64], target: u64, allow_target: bool) -> bool {
    active && rivals.contains(&handle) && !(allow_target && handle == target)
}

/// Milliseconds since the first call (monotonic).
fn now_ms() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
        + 1
}

/// The hook's answer for `chr`: null for a hidden rival; counted per site.
fn judge(c: &Counters, chr: usize, allow_target: bool) -> usize {
    c.calls.fetch_add(1, Ordering::Relaxed);
    if chr == 0 || !ACTIVE.load(Ordering::Relaxed) {
        return chr;
    }
    let n = RIVAL_COUNT.load(Ordering::Acquire).min(MAX_RIVALS);
    if n == 0 {
        return chr;
    }
    let Some(handle) = super::mem::read::<u64>(chr + CHR_HANDLE) else {
        return chr;
    };
    let mut list = [0u64; MAX_RIVALS];
    for (i, slot) in list.iter_mut().enumerate().take(n) {
        *slot = RIVALS[i].load(Ordering::Relaxed);
    }
    let target = TARGET.load(Ordering::Relaxed);
    if hide(true, handle, &list[..n], target, allow_target) {
        c.hidden.fetch_add(1, Ordering::Relaxed);
        c.last_hidden.store(now_ms(), Ordering::Relaxed);
        0
    } else {
        if allow_target && handle == target && list[..n].contains(&handle) {
            c.last_target.store(now_ms(), Ordering::Relaxed);
        }
        chr
    }
}

type ById = unsafe extern "system" fn(usize, u32) -> usize;
type ByHandle = unsafe extern "system" fn(usize, *const u64) -> usize;

/// Map pin loop (game thread, every frame).
unsafe extern "system" fn hook_map(man: usize, id: u32) -> usize {
    // SAFETY: ORIG_BY_ID holds the game's function `exe+0x5197E0` (set before the site was patched); same arguments as the game's call.
    let orig: ById =
        unsafe { std::mem::transmute::<usize, ById>(ORIG_BY_ID.load(Ordering::Relaxed)) };
    let chr = unsafe { orig(man, id) };
    judge(&MAP, chr, false)
}

/// Party bar producer (game thread, every frame).
unsafe extern "system" fn hook_party(man: usize, handle: *const u64) -> usize {
    // SAFETY: ORIG_BY_HANDLE holds the game's function `exe+0x518F30`; same arguments as the game's call.
    let orig: ByHandle =
        unsafe { std::mem::transmute::<usize, ByHandle>(ORIG_BY_HANDLE.load(Ordering::Relaxed)) };
    let chr = unsafe { orig(man, handle) };
    judge(&PARTY, chr, false)
}

/// Overhead name tag producer (game thread, every frame).
unsafe extern "system" fn hook_tag(man: usize, handle: *const u64) -> usize {
    // SAFETY: as `hook_party`.
    let orig: ByHandle =
        unsafe { std::mem::transmute::<usize, ByHandle>(ORIG_BY_HANDLE.load(Ordering::Relaxed)) };
    let chr = unsafe { orig(man, handle) };
    judge(&TAG, chr, true)
}

/// The rel32 a `call` at `site` needs to reach `dest` (None when farther than ±2 GB). Pure, tested.
fn rel32(site: usize, dest: usize) -> Option<i32> {
    i32::try_from(dest as i64 - (site as i64 + 5)).ok()
}

/// `mov rax, imm64; jmp rax`.
fn trampoline(dest: usize) -> [u8; 12] {
    let mut t = [0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xE0];
    t[2..10].copy_from_slice(&(dest as u64).to_le_bytes());
    t
}

/// One RWX page within ±2 GB below or above the exe (64 KiB steps).
fn alloc_near(base: usize) -> Option<usize> {
    use windows::Win32::System::Memory::{
        VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
    };
    const STEP: usize = 0x1_0000;
    for k in 1..(0x6000_0000 / STEP) {
        for addr in [base.wrapping_sub(k * STEP), base + 0x800_0000 + k * STEP] {
            // SAFETY: a request for fresh memory at a hint address; VirtualAlloc returns null when the range is taken.
            let p = unsafe {
                VirtualAlloc(
                    Some(addr as *const std::ffi::c_void),
                    0x1000,
                    MEM_RESERVE | MEM_COMMIT,
                    PAGE_EXECUTE_READWRITE,
                )
            };
            if !p.is_null() {
                return Some(p as usize);
            }
        }
    }
    None
}

/// Point the `call` at `site` to `tramp` if it still calls `callee`.
fn patch(site: usize, callee: usize, tramp: usize) -> Result<(), String> {
    use windows::Win32::System::Diagnostics::Debug::FlushInstructionCache;
    use windows::Win32::System::Memory::{
        VirtualProtect, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;
    let code = super::mem::read::<[u8; 5]>(site).ok_or("site not readable")?;
    let now = i32::from_le_bytes([code[1], code[2], code[3], code[4]]);
    if code[0] != 0xE8 || Some(now) != rel32(site, callee) {
        return Err(format!(
            "unexpected bytes {:02X?} (another mod or another game version?)",
            code
        ));
    }
    let new = rel32(site, tramp).ok_or("trampoline out of rel32 range")?;
    let mut old = PAGE_PROTECTION_FLAGS(0);
    // SAFETY: the 5 bytes at `site` are a call instruction of the game's .text (checked above); the protection is widened for the
    // store and put back. The displacement is one 4-byte store that lies in one cache line at every site (MEMORY_MAP §20), so a
    // game thread executing the call sees the old or the new target, never a mix.
    unsafe {
        VirtualProtect(
            site as *const std::ffi::c_void,
            5,
            PAGE_EXECUTE_READWRITE,
            &mut old,
        )
        .map_err(|e| format!("VirtualProtect: {e}"))?;
        std::ptr::write_unaligned((site + 1) as *mut i32, new);
        let mut tmp = PAGE_PROTECTION_FLAGS(0);
        let _ = VirtualProtect(site as *const std::ffi::c_void, 5, old, &mut tmp);
        let _ = FlushInstructionCache(
            GetCurrentProcess(),
            Some(site as *const std::ffi::c_void),
            5,
        );
    }
    Ok(())
}

/// Install the three hooks once (main loop, both builds).
fn install(base: usize) {
    if INSTALL_TRIED.swap(true, Ordering::Relaxed) {
        return;
    }
    ORIG_BY_ID.store(base + SITE_MAP.1, Ordering::Relaxed);
    ORIG_BY_HANDLE.store(base + SITE_PARTY.1, Ordering::Relaxed);
    let Some(page) = alloc_near(base) else {
        tracing::warn!(
            "hudfilter: no page near the exe for the trampolines; rivals stay on the HUD"
        );
        return;
    };
    let hooks: [(&Counters, &str, (usize, usize), usize); 3] = [
        (&MAP, "map / compass", SITE_MAP, hook_map as ById as usize),
        (
            &PARTY,
            "party bar",
            SITE_PARTY,
            hook_party as ByHandle as usize,
        ),
        (&TAG, "name tag", SITE_TAG, hook_tag as ByHandle as usize),
    ];
    for (i, (c, name, (site, callee), hook)) in hooks.into_iter().enumerate() {
        let tramp = page + i * 16;
        let code = trampoline(hook);
        // SAFETY: `page` is our own fresh RWX page of 0x1000 bytes; 3 × 16 bytes fit.
        unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), tramp as *mut u8, code.len()) };
        match patch(base + site, base + callee, tramp) {
            Ok(()) => {
                c.patched.store(true, Ordering::Relaxed);
                tracing::info!(
                    "hudfilter: {name} hooked at exe+0x{site:X} (trampoline 0x{tramp:X})"
                );
            }
            Err(e) => tracing::warn!("hudfilter: {name} at exe+0x{site:X} NOT hooked: {e}"),
        }
    }
}

/// Main loop, every tick (both builds). `in_match`: the mod's PvP mode runs in an expedition; `rivals`: the rival players' handles;
/// `local`: LOCAL's PlayerIns.
pub(crate) fn tick(base: usize, in_match: bool, rivals: &[u64], local: Option<usize>) {
    install(base);
    let n = rivals.len().min(MAX_RIVALS);
    for (slot, &h) in RIVALS.iter().zip(rivals) {
        slot.store(h, Ordering::Relaxed);
    }
    RIVAL_COUNT.store(n, Ordering::Release);
    let target = local
        .and_then(|l| super::mem::read::<u64>(l + PLAYER_LOCK_TARGET))
        .unwrap_or(u64::MAX);
    TARGET.store(target, Ordering::Relaxed);
    let want = wanted(in_match);
    if ACTIVE.swap(want, Ordering::Relaxed) != want {
        tracing::info!(
            "hudfilter: rivals {} the map, compass, party bars and name tags ({n} rival(s))",
            if want { "hidden from" } else { "back on" }
        );
    }
}

/// Console `rivalhud` with no argument.
pub(crate) fn describe() -> String {
    let n = RIVAL_COUNT.load(Ordering::Relaxed).min(MAX_RIVALS);
    let rivals: Vec<String> = RIVALS[..n]
        .iter()
        .map(|h| format!("{:#X}", h.load(Ordering::Relaxed)))
        .collect();
    let target = TARGET.load(Ordering::Relaxed);
    format!(
        "rivalhud: {} ({}); rivals [{}]; lock-on target {}; {}; {}; {}",
        if ACTIVE.load(Ordering::Relaxed) {
            "hiding rivals"
        } else {
            "showing everyone"
        },
        ["automatic (in a match)", "forced on", "forced off"]
            [usize::from(OVERRIDE.load(Ordering::Relaxed)).min(2)],
        rivals.join(", "),
        if target == u64::MAX || target == 0 {
            "none".to_string()
        } else {
            format!("{target:#X}")
        },
        MAP.line("map / compass"),
        PARTY.line("party bar"),
        TAG.line("name tag"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_sites() {
        assert_eq!(SITE_MAP, (0xC3C04D, 0x5197E0));
        assert_eq!(SITE_PARTY, (0x7DAAA4, 0x518F30));
        assert_eq!(SITE_TAG, (0x7DA1F9, 0x518F30));
        assert_eq!(PLAYER_LOCK_TARGET, 0x768);
        // The game's own displacements as read from the file (E8 8E D7 8D FF etc.).
        assert_eq!(rel32(SITE_MAP.0, SITE_MAP.1), Some(0xFF8D_D78Eu32 as i32));
        assert_eq!(
            rel32(SITE_PARTY.0, SITE_PARTY.1),
            Some(0xFFD3_E487u32 as i32)
        );
        assert_eq!(rel32(SITE_TAG.0, SITE_TAG.1), Some(0xFFD3_ED32u32 as i32));
        // Each displacement (site+1 .. site+5) stays inside one 64-byte cache line.
        for (s, _) in [SITE_MAP, SITE_PARTY, SITE_TAG] {
            assert_eq!((s + 1) / 64, (s + 4) / 64);
        }
    }

    #[test]
    fn rivals_are_hidden_teammates_and_the_target_tag_are_not() {
        let rivals = [0x10, 0x20];
        assert!(hide(true, 0x10, &rivals, u64::MAX, false));
        assert!(
            !hide(true, 0x30, &rivals, u64::MAX, false),
            "a teammate stays"
        );
        assert!(
            !hide(false, 0x10, &rivals, u64::MAX, false),
            "off outside a match"
        );
        // The lock-on target keeps its name tag, not its map pin or party bar.
        assert!(!hide(true, 0x20, &rivals, 0x20, true));
        assert!(hide(true, 0x20, &rivals, 0x20, false));
        assert!(hide(true, 0x10, &rivals, 0x20, true));
    }

    #[test]
    fn trampoline_and_rel32() {
        let t = trampoline(0x1122_3344_5566_7788);
        assert_eq!(&t[..2], &[0x48, 0xB8]);
        assert_eq!(&t[2..10], &0x1122_3344_5566_7788u64.to_le_bytes());
        assert_eq!(&t[10..], &[0xFF, 0xE0]);
        assert_eq!(rel32(0x1000, 0x1005), Some(0));
        assert_eq!(rel32(0x1000, 0x1000), Some(-5));
        assert_eq!(rel32(0x1_0000_0000, 0x10), None);
    }

    #[test]
    fn the_override_beats_the_match() {
        set_override(Some(true));
        assert!(wanted(false));
        set_override(Some(false));
        assert!(!wanted(true));
        set_override(None);
        assert!(wanted(true) && !wanted(false));
    }
}
