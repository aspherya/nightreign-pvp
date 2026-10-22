//! R2 (second half, OFF-1): typed views over [`GameMem`]. Each view wraps one object address and turns the offsets of
//! [`super::layout`] into named accessors, so feature code calls `chr.chr_type()`, never `addr + 0x70` (ARCHITECTURE §6).
//! Views hold no state of their own and are created per use.

use super::gamemem::GameMem;
use super::layout::{
    CHR_TYPE, CHR_TYPE_EXPEDITION, CHR_TYPE_ROUNDTABLE, MENU_SETTINGS, MENU_SETTINGS_PLAYERS,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::gamemem::FakeMem;

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
