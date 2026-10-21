//! The game's event-flag manager (`CSEventFlagMan`): layout checks and flag lookup. R5 moved this out of the T-027 probes
//! (`probe2`, deleted) because `flags` (UI-2 messages, player build) and `flagwatch` (dev) use it.
//!
//! Layout: ER `cs/event_flag.rs` of fsrs@59fbd3b, offsets derived by hand; verified on NR 1.03.3 by UI-0 step 5 (structural
//! checks pass, flag 6001 "always ON" reads ON, 7500 goes ON at DAY I; MEMORY_MAP §13). Lookup: group = flag / divisor; the
//! group's descriptor sits in an MSVC `std::map` (red-black tree); mode 1 = block at `flag_blocks + holder_offset *
//! holder_size`, mode 2 = external block pointer; byte = (flag % divisor) / 8, bit = 7 - (flag % divisor) % 8. Read-only.

use super::mem::{is_ptr, RegionCache};

// ---- CSEventFlagMan (ER offsets, DERIVED BY HAND from fsrs@59fbd3b cs/event_flag.rs; HYPOTHESIS for NR 1.3.3.0) ----
/// `CSFD4VirtualMemoryFlag.event_flag_divisor` (u32). Struct start = `CSEventFlagMan` start.
const EF_DIVISOR: usize = 0x1C;
/// `event_flag_holder_size` (u32): bytes per flag block (ER: 125).
const EF_HOLDER_SIZE: usize = 0x20;
/// `event_flag_holder_count` (u32).
const EF_HOLDER_COUNT: usize = 0x24;
/// `flag_blocks` (pointer to the block array).
const EF_BLOCKS: usize = 0x28;
/// `flag_block_descriptors: DLMap` = `{allocator @0, head @8, size @0x10}` (comparator is a ZST).
const EF_MAP_HEAD: usize = 0x38;
const EF_MAP_SIZE: usize = 0x40;
/// `CSEventFlagMan.world_type` (u32), right after the 0x78-byte `CSFD4VirtualMemoryFlag`.
const EF_WORLD_TYPE: usize = 0x78;
/// Bytes read of the manager header.
pub(crate) const EF_HEADER_LEN: usize = 0x80;

// ---- MSVC std::map node (fsrs shared/stl rb_tree.rs `RbNode<Pair<u32, FlagBlockDescriptor>>`) ----
const NODE_LEFT: usize = 0x00;
const NODE_PARENT: usize = 0x08;
const NODE_RIGHT: usize = 0x10;
pub(crate) const NODE_IS_NIL: usize = 0x19;
/// `Pair.first` (the group id, u32).
const NODE_KEY: usize = 0x20;
/// `FlagBlockDescriptor.location_mode` (u32).
const NODE_MODE: usize = 0x28;
/// The 8-byte location union (holder offset u32, or an external block pointer).
const NODE_LOC: usize = 0x30;
pub(crate) const NODE_LEN: usize = 0x38;
/// Depth guard for the tree walk (a red-black tree of 10^6 nodes is under 40 deep).
const TREE_MAX_DEPTH: usize = 128;

fn le32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(off..off.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn le64(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        b.get(off..off.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// UI-0 step 5: every node of the descriptor map in key order (in-order walk of the MSVC tree), at most `limit` nodes.
pub(crate) fn all_groups(
    rc: &mut RegionCache,
    head: usize,
    limit: usize,
) -> Result<Vec<Node>, &'static str> {
    let head_node = read_node(rc, head).ok_or("map head unreadable")?;
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut cur = head_node.parent;
    let mut steps = 0usize;
    loop {
        while cur != head {
            let n = read_node(rc, cur).ok_or("tree node unreadable")?;
            if n.is_nil {
                break;
            }
            if stack.len() >= TREE_MAX_DEPTH {
                return Err("tree walk exceeded the depth guard");
            }
            stack.push(cur);
            cur = n.left;
        }
        let Some(top) = stack.pop() else { break };
        let n = read_node(rc, top).ok_or("tree node unreadable")?;
        out.push(n);
        steps += 1;
        if steps >= limit {
            break;
        }
        cur = n.right;
    }
    Ok(out)
}

/// `(group, byte, bit)` of a flag id, or `None` for a zero divisor (fsrs `EventFlag::{group,byte,bit}` with the
/// divisor taken from the manager instead of the hardcoded 1000).
pub(crate) fn flag_split(flag: u32, divisor: u32) -> Option<(u32, u32, u32)> {
    if divisor == 0 {
        return None;
    }
    let rem = flag % divisor;
    Some((flag / divisor, rem / 8, 7 - (rem % 8)))
}

/// Address of a flag block from its descriptor (fsrs `FlagBlockDescriptor::resolve`). `None` for an unknown
/// mode (fsrs: no block, flag reads as off) or an implausible pointer.
pub(crate) fn block_addr(mode: u32, loc: u64, blocks: usize, holder_size: u32) -> Option<usize> {
    match mode {
        1 => {
            if !is_ptr(blocks) {
                return None;
            }
            // `holder_offset` is the low u32 of the union; blocks are `holder_size` bytes each.
            let idx = (loc & 0xFFFF_FFFF) as usize;
            blocks.checked_add(idx.checked_mul(holder_size as usize)?)
        }
        2 => {
            let p = usize::try_from(loc).ok()?;
            is_ptr(p).then_some(p)
        }
        _ => None,
    }
}

/// The manager header, parsed. Readability flags are filled in by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EfLayout {
    pub divisor: u32,
    pub holder_size: u32,
    pub holder_count: u32,
    pub blocks: usize,
    pub head: usize,
    pub map_size: u64,
    pub world_type: u32,
    pub blocks_readable: bool,
    pub head_readable: bool,
    /// `is_nil` byte of the map's head node (the MSVC sentinel has it set to 1).
    pub head_is_nil: Option<u8>,
}

/// Parse the header bytes (at least [`EF_HEADER_LEN`]); readability fields start false/None.
pub(crate) fn parse_layout(b: &[u8]) -> Option<EfLayout> {
    Some(EfLayout {
        divisor: le32(b, EF_DIVISOR)?,
        holder_size: le32(b, EF_HOLDER_SIZE)?,
        holder_count: le32(b, EF_HOLDER_COUNT)?,
        blocks: usize::try_from(le64(b, EF_BLOCKS)?).ok()?,
        head: usize::try_from(le64(b, EF_MAP_HEAD)?).ok()?,
        map_size: le64(b, EF_MAP_SIZE)?,
        world_type: le32(b, EF_WORLD_TYPE)?,
        blocks_readable: false,
        head_readable: false,
        head_is_nil: None,
    })
}

/// One structural check of the layout hypothesis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

/// The structural checks. ER values: divisor 1000, holder size 125 (= ceil(1000 / 8)).
pub(crate) fn structural_checks(l: &EfLayout) -> Vec<Check> {
    let c = |name, ok, detail: String| Check { name, ok, detail };
    let min_holder = l.divisor.div_ceil(8);
    vec![
        c(
            "divisor",
            (100..=100_000).contains(&l.divisor),
            format!("{} (ER: 1000)", l.divisor),
        ),
        c(
            "holder_size",
            l.divisor != 0 && (min_holder..=4096).contains(&l.holder_size),
            format!(
                "{} (ER: 125; must be {min_holder}..=4096 for this divisor)",
                l.holder_size
            ),
        ),
        c(
            "holder_count",
            (1..=1_000_000).contains(&l.holder_count),
            format!("{}", l.holder_count),
        ),
        c(
            "flag_blocks pointer",
            is_ptr(l.blocks) && l.blocks_readable,
            format!("0x{:X} readable={}", l.blocks, l.blocks_readable),
        ),
        c(
            "descriptor map head",
            is_ptr(l.head) && l.head_readable && l.head_is_nil == Some(1),
            format!(
                "0x{:X} readable={} is_nil={:?}",
                l.head, l.head_readable, l.head_is_nil
            ),
        ),
        c(
            "descriptor map size",
            (1..=1_000_000).contains(&l.map_size),
            format!("{}", l.map_size),
        ),
    ]
}

/// `true` if every check passed.
pub(crate) fn checks_ok(checks: &[Check]) -> bool {
    checks.iter().all(|c| c.ok)
}

/// A tree node, parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    pub left: usize,
    pub parent: usize,
    pub right: usize,
    pub is_nil: bool,
    pub key: u32,
    pub mode: u32,
    pub loc: u64,
}

pub(crate) fn parse_node(b: &[u8]) -> Option<Node> {
    Some(Node {
        left: usize::try_from(le64(b, NODE_LEFT)?).ok()?,
        parent: usize::try_from(le64(b, NODE_PARENT)?).ok()?,
        right: usize::try_from(le64(b, NODE_RIGHT)?).ok()?,
        is_nil: *b.get(NODE_IS_NIL)? != 0,
        key: le32(b, NODE_KEY)?,
        mode: le32(b, NODE_MODE)?,
        loc: le64(b, NODE_LOC)?,
    })
}

fn read_node(rc: &mut RegionCache, addr: usize) -> Option<Node> {
    if !is_ptr(addr) {
        return None;
    }
    let b = rc.read_prefix(addr, NODE_LEN);
    if b.len() < NODE_LEN {
        return None;
    }
    parse_node(&b)
}

/// Why a lookup produced no value (tests: the tree walk and the block resolution together).
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlagState {
    On,
    Off,
    /// The flag's group has no block, or the descriptor mode is unknown: fsrs reads this as "off".
    NoBlock,
    /// A guarded read failed or a pointer was implausible; the reason is for the log.
    Unreadable(&'static str),
}

/// Binary search of the map for `group` (MSVC `std::map`: `head.parent` is the root, children that are absent
/// point at `head`, nodes have an `is_nil` byte). `Ok(None)` = group not in the map.
pub(crate) fn find_group(
    rc: &mut RegionCache,
    head: usize,
    group: u32,
) -> Result<Option<Node>, &'static str> {
    let head_node = read_node(rc, head).ok_or("map head unreadable")?;
    let mut cur = head_node.parent;
    for _ in 0..TREE_MAX_DEPTH {
        if cur == head {
            return Ok(None);
        }
        let n = read_node(rc, cur).ok_or("tree node unreadable")?;
        if n.is_nil {
            return Ok(None);
        }
        cur = match group.cmp(&n.key) {
            std::cmp::Ordering::Less => n.left,
            std::cmp::Ordering::Greater => n.right,
            std::cmp::Ordering::Equal => return Ok(Some(n)),
        };
    }
    Err("tree walk exceeded the depth guard")
}

#[cfg(test)]
/// Look one flag up with the ER algorithm. The caller has already passed the structural checks.
pub(crate) fn lookup_flag(rc: &mut RegionCache, l: &EfLayout, flag: u32) -> FlagState {
    let Some((group, byte, bit)) = flag_split(flag, l.divisor) else {
        return FlagState::Unreadable("divisor is zero");
    };
    if byte >= l.holder_size {
        return FlagState::Unreadable("byte index is outside the holder");
    }
    let d = match find_group(rc, l.head, group) {
        Err(e) => return FlagState::Unreadable(e),
        Ok(None) => return FlagState::NoBlock,
        Ok(Some(d)) => d,
    };
    let Some(block) = block_addr(d.mode, d.loc, l.blocks, l.holder_size) else {
        return FlagState::NoBlock;
    };
    match rc.read::<u8>(block + byte as usize) {
        Some(b) if (b >> bit) & 1 == 1 => FlagState::On,
        Some(_) => FlagState::Off,
        None => FlagState::Unreadable("flag byte unreadable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- a synthetic CSEventFlagMan in heap memory (addresses are real, so RegionCache works) ----

    struct Arena {
        buf: Vec<u8>,
        base: usize,
    }

    impl Arena {
        fn new() -> Self {
            let buf = vec![0u8; 0x2000];
            let p = buf.as_ptr() as usize;
            let base = (p + 15) & !15;
            Arena { buf, base }
        }
        fn addr(&self, off: usize) -> usize {
            self.base + off
        }
        fn idx(&self, off: usize) -> usize {
            self.base - self.buf.as_ptr() as usize + off
        }
        fn put(&mut self, off: usize, bytes: &[u8]) {
            let i = self.idx(off);
            self.buf[i..i + bytes.len()].copy_from_slice(bytes);
        }
        fn put64(&mut self, off: usize, v: u64) {
            self.put(off, &v.to_le_bytes());
        }
        fn put32(&mut self, off: usize, v: u32) {
            self.put(off, &v.to_le_bytes());
        }
        #[allow(clippy::too_many_arguments)]
        fn node(
            &mut self,
            off: usize,
            left: usize,
            parent: usize,
            right: usize,
            nil: bool,
            key: u32,
            mode: u32,
            loc: u64,
        ) {
            self.put64(off + NODE_LEFT, left as u64);
            self.put64(off + NODE_PARENT, parent as u64);
            self.put64(off + NODE_RIGHT, right as u64);
            self.put(off + NODE_IS_NIL, &[u8::from(nil)]);
            self.put32(off + NODE_KEY, key);
            self.put32(off + NODE_MODE, mode);
            self.put64(off + NODE_LOC, loc);
        }
    }

    const HEAD: usize = 0x100;
    const N3: usize = 0x200;
    const N1: usize = 0x240;
    const N7: usize = 0x280;
    const N5: usize = 0x2C0;
    const BLOCKS: usize = 0x400;
    const EXT: usize = 0x800;

    /// Groups 1, 3 (root), 5, 7. 1/3/7 use mode 1 (offsets 0/1/2), 5 uses mode 2 (external block).
    fn arena() -> (Arena, EfLayout) {
        let mut a = Arena::new();
        let head = a.addr(HEAD);
        let (n3, n1, n7, n5) = (a.addr(N3), a.addr(N1), a.addr(N7), a.addr(N5));
        a.node(HEAD, n1, n3, n7, true, 0, 0, 0);
        a.node(N3, n1, head, n7, false, 3, 1, 1);
        a.node(N1, head, n3, head, false, 1, 1, 0);
        a.node(N7, n5, n3, head, false, 7, 1, 2);
        a.node(N5, head, n7, head, false, 5, 2, a.addr(EXT) as u64);
        // Group 7, flag 7502: rem 502 -> byte 62, bit 7 - 6 = 1. Flag 7000: byte 0 bit 7. Flag 7001: byte 0 bit 6.
        let g7 = BLOCKS + 2 * 125;
        a.put(g7 + 62, &[0b0000_0010]);
        a.put(g7, &[0b1000_0000]);
        // Group 5, flag 5010: rem 10 -> byte 1, bit 7 - 2 = 5, in the external block.
        a.put(EXT + 1, &[0b0010_0000]);
        let l = EfLayout {
            divisor: 1000,
            holder_size: 125,
            holder_count: 4,
            blocks: a.addr(BLOCKS),
            head,
            map_size: 4,
            world_type: 0,
            blocks_readable: true,
            head_readable: true,
            head_is_nil: Some(1),
        };
        (a, l)
    }

    #[test]
    fn flag_arithmetic_matches_the_fsrs_algorithm() {
        assert_eq!(flag_split(7502, 1000), Some((7, 62, 1)));
        assert_eq!(flag_split(7000, 1000), Some((7, 0, 7)));
        assert_eq!(flag_split(7001, 1000), Some((7, 0, 6)));
        assert_eq!(flag_split(2200, 1000), Some((2, 25, 7)));
        assert_eq!(flag_split(2201, 1000), Some((2, 25, 6)));
        assert_eq!(flag_split(999, 1000), Some((0, 124, 0)));
        assert_eq!(flag_split(5, 0), None);
    }

    #[test]
    fn block_addresses_by_mode() {
        assert_eq!(block_addr(1, 2, 0x10000, 125), Some(0x10000 + 250));
        assert_eq!(
            block_addr(1, 0x1_0000_0002, 0x10000, 125),
            Some(0x10000 + 250)
        );
        assert_eq!(block_addr(1, 2, 0, 125), None); // null block array
        assert_eq!(
            block_addr(2, 0x7FF0_0000_1000, 0, 125),
            Some(0x7FF0_0000_1000)
        );
        assert_eq!(block_addr(2, 0, 0x10000, 125), None);
        assert_eq!(block_addr(2, 0x1003, 0x10000, 125), None); // unaligned
        assert_eq!(block_addr(0, 1, 0x10000, 125), None);
        assert_eq!(block_addr(3, 1, 0x10000, 125), None);
    }

    #[test]
    fn lookup_walks_the_tree_and_reads_bits() {
        let (_a, l) = arena();
        let mut rc = RegionCache::new();
        assert_eq!(lookup_flag(&mut rc, &l, 7502), FlagState::On);
        assert_eq!(lookup_flag(&mut rc, &l, 7000), FlagState::On);
        assert_eq!(lookup_flag(&mut rc, &l, 7001), FlagState::Off);
        assert_eq!(lookup_flag(&mut rc, &l, 7503), FlagState::Off);
        assert_eq!(lookup_flag(&mut rc, &l, 7501), FlagState::Off);
        // Mode 2 (external block), a group deeper in the tree.
        assert_eq!(lookup_flag(&mut rc, &l, 5010), FlagState::On);
        assert_eq!(lookup_flag(&mut rc, &l, 5011), FlagState::Off);
        // Groups 1 and 3 exist but are empty; groups 2 and 9 are absent.
        assert_eq!(lookup_flag(&mut rc, &l, 1000), FlagState::Off);
        assert_eq!(lookup_flag(&mut rc, &l, 3999), FlagState::Off);
        assert_eq!(lookup_flag(&mut rc, &l, 2200), FlagState::NoBlock);
        assert_eq!(lookup_flag(&mut rc, &l, 9000), FlagState::NoBlock);
        // Byte index beyond the holder.
        let small = EfLayout {
            holder_size: 10,
            ..l.clone()
        };
        assert!(matches!(
            lookup_flag(&mut rc, &small, 7502),
            FlagState::Unreadable(_)
        ));
        // Zero divisor.
        let zero = EfLayout {
            divisor: 0,
            ..l.clone()
        };
        assert!(matches!(
            lookup_flag(&mut rc, &zero, 7502),
            FlagState::Unreadable(_)
        ));
    }

    #[test]
    fn garbage_trees_do_not_hang_or_fault() {
        let (mut a, l) = arena();
        // A cycle: node 7 points left at itself.
        let n7 = a.addr(N7);
        a.node(N7, n7, a.addr(N3), a.addr(HEAD), false, 7, 1, 2);
        let mut rc = RegionCache::new();
        // Group 6 < 7 walks left forever -> depth guard.
        assert!(matches!(
            lookup_flag(&mut rc, &l, 6000),
            FlagState::Unreadable(_)
        ));
        // Wild head pointer.
        let wild = EfLayout {
            head: 0x1234_5678,
            ..l.clone()
        };
        assert!(matches!(
            lookup_flag(&mut rc, &wild, 7502),
            FlagState::Unreadable(_)
        ));
    }

    #[test]
    fn structural_checks_pass_and_fail_visibly() {
        let (_a, l) = arena();
        let checks = structural_checks(&l);
        assert_eq!(checks.len(), 6);
        assert!(checks_ok(&checks), "{checks:?}");
        let bad = EfLayout {
            divisor: 0,
            holder_size: 0,
            blocks: 0,
            head_is_nil: Some(0),
            map_size: 0,
            ..l.clone()
        };
        let failed: Vec<&str> = structural_checks(&bad)
            .iter()
            .filter(|c| !c.ok)
            .map(|c| c.name)
            .collect();
        assert_eq!(
            failed,
            [
                "divisor",
                "holder_size",
                "flag_blocks pointer",
                "descriptor map head",
                "descriptor map size"
            ]
        );
        let small_holder = EfLayout {
            holder_size: 100, // < ceil(1000 / 8)
            ..l
        };
        assert!(!checks_ok(&structural_checks(&small_holder)));
    }

    #[test]
    fn layout_parses_from_the_header_bytes() {
        let mut b = vec![0u8; EF_HEADER_LEN];
        b[EF_DIVISOR..EF_DIVISOR + 4].copy_from_slice(&1000u32.to_le_bytes());
        b[EF_HOLDER_SIZE..EF_HOLDER_SIZE + 4].copy_from_slice(&125u32.to_le_bytes());
        b[EF_HOLDER_COUNT..EF_HOLDER_COUNT + 4].copy_from_slice(&7u32.to_le_bytes());
        b[EF_BLOCKS..EF_BLOCKS + 8].copy_from_slice(&0x1111_0000u64.to_le_bytes());
        b[EF_MAP_HEAD..EF_MAP_HEAD + 8].copy_from_slice(&0x2222_0000u64.to_le_bytes());
        b[EF_MAP_SIZE..EF_MAP_SIZE + 8].copy_from_slice(&9u64.to_le_bytes());
        b[EF_WORLD_TYPE..EF_WORLD_TYPE + 4].copy_from_slice(&3u32.to_le_bytes());
        let l = parse_layout(&b).expect("parse");
        assert_eq!(
            (
                l.divisor,
                l.holder_size,
                l.holder_count,
                l.map_size,
                l.world_type
            ),
            (1000, 125, 7, 9, 3)
        );
        assert_eq!((l.blocks, l.head), (0x1111_0000, 0x2222_0000));
        assert!(parse_layout(&b[..0x70]).is_none());
    }
}
