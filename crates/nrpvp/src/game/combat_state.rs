//! T-033/T-034/T-035/T-039: combat-state tracker, the rival revive rule and the downed rule.
//!
//! Facts used (all "VERIFIED on NR 1.3.3.0 by FS-0 runs 7-9, TECHNICAL_NOTES section 4", identical on every client unless noted):
//! - `PlayerIns+0x710` u8 == 1 while downed, 0 otherwise;
//! - `PlayerIns+0x730` = number of downs;
//! - `PlayerIns+0x1A0` u64 = handle of the last attacker, valid on the victim's OWN client only (0xFFFF_FFFF_FFFF_FFFF or some
//!   other value on remote copies);
//! - a character's handle is the u64 at `ChrIns+0x08` (seen 0xFFFFFFFF10C00000 host, 0xFFFFFFFF10C00001 joiner);
//! - `PlayerIns+0x5A0` -> object whose RTTI class is `CS::PlayerGameData` (vtable exe+0x2BE6E28); HP = u32 at `+0x10` (240 full,
//!   0 downed, 120 just revived). FS-0 run 8: that field is a MIRROR (writing 0 had no effect). The same pattern is at
//!   `PlayerIns+0xBA0` (u32, mirror) and `[PlayerIns+0xB78]+0x70` (u32): FS-0 run 9 found THAT one authoritative. Writing 0 to it
//!   on the owner's client right after a revive downs the player again at once. Leaving the downed state with hp 0 = bled out,
//!   with hp > 0 = revived. The bleed-out lasts about 15-17 s and restarts on every down.
//!
//! 1. [`Tracker`] (both modes): every 100 ms, per ChrSet player: downed flag, down count, HP, own handle; for LOCAL also the last
//!    attacker, classified against the other players' handles and the rosters (ALLY / RIVAL / SELF / OTHER). Only transitions are
//!    produced. Dev mode logs all of them; the player build logs nothing but the rule line below.
//! 2. **Rule v1** (both modes, always on unless dev mode sets `rival_revive = "vanilla"`): on LOCAL's UP transition (downed 1 -> 0
//!    with HP > 0, not a respawn) whose last attacker is a RIVAL, write u32 0 to `[PlayerIns+0xB78]+0x70` once, so LOCAL is downed
//!    again instead of revived ([`Core::apply_rule`], the only store in this module). Gates right before the write: PlayerIns
//!    vtable, plausible `[PlayerIns+0xB78]`, a current value > 0 that equals the mirror at `PlayerIns+0xBA0` or the PlayerGameData
//!    HP. Never for remote players, never for ALLY/SELF/OTHER.
//!
//! Memory access goes through the guarded [`CombatMem`] trait; the real implementation uses `mem::read` (VirtualQuery-guarded)
//! and `mem::write_bytes` (the single store: [`Core::apply_rule`]).

use std::collections::{HashMap, HashSet};

use super::mem::{self, is_ptr};
use super::rtti::Rtti;
use crate::config::Config;
use crate::overlay::shared::Relation;

// VERIFIED on NR 1.3.3.0 by FS-0 run 7, TECHNICAL_NOTES section 4.
pub(crate) const OFF_DOWNED: usize = 0x710;
pub(crate) const OFF_DOWNS: usize = 0x730;
pub(crate) const OFF_LAST_ATTACKER: usize = 0x1A0;
pub(crate) const OFF_GAMEDATA: usize = 0x5A0;
pub(crate) const GD_HP: usize = 0x10;
pub(crate) const CHR_HANDLE: usize = 0x08;
/// Readable RTTI name of the HP object's class.
pub(crate) const GAMEDATA_CLASS: &str = "CS::PlayerGameData";

// T-039: the downed-state fields on the owner's client. VERIFIED on NR 1.3.3.0 by FS-0 run 7 + run 14, TECHNICAL_NOTES section 4.
/// f32 revive bar, amount left (40/90/240 at down 1/2/3; each hit subtracts; 0 = revived).
pub(crate) const OFF_REVIVE_BAR: usize = 0x714;
/// f32 bleed-out clock, seconds left (death at 0; writable).
pub(crate) const OFF_BLEED_CLOCK: usize = 0x718;
/// f32 pause timer (about 3.5 s after a hit, counts down; the clock is frozen while it is above 0).
pub(crate) const OFF_PAUSE: usize = 0x71C;
/// u64 current attacker handle (all-ones when none).
pub(crate) const OFF_CUR_ATTACKER: usize = 0x198;
/// A bar drop larger than this between two ticks is a hit.
pub(crate) const BAR_HIT_EPS: f32 = 0.01;
/// Lowest clock value the rule writes.
pub(crate) const CLOCK_FLOOR: f32 = 0.05;
/// Default / clamp range of `rival_hit_seconds`.
pub(crate) const DEFAULT_RIVAL_HIT_SECONDS: f32 = 3.0;
pub(crate) const RIVAL_HIT_SECONDS_RANGE: (f32, f32) = (0.5, 15.0);

/// Tracker period.
pub(crate) const STATE_MS: u64 = 100;

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
    player_vts: HashSet<usize>,
    gd_vts: HashSet<usize>,
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

// ---------------------------------------------------------------------------------------------------
// Attacker classification
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttackerKind {
    Ally,
    Rival,
    SelfHit,
    /// Not a known player: a PvE enemy, nobody, or a handle that matches no roster entry.
    Other,
}

impl AttackerKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            AttackerKind::Ally => "ALLY",
            AttackerKind::Rival => "RIVAL",
            AttackerKind::SelfHit => "SELF",
            AttackerKind::Other => "OTHER",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Attacker {
    pub kind: AttackerKind,
    /// The valid handle that was read, if any.
    pub handle: Option<u64>,
    /// Roster id of the matched player.
    pub id: Option<u64>,
}

impl Attacker {
    pub(crate) fn text(&self) -> String {
        match (self.kind, self.handle) {
            (AttackerKind::Other, None) => "OTHER (none)".to_string(),
            (AttackerKind::Other, Some(h)) => format!("OTHER handle=0x{h:X} (not a player)"),
            (k, h) => format!(
                "{} id={} handle={}",
                k.label(),
                self.id.map_or("?".to_string(), |v| format!("0x{v:X}")),
                h.map_or("?".to_string(), |v| format!("0x{v:X}"))
            ),
        }
    }
}

/// 0 and all-ones are "no attacker" (all-ones is what remote copies hold).
pub(crate) fn valid_handle(h: u64) -> bool {
    h != 0 && h != u64::MAX
}

/// One player as far as classification is concerned.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlayerInfo {
    pub handle: Option<u64>,
    pub id: Option<u64>,
    pub relation: Option<Relation>,
}

/// Match a last-attacker handle against the players' own handles. A missing roster relation, an unreadable handle, an invalid
/// handle or no match all give `Other` (so the rule never fires on a guess).
pub(crate) fn classify(raw: Option<u64>, players: &[PlayerInfo]) -> Attacker {
    let Some(h) = raw.filter(|&h| valid_handle(h)) else {
        return Attacker {
            kind: AttackerKind::Other,
            handle: None,
            id: None,
        };
    };
    let kind_of = |p: &PlayerInfo| match p.relation {
        Some(Relation::Local) => AttackerKind::SelfHit,
        Some(Relation::Ally) => AttackerKind::Ally,
        Some(Relation::Rival) => AttackerKind::Rival,
        None => AttackerKind::Other,
    };
    match players.iter().find(|p| p.handle == Some(h)) {
        Some(p) => Attacker {
            kind: kind_of(p),
            handle: Some(h),
            id: p.id,
        },
        None => Attacker {
            kind: AttackerKind::Other,
            handle: Some(h),
            id: None,
        },
    }
}

// ---------------------------------------------------------------------------------------------------
// Tracker
// ---------------------------------------------------------------------------------------------------

/// A ChrSet player to track (from `TeamCtl::combat_subjects`).
#[derive(Clone, Debug)]
pub(crate) struct Subject {
    pub addr: usize,
    /// `LOCAL[0]` / `REMOTE[1]`.
    pub label: String,
    pub local: bool,
    pub id: Option<u64>,
    pub relation: Option<Relation>,
    /// Roster team number (None when the rosters are unknown).
    pub team_no: Option<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Reading {
    pub downed: Option<bool>,
    pub downs: Option<u8>,
    pub hp: Option<u32>,
    pub handle: Option<u64>,
    pub attacker_raw: Option<u64>,
}

fn read_f32(mem: &dyn CombatMem, addr: usize) -> Option<f32> {
    mem.read_u32(addr).map(f32::from_bits)
}

/// Handles and rosters of every tracked player (for classifying an attacker outside the tracker's 100 ms pass).
fn player_infos(mem: &dyn CombatMem, subjects: &[Subject]) -> Vec<PlayerInfo> {
    subjects
        .iter()
        .filter(|s| mem.is_player(s.addr))
        .map(|s| PlayerInfo {
            handle: mem.read_u64(s.addr + CHR_HANDLE),
            id: s.id,
            relation: s.relation,
        })
        .collect()
}

/// `PlayerGameData` of a PlayerIns: the pointer at +0x5A0, accepted only if its vtable resolves to the class.
fn gamedata_of(mem: &dyn CombatMem, addr: usize) -> Option<usize> {
    let g = usize::try_from(mem.read_u64(addr + OFF_GAMEDATA)?).ok()?;
    (is_ptr(g) && mem.is_gamedata(g)).then_some(g)
}

pub(crate) fn read_state(mem: &dyn CombatMem, addr: usize) -> Reading {
    Reading {
        downed: mem.read_u8(addr + OFF_DOWNED).and_then(|v| match v {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }),
        downs: mem.read_u8(addr + OFF_DOWNS),
        hp: gamedata_of(mem, addr).and_then(|g| mem.read_u32(g + GD_HP)),
        handle: mem.read_u64(addr + CHR_HANDLE),
        attacker_raw: mem.read_u64(addr + OFF_LAST_ATTACKER),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventKind {
    /// First sight of a player (state seeded, nothing happened).
    Init,
    Down,
    /// Downed 1 -> 0 with HP below the maximum seen (a revive, or HP not restored yet).
    Up,
    /// Downed 1 -> 0 with HP at the maximum seen (respawn after a bleed-out).
    Respawn,
    /// HP rose from 0 without the downed flag changing.
    HpRestored,
}

impl EventKind {
    fn label(self) -> &'static str {
        match self {
            EventKind::Init => "INIT",
            EventKind::Down => "DOWN",
            EventKind::Up => "UP",
            EventKind::Respawn => "RESPAWN",
            EventKind::HpRestored => "HP restored",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Event {
    pub label: String,
    pub id: Option<u64>,
    pub addr: usize,
    pub local: bool,
    pub kind: EventKind,
    pub downed: bool,
    pub downs: Option<u8>,
    pub hp: Option<u32>,
    /// LOCAL only: the classified attacker and whether it came from the latch (the field was already cleared).
    pub attacker: Option<(Attacker, bool)>,
}

fn opt<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map_or("?".to_string(), |x| x.to_string())
}

impl Event {
    fn who(&self) -> String {
        format!(
            "{} id={}",
            self.label,
            self.id.map_or("?".to_string(), |v| format!("0x{v:X}"))
        )
    }

    pub(crate) fn line(&self) -> String {
        let att = match (&self.attacker, self.local) {
            (Some((a, latched)), _) => format!(
                ", last_attacker={}{}",
                a.text(),
                if *latched { " (latched)" } else { "" }
            ),
            (None, true) => String::new(),
            (None, false) => ", last_attacker=n/a (remote copy)".to_string(),
        };
        if self.kind == EventKind::Init {
            return format!(
                "T-033 STATE who={} INIT (downed={}, downs={}, hp={})",
                self.who(),
                self.downed,
                opt(self.downs),
                opt(self.hp)
            );
        }
        format!(
            "T-033 STATE who={} {} (downs={}, hp={}{att})",
            self.who(),
            self.kind.label(),
            opt(self.downs),
            opt(self.hp)
        )
    }

    /// True for the LOCAL revive that the rule may act on: kind UP with HP > 0 and a RIVAL as last attacker.
    fn is_rival_revive(&self) -> bool {
        self.local
            && self.kind == EventKind::Up
            && self.hp.is_some_and(|h| h > 0)
            && self
                .attacker
                .is_some_and(|(a, _)| a.kind == AttackerKind::Rival)
    }
}

/// Rule gate: the policy is `Redown` and the event is a rival revive of LOCAL. One event per transition, so once per UP.
pub(crate) fn rule_should_fire(policy: RevivePolicy, ev: &Event) -> bool {
    policy == RevivePolicy::Redown && ev.is_rival_revive()
}

/// T-039: what the downed rule remembers about LOCAL while its state byte is 1.
#[derive(Clone, Copy, Debug)]
struct DownedMem {
    addr: usize,
    /// The bar value at the previous tick (never lowered by rival hits).
    bar_before: f32,
    gate_logged: bool,
}

#[derive(Clone, Debug)]
struct PState {
    downed: bool,
    hp: Option<u32>,
    max_hp: u32,
    latched: Option<u64>,
    line: String,
}

#[derive(Default)]
pub(crate) struct Tracker {
    states: HashMap<usize, PState>,
    next_ms: u64,
    local_downed: Option<bool>,
    local_addr: Option<usize>,
    local_reading: Option<Reading>,
}

impl Tracker {
    /// Sample every 100 ms; returns the transitions seen. Players that are not PlayerIns-derived objects are skipped.
    pub(crate) fn tick(
        &mut self,
        now_ms: u64,
        subjects: &[Subject],
        mem: &dyn CombatMem,
    ) -> Vec<Event> {
        if now_ms < self.next_ms {
            return Vec::new();
        }
        self.next_ms = now_ms + STATE_MS;
        self.states
            .retain(|a, _| subjects.iter().any(|s| s.addr == *a));
        let readings: Vec<(&Subject, Reading)> = subjects
            .iter()
            .filter(|s| mem.is_player(s.addr))
            .map(|s| (s, read_state(mem, s.addr)))
            .collect();
        let infos: Vec<PlayerInfo> = readings
            .iter()
            .map(|(s, r)| PlayerInfo {
                handle: r.handle,
                id: s.id,
                relation: s.relation,
            })
            .collect();
        self.local_downed = None;
        self.local_addr = None;
        self.local_reading = None;
        let mut out = Vec::new();
        for (s, r) in &readings {
            let Some(downed) = r.downed else { continue };
            if s.local {
                self.local_downed = Some(downed);
                self.local_addr = Some(s.addr);
                self.local_reading = Some(*r);
            }
            let max_seen = r.hp.unwrap_or(0);
            let line = format!(
                "{} {} downs={} hp={}{}",
                s.label,
                crate::overlay::shared::short_id(s.id),
                opt(r.downs),
                opt(r.hp),
                if downed { " DOWNED" } else { "" }
            );
            let ev = |kind, attacker| Event {
                label: s.label.clone(),
                id: s.id,
                addr: s.addr,
                local: s.local,
                kind,
                downed,
                downs: r.downs,
                hp: r.hp,
                attacker,
            };
            let Some(prev) = self.states.get_mut(&s.addr) else {
                self.states.insert(
                    s.addr,
                    PState {
                        downed,
                        hp: r.hp,
                        max_hp: max_seen,
                        latched: if downed {
                            r.attacker_raw.filter(|&h| valid_handle(h))
                        } else {
                            None
                        },
                        line,
                    },
                );
                out.push(ev(EventKind::Init, None));
                continue;
            };
            let cur_valid = r.attacker_raw.filter(|&h| valid_handle(h));
            let mut event = None;
            if !prev.downed && downed {
                // Fresh down: the field now names the hit that downed the player; forget any older latch.
                prev.latched = cur_valid;
                let att = s.local.then(|| (classify(cur_valid, &infos), false));
                event = Some(ev(EventKind::Down, att));
            } else if prev.downed && downed {
                if cur_valid.is_some() {
                    prev.latched = cur_valid;
                }
            } else if prev.downed && !downed {
                let (raw, latched) = match cur_valid {
                    Some(h) => (Some(h), false),
                    None => (prev.latched, prev.latched.is_some()),
                };
                let att = s.local.then(|| (classify(raw, &infos), latched));
                let kind = match r.hp {
                    Some(h) if h > 0 && h >= prev.max_hp => EventKind::Respawn,
                    _ => EventKind::Up,
                };
                event = Some(ev(kind, att));
                prev.latched = None;
            } else if prev.hp == Some(0) && r.hp.is_some_and(|h| h > 0) {
                let att = s.local.then(|| (classify(cur_valid, &infos), false));
                event = Some(ev(EventKind::HpRestored, att));
            }
            prev.downed = downed;
            prev.hp = r.hp;
            prev.max_hp = prev.max_hp.max(max_seen);
            prev.line = line;
            out.extend(event);
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------------
// Rival revive rule (T-035)
// ---------------------------------------------------------------------------------------------------

/// `rival_revive = "redown" | "vanilla"` (the player build always uses `Redown`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RevivePolicy {
    /// A rival completing the revive meter downs the player again at once (rule v1).
    #[default]
    Redown,
    /// No rule: revives work as in vanilla (dev comparison tests).
    Vanilla,
}

impl RevivePolicy {
    pub(crate) fn label(self) -> &'static str {
        match self {
            RevivePolicy::Redown => "redown",
            RevivePolicy::Vanilla => "vanilla",
        }
    }
}

/// `PlayerIns+0xB78` is a pointer; the authoritative HP is the u32 at `+0x70` of its target. VERIFIED on NR 1.3.3.0 by FS-0 run 9
/// (writing 0 there on the owner's client right after a revive downs the player again at once), TECHNICAL_NOTES section 4; the
/// pointee has no vtable, so it cannot be class-checked.
pub(crate) const OFF_HP_PTR: usize = 0xB78;
pub(crate) const HP_PTR_FIELD: usize = 0x70;
/// `PlayerIns+0xBA0` u32 mirrors the HP (FS-0 run 7). Used only to cross-check the value before the write.
pub(crate) const OFF_HP_MIRROR: usize = 0xBA0;

/// In the player build the rule's log lines are limited to one per this many milliseconds (the write itself is not limited).
pub(crate) const PLAYER_LOG_MS: u64 = 5000;

/// The write gate: the authoritative value must be non-zero and equal to the mirror or the PlayerGameData HP (at least one must
/// agree). Everything else means the offsets do not describe this build and nothing is written.
pub(crate) fn hp_gate(auth: Option<u32>, mirror: Option<u32>, gamedata: Option<u32>) -> bool {
    match auth {
        Some(a) if a > 0 => mirror == Some(a) || gamedata == Some(a),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------------
// Core: tracker + rules, driven by an explicit clock
// ---------------------------------------------------------------------------------------------------

pub(crate) struct Core {
    dev: bool,
    policy: RevivePolicy,
    tracker: Tracker,
    last_rule_log_ms: Option<u64>,
    suppressed: u32,
    /// T-039: seconds taken off the bleed-out clock per rival hit.
    hit_seconds: f32,
    dn: Option<DownedMem>,
    downed_line: Option<String>,
    /// Dev: last logged (state byte, revive bar) per tracked player, see [`Core::watch_block`].
    block_watch: HashMap<usize, (u8, f32)>,
    /// Downed solo rivals (remote copies): the bar value before any hit, see [`Core::remote_step`].
    remote_bars: HashMap<usize, f32>,
    /// Try the re-down as soon as a rival's hit starts LOCAL's get-up (state 2) instead of waiting for state 0, see
    /// [`Core::getup_step`]. EXPERIMENT, off: the effect of the store during state 2 is UNKNOWN (FS-0 run 20 never reached a get-up).
    pub(crate) early_redown: bool,
    /// Nothing (more) to do for a get-up: no down was seen before it, the store was made, or the reviver is not a rival.
    getup_done: bool,
    getup_gate_logged: bool,
    last_dn_log_ms: Option<u64>,
    /// Rule writes made this session.
    pub(crate) fired: u32,
}

impl Core {
    pub(crate) fn new(dev: bool, policy: RevivePolicy) -> Self {
        Core {
            dev,
            policy: if dev { policy } else { RevivePolicy::Redown },
            tracker: Tracker::default(),
            last_rule_log_ms: None,
            suppressed: 0,
            hit_seconds: DEFAULT_RIVAL_HIT_SECONDS,
            dn: None,
            downed_line: None,
            block_watch: HashMap::new(),
            remote_bars: HashMap::new(),
            early_redown: false,
            getup_done: true,
            getup_gate_logged: false,
            last_dn_log_ms: None,
            fired: 0,
        }
    }

    /// T-039: `rival_hit_seconds` (clamped again here, so a bad value can never reach the clock).
    pub(crate) fn set_hit_seconds(&mut self, secs: f32) {
        self.hit_seconds = if secs.is_finite() {
            secs.clamp(RIVAL_HIT_SECONDS_RANGE.0, RIVAL_HIT_SECONDS_RANGE.1)
        } else {
            DEFAULT_RIVAL_HIT_SECONDS
        };
    }

    /// T-039: the dev status line for LOCAL while downed (tests).
    #[cfg(test)]
    pub(crate) fn downed_line(&self) -> Option<&str> {
        self.downed_line.as_deref()
    }

    pub(crate) fn policy(&self) -> RevivePolicy {
        self.policy
    }

    /// One pass; returns the log lines.
    pub(crate) fn step(
        &mut self,
        now_ms: u64,
        subjects: &[Subject],
        mem: &mut dyn CombatMem,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        let events = self.tracker.tick(now_ms, subjects, mem);
        if self.dev {
            lines.extend(events.iter().map(Event::line));
        }
        for ev in &events {
            if rule_should_fire(self.policy, ev) {
                lines.extend(self.apply_rule(now_ms, ev.addr, mem));
            }
        }
        lines.extend(self.downed_step(now_ms, subjects, mem));
        lines.extend(self.remote_step(subjects, mem));
        if self.dev {
            lines.extend(self.watch_block(subjects, mem));
        }
        lines
    }

    /// T-039, every call (the main loop's 50 ms tick), LOCAL only. While LOCAL's state byte is 1 it remembers the revive bar; when
    /// the bar drops (a hit landed) and the attacker is a RIVAL, the bar is put back, the bleed-out clock loses `hit_seconds` and
    /// the pause timer is cleared. Other attackers: nothing. If a rival's hit completes the bar before we react, the state byte
    /// leaves 1 and rule v1 (the UP transition) is the safety net.
    fn downed_step(
        &mut self,
        now_ms: u64,
        subjects: &[Subject],
        mem: &mut dyn CombatMem,
    ) -> Vec<String> {
        self.downed_line = None;
        let Some(addr) = subjects
            .iter()
            .find(|s| s.local && mem.is_player(s.addr))
            .map(|s| s.addr)
        else {
            self.dn = None;
            return Vec::new();
        };
        let state = mem.read_u8(addr + OFF_DOWNED);
        // A get-up is only acted on when it follows a down that was seen here.
        if state != Some(2) {
            self.getup_done = state != Some(1);
            self.getup_gate_logged = false;
        }
        if state != Some(1) {
            self.dn = None;
            return if state == Some(2) {
                self.getup_step(now_ms, addr, subjects, mem)
            } else {
                Vec::new()
            };
        }
        let (Some(bar), Some(clock), Some(pause)) = (
            read_f32(mem, addr + OFF_REVIVE_BAR),
            read_f32(mem, addr + OFF_BLEED_CLOCK),
            read_f32(mem, addr + OFF_PAUSE),
        ) else {
            return Vec::new();
        };
        if self.dev {
            self.downed_line = Some(format!(
                "downed: bar {bar:.1} left, clock {clock:.1} s, pause {pause:.1}"
            ));
        }
        if !bar.is_finite() {
            return Vec::new();
        }
        let mut st = match self.dn {
            Some(d) if d.addr == addr => d,
            _ => DownedMem {
                addr,
                bar_before: bar,
                gate_logged: false,
            },
        };
        let mut lines = Vec::new();
        if st.bar_before - bar > BAR_HIT_EPS {
            // A hit landed since the last tick.
            let infos = player_infos(mem, subjects);
            let raw = [OFF_LAST_ATTACKER, OFF_CUR_ATTACKER]
                .into_iter()
                .filter_map(|o| mem.read_u64(addr + o))
                .find(|&h| valid_handle(h));
            if classify(raw, &infos).kind == AttackerKind::Rival {
                match self.restore_after_rival_hit(addr, st.bar_before, bar, clock, pause, mem) {
                    // Keep bar_before: several rival hits never accumulate on the bar.
                    Ok(new_clock) => {
                        if self.may_log_downed(now_ms) {
                            lines.push(format!(
                                "rule: rival hit on downed LOCAL: bar {bar} -> {} restored, clock {clock} -> {new_clock}, pause {pause} -> 0",
                                st.bar_before
                            ));
                        }
                    }
                    Err(why) => {
                        if !st.gate_logged {
                            st.gate_logged = true;
                            lines.push(format!(
                                "rule: rival hit on downed LOCAL but nothing was written: {why}"
                            ));
                        }
                        st.bar_before = bar;
                    }
                }
            } else {
                st.bar_before = bar;
            }
        } else {
            st.bar_before = bar;
        }
        self.dn = Some(st);
        lines
    }

    /// LOCAL is getting up (state byte 2, about 3.5 s) because the revive bar was completed. If the last attacker is a RIVAL (one
    /// hit removed the whole bar before the downed rule could undo it; seen in FS-0 runs 18 and 19), make rule v1's store now
    /// instead of at state 0, through the same gates ([`Core::try_rule`]). Retried every call while the HP gate fails; rule v1 at
    /// the UP transition stays as the fallback. EXPERIMENT: what the game does with health 0 during state 2 is UNKNOWN.
    fn getup_step(
        &mut self,
        now_ms: u64,
        addr: usize,
        subjects: &[Subject],
        mem: &mut dyn CombatMem,
    ) -> Vec<String> {
        if !self.early_redown || self.getup_done || self.policy != RevivePolicy::Redown {
            return Vec::new();
        }
        let infos = player_infos(mem, subjects);
        let raw = [OFF_LAST_ATTACKER, OFF_CUR_ATTACKER]
            .into_iter()
            .filter_map(|o| mem.read_u64(addr + o))
            .find(|&h| valid_handle(h));
        if classify(raw, &infos).kind != AttackerKind::Rival {
            self.getup_done = true;
            return Vec::new();
        }
        match self.try_rule(addr, mem) {
            Ok((before, back)) => {
                self.getup_done = true;
                if self.may_log(now_ms) {
                    vec![format!(
                        "rule: rival completed the revive meter of LOCAL; health set to 0 during the get-up (hp {before} -> 0, readback {})",
                        opt(back)
                    )]
                } else {
                    Vec::new()
                }
            }
            Err(why) => {
                if self.dev && !self.getup_gate_logged {
                    self.getup_gate_logged = true;
                    vec![format!(
                        "rule: LOCAL is getting up after a rival's hit; nothing written yet: {why}"
                    )]
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// The display half of the downed rule, every call. A remote copy holds the same block as the owner (+0x710 state, +0x714
    /// bar, +0x718 clock, +0x71C pause) but it is only refreshed when a hit lands, with the owner's values at that moment
    /// [VERIFIED on NR 1.3.3.0 by FS-0 run 18, re/MEMORY_MAP.md section 6]. The owner's client then undoes a rival's hit, which
    /// this copy never hears about, so the gauge on this screen kept the first hit. Here the same three stores are made on the
    /// copy. A copy does not say who hit (its +0x1A0 is not valid), so this is limited to a downed RIVAL with no teammate in the
    /// session: every hit on such a player comes from a rival of theirs.
    fn remote_step(&mut self, subjects: &[Subject], mem: &mut dyn CombatMem) -> Vec<String> {
        let solo_rival = |s: &Subject| {
            !s.local
                && s.relation == Some(Relation::Rival)
                && s.team_no.is_some()
                && !subjects
                    .iter()
                    .any(|o| o.addr != s.addr && o.team_no == s.team_no)
        };
        let mut seen = HashSet::new();
        let mut lines = Vec::new();
        for s in subjects.iter().filter(|s| solo_rival(s)) {
            if !mem.is_player(s.addr) || mem.read_u8(s.addr + OFF_DOWNED) != Some(1) {
                continue;
            }
            let (Some(bar), Some(clock), Some(pause)) = (
                read_f32(mem, s.addr + OFF_REVIVE_BAR),
                read_f32(mem, s.addr + OFF_BLEED_CLOCK),
                read_f32(mem, s.addr + OFF_PAUSE),
            ) else {
                continue;
            };
            if !bar.is_finite() {
                continue;
            }
            seen.insert(s.addr);
            let before = *self.remote_bars.entry(s.addr).or_insert(bar);
            if before - bar <= BAR_HIT_EPS {
                // Never lowered: the value at the start of the down stays the reference.
                self.remote_bars.insert(s.addr, before.max(bar));
                continue;
            }
            match self.restore_after_rival_hit(s.addr, before, bar, clock, pause, mem) {
                Ok(new_clock) => {
                    if self.dev {
                        lines.push(format!(
                            "rule: rival hit on downed {} (display copy): bar {bar} -> {before} restored, clock {clock} -> {new_clock}, pause {pause} -> 0",
                            s.label
                        ));
                    }
                }
                Err(why) => {
                    if self.dev {
                        lines.push(format!(
                            "rule: rival hit on downed {} (display copy) but nothing was written: {why}",
                            s.label
                        ));
                    }
                    self.remote_bars.insert(s.addr, bar);
                }
            }
        }
        self.remote_bars.retain(|a, _| seen.contains(a));
        lines
    }

    /// Dev, read-only, every call: one line whenever a tracked player's state byte or revive bar changes while the block is in
    /// use, for LOCAL (read after the rule, so a rival hit that was undone does not show) and for the remote copies. Purpose:
    /// find out what the copy of a downed rival holds on the attacker's client (UNKNOWN as of 2026-10-12).
    fn watch_block(&mut self, subjects: &[Subject], mem: &dyn CombatMem) -> Vec<String> {
        self.block_watch
            .retain(|a, _| subjects.iter().any(|s| s.addr == *a));
        let mut lines = Vec::new();
        for s in subjects.iter().filter(|s| mem.is_player(s.addr)) {
            let (Some(state), Some(bar), Some(clock), Some(pause)) = (
                mem.read_u8(s.addr + OFF_DOWNED),
                read_f32(mem, s.addr + OFF_REVIVE_BAR),
                read_f32(mem, s.addr + OFF_BLEED_CLOCK),
                read_f32(mem, s.addr + OFF_PAUSE),
            ) else {
                continue;
            };
            let prev = self.block_watch.insert(s.addr, (state, bar));
            let changed = match prev {
                Some((ps, pb)) => {
                    ps != state || (pb - bar).abs() > BAR_HIT_EPS || pb.is_nan() != bar.is_nan()
                }
                // First sight: only worth a line if the block is in use.
                None => state != 0,
            };
            if changed {
                let was = prev.map_or("?".to_string(), |(ps, pb)| format!("state {ps}, bar {pb}"));
                lines.push(format!(
                    "T-039 BLOCK who={} {was} -> state {state}, bar {bar} (clock {clock}, pause {pause})",
                    s.label
                ));
            }
        }
        lines
    }

    /// The gated stores of the downed rule; `Ok(new clock)`. Gates right before the writes: PlayerIns vtable, state byte still 1,
    /// finite values, bar in 0..=1000, clock in 0..=60.
    fn restore_after_rival_hit(
        &mut self,
        addr: usize,
        bar_before: f32,
        bar: f32,
        clock: f32,
        pause: f32,
        mem: &mut dyn CombatMem,
    ) -> Result<f32, String> {
        if !mem.is_player(addr) {
            return Err(format!("0x{addr:X} no longer has a PlayerIns vtable"));
        }
        if mem.read_u8(addr + OFF_DOWNED) != Some(1) {
            return Err("state byte is no longer 1".to_string());
        }
        if !(bar.is_finite() && clock.is_finite() && pause.is_finite() && bar_before.is_finite()) {
            return Err("a value is not finite".to_string());
        }
        if !(0.0..=1000.0).contains(&bar_before) {
            return Err(format!("bar {bar_before} outside 0..=1000"));
        }
        if !(0.0..=60.0).contains(&clock) {
            return Err(format!("clock {clock} outside 0..=60"));
        }
        let new_clock = (clock - self.hit_seconds).max(CLOCK_FLOOR);
        for (off, v) in [
            (OFF_REVIVE_BAR, bar_before),
            (OFF_BLEED_CLOCK, new_clock),
            (OFF_PAUSE, 0.0),
        ] {
            mem.write(addr + off, &v.to_le_bytes())
                .map_err(|e| format!("write refused: {e}"))?;
        }
        self.fired += 1;
        Ok(new_clock)
    }

    /// Player build: at most one downed-rule line per [`PLAYER_LOG_MS`]; dev mode logs every one.
    fn may_log_downed(&mut self, now_ms: u64) -> bool {
        if self.dev
            || self
                .last_dn_log_ms
                .is_none_or(|t| now_ms.saturating_sub(t) >= PLAYER_LOG_MS)
        {
            self.last_dn_log_ms = Some(now_ms);
            true
        } else {
            false
        }
    }

    /// Player build: at most one rule line per [`PLAYER_LOG_MS`]; dev mode logs every one.
    fn may_log(&mut self, now_ms: u64) -> bool {
        if self.dev
            || self
                .last_rule_log_ms
                .is_none_or(|t| now_ms.saturating_sub(t) >= PLAYER_LOG_MS)
        {
            self.last_rule_log_ms = Some(now_ms);
            true
        } else {
            self.suppressed += 1;
            false
        }
    }

    /// Rule v1: u32 0 into `[PlayerIns+0xB78]+0x70` of LOCAL (the caller passes LOCAL's PlayerIns after the RIVAL/UP/hp checks).
    /// Offsets VERIFIED on NR 1.3.3.0 by FS-0 run 9, TECHNICAL_NOTES section 4. Gates here: PlayerIns vtable, plausible readable
    /// `[PlayerIns+0xB78]`, value > 0 agreeing with the mirror or the PlayerGameData HP. Anything else: skip and log.
    fn apply_rule(&mut self, now_ms: u64, addr: usize, mem: &mut dyn CombatMem) -> Vec<String> {
        let outcome = self.try_rule(addr, mem);
        if !self.may_log(now_ms) {
            return Vec::new();
        }
        let dropped = std::mem::take(&mut self.suppressed);
        let note = if dropped > 0 {
            format!(" ({dropped} earlier line(s) suppressed)")
        } else {
            String::new()
        };
        vec![match outcome {
            Ok((before, back)) => format!(
                "rule: rival completed the revive meter of LOCAL; downed again (hp {before} -> 0, readback {}){note}",
                opt(back)
            ),
            Err(why) => format!(
                "rule: rival completed the revive meter of LOCAL but nothing was written: {why}{note}"
            ),
        }]
    }

    /// The gated write; `Ok((hp before, readback))`.
    fn try_rule(
        &mut self,
        addr: usize,
        mem: &mut dyn CombatMem,
    ) -> Result<(u32, Option<u32>), String> {
        if !mem.is_player(addr) {
            return Err(format!("0x{addr:X} no longer has a PlayerIns vtable"));
        }
        let p = mem
            .read_u64(addr + OFF_HP_PTR)
            .and_then(|v| usize::try_from(v).ok())
            .filter(|&v| is_ptr(v))
            .ok_or_else(|| format!("[PlayerIns+0x{OFF_HP_PTR:X}] is not a plausible pointer"))?;
        let target = p + HP_PTR_FIELD;
        let auth = mem.read_u32(target);
        let mirror = mem.read_u32(addr + OFF_HP_MIRROR);
        let gd = gamedata_of(mem, addr).and_then(|g| mem.read_u32(g + GD_HP));
        if !hp_gate(auth, mirror, gd) {
            return Err(format!(
                "hp gate failed (authoritative {}, mirror {}, PlayerGameData {})",
                opt(auth),
                opt(mirror),
                opt(gd)
            ));
        }
        let before = auth.unwrap_or(0);
        mem.write(target, &0u32.to_le_bytes())
            .map_err(|e| format!("write refused: {e}"))?;
        self.fired += 1;
        Ok((before, mem.read_u32(target)))
    }
}

// ---------------------------------------------------------------------------------------------------
// Controller used by the main loop
// ---------------------------------------------------------------------------------------------------

pub(crate) struct CombatCtl {
    core: Core,
    mem: RealCombatMem,
    rtti_ready: bool,
    base: usize,
    started: std::time::Instant,
    subjects: Vec<Subject>,
}

impl CombatCtl {
    /// Both modes.
    pub(crate) fn new(cfg: &Config, base: usize) -> Self {
        let started = std::time::Instant::now();
        let mut core = Core::new(cfg.dev_mode, cfg.revive_policy());
        core.set_hit_seconds(cfg.rival_hit_seconds);
        tracing::info!(
            "T-035: combat-state tracker on (100 ms); T-039 downed rule on (50 ms, rival_hit_seconds {}); rival revive rule: {}{}",
            cfg.rival_hit_seconds,
            core.policy().label(),
            if cfg.dev_mode {
                ""
            } else {
                " (player build: always on)"
            }
        );
        CombatCtl {
            core,
            mem: RealCombatMem::default(),
            rtti_ready: false,
            base,
            started,
            subjects: Vec::new(),
        }
    }

    pub(crate) fn set_rtti(&mut self, rtti: &Rtti) {
        if self.mem.set_rtti(rtti, self.base) {
            self.rtti_ready = true;
            tracing::info!(
                "T-033: RTTI ready: {} PlayerIns vtable(s), {} {GAMEDATA_CLASS} vtable(s)",
                self.mem.player_vts.len(),
                self.mem.gd_vts.len()
            );
        }
    }

    pub(crate) fn tick(&mut self, subjects: Vec<Subject>) {
        self.subjects = subjects;
        if !self.rtti_ready {
            return;
        }
        let now_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        for l in self.core.step(now_ms, &self.subjects, &mut self.mem) {
            tracing::info!("{l}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P0: usize = 0x1000_0000; // LOCAL
    const P1: usize = 0x1001_0000; // rival
    const P2: usize = 0x1002_0000; // ally
    const G0: usize = 0x2000_0000;
    const G1: usize = 0x2001_0000;
    const H0: u64 = 0xFFFF_FFFF_10C0_0000;
    const H1: u64 = 0xFFFF_FFFF_10C0_0001;
    const H2: u64 = 0xFFFF_FFFF_10C0_0002;
    /// Offset of the fake `[PlayerIns+0xB78]` block from the game-data address.
    const BLK: usize = 0x4000;

    #[derive(Default)]
    struct Fake {
        bytes: HashMap<usize, u8>,
        players: HashSet<usize>,
        gds: HashSet<usize>,
        writes: Vec<(usize, Vec<u8>)>,
        refuse_write: bool,
        /// Addresses whose next N stores are accepted but do not change the value (the game puts the value back).
        no_take: HashMap<usize, u32>,
    }

    impl Fake {
        fn set(&mut self, addr: usize, data: &[u8]) {
            for (i, b) in data.iter().enumerate() {
                self.bytes.insert(addr + i, *b);
            }
        }
        fn get(&self, addr: usize, n: usize) -> Option<Vec<u8>> {
            (0..n)
                .map(|i| self.bytes.get(&(addr + i)).copied())
                .collect()
        }
        /// A standing player with handle `h`, game data at `g`.
        fn player(&mut self, p: usize, g: usize, h: u64, hp: u32) {
            self.players.insert(p);
            self.gds.insert(g);
            self.set(p + OFF_DOWNED, &[0]);
            self.set(p + OFF_DOWNS, &[0]);
            self.set(p + CHR_HANDLE, &h.to_le_bytes());
            self.set(p + OFF_LAST_ATTACKER, &u64::MAX.to_le_bytes());
            self.set(p + OFF_GAMEDATA, &(g as u64).to_le_bytes());
            self.set(p + OFF_HP_PTR, &((g + BLK) as u64).to_le_bytes());
            self.hp3(p, g, hp);
        }
        /// All three HP locations of the player at `p` (game data `g`).
        fn hp3(&mut self, p: usize, g: usize, hp: u32) {
            self.set(g + GD_HP, &hp.to_le_bytes());
            self.set(p + OFF_HP_MIRROR, &hp.to_le_bytes());
            self.set(g + BLK + HP_PTR_FIELD, &hp.to_le_bytes());
        }
        fn downed(&mut self, p: usize, downs: u8, v: bool) {
            self.set(p + OFF_DOWNED, &[u8::from(v)]);
            self.set(p + OFF_DOWNS, &[downs]);
        }
        fn attacker(&mut self, p: usize, h: u64) {
            self.set(p + OFF_LAST_ATTACKER, &h.to_le_bytes());
        }
        fn hp(&mut self, g: usize, hp: u32) {
            self.set(g + GD_HP, &hp.to_le_bytes());
        }
    }

    impl CombatMem for Fake {
        fn is_player(&self, a: usize) -> bool {
            self.players.contains(&a)
        }
        fn is_gamedata(&self, a: usize) -> bool {
            self.gds.contains(&a)
        }
        fn read_u8(&self, a: usize) -> Option<u8> {
            self.get(a, 1).map(|v| v[0])
        }
        fn read_u32(&self, a: usize) -> Option<u32> {
            self.get(a, 4)
                .map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
        }
        fn read_u64(&self, a: usize) -> Option<u64> {
            self.get(a, 8)
                .map(|v| u64::from_le_bytes(v.try_into().unwrap_or([0; 8])))
        }
        fn write(&mut self, a: usize, d: &[u8]) -> Result<(), &'static str> {
            if self.refuse_write {
                return Err("refused by test");
            }
            self.writes.push((a, d.to_vec()));
            if let Some(n) = self.no_take.get_mut(&a).filter(|n| **n > 0) {
                *n -= 1;
            } else {
                self.set(a, d);
            }
            Ok(())
        }
    }

    fn subjects() -> Vec<Subject> {
        vec![
            Subject {
                addr: P0,
                label: "LOCAL[0]".into(),
                local: true,
                id: Some(0x6001),
                relation: Some(Relation::Local),
                team_no: Some(0),
            },
            Subject {
                addr: P1,
                label: "REMOTE[1]".into(),
                local: false,
                id: Some(0x6002),
                relation: Some(Relation::Rival),
                team_no: Some(1),
            },
            Subject {
                addr: P2,
                label: "REMOTE[2]".into(),
                local: false,
                id: Some(0x6003),
                relation: Some(Relation::Ally),
                team_no: Some(0),
            },
        ]
    }

    fn world() -> Fake {
        let mut f = Fake::default();
        f.player(P0, G0, H0, 240);
        f.player(P1, G1, H1, 240);
        f.player(P2, 0x2002_0000, H2, 240);
        f
    }

    #[test]
    fn classification_covers_every_kind_and_missing_data() {
        let infos = [
            PlayerInfo {
                handle: Some(H0),
                id: Some(1),
                relation: Some(Relation::Local),
            },
            PlayerInfo {
                handle: Some(H1),
                id: Some(2),
                relation: Some(Relation::Rival),
            },
            PlayerInfo {
                handle: Some(H2),
                id: Some(3),
                relation: Some(Relation::Ally),
            },
            PlayerInfo {
                handle: None,
                id: Some(4),
                relation: Some(Relation::Rival),
            },
            PlayerInfo {
                handle: Some(0x1234),
                id: Some(5),
                relation: None,
            },
        ];
        let k = |raw| classify(raw, &infos).kind;
        assert_eq!(k(Some(H0)), AttackerKind::SelfHit);
        assert_eq!(k(Some(H1)), AttackerKind::Rival);
        assert_eq!(classify(Some(H1), &infos).id, Some(2));
        assert_eq!(k(Some(H2)), AttackerKind::Ally);
        // Unknown handle (a PvE enemy), none, all-ones, zero, and a player with no roster relation: all OTHER.
        assert_eq!(k(Some(0xDEAD_BEEF)), AttackerKind::Other);
        assert_eq!(k(None), AttackerKind::Other);
        assert_eq!(k(Some(u64::MAX)), AttackerKind::Other);
        assert_eq!(k(Some(0)), AttackerKind::Other);
        assert_eq!(k(Some(0x1234)), AttackerKind::Other);
        // No players at all (rosters missing).
        assert_eq!(classify(Some(H1), &[]).kind, AttackerKind::Other);
        assert!(classify(Some(0xDEAD_BEEF), &infos)
            .text()
            .contains("not a player"));
        assert!(classify(Some(H1), &infos)
            .text()
            .starts_with("RIVAL id=0x2"));
    }

    #[test]
    fn transitions_down_up_and_respawn_are_detected_and_logged_once() {
        // Tracker only: the rule is off (its own tests are below).
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Vanilla);
        // First tick seeds.
        let l = c.step(0, &subjects(), &mut f);
        assert_eq!(l.iter().filter(|x| x.contains("INIT")).count(), 3, "{l:?}");
        // Nothing changes: nothing logged.
        assert!(c.step(100, &subjects(), &mut f).is_empty());
        // The rival downs LOCAL.
        f.downed(P0, 1, true);
        f.hp(G0, 0);
        f.attacker(P0, H1);
        let l = c.step(200, &subjects(), &mut f);
        assert_eq!(l.len(), 1, "{l:?}");
        assert!(l[0]
            .contains("who=LOCAL[0] id=0x6001 DOWN (downs=1, hp=0, last_attacker=RIVAL id=0x6002"));
        // Same state again (and a tick inside the 100 ms gate): no new line.
        assert!(c.step(250, &subjects(), &mut f).is_empty());
        assert!(c.step(300, &subjects(), &mut f).is_empty());
        // Revived at 50% HP, the field has been cleared: the latch supplies the attacker.
        f.downed(P0, 1, false);
        f.hp(G0, 120);
        f.attacker(P0, u64::MAX);
        let l = c.step(400, &subjects(), &mut f);
        assert_eq!(l.len(), 1);
        assert!(
            l[0].contains("UP (downs=1, hp=120, last_attacker=RIVAL"),
            "{l:?}"
        );
        assert!(l[0].contains("(latched)"));
        assert!(c.step(500, &subjects(), &mut f).is_empty());
        // Down again, bleed out: HP back to the maximum means RESPAWN.
        f.downed(P0, 2, true);
        f.hp(G0, 0);
        f.attacker(P0, 0xBEEF);
        let l = c.step(600, &subjects(), &mut f);
        assert!(
            l[0].contains("DOWN (downs=2, hp=0, last_attacker=OTHER handle=0xBEEF"),
            "{l:?}"
        );
        f.downed(P0, 2, false);
        f.hp(G0, 240);
        let l = c.step(700, &subjects(), &mut f);
        assert!(l[0].contains("RESPAWN (downs=2, hp=240"), "{l:?}");
        // A remote copy: no attacker is claimed.
        f.downed(P1, 1, true);
        f.hp(0x2001_0000, 0);
        let l = c.step(800, &subjects(), &mut f);
        assert!(
            l[0].contains("who=REMOTE[1] id=0x6002 DOWN") && l[0].contains("n/a (remote copy)"),
            "{l:?}"
        );
        // HP restored without a downed change.
        f.hp(G0, 100);
        f.hp(G0, 0);
        c.step(900, &subjects(), &mut f);
        f.hp(G0, 120);
        let l = c.step(1000, &subjects(), &mut f);
        assert!(l[0].contains("HP restored (downs=2, hp=120"), "{l:?}");
    }

    #[test]
    fn a_player_whose_class_check_fails_is_skipped_and_hp_needs_the_class() {
        let mut f = world();
        // PlayerGameData pointer leads to something that is not the class: HP unknown, no crash.
        f.gds.remove(&G0);
        let r = read_state(&f, P0);
        assert_eq!(r.hp, None);
        assert_eq!(r.downed, Some(false));
        // Not a PlayerIns at all: skipped entirely.
        f.players.remove(&P2);
        let mut c = Core::new(true, RevivePolicy::Redown);
        let l = c.step(0, &subjects(), &mut f);
        assert_eq!(l.iter().filter(|x| x.contains("INIT")).count(), 2, "{l:?}");
    }

    // ---- T-035: rule v1 ----

    /// Address of the authoritative HP of the player whose game data is `g`.
    fn auth_addr(g: usize) -> usize {
        g + BLK + HP_PTR_FIELD
    }

    /// LOCAL is downed by `attacker` at t=100 and revived (HP `hp_after` in all three places) at t=200. Returns the lines of the
    /// UP tick.
    fn drive_revive(f: &mut Fake, c: &mut Core, attacker: u64, hp_after: u32) -> Vec<String> {
        c.step(0, &subjects(), f);
        f.downed(P0, 1, true);
        f.hp3(P0, G0, 0);
        f.attacker(P0, attacker);
        c.step(100, &subjects(), f);
        f.downed(P0, 1, false);
        f.hp3(P0, G0, hp_after);
        f.attacker(P0, u64::MAX);
        c.step(200, &subjects(), f)
    }

    fn rule_lines(l: &[String]) -> Vec<&String> {
        l.iter().filter(|x| x.starts_with("rule:")).collect()
    }

    #[test]
    fn rule_fires_once_for_local_up_with_hp_and_a_rival_in_both_modes() {
        for dev in [true, false] {
            let mut f = world();
            let mut c = Core::new(dev, RevivePolicy::Redown);
            let l = drive_revive(&mut f, &mut c, H1, 120);
            // One store, four zero bytes, to the authoritative HP only.
            assert_eq!(
                f.writes,
                vec![(auth_addr(G0), vec![0, 0, 0, 0])],
                "dev={dev}"
            );
            assert_eq!(c.fired, 1);
            let r = rule_lines(&l);
            assert_eq!(r.len(), 1, "{l:?}");
            assert_eq!(
                r[0],
                "rule: rival completed the revive meter of LOCAL; downed again (hp 120 -> 0, readback 0)"
            );
            // The game downs the player again and later the player is revived by the rival again: no extra write from the
            // same UP, and the next UP is a separate event.
            c.step(300, &subjects(), &mut f);
            c.step(400, &subjects(), &mut f);
            assert_eq!(f.writes.len(), 1, "once per UP");
            f.downed(P0, 2, true);
            f.hp3(P0, G0, 0);
            f.attacker(P0, H1);
            c.step(500, &subjects(), &mut f);
            f.downed(P0, 2, false);
            f.hp3(P0, G0, 90);
            f.attacker(P0, u64::MAX);
            let l = c.step(600, &subjects(), &mut f);
            assert_eq!(f.writes.len(), 2);
            // The player build limits the log to one rule line per 5 s; dev mode logs every one.
            assert_eq!(rule_lines(&l).len(), usize::from(dev), "{l:?}");
        }
    }

    #[test]
    fn rule_never_fires_for_ally_self_other_respawn_zero_hp_or_remote_players() {
        for dev in [true, false] {
            for att in [H2, H0, 0xDEAD_BEEF, u64::MAX] {
                let mut f = world();
                let mut c = Core::new(dev, RevivePolicy::Redown);
                let l = drive_revive(&mut f, &mut c, att, 120);
                assert!(f.writes.is_empty(), "dev={dev} attacker {att:#X}");
                assert!(rule_lines(&l).is_empty(), "{l:?}");
            }
            // A respawn (HP back to the maximum) is not a revive.
            let mut f = world();
            let mut c = Core::new(dev, RevivePolicy::Redown);
            drive_revive(&mut f, &mut c, H1, 240);
            assert!(f.writes.is_empty(), "dev={dev} respawn");
            // A revive that left HP at 0 is not eligible either.
            let mut f = world();
            let mut c = Core::new(dev, RevivePolicy::Redown);
            drive_revive(&mut f, &mut c, H1, 0);
            assert!(f.writes.is_empty(), "dev={dev} hp 0");
            // A remote player's own down and revive never writes, whoever the attacker was.
            let mut f = world();
            let mut c = Core::new(dev, RevivePolicy::Redown);
            c.step(0, &subjects(), &mut f);
            f.downed(P1, 1, true);
            f.hp3(P1, G1, 0);
            f.attacker(P1, H0);
            c.step(100, &subjects(), &mut f);
            f.downed(P1, 1, false);
            f.hp3(P1, G1, 120);
            c.step(200, &subjects(), &mut f);
            f.downed(P2, 1, true);
            f.hp3(P2, 0x2002_0000, 0);
            c.step(300, &subjects(), &mut f);
            f.downed(P2, 1, false);
            f.hp3(P2, 0x2002_0000, 120);
            c.step(400, &subjects(), &mut f);
            assert!(f.writes.is_empty(), "dev={dev} remote");
        }
    }

    #[test]
    fn rival_revive_vanilla_disables_the_rule_in_dev_mode_only() {
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Vanilla);
        let l = drive_revive(&mut f, &mut c, H1, 120);
        assert!(f.writes.is_empty());
        assert!(rule_lines(&l).is_empty());
        // The tracker still reports the UP in dev mode.
        assert!(l.iter().any(|x| x.contains("UP (downs=1")), "{l:?}");
        // The player build cannot be switched off, whatever the policy it is given.
        let mut f = world();
        let mut c = Core::new(false, RevivePolicy::Vanilla);
        assert_eq!(c.policy(), RevivePolicy::Redown);
        drive_revive(&mut f, &mut c, H1, 120);
        assert_eq!(f.writes.len(), 1);
        // The gate function itself.
        let ev = Event {
            label: "LOCAL[0]".into(),
            id: None,
            addr: P0,
            local: true,
            kind: EventKind::Up,
            downed: false,
            downs: Some(1),
            hp: Some(120),
            attacker: Some((
                Attacker {
                    kind: AttackerKind::Rival,
                    handle: Some(H1),
                    id: None,
                },
                false,
            )),
        };
        assert!(rule_should_fire(RevivePolicy::Redown, &ev));
        assert!(!rule_should_fire(RevivePolicy::Vanilla, &ev));
        let mut remote = ev.clone();
        remote.local = false;
        assert!(!rule_should_fire(RevivePolicy::Redown, &remote));
    }

    #[test]
    fn hp_gate_needs_a_positive_value_that_the_mirror_or_game_data_confirms() {
        assert!(hp_gate(Some(120), Some(120), Some(120)));
        assert!(hp_gate(Some(120), Some(120), None));
        assert!(hp_gate(Some(120), None, Some(120)));
        assert!(hp_gate(Some(120), Some(7), Some(120)));
        assert!(hp_gate(Some(120), Some(120), Some(7)));
        assert!(!hp_gate(Some(120), Some(7), Some(8)));
        assert!(!hp_gate(Some(120), None, None));
        assert!(!hp_gate(Some(0), Some(0), Some(0)));
        assert!(!hp_gate(None, Some(120), Some(120)));
    }

    #[test]
    fn rule_skips_and_logs_when_a_gate_fails() {
        // Mirror and game data both disagree with the authoritative value.
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        c.step(0, &subjects(), &mut f);
        f.downed(P0, 1, true);
        f.hp3(P0, G0, 0);
        f.attacker(P0, H1);
        c.step(100, &subjects(), &mut f);
        f.downed(P0, 1, false);
        f.hp3(P0, G0, 120);
        f.set(auth_addr(G0), &77u32.to_le_bytes());
        f.attacker(P0, u64::MAX);
        let l = c.step(200, &subjects(), &mut f);
        assert!(f.writes.is_empty());
        let r = rule_lines(&l);
        assert_eq!(r.len(), 1, "{l:?}");
        assert!(
            r[0].contains("nothing was written") && r[0].contains("authoritative 77"),
            "{r:?}"
        );
        // Without a class-checked PlayerGameData the tracker has no HP at all.
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        f.gds.remove(&G0); // PlayerGameData cannot be class-checked any more
        drive_revive(&mut f, &mut c, H1, 120);
        assert!(f.writes.is_empty());
        // Authoritative value already 0 at the time of the UP.
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        c.step(0, &subjects(), &mut f);
        f.downed(P0, 1, true);
        f.hp3(P0, G0, 0);
        f.attacker(P0, H1);
        c.step(100, &subjects(), &mut f);
        f.downed(P0, 1, false);
        f.hp(G0, 120); // only the PlayerGameData copy shows the new HP
        f.attacker(P0, u64::MAX);
        let l = c.step(200, &subjects(), &mut f);
        assert!(
            f.writes.is_empty(),
            "authoritative HP is 0: nothing to write"
        );
        assert!(rule_lines(&l)[0].contains("authoritative 0"), "{l:?}");
        // Null / implausible pointer at +0xB78.
        for bad in [0u64, 0x1234_5679, u64::MAX] {
            let mut f = world();
            let mut c = Core::new(true, RevivePolicy::Redown);
            f.set(P0 + OFF_HP_PTR, &bad.to_le_bytes());
            let l = drive_revive(&mut f, &mut c, H1, 120);
            assert!(f.writes.is_empty(), "pointer {bad:#X}");
            assert!(
                rule_lines(&l)[0].contains("not a plausible pointer"),
                "{l:?}"
            );
        }
        // Unreadable authoritative HP (pointer plausible but nothing mapped there).
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        f.set(P0 + OFF_HP_PTR, &0x7000_0000u64.to_le_bytes());
        let l = drive_revive(&mut f, &mut c, H1, 120);
        assert!(f.writes.is_empty());
        assert!(rule_lines(&l)[0].contains("authoritative ?"), "{l:?}");
        // The PlayerIns stopped being one between the event and the write.
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        let r = c.try_rule(P0, &mut f);
        assert!(r.is_ok());
        f.players.remove(&P0);
        let r = c.try_rule(P0, &mut f);
        assert!(
            r.is_err_and(|e| e.contains("no longer has a PlayerIns vtable")),
            "vtable check"
        );
        // A refused store is logged and not counted.
        let mut f = world();
        f.refuse_write = true;
        let mut c = Core::new(true, RevivePolicy::Redown);
        let l = drive_revive(&mut f, &mut c, H1, 120);
        assert!(rule_lines(&l)[0].contains("write refused"), "{l:?}");
        assert_eq!(c.fired, 0);
    }

    #[test]
    fn rule_mirror_gate_accepts_either_confirmation_in_a_full_run() {
        // Mirror differs, PlayerGameData agrees: fires.
        let mut f = world();
        let mut c = Core::new(false, RevivePolicy::Redown);
        c.step(0, &subjects(), &mut f);
        f.downed(P0, 1, true);
        f.hp3(P0, G0, 0);
        f.attacker(P0, H1);
        c.step(100, &subjects(), &mut f);
        f.downed(P0, 1, false);
        f.hp3(P0, G0, 120);
        f.set(P0 + OFF_HP_MIRROR, &5u32.to_le_bytes());
        f.attacker(P0, u64::MAX);
        c.step(200, &subjects(), &mut f);
        assert_eq!(f.writes.len(), 1);
        // PlayerGameData differs, mirror agrees: fires.
        let mut f = world();
        let mut c = Core::new(false, RevivePolicy::Redown);
        c.step(0, &subjects(), &mut f);
        f.downed(P0, 1, true);
        f.hp3(P0, G0, 0);
        f.attacker(P0, H1);
        c.step(100, &subjects(), &mut f);
        f.downed(P0, 1, false);
        f.hp3(P0, G0, 120);
        f.set(auth_addr(G0), &120u32.to_le_bytes());
        f.set(G0 + GD_HP, &121u32.to_le_bytes()); // tracker sees 121 > 0, the mirror still agrees with the authoritative 120
        f.attacker(P0, u64::MAX);
        c.step(200, &subjects(), &mut f);
        assert_eq!(f.writes.len(), 1);
    }

    #[test]
    fn player_mode_logs_only_the_rule_line_and_rate_limits_it() {
        let mut f = world();
        let mut c = Core::new(false, RevivePolicy::Redown);
        // No tracker lines in the player build, not even for the INIT or a remote player's down.
        assert!(c.step(0, &subjects(), &mut f).is_empty());
        f.downed(P1, 1, true);
        assert!(c.step(100, &subjects(), &mut f).is_empty());
        // First rival revive: one line. Two more within 5 s: written but not logged. After 5 s: logged again with the count.
        let mut t = 1000;
        let mut logged = Vec::new();
        for _ in 0..4 {
            f.downed(P0, 1, true);
            f.hp3(P0, G0, 0);
            f.attacker(P0, H1);
            t += 100;
            c.step(t, &subjects(), &mut f);
            f.downed(P0, 1, false);
            f.hp3(P0, G0, 120);
            f.attacker(P0, u64::MAX);
            t += 100;
            let l = c.step(t, &subjects(), &mut f);
            logged.push(rule_lines(&l).len());
            t += if logged.len() == 3 { 5000 } else { 500 };
        }
        assert_eq!(logged, vec![1, 0, 0, 1]);
        assert_eq!(
            f.writes.len(),
            4,
            "every UP is acted on, only the log is limited"
        );
        // Dev mode logs every one.
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        let mut t = 0;
        c.step(t, &subjects(), &mut f);
        let mut n = 0;
        for _ in 0..3 {
            f.downed(P0, 1, true);
            f.hp3(P0, G0, 0);
            f.attacker(P0, H1);
            t += 100;
            c.step(t, &subjects(), &mut f);
            f.downed(P0, 1, false);
            f.hp3(P0, G0, 120);
            f.attacker(P0, u64::MAX);
            t += 100;
            n += rule_lines(&c.step(t, &subjects(), &mut f)).len();
        }
        assert_eq!(n, 3);
    }

    // ---- T-039 downed rule ----

    fn setf(f: &mut Fake, a: usize, v: f32) {
        f.set(a, &v.to_le_bytes());
    }
    fn getf(f: &Fake, a: usize) -> f32 {
        f32::from_bits(f.read_u32(a).expect("set"))
    }
    /// LOCAL downed with the given bar / clock / pause.
    fn down_local(f: &mut Fake, bar: f32, clock: f32, pause: f32) {
        f.downed(P0, 1, true);
        setf(f, P0 + OFF_REVIVE_BAR, bar);
        setf(f, P0 + OFF_BLEED_CLOCK, clock);
        setf(f, P0 + OFF_PAUSE, pause);
    }
    /// A hit: the bar drops to `bar`, the pause starts and `attacker` is recorded in +0x1A0.
    fn hit(f: &mut Fake, bar: f32, attacker: u64) {
        setf(f, P0 + OFF_REVIVE_BAR, bar);
        setf(f, P0 + OFF_PAUSE, 3.5);
        f.attacker(P0, attacker);
    }
    fn dn_lines(l: &[String]) -> Vec<&String> {
        l.iter()
            .filter(|x| x.starts_with("rule: rival hit"))
            .collect()
    }
    /// A core already past INIT, LOCAL downed (bar 90, clock 14.9, pause 0) and seen once at t=50.
    fn downed_world(dev: bool) -> (Fake, Core, u64) {
        let mut f = world();
        let mut c = Core::new(dev, RevivePolicy::Redown);
        c.step(0, &subjects(), &mut f);
        down_local(&mut f, 90.0, 14.9, 0.0);
        c.step(50, &subjects(), &mut f);
        (f, c, 50)
    }

    #[test]
    fn rival_hit_restores_the_bar_takes_seconds_off_the_clock_and_clears_the_pause() {
        for dev in [true, false] {
            let (mut f, mut c, t) = downed_world(dev);
            hit(&mut f, 70.0, H1);
            let l = c.step(t + 50, &subjects(), &mut f);
            assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 90.0);
            assert!((getf(&f, P0 + OFF_BLEED_CLOCK) - 11.9).abs() < 1e-4);
            assert_eq!(getf(&f, P0 + OFF_PAUSE), 0.0);
            assert_eq!(dn_lines(&l).len(), 1, "{l:?}");
            assert!(dn_lines(&l)[0].contains("bar 70 -> 90 restored"));
            assert_eq!(c.fired, 1);
            assert_eq!(f.writes.len(), 3);
            // Nothing else was touched (HP, state byte).
            assert_eq!(f.read_u8(P0 + OFF_DOWNED), Some(1));
        }
    }

    #[test]
    fn clock_floor_is_0_05_and_the_configured_seconds_are_used() {
        let (mut f, mut c, t) = downed_world(true);
        setf(&mut f, P0 + OFF_BLEED_CLOCK, 2.0);
        c.step(t + 50, &subjects(), &mut f);
        hit(&mut f, 70.0, H1);
        c.step(t + 100, &subjects(), &mut f);
        assert!((getf(&f, P0 + OFF_BLEED_CLOCK) - 0.05).abs() < 1e-6);
        // Custom seconds, clamped inside Core as well.
        let (mut f, mut c, t) = downed_world(true);
        c.set_hit_seconds(5.5);
        hit(&mut f, 70.0, H1);
        c.step(t + 50, &subjects(), &mut f);
        assert!((getf(&f, P0 + OFF_BLEED_CLOCK) - 9.4).abs() < 1e-4);
        let (mut f, mut c, t) = downed_world(true);
        c.set_hit_seconds(500.0);
        hit(&mut f, 70.0, H1);
        c.step(t + 50, &subjects(), &mut f);
        assert!((getf(&f, P0 + OFF_BLEED_CLOCK) - (14.9 - 15.0f32).max(CLOCK_FLOOR)).abs() < 1e-6);
    }

    #[test]
    fn consecutive_rival_hits_never_reduce_the_bar() {
        let (mut f, mut c, mut t) = downed_world(false);
        for i in 0..4 {
            // The game subtracts 20 from the (restored) bar on every hit.
            let cur = getf(&f, P0 + OFF_REVIVE_BAR);
            hit(&mut f, cur - 20.0, H1);
            t += 50;
            c.step(t, &subjects(), &mut f);
            assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 90.0, "hit {i}");
            // A quiet tick in between changes nothing.
            t += 50;
            c.step(t, &subjects(), &mut f);
            assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 90.0);
        }
        assert_eq!(c.fired, 4);
        // 14.9 - 4 * 3.0 floors at 0.05 on the last hit.
        assert!(getf(&f, P0 + OFF_BLEED_CLOCK) < 3.0);
    }

    #[test]
    fn ally_self_and_other_hits_are_untouched_and_the_bar_is_tracked() {
        for attacker in [H2, H0, 0xFFFF_FFFF_10C0_0099, u64::MAX] {
            let (mut f, mut c, t) = downed_world(true);
            hit(&mut f, 70.0, attacker);
            let l = c.step(t + 50, &subjects(), &mut f);
            assert!(f.writes.is_empty(), "attacker {attacker:X}");
            assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 70.0);
            assert!(dn_lines(&l).is_empty());
            // bar_before follows: a later rival hit restores to 70, not 90.
            setf(&mut f, P0 + OFF_PAUSE, 0.0);
            hit(&mut f, 50.0, H1);
            c.step(t + 100, &subjects(), &mut f);
            assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 70.0);
        }
    }

    #[test]
    fn bar_regrowth_is_tracked_and_the_current_attacker_is_the_fallback() {
        let (mut f, mut c, t) = downed_world(true);
        setf(&mut f, P0 + OFF_REVIVE_BAR, 95.0);
        assert!(c
            .step(t + 50, &subjects(), &mut f)
            .iter()
            .all(|l| !l.starts_with("rule:")));
        assert!(f.writes.is_empty());
        // +0x1A0 holds nothing valid, +0x198 names the rival.
        setf(&mut f, P0 + OFF_REVIVE_BAR, 80.0);
        f.attacker(P0, u64::MAX);
        f.set(P0 + OFF_CUR_ATTACKER, &H1.to_le_bytes());
        c.step(t + 100, &subjects(), &mut f);
        assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 95.0);
    }

    #[test]
    fn no_writes_when_not_downed_for_remote_players_or_when_a_gate_fails() {
        // Not downed: a "bar drop" in memory is ignored.
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        c.step(0, &subjects(), &mut f);
        setf(&mut f, P0 + OFF_REVIVE_BAR, 90.0);
        setf(&mut f, P0 + OFF_BLEED_CLOCK, 10.0);
        c.step(50, &subjects(), &mut f);
        hit(&mut f, 70.0, H1);
        c.step(100, &subjects(), &mut f);
        assert!(f.writes.is_empty());
        // State 2 (getting up) is not downed either.
        f.set(P0 + OFF_DOWNED, &[2]);
        setf(&mut f, P0 + OFF_REVIVE_BAR, 50.0);
        c.step(150, &subjects(), &mut f);
        assert!(f.writes.is_empty());
        // A downed REMOTE player is never written, even when a rival "hits" it.
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        c.step(0, &subjects(), &mut f);
        f.downed(P1, 1, true);
        setf(&mut f, P1 + OFF_REVIVE_BAR, 90.0);
        setf(&mut f, P1 + OFF_BLEED_CLOCK, 10.0);
        c.step(50, &subjects(), &mut f);
        setf(&mut f, P1 + OFF_REVIVE_BAR, 70.0);
        f.attacker(P1, H0);
        c.step(100, &subjects(), &mut f);
        assert!(f.writes.is_empty());
        // Gates: bad clock, non-finite clock, refused write, vanished PlayerIns. Nothing written (or a refusal) and one log line.
        for case in 0..4 {
            let (mut f, mut c, t) = downed_world(true);
            match case {
                0 => setf(&mut f, P0 + OFF_BLEED_CLOCK, 99.0),
                1 => setf(&mut f, P0 + OFF_BLEED_CLOCK, f32::NAN),
                2 => f.refuse_write = true,
                _ => {
                    f.players.remove(&P0);
                }
            }
            hit(&mut f, 70.0, H1);
            let l = c.step(t + 50, &subjects(), &mut f);
            if case != 2 {
                assert!(f.writes.is_empty(), "case {case}");
            }
            assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 70.0, "case {case}");
            if case < 3 {
                let msgs: Vec<_> = l
                    .iter()
                    .filter(|x| x.contains("nothing was written"))
                    .collect();
                assert_eq!(msgs.len(), 1, "case {case}: {l:?}");
                // Logged once: a second rival hit stays quiet.
                hit(&mut f, 50.0, H1);
                let l2 = c.step(t + 100, &subjects(), &mut f);
                assert!(l2.iter().all(|x| !x.contains("nothing was written")));
            }
        }
    }

    #[test]
    fn rule_v1_still_fires_when_a_rival_completes_the_bar() {
        let mut f = world();
        let mut c = Core::new(false, RevivePolicy::Redown);
        c.early_redown = false;
        c.step(0, &subjects(), &mut f);
        f.hp3(P0, G0, 0);
        down_local(&mut f, 5.0, 10.0, 0.0);
        f.attacker(P0, H1);
        c.step(100, &subjects(), &mut f);
        c.step(150, &subjects(), &mut f);
        // The hit completes the bar in one tick: state leaves 1, HP comes back.
        f.downed(P0, 1, false);
        f.set(P0 + OFF_DOWNED, &[2]);
        setf(&mut f, P0 + OFF_REVIVE_BAR, 0.0);
        f.hp3(P0, G0, 120);
        f.attacker(P0, H1);
        let w = f.writes.len();
        c.step(200, &subjects(), &mut f);
        // State 2 reads as unknown for the tracker, so the revive is seen when it returns to 0.
        f.set(P0 + OFF_DOWNED, &[0]);
        f.attacker(P0, u64::MAX);
        let l = c.step(300, &subjects(), &mut f);
        assert_eq!(f.writes.len(), w + 1);
        assert_eq!(f.writes[w].0, auth_addr(G0));
        assert_eq!(rule_lines(&l).len(), 1, "{l:?}");
    }

    #[test]
    fn a_rival_completing_the_bar_is_answered_during_the_get_up_and_a_teammate_is_not() {
        for (attacker, expect) in [(H1, 1usize), (H2, 0)] {
            let mut f = world();
            let mut c = Core::new(true, RevivePolicy::Redown);
            c.early_redown = true;
            c.step(0, &subjects(), &mut f);
            f.hp3(P0, G0, 0);
            down_local(&mut f, 40.0, 10.0, 0.0);
            f.attacker(P0, attacker);
            c.step(100, &subjects(), &mut f);
            // One hit completes the bar: state 2, HP not granted yet.
            f.set(P0 + OFF_DOWNED, &[2]);
            setf(&mut f, P0 + OFF_REVIVE_BAR, 0.0);
            let l = c.step(150, &subjects(), &mut f);
            assert!(f.writes.is_empty(), "hp 0: gate holds");
            assert_eq!(rule_lines(&l).len(), expect, "{l:?}");
            // HP arrives: one store to the authoritative HP, once.
            f.hp3(P0, G0, 120);
            let l = c.step(200, &subjects(), &mut f);
            c.step(250, &subjects(), &mut f);
            assert_eq!(f.writes.len(), expect, "attacker {attacker:X}");
            if expect == 1 {
                assert_eq!(f.writes[0], (auth_addr(G0), vec![0, 0, 0, 0]));
                assert!(rule_lines(&l)[0].contains("during the get-up"), "{l:?}");
            }
        }
    }

    /// The remote copy at `p` goes down (bar/clock as the owner sent them) and is then hit.
    fn remote_down_and_hit(f: &mut Fake, c: &mut Core, subs: &[Subject], p: usize) {
        c.step(0, subs, f);
        f.downed(p, 1, true);
        setf(f, p + OFF_REVIVE_BAR, 40.0);
        setf(f, p + OFF_BLEED_CLOCK, 15.0);
        setf(f, p + OFF_PAUSE, 0.0);
        c.step(50, subs, f);
        assert!(f.writes.is_empty());
        setf(f, p + OFF_REVIVE_BAR, 3.0);
        setf(f, p + OFF_BLEED_CLOCK, 12.5);
        setf(f, p + OFF_PAUSE, 3.48);
    }

    #[test]
    fn a_hit_on_a_downed_solo_rival_is_undone_on_the_display_copy_too() {
        for dev in [true, false] {
            let mut f = world();
            let mut c = Core::new(dev, RevivePolicy::Redown);
            let subs = subjects();
            remote_down_and_hit(&mut f, &mut c, &subs, P1);
            let l = c.step(100, &subs, &mut f);
            assert_eq!(getf(&f, P1 + OFF_REVIVE_BAR), 40.0);
            assert!((getf(&f, P1 + OFF_BLEED_CLOCK) - 9.5).abs() < 1e-4);
            assert_eq!(getf(&f, P1 + OFF_PAUSE), 0.0);
            assert_eq!(f.writes.len(), 3);
            let n = l.iter().filter(|x| x.contains("(display copy)")).count();
            assert_eq!(n, usize::from(dev), "{l:?}");
            // A second hit is measured against the same full bar.
            setf(&mut f, P1 + OFF_REVIVE_BAR, 2.0);
            c.step(150, &subs, &mut f);
            assert_eq!(getf(&f, P1 + OFF_REVIVE_BAR), 40.0);
            assert_eq!(f.writes.len(), 6);
        }
    }

    #[test]
    fn display_copies_of_allies_and_of_rivals_with_a_teammate_are_left_alone() {
        // An ally of LOCAL.
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        let subs = subjects();
        remote_down_and_hit(&mut f, &mut c, &subs, P2);
        c.step(100, &subs, &mut f);
        assert!(f.writes.is_empty());
        // Two rivals on one team: the copy cannot tell a teammate's hit from a rival's. Unknown rosters: the same.
        for team in [Some(1), None] {
            let mut f = world();
            let mut c = Core::new(true, RevivePolicy::Redown);
            let mut subs = subjects();
            subs[1].team_no = team;
            subs[2].team_no = team;
            subs[2].relation = Some(Relation::Rival);
            remote_down_and_hit(&mut f, &mut c, &subs, P1);
            c.step(100, &subs, &mut f);
            assert!(f.writes.is_empty(), "team {team:?}");
        }
    }

    #[test]
    fn player_mode_downed_rule_log_is_rate_limited_but_every_hit_is_acted_on() {
        let (mut f, mut c, mut t) = downed_world(false);
        let mut logged = Vec::new();
        for i in 0..4 {
            let cur = getf(&f, P0 + OFF_REVIVE_BAR);
            setf(&mut f, P0 + OFF_BLEED_CLOCK, 14.0);
            hit(&mut f, cur - 10.0, H1);
            t += 50;
            let l = c.step(t, &subjects(), &mut f);
            logged.push(dn_lines(&l).len());
            t += if i == 2 { 5000 } else { 100 };
            c.step(t, &subjects(), &mut f);
        }
        assert_eq!(logged, vec![1, 0, 0, 1]);
        assert_eq!(c.fired, 4);
        // Dev mode logs every hit and shows the status line.
        let (mut f, mut c, mut t) = downed_world(true);
        assert_eq!(
            c.downed_line(),
            Some("downed: bar 90.0 left, clock 14.9 s, pause 0.0")
        );
        let mut n = 0;
        for _ in 0..3 {
            let cur = getf(&f, P0 + OFF_REVIVE_BAR);
            hit(&mut f, cur - 10.0, H1);
            t += 50;
            n += dn_lines(&c.step(t, &subjects(), &mut f)).len();
        }
        assert_eq!(n, 3);
        // No status line outside dev mode or when not downed.
        let (_, c, _) = downed_world(false);
        assert!(c.downed_line().is_none());
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        c.step(0, &subjects(), &mut f);
        assert!(c.downed_line().is_none());
    }
}
