//! Reading the game's text tables (FMG) through `MsgRepository`. R1 moved this out of the dev-only UI-1 C1 prototype
//! (`c1proto`) because `menuflow` (player build) uses it. Read-only. Game 1.03.3, exe 1.3.3.0 (UI-1 card, trace of 2026-10-19).

/// `MsgRepository` slot (game 1.03.3; TC-004).
const RVA_MSGREPO: usize = 0x3C2_46A8;
const CAT_DIALOGUES: u32 = 204;

fn r64(a: usize) -> Option<usize> {
    super::mem::read::<u64>(a).map(|v| v as usize)
}
fn r32(a: usize) -> Option<u32> {
    super::mem::read::<u32>(a)
}

/// Index into the FMG's string-offset table for `id`, from the groups `(offset_index, first_id, last_id)` (SoulsFormats FMG; the same
/// search as `exe+0x27F5710`).
pub(crate) fn fmg_index(groups: &[(u32, u32, u32)], id: u32) -> Option<u32> {
    groups
        .iter()
        .find(|g| id >= g.1 && id <= g.2)
        .map(|g| g.0 + (id - g.1))
}

/// The CL_Dialogues FMG (slot 0) from the repository table (`[[repo+8] + slot*8] + cat*8`, bounds at repo+0x10/+0x14).
pub(crate) fn dialogues_fmg(base: usize) -> Option<usize> {
    fmg_for(base, CAT_DIALOGUES)
}

/// The FMG of `category` (slot 0) from the repository table.
pub(crate) fn fmg_for(base: usize, category: u32) -> Option<usize> {
    let repo = r64(base + RVA_MSGREPO).filter(|p| *p != 0)?;
    let table = r64(repo + 8)?;
    let cats = r32(repo + 0x14)?;
    if category >= cats {
        return None;
    }
    let row = r64(table).filter(|p| *p != 0)?;
    r64(row + category as usize * 8).filter(|p| *p != 0)
}

/// Address of the string-offset slot of `id` in `fmg`, and the FMG's group count (for the log).
pub(crate) fn offset_slot(fmg: usize, id: u32) -> Option<(usize, u32)> {
    let count = r32(fmg + 0xC)?;
    if count == 0 || count > 100_000 {
        return None;
    }
    let mut groups = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let g = fmg + 0x28 + i * 16;
        groups.push((r32(g)?, r32(g + 4)?, r32(g + 8)?));
    }
    let idx = fmg_index(&groups, id)?;
    let offsets = r64(fmg + 0x18)?;
    Some((offsets + idx as usize * 8, count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmg_index_follows_the_groups() {
        let g = [(0, 10, 12), (3, 100, 101), (5, 506_040, 506_040)];
        assert_eq!(fmg_index(&g, 11), Some(1));
        assert_eq!(fmg_index(&g, 101), Some(4));
        assert_eq!(fmg_index(&g, 506_040), Some(5));
        assert_eq!(fmg_index(&g, 50), None);
    }
}
