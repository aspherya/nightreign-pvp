//! RW-3: drop items on the ground with the game's own drop function `exe+0x576110` (the one the red phantom's "open" calls), at a
//! given block-local position. Game 1.03.3 / exe 1.3.3.0; the findings are in MEMORY_MAP §24. Both builds (the defeat drop,
//! `game::defeatdrop`, uses it); the console aid `item_drop` is dev only.
//!
//! **The call must run on the game's own thread.** The chain allocates from a per-thread heap (`gs:[0x58]` TLS), so from the mod's
//! thread it dereferences a null allocator and kills the game (E1, three crashes). A job is therefore queued ([`queue`]) and
//! carried to the game thread by an execute breakpoint on a function the game runs every frame (`exe+0x53E600`, the DeathBox
//! instance manager's update): the breakpoint handler pops the job and makes the call. [`pump`] (main loop, every pass) arms the
//! breakpoint while jobs wait and releases the debug register afterwards. The handler keeps matching the address after the release
//! (the register is cleared by the hwbp thread up to ~100 ms later; an unmatched late single-step would kill the game).
//!
//! Signature `[VERIFIED live, MEMORY_MAP §24]`: `bool drop(MapItemMan*, ItemVec*, f32 pos[3], u32* mapId, f32 yaw)`; the vector has
//! its begin at `+8`, end at `+0x10`, entries are 0x50 bytes built by `exe+0x578A60(entry, &id, count)`. Item ids: plain id =
//! weapon, `0x10000000 | id` armour, `0x20000000 | id` talisman, `0x40000000 | id` goods. The drop is created on the host and
//! replicated by the game to the clients (a pick on either side works); a drop requested on a client is forwarded by the game
//! (`exe+0x57FDE0`), not confirmed live. Position and map id are the pair `exe+0x66B800` / `exe+0x66B7C0` return for a `PlayerIns`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, Once, PoisonError};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::STATUS_SINGLE_STEP;
use windows::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};
use windows::Win32::System::Threading::GetCurrentThreadId;

use super::mem;
use crate::dr::{EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH, TARGETS};
use crate::hwbp;

/// `MapItemMan` singleton slot (MEMORY_MAP §22).
const RVA_MAP_ITEM_MAN: usize = 0x3C1_0B68;
/// `WorldChrMan` slot (as `teleport.rs`) and LOCAL's `PlayerIns` inside it (the upload filler's source).
const RVA_WCM_SLOT: usize = 0x3C0_F0A8;
const WCM_LOCAL: usize = 0x174E8;
const RVA_DROP: usize = 0x57_6110;
const RVA_ENTRY_FROM_ID: usize = 0x57_8A60;
const RVA_MAP_ID: usize = 0x66_B7C0;
const RVA_POS: usize = 0x66_B800;
/// The DeathBox instance manager's per-frame update: runs on the game's own thread (RW3 static note §2).
const RVA_FRAME: usize = 0x53_E600;
const ENTRY_SIZE: usize = 0x50;
/// `MapItemMan` pointer slots compared before and after the call (MEMORY_MAP §22).
const SLOT_FIRST: usize = 0x40;
const SLOT_END: usize = 0x100;
/// How long a job may wait for the game thread before it is reported lost.
const GIVE_UP: Duration = Duration::from_secs(10);
/// The breakpoint stays armed this long after the last job (a job may still be queued by the same defeat).
const LINGER: Duration = Duration::from_secs(2);

/// One drop: these items at this block-local position of this map.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Job {
    /// `(item id, count)`.
    pub(crate) items: Vec<(u32, u16)>,
    /// The map id as `exe+0x66B7C0` returns it.
    pub(crate) map: u32,
    /// Block-local position as `exe+0x66B800` returns it.
    pub(crate) pos: [f32; 3],
    /// Who asked (for the log).
    pub(crate) tag: String,
}

struct Queue {
    jobs: Vec<(Job, Instant)>,
    base: usize,
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue {
    jobs: Vec::new(),
    base: 0,
});
static LAST: Mutex<String> = Mutex::new(String::new());
static FRAME_ADDR: AtomicUsize = AtomicUsize::new(0);
/// Jobs the game thread has finished (for the console aid's wait).
static DONE: AtomicUsize = AtomicUsize::new(0);
/// While a job is in flight: log a first-chance exception with its address (the stack-cookie failure that follows it hides it).
static LOGGING: AtomicBool = AtomicBool::new(false);
static VEH_ONCE: Once = Once::new();

struct Arm {
    slot: Option<usize>,
    released_at: Option<Instant>,
    busy_logged: bool,
}

static ARM: Mutex<Arm> = Mutex::new(Arm {
    slot: None,
    released_at: None,
    busy_logged: false,
});

fn record(text: String) {
    tracing::info!("{text}");
    if let Ok(mut g) = LAST.lock() {
        *g = text;
    }
}

/// The last result line (the console aid's reply).
#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn reply() -> String {
    match LAST.lock() {
        Ok(g) if !g.is_empty() => g.clone(),
        _ => "item_drop: not run yet".into(),
    }
}

/// Ask for a drop. It happens on the game thread within a frame or two once [`pump`] has armed the breakpoint.
pub(crate) fn queue(base: usize, job: Job) {
    let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    q.base = base;
    tracing::info!(
        "itemdrop: queued {} item(s) for {} (map {:#X}, pos ({:.1}, {:.1}, {:.1}))",
        job.items.len(),
        job.tag,
        job.map,
        job.pos[0],
        job.pos[1],
        job.pos[2]
    );
    q.jobs.push((job, Instant::now()));
}

/// Main loop, every pass: arm the breakpoint while jobs wait, release it afterwards, give up on a job the game thread never took.
pub(crate) fn pump() {
    let (waiting, oldest) = {
        let q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
        (q.jobs.len(), q.jobs.first().map(|j| j.1))
    };
    let mut a = ARM.lock().unwrap_or_else(PoisonError::into_inner);
    if waiting > 0 {
        a.released_at = None;
        if a.slot.is_none() {
            arm(&mut a);
        }
        if oldest.is_some_and(|t| t.elapsed() > GIVE_UP) {
            let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
            let lost = q.jobs.len();
            q.jobs.clear();
            drop(q);
            record(format!(
                "itemdrop: the game thread never reached exe+{RVA_FRAME:#X} in {} s: {lost} job(s) dropped",
                GIVE_UP.as_secs()
            ));
        }
    } else if let Some(slot) = a.slot {
        let since = *a.released_at.get_or_insert_with(Instant::now);
        if since.elapsed() >= LINGER {
            LOGGING.store(false, Ordering::SeqCst);
            hwbp::clear(slot);
            a.slot = None;
            a.released_at = None;
        }
    }
}

fn arm(a: &mut Arm) {
    let base = QUEUE.lock().unwrap_or_else(PoisonError::into_inner).base;
    let Some(slot) = (0..TARGETS).find(|s| hwbp::is_free(*s)) else {
        if !a.busy_logged {
            a.busy_logged = true;
            tracing::warn!("itemdrop: no free hardware breakpoint slot; the drop waits");
        }
        return;
    };
    a.busy_logged = false;
    VEH_ONCE.call_once(|| {
        // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature and passes on everything but our breakpoint and, while a job
        // runs, logs exceptions.
        unsafe { AddVectoredExceptionHandler(1, Some(veh)) };
    });
    FRAME_ADDR.store(base + RVA_FRAME, Ordering::SeqCst);
    LOGGING.store(true, Ordering::SeqCst);
    hwbp::set(slot, base + RVA_FRAME);
    a.slot = Some(slot);
}

unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS (null-checked). On our breakpoint address the handler runs the queued jobs
    // on this (the game's) thread and sets RF so the step runs unchanged; for any other exception it only logs and passes on.
    unsafe {
        if info.is_null() {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let rec = (*info).ExceptionRecord;
        let ctx = (*info).ContextRecord;
        if rec.is_null() || ctx.is_null() {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let frame = FRAME_ADDR.load(Ordering::SeqCst);
        if (*rec).ExceptionCode == STATUS_SINGLE_STEP {
            if frame == 0 || (*ctx).Rip as usize != frame {
                return EXCEPTION_CONTINUE_SEARCH;
            }
            let (base, jobs) = match QUEUE.try_lock() {
                Ok(mut q) => (q.base, std::mem::take(&mut q.jobs)),
                Err(_) => (0, Vec::new()),
            };
            for (job, _) in jobs {
                run_job(base, &job);
                DONE.fetch_add(1, Ordering::SeqCst);
            }
            (*ctx).EFlags |= EFLAGS_RF;
            return EXCEPTION_CONTINUE_EXECUTION;
        }
        if LOGGING.load(Ordering::SeqCst) && frame != 0 {
            let c = &*ctx;
            let base = frame - RVA_FRAME;
            let code = (*rec).ExceptionCode.0;
            // Skip the thread-name and debugger-print exceptions the engine raises as part of normal life.
            if code != 0x406D_1388_u32 as i32 && code != 0x4001_000A {
                tracing::error!(
                    "itemdrop: EXCEPTION {code:#X} at exe+{:#X} (info {:#X} {:#X}) rax {:#X} rcx {:#X} rdx {:#X}",
                    (c.Rip as usize).wrapping_sub(base),
                    (*rec).ExceptionInformation[0],
                    (*rec).ExceptionInformation[1],
                    c.Rax,
                    c.Rcx,
                    c.Rdx
                );
            }
        }
    }
    EXCEPTION_CONTINUE_SEARCH
}

fn slots(mim: usize) -> Vec<usize> {
    (SLOT_FIRST..SLOT_END)
        .step_by(8)
        .map(|o| mem::read::<usize>(mim + o).unwrap_or(0))
        .collect()
}

/// Runs on the game thread.
fn run_job(base: usize, job: &Job) {
    let text = match try_job(base, job) {
        Ok(t) | Err(t) => t,
    };
    tracing::info!(
        "itemdrop: on the game thread (tid {}): {text}",
        // SAFETY: no preconditions.
        unsafe { GetCurrentThreadId() }
    );
    if let Ok(mut g) = LAST.lock() {
        *g = text;
    }
}

fn try_job(base: usize, job: &Job) -> Result<String, String> {
    if job.items.is_empty() {
        return Err("item_drop: no items".into());
    }
    let mim = mem::read::<usize>(base + RVA_MAP_ITEM_MAN)
        .filter(|m| mem::is_ptr(*m))
        .ok_or("item_drop: no MapItemMan")?;
    // SAFETY (both transmutes): exe base + RVAs verified by static disassembly and live calls for 1.03.3 (MEMORY_MAP §24).
    let make_entry: unsafe extern "system" fn(*mut u8, *const u32, u16) -> usize =
        unsafe { std::mem::transmute(base + RVA_ENTRY_FROM_ID) };
    let drop_fn: unsafe extern "system" fn(
        usize,
        *const u64,
        *const [f32; 3],
        *const u32,
        f32,
    ) -> u64 = unsafe { std::mem::transmute(base + RVA_DROP) };

    let mut entries = vec![[0u8; ENTRY_SIZE]; job.items.len()];
    for (e, (id, n)) in entries.iter_mut().zip(&job.items) {
        // SAFETY: `e` is 0x50 bytes, the size the builder writes; `id` is a live reference.
        unsafe { make_entry(e.as_mut_ptr(), id, *n) };
    }
    let begin = entries.as_ptr() as u64;
    let end = begin + (entries.len() * ENTRY_SIZE) as u64;
    // Vector header: [0] unused head, [1] begin, [2] end, [3] capacity end.
    let vec = [0u64, begin, end, end];
    let before = slots(mim);
    // SAFETY: `vec` and `entries` outlive the call; the function does not take ownership (E1).
    let rax = unsafe { drop_fn(mim, vec.as_ptr(), &job.pos, &job.map, 0.0) };
    let after = slots(mim);
    let changed: Vec<String> = before
        .iter()
        .zip(after.iter())
        .enumerate()
        .filter(|(_, (b, a))| b != a)
        .map(|(i, (b, a))| format!("+{:#X}: {b:#X} -> {a:#X}", SLOT_FIRST + i * 8))
        .collect();
    Ok(format!(
        "item_drop: {} item(s) for {} at map {:#X} pos ({:.2}, {:.2}, {:.2}) -> rax {rax:#X}; MapItemMan slots changed: {}",
        job.items.len(),
        job.tag,
        job.map,
        job.pos[0],
        job.pos[1],
        job.pos[2],
        if changed.is_empty() {
            "none".into()
        } else {
            changed.join(", ")
        }
    ))
}

/// LOCAL's map id and block-local position, as the game's own upload filler reads them (`exe+0xB0DF60`).
pub(crate) fn local_place(base: usize) -> Result<(u32, [f32; 3]), String> {
    let wcm = mem::read::<usize>(base + RVA_WCM_SLOT)
        .filter(|w| mem::is_ptr(*w))
        .ok_or("no world")?;
    let local = mem::read::<usize>(wcm + WCM_LOCAL)
        .filter(|p| mem::is_ptr(*p))
        .ok_or("no LOCAL at WorldChrMan+0x174E8")?;
    // SAFETY: both are plain getters (they only read `[PlayerIns+0x5F0]`'s data); they ran on the mod's thread in E1 without harm.
    let get_map: unsafe extern "system" fn(usize, *mut u32) -> *const u32 =
        unsafe { std::mem::transmute(base + RVA_MAP_ID) };
    let get_pos: unsafe extern "system" fn(usize, *mut [f32; 3]) -> *const [f32; 3] =
        unsafe { std::mem::transmute(base + RVA_POS) };
    let mut map_out = 0u32;
    let mut pos_out = [0f32; 3];
    // SAFETY: see above; both out pointers are live locals.
    let (map, pos) = unsafe {
        let m = *get_map(local, &mut map_out);
        let p = *get_pos(local, &mut pos_out);
        (m, p)
    };
    Ok((map, pos))
}

/// Console aid (dev): drop one item at LOCAL's position (+ `offset_cm`) and wait for the game thread (up to 5 s). Runs on the main
/// loop, so it pumps the breakpoint itself while it waits.
#[cfg(feature = "dev")]
pub(crate) fn aid(base: usize, item_id: u32, count: u16, offset_cm: [i32; 3]) {
    let (map, pos) = match local_place(base) {
        Ok(p) => p,
        Err(e) => return record(format!("item_drop: {e}")),
    };
    let pos = [
        pos[0] + offset_cm[0] as f32 / 100.0,
        pos[1] + offset_cm[1] as f32 / 100.0,
        pos[2] + offset_cm[2] as f32 / 100.0,
    ];
    let want = DONE.load(Ordering::SeqCst) + 1;
    queue(
        base,
        Job {
            items: vec![(item_id, count)],
            map,
            pos,
            tag: "console".into(),
        },
    );
    for _ in 0..100 {
        pump();
        std::thread::sleep(Duration::from_millis(50));
        if DONE.load(Ordering::SeqCst) >= want {
            return;
        }
    }
    record("item_drop: the game thread did not take the job in 5 s (see the log)".into());
}
