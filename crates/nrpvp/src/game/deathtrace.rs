//! M5-8 (dev only): what the game does after a player dies. Console `death_trace [0|1]`, `death_hold [0|1]`, `speffects` (CONSOLE §0).
//! Game 1.03.3 / exe 1.3.3.0; the reading is in `archive/research/M5-8-death.md`.
//!
//! M5-9: the hold and the release of the comeback (the start step `exe+0x5C4810` and the gate call) moved to
//! [`super::deathhold`], which runs in both builds; `death_hold` here only drives that module by hand. What stays here is the
//! logging: breakpoints, armed only while LOCAL is dead (HP 0, not downed) and `death_trace 1`, on the later comeback steps
//! [`P_BEGIN`] `exe+0x5C3F80` ("begin respawn" -> `exe+0x66E4A0(LOCAL)`) and [`P_RESPAWN`] `exe+0x5C4360` (respawn at the chosen
//! point -> `exe+0x66E8E0`), which log when they run, with the thread and the arguments. While LOCAL's HP is 0 the main loop logs
//! LOCAL's state every second. The VEH records into atomics only. Log lines start with `M5-8`.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, Once, PoisonError};
use std::time::Instant;

use windows::Win32::Foundation::STATUS_SINGLE_STEP;
use windows::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};
use windows::Win32::System::Threading::GetCurrentThreadId;

use super::gamemem::RealMem;
use super::views::ChrIns;
use super::{deathhold, layout, mem};
use crate::dr::{EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH, TARGETS};
use crate::hwbp;

const P_BEGIN: usize = 0;
const P_RESPAWN: usize = 1;
const POINTS: usize = 2;
/// (RVA, first 6 bytes in exe 1.3.3.0, name). MEMORY_MAP §17.
const TABLE: [(usize, [u8; 6], &str); POINTS] = [
    (
        0x5C_3F80,
        [0x48, 0x89, 0x5C, 0x24, 0x08, 0x48],
        "begin respawn",
    ),
    (
        0x5C_4360,
        [0x48, 0x89, 0x5C, 0x24, 0x18, 0x55],
        "respawn at point",
    ),
];
const STACK_QWORDS: usize = 8;

/// Byte with bit 0x40 = "respawn begun" (`exe+0x3CD780`, set by `exe+0x66E4A0`): logged as evidence (MEMORY_MAP §17).
const PLAYER_DEATH_BITS: usize = 0x1F4;
/// SpEffects the death trace names when LOCAL holds them (archive/research/M5-8-death.md §R1).
const WATCHED: [i32; 12] = [
    102120, 102121, 102130, 102140, 102145, 102455, 540155, 540157, 6999100, 6999500, 8970061,
    705001,
];

static TRACE: AtomicBool = AtomicBool::new(false);
/// Breakpoint addresses (0 = not resolved); never reset, so a late hit after a disarm is still recognised.
static ADDRS: [AtomicUsize; POINTS] = [const { AtomicUsize::new(0) }; POINTS];
static HITS: [AtomicU64; POINTS] = [const { AtomicU64::new(0) }; POINTS];
static FIRST_RCX: [AtomicU64; POINTS] = [const { AtomicU64::new(0) }; POINTS];
static FIRST_RDX: [AtomicU64; POINTS] = [const { AtomicU64::new(0) }; POINTS];
static FIRST_TID: [AtomicU32; POINTS] = [const { AtomicU32::new(0) }; POINTS];
static FIRST_STACK: [[AtomicU64; STACK_QWORDS]; POINTS] =
    [const { [const { AtomicU64::new(0) }; STACK_QWORDS] }; POINTS];
static FIRST_SET: [AtomicBool; POINTS] = [const { AtomicBool::new(false) }; POINTS];
static VEH_ONCE: Once = Once::new();
/// LOCAL's SpEffect ids, refreshed every tick for `speffects` (pipe thread).
static SPEFFECTS: Mutex<Vec<i32>> = Mutex::new(Vec::new());

/// Console `death_trace [0|1]` (pipe thread).
pub(crate) fn trace_cmd(on: Option<bool>) -> String {
    if let Some(v) = on {
        TRACE.store(v, Ordering::Relaxed);
        tracing::info!("M5-8 death_trace: {}", if v { "ON" } else { "OFF" });
    }
    status()
}

/// Console `death_hold [0|1]` (pipe thread): drives [`deathhold`] by hand. `0` with a held step also releases it.
pub(crate) fn hold_cmd(on: Option<bool>) -> String {
    if let Some(v) = on {
        deathhold::set_manual(v);
    }
    status()
}

fn status() -> String {
    format!(
        "death_trace {}; {}; hits begin/respawn {}/{}; lines start with M5-8 / M5-9",
        u8::from(TRACE.load(Ordering::Relaxed)),
        deathhold::status(),
        HITS[P_BEGIN].load(Ordering::Relaxed),
        HITS[P_RESPAWN].load(Ordering::Relaxed)
    )
}

/// Console `speffects` (pipe thread): LOCAL's SpEffect ids as the main loop last read them.
pub(crate) fn speffects_cmd() -> String {
    let ids = SPEFFECTS.lock().unwrap_or_else(PoisonError::into_inner);
    if ids.is_empty() {
        return "no SpEffects read (not in a world, or LOCAL not found)".into();
    }
    let named: Vec<String> = ids
        .iter()
        .filter(|i| WATCHED.contains(i))
        .map(i32::to_string)
        .collect();
    format!(
        "{} SpEffect(s): {}; of interest: {}",
        ids.len(),
        ids.iter().map(i32::to_string).collect::<Vec<_>>().join(" "),
        if named.is_empty() {
            "none".to_string()
        } else {
            named.join(" ")
        }
    )
}

fn ensure_veh() {
    VEH_ONCE.call_once(|| {
        // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature and returns CONTINUE_SEARCH for anything that is not one of
        // our breakpoint addresses.
        if unsafe { AddVectoredExceptionHandler(1, Some(veh)) }.is_null() {
            tracing::warn!("M5-8 death_trace: AddVectoredExceptionHandler failed");
        }
    });
}

unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS; pointers are null-checked. Both addresses are the first instruction of a
    // function (byte-checked before arming), so `[rsp]` is the caller's return address; stack reads are checked against the TEB
    // stack bounds. The handler only records and sets RF, so the step runs unchanged. Nothing here allocates, logs or locks.
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
        let Some(i) = (0..POINTS).find(|&i| {
            let a = ADDRS[i].load(Ordering::Relaxed);
            a != 0 && a == rip
        }) else {
            return EXCEPTION_CONTINUE_SEARCH;
        };
        let rsp = (*ctx).Rsp as usize;
        let (lo, hi) = stack_bounds();
        let stack_ok = rsp.is_multiple_of(8)
            && lo != 0
            && rsp >= lo + 0x100
            && rsp.checked_add(STACK_QWORDS * 8).is_some_and(|e| e <= hi);
        HITS[i].fetch_add(1, Ordering::Relaxed);
        if !FIRST_SET[i].swap(true, Ordering::AcqRel) {
            FIRST_RCX[i].store((*ctx).Rcx, Ordering::Relaxed);
            FIRST_RDX[i].store((*ctx).Rdx, Ordering::Relaxed);
            FIRST_TID[i].store(GetCurrentThreadId(), Ordering::Relaxed);
            for (k, slot) in FIRST_STACK[i].iter().enumerate() {
                let v = if stack_ok {
                    *((rsp + k * 8) as *const u64)
                } else {
                    0
                };
                slot.store(v, Ordering::Relaxed);
            }
        }
        (*ctx).EFlags |= EFLAGS_RF;
    }
    EXCEPTION_CONTINUE_EXECUTION
}

/// Bounds `(low, high)` of the current thread's stack from the TEB (`gs:[8]` StackBase, `gs:[16]` StackLimit).
#[inline]
fn stack_bounds() -> (usize, usize) {
    let (hi, lo): (usize, usize);
    // SAFETY: reads two qwords of the TEB through gs, which always points at the current thread's TEB on x86_64 Windows.
    unsafe {
        std::arch::asm!("mov {0}, gs:[8]", "mov {1}, gs:[16]", out(reg) hi, out(reg) lo,
            options(nostack, preserves_flags, readonly));
    }
    (lo, hi)
}

struct State {
    slots: [Option<usize>; POINTS],
    bytes_bad: bool,
    busy_logged: bool,
    seen: [u64; POINTS],
    started: Option<Instant>,
    last_watch: Option<Instant>,
    last_hp: Option<u32>,
}

static STATE: Mutex<State> = Mutex::new(State {
    slots: [None; POINTS],
    bytes_bad: false,
    busy_logged: false,
    seen: [0; POINTS],
    started: None,
    last_watch: None,
    last_hp: None,
});

fn label(v: u64, base: usize, size: usize) -> Option<String> {
    let v = v as usize;
    (v >= base && v - base < size).then(|| format!("exe+0x{:X}", v - base))
}

fn world_pos(chr: usize) -> Option<[f32; 3]> {
    let list = mem::read::<usize>(chr + layout::PLAYER_MODULES)?;
    let phys = mem::read::<usize>(list + 0x68)?;
    let p = mem::read::<[f32; 3]>(phys + layout::PHYSICS_POS)?;
    let o = mem::read::<[f32; 3]>(chr + layout::PLAYER_ORIGIN)?;
    Some([p[0] - o[0], p[1] - o[1], p[2] - o[2]])
}

/// The vtable of the object at `p` (as `exe+0x...`), for the log.
fn class_of(p: u64, base: usize, size: usize) -> String {
    let vt = mem::read::<u64>(p as usize).unwrap_or(0);
    match label(vt, base, size) {
        Some(l) => format!("vtable {l}"),
        None => "no vtable in the exe".to_string(),
    }
}

fn watch_line(chr: usize, ids: &[i32]) -> String {
    let named: Vec<String> = ids
        .iter()
        .filter(|i| WATCHED.contains(i))
        .map(i32::to_string)
        .collect();
    let pos = world_pos(chr).map_or("?".to_string(), |p| {
        format!("({:.1}, {:.1}, {:.1})", p[0], p[1], p[2])
    });
    let view = ChrIns::new(&RealMem, chr);
    format!(
        "hp={:?} state={:?} team={:?} +0x1F4={:?} pos={pos} speffects of interest: [{}]",
        view.hp(),
        view.downed_state(),
        view.team(),
        mem::read::<u8>(chr + PLAYER_DEATH_BITS).map(|b| format!("{b:#04x}")),
        named.join(" ")
    )
}

fn free_slot(taken: &[Option<usize>; POINTS]) -> Option<usize> {
    (0..TARGETS).find(|s| hwbp::is_free(*s) && !taken.contains(&Some(*s)))
}

/// Main loop (dev only), every tick: refresh LOCAL's SpEffects, arm / disarm the trace breakpoints, log what the VEH saw and,
/// while LOCAL is dead, LOCAL's state once a second.
pub(crate) fn tick(local: Option<usize>, base: usize, size: usize) {
    let ids = local
        .map(|c| ChrIns::new(&RealMem, c).speffect_ids())
        .unwrap_or_default();
    let mut st = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    // Armed only while LOCAL is dead (HP 0 and not in the downed state): the downed rules (`reviveblock`) need free slots while
    // LOCAL is down.
    let (hp, state) = local.map_or((None, None), |c| {
        let v = ChrIns::new(&RealMem, c);
        (v.hp(), v.downed_state())
    });
    let dead_now = deathhold::is_dead(hp, state);
    let traced = TRACE.load(Ordering::Relaxed);
    for i in 0..POINTS {
        match (dead_now && traced, st.slots[i]) {
            (true, None) if !st.bytes_bad => {
                let (rva, bytes, name) = TABLE[i];
                if mem::read::<[u8; 6]>(base + rva) != Some(bytes) {
                    st.bytes_bad = true;
                    tracing::warn!(
                        "M5-8 death_trace: {name} (exe+0x{rva:X}) bytes differ from exe 1.3.3.0; nothing armed"
                    );
                    continue;
                }
                let Some(s) = free_slot(&st.slots) else {
                    if !st.busy_logged {
                        st.busy_logged = true;
                        tracing::info!("M5-8 death_trace: no free hardware slot for {name}");
                    }
                    continue;
                };
                ensure_veh();
                ADDRS[i].store(base + rva, Ordering::Relaxed);
                FIRST_SET[i].store(false, Ordering::Relaxed);
                hwbp::set(s, base + rva);
                st.slots[i] = Some(s);
                st.busy_logged = false;
                if st.started.is_none() {
                    st.started = Some(Instant::now());
                }
                tracing::info!("M5-8 death_trace: armed slot {s} on exe+0x{rva:X} ({name})");
            }
            (false, Some(s)) => {
                hwbp::clear(s);
                st.slots[i] = None;
                if st.slots.iter().all(Option::is_none) {
                    st.started = None;
                    tracing::info!("M5-8 death_trace: disarmed. {}", status());
                }
            }
            _ => {}
        }
    }
    let t = st.started.map_or(0, |s| s.elapsed().as_millis());
    for i in 0..POINTS {
        let n = HITS[i].load(Ordering::Relaxed);
        if n != st.seen[i] {
            let stack: Vec<String> = FIRST_STACK[i]
                .iter()
                .enumerate()
                .filter_map(|(k, v)| {
                    label(v.load(Ordering::Relaxed), base, size)
                        .map(|l| format!("[rsp+0x{:X}]={l}", k * 8))
                })
                .collect();
            let rcx = FIRST_RCX[i].load(Ordering::Relaxed);
            let rdx = FIRST_RDX[i].load(Ordering::Relaxed);
            tracing::info!(
                "M5-8 STEP {} hit #{n} (t+{t} ms, thread {}): rcx 0x{rcx:X} ({}) rdx 0x{rdx:X} ({}); first-hit stack: {}",
                TABLE[i].2,
                FIRST_TID[i].load(Ordering::Relaxed),
                class_of(rcx, base, size),
                class_of(rdx, base, size),
                if stack.is_empty() {
                    "none in the exe".to_string()
                } else {
                    stack.join(" ")
                }
            );
            st.seen[i] = n;
        }
    }
    if let Some(chr) = local {
        if traced || deathhold::held() {
            let dead = hp == Some(0);
            let changed = hp.map(|h| h == 0) != st.last_hp.map(|h| h == 0);
            let due = st
                .last_watch
                .is_none_or(|w| w.elapsed().as_millis() >= 1000);
            if changed || (dead && due) {
                tracing::info!("M5-8 LOCAL {}", watch_line(chr, &ids));
                st.last_watch = Some(Instant::now());
            }
            st.last_hp = hp;
        }
    }
    drop(st);
    *SPEFFECTS.lock().unwrap_or_else(PoisonError::into_inner) = ids;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_report_both_switches() {
        let s = trace_cmd(None);
        assert!(s.contains("death_trace") && s.contains("death_hold"));
        assert!(
            speffects_cmd().contains("no SpEffects") || speffects_cmd().contains("SpEffect(s)")
        );
    }

    #[test]
    fn labels_only_inside_the_exe() {
        assert_eq!(label(0x1010, 0x1000, 0x100).as_deref(), Some("exe+0x10"));
        assert_eq!(label(0x1100, 0x1000, 0x100), None);
    }

    #[test]
    fn the_table_names_two_distinct_points() {
        for (i, a) in TABLE.iter().enumerate() {
            assert!(TABLE.iter().skip(i + 1).all(|b| b.0 != a.0));
        }
    }
}
