//! R7-6 (P2-6): the hook modules as thin [`Feature`] adapters. Each adapter calls its module's existing `tick` with exactly the
//! arguments the main loop used to build by hand (`game::watch`, before R7-6), and the list keeps that call order. The modules keep
//! their statics and install code (their hooks run on game threads); nothing here knows how they work.
//!
//! Two phases keep the tick order (it is behaviour):
//! - `multiplay` is a [`Phase::Rules`] feature: it ran just before the rules features, with the world read before `combat.tick`.
//! - the rest are [`Phase::AfterCombat`]: they ran right after `combat.tick`, so they read the world as [`World::after_combat`]
//!   refreshed it (rivals, `in_match`, `local_state` of the current tick). Their order in the list is the order they always ran:
//!   deathhold, menugate, hudfilter, runeshare, defeatdrop, orbclaim, maphide, hawk. `maphide` stays after `hudfilter` (it reads
//!   [`hudfilter::map_hooked`]). The hardware-slot claim order (deathhold / menugate / defeatdrop / reviveblock) follows from it.
//!
//! Still beside `combat` in the loop, not here: `reviveblock` (the always-on revive rule, it needs `&mut CombatCtl`), `bartrace`,
//! `deathtrace` and `teleport::hitkill_tick` (dev instrumentation).
//!
//! Each adapter's call goes through the [`Hooks`] port so a test can record what was forwarded; [`Real`] is the port the game uses.

use super::{Ctx, Feature, Phase};
use crate::game::world::World;
use crate::game::{
    deathhold, defeatdrop, hawk, hudfilter, maphide, menugate, multiplay, orbclaim, runeshare,
    toastfilter,
};

/// The calls into the hook modules, and the two facts they depend on besides the world.
pub(crate) trait Hooks {
    /// `mm::effective_team_size` of the configured size (read at call time: a match can override it).
    fn effective_team_size(&self, configured: usize) -> usize;
    /// `hudfilter::map_hooked()`.
    fn map_hooked(&self) -> bool;
    fn multiplay(&mut self, team_size: usize);
    fn deathhold(&mut self, local: Option<usize>, base: usize);
    fn menugate(&mut self, local: Option<usize>, base: usize);
    fn hudfilter(
        &mut self,
        base: usize,
        in_match: bool,
        rivals: &[u64],
        rival_chrs: &[usize],
        local: Option<usize>,
    );
    fn runeshare(
        &mut self,
        base: usize,
        in_match: bool,
        solo: bool,
        rivals: &[u64],
        local: Option<usize>,
    );
    fn defeatdrop(&mut self, base: usize, in_match: bool);
    fn orbclaim(&mut self, base: usize, in_match: bool, my_team: Option<u8>, local: Option<usize>);
    fn maphide(&mut self, base: usize, solo_match: bool);
    fn toastfilter(&mut self, in_match: bool);
    fn hawk(&mut self, base: usize, size: usize, local: Option<usize>);
}

/// The port the game uses: straight calls into the modules.
pub(crate) struct Real;

impl Hooks for Real {
    fn effective_team_size(&self, configured: usize) -> usize {
        crate::mm::effective_team_size(configured)
    }
    fn map_hooked(&self) -> bool {
        hudfilter::map_hooked()
    }
    fn multiplay(&mut self, team_size: usize) {
        multiplay::tick(team_size);
    }
    fn deathhold(&mut self, local: Option<usize>, base: usize) {
        deathhold::tick(local, base);
    }
    fn menugate(&mut self, local: Option<usize>, base: usize) {
        menugate::tick(local, base);
    }
    fn hudfilter(
        &mut self,
        base: usize,
        in_match: bool,
        rivals: &[u64],
        rival_chrs: &[usize],
        local: Option<usize>,
    ) {
        hudfilter::tick(base, in_match, rivals, rival_chrs, local);
    }
    fn runeshare(
        &mut self,
        base: usize,
        in_match: bool,
        solo: bool,
        rivals: &[u64],
        local: Option<usize>,
    ) {
        runeshare::tick(base, in_match, solo, rivals, local);
    }
    fn defeatdrop(&mut self, base: usize, in_match: bool) {
        defeatdrop::tick(base, in_match);
    }
    fn orbclaim(&mut self, base: usize, in_match: bool, my_team: Option<u8>, local: Option<usize>) {
        orbclaim::tick(base, in_match, my_team, local);
    }
    fn maphide(&mut self, base: usize, solo_match: bool) {
        maphide::tick(base, solo_match);
    }
    fn toastfilter(&mut self, in_match: bool) {
        toastfilter::tick(in_match);
    }
    fn hawk(&mut self, base: usize, size: usize, local: Option<usize>) {
        hawk::tick(base, size, local);
    }
}

/// LOCAL's PlayerIns as the downed rule resolved it after `combat.tick` (what the modules took as `local`).
fn local_of(w: &World) -> Option<usize> {
    w.local_state.map(|(a, _)| a)
}

/// RW-10: enemy scaling follows the team size.
pub(crate) struct Multiplay {
    configured: usize,
}

impl Multiplay {
    pub(crate) fn new(configured_team_size: usize) -> Self {
        Multiplay {
            configured: configured_team_size,
        }
    }
    fn run(&self, _w: &World, h: &mut dyn Hooks) {
        let size = h.effective_team_size(self.configured);
        h.multiplay(size);
    }
}

impl Feature for Multiplay {
    fn name(&self) -> &'static str {
        "multiplay"
    }
    fn tick(&mut self, w: &World, _ctx: &mut Ctx) {
        self.run(w, &mut Real);
    }
}

/// UI-5: no "Ally is showing location" toast in a match (a rival's pin must not tell where it is).
pub(crate) struct ToastFilter;

impl ToastFilter {
    fn run(&self, w: &World, h: &mut dyn Hooks) {
        h.toastfilter(w.in_match);
    }
}

impl Feature for ToastFilter {
    fn name(&self) -> &'static str {
        "toastfilter"
    }
    fn phase(&self) -> Phase {
        Phase::AfterCombat
    }
    fn tick(&mut self, w: &World, _ctx: &mut Ctx) {
        self.run(w, &mut Real);
    }
}

/// M5-9: hold a dead LOCAL's comeback at night in Solo, release it at the morning.
pub(crate) struct DeathHold;

impl DeathHold {
    fn run(&self, w: &World, h: &mut dyn Hooks) {
        h.deathhold(local_of(w), w.base);
    }
}

impl Feature for DeathHold {
    fn name(&self) -> &'static str {
        "deathhold"
    }
    fn phase(&self) -> Phase {
        Phase::AfterCombat
    }
    fn tick(&mut self, w: &World, _ctx: &mut Ctx) {
        self.run(w, &mut Real);
    }
}

/// RW-1: the Esc menu of a defeated player.
pub(crate) struct MenuGate;

impl MenuGate {
    fn run(&self, w: &World, h: &mut dyn Hooks) {
        h.menugate(local_of(w), w.base);
    }
}

impl Feature for MenuGate {
    fn name(&self) -> &'static str {
        "menugate"
    }
    fn phase(&self) -> Phase {
        Phase::AfterCombat
    }
    fn tick(&mut self, w: &World, _ctx: &mut Ctx) {
        self.run(w, &mut Real);
    }
}

/// RW-9: rivals are not on the map, the compass, the party bars or the name tags while a match runs.
pub(crate) struct HudFilter;

impl HudFilter {
    fn run(&self, w: &World, h: &mut dyn Hooks) {
        h.hudfilter(
            w.base,
            w.in_match,
            &w.rivals.handles,
            &w.rivals.addrs,
            local_of(w),
        );
    }
}

impl Feature for HudFilter {
    fn name(&self) -> &'static str {
        "hudfilter"
    }
    fn phase(&self) -> Phase {
        Phase::AfterCombat
    }
    fn tick(&mut self, w: &World, _ctx: &mut Ctx) {
        self.run(w, &mut Real);
    }
}

/// RW-8: a rival's kill credits no runes to LOCAL.
pub(crate) struct RuneShare {
    configured: usize,
}

impl RuneShare {
    pub(crate) fn new(configured_team_size: usize) -> Self {
        RuneShare {
            configured: configured_team_size,
        }
    }
    fn run(&self, w: &World, h: &mut dyn Hooks) {
        let solo = h.effective_team_size(self.configured) == 1;
        h.runeshare(w.base, w.in_match, solo, &w.rivals.handles, local_of(w));
    }
}

impl Feature for RuneShare {
    fn name(&self) -> &'static str {
        "runeshare"
    }
    fn phase(&self) -> Phase {
        Phase::AfterCombat
    }
    fn tick(&mut self, w: &World, _ctx: &mut Ctx) {
        self.run(w, &mut Real);
    }
}

/// RW-3: the host makes the drops of defeated players' gear.
pub(crate) struct DefeatDrop;

impl DefeatDrop {
    fn run(&self, w: &World, h: &mut dyn Hooks) {
        h.defeatdrop(w.base, w.in_match);
    }
}

impl Feature for DefeatDrop {
    fn name(&self) -> &'static str {
        "defeatdrop"
    }
    fn phase(&self) -> Phase {
        Phase::AfterCombat
    }
    fn tick(&mut self, w: &World, _ctx: &mut Ctx) {
        self.run(w, &mut Real);
    }
}

/// RW-5: a boss's Dormant Power orb belongs to the first team that opens it.
pub(crate) struct OrbClaim;

impl OrbClaim {
    fn run(&self, w: &World, h: &mut dyn Hooks) {
        h.orbclaim(w.base, w.in_match, w.local_team_no, local_of(w));
    }
}

impl Feature for OrbClaim {
    fn name(&self) -> &'static str {
        "orbclaim"
    }
    fn phase(&self) -> Phase {
        Phase::AfterCombat
    }
    fn tick(&mut self, w: &World, _ctx: &mut Ctx) {
        self.run(w, &mut Real);
    }
}

/// RW-9 fallback (Solo): the map / compass list emptied when the map site could not be hooked. After [`HudFilter`].
pub(crate) struct MapHide {
    configured: usize,
}

impl MapHide {
    pub(crate) fn new(configured_team_size: usize) -> Self {
        MapHide {
            configured: configured_team_size,
        }
    }
    fn run(&self, w: &World, h: &mut dyn Hooks) {
        let solo_match =
            w.in_match && !h.map_hooked() && h.effective_team_size(self.configured) == 1;
        h.maphide(w.base, solo_match);
    }
}

impl Feature for MapHide {
    fn name(&self) -> &'static str {
        "maphide"
    }
    fn phase(&self) -> Phase {
        Phase::AfterCombat
    }
    fn tick(&mut self, w: &World, _ctx: &mut Ctx) {
        self.run(w, &mut Real);
    }
}

/// HAWK-2: keep the env swap in place, run the hawk crossing's ride phases.
pub(crate) struct Hawk;

impl Hawk {
    fn run(&self, w: &World, h: &mut dyn Hooks) {
        h.hawk(w.base, w.size, local_of(w));
    }
}

impl Feature for Hawk {
    fn name(&self) -> &'static str {
        "hawk"
    }
    fn phase(&self) -> Phase {
        Phase::AfterCombat
    }
    fn tick(&mut self, w: &World, _ctx: &mut Ctx) {
        self.run(w, &mut Real);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::world::Rivals;

    /// Records every forwarded call as text.
    #[derive(Default)]
    struct Rec {
        calls: Vec<String>,
        team: usize,
        map_hooked: bool,
    }

    impl Hooks for Rec {
        fn effective_team_size(&self, configured: usize) -> usize {
            if self.team == 0 {
                configured
            } else {
                self.team
            }
        }
        fn map_hooked(&self) -> bool {
            self.map_hooked
        }
        fn multiplay(&mut self, t: usize) {
            self.calls.push(format!("multiplay({t})"));
        }
        fn deathhold(&mut self, l: Option<usize>, b: usize) {
            self.calls.push(format!("deathhold({l:?},{b})"));
        }
        fn menugate(&mut self, l: Option<usize>, b: usize) {
            self.calls.push(format!("menugate({l:?},{b})"));
        }
        fn hudfilter(&mut self, b: usize, m: bool, r: &[u64], c: &[usize], l: Option<usize>) {
            self.calls
                .push(format!("hudfilter({b},{m},{r:?},{c:?},{l:?})"));
        }
        fn runeshare(&mut self, b: usize, m: bool, s: bool, r: &[u64], l: Option<usize>) {
            self.calls
                .push(format!("runeshare({b},{m},{s},{r:?},{l:?})"));
        }
        fn defeatdrop(&mut self, b: usize, m: bool) {
            self.calls.push(format!("defeatdrop({b},{m})"));
        }
        fn toastfilter(&mut self, m: bool) {
            self.calls.push(format!("toastfilter({m})"));
        }
        fn orbclaim(&mut self, b: usize, m: bool, t: Option<u8>, l: Option<usize>) {
            self.calls.push(format!("orbclaim({b},{m},{t:?},{l:?})"));
        }
        fn maphide(&mut self, b: usize, s: bool) {
            self.calls.push(format!("maphide({b},{s})"));
        }
        fn hawk(&mut self, b: usize, s: usize, l: Option<usize>) {
            self.calls.push(format!("hawk({b},{s},{l:?})"));
        }
    }

    fn world() -> World {
        World {
            base: 0x1000,
            size: 0x2000,
            in_match: true,
            rivals: Rivals {
                handles: vec![7, 8],
                addrs: vec![0x30, 0x40],
            },
            local_state: Some((0x10, 2)),
            local_team_no: Some(3),
            ..World::default()
        }
    }

    #[test]
    fn each_adapter_forwards_the_loops_old_arguments() {
        let w = world();
        let mut h = Rec::default();
        Multiplay::new(3).run(&w, &mut h);
        DeathHold.run(&w, &mut h);
        MenuGate.run(&w, &mut h);
        HudFilter.run(&w, &mut h);
        RuneShare::new(3).run(&w, &mut h);
        DefeatDrop.run(&w, &mut h);
        OrbClaim.run(&w, &mut h);
        MapHide::new(3).run(&w, &mut h);
        ToastFilter.run(&w, &mut h);
        Hawk.run(&w, &mut h);
        assert_eq!(
            h.calls,
            [
                "multiplay(3)",
                "deathhold(Some(16),4096)",
                "menugate(Some(16),4096)",
                "hudfilter(4096,true,[7, 8],[48, 64],Some(16))",
                "runeshare(4096,true,false,[7, 8],Some(16))",
                "defeatdrop(4096,true)",
                "orbclaim(4096,true,Some(3),Some(16))",
                "maphide(4096,false)",
                "toastfilter(true)",
                "hawk(4096,8192,Some(16))",
            ]
        );
    }

    #[test]
    fn team_size_comes_from_the_match_override_at_call_time() {
        let w = world();
        let mut h = Rec {
            team: 1,
            ..Rec::default()
        };
        Multiplay::new(3).run(&w, &mut h);
        RuneShare::new(3).run(&w, &mut h);
        MapHide::new(3).run(&w, &mut h);
        ToastFilter.run(&w, &mut h);
        assert_eq!(
            h.calls,
            [
                "multiplay(1)",
                "runeshare(4096,true,true,[7, 8],Some(16))",
                "maphide(4096,true)",
                "toastfilter(true)"
            ]
        );
    }

    #[test]
    fn maphide_is_a_solo_match_without_the_map_hook_only() {
        let mut w = world();
        let mut h = Rec {
            team: 1,
            map_hooked: true,
            ..Rec::default()
        };
        MapHide::new(1).run(&w, &mut h);
        h.map_hooked = false;
        w.in_match = false;
        MapHide::new(1).run(&w, &mut h);
        assert_eq!(h.calls, ["maphide(4096,false)", "maphide(4096,false)"]);
    }

    #[test]
    fn no_local_player_forwards_none() {
        let w = World {
            local_state: None,
            ..world()
        };
        let mut h = Rec::default();
        DeathHold.run(&w, &mut h);
        Hawk.run(&w, &mut h);
        assert_eq!(h.calls, ["deathhold(None,4096)", "hawk(4096,8192,None)"]);
    }

    #[test]
    fn phases_are_multiplay_rules_and_the_rest_after_combat() {
        assert_eq!(Multiplay::new(1).phase(), Phase::Rules);
        let after: [&dyn Feature; 8] = [
            &DeathHold,
            &MenuGate,
            &HudFilter,
            &RuneShare::new(1),
            &DefeatDrop,
            &OrbClaim,
            &MapHide::new(1),
            &Hawk,
        ];
        assert!(after.iter().all(|f| f.phase() == Phase::AfterCombat));
    }
}
