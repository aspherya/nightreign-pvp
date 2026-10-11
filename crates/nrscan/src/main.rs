//! nrscan (T-037/T-038): external memory scanner for the Nightreign bleed-out clock and revive bar.
//! Read-only (PROCESS_QUERY_INFORMATION | PROCESS_VM_READ) except `poke` and `clock --poke-*`, which additionally open the
//! process with PROCESS_VM_WRITE | PROCESS_VM_OPERATION and refuse any exe not under `\Nucleus\content\` unless `--i-know`.

mod changed;
mod cmds;
mod explain;
mod filters;
mod logparse;
mod mem;
mod poke;
mod report;
mod route;
mod rtti;
mod scan;
mod winmem;

use logparse::{parse_state_line, LogInfo, StateKind, Tail};
use report::say;
use scan::{BarSurv, ClockCfg, Kind, Say, Surv};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use winmem::Proc;

const HELP: &str = "nrscan - memory scanner for ELDEN RING NIGHTREIGN (T-037/T-038)

USAGE
  nrscan list
  nrscan clock   --instance N [--min 0.2] [--max 20] [--interval-ms 1500] [--tolerance 0.25] [--now] [--u32]
                 [--near-mb N [--near-all]] [--poke-add S | --poke-set V] [--poke-at 4] [--poke-max-survivors 4] [--i-know]
  nrscan bar     --instance N [--interval-ms 300] [--near-mb 256 [--near-all]] [--everywhere] [--now]
  nrscan bar     --instance N --changed [--interval-ms 300] [--near-mb 256] [--hits-from-keyboard] [--hit-hp]
  nrscan explain --instance N [--route] [--route-depth 4] [--route-objects 200000] [--route-secs 20] <addr> [<addr>...]
  nrscan peek    --instance N <addr> [--f32 | --u32 | --hex N] [--interval-ms 500] [--count N] [--max-secs S]
  nrscan poke    --instance N <addr> (--f32 <v> | --u32 <v>) [--i-know]
Common options: --log <path to nrpvp.log>   --pid <pid> (instead of --instance)   --max-secs <s>   --help

  list     pid (dec/hex), exe path and instance name of every nightreign.exe.
  clock    wait for LOCAL DOWN in the instance's nrpvp.log, then find f32 (or --u32 counters) that move with elapsed time
           (x1, x30, x60[, x1000 for --u32]) in one direction; stops when LOCAL is UP / HP restored or after 25 s.
           --near-mb N: only regions within N MB of a PlayerIns address from the log (+ singletons with --near-all).
           WRITE TEST: --poke-add S (or --poke-set V): once the survivors are <= --poke-max-survivors (4) and >= --poke-at (4)
           seconds have passed since the DOWN, write the new value into every survivor once (read back, logged), keep sampling,
           and report the time DOWN -> end of the downed state and whether the values kept counting or snapped back.
  bar      wait for LOCAL DOWN, then find 4-byte values starting at 0 that rise in steps (revive bar); stops at UP or 40 s.
           Only regions within --near-mb (default 256) of a PlayerIns address (+ singletons with --near-all) are scanned,
           unless --everywhere.
  bar --changed
           snapshot the near regions at the DOWN and compare at every sample; report words that changed on >= 60% of hits and
           <= 10% of non-hit samples. Hits are detected from memory (PlayerIns+0x198 / +0x1A0 changing to an attacker handle,
           plus HP with --hit-hp) or, with --hits-from-keyboard, by pressing Enter at each hit. Snapshot limited to 1.5 GB.
  explain  region, class names (RTTI) of the enclosing objects, pointers to the address, and with --route a breadth-first route
           from PlayerIns / WorldChrMan (depth 4) like [[PlayerIns+0xA]+0xB]+0xC with the class of every hop.
  peek     read an address live (default: f32 and u32), repeated every --interval-ms until Ctrl+C / --count / --max-secs.
  poke     write one f32 or u32 (Nucleus instances only unless --i-know).
Logs default to <dist>\\nucleus-logs\\Instance<N>\\nrpvp.log; reports go to <dist>\\nrscan\\.
Exit codes: 0 ok, 1 error (process/log not found, scan failed, write refused), 2 usage, 3 scan finished with no survivors.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(&args);
    std::process::exit(code);
}

fn run(args: &[String]) -> i32 {
    let Some(cmd) = args.first() else {
        println!("{HELP}");
        return 2;
    };
    if args.iter().any(|a| a == "--help" || a == "-h") || cmd == "help" {
        println!("{HELP}");
        return 0;
    }
    let rest = &args[1..];
    match cmd.as_str() {
        "list" => cmd_list(),
        "clock" => cmd_clock(rest),
        "bar" => {
            if flag(rest, "--changed") {
                cmds::cmd_bar_changed(rest)
            } else {
                cmd_bar(rest)
            }
        }
        "explain" => cmd_explain(rest),
        "peek" => cmds::cmd_peek(rest),
        "poke" => cmds::cmd_poke(rest),
        _ => {
            eprintln!("unknown command '{cmd}'\n\n{HELP}");
            2
        }
    }
}

// ------------------------------------------------------------ arg helpers

pub(crate) fn flag(rest: &[String], name: &str) -> bool {
    rest.iter().any(|a| a == name)
}

pub(crate) fn opt<T: std::str::FromStr>(rest: &[String], name: &str) -> Result<Option<T>, String> {
    match rest.iter().position(|a| a == name) {
        None => Ok(None),
        Some(i) => rest
            .get(i + 1)
            .and_then(|v| v.parse::<T>().ok())
            .map(Some)
            .ok_or_else(|| format!("bad or missing value for {name}")),
    }
}

/// Tokens that are neither flags nor the values of the flags in `valued`.
pub(crate) fn positionals<'a>(rest: &'a [String], valued: &[&str]) -> Vec<&'a String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        if valued.contains(&a.as_str()) {
            i += 2;
            continue;
        }
        if a.starts_with("--") {
            i += 1;
            continue;
        }
        out.push(a);
        i += 1;
    }
    out
}

/// Every option of any command that takes a value (so `positionals` can skip them).
pub(crate) const VALUED: &[&str] = &[
    "--instance",
    "--pid",
    "--log",
    "--interval-ms",
    "--max-secs",
    "--min",
    "--max",
    "--tolerance",
    "--near-mb",
    "--poke-add",
    "--poke-set",
    "--poke-at",
    "--poke-max-survivors",
    "--route-depth",
    "--route-objects",
    "--route-secs",
    "--hex",
    "--count",
    "--hit-player",
];

pub(crate) fn parse_addr(s: &str) -> Option<u64> {
    let t = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if s.trim().starts_with("0x")
        || s.trim().starts_with("0X")
        || t.chars()
            .any(|c| c.is_ascii_hexdigit() && !c.is_ascii_digit())
    {
        u64::from_str_radix(t, 16).ok()
    } else {
        t.parse::<u64>().ok()
    }
}

fn dist_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir
                .file_name()
                .map(|n| n.eq_ignore_ascii_case("dist"))
                .unwrap_or(false)
            {
                return dir.to_path_buf();
            }
        }
    }
    std::env::current_dir().unwrap_or_default().join("dist")
}

pub(crate) struct Target {
    pub proc: Proc,
    pub log: PathBuf,
    pub name: String,
}

pub(crate) fn target(rest: &[String]) -> Result<Target, String> {
    target_with(rest, false)
}

/// `write`: open the process read+write. Refused unless the exe path contains `\Nucleus\content\` (or `--i-know`).
pub(crate) fn target_with(rest: &[String], write: bool) -> Result<Target, String> {
    let pid_opt: Option<u32> = opt(rest, "--pid")?;
    let inst: Option<u32> = opt(rest, "--instance")?;
    let games = winmem::list_games();
    let (pid, name) = match (pid_opt, inst) {
        (Some(p), _) => (p, format!("pid{p}")),
        (None, Some(n)) => {
            let want = format!("Instance{n}");
            let g = games
                .iter()
                .find(|g| {
                    g.instance()
                        .map(|i| i.eq_ignore_ascii_case(&want))
                        .unwrap_or(false)
                })
                .ok_or_else(|| {
                    format!("no nightreign.exe found under an '{want}' folder (try `nrscan list`)")
                })?;
            (g.pid, want)
        }
        (None, None) => return Err("need --instance N (or --pid)".into()),
    };
    let log: PathBuf = match opt::<String>(rest, "--log")? {
        Some(p) => PathBuf::from(p),
        None => dist_dir()
            .join("nucleus-logs")
            .join(&name)
            .join("nrpvp.log"),
    };
    let proc = if write {
        let exe = winmem::exe_path(pid).unwrap_or_default();
        if !poke::poke_allowed(&exe, flag(rest, "--i-know")) {
            return Err(format!(
                "refusing to write: pid {pid} runs '{exe}', which is not under \\Nucleus\\content\\ (pass --i-know to override)"
            ));
        }
        Proc::open_rw(pid)?
    } else {
        Proc::open(pid)?
    };
    Ok(Target { proc, log, name })
}

pub(crate) fn out_path(prefix: &str) -> PathBuf {
    dist_dir()
        .join("nrscan")
        .join(format!("{prefix}_{}.txt", report::utc_stamp()))
}

pub(crate) fn save(path: &Path) {
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    match std::fs::write(path, report::take()) {
        Ok(()) => println!("report written to {}", path.display()),
        Err(e) => eprintln!("could not write {}: {e}", path.display()),
    }
}

pub(crate) fn say_fn() -> impl Fn(&str) + Sync {
    |s: &str| say(s)
}

// ------------------------------------------------------------ list

fn cmd_list() -> i32 {
    let games = winmem::list_games();
    if games.is_empty() {
        println!("no nightreign.exe running");
        return 0;
    }
    for g in games {
        println!(
            "pid {} (0x{:X})  {}  {}",
            g.pid,
            g.pid,
            g.instance()
                .unwrap_or_else(|| "<no Instance folder>".into()),
            g.path
        );
    }
    0
}

// ------------------------------------------------------------ waiting on the log

/// Watches the log; `down` / `up` latch when a LOCAL event is seen.
pub(crate) struct Watch {
    tail: Tail,
    last_poll: Instant,
    pub down: bool,
    pub up: bool,
    /// when `up` first latched
    pub up_at: Option<Instant>,
}

impl Watch {
    pub(crate) fn new(log: &Path) -> Watch {
        Watch {
            tail: Tail::from_end(log),
            last_poll: Instant::now() - Duration::from_secs(1),
            down: false,
            up: false,
            up_at: None,
        }
    }

    pub(crate) fn poll(&mut self) {
        if self.last_poll.elapsed() < Duration::from_millis(25) {
            return;
        }
        self.last_poll = Instant::now();
        for l in self.tail.poll() {
            if let Some(e) = parse_state_line(&l) {
                if e.local {
                    match e.kind {
                        StateKind::Down => self.down = true,
                        StateKind::Up | StateKind::HpRestored => {
                            if !self.up {
                                self.up_at = Some(Instant::now());
                            }
                            self.up = true;
                        }
                        StateKind::Other => {}
                    }
                }
            }
        }
    }
}

pub(crate) fn wait_for_down(w: &mut Watch, log: &Path) {
    say(format!(
        "waiting for LOCAL DOWN in {} (Ctrl+C to abort) ...",
        log.display()
    ));
    let mut n = 0u64;
    while !w.down {
        w.poll();
        std::thread::sleep(Duration::from_millis(25));
        n += 1;
        if n.is_multiple_of(800) {
            println!("  still waiting ({} s)", n / 40);
        }
    }
    w.up = false;
    w.up_at = None;
}

pub(crate) fn region_summary(regions: &[winmem::Region]) {
    say(format!(
        "{} committed private RW region(s), {} MB total",
        regions.len(),
        regions.iter().map(|r| r.size).sum::<u64>() >> 20
    ));
}

/// Clip `regions` to windows of `mb` MB around the PlayerIns anchors of the log (all singletons too with `all`).
/// Returns the clipped regions and the anchors used; the regions are unchanged when the log has no anchors.
pub(crate) fn near_regions(
    regions: &[winmem::Region],
    info: &LogInfo,
    mb: u64,
    all: bool,
) -> (Vec<winmem::Region>, Vec<u64>) {
    let anchors = if all {
        info.anchors()
    } else {
        info.players.clone()
    };
    if anchors.is_empty() {
        say("--near-mb: no anchor addresses in the log, scanning everywhere");
        return (regions.to_vec(), anchors);
    }
    let out = filters::clip_regions(regions, &filters::merge_windows(&anchors, mb << 20));
    say(format!(
        "--near-mb {mb}: {} anchor(s), {} region piece(s) covering {} MB",
        anchors.len(),
        out.len(),
        out.iter().map(|r| r.size).sum::<u64>() >> 20
    ));
    (out, anchors)
}

// ------------------------------------------------------------ clock

fn fmt_hist(kind: Kind, hist: &[(f32, u32)]) -> String {
    hist.iter()
        .map(|&(t, b)| match kind {
            Kind::F32 => format!("{:.3}@{:.2}", f32::from_bits(b), t),
            Kind::U32 => format!("{b}@{t:.2}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn cmd_clock(rest: &[String]) -> i32 {
    match clock_inner(rest) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn poke_cfg(rest: &[String]) -> Result<Option<poke::PokeCfg>, String> {
    let add: Option<f32> = opt(rest, "--poke-add")?;
    let set: Option<f32> = opt(rest, "--poke-set")?;
    let mode = match (add, set) {
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => return Err("use either --poke-add or --poke-set, not both".into()),
        (Some(a), None) => poke::PokeMode::Add(a),
        (None, Some(v)) => poke::PokeMode::Set(v),
    };
    Ok(Some(poke::PokeCfg {
        mode,
        at_secs: opt(rest, "--poke-at")?.unwrap_or(4.0),
        max_survivors: opt(rest, "--poke-max-survivors")?.unwrap_or(4),
    }))
}

fn clock_inner(rest: &[String]) -> Result<i32, String> {
    let pk = poke_cfg(rest)?;
    let u32mode = flag(rest, "--u32");
    if pk.is_some() && u32mode {
        return Err("--poke-* only supports the f32 clock (not --u32)".into());
    }
    let tg = target_with(rest, pk.is_some())?;
    let (dmin, dmax) = if u32mode { (1.0, 60000.0) } else { (0.2, 20.0) };
    let cfg = ClockCfg {
        kind: if u32mode { Kind::U32 } else { Kind::F32 },
        min: opt(rest, "--min")?.unwrap_or(dmin),
        max: opt(rest, "--max")?.unwrap_or(dmax),
        tol: opt(rest, "--tolerance")?.unwrap_or(0.25),
        interval_ms: opt(rest, "--interval-ms")?.unwrap_or(1500),
    };
    let max_secs: f64 = opt(rest, "--max-secs")?.unwrap_or(25.0);
    say(format!(
        "nrscan clock: {} pid {} ({:?}, range [{}, {}], tolerance {}, interval {} ms){}",
        tg.name,
        tg.proc.pid,
        cfg.kind,
        cfg.min,
        cfg.max,
        cfg.tol,
        cfg.interval_ms,
        match &pk {
            Some(p) => format!(
                "; WRITE TEST {:?} at >= {} s when <= {} survivor(s)",
                p.mode, p.at_secs, p.max_survivors
            ),
            None => String::new(),
        }
    ));
    let mut w = Watch::new(&tg.log);
    if flag(rest, "--now") {
        say("--now: starting immediately");
    } else {
        wait_for_down(&mut w, &tg.log);
        say("LOCAL DOWN seen: scanning");
    }
    let started = Instant::now();
    let mut regions = tg.proc.regions();
    region_summary(&regions);
    // --near-mb N: only memory within N MB of a PlayerIns address from the log (a full pass over ~7 GB takes longer than
    // the 15 s bleed-out). --near-all also uses the singleton instances as anchors.
    if let Some(mb) = opt::<u64>(rest, "--near-mb")? {
        let info = LogInfo::load(&tg.log);
        regions = near_regions(&regions, &info, mb, flag(rest, "--near-all")).0;
    }
    let qs = say_fn();
    let sref: Say = &qs;
    let out = scan::clock_scan(
        &tg.proc,
        &cfg,
        &regions,
        started,
        &mut || {
            w.poll();
            w.up || started.elapsed().as_secs_f64() > max_secs
        },
        pk,
        sref,
    );
    say(format!(
        "scan ended {:.1}s after start{}",
        started.elapsed().as_secs_f64(),
        if w.up {
            " (LOCAL left the downed state)"
        } else {
            ""
        }
    ));
    say(format!(
        "pass 1: {:.2}s ({:.0} MB/s); candidate counts per pass: {:?}; final range [{}, {}]",
        out.pass1_secs,
        out.pass1_bytes as f64 / 1048576.0 / out.pass1_secs.max(1e-9),
        out.counts,
        out.final_range.0,
        out.final_range.1
    ));
    say(format!(
        "{} survivor(s){}",
        out.survivors.len(),
        if out.survivors.len() > 200 {
            " (listing 200)"
        } else {
            ""
        }
    ));
    let mut order: Vec<&Surv> = out.survivors.iter().collect();
    order.sort_by_key(|s| (s.tr.mask & 1 == 0, s.addr));
    for s in order.iter().take(200) {
        let scale = [1, 30, 60, 1000]
            .iter()
            .enumerate()
            .filter(|(i, _)| s.tr.mask >> i & 1 == 1)
            .map(|(_, v)| format!("x{v}"))
            .collect::<Vec<_>>()
            .join("/");
        say(format!(
            "  {:#X} dir {:+} scale {}: {}",
            s.addr,
            s.tr.dir,
            scale,
            fmt_hist(cfg.kind, &s.hist)
        ));
    }
    if let Some(p) = pk {
        poke_report(&out, &p, &w, started);
    }
    let top: Vec<u64> = order.iter().take(10).map(|s| s.addr).collect();
    if !top.is_empty() {
        let info = LogInfo::load(&tg.log);
        say(format!("explaining the top {} survivor(s) ({} PlayerIns, {} singleton address(es) from the log)", top.len(), info.players.len(), info.singletons.len()));
        let opts = explain::ExplainOpts {
            route: true,
            ..Default::default()
        };
        explain::explain(&tg.proc, &info, &top, &opts, sref);
    }
    save(&out_path("clock"));
    Ok(if out.survivors.is_empty() { 3 } else { 0 })
}

/// The write-test summary: time DOWN -> end of the downed state, each survivor's value just before the end, verdict.
fn poke_report(out: &scan::ClockOut, pk: &poke::PokeCfg, w: &Watch, started: Instant) {
    say("=== WRITE TEST RESULT ===");
    if out.pokes.is_empty() {
        say("no poke was performed (survivors never dropped to the required count, or the downed state ended first)");
        return;
    }
    let end = w.up_at.map(|u| u.duration_since(started).as_secs_f32());
    match end {
        Some(e) => say(format!(
            "time from DOWN to the end of the downed state: {e:.2} s (run 10 without a poke: about 17.4 s; a clock that was moved forward by S s ends about S s earlier)"
        )),
        None => say("the downed state did not end while the scan was running (max-secs reached first)"),
    }
    say(format!("poke mode {:?}", pk.mode));
    for ev in &out.pokes {
        let Some(s) = out.survivors.iter().find(|s| s.addr == ev.addr) else {
            continue;
        };
        let f = |b: u32| f32::from_bits(b);
        let pre: Vec<(f32, f32)> = s
            .hist
            .iter()
            .filter(|h| h.0 < ev.t)
            .map(|h| (h.0, f(h.1)))
            .collect();
        let after: Vec<(f32, f32)> = s
            .hist
            .iter()
            .filter(|h| h.0 > ev.t)
            .map(|h| (h.0, f(h.1)))
            .collect();
        let before_end: Vec<(f32, f32)> = after
            .iter()
            .copied()
            .filter(|h| end.map(|e| h.0 <= e).unwrap_or(true))
            .collect();
        let post_end: Vec<(f32, f32)> = after
            .iter()
            .copied()
            .filter(|h| end.map(|e| h.0 > e).unwrap_or(false))
            .collect();
        let slope = poke::estimate_slope(&pre);
        let verdict = poke::classify_poke(
            ev,
            slope,
            if before_end.is_empty() {
                &after
            } else {
                &before_end
            },
        );
        say(format!(
            "  {:#X}: poked at +{:.2}s {:.3} -> wanted {:.3} -> readback {:.3}; slope before the poke {:.3}/s",
            ev.addr, ev.t, ev.old, ev.wanted, ev.readback, slope
        ));
        match before_end.last() {
            Some((t, v)) => say(format!(
                "      value just before the end: {v:.3} at +{t:.2}s"
            )),
            None => say("      no sample between the poke and the end"),
        }
        if !post_end.is_empty() {
            say(format!(
                "      after the end: {}",
                post_end
                    .iter()
                    .map(|(t, v)| format!("{v:.3}@{t:.2}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        say(format!(
            "      verdict: {}",
            match verdict {
                poke::PokeVerdict::KeptCounting => "KEPT COUNTING from the new value (the write took effect)".to_string(),
                poke::PokeVerdict::SnappedBack => "SNAPPED BACK onto the old trajectory (the game keeps its own copy / rewrites it)".to_string(),
                poke::PokeVerdict::Other(s) => format!("other: {s}"),
                poke::PokeVerdict::NoSamples => "no samples after the poke".to_string(),
            }
        ));
    }
}

// ------------------------------------------------------------ bar

struct BarRow {
    addr: u64,
    kind: &'static str,
    changes: usize,
    fin: f64,
    reset: bool,
    near: Option<f64>,
    hist: String,
}

fn cmd_bar(rest: &[String]) -> i32 {
    match bar_inner(rest) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn bar_inner(rest: &[String]) -> Result<i32, String> {
    let tg = target(rest)?;
    let interval: u64 = opt(rest, "--interval-ms")?.unwrap_or(300);
    let max_secs: f64 = opt(rest, "--max-secs")?.unwrap_or(40.0);
    let everywhere = flag(rest, "--everywhere");
    let near_mb: u64 = opt(rest, "--near-mb")?.unwrap_or(256);
    say(format!(
        "nrscan bar: {} pid {} (interval {} ms)",
        tg.name, tg.proc.pid, interval
    ));
    let mut w = Watch::new(&tg.log);
    if flag(rest, "--now") {
        say("--now: starting immediately");
    } else {
        wait_for_down(&mut w, &tg.log);
        say("LOCAL DOWN seen: scanning");
    }
    let started = Instant::now();
    let info = LogInfo::load(&tg.log);
    say(format!(
        "log: {} PlayerIns address(es), {} singleton instance(s)",
        info.players.len(),
        info.singletons.len()
    ));
    let mut regions = tg.proc.regions();
    region_summary(&regions);
    let (anchors, radius) = if everywhere {
        (Vec::new(), None)
    } else {
        let (r, a) = near_regions(&regions, &info, near_mb, flag(rest, "--near-all"));
        regions = r;
        if a.is_empty() {
            (a, None)
        } else {
            (a, Some(near_mb << 20))
        }
    };
    say(format!(
        "pre-filter {}: {} region piece(s) covering {} MB ({} bytes)",
        match radius {
            Some(_) => format!("+-{near_mb} MB around anchors"),
            None => "off".to_string(),
        },
        regions.len(),
        regions.iter().map(|r| r.size).sum::<u64>() >> 20,
        regions.iter().map(|r| r.size).sum::<u64>()
    ));
    let qs = say_fn();
    let sref: Say = &qs;
    let out = scan::bar_scan(
        &tg.proc,
        &regions,
        anchors,
        radius,
        interval,
        started,
        &mut || {
            w.poll();
            w.up || started.elapsed().as_secs_f64() > max_secs
        },
        sref,
    );
    say(format!(
        "scan ended {:.1}s after start{}",
        started.elapsed().as_secs_f64(),
        if w.up {
            " (LOCAL left the downed state)"
        } else {
            ""
        }
    ));
    say(format!(
        "pass 1: {:.2}s ({:.0} MB/s); {} still-zero, {} changed-and-plausible candidate(s)",
        out.pass1_secs,
        out.pass1_bytes as f64 / 1048576.0 / out.pass1_secs.max(1e-9),
        out.unchanged,
        out.changed.len()
    ));
    let mut rows: Vec<BarRow> = Vec::new();
    let mut others = 0usize;
    for b in &out.changed {
        for (ok, kind, vals) in [(b.f_ok, "f32", b.fvals()), (b.u_ok, "u32", b.uvals())] {
            if !ok {
                continue;
            }
            let (changes, fin, reset, near) = filters::bar_summary(&vals);
            if changes >= 1 && (near.is_some() || reset) {
                rows.push(BarRow {
                    addr: b.addr,
                    kind,
                    changes,
                    fin,
                    reset,
                    near,
                    hist: fmt_bar_hist(b, kind),
                });
            } else {
                others += 1;
            }
        }
    }
    let score = |r: &BarRow| {
        (if r.near.is_some() { 4.0 } else { 0.0 })
            + (if r.reset { 2.0 } else { 0.0 })
            + (r.changes.min(5) as f64) * 0.1
    };
    rows.sort_by(|a, b| {
        score(b)
            .partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.addr.cmp(&b.addr))
    });
    say(format!("{} ranked survivor(s) (final value within 10% of 40/90/240, or reset to 0 at the UP); {} other plausible", rows.len(), others));
    for r in rows.iter().take(200) {
        say(format!(
            "  {:#X} {} changes={} final={} near={} reset={} : {}",
            r.addr,
            r.kind,
            r.changes,
            r.fin,
            r.near.map(|n| n.to_string()).unwrap_or_else(|| "-".into()),
            r.reset,
            r.hist
        ));
    }
    let mut top: Vec<u64> = Vec::new();
    for r in &rows {
        if !top.contains(&r.addr) {
            top.push(r.addr);
        }
        if top.len() == 10 {
            break;
        }
    }
    if !top.is_empty() {
        say(format!("explaining the top {} survivor(s)", top.len()));
        let opts = explain::ExplainOpts {
            route: true,
            ..Default::default()
        };
        explain::explain(&tg.proc, &info, &top, &opts, sref);
    }
    save(&out_path("bar"));
    Ok(if rows.is_empty() { 3 } else { 0 })
}

fn fmt_bar_hist(b: &BarSurv, kind: &str) -> String {
    // compress runs of equal values: value@first-time
    let mut parts: Vec<String> = Vec::new();
    let mut last: Option<u32> = None;
    for &(t, bits) in &b.hist {
        if last != Some(bits) {
            parts.push(if kind == "f32" {
                format!("{:.3}@{:.2}", f32::from_bits(bits), t)
            } else {
                format!("{bits}@{t:.2}")
            });
            last = Some(bits);
        }
    }
    parts.join(" ")
}

// ------------------------------------------------------------ explain

pub(crate) fn route_cfg(rest: &[String]) -> Result<route::RouteCfg, String> {
    let mut c = route::RouteCfg::default();
    if let Some(d) = opt::<usize>(rest, "--route-depth")? {
        c.depth = d;
    }
    if let Some(o) = opt::<usize>(rest, "--route-objects")? {
        c.max_objects = o;
    }
    if let Some(s) = opt::<f64>(rest, "--route-secs")? {
        c.time_cap = Duration::from_secs_f64(s);
    }
    Ok(c)
}

fn cmd_explain(rest: &[String]) -> i32 {
    let mut addrs = Vec::new();
    for a in positionals(rest, VALUED) {
        match parse_addr(a) {
            Some(v) => addrs.push(v),
            None => {
                eprintln!("bad address '{a}'");
                return 2;
            }
        }
    }
    if addrs.is_empty() {
        eprintln!("explain needs at least one address\n\n{HELP}");
        return 2;
    }
    let cfg = match route_cfg(rest) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    let tg = match target(rest) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let info = LogInfo::load(&tg.log);
    say(format!(
        "nrscan explain: {} pid {}; log has {} PlayerIns, {} singleton instance(s)",
        tg.name,
        tg.proc.pid,
        info.players.len(),
        info.singletons.len()
    ));
    let qs = say_fn();
    let opts = explain::ExplainOpts {
        route: flag(rest, "--route"),
        cfg,
    };
    explain::explain(&tg.proc, &info, &addrs, &opts, &qs);
    save(&out_path("explain"));
    0
}
