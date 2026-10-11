//! T-028 (read-only part): where is each player? Position, block id, ground contact and animation id candidates for
//! the local PlayerIns and every other PlayerIns of the ChrSet (the remote copies), sampled so two clients' logs can
//! be compared line by line (same `id=` and `ts=` fields).
//!
//! EVERYTHING here is an ER-layout HYPOTHESIS for NR 1.3.3.0 and every logged value is labelled `UNVERIFIED`.
//! Strictly read-only: guarded reads through [`RegionCache`], no writes, no calls into game functions.
//!
//! Sources (ER, fsrs = `vswarte/fromsoftware-rs` at 59fbd3b, see S6-team-spawns.md sections 2 and 5):
//! * `ChrIns.modules` at ER +0x190 (NR +8 guess: +0x198) -> `ChrInsModuleContainer`; `physics` is the 14th pointer,
//!   +0x68 (neighbour +0x70 also tried). [SOURCE: fsrs chr_ins.rs#L211, chr_ins/module.rs; PT pointers.rs#L527]
//! * `CSChrPhysicsModule` (fsrs chr_ins/module/physics.rs, offsets derived by hand from the struct, fetched
//!   2026-10-10): `owner` (ChrIns*) +0x08, `position` (x,y,z,w f32) +0x70, `standing_on_solid_ground` +0x92,
//!   `touching_solid_ground` +0x93, `is_falling` +0x1D0, `is_touching_ground` +0x1D1. The `owner` pointer equal to the
//!   PlayerIns is the structural check of a candidate. NR may shift the position/flags: +8 neighbours are read too.
//! * `PlayerIns.block_position` (x,y,z,yaw f32) ER +0x6C0 and `current_block_id` (u32) ER +0x6D0; NR +8 guess
//!   0x6C8/0x6D8. [SOURCE: PT pointers.rs#L350-L368; S6 section 2]
//! * Animation id: container +0x18 (time act module) -> +0x20 (u32). [SOURCE: PT pointers.rs#L511-L513, ER]; +0x28
//!   is the NR +8 neighbour.

use std::collections::{HashMap, HashSet};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::discover::{hex_lines, is_ptr};
use super::mem::{va_rva, RegionCache};

const TAG: &str = "T-028 POS UNVERIFIED";

/// ChrIns -> module container pointer: ER, NR +8 guess.
pub(crate) const MODULES_PTR_OFFS: [usize; 2] = [0x190, 0x198];
/// Container -> physics module pointer: ER, +8 neighbour.
pub(crate) const PHYS_IN_MODULES: [usize; 2] = [0x68, 0x70];
/// Container -> time act module pointer (ER).
const TIMEACT_IN_MODULES: usize = 0x18;
/// Time act module -> current animation id (u32): ER, +8 neighbour.
pub(crate) const ANIM_OFFS: [usize; 2] = [0x20, 0x28];
/// Physics module -> `owner` (ChrIns*).
const PHYS_OWNER: usize = 0x08;
/// Physics module -> position (x, y, z f32): ER, +8 neighbour.
pub(crate) const POS_OFFS: [usize; 2] = [0x70, 0x78];
/// Physics module ground bytes: ER (0x92, 0x93, 0x1D0, 0x1D1) and the same +8.
pub(crate) const GROUND_OFFS: [usize; 8] = [0x92, 0x93, 0x1D0, 0x1D1, 0x9A, 0x9B, 0x1D8, 0x1D9];
/// PlayerIns (block_position, current_block_id): ER, NR +8 guess.
pub(crate) const BLOCK_VARIANTS: [(usize, usize); 2] = [(0x6C0, 0x6D0), (0x6C8, 0x6D8)];
/// ChrIns per-player id, identical on all clients [VERIFIED: FS-0/FS-1 2026-10-10, TECHNICAL_NOTES section 4].
const CHR_PLAYER_ID: usize = 0x78;
/// Bytes of a physics module hex-dumped once.
const PHYS_DUMP_LEN: usize = 0x100;
/// At most this many physics-module dumps per WorldChrMan instance.
const MAX_DUMPS: usize = 8;

/// Sampling schedule (card T-028): every 100 ms for the first 60 s after a new WorldChrMan appears, then 1 s.
pub(crate) const FAST_WINDOW_MS: u64 = 60_000;
pub(crate) const FAST_INTERVAL_MS: u64 = 100;
pub(crate) const SLOW_INTERVAL_MS: u64 = 1000;

/// Sample period for a WorldChrMan instance of age `age_ms`.
pub(crate) fn pos_interval_ms(age_ms: u64) -> u64 {
    if age_ms < FAST_WINDOW_MS {
        FAST_INTERVAL_MS
    } else {
        SLOW_INTERVAL_MS
    }
}

/// Is a sample due? `since_last_ms` is `None` before the first sample.
pub(crate) fn pos_due(age_ms: u64, since_last_ms: Option<u64>) -> bool {
    since_last_ms.is_none_or(|s| s >= pos_interval_ms(age_ms))
}

/// Finite, within a believable world extent, and not the all-zero default.
pub(crate) fn pos_plausible(p: (f32, f32, f32)) -> bool {
    let ok = |v: f32| v.is_finite() && v.abs() <= 100_000.0;
    ok(p.0) && ok(p.1) && ok(p.2) && !(p.0 == 0.0 && p.1 == 0.0 && p.2 == 0.0)
}

/// BlockId area byte (bits 24-31) in 1..=99 (ER overworld is 50-88; `-1` = none is not a location).
pub(crate) fn block_id_plausible(id: u32) -> bool {
    (1..=99).contains(&(id >> 24))
}

fn f3(b: &[u8]) -> Option<(f32, f32, f32)> {
    let f =
        |o: usize| -> Option<f32> { Some(f32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?)) };
    Some((f(0)?, f(4)?, f(8)?))
}

fn f4(b: &[u8]) -> Option<[f32; 4]> {
    let f =
        |o: usize| -> Option<f32> { Some(f32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?)) };
    Some([f(0)?, f(4)?, f(8)?, f(12)?])
}

/// One chain candidate: `ChrIns+modules_off -> +phys_off -> physics module`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PhysCand {
    pub modules_off: usize,
    pub phys_off: usize,
    pub modules: Option<usize>,
    pub phys: Option<usize>,
    pub vtable_in_image: bool,
    /// `physics+0x08` equals the PlayerIns (the structural check).
    pub owner_ok: bool,
    /// Position at each of [`POS_OFFS`].
    pub pos: [Option<(f32, f32, f32)>; 2],
}

impl PhysCand {
    pub(crate) fn good(&self) -> bool {
        self.vtable_in_image && self.owner_ok
    }
}

fn in_image(addr: usize, base: usize, size: usize) -> bool {
    addr >= base && addr - base < size
}

/// Read every chain candidate of `player` (a PlayerIns). All reads are guarded.
pub(crate) fn read_phys_cands(
    rc: &mut RegionCache,
    player: usize,
    base: usize,
    size: usize,
) -> Vec<PhysCand> {
    let mut out = Vec::new();
    for &mo in &MODULES_PTR_OFFS {
        let modules = rc.read::<usize>(player + mo).filter(|&p| is_ptr(p));
        for &po in &PHYS_IN_MODULES {
            let phys = modules
                .and_then(|m| rc.read::<usize>(m + po))
                .filter(|&p| is_ptr(p));
            let vtable = phys.and_then(|p| rc.read::<usize>(p));
            let owner = phys.and_then(|p| rc.read::<usize>(p + PHYS_OWNER));
            let mut pos = [None, None];
            if let Some(p) = phys {
                for (i, &o) in POS_OFFS.iter().enumerate() {
                    pos[i] = f3(&rc.read_prefix(p + o, 12));
                }
            }
            out.push(PhysCand {
                modules_off: mo,
                phys_off: po,
                modules,
                phys,
                vtable_in_image: vtable.is_some_and(|v| in_image(v, base, size)),
                owner_ok: owner == Some(player),
                pos,
            });
        }
    }
    out
}

/// First good candidate that has a plausible position: `(candidate index, position offset index)`.
pub(crate) fn select(c: &[PhysCand]) -> Option<(usize, usize)> {
    c.iter().enumerate().find_map(|(i, c)| {
        if !c.good() {
            return None;
        }
        c.pos
            .iter()
            .position(|p| p.is_some_and(pos_plausible))
            .map(|k| (i, k))
    })
}

/// One sample of one player.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Sample {
    /// `(modules_off, phys_off, pos_off)` of the selected chain.
    pub via: Option<(usize, usize, usize)>,
    pub pos: Option<(f32, f32, f32)>,
    /// `(offset, byte)` of [`GROUND_OFFS`].
    pub ground: Vec<(usize, Option<u8>)>,
    /// Animation id at [`ANIM_OFFS`].
    pub anim: [Option<u32>; 2],
    /// Per [`BLOCK_VARIANTS`]: (block_position, block id).
    pub blocks: [(Option<[f32; 4]>, Option<u32>); 2],
}

impl Sample {
    /// Everything but the time, at the precision of the log (1 cm): two samples with equal bodies are "unchanged".
    pub(crate) fn body(&self) -> String {
        let h = |v: Option<u32>| v.map_or("?".to_string(), |v| format!("0x{v:08X}"));
        let bp = |b: &Option<[f32; 4]>| match b {
            Some(b) => format!("({:.2},{:.2},{:.2},{:.2})", b[0], b[1], b[2], b[3]),
            None => "?".to_string(),
        };
        let pos = match self.pos {
            Some(p) => format!("({:.2},{:.2},{:.2})", p.0, p.1, p.2),
            None => "?".to_string(),
        };
        let via = match self.via {
            Some((m, p, o)) => format!("m{m:X}/p{p:X}/o{o:X}"),
            None => "none".to_string(),
        };
        let gnd: Vec<String> = self
            .ground
            .iter()
            .map(|(o, b)| format!("{o:X}:{}", b.map_or("?".to_string(), |b| b.to_string())))
            .collect();
        let anim = |v: Option<u32>| v.map_or("?".to_string(), |v| v.to_string());
        format!(
            "pos={pos} via={via} bid[{:X}]={} bid[{:X}]={} bp[{:X}]={} bp[{:X}]={} gnd=[{}] anim[{:X}]={} anim[{:X}]={}",
            BLOCK_VARIANTS[0].1,
            h(self.blocks[0].1),
            BLOCK_VARIANTS[1].1,
            h(self.blocks[1].1),
            BLOCK_VARIANTS[0].0,
            bp(&self.blocks[0].0),
            BLOCK_VARIANTS[1].0,
            bp(&self.blocks[1].0),
            gnd.join(" "),
            ANIM_OFFS[0],
            anim(self.anim[0]),
            ANIM_OFFS[1],
            anim(self.anim[1]),
        )
    }

    /// Short overlay text.
    pub(crate) fn overlay(&self) -> String {
        let pos = match self.pos {
            Some(p) => format!("({:.1}, {:.1}, {:.1})", p.0, p.1, p.2),
            None => "no plausible chain".to_string(),
        };
        let bid = |v: Option<u32>| match v {
            Some(v) if block_id_plausible(v) => format!("0x{v:08X}"),
            Some(_) => "-".to_string(),
            None => "?".to_string(),
        };
        let g = |o: usize| {
            self.ground
                .iter()
                .find(|(x, _)| *x == o)
                .and_then(|(_, b)| *b)
                .map_or("?".to_string(), |b| b.to_string())
        };
        format!(
            "pos {pos} bid {} / {} gnd {}{}{}{} anim {}/{}",
            bid(self.blocks[0].1),
            bid(self.blocks[1].1),
            g(0x92),
            g(0x93),
            g(0x1D0),
            g(0x1D1),
            self.anim[0].map_or("?".to_string(), |v| v.to_string()),
            self.anim[1].map_or("?".to_string(), |v| v.to_string()),
        )
    }
}

/// Take one sample of `player` using the candidates already read.
pub(crate) fn read_sample(rc: &mut RegionCache, player: usize, cands: &[PhysCand]) -> Sample {
    let mut s = Sample::default();
    for (i, &(bp, id)) in BLOCK_VARIANTS.iter().enumerate() {
        s.blocks[i] = (
            f4(&rc.read_prefix(player + bp, 16)),
            rc.read::<u32>(player + id),
        );
    }
    let Some((ci, ki)) = select(cands) else {
        return s;
    };
    let c = &cands[ci];
    s.via = Some((c.modules_off, c.phys_off, POS_OFFS[ki]));
    let Some(phys) = c.phys else { return s };
    // Fresh read of the position (the candidate list may be a few samples old).
    s.pos = f3(&rc.read_prefix(phys + POS_OFFS[ki], 12));
    s.ground = GROUND_OFFS
        .iter()
        .map(|&o| (o, rc.read::<u8>(phys + o)))
        .collect();
    if let Some(m) = c.modules {
        let timeact = rc
            .read::<usize>(m + TIMEACT_IN_MODULES)
            .filter(|&p| is_ptr(p));
        if let Some(t) = timeact {
            for (i, &o) in ANIM_OFFS.iter().enumerate() {
                s.anim[i] = rc.read::<u32>(t + o);
            }
        }
    }
    s
}

/// A player to sample: ChrSet index, address, local or remote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub index: usize,
    pub addr: usize,
    /// Address of this player's 16-byte ChrSet entry (0 = unknown); T-031 re-reads it fresh.
    pub entry_addr: usize,
    pub local: bool,
}

impl Target {
    pub(crate) fn label(&self) -> String {
        if self.local {
            format!("LOCAL[{}]", self.index)
        } else {
            format!("REMOTE[{}]", self.index)
        }
    }
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Orchestrator: schedule, change detection, logging, overlay lines.
pub(crate) struct PosProbe {
    base: usize,
    size: usize,
    wcm: usize,
    epoch: Option<Instant>,
    last_sample: Option<Instant>,
    last_body: HashMap<usize, String>,
    tabled: HashSet<usize>,
    dumped: HashSet<usize>,
    view: Vec<String>,
}

impl PosProbe {
    pub(crate) fn new(base: usize, size: usize) -> Self {
        PosProbe {
            base,
            size,
            wcm: 0,
            epoch: None,
            last_sample: None,
            last_body: HashMap::new(),
            tabled: HashSet::new(),
            dumped: HashSet::new(),
            view: Vec::new(),
        }
    }

    pub(crate) fn lines(&self) -> &[String] {
        &self.view
    }

    /// One pass. Cheap when no sample is due. `wcm` = WorldChrMan instance (0 = not live).
    pub(crate) fn tick(&mut self, wcm: usize, targets: &[Target]) {
        if wcm == 0 {
            if self.wcm != 0 {
                tracing::info!("{TAG}: WorldChrMan gone; position sampling paused");
            }
            self.wcm = 0;
            self.epoch = None;
            self.last_sample = None;
            self.view = vec!["pos: WorldChrMan not live".to_string()];
            return;
        }
        if wcm != self.wcm {
            self.wcm = wcm;
            self.epoch = Some(Instant::now());
            self.last_sample = None;
            self.last_body.clear();
            self.tabled.clear();
            self.dumped.clear();
            tracing::info!(
                "{TAG}: new WorldChrMan {}: sampling every {} ms for {} s, then every {} ms; lines only when something changed",
                va_rva(wcm, self.base, self.size),
                FAST_INTERVAL_MS,
                FAST_WINDOW_MS / 1000,
                SLOW_INTERVAL_MS
            );
        }
        let Some(epoch) = self.epoch else { return };
        let age_ms = u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
        let since = self
            .last_sample
            .map(|t| u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX));
        if !pos_due(age_ms, since) {
            return;
        }
        self.last_sample = Some(Instant::now());
        if targets.is_empty() {
            self.view = vec!["pos: no PlayerIns in the ChrSet yet".to_string()];
            return;
        }
        let mut rc = RegionCache::new();
        let mut view = Vec::new();
        for t in targets {
            let cands = read_phys_cands(&mut rc, t.addr, self.base, self.size);
            if self.tabled.insert(t.addr) {
                self.log_table(&mut rc, t, &cands);
            }
            let s = read_sample(&mut rc, t.addr, &cands);
            let body = s.body();
            if self.last_body.get(&t.addr) != Some(&body) {
                let id = rc.read::<u64>(t.addr + CHR_PLAYER_ID);
                tracing::info!(
                    "{TAG} {} id={} t=+{age_ms}ms ts={} {body}",
                    t.label(),
                    id.map_or("?".to_string(), |v| format!("0x{v:X}")),
                    unix_ms()
                );
                self.last_body.insert(t.addr, body);
            }
            view.push(format!("{}: {}", t.label(), s.overlay()));
        }
        self.view = view;
    }

    /// The full candidate table of one player, once per WorldChrMan instance.
    fn log_table(&mut self, rc: &mut RegionCache, t: &Target, cands: &[PhysCand]) {
        tracing::info!(
            "{TAG}: {} PlayerIns {} chain candidates (ChrIns+modules -> +phys -> physics module; ER 0x190/0x68/0x70, NR +8 guesses; good = vtable in exe AND physics+8 == PlayerIns):",
            t.label(),
            va_rva(t.addr, self.base, self.size)
        );
        for c in cands {
            let at = |p: Option<usize>| {
                p.map_or("unreadable".to_string(), |p| {
                    va_rva(p, self.base, self.size)
                })
            };
            let pos = |p: Option<(f32, f32, f32)>| {
                p.map_or("?".to_string(), |p| {
                    format!(
                        "({:.2},{:.2},{:.2}){}",
                        p.0,
                        p.1,
                        p.2,
                        if pos_plausible(p) {
                            ""
                        } else {
                            " [implausible]"
                        }
                    )
                })
            };
            tracing::info!(
                "{TAG}:   modules@+0x{:X}={} phys@+0x{:X}={} vtable_in_exe={} owner==PlayerIns={} => {} ; pos@+0x{:X}={} pos@+0x{:X}={}",
                c.modules_off,
                at(c.modules),
                c.phys_off,
                at(c.phys),
                c.vtable_in_image,
                c.owner_ok,
                if c.good() { "GOOD" } else { "bad" },
                POS_OFFS[0],
                pos(c.pos[0]),
                POS_OFFS[1],
                pos(c.pos[1]),
            );
        }
        match select(cands) {
            Some((i, k)) => tracing::info!(
                "{TAG}:   selected: modules@+0x{:X} phys@+0x{:X} position@+0x{:X}",
                cands[i].modules_off,
                cands[i].phys_off,
                POS_OFFS[k]
            ),
            None => tracing::warn!(
                "{TAG}:   NO plausible chain for this player: the ER layout does not hold here (position stays unknown)"
            ),
        }
        for c in cands.iter().filter(|c| c.good()) {
            if let Some(p) = c.phys {
                if self.dumped.len() < MAX_DUMPS && self.dumped.insert(p) {
                    tracing::info!(
                        "{TAG}:   physics module {} first 0x{PHYS_DUMP_LEN:X} bytes (once):",
                        va_rva(p, self.base, self.size)
                    );
                    for l in hex_lines(&rc.read_prefix(p, PHYS_DUMP_LEN)) {
                        tracing::info!("{TAG}:     {l}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Arena {
        buf: Vec<u8>,
        base: usize,
    }

    impl Arena {
        fn new() -> Self {
            let buf = vec![0u8; 0x4000];
            let base = (buf.as_ptr() as usize + 15) & !15;
            Arena { buf, base }
        }
        fn addr(&self, off: usize) -> usize {
            self.base + off
        }
        fn put(&mut self, off: usize, bytes: &[u8]) {
            let i = self.base - self.buf.as_ptr() as usize + off;
            self.buf[i..i + bytes.len()].copy_from_slice(bytes);
        }
        fn put64(&mut self, off: usize, v: u64) {
            self.put(off, &v.to_le_bytes());
        }
        fn put32(&mut self, off: usize, v: u32) {
            self.put(off, &v.to_le_bytes());
        }
        fn putf(&mut self, off: usize, v: f32) {
            self.put(off, &v.to_le_bytes());
        }
    }

    const IMG: usize = 0x7FF6_0000_0000;
    const IMG_SIZE: usize = 0x800_0000;
    const PLAYER: usize = 0x0;
    const MODS: usize = 0x1000;
    const PHYS: usize = 0x1400;
    const TIMEACT: usize = 0x2000;

    /// A believable ER-layout chain: player+0x198 -> modules, modules+0x68 -> physics, physics+0x70 = position.
    fn world() -> Arena {
        let mut a = Arena::new();
        a.put64(PLAYER + 0x198, a.addr(MODS) as u64);
        a.put64(MODS + 0x68, a.addr(PHYS) as u64);
        a.put64(MODS + 0x18, a.addr(TIMEACT) as u64);
        a.put64(PHYS, (IMG + 0x1000) as u64);
        a.put64(PHYS + 8, a.addr(PLAYER) as u64);
        a.putf(PHYS + 0x70, 100.5);
        a.putf(PHYS + 0x74, 30.25);
        a.putf(PHYS + 0x78, -200.0);
        a.put(PHYS + 0x92, &[1]);
        a.put(PHYS + 0x1D1, &[1]);
        a.put32(TIMEACT + 0x20, 20_000);
        a.put32(PLAYER + 0x6D8, 0x3C2A_2400);
        a.putf(PLAYER + 0x6C8, 1.0);
        a.putf(PLAYER + 0x6CC, 2.0);
        a.putf(PLAYER + 0x6D0, 3.0);
        a.putf(PLAYER + 0x6D4, 0.5);
        a
    }

    #[test]
    fn schedule_is_fast_for_a_minute_then_slow() {
        assert_eq!(pos_interval_ms(0), 100);
        assert_eq!(pos_interval_ms(59_999), 100);
        assert_eq!(pos_interval_ms(60_000), 1000);
        assert!(pos_due(0, None));
        assert!(!pos_due(1000, Some(99)));
        assert!(pos_due(1000, Some(100)));
        assert!(!pos_due(61_000, Some(999)));
        assert!(pos_due(61_000, Some(1000)));
    }

    #[test]
    fn plausibility_filters() {
        assert!(pos_plausible((100.5, 30.25, -200.0)));
        assert!(!pos_plausible((0.0, 0.0, 0.0)));
        assert!(!pos_plausible((f32::NAN, 1.0, 1.0)));
        assert!(!pos_plausible((f32::INFINITY, 1.0, 1.0)));
        assert!(!pos_plausible((1.0e9, 1.0, 1.0)));
        assert!(block_id_plausible(0x3C2A_2400));
        assert!(!block_id_plausible(0xFFFF_FFFF));
        assert!(!block_id_plausible(0));
        assert!(!block_id_plausible(0x7D00_0000)); // area 125
    }

    #[test]
    fn the_chain_is_found_and_sampled() {
        let a = world();
        let mut rc = RegionCache::new();
        let c = read_phys_cands(&mut rc, a.addr(PLAYER), IMG, IMG_SIZE);
        assert_eq!(c.len(), 4);
        // Only modules@0x198 / phys@0x68 is good.
        let good: Vec<(usize, usize)> = c
            .iter()
            .filter(|c| c.good())
            .map(|c| (c.modules_off, c.phys_off))
            .collect();
        assert_eq!(good, [(0x198, 0x68)]);
        let (i, k) = select(&c).expect("selected");
        assert_eq!((c[i].modules_off, POS_OFFS[k]), (0x198, 0x70));
        let s = read_sample(&mut rc, a.addr(PLAYER), &c);
        assert_eq!(s.pos, Some((100.5, 30.25, -200.0)));
        assert_eq!(s.via, Some((0x198, 0x68, 0x70)));
        assert_eq!(s.anim, [Some(20_000), Some(0)]);
        assert_eq!(s.blocks[1].1, Some(0x3C2A_2400));
        assert_eq!(s.blocks[1].0, Some([1.0, 2.0, 3.0, 0.5]));
        assert!(s.ground.contains(&(0x92, Some(1))));
        assert!(s.ground.contains(&(0x93, Some(0))));
        assert!(s.body().contains("pos=(100.50,30.25,-200.00)"));
        assert!(s.overlay().contains("100.5"));
    }

    #[test]
    fn wrong_layouts_select_nothing() {
        let mut a = world();
        // Owner does not point back at the player: not good.
        a.put64(PHYS + 8, 0x1234_5678);
        let mut rc = RegionCache::new();
        let c = read_phys_cands(&mut rc, a.addr(PLAYER), IMG, IMG_SIZE);
        assert!(select(&c).is_none());
        let s = read_sample(&mut rc, a.addr(PLAYER), &c);
        assert_eq!(s.pos, None);
        assert_eq!(s.via, None);
        // Block candidates are still read.
        assert_eq!(s.blocks[1].1, Some(0x3C2A_2400));
        // Vtable outside the exe: not good either.
        let mut b = world();
        b.put64(PHYS, 0x42);
        let c = read_phys_cands(&mut rc, b.addr(PLAYER), IMG, IMG_SIZE);
        assert!(select(&c).is_none());
        // Unreadable player: no panic, nothing selected.
        let c = read_phys_cands(&mut rc, 0x10, IMG, IMG_SIZE);
        assert_eq!(c.len(), 4);
        assert!(select(&c).is_none());
    }

    #[test]
    fn shifted_position_offset_is_found() {
        let mut a = world();
        a.putf(PHYS + 0x70, f32::NAN);
        a.putf(PHYS + 0x74, 0.0);
        a.putf(PHYS + 0x78, 12.0);
        a.putf(PHYS + 0x7C, 13.0);
        a.putf(PHYS + 0x80, 14.0);
        let mut rc = RegionCache::new();
        let c = read_phys_cands(&mut rc, a.addr(PLAYER), IMG, IMG_SIZE);
        let (_, k) = select(&c).expect("selected");
        assert_eq!(POS_OFFS[k], 0x78);
    }

    #[test]
    fn change_detection_works_at_one_centimetre() {
        let a = world();
        let mut rc = RegionCache::new();
        let c = read_phys_cands(&mut rc, a.addr(PLAYER), IMG, IMG_SIZE);
        let s1 = read_sample(&mut rc, a.addr(PLAYER), &c);
        let mut s2 = s1.clone();
        // Float noise below the logged precision is not a change.
        s2.pos = Some((100.500_01, 30.25, -200.0));
        assert_eq!(s1.body(), s2.body());
        s2.pos = Some((100.52, 30.25, -200.0));
        assert_ne!(s1.body(), s2.body());
        let mut s3 = s1.clone();
        s3.anim[0] = Some(20_001);
        assert_ne!(s1.body(), s3.body());
        let mut s4 = s1.clone();
        s4.ground[0].1 = Some(0);
        assert_ne!(s1.body(), s4.body());
    }

    #[test]
    fn probe_survives_garbage_players_and_resets_on_a_new_wcm() {
        let mut p = PosProbe::new(IMG, IMG_SIZE);
        p.tick(0, &[]);
        assert!(p.lines()[0].contains("not live"));
        p.tick(0x1000_0000, &[]);
        assert!(p.lines()[0].contains("no PlayerIns"));
        let t = [
            Target {
                index: 0,
                addr: 0x20,
                entry_addr: 0,
                local: true,
            },
            Target {
                index: 1,
                addr: 0x30,
                entry_addr: 0,
                local: false,
            },
        ];
        // The first call after the (empty) one is gated by the 100 ms schedule; force it.
        p.last_sample = None;
        p.tick(0x1000_0000, &t);
        assert_eq!(p.lines().len(), 2);
        assert!(p.lines()[0].starts_with("LOCAL[0]"));
        assert!(p.lines()[1].starts_with("REMOTE[1]"));
        assert!(p.lines()[0].contains("no plausible chain"));
        // Not due again right away.
        let before = p.lines().to_vec();
        p.tick(0x1000_0000, &[]);
        assert_eq!(p.lines(), before.as_slice());
    }

    #[test]
    fn a_real_chain_is_logged_once_then_only_on_change() {
        let a = world();
        let mut p = PosProbe::new(IMG, IMG_SIZE);
        let t = [Target {
            index: 0,
            addr: a.addr(PLAYER),
            entry_addr: 0,
            local: true,
        }];
        p.tick(0x1000_0000, &t);
        assert!(p.lines()[0].contains("100.5"), "{:?}", p.lines());
        let body = p.last_body.get(&a.addr(PLAYER)).cloned();
        assert!(body.is_some());
        p.last_sample = None;
        p.tick(0x1000_0000, &t);
        assert_eq!(p.last_body.get(&a.addr(PLAYER)).cloned(), body);
        // A new WorldChrMan forgets everything.
        p.last_sample = None;
        p.tick(0x2000_0000, &t);
        assert!(p.tabled.contains(&a.addr(PLAYER)));
    }
}
