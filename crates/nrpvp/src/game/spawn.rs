//! F3: separate team spawns. Once per expedition, at the moment the hawk drops the LOCAL player, move that player to its team's
//! own point: a fixed distance from the drop point, in a direction that depends on the team, and a little higher. The player
//! then falls and lands as in vanilla, so the ground height at the target does not need to be known.
//!
//! Every game moves only its own player. The team comes from the player ids (`team::build_view`), which every game computes
//! the same way, so teammates go to the same side and rivals to different sides without any message.
//!
//! Position facts (NR 1.3.3.0, MEMORY_MAP.md section 8): position = `[physics module]+0x70`, world = position - origin
//! (`PlayerIns+0x80`); the teleport keeps world position correctly (T-028 run 4) and other players see it (Nucleus, 2026-10-14).
//! The hawk/drop signature comes from T-028 runs 3 and 4: during the ride the ground byte `+0x92` of the physics module reads 1
//! while the player moves at 12..17 m/s; at the drop it turns 0 and the player falls about 80 m. Sprinting is about 8 m/s.

use std::time::{Duration, Instant};

use super::mem;
use super::poswrite::{decode, fmt3, physics_module, plausible, RealMem, ORIGIN_OFF};

/// Ground byte inside the physics module: 1 standing (and riding the hawk), 0 in the air. [VERIFIED: T-028 runs 1-4]
const GROUND_OFF: usize = 0x92;
/// Position inside the physics module. [VERIFIED: T-028 run 1]
const POS_OFF: usize = 0x70;
/// Horizontal world speed that counts as the hawk ride (sprint is ~8 m/s, the ride 12..17).
pub(crate) const HAWK_MIN_SPEED: f32 = 11.0;
/// The ride must last this long before a drop counts (the vanilla ride is ~12 s).
pub(crate) const HAWK_MIN_SECS: f32 = 3.0;
/// Speed is measured over at least this long, so a 50 ms tick does not make it noisy.
const SPEED_WINDOW_SECS: f32 = 0.25;
/// Metres added to the drop height at the target, in case the ground there is higher.
pub(crate) const LIFT: f32 = 40.0;
pub(crate) const DEFAULT_RADIUS: f32 = 200.0;
pub(crate) const RADIUS_RANGE: (f32, f32) = (20.0, 1000.0);
/// How often the controller samples the local player.
const EVERY: Duration = Duration::from_millis(50);
/// Follow-up logs after the move.
const FOLLOW: [(u64, &str); 3] = [(1000, "+1s"), (5000, "+5s"), (15000, "+15s")];

/// Watches the local player's world position and ground byte; says once when the hawk drops the player.
#[derive(Debug, Default)]
pub(crate) struct DropDetector {
    /// Last sample used for the speed: (time s, world position).
    anchor: Option<(f32, [f32; 3])>,
    speed: f32,
    fast_since: Option<f32>,
    ride_seen: bool,
    fired: bool,
}

impl DropDetector {
    /// Feed one sample (`t` in seconds, increasing). True exactly once: the first airborne sample after a hawk ride.
    pub(crate) fn feed(&mut self, t: f32, world: [f32; 3], ground: bool) -> bool {
        if self.fired {
            return false;
        }
        match self.anchor {
            Some((t0, p0)) if t - t0 >= SPEED_WINDOW_SECS => {
                let (dx, dz) = (world[0] - p0[0], world[2] - p0[2]);
                self.speed = (dx * dx + dz * dz).sqrt() / (t - t0);
                self.anchor = Some((t, world));
            }
            Some(_) => {}
            None => self.anchor = Some((t, world)),
        }
        if ground && self.speed >= HAWK_MIN_SPEED {
            let since = *self.fast_since.get_or_insert(t);
            if t - since >= HAWK_MIN_SECS {
                self.ride_seen = true;
            }
        } else if ground {
            self.fast_since = None;
        }
        if !ground && self.ride_seen {
            self.fired = true;
            return true;
        }
        false
    }

    pub(crate) fn ride_seen(&self) -> bool {
        self.ride_seen
    }
}

/// The world point for team `rank` of `count` (0-based rank among the sorted team numbers): `radius` metres from the drop
/// point, teams spread evenly around it (team 0 towards +x), [`LIFT`] metres higher.
pub(crate) fn target(drop: [f32; 3], rank: usize, count: usize, radius: f32) -> [f32; 3] {
    let a = std::f32::consts::TAU * rank as f32 / count.max(1) as f32;
    [
        drop[0] + radius * a.cos(),
        drop[1] + LIFT,
        drop[2] + radius * a.sin(),
    ]
}

/// The local team's rank among the distinct team numbers, and how many teams there are.
pub(crate) fn rank_of(local_team: u8, teams: &[u8]) -> Option<(usize, usize)> {
    let mut t = teams.to_vec();
    t.sort_unstable();
    t.dedup();
    t.iter().position(|&x| x == local_team).map(|r| (r, t.len()))
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// What one sample of the local player read.
struct Sample {
    phys: usize,
    pos: [f32; 3],
    origin: [f32; 3],
    ground: bool,
}

fn read_sample(player: usize, base: usize) -> Option<Sample> {
    let (phys, _) = physics_module(&RealMem, player, base).ok()?;
    let pos = decode(&mem::read::<[u8; 12]>(phys + POS_OFF)?);
    let origin = decode(&mem::read::<[u8; 12]>(player + ORIGIN_OFF)?);
    let ground = mem::read::<u8>(phys + GROUND_OFF)? == 1;
    (plausible(pos) && plausible(origin)).then_some(Sample {
        phys,
        pos,
        origin,
        ground,
    })
}

/// Per-expedition state, driven from `TeamController::tick`.
pub(crate) struct SpawnCtl {
    radius: f32,
    base: usize,
    inst: usize,
    epoch: Instant,
    next: Instant,
    det: DropDetector,
    ride_logged: bool,
    follow: Vec<(Instant, &'static str, usize)>,
}

impl SpawnCtl {
    pub(crate) fn new(radius: f32, base: usize) -> Self {
        let now = Instant::now();
        SpawnCtl {
            radius,
            base,
            inst: 0,
            epoch: now,
            next: now,
            det: DropDetector::default(),
            ride_logged: false,
            follow: Vec::new(),
        }
    }

    /// Worth a pass now: a new WorldChrMan instance, a sample due before the drop, or follow-up logs pending.
    pub(crate) fn due(&self, inst: usize) -> bool {
        inst != self.inst || !self.follow.is_empty() || (!self.det.fired && Instant::now() >= self.next)
    }

    /// One pass. `inst` = WorldChrMan instance, `local` = the LOCAL PlayerIns (already resolved and vtable-checked) or None,
    /// `in_expedition` = its team_type is the expedition value, `teams` = called only at the drop: (local team number, team
    /// number of every player), or None if the roster cannot be trusted.
    pub(crate) fn tick(
        &mut self,
        inst: usize,
        local: Option<usize>,
        in_expedition: bool,
        teams: impl FnOnce() -> Option<(u8, Vec<u8>)>,
    ) {
        let now = Instant::now();
        if inst != self.inst {
            self.inst = inst;
            self.epoch = now;
            self.det = DropDetector::default();
            self.ride_logged = false;
            self.follow.clear();
        }
        self.follow_tick(now);
        if now < self.next {
            return;
        }
        self.next = now + EVERY;
        let (Some(local), true) = (local, in_expedition) else {
            return;
        };
        let Some(s) = read_sample(local, self.base) else {
            return;
        };
        let world = sub(s.pos, s.origin);
        let t = self.epoch.elapsed().as_secs_f32();
        let fired = self.det.feed(t, world, s.ground);
        if self.det.ride_seen() && !self.ride_logged {
            self.ride_logged = true;
            tracing::info!("T-028 SPAWN: hawk ride seen at world {}", fmt3(world));
        }
        if !fired {
            return;
        }
        let Some((rank, count)) = teams().and_then(|(mine, all)| rank_of(mine, &all)) else {
            tracing::warn!(
                "T-028 SPAWN: hawk drop at world {} but the local team is unknown; not moving",
                fmt3(world)
            );
            return;
        };
        let to = target(world, rank, count, self.radius);
        let local_pos = add(to, s.origin);
        if !plausible(local_pos) {
            tracing::warn!("T-028 SPAWN: target {} is implausible; not moving", fmt3(local_pos));
            return;
        }
        let mut bytes = [0u8; 12];
        for (i, v) in local_pos.iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        match mem::write_bytes(s.phys + POS_OFF, &bytes) {
            Ok(()) => {
                tracing::info!(
                    "T-028 SPAWN: hawk drop at world {}; team rank {rank} of {count} -> moved to world {} ({} m, lift {} m)",
                    fmt3(world),
                    fmt3(to),
                    self.radius,
                    LIFT
                );
                self.follow = FOLLOW
                    .iter()
                    .map(|&(ms, label)| (now + Duration::from_millis(ms), label, local))
                    .collect();
            }
            Err(e) => tracing::warn!("T-028 SPAWN: write refused: {e}"),
        }
    }

    fn follow_tick(&mut self, now: Instant) {
        if self.follow.is_empty() {
            return;
        }
        let (due, rest): (Vec<_>, Vec<_>) = self.follow.drain(..).partition(|f| now >= f.0);
        self.follow = rest;
        for (_, label, player) in due {
            match read_sample(player, self.base) {
                Some(s) => tracing::info!(
                    "T-028 SPAWN {label}: world {} on ground {}",
                    fmt3(sub(s.pos, s.origin)),
                    s.ground
                ),
                None => tracing::info!("T-028 SPAWN {label}: position unreadable"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Samples every 50 ms: `secs` seconds at `speed` m/s along x with the given ground byte, starting at `t`/`x`.
    fn run(d: &mut DropDetector, t: &mut f32, x: &mut f32, secs: f32, speed: f32, ground: bool) -> usize {
        let mut fired = 0;
        let end = *t + secs;
        while *t < end {
            *t += 0.05;
            *x += speed * 0.05;
            if d.feed(*t, [*x, 100.0, 0.0], ground) {
                fired += 1;
            }
        }
        fired
    }

    #[test]
    fn fires_once_at_the_drop_after_a_ride_and_never_for_sprinting_or_jumping() {
        let (mut t, mut x) = (0.0, 0.0);
        let mut d = DropDetector::default();
        // Waiting before the ride (in the air, still), sprinting and jumping on foot: nothing.
        assert_eq!(run(&mut d, &mut t, &mut x, 5.0, 0.0, false), 0);
        assert_eq!(run(&mut d, &mut t, &mut x, 10.0, 8.5, true), 0);
        assert_eq!(run(&mut d, &mut t, &mut x, 1.0, 8.5, false), 0);
        // A short burst of speed is not a ride.
        assert_eq!(run(&mut d, &mut t, &mut x, 2.0, 15.0, true), 0);
        assert_eq!(run(&mut d, &mut t, &mut x, 0.5, 5.0, false), 0);
        assert!(!d.ride_seen());
        // The ride, then the drop: exactly one.
        assert_eq!(run(&mut d, &mut t, &mut x, 12.0, 15.0, true), 0);
        assert!(d.ride_seen());
        assert_eq!(run(&mut d, &mut t, &mut x, 3.0, 15.0, false), 1);
        assert_eq!(run(&mut d, &mut t, &mut x, 20.0, 15.0, true), 0);
        assert_eq!(run(&mut d, &mut t, &mut x, 3.0, 15.0, false), 0);
    }

    #[test]
    fn teams_get_distinct_sides_and_teammates_the_same_one() {
        assert_eq!(rank_of(5, &[5, 2, 5, 9]), Some((1, 3)));
        assert_eq!(rank_of(2, &[5, 2, 5, 9]), Some((0, 3)));
        assert_eq!(rank_of(4, &[5, 2]), None);
        let d = [10.0, 50.0, -20.0];
        let a = target(d, 0, 2, 200.0);
        let b = target(d, 1, 2, 200.0);
        assert!((a[0] - 210.0).abs() < 1e-3 && (a[2] + 20.0).abs() < 1e-3 && a[1] == 90.0);
        assert!((b[0] + 190.0).abs() < 1e-3 && (b[2] + 20.0).abs() < 1e-2);
        // Solo (one team): still moved, so the feature can be checked alone.
        let s = target(d, 0, 1, 200.0);
        assert!((s[0] - 210.0).abs() < 1e-3);
    }
}
