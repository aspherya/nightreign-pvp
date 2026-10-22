//! CON-1/CON-2: the developer console (CONSOLE.md). Developer builds only (`dev` feature) and only with `dev_mode = true`.
//!
//! - [`parse`]: the CS-style line syntax. [`target`]: who a player command acts on.
//! - This module: the command registry ([`COMMANDS`], also the source of `help`), and the bridge to the main loop.
//! - [`pipe`]: the named-pipe server `\\.\pipe\nrpvp-<pid>` that `nrcon.exe` talks to.
//!
//! Threads: the pipe thread parses a line and answers what needs no game state itself (`help`, `echo`, `mm_status`). Everything
//! that reads or writes game memory is queued as a [`MainReq`] and executed by the main loop on its next tick ([`take`] +
//! [`answer`]), through the same `run_command` path as the hotkeys and the old overlay buttons; the pipe thread waits for the
//! reply. The console never touches game memory itself, and never calls Steam (matchmaking goes through `mm::send`).

pub(crate) mod parse;
pub(crate) mod pipe;
pub(crate) mod target;

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use crate::features::bots::BotsCmd;
use crate::features::mode::GameMode;
use crate::mm::Mode;
use crate::overlay::shared::{Command, HawkCmd, PvpveState, Relation, Snapshot};
use target::{parse_target, pick, session_no, Target, TARGET_HELP};

/// How long the pipe thread waits for the main loop (it ticks every 50 ms in a world, every 500 ms outside one).
const MAIN_WAIT: Duration = Duration::from_secs(3);
/// Requests queued but not yet taken; more are refused (a stuck main loop must not grow memory).
const MAX_QUEUED: usize = 32;

/// What the main loop must do for one console command.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MainReq {
    /// Run these commands (in order), then reply `ack`.
    Run(Vec<Command>, String),
    /// Write `team_type` (`Some`) or restore the captured original (`None`) for the target's players.
    Team(Option<u8>, Target),
    /// `pvp 1|0`: resume, or pause and put back every player's captured original (the old F12).
    Pvp(bool),
    /// Persist on (`true`) or off for the target's players.
    Persist(bool, Target),
    Status,
    Players,
    /// T-040: `revive_fail <n>` sets the forced-failure count; `revive_fail` alone reports what is left.
    ReviveFail(Option<u32>),
    /// T-040 follow-up: `early_redown 0|1` switches the get-up re-down experiment; `early_redown` alone reports it.
    EarlyRedown(Option<bool>),
    /// T-041 step 1: `bar_trace 0|1` switches the revive-bar write trace; `bar_trace` alone reports it.
    BarTrace(Option<bool>),
    /// T-041 step 3 prototype: `revive_block 0|1`; no argument reports it.
    ReviveBlock(Option<bool>),
    /// HAWK-3: one `hawk_*` command; the reply is the hawk state line.
    Hawk(HawkCmd),
    /// OFF-1: `mode br|offline` switches the game mode (now, or once back in the Roundtable); the reply is the mode line.
    Mode(GameMode),
    /// OFF-4: `bots <table>` / `bots_clear` ask the bot feature; the reply is the bots line.
    Bots(BotsCmd),
}

/// What a command does.
enum Act {
    /// Answered on the pipe thread.
    Reply(String),
    /// Answered by the main loop.
    Main(MainReq),
}

struct Spec {
    name: &'static str,
    args: &'static str,
    help: &'static str,
    run: fn(&[String]) -> Result<Act, String>,
}

/// Every console command. `help` lists them in this order; CONSOLE.md §3 is generated from the same text.
const COMMANDS: &[Spec] = &[
    Spec { name: "help", args: "[name]", help: "list the commands, or one command's help", run: help },
    Spec { name: "echo", args: "text…", help: "write `console: echo: text` to the log (a note for whoever reads it)", run: echo },
    Spec { name: "mark", args: "[text…]", help: "write `MARK #n: text` to the log (dev mode also starts the MARK dump, like the old Mark key)", run: mark },
    Spec { name: "log", args: "[n]", help: "the last n log lines (default 20, at most 300) straight from the game", run: log_tail },
    Spec { name: "status", args: "", help: "world, local player, team value, PvP mode, game mode, matchmaking line", run: |_| Ok(Act::Main(MainReq::Status)) },
    Spec { name: "mode", args: "[br|offline]", help: "game mode: br = Commence starts the queue, offline = Commence Solo starts a solo expedition at once; during an expedition the change waits for the Roundtable; no argument shows it", run: mode },
    Spec { name: "bots", args: "[10|30|50|10000]", help: "OFF-4 (offline mode): start the game's own NPC bots from NPCBotTableParam table 10 / 30 / 50 (2 Nightfarers, level 1) or 10000 (2 of 8, level 6); no argument shows the last request, the bot manager's state and the OFF-5 match line", run: bots },
    Spec { name: "bots_clear", args: "", help: "OFF-4 spike (offline mode): remove every NPC bot the game spawned", run: |_| Ok(Act::Main(MainReq::Bots(BotsCmd::Clear))) },
    Spec { name: "players", args: "", help: "every player in this game's ChrSet: slot, session pN, id, team value, relation", run: |_| Ok(Act::Main(MainReq::Players)) },
    Spec { name: "team_set", args: "<value> [target]", help: "write team_type on this game's copy of the target's players (default me)", run: team_set },
    Spec { name: "team_restore", args: "[target]", help: "put back the team_type captured before the first write (default me)", run: team_restore },
    Spec { name: "pvp", args: "0|1", help: "pause (0) or resume (1) the automatic rival marking", run: pvp },
    Spec { name: "mm_queue_solo", args: "", help: "join the Solo matchmaking queue", run: |_| Ok(mm(Command::MmQueue(Mode::Solo), "queue solo")) },
    Spec { name: "mm_queue_duo", args: "", help: "join the Duo matchmaking queue", run: |_| Ok(mm(Command::MmQueue(Mode::Duo), "queue duo")) },
    Spec { name: "mm_queue_squad", args: "", help: "join the Squad matchmaking queue", run: |_| Ok(mm(Command::MmQueue(Mode::Squad), "queue squad")) },
    Spec { name: "mm_leave", args: "", help: "leave the queue or the ready match", run: |_| Ok(mm(Command::MmLeave, "leave")) },
    Spec { name: "mm_status", args: "", help: "the matchmaking status line", run: mm_status },
    Spec { name: "team_persist", args: "0|1 [target]", help: "keep re-writing the last written team_type on the target's players (1) or stop (0)", run: team_persist },
    Spec { name: "pos_next", args: "", help: "snap-back test (T-028): select the next position candidate", run: |_| Ok(run_ack(Command::PosCycle, "next position candidate selected (see the log)")) },
    Spec { name: "pos_save", args: "", help: "snap-back test: save your current values of every position candidate", run: |_| Ok(run_ack(Command::PosSave, "position values saved (see the log)")) },
    Spec { name: "pos_go", args: "", help: "snap-back test: write the saved values into the selected candidate (you only)", run: |_| Ok(run_ack(Command::PosWrite, "position written; follow-up values at +100 ms / +500 ms / +2 s in the log")) },
    Spec { name: "revive_fail", args: "[n]", help: "T-040 test: make the next n rival re-down tries fail (forced by console); no argument shows the count left", run: revive_fail },
    Spec { name: "early_redown", args: "[0|1]", help: "experiment: knock a rival-revived player down during the get-up (1) or only after it (0, default); no argument shows the state", run: early_redown },
    Spec { name: "bar_trace", args: "[0|1]", help: "T-041 trace: log which instruction writes the downed LOCAL player's revive bar (hardware write watch, slot 3); no argument shows ON/OFF and hits", run: bar_trace },
    Spec { name: "revive_block", args: "[0|1]", help: "T-041 prototype: stop a rival's revive of LOCAL before the game starts the get-up (hardware breakpoint while downed); no argument shows ON/OFF and blocks", run: revive_block },
    Spec { name: "hawk_env", args: "", help: "HAWK-3: where the game's env switch is and whose code it is (no change)", run: |_| Ok(Act::Main(MainReq::Hawk(HawkCmd::Env))) },
    Spec { name: "hawk_trace", args: "[0|1|all]", help: "HAWK-3: log the hawk answers (377/384/394/395) per character; all = also every id the rider asks; no argument shows the state", run: hawk_trace },
    Spec { name: "hawk_drop", args: "", help: "HAWK-3: answer \"route ended\" (377 = 2) to the rider for 1.5 s, so the start hawk drops you", run: |_| Ok(Act::Main(MainReq::Hawk(HawkCmd::Drop))) },
    Spec { name: "hawk_tree", args: "0|1", help: "HAWK-3: start-hawk rides answer 1 (tree hawk) instead of 4 (set before the ride starts)", run: hawk_tree },
    Spec { name: "hawk_cruise", args: "0|1", help: "HAWK-4: start rides fly the route as tree rides with the stick hidden; holding the stick starts a dive (set before the ride)", run: hawk_cruise },
    Spec { name: "hawk_dive", args: "[hold] [ms]", help: "HAWK-4: dive trigger (394 units, default 400) and dive length (ms, default 7000); no argument shows the state", run: hawk_dive },
    Spec { name: "hawk_start", args: "0|1|2", help: "HAWK-4: ride start: 0 show the tree start animation, 1 cut it (default), 2 vanilla start then tree ride 2 s later", run: hawk_start },
    Spec { name: "hawk_boost", args: "0|1", help: "HAWK-4: dives fly at the hawk's x1.3 speed (1, default) or normal speed (0)", run: hawk_boost },
    Spec { name: "hawk_off", args: "", help: "HAWK-3: everything off and the game's env switch put back", run: |_| Ok(Act::Main(MainReq::Hawk(HawkCmd::Off))) },
    Spec { name: "mm_join", args: "link|invite|direct|unlock_public|unlock_invisible|relock", help: "one join-method attempt for the ready match (dev, mm_join = true)", run: mm_join },
    Spec { name: "mm_sc_create", args: "", help: "create a lobby the game's way and see if Seamless Co-op adopts it (dev, mm_sc_create = true)", run: |_| Ok(mm(Command::MmScCreate, "SC lobby create test")) },
    Spec { name: "mm_quiet_test", args: "", help: "suppress the Steam invite window for 60 s (dev, mm_quiet_invite = true)", run: |_| Ok(mm(Command::MmQuietTest, "quiet invite test")) },
];

static ENABLED: AtomicBool = AtomicBool::new(false);
type Queued = (MainReq, Sender<String>);
static QUEUE: Mutex<VecDeque<Queued>> = Mutex::new(VecDeque::new());

/// Start the console: the pipe server thread. `label` (the DLL directory) tells instances apart in `nrcon list`.
pub(crate) fn spawn(label: String) {
    ENABLED.store(true, Ordering::SeqCst);
    pipe::spawn(label);
}

/// Main loop: the requests to execute this tick.
pub(crate) fn take() -> Vec<Queued> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Vec::new();
    }
    QUEUE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .drain(..)
        .collect()
}

/// Execute one input line (several commands with `;`) and return the reply text. Called on the pipe thread.
pub(crate) fn execute_line(line: &str) -> String {
    let cmds = parse::split_commands(line);
    if cmds.is_empty() {
        return String::new();
    }
    let mut out = Vec::new();
    for tokens in cmds {
        let reply = match run_one(&tokens) {
            Ok(r) => r,
            Err(e) => format!("error: {e}"),
        };
        tracing::info!("console: {} -> {}", tokens.join(" "), first_line(&reply));
        out.push(reply);
    }
    out.join("\n")
}

fn first_line(s: &str) -> String {
    let mut l = s.lines().next().unwrap_or("").to_string();
    if s.lines().nth(1).is_some() {
        l.push_str(" …");
    }
    l
}

fn run_one(tokens: &[String]) -> Result<String, String> {
    let name = tokens[0].as_str();
    let spec = COMMANDS
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| format!("unknown command '{name}' (try help)"))?;
    match (spec.run)(&tokens[1..])
        .map_err(|e| format!("{e}\nusage: {} {}", spec.name, spec.args))?
    {
        Act::Reply(r) => Ok(r),
        Act::Main(req) => ask_main(req),
    }
}

fn ask_main(req: MainReq) -> Result<String, String> {
    let (tx, rx) = mpsc::channel();
    {
        let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
        if q.len() >= MAX_QUEUED {
            return Err("main loop busy (too many queued commands)".into());
        }
        q.push_back((req, tx));
    }
    rx.recv_timeout(MAIN_WAIT).map_err(|_| {
        "no answer from the main loop (not running yet, or stalled; see the heartbeat line)"
            .to_string()
    })
}

// ---- command bodies (pipe thread) ----

fn help(args: &[String]) -> Result<Act, String> {
    if let Some(n) = args.first() {
        let c = COMMANDS
            .iter()
            .find(|c| c.name == n.as_str())
            .ok_or_else(|| format!("unknown command '{n}'"))?;
        return Ok(Act::Reply(format!("{} {}\n  {}", c.name, c.args, c.help)));
    }
    let mut s = String::from("commands (targets: ");
    s.push_str(TARGET_HELP);
    s.push_str("; `;` chains commands):");
    for c in COMMANDS {
        let _ = write!(s, "\n  {:<15} {:<18} {}", c.name, c.args, c.help);
    }
    Ok(Act::Reply(s))
}

fn echo(args: &[String]) -> Result<Act, String> {
    let text = args.join(" ");
    tracing::info!("console: echo: {text}");
    Ok(Act::Reply(text))
}

fn log_tail(args: &[String]) -> Result<Act, String> {
    let n = match args.first() {
        None => 20,
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| "n must be a number")?
            .clamp(1, 300),
    };
    let (_, lines) = crate::logring::snapshot();
    let skip = lines.len().saturating_sub(n);
    Ok(Act::Reply(lines[skip..].join(
        "
",
    )))
}

fn mark(args: &[String]) -> Result<Act, String> {
    let text = (!args.is_empty()).then(|| args.join(" "));
    Ok(Act::Main(MainReq::Run(
        vec![Command::Mark(text)],
        "MARK written (see the log)".into(),
    )))
}

fn team_set(args: &[String]) -> Result<Act, String> {
    let v = args
        .first()
        .ok_or("missing value")?
        .parse::<u8>()
        .map_err(|_| "value must be 0..255")?;
    let t = parse_target(args.get(1).map(String::as_str))?;
    Ok(Act::Main(MainReq::Team(Some(v), t)))
}

fn team_restore(args: &[String]) -> Result<Act, String> {
    let t = parse_target(args.first().map(String::as_str))?;
    Ok(Act::Main(MainReq::Team(None, t)))
}

fn pvp(args: &[String]) -> Result<Act, String> {
    let on = match args.first().map(String::as_str) {
        Some("1") => true,
        Some("0") => false,
        _ => return Err("expected 0 or 1".into()),
    };
    Ok(Act::Main(MainReq::Pvp(on)))
}

fn run_ack(c: Command, ack: &str) -> Act {
    Act::Main(MainReq::Run(vec![c], ack.into()))
}

fn team_persist(args: &[String]) -> Result<Act, String> {
    let on = match args.first().map(String::as_str) {
        Some("1") => true,
        Some("0") => false,
        _ => return Err("expected 0 or 1".into()),
    };
    let t = parse_target(args.get(1).map(String::as_str))?;
    Ok(Act::Main(MainReq::Persist(on, t)))
}

fn revive_fail(args: &[String]) -> Result<Act, String> {
    let n = match args.first() {
        None => None,
        Some(v) => Some(v.parse::<u32>().map_err(|_| "n must be a number")?),
    };
    Ok(Act::Main(MainReq::ReviveFail(n)))
}

fn early_redown(args: &[String]) -> Result<Act, String> {
    let on = match args.first().map(String::as_str) {
        None => None,
        Some("0") => Some(false),
        Some("1") => Some(true),
        Some(_) => return Err("expected 0 or 1".into()),
    };
    Ok(Act::Main(MainReq::EarlyRedown(on)))
}

fn revive_block(args: &[String]) -> Result<Act, String> {
    let on = match args.first().map(String::as_str) {
        None => None,
        Some("0") => Some(false),
        Some("1") => Some(true),
        Some(_) => return Err("expected 0 or 1".into()),
    };
    Ok(Act::Main(MainReq::ReviveBlock(on)))
}

fn bar_trace(args: &[String]) -> Result<Act, String> {
    let on = match args.first().map(String::as_str) {
        None => None,
        Some("0") => Some(false),
        Some("1") => Some(true),
        Some(_) => return Err("expected 0 or 1".into()),
    };
    Ok(Act::Main(MainReq::BarTrace(on)))
}

fn hawk_trace(args: &[String]) -> Result<Act, String> {
    let c = match args.first().map(String::as_str) {
        None => HawkCmd::Status,
        Some("0") => HawkCmd::Trace(false, false),
        Some("1") => HawkCmd::Trace(true, false),
        Some("all") => HawkCmd::Trace(true, true),
        Some(_) => return Err("expected 0, 1 or all".into()),
    };
    Ok(Act::Main(MainReq::Hawk(c)))
}

fn hawk_tree(args: &[String]) -> Result<Act, String> {
    let on = match args.first().map(String::as_str) {
        Some("0") => false,
        Some("1") => true,
        _ => return Err("expected 0 or 1".into()),
    };
    Ok(Act::Main(MainReq::Hawk(HawkCmd::Tree(on))))
}

fn hawk_cruise(args: &[String]) -> Result<Act, String> {
    let on = match args.first().map(String::as_str) {
        Some("0") => false,
        Some("1") => true,
        _ => return Err("expected 0 or 1".into()),
    };
    Ok(Act::Main(MainReq::Hawk(HawkCmd::Cruise(on))))
}

fn hawk_dive(args: &[String]) -> Result<Act, String> {
    if args.is_empty() {
        return Ok(Act::Main(MainReq::Hawk(HawkCmd::Status)));
    }
    let num = |i: usize, max: u32| -> Result<u32, String> {
        args[i]
            .parse::<u32>()
            .ok()
            .filter(|v| (1..=max).contains(v))
            .ok_or_else(|| format!("'{}' must be a number 1..{max}", args[i]))
    };
    let hold = num(0, 100_000)?;
    let ms = if args.len() > 1 {
        num(1, 60_000)?
    } else {
        7000
    };
    Ok(Act::Main(MainReq::Hawk(HawkCmd::Dive(hold, ms))))
}

fn hawk_start(args: &[String]) -> Result<Act, String> {
    let m = match args.first().map(String::as_str) {
        Some("0") => 0,
        Some("1") => 1,
        Some("2") => 2,
        _ => return Err("expected 0, 1 or 2".into()),
    };
    Ok(Act::Main(MainReq::Hawk(HawkCmd::Start(m))))
}

fn hawk_boost(args: &[String]) -> Result<Act, String> {
    let on = match args.first().map(String::as_str) {
        Some("0") => false,
        Some("1") => true,
        _ => return Err("expected 0 or 1".into()),
    };
    Ok(Act::Main(MainReq::Hawk(HawkCmd::Boost(on))))
}

fn mode(args: &[String]) -> Result<Act, String> {
    match args.first() {
        None => Ok(Act::Reply(crate::features::mode::status_line())),
        Some(a) => GameMode::parse(a)
            .map(|m| Act::Main(MainReq::Mode(m)))
            .ok_or_else(|| "expected br or offline".into()),
    }
}

fn bots(args: &[String]) -> Result<Act, String> {
    match args.first() {
        None => Ok(Act::Reply(crate::features::bots::status_line())),
        Some(a) => a
            .parse::<i32>()
            .map(|t| Act::Main(MainReq::Bots(BotsCmd::Spawn(t))))
            .map_err(|_| "table must be a number (10, 30, 50 or 10000)".into()),
    }
}

fn mm_join(args: &[String]) -> Result<Act, String> {
    use crate::mm::join::JoinCmd;
    let j = match args.first().map(String::as_str) {
        Some("link") => JoinCmd::Link,
        Some("invite") => JoinCmd::Invite,
        Some("direct") => JoinCmd::Direct,
        Some("unlock_public") => JoinCmd::UnlockPublic,
        Some("unlock_invisible") => JoinCmd::UnlockInvisible,
        Some("relock") => JoinCmd::RelockPrivate,
        _ => return Err("unknown join method".into()),
    };
    Ok(mm(Command::MmJoin(j), "join attempt"))
}

fn mm(c: Command, what: &str) -> Act {
    let ack = format!("{what}: sent to the matchmaking worker (mm_status to follow it)");
    Act::Main(MainReq::Run(vec![c], ack))
}

fn mm_status(_: &[String]) -> Result<Act, String> {
    if !crate::mm::enabled() {
        return Ok(Act::Reply("matchmaking is off (mm_queue = false)".into()));
    }
    let s = crate::mm::status();
    Ok(Act::Reply(if s.line.is_empty() {
        "idle".into()
    } else {
        s.line
    }))
}

// ---- main loop side ----

/// Execute `req` against this tick's snapshot; `run` executes a command through the main loop's `run_command`. Returns the reply.
pub(crate) fn answer(req: MainReq, snap: &Snapshot, run: &mut dyn FnMut(Command)) -> String {
    match req {
        MainReq::Run(cmds, ack) => {
            for c in cmds {
                run(c);
            }
            ack
        }
        MainReq::Persist(on, t) => {
            if !snap.vtables_known {
                return "error: player classes not known yet (RTTI); try again in a moment".into();
            }
            let rows = match pick(t, &snap.players) {
                Ok(r) => r,
                Err(e) => return format!("error: {e}"),
            };
            let n = rows.len();
            for r in rows {
                run(Command::PersistFor { addr: r.addr, on });
            }
            format!(
                "persist {} for {n} player(s) on this game",
                if on { "on" } else { "off" }
            )
        }
        MainReq::Pvp(on) => {
            run(Command::SetPvpve(on));
            if on {
                return "PvP marking resumed".into();
            }
            let mut n = 0;
            for r in snap.players.iter().filter(|r| r.original.is_some()) {
                run(Command::RestoreFor { addr: r.addr });
                n += 1;
            }
            format!("PvP marking paused; {n} player(s) restored on this game")
        }
        MainReq::ReviveFail(Some(n)) => {
            run(Command::ReviveFail(n));
            format!("the next {n} rival re-down try(s) will fail with \"forced by console\"")
        }
        MainReq::ReviveFail(None) => {
            format!("{} forced re-down failure(s) left", snap.revive_fail_left)
        }
        MainReq::EarlyRedown(Some(on)) => {
            run(Command::EarlyRedown(on));
            format!(
                "early re-down (during the get-up) {}",
                if on { "ON" } else { "OFF" }
            )
        }
        MainReq::EarlyRedown(None) => format!(
            "early re-down (during the get-up) {}",
            if snap.early_redown { "ON" } else { "OFF" }
        ),
        MainReq::BarTrace(Some(on)) => {
            run(Command::BarTrace(on));
            format!(
                "bar_trace {} (arms while LOCAL is downed; results in the log, lines start with T-041)",
                if on { "ON" } else { "OFF" }
            )
        }
        MainReq::BarTrace(None) => format!(
            "bar_trace {}, {} write(s) captured so far",
            if snap.bar_trace { "ON" } else { "OFF" },
            snap.bar_trace_hits
        ),
        MainReq::ReviveBlock(Some(on)) => {
            run(Command::ReviveBlock(on));
            format!(
                "revive_block {} (stops a rival's revive of LOCAL before the get-up; lines start with T-041)",
                if on { "ON" } else { "OFF" }
            )
        }
        MainReq::ReviveBlock(None) => format!(
            "revive_block {}, {} block(s) so far",
            if snap.revive_block { "ON" } else { "OFF" },
            snap.revive_blocks
        ),
        MainReq::Hawk(c) => {
            run(Command::Hawk(c));
            crate::game::hawk::status_line()
        }
        MainReq::Mode(m) => {
            run(Command::Mode(m));
            crate::features::mode::status_line()
        }
        MainReq::Bots(b) => {
            run(Command::Bots(b));
            crate::features::bots::status_line()
        }
        MainReq::Status => status_text(snap),
        MainReq::Players => players_text(snap),
        MainReq::Team(value, t) => {
            if !snap.vtables_known {
                return "error: player classes not known yet (RTTI); try again in a moment".into();
            }
            let rows = match pick(t, &snap.players) {
                Ok(r) => r,
                Err(e) => return format!("error: {e}"),
            };
            let n = rows.len();
            for r in rows {
                run(match (value, t) {
                    (Some(v), Target::Me) => Command::Set(v),
                    (Some(v), _) => Command::SetFor {
                        addr: r.addr,
                        value: v,
                    },
                    (None, Target::Me) => Command::Restore,
                    (None, _) => Command::RestoreFor { addr: r.addr },
                });
            }
            match value {
                Some(v) => {
                    format!("team_type {v} written for {n} player(s) on this game (see `players`)")
                }
                None => format!("restored {n} player(s) on this game (see `players`)"),
            }
        }
    }
}

fn status_text(s: &Snapshot) -> String {
    let world = if s.wcm_instance == 0 {
        "no world (title, menu or loading)".to_string()
    } else {
        format!("world loaded, {} player(s)", s.players.len())
    };
    let me = match (s.local_player, s.team_type) {
        (Some(p), Some(t)) => format!("me = 0x{p:X}, team_type {t}"),
        (Some(p), None) => format!("me = 0x{p:X}"),
        _ => format!(
            "me = none{}",
            if s.note.is_empty() {
                String::new()
            } else {
                format!(" ({})", s.note)
            }
        ),
    };
    let pvp = match s.pvpve {
        PvpveState::Off => "off".to_string(),
        PvpveState::On => format!(
            "on (scheme {}, rivals marked {}, {} writes)",
            s.pvpve_scheme, s.pvpve_team, s.pvpve_writes
        ),
        PvpveState::Paused => "paused".to_string(),
    };
    let mm = if crate::mm::enabled() {
        let l = crate::mm::status().line;
        if l.is_empty() {
            "idle".to_string()
        } else {
            l
        }
    } else {
        "off".to_string()
    };
    format!(
        "nrpvp {} (dev), pid {}\n{world}\n{me}\npvp: {pvp}, team_size {}\n{}\nmatchmaking: {mm}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        s.team_size,
        crate::features::mode::status_line()
    )
}

fn players_text(s: &Snapshot) -> String {
    if s.players.is_empty() {
        return "no players loaded".into();
    }
    let mut out = String::from("slot  who  id              team  relation");
    for r in &s.players {
        let who = session_no(r.id).map_or("?".to_string(), |n| format!("p{n}"));
        let rel = match r.relation {
            Some(Relation::Local) => "me",
            Some(Relation::Ally) => "ally",
            Some(Relation::Rival) => "rival",
            None => "-",
        };
        let id = r.id.map_or("?".to_string(), |v| format!("0x{v:X}"));
        let team = r.team.map_or("?".to_string(), |v| v.to_string());
        let _ = write!(
            out,
            "\ns{:<4} {:<4} {:<15} {:<5} {}",
            r.index, who, id, team, rel
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::shared::{PlayerRow, Role};

    fn row(index: usize, local: bool, id: u64, rel: Relation) -> PlayerRow {
        PlayerRow {
            index,
            addr: 0x100 + index,
            role: if local { Role::Local } else { Role::Remote },
            basis: String::new(),
            team: Some(2),
            state8: 0,
            state9: 0,
            original: None,
            persist: None,
            reverts_last_sec: 0,
            id: Some(id),
            team_no: None,
            relation: Some(rel),
            reverts_total: 0,
        }
    }

    fn snap() -> Snapshot {
        Snapshot {
            wcm_instance: 0x5000,
            vtables_known: true,
            players: vec![
                row(0, true, 0x6001_FFFF_FFFF, Relation::Local),
                row(1, false, 0x6002_FFFF_FFFF, Relation::Rival),
            ],
            ..Snapshot::default()
        }
    }

    #[test]
    fn every_command_has_a_unique_underscore_name_and_help() {
        for (i, c) in COMMANDS.iter().enumerate() {
            assert!(!c.name.contains(' ') && !c.help.is_empty(), "{}", c.name);
            assert!(
                COMMANDS[i + 1..].iter().all(|o| o.name != c.name),
                "{}",
                c.name
            );
        }
    }

    #[test]
    fn pipe_thread_commands_answer_without_the_main_loop() {
        assert!(execute_line("help").contains("team_set"));
        assert!(execute_line("help team_set").starts_with("team_set <value> [target]"));
        assert_eq!(execute_line(r#"echo "hi there""#), "hi there");
        assert!(execute_line("nope").starts_with("error: unknown command"));
        assert!(execute_line("team_set").contains("usage: team_set"));
        assert!(execute_line("team_set 29 bob").contains("unknown target"));
        assert!(execute_line("pvp 2").contains("expected 0 or 1"));
    }

    #[test]
    fn hawk_commands_parse_and_map_to_the_hawk_command() {
        assert!(execute_line("hawk_trace 2").contains("expected 0, 1 or all"));
        assert!(execute_line("hawk_tree").contains("usage: hawk_tree 0|1"));
        let cases = [
            (&["all"][..], HawkCmd::Trace(true, true)),
            (&["1"][..], HawkCmd::Trace(true, false)),
            (&["0"][..], HawkCmd::Trace(false, false)),
            (&[][..], HawkCmd::Status),
        ];
        for (args, want) in cases {
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            assert!(matches!(hawk_trace(&args), Ok(Act::Main(MainReq::Hawk(c))) if c == want));
        }
        assert!(execute_line("hawk_dive 0").contains("must be a number"));
        assert!(matches!(
            hawk_dive(&["600".into(), "5000".into()]),
            Ok(Act::Main(MainReq::Hawk(HawkCmd::Dive(600, 5000))))
        ));
        assert!(matches!(
            hawk_cruise(&["1".into()]),
            Ok(Act::Main(MainReq::Hawk(HawkCmd::Cruise(true))))
        ));
        assert!(execute_line("hawk_start 3").contains("expected 0, 1 or 2"));
        assert!(matches!(
            hawk_start(&["2".into()]),
            Ok(Act::Main(MainReq::Hawk(HawkCmd::Start(2))))
        ));
        assert!(matches!(
            hawk_boost(&["0".into()]),
            Ok(Act::Main(MainReq::Hawk(HawkCmd::Boost(false))))
        ));
        let mut ran = Vec::new();
        answer(MainReq::Hawk(HawkCmd::Drop), &snap(), &mut |c| ran.push(c));
        assert_eq!(ran, vec![Command::Hawk(HawkCmd::Drop)]);
    }

    #[test]
    fn mode_parses_and_maps_to_the_mode_command() {
        assert!(execute_line("mode solo").contains("expected br or offline"));
        assert!(execute_line("mode solo").contains("usage: mode [br|offline]"));
        assert!(matches!(
            mode(&["offline".into()]),
            Ok(Act::Main(MainReq::Mode(GameMode::Offline)))
        ));
        assert!(matches!(
            mode(&["BR".into()]),
            Ok(Act::Main(MainReq::Mode(GameMode::BattleRoyale)))
        ));
        // No argument: answered on the pipe thread with the mode line.
        assert!(execute_line("mode").starts_with("mode: "));
        let mut ran = Vec::new();
        let r = answer(MainReq::Mode(GameMode::Offline), &snap(), &mut |c| {
            ran.push(c)
        });
        assert_eq!(ran, vec![Command::Mode(GameMode::Offline)]);
        assert!(r.starts_with("mode: "), "{r}");
    }

    #[test]
    fn bots_parse_and_map_to_the_bots_command() {
        assert!(execute_line("bots ten").contains("table must be a number"));
        assert!(matches!(
            bots(&["10000".into()]),
            Ok(Act::Main(MainReq::Bots(BotsCmd::Spawn(10_000))))
        ));
        // No argument: answered on the pipe thread.
        assert!(execute_line("bots").starts_with("bots: "));
        let mut ran = Vec::new();
        let r = answer(MainReq::Bots(BotsCmd::Clear), &snap(), &mut |c| ran.push(c));
        assert_eq!(ran, vec![Command::Bots(BotsCmd::Clear)]);
        assert!(r.starts_with("bots: "), "{r}");
    }

    #[test]
    fn team_commands_map_to_the_existing_main_loop_commands() {
        let s = snap();
        let mut ran = Vec::new();
        let r = answer(MainReq::Team(Some(29), Target::Me), &s, &mut |c| {
            ran.push(c)
        });
        assert!(r.contains("1 player"));
        assert_eq!(ran, vec![Command::Set(29)]);
        ran.clear();
        answer(MainReq::Team(Some(29), Target::Session(2)), &s, &mut |c| {
            ran.push(c)
        });
        assert_eq!(
            ran,
            vec![Command::SetFor {
                addr: 0x101,
                value: 29
            }]
        );
        ran.clear();
        answer(MainReq::Team(None, Target::All), &s, &mut |c| ran.push(c));
        assert_eq!(
            ran,
            vec![
                Command::RestoreFor { addr: 0x100 },
                Command::RestoreFor { addr: 0x101 }
            ]
        );
        ran.clear();
        let r = answer(MainReq::Team(Some(29), Target::Session(5)), &s, &mut |c| {
            ran.push(c)
        });
        assert!(r.starts_with("error:") && ran.is_empty());
    }

    #[test]
    fn revive_fail_sets_or_reports_the_forced_failure_count() {
        assert_eq!(
            run_one(&["revive_fail".into(), "x".into()]).unwrap_or_else(|e| e),
            "n must be a number
usage: revive_fail [n]"
        );
        let mut s = snap();
        s.revive_fail_left = 2;
        let mut ran = Vec::new();
        let r = answer(MainReq::ReviveFail(Some(3)), &s, &mut |c| ran.push(c));
        assert_eq!(ran, vec![Command::ReviveFail(3)]);
        assert!(r.contains("next 3"), "{r}");
        ran.clear();
        let r = answer(MainReq::ReviveFail(None), &s, &mut |c| ran.push(c));
        assert!(ran.is_empty() && r.starts_with("2 forced"), "{r}");
    }

    #[test]
    fn early_redown_switches_or_reports_the_experiment() {
        assert!(run_one(&["early_redown".into(), "2".into()]).is_err());
        let mut s = snap();
        let mut ran = Vec::new();
        let r = answer(MainReq::EarlyRedown(Some(true)), &s, &mut |c| ran.push(c));
        assert_eq!(ran, vec![Command::EarlyRedown(true)]);
        assert!(r.ends_with("ON"), "{r}");
        ran.clear();
        s.early_redown = false;
        let r = answer(MainReq::EarlyRedown(None), &s, &mut |c| ran.push(c));
        assert!(ran.is_empty() && r.ends_with("OFF"), "{r}");
    }

    #[test]
    fn bar_trace_switches_or_reports() {
        assert!(run_one(&["bar_trace".into(), "x".into()]).is_err());
        let mut s = snap();
        let mut ran = Vec::new();
        let r = answer(MainReq::BarTrace(Some(true)), &s, &mut |c| ran.push(c));
        assert_eq!(ran, vec![Command::BarTrace(true)]);
        assert!(r.contains("ON"), "{r}");
        ran.clear();
        s.bar_trace_hits = 7;
        let r = answer(MainReq::BarTrace(None), &s, &mut |c| ran.push(c));
        assert!(
            ran.is_empty() && r.contains("OFF") && r.contains("7 write"),
            "{r}"
        );
    }

    #[test]
    fn pvp_off_pauses_and_restores_every_captured_original() {
        let mut s = snap();
        s.players[1].original = Some(2);
        let mut ran = Vec::new();
        let r = answer(MainReq::Pvp(false), &s, &mut |c| ran.push(c));
        assert_eq!(
            ran,
            vec![
                Command::SetPvpve(false),
                Command::RestoreFor { addr: 0x101 }
            ]
        );
        assert!(r.contains("1 player(s) restored"));
        ran.clear();
        answer(MainReq::Pvp(true), &s, &mut |c| ran.push(c));
        assert_eq!(ran, vec![Command::SetPvpve(true)]);
    }

    #[test]
    fn status_and_players_read_the_snapshot() {
        let s = snap();
        let p = answer(MainReq::Players, &s, &mut |_| {});
        assert!(p.contains("p2") && p.contains("rival") && p.contains("me"));
        let st = answer(MainReq::Status, &s, &mut |_| {});
        assert!(st.contains("world loaded, 2 player(s)"));
        assert!(st.contains("\nmode: "), "{st}");
        assert_eq!(
            answer(MainReq::Players, &Snapshot::default(), &mut |_| {}),
            "no players loaded"
        );
    }
}
