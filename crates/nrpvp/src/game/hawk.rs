//! HAWK-3 / HAWK-4 (dev only; console `hawk_env`, `hawk_trace`, `hawk_drop`, `hawk_tree`, `hawk_cruise`, `hawk_dive`,
//! `hawk_start`, `hawk_boost`, `hawk_off`; CONSOLE §0): read and change what the engine answers the player's action script
//! (`c0000.hks`) about the hawk ride. Game 1.03.3 / exe 1.3.3.0; design PROJECT_PLAN §13 v1 (accepted 2026-10-22), facts MEMORY_MAP
//! §14 and TECHNICAL_NOTES §4b, REA notes `Z:\Tools\rea\elden-ring-nightreign\hawk-hks-env.md`. HAWK-2 turns this into the player
//! feature.
//!
//! The script's `env(id)` reaches the engine through `exe+0xD403F0`, which tail-jumps to a **function pointer kept in data**:
//! `[[exe+0x3C1D3D8] + 0x10]`, with `ecx` = id, `rdx` = Lua state, `r8` = `[[script ctx] + 0x28]` (the character, HYPOTHESIS until
//! the trace shows it). The Lua stub keeps only `eax` of the answer `[VERIFIED: static, exe+0x1624185]`. So no code is patched: the
//! slot is swapped (one atomic store) for [`env_hook`], which calls the original and changes only:
//! - `GetBirdEventID` (377) → 2 ("route ended") while a drop is requested: the vanilla start-hawk loop then runs the vanilla
//!   separation fall;
//! - 377 = 4 (start hawk) → 1 (tree hawk) while `hawk_tree` is on: the script would run the tree-hawk loop, which reads the stick
//!   and has its own exits. Whether the engine's route follower then steers is what the test answers.
//!
//! HAWK-4 (console `hawk_cruise`, `hawk_dive`; design PROJECT_PLAN §13 v1): cruise = tree mode at the ride start plus 394
//! (`GetBirdMovementDirectionTime`) answered 0, so the script sees no stick and the hawk holds its route. The hook still sees the
//! game's real 394: once it reaches the dive hold, 394 passes through (the vanilla off-route descent runs) and 384
//! (`GetBirdPathEscapeTime`, ms; the tree loop forces a fall at 2000) is capped below 2000 until the dive time is up. The vanilla
//! tree-hawk exits (hold action, L3) work in every phase.
//!
//! HAWK-4 polish (`hawk_start`, `hawk_boost`): the tree ride's start animation (`BirdAct_MoveStart`, the hawk grabbing the player)
//! ends when the script reads `IsAnimEnd` (env 339) TRUE, so we answer TRUE for [`SKIP_WINDOW`] after a tree start. Mode 2 keeps
//! the vanilla start (4) and switches to the tree ride [`LATE_SWITCH`] later (the script re-checks 377 every frame). During a dive,
//! `GetSpEffectID(99230)` (env 1116) answers TRUE to the speed check, so the script's own `BirdActSpeed` goes to ×1.3; the script
//! asks that id twice per loop frame (speed, then "exits off?"), so we answer TRUE, FALSE alternately and the exits stay on.
//!
//! Everything acts only on the RIDER (`r8` = PlayerIns, MEMORY_MAP §14): LOCAL's PlayerIns if it is one of the characters asking for
//! 377, else the only one asking; tree / cruise also act while no rider is known yet (the first frames of a ride).
//! The hook records into atomics only (no logging, allocation or locks); [`tick`] on the main loop logs. Log lines start with
//! `HAWK-3`.

use std::fmt::Write as _;
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering,
};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::mem;
use crate::overlay::shared::HawkCmd;

/// Global holding the env object (made at `exe+0xD5775A`, constructor `exe+0xD400B0`).
const ENV_OBJ_RVA: usize = 0x3C1D3D8;
/// Env object -> the env switch function pointer, set at run time.
const ENV_OBJ_SWITCH: usize = 0x10;

/// Engine ids of the hawk values (TECHNICAL_NOTES §4b).
const ID_BIRD_EVENT: u32 = 377;
const ID_BIRD_ESCAPE_TIME: u32 = 384;
const ID_BIRD_DIRECTION_TIME: u32 = 394;
const ID_BIRD_ACTION: u32 = 395;
/// Order of the per-character columns in [`Caller`].
const WATCHED: [u32; 4] = [
    ID_BIRD_EVENT,
    ID_BIRD_ESCAPE_TIME,
    ID_BIRD_DIRECTION_TIME,
    ID_BIRD_ACTION,
];
const EVENT_ROUTE_END: i32 = 2;
const EVENT_START_HAWK: i32 = 4;
const EVENT_TREE_HAWK: i32 = 1;

/// How long `hawk_drop` keeps answering "route ended" (the script may read it a few frames late).
const DROP_HOLD: Duration = Duration::from_millis(1500);
/// Characters asking for 377 that the hook keeps apart (players only, as far as we know).
const CALLERS: usize = 16;
/// `hawk_trace all`: ids below this are tracked; an id that changes more than [`NOISY`] times in one tick is muted.
const ALL_IDS: usize = 2048;
const NOISY: u32 = 8;
const RING: usize = 512;
const MAX_LINES_PER_TICK: usize = 40;

type EnvFn = unsafe extern "system" fn(u32, usize, usize, usize) -> u64;

/// The game's own switch (never reset to 0 once set: a call already inside [`env_hook`] still needs it after an unswap).
static ORIG: AtomicUsize = AtomicUsize::new(0);
/// The character the overrides and the `all` trace act on (0 = none yet).
static RIDER: AtomicUsize = AtomicUsize::new(0);
static DROP: AtomicBool = AtomicBool::new(false);
static TREE: AtomicBool = AtomicBool::new(false);
static TRACE_ALL: AtomicBool = AtomicBool::new(false);
static DROPS_ANSWERED: AtomicU64 = AtomicU64::new(0);
static TREES_ANSWERED: AtomicU64 = AtomicU64::new(0);

/// HAWK-4 cruise lock on (`hawk_cruise 1`).
static CRUISE: AtomicBool = AtomicBool::new(false);
/// Ride phase under the cruise lock: [`PHASE_NONE`], [`PHASE_CRUISE`], [`PHASE_DIVE`], [`PHASE_DONE`].
static PHASE: AtomicU8 = AtomicU8::new(PHASE_NONE);
const PHASE_NONE: u8 = 0;
const PHASE_CRUISE: u8 = 1;
const PHASE_DIVE: u8 = 2;
const PHASE_DONE: u8 = 3;
/// Real 394 value (any direction) that starts the dive; set by `hawk_dive`.
static DIVE_HOLD: AtomicI32 = AtomicI32::new(400);
/// Dive length in ms (the main loop ends it); set by `hawk_dive`.
static DIVE_MS: AtomicU32 = AtomicU32::new(7000);
/// The real 394 value that started the dive, for the log.
static DIVE_TRIGGER: AtomicI32 = AtomicI32::new(0);
/// HAWK-4 polish: `hawk_start` mode ([`START_SHOW`], [`START_SKIP`], [`START_LATE`]) and the dive speed boost.
static START_MODE: AtomicU8 = AtomicU8::new(START_SKIP);
const START_SHOW: u8 = 0;
const START_SKIP: u8 = 1;
const START_LATE: u8 = 2;
static BOOST: AtomicBool = AtomicBool::new(true);
/// Set by the hook when a tree start was answered; the hook answers `IsAnimEnd` TRUE while it is set; the main loop clears it.
static SKIP_ANIM: AtomicBool = AtomicBool::new(false);
/// Mode 2: the hook saw a vanilla start (4); the main loop arms [`SWITCH_NOW`] after [`LATE_SWITCH`].
static VANILLA_START: AtomicBool = AtomicBool::new(false);
static SWITCH_NOW: AtomicBool = AtomicBool::new(false);
/// Parity of the rider's `GetSpEffectID(99230)` calls during a dive (speed check, then exits check).
static BOOST_PARITY: AtomicBool = AtomicBool::new(false);
static BOOSTS_ANSWERED: AtomicU64 = AtomicU64::new(0);
/// The game's Lua "to integer" (`exe+0x1676F10`: index out of range → 0, never raises), to read `GetSpEffectID`'s id argument.
static LUA_TOINT: AtomicUsize = AtomicUsize::new(0);
const LUA_TOINT_RVA: usize = 0x1676F10;
const ID_IS_ANIM_END: u32 = 339;
const ID_GET_SPEFFECT: u32 = 1116;
const SPEFFECT_HAWK_SPEED: i32 = 99230;
const LUA_TRUE: u64 = 1;
const SKIP_WINDOW: Duration = Duration::from_millis(400);
const LATE_SWITCH: Duration = Duration::from_millis(2000);
/// The vanilla tree loop forces a fall at this escape time (TECHNICAL_NOTES §4b); the dive keeps 384 just below it.
const ESCAPE_FALL: i32 = 2000;
const STICK_HIDDEN: u64 = 0;

struct Caller {
    chr: AtomicUsize,
    calls: AtomicU64,
    /// Last answer per [`WATCHED`] id (what the game said, before any override).
    last: [AtomicI32; 4],
}
#[allow(clippy::declare_interior_mutable_const)]
const CALLER0: Caller = Caller {
    chr: AtomicUsize::new(0),
    calls: AtomicU64::new(0),
    last: [const { AtomicI32::new(i32::MIN) }; 4],
};
static CALLERS_SEEN: [Caller; CALLERS] = [CALLER0; CALLERS];

/// `hawk_trace all`: last answer per id for the rider, muted ids, and a ring of `(id << 32) | value` changes.
static ALL_LAST: [AtomicI32; ALL_IDS] = [const { AtomicI32::new(i32::MIN) }; ALL_IDS];
static ALL_MUTE: [AtomicU8; ALL_IDS] = [const { AtomicU8::new(0) }; ALL_IDS];
static RING_BUF: [AtomicU64; RING] = [const { AtomicU64::new(0) }; RING];
static RING_HEAD: AtomicU32 = AtomicU32::new(0);

/// Main-loop state (only the main loop touches it).
struct Ctl {
    /// We want the swap in place (trace, drop or tree on).
    want: bool,
    trace: bool,
    obj: usize,
    slot: usize,
    drop_until: Option<Instant>,
    ring_tail: u32,
    /// What [`tick`] last logged per caller and id.
    logged: [[i32; 4]; CALLERS],
    logged_chr: [usize; CALLERS],
    rider_note: usize,
    /// LOCAL's PlayerIns last tick; a change (area load) empties the caller table.
    last_local: Option<usize>,
    /// HAWK-4: the phase last logged, and when the dive started.
    phase_logged: u8,
    dive_since: Option<Instant>,
    skip_since: Option<Instant>,
    vanilla_since: Option<Instant>,
    reply: String,
}

static CTL: Mutex<Ctl> = Mutex::new(Ctl {
    want: false,
    trace: false,
    obj: 0,
    slot: 0,
    drop_until: None,
    ring_tail: 0,
    logged: [[i32::MIN; 4]; CALLERS],
    logged_chr: [0; CALLERS],
    rider_note: 0,
    last_local: None,
    phase_logged: PHASE_NONE,
    dive_since: None,
    skip_since: None,
    vanilla_since: None,
    reply: String::new(),
});

unsafe extern "system" fn env_hook(id: u32, l: usize, chr: usize, r9: usize) -> u64 {
    // SAFETY: ORIG is the game's env switch, read from the slot before the swap and never 0 while the slot points here; the
    // arguments are passed on unchanged in the same registers (the game reached the slot by a tail-jump with these four).
    let v = unsafe {
        std::mem::transmute::<usize, EnvFn>(ORIG.load(Ordering::Relaxed))(id, l, chr, r9)
    };
    let ans = v as u32 as i32;
    if TRACE_ALL.load(Ordering::Relaxed) && chr == RIDER.load(Ordering::Relaxed) {
        record_all(id, ans);
    }
    let rider = RIDER.load(Ordering::Relaxed);
    let is_rider = chr != 0 && chr == rider;
    match id {
        ID_IS_ANIM_END if SKIP_ANIM.load(Ordering::Relaxed) && (is_rider || rider == 0) => {
            return LUA_TRUE;
        }
        ID_GET_SPEFFECT
            if is_rider
                && BOOST.load(Ordering::Relaxed)
                && PHASE.load(Ordering::Relaxed) == PHASE_DIVE
                && lua_int_arg(l, 2) == SPEFFECT_HAWK_SPEED =>
        {
            // First call of a pair = the speed check (TRUE), second = "exits off?" (the game's own answer, FALSE: exits stay on).
            if !BOOST_PARITY.fetch_xor(true, Ordering::Relaxed) {
                BOOSTS_ANSWERED.fetch_add(1, Ordering::Relaxed);
                return LUA_TRUE;
            }
            return v;
        }
        _ => {}
    }
    let Some(col) = WATCHED.iter().position(|&w| w == id) else {
        return v;
    };
    record_caller(chr, col, ans);
    match id {
        ID_BIRD_EVENT => {
            if DROP.load(Ordering::Relaxed) && is_rider {
                DROPS_ANSWERED.fetch_add(1, Ordering::Relaxed);
                return EVENT_ROUTE_END as u64;
            }
            // While no rider is known yet too: a ride's first frames can come before the main loop has picked one.
            let cruise = CRUISE.load(Ordering::Relaxed);
            let tree = TREE.load(Ordering::Relaxed) || cruise;
            let mine = is_rider || rider == 0;
            if tree && mine && SWITCH_NOW.swap(false, Ordering::Relaxed) {
                return start_tree(cruise);
            }
            if ans == EVENT_START_HAWK && tree && mine {
                if START_MODE.load(Ordering::Relaxed) == START_LATE {
                    VANILLA_START.store(true, Ordering::Relaxed);
                    return v;
                }
                return start_tree(cruise);
            }
        }
        ID_BIRD_DIRECTION_TIME if is_rider && CRUISE.load(Ordering::Relaxed) => {
            if PHASE.load(Ordering::Relaxed) == PHASE_CRUISE {
                if ans < DIVE_HOLD.load(Ordering::Relaxed) {
                    return STICK_HIDDEN;
                }
                DIVE_TRIGGER.store(ans, Ordering::Relaxed);
                PHASE.store(PHASE_DIVE, Ordering::Relaxed);
            }
        }
        ID_BIRD_ESCAPE_TIME
            if is_rider
                && CRUISE.load(Ordering::Relaxed)
                && PHASE.load(Ordering::Relaxed) == PHASE_DIVE
                && ans >= ESCAPE_FALL =>
        {
            return (ESCAPE_FALL - 1) as u64;
        }
        _ => {}
    }
    v
}

/// Hook side of a tree start: count it, enter cruise, open the start-animation skip, answer 1.
fn start_tree(cruise: bool) -> u64 {
    TREES_ANSWERED.fetch_add(1, Ordering::Relaxed);
    if cruise {
        PHASE.store(PHASE_CRUISE, Ordering::Relaxed);
        BOOST_PARITY.store(false, Ordering::Relaxed);
    }
    if START_MODE.load(Ordering::Relaxed) != START_SHOW {
        SKIP_ANIM.store(true, Ordering::Relaxed);
    }
    EVENT_TREE_HAWK as u64
}

/// Lua argument `idx` of the current `env` call as an integer (0 when missing or not a number), through the game's own reader.
fn lua_int_arg(l: usize, idx: i32) -> i32 {
    let f = LUA_TOINT.load(Ordering::Relaxed);
    if f == 0 || l == 0 {
        return 0;
    }
    // SAFETY: `f` is the game's Lua to-integer (`exe+0x1676F10`, static reading 2026-10-22: it bounds-checks the index against the
    // stack top and converts the number; it never raises), `l` is the Lua state the game just passed to the env switch, on the
    // same thread, inside that call.
    unsafe { std::mem::transmute::<usize, unsafe extern "system" fn(usize, i32) -> i32>(f)(l, idx) }
}

/// Address of [`env_hook`], the value the swapped slot holds.
fn hook_addr() -> usize {
    env_hook as *const () as usize
}

fn record_caller(chr: usize, col: usize, ans: i32) {
    if chr == 0 {
        return;
    }
    for c in &CALLERS_SEEN {
        let cur = c.chr.load(Ordering::Relaxed);
        let mine = cur == chr
            || (cur == 0
                && match c
                    .chr
                    .compare_exchange(0, chr, Ordering::Relaxed, Ordering::Relaxed)
                {
                    Ok(_) => true,
                    Err(other) => other == chr,
                });
        if mine {
            c.calls.fetch_add(1, Ordering::Relaxed);
            c.last[col].store(ans, Ordering::Relaxed);
            return;
        }
    }
}

fn record_all(id: u32, ans: i32) {
    let i = id as usize;
    if i >= ALL_IDS || ALL_MUTE[i].load(Ordering::Relaxed) != 0 {
        return;
    }
    if ALL_LAST[i].swap(ans, Ordering::Relaxed) == ans {
        return;
    }
    let h = RING_HEAD.fetch_add(1, Ordering::Relaxed);
    RING_BUF[h as usize % RING].store(
        (u64::from(id) << 32) | u64::from(ans as u32),
        Ordering::Relaxed,
    );
}

fn ctl() -> std::sync::MutexGuard<'static, Ctl> {
    CTL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One-line state for the console reply.
pub(crate) fn status_line() -> String {
    let c = ctl();
    let mut s = c.reply.clone();
    if !s.is_empty() {
        s.push_str("; ");
    }
    let _ = write!(
        s,
        "swap {}, trace {}{}, drop {}, tree {}, cruise {} (phase {}, dive hold {} / {} ms, start {}, boost {}), rider 0x{:X}, answered drop {} / tree {} / boost {}",
        if c.want { "ON" } else { "OFF" },
        if c.trace { "ON" } else { "OFF" },
        if TRACE_ALL.load(Ordering::Relaxed) {
            " (all ids)"
        } else {
            ""
        },
        if DROP.load(Ordering::Relaxed) {
            "ACTIVE"
        } else {
            "idle"
        },
        if TREE.load(Ordering::Relaxed) {
            "ON"
        } else {
            "OFF"
        },
        if CRUISE.load(Ordering::Relaxed) {
            "ON"
        } else {
            "OFF"
        },
        phase_name(PHASE.load(Ordering::Relaxed)),
        DIVE_HOLD.load(Ordering::Relaxed),
        DIVE_MS.load(Ordering::Relaxed),
        start_name(START_MODE.load(Ordering::Relaxed)),
        if BOOST.load(Ordering::Relaxed) {
            "ON"
        } else {
            "OFF"
        },
        RIDER.load(Ordering::Relaxed),
        DROPS_ANSWERED.load(Ordering::Relaxed),
        TREES_ANSWERED.load(Ordering::Relaxed),
        BOOSTS_ANSWERED.load(Ordering::Relaxed),
    );
    s
}

/// Console command, on the main loop.
pub(crate) fn command(cmd: HawkCmd, base: usize, size: usize) {
    let mut c = ctl();
    c.reply.clear();
    match cmd {
        HawkCmd::Env => {
            let r = read_env(base, size);
            tracing::info!("HAWK-3 env: {r}");
            c.reply = r;
        }
        HawkCmd::Trace(on, all) => {
            c.trace = on;
            TRACE_ALL.store(on && all, Ordering::Relaxed);
            if on && all {
                for m in &ALL_MUTE {
                    m.store(0, Ordering::Relaxed);
                }
                c.ring_tail = RING_HEAD.load(Ordering::Relaxed);
            }
            tracing::info!(
                "HAWK-3 trace {}{}",
                if on { "ON" } else { "OFF" },
                if on && all {
                    " (all ids of the rider)"
                } else {
                    ""
                }
            );
        }
        HawkCmd::Drop => {
            if RIDER.load(Ordering::Relaxed) == 0 {
                c.reply = "no rider known yet (no character has asked for GetBirdEventID since the swap; turn hawk_trace on before the ride)".into();
            }
            DROP.store(true, Ordering::Relaxed);
            c.drop_until = Some(Instant::now() + DROP_HOLD);
            tracing::info!(
                "HAWK-3 drop: answering GetBirdEventID = 2 to the rider for {} ms",
                DROP_HOLD.as_millis()
            );
        }
        HawkCmd::Tree(on) => {
            TREE.store(on, Ordering::Relaxed);
            tracing::info!(
                "HAWK-3 tree {}: start-hawk rides answer 1 (tree hawk) instead of 4",
                if on { "ON" } else { "OFF" }
            );
        }
        HawkCmd::Cruise(on) => {
            CRUISE.store(on, Ordering::Relaxed);
            PHASE.store(PHASE_NONE, Ordering::Relaxed);
            tracing::info!(
                "HAWK-4 cruise {}: start rides run as tree rides with the stick hidden until a {}-unit hold, then a {} ms dive",
                if on { "ON" } else { "OFF" },
                DIVE_HOLD.load(Ordering::Relaxed),
                DIVE_MS.load(Ordering::Relaxed)
            );
        }
        HawkCmd::Dive(hold, ms) => {
            DIVE_HOLD.store(hold as i32, Ordering::Relaxed);
            DIVE_MS.store(ms, Ordering::Relaxed);
            tracing::info!("HAWK-4 dive: hold {hold} units of 394, length {ms} ms");
        }
        HawkCmd::Start(m) => {
            START_MODE.store(m, Ordering::Relaxed);
            tracing::info!("HAWK-4 start: {}", start_name(m));
        }
        HawkCmd::Boost(on) => {
            BOOST.store(on, Ordering::Relaxed);
            tracing::info!(
                "HAWK-4 boost {}: dives fly at the hawk's x1.3 speed",
                if on { "ON" } else { "OFF" }
            );
        }
        HawkCmd::Off => {
            CRUISE.store(false, Ordering::Relaxed);
            PHASE.store(PHASE_NONE, Ordering::Relaxed);
            c.trace = false;
            TRACE_ALL.store(false, Ordering::Relaxed);
            TREE.store(false, Ordering::Relaxed);
            DROP.store(false, Ordering::Relaxed);
            c.drop_until = None;
            c.want = false;
            unswap(&mut c, base, size);
        }
        HawkCmd::Status => {}
    }
    if !matches!(
        cmd,
        HawkCmd::Off
            | HawkCmd::Env
            | HawkCmd::Status
            | HawkCmd::Dive(..)
            | HawkCmd::Start(_)
            | HawkCmd::Boost(_)
    ) {
        // Once swapped, the pass-through stays until `hawk_off`.
        c.want = c.want
            || c.trace
            || TREE.load(Ordering::Relaxed)
            || CRUISE.load(Ordering::Relaxed)
            || DROP.load(Ordering::Relaxed);
        if c.want {
            ensure_swapped(&mut c, base, size);
        }
    }
}

/// The env object, the switch it holds now and whose code that is.
fn read_env(base: usize, size: usize) -> String {
    let Some(obj) = mem::read::<usize>(base + ENV_OBJ_RVA).filter(|&o| o != 0) else {
        return format!("env object not made yet (exe+0x{ENV_OBJ_RVA:X} is 0)");
    };
    let Some(f) = mem::read::<usize>(obj + ENV_OBJ_SWITCH) else {
        return format!("env object 0x{obj:X}: switch slot unreadable");
    };
    let hook = hook_addr();
    let who = if f == hook {
        format!(
            "ours (hawk swap in place; game switch {})",
            mem::va_rva(ORIG.load(Ordering::Relaxed), base, size)
        )
    } else if f == 0 {
        "empty".into()
    } else if (base..base + size).contains(&f) {
        format!("game code {}", mem::va_rva(f, base, size))
    } else {
        format!(
            "0x{f:X}, OUTSIDE the exe (another mod, e.g. Seamless Co-op, owns it: not swapping)"
        )
    };
    format!("env object 0x{obj:X} (exe+0x{ENV_OBJ_RVA:X}), switch at +0x{ENV_OBJ_SWITCH:X} = {who}")
}

fn ensure_swapped(c: &mut Ctl, base: usize, size: usize) {
    let Some(obj) = mem::read::<usize>(base + ENV_OBJ_RVA).filter(|&o| o != 0) else {
        c.reply = "env object not made yet; will swap when it exists".into();
        return;
    };
    let slot = obj + ENV_OBJ_SWITCH;
    let Some(cur) = mem::read::<usize>(slot) else {
        return;
    };
    let hook = hook_addr();
    if cur == hook {
        c.obj = obj;
        c.slot = slot;
        return;
    }
    if cur == 0 || !(base..base + size).contains(&cur) {
        let msg = format!("not swapping: switch slot holds 0x{cur:X}, not game code");
        if c.reply != msg {
            tracing::warn!("HAWK-3 {msg}");
        }
        c.reply = msg;
        return;
    }
    ORIG.store(cur, Ordering::SeqCst);
    LUA_TOINT.store(base + LUA_TOINT_RVA, Ordering::SeqCst);
    match mem::swap_ptr(slot, cur, hook) {
        Ok(()) => {
            c.obj = obj;
            c.slot = slot;
            tracing::info!(
                "HAWK-3 swap ON: env object 0x{obj:X}, switch {} -> nrpvp env_hook",
                mem::va_rva(cur, base, size)
            );
        }
        Err(e) => {
            tracing::warn!("HAWK-3 swap failed: {e}");
            c.reply = format!("swap failed: {e}");
        }
    }
}

fn unswap(c: &mut Ctl, base: usize, size: usize) {
    if c.slot == 0 {
        return;
    }
    let orig = ORIG.load(Ordering::SeqCst);
    match mem::swap_ptr(c.slot, hook_addr(), orig) {
        Ok(()) => tracing::info!(
            "HAWK-3 swap OFF: switch back to {}",
            mem::va_rva(orig, base, size)
        ),
        Err(e) => tracing::warn!("HAWK-3 unswap: {e}; leaving the slot alone"),
    }
    c.slot = 0;
    c.obj = 0;
}

/// Main loop, every tick: keep the swap in place, end a drop, pick the rider, log what changed.
pub(crate) fn tick(base: usize, size: usize, local: Option<usize>) {
    let mut c = ctl();
    if let Some(t) = c.drop_until {
        if Instant::now() >= t {
            DROP.store(false, Ordering::Relaxed);
            c.drop_until = None;
            tracing::info!(
                "HAWK-3 drop window over ({} answer(s) of 2 so far)",
                DROPS_ANSWERED.load(Ordering::Relaxed)
            );
        }
    }
    if local != c.last_local {
        c.last_local = local;
        for s in &CALLERS_SEEN {
            s.chr.store(0, Ordering::Relaxed);
            s.calls.store(0, Ordering::Relaxed);
            for v in &s.last {
                v.store(i32::MIN, Ordering::Relaxed);
            }
        }
        c.logged_chr = [0; CALLERS];
    }
    if !c.want {
        return;
    }
    // The env object or its switch can be re-made (area load): re-swap onto the new one.
    let obj = mem::read::<usize>(base + ENV_OBJ_RVA).unwrap_or(0);
    let cur = (obj != 0)
        .then(|| mem::read::<usize>(obj + ENV_OBJ_SWITCH))
        .flatten()
        .unwrap_or(0);
    if obj != c.obj || cur != hook_addr() {
        if c.slot != 0 {
            tracing::info!("HAWK-3 env object/switch changed (object 0x{obj:X}, switch 0x{cur:X}); swapping again");
            c.slot = 0;
        }
        ensure_swapped(&mut c, base, size);
    }
    pick_rider(&mut c, local);
    phase_tick(&mut c);
    log_callers(&mut c, local);
    if TRACE_ALL.load(Ordering::Relaxed) {
        drain_all(&mut c);
    }
}

fn pick_rider(c: &mut Ctl, local: Option<usize>) {
    let chrs: Vec<usize> = CALLERS_SEEN
        .iter()
        .map(|s| s.chr.load(Ordering::Relaxed))
        .filter(|&p| p != 0)
        .collect();
    let rider = match local {
        Some(l) if chrs.contains(&l) => l,
        _ if chrs.len() == 1 => chrs[0],
        _ => 0,
    };
    RIDER.store(rider, Ordering::Relaxed);
    if rider != 0 && rider != c.rider_note {
        c.rider_note = rider;
        if local == Some(rider) {
            tracing::info!(
                "HAWK-3 rider = LOCAL PlayerIns 0x{rider:X} (r8 of env is the PlayerIns)"
            );
        } else {
            tracing::info!(
                "HAWK-3 rider = 0x{rider:X}, the only character asking for 377; it is NOT LOCAL's PlayerIns ({}){}",
                local.map_or("unknown".into(), |l| format!("0x{l:X}")),
                local.map_or(String::new(), |l| relation(rider, l))
            );
        }
    }
}

/// Where LOCAL's PlayerIns appears inside the rider object, or the rider inside the PlayerIns (first 0x2000 bytes).
fn relation(rider: usize, local: usize) -> String {
    let find = |obj: usize, want: usize| -> Option<usize> {
        (0..0x2000 / 8)
            .find(|&k| mem::read::<usize>(obj + k * 8) == Some(want))
            .map(|k| k * 8)
    };
    match (find(rider, local), find(local, rider)) {
        (Some(o), _) => format!("; rider+0x{o:X} = PlayerIns"),
        (_, Some(o)) => format!("; PlayerIns+0x{o:X} = rider"),
        _ => "; no pointer between them in the first 0x2000 bytes".into(),
    }
}

fn log_callers(c: &mut Ctl, local: Option<usize>) {
    let mut lines = 0;
    for (i, s) in CALLERS_SEEN.iter().enumerate() {
        let chr = s.chr.load(Ordering::Relaxed);
        if chr == 0 {
            continue;
        }
        let who = if Some(chr) == local { "LOCAL" } else { "other" };
        if c.logged_chr[i] != chr {
            c.logged_chr[i] = chr;
            c.logged[i] = [i32::MIN; 4];
            tracing::info!("HAWK-3 new character asking hawk ids: 0x{chr:X} ({who})");
        }
        if !c.trace {
            continue;
        }
        for (k, id) in WATCHED.iter().enumerate() {
            let v = s.last[k].load(Ordering::Relaxed);
            if v != c.logged[i][k] && lines < MAX_LINES_PER_TICK {
                tracing::info!(
                    "HAWK-3 0x{chr:X} ({who}) env {id} {} -> {v} ({} calls)",
                    fmt_val(c.logged[i][k]),
                    s.calls.load(Ordering::Relaxed)
                );
                c.logged[i][k] = v;
                lines += 1;
            }
        }
    }
}

fn drain_all(c: &mut Ctl) {
    let head = RING_HEAD.load(Ordering::Relaxed);
    let pending = head.wrapping_sub(c.ring_tail);
    if pending as usize > RING {
        tracing::info!(
            "HAWK-3 all: {} change(s) lost (ring full)",
            pending as usize - RING
        );
        c.ring_tail = head.wrapping_sub(RING as u32);
    }
    let mut per_id = std::collections::BTreeMap::<u32, (u32, i32)>::new();
    while c.ring_tail != head {
        let e = RING_BUF[c.ring_tail as usize % RING].load(Ordering::Relaxed);
        c.ring_tail = c.ring_tail.wrapping_add(1);
        let slot = per_id.entry((e >> 32) as u32).or_insert((0, 0));
        slot.0 += 1;
        slot.1 = e as u32 as i32;
    }
    for (n, (id, (changes, v))) in per_id.into_iter().enumerate() {
        if changes > NOISY {
            ALL_MUTE[id as usize].store(1, Ordering::Relaxed);
            tracing::info!("HAWK-3 all: env {id} muted ({changes} changes in one tick; last {v})");
        } else if n < MAX_LINES_PER_TICK {
            tracing::info!("HAWK-3 all: env {id} = {v}");
        }
    }
}

/// HAWK-4: log phase changes and end the dive after its time (stop capping 384, so the vanilla tree loop drops the player); close
/// the start-animation skip window; arm the late switch.
fn phase_tick(c: &mut Ctl) {
    if SKIP_ANIM.load(Ordering::Relaxed) {
        let since = *c.skip_since.get_or_insert_with(|| {
            tracing::info!("HAWK-4 tree start: skipping the start animation");
            Instant::now()
        });
        if since.elapsed() >= SKIP_WINDOW {
            SKIP_ANIM.store(false, Ordering::Relaxed);
            c.skip_since = None;
        }
    }
    if VANILLA_START.load(Ordering::Relaxed) {
        let since = *c.vanilla_since.get_or_insert_with(|| {
            tracing::info!(
                "HAWK-4 vanilla start seen; switching to the tree ride in {} ms",
                LATE_SWITCH.as_millis()
            );
            Instant::now()
        });
        if since.elapsed() >= LATE_SWITCH {
            VANILLA_START.store(false, Ordering::Relaxed);
            SWITCH_NOW.store(true, Ordering::Relaxed);
            c.vanilla_since = None;
        }
    }
    if PHASE.load(Ordering::Relaxed) == PHASE_DIVE {
        let since = *c.dive_since.get_or_insert_with(Instant::now);
        let len = Duration::from_millis(u64::from(DIVE_MS.load(Ordering::Relaxed)));
        if since.elapsed() >= len {
            PHASE.store(PHASE_DONE, Ordering::Relaxed);
        }
    } else {
        c.dive_since = None;
    }
    let p = PHASE.load(Ordering::Relaxed);
    if p != c.phase_logged {
        let extra = if p == PHASE_DIVE {
            format!(" (real 394 = {})", DIVE_TRIGGER.load(Ordering::Relaxed))
        } else {
            String::new()
        };
        tracing::info!(
            "HAWK-4 phase {} -> {}{extra}",
            phase_name(c.phase_logged),
            phase_name(p)
        );
        c.phase_logged = p;
    }
}

fn start_name(m: u8) -> &'static str {
    match m {
        START_SHOW => "show (tree start animation)",
        START_LATE => "late (vanilla start, tree ride 2 s later, skip)",
        _ => "skip (tree start, animation cut)",
    }
}

fn phase_name(p: u8) -> &'static str {
    match p {
        PHASE_CRUISE => "cruise",
        PHASE_DIVE => "dive",
        PHASE_DONE => "dive over",
        _ => "none",
    }
}

fn fmt_val(v: i32) -> String {
    if v == i32::MIN {
        "(none)".into()
    } else {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_val_marks_never_seen() {
        assert_eq!(fmt_val(i32::MIN), "(none)");
        assert_eq!(fmt_val(4), "4");
    }

    #[test]
    fn phase_names_cover_every_phase() {
        assert_eq!(phase_name(PHASE_CRUISE), "cruise");
        assert_eq!(phase_name(PHASE_DIVE), "dive");
        assert_eq!(phase_name(PHASE_DONE), "dive over");
        assert_eq!(phase_name(PHASE_NONE), "none");
        assert!(start_name(START_SKIP).starts_with("skip"));
        assert!(start_name(START_LATE).starts_with("late"));
        assert!(start_name(START_SHOW).starts_with("show"));
    }

    #[test]
    fn watched_ids_are_the_documented_bird_ids() {
        assert_eq!(WATCHED, [377, 384, 394, 395]);
    }
}
