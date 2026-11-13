//! ARENA-3 (dev only, with the Arena): the player enters the Arena with the loadout built at the Sparring Grounds, every weapon at
//! its maximum upgrade, at level 15 with full HP / FP. Rules: PROJECT_SCOPE §Arena; findings: MEMORY_MAP §29. Game 1.03.3.
//!
//! The expedition start wipes the Sparring items and rebuilds the starting kit, so:
//! 1. **Capture** ([`capture_tick`], main loop): while LOCAL is at the Roundtable and the queue is idle or searching, remember the six
//!    hand slots (id, and the instance's custom row + skill: Seppuku lives there) and the consumables (every good but the flask),
//!    every [`CAPTURE_EVERY_MS`]. The last capture before the match is the loadout.
//! 2. **Apply** ([`apply`], once LOCAL stands on its arena spawn): on the game's thread, SetLevel(15), then for each captured weapon
//!    add its max-upgrade id, equip it into the same slot and write the custom row + skill into the new instance; then give back
//!    every captured good the expedition took away. The recovery flag fills HP / FP afterwards.
//!
//! No loadout chosen = the slots hold the Nightfarer's starting weapon, which is given back at its maximum upgrade (the "default
//! weapon at legendary" rule). Relics are the game's (untouched). Not covered yet: passives, the flask count, the quick slots.
//!
//! ARENA-LV (Arena test 2026-11-11: the host played with level-1 max HP; a player lost a level between rounds): every round reset
//! sets level 15 again ([`reapply`]) and, while an Arena match is on, [`hold_tick`] sets it again when the level or the max HP
//! slips below what the last SetLevel(15) gave. Every SetLevel logs the level and the max HP it left.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use super::inventory::{self, HAND_SLOTS, UNARMED};
use super::mem;

/// The Arena's level (PROJECT_SCOPE §Arena).
pub(crate) const ARENA_LEVEL: u32 = 15;
/// The game's SetLevel `exe+0x209E20(PGD*, level)`: writes `+0x68` and recalculates the attributes (MEMORY_MAP §29).
const RVA_SET_LEVEL: usize = 0x20_9E20;
/// The game's stat recalculation `exe+0x20DCD0(PGD*)`: base max HP / FP / stamina from the attributes, times the SpEffect
/// multipliers (`exe+0x6C90E0`, `exe+0x50C1B0`), written to `+0x18` / `+0x14` and siblings (game 1.03.3, MEMORY_MAP §29).
const RVA_RECALC_STATS: usize = 0x20_DCD0;
/// `PlayerGameData+0x10C`: the level reached (the skills' level requirement reads it).
const LEVEL_REACHED: usize = 0x10C;
/// Our per-game flag for the Bonfire-like Recovery event (as `arenarounds`).
const FLAG_HEAL: u32 = 10_007_410;
/// How often the Roundtable state is captured.
const CAPTURE_EVERY_MS: u128 = 2_000;
/// After a Commence the game runs, the capture holds still this long: the game wipes the loadout at the Roundtable right after the
/// press, before the load screen (seen live: a capture there took the starting kit). Longer than a Commence-to-landing.
const FREEZE_AFTER_COMMENCE_MS: u128 = 90_000;
/// `PlayerGameData+0x68`: the character level (MEMORY_MAP §5, §29).
const PGD_LEVEL: usize = 0x68;
/// ARENA-LV: the hold sets the level again at most this often.
const HOLD_EVERY_MS: u128 = 3_000;
/// ARENA-LV: the max HP (`PlayerGameData+0x14`) the last SetLevel(15) left; 0 = no SetLevel(15) in this match yet.
static HELD_MAX_HP: AtomicU32 = AtomicU32::new(0);
/// ARENA-LV: when the hold last asked for a SetLevel (main loop only).
static LAST_HOLD: Mutex<Option<Instant>> = Mutex::new(None);
/// The starting flask (Flask of Crimson Tears rows): the expedition hands it out itself.
const FLASK_ROWS: std::ops::RangeInclusive<u32> = 1000..=1025;

/// One hand-slot weapon as captured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Weapon {
    pub slot: u8,
    pub id: u32,
    /// The instance's `EquipParamCustomWeapon` row (-1 = none).
    pub custom: u32,
    /// The instance's skill (`SwordArtsParam`; -1 = the weapon's default).
    pub skill: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Loadout {
    pub weapons: Vec<Weapon>,
    /// `(good id, quantity)`.
    pub goods: Vec<(u32, u32)>,
}

/// A hand slot worth giving back (pure).
pub(crate) fn real_weapon(id: u32) -> bool {
    id != 0 && id != u32::MAX && id != UNARMED
}

/// The consumables of an inventory (pure): goods (category 4) but the flask, summed per id.
pub(crate) fn goods_of(entries: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &(id, qty) in entries {
        if id >> 28 != 4 || qty == 0 || FLASK_ROWS.contains(&(id & 0x0FFF_FFFF)) {
            continue;
        }
        match out.iter_mut().find(|g| g.0 == id) {
            Some(g) => g.1 += qty,
            None => out.push((id, qty)),
        }
    }
    out
}

/// What to give back (pure): every captured good the player holds fewer of now, by the difference.
pub(crate) fn goods_to_give(captured: &[(u32, u32)], now: &[(u32, u32)]) -> Vec<(u32, u32)> {
    captured
        .iter()
        .filter_map(|&(id, q)| {
            let have = now.iter().find(|g| g.0 == id).map_or(0, |g| g.1);
            (q > have).then_some((id, q - have))
        })
        .collect()
}

struct State {
    loadout: Option<Loadout>,
    last_capture: Option<Instant>,
    /// The game's Commence count last seen, and since when the capture is frozen.
    commences: u32,
    frozen_since: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    loadout: None,
    last_capture: None,
    commences: 0,
    frozen_since: None,
});

/// Whether the capture holds still now (pure): a new game Commence froze it less than [`FREEZE_AFTER_COMMENCE_MS`] ago.
pub(crate) fn frozen(frozen_for_ms: Option<u128>) -> bool {
    frozen_for_ms.is_some_and(|ms| ms < FREEZE_AFTER_COMMENCE_MS)
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Read the loadout now (read only).
pub(crate) fn capture(base: usize) -> Result<Loadout, String> {
    let pgd = inventory::local_pgd()?;
    let mut weapons = Vec::new();
    for slot in 0..HAND_SLOTS {
        let Some(id) = inventory::slot_id(pgd, slot).filter(|id| real_weapon(*id)) else {
            continue;
        };
        let inst = inventory::slot_handle(pgd, slot).and_then(|h| inventory::gaitem_of(base, h));
        let field = |off| {
            inst.and_then(|p| mem::read::<u32>(p + off))
                .unwrap_or(u32::MAX)
        };
        weapons.push(Weapon {
            slot,
            id,
            custom: field(inventory::GAITEM_CUSTOM),
            skill: field(inventory::GAITEM_SKILL),
        });
    }
    Ok(Loadout {
        weapons,
        goods: goods_of(&inventory::entries(pgd)),
    })
}

/// Main loop: while LOCAL is at the Roundtable and the queue is idle or searching, refresh the captured loadout.
pub(crate) fn capture_tick(base: usize, at_roundtable: bool) {
    if !at_roundtable {
        return;
    }
    let mut st = lock();
    let commences = super::menuflow::game_commences();
    if commences != st.commences {
        st.commences = commences;
        st.frozen_since = Some(Instant::now());
        tracing::info!("arena_loadout: Commence ran: the captured loadout holds still");
    }
    if frozen(st.frozen_since.map(|t| t.elapsed().as_millis())) {
        return;
    }
    if st
        .last_capture
        .is_some_and(|t| t.elapsed().as_millis() < CAPTURE_EVERY_MS)
    {
        return;
    }
    st.last_capture = Some(Instant::now());
    match capture(base) {
        Ok(l) => {
            if st.loadout.as_ref() != Some(&l) {
                tracing::info!(
                    "arena_loadout: captured {} weapon(s) {:?}, {} good(s)",
                    l.weapons.len(),
                    l.weapons
                        .iter()
                        .map(|w| format!(
                            "s{} {:08X} c{:X} k{}",
                            w.slot, w.id, w.custom, w.skill as i32
                        ))
                        .collect::<Vec<_>>(),
                    l.goods.len()
                );
            }
            st.loadout = Some(l);
        }
        Err(e) => tracing::debug!("arena_loadout: capture: {e}"),
    }
}

/// The last captured loadout (console).
pub(crate) fn captured() -> Option<Loadout> {
    lock().loadout.clone()
}

/// Once LOCAL stands on its arena spawn: level 15, the captured loadout at max upgrade, the goods back, then full HP / FP.
pub(crate) fn apply(base: usize) {
    let Some(loadout) = lock().loadout.clone() else {
        tracing::warn!("arena_loadout: nothing captured at the Roundtable; only the level is set");
        apply_level(base);
        return;
    };
    let Ok(pgd) = inventory::local_pgd() else {
        tracing::warn!("arena_loadout: no LOCAL PlayerGameData; nothing applied");
        return;
    };
    let plan: Vec<(Weapon, u32)> = loadout
        .weapons
        .iter()
        .map(|w| (*w, super::params::max_upgrade(base, w.id)))
        .collect();
    let goods = goods_to_give(&loadout.goods, &goods_of(&inventory::entries(pgd)));
    tracing::info!(
        "arena_loadout: applying {} weapon(s) {:?}, {} good(s)",
        plan.len(),
        plan.iter()
            .map(|(w, id)| format!("s{} {:08X}->{id:08X}", w.slot, w.id))
            .collect::<Vec<_>>(),
        goods.len()
    );
    inventory::queue(
        base,
        "arena_loadout",
        Box::new(move |base| {
            set_level(base, pgd);
            for (w, id) in &plan {
                let Some(index) = inventory::add_on_game_thread(base, pgd, *id, 1) else {
                    tracing::warn!("arena_loadout: {id:08X} not added");
                    continue;
                };
                if inventory::equip_on_game_thread(base, pgd, w.slot, index) != Some(*id) {
                    tracing::warn!("arena_loadout: slot {} does not hold {id:08X}", w.slot);
                    continue;
                }
                if w.custom == u32::MAX && w.skill == u32::MAX {
                    continue;
                }
                let inst =
                    inventory::slot_handle(pgd, w.slot).and_then(|h| inventory::gaitem_of(base, h));
                match inst.map(|p| inventory::set_kind(p, w.custom, w.skill)) {
                    Some(Ok(())) => tracing::info!(
                        "arena_loadout: slot {} kind {:08X}, skill {}",
                        w.slot,
                        w.custom,
                        w.skill as i32
                    ),
                    Some(Err(e)) => {
                        tracing::warn!("arena_loadout: slot {} kind not written: {e}", w.slot)
                    }
                    None => tracing::warn!("arena_loadout: slot {} has no instance", w.slot),
                }
            }
            for (id, qty) in &goods {
                if let Some(index) = inventory::add_on_game_thread(base, pgd, *id, *qty) {
                    // A good only shows when it sits in the pouch (as a pickup puts it there).
                    inventory::pouch_on_game_thread(base, pgd, index);
                }
            }
            // The rival's game draws what this broadcast carries; without it the rival saw our starting kit (2026-11-12).
            inventory::send_player_data_on_game_thread(base);
            heal(base);
        }),
    );
}

fn apply_level(base: usize) {
    reapply(base, true);
}

/// ARENA-LV: SetLevel(15) on the game's thread, then (`heal`) the recovery event that fills HP / FP up to the new maximum.
pub(crate) fn reapply(base: usize, heal_after: bool) {
    let Ok(pgd) = inventory::local_pgd() else {
        tracing::warn!("arena_level: no LOCAL PlayerGameData; level not set");
        return;
    };
    inventory::queue(
        base,
        "arena_level",
        Box::new(move |base| {
            set_level(base, pgd);
            // Each reset also sends the equipment again: a rival whose game missed the first broadcast (still loading the arena)
            // gets it here, at the latest by round 2.
            inventory::send_player_data_on_game_thread(base);
            if heal_after {
                heal(base);
            }
        }),
    );
}

/// Send LOCAL's player data (equipment, level) to the session again, on the game's thread: when both players are first seen up in
/// the arena, so a rival still loading at the spawn's broadcast gets it before the fight.
pub(crate) fn resend(base: usize) {
    inventory::queue(
        base,
        "arena_send",
        Box::new(inventory::send_player_data_on_game_thread),
    );
}

/// ARENA-LV (pure): whether LOCAL's level has slipped: not 15, or a max HP below what the last SetLevel(15) left (`held_max`). Before
/// the first SetLevel(15) of a match (`held_max` 0) nothing has slipped.
pub(crate) fn level_slipped(level: u32, max_hp: u32, held_max: u32) -> bool {
    held_max > 0 && (level != ARENA_LEVEL || max_hp < held_max)
}

/// ARENA-LV, main loop every tick: while an Arena match is on (`on`), set level 15 again when it slipped (one line each time, at most
/// every [`HOLD_EVERY_MS`], no heal: a fight may be on). Off: forget the match's max HP.
pub(crate) fn hold_tick(base: usize, on: bool) {
    if !on {
        HELD_MAX_HP.store(0, Ordering::Relaxed);
        *lock_hold() = None;
        return;
    }
    let held = HELD_MAX_HP.load(Ordering::Relaxed);
    if held == 0 {
        return;
    }
    let Ok(pgd) = inventory::local_pgd() else {
        return;
    };
    let level = mem::read::<u32>(pgd + PGD_LEVEL).unwrap_or(0);
    let max_hp = mem::read::<u32>(pgd + super::layout::GAMEDATA_MAX_HP).unwrap_or(0);
    if level == 0 || !level_slipped(level, max_hp, held) {
        return;
    }
    let mut last = lock_hold();
    if last.is_some_and(|t| t.elapsed().as_millis() < HOLD_EVERY_MS) {
        return;
    }
    *last = Some(Instant::now());
    drop(last);
    tracing::info!(
        "arena_level: slipped to level {level}, max HP {max_hp} (SetLevel(15) left {held}); setting level {ARENA_LEVEL} again"
    );
    reapply(base, false);
}

fn lock_hold() -> std::sync::MutexGuard<'static, Option<Instant>> {
    LAST_HOLD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// GAME THREAD. The game's SetLevel.
fn set_level(base: usize, pgd: usize) {
    type SetLevel = unsafe extern "system" fn(usize, u32) -> u64;
    // SAFETY: base + a fixed RVA inside the exe image; signature from its prologue and caller (MEMORY_MAP §29); verified live with a
    // call from the mod's thread (rawcall), here on the game's thread.
    let f: SetLevel = unsafe { std::mem::transmute::<usize, SetLevel>(base + RVA_SET_LEVEL) };
    // SAFETY: see above.
    unsafe { f(pgd, ARENA_LEVEL) };
    // The skills' level requirement reads `+0x10C` (the level the expedition reached; the Sparring Grounds set it too, SetLevel does
    // not): with it at 1 the Marais and Carian Regal Scepter skills stayed locked (Leonardo, 2026-11-09, MEMORY_MAP §29).
    if let Err(e) = mem::write_bytes(pgd + LEVEL_REACHED, &ARENA_LEVEL.to_le_bytes()) {
        tracing::warn!("arena_loadout: +0x10C not written: {e}");
    }
    // ARENA-LV bug #1: SetLevel writes the attributes but never the base max HP / FP / stamina (`+0x18`, `+0x24`, ...); the game's
    // own stat recalculation does, and the character's data module follows it (live 2026-11-12: Wylder 240 -> 1120, MEMORY_MAP §29).
    type RecalcStats = unsafe extern "system" fn(usize) -> u64;
    // SAFETY: base + a fixed RVA inside the exe image; one argument (`mov rdi, rcx` = PlayerGameData, read as `[rdi+8]`, `[rdi+0x14]`)
    // from its prologue; ten callers in the exe pass a PlayerGameData; verified live with `rawcall 20DCD0 pgd`. Game thread, as above.
    let recalc: RecalcStats =
        unsafe { std::mem::transmute::<usize, RecalcStats>(base + RVA_RECALC_STATS) };
    // SAFETY: see above.
    unsafe { recalc(pgd) };
    let level = mem::read::<u32>(pgd + PGD_LEVEL).unwrap_or(0);
    let max_hp = mem::read::<u32>(pgd + super::layout::GAMEDATA_MAX_HP).unwrap_or(0);
    if level == ARENA_LEVEL && max_hp > 0 {
        HELD_MAX_HP.store(max_hp, Ordering::Relaxed);
    }
    // ARENA-LV diagnosis (bug #1: max HP stays at the level-1 value): the attribute bytes SetLevel writes (`+0x34..+0x50`, MEMORY_MAP
    // §29) and the character's own max HP (data module), next to the PlayerGameData copy.
    let attrs: Vec<u8> = (0..8)
        .map(|i| mem::read::<u8>(pgd + 0x34 + 4 * i).unwrap_or(0))
        .collect();
    let chr_max = super::teleport::local_chr()
        .ok()
        .and_then(|c| mem::read::<usize>(c + super::layout::PLAYER_MODULES))
        .and_then(mem::read::<usize>)
        .and_then(|d| mem::read::<u32>(d + super::layout::DATA_MODULE_MAX_HP));
    tracing::info!(
        "arena_loadout: level -> {level}, reached {}, max HP {max_hp} (character {chr_max:?}), attributes {attrs:?}",
        mem::read::<u32>(pgd + LEVEL_REACHED).unwrap_or(0)
    );
}

fn heal(base: usize) {
    if let Err(e) = super::flags::set(base, FLAG_HEAL, true) {
        tracing::warn!("arena_loadout: recovery flag {FLAG_HEAL} not set: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commence_freezes_the_capture_for_a_while() {
        assert!(!frozen(None));
        assert!(frozen(Some(0)));
        assert!(frozen(Some(FREEZE_AFTER_COMMENCE_MS - 1)));
        assert!(!frozen(Some(FREEZE_AFTER_COMMENCE_MS)));
    }

    #[test]
    fn the_level_slips_when_it_is_not_15_or_max_hp_drops() {
        assert!(!level_slipped(1, 220, 0), "no SetLevel(15) yet");
        assert!(!level_slipped(15, 860, 860));
        assert!(
            !level_slipped(15, 900, 860),
            "more max HP (a passive) is fine"
        );
        assert!(level_slipped(14, 820, 860), "a level lost between rounds");
        assert!(
            level_slipped(15, 220, 860),
            "level 15 with level-1 stats (the host, 2026-11-11)"
        );
    }

    #[test]
    fn empty_hands_are_not_weapons() {
        assert!(!real_weapon(UNARMED));
        assert!(!real_weapon(0));
        assert!(!real_weapon(u32::MAX));
        assert!(real_weapon(0x0089_588C));
    }

    #[test]
    fn goods_skip_the_flask_and_other_categories_and_sum_per_id() {
        let inv = [
            (0x4000_03E9, 3), // flask row 1001
            (0x4000_0258, 2),
            (0x4000_0258, 1),
            (0x4000_2B06, 1), // a row above 10000 is a Sparring consumable too
            (0x0089_588C, 1), // weapon
            (0x800F_6249, 1), // relic
            (0x4000_04BA, 0),
        ];
        assert_eq!(goods_of(&inv), vec![(0x4000_0258, 3), (0x4000_2B06, 1)]);
    }

    #[test]
    fn only_the_missing_goods_are_given() {
        let captured = [(0x4000_0258, 2), (0x4000_04BA, 2), (0x4000_2B12, 1)];
        let now = [(0x4000_04BA, 2), (0x4000_0258, 1)];
        assert_eq!(
            goods_to_give(&captured, &now),
            vec![(0x4000_0258, 1), (0x4000_2B12, 1)]
        );
    }
}
