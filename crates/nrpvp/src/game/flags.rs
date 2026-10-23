//! UI-2: read and write event flags from the DLL (the channel to our event-script events in `gamedata/emevd-events.json`).
//!
//! Layout: the ER `CSEventFlagMan` layout of `probe2.rs`, verified on NR 1.03.3 (UI-0 step 5: structural checks pass, 6001 "always ON"
//! reads ON, 7500 goes ON at DAY I). Slot `exe+0x3C115A8` (TC-004). Bit order: byte = n / 8, bit = 7 - n % 8 (fsrs `EventFlag`).
//! **Writes are limited to our group 10007** ([`OUR_FIRST`]..=[`OUR_LAST`]: storage present, no reference in any NR talk script or event
//! script, UI-0 step 5), so the game never writes the same byte concurrently. One byte read-modify-write through `mem::write_u8`.

use super::event_flags::{
    block_addr, checks_ok, find_group, flag_split, parse_layout, structural_checks, EF_HEADER_LEN,
    NODE_IS_NIL, NODE_LEN,
};
use super::mem::RegionCache;

/// `CSEventFlagMan` slot (game 1.03.3, exe 1.3.3.0; TC-004, UI-0 step 5).
pub(crate) const RVA_FLAGMAN_SLOT: usize = 0x3C1_15A8;
pub(crate) const OUR_FIRST: u32 = 10_007_000;
pub(crate) const OUR_LAST: u32 = 10_007_999;

/// Whether the DLL may write `flag`.
pub(crate) fn writable(flag: u32) -> bool {
    (OUR_FIRST..=OUR_LAST).contains(&flag)
}

/// `byte` with `bit` set or cleared.
pub(crate) fn with_bit(byte: u8, bit: u32, on: bool) -> u8 {
    if on {
        byte | (1 << bit)
    } else {
        byte & !(1 << bit)
    }
}

/// Address of the byte holding `flag` and its bit, through the live manager.
fn locate(base: usize, flag: u32) -> Result<(usize, u32), &'static str> {
    let mut rc = RegionCache::direct();
    let inst = rc
        .read::<usize>(base + RVA_FLAGMAN_SLOT)
        .filter(|p| *p != 0)
        .ok_or("flag manager not live")?;
    let hdr = rc.read_prefix(inst, EF_HEADER_LEN);
    let mut l = parse_layout(&hdr).ok_or("flag manager header unreadable")?;
    // M5-5: plain copies, not `readable_len` (a `VirtualQuery` even on the direct cache): the night match reads 12 flags per
    // 50 ms tick, and the queries made each tick of the Nucleus run take 1-2.4 s (2026-10-23, run 3; STATUS traps).
    l.blocks_readable = super::mem::read::<u8>(l.blocks).is_some();
    l.head_readable = super::mem::copy_out(l.head, &mut [0u8; NODE_LEN]);
    l.head_is_nil = rc.read::<u8>(l.head.wrapping_add(NODE_IS_NIL));
    if !checks_ok(&structural_checks(&l)) {
        return Err("flag manager layout checks failed");
    }
    let (group, byte, bit) = flag_split(flag, l.divisor).ok_or("divisor is zero")?;
    if byte >= l.holder_size {
        return Err("byte index outside the block");
    }
    let node = find_group(&mut rc, l.head, group)?.ok_or("the flag's group has no storage")?;
    let block =
        block_addr(node.mode, node.loc, l.blocks, l.holder_size).ok_or("no block for the group")?;
    Ok((block + byte as usize, bit))
}

/// Read `flag`.
pub(crate) fn get(base: usize, flag: u32) -> Result<bool, &'static str> {
    let (addr, bit) = locate(base, flag)?;
    let b = super::mem::read::<u8>(addr).ok_or("flag byte unreadable")?;
    Ok((b >> bit) & 1 == 1)
}

/// Set `flag` ON or OFF (our group only). Returns the previous state.
pub(crate) fn set(base: usize, flag: u32, on: bool) -> Result<bool, &'static str> {
    if !writable(flag) {
        return Err("flag outside our range 10007000..=10007999");
    }
    write(base, flag, on)
}

/// M5-5 / M6-1: the game's own flags the night match writes when our event file is not loaded (Nucleus runs, where no me3
/// package is applied): 7502 night 1 done (host only; the host's own event script then passes 7503 / 7505 to everyone, M5-4 run 2),
/// 7000 win / 7001 failure (each game for itself; M5-4 run 2: the result screen followed in both games). Each is written at most
/// once per expedition, so the read-modify-write rarely meets a game write of the same byte.
pub(crate) const MATCH_WRITABLE: [u32; 3] = [7000, 7001, 7502];

/// Set one of [`MATCH_WRITABLE`]. Returns the previous state.
pub(crate) fn set_match_flag(base: usize, flag: u32, on: bool) -> Result<bool, &'static str> {
    if !MATCH_WRITABLE.contains(&flag) {
        return Err("not a match flag the DLL may write");
    }
    write(base, flag, on)
}

fn write(base: usize, flag: u32, on: bool) -> Result<bool, &'static str> {
    let (addr, bit) = locate(base, flag)?;
    let b = super::mem::read::<u8>(addr).ok_or("flag byte unreadable")?;
    let prev = (b >> bit) & 1 == 1;
    if prev != on {
        super::mem::write_u8(addr, with_bit(b, bit, on))?;
    }
    Ok(prev)
}

/// M5-4 research (dev console `flag`): the game's own flags a developer may write besides our group, for Nucleus runs where our
/// event files are not loaded: 7000 game clear (win), 7001 game over (loss), 7502 / 7507 night 1 / night 2 done
/// (`research/M5-no-boss-nights.md`). The game writes these bytes rarely (once per expedition each), so the read-modify-write is safe
/// enough for a test tool. Features use [`set_match_flag`].
#[cfg(feature = "dev")]
pub(crate) const RESEARCH_WRITABLE: [u32; 4] = [7000, 7001, 7502, 7507];

/// Last `flag` console answer (main loop writes, pipe thread reads).
#[cfg(feature = "dev")]
static LAST_CONSOLE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Console `flag <id> [0|1]` (dev, main loop): read any flag; write ours or [`RESEARCH_WRITABLE`]. The answer is kept for
/// [`console_reply`].
#[cfg(feature = "dev")]
pub(crate) fn console_cmd(base: usize, flag: u32, value: Option<bool>) {
    let line = match value {
        None => match get(base, flag) {
            Ok(on) => format!("flag {flag}: {}", if on { "ON" } else { "off" }),
            Err(e) => format!("flag {flag}: error: {e}"),
        },
        Some(on) if writable(flag) || RESEARCH_WRITABLE.contains(&flag) => {
            match write(base, flag, on) {
                Ok(prev) => format!(
                    "flag {flag}: set {} (was {})",
                    if on { "ON" } else { "off" },
                    if prev { "ON" } else { "off" }
                ),
                Err(e) => format!("flag {flag}: error: {e}"),
            }
        }
        Some(_) => format!(
            "flag {flag}: not writable (ours 10007000..=10007999, research {RESEARCH_WRITABLE:?})"
        ),
    };
    tracing::info!("flag: {line}");
    *LAST_CONSOLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = line;
}

/// The last `flag` answer (console).
#[cfg(feature = "dev")]
pub(crate) fn console_reply() -> String {
    LAST_CONSOLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// UI-2 spike (dev, `ui_msg_test = true`): after the flag manager is live in a world, set each test flag of
/// `gamedata/emevd-events.json` in turn and read it back 2 s later (our event clears it once it has shown its message).
pub(crate) fn spawn_msg_test(base: usize) {
    const TEST: [(u32, &str); 3] = [
        (10_007_100, "dialog"),
        (10_007_101, "blinking"),
        (10_007_102, "status"),
    ];
    let r = std::thread::Builder::new().name("nrpvp-msgtest".into()).spawn(move || {
        tracing::info!("ui_msg_test: waiting for the flag manager and 25 s of world time");
        let mut live_for = 0u32;
        while live_for < 25 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            live_for = if get(base, OUR_FIRST).is_ok() { live_for + 1 } else { 0 };
        }
        for round in 1..=2 {
            for (flag, kind) in TEST {
                match set(base, flag, true) {
                    Ok(prev) => tracing::info!("ui_msg_test: round {round}: flag {flag} ({kind}) set ON (was {})", if prev { "ON" } else { "off" }),
                    Err(e) => tracing::warn!("ui_msg_test: round {round}: flag {flag} ({kind}) not set: {e}"),
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
                match get(base, flag) {
                    Ok(false) => tracing::info!("ui_msg_test: flag {flag} ({kind}) is OFF again after 2 s: our event ran"),
                    Ok(true) => tracing::info!("ui_msg_test: flag {flag} ({kind}) still ON after 2 s: our event did NOT run (yet)"),
                    Err(e) => tracing::warn!("ui_msg_test: flag {flag} read failed: {e}"),
                }
                std::thread::sleep(std::time::Duration::from_secs(8));
                let _ = set(base, flag, false);
            }
        }
        tracing::info!("ui_msg_test: done");
    });
    if r.is_err() {
        tracing::warn!("ui_msg_test: could not spawn the thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_our_group_is_writable() {
        assert!(writable(10_007_000) && writable(10_007_999) && writable(10_007_100));
        assert!(!writable(10_006_999) && !writable(10_008_000) && !writable(6001));
    }

    #[test]
    fn match_flags_are_only_the_endings_and_night_one() {
        assert_eq!(MATCH_WRITABLE, [7000, 7001, 7502]);
        assert!(MATCH_WRITABLE.iter().all(|f| !writable(*f)));
    }

    #[test]
    fn bits_are_set_and_cleared() {
        assert_eq!(with_bit(0b0000_0000, 7, true), 0b1000_0000);
        assert_eq!(with_bit(0b1111_1111, 0, false), 0b1111_1110);
        assert_eq!(with_bit(0b0000_0100, 2, true), 0b0000_0100);
    }
}
