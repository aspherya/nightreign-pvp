//! What the combat logic reads and writes, behind the [`CombatMem`] trait (a fake implements it in the tests).

use std::collections::HashSet;

use super::*;
use crate::game::mem::{self, is_ptr};
use crate::game::rtti::Rtti;

// ---------------------------------------------------------------------------------------------------
// Memory access
// ---------------------------------------------------------------------------------------------------

/// What the combat logic needs from game memory. All reads are guarded; `write` is the single store (the rival revive rule).
pub(crate) trait CombatMem {
    /// The object's first qword is a PlayerIns-derived vtable (RTTI).
    fn is_player(&self, addr: usize) -> bool;
    /// The object's first qword is a vtable whose RTTI class is `CS::PlayerGameData`.
    fn is_gamedata(&self, addr: usize) -> bool;
    fn read_u8(&self, addr: usize) -> Option<u8>;
    fn read_u32(&self, addr: usize) -> Option<u32>;
    fn read_u64(&self, addr: usize) -> Option<u64>;
    fn write(&mut self, addr: usize, data: &[u8]) -> Result<(), &'static str>;
}

/// Guarded game memory plus the RTTI-derived vtable sets.
#[derive(Default)]
pub(crate) struct RealCombatMem {
    pub(super) player_vts: HashSet<usize>,
    pub(super) gd_vts: HashSet<usize>,
}

impl RealCombatMem {
    /// Fill the vtable sets from RTTI. Returns true when both sets are non-empty.
    pub(crate) fn set_rtti(&mut self, rtti: &Rtti, base: usize) -> bool {
        self.player_vts.clear();
        self.gd_vts.clear();
        for (i, c) in rtti.classes.iter().enumerate() {
            let is_gd = rtti.name(i) == GAMEDATA_CLASS;
            if rtti.is_player(i) || is_gd {
                for &vt in &c.vtables {
                    if is_gd {
                        self.gd_vts.insert(base + vt as usize);
                    } else {
                        self.player_vts.insert(base + vt as usize);
                    }
                }
            }
        }
        !self.player_vts.is_empty() && !self.gd_vts.is_empty()
    }
}

impl CombatMem for RealCombatMem {
    fn is_player(&self, addr: usize) -> bool {
        is_ptr(addr) && mem::read::<usize>(addr).is_some_and(|vt| self.player_vts.contains(&vt))
    }
    fn is_gamedata(&self, addr: usize) -> bool {
        is_ptr(addr) && mem::read::<usize>(addr).is_some_and(|vt| self.gd_vts.contains(&vt))
    }
    fn read_u8(&self, addr: usize) -> Option<u8> {
        mem::read::<u8>(addr)
    }
    fn read_u32(&self, addr: usize) -> Option<u32> {
        mem::read::<u32>(addr)
    }
    fn read_u64(&self, addr: usize) -> Option<u64> {
        mem::read::<u64>(addr)
    }
    fn write(&mut self, addr: usize, data: &[u8]) -> Result<(), &'static str> {
        mem::write_bytes(addr, data)
    }
}
