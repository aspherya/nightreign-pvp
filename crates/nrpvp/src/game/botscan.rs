//! BOT-0 spike aids (dev only; BOTS.md §2f): are the host's bots on a client's game, and what does the bot manager's solo gate count?
//!
//! - `bots_scan`: walk every WorldChrMan character set (`[WorldChrMan+0x170B8 + i*8]`, i = 0..=0x84, the handle lookup `exe+0x518F30`
//!   bounds; MEMORY_MAP §15) and list the characters that look like Nightfarer bots: NpcParam 600040000 (bot tables), 6000300x0 (the
//!   small-base invaders, OFF-3) or a handle in set 0x0E (bits 20..27, the bot set). That NPC sets share the player ChrSet layout
//!   (`+0x10` capacity, `+0x18` entries, 16 bytes each) is a HYPOTHESIS: an entry counts only when it is a pointer and reads a handle.
//! - `bots_gate [hold <ms>]`: read the i32 the bot instruction `2003[97]` checks (`[[exe+0x3C13258]+0xDC8]+0x14`, refused when above 1;
//!   OFF-3 static, game 1.03.3), or hold it at 1 for `ms` milliseconds from a short-lived thread (written every millisecond), then
//!   write the old value back. A spike aid: what else reads the value is UNKNOWN.
//!
//! Log lines start with `botscan:`.

use std::time::{Duration, Instant};

use super::layout::{
    CHRSET_CAPACITY, CHRSET_ENTRIES, CHRSET_ENTRY_SIZE, CHR_HANDLE, CHR_ID, CHR_TEAM,
};
use super::mem;

/// WorldChrMan slot (exe 1.3.3.0; `game/puppet.rs` uses the same).
const RVA_WCM_SLOT: usize = 0x3C0_F0A8;
/// WorldChrMan -> character-set pointers (OFF-5 static reading of `exe+0x518F30`, game 1.03.3).
const WCM_CHRSETS: usize = 0x170B8;
const WCM_CHRSET_COUNT: usize = 0x85;
/// A set larger than this is not plausible.
const MAX_CAPACITY: u32 = 0x2000;
/// The solo gate: `[[exe+0x3C13258]+0xDC8]+0x14` (i32; OFF-3 static, game 1.03.3).
const RVA_GATE_OWNER: usize = 0x3C1_3258;
const GATE_OBJ: usize = 0xDC8;
const GATE_COUNT: usize = 0x14;

fn base() -> Result<usize, String> {
    super::puppet::exe_base()
}

fn is_bot_like(npc: u32, handle: u64) -> bool {
    npc == super::layout::BOT_NPC_PARAM
        || (600_030_000..=600_039_999).contains(&npc)
        || ((handle >> 20) & 0xFF) == 0x0E
}

/// `bots_scan`: every bot-like character in every set.
pub(crate) fn scan_cmd() -> Result<String, String> {
    let b = base()?;
    let wcm = mem::read::<usize>(b + RVA_WCM_SLOT)
        .filter(|&w| w != 0)
        .ok_or("no WorldChrMan")?;
    let mut sets = 0;
    let mut chars = 0;
    let mut found = Vec::new();
    for i in 0..WCM_CHRSET_COUNT {
        let Some(set) = mem::read::<usize>(wcm + WCM_CHRSETS + i * 8).filter(|&p| mem::is_ptr(p))
        else {
            continue;
        };
        let Some(cap) =
            mem::read::<u32>(set + CHRSET_CAPACITY).filter(|&c| c > 0 && c <= MAX_CAPACITY)
        else {
            continue;
        };
        let Some(entries) = mem::read::<usize>(set + CHRSET_ENTRIES).filter(|&p| mem::is_ptr(p))
        else {
            continue;
        };
        sets += 1;
        for k in 0..cap as usize {
            let Some(chr) =
                mem::read::<usize>(entries + k * CHRSET_ENTRY_SIZE).filter(|&c| mem::is_ptr(c))
            else {
                continue;
            };
            let Some(handle) = mem::read::<u64>(chr + CHR_HANDLE) else {
                continue;
            };
            chars += 1;
            let npc = mem::read::<u32>(chr + CHR_ID).unwrap_or(0);
            if is_bot_like(npc, handle) {
                let team = mem::read::<u8>(chr + CHR_TEAM).unwrap_or(0);
                let hp = super::views::ChrIns::new(&super::gamemem::RealMem, chr)
                    .hp()
                    .map_or("-".to_string(), |h| h.to_string());
                found.push(format!(
                    "  set {i} [{k}] 0x{handle:X} chr 0x{chr:X} npc {npc} team {team} hp {hp}"
                ));
            }
        }
    }
    let line = format!(
        "botscan: {} bot-like character(s) in {sets} set(s), {chars} character(s) scanned",
        found.len()
    );
    tracing::info!("{line}");
    for f in &found {
        tracing::info!("botscan: {}", f.trim());
    }
    Ok(std::iter::once(line)
        .chain(found)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn gate_addr() -> Result<usize, String> {
    let b = base()?;
    let owner = mem::read::<usize>(b + RVA_GATE_OWNER)
        .filter(|&p| mem::is_ptr(p))
        .ok_or("gate owner null")?;
    let obj = mem::read::<usize>(owner + GATE_OBJ)
        .filter(|&p| mem::is_ptr(p))
        .ok_or("gate object null")?;
    Ok(obj + GATE_COUNT)
}

/// `bots_gate [hold <ms>]`.
pub(crate) fn gate_cmd(args: &[String]) -> Result<String, String> {
    let addr = gate_addr()?;
    let now = mem::read::<i32>(addr).ok_or("gate unreadable")?;
    match args.first().map(String::as_str) {
        None => {
            let line = format!("botscan: solo gate = {now} (2003[97] refuses above 1)");
            tracing::info!("{line}");
            Ok(line)
        }
        Some("hold") => {
            let ms: u64 = args
                .get(1)
                .and_then(|s| s.parse().ok())
                .filter(|&m| (1..=10_000).contains(&m))
                .ok_or("usage: bots_gate hold <1..10000 ms>")?;
            std::thread::spawn(move || {
                let end = Instant::now() + Duration::from_millis(ms);
                let mut writes = 0u32;
                let mut seen_other = 0u32;
                while Instant::now() < end {
                    if mem::read::<i32>(addr) != Some(1) {
                        seen_other += 1;
                        let _ = mem::write_bytes(addr, &1i32.to_le_bytes());
                        writes += 1;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                let _ = mem::write_bytes(addr, &now.to_le_bytes());
                tracing::info!(
                    "botscan: gate hold over: {writes} write(s), {seen_other} time(s) the game had changed it back; restored {now}"
                );
            });
            let line = format!("botscan: holding the solo gate at 1 for {ms} ms (was {now})");
            tracing::info!("{line}");
            Ok(line)
        }
        Some(_) => Err("usage: bots_gate [hold <ms>]".into()),
    }
}

/// `bots_kill [n|all] [mob]` (test aid, BOT-2): HP 0 on living rival bot `n` (1-based, in bot-set order) or on all of them (the bot
/// tables' Nightfarers that are not on the ally team 2), with the bot's last attacker written as LOCAL (a rival kill: out) or, with
/// `mob`, as a made-up enemy handle (a mob kill: the fill replaces it). The game then kills the bot as it would after a hit.
pub(crate) fn kill_cmd(args: &[String]) -> Result<String, String> {
    let which = args.first().map(String::as_str).unwrap_or("all");
    let by_mob = args.get(1).is_some_and(|a| a == "mob");
    let b = base()?;
    let wcm = mem::read::<usize>(b + RVA_WCM_SLOT)
        .filter(|&w| w != 0)
        .ok_or("no WorldChrMan")?;
    let rm = super::gamemem::RealMem;
    let bots: Vec<usize> = super::views::bot_set_chrs(&rm, wcm)
        .into_iter()
        .filter(|&c| {
            let v = super::views::ChrIns::new(&rm, c);
            v.npc_id() == Some(super::layout::BOT_NPC_PARAM)
                && !matches!(v.team(), Some(0 | 2) | None)
                && v.hp().is_some_and(|h| h > 0)
        })
        .collect();
    let chosen: Vec<usize> = if which == "all" {
        bots.clone()
    } else {
        let n: usize = which.parse().map_err(|_| "usage: bots_kill [n|all]")?;
        vec![*bots.get(n.wrapping_sub(1)).ok_or("no such rival bot")?]
    };
    // LOCAL = the player set's slot 0.
    let local_handle =
        mem::read::<usize>(wcm + super::layout::WCM_CHRSET + super::layout::CHRSET_ENTRIES)
            .and_then(mem::read::<usize>)
            .and_then(|l| mem::read::<u64>(l + CHR_HANDLE))
            .ok_or("LOCAL unreadable")?;
    let attacker = if by_mob {
        0x0000_0001_1100_0000u64
    } else {
        local_handle
    };
    let mut done = 0;
    for chr in &chosen {
        let _ = mem::write_bytes(
            chr + super::layout::PLAYER_LAST_ATTACKER,
            &attacker.to_le_bytes(),
        );
        let data =
            mem::read::<usize>(chr + super::layout::PLAYER_MODULES).and_then(mem::read::<usize>);
        if let Some(d) = data.filter(|&d| mem::is_ptr(d)) {
            if mem::write_bytes(d + super::layout::DATA_MODULE_HP, &0u32.to_le_bytes()).is_ok() {
                done += 1;
            }
        }
    }
    let line = format!(
        "botscan: bots_kill: HP 0 on {done} of {} living rival bot(s), last attacker 0x{attacker:X} ({})",
        bots.len(),
        if by_mob { "a mob" } else { "LOCAL" }
    );
    tracing::info!("{line}");
    Ok(line)
}
