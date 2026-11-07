//! R3 (second half, OFF-1): the `World` read model. The main loop reads what features need once per tick into a [`World`] and hands
//! it to every [`crate::features::Feature`]; features decide from it and never read the game themselves for these facts
//! (ARCHITECTURE §4 "Read model per tick"). It grows with the features that need more; the team and downed-rule controllers still
//! read their own data (they move onto it when a card touches them).

use super::combat_state::CombatCtl;
use super::gamemem::GameMem;
use super::layout;
use super::views::{bot_set_chrs, BotMan, BotManState, ChrIns, ClockState, Place, PlayArea};
use super::{deathhold, flags, input, menugate, spawn};

/// M5-5: players in a match (Seamless Co-op's limit).
pub(crate) const MAX_FIGHTERS: usize = 6;

/// M5-5: the event flags the night match reads (game 1.03.3, `archive/research/M5-no-boss-nights.md` §1 and §6), filled by the main loop
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

/// M5-5: the night flags, all or nothing (`None` while the flag manager is not live).
fn night_flags(base: usize) -> Option<NightFlags> {
    let mut v = [false; 13];
    for (o, id) in v.iter_mut().zip(NightFlags::IDS) {
        *o = flags::get(base, id).ok()?;
    }
    Some(NightFlags::from_values(v))
}

/// BOT-1: every character in the bot set with what the bot rules read.
fn read_bots(mem: &dyn GameMem, wcm: usize) -> Vec<BotChr> {
    if wcm == 0 {
        return Vec::new();
    }
    bot_set_chrs(mem, wcm)
        .into_iter()
        .filter_map(|chr| {
            let c = ChrIns::new(mem, chr);
            Some(BotChr {
                handle: c.handle()?,
                chr,
                team: c.team().unwrap_or(0),
                npc_id: c.npc_id().unwrap_or(0),
                hp: c.hp().unwrap_or(0),
                max_hp: c.max_hp().unwrap_or(0),
                pos: None,
                attacker: mem
                    .read_u64(chr + layout::PLAYER_LAST_ATTACKER)
                    .unwrap_or(0),
            })
        })
        .collect()
}

/// What the main loop knows before it reads the world.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Inputs {
    /// WorldChrMan instance, 0 = none.
    pub wcm_instance: usize,
    /// The local PlayerIns as the downed rule last resolved it.
    pub local: Option<usize>,
    pub mm_phase: u8,
    /// `CSNpcBotMan` instance, 0 = none.
    pub bot_man: usize,
    /// M5: `WorldPlayAreaManager` instance, 0 = none.
    pub play_area: usize,
    /// R7-3: the exe base (for the readers that need it).
    pub base: usize,
    /// R7-6: the exe image size (the hawk adapter's `size` argument).
    pub size: usize,
    /// R7-3: [`World::local_state`] as the downed rule resolved it (the same call that gave `local`; no extra read).
    pub local_state: Option<(usize, u8)>,
    /// R7-3: [`World::local_team_no`] (no memory read).
    pub local_team_no: Option<u8>,
    /// R7-3: the mod's PvP mode is on (`TeamCtl::pvpve_state` = On) at the start of the tick.
    pub pvpve_on: bool,
}

/// One tick's view of the game.
#[derive(Clone, Debug, Default, PartialEq)]
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
    /// M5: the play-area clock (`None` = no manager, or no clock running: outside an expedition).
    pub clock: Option<ClockState>,
    /// M5-5: the night flags (filled by the main loop; `None` = flag manager not live).
    pub night: Option<NightFlags>,
    /// HM-1: this game's session role (`layout::SESSION_ROLE`; `None` = no session manager). The game changes it when it migrates the host.
    pub session_role: Option<u32>,
    /// M5-5: every ChrSet player (filled by the main loop from the downed rule's controller).
    pub fighters: [Option<crate::game::combat_state::Fighter>; MAX_FIGHTERS],
    /// M5-6: LOCAL's world position and its "on ground" byte (filled by the main loop).
    pub local_pos: Option<([f32; 3], bool)>,
    /// M5-9: the team size the rules use (`mm::effective_team_size`; filled by the main loop; 0 = not filled). 1 = Solo.
    pub team_size: usize,
    /// M5-9: LOCAL holds a revive-item SpEffect (`layout::SPEFFECTS_REVIVE_ITEM`); read only in an expedition.
    pub local_revive_item: bool,
    /// RW-1: LOCAL stands in the Night's Tide (holds a rain SpEffect, `layout::SPEFFECTS_TIDE_RAIN`); read only in an expedition.
    pub local_in_tide: bool,
    /// RW-1: LOCAL's recent hits and who dealt them (filled by the main loop from the combat controller).
    pub local_hits: crate::game::combat_state::LocalHits,
    /// RW-1: the comeback's start step is held (`game::deathhold::held`): the dead body is hidden from the others from then on.
    pub death_held: bool,
    /// RW-1: the spectator asked for the next / previous player this tick (mouse buttons or the pad's shoulder buttons; edges).
    pub spec_next: bool,
    pub spec_prev: bool,
    /// RW-1: the leave key (F10) went down since the last tick (edge; filled by the main loop).
    pub leave_pressed: bool,
    /// RW-7: each fighter's world position, same order as [`World::fighters`] (filled by the main loop).
    pub positions: [Option<[f32; 3]>; MAX_FIGHTERS],
    /// R7-3: the exe base.
    pub base: usize,
    /// R7-6: the exe image size.
    pub size: usize,
    /// R7-3: a match is running: in an expedition with the mod's PvP mode on. [`World::read`] sets the value at the START of the
    /// tick; [`World::after_combat`] refreshes it after `combat.tick`, which is the value the hook modules always used.
    pub in_match: bool,
    /// R7-6: the rivals' handles and addresses (`CombatCtl::rival_handles` / `rival_addrs`). Empty until [`World::after_combat`]
    /// fills them (those calls read game memory, so they run once per tick, after `combat.tick`).
    pub rivals: Rivals,
    /// R7-3: the local player and its downed byte, from the same `CombatCtl::local_state` call as `local`. [`World::after_combat`]
    /// replaces it with the call's result after `combat.tick` (unfiltered, as the hook modules always read it).
    pub local_state: Option<(usize, u8)>,
    /// R7-3: LOCAL's roster team number (refreshed by [`World::after_combat`]).
    pub local_team_no: Option<u8>,
    /// BOT-1: the characters of the bot manager's set (read in an expedition only; empty elsewhere).
    pub bot_chrs: Vec<BotChr>,
    /// BOT-2: LOCAL's handle (who killed a bot).
    pub local_handle: Option<u64>,
    /// BOT-2: bots killed by a mob whose replacement has not spawned yet (`features::bots::pending`, from the previous tick; filled by
    /// the main loop). While above 0 the match is not decided: the replacement is still a rival.
    pub bots_pending: usize,
}

/// BOT-1: one character of the bot manager's set (`views::bot_set_chrs`), read every tick in an expedition.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct BotChr {
    /// Its handle (`0x…10E0000n` in OFF-5).
    pub handle: u64,
    pub chr: usize,
    pub team: u8,
    /// `CHR_ID`: its NpcParam id (`layout::BOT_NPC_PARAM` for the bot tables' Nightfarers).
    pub npc_id: u32,
    pub hp: u32,
    pub max_hp: u32,
    /// World position (filled by the main loop, [`World::fill_bot_positions`]; `None` = not read or unreadable).
    pub pos: Option<[f32; 3]>,
    /// BOT-2: its last attacker's handle (`PLAYER_LAST_ATTACKER`; 0 or all-ones = none). Valid on the game that applied the hits
    /// (MEMORY_MAP §21): the host's, where the bots live.
    pub attacker: u64,
}

/// R7-3: the players this game marks as RIVAL (see [`World::rivals`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Rivals {
    pub handles: Vec<u64>,
    pub addrs: Vec<usize>,
}

impl World {
    /// Read this tick's world.
    pub(crate) fn read(mem: &dyn GameMem, i: &Inputs) -> Self {
        let local = i.local.filter(|_| i.wcm_instance != 0);
        let place = local.and_then(|p| ChrIns::new(mem, p).place());
        let local_revive_item = place == Some(Place::Expedition)
            && local.is_some_and(|p| {
                ChrIns::new(mem, p).holds_any_speffect(&layout::SPEFFECTS_REVIVE_ITEM)
            });
        let local_in_tide = place == Some(Place::Expedition)
            && local.is_some_and(|p| {
                ChrIns::new(mem, p).holds_any_speffect(&layout::SPEFFECTS_TIDE_RAIN)
            });
        World {
            wcm_instance: i.wcm_instance,
            local,
            place,
            mm_phase: i.mm_phase,
            bots: (i.bot_man != 0)
                .then(|| BotMan::new(mem, i.bot_man).state())
                .flatten(),
            clock: (i.play_area != 0)
                .then(|| PlayArea::new(mem, i.play_area).clock())
                .flatten(),
            night: None,
            session_role: mem
                .read_u64(i.base + layout::RVA_SESSION)
                .filter(|&s| s != 0)
                .and_then(|s| mem.read_u32(s as usize + layout::SESSION_ROLE)),
            fighters: [None; MAX_FIGHTERS],
            local_pos: None,
            team_size: 0,
            local_revive_item,
            local_in_tide,
            local_hits: crate::game::combat_state::LocalHits::default(),
            leave_pressed: false,
            spec_next: false,
            spec_prev: false,
            death_held: false,
            positions: [None; MAX_FIGHTERS],
            base: i.base,
            size: i.size,
            in_match: place == Some(Place::Expedition) && i.pvpve_on,
            rivals: Rivals::default(),
            local_state: local.and(i.local_state),
            local_team_no: i.local_team_no,
            local_handle: local.and_then(|p| ChrIns::new(mem, p).handle()),
            bots_pending: 0,
            bot_chrs: if place == Some(Place::Expedition) {
                read_bots(mem, i.wcm_instance)
            } else {
                Vec::new()
            },
        }
    }

    /// BOT-1: each bot's world position (its physics position minus LOCAL's origin, like the players'; `spawn::world_position_of`).
    pub(crate) fn fill_bot_positions(&mut self, base: usize) {
        let Some(local) = self.local else {
            return;
        };
        for b in &mut self.bot_chrs {
            b.pos = spawn::world_position_of(b.chr, local, base);
        }
    }

    /// BOT-2: the bots that play the match as rivals: the bot tables' Nightfarers ([`layout::BOT_NPC_PARAM`]) that the bot fill has put on
    /// a rival team (anything but the game's ally value 2). A bot left on 2 is one the fill did not keep (the clear removes it).
    pub(crate) fn rival_bots(&self) -> impl Iterator<Item = &BotChr> {
        self.bot_chrs
            .iter()
            .filter(|b| b.npc_id == layout::BOT_NPC_PARAM && b.team != 0 && b.team != 2)
    }

    /// BOT-1: the bot with this handle, if it is in the set this tick.
    pub(crate) fn bot(&self, handle: u64) -> Option<&BotChr> {
        self.bot_chrs.iter().find(|b| b.handle == handle)
    }

    /// R7-3: what the night match reads besides the read model, filled in one place (it was a block in the main loop). Only in an
    /// expedition. The order of the reads is the order the loop always had: flags, every player's downed state, LOCAL's position and
    /// hits, the F10 and spectate edges (read every tick so their state stays current, but dropped while a menu is open), the death
    /// hold, each fighter's position, the team size.
    pub(crate) fn fill_expedition(
        &mut self,
        combat: Option<&CombatCtl>,
        base: usize,
        team_size: usize,
    ) {
        if !self.in_expedition() {
            return;
        }
        self.night = night_flags(base);
        if let Some(c) = combat {
            for (o, f) in self.fighters.iter_mut().zip(c.fighters()) {
                *o = Some(f);
            }
        }
        self.local_pos = self.local.and_then(|p| spawn::world_position(p, base));
        if let Some(c) = combat {
            self.local_hits = c.local_hits();
        }
        self.leave_pressed = input::leave_key_edge();
        let (sn, sp) = input::spectate_switch_edges();
        let in_menu = menugate::menu_open(base);
        (self.spec_next, self.spec_prev) = (sn && !in_menu, sp && !in_menu);
        self.death_held = deathhold::held();
        if let Some(local) = self.local {
            for (o, f) in self.positions.iter_mut().zip(self.fighters.iter()) {
                *o = f.and_then(|f| {
                    if f.local {
                        spawn::world_position(f.addr, base).map(|p| p.0)
                    } else {
                        spawn::world_position_of(f.addr, local, base)
                    }
                });
            }
        }
        self.team_size = team_size;
    }

    /// R7-6: the facts the hook modules read, refreshed right after `combat.tick` (the world itself was read before it, so
    /// the rivals are the current tick's, as they always were for these modules). `pvpve_on` is `TeamCtl::pvpve_state` NOW.
    pub(crate) fn after_combat(&mut self, combat: Option<&CombatCtl>, pvpve_on: bool) {
        let (handles, addrs) = combat
            .map(|c| (c.rival_handles(), c.rival_addrs()))
            .unwrap_or_default();
        self.apply_after_combat(
            pvpve_on,
            combat.and_then(|c| c.local_state()),
            combat.and_then(|c| c.local_team_no()),
            Rivals { handles, addrs },
        );
    }

    /// The pure half of [`World::after_combat`].
    pub(crate) fn apply_after_combat(
        &mut self,
        pvpve_on: bool,
        local_state: Option<(usize, u8)>,
        local_team_no: Option<u8>,
        rivals: Rivals,
    ) {
        self.in_match = self.in_expedition() && pvpve_on;
        self.local_state = local_state;
        self.local_team_no = local_team_no;
        self.rivals = rivals;
    }

    /// True while the local player is on an expedition (not the Roundtable, not a load screen).
    pub(crate) fn in_expedition(&self) -> bool {
        self.place == Some(Place::Expedition)
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
        assert_eq!(NightFlags::IDS[9], crate::features::nights::FLAG_NIGHT_END);
        assert_eq!(NightFlags::IDS[10], crate::features::br_match::FLAG_WIN);
        assert_eq!(NightFlags::IDS[11], crate::features::br_match::FLAG_LOSE);
    }
    use crate::game::gamemem::FakeMem;
    use crate::game::layout::CHR_TYPE;

    fn inputs(wcm: usize, local: Option<usize>) -> Inputs {
        Inputs {
            wcm_instance: wcm,
            local,
            ..Inputs::default()
        }
    }

    #[test]
    fn bots_are_read_in_an_expedition_only() {
        use crate::game::layout::{
            CHRSET_CAPACITY, CHRSET_ENTRIES, CHR_HANDLE, CHR_ID, CHR_TEAM, WCM_BOT_SET,
        };
        let m = FakeMem::default();
        let (wcm, entries, chr) = (0x5_0000, 0x30_0000, 0x40_0000);
        m.put_u32(wcm + WCM_BOT_SET + CHRSET_CAPACITY, 1);
        m.put_u64(wcm + WCM_BOT_SET + CHRSET_ENTRIES, entries as u64);
        m.put_u64(entries, chr as u64);
        m.put_u64(entries + 8, 0);
        m.put_u64(chr + CHR_HANDLE, 0xFFFF_FFFF_10E0_0000);
        m.put_u32(chr + CHR_ID, 600_040_000);
        m.put(chr + CHR_TEAM, &[2]);
        m.put_u32(0x10_0000 + CHR_TYPE, 13);
        let w = World::read(&m, &inputs(wcm, Some(0x10_0000)));
        let b = w.bot(0xFFFF_FFFF_10E0_0000).expect("found");
        assert_eq!((b.chr, b.team, b.npc_id), (chr, 2, 600_040_000));
        m.put_u32(0x10_0000 + CHR_TYPE, 0);
        assert!(World::read(&m, &inputs(wcm, Some(0x10_0000)))
            .bot_chrs
            .is_empty());
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
    fn after_combat_refreshes_what_the_hook_modules_read() {
        let m = FakeMem::default();
        m.put_u32(0x10_0000 + CHR_TYPE, 13);
        let mut w = World::read(&m, &inputs(0x5_0000, Some(0x10_0000)));
        assert!(!w.in_match, "PvP mode was off at the start of the tick");
        let r = Rivals {
            handles: vec![7],
            addrs: vec![0x20_0000],
        };
        w.apply_after_combat(true, Some((0x10_0000, 0)), Some(3), r.clone());
        assert!(w.in_match);
        assert_eq!(w.rivals, r);
        assert_eq!(w.local_state, Some((0x10_0000, 0)));
        assert_eq!(w.local_team_no, Some(3));
        // Off again, or not in an expedition: no match; no combat controller: no rivals.
        w.apply_after_combat(false, None, None, Rivals::default());
        assert!(!w.in_match && w.rivals == Rivals::default() && w.local_state.is_none());
        w.after_combat(None, true);
        assert!(w.in_match && w.rivals == Rivals::default());
        w.place = Some(Place::Roundtable);
        w.after_combat(None, true);
        assert!(!w.in_match);
    }

    #[test]
    fn read_fills_the_loop_facts_without_reading_more() {
        let m = FakeMem::default();
        m.put_u32(0x10_0000 + CHR_TYPE, 13);
        let i = Inputs {
            base: 0x1000,
            local_state: Some((0x10_0000, 1)),
            local_team_no: Some(2),
            pvpve_on: true,
            ..inputs(0x5_0000, Some(0x10_0000))
        };
        let w = World::read(&m, &i);
        assert_eq!(w.base, 0x1000);
        assert_eq!(w.local_state, Some((0x10_0000, 1)));
        assert_eq!(w.local_team_no, Some(2));
        assert!(w.in_match);
        assert_eq!(w.rivals, Rivals::default());
        // Not in an expedition, or the PvP mode off: no match.
        m.put_u32(0x10_0000 + CHR_TYPE, 0);
        assert!(!World::read(&m, &i).in_match);
        m.put_u32(0x10_0000 + CHR_TYPE, 13);
        let off = Inputs {
            pvpve_on: false,
            ..i
        };
        assert!(!World::read(&m, &off).in_match);
        // No world: no local player, so no local state either.
        let none = Inputs {
            wcm_instance: 0,
            ..i
        };
        assert_eq!(World::read(&m, &none).local_state, None);
    }
}
