//! RW-9 (both builds): rivals are not shown by the HUD. Game 1.03.3 / exe 1.3.3.0; the reading is in MEMORY_MAP §20 ("Rival filter").
//!
//! Three per-frame HUD producers look a player character up by handle or map id and skip the player when the lookup returns null
//! (each then writes its entry as "not shown"):
//!
//! | Site | Call | Shows | Our answer for a rival |
//! |---|---|---|---|
//! | `exe+0xC3C04D` | `exe+0x5197E0(chrman, map id)` in the map pin loop | the world-map pin and the compass mark (one list) | null, always |
//! | `exe+0x7DAAA4` | `exe+0x518F30(chrman, &handle)` in the party bar producer `exe+0x7DA8B0` | the party bar (portrait, name, HP) top left | null, always |
//! | `exe+0x7DA1F9` | `exe+0x518F30(chrman, &handle)` in the overhead tag producer `exe+0x7D9F80` | the name over the head and its off-screen arrow | null, always: the lock-on target too, since the game's own enemy bar already shows its name (DECISIONS 2026-11-06) |
//!
//! Each `call rel32` is pointed at a small trampoline (`mov rax, imm64; jmp rax`) in a page we allocate within ±2 GB of the exe, which
//! jumps to our hook; the hook calls the game's own function and returns null for a character whose handle is in the rival list. Only
//! the 4 displacement bytes of each call change (one store inside one cache line); the bytes are checked before (a site that does not
//! hold the expected call is left alone). Teammates are not rivals, so Duo / Trio keep their own team on the map, compass and bars;
//! enemies' HP bars from lock-on or hits are a different producer and stay vanilla (DECISIONS 2026-10-26). Log lines start with
//! `hudfilter:`.
//!
//! **Map markers (pins a player places, MEMORY_MAP §20 "Map markers").** Every player's pins are records in the marker manager
//! (`[exe+0x3C17BE0]`, list at `+0x10`, node `+0x10` = record); record `+0` is the key, its u16 at `+2` the placer's map id
//! (`[PlayerIns+0x5A0]+8`). Three loops copy the records out, and all are hooked the same way (rival's record → skipped):
//!
//! | Site | Call | Shows | Our answer for a rival's record |
//! |---|---|---|---|
//! | `exe+0x911B13` | `exe+0x9040E0(.., .., &record+4, ..)` in the view model's pin sync (false → the pin is not added) | the pin on the map and the compass | false |
//! | `exe+0x7D9908` | `exe+0x7D7030(&list, record)` in the 3D marker producer `exe+0x7D9560` | the marker in the world | not called |
//! | `exe+0xEE220B` | `exe+0xEDBBD0(&ctx, index, record)` in the marker sfx sync (a pin not seen by the loop loses its entry) | the green beam of light in the world | not called |

use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicU8, AtomicUsize, Ordering};

/// `PlayerIns` -> the lock-on target's handle (u64, -1 = none). `[VERIFIED: static, MEMORY_MAP §20; live: see §20 "Rival filter"]`
pub(crate) const PLAYER_LOCK_TARGET: usize = 0x768;
/// `ChrIns` -> its handle (`layout::CHR_HANDLE`).
const CHR_HANDLE: usize = super::layout::CHR_HANDLE;

/// Site RVA, the callee RVA its `call rel32` must hold. `[VERIFIED: static disassembly of exe 1.3.3.0, MEMORY_MAP §20]`
const SITE_MAP: (usize, usize) = (0xC3_C04D, 0x51_97E0);
const SITE_PARTY: (usize, usize) = (0x7D_AAA4, 0x51_8F30);
const SITE_TAG: (usize, usize) = (0x7D_A1F9, 0x51_8F30);
/// Map markers: the view model's pin sync (map + compass) and the 3D marker producer. `[VERIFIED: static + live peek / write watch,
/// two instances 2026-10-27, MEMORY_MAP §20 "Map markers"]`
const SITE_PIN_MAP: (usize, usize) = (0x91_1B13, 0x90_40E0);
const SITE_PIN_WORLD: (usize, usize) = (0x7D_9908, 0x7D_7030);
/// Map markers: the sync of the pin's beam of light. `[VERIFIED: static disassembly of exe 1.3.3.0, MEMORY_MAP §20 "Map markers"]`
const SITE_PIN_BEAM: (usize, usize) = (0xEE_220B, 0xED_BBD0);
/// `PlayerIns+0x5A0` -> player data; its u32 at `+8` is the map id a marker record names at `+2`.
const PLAYER_DATA: usize = 0x5A0;
const DATA_MAP_ID: usize = 0x8;
/// Marker record: the placer's map id (u16) at `+2`. The map site gets `record + 4`.
const RECORD_OWNER: usize = 0x2;
const MAP_ARG_OFFSET: usize = 0x4;

const MAX_RIVALS: usize = 8;
static RIVALS: [AtomicU64; MAX_RIVALS] = [const { AtomicU64::new(0) }; MAX_RIVALS];
static RIVAL_COUNT: AtomicUsize = AtomicUsize::new(0);
/// The rivals' map ids (0 = none), for their markers.
static RIVAL_IDS: [AtomicU16; MAX_RIVALS] = [const { AtomicU16::new(0) }; MAX_RIVALS];
static RIVAL_ID_COUNT: AtomicUsize = AtomicUsize::new(0);
/// LOCAL's lock-on target (diagnostic only: shown by `rivalhud`, no site lets it through).
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
    /// [`now_ms`] of the last rival hidden.
    last_hidden: AtomicU64,
}
static MAP: Counters = Counters::new();
static PARTY: Counters = Counters::new();
static TAG: Counters = Counters::new();
static PIN_MAP: Counters = Counters::new();
static PIN_WORLD: Counters = Counters::new();
static PIN_BEAM: Counters = Counters::new();
static ORIG_PIN_MAP: AtomicUsize = AtomicUsize::new(0);
static ORIG_PIN_WORLD: AtomicUsize = AtomicUsize::new(0);
static ORIG_PIN_BEAM: AtomicUsize = AtomicUsize::new(0);
static INSTALL_TRIED: AtomicBool = AtomicBool::new(false);

impl Counters {
    const fn new() -> Self {
        Self {
            patched: AtomicBool::new(false),
            calls: AtomicU64::new(0),
            hidden: AtomicU64::new(0),
            last_hidden: AtomicU64::new(0),
        }
    }
    fn line(&self, name: &str) -> String {
        if self.patched.load(Ordering::Relaxed) {
            let t = self.last_hidden.load(Ordering::Relaxed);
            format!(
                "{name} hooked{} ({} hidden / {} looked up)",
                if t != 0 && now_ms().saturating_sub(t) < 1000 {
                    ", hiding a rival now"
                } else {
                    ", no rival hidden in the last second"
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

/// What a hook answers for a looked-up character (pure, tested).
fn hide(active: bool, handle: u64, rivals: &[u64]) -> bool {
    active && rivals.contains(&handle)
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
fn judge(c: &Counters, chr: usize) -> usize {
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
    if hide(true, handle, &list[..n]) {
        c.hidden.fetch_add(1, Ordering::Relaxed);
        c.last_hidden.store(now_ms(), Ordering::Relaxed);
        0
    } else {
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
    judge(&MAP, chr)
}

/// Party bar producer (game thread, every frame).
unsafe extern "system" fn hook_party(man: usize, handle: *const u64) -> usize {
    // SAFETY: ORIG_BY_HANDLE holds the game's function `exe+0x518F30`; same arguments as the game's call.
    let orig: ByHandle =
        unsafe { std::mem::transmute::<usize, ByHandle>(ORIG_BY_HANDLE.load(Ordering::Relaxed)) };
    let chr = unsafe { orig(man, handle) };
    judge(&PARTY, chr)
}

/// Overhead name tag producer (game thread, every frame).
unsafe extern "system" fn hook_tag(man: usize, handle: *const u64) -> usize {
    // SAFETY: as `hook_party`.
    let orig: ByHandle =
        unsafe { std::mem::transmute::<usize, ByHandle>(ORIG_BY_HANDLE.load(Ordering::Relaxed)) };
    let chr = unsafe { orig(man, handle) };
    judge(&TAG, chr)
}

/// Is a marker placed by `owner` (map id) a rival's? Pure, tested.
fn rival_marker(active: bool, owner: u16, rival_ids: &[u16]) -> bool {
    active && owner != 0 && rival_ids.contains(&owner)
}

/// The answer for the marker record whose owner u16 is at `owner_at`: hide it (counted per site)?
fn judge_marker(c: &Counters, owner_at: usize) -> bool {
    c.calls.fetch_add(1, Ordering::Relaxed);
    if !ACTIVE.load(Ordering::Relaxed) {
        return false;
    }
    let n = RIVAL_ID_COUNT.load(Ordering::Acquire).min(MAX_RIVALS);
    if n == 0 {
        return false;
    }
    let Some(owner) = super::mem::read::<u16>(owner_at) else {
        return false;
    };
    let mut ids = [0u16; MAX_RIVALS];
    for (i, slot) in ids.iter_mut().enumerate().take(n) {
        *slot = RIVAL_IDS[i].load(Ordering::Relaxed);
    }
    let hide = rival_marker(true, owner, &ids[..n]);
    if hide {
        c.hidden.fetch_add(1, Ordering::Relaxed);
        c.last_hidden.store(now_ms(), Ordering::Relaxed);
    }
    hide
}

type PinMap = unsafe extern "system" fn(usize, usize, usize, usize) -> usize;
type PinWorld = unsafe extern "system" fn(usize, usize) -> usize;
type PinBeam = unsafe extern "system" fn(usize, u32, usize) -> usize;

/// View model pin sync (map + compass; game thread, every frame): `rec4` = record + 4. False (al = 0) = "do not add this pin".
unsafe extern "system" fn hook_pin_map(
    this: usize,
    out: usize,
    rec4: usize,
    extra: usize,
) -> usize {
    if rec4 >= MAP_ARG_OFFSET && judge_marker(&PIN_MAP, rec4 - MAP_ARG_OFFSET + RECORD_OWNER) {
        return 0;
    }
    // SAFETY: ORIG_PIN_MAP holds the game's function `exe+0x9040E0` (set before the site was patched); same arguments.
    let orig: PinMap =
        unsafe { std::mem::transmute::<usize, PinMap>(ORIG_PIN_MAP.load(Ordering::Relaxed)) };
    unsafe { orig(this, out, rec4, extra) }
}

/// 3D marker producer (game thread, every frame): `record` is collected into the marker list unless it is a rival's.
unsafe extern "system" fn hook_pin_world(list: usize, record: usize) -> usize {
    if record != 0 && judge_marker(&PIN_WORLD, record + RECORD_OWNER) {
        return 0;
    }
    // SAFETY: ORIG_PIN_WORLD holds the game's function `exe+0x7D7030`; same arguments.
    let orig: PinWorld =
        unsafe { std::mem::transmute::<usize, PinWorld>(ORIG_PIN_WORLD.load(Ordering::Relaxed)) };
    unsafe { orig(list, record) }
}

/// Marker sfx sync (game thread, when the records change): a record the loop does not hand over has no beam, and an entry it had loses it.
unsafe extern "system" fn hook_pin_beam(ctx: usize, index: u32, record: usize) -> usize {
    if record != 0 && judge_marker(&PIN_BEAM, record + RECORD_OWNER) {
        return 0;
    }
    // SAFETY: ORIG_PIN_BEAM holds the game's function `exe+0xEDBBD0`; same arguments.
    let orig: PinBeam =
        unsafe { std::mem::transmute::<usize, PinBeam>(ORIG_PIN_BEAM.load(Ordering::Relaxed)) };
    unsafe { orig(ctx, index, record) }
}

/// The rel32 a `call` at `site` needs to reach `dest` (None when farther than ±2 GB). Pure, tested.
pub(crate) fn rel32(site: usize, dest: usize) -> Option<i32> {
    i32::try_from(dest as i64 - (site as i64 + 5)).ok()
}

/// `mov rax, imm64; jmp rax`.
pub(crate) fn trampoline(dest: usize) -> [u8; 12] {
    let mut t = [0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xE0];
    t[2..10].copy_from_slice(&(dest as u64).to_le_bytes());
    t
}

/// One RWX page within ±2 GB below or above the exe (64 KiB steps).
pub(crate) fn alloc_near(base: usize) -> Option<usize> {
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
pub(crate) fn patch(site: usize, callee: usize, tramp: usize) -> Result<(), String> {
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

/// Install the hooks once (main loop, both builds).
fn install(base: usize) {
    if INSTALL_TRIED.swap(true, Ordering::Relaxed) {
        return;
    }
    ORIG_BY_ID.store(base + SITE_MAP.1, Ordering::Relaxed);
    ORIG_BY_HANDLE.store(base + SITE_PARTY.1, Ordering::Relaxed);
    ORIG_PIN_MAP.store(base + SITE_PIN_MAP.1, Ordering::Relaxed);
    ORIG_PIN_WORLD.store(base + SITE_PIN_WORLD.1, Ordering::Relaxed);
    ORIG_PIN_BEAM.store(base + SITE_PIN_BEAM.1, Ordering::Relaxed);
    let Some(page) = alloc_near(base) else {
        tracing::warn!(
            "hudfilter: no page near the exe for the trampolines; rivals stay on the HUD"
        );
        return;
    };
    let hooks: [(&Counters, &str, (usize, usize), usize); 6] = [
        (&MAP, "map / compass", SITE_MAP, hook_map as ById as usize),
        (
            &PARTY,
            "party bar",
            SITE_PARTY,
            hook_party as ByHandle as usize,
        ),
        (&TAG, "name tag", SITE_TAG, hook_tag as ByHandle as usize),
        (
            &PIN_MAP,
            "markers on map",
            SITE_PIN_MAP,
            hook_pin_map as PinMap as usize,
        ),
        (
            &PIN_WORLD,
            "markers in 3D",
            SITE_PIN_WORLD,
            hook_pin_world as PinWorld as usize,
        ),
        (
            &PIN_BEAM,
            "marker beams",
            SITE_PIN_BEAM,
            hook_pin_beam as PinBeam as usize,
        ),
    ];
    for (i, (c, name, (site, callee), hook)) in hooks.into_iter().enumerate() {
        let tramp = page + i * 16;
        let code = trampoline(hook);
        // SAFETY: `page` is our own fresh RWX page of 0x1000 bytes; 6 × 16 bytes fit.
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
/// `local`: LOCAL's PlayerIns; `rival_chrs`: the rivals' PlayerIns (their map ids name their map markers).
pub(crate) fn tick(
    base: usize,
    in_match: bool,
    rivals: &[u64],
    rival_chrs: &[usize],
    local: Option<usize>,
) {
    install(base);
    BASE.store(base, Ordering::Relaxed);
    let n = rivals.len().min(MAX_RIVALS);
    for (slot, &h) in RIVALS.iter().zip(rivals) {
        slot.store(h, Ordering::Relaxed);
    }
    RIVAL_COUNT.store(n, Ordering::Release);
    let local_id = local.and_then(map_id);
    let ids = marker_ids(rival_chrs.iter().map(|&c| map_id(c)), local_id);
    for (slot, &id) in RIVAL_IDS.iter().zip(&ids) {
        slot.store(id, Ordering::Relaxed);
    }
    RIVAL_ID_COUNT.store(ids.len(), Ordering::Release);
    let target = local
        .and_then(|l| super::mem::read::<u64>(l + PLAYER_LOCK_TARGET))
        .unwrap_or(u64::MAX);
    TARGET.store(target, Ordering::Relaxed);
    let want = wanted(in_match);
    if ACTIVE.swap(want, Ordering::Relaxed) != want {
        tracing::info!(
            "hudfilter: rivals and their map markers {} the map, compass, 3D world, party bars and name tags ({n} rival(s), map ids {ids:?})",
            if want { "hidden from" } else { "back on" }
        );
    }
}

/// A player's map id (`[PlayerIns+0x5A0]+8`, the u16 a marker record names), if it reads as one.
fn map_id(chr: usize) -> Option<u16> {
    let data = super::mem::read::<usize>(chr + PLAYER_DATA).filter(|d| *d != 0)?;
    let id = super::mem::read::<u32>(data + DATA_MAP_ID)?;
    u16::try_from(id).ok()
}

/// The rivals' marker owner ids: readable, non-zero, never LOCAL's own (its markers always stay), no repeats, at most
/// [`MAX_RIVALS`]. Pure, tested.
fn marker_ids(ids: impl Iterator<Item = Option<u16>>, local: Option<u16>) -> Vec<u16> {
    let mut out = Vec::new();
    for id in ids.flatten() {
        if id != 0
            && id != u16::MAX
            && Some(id) != local
            && !out.contains(&id)
            && out.len() < MAX_RIVALS
        {
            out.push(id);
        }
    }
    out
}

/// Console `rivalhud` with no argument.
pub(crate) fn describe() -> String {
    let n = RIVAL_COUNT.load(Ordering::Relaxed).min(MAX_RIVALS);
    let rivals: Vec<String> = RIVALS[..n]
        .iter()
        .map(|h| format!("{:#X}", h.load(Ordering::Relaxed)))
        .collect();
    let target = TARGET.load(Ordering::Relaxed);
    let ids: Vec<String> = RIVAL_IDS[..RIVAL_ID_COUNT.load(Ordering::Relaxed).min(MAX_RIVALS)]
        .iter()
        .map(|i| i.load(Ordering::Relaxed).to_string())
        .collect();
    format!(
        "rivalhud: {} ({}); rivals [{}]; rival map ids [{}]; lock-on target {}; {}; {}; {}; {}; {}; {}",
        if ACTIVE.load(Ordering::Relaxed) {
            "hiding rivals"
        } else {
            "showing everyone"
        },
        ["automatic (in a match)", "forced on", "forced off"]
            [usize::from(OVERRIDE.load(Ordering::Relaxed)).min(2)],
        rivals.join(", "),
        ids.join(", "),
        if target == u64::MAX || target == 0 {
            "none".to_string()
        } else {
            format!("{target:#X}")
        },
        MAP.line("map / compass"),
        PARTY.line("party bar"),
        TAG.line("name tag"),
        PIN_MAP.line("markers on map"),
        PIN_WORLD.line("markers in 3D"),
        PIN_BEAM.line("marker beams"),
    ) + &markers_line()
}

/// The marker manager (`[exe+0x3C17BE0]`): its record list (a ring of nodes, `+0x10` = head; node `+0` next, `+0x10` record).
const RVA_MARKER_MAN: usize = 0x3C1_7BE0;
const MAN_RECORDS: usize = 0x10;
const NODE_RECORD: usize = 0x10;
/// The view model's user-pin list (`CS::WorldMapUserPinDataList` at `vm+0xF0`, its pointer vector at `+0x100`/`+0x108`); item
/// (`CS::WorldMapUserPinData`) `+0x22` = the placer's map id.
const VM_PINS_BEGIN: usize = 0x100;
const VM_PINS_END: usize = 0x108;
const PIN_OWNER: usize = 0x22;
static BASE: AtomicUsize = AtomicUsize::new(0);

/// `; markers: records [owners], map list [owners]` (read-only; what arrived vs what the map and compass show), for `rivalhud`.
fn markers_line() -> String {
    use super::mem::read;
    let base = BASE.load(Ordering::Relaxed);
    if base == 0 {
        return String::new();
    }
    let mut records = Vec::new();
    if let Some(head) = read::<usize>(base + RVA_MARKER_MAN)
        .filter(|m| *m != 0)
        .and_then(|m| read::<usize>(m + MAN_RECORDS))
        .filter(|h| *h != 0)
    {
        let mut node = read::<usize>(head).unwrap_or(head);
        while node != head && node != 0 && records.len() < 32 {
            if let Some(owner) = read::<usize>(node + NODE_RECORD)
                .filter(|r| *r != 0)
                .and_then(|r| read::<u16>(r + RECORD_OWNER))
            {
                records.push(owner.to_string());
            }
            node = read::<usize>(node).unwrap_or(0);
        }
    }
    let mut shown = Vec::new();
    if let Some(vm) = super::maphide::view_model(base) {
        let begin = read::<usize>(vm + VM_PINS_BEGIN).unwrap_or(0);
        let end = read::<usize>(vm + VM_PINS_END).unwrap_or(0);
        let n = end.wrapping_sub(begin) / 8;
        for i in 0..n.min(32) {
            if let Some(owner) = read::<usize>(begin + 8 * i)
                .filter(|it| *it != 0)
                .and_then(|it| read::<u16>(it + PIN_OWNER))
            {
                shown.push(owner.to_string());
            }
        }
    }
    format!(
        "; markers: records [{}], map list [{}]",
        records.join(", "),
        shown.join(", ")
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
        // Map markers: E8 C8 25 FF FF and E8 23 D7 FF FF in the file.
        assert_eq!(SITE_PIN_MAP, (0x911B13, 0x9040E0));
        assert_eq!(SITE_PIN_WORLD, (0x7D9908, 0x7D7030));
        assert_eq!(
            rel32(SITE_PIN_MAP.0, SITE_PIN_MAP.1),
            Some(0xFFFF_25C8u32 as i32)
        );
        assert_eq!(
            rel32(SITE_PIN_WORLD.0, SITE_PIN_WORLD.1),
            Some(0xFFFF_D723u32 as i32)
        );
        // The marker beams: E8 C0 99 FF FF in the file.
        assert_eq!(SITE_PIN_BEAM, (0xEE220B, 0xEDBBD0));
        assert_eq!(
            rel32(SITE_PIN_BEAM.0, SITE_PIN_BEAM.1),
            Some(0xFFFF_99C0u32 as i32)
        );
        assert_eq!(
            (PLAYER_DATA, DATA_MAP_ID, RECORD_OWNER, MAP_ARG_OFFSET),
            (0x5A0, 8, 2, 4)
        );
        // Each displacement (site+1 .. site+5) stays inside one 64-byte cache line.
        for (s, _) in [
            SITE_MAP,
            SITE_PARTY,
            SITE_TAG,
            SITE_PIN_MAP,
            SITE_PIN_WORLD,
            SITE_PIN_BEAM,
        ] {
            assert_eq!((s + 1) / 64, (s + 4) / 64);
        }
    }

    #[test]
    fn rival_markers_are_hidden_own_and_teammate_markers_are_not() {
        // Seen live: p1's marker record names map id 1, p2's names 2 (MEMORY_MAP §20 "Map markers").
        let ids = marker_ids([Some(2), Some(3)].into_iter(), Some(1));
        assert!(rival_marker(true, 2, &ids) && rival_marker(true, 3, &ids));
        assert!(!rival_marker(true, 1, &ids), "LOCAL's own marker stays");
        assert!(!rival_marker(true, 4, &ids), "a teammate's marker stays");
        assert!(!rival_marker(false, 2, &ids), "off outside a match");
        assert!(!rival_marker(true, 0, &ids));
    }

    #[test]
    fn marker_ids_skip_local_unreadable_and_repeats() {
        assert_eq!(
            marker_ids(
                [Some(1), None, Some(2), Some(2), Some(0), Some(0xFFFF)].into_iter(),
                Some(1)
            ),
            vec![2]
        );
        assert_eq!(marker_ids((1..20).map(Some), None).len(), MAX_RIVALS);
    }

    #[test]
    fn rivals_are_hidden_teammates_are_not() {
        let rivals = [0x10, 0x20];
        assert!(hide(true, 0x10, &rivals));
        assert!(hide(true, 0x20, &rivals), "the lock-on target too");
        assert!(!hide(true, 0x30, &rivals), "a teammate stays");
        assert!(!hide(false, 0x10, &rivals), "off outside a match");
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
