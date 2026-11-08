//! Dev build only: the `hawk_*` console commands (CONSOLE §0) and the all-ids trace of HAWK-3. They override the `[hawk]`
//! settings for this session; `hawk_off` turns everything off and puts the game's switch back.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::time::Instant;

use super::*;
use crate::overlay::shared::HawkCmd;

/// `hawk_trace all`: ids below this are tracked; an id that changes more than [`NOISY`] times in one tick is muted.
const ALL_IDS: usize = 2048;
const NOISY: u32 = 8;
const RING: usize = 512;
const MAX_LINES_PER_TICK: usize = 40;

pub(super) static TRACE_ALL: AtomicBool = AtomicBool::new(false);
/// `hawk_trace all`: last answer per id for the rider, muted ids, and a ring of `(id << 32) | value` changes.
static ALL_LAST: [AtomicI32; ALL_IDS] = [const { AtomicI32::new(i32::MIN) }; ALL_IDS];
static ALL_MUTE: [AtomicU8; ALL_IDS] = [const { AtomicU8::new(0) }; ALL_IDS];
static RING_BUF: [AtomicU64; RING] = [const { AtomicU64::new(0) }; RING];
static RING_HEAD: AtomicU32 = AtomicU32::new(0);

/// Hook side of `hawk_trace all` (atomics only).
pub(super) fn record_all(id: u32, ans: i32) {
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

/// One-line state for the console reply.
pub(crate) fn status_line() -> String {
    let c = ctl();
    let mut s = c.reply.clone();
    if !s.is_empty() {
        s.push_str("; ");
    }
    let on = |b: bool| if b { "ON" } else { "OFF" };
    let _ = write!(
        s,
        "swap {}, trace {}{}, drop {}, tree {}, cruise {} (phase {}, dive hold {} / {} ms, start {}, boost {}), rider 0x{:X}, local 0x{:X}, answered drop {} / tree {} / boost {}",
        on(c.want),
        on(c.trace),
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
        on(TREE.load(Ordering::Relaxed)),
        on(CRUISE.load(Ordering::Relaxed)),
        phase_name(PHASE.load(Ordering::Relaxed)),
        DIVE_HOLD.load(Ordering::Relaxed),
        DIVE_MS.load(Ordering::Relaxed),
        start_name(START_MODE.load(Ordering::Relaxed)),
        on(BOOST.load(Ordering::Relaxed)),
        RIDER.load(Ordering::Relaxed),
        LOCAL.load(Ordering::Relaxed),
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
        HawkCmd::Ride(kind, secs, rise) => {
            drop(c);
            let local = LOCAL.load(Ordering::Relaxed);
            if rise > 0 && local != 0 {
                // Probe: lift LOCAL first (one move, straight up), then ask the ride while it falls.
                let r = crate::game::spawn::world_position_of(local, local, base).map(|p| {
                    crate::game::spawn::teleport(local, base, [p[0], p[1] + rise as f32, p[2]])
                });
                tracing::info!("FS7-5 lift {rise} m before the ride: {r:?}");
            }
            request_ride(i32::from(kind), secs);
            c = ctl();
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

/// Main loop side of `hawk_trace all`: log the changed ids, mute the noisy ones.
pub(super) fn drain_all(c: &mut Ctl) {
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

fn start_name(m: u8) -> &'static str {
    match m {
        START_SHOW => "show (tree start animation)",
        START_LATE => "late (vanilla start, tree ride 2 s later, skip)",
        _ => "skip (tree start, animation cut)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_names_cover_every_mode() {
        assert!(start_name(START_SKIP).starts_with("skip"));
        assert!(start_name(START_LATE).starts_with("late"));
        assert!(start_name(START_SHOW).starts_with("show"));
    }
}
