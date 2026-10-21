//! T-033/T-035: per-player downed state, HP and last attacker, classified ALLY / RIVAL / SELF / OTHER; only transitions
//! are produced.

use std::collections::HashMap;

use super::*;
use crate::game::mem::is_ptr;
use crate::overlay::shared::Relation;

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

pub(super) fn read_f32(mem: &dyn CombatMem, addr: usize) -> Option<f32> {
    mem.read_u32(addr).map(f32::from_bits)
}

/// Handles and rosters of every tracked player (for classifying an attacker outside the tracker's 100 ms pass).
pub(super) fn player_infos(mem: &dyn CombatMem, subjects: &[Subject]) -> Vec<PlayerInfo> {
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
pub(super) fn gamedata_of(mem: &dyn CombatMem, addr: usize) -> Option<usize> {
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
    pub(super) fn label(self) -> &'static str {
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

pub(super) fn opt<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map_or("?".to_string(), |x| x.to_string())
}

impl Event {
    pub(super) fn who(&self) -> String {
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
    pub(super) fn is_rival_revive(&self) -> bool {
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
pub(super) struct DownedMem {
    pub(super) addr: usize,
    /// The bar value at the previous tick (never lowered by rival hits).
    pub(super) bar_before: f32,
    pub(super) gate_logged: bool,
}

#[derive(Clone, Debug)]
pub(super) struct PState {
    pub(super) downed: bool,
    pub(super) hp: Option<u32>,
    pub(super) max_hp: u32,
    pub(super) latched: Option<u64>,
    pub(super) line: String,
}

#[derive(Default)]
pub(crate) struct Tracker {
    pub(super) states: HashMap<usize, PState>,
    pub(super) next_ms: u64,
    pub(super) local_downed: Option<bool>,
    pub(super) local_addr: Option<usize>,
    pub(super) local_reading: Option<Reading>,
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
