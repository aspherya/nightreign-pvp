//! R2 (second half, OFF-1): typed views over [`GameMem`]. Each view wraps one object address and turns the offsets of
//! [`super::layout`] into named accessors, so feature code calls `chr.chr_type()`, never `addr + 0x70` (ARCHITECTURE §6).
//! Views hold no state of their own and are created per use.

use super::gamemem::GameMem;
use super::layout::{
    BOTMAN_ARMED, BOTMAN_BOTS_BEGIN, BOTMAN_BOTS_END, BOTMAN_BOT_ENTRY, BOTMAN_TABLE,
    CHRSET_CAPACITY, CHRSET_ENTRIES, CHRSET_ENTRY_SIZE, CHRSET_MAX_CAPACITY, CHR_HANDLE, CHR_ID,
    CHR_TEAM, CHR_TYPE, CHR_TYPE_EXPEDITION, CHR_TYPE_ROUNDTABLE, MENU_SETTINGS,
    MENU_SETTINGS_PLAYERS, MODULES_SPAN, PHYSICS_GROUND, PLAYER_MODULES, WCM_CHRSETS,
    WCM_CHRSET_COUNT,
};

/// Where a player character is, from its character type (MEMORY_MAP §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Roundtable,
    Expedition,
    /// Any other value (not seen for players so far).
    Other(u32),
}

/// Any character (`ChrIns`; players and enemies share this start).
pub(crate) struct ChrIns<'m> {
    mem: &'m dyn GameMem,
    addr: usize,
}

impl<'m> ChrIns<'m> {
    pub(crate) fn new(mem: &'m dyn GameMem, addr: usize) -> Self {
        ChrIns { mem, addr }
    }

    pub(crate) fn chr_type(&self) -> Option<u32> {
        self.mem.read_u32(self.addr + CHR_TYPE)
    }

    pub(crate) fn place(&self) -> Option<Place> {
        self.chr_type().map(|t| match t {
            CHR_TYPE_ROUNDTABLE => Place::Roundtable,
            CHR_TYPE_EXPEDITION => Place::Expedition,
            o => Place::Other(o),
        })
    }

    /// The character's handle (other objects, e.g. the bot manager, refer to it by this value).
    pub(crate) fn handle(&self) -> Option<u64> {
        self.mem.read_u64(self.addr + CHR_HANDLE)
    }

    /// Character / NPC id (0 for real players).
    pub(crate) fn npc_id(&self) -> Option<u32> {
        self.mem.read_u32(self.addr + CHR_ID)
    }

    pub(crate) fn team(&self) -> Option<u8> {
        self.mem.read_u8(self.addr + CHR_TEAM)
    }

    /// This game's copy of the team type (MEMORY_MAP §4). Callers gate what they write and to whom.
    pub(crate) fn set_team(&self, v: u8) -> Result<(), &'static str> {
        self.mem.write_u8(self.addr + CHR_TEAM, v)
    }

    /// The physics module's ground byte (1 standing or riding the hawk, 0 in the air; MEMORY_MAP §8). `physics_vtable` = exe base
    /// + `PHYSICS_VTABLE_RVA`; the module is the first pointer in the module list whose pointee has that vtable.
    pub(crate) fn grounded(&self, physics_vtable: usize) -> Option<bool> {
        let list = self.mem.read_ptr(self.addr + PLAYER_MODULES)?;
        let phys = (0..MODULES_SPAN).step_by(8).find_map(|slot| {
            let p = self.mem.read_ptr(list + slot)?;
            (self.mem.read_u64(p)? as usize == physics_vtable).then_some(p)
        })?;
        self.mem.read_u8(phys + PHYSICS_GROUND).map(|g| g != 0)
    }
}

/// WorldChrMan: where every character lives (OFF-5 uses its character sets to find NPC bots by handle).
pub(crate) struct WorldChrMan<'m> {
    mem: &'m dyn GameMem,
    addr: usize,
}

impl<'m> WorldChrMan<'m> {
    pub(crate) fn new(mem: &'m dyn GameMem, addr: usize) -> Self {
        WorldChrMan { mem, addr }
    }

    /// The ChrIns whose handle is `handle`, and the set it was found in. Sets are tried in this order: `hint`, the set the handle's
    /// bits 20..27 name (the ER encoding, HYPOTHESIS for NR), then every set. An entry counts only when its handle field matches.
    pub(crate) fn find_by_handle(
        &self,
        handle: u64,
        hint: Option<usize>,
    ) -> Option<(usize, usize)> {
        let guess = ((handle >> 20) & 0xFF) as usize;
        let order = hint
            .into_iter()
            .chain(std::iter::once(guess))
            .chain(0..WCM_CHRSET_COUNT)
            .filter(|&i| i < WCM_CHRSET_COUNT);
        let mut tried = [false; WCM_CHRSET_COUNT];
        for i in order {
            if std::mem::replace(&mut tried[i], true) {
                continue;
            }
            if let Some(chr) = self.find_in_set(i, handle) {
                return Some((chr, i));
            }
        }
        None
    }

    fn find_in_set(&self, set: usize, handle: u64) -> Option<usize> {
        let p = self.mem.read_ptr(self.addr + WCM_CHRSETS + set * 8)?;
        let cap = self.mem.read_u32(p + CHRSET_CAPACITY)?;
        if cap == 0 || cap > CHRSET_MAX_CAPACITY {
            return None;
        }
        let entries = self.mem.read_ptr(p + CHRSET_ENTRIES)?;
        let mut buf = vec![0u8; cap as usize * CHRSET_ENTRY_SIZE];
        if !self.mem.read_into(entries, &mut buf) {
            return None;
        }
        (0..cap as usize).find_map(|k| {
            let o = k * CHRSET_ENTRY_SIZE;
            let chr = u64::from_le_bytes(buf[o..o + 8].try_into().ok()?) as usize;
            (crate::game::mem::is_ptr(chr) && self.mem.read_u64(chr + CHR_HANDLE)? == handle)
                .then_some(chr)
        })
    }
}

/// The stock expedition menu object (`r14` in the Commence checks, `menuflow`).
pub(crate) struct ExpeditionMenu<'m> {
    mem: &'m dyn GameMem,
    addr: usize,
}

impl<'m> ExpeditionMenu<'m> {
    pub(crate) fn new(mem: &'m dyn GameMem, addr: usize) -> Self {
        ExpeditionMenu { mem, addr }
    }

    /// Its matchmaking settings object, when the pointer is plausible.
    pub(crate) fn settings(&self) -> Option<MenuSettings<'m>> {
        let s = self.mem.read_ptr(self.addr.checked_add(MENU_SETTINGS)?)?;
        Some(MenuSettings {
            mem: self.mem,
            addr: s,
        })
    }
}

/// The expedition menu's settings object (Number of Players and the other matchmaking settings).
pub(crate) struct MenuSettings<'m> {
    mem: &'m dyn GameMem,
    addr: usize,
}

impl MenuSettings<'_> {
    /// Number of Players byte: `menuflow::PLAYERS_ONE` (2), `PLAYERS_TWO` (1), `PLAYERS_THREE` (0).
    pub(crate) fn players(&self) -> Option<u8> {
        self.mem.read_u8(self.addr + MENU_SETTINGS_PLAYERS)
    }

    /// The same byte the game's own Number of Players list writes.
    pub(crate) fn set_players(&self, v: u8) -> Result<(), &'static str> {
        self.mem.write_u8(self.addr + MENU_SETTINGS_PLAYERS, v)
    }
}

/// What the NPC bot manager holds (OFF-4 evidence; MEMORY_MAP §15).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BotManState {
    /// `NPCBotTableParam` table id, -1 = none.
    pub table: i32,
    pub armed: bool,
    /// Spawned bots in the manager's list.
    pub bots: usize,
    /// First qword of the first [`BOT_HEADS`] entries (what they hold is UNKNOWN: logged as evidence).
    pub heads: [u64; BOT_HEADS],
}

/// Entries of the bot list kept in [`BotManState::heads`].
pub(crate) const BOT_HEADS: usize = 8;
/// More entries than this is not a plausible list (6 players per match; a table has at most 8 rows).
const MAX_BOTS: usize = 64;

/// The NPC bot manager (`CSNpcBotMan`, an FD4 singleton re-created on every area load).
pub(crate) struct BotMan<'m> {
    mem: &'m dyn GameMem,
    addr: usize,
}

impl<'m> BotMan<'m> {
    pub(crate) fn new(mem: &'m dyn GameMem, addr: usize) -> Self {
        BotMan { mem, addr }
    }

    /// One read of the manager; `None` when its header is unreadable.
    pub(crate) fn state(&self) -> Option<BotManState> {
        let table = self.mem.read_u32(self.addr + BOTMAN_TABLE)? as i32;
        let armed = self.mem.read_u8(self.addr + BOTMAN_ARMED)? != 0;
        let begin = self.mem.read_u64(self.addr + BOTMAN_BOTS_BEGIN)? as usize;
        let end = self.mem.read_u64(self.addr + BOTMAN_BOTS_END)? as usize;
        let bots = if begin != 0 && end >= begin && (end - begin).is_multiple_of(BOTMAN_BOT_ENTRY) {
            ((end - begin) / BOTMAN_BOT_ENTRY).min(MAX_BOTS)
        } else {
            0
        };
        let mut heads = [0u64; BOT_HEADS];
        for (i, h) in heads.iter_mut().enumerate().take(bots) {
            *h = self.mem.read_u64(begin + i * BOTMAN_BOT_ENTRY).unwrap_or(0);
        }
        Some(BotManState {
            table,
            armed,
            bots,
            heads,
        })
    }

    /// OFF-5 "detach": make the manager forget the bot in `slot` (its handle becomes -1, as in an empty slot). The bot stays in the
    /// world [HYPOTHESIS until the OFF-5 run]; the manager's next `2003[97]` can fill the slot again. Slots 0 and 1 exist (MEMORY_MAP §15).
    pub(crate) fn forget_slot(&self, slot: usize) -> Result<(), &'static str> {
        let begin = self
            .mem
            .read_ptr(self.addr + BOTMAN_BOTS_BEGIN)
            .ok_or("bot list unreadable")?;
        let end = self
            .mem
            .read_ptr(self.addr + BOTMAN_BOTS_END)
            .ok_or("bot list unreadable")?;
        if begin + (slot + 1) * BOTMAN_BOT_ENTRY > end {
            return Err("no such bot slot");
        }
        self.mem
            .write_bytes(begin + slot * BOTMAN_BOT_ENTRY, &u64::MAX.to_le_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::gamemem::FakeMem;

    #[test]
    fn a_character_is_found_by_handle_in_any_set_and_the_hint_comes_first() {
        let m = FakeMem::default();
        let wcm = 0x10_0000;
        // Set 14 (the handle's bits 20..27) is empty; set 3 holds the bot in its 2nd entry.
        let (set, entries, chr) = (0x20_0000, 0x30_0000, 0x40_0000);
        for i in 0..WCM_CHRSET_COUNT {
            m.put_u64(wcm + WCM_CHRSETS + i * 8, 0);
        }
        m.put_u64(wcm + WCM_CHRSETS + 3 * 8, set as u64);
        m.put_u32(set + CHRSET_CAPACITY, 2);
        m.put_u64(set + CHRSET_ENTRIES, entries as u64);
        m.put(entries, &[0u8; 2 * CHRSET_ENTRY_SIZE]);
        m.put_u64(entries + CHRSET_ENTRY_SIZE, chr as u64);
        let handle = 0xFFFF_FFFF_10E0_0001;
        m.put_u64(chr + CHR_HANDLE, handle);
        let w = WorldChrMan::new(&m, wcm);
        assert_eq!(w.find_by_handle(handle, None), Some((chr, 3)));
        assert_eq!(w.find_by_handle(handle, Some(3)), Some((chr, 3)));
        assert_eq!(w.find_by_handle(0xFFFF_FFFF_10E0_0002, None), None);
    }

    #[test]
    fn team_and_ground_through_the_character() {
        let m = FakeMem::default();
        let (chr, list, phys, vt) = (0x10_0000, 0x20_0000, 0x30_0000, 0x1_4000_0000);
        m.put(chr + CHR_TEAM, &[2]);
        m.put_u64(chr + PLAYER_MODULES, list as u64);
        for s in (0..MODULES_SPAN).step_by(8) {
            m.put_u64(list + s, 0);
        }
        m.put_u64(list + 0x68, phys as u64);
        m.put_u64(phys, vt as u64);
        m.put(phys + PHYSICS_GROUND, &[0]);
        let c = ChrIns::new(&m, chr);
        c.set_team(29).expect("write");
        assert_eq!(c.team(), Some(29));
        assert_eq!(c.grounded(vt), Some(false));
        m.put(phys + PHYSICS_GROUND, &[1]);
        assert_eq!(c.grounded(vt), Some(true));
        assert_eq!(c.grounded(vt + 8), None, "wrong vtable: no physics module");
    }

    #[test]
    fn forgetting_a_slot_empties_only_that_slot() {
        let m = FakeMem::default();
        let (man, list) = (0x10_0000, 0x20_0000);
        m.put_u64(man + BOTMAN_BOTS_BEGIN, list as u64);
        m.put_u64(man + BOTMAN_BOTS_END, (list + 2 * BOTMAN_BOT_ENTRY) as u64);
        m.put_u64(list, 0xAAAA);
        m.put_u64(list + BOTMAN_BOT_ENTRY, 0xBBBB);
        let b = BotMan::new(&m, man);
        b.forget_slot(1).expect("forget");
        assert_eq!(m.read_u64(list), Some(0xAAAA));
        assert_eq!(m.read_u64(list + BOTMAN_BOT_ENTRY), Some(u64::MAX));
        assert!(b.forget_slot(2).is_err());
    }

    #[test]
    fn bot_manager_reads_table_armed_and_the_bot_list() {
        let m = FakeMem::default();
        let (man, list) = (0x10_0000, 0x20_0000);
        m.put_u32(man + BOTMAN_TABLE, 10);
        m.put(man + BOTMAN_ARMED, &[1]);
        m.put_u64(man + BOTMAN_BOTS_BEGIN, list as u64);
        m.put_u64(man + BOTMAN_BOTS_END, (list + 2 * BOTMAN_BOT_ENTRY) as u64);
        m.put_u64(list, 0xAAAA);
        m.put_u64(list + BOTMAN_BOT_ENTRY, 0xBBBB);
        let s = BotMan::new(&m, man).state().expect("state");
        assert_eq!((s.table, s.armed, s.bots), (10, true, 2));
        assert_eq!(&s.heads[..3], &[0xAAAA, 0xBBBB, 0]);
        // Cleared: table -1, not armed, empty list.
        m.put_u32(man + BOTMAN_TABLE, u32::MAX);
        m.put(man + BOTMAN_ARMED, &[0]);
        m.put_u64(man + BOTMAN_BOTS_END, list as u64);
        let s = BotMan::new(&m, man).state().expect("state");
        assert_eq!((s.table, s.armed, s.bots), (-1, false, 0));
        assert!(BotMan::new(&m, 0x90_0000).state().is_none());
    }

    #[test]
    fn chr_type_maps_to_place() {
        let m = FakeMem::default();
        m.put_u32(0x10_0000 + CHR_TYPE, 0);
        m.put_u32(0x20_0000 + CHR_TYPE, 13);
        m.put_u32(0x30_0000 + CHR_TYPE, 5);
        assert_eq!(ChrIns::new(&m, 0x10_0000).place(), Some(Place::Roundtable));
        assert_eq!(ChrIns::new(&m, 0x20_0000).place(), Some(Place::Expedition));
        assert_eq!(ChrIns::new(&m, 0x30_0000).place(), Some(Place::Other(5)));
        assert_eq!(ChrIns::new(&m, 0x40_0000).place(), None);
    }

    #[test]
    fn menu_settings_players_read_and_write() {
        let m = FakeMem::default();
        let (menu, settings) = (0x10_0000, 0x20_0000);
        m.put_u64(menu + MENU_SETTINGS, settings as u64);
        m.put(settings + MENU_SETTINGS_PLAYERS, &[2]);
        let s = ExpeditionMenu::new(&m, menu).settings().expect("settings");
        assert_eq!(s.players(), Some(2));
        s.set_players(1).expect("write");
        assert_eq!(s.players(), Some(1));
    }

    #[test]
    fn menu_without_a_settings_pointer_has_no_settings() {
        let m = FakeMem::default();
        m.put_u64(0x10_0000 + MENU_SETTINGS, 0);
        assert!(ExpeditionMenu::new(&m, 0x10_0000).settings().is_none());
        assert!(ExpeditionMenu::new(&m, 0x50_0000).settings().is_none());
    }
}
