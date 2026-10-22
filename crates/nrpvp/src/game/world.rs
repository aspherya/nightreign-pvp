//! R3 (second half, OFF-1): the `World` read model. The main loop reads what features need once per tick into a [`World`] and hands
//! it to every [`crate::features::Feature`]; features decide from it and never read the game themselves for these facts
//! (ARCHITECTURE §4 "Read model per tick"). It grows with the features that need more; the team and downed-rule controllers still
//! read their own data (they move onto it when a card touches them).

use super::gamemem::GameMem;
use super::views::{ChrIns, Place};

/// One tick's view of the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct World {
    /// WorldChrMan instance, 0 = no world (title, menu or a load screen).
    pub wcm_instance: usize,
    /// The local PlayerIns, when resolved (the downed rule's local subject).
    pub local: Option<usize>,
    /// Where the local player is (`None` = no local player or unreadable).
    pub place: Option<Place>,
    /// The matchmaking queue phase (`crate::mm::PHASE_*`; idle when the queue is off).
    pub mm_phase: u8,
}

impl World {
    /// Read this tick's world. `local` comes from the controllers that already resolved it.
    pub(crate) fn read(
        mem: &dyn GameMem,
        wcm_instance: usize,
        local: Option<usize>,
        mm_phase: u8,
    ) -> Self {
        let local = local.filter(|_| wcm_instance != 0);
        World {
            wcm_instance,
            local,
            place: local.and_then(|p| ChrIns::new(mem, p).place()),
            mm_phase,
        }
    }

    /// True while the local player is on an expedition (not the Roundtable, not a load screen).
    pub(crate) fn in_expedition(&self) -> bool {
        self.place == Some(Place::Expedition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::gamemem::FakeMem;
    use crate::game::layout::CHR_TYPE;

    #[test]
    fn place_comes_from_the_local_player() {
        let m = FakeMem::default();
        m.put_u32(0x10_0000 + CHR_TYPE, 13);
        let w = World::read(&m, 0x5_0000, Some(0x10_0000), 0);
        assert!(w.in_expedition());
        m.put_u32(0x10_0000 + CHR_TYPE, 0);
        let w = World::read(&m, 0x5_0000, Some(0x10_0000), 0);
        assert_eq!(w.place, Some(Place::Roundtable));
        assert!(!w.in_expedition());
    }

    #[test]
    fn no_world_means_no_local_player() {
        let m = FakeMem::default();
        m.put_u32(0x10_0000 + CHR_TYPE, 13);
        let w = World::read(&m, 0, Some(0x10_0000), 0);
        assert_eq!(w.local, None);
        assert!(!w.in_expedition());
    }
}
