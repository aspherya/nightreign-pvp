//! One test session: stage the mod into a run folder, preflight, launch through me3, check me3's attach config, watch the game,
//! stop it. State lives in the run folder (`session.json`), so separate `nro` invocations (and the MCP server) share it.
//!
//! Run folder layout:
//! ```text
//! runs\<id>\mod\            staged nrpvp.dll / .pdb / nrpvp.toml / gamedata\ (the mod logs next to its DLL: mod\nrpvp.log, mod\logs\)
//! runs\<id>\profile.me3.toml
//! runs\<id>\me3.log         me3's stdout + stderr (attach config line = check L1)
//! runs\<id>\preflight.json  the report the launch was decided on
//! runs\<id>\console.log     every console line nro sent and its reply
//! runs\<id>\watch.log       watchdog: processes, the game's TCP endpoints, the end-of-run save check
//! runs\<id>\shots\*.png
//! runs\<id>\session.json
//! ```

use nro_core::me3log;
use nro_core::pins::Pins;
use nro_core::preflight::{self, Facts, Report};
use nro_core::profile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::{console, sys};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    pub id: String,
    pub dir: String,
    pub me3_pid: Option<u32>,
    pub game_pid: Option<u32>,
    pub watch_pid: Option<u32>,
    /// "staged" | "refused" | "launching" | "running" | "ended" | "aborted"
    pub phase: String,
    pub note: String,
    pub started_unix: u64,
    /// Vanilla save files and their hashes before launch.
    pub vanilla_saves: BTreeMap<String, String>,
}

pub fn home() -> PathBuf {
    std::env::var_os("NRO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            strip_unc(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("..")
                    .canonicalize()
                    .unwrap_or_else(|_| ".".into()),
            )
        })
}

fn strip_unc(p: PathBuf) -> PathBuf {
    let s = p.display().to_string();
    s.strip_prefix(r"\\?\").map(PathBuf::from).unwrap_or(p)
}

pub fn pins() -> Result<Pins, String> {
    let file = home().join("pins").join("nightreign.toml");
    match fs::read_to_string(&file) {
        Ok(t) => Pins::parse(&t),
        Err(_) => Pins::parse(nro_core::pins::EMBEDDED),
    }
}

pub fn runs_dir() -> PathBuf {
    home().join("runs")
}

impl State {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join("session.json")
    }
    pub fn load(dir: &Path) -> Result<Self, String> {
        let t =
            fs::read_to_string(Self::path(dir)).map_err(|e| format!("{}: {e}", dir.display()))?;
        serde_json::from_str(&t).map_err(|e| e.to_string())
    }
    pub fn save(&self) -> Result<(), String> {
        let dir = PathBuf::from(&self.dir);
        fs::write(
            Self::path(&dir),
            serde_json::to_string_pretty(self).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        fs::write(runs_dir().join("current.txt"), &self.dir).map_err(|e| e.to_string())
    }
    pub fn current() -> Result<Self, String> {
        let dir = fs::read_to_string(runs_dir().join("current.txt"))
            .map_err(|_| "no session yet (nro launch)".to_string())?;
        Self::load(Path::new(dir.trim()))
    }
    pub fn dir(&self) -> PathBuf {
        PathBuf::from(&self.dir)
    }
    /// The game process, if it is still running.
    pub fn live_pid(&self) -> Option<u32> {
        self.game_pid.filter(|&p| sys::alive(p))
    }
    pub fn log(&self, file: &str, line: &str) {
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir().join(file))
        {
            let _ = writeln!(f, "{} {line}", now_stamp());
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn now_stamp() -> String {
    let s = now_unix();
    let (h, m, sec) = ((s / 3600) % 24, (s / 60) % 60, s % 60);
    format!("{h:02}:{m:02}:{sec:02}Z")
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for e in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let t = to.join(e.file_name());
        if e.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_dir(&e.path(), &t)?;
        } else {
            fs::copy(e.path(), &t).map_err(|er| format!("copy {}: {er}", e.path().display()))?;
        }
    }
    Ok(())
}

/// Options for one launch.
#[derive(Debug, Clone, Default)]
pub struct Stage {
    /// A short name for the run folder.
    pub name: String,
    /// The mod build to test (folder with nrpvp.dll, nrpvp.toml, gamedata\). Default: `<mod repo>\dist`.
    pub mod_dist: Option<PathBuf>,
    /// Keys written over the staged nrpvp.toml.
    pub config: BTreeMap<String, toml::Value>,
    /// Load the game-data package (default true when the dist has one).
    pub no_gamedata: bool,
}

/// The keys every orchestrated run forces (preflight P7 checks the result).
fn forced_config() -> BTreeMap<String, toml::Value> {
    let mut c = BTreeMap::new();
    c.insert("dev_mode".into(), toml::Value::Boolean(true));
    c.insert("mm_queue".into(), toml::Value::Boolean(false));
    c.insert("br_menu".into(), toml::Value::Boolean(false));
    c
}

pub fn stage(pins: &Pins, opt: &Stage) -> Result<State, String> {
    let id = format!(
        "{}-{}",
        now_unix(),
        if opt.name.is_empty() {
            "run"
        } else {
            &opt.name
        }
    );
    let dir = runs_dir().join(&id);
    let m = dir.join("mod");
    fs::create_dir_all(&m).map_err(|e| e.to_string())?;
    let dist = opt.mod_dist.clone().unwrap_or_else(|| home().join("dist"));
    for f in ["nrpvp.dll", "nrpvp.pdb"] {
        let src = dist.join(f);
        if src.is_file() {
            fs::copy(&src, m.join(f)).map_err(|e| format!("copy {}: {e}", src.display()))?;
        } else if f == "nrpvp.dll" {
            return Err(format!(
                "{} missing (build the mod first: scripts\\build.ps1)",
                src.display()
            ));
        }
    }
    let base = fs::read_to_string(dist.join("nrpvp.toml")).unwrap_or_default();
    let mut cfg = opt.config.clone();
    cfg.extend(forced_config());
    fs::write(
        m.join("nrpvp.toml"),
        nro_core::scenario::apply_config(&base, &cfg),
    )
    .map_err(|e| e.to_string())?;
    let gd = dist.join("gamedata");
    let with_gd = !opt.no_gamedata && gd.join("msg").is_dir();
    if with_gd {
        copy_dir(&gd, &m.join("gamedata"))?;
    }
    let nrsc = PathBuf::from(&pins.game_dir)
        .join("SeamlessCoop")
        .join("nrsc.dll");
    let gdp = m.join("gamedata").display().to_string();
    let prof = profile::text(
        &nrsc.display().to_string(),
        &m.join("nrpvp.dll").display().to_string(),
        with_gd.then_some(gdp.as_str()),
    );
    fs::write(dir.join("profile.me3.toml"), prof).map_err(|e| e.to_string())?;
    let st = State {
        id,
        dir: dir.display().to_string(),
        phase: "staged".into(),
        started_unix: now_unix(),
        ..Default::default()
    };
    st.save()?;
    Ok(st)
}

pub fn vanilla_saves() -> Option<BTreeMap<String, String>> {
    let root = PathBuf::from(std::env::var_os("APPDATA")?).join("Nightreign");
    let mut out = BTreeMap::new();
    for d in fs::read_dir(&root).ok()?.flatten() {
        if !d.path().is_dir() {
            continue;
        }
        for f in fs::read_dir(d.path()).ok()?.flatten() {
            let n = f.file_name().to_string_lossy().to_ascii_lowercase();
            if n.starts_with("nr0000.sl2") {
                out.insert(f.path().display().to_string(), sys::sha256(&f.path())?);
            }
        }
    }
    Some(out)
}

/// The me3 command line nro uses (P1: the only one).
fn me3_args(pins: &Pins, dir: &Path) -> Vec<String> {
    vec![
        "launch".into(),
        "-g".into(),
        "nightreign".into(),
        "-e".into(),
        PathBuf::from(&pins.game_dir)
            .join("nightreign.exe")
            .display()
            .to_string(),
        "-p".into(),
        dir.join("profile.me3.toml").display().to_string(),
    ]
}

pub fn facts(pins: &Pins, st: &State) -> Facts {
    let dir = st.dir();
    let profile = fs::read_to_string(dir.join("profile.me3.toml")).unwrap_or_default();
    let all = me3_args(pins, &dir);
    let me3 = PathBuf::from(&pins.me3_dir).join("me3.exe");
    let me3_version = Command::new(&me3)
        .arg("--version")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    Facts {
        game_hashes: pins
            .sha256
            .keys()
            .map(|k| {
                (
                    k.clone(),
                    sys::sha256(&PathBuf::from(&pins.game_dir).join(k)),
                )
            })
            .collect(),
        me3_hashes: pins
            .me3
            .files
            .keys()
            .map(|k| {
                (
                    k.clone(),
                    sys::sha256(&PathBuf::from(&pins.me3_dir).join(k)),
                )
            })
            .collect(),
        me3_version,
        processes: sys::processes(),
        profile_files: profile::paths(&profile)
            .into_iter()
            .map(|p| (p.clone(), Path::new(&p).exists()))
            .collect(),
        profile,
        me3_args: all[7..].to_vec(),
        mod_toml: fs::read_to_string(dir.join("mod").join("nrpvp.toml")).ok(),
        vanilla_saves_hashed: vanilla_saves().is_some(),
    }
}

pub fn preflight(pins: &Pins, st: &State) -> Report {
    let r = preflight::evaluate(pins, &facts(pins, st));
    let _ = fs::write(
        st.dir().join("preflight.json"),
        serde_json::to_string_pretty(&r).unwrap(),
    );
    r
}

/// Preflight, then launch. Refuses (and changes nothing else) when any check fails.
/// Our own stdin/stdout/stderr must not be inherited by me3, the game or the watchdog: a shell or an MCP client reading our output
/// would otherwise wait for those long-lived processes to close it.
fn no_std_inherit() {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for h in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: clearing the inherit flag on our own standard handles; invalid handles make the call fail harmlessly.
        unsafe { SetHandleInformation(GetStdHandle(h), HANDLE_FLAG_INHERIT, 0) };
    }
}

pub fn launch(pins: &Pins, st: &mut State) -> Result<Report, String> {
    no_std_inherit();
    let r = preflight(pins, st);
    if !r.ok {
        st.phase = "refused".into();
        st.note = r
            .checks
            .iter()
            .filter(|c| !c.ok)
            .map(|c| format!("{}: {}", c.id, c.detail))
            .collect::<Vec<_>>()
            .join(" | ");
        st.save()?;
        return Ok(r);
    }
    st.vanilla_saves = vanilla_saves().unwrap_or_default();
    let dir = st.dir();
    let log = fs::File::create(dir.join("me3.log")).map_err(|e| e.to_string())?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    let child = Command::new(PathBuf::from(&pins.me3_dir).join("me3.exe"))
        .args(me3_args(pins, &dir))
        .current_dir(&pins.game_dir)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log2)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("could not start me3: {e}"))?;
    st.me3_pid = Some(child.id());
    st.phase = "launching".into();
    st.save()?;

    // The game process: the nightreign.exe that was not there at preflight (P5 guaranteed none was).
    let exe = PathBuf::from(&pins.game_dir)
        .join("nightreign.exe")
        .display()
        .to_string()
        .to_ascii_lowercase();
    let t0 = Instant::now();
    let pid = loop {
        if let Some(p) = sys::processes().into_iter().find(|p| {
            p.name.eq_ignore_ascii_case("nightreign.exe")
                && p.path.as_deref().map(str::to_ascii_lowercase) == Some(exe.clone())
        }) {
            break p.pid;
        }
        if t0.elapsed() > Duration::from_secs(90) {
            st.phase = "aborted".into();
            st.note = "the game process did not appear within 90 s (see me3.log)".into();
            st.save()?;
            return Err(st.note.clone());
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    st.game_pid = Some(pid);
    st.save()?;

    // L1: me3's attach config.
    let expected =
        profile::paths(&fs::read_to_string(dir.join("profile.me3.toml")).unwrap_or_default());
    let t1 = Instant::now();
    loop {
        let text = fs::read_to_string(dir.join("me3.log")).unwrap_or_default();
        if let Some(a) = me3log::attach(&text) {
            if let Err(why) = me3log::verify(&a, &expected) {
                sys::kill(pid);
                st.phase = "aborted".into();
                st.note = format!("L1 safety: {why}; the game was killed");
                st.save()?;
                return Err(st.note.clone());
            }
            st.log(
                "watch.log",
                "L1 ok: me3 attach config start_online = false, modules as staged",
            );
            break;
        }
        if t1.elapsed() > Duration::from_secs(30) {
            sys::kill(pid);
            st.phase = "aborted".into();
            st.note =
                "L1 safety: me3 printed no attach config within 30 s; the game was killed".into();
            st.save()?;
            return Err(st.note.clone());
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    // Watchdog: a detached `nro watch` that outlives this command.
    // From a copy in the run folder, so the watchdog never locks the nro.exe a rebuild wants to replace.
    // The CLI binary does the watching (`nro watch`); the MCP server finds it next to itself.
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let me = me.with_file_name("nro.exe");
    if !me.is_file() {
        return Err(format!(
            "{} not found (build the whole workspace)",
            me.display()
        ));
    }
    let copy = dir.join("nro-watch.exe");
    let me = if fs::copy(&me, &copy).is_ok() {
        copy
    } else {
        me
    };
    let w = Command::new(me)
        .args(["watch", &st.dir])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .map_err(|e| format!("could not start the watchdog: {e}"))?;
    st.watch_pid = Some(w.id());
    st.phase = "running".into();
    st.save()?;
    Ok(r)
}

/// Wait until the mod's console answers (the DLL is loaded and its pipe is up).
pub fn wait_console(st: &State, timeout: Duration) -> Result<(), String> {
    let pid = st.game_pid.ok_or("no game")?;
    let t = Instant::now();
    while t.elapsed() < timeout {
        if !sys::alive(pid) {
            return Err("the game exited".into());
        }
        if console::exists(pid) && console::ask(pid, "whoami", Duration::from_secs(2)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(format!(
        "the console did not answer within {} s",
        timeout.as_secs()
    ))
}

/// Send a console line, recording it in console.log.
pub fn cmd(st: &State, line: &str) -> Result<String, String> {
    let pid = st.live_pid().ok_or("the game is not running")?;
    let r = console::ask(pid, line, Duration::from_secs(10));
    match &r {
        Ok(t) => st.log("console.log", &format!("> {line}\n{t}")),
        Err(e) => st.log("console.log", &format!("> {line}\n!! {e}")),
    }
    r
}

pub fn stop(st: &mut State) -> String {
    let mut msg = Vec::new();
    if let Some(p) = st.live_pid() {
        msg.push(format!("game pid {p} killed: {}", sys::kill(p)));
    }
    if let Some(p) = st.me3_pid.filter(|&p| sys::alive(p)) {
        msg.push(format!("me3 pid {p} killed: {}", sys::kill(p)));
    }
    st.phase = "ended".into();
    let _ = st.save();
    msg.join("; ")
}

/// `nro watch <dir>`: every 2 s until the game exits. Kills the game if EasyAntiCheat or the anti-cheat launcher shows up;
/// records every remote TCP endpoint the game uses (L2 measurement); after exit, checks the vanilla saves are unchanged.
pub fn watch(dir: &Path) -> Result<(), String> {
    let mut st = State::load(dir)?;
    let pid = st.game_pid.ok_or("no game pid")?;
    let mut seen = std::collections::BTreeSet::new();
    st.log("watch.log", &format!("watchdog on game pid {pid}"));
    while sys::alive(pid) {
        for p in sys::processes() {
            let n = p.name.to_ascii_lowercase();
            if n.contains("easyanticheat") || n == "start_protected_game.exe" {
                sys::kill(pid);
                st.phase = "aborted".into();
                st.note = format!(
                    "safety: {} (pid {}) appeared; the game was killed",
                    p.name, p.pid
                );
                st.log("watch.log", &st.note);
                st.save()?;
            }
        }
        for ep in crate::net::tcp_remotes(pid) {
            if seen.insert(ep.clone()) {
                st.log("watch.log", &format!("tcp remote {ep}"));
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    let after = vanilla_saves().unwrap_or_default();
    let same = after == st.vanilla_saves;
    st.log(
        "watch.log",
        &format!("game exited; vanilla saves unchanged: {same}"),
    );
    let mut st = State::load(dir).unwrap_or(st);
    if st.phase == "running" || st.phase == "launching" {
        st.phase = "ended".into();
    }
    if !same {
        st.note = format!("{} | vanilla saves CHANGED", st.note);
    }
    st.save()
}
