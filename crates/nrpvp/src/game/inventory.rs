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
