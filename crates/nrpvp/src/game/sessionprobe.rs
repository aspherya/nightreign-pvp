//! HM-0 research (dev feature only, read-only): what `CSSessionManager` holds on this game, for the host-loss study (HOST_MIGRATION.md).
//! The field names come from Elden Ring's layout in fromsoftware-rs; the raw qwords are printed next to them so a run can check them.

use super::mem;

/// Slot of `CSSessionManager` (MEMORY_MAP §1).
const RVA_SESSION: usize = 0x3C21EC8;
/// Slot of the object whose `+0x10` is the member table (NETWORKING §2a).
const RVA_MEMBER_TABLE: usize = 0x3C078D0;
/// Stride of one `players` entry (NR 1.03.3: 3 players = 0x2D0 bytes, steam id at +0x10 of each; ER has 0x100).
const ENTRY_SIZE: usize = 0xF0;
/// At most this many `players` entries are printed.
const MAX_ENTRIES: usize = 8;

fn qwords(addr: usize, len: usize) -> String {
    let bytes = mem::copy_guarded(addr, len).0;
    bytes
        .chunks(8)
        .enumerate()
        .map(|(i, c)| {
            let mut v = [0u8; 8];
            v[..c.len()].copy_from_slice(c);
            format!("+{:03X} {:016X}", i * 8, u64::from_le_bytes(v))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `session`: role, protocol state, limit, host and every `players` entry of this game's session manager, with raw qwords.
pub(crate) fn session_cmd() -> Result<String, String> {
    let b = crate::game::puppet::exe_base()?;
    let s = mem::read::<usize>(b + RVA_SESSION)
        .filter(|s| mem::is_ptr(*s))
        .ok_or("no CSSessionManager")?;
    let u32_at = |off: usize| mem::read::<u32>(s + off).unwrap_or(u32::MAX);
    let first = mem::read::<usize>(s + 0x78).unwrap_or(0);
    let last = mem::read::<usize>(s + 0x80).unwrap_or(0);
    let count = if mem::is_ptr(first) && last >= first {
        (last - first) / ENTRY_SIZE
    } else {
        0
    };
    let mut out = vec![format!(
        "session 0x{s:X}: lobby_state {} protocol {} limit {} override {} host_steam {} players {count} (0x{:X} bytes)",
        u32_at(0xC),
        u32_at(0x10),
        u32_at(0x170),
        u32_at(0x25C),
        mem::read::<u64>(s + 0xA0).unwrap_or(0),
        last.saturating_sub(first),
    )];
    for i in 0..count.min(MAX_ENTRIES) {
        let e = first + i * ENTRY_SIZE;
        out.push(format!(
            "player {i} 0x{e:X}: steam {} +D0 {} +D4 {} flags+E0 {:016X} raw {}",
            mem::read::<u64>(e + 0x10).unwrap_or(0),
            mem::read::<i32>(e + 0xD0).unwrap_or(-2),
            mem::read::<i32>(e + 0xD4).unwrap_or(-2),
            mem::read::<u64>(e + 0xE0).unwrap_or(0),
            qwords(e + 0xD0, 0x30),
        ));
    }
    out.push(format!("raw {}", qwords(s, 0x180)));
    if let Some(t) = mem::read::<usize>(b + RVA_MEMBER_TABLE).filter(|t| mem::is_ptr(*t)) {
        out.push(format!("member table 0x{t:X}: {}", qwords(t, 0x20)));
    }
    let text = out.join("\n");
    tracing::info!("session: {text}");
    Ok(text)
}
