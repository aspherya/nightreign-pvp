//! R2 (second half, OFF-1): typed views over [`GameMem`]. Each view wraps one object address and turns the offsets of
//! [`super::layout`] into named accessors, so feature code calls `chr.chr_type()`, never `addr + 0x70` (ARCHITECTURE §6).
//! Views hold no state of their own and are created per use.

use super::gamemem::GameMem;
use super::layout::{
    BOTMAN_ARMED, BOTMAN_BOTS_BEGIN, BOTMAN_BOTS_END, BOTMAN_BOT_ENTRY, BOTMAN_TABLE, CHR_HANDLE,
    CHR_ID, CHR_TEAM, CHR_TYPE, CHR_TYPE_EXPEDITION, CHR_TYPE_ROUNDTABLE, DATA_MODULE_HP,
    MENU_SETTINGS, MENU_SETTINGS_PLAYERS, PLAYAREA_CLOCK, PLAYAREA_CLOCK_PACKED, PLAYAREA_RUNNING,
    PLAYAREA_SET_HOUR, PLAYAREA_SET_MINUTE, PLAYAREA_SET_SECOND, PLAYAREA_SPEED, PLAYER_DOWNED,
    PLAYER_MODULES, PLAYER_SPEFFECTS, SPEFFECT_FIRST, SPEFFECT_ID, SPEFFECT_MAX, SPEFFECT_NEXT,
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

    /// PlayerIns only: current HP from the character data module (MEMORY_MAP §5).
    pub(crate) fn hp(&self) -> Option<u32> {
        let list = self.mem.read_ptr(self.addr + PLAYER_MODULES)?;
        let data = self.mem.read_ptr(list)?;
        self.mem.read_u32(data + DATA_MODULE_HP)
    }

    /// PlayerIns only: the downed state byte, 0 up / 1 downed / 2 getting up (MEMORY_MAP §6).
    pub(crate) fn downed_state(&self) -> Option<u8> {
        self.mem.read_u8(self.addr + PLAYER_DOWNED)
    }

    /// The SpEffect ids on this character (the game's own list walk, MEMORY_MAP §17). Bounded; stops at the first unreadable node.
    pub(crate) fn speffect_ids(&self) -> Vec<i32> {
        let mut out = Vec::new();
        let Some(holder) = self.mem.read_ptr(self.addr + PLAYER_SPEFFECTS) else {
            return out;
        };
        let mut node = self.mem.read_ptr(holder + SPEFFECT_FIRST);
        while let Some(n) = node.filter(|_| out.len() < SPEFFECT_MAX) {
            match self.mem.read_u32(n + SPEFFECT_ID) {
                Some(id) => out.push(id as i32),
                None => break,
            }
            node = self.mem.read_ptr(n + SPEFFECT_NEXT);
        }
        out
    }

    /// True when this character holds any of `ids`.
    pub(crate) fn holds_any_speffect(&self, ids: &[i32]) -> bool {
        self.speffect_ids().iter().any(|i| ids.contains(i))
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

/// A time of day on the play-area clock (07:00 is morning; the game holds the clock at 23:59:59 for the night).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClockTime {
    pub h: u8,
    pub m: u8,
    pub s: u8,
}

impl ClockTime {
    /// `None` unless h <= 23, m <= 59, s <= 59 (the game's own check on its set-time mailbox).
    pub(crate) fn new(h: u8, m: u8, s: u8) -> Option<Self> {
        (h <= 23 && m <= 59 && s <= 59).then_some(ClockTime { h, m, s })
    }

    /// From the packed half of a `DLDateTime`.
    pub(crate) fn from_packed(p: u64) -> Self {
        ClockTime {
            h: ((p >> 34) & 0x1F) as u8,
            m: ((p >> 39) & 0x3F) as u8,
            s: ((p >> 45) & 0x3F) as u8,
        }
    }
}

impl std::fmt::Display for ClockTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.h, self.m, self.s)
    }
}

/// What the play-area clock reads now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClockState {
    pub time: ClockTime,
    pub speed: f32,
    /// The clock advanced in the game's last update.
    pub running: bool,
    /// The set-time mailbox is empty (no request waiting for the game's next update).
    pub mailbox_free: bool,
}

/// The play-area manager (`WorldPlayAreaManager`, an FD4 singleton re-created on area loads): the day clock and the circle.
pub(crate) struct PlayArea<'m> {
    mem: &'m dyn GameMem,
    addr: usize,
}

impl<'m> PlayArea<'m> {
    pub(crate) fn new(mem: &'m dyn GameMem, addr: usize) -> Self {
        PlayArea { mem, addr }
    }

    /// `None` when unreadable or when no clock runs (its FILETIME half is 0).
    pub(crate) fn clock(&self) -> Option<ClockState> {
        if self.mem.read_u64(self.addr + PLAYAREA_CLOCK)? == 0 {
            return None;
        }
        let packed = self.mem.read_u64(self.addr + PLAYAREA_CLOCK_PACKED)?;
        let mailbox = [PLAYAREA_SET_HOUR, PLAYAREA_SET_MINUTE, PLAYAREA_SET_SECOND]
            .map(|o| self.mem.read_u32(self.addr + o));
        Some(ClockState {
            time: ClockTime::from_packed(packed),
            speed: f32::from_bits(self.mem.read_u32(self.addr + PLAYAREA_SPEED)?),
            running: self.mem.read_u8(self.addr + PLAYAREA_RUNNING)? != 0,
            mailbox_free: mailbox.iter().all(|v| *v == Some(u32::MAX)),
        })
    }

    /// Ask the game to set its clock to `t` on its next update (its own setter, and in a session its own sync, do the rest).
    /// Refused while the mailbox holds another request. Second and minute go first and the hour last: the game ignores the
    /// triple while the hour still reads -1, so it never sees a half-written time.
    pub(crate) fn request_time(&self, t: ClockTime) -> Result<(), &'static str> {
        if !self.clock().ok_or("no play-area clock")?.mailbox_free {
            return Err("the game's set-time mailbox is busy");
        }
        for (off, v) in [
            (PLAYAREA_SET_SECOND, t.s),
            (PLAYAREA_SET_MINUTE, t.m),
            (PLAYAREA_SET_HOUR, t.h),
        ] {
            self.mem
                .write_bytes(self.addr + off, &u32::from(v).to_le_bytes())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::gamemem::FakeMem;

    fn play_area(m: &FakeMem, addr: usize, h: u64, mi: u64, s: u64) {
        m.put_u64(addr + PLAYAREA_CLOCK, 0x01DC_0000_0000_0000);
        m.put_u64(
            addr + PLAYAREA_CLOCK_PACKED,
            (s << 45) | (mi << 39) | (h << 34) | 2026,
        );
        m.put_u32(addr + PLAYAREA_SPEED, 2.0f32.to_bits());
        for o in [PLAYAREA_SET_HOUR, PLAYAREA_SET_MINUTE, PLAYAREA_SET_SECOND] {
            m.put_u32(addr + o, u32::MAX);
        }
        m.put(addr + PLAYAREA_RUNNING, &[1]);
    }

    #[test]
    fn play_area_clock_reads_the_packed_time() {
        let m = FakeMem::default();
        play_area(&m, 0x50_0000, 22, 58, 7);
        let c = PlayArea::new(&m, 0x50_0000).clock().expect("clock");
        assert_eq!(c.time, ClockTime::new(22, 58, 7).unwrap());
        assert_eq!(c.time.to_string(), "22:58:07");
        assert!(c.running && c.mailbox_free);
        assert_eq!(c.speed, 2.0);
        m.put_u64(0x50_0000 + PLAYAREA_CLOCK, 0);
        assert!(
            PlayArea::new(&m, 0x50_0000).clock().is_none(),
            "no clock outside an expedition"
        );
    }

    #[test]
    fn a_time_request_fills_the_mailbox_hour_last_and_only_when_free() {
        let m = FakeMem::default();
        play_area(&m, 0x50_0000, 9, 0, 0);
        let pa = PlayArea::new(&m, 0x50_0000);
        pa.request_time(ClockTime::new(22, 55, 0).unwrap())
            .expect("written");
        assert_eq!(m.read_u32(0x50_0000 + PLAYAREA_SET_HOUR), Some(22));
        assert_eq!(m.read_u32(0x50_0000 + PLAYAREA_SET_MINUTE), Some(55));
        assert_eq!(m.read_u32(0x50_0000 + PLAYAREA_SET_SECOND), Some(0));
        assert!(!pa.clock().unwrap().mailbox_free);
        assert!(
            pa.request_time(ClockTime::new(7, 0, 0).unwrap()).is_err(),
            "busy until the game takes it"
        );
    }

    #[test]
    fn clock_times_out_of_range_are_refused() {
        assert!(ClockTime::new(24, 0, 0).is_none());
        assert!(ClockTime::new(23, 60, 0).is_none());
        assert!(ClockTime::new(23, 59, 59).is_some());
    }

    #[test]
    fn speffects_hp_and_state_are_read_through_the_character() {
        let m = FakeMem::default();
        let (chr, holder, n1, n2, list, data) = (
            0x10_0000, 0x20_0000, 0x30_0000, 0x31_0000, 0x40_0000, 0x50_0000,
        );
        m.put_u64(chr + PLAYER_SPEFFECTS, holder as u64);
        m.put_u64(holder + SPEFFECT_FIRST, n1 as u64);
        m.put_u32(n1 + SPEFFECT_ID, 102_130);
        m.put_u64(n1 + SPEFFECT_NEXT, n2 as u64);
        m.put_u32(n2 + SPEFFECT_ID, 6_999_500);
        m.put_u64(n2 + SPEFFECT_NEXT, 0);
        m.put_u64(chr + PLAYER_MODULES, list as u64);
        m.put_u64(list, data as u64);
        m.put_u32(data + DATA_MODULE_HP, 240);
        m.put(chr + PLAYER_DOWNED, &[1]);
        let c = ChrIns::new(&m, chr);
        assert_eq!(c.speffect_ids(), vec![102_130, 6_999_500]);
        assert!(c.holds_any_speffect(&crate::game::layout::SPEFFECTS_REVIVE_ITEM));
        assert!(!c.holds_any_speffect(&[540_155]));
        assert_eq!((c.hp(), c.downed_state()), (Some(240), Some(1)));
        // No holder: an empty list, not a fault.
        assert!(ChrIns::new(&m, 0x60_0000).speffect_ids().is_empty());
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
