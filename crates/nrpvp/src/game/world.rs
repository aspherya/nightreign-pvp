//! R3 (second half, OFF-1): the `World` read model. The main loop reads what features need once per tick into a [`World`] and hands
//! it to every [`crate::features::Feature`]; features decide from it and never read the game themselves for these facts
//! (ARCHITECTURE §4 "Read model per tick"). It grows with the features that need more; the team and downed-rule controllers still
//! read their own data (they move onto it when a card touches them).

use super::gamemem::GameMem;
use super::layout;
use super::views::{BotMan, BotManState, ChrIns, ClockState, Place, PlayArea, WorldChrMan};

/// Bot characters a feature can ask the world to watch at once (OFF-5: 5 rivals + 1 spare).
pub(crate) const MAX_WATCHED: usize = 6;
/// M5-5: players in a match (Seamless Co-op's limit).
pub(crate) const MAX_FIGHTERS: usize = 6;

/// M5-5: the event flags the night match reads (game 1.03.3, `research/M5-no-boss-nights.md` §1 and §6), filled by the main loop
/// through `game::flags`. `None` in the [`World`] = the flag manager is not live.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NightFlags {
    /// 7515: the clock reads 23:59 (`common` 1107).
    pub night: bool,
    /// 7505: day 2 (set by `common` 1100 once the morning after night 1 has come).
    pub day2: bool,
    /// 7502 (host; network-wide only when our event sets it) or 7503 (the interval, passed to every game by the host's events).
    pub night1_done: bool,
    /// 7507 or 7508.
    pub night2_done: bool,
    /// 7010-7012: a revive item is working on someone (`common` 1183).
    pub reviving: bool,
    /// Our flags, read back for the fallback (still ON = our event file is not loaded).
    pub our_night_end: bool,
    pub our_win: bool,
    pub our_lose: bool,
    /// Our die-at-once flag 10007420 (M5-9): ON gives LOCAL SpEffect 102130 (event 99100805).
    pub our_die: bool,
}

impl NightFlags {
    /// The flags to read, in [`NightFlags::from_values`] order.
    pub(crate) const IDS: [u32; 13] = [
        7515, 7505, 7502, 7503, 7507, 7508, 7010, 7011, 7012, 10_007_400, 10_007_411, 10_007_412,
        10_007_420,
    ];

    /// From the values of [`NightFlags::IDS`].
    pub(crate) fn from_values(v: [bool; 13]) -> Self {
        NightFlags {
            night: v[0],
            day2: v[1],
            night1_done: v[2] || v[3],
            night2_done: v[4] || v[5],
            reviving: v[6] || v[7] || v[8],
            our_night_end: v[9],
            our_win: v[10],
            our_lose: v[11],
            our_die: v[12],
        }
    }
}

/// A watched bot character, found by its handle (OFF-5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BotChr {
    pub handle: u64,
    /// Its ChrIns and the WorldChrMan character set it was found in (the next tick's search hint).
    pub chr: usize,
    pub set: usize,
    pub team: u8,
    /// `CHR_ID` (+0x68): its NpcParam id, if that is what the field holds for NPCs (logged as evidence).
    pub npc_id: u32,
}

/// What the main loop knows before it reads the world.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Inputs<'a> {
    /// WorldChrMan instance, 0 = none.
    pub wcm_instance: usize,
    /// The local PlayerIns as the downed rule last resolved it.
    pub local: Option<usize>,
    pub mm_phase: u8,
    /// `CSNpcBotMan` instance, 0 = none.
    pub bot_man: usize,
    /// Bot handles to find, each with the set it was last found in.
    pub watched: &'a [(u64, Option<usize>)],
    /// M5: `WorldPlayAreaManager` instance, 0 = none.
    pub play_area: usize,
}

/// One tick's view of the game.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct World {
    /// WorldChrMan instance, 0 = no world (title, menu or a load screen).
    pub wcm_instance: usize,
    /// The local PlayerIns, when resolved (the downed rule's local subject).
    pub local: Option<usize>,
    /// Where the local player is (`None` = no local player or unreadable).
    pub place: Option<Place>,
    /// The matchmaking queue phase (`crate::mm::PHASE_*`; idle when the queue is off).
    pub mm_phase: u8,
    /// OFF-4: the NPC bot manager's state (`None` = no instance or unreadable).
    pub bots: Option<BotManState>,
    /// OFF-5: the watched bot characters that were found, in the order asked.
    pub bot_chrs: [Option<BotChr>; MAX_WATCHED],
    /// M5: the play-area clock (`None` = no manager, or no clock running: outside an expedition).
    pub clock: Option<ClockState>,
    /// M5-5: the night flags (filled by the main loop; `None` = flag manager not live).
    pub night: Option<NightFlags>,
    /// M5-5: every ChrSet player (filled by the main loop from the downed rule's controller).
    pub fighters: [Option<crate::game::combat_state::Fighter>; MAX_FIGHTERS],
    /// M5-6: LOCAL's world position and its "on ground" byte (filled by the main loop).
    pub local_pos: Option<([f32; 3], bool)>,
    /// M5-9: the team size the rules use (`mm::effective_team_size`; filled by the main loop; 0 = not filled). 1 = Solo.
    pub team_size: usize,
    /// M5-9: LOCAL holds a revive-item SpEffect (`layout::SPEFFECTS_REVIVE_ITEM`); read only in an expedition.
    pub local_revive_item: bool,
}

impl World {
    /// Read this tick's world.
    pub(crate) fn read(mem: &dyn GameMem, i: &Inputs) -> Self {
        let local = i.local.filter(|_| i.wcm_instance != 0);
        let mut bot_chrs = [None; MAX_WATCHED];
        if i.wcm_instance != 0 {
            let wcm = WorldChrMan::new(mem, i.wcm_instance);
            for (out, &(handle, hint)) in bot_chrs.iter_mut().zip(i.watched) {
                *out = wcm.find_by_handle(handle, hint).map(|(chr, set)| {
                    let c = ChrIns::new(mem, chr);
                    BotChr {
                        handle,
                        chr,
                        set,
                        team: c.team().unwrap_or(0),
                        npc_id: c.npc_id().unwrap_or(0),
                    }
                });
            }
        }
        let place = local.and_then(|p| ChrIns::new(mem, p).place());
        let local_revive_item = place == Some(Place::Expedition)
            && local.is_some_and(|p| {
                ChrIns::new(mem, p).holds_any_speffect(&layout::SPEFFECTS_REVIVE_ITEM)
            });
        World {
            wcm_instance: i.wcm_instance,
            local,
            place,
            mm_phase: i.mm_phase,
            bots: (i.bot_man != 0)
                .then(|| BotMan::new(mem, i.bot_man).state())
                .flatten(),
            bot_chrs,
            clock: (i.play_area != 0)
                .then(|| PlayArea::new(mem, i.play_area).clock())
                .flatten(),
            night: None,
            fighters: [None; MAX_FIGHTERS],
            local_pos: None,
            team_size: 0,
            local_revive_item,
        }
    }

    /// M5-9: a Solo match (team size 1).
    pub(crate) fn solo(&self) -> bool {
        self.team_size == 1
    }

    /// True while the local player is on an expedition (not the Roundtable, not a load screen).
    pub(crate) fn in_expedition(&self) -> bool {
        self.place == Some(Place::Expedition)
    }

    /// The watched bot with this handle, if it was found this tick.
    pub(crate) fn bot_chr(&self, handle: u64) -> Option<BotChr> {
        self.bot_chrs
            .iter()
            .flatten()
            .find(|b| b.handle == handle)
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn night_flags_merge_host_and_client_views() {
        let mut v = [false; 13];
        v[3] = true; // 7503 only (a client after the host ended night 1)
        v[7] = true;
        let f = NightFlags::from_values(v);
        assert!(f.night1_done && f.reviving && !f.night && !f.night2_done);
        assert_eq!(
            NightFlags::IDS[9],
            crate::features::night_match::FLAG_NIGHT_END
        );
        assert_eq!(NightFlags::IDS[10], crate::features::night_match::FLAG_WIN);
        assert_eq!(NightFlags::IDS[11], crate::features::night_match::FLAG_LOSE);
    }
    use crate::game::gamemem::FakeMem;
    use crate::game::layout::{
        CHRSET_CAPACITY, CHRSET_ENTRIES, CHR_HANDLE, CHR_TEAM, CHR_TYPE, WCM_CHRSETS,
        WCM_CHRSET_COUNT,
    };

    fn inputs(wcm: usize, local: Option<usize>) -> Inputs<'static> {
        Inputs {
            wcm_instance: wcm,
            local,
            ..Inputs::default()
        }
    }

    #[test]
    fn place_comes_from_the_local_player() {
        let m = FakeMem::default();
        m.put_u32(0x10_0000 + CHR_TYPE, 13);
        let w = World::read(&m, &inputs(0x5_0000, Some(0x10_0000)));
        assert!(w.in_expedition());
        m.put_u32(0x10_0000 + CHR_TYPE, 0);
        let w = World::read(&m, &inputs(0x5_0000, Some(0x10_0000)));
        assert_eq!(w.place, Some(Place::Roundtable));
        assert!(!w.in_expedition());
    }

    #[test]
    fn no_world_means_no_local_player() {
        let m = FakeMem::default();
        m.put_u32(0x10_0000 + CHR_TYPE, 13);
        let w = World::read(&m, &inputs(0, Some(0x10_0000)));
        assert_eq!(w.local, None);
        assert!(!w.in_expedition());
    }

    #[test]
    fn watched_bots_are_found_with_their_team() {
        let m = FakeMem::default();
        let (wcm, set, entries, chr) = (0x10_0000, 0x20_0000, 0x30_0000, 0x40_0000);
        for i in 0..WCM_CHRSET_COUNT {
            m.put_u64(wcm + WCM_CHRSETS + i * 8, 0);
        }
        m.put_u64(wcm + WCM_CHRSETS + 14 * 8, set as u64);
        m.put_u32(set + CHRSET_CAPACITY, 1);
        m.put_u64(set + CHRSET_ENTRIES, entries as u64);
        m.put_u64(entries, chr as u64);
        m.put_u64(entries + 8, 0);
        m.put_u64(chr + CHR_HANDLE, 0xFFFF_FFFF_10E0_0000);
        m.put(chr + CHR_TEAM, &[2]);
        let watched = [(0xFFFF_FFFF_10E0_0000, None), (0xFFFF_FFFF_10E0_0001, None)];
        let w = World::read(
            &m,
            &Inputs {
                wcm_instance: wcm,
                watched: &watched,
                ..Inputs::default()
            },
        );
        let b = w.bot_chr(0xFFFF_FFFF_10E0_0000).expect("found");
        assert_eq!((b.chr, b.set, b.team), (chr, 14, 2));
        assert_eq!(w.bot_chr(0xFFFF_FFFF_10E0_0001), None);
    }
}
