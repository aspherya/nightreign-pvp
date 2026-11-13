//! ARENA-3 (dev only, with the Arena): read param rows from the game's own loaded params, by plain memory reads (no game call).
//! Game 1.03.3 / exe 1.3.3.0, MEMORY_MAP §29 "Param rows in memory" (static reading of `exe+0xE7DAA0` and `exe+0xE9E680`).
//!
//! Repository `[exe+0x3C28880]`; param `idx` at `[repo + idx*0x48 + 0x88]` (while `[repo + idx*0x48 + 0x80]` > 0); its file at
//! `[[that + 0x80] + 0x80]`; after the file (`align16([file - 0x10])` bytes on) a sorted `{i32 id, i32 row}` table of
//! `[file - 0xC]` entries; a row's data at `file + [file + 0x48 + row*0x18]`.

use super::mem;

const RVA_REPOSITORY: usize = 0x3C2_8880;
const PARAM_STRIDE: usize = 0x48;
const PARAM_COUNT: usize = 0x80;
const PARAM_RES: usize = 0x88;
/// Param indices (name table `exe+0x39B4AD0`).
pub(crate) const EQUIP_PARAM_WEAPON: usize = 0x0;
/// MEMORY_MAP §29: the custom-weapon rows used by Sparring Equipment.
pub(crate) const EQUIP_PARAM_CUSTOM_WEAPON: usize = 0x9E;
pub(crate) const REINFORCE_PARAM_WEAPON: usize = 0x4;
/// `EquipParamWeapon.reinforceTypeId` (s16), Smithbox NR paramdef.
const WEAPON_REINFORCE_TYPE: usize = 0xDA;
/// A weapon's upgrade level is the last two digits of its id (Elden Ring's scheme, at most +25); the affinity uses the hundreds.
const LEVEL_SPAN: u32 = 100;
const MAX_LEVEL: u32 = 25;

/// The file of param `idx`, if loaded.
fn file(base: usize, idx: usize) -> Option<usize> {
    let repo = mem::read::<usize>(base + RVA_REPOSITORY).filter(|p| mem::is_ptr(*p))?;
    let slot = repo + idx * PARAM_STRIDE;
    if mem::read::<u32>(slot + PARAM_COUNT)? == 0 {
        return None;
    }
    let res = mem::read::<usize>(slot + PARAM_RES).filter(|p| mem::is_ptr(*p))?;
    let holder = mem::read::<usize>(res + 0x80).filter(|p| mem::is_ptr(*p))?;
    mem::read::<usize>(holder + 0x80).filter(|p| mem::is_ptr(*p))
}

/// The row index of `id` in a sorted `(id, row)` table (pure; the game's binary search).
pub(crate) fn find(table: &[(i32, i32)], id: i32) -> Option<usize> {
    table
        .binary_search_by_key(&id, |e| e.0)
        .ok()
        .and_then(|i| usize::try_from(table[i].1).ok())
}

/// The data of row `id` of param `idx`, if it exists.
pub(crate) fn row(base: usize, idx: usize, id: i32) -> Option<usize> {
    let f = file(base, idx)?;
    let size = usize::try_from(mem::read::<i32>(f - 0x10)?).ok()?;
    let n = usize::try_from(mem::read::<i32>(f - 0xC)?).ok()?;
    if n == 0 || n > 0x10_0000 {
        return None;
    }
    let table_at = f + ((size + 0xF) & !0xF);
    let (b, _) = mem::copy_guarded(table_at, n * 8);
    let table: Vec<(i32, i32)> = b
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| {
            (
                i32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                i32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            )
        })
        .collect();
    let r = find(&table, id)?;
    let off = mem::read::<u64>(f + 0x48 + r * 0x18)?;
    Some(f + usize::try_from(off).ok()?)
}

/// The weapon id at the highest upgrade level its `ReinforceParamWeapon` set has (pure core): `reinforce_type` of the weapon at +0,
/// `has_level(type + level)` says whether that reinforce row exists.
pub(crate) fn max_upgrade_with(
    weapon_id: u32,
    reinforce_type: Option<i16>,
    has_level: impl Fn(i32) -> bool,
) -> u32 {
    let plain = weapon_id - weapon_id % LEVEL_SPAN;
    let Some(t) = reinforce_type.filter(|t| *t >= 0) else {
        return weapon_id;
    };
    (0..=MAX_LEVEL)
        .rev()
        .find(|l| has_level(i32::from(t) + *l as i32))
        .map_or(weapon_id, |l| plain + l)
}

/// The weapon id at its maximum upgrade, read from the game's params (the id unchanged when anything is unreadable).
pub(crate) fn max_upgrade(base: usize, weapon_id: u32) -> u32 {
    let plain = weapon_id - weapon_id % LEVEL_SPAN;
    let rt = i32::try_from(plain)
        .ok()
        .and_then(|id| row(base, EQUIP_PARAM_WEAPON, id))
        .and_then(|r| mem::read::<i16>(r + WEAPON_REINFORCE_TYPE));
    max_upgrade_with(weapon_id, rt, |id| {
        row(base, REINFORCE_PARAM_WEAPON, id).is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_search_returns_the_row_index() {
        let t = [(10, 0), (17, 3), (100, 1)];
        assert_eq!(find(&t, 17), Some(3));
        assert_eq!(find(&t, 11), None);
    }

    #[test]
    fn max_upgrade_takes_the_highest_existing_level_of_the_set() {
        // Uchigatana-like: type 0 has +0..+3 (MEMORY_MAP §29); Blood affinity 1100 stays.
        let common = |id: i32| (0..=3).contains(&id);
        assert_eq!(max_upgrade_with(9_001_100, Some(0), common), 9_001_103);
        assert_eq!(max_upgrade_with(9_001_102, Some(0), common), 9_001_103);
        // Legendary: type 400 has +0 only.
        assert_eq!(
            max_upgrade_with(3_150_000, Some(400), |id| id == 400),
            3_150_000
        );
        // Unknown type or no rows: unchanged.
        assert_eq!(max_upgrade_with(1_750_001, None, common), 1_750_001);
        assert_eq!(max_upgrade_with(1_750_001, Some(100), |_| false), 1_750_001);
    }
}
