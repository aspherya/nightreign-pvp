//! R2 (second half, OFF-1): the one port to game memory, [`GameMem`], with the real adapter [`RealMem`] and a fake for tests
//! ([`FakeMem`]). Feature code reaches memory through the typed views in [`super::views`], which take any `GameMem`; pointer math
//! lives there and offsets in [`super::layout`] (ARCHITECTURE §4 "Ports and adapters", §6 "Typed access only").
//!
//! Older code still has its own paths (`mem::read`, `RegionCache`, the downed rule's `CombatMem`); they move onto this port when a
//! card touches them (ARCHITECTURE §6 R2).

#[cfg(test)]
use std::collections::BTreeMap;

use super::mem;

/// Guarded reads and the gated one-byte write. Reads never fault (a stale pointer gives `None`); `write_u8` checks that the page
/// is committed and writable. Object-safe, so a view can hold `&dyn GameMem`.
pub(crate) trait GameMem {
    /// Fill `out` from `addr`; false (and `out` unspecified) when any byte is unreadable.
    fn read_into(&self, addr: usize, out: &mut [u8]) -> bool;
    fn write_u8(&self, addr: usize, value: u8) -> Result<(), &'static str>;

    fn read_u8(&self, addr: usize) -> Option<u8> {
        let mut b = [0u8; 1];
        self.read_into(addr, &mut b).then_some(b[0])
    }
    fn read_u32(&self, addr: usize) -> Option<u32> {
        let mut b = [0u8; 4];
        self.read_into(addr, &mut b).then(|| u32::from_le_bytes(b))
    }
    fn read_u64(&self, addr: usize) -> Option<u64> {
        let mut b = [0u8; 8];
        self.read_into(addr, &mut b).then(|| u64::from_le_bytes(b))
    }
    /// A pointer field: `None` when unreadable or not a plausible user-mode pointer.
    fn read_ptr(&self, addr: usize) -> Option<usize> {
        self.read_u64(addr)
            .and_then(|v| usize::try_from(v).ok())
            .filter(|&p| mem::is_ptr(p))
    }
}

/// This process's memory: `ReadProcessMemory` reads ([`mem::read`], safe on the main loop and in an exception handler: no
/// `VirtualQuery`, no lock) and the `VirtualQuery`-checked [`mem::write_u8`].
pub(crate) struct RealMem;

impl GameMem for RealMem {
    fn read_into(&self, addr: usize, out: &mut [u8]) -> bool {
        mem::copy_out(addr, out)
    }
    fn write_u8(&self, addr: usize, value: u8) -> Result<(), &'static str> {
        mem::write_u8(addr, value)
    }
}

/// Test memory: a sparse byte map; unmapped bytes are unreadable, writes go anywhere mapped.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct FakeMem {
    bytes: std::cell::RefCell<BTreeMap<usize, u8>>,
}

#[cfg(test)]
impl FakeMem {
    pub(crate) fn put(&self, addr: usize, data: &[u8]) {
        let mut m = self.bytes.borrow_mut();
        for (i, b) in data.iter().enumerate() {
            m.insert(addr + i, *b);
        }
    }
    pub(crate) fn put_u64(&self, addr: usize, v: u64) {
        self.put(addr, &v.to_le_bytes());
    }
    pub(crate) fn put_u32(&self, addr: usize, v: u32) {
        self.put(addr, &v.to_le_bytes());
    }
}

#[cfg(test)]
impl GameMem for FakeMem {
    fn read_into(&self, addr: usize, out: &mut [u8]) -> bool {
        let m = self.bytes.borrow();
        for (i, o) in out.iter_mut().enumerate() {
            match m.get(&(addr + i)) {
                Some(b) => *o = *b,
                None => return false,
            }
        }
        true
    }
    fn write_u8(&self, addr: usize, value: u8) -> Result<(), &'static str> {
        let mut m = self.bytes.borrow_mut();
        match m.get_mut(&addr) {
            Some(b) => {
                *b = value;
                Ok(())
            }
            None => Err("unmapped"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_reads_little_endian_and_refuses_gaps() {
        let m = FakeMem::default();
        m.put_u64(0x1_0000, 0x2_0000);
        assert_eq!(m.read_u64(0x1_0000), Some(0x2_0000));
        assert_eq!(m.read_ptr(0x1_0000), Some(0x2_0000));
        assert_eq!(m.read_u8(0x1_0000), Some(0));
        assert_eq!(m.read_u64(0x1_0004), None, "half mapped");
        assert!(m.write_u8(0x9_0000, 1).is_err());
    }

    #[test]
    fn small_values_are_not_pointers() {
        let m = FakeMem::default();
        m.put_u64(0x1_0000, 0x10);
        assert_eq!(m.read_ptr(0x1_0000), None);
    }

    #[test]
    fn real_mem_reads_our_own_memory_and_fails_softly() {
        let v: u64 = 0x1122_3344_5566_7788;
        let r = RealMem;
        assert_eq!(r.read_u64(std::ptr::addr_of!(v) as usize), Some(v));
        assert_eq!(r.read_u64(8), None);
    }
}
