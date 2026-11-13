//! ARENA-3 (dev only, with the Arena): LOCAL's inventory through the game's own calls. Game 1.03.3 / exe 1.3.3.0, MEMORY_MAP §29.
//!
//! - Add: `exe+0x1F05F0(EquipGameData*, const ItemSpec*, qty, bool, u8, u8)` builds the item from `spec[0]` (the id; its top nibble
//!   is the category) and returns the new entry's inventory index. `EquipGameData` = `PGD+0x1D8`.
//! - Equip: `exe+0x1F3430(EquipGameData*, slot, inventory index, bool)`, slots 0..5 = L1, R1, L2, R2, L3, R3.
//! - A weapon's instance (`CSGaitemImp`, `exe+0x3C131C8`): `+0x8` handle, `+0xC` id, `+0x18` custom weapon row (the rolled kind,
//!   `EquipParamCustomWeapon`), `+0x1C` its skill (`SwordArtsParam`); -1 = none / the default.
//!
//! **Both calls must run on the game's own thread** (they allocate from a per-thread heap; the equip from the mod's thread killed the
//! game, TRAPS): [`queue`] carries the work there with `itemdrop`'s breakpoint.

use super::defeatdrop::{GEAR_BLOCK, INV_ARRAY, INV_CAP, INV_ENTRY, INV_HEADER, WEAPON_IDS};
use super::{layout, mem};

const RVA_ADD_ITEM: usize = 0x1F_05F0;
const RVA_EQUIP: usize = 0x1F_3430;
const RVA_GAITEM_IMP: usize = 0x3C1_31C8;
const GAITEM_ARRAY: usize = 0x8;
const GAITEM_ARRAY_END: usize = 0x1_9020;
/// Inside a weapon instance.
pub(crate) const GAITEM_HANDLE: usize = 0x8;
pub(crate) const GAITEM_CUSTOM: usize = 0x18;
pub(crate) const GAITEM_SKILL: usize = 0x1C;
/// The six hand slots' handles (`PGD+0x28C..0x2A0`) and ids (`PGD+0x30C..0x320`).
pub(crate) const SLOT_HANDLES: usize = 0x28C;
pub(crate) const HAND_SLOTS: u8 = 6;
/// Empty hand: the unarmed weapon (and 0 / -1).
pub(crate) const UNARMED: u32 = 110_000;
/// The Roundtable's list holds ~616 entries (relics); `+0x3BC` is not a cap on it (MEMORY_MAP §29).
const MAX_USED: usize = 0x1000;

type AddItem = unsafe extern "system" fn(usize, *const u32, u32, u8, u8, u8) -> u32;
type Equip = unsafe extern "system" fn(usize, i32, i32, u8) -> u64;

/// LOCAL's `PlayerGameData`.
pub(crate) fn local_pgd() -> Result<usize, String> {
    let chr = super::teleport::local_chr()?;
    mem::read::<usize>(chr + layout::PLAYER_GAMEDATA)
        .filter(|d| mem::is_ptr(*d))
        .ok_or_else(|| "LOCAL has no PlayerGameData".to_string())
}

/// The weapon id in hand slot `slot` (0..5).
pub(crate) fn slot_id(pgd: usize, slot: u8) -> Option<u32> {
    mem::read::<u32>(pgd + GEAR_BLOCK + WEAPON_IDS + 4 * usize::from(slot))
}

/// The handle in hand slot `slot` (0..5).
pub(crate) fn slot_handle(pgd: usize, slot: u8) -> Option<u32> {
    mem::read::<u32>(pgd + SLOT_HANDLES + 4 * usize::from(slot))
}

/// `(item id, quantity)` of every used inventory entry.
pub(crate) fn entries(pgd: usize) -> Vec<(u32, u32)> {
    let header = pgd + INV_HEADER;
    let used = mem::read::<u32>(header).unwrap_or(0) as usize;
    let cap = mem::read::<u32>(header + 4).unwrap_or(0) as usize;
    let Some(array) = mem::read::<usize>(header + INV_ARRAY).filter(|a| mem::is_ptr(*a)) else {
        return Vec::new();
    };
    if cap != INV_CAP || used > MAX_USED {
        return Vec::new();
    }
    let (b, _) = mem::copy_guarded(array, used * INV_ENTRY);
    b.as_chunks::<INV_ENTRY>()
        .0
        .iter()
        .map(|c| {
            (
                u32::from_le_bytes([c[4], c[5], c[6], c[7]]),
                u32::from_le_bytes([c[8], c[9], c[10], c[11]]),
            )
        })
        .collect()
}

/// The instance (`GaitemIns`) of an item handle, if any (read only).
pub(crate) fn gaitem_of(base: usize, handle: u32) -> Option<usize> {
    let imp = mem::read::<usize>(base + RVA_GAITEM_IMP).filter(|p| mem::is_ptr(*p))?;
    let (b, _) = mem::copy_guarded(imp + GAITEM_ARRAY, GAITEM_ARRAY_END - GAITEM_ARRAY);
    b.as_chunks::<8>()
        .0
        .iter()
        .map(|c| usize::from_le_bytes(*c))
        .filter(|&p| mem::is_ptr(p))
        .find(|&p| mem::read::<u32>(p + GAITEM_HANDLE) == Some(handle))
}

/// Run `f` on the game's thread (within a frame or two; `itemdrop`'s breakpoint carries it).
pub(crate) fn queue(base: usize, tag: &'static str, f: super::itemdrop::Call) {
    super::itemdrop::queue_call(base, tag, f);
}

/// GAME THREAD ONLY. Add `qty` of `id`; the new entry's inventory index, or `None` when nothing was added.
pub(crate) fn add_on_game_thread(base: usize, pgd: usize, id: u32, qty: u32) -> Option<u32> {
    let before = mem::read::<u32>(pgd + INV_HEADER).unwrap_or(0);
    let spec = [id, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    // SAFETY: base + a fixed RVA inside the exe image; the signature was read from its call sites (MEMORY_MAP §29); the spec lives on
    // this stack for the call; the caller runs on the game's thread (module doc).
    let add: AddItem = unsafe { std::mem::transmute::<usize, AddItem>(base + RVA_ADD_ITEM) };
    // SAFETY: see above.
    let index = unsafe { add(pgd + GEAR_BLOCK, spec.as_ptr(), qty, 0, 0, 0) };
    let after = mem::read::<u32>(pgd + INV_HEADER).unwrap_or(0);
    tracing::info!("inventory: add {id:08X} x{qty}: index {index:#X}, count {before} -> {after}");
    // A stackable good already held raises its quantity without a new entry.
    (after > before || id >> 28 == 4).then_some(index)
}

/// GAME THREAD ONLY. Equip inventory entry `index` into hand slot `slot` (0..5); the id the slot holds afterwards.
pub(crate) fn equip_on_game_thread(base: usize, pgd: usize, slot: u8, index: u32) -> Option<u32> {
    // SAFETY: as in `add_on_game_thread`; `index` is one the add returned.
    let equip: Equip = unsafe { std::mem::transmute::<usize, Equip>(base + RVA_EQUIP) };
    // SAFETY: see above.
    unsafe { equip(pgd + GEAR_BLOCK, i32::from(slot), index as i32, 0) };
    let now = slot_id(pgd, slot);
    tracing::info!("inventory: equip slot {slot} <- index {index:#X}: holds {now:08X?}");
    now
}

/// `exe+0xDE20E0()`: packs LOCAL's PlayerGameData (`exe+0x20BDE0(PGD, buf)`) and broadcasts it to the session as packet 0x0C (0x1F8
/// bytes; skipped by the game itself when not in a session). The other games apply it to their copy of this player
/// (`GameDataMan+0x10` slots, receive `exe+0xDDAE6B` -> `exe+0x20DCB0`), which is what they draw. The add / equip calls never send
/// it; the game's ground pickup does, through `exe+0x66EFF0(LOCAL)` right after its equip (MEMORY_MAP §29, 2026-11-13).
const RVA_SEND_PLAYER_DATA: usize = 0xDE_20E0;
/// `mov [rsp+8],rbx; push rdi; sub rsp,0x240` (game 1.03.3, exe 1.3.3.0).
const SEND_PLAYER_DATA_BYTES: [u8; 12] = [
    0x48, 0x89, 0x5C, 0x24, 0x08, 0x57, 0x48, 0x81, 0xEC, 0x40, 0x02, 0x00,
];

/// GAME THREAD ONLY. Tell the other players' games what LOCAL now holds (equipment, level): the game's own player-data broadcast.
/// Without it, an equip made by us shows only on our own screen (Arena test 2026-11-12: each player saw the rival's starting weapon).
pub(crate) fn send_player_data_on_game_thread(base: usize) {
    if mem::read::<[u8; 12]>(base + RVA_SEND_PLAYER_DATA) != Some(SEND_PLAYER_DATA_BYTES) {
        tracing::warn!(
            "inventory: the player-data broadcast at exe+0x{RVA_SEND_PLAYER_DATA:X} differs from game 1.03.3; not sent"
        );
        return;
    }
    type Send = unsafe extern "system" fn() -> u8;
    // SAFETY: base + a fixed RVA inside the exe image, first bytes checked above; no arguments (it reads GameDataMan itself, checks
    // the session state, builds the packet on its own stack); the game's thread, like its pickup handler's call. Verified live with
    // `rawcall DE20E0` on two instances in the Arena (2026-11-13).
    let sent = unsafe { std::mem::transmute::<usize, Send>(base + RVA_SEND_PLAYER_DATA)() };
    tracing::info!("inventory: player data sent to the session (returned {sent})");
}

/// `GameDataMan` (`exe+0x3C078D0`): `+0x10` the other players' `PlayerGameData` copies, 0x8A0 each, `+0x8` = -1 when unused
/// (MEMORY_MAP §20, §29). What the session told this game about each other player, i.e. what it draws.
const RVA_GAME_DATA_MAN: usize = 0x3C0_78D0;
const GDM_REMOTE: usize = 0x10;
const REMOTE_SIZE: usize = 0x8A0;
const REMOTE_SLOTS: usize = 5;

/// Console `rival_gear` (read only, any thread): the hand slots this game holds for every other player, next to LOCAL's own.
pub(crate) fn rival_gear_cmd() -> Result<String, String> {
    let base = super::puppet::exe_base()?;
    let gdm = mem::read::<usize>(base + RVA_GAME_DATA_MAN)
        .filter(|p| mem::is_ptr(*p))
        .ok_or("no GameDataMan")?;
    let arr = mem::read::<usize>(gdm + GDM_REMOTE)
        .filter(|p| mem::is_ptr(*p))
        .ok_or("no remote PlayerGameData list")?;
    let hands = |pgd: usize| -> String {
        (0..HAND_SLOTS)
            .map(|s| slot_id(pgd, s).map_or("?".into(), |id| format!("{id:08X}")))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut out = match local_pgd() {
        Ok(p) => format!("rival_gear: me [{}]", hands(p)),
        Err(_) => "rival_gear: me [?]".to_string(),
    };
    for k in 0..REMOTE_SLOTS {
        let pgd = arr + k * REMOTE_SIZE;
        if mem::read::<u32>(pgd + 0x8).is_none_or(|id| id == u32::MAX) {
            continue;
        }
        out.push_str(&format!("; other {k} [{}]", hands(pgd)));
    }
    Ok(out)
}

/// The pouch (quick items): slots 32..49 of the equipment (`exe+0x1F7840`: `slot - 0x20 <= 0x11`), `(handle, index)` pairs from
/// `PGD+0x428` (slot 32). A good only shows in Nightreign when it sits in one of them (MEMORY_MAP §29).
const RVA_POUCH_FREE: usize = 0x1F_2540;
const RVA_POUCH_SET: usize = 0x1F_3960;
const POUCH: usize = 0x428;
const POUCH_FIRST: i32 = 32;
const POUCH_SLOTS: usize = 18;

/// The inventory indices held in the pouch now.
pub(crate) fn pouch_indices(pgd: usize) -> Vec<u32> {
    (0..POUCH_SLOTS)
        .filter_map(|k| mem::read::<u32>(pgd + POUCH + 8 * k + 4))
        .filter(|&i| i != u32::MAX)
        .collect()
}

/// GAME THREAD ONLY. Put inventory entry `index` into the first free pouch slot, as a pickup does (`exe+0x57A433`: the free slot
/// from `exe+0x1F2540`, then `exe+0x1F3960(EquipGameData, slot, index)`). The slot used, or `None` (pouch full / already there).
pub(crate) fn pouch_on_game_thread(base: usize, pgd: usize, index: u32) -> Option<i32> {
    if pouch_indices(pgd).contains(&index) {
        return None;
    }
    type Free = unsafe extern "system" fn(usize) -> i32;
    type Set = unsafe extern "system" fn(usize, i32, u32) -> u64;
    // SAFETY: base + fixed RVAs inside the exe image; signatures from the pickup's call site (`exe+0x57A41D..0x57A43B`); game thread.
    let free: Free = unsafe { std::mem::transmute::<usize, Free>(base + RVA_POUCH_FREE) };
    // SAFETY: see above.
    let slot = unsafe { free(pgd + GEAR_BLOCK) };
    if !(POUCH_FIRST..POUCH_FIRST + POUCH_SLOTS as i32).contains(&slot) {
        tracing::warn!("inventory: no free pouch slot for index {index:#X} ({slot})");
        return None;
    }
    // SAFETY: see above.
    let set: Set = unsafe { std::mem::transmute::<usize, Set>(base + RVA_POUCH_SET) };
    // SAFETY: see above; `slot` is the one the game picked.
    unsafe { set(pgd + GEAR_BLOCK, slot, index) };
    tracing::info!("inventory: pouch slot {slot} <- index {index:#X}");
    Some(slot)
}

/// Write a weapon instance's custom row and skill (`-1` = none).
pub(crate) fn set_kind(gaitem: usize, custom: u32, skill: u32) -> Result<(), &'static str> {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&custom.to_le_bytes());
    b[4..].copy_from_slice(&skill.to_le_bytes());
    mem::write_bytes(gaitem + GAITEM_CUSTOM, &b)
}

/// Quick items in pouch order, excluding the starting flask. Handles are resolved against inventory entries,
/// never persisted: a loaded preset must use the new instances' handles and indices.
pub(crate) fn quick_goods(pgd: usize) -> Result<Vec<(u32, u32)>, String> {
    let entries = entry_snapshot(pgd)?;
    let mut out = Vec::new();
    for slot in 0..POUCH_SLOTS {
        let handle = mem::read::<u32>(pgd + POUCH + slot * 8).ok_or("Pouch unavailable")?;
        if handle == u32::MAX || handle == 0 {
            continue;
        }
        let &(_, id, qty) = entries
            .iter()
            .find(|e| e.0 == handle)
            .ok_or("Pouch item unavailable")?;
        if qty > 999 {
            return Err("Invalid quick item quantity".into());
        }
        if id >> 28 == 4
            && qty > 0
            && !(1000..=1025).contains(&(id & 0x0fff_ffff))
            && !out.iter().any(|&(other, _)| other == id)
        {
            out.push((id, qty));
        }
    }
    Ok(out)
}

/// Read the live inventory once, so resolving eighteen pouch handles does not issue thousands of process-memory reads.
fn entry_snapshot(pgd: usize) -> Result<Vec<(u32, u32, u32)>, String> {
    let used = mem::read::<u32>(pgd + INV_HEADER).ok_or("Inventory unavailable")? as usize;
    if used > MAX_USED || mem::read::<u32>(pgd + INV_HEADER + 4) != Some(INV_CAP as u32) {
        return Err("Invalid inventory size".into());
    }
    let array = mem::read::<usize>(pgd + INV_HEADER + INV_ARRAY)
        .filter(|p| mem::is_ptr(*p))
        .ok_or("Inventory unavailable")?;
    let mut bytes = vec![0; used * INV_ENTRY];
    if !mem::copy_out(array, &mut bytes) {
        return Err("Inventory entries unavailable".into());
    }
    Ok(bytes
        .as_chunks::<INV_ENTRY>()
        .0
        .iter()
        .map(|b| {
            let word = |off| u32::from_le_bytes(b[off..off + 4].try_into().unwrap());
            (word(0), word(4), word(8))
        })
        .collect())
}

/// GAME THREAD ONLY. Replace selected equipment, keeping the game-managed flask and relics.
/// Native remove updates the inventory and clears equipped references; the existing add/equip calls build fresh instances.
pub(crate) fn restore_preset(
    base: usize,
    pgd: usize,
    loadout: &super::arenaloadout::Loadout,
) -> Result<(), String> {
    let mut bytes = [0; layout::LOADOUT_REMOVE_BYTES.len()];
    if !mem::copy_out(base + layout::LOADOUT_REMOVE_RVA, &mut bytes)
        || bytes != layout::LOADOUT_REMOVE_BYTES
    {
        return Err("Item removal signature does not match this game build".into());
    }
    let goods = quick_goods(pgd)?;
    let indices: Vec<_> = (0..HAND_SLOTS)
        .filter_map(|slot| {
            let id = slot_id(pgd, slot)?;
            super::arenaloadout::real_weapon(id)
                .then(|| mem::read::<u32>(pgd + layout::EQUIPPED_INDICES + usize::from(slot) * 4))
                .flatten()
        })
        .collect();
    // The pouch's indices refer to the same inventory as the hand slots. Capture them before removing anything.
    let mut removals: Vec<(u32, u32)> = indices.into_iter().map(|index| (index, 1)).collect();
    let entries = entry_snapshot(pgd)?;
    for slot in 0..POUCH_SLOTS {
        let handle = mem::read::<u32>(pgd + POUCH + slot * 8).ok_or("Pouch unavailable")?;
        let index = mem::read::<u32>(pgd + POUCH + slot * 8 + 4).ok_or("Pouch unavailable")?;
        if index == u32::MAX {
            continue;
        }
        let id = entries.iter().find(|e| e.0 == handle).map(|e| e.1);
        if let Some(qty) = id.and_then(|id| goods.iter().find(|g| g.0 == id).map(|g| g.1)) {
            removals.push((index, qty));
        }
    }
    removals.sort_unstable_by_key(|r| r.0);
    removals.dedup_by_key(|r| r.0);
    if removals
        .iter()
        .any(|&(index, qty)| index == u32::MAX || qty > 999)
    {
        return Err("Invalid inventory index or quantity".into());
    }
    type Remove = unsafe extern "system" fn(usize, u32, i32, u8) -> i32;
    // SAFETY: NR 1.03.3 signature verified above, arguments from its caller 0x1F0863;
    // game-thread-only, live inventory indices, NEGATIVE quantity delta (0x1F5056 adds it to the current count),
    // r9=1 suppresses automatic replacement of a consumed good.
    let remove: Remove = unsafe { std::mem::transmute(base + layout::LOADOUT_REMOVE_RVA) };
    for (index, qty) in removals {
        unsafe {
            remove(pgd + GEAR_BLOCK, index, -(qty as i32), 1);
        }
    }
    for slot in 0..HAND_SLOTS {
        equip_on_game_thread(base, pgd, slot, u32::MAX);
    }
    for w in &loadout.weapons {
        let index = add_on_game_thread(base, pgd, w.id, 1).ok_or("Cannot add weapon")?;
        if equip_on_game_thread(base, pgd, w.slot, index) != Some(w.id) {
            return Err("Cannot equip weapon".into());
        }
        let instance = slot_handle(pgd, w.slot)
            .and_then(|h| gaitem_of(base, h))
            .ok_or("Weapon instance unavailable")?;
        set_kind(instance, w.custom, w.skill)?;
    }
    for &(id, qty) in &loadout.goods {
        let index = add_on_game_thread(base, pgd, id, qty).ok_or("Cannot add item")?;
        pouch_on_game_thread(base, pgd, index).ok_or("Cannot equip quick item")?;
    }
    Ok(())
}

#[cfg(test)]
mod preset_tests {
    use super::*;

    #[test]
    fn captures_pouch_order_once_per_item_and_excludes_flask_and_unselected_goods() {
        let entries = [
            [1u32, 0x4000_03e8, 1, 0, 0],
            [2, 0x4000_2b06, 2, 0, 0],
            [3, 0x4000_00c8, 4, 0, 0],
            [4, 0x4000_00c9, 9, 0, 0],
        ];
        let mut pgd = vec![0u8; POUCH + POUCH_SLOTS * 8];
        pgd[INV_HEADER..INV_HEADER + 4].copy_from_slice(&4u32.to_le_bytes());
        pgd[INV_HEADER + 4..INV_HEADER + 8].copy_from_slice(&(INV_CAP as u32).to_le_bytes());
        pgd[INV_HEADER + INV_ARRAY..INV_HEADER + INV_ARRAY + 8]
            .copy_from_slice(&(entries.as_ptr() as usize).to_le_bytes());
        for (slot, handle) in [1u32, 3, 2, 3].into_iter().enumerate() {
            pgd[POUCH + slot * 8..POUCH + slot * 8 + 4].copy_from_slice(&handle.to_le_bytes());
        }
        assert_eq!(
            quick_goods(pgd.as_ptr() as usize).unwrap(),
            vec![(0x4000_00c8, 4), (0x4000_2b06, 2)]
        );
        pgd[POUCH..POUCH + 4].copy_from_slice(&55u32.to_le_bytes());
        assert!(quick_goods(pgd.as_ptr() as usize).is_err());
        pgd[INV_HEADER..INV_HEADER + 4].copy_from_slice(&(MAX_USED as u32 + 1).to_le_bytes());
        assert!(quick_goods(pgd.as_ptr() as usize).is_err());
    }
}
