//! Hawk crossing v1 (HAWK-2, both builds; design PROJECT_PLAN §13 v1, accepted 2026-10-22): change what the engine answers the
//! player's action script (`c0000.hks`) about the start hawk ride, so it cruises the crossing line, dives while the stick is held and
//! drops with the vanilla tree-hawk buttons. Game 1.03.3 / exe 1.3.3.0; facts MEMORY_MAP §14 and TECHNICAL_NOTES §4b, REA notes
//! `Z:\Tools\rea\elden-ring-nightreign\hawk-hks-env.md`. On by default (`[hawk]` in `nrpvp.toml`, [`configure`]); the dev build's
//! console (`hawk_*`, CONSOLE §0, [`dev`]) overrides it at run time. Log lines start with `HAWK-2` (settings), `HAWK-3` (swap,
//! rider) and `HAWK-4` (ride phases).
//!
//! The script's `env(id)` reaches the engine through `exe+0xD403F0`, which tail-jumps to a **function pointer kept in data**:
//! `[[exe+0x3C1D3D8] + 0x10]`, with `ecx` = id, `rdx` = Lua state, `r8` = `[[script ctx] + 0x28]` (the character's PlayerIns,
//! VERIFIED HAWK-3 run 1). The Lua stub keeps only `eax` of the answer `[VERIFIED: static, exe+0x1624185]`. So no code is patched:
//! the slot is swapped (one atomic store) for [`env_hook`], which calls the original and changes only:
//! - cruise: 377 (`GetBirdEventID`) = 4 (start hawk) → 1 (tree hawk), so the tree-hawk loop runs (it has the exits and steering),
//!   and 394 (`GetBirdMovementDirectionTime`) answered 0, so the script sees no stick and the hawk holds its route;
//! - dive: the hook still sees the game's real 394: once it reaches the dive hold, 394 passes through (the vanilla off-route descent
//!   runs) and 384 (`GetBirdPathEscapeTime`, ms; the tree loop forces a fall at 2000) is capped below 2000 until the dive time is
//!   up. During a dive `GetSpEffectID(99230)` (env 1116) answers TRUE to the speed check, so the script's own `BirdActSpeed` goes
//!   to ×1.3; the script asks that id twice per loop frame (speed, then "exits off?"), so we answer TRUE, FALSE alternately and the
//!   exits stay on;
//! - start cut: the tree ride's start animation (`BirdAct_MoveStart`) ends when the script reads `IsAnimEnd` (env 339) TRUE, so we
//!   answer TRUE for [`SKIP_WINDOW`] after a tree start;
//! - dev only: `hawk_drop` (377 → 2, "route ended"), `hawk_tree` (tree mode without the lock), `hawk_start 2` (vanilla start, tree
//!   ride [`LATE_SWITCH`] later; the script re-checks 377 every frame).
//!
//! The vanilla tree-hawk exits (hold action, L3) work in every phase. A ride the game itself starts as a tree ride (a Spectral Hawk
//! Tree, native 377 = 1) ends the lock, so those rides stay vanilla. Only LOCAL is changed: once the main loop knows LOCAL's
//! PlayerIns ([`LOCAL`]) the hook acts on that character only; before that (the first frames after a load) on the rider rule (the
//! only character asking 377). The swap stays in place for the whole session: outside a start ride it only passes answers through
//! (0 ms cost, HAWK-3 run 1).
//!
//! The hook records into atomics only (no logging, allocation or locks); [`tick`] on the main loop logs.
//!
//! Not yet a `Feature` (ARCHITECTURE §6 R3): the trait lives on the unmerged OFF-1 branch. When it lands, [`configure`] becomes the
//! feature's constructor and [`tick`] its `Feature::tick`, with `local` read from the `World`.

#[cfg(feature = "dev")]
mod dev;
#[cfg(feature = "dev")]
pub(crate) use dev::{command, status_line};

use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering,
};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::mem;
use crate::config::HawkConfig;

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

/// Characters asking for 377 that the hook keeps apart (players only, as far as we know).
const CALLERS: usize = 16;

type EnvFn = unsafe extern "system" fn(u32, usize, usize, usize) -> u64;

/// The game's own switch (never reset to 0 once set: a call already inside [`env_hook`] still needs it after an unswap).
static ORIG: AtomicUsize = AtomicUsize::new(0);
/// The character the dev overrides and the `all` trace act on (0 = none yet).
static RIDER: AtomicUsize = AtomicUsize::new(0);
/// HAWK-2: LOCAL's PlayerIns as the main loop last saw it (0 = not known, e.g. during a load).
static LOCAL: AtomicUsize = AtomicUsize::new(0);
static DROP: AtomicBool = AtomicBool::new(false);
static TREE: AtomicBool = AtomicBool::new(false);
static DROPS_ANSWERED: AtomicU64 = AtomicU64::new(0);
static TREES_ANSWERED: AtomicU64 = AtomicU64::new(0);
/// Native tree rides (Spectral Hawk Trees) seen by the hook; each one ends the cruise lock.
static NATIVE_TREES: AtomicU64 = AtomicU64::new(0);

/// Cruise lock on (`[hawk] enabled`, console `hawk_cruise`).
static CRUISE: AtomicBool = AtomicBool::new(false);
/// Ride phase under the cruise lock: [`PHASE_NONE`], [`PHASE_CRUISE`], [`PHASE_DIVE`], [`PHASE_DONE`].
static PHASE: AtomicU8 = AtomicU8::new(PHASE_NONE);
const PHASE_NONE: u8 = 0;
const PHASE_CRUISE: u8 = 1;
const PHASE_DIVE: u8 = 2;
const PHASE_DONE: u8 = 3;
/// Real 394 value (any direction) that starts the dive (`[hawk] dive_hold`, console `hawk_dive`).
static DIVE_HOLD: AtomicI32 = AtomicI32::new(400);
/// Dive length in ms (the main loop ends it; `[hawk] dive_secs`, console `hawk_dive`).
static DIVE_MS: AtomicU32 = AtomicU32::new(7000);
/// The real 394 value that started the dive, for the log.
static DIVE_TRIGGER: AtomicI32 = AtomicI32::new(0);
/// Ride start mode ([`START_SHOW`], [`START_SKIP`], [`START_LATE`]) and the dive speed boost.
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

/// Main-loop state (only the main loop touches it).
struct Ctl {
    /// We want the swap in place (cruise from the config, or a dev trace / drop / tree).
    want: bool,
    trace: bool,
    obj: usize,
    slot: usize,
    drop_until: Option<Instant>,
    #[cfg(feature = "dev")]
    ring_tail: u32,
    /// What [`tick`] last logged per caller and id.
    logged: [[i32; 4]; CALLERS],
    logged_chr: [usize; CALLERS],
    rider_note: usize,
    /// LOCAL's PlayerIns last tick; a change (area load) empties the caller table.
    last_local: Option<usize>,
    /// The phase last logged, and when the dive started.
    phase_logged: u8,
    dive_since: Option<Instant>,
    skip_since: Option<Instant>,
    vanilla_since: Option<Instant>,
    native_trees_logged: u64,
    reply: String,
}

static CTL: Mutex<Ctl> = Mutex::new(Ctl {
    want: false,
    trace: false,
    obj: 0,
    slot: 0,
    drop_until: None,
    #[cfg(feature = "dev")]
    ring_tail: 0,
    logged: [[i32::MIN; 4]; CALLERS],
    logged_chr: [0; CALLERS],
    rider_note: 0,
    last_local: None,
    phase_logged: PHASE_NONE,
    dive_since: None,
    skip_since: None,
    vanilla_since: None,
    native_trees_logged: 0,
    reply: String::new(),
});

/// Whether the hook's overrides act on `chr`: LOCAL once known, else the rider (or anyone while no rider is known yet: a ride's
/// first frames can come before the main loop has picked one).
fn is_mine(chr: usize, local: usize, rider: usize) -> bool {
    if local != 0 {
        chr == local
    } else {
        (chr != 0 && chr == rider) || rider == 0
    }
}

unsafe extern "system" fn env_hook(id: u32, l: usize, chr: usize, r9: usize) -> u64 {
    // SAFETY: ORIG is the game's env switch, read from the slot before the swap and never 0 while the slot points here; the
    // arguments are passed on unchanged in the same registers (the game reached the slot by a tail-jump with these four).
    let v = unsafe {
        std::mem::transmute::<usize, EnvFn>(ORIG.load(Ordering::Relaxed))(id, l, chr, r9)
    };
    let ans = v as u32 as i32;
    let rider = RIDER.load(Ordering::Relaxed);
    #[cfg(feature = "dev")]
    if dev::TRACE_ALL.load(Ordering::Relaxed) && chr == rider {
        dev::record_all(id, ans);
    }
    let is_rider = chr != 0 && chr == rider;
    let mine = is_mine(chr, LOCAL.load(Ordering::Relaxed), rider);
    match id {
        ID_IS_ANIM_END if SKIP_ANIM.load(Ordering::Relaxed) && mine => {
            return LUA_TRUE;
        }
        ID_GET_SPEFFECT
            if mine
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
            let cruise = CRUISE.load(Ordering::Relaxed);
            let tree = TREE.load(Ordering::Relaxed) || cruise;
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
            // A ride the game starts as a tree ride (Spectral Hawk Tree) stays vanilla: no lock, no dive.
            if ans == EVENT_TREE_HAWK && mine && PHASE.load(Ordering::Relaxed) != PHASE_NONE {
                PHASE.store(PHASE_NONE, Ordering::Relaxed);
                NATIVE_TREES.fetch_add(1, Ordering::Relaxed);
            }
        }
        ID_BIRD_DIRECTION_TIME if mine && CRUISE.load(Ordering::Relaxed) => {
            if PHASE.load(Ordering::Relaxed) == PHASE_CRUISE {
                if ans < DIVE_HOLD.load(Ordering::Relaxed) {
                    return STICK_HIDDEN;
                }
                DIVE_TRIGGER.store(ans, Ordering::Relaxed);
                PHASE.store(PHASE_DIVE, Ordering::Relaxed);
            }
        }
        ID_BIRD_ESCAPE_TIME
            if mine
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

fn ctl() -> std::sync::MutexGuard<'static, Ctl> {
    CTL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What [`configure`] sets: the `[hawk]` settings as the hook's values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tuning {
    cruise: bool,
    dive_hold: i32,
    dive_ms: u32,
    boost: bool,
    start: u8,
}

impl Tuning {
    fn from_config(h: &HawkConfig) -> Self {
        Tuning {
            cruise: h.enabled,
            dive_hold: i32::try_from(h.dive_hold).unwrap_or(i32::MAX),
            // `parse` clamps dive_secs to a small positive range, so this never saturates.
            dive_ms: (h.dive_secs * 1000.0).round() as u32,
            boost: h.dive_boost,
            start: if h.start_cut { START_SKIP } else { START_SHOW },
        }
    }
}

/// HAWK-2, at startup (both builds): the `[hawk]` settings. Enabled = the swap goes in as soon as the env object exists and every
/// start ride cruises; the console's `hawk_*` commands (dev build) change these values later.
pub(crate) fn configure(h: &HawkConfig) {
    let t = Tuning::from_config(h);
    CRUISE.store(t.cruise, Ordering::Relaxed);
    DIVE_HOLD.store(t.dive_hold, Ordering::Relaxed);
    DIVE_MS.store(t.dive_ms, Ordering::Relaxed);
    BOOST.store(t.boost, Ordering::Relaxed);
    START_MODE.store(t.start, Ordering::Relaxed);
    PHASE.store(PHASE_NONE, Ordering::Relaxed);
    ctl().want = t.cruise;
    if t.cruise {
        tracing::info!(
            "HAWK-2 hawk crossing ON: start rides cruise the route; a {}-unit stick hold starts a {} ms dive (boost {}, start animation {})",
            t.dive_hold,
            t.dive_ms,
            if t.boost { "on" } else { "off" },
            if t.start == START_SKIP { "cut" } else { "shown" }
        );
    } else {
        tracing::info!("HAWK-2 hawk crossing OFF ([hawk] enabled = false): vanilla start rides");
    }
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

/// Main loop, every tick (both builds): keep the swap in place, end a drop, pick the rider, run the ride phases, log what changed.
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
    LOCAL.store(local.unwrap_or(0), Ordering::Relaxed);
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
    #[cfg(feature = "dev")]
    if dev::TRACE_ALL.load(Ordering::Relaxed) {
        dev::drain_all(&mut c);
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

/// Logs each new character asking hawk ids; with the dev trace on, every change of their answers.
fn log_callers(c: &mut Ctl, local: Option<usize>) {
    /// Most trace lines per tick.
    const MAX_LINES_PER_TICK: usize = 40;
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

/// Log phase changes and end the dive after its time (stop capping 384, so the vanilla tree loop drops the player); close the
/// start-animation skip window; arm the late switch.
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
    let natives = NATIVE_TREES.load(Ordering::Relaxed);
    if natives != c.native_trees_logged {
        c.native_trees_logged = natives;
        tracing::info!(
            "HAWK-4 the game started a tree ride itself (Spectral Hawk Tree): no cruise lock on it"
        );
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
    }

    #[test]
    fn watched_ids_are_the_documented_bird_ids() {
        assert_eq!(WATCHED, [377, 384, 394, 395]);
    }

    #[test]
    fn overrides_act_on_local_once_known() {
        // LOCAL known: only LOCAL, even when another character is the rider.
        assert!(is_mine(0xA0, 0xA0, 0xB0));
        assert!(!is_mine(0xB0, 0xA0, 0xB0));
        assert!(!is_mine(0xB0, 0xA0, 0));
        // LOCAL not known yet: the rider, or anyone while there is no rider.
        assert!(is_mine(0xB0, 0, 0xB0));
        assert!(!is_mine(0xC0, 0, 0xB0));
        assert!(is_mine(0xC0, 0, 0));
    }

    #[test]
    fn default_settings_are_the_accepted_hawk4_values() {
        // HAWK-4 (Leonardo, 2026-10-22): cruise on, dive hold 400, dive 7000 ms, start animation cut, boost on.
        let t = Tuning::from_config(&HawkConfig::default());
        assert_eq!(
            t,
            Tuning {
                cruise: true,
                dive_hold: 400,
                dive_ms: 7000,
                boost: true,
                start: START_SKIP,
            }
        );
        let off = Tuning::from_config(&HawkConfig {
            enabled: false,
            start_cut: false,
            dive_secs: 2.5,
            ..HawkConfig::default()
        });
        assert!(!off.cruise);
        assert_eq!(off.start, START_SHOW);
        assert_eq!(off.dive_ms, 2500);
    }
}
