//! The operations both front ends expose (CLI `nro <op>`, MCP tool `<op>`). Each returns JSON.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::session::{self, Stage, State};
use crate::{run, sys};

pub type R = Result<Value, String>;

fn cur() -> Result<State, String> {
    State::current()
}

pub fn env() -> R {
    let pins = session::pins()?;
    let interesting: Vec<Value> = sys::processes()
        .into_iter()
        .filter(|p| {
            let n = p.name.to_ascii_lowercase();
            n == "nightreign.exe"
                || n == "steam.exe"
                || n.starts_with("me3")
                || n.contains("easyanticheat")
                || n == "start_protected_game.exe"
        })
        .map(|p| json!({"pid": p.pid, "name": p.name, "path": p.path}))
        .collect();
    let cur = State::current().ok();
    Ok(json!({
        "home": session::home().display().to_string(),
        "game_dir": pins.game_dir, "me3_dir": pins.me3_dir,
        "processes": interesting,
        "session": cur.map(|s| json!({"id": s.id, "dir": s.dir, "phase": s.phase, "game_pid": s.live_pid(), "note": s.note})),
    }))
}

fn stage_opts(
    name: &str,
    mod_dist: Option<String>,
    config: BTreeMap<String, toml::Value>,
) -> Stage {
    Stage {
        name: name.into(),
        mod_dist: mod_dist.map(PathBuf::from),
        config,
        no_gamedata: false,
    }
}

/// Stage a run folder and evaluate preflight without launching.
pub fn preflight(mod_dist: Option<String>) -> R {
    let pins = session::pins()?;
    let st = session::stage(&pins, &stage_opts("preflight", mod_dist, BTreeMap::new()))?;
    let r = session::preflight(&pins, &st);
    Ok(json!({"run": st.dir, "report": r}))
}

pub fn launch(
    mod_dist: Option<String>,
    config: BTreeMap<String, toml::Value>,
    to_world: bool,
) -> R {
    if let Ok(s) = State::current() {
        if s.live_pid().is_some() {
            return Err(format!(
                "a session is already running (game pid {:?}); nro stop first",
                s.game_pid
            ));
        }
    }
    let pins = session::pins()?;
    let mut st = session::stage(&pins, &stage_opts("session", mod_dist, config))?;
    let r = session::launch(&pins, &mut st)?;
    if !r.ok {
        return Ok(json!({"launched": false, "run": st.dir, "refused": st.note, "report": r}));
    }
    session::wait_console(&st, Duration::from_secs(180))?;
    let mut world = Value::Null;
    if to_world {
        world = json!(run::title_to_world(&st, Duration::from_secs(300))?);
    }
    Ok(
        json!({"launched": true, "run": st.dir, "game_pid": st.game_pid, "warnings": r.warnings, "world": world}),
    )
}

pub fn status() -> R {
    let st = cur()?;
    let console = st
        .live_pid()
        .map(|_| session::cmd(&st, "status").unwrap_or_else(|e| format!("console: {e}")));
    Ok(
        json!({"id": st.id, "dir": st.dir, "phase": st.phase, "note": st.note, "game_pid": st.live_pid(), "console": console}),
    )
}

pub fn console(line: &str) -> R {
    let st = cur()?;
    Ok(json!(session::cmd(&st, line)?))
}

pub fn keys(seq: &str) -> R {
    let st = cur()?;
    let s = nro_core::scenario::Step::Keys(seq.into());
    run::step(&st, &s).map(|_| json!("ok"))
}

pub fn navigate(to: &str) -> R {
    let st = cur()?;
    run::step(&st, &nro_core::scenario::Step::Navigate(to.into())).map(|(_, d)| json!(d))
}

pub fn screenshot(name: &str) -> R {
    let st = cur()?;
    run::step(&st, &nro_core::scenario::Step::Screenshot(name.into())).map(|(_, d)| json!(d))
}

/// The tail of the mod's own log file in the run folder (works even when the console is gone).
pub fn log_tail(n: usize, filter: Option<&str>) -> R {
    let st = cur()?;
    let t = std::fs::read_to_string(st.dir().join("mod").join("nrpvp.log"))
        .map_err(|e| format!("nrpvp.log: {e}"))?;
    let lines: Vec<&str> = t
        .lines()
        .filter(|l| filter.is_none_or(|f| l.contains(f)))
        .collect();
    Ok(json!(lines[lines.len().saturating_sub(n)..].join("\n")))
}

pub fn stop() -> R {
    let mut st = cur()?;
    Ok(json!(session::stop(&mut st)))
}

pub fn run_scenario(file: &str, mod_dist: Option<String>) -> R {
    let p = PathBuf::from(file);
    let p = if p.is_absolute() {
        p
    } else {
        session::home().join(p)
    };
    Ok(serde_json::to_value(run::run(&p, mod_dist.map(PathBuf::from))?).unwrap())
}

pub fn scenarios() -> R {
    let dir = session::home().join("scenarios");
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        if let Ok(t) = std::fs::read_to_string(e.path()) {
            if let Ok(s) = nro_core::scenario::Scenario::parse(&t) {
                out.push(json!({"file": format!("scenarios/{}", e.file_name().to_string_lossy()), "name": s.name, "about": s.about}));
            }
        }
    }
    Ok(json!(out))
}

fn count_puppets(list: &str) -> usize {
    list.matches(" PUPPET ").count()
}

/// `puppets <n>`: make n puppet players (1..=5) in the running session (dev build with ORCH-2, offline mode, a world loaded).
/// The game's bot manager makes 2 characters per request; each request is retried when nothing comes within 15 s (the manager
/// only spawns once the player stands on the ground).
pub fn puppets(n: usize) -> R {
    if !(1..=5).contains(&n) {
        return Err("1 to 5 puppets (the player list has 6 slots, slot 0 is you)".into());
    }
    let st = cur()?;
    session::cmd(&st, &format!("puppet_max {n}"))?;
    session::cmd(&st, "puppet_arm 1")?;
    let t0 = std::time::Instant::now();
    let mut asked = std::time::Instant::now() - Duration::from_secs(60);
    loop {
        let list = session::cmd(&st, "puppet_list")?;
        let have = count_puppets(&list);
        if have >= n {
            return Ok(json!({"puppets": have, "list": list}));
        }
        if t0.elapsed() > Duration::from_secs(180) {
            return Err(format!(
                "only {have} of {n} puppets after 180 s:
{list}"
            ));
        }
        if asked.elapsed() > Duration::from_secs(15) {
            let _ = session::cmd(&st, "bots_clear");
            std::thread::sleep(Duration::from_millis(600));
            let r = session::cmd(&st, "bots 10")?;
            if r.contains("need the offline mode") || r.contains("no world") {
                return Err(r);
            }
            asked = std::time::Instant::now();
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// `puppet_ring [radius]`: put every puppet on a circle around the player (for hits and screenshots).
pub fn puppet_ring(radius: f32) -> R {
    let st = cur()?;
    let list = session::cmd(&st, "puppet_list")?;
    let slots: Vec<usize> = list
        .lines()
        .filter(|l| l.contains(" PUPPET "))
        .filter_map(|l| {
            l.trim()
                .strip_prefix("slot ")?
                .split(':')
                .next()?
                .parse()
                .ok()
        })
        .collect();
    let mut out = Vec::new();
    for (i, s) in slots.iter().enumerate() {
        let a = i as f32 / slots.len().max(1) as f32 * std::f32::consts::TAU;
        out.push(session::cmd(
            &st,
            &format!(
                "puppet_move {s} {:.2} {:.2}",
                radius * a.cos(),
                radius * a.sin()
            ),
        )?);
    }
    Ok(json!(out))
}

fn local_pos(list: &str) -> Option<(f32, f32)> {
    let l = list.lines().find(|l| l.contains(" LOCAL "))?;
    let p = l.split("pos (").nth(1)?.split(')').next()?;
    let v: Vec<f32> = p.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (v.len() == 3).then(|| (v[0], v[2]))
}

/// `puppet_front [dist]`: put every puppet `dist` metres in front of the player (side by side, 0.5 m apart). The facing comes from
/// a short step forward (W for 400 ms): the direction the player moved.
pub fn puppet_front(dist: f32) -> R {
    let st = cur()?;
    let pid = st.live_pid().ok_or("the game is not running")?;
    let a = local_pos(&session::cmd(&st, "puppet_list")?).ok_or("no LOCAL position")?;
    crate::desk::request(json!({"op": "keys", "pid": pid, "seq": "W 400ms; wait 600ms"}))?;
    let list = session::cmd(&st, "puppet_list")?;
    let b = local_pos(&list).ok_or("no LOCAL position")?;
    let (dx, dz) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dz * dz).sqrt();
    if len < 0.05 {
        return Err("the player did not move on W (menu open, or blocked): facing unknown".into());
    }
    let (fx, fz) = (dx / len, dz / len);
    let slots: Vec<usize> = list
        .lines()
        .filter(|l| l.contains(" PUPPET "))
        .filter_map(|l| {
            l.trim()
                .strip_prefix("slot ")?
                .split(':')
                .next()?
                .parse()
                .ok()
        })
        .collect();
    let mut out = Vec::new();
    for (i, s) in slots.iter().enumerate() {
        let side = (i as f32 - (slots.len() as f32 - 1.0) / 2.0) * 0.5;
        let (ox, oz) = (fx * dist - fz * side, fz * dist + fx * side);
        out.push(session::cmd(
            &st,
            &format!("puppet_move {s} {ox:.2} {oz:.2}"),
        )?);
    }
    Ok(json!({"facing": [fx, fz], "moved": out}))
}
