//! RW-9 (both builds), Solo part: in a Solo match every other player is a rival, so nobody may show on the world map or as a mark on the
//! top compass bar. Game 1.03.3 / exe 1.3.3.0; the reading is in MEMORY_MAP §20.
//!
//! The map and the compass draw the other players from one list: the map view model `CS::WorldMapViewModel`
//! (`vm = [[CSMenuMan+0xA0]+0x240]`) holds a `WorldMapPinDataList<WorldMapPlayerData*>` at `vm+0xA0` whose pointer vector is
//! `[vm+0xB0, vm+0xB8)` (count = (end - begin) / 8). The game rewrites the items' display bytes every frame, so a "hidden" byte would be
//! overwritten; the vector's END pointer is not touched by the per-frame update, so [`tick`] sets end = begin (an empty list) and puts the
//! old end back when the hiding stops or the view model changes. A plain 8-byte write on the main loop: no hardware slot, no code patch.
//! Log lines start with `maphide:`.
//!
//! Solo only: in Duo / Trio a teammate must stay visible, which needs a per-player decision (a breakpoint at `exe+0x90EE00`, later).

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use super::mem;

/// `CSMenuMan` singleton slot, its menu control pointer, the view model pointer inside that, and the view model's vtable
/// (`CS::WorldMapViewModel`). `[VERIFIED: static, MEMORY_MAP §20]`
const RVA_MENU_MAN: usize = 0x3C1_49D8;
const MAN_CTRL: usize = 0xA0;
const CTRL_MAP_VM: usize = 0x240;
const RVA_VM_VTABLE: usize = 0x2C4_A548;
/// The pointer vector of the other-players list inside the view model.
const VM_LIST_BEGIN: usize = 0xB0;
const VM_LIST_END: usize = 0xB8;
/// `GameDataMan` slot (its `+0x10` points at the remote players' data, 0x8A0 per player, `+8` = id). `[HYPOTHESIS: ER layout; read live]`
const RVA_GAME_DATA_MAN: usize = 0x3C0_78D0;

/// Console override: 0 = automatic (Solo match), 1 = forced on, 2 = forced off.
static OVERRIDE: AtomicU8 = AtomicU8::new(0);
/// The view model whose list is emptied and the end pointer it had; 0 = nothing hidden.
static HIDDEN_VM: AtomicUsize = AtomicUsize::new(0);
static SAVED_END: AtomicUsize = AtomicUsize::new(0);
static BASE: AtomicUsize = AtomicUsize::new(0);

/// Console `maphide [0|1|auto]`.
pub(crate) fn set_override(v: Option<bool>) {
    let code = match v {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    };
    OVERRIDE.store(code, Ordering::Relaxed);
}

/// Should the list be empty right now? `solo_match` = the mod's PvP mode runs in an expedition with team size 1.
fn wanted(solo_match: bool) -> bool {
    match OVERRIDE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => solo_match,
    }
}

/// What to do with the list this tick (pure, tested).
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Nothing,
    /// Write end = begin; remember the old end.
    Empty(usize),
    /// Write the remembered end back.
    Restore(usize),
}

fn decide(want: bool, begin: usize, end: usize, saved: Option<usize>) -> Step {
    match (want, saved) {
        (true, None) if end != begin => Step::Empty(end),
        (false, Some(old)) if end == begin => Step::Restore(old),
        _ => Step::Nothing,
    }
}

/// The live view model, if the menu manager has one with the expected vtable.
pub(crate) fn view_model(base: usize) -> Option<usize> {
    let man = mem::read::<usize>(base + RVA_MENU_MAN).filter(|m| *m != 0)?;
    let ctrl = mem::read::<usize>(man + MAN_CTRL).filter(|c| *c != 0)?;
    let vm = mem::read::<usize>(ctrl + CTRL_MAP_VM).filter(|v| *v != 0)?;
    (mem::read::<usize>(vm)? == base + RVA_VM_VTABLE).then_some(vm)
}

/// Console `maphide` with no argument: the state in words.
pub(crate) fn describe() -> String {
    let base = BASE.load(Ordering::Relaxed);
    let mode = ["automatic (Solo match)", "forced on", "forced off"]
        [usize::from(OVERRIDE.load(Ordering::Relaxed)).min(2)];
    let Some(vm) = view_model(base) else {
        return format!("maphide: {mode}; no map view model (not in a world?)");
    };
    let begin = mem::read::<usize>(vm + VM_LIST_BEGIN).unwrap_or(0);
    let end = mem::read::<usize>(vm + VM_LIST_END).unwrap_or(0);
    let hidden = HIDDEN_VM.load(Ordering::Relaxed) == vm;
    // Research for Duo / Trio: what each list item and each remote slot holds (item +0xC shown, map x / y; slot +8 = the player id
    // the producer matches against `[chr+0x5A0]+8`, shown by `puppet_list` as "map id"). Items are read while hidden too.
    let items = HIDDEN_VM.load(Ordering::Relaxed) == vm;
    let n = (if items {
        SAVED_END.load(Ordering::Relaxed)
    } else {
        end
    })
    .wrapping_sub(begin)
        / 8;
    let mut detail = String::new();
    for i in 0..n.min(4) {
        let it = mem::read::<usize>(begin + 8 * i).unwrap_or(0);
        let shown = mem::read::<u8>(it + 0xC).unwrap_or(0xFF);
        let xy = (mem::read::<f32>(it + 0x10), mem::read::<f32>(it + 0x14));
        let slot = mem::read::<usize>(base + RVA_GAME_DATA_MAN)
            .filter(|g| *g != 0)
            .and_then(|g| mem::read::<usize>(g + 0x10))
            .and_then(|l| mem::read::<u64>(l + i * 0x8A0 + 8))
            .map_or("?".into(), |v| format!("{v:#X}"));
        detail += &format!("; item {i}: shown {shown}, xy {xy:?}, remote slot id {slot}");
    }
    format!(
        "maphide: {mode}; vm 0x{vm:X}, other-players list {} item(s), {}{detail}",
        end.wrapping_sub(begin) / 8,
        if hidden {
            "hidden by us"
        } else {
            "left to the game"
        }
    )
}

/// Main loop, every tick (both builds). `solo_match`: the mod's PvP mode is on in an expedition with team size 1.
pub(crate) fn tick(base: usize, solo_match: bool) {
    BASE.store(base, Ordering::Relaxed);
    let want = wanted(solo_match);
    let hidden_vm = HIDDEN_VM.load(Ordering::Relaxed);
    let vm = view_model(base);
    if hidden_vm != 0 && vm != Some(hidden_vm) {
        // The view model went away (leaving the world): nothing to put back, the new one starts untouched.
        tracing::info!("maphide: the map view model changed, forgetting 0x{hidden_vm:X}");
        HIDDEN_VM.store(0, Ordering::Relaxed);
    }
    let Some(vm) = vm else {
        return;
    };
    let (Some(begin), Some(end)) = (
        mem::read::<usize>(vm + VM_LIST_BEGIN),
        mem::read::<usize>(vm + VM_LIST_END),
    ) else {
        return;
    };
    let saved =
        (HIDDEN_VM.load(Ordering::Relaxed) == vm).then(|| SAVED_END.load(Ordering::Relaxed));
    match decide(want, begin, end, saved) {
        Step::Nothing => {}
        Step::Empty(old) => match mem::write_bytes(vm + VM_LIST_END, &begin.to_le_bytes()) {
            Ok(()) => {
                SAVED_END.store(old, Ordering::Relaxed);
                HIDDEN_VM.store(vm, Ordering::Relaxed);
                tracing::info!(
                    "maphide: emptied the other-players list of 0x{vm:X} ({} item(s) hidden)",
                    old.wrapping_sub(begin) / 8
                );
            }
            Err(e) => tracing::warn!("maphide: could not empty the list: {e}"),
        },
        Step::Restore(old) => match mem::write_bytes(vm + VM_LIST_END, &old.to_le_bytes()) {
            Ok(()) => {
                HIDDEN_VM.store(0, Ordering::Relaxed);
                tracing::info!("maphide: other-players list of 0x{vm:X} put back");
            }
            Err(e) => tracing::warn!("maphide: could not restore the list: {e}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_offsets() {
        assert_eq!(RVA_MENU_MAN, 0x3C149D8);
        assert_eq!(CTRL_MAP_VM, 0x240);
        assert_eq!(RVA_VM_VTABLE, 0x2C4A548);
        assert_eq!((VM_LIST_BEGIN, VM_LIST_END), (0xB0, 0xB8));
    }

    #[test]
    fn empties_a_filled_list_once_and_restores_it() {
        assert_eq!(decide(true, 0x100, 0x110, None), Step::Empty(0x110));
        // Emptied: nothing more to do while wanted.
        assert_eq!(decide(true, 0x100, 0x100, Some(0x110)), Step::Nothing);
        assert_eq!(
            decide(false, 0x100, 0x100, Some(0x110)),
            Step::Restore(0x110)
        );
        // The game changed the vector meanwhile: leave it alone.
        assert_eq!(decide(false, 0x100, 0x120, Some(0x110)), Step::Nothing);
        assert_eq!(decide(false, 0x100, 0x110, None), Step::Nothing);
        assert_eq!(decide(true, 0x100, 0x100, None), Step::Nothing);
    }

    #[test]
    fn the_override_beats_the_match() {
        set_override(Some(true));
        assert!(wanted(false));
        set_override(Some(false));
        assert!(!wanted(true));
        set_override(None);
        assert!(wanted(true) && !wanted(false));
    }
}
