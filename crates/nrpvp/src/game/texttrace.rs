//! UI-0 step 5 (dev only, `ui_text_trace = true`): log every text id the game looks up, so the menus' text ids (and the ones
//! Seamless Co-op replaces) can be read from the log instead of from screenshots.
//!
//! No code byte of the game or of `nrsc.dll` is written. Same mechanism as `steam::lobtrace` / `steam::quietinvite`: hardware
//! EXECUTE breakpoints on every thread, caught by a vectored exception handler (VEH). Seamless Co-op is known to wrap the game's
//! message repository (S7 1.1), so patching the lookup would be a second hook on code it may already hook; a debug register does not
//! touch the bytes and still fires when the entry has been patched with a jump.
//!
//! Addresses (game 1.03.3, exe 1.3.3.0; UI-0 results log, step 5; found with `.claude/archive/experiments/UI-0/msgrepo_xrefs.py`):
//! - DR0 `exe+0x27F4D20` `LookupEntry(repo, slot, category, id)`: rcx = `MsgRepository`, edx = slot, r8d = FMG file id in the msgbnd
//!   (200 = CL_MenuText, 504 = CL_MenuText_dlc01, ...), r9d = text id. Same shape as ER's `MsgRepositoryImp::LookupEntry`.
//! - DR1 `exe+0x27F5710` the FMG search it tail-jumps to: rcx = FMG, edx = text id. Some getters reach the text by another
//!   (obfuscated) path; every lookup that reads an FMG ends here. The FMG pointer is mapped back to its category through the
//!   repository table (`[repo+8][slot][category]`, sizes at `repo+0x10` / `repo+0x14`, read from `LookupEntry`'s bounds checks).
//! - The `MsgRepository` slot `exe+0x3C246A8` [VERIFIED: TC-004 singleton log].
//!
//! The first bytes of both functions are checked before arming; a jump there (someone hooked it) is logged with its target module.
//!
//! The VEH is allocation-free and lock-free: a lock-free "seen" set drops repeats, new keys go into a static ring, the trace thread
//! logs them. Every hit costs an exception round trip, so menus may stutter while the trace is armed; it disarms itself after
//! [`TRACE_DURATION`]. Log lines start with `ui_text_trace:`.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::STATUS_SINGLE_STEP;
use windows::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, RemoveVectoredExceptionHandler, EXCEPTION_POINTERS,
};
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::steam::lobtrace::{
    apply_all, module_of, EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH,
    TARGETS,
};

/// `LookupEntry` (game 1.03.3, exe 1.3.3.0; UI-0 step 5).
pub(crate) const RVA_LOOKUP: usize = 0x27F_4D20;
/// The FMG search `LookupEntry` jumps to (game 1.03.3, exe 1.3.3.0; UI-0 step 5).
pub(crate) const RVA_FMG: usize = 0x27F_5710;
/// The `MsgRepository` singleton slot (game 1.03.3, exe 1.3.3.0; TC-004).
pub(crate) const RVA_MSGREPO: usize = 0x3C2_46A8;
/// `cmp edx,[rcx+10h]; jae; cmp r8d,[rcx+14h]; jae; mov rax,[rcx+8]`.
pub(crate) const LOOKUP_BYTES: [u8; 15] = [
    0x3B, 0x51, 0x10, 0x73, 0x29, 0x44, 0x3B, 0x41, 0x14, 0x73, 0x23, 0x48, 0x8B, 0x41, 0x08,
];
/// `xor r9d,r9d; mov r11,rcx; cmp edx,[rcx+2Ch]; jb`.
pub(crate) const FMG_BYTES: [u8; 11] = [
    0x45, 0x33, 0xC9, 0x4C, 0x8B, 0xD9, 0x3B, 0x51, 0x2C, 0x72, 0x42,
];

const TRACE_DURATION: Duration = Duration::from_secs(20 * 60);
const APPLY_EVERY: Duration = Duration::from_secs(2);
const TICK: Duration = Duration::from_millis(250);
const STATS_EVERY: Duration = Duration::from_secs(15);
const SEEN_SLOTS: usize = 1 << 15;
const RING_SLOTS: usize = 4096;
/// Upper bounds for the repository table walk (sanity, not game facts).
const MAX_SLOTS: u32 = 16;
const MAX_CATS: u32 = 4096;

const KIND_LOOKUP: u64 = 1;
const KIND_FMG: u64 = 2;

// Shared with the VEH.
static ADDR: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
static SEEN: [AtomicU64; SEEN_SLOTS] = [const { AtomicU64::new(0) }; SEEN_SLOTS];
static RING: [[AtomicU64; 3]; RING_SLOTS] =
    [const { [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)] }; RING_SLOTS];
static RING_STATE: [AtomicU32; RING_SLOTS] = [const { AtomicU32::new(0) }; RING_SLOTS];
static RING_NEXT: AtomicUsize = AtomicUsize::new(0);
static HITS: AtomicU64 = AtomicU64::new(0);
static DROPPED: AtomicU32 = AtomicU32::new(0);
static SEEN_FULL: AtomicU32 = AtomicU32::new(0);

const SLOT_FREE: u32 = 0;
const SLOT_WRITING: u32 = 1;
const SLOT_READY: u32 = 2;

// ---------------------------------------------------------------------------------------------------------------------------
// Pure parts (unit tested)
// ---------------------------------------------------------------------------------------------------------------------------

/// Dedupe key of a hit; never 0 (0 marks an empty slot of the seen set).
pub(crate) fn key(kind: u64, a: u64, id: u32) -> u64 {
    let k = match kind {
        KIND_LOOKUP => (1u64 << 63) | ((a & 0x7FFF_FFFF) << 32) | u64::from(id),
        // FMG pointers are 8-aligned user-space addresses: mix them with the id (a collision only hides one line).
        _ => (1u64 << 62) ^ (a >> 3).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ u64::from(id),
    };
    if k == 0 {
        1
    } else {
        k
    }
}

/// Insert `k` into an open-addressing set of atomics. `Some(true)` = new, `Some(false)` = already there, `None` = set full.
pub(crate) fn seen_insert(set: &[AtomicU64], k: u64) -> Option<bool> {
    let n = set.len();
    let mut i = (k.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 17) as usize % n;
    for _ in 0..64 {
        match set[i].compare_exchange(0, k, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return Some(true),
            Err(cur) if cur == k => return Some(false),
            Err(_) => i = (i + 1) % n,
        }
    }
    None
}

/// What the first bytes of a function say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// The bytes of game 1.03.3.
    Expected,
    /// A `jmp rel32` (E9) or `jmp [rip+d]` (FF 25): someone hooked it. Target address.
    Hooked(u64),
    Unknown,
}

/// Classify `bytes` (read at `addr`) against `expected`.
pub(crate) fn classify(
    bytes: &[u8],
    expected: &[u8],
    addr: u64,
    read_u64: impl Fn(u64) -> Option<u64>,
) -> Entry {
    if bytes.len() >= expected.len() && bytes[..expected.len()] == *expected {
        return Entry::Expected;
    }
    match bytes {
        [0xE9, a, b, c, d, ..] => {
            let rel = i32::from_le_bytes([*a, *b, *c, *d]);
            Entry::Hooked(addr.wrapping_add(5).wrapping_add(rel as i64 as u64))
        }
        [0xFF, 0x25, a, b, c, d, ..] => {
            let rel = i32::from_le_bytes([*a, *b, *c, *d]);
            read_u64(addr.wrapping_add(6).wrapping_add(rel as i64 as u64))
                .map_or(Entry::Unknown, Entry::Hooked)
        }
        _ => Entry::Unknown,
    }
}

/// FMG file id -> name in `msg/<lang>/menu_dlc01.msgbnd.dcx` and `item_dlc01.msgbnd.dcx` (game 1.03.3; WitchyBND `_witchy-bnd4.xml`
/// of both bundles, UI-0 step 5). The two bundles share one id space without overlaps.
pub(crate) fn category_name(cat: u32) -> &'static str {
    match cat {
        1 => "TalkMsg",
        2 => "BloodMsg",
        3 => "MovieSubtitle",
        10 => "GoodsName",
        11 => "WeaponName",
        12 => "ProtectorName",
        13 => "AccessoryName",
        14 => "MagicName",
        18 => "NpcName",
        19 => "PlaceName",
        20 => "GoodsInfo",
        21 => "WeaponInfo",
        22 => "ProtectorInfo",
        23 => "AccessoryInfo",
        24 => "GoodsCaption",
        25 => "WeaponCaption",
        26 => "ProtectorCaption",
        27 => "AccessoryCaption",
        28 => "MagicInfo",
        29 => "MagicCaption",
        31 => "NetworkMessage",
        32 => "ActionButtonText",
        33 => "EventTextForTalk",
        34 => "EventTextForMap",
        41 => "GoodsDialog",
        42 => "ArtsName",
        43 => "ArtsCaption",
        44 => "AttachEffectName",
        45 => "ArtsInfo",
        46 => "GoodsInfo2",
        47 => "AntiqueName",
        48 => "AntiqueInfo",
        49 => "AntiqueCaption",
        50 => "AttachEffectInfo",
        51 => "PermanentBuffName",
        52 => "PermanentBuffInfo",
        53 => "PermanentBuffCaption",
        200 => "CL_MenuText",
        201 => "CL_LineHelp",
        202 => "CL_KeyGuide",
        203 => "CL_System_Message_win64",
        204 => "CL_Dialogues",
        205 => "LoadingTitle",
        206 => "LoadingText",
        207 => "TutorialTitle",
        208 => "TutorialBody",
        209 => "TextEmbedImageName_win64",
        210 => "ToS_win64",
        211 => "PersonalScenarioObjective",
        212 => "PersonalScenarioTitle",
        213 => "PersonalScenarioBody",
        214 => "SpEffectName",
        215 => "SpEffectInfo",
        216 => "SpEffectCaption",
        400..=428 => "(item _dlc01)",
        500 => "ActionButtonText_dlc01",
        501 => "CL_Dialogues_dlc01",
        502 => "CL_KeyGuide_dlc01",
        503 => "CL_LineHelp_dlc01",
        504 => "CL_MenuText_dlc01",
        505 => "CL_System_Message_win64_dlc01",
        506 => "EventTextForMap_dlc01",
        507 => "EventTextForTalk_dlc01",
        508 => "NetworkMessage_dlc01",
        509..=513 => "(menu _dlc01)",
        515 => "TalkMsg_dlc01",
        516 => "TutorialTitle_dlc01",
        517 => "TutorialBody_dlc01",
        _ => "?",
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// The vectored exception handler
// ---------------------------------------------------------------------------------------------------------------------------

/// Runs on the faulting thread with an arbitrary lock state: atomics only, no allocation, no logging.
unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS; both pointers are null-checked before use.
    unsafe {
        if info.is_null() {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let rec = (*info).ExceptionRecord;
        let ctx = (*info).ContextRecord;
        if rec.is_null() || ctx.is_null() || (*rec).ExceptionCode != STATUS_SINGLE_STEP {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let rip = (*ctx).Rip as usize;
        let (kind, a, id) = if rip != 0 && rip == ADDR[0].load(Ordering::Relaxed) {
            // LookupEntry(repo, slot, category, id): (slot, category) packed in `a`.
            (
                KIND_LOOKUP,
                ((*ctx).Rdx & 0xFFFF) << 16 | ((*ctx).R8 & 0xFFFF),
                (*ctx).R9 as u32,
            )
        } else if rip != 0 && rip == ADDR[1].load(Ordering::Relaxed) {
            (KIND_FMG, (*ctx).Rcx, (*ctx).Rdx as u32)
        } else {
            return EXCEPTION_CONTINUE_SEARCH;
        };
        HITS.fetch_add(1, Ordering::Relaxed);
        match seen_insert(&SEEN, key(kind, a, id)) {
            Some(true) => push(kind, a, id),
            Some(false) => {}
            None => {
                SEEN_FULL.fetch_add(1, Ordering::Relaxed);
            }
        }
        (*ctx).EFlags |= EFLAGS_RF;
    }
    EXCEPTION_CONTINUE_EXECUTION
}

fn push(kind: u64, a: u64, id: u32) {
    let start = RING_NEXT.fetch_add(1, Ordering::Relaxed);
    for j in 0..8 {
        let i = (start + j) % RING_SLOTS;
        if RING_STATE[i]
            .compare_exchange(
                SLOT_FREE,
                SLOT_WRITING,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            RING[i][0].store(kind, Ordering::Relaxed);
            RING[i][1].store(a, Ordering::Relaxed);
            RING[i][2].store(u64::from(id), Ordering::Relaxed);
            RING_STATE[i].store(SLOT_READY, Ordering::Release);
            return;
        }
    }
    DROPPED.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------------------------------------------------------
// The trace thread
// ---------------------------------------------------------------------------------------------------------------------------

/// Start the trace thread (developer mode with `ui_text_trace = true`). `base` / `size`: the image of `nightreign.exe`.
pub(crate) fn spawn(base: usize, size: usize) {
    let r = std::thread::Builder::new().name("nrpvp-texttrace".into()).spawn(move || {
        if std::panic::catch_unwind(|| run(base, size)).is_err() {
            tracing::error!("ui_text_trace: trace thread panicked; debug registers may stay armed until the game exits");
        }
    });
    if r.is_err() {
        tracing::warn!("ui_text_trace: could not spawn the trace thread");
    }
}

fn read_u64(addr: u64) -> Option<u64> {
    super::mem::read::<u64>(addr as usize)
}

fn check_entry(name: &str, addr: usize, expected: &[u8]) -> bool {
    let Some(bytes) = super::mem::read::<[u8; 16]>(addr) else {
        tracing::warn!("ui_text_trace: {name} at 0x{addr:X} is unreadable; not armed");
        return false;
    };
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    match classify(&bytes, expected, addr as u64, read_u64) {
        Entry::Expected => {
            tracing::info!(
                "ui_text_trace: {name} at 0x{addr:X}: bytes as in game 1.03.3 (not hooked): {}",
                hex.join(" ")
            );
            true
        }
        Entry::Hooked(t) => {
            let m =
                module_of(t).map_or("no module".to_string(), |(n, off)| format!("{n}+0x{off:X}"));
            tracing::info!(
                "ui_text_trace: {name} at 0x{addr:X} starts with a JUMP to 0x{t:X} ({m}): another module hooks it; the breakpoint still fires at the entry. Bytes: {}",
                hex.join(" ")
            );
            true
        }
        Entry::Unknown => {
            tracing::warn!(
                "ui_text_trace: {name} at 0x{addr:X}: unexpected bytes {} (not game 1.03.3?); not armed",
                hex.join(" ")
            );
            false
        }
    }
}

/// FMG pointer -> (slot, category), read from the repository table.
fn fmg_map(base: usize) -> HashMap<u64, (u32, u32)> {
    let mut m = HashMap::new();
    let Some(repo) = read_u64((base + RVA_MSGREPO) as u64).filter(|p| *p != 0) else {
        return m;
    };
    let (Some(table), Some(slots), Some(cats)) = (
        read_u64(repo + 8),
        super::mem::read::<u32>(repo as usize + 0x10),
        super::mem::read::<u32>(repo as usize + 0x14),
    ) else {
        return m;
    };
    for s in 0..slots.min(MAX_SLOTS) {
        let Some(row) = read_u64(table + u64::from(s) * 8).filter(|p| *p != 0) else {
            continue;
        };
        for c in 0..cats.min(MAX_CATS) {
            if let Some(f) = read_u64(row + u64::from(c) * 8).filter(|p| *p != 0) {
                m.insert(f, (s, c));
            }
        }
    }
    m
}

fn run(base: usize, size: usize) {
    let _ = size;
    let lookup = base + RVA_LOOKUP;
    let fmg = base + RVA_FMG;
    let mut addrs = [0u64; TARGETS];
    let mut mask = 0u8;
    if check_entry("LookupEntry", lookup, &LOOKUP_BYTES) {
        addrs[0] = lookup as u64;
        ADDR[0].store(lookup, Ordering::Relaxed);
        mask |= 1;
    }
    if check_entry("FMG search", fmg, &FMG_BYTES) {
        addrs[1] = fmg as u64;
        ADDR[1].store(fmg, Ordering::Relaxed);
        mask |= 2;
    }
    if mask == 0 {
        tracing::warn!("ui_text_trace: nothing to arm");
        return;
    }
    // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature and returns CONTINUE_SEARCH for anything not on our addresses.
    let handle = unsafe { AddVectoredExceptionHandler(1, Some(veh)) };
    if handle.is_null() {
        tracing::warn!("ui_text_trace: AddVectoredExceptionHandler failed; nothing armed");
        return;
    }
    // SAFETY: GetCurrentThreadId has no preconditions.
    let own = unsafe { GetCurrentThreadId() };
    let pid = std::process::id();
    let mut applied: HashMap<u32, u8> = HashMap::new();
    let mut scratch: Vec<u32> = Vec::with_capacity(2048);
    let start = Instant::now();
    let mut next_apply = start;
    let mut next_stats = start + STATS_EVERY;
    let mut map = fmg_map(base);
    let mut map_built = Instant::now();
    tracing::info!(
        "ui_text_trace: ARMING (mask {mask:#b}, {} FMGs in the repository table); runs {} min, hardware breakpoints, no code is patched",
        map.len(),
        TRACE_DURATION.as_secs() / 60
    );
    let mut logged: HashSet<(u32, u32)> = HashSet::new();
    let mut unknown_fmg: u32 = 0;
    let (mut last_hits, mut last_t) = (0u64, start);
    loop {
        std::thread::sleep(TICK);
        let now = Instant::now();
        if now.duration_since(start) >= TRACE_DURATION {
            break;
        }
        if now >= next_apply {
            let (written, failed, first) =
                apply_all(pid, own, &addrs, mask, false, &mut applied, &mut scratch);
            if written > 0 || failed > 0 {
                tracing::debug!(
                    "ui_text_trace: armed {written} new thread(s), {} in total, {failed} failed{}",
                    applied.len(),
                    first.map_or(String::new(), |(t, e)| format!("; first: thread {t}: {e}"))
                );
            }
            next_apply = now + APPLY_EVERY;
        }
        for i in 0..RING_SLOTS {
            if RING_STATE[i].load(Ordering::Acquire) != SLOT_READY {
                continue;
            }
            let kind = RING[i][0].load(Ordering::Relaxed);
            let a = RING[i][1].load(Ordering::Relaxed);
            let id = RING[i][2].load(Ordering::Relaxed) as u32;
            RING_STATE[i].store(SLOT_FREE, Ordering::Release);
            let (cat, how) = if kind == KIND_LOOKUP {
                (
                    (a & 0xFFFF) as u32,
                    format!("LookupEntry slot {}", (a >> 16) & 0xFFFF),
                )
            } else {
                if !map.contains_key(&a) && map_built.elapsed() > Duration::from_secs(1) {
                    map = fmg_map(base);
                    map_built = Instant::now();
                }
                match map.get(&a) {
                    Some((s, c)) => (*c, format!("FMG search (slot {s})")),
                    None => {
                        unknown_fmg += 1;
                        if unknown_fmg <= 20 {
                            tracing::info!("ui_text_trace: id {id} from an FMG not in the repository table (0x{a:X})");
                        }
                        continue;
                    }
                }
            };
            if logged.insert((cat, id)) {
                tracing::info!(
                    "ui_text_trace: TEXT {} {cat} id {id} [{how}]",
                    category_name(cat)
                );
            }
        }
        if now >= next_stats {
            let h = HITS.load(Ordering::Relaxed);
            let secs = now.duration_since(last_t).as_secs_f64().max(0.001);
            tracing::info!(
                "ui_text_trace: stats: {} unique texts, {:.0} lookups/s, dropped {}, seen-set full {}, unknown FMGs {unknown_fmg}",
                logged.len(),
                (h - last_hits) as f64 / secs,
                DROPPED.load(Ordering::Relaxed),
                SEEN_FULL.load(Ordering::Relaxed)
            );
            (last_hits, last_t) = (h, now);
            next_stats = now + STATS_EVERY;
        }
    }
    let (written, failed, _) = apply_all(pid, own, &addrs, 0, true, &mut applied, &mut scratch);
    // SAFETY: `handle` came from AddVectoredExceptionHandler above and is removed once.
    unsafe { RemoveVectoredExceptionHandler(handle) };
    tracing::info!(
        "ui_text_trace: finished after {} min: {} unique texts; debug registers cleared on {written} thread(s) ({failed} failed)",
        TRACE_DURATION.as_secs() / 60,
        logged.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_nonzero_and_separate_kinds() {
        assert_ne!(key(KIND_LOOKUP, 0, 0), 0);
        assert_ne!(key(KIND_LOOKUP, 200, 130410), key(KIND_LOOKUP, 504, 130410));
        assert_ne!(key(KIND_LOOKUP, 200, 1), key(KIND_FMG, 200, 1));
        assert_eq!(
            key(KIND_FMG, 0x7FF0_0000_1000, 5),
            key(KIND_FMG, 0x7FF0_0000_1000, 5)
        );
    }

    #[test]
    fn seen_set_reports_new_then_old_then_full() {
        let set: Vec<AtomicU64> = (0..4).map(|_| AtomicU64::new(0)).collect();
        assert_eq!(seen_insert(&set, 11), Some(true));
        assert_eq!(seen_insert(&set, 11), Some(false));
        for k in 12..15 {
            assert_eq!(seen_insert(&set, k), Some(true));
        }
        assert_eq!(seen_insert(&set, 99), None);
    }

    #[test]
    fn entry_bytes_are_classified() {
        let none = |_| None;
        let mut b = LOOKUP_BYTES.to_vec();
        b.push(0xCC);
        assert_eq!(classify(&b, &LOOKUP_BYTES, 0x1000, none), Entry::Expected);
        // jmp rel32 +0x10 from 0x1000 -> 0x1015
        assert_eq!(
            classify(&[0xE9, 0x10, 0, 0, 0, 0x90], &LOOKUP_BYTES, 0x1000, none),
            Entry::Hooked(0x1015)
        );
        // jmp [rip+0] at 0x1000 -> pointer read from 0x1006
        let rd = |a| (a == 0x1006).then_some(0xABCD);
        assert_eq!(
            classify(&[0xFF, 0x25, 0, 0, 0, 0, 0x90], &LOOKUP_BYTES, 0x1000, rd),
            Entry::Hooked(0xABCD)
        );
        assert_eq!(
            classify(&[0x90; 16], &LOOKUP_BYTES, 0x1000, none),
            Entry::Unknown
        );
    }

    #[test]
    fn category_names_match_the_witchy_manifest() {
        assert_eq!(category_name(200), "CL_MenuText");
        assert_eq!(category_name(504), "CL_MenuText_dlc01");
        assert_eq!(category_name(33), "EventTextForTalk");
        assert_eq!(category_name(9999), "?");
    }
}
