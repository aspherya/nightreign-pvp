//! T-032 (c): the snap-back test. Dev mode only, LOCAL player only.
//!
//! FS-0 run 6 (NR 1.3.3.0) found several f32 triples that move smoothly with the player but could not say which one is
//! authoritative. This module lets the HUMAN find out: F9 selects a position candidate, F7 saves the LOCAL player's current
//! 12 bytes (3 x f32) of EVERY candidate, F6 writes the saved 12 bytes into the SELECTED candidate only, and the caller logs
//! every candidate again at +100 ms, +500 ms and +2 s to show whether the game snapped the value back and which other
//! candidates followed. Everything here is a HYPOTHESIS test; nothing is verified until the HUMAN reports.
//!
//! Candidate syntax (config `pos_candidates`): `PlayerIns+0x6C0` (offset inside the PlayerIns), `[PlayerIns+0x60]+0x1EC`
//! (offset inside the object the pointer at `PlayerIns+0x60` points to) or `Physics+0x70` (offset inside the player's
//! `CS::CSChrPhysicsModule`, found by its vtable in the module list, see [`MODULES_OFF`]). Offsets are hex with `0x` or decimal.
//!
//! Safety gates of [`PosTest::write`] (all must pass): the caller re-resolved the LOCAL PlayerIns (ChrSet entry[0] rule, vtable
//! check) right before; the values were saved for that same PlayerIns; the saved triple is finite with every |component| below
//! [`MAX_ABS`]; for a pointee candidate the pointer is re-read and the pointee's vtable must equal the one seen at save time
//! (or, if it had none, the pointer value must be unchanged); the store goes through `mem::write_bytes` (VirtualQuery
//! writable-page check). The logic is generic over [`PosMem`] so it is tested on fake memory.

use std::fmt::Write as _;

use super::discover::is_ptr;
use super::mem;

/// Saved value must be finite with every |component| below this.
pub(crate) const MAX_ABS: f32 = 100_000.0;
/// Follow-up logs after a write, in milliseconds.
pub(crate) const FOLLOW_MS: [u64; 3] = [100, 500, 2000];
/// Offsets above this are rejected when parsing (a PlayerIns is a few KiB; a pointee read is bounded too).
const MAX_OFF: u64 = 0xFFFF;
/// PlayerIns -> the character's module list (a block of pointers, no vtable). NR 1.3.3.0: seen in the FS-0 run 7 dump
/// (`[PlayerIns+0x1B8]`, 57 pointers); that it is the module list is a HYPOTHESIS (ER has it at +0x190, which is
/// `CS::SpecialEffect` in NR). MEMORY_MAP.md section 8.
pub(crate) const MODULES_OFF: usize = 0x1B8;
/// Bytes of the module list searched for the physics module.
pub(crate) const MODULES_SPAN: usize = 0x1C8;
/// Vtable of `CS::CSChrPhysicsModule` as an exe offset. NR 1.3.3.0, read from the RTTI of `nightreign.exe` on disk
/// (2026-10-13). The game reads the position at +0x70 of this object (code at exe+0x46E5C1). MEMORY_MAP.md section 8.
pub(crate) const PHYSICS_VTABLE_RVA: usize = 0x2C1_3720;
/// PlayerIns -> origin of the local frame the physics position is measured in (x, y, z f32; y stays 0). The game moves it in
/// 8 m steps as the player walks, so world position = physics position - origin. NR 1.3.3.0, T-028 run 3 (2026-10-14):
/// local minus this value is a smooth path across every jump of the local value. MEMORY_MAP.md section 8.
pub(crate) const ORIGIN_OFF: usize = 0x80;

/// Where one candidate triple lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cand {
    /// `PlayerIns+off`.
    Direct { off: u32 },
    /// `[PlayerIns+poff]+off`.
    Via { poff: u32, off: u32 },
    /// `Physics+off`: inside the player's `CS::CSChrPhysicsModule`.
    Physics { off: u32 },
}

impl Cand {
    /// Canonical text, as accepted by [`parse_candidate`].
    pub(crate) fn text(&self) -> String {
        match *self {
            Cand::Direct { off } => format!("PlayerIns+0x{off:X}"),
            Cand::Via { poff, off } => format!("[PlayerIns+0x{poff:X}]+0x{off:X}"),
            Cand::Physics { off } => format!("Physics+0x{off:X}"),
        }
    }
}

fn parse_off(s: &str) -> Result<u32, String> {
    let t = s.trim();
    let v = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(h, 16)
    } else {
        t.parse::<u64>()
    }
    .map_err(|_| format!("{t:?} is not a number"))?;
    if v > MAX_OFF {
        return Err(format!("offset 0x{v:X} is larger than 0x{MAX_OFF:X}"));
    }
    u32::try_from(v).map_err(|_| "offset out of range".to_string())
}

fn after_playerins(s: &str) -> Result<&str, String> {
    after_name(s, "playerins")
}

/// Strip a case-insensitive `name` prefix and the `+` after it; the rest is the offset text.
fn after_name<'a>(s: &'a str, name: &str) -> Result<&'a str, String> {
    let t = s.trim();
    if t.len() < name.len()
        || !t.is_char_boundary(name.len())
        || !t[..name.len()].eq_ignore_ascii_case(name)
    {
        return Err(format!("expected {name}"));
    }
    t[name.len()..]
        .trim_start()
        .strip_prefix('+')
        .ok_or_else(|| format!("expected '+' after {name}"))
}

/// Parse one candidate: `PlayerIns+0x6C0`, `[PlayerIns+0x60]+0x1EC` or `Physics+0x70`.
pub(crate) fn parse_candidate(s: &str) -> Result<Cand, String> {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix('[') {
        let (inner, tail) = rest
            .split_once(']')
            .ok_or_else(|| "missing ']'".to_string())?;
        let poff = parse_off(after_playerins(inner)?)?;
        let off = parse_off(
            tail.trim_start()
                .strip_prefix('+')
                .ok_or_else(|| "expected '+offset' after ']'".to_string())?,
        )?;
        Ok(Cand::Via { poff, off })
    } else if let Ok(rest) = after_name(t, "physics") {
        Ok(Cand::Physics {
            off: parse_off(rest)?,
        })
    } else {
        Ok(Cand::Direct {
            off: parse_off(after_playerins(t)?)?,
        })
    }
}

/// Parse a configured list. Invalid entries are skipped and reported in the returned warnings.
pub(crate) fn parse_list(list: &[String]) -> (Vec<Cand>, Vec<String>) {
    let mut out: Vec<Cand> = Vec::new();
    let mut warnings = Vec::new();
    for s in list {
        match parse_candidate(s) {
            Ok(c) if out.contains(&c) => {
                warnings.push(format!("pos_candidates: duplicate entry {s:?} skipped"));
            }
            Ok(c) => out.push(c),
            Err(e) => warnings.push(format!("pos_candidates: entry {s:?} skipped: {e}")),
        }
    }
    (out, warnings)
}

/// The defaults of `pos_candidates`: the physics module position first (the field a community cheat table edits to move
/// the player), then the FS-0 run 6 findings.
pub(crate) fn default_candidates() -> Vec<String> {
    [
        "Physics+0x70",
        "PlayerIns+0x6C0",
        "PlayerIns+0x6D0",
        "[PlayerIns+0x60]+0x1EC",
        "[PlayerIns+0x6B0]+0x17C",
        "[PlayerIns+0x6B0]+0x18C",
        "[PlayerIns+0x5F0]+0x64",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Memory the test reads and writes. The real one is [`RealMem`]; tests use a fake.
pub(crate) trait PosMem {
    fn read_u64(&self, addr: usize) -> Option<u64>;
    fn read12(&self, addr: usize) -> Option<[u8; 12]>;
    fn write(&mut self, addr: usize, data: &[u8]) -> Result<(), &'static str>;
}

/// Guarded game memory.
pub(crate) struct RealMem;

impl PosMem for RealMem {
    fn read_u64(&self, addr: usize) -> Option<u64> {
        mem::read::<u64>(addr)
    }
    fn read12(&self, addr: usize) -> Option<[u8; 12]> {
        mem::read::<[u8; 12]>(addr)
    }
    fn write(&mut self, addr: usize, data: &[u8]) -> Result<(), &'static str> {
        mem::write_bytes(addr, data)
    }
}

pub(crate) fn decode(b: &[u8; 12]) -> [f32; 3] {
    let f = |i: usize| f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    [f(0), f(4), f(8)]
}

pub(crate) fn fmt3(v: [f32; 3]) -> String {
    format!("({:.3},{:.3},{:.3})", v[0], v[1], v[2])
}

/// A finite triple with every |component| below [`MAX_ABS`].
pub(crate) fn plausible(v: [f32; 3]) -> bool {
    v.iter().all(|x| x.is_finite() && x.abs() < MAX_ABS)
}

/// What was captured for one candidate at save time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Saved {
    bytes: [u8; 12],
    /// Pointer value read from the PlayerIns (Via candidates).
    ptr: Option<usize>,
    /// First qword of the pointee if it is an exe-image address (the vtable).
    vtable: Option<usize>,
    /// The local-frame origin at save time (Physics candidates): a later write is shifted by how far the origin moved.
    origin: Option<[u8; 12]>,
}

/// A resolved candidate address plus the facts a later write is checked against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Resolved {
    addr: usize,
    ptr: Option<usize>,
    vtable: Option<usize>,
    /// Offset inside the module list where the object was found (Physics candidates).
    slot: Option<usize>,
}

/// What a successful write did, for the log.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WriteReport {
    pub candidate: String,
    pub addr: usize,
    pub old: [f32; 3],
    pub new: [f32; 3],
    pub readback: [f32; 3],
}

impl WriteReport {
    pub(crate) fn line(&self) -> String {
        format!(
            "T-032 POS write candidate={} addr=0x{:X} old={} new={} readback={}",
            self.candidate,
            self.addr,
            fmt3(self.old),
            fmt3(self.new),
            fmt3(self.readback)
        )
    }
}

/// The player's `CS::CSChrPhysicsModule` and the offset in the module list where it was found: the first pointer in the
/// first [`MODULES_SPAN`] bytes of `[PlayerIns+MODULES_OFF]` whose pointee has the physics vtable (`base` = exe base).
pub(crate) fn physics_module(
    m: &dyn PosMem,
    player: usize,
    base: usize,
) -> Result<(usize, usize), String> {
    let list = m
        .read_u64(player + MODULES_OFF)
        .ok_or("the module list pointer is unreadable")? as usize;
    if !is_ptr(list) {
        return Err(format!(
            "the module list pointer 0x{list:X} is null or implausible"
        ));
    }
    let want = base + PHYSICS_VTABLE_RVA;
    (0..MODULES_SPAN)
        .step_by(8)
        .find_map(|slot| {
            let p = m.read_u64(list + slot)? as usize;
            (is_ptr(p) && m.read_u64(p)? as usize == want).then_some((p, slot))
        })
        .ok_or_else(|| {
            format!(
                "no CS::CSChrPhysicsModule (vtable exe+0x{PHYSICS_VTABLE_RVA:X}) in the first 0x{MODULES_SPAN:X} bytes of [PlayerIns+0x{MODULES_OFF:X}]"
            )
        })
}

/// Candidate list, selection and the saved values.
#[derive(Debug)]
pub(crate) struct PosTest {
    cands: Vec<Cand>,
    sel: usize,
    saved: Vec<Option<Saved>>,
    /// The PlayerIns the values were saved from.
    saved_for: Option<usize>,
    base: usize,
    size: usize,
}

impl PosTest {
    /// `cands` empty falls back to the defaults. `base`/`size` = the exe image (a pointee's first qword inside it is a vtable).
    pub(crate) fn new(cands: &[Cand], base: usize, size: usize) -> Self {
        let cands: Vec<Cand> = if cands.is_empty() {
            parse_list(&default_candidates()).0
        } else {
            cands.to_vec()
        };
        let n = cands.len();
        PosTest {
            cands,
            sel: 0,
            saved: vec![None; n],
            saved_for: None,
            base,
            size,
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.cands.len()
    }

    /// 1-based position of the selection.
    pub(crate) fn position(&self) -> (usize, usize) {
        (self.sel + 1, self.cands.len())
    }

    pub(crate) fn selected_text(&self) -> String {
        self.cands.get(self.sel).map_or(String::new(), Cand::text)
    }

    pub(crate) fn has_saved(&self) -> bool {
        self.saved_for.is_some()
    }

    /// Select the next candidate; returns the log line.
    pub(crate) fn cycle(&mut self) -> String {
        if !self.cands.is_empty() {
            self.sel = (self.sel + 1) % self.cands.len();
        }
        let (i, n) = self.position();
        format!("T-032 POS candidate = {} ({i}/{n})", self.selected_text())
    }

    /// Forget the saved values (a new WorldChrMan instance re-creates the PlayerIns).
    pub(crate) fn forget(&mut self) {
        self.saved.iter_mut().for_each(|s| *s = None);
        self.saved_for = None;
    }

    fn in_image(&self, v: usize) -> bool {
        v >= self.base && v - self.base < self.size
    }

    fn resolve(&self, m: &dyn PosMem, player: usize, c: Cand) -> Result<Resolved, String> {
        match c {
            Cand::Direct { off } => Ok(Resolved {
                addr: player + off as usize,
                ptr: None,
                vtable: None,
                slot: None,
            }),
            Cand::Via { poff, off } => {
                let ptr = m
                    .read_u64(player + poff as usize)
                    .ok_or("the pointer is unreadable")? as usize;
                if !is_ptr(ptr) {
                    return Err(format!("the pointer 0x{ptr:X} is null or implausible"));
                }
                let vtable = m
                    .read_u64(ptr)
                    .map(|v| v as usize)
                    .filter(|&v| self.in_image(v));
                Ok(Resolved {
                    addr: ptr + off as usize,
                    ptr: Some(ptr),
                    vtable,
                    slot: None,
                })
            }
            Cand::Physics { off } => {
                let (p, slot) = physics_module(m, player, self.base)?;
                Ok(Resolved {
                    addr: p + off as usize,
                    ptr: Some(p),
                    vtable: Some(self.base + PHYSICS_VTABLE_RVA),
                    slot: Some(slot),
                })
            }
        }
    }

    /// Current value of every candidate of `player`, as `(text, value)`.
    pub(crate) fn read_all(
        &self,
        m: &dyn PosMem,
        player: usize,
    ) -> Vec<(String, Option<[f32; 3]>)> {
        self.cands
            .iter()
            .map(|&c| {
                let v = self
                    .resolve(m, player, c)
                    .ok()
                    .and_then(|r| m.read12(r.addr))
                    .map(|b| decode(&b));
                (c.text(), v)
            })
            .collect()
    }

    /// One line with every candidate's value; the selected one is starred.
    pub(crate) fn values_line(&self, m: &dyn PosMem, player: usize, label: &str) -> String {
        let mut s = format!("T-032 POS values {label}:");
        for (i, (t, v)) in self.read_all(m, player).iter().enumerate() {
            let _ = write!(
                s,
                " {}{t}={}",
                if i == self.sel { "*" } else { "" },
                v.map_or("unreadable".to_string(), fmt3)
            );
        }
        s
    }

    /// F7: save the current 12 bytes of every candidate of `player` (the LOCAL PlayerIns, already validated by the caller).
    /// Returns the log lines. Candidates that cannot be read stay unsaved.
    pub(crate) fn save(&mut self, m: &dyn PosMem, player: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut any = false;
        for i in 0..self.cands.len() {
            let c = self.cands[i];
            let mut found = String::new();
            let got = self.resolve(m, player, c).and_then(|r| {
                if let (Some(slot), Some(p)) = (r.slot, r.ptr) {
                    found = format!(
                        " module list +0x{slot:X}, owner at +0x8 is {}",
                        if m.read_u64(p + 8) == Some(player as u64) {
                            "this PlayerIns"
                        } else {
                            "NOT this PlayerIns"
                        }
                    );
                }
                let origin = match c {
                    Cand::Physics { .. } => {
                        let o = m
                            .read12(player + ORIGIN_OFF)
                            .filter(|b| plausible(decode(b)))
                            .ok_or("the origin at PlayerIns+0x80 is unreadable or implausible")?;
                        found.push_str(&format!(" origin={}", fmt3(decode(&o))));
                        Some(o)
                    }
                    _ => None,
                };
                m.read12(r.addr)
                    .map(|bytes| Saved {
                        bytes,
                        ptr: r.ptr,
                        vtable: r.vtable,
                        origin,
                    })
                    .ok_or_else(|| format!("0x{:X} is unreadable", r.addr))
            });
            match got {
                Ok(s) => {
                    any = true;
                    let extra = match (s.ptr, s.vtable) {
                        (Some(p), Some(v)) => {
                            format!(" pointee=0x{p:X} vtable=exe+0x{:X}", v - self.base)
                        }
                        (Some(p), None) => format!(" pointee=0x{p:X} vtable=none"),
                        _ => String::new(),
                    };
                    lines.push(format!(
                        "T-032 POS saved candidate={} value={}{}{extra}{found}",
                        c.text(),
                        fmt3(decode(&s.bytes)),
                        if plausible(decode(&s.bytes)) {
                            ""
                        } else {
                            " (NOT plausible: F6 would refuse it)"
                        }
                    ));
                    self.saved[i] = Some(s);
                }
                Err(e) => {
                    lines.push(format!(
                        "T-032 POS save candidate={} skipped: {e}",
                        c.text()
                    ));
                    self.saved[i] = None;
                }
            }
        }
        self.saved_for = any.then_some(player);
        lines
    }

    /// F6: write the saved 12 bytes into the SELECTED candidate of `player`. `player` must be the LOCAL PlayerIns, re-resolved
    /// (entry[0] rule + vtable check) by the caller immediately before this call.
    pub(crate) fn write(&self, m: &mut dyn PosMem, player: usize) -> Result<WriteReport, String> {
        let c = *self.cands.get(self.sel).ok_or("no candidate selected")?;
        let name = c.text();
        let saved = self
            .saved
            .get(self.sel)
            .copied()
            .flatten()
            .ok_or_else(|| format!("nothing saved for {name}; press the save key first"))?;
        if self.saved_for != Some(player) {
            return Err(format!(
                "the values were saved for another PlayerIns ({}); this is 0x{player:X}; save again",
                self.saved_for
                    .map_or("none".to_string(), |p| format!("0x{p:X}"))
            ));
        }
        let mut new = decode(&saved.bytes);
        if let Some(o0) = saved.origin {
            let o1 = m
                .read12(player + ORIGIN_OFF)
                .map(|b| decode(&b))
                .filter(|o| plausible(*o))
                .ok_or("the origin at PlayerIns+0x80 is unreadable or implausible; refusing")?;
            let o0 = decode(&o0);
            for i in 0..3 {
                new[i] += o1[i] - o0[i];
            }
        }
        if !plausible(new) {
            return Err(format!(
                "the saved value {} for {name} is not finite or has a component of at least {MAX_ABS}; refusing",
                fmt3(new)
            ));
        }
        let now = self.resolve(&*m, player, c)?;
        if !matches!(c, Cand::Direct { .. }) {
            match saved.vtable {
                Some(v) if now.vtable != Some(v) => {
                    return Err(format!(
                        "the pointee's vtable is now {} but was exe+0x{:X} at save time; refusing",
                        now.vtable
                            .map_or("none".to_string(), |x| format!("exe+0x{:X}", x - self.base)),
                        v - self.base
                    ));
                }
                None if now.ptr != saved.ptr => {
                    return Err(format!(
                        "the pointee had no vtable at save time and the pointer changed (0x{:X} -> 0x{:X}); refusing",
                        saved.ptr.unwrap_or(0),
                        now.ptr.unwrap_or(0)
                    ));
                }
                _ => {}
            }
        }
        let old = m
            .read12(now.addr)
            .map(|b| decode(&b))
            .ok_or("the target is unreadable")?;
        let mut bytes = [0u8; 12];
        for (i, v) in new.iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        m.write(now.addr, &bytes)
            .map_err(|e| format!("guarded write refused: {e}"))?;
        let readback = m
            .read12(now.addr)
            .map(|b| decode(&b))
            .ok_or("the target is unreadable after the write")?;
        Ok(WriteReport {
            candidate: name,
            addr: now.addr,
            old,
            new,
            readback,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const IMG: usize = 0x7FF6_0000_0000;
    const IMG_SIZE: usize = 0x800_0000;
    const VT_A: usize = IMG + 0x2C0_A898;
    const VT_B: usize = IMG + 0x2C8_FD20;
    const PLAYER: usize = 0x1000_0000;
    const POINTEE: usize = 0x2000_0000;
    const POINTEE2: usize = 0x3000_0000;

    /// Byte-addressed fake memory; unmapped addresses are unreadable.
    #[derive(Default)]
    struct Fake {
        bytes: HashMap<usize, u8>,
        writes: Vec<(usize, Vec<u8>)>,
        refuse_writes: bool,
    }

    impl Fake {
        fn map(&mut self, addr: usize, len: usize) {
            for i in 0..len {
                self.bytes.entry(addr + i).or_insert(0);
            }
        }
        fn put(&mut self, addr: usize, data: &[u8]) {
            for (i, &b) in data.iter().enumerate() {
                self.bytes.insert(addr + i, b);
            }
        }
        fn put_u64(&mut self, addr: usize, v: usize) {
            self.put(addr, &(v as u64).to_le_bytes());
        }
        fn put3(&mut self, addr: usize, v: [f32; 3]) {
            let mut b = Vec::new();
            for x in v {
                b.extend_from_slice(&x.to_le_bytes());
            }
            self.put(addr, &b);
        }
        fn get3(&self, addr: usize) -> [f32; 3] {
            decode(&self.read12(addr).expect("mapped"))
        }
    }

    impl PosMem for Fake {
        fn read_u64(&self, addr: usize) -> Option<u64> {
            let mut b = [0u8; 8];
            for (i, x) in b.iter_mut().enumerate() {
                *x = *self.bytes.get(&(addr + i))?;
            }
            Some(u64::from_le_bytes(b))
        }
        fn read12(&self, addr: usize) -> Option<[u8; 12]> {
            let mut b = [0u8; 12];
            for (i, x) in b.iter_mut().enumerate() {
                *x = *self.bytes.get(&(addr + i))?;
            }
            Some(b)
        }
        fn write(&mut self, addr: usize, data: &[u8]) -> Result<(), &'static str> {
            if self.refuse_writes {
                return Err("fake refuses");
            }
            for i in 0..data.len() {
                if !self.bytes.contains_key(&(addr + i)) {
                    return Err("unmapped");
                }
            }
            self.put(addr, data);
            self.writes.push((addr, data.to_vec()));
            Ok(())
        }
    }

    /// The six FS-0 run 6 candidates (the defaults without the leading `Physics+0x70`).
    fn cands() -> Vec<Cand> {
        parse_list(&default_candidates()).0[1..].to_vec()
    }

    /// A PlayerIns at PLAYER with positions at 0x6C0 and 0x6D0, a pointer at 0x60 to POINTEE (vtable VT_A, triple at 0x1EC) and
    /// a pointer at 0x6B0 to POINTEE2 (no vtable, triples at 0x17C and 0x18C); 0x5F0 holds a null pointer.
    fn world() -> Fake {
        let mut f = Fake::default();
        f.map(PLAYER, 0x1000);
        f.map(POINTEE, 0x200);
        f.map(POINTEE2, 0x200);
        f.put_u64(PLAYER + 0x60, POINTEE);
        f.put_u64(POINTEE, VT_A);
        f.put_u64(PLAYER + 0x6B0, POINTEE2);
        f.put3(PLAYER + 0x6C0, [10.0, 20.0, 30.0]);
        f.put3(PLAYER + 0x6D0, [11.0, 21.0, 31.0]);
        f.put3(POINTEE + 0x1EC, [12.0, 22.0, 32.0]);
        f.put3(POINTEE2 + 0x17C, [13.0, 23.0, 33.0]);
        f.put3(POINTEE2 + 0x18C, [14.0, 24.0, 34.0]);
        f
    }

    fn test() -> PosTest {
        PosTest::new(&cands(), IMG, IMG_SIZE)
    }

    #[test]
    fn candidate_syntax_parses_and_round_trips() {
        assert_eq!(
            parse_candidate("PlayerIns+0x6C0"),
            Ok(Cand::Direct { off: 0x6C0 })
        );
        assert_eq!(
            parse_candidate("  playerins + 0x6c0 "),
            Ok(Cand::Direct { off: 0x6C0 })
        );
        assert_eq!(
            parse_candidate("PlayerIns+16"),
            Ok(Cand::Direct { off: 16 })
        );
        assert_eq!(
            parse_candidate("[PlayerIns+0x60]+0x1EC"),
            Ok(Cand::Via {
                poff: 0x60,
                off: 0x1EC
            })
        );
        assert_eq!(
            parse_candidate("[ PlayerIns + 0X5F0 ] + 100"),
            Ok(Cand::Via {
                poff: 0x5F0,
                off: 100
            })
        );
        for s in default_candidates() {
            let c = parse_candidate(&s).expect(&s);
            assert_eq!(c.text(), s, "canonical text");
        }
        for bad in [
            "",
            "PlayerIns",
            "PlayerIns+",
            "PlayerIns+zz",
            "PlayerIns-0x10",
            "Player+0x10",
            "[PlayerIns+0x60]",
            "[PlayerIns+0x60]0x10",
            "[PlayerIns+0x60+0x1EC",
            "[PlayerIns+0x60]+",
            "[[PlayerIns+0x60]+0x4]+0x8",
            "PlayerIns+0x10000",
            "PlayerIns+-1",
            "é",
            "PlayerInsé+1",
        ] {
            assert!(parse_candidate(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_physics_module_is_found_by_its_vtable_in_the_module_list() {
        const LIST: usize = 0x6000_0000;
        const PHYS: usize = 0x7000_0000;
        let vt = IMG + PHYSICS_VTABLE_RVA;
        assert_eq!(
            parse_candidate(" physics + 0x70 "),
            Ok(Cand::Physics { off: 0x70 })
        );
        let mut f = world();
        f.map(LIST, MODULES_SPAN);
        f.map(PHYS, 0x200);
        let mut t = PosTest::new(&[Cand::Physics { off: 0x70 }], IMG, IMG_SIZE);
        // No module list pointer yet, then a list without the physics module: nothing saved, nothing written.
        assert!(t.save(&f, PLAYER)[0].contains("skipped"));
        f.put_u64(PLAYER + MODULES_OFF, LIST);
        f.put_u64(LIST + 0x60, POINTEE); // another module (vtable VT_A)
        assert!(t.save(&f, PLAYER)[0].contains("no CS::CSChrPhysicsModule"));
        assert!(!t.has_saved());
        // The physics module sits in a slot that is not ER's 0x68.
        f.put_u64(LIST + 0x78, PHYS);
        f.put_u64(PHYS, vt);
        f.put_u64(PHYS + 8, PLAYER);
        f.put3(PHYS + 0x70, [1.0, 2.0, 3.0]);
        let lines = t.save(&f, PLAYER);
        assert!(
            lines[0].contains("module list +0x78, owner at +0x8 is this PlayerIns"),
            "{}",
            lines[0]
        );
        f.put3(PHYS + 0x70, [50.0, 2.0, 3.0]);
        let r = t.write(&mut f, PLAYER).expect("write");
        assert_eq!(r.addr, PHYS + 0x70);
        assert_eq!(f.get3(PHYS + 0x70), [1.0, 2.0, 3.0]);
        // The origin moved by (-32, 0, 24) since the save: the same world spot is now that far off in local numbers.
        f.put3(PLAYER + ORIGIN_OFF, [-32.0, 0.0, 24.0]);
        t.write(&mut f, PLAYER).expect("write after an origin move");
        assert_eq!(f.get3(PHYS + 0x70), [-31.0, 2.0, 27.0]);
        // An unreadable origin is refused rather than guessed.
        f.put3(PLAYER + ORIGIN_OFF, [f32::NAN, 0.0, 0.0]);
        assert!(t.write(&mut f, PLAYER).unwrap_err().contains("origin"));
        f.put3(PLAYER + ORIGIN_OFF, [0.0; 3]);
        // The object in that slot is no longer a physics module: refused.
        f.put_u64(PHYS, VT_B);
        assert!(t.write(&mut f, PLAYER).is_err());
        assert_eq!(f.writes.len(), 2);
    }

    #[test]
    fn invalid_list_entries_are_skipped_with_a_warning() {
        let list: Vec<String> = [
            "PlayerIns+0x6C0",
            "nonsense",
            "PlayerIns+0x6C0",
            "[PlayerIns+0x60]+0x1EC",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (c, w) = parse_list(&list);
        assert_eq!(c.len(), 2);
        assert_eq!(w.len(), 2);
        assert!(w[0].contains("nonsense") && w[1].contains("duplicate"));
        assert_eq!(parse_list(&default_candidates()).0.len(), 7);
        // An empty / all-invalid list uses the defaults in PosTest.
        assert_eq!(PosTest::new(&[], IMG, IMG_SIZE).len(), 7);
    }

    #[test]
    fn select_cycles_and_wraps() {
        let mut t = test();
        assert_eq!(t.selected_text(), "PlayerIns+0x6C0");
        assert_eq!(t.cycle(), "T-032 POS candidate = PlayerIns+0x6D0 (2/6)");
        for _ in 0..4 {
            t.cycle();
        }
        assert_eq!(t.position(), (6, 6));
        assert_eq!(t.cycle(), "T-032 POS candidate = PlayerIns+0x6C0 (1/6)");
    }

    #[test]
    fn save_then_write_writes_only_the_selected_candidate() {
        let mut f = world();
        let mut t = test();
        assert!(!t.has_saved());
        let lines = t.save(&f, PLAYER);
        assert!(t.has_saved());
        // 5 readable candidates saved, [PlayerIns+0x5F0] has a null pointer.
        assert_eq!(lines.len(), 6);
        assert!(lines[2].contains("vtable=exe+0x2C0A898"), "{}", lines[2]);
        assert!(lines[3].contains("vtable=none"), "{}", lines[3]);
        assert!(lines[5].contains("skipped"), "{}", lines[5]);
        // The player walks away; select the second candidate and write.
        f.put3(PLAYER + 0x6C0, [110.0, 20.0, 30.0]);
        f.put3(PLAYER + 0x6D0, [111.0, 21.0, 31.0]);
        t.cycle();
        let r = t.write(&mut f, PLAYER).expect("write");
        assert_eq!(r.candidate, "PlayerIns+0x6D0");
        assert_eq!(r.old, [111.0, 21.0, 31.0]);
        assert_eq!(r.new, [11.0, 21.0, 31.0]);
        assert_eq!(r.readback, [11.0, 21.0, 31.0]);
        assert_eq!(f.writes.len(), 1);
        assert_eq!(f.writes[0].0, PLAYER + 0x6D0);
        assert_eq!(f.writes[0].1.len(), 12);
        // The others were not touched.
        assert_eq!(f.get3(PLAYER + 0x6C0), [110.0, 20.0, 30.0]);
        assert!(r
            .line()
            .starts_with("T-032 POS write candidate=PlayerIns+0x6D0"));
        assert!(r.line().contains("old=(111.000,21.000,31.000)"));
        // A pointee candidate with a vtable is written through the pointer.
        t.cycle();
        f.put3(POINTEE + 0x1EC, [500.0, 0.0, 0.0]);
        let r = t.write(&mut f, PLAYER).expect("via write");
        assert_eq!(f.get3(POINTEE + 0x1EC), [12.0, 22.0, 32.0]);
        assert_eq!(r.addr, POINTEE + 0x1EC);
        assert_eq!(f.writes.len(), 2);
    }

    #[test]
    fn nothing_saved_or_saved_for_another_player_is_refused() {
        let mut f = world();
        let mut t = test();
        assert!(t
            .write(&mut f, PLAYER)
            .unwrap_err()
            .contains("nothing saved"));
        t.save(&f, PLAYER);
        let e = t.write(&mut f, PLAYER + 0x8000).unwrap_err();
        assert!(e.contains("another PlayerIns"), "{e}");
        t.forget();
        assert!(!t.has_saved());
        assert!(t
            .write(&mut f, PLAYER)
            .unwrap_err()
            .contains("nothing saved"));
        assert!(f.writes.is_empty());
    }

    #[test]
    fn a_pointee_whose_vtable_changed_is_refused() {
        let mut f = world();
        let mut t = test();
        t.save(&f, PLAYER);
        t.cycle();
        t.cycle(); // [PlayerIns+0x60]+0x1EC
                   // The object behind the pointer was replaced by something else.
        f.put_u64(POINTEE, VT_B);
        let e = t.write(&mut f, PLAYER).unwrap_err();
        assert!(e.contains("vtable"), "{e}");
        // The pointee lost its vtable altogether.
        f.put_u64(POINTEE, 0x1234);
        assert!(t.write(&mut f, PLAYER).unwrap_err().contains("vtable"));
        // The pointer now leads to unreadable memory.
        f.put_u64(PLAYER + 0x60, 0x4000_0000);
        assert!(t.write(&mut f, PLAYER).is_err());
        // The pointer is null.
        f.put_u64(PLAYER + 0x60, 0);
        assert!(t.write(&mut f, PLAYER).unwrap_err().contains("null"));
        assert!(f.writes.is_empty());
    }

    #[test]
    fn a_pointee_without_a_vtable_needs_the_same_pointer() {
        let mut f = world();
        let mut t = test();
        t.save(&f, PLAYER);
        for _ in 0..3 {
            t.cycle();
        } // [PlayerIns+0x6B0]+0x17C
        assert_eq!(t.selected_text(), "[PlayerIns+0x6B0]+0x17C");
        // Same pointer: allowed.
        f.put3(POINTEE2 + 0x17C, [99.0, 0.0, 0.0]);
        t.write(&mut f, PLAYER).expect("same pointer");
        assert_eq!(f.get3(POINTEE2 + 0x17C), [13.0, 23.0, 33.0]);
        // The pointer moved to another (mapped) block: refused.
        f.map(0x5000_0000, 0x200);
        f.put_u64(PLAYER + 0x6B0, 0x5000_0000);
        let e = t.write(&mut f, PLAYER).unwrap_err();
        assert!(e.contains("pointer changed"), "{e}");
        assert_eq!(f.writes.len(), 1);
    }

    #[test]
    fn non_finite_or_huge_saved_values_are_refused() {
        for bad in [
            [f32::NAN, 0.0, 0.0],
            [0.0, f32::INFINITY, 0.0],
            [0.0, 0.0, f32::NEG_INFINITY],
            [100_000.0, 0.0, 0.0],
            [0.0, -100_000.0, 0.0],
            [0.0, 0.0, 1.0e30],
        ] {
            let mut f = world();
            f.put3(PLAYER + 0x6C0, bad);
            let mut t = test();
            let saved = t.save(&f, PLAYER);
            assert!(saved[0].contains("NOT plausible"), "{}", saved[0]);
            f.put3(PLAYER + 0x6C0, [1.0, 2.0, 3.0]);
            let e = t.write(&mut f, PLAYER).unwrap_err();
            assert!(e.contains("not finite"), "{e}");
            assert_eq!(f.get3(PLAYER + 0x6C0), [1.0, 2.0, 3.0]);
            assert!(f.writes.is_empty());
        }
        // Just inside the limit is fine.
        let mut f = world();
        f.put3(PLAYER + 0x6C0, [99_999.0, -99_999.0, 0.0]);
        let mut t = test();
        t.save(&f, PLAYER);
        f.put3(PLAYER + 0x6C0, [0.0; 3]);
        assert!(t.write(&mut f, PLAYER).is_ok());
    }

    #[test]
    fn a_refused_store_is_an_error_and_unreadable_targets_are_reported() {
        let mut f = world();
        let mut t = test();
        t.save(&f, PLAYER);
        f.refuse_writes = true;
        let e = t.write(&mut f, PLAYER).unwrap_err();
        assert!(e.contains("guarded write refused"), "{e}");
        // Values line shows unreadable candidates.
        let l = t.values_line(&f, PLAYER, "+100ms");
        assert!(l.starts_with("T-032 POS values +100ms:"));
        assert!(l.contains("*PlayerIns+0x6C0=(10.000,20.000,30.000)"), "{l}");
        assert!(l.contains("[PlayerIns+0x5F0]+0x64=unreadable"), "{l}");
    }
}
