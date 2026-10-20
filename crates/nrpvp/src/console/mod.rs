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

use crate::mm::Mode;
use crate::overlay::shared::{Command, PvpveState, Relation, Snapshot};
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
    Status,
    Players,
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
    Spec { name: "status", args: "", help: "world, local player, team value, PvP mode, matchmaking line", run: |_| Ok(Act::Main(MainReq::Status)) },
    Spec { name: "players", args: "", help: "every player in this game's ChrSet: slot, session pN, id, team value, relation", run: |_| Ok(Act::Main(MainReq::Players)) },
    Spec { name: "team_set", args: "<value> [target]", help: "write team_type on this game's copy of the target's players (default me)", run: team_set },
    Spec { name: "team_restore", args: "[target]", help: "put back the team_type captured before the first write (default me)", run: team_restore },
    Spec { name: "pvp", args: "0|1", help: "pause (0) or resume (1) the automatic rival marking", run: pvp },
    Spec { name: "mm_queue_solo", args: "", help: "join the Solo matchmaking queue", run: |_| Ok(mm(Command::MmQueue(Mode::Solo), "queue solo")) },
    Spec { name: "mm_queue_duo", args: "", help: "join the Duo matchmaking queue", run: |_| Ok(mm(Command::MmQueue(Mode::Duo), "queue duo")) },
    Spec { name: "mm_queue_squad", args: "", help: "join the Squad matchmaking queue", run: |_| Ok(mm(Command::MmQueue(Mode::Squad), "queue squad")) },
    Spec { name: "mm_leave", args: "", help: "leave the queue or the ready match", run: |_| Ok(mm(Command::MmLeave, "leave")) },
    Spec { name: "mm_status", args: "", help: "the matchmaking status line", run: mm_status },
    Spec { name: "discover", args: "", help: "run the character discovery and dump (the old F9)", run: |_| Ok(Act::Main(MainReq::Run(vec![Command::RunDiscovery], "discovery requested (see the log)".into()))) },
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
    let what = if on {
        "PvP marking resumed"
    } else {
        "PvP marking paused"
    };
    Ok(Act::Main(MainReq::Run(
        vec![Command::SetPvpve(on)],
        what.into(),
    )))
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
            "on ({} rivals as team {}, {} writes)",
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
        "nrpvp {} (dev), pid {}\n{world}\n{me}\npvp: {pvp}, team_size {}\nmatchmaking: {mm}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        s.team_size
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
    fn status_and_players_read_the_snapshot() {
        let s = snap();
        let p = answer(MainReq::Players, &s, &mut |_| {});
        assert!(p.contains("p2") && p.contains("rival") && p.contains("me"));
        let st = answer(MainReq::Status, &s, &mut |_| {});
        assert!(st.contains("world loaded, 2 player(s)"));
        assert_eq!(
            answer(MainReq::Players, &Snapshot::default(), &mut |_| {}),
            "no players loaded"
        );
    }
}
