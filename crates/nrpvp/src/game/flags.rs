//! UI-2: read and write event flags from the DLL (the channel to our event-script events in `gamedata/emevd-events.json`).
//!
//! Layout: the ER `CSEventFlagMan` layout of `probe2.rs`, verified on NR 1.03.3 (UI-0 step 5: structural checks pass, 6001 "always ON"
//! reads ON, 7500 goes ON at DAY I). Slot `exe+0x3C115A8` (TC-004). Bit order: byte = n / 8, bit = 7 - n % 8 (fsrs `EventFlag`).
//! **Writes are limited to our group 10007** ([`OUR_FIRST`]..=[`OUR_LAST`]: storage present, no reference in any NR talk script or event
//! script, UI-0 step 5), so the game never writes the same byte concurrently. One byte read-modify-write through `mem::write_u8`.

use super::mem::RegionCache;
use super::probe2::{
    block_addr, checks_ok, find_group, flag_split, parse_layout, structural_checks, EF_HEADER_LEN,
    NODE_IS_NIL, NODE_LEN,
};

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
    l.blocks_readable = rc.readable_len(l.blocks, 1) > 0;
    l.head_readable = rc.readable_len(l.head, NODE_LEN) >= NODE_LEN;
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
    let (addr, bit) = locate(base, flag)?;
    let b = super::mem::read::<u8>(addr).ok_or("flag byte unreadable")?;
    let prev = (b >> bit) & 1 == 1;
    if prev != on {
        super::mem::write_u8(addr, with_bit(b, bit, on))?;
    }
    Ok(prev)
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
    fn bits_are_set_and_cleared() {
        assert_eq!(with_bit(0b0000_0000, 7, true), 0b1000_0000);
        assert_eq!(with_bit(0b1111_1111, 0, false), 0b1111_1110);
        assert_eq!(with_bit(0b0000_0100, 2, true), 0b0000_0100);
    }
}
