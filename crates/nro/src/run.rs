//! The scenario runner: stage → preflight → launch → steps → report.json → teardown.

use nro_core::scenario::{Scenario, Step};
use serde::Serialize;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::session::{self, Stage, State};

#[derive(Debug, Serialize)]
pub struct StepResult {
    pub step: String,
    pub ok: bool,
    pub detail: String,
    pub secs: f64,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub scenario: String,
    pub run: String,
    /// PASS | FAIL | ABORTED | REFUSED
    pub verdict: String,
    pub steps: Vec<StepResult>,
    pub note: String,
}

/// Title screen → loaded world (the Roundtable): any key on the title, OK on the notice, Continue, then the load. Repeats until
/// the console says a world is loaded.
pub fn title_to_world(st: &State, timeout: Duration) -> Result<String, String> {
    let t = Instant::now();
    let pid = st.live_pid().ok_or("the game is not running")?;
    while t.elapsed() < timeout {
        if let Ok(s) = session::cmd(st, "status") {
            if s.contains("world loaded") {
                return Ok(s);
            }
        }
        // "Press any button" (any key), the "previous session did not end" notice's OK (the session before was killed, which is
        // how every orchestrated session ends), then Continue on the main menu. Mouse clicks: the menus ignore injected keys.
        // Positions are fractions of the client area (1.03.3 title layout at 16:9).
        let _ = crate::desk::request(serde_json::json!({"op": "keys", "pid": pid,
            "seq": "ENTER; wait 2s; click 0.5 0.66; wait 2s; click 0.5 0.749; wait 3s"}));
    }
    Err(format!("no world after {} s", timeout.as_secs()))
}

/// Roundtable → expedition (offline mode: Commence Solo starts at once), then off the start hawk. From the Roundtable spawn point
/// (facing the table, 1.03.3): walk 5 s, F (interact: the expedition menu), F (target), F (Commence Solo Expedition); wait for the
/// local team value 2 (expedition, MEMORY_MAP §12); then Alt (disembark) and a few seconds to land.
pub fn to_expedition(st: &State, timeout: Duration) -> Result<String, String> {
    let pid = st.live_pid().ok_or("the game is not running")?;
    let keys =
        |seq: &str| crate::desk::request(serde_json::json!({"op": "keys", "pid": pid, "seq": seq}));
    let s = session::cmd(st, "status")?;
    if !s.contains("team_type 2") {
        if !s.contains("team_type 1") {
            return Err(format!("not in the Roundtable: {s}"));
        }
        // The Number of Players setting is a ring of three values that the game keeps between runs (and a friend session or a
        // player can leave it on Duo or Trio): "Matchmaking failed" otherwise. Attempt 1 takes it as it is, each later attempt
        // dismisses the dialog and steps the ring once, so all three values are tried (Matchmaking Settings tab = F2, Target = F1).
        let t = Instant::now();
        let mut started = false;
        for attempt in 0..3 {
            let step = if attempt == 0 {
                "W 5000ms; wait 1s; F; wait 2s; F2; wait 1s; F1; wait 1s; F; wait 2s; F; wait 3s"
            } else {
                "ENTER; wait 1s; F; wait 2s; F2; wait 1s; RIGHT; wait 1s; F1; wait 1s; F; wait 2s; F; wait 3s"
            };
            keys(step)?;
            let try_until = Instant::now() + Duration::from_secs(25);
            while Instant::now() < try_until {
                if session::cmd(st, "status").is_ok_and(|s| s.contains("team_type 2")) {
                    started = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            if started || t.elapsed() > timeout {
                break;
            }
        }
        if !started {
            return Err("the expedition did not start".into());
        }
        std::thread::sleep(Duration::from_secs(6));
        keys("ALT; wait 10s")?;
    }
    session::cmd(st, "status")
}

fn step_name(s: &Step) -> String {
    match s {
        Step::Console(l) => format!("console {l}"),
        Step::Wait(w) => format!("wait {} ~ {:?}", w.console, w.contains.needles()),
        Step::Expect(e) => format!("expect {} ~ {:?}", e.console, e.contains.needles()),
        Step::Keys(k) => format!("keys {k}"),
        Step::Navigate(n) => format!("navigate {n}"),
        Step::SleepMs(m) => format!("sleep {m} ms"),
        Step::Screenshot(n) => format!("screenshot {n}"),
        Step::Puppets(n) => format!("puppets {n}"),
        Step::PuppetRing(r) => format!("puppet_ring {r}"),
        Step::PuppetFront(d) => format!("puppet_front {d}"),
    }
}

/// Run one step. `Err` = the run cannot go on; `Ok((false, ..))` = an expectation failed (the run goes on, verdict FAIL).
pub fn step(st: &State, s: &Step) -> Result<(bool, String), String> {
    match s {
        Step::Console(l) => session::cmd(st, l).map(|r| (true, r)),
        Step::Wait(w) => {
            let t = Instant::now();
            let mut last = String::new();
            while t.elapsed() < Duration::from_secs(w.timeout_s) {
                if st.live_pid().is_none() {
                    return Err("the game exited".into());
                }
                if let Ok(r) = session::cmd(st, &w.console) {
                    if w.contains.all_in(&r) {
                        return Ok((true, r));
                    }
                    last = r;
                }
                std::thread::sleep(Duration::from_millis(w.every_ms));
            }
            Err(format!(
                "timed out after {} s; last reply: {last}",
                w.timeout_s
            ))
        }
        Step::Expect(e) => {
            let r = session::cmd(st, &e.console)?;
            Ok((e.contains.all_in(&r) && e.absent.none_in(&r), r))
        }
        Step::Keys(k) => {
            let pid = st.live_pid().ok_or("the game is not running")?;
            crate::desk::request(serde_json::json!({"op": "keys", "pid": pid, "seq": k}))
                .map(|_| (true, String::new()))
        }
        Step::Navigate(n) => match n.as_str() {
            "title_to_world" => title_to_world(st, Duration::from_secs(300)).map(|r| (true, r)),
            "expedition" => to_expedition(st, Duration::from_secs(240)).map(|r| (true, r)),
            other => Err(format!(
                "unknown navigate target {other:?} (known: title_to_world, expedition)"
            )),
        },
        Step::SleepMs(m) => {
            std::thread::sleep(Duration::from_millis(*m));
            Ok((true, String::new()))
        }
        Step::Puppets(n) => crate::ops::puppets(*n as usize)
            .map(|v| (true, v["list"].as_str().unwrap_or_default().to_string())),
        Step::PuppetRing(r) => crate::ops::puppet_ring(*r).map(|v| (true, v.to_string())),
        Step::PuppetFront(d) => crate::ops::puppet_front(*d).map(|v| (true, v.to_string())),
        Step::Screenshot(n) => {
            let pid = st.live_pid().ok_or("the game is not running")?;
            let p = st.dir().join("shots").join(format!("{n}.png"));
            crate::desk::request(
                serde_json::json!({"op": "shot", "pid": pid, "path": p.display().to_string()}),
            )
            .map(|v| (true, v.as_str().unwrap_or_default().to_string()))
        }
    }
}

pub fn run(file: &std::path::Path, mod_dist: Option<PathBuf>) -> Result<Report, String> {
    let sc = Scenario::parse(
        &std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?,
    )?;
    let pins = session::pins()?;
    let mut st = session::stage(
        &pins,
        &Stage {
            name: sc.name.clone(),
            mod_dist,
            config: sc.config.clone(),
            no_gamedata: false,
        },
    )?;
    let mut rep = Report {
        scenario: sc.name.clone(),
        run: st.dir.clone(),
        verdict: String::new(),
        steps: Vec::new(),
        note: String::new(),
    };
    let pf = session::launch(&pins, &mut st)?;
    if !pf.ok {
        rep.verdict = "REFUSED".into();
        rep.note = st.note.clone();
        write(&st, &rep);
        return Ok(rep);
    }
    let mut failed = false;
    let mut aborted = None;
    match session::wait_console(&st, Duration::from_secs(180)) {
        Err(e) => aborted = Some(format!("console: {e}")),
        Ok(()) => {
            for s in &sc.steps {
                let t = Instant::now();
                let r = step(&st, s);
                let secs = t.elapsed().as_secs_f64();
                match r {
                    Ok((ok, detail)) => {
                        failed |= !ok;
                        rep.steps.push(StepResult {
                            step: step_name(s),
                            ok,
                            detail,
                            secs,
                        });
                    }
                    Err(e) => {
                        rep.steps.push(StepResult {
                            step: step_name(s),
                            ok: false,
                            detail: e.clone(),
                            secs,
                        });
                        aborted = Some(e);
                        break;
                    }
                }
            }
        }
    }
    // Evidence before teardown: the mod's log tail.
    if st.live_pid().is_some() {
        if let Ok(t) = session::cmd(&st, "log 80") {
            let _ = std::fs::write(st.dir().join("log-tail.txt"), t);
        }
    }
    if sc.teardown.as_deref() != Some("keep") {
        rep.note = session::stop(&mut st);
    }
    let st2 = State::load(&st.dir()).unwrap_or(st.clone());
    rep.verdict = if st2.phase == "aborted" {
        rep.note = format!("{} | {}", st2.note, rep.note);
        "ABORTED".into()
    } else if let Some(a) = aborted {
        rep.note = format!("{a} | {}", rep.note);
        "ABORTED".into()
    } else if failed {
        "FAIL".into()
    } else {
        "PASS".into()
    };
    write(&st, &rep);
    Ok(rep)
}

fn write(st: &State, rep: &Report) {
    let _ = std::fs::write(
        st.dir().join("report.json"),
        serde_json::to_string_pretty(rep).unwrap(),
    );
}
