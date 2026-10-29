//! Orchestrator aid (dev only): read and set this game's own player position, so `nro` can put the host at a fixed spot (in front of
//! the Roundtable table, `navigate = "invite_members"`) whatever position the save remembers. LOCAL only; no game code is touched.
//!
//! Position (MEMORY_MAP §8, game 1.03.3 / exe 1.3.3.0): the player's `CS::CSChrPhysicsModule` (`[[PlayerIns+0x1B8]+0x68]`, vtable
//! `exe+0x2C13720` checked) holds x, y, z at `+0x70`; world position = physics position minus the moving origin `PlayerIns+0x80`.
//! Writing the physics position moves the player and it stays `[VERIFIED: T-028 run 4, solo, and Nucleus 2026-10-14]`.
//! `pos` also prints the four floats at `Physics+0x50` (the orientation quaternion in Elden Ring's layout of this class) for research:
//! `[HYPOTHESIS]`, read only; and `menu` = `CSMenuMan+0x48` (0 = no game menu open, MEMORY_MAP §19).
//!
//! With a yaw, `tp` also writes that heading as the quaternion (0, sin(yaw/2), 0, cos(yaw/2)) at `Physics+0x50`, only when the value
//! there reads as a rotation about y. Safety gates of [`tp_cmd`]: a world is loaded, LOCAL is ChrSet entry 0, the physics module has its RTTI vtable,
//! the target is finite and at most [`MAX_JUMP`] metres from where the player stands now; the store goes through `mem::write_bytes`.
//! Log lines start with `teleport:`.

use super::layout::{
    CHRSET_ENTRIES, PHYSICS_POS, PHYSICS_VTABLE_RVA, PLAYER_MODULES, PLAYER_ORIGIN, WCM_CHRSET,
};
use super::mem;

/// `WorldChrMan` singleton slot (exe 1.3.3.0; MEMORY_MAP §1).
const RVA_WCM_SLOT: usize = 0x3C0_F0A8;
/// Physics module inside the module list (`[PlayerIns+0x1B8]+0x68`, the same read as `puppet.rs`).
const MODULES_PHYSICS: usize = 0x68;
/// Orientation candidate inside the physics module (read only, HYPOTHESIS).
const PHYSICS_ORIENT: usize = 0x50;
/// A teleport may move the player at most this far (metres): the Roundtable is about 60 m across; a typo must not drop the player
/// out of the map.
pub(crate) const MAX_JUMP: f32 = 100.0;

/// Parse `tp <x> <y> <z> [yaw]` (world metres; yaw in degrees as `pos` prints it).
pub(crate) fn parse_target(args: &[String]) -> Result<([f32; 3], Option<f32>), String> {
    if !(3..=4).contains(&args.len()) {
        return Err(
            "usage: tp <x> <y> <z> [yaw] (world metres and degrees, as `pos` prints them)".into(),
        );
    }
    let mut v = [0f32; 4];
    for (i, a) in args.iter().enumerate() {
        let x: f32 = a.parse().map_err(|_| format!("{a:?} is not a number"))?;
        if !x.is_finite() {
            return Err(format!("{a:?} is not a finite number"));
        }
        v[i] = x;
    }
    if args.len() == 4 && v[3].abs() > 360.0 {
        return Err("yaw is -360..360 degrees".into());
    }
    Ok(([v[0], v[1], v[2]], (args.len() == 4).then_some(v[3])))
}

/// The unit quaternion (x, y, z, w) of a rotation by `yaw` degrees about the y axis (the inverse of [`yaw_deg`]).
pub(crate) fn quat_of_yaw(yaw: f32) -> [f32; 4] {
    let h = yaw.to_radians() / 2.0;
    [0.0, h.sin(), 0.0, h.cos()]
}

/// The target must be finite and within [`MAX_JUMP`] of the current world position.
pub(crate) fn check_jump(cur: [f32; 3], to: [f32; 3]) -> Result<f32, String> {
    if cur.iter().chain(to.iter()).any(|v| !v.is_finite()) {
        return Err("position is not finite".into());
    }
    let d = ((to[0] - cur[0]).powi(2) + (to[1] - cur[1]).powi(2) + (to[2] - cur[2]).powi(2)).sqrt();
    if d > MAX_JUMP {
        return Err(format!(
            "target is {d:.1} m away; at most {MAX_JUMP} m per teleport"
        ));
    }
    Ok(d)
}

/// World = physics - origin.
pub(crate) fn world_of(phys: [f32; 3], origin: [f32; 3]) -> [f32; 3] {
    [
        phys[0] - origin[0],
        phys[1] - origin[1],
        phys[2] - origin[2],
    ]
}

/// Physics = world + origin.
pub(crate) fn phys_of(world: [f32; 3], origin: [f32; 3]) -> [f32; 3] {
    [
        world[0] + origin[0],
        world[1] + origin[1],
        world[2] + origin[2],
    ]
}

/// Heading in degrees (0 = +z, 90 = +x) of a unit quaternion (x, y, z, w) that rotates about the y axis; `None` if it is not one.
pub(crate) fn yaw_deg(q: [f32; 4]) -> Option<f32> {
    let n = (q.iter().map(|v| v * v).sum::<f32>()).sqrt();
    if !(0.98..=1.02).contains(&n) || q[0].abs() > 0.05 || q[2].abs() > 0.05 {
        return None;
    }
    Some((2.0 * q[1].atan2(q[3])).to_degrees())
}

fn read3(a: usize) -> Option<[f32; 3]> {
    Some([
        mem::read::<f32>(a)?,
        mem::read::<f32>(a + 4)?,
        mem::read::<f32>(a + 8)?,
    ])
}

fn read_quat(phys: usize) -> [f32; 4] {
    [0, 4, 8, 12].map(|o| mem::read::<f32>(phys + PHYSICS_ORIENT + o).unwrap_or(f32::NAN))
}

/// LOCAL's PlayerIns and its physics module, both vtable-checked.
fn local() -> Result<(usize, usize), String> {
    let b = crate::game::puppet::exe_base()?;
    let wcm = mem::read::<usize>(b + RVA_WCM_SLOT)
        .filter(|&w| w != 0)
        .ok_or("no world")?;
    let ents = mem::read::<usize>(wcm + WCM_CHRSET + CHRSET_ENTRIES)
        .filter(|&e| e != 0)
        .ok_or("no player list")?;
    let chr = mem::read::<usize>(ents)
        .filter(|&c| c != 0)
        .ok_or("no LOCAL")?;
    let phys = mem::read::<usize>(chr + PLAYER_MODULES)
        .and_then(|m| mem::read::<usize>(m + MODULES_PHYSICS))
        .filter(|&p| p != 0)
        .ok_or("no physics module")?;
    if mem::read::<usize>(phys) != Some(b + PHYSICS_VTABLE_RVA) {
        return Err("physics module vtable does not match".into());
    }
    Ok((chr, phys))
}

/// `pos`: LOCAL's world position, physics position and the orientation candidate.
pub(crate) fn pos_cmd() -> Result<String, String> {
    let (chr, phys) = local()?;
    let p = read3(phys + PHYSICS_POS).ok_or("position unreadable")?;
    let o = read3(chr + PLAYER_ORIGIN).ok_or("origin unreadable")?;
    let w = world_of(p, o);
    let q = [
        mem::read::<f32>(phys + PHYSICS_ORIENT).unwrap_or(f32::NAN),
        mem::read::<f32>(phys + PHYSICS_ORIENT + 4).unwrap_or(f32::NAN),
        mem::read::<f32>(phys + PHYSICS_ORIENT + 8).unwrap_or(f32::NAN),
        mem::read::<f32>(phys + PHYSICS_ORIENT + 12).unwrap_or(f32::NAN),
    ];
    let menu = crate::game::puppet::exe_base()
        .ok()
        .and_then(super::menugate::menu_word);
    Ok(format!(
        "pos world ({:.2}, {:.2}, {:.2}) physics ({:.2}, {:.2}, {:.2}) origin ({:.1}, {:.1}, {:.1}) q+0x50 ({:.3}, {:.3}, {:.3}, {:.3}) yaw {} menu {}",
        w[0], w[1], w[2], p[0], p[1], p[2], o[0], o[1], o[2], q[0], q[1], q[2], q[3],
        yaw_deg(q).map_or("?".into(), |y| format!("{y:.0}")),
        menu.map_or("?".into(), |m| format!("0x{m:X}"))
    ))
}

/// `tp <x> <y> <z> [yaw]`: put LOCAL at that world position (metres) and, with a yaw, turn it to face that heading.
pub(crate) fn tp_cmd(args: &[String]) -> Result<String, String> {
    let (to, yaw) = parse_target(args)?;
    let (chr, phys) = local()?;
    let p = read3(phys + PHYSICS_POS).ok_or("position unreadable")?;
    let o = read3(chr + PLAYER_ORIGIN).ok_or("origin unreadable")?;
    let d = check_jump(world_of(p, o), to)?;
    // The heading is only written over a value that reads as a rotation about y (the field is a HYPOTHESIS until then).
    if yaw.is_some() {
        let q = [
            mem::read::<f32>(phys + PHYSICS_ORIENT).unwrap_or(f32::NAN),
            mem::read::<f32>(phys + PHYSICS_ORIENT + 4).unwrap_or(f32::NAN),
            mem::read::<f32>(phys + PHYSICS_ORIENT + 8).unwrap_or(f32::NAN),
            mem::read::<f32>(phys + PHYSICS_ORIENT + 12).unwrap_or(f32::NAN),
        ];
        if yaw_deg(q).is_none() {
            return Err(format!(
                "Physics+0x50 does not read as a heading ({q:?}); nothing written"
            ));
        }
    }
    let np = phys_of(to, o);
    let mut bytes = Vec::with_capacity(12);
    for v in np {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    mem::write_bytes(phys + PHYSICS_POS, &bytes).map_err(|e| e.to_string())?;
    if let Some(y) = yaw {
        let mut qb = Vec::with_capacity(16);
        for v in quat_of_yaw(y) {
            qb.extend_from_slice(&v.to_le_bytes());
        }
        mem::write_bytes(phys + PHYSICS_ORIENT, &qb).map_err(|e| e.to_string())?;
    }
    let turned = yaw.map_or(String::new(), |y| format!(", facing yaw {y:.0}"));
    tracing::info!(
        "teleport: LOCAL to world ({:.2}, {:.2}, {:.2}), {d:.1} m{turned}",
        to[0],
        to[1],
        to[2]
    );
    Ok(format!(
        "tp: LOCAL to world ({:.2}, {:.2}, {:.2}), moved {d:.1} m{turned}",
        to[0], to[1], to[2]
    ))
}

/// `tp_player <slot 1..5> [dx dz]`: put LOCAL next to the player in that player-list slot (+dx, +1 m, +dz metres in the shared physics
/// frame, default 3 m along x), whatever the distance: a test aid for RW-9 (two instances must see each other). Both positions are
/// physics positions, which every character of the world shares (MEMORY_MAP §8), so no origin is involved.
pub(crate) fn tp_player_cmd(args: &[String]) -> Result<String, String> {
    let usage = "usage: tp_player <slot 1..5> [dx dz]";
    let slot: usize = args
        .first()
        .and_then(|v| v.parse().ok())
        .filter(|s| (1..6).contains(s))
        .ok_or(usage)?;
    let off = |i: usize, d: f32| -> Result<f32, String> {
        args.get(i)
            .map_or(Ok(d), |v| v.parse::<f32>().map_err(|_| usage.to_string()))
            .and_then(|v| {
                if v.is_finite() && v.abs() <= 50.0 {
                    Ok(v)
                } else {
                    Err("offsets are at most 50 m".into())
                }
            })
    };
    let (dx, dz) = (off(1, 3.0)?, off(2, 0.0)?);
    let (_, phys) = local()?;
    let b = crate::game::puppet::exe_base()?;
    let wcm = mem::read::<usize>(b + RVA_WCM_SLOT)
        .filter(|&w| w != 0)
        .ok_or("no world")?;
    let ents = mem::read::<usize>(wcm + WCM_CHRSET + CHRSET_ENTRIES)
        .filter(|&e| e != 0)
        .ok_or("no player list")?;
    let other = mem::read::<usize>(ents + slot * super::layout::CHRSET_ENTRY_SIZE)
        .filter(|&c| c != 0)
        .ok_or("that slot is empty")?;
    let ophys = mem::read::<usize>(other + PLAYER_MODULES)
        .and_then(|m| mem::read::<usize>(m + MODULES_PHYSICS))
        .filter(|&p| p != 0)
        .ok_or("no physics module on that player")?;
    if mem::read::<usize>(ophys) != Some(b + PHYSICS_VTABLE_RVA) {
        return Err("that player's physics module vtable does not match".into());
    }
    let to = read3(ophys + PHYSICS_POS).ok_or("that player's position is unreadable")?;
    let np = [to[0] + dx, to[1] + 1.0, to[2] + dz];
    if np.iter().any(|v| !v.is_finite()) {
        return Err("that player's position is not finite".into());
    }
    let mut bytes = Vec::with_capacity(12);
    for v in np {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    mem::write_bytes(phys + PHYSICS_POS, &bytes).map_err(|e| e.to_string())?;
    // Face the other player (only over a value that reads as a heading, as `tp` does).
    let q = read_quat(phys);
    let facing = if yaw_deg(q).is_some() && (dx != 0.0 || dz != 0.0) {
        let yaw = (-dx).atan2(-dz).to_degrees();
        let mut qb = Vec::with_capacity(16);
        for v in quat_of_yaw(yaw) {
            qb.extend_from_slice(&v.to_le_bytes());
        }
        mem::write_bytes(phys + PHYSICS_ORIENT, &qb).map_err(|e| e.to_string())?;
        format!(", facing it (yaw {yaw:.0})")
    } else {
        String::new()
    };
    tracing::info!("teleport: LOCAL next to player slot {slot} (physics {np:?}){facing}");
    Ok(format!(
        "tp_player: LOCAL next to slot {slot} (+{dx:.1}, +1, +{dz:.1} m){facing}"
    ))
}

/// RW-8 test aid: enemies come from the character sets `WorldChrMan` holds (inline or by pointer), recognised by vtable
/// (`CS::ChrSet`, `CS::OpenFieldChrSet`; set `+0x10` capacity, `+0x18` entries of 16 bytes, entry `+0` = character), and an
/// enemy by the `CS::EnemyIns` vtable. RVAs from the RTTI of exe 1.3.3.0 (MEMORY_MAP §21). Read only, except `mob_go`'s HP write.
const VT_CHRSET: usize = 0x2C1_5D70;
const VT_OPENFIELD_CHRSET: usize = 0x2C1_9B48;
const VT_ENEMY: usize = 0x2C1_8120;
/// How far into `WorldChrMan` the sets are searched (bytes).
const WCM_SCAN: usize = 0x2_0000;
const MAX_SET_CAPACITY: u32 = 4096;
/// `mobs` / `mob_go` skip enemies with more HP (big ones survived a swing at HP 1 in run 5, 2026-10-27).
const MOB_MAX_HP: u32 = 1000;
/// `boss_list` / `boss_go` / `boss_kill` take the other side: characters with more than this HP (field bosses such as the
/// Death Rite Bird and the Black Knife Assassin; the boss health bar is the in-game sign, this is our test-aid cut-off).
const BOSS_MIN_HP: u32 = 1000;

/// One living enemy: address, handle, npc id, HP, physics position and which set it came from.
#[derive(Clone)]
struct Mob {
    chr: usize,
    handle: u64,
    npc: u32,
    hp: u32,
    pos: [f32; 3],
    /// Came from the open-field set (world characters) rather than the plain `ChrSet` (Roundtable NPCs are in `ChrSet`).
    open_field: bool,
}

fn hp_field(chr: usize) -> Option<usize> {
    // The character data module is module 0 (`[[chr+0x1B8]+0]`, puppet_hp's read; MEMORY_MAP §5).
    let m = mem::read::<usize>(chr + PLAYER_MODULES)?;
    let data = mem::read::<usize>(m).filter(|d| *d != 0)?;
    Some(data + super::layout::DATA_MODULE_HP)
}

fn enemies_alive(min_hp: u32, max_hp: u32) -> Result<Vec<Mob>, String> {
    let b = crate::game::puppet::exe_base()?;
    let wcm = mem::read::<usize>(b + RVA_WCM_SLOT)
        .filter(|&w| w != 0)
        .ok_or("no world")?;
    let is_set_vt = |v: usize| v == b + VT_CHRSET || v == b + VT_OPENFIELD_CHRSET;
    let mut sets = Vec::new();
    for off in (0..WCM_SCAN).step_by(8) {
        let Some(v) = mem::read::<usize>(wcm + off) else {
            break;
        };
        let set = if is_set_vt(v) {
            wcm + off
        } else if mem::is_ptr(v) && mem::read::<usize>(v).is_some_and(is_set_vt) {
            v
        } else {
            continue;
        };
        if !sets.iter().any(|(s, _)| *s == set) {
            // The set's first qword is its vtable: open-field (world characters) vs the plain ChrSet (Roundtable NPCs live there).
            let open_field = mem::read::<usize>(set) == Some(b + VT_OPENFIELD_CHRSET);
            sets.push((set, open_field));
        }
    }
    let mut out = Vec::new();
    for (set, open_field) in sets {
        let cap = mem::read::<u32>(set + super::layout::CHRSET_CAPACITY).unwrap_or(0);
        let Some(ents) = mem::read::<usize>(set + CHRSET_ENTRIES).filter(|e| mem::is_ptr(*e))
        else {
            continue;
        };
        for i in 0..cap.min(MAX_SET_CAPACITY) as usize {
            let Some(chr) =
                mem::read::<usize>(ents + i * super::layout::CHRSET_ENTRY_SIZE).filter(|c| *c != 0)
            else {
                continue;
            };
            if mem::read::<usize>(chr) != Some(b + VT_ENEMY)
                || out.iter().any(|m: &Mob| m.chr == chr)
            {
                continue;
            }
            let hp = hp_field(chr).and_then(mem::read::<u32>).unwrap_or(0);
            let pos = mem::read::<usize>(chr + PLAYER_MODULES)
                .and_then(|m| mem::read::<usize>(m + MODULES_PHYSICS))
                .filter(|p| mem::read::<usize>(*p) == Some(b + PHYSICS_VTABLE_RVA))
                .and_then(|p| read3(p + PHYSICS_POS));
            let handle = mem::read::<u64>(chr + super::layout::CHR_HANDLE).unwrap_or(0);
            // Handles 0xFFFFFFFF_xxxxxxxx are the session's own characters (players, the hawks: seen 2026-10-27, npc 100000 at
            // the player's feet); map enemies carry a map-based handle (e.g. 0x3C2C2500_11000013).
            if let (true, true, Some(pos)) = (
                hp > min_hp && hp <= max_hp,
                handle >> 32 != 0xFFFF_FFFF,
                pos,
            ) {
                out.push(Mob {
                    chr,
                    handle,
                    npc: mem::read::<u32>(chr + super::layout::CHR_ID).unwrap_or(0),
                    hp,
                    pos,
                    open_field,
                });
            }
        }
    }
    Ok(out)
}

/// The ordinary mobs (HP up to [`MOB_MAX_HP`]).
fn mobs_alive() -> Result<Vec<Mob>, String> {
    enemies_alive(0, MOB_MAX_HP)
}

/// Bosses (HP above [`BOSS_MIN_HP`]): prefer the open-field set, where the world characters live; at the Roundtable the
/// plain `ChrSet` also holds NPCs with high HP (1939), so they are only a fallback.
fn bosses_alive() -> Result<Vec<Mob>, String> {
    let all = enemies_alive(BOSS_MIN_HP, u32::MAX)?;
    let field: Vec<Mob> = all.iter().filter(|m| m.open_field).cloned().collect();
    Ok(if field.is_empty() { all } else { field })
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// `mobs`: the nearest living enemies (handle, npc id, HP, distance).
pub(crate) fn mobs_cmd() -> Result<String, String> {
    let (_, phys) = local()?;
    let me = read3(phys + PHYSICS_POS).ok_or("position unreadable")?;
    let mut list = mobs_alive()?;
    list.sort_by(|a, b| dist(a.pos, me).total_cmp(&dist(b.pos, me)));
    let lines: Vec<String> = list
        .iter()
        .take(8)
        .map(|m| {
            format!(
                "{:#X} npc {} hp {} {:.0} m",
                m.handle,
                m.npc,
                m.hp,
                dist(m.pos, me)
            )
        })
        .collect();
    Ok(format!(
        "mobs: {} alive; nearest: {}",
        list.len(),
        lines.join("; ")
    ))
}

/// `boss_list`: the living bosses (HP above [`BOSS_MIN_HP`]) with handle, npc id, HP, world position and distance.
pub(crate) fn boss_list_cmd() -> Result<String, String> {
    let (chr, phys) = local()?;
    let me = read3(phys + PHYSICS_POS).ok_or("position unreadable")?;
    let origin = read3(chr + PLAYER_ORIGIN).unwrap_or([0.0; 3]);
    let mut list = bosses_alive()?;
    list.sort_by(|a, b| dist(a.pos, me).total_cmp(&dist(b.pos, me)));
    let lines: Vec<String> = list
        .iter()
        .take(8)
        .map(|m| {
            format!(
                "{:#X} npc {} hp {} at ({:.1}, {:.1}, {:.1}) {:.0} m{}",
                m.handle,
                m.npc,
                m.hp,
                m.pos[0] - origin[0],
                m.pos[1] - origin[1],
                m.pos[2] - origin[2],
                dist(m.pos, me),
                if m.open_field {
                    " [field]"
                } else {
                    " [chrset]"
                }
            )
        })
        .collect();
    Ok(format!(
        "boss_list: {} boss(es); nearest: {}",
        list.len(),
        lines.join("; ")
    ))
}

/// Move LOCAL next to the `skip`-th entry of a scanned list (1.5 m on the line towards where LOCAL is, facing it); with
/// `hp`, write that entry's HP on this game first (only the owner game makes it stick). `label` is the console command.
fn go_to_enemy(
    mut list: Vec<Mob>,
    hp: Option<u32>,
    skip: usize,
    label: &str,
) -> Result<String, String> {
    let (_, phys) = local()?;
    let me = read3(phys + PHYSICS_POS).ok_or("position unreadable")?;
    list.sort_by(|a, b| dist(a.pos, me).total_cmp(&dist(b.pos, me)));
    let m = list.get(skip).ok_or("no living enemy loaded")?;
    let d = dist(m.pos, me);
    if d > 300.0 {
        return Err(format!("the nearest enemy is {d:.0} m away (at most 300)"));
    }
    if let (Some(hp), Some(f)) = (hp, hp_field(m.chr)) {
        mem::write_bytes(f, &hp.to_le_bytes()).map_err(|e| e.to_string())?;
    }
    // Stand 1.5 m from it on the line towards where LOCAL is now, facing it.
    let (dx, dz) = (me[0] - m.pos[0], me[2] - m.pos[2]);
    let len = (dx * dx + dz * dz).sqrt().max(0.01);
    let (ux, uz) = (dx / len, dz / len);
    let np = [m.pos[0] + ux * 1.5, m.pos[1] + 0.5, m.pos[2] + uz * 1.5];
    let mut bytes = Vec::with_capacity(12);
    for v in np {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    mem::write_bytes(phys + PHYSICS_POS, &bytes).map_err(|e| e.to_string())?;
    let q = read_quat(phys);
    let yaw = (-ux).atan2(-uz).to_degrees();
    if yaw_deg(q).is_some() {
        let mut qb = Vec::with_capacity(16);
        for v in quat_of_yaw(yaw) {
            qb.extend_from_slice(&v.to_le_bytes());
        }
        mem::write_bytes(phys + PHYSICS_ORIENT, &qb).map_err(|e| e.to_string())?;
    }
    tracing::info!(
        "teleport: {label} next to enemy {:#X} (npc {}, hp {} -> {:?}), {d:.0} m, yaw {yaw:.0}",
        m.handle,
        m.npc,
        m.hp,
        hp
    );
    Ok(format!(
        "{label}: next to enemy {:#X} npc {} (hp {}{}), was {d:.0} m away, facing it",
        m.handle,
        m.npc,
        m.hp,
        hp.map_or(String::new(), |h| format!(" -> {h}"))
    ))
}

/// `mob_go [hp] [skip]`: put LOCAL 1.5 m from the nearest ordinary enemy (the `skip`-th nearest), facing it; with `hp`,
/// write that enemy's HP first (e.g. 1, so one hit kills; only the owner game makes it stick).
pub(crate) fn mob_go_cmd(args: &[String]) -> Result<String, String> {
    let usage = "usage: mob_go [hp] [skip]";
    let hp: Option<u32> = args
        .first()
        .map(|v| v.parse().map_err(|_| usage))
        .transpose()?;
    let skip: usize = args
        .get(1)
        .map(|v| v.parse().map_err(|_| usage))
        .transpose()?
        .unwrap_or(0);
    go_to_enemy(mobs_alive()?, hp, skip, "mob_go")
}

/// `boss_go [skip]`: put LOCAL 1.5 m from the nearest boss (HP above [`BOSS_MIN_HP`]), facing it.
pub(crate) fn boss_go_cmd(args: &[String]) -> Result<String, String> {
    let usage = "usage: boss_go [skip]";
    let skip: usize = args
        .first()
        .map(|v| v.parse().map_err(|_| usage))
        .transpose()?
        .unwrap_or(0);
    go_to_enemy(bosses_alive()?, None, skip, "boss_go")
}

/// Write LOCAL's handle into an entry's attacker fields (`+0x198` / `+0x1A0`, the fields a real hit by LOCAL fills;
/// MEMORY_MAP §21) and set its HP to 0, so this game runs the death as after a real LOCAL kill. `label` is the console
/// command. Dev only.
fn kill_enemy(list: Vec<Mob>, skip: usize, label: &str) -> Result<String, String> {
    let (chr, phys) = local()?;
    let handle = mem::read::<u64>(chr + super::layout::CHR_HANDLE)
        .filter(|h| *h != 0 && *h != u64::MAX)
        .ok_or("LOCAL's handle unreadable")?;
    let me = read3(phys + PHYSICS_POS).ok_or("position unreadable")?;
    let mut list = list;
    list.sort_by(|a, b| dist(a.pos, me).total_cmp(&dist(b.pos, me)));
    let m = list.get(skip).ok_or("no living enemy loaded")?;
    let f = hp_field(m.chr).ok_or("enemy HP field unreadable")?;
    for off in [
        super::layout::PLAYER_LAST_ATTACKER,
        super::layout::PLAYER_CUR_ATTACKER,
    ] {
        mem::write_bytes(m.chr + off, &handle.to_le_bytes()).map_err(|e| e.to_string())?;
    }
    mem::write_bytes(f, &0u32.to_le_bytes()).map_err(|e| e.to_string())?;
    let d = dist(m.pos, me);
    tracing::info!(
        "teleport: {label} enemy {:#X} (npc {}), {d:.0} m: hp {} -> 0, attacker -> LOCAL 0x{handle:X}",
        m.handle,
        m.npc,
        m.hp
    );
    Ok(format!(
        "{label}: enemy {:#X} npc {} at {d:.0} m: hp {} -> 0, last attacker -> LOCAL 0x{handle:X}",
        m.handle, m.npc, m.hp
    ))
}

/// `mob_kill [skip]`: RW-8 test aid on the ordinary mobs (see [`kill_enemy`]).
pub(crate) fn mob_kill_cmd(args: &[String]) -> Result<String, String> {
    let usage = "usage: mob_kill [skip]";
    let skip: usize = args
        .first()
        .map(|v| v.parse().map_err(|_| usage))
        .transpose()?
        .unwrap_or(0);
    kill_enemy(mobs_alive()?, skip, "mob_kill")
}

/// `boss_kill [skip]`: RW-5 test aid — the same on the nearest boss.
pub(crate) fn boss_kill_cmd(args: &[String]) -> Result<String, String> {
    let usage = "usage: boss_kill [skip]";
    let skip: usize = args
        .first()
        .map(|v| v.parse().map_err(|_| usage))
        .transpose()?
        .unwrap_or(0);
    kill_enemy(bosses_alive()?, skip, "boss_kill")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_three_numbers_only() {
        assert_eq!(
            parse_target(&s(&["1", "-2.5", "3e1"])),
            Ok(([1.0, -2.5, 30.0], None))
        );
        assert_eq!(
            parse_target(&s(&["1", "2", "3", "-170"])),
            Ok(([1.0, 2.0, 3.0], Some(-170.0)))
        );
        assert!(parse_target(&s(&["1", "2", "3", "400"])).is_err());
        assert!(parse_target(&s(&["1", "2", "3", "4", "5"])).is_err());
        assert!(parse_target(&s(&["1", "2"])).is_err());
        assert!(parse_target(&s(&["1", "2", "x"])).is_err());
        assert!(parse_target(&s(&["1", "2", "inf"])).is_err());
        assert!(parse_target(&s(&["NaN", "2", "3"])).is_err());
    }

    #[test]
    fn jump_is_bounded() {
        assert!(check_jump([0.0; 3], [10.0, 0.0, 10.0]).is_ok());
        assert!(check_jump([0.0; 3], [100.1, 0.0, 0.0]).is_err());
        assert!(check_jump([f32::NAN, 0.0, 0.0], [0.0; 3]).is_err());
    }

    #[test]
    fn world_and_physics_round_trip() {
        let o = [24.0, 0.0, -32.0];
        let w = [1.5, 2.0, 3.5];
        assert_eq!(world_of(phys_of(w, o), o), w);
    }

    #[test]
    fn yaw_of_a_y_rotation() {
        let h = std::f32::consts::FRAC_PI_4; // 90 degrees about y
        let y = yaw_deg([0.0, h.sin(), 0.0, h.cos()]).unwrap();
        assert!((y - 90.0).abs() < 0.01);
        assert_eq!(yaw_deg([0.0, 0.0, 0.0, 0.0]), None);
        for y in [-170.0f32, -90.0, 0.0, 45.0, 179.0] {
            assert!((yaw_deg(quat_of_yaw(y)).unwrap() - y).abs() < 0.01, "{y}");
        }
        assert_eq!(yaw_deg([0.7, 0.0, 0.0, 0.7]), None);
    }
}
