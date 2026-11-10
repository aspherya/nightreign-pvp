//! ARENA-3 spike 5 (dev only): what the Sparring Equipment, a level-up and the flasks change in LOCAL's `PlayerGameData`.
//! Game 1.03.3 / exe 1.3.3.0. Layout: MEMORY_MAP §24 (inventory list, equipped weapon ids) and §11 (level `+0x68`, runes `+0x6C`: a cheat-table lead; runes verified in §24, the level is checked here).
//!
//! `inv` prints the level, the runes, the six equipped weapon ids and every used inventory entry. `inv mark` keeps a copy of the
//! first [`PGD_SPAN`] bytes of `PlayerGameData` and of the inventory entries; `inv diff` lists every `u32` that changed since and the
//! inventory entries that came, went or changed. Read only; answered on the pipe thread; the full lists also go to the log (`inv:`).

use std::sync::Mutex;

use super::defeatdrop::{
    GEAR_BLOCK, INV_ARRAY, INV_CAP, INV_ENTRY, INV_HEADER, WEAPON_IDS, WEAPON_SLOTS,
};
use super::{layout, mem};

/// How much of `PlayerGameData` a mark keeps (Elden Ring's is about 0xA00; the inventory header sits at 0x3B8).
const PGD_SPAN: usize = 0x1000;
/// Level and runes inside `PlayerGameData` (MEMORY_MAP §11 lead, Elden Ring offsets; runes verified in §24, the level `[HYPOTHESIS]`).
const LEVEL: usize = 0x68;
const RUNES: usize = 0x6C;
/// A sane upper bound for the used count (never seen above ~620).
const MAX_USED: usize = 0x1000;
/// At most this many changed words in the reply (the log has all of them).
const REPLY_WORDS: usize = 60;

/// One inventory entry: `[handle, item id, quantity, sort id, flags]` (0x14 bytes, MEMORY_MAP §24).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub handle: u32,
    pub id: u32,
    pub qty: u32,
    pub sort: u32,
    pub flags: u32,
}

struct Mark {
    pgd: usize,
    bytes: Vec<u8>,
    entries: Vec<Entry>,
}

static MARK: Mutex<Option<Mark>> = Mutex::new(None);

/// LOCAL's `PlayerGameData`.
pub(crate) fn pgd() -> Result<usize, String> {
    let chr = super::teleport::local_chr()?;
    mem::read::<usize>(chr + layout::PLAYER_GAMEDATA)
        .filter(|d| mem::is_ptr(*d))
        .ok_or_else(|| "LOCAL has no PlayerGameData".to_string())
}

fn entries(pgd: usize) -> Vec<Entry> {
    let header = pgd + INV_HEADER;
    let used = mem::read::<u32>(header).unwrap_or(0) as usize;
    let cap = mem::read::<u32>(header + 4).unwrap_or(0) as usize;
    let Some(array) = mem::read::<usize>(header + INV_ARRAY).filter(|a| mem::is_ptr(*a)) else {
        return Vec::new();
    };
    // `+0x3BC` (0x180) is not a cap: the Roundtable list holds ~616 entries with a relic collection (MEMORY_MAP §29).
    if cap != INV_CAP || used > MAX_USED {
        return Vec::new();
    }
    let (b, _) = mem::copy_guarded(array, used * INV_ENTRY);
    parse_entries(&b)
}

/// The entries in a copy of the inventory array (pure).
pub(crate) fn parse_entries(b: &[u8]) -> Vec<Entry> {
    b.as_chunks::<INV_ENTRY>()
        .0
        .iter()
        .map(|c| {
            let w = |i: usize| u32::from_le_bytes([c[i], c[i + 1], c[i + 2], c[i + 3]]);
            Entry {
                handle: w(0),
                id: w(4),
                qty: w(8),
                sort: w(12),
                flags: w(16),
            }
        })
        .collect()
}

/// `(offset, old, new)` of every aligned `u32` that differs (pure).
pub(crate) fn diff_words(old: &[u8], new: &[u8]) -> Vec<(usize, u32, u32)> {
    old.as_chunks::<4>()
        .0
        .iter()
        .zip(new.as_chunks::<4>().0)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, (a, b))| (i * 4, u32::from_le_bytes(*a), u32::from_le_bytes(*b)))
        .collect()
}

/// Lines for the inventory entries that came (`+`), went (`-`) or changed (`~`), matched by handle (pure).
pub(crate) fn diff_entries(old: &[Entry], new: &[Entry]) -> Vec<String> {
    let mut out = Vec::new();
    for o in old {
        match new.iter().find(|n| n.handle == o.handle) {
            None => out.push(format!("- {}", show(o))),
            Some(n) if n != o => out.push(format!("~ {} -> {}", show(o), show(n))),
            Some(_) => {}
        }
    }
    for n in new
        .iter()
        .filter(|n| !old.iter().any(|o| o.handle == n.handle))
    {
        out.push(format!("+ {}", show(n)));
    }
    out
}

fn show(e: &Entry) -> String {
    format!(
        "h{:08X} id {:08X} x{} sort {:X} fl {:X}",
        e.handle, e.id, e.qty, e.sort, e.flags
    )
}

fn summary(pgd: usize, ents: &[Entry]) -> String {
    let level = mem::read::<u32>(pgd + LEVEL).unwrap_or(u32::MAX);
    let runes = mem::read::<u32>(pgd + RUNES).unwrap_or(u32::MAX);
    let weapons: Vec<String> = (0..WEAPON_SLOTS)
        .map(|i| {
            mem::read::<u32>(pgd + GEAR_BLOCK + WEAPON_IDS + 4 * i)
                .map_or("?".into(), |v| format!("{v:08X}"))
        })
        .collect();
    let items: Vec<String> = ents
        .iter()
        .filter(|e| e.id != u32::MAX)
        .map(|e| format!("{:08X}x{}", e.id, e.qty))
        .collect();
    format!(
        "pgd 0x{pgd:X} level {level} runes {runes} weapons [{}] inventory {}: {}",
        weapons.join(" "),
        ents.len(),
        items.join(" ")
    )
}

/// `inv [mark|diff]`.
pub(crate) fn cmd(args: &[String]) -> Result<String, String> {
    let pgd = pgd()?;
    let ents = entries(pgd);
    match args.first().map(String::as_str) {
        None => {
            for e in &ents {
                tracing::info!("inv: {}", show(e));
            }
            Ok(format!("inv: {}", summary(pgd, &ents)))
        }
        Some("mark") => {
            let (bytes, _) = mem::copy_guarded(pgd, PGD_SPAN);
            let n = ents.len();
            let s = summary(pgd, &ents);
            tracing::info!("inv: mark {s}");
            *MARK.lock().map_err(|_| "lock")? = Some(Mark {
                pgd,
                bytes,
                entries: ents,
            });
            Ok(format!("inv: marked ({n} entries) {s}"))
        }
        Some("diff") => {
            let guard = MARK.lock().map_err(|_| "lock")?;
            let m = guard.as_ref().ok_or("no mark yet: `inv mark` first")?;
            if m.pgd != pgd {
                return Err(format!(
                    "PlayerGameData moved (0x{:X} -> 0x{pgd:X}): `inv mark` again",
                    m.pgd
                ));
            }
            let (now, _) = mem::copy_guarded(pgd, PGD_SPAN);
            let words = diff_words(&m.bytes, &now);
            let items = diff_entries(&m.entries, &ents);
            let mut lines: Vec<String> = words
                .iter()
                .map(|(o, a, b)| format!("+0x{o:03X}: {a:08X} -> {b:08X} ({a} -> {b})"))
                .collect();
            for l in lines.iter().chain(items.iter()) {
                tracing::info!("inv: diff {l}");
            }
            let total = lines.len();
            lines.truncate(REPLY_WORDS);
            Ok(format!(
                "inv diff: {total} word(s) changed in PlayerGameData, {} inventory change(s)\n{}\n{}",
                items.len(),
                lines.join("\n"),
                items.join("\n")
            ))
        }
        Some(o) => Err(format!("unknown argument {o:?}")),
    }
}

/// How much of an instance `gaitem` dumps.
const GAITEM_DUMP: usize = 0x80;

/// `give`: add `qty` of item `id` to LOCAL's inventory with the game's own call and, with a slot, equip it there, on the game's
/// thread (`game::inventory`); the result goes to the log (dev only).
pub(crate) fn give(base: usize, local: Option<usize>, id: u32, qty: u32, slot: Option<u8>) {
    let Some(pgd) = local
        .and_then(|l| mem::read::<usize>(l + layout::PLAYER_GAMEDATA))
        .filter(|d| mem::is_ptr(*d))
    else {
        tracing::warn!("give: no LOCAL PlayerGameData");
        return;
    };
    super::inventory::queue(
        base,
        "give",
        Box::new(move |base| {
            let Some(index) = super::inventory::add_on_game_thread(base, pgd, id, qty) else {
                return;
            };
            if let Some(slot) = slot {
                super::inventory::equip_on_game_thread(base, pgd, slot, index);
            }
        }),
    );
}

/// `gaitem [<handle hex>]`: dump the instance of a handle, or of each of the six hand slots (`PGD+0x28C..`), as `u32` words.
pub(crate) fn gaitem_cmd(args: &[String]) -> Result<String, String> {
    let base = crate::game::puppet::exe_base()?;
    let handles: Vec<u32> = match args.first() {
        Some(h) => vec![u32::from_str_radix(h.trim_start_matches("0x"), 16)
            .map_err(|_| "handle must be hex")?],
        None => {
            let pgd = pgd()?;
            (0..super::inventory::HAND_SLOTS)
                .filter_map(|i| super::inventory::slot_handle(pgd, i))
                .collect()
        }
    };
    let mut out = Vec::new();
    for h in handles {
        let Some(p) = super::inventory::gaitem_of(base, h) else {
            out.push(format!("{h:08X}: no instance"));
            continue;
        };
        let (b, _) = mem::copy_guarded(p, GAITEM_DUMP);
        let words: Vec<String> = b
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| format!("{:08X}", u32::from_le_bytes(*c)))
            .collect();
        let line = format!("{h:08X} @0x{p:X}: {}", words.join(" "));
        tracing::info!("gaitem: {line}");
        out.push(line);
    }
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(handle: u32, id: u32, qty: u32) -> Entry {
        Entry {
            handle,
            id,
            qty,
            sort: 0,
            flags: 0,
        }
    }

    #[test]
    fn entries_parse_from_the_array_copy() {
        let mut b = Vec::new();
        for w in [1u32, 0x4000_03E9, 3, 7, 0] {
            b.extend_from_slice(&w.to_le_bytes());
        }
        b.extend_from_slice(&[0; 4]); // a partial entry is ignored
        assert_eq!(
            parse_entries(&b),
            vec![Entry {
                sort: 7,
                ..e(1, 0x4000_03E9, 3)
            }]
        );
    }

    #[test]
    fn only_changed_words_are_listed() {
        let old = [1u8, 0, 0, 0, 2, 0, 0, 0];
        let new = [1u8, 0, 0, 0, 15, 0, 0, 0];
        assert_eq!(diff_words(&old, &new), vec![(4, 2, 15)]);
    }

    #[test]
    fn entries_are_matched_by_handle() {
        let old = [e(1, 0x10, 1), e(2, 0x20, 1)];
        let new = [e(2, 0x20, 3), e(3, 0x30, 1)];
        let d = diff_entries(&old, &new);
        assert_eq!(d.len(), 3);
        assert!(d[0].starts_with("- h00000001"));
        assert!(d[1].starts_with("~ h00000002") && d[1].ends_with("x3 sort 0 fl 0"));
        assert!(d[2].starts_with("+ h00000003"));
    }
}
