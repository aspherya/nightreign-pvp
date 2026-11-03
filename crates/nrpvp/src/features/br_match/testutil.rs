//! Helpers shared by the unit tests of the match rules (tests only).

use super::defeat::DEAD_DEBOUNCE_MS;
use super::free_day1::DAY1_SPAN_SECS;
use super::Match;
use crate::features::Act;
use crate::game::combat_state::{Fighter, HitKind, LocalHits};
use crate::game::views::{ClockState, ClockTime, Place};
use crate::game::world::{NightFlags, World, MAX_FIGHTERS};

pub(super) const ME: u64 = 0x6001_FFFF_FFFF;
pub(super) const P2: u64 = 0x6002_FFFF_FFFF;
pub(super) const P3: u64 = 0x6003_FFFF_FFFF;

pub(super) fn fighter(id: u64, local: bool, state: u8, hp: u32) -> Fighter {
    Fighter {
        addr: 0x10_0000 + (id >> 32) as usize,
        id,
        team: id,
        local,
        state,
        hp,
        clock: 0.0,
    }
}

pub(super) fn up(id: u64, local: bool) -> Fighter {
    fighter(id, local, 0, 240)
}

pub(super) fn dead(id: u64, local: bool) -> Fighter {
    fighter(id, local, 0, 0)
}

pub(super) fn clock_at(h: u8, m: u8, speed: f32) -> ClockState {
    ClockState {
        time: ClockTime { h, m, s: 0 },
        speed,
        running: true,
        mailbox_free: true,
    }
}

pub(super) fn world(fs: &[Fighter], nf: NightFlags) -> World {
    let mut fighters = [None; MAX_FIGHTERS];
    for (o, f) in fighters.iter_mut().zip(fs) {
        *o = Some(*f);
    }
    World {
        wcm_instance: 0x5_0000,
        local: Some(0x10_0000),
        place: Some(Place::Expedition),
        night: Some(nf),
        fighters,
        local_pos: Some(([10.0, 20.0, 30.0], true)),
        clock: Some(clock_at(9, 0, DAY1_SPAN_SECS / 1020.0)),
        ..World::default()
    }
}

pub(super) fn core() -> Match {
    Match::battle_royale(1020, 20, false)
}

pub(super) fn run(c: &mut Match, ms: u64, w: &World) -> (Vec<Act>, Vec<(u32, bool)>, Vec<String>) {
    let mut flags = Vec::new();
    let (acts, lines) = c.step(ms, w, &mut flags);
    (acts, flags, lines)
}

/// The dead world twice, [`DEAD_DEBOUNCE_MS`] apart (HP 0 must last that long to count as a death); both results merged.
pub(super) fn run_dead(
    c: &mut Match,
    ms: u64,
    w: &World,
) -> (Vec<Act>, Vec<(u32, bool)>, Vec<String>) {
    let (mut a, mut f, mut l) = run(c, ms, w);
    let (a2, f2, l2) = run(c, ms + DEAD_DEBOUNCE_MS, w);
    a.extend(a2);
    f.extend(f2);
    l.extend(l2);
    (a, f, l)
}

pub(super) fn rival_hit(w: &mut World, age: u64) {
    w.local_hits = LocalHits {
        last: Some((HitKind::Rival, age)),
        rival_age_ms: Some(age),
        rival_id: None,
    };
}
