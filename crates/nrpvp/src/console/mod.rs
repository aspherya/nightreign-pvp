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
use crate::features::clock::ClockCmd;
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
    /// M5: `clock [time]` reads or sets the play-area clock; the reply is the clock line.
    Clock(ClockCmd),
    /// M5-3: `night_end` ends the current night now; the reply is the nights line.
    NightEnd,
    /// M5-4: `flag <id> [0|1]` reads or writes an event flag; the reply is the flag line.
    Flag(u32, Option<bool>),
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
    Spec { name: "clock", args: "[night|circle|end|hh:mm[:ss]]", help: "M5: the expedition's play-area clock (07:00 morning, 23:59:59 night); with a time, the game jumps its clock there on its next frame: circle = 20:20 (final circle starts 20:24), night = 22:55 (night from 23:00), end = 23:58:30", run: clock },
    Spec { name: "night_end", args: "", help: "M5-3: end the current night now (otherwise it ends when one team is left standing, or after 10 min); the game's morning follows. Host only", run: |_| Ok(Act::Main(MainReq::NightEnd)) },
    Spec { name: "night_status", args: "", help: "the match rules: free day 1 / day 2 / night, players, teams seen, how many are out, whether LOCAL is defeated, the winner, mob deaths, places recorded, death held", run: |_| Ok(Act::Reply(crate::features::night_match::status_line())) },
    Spec { name: "hp0", args: "", help: "test aid: put this game's player at 0 HP on the next tick (turns god off). In a session it downs the player; with `flag 10007420 1` (die at once) it is a real death even offline", run: hp0 },
    Spec { name: "spectate", args: "[0|1]", help: "RW-1: 1 = a defeated player's held body follows a living player (the mod's spectator), 0 = it stays where it fell (shows what the game itself does for a dead player); no argument shows it", run: spectate },
    Spec { name: "trace_write", args: "<addr>|0", help: "research: log which instruction writes the 4-byte aligned address (hardware write watch, slot 3) with registers and a stack excerpt as `T-041 bar_trace` lines; 0 stops. Heap addresses change every launch", run: trace_write },
    Spec { name: "rune_watch", args: "[0|1]", help: "RW-8 research: 1 = log which instruction writes LOCAL's rune count (PlayerGameData+0x6C; hardware write watch, slot 3) with registers and a stack excerpt as `T-041 bar_trace`; 0 = off", run: rune_watch },
    Spec { name: "padpoll", args: "", help: "RW-1 research: every virtual input bit of the game's own pad that is set now (hold a button, call it)", run: |_| Ok(Act::Reply(crate::game::menugate::describe())) },
    Spec { name: "menugate", args: "[0|1]", help: "RW-1 research: 1 = the Esc key asks the game to open the pause menu even while LOCAL is dead (sets CSMenuMan ctrl+0x179); 0 = left to the game (the night match does this by itself for a defeated player)", run: |a| { let on = match a.first().map(String::as_str) { Some("0") => false, Some("1") | None => true, Some(_) => return Err("expected 0 or 1".into()) }; crate::game::menugate::set_active(on); Ok(Act::Reply(format!("menugate {}", u8::from(on)))) } },
    Spec { name: "poke", args: "<addr|me+0x..|wcm+0x..|exe+0x..> <hex bytes>", help: "research (ESC while dead, RW-1): write up to 16 bytes of game memory (guarded writable check); `me` = LOCAL's PlayerIns. Logged as `research poke`", run: |a| crate::game::puppet::poke_cmd(a).map(Act::Reply) },
    Spec { name: "memscan", args: "[utf16|ascii|hex] <text|hex bytes>", help: "research (RW-6): scan the whole process for a string (UTF-16 by default) or bytes on a background thread (about a minute) and log every hit with 64 bytes around it as `memscan:`; no argument shows the last result. Used to find where the game keeps a text the menu shows", run: memscan },
    Spec { name: "rival_hit", args: "", help: "RW-1 test aid: record a rival's hit on this game's player now (the player build's hit log does it for real hits); a death within 30 s after it is a defeat for good, a death without it is a normal one", run: rival_hit },
    Spec { name: "god", args: "[0|1]", help: "test aid: 1 = this game's player is put back to full HP every tick while up (a single hit bigger than full HP still downs); 0 off (default); no argument shows it", run: god },
    Spec { name: "night_count", args: "[0|1]", help: "test switch: 1 (default) the match ends when one team is left standing; 0 it never ends by count; no argument shows it", run: night_count },
    Spec { name: "death_trace", args: "[0|1]", help: "M5-8 research: while this game's player is dead, log the comeback steps (RejoinMapEnterStep breakpoints) and the player's state every second; no argument shows it", run: death_trace },
    Spec { name: "death_hold", args: "[0|1]", help: "M5-9: 1 = a dead player stays dead (the comeback's start step is held, same module as the Solo night hold); 0 = a held step is called through the gate (exe+0x5BCE10, else exe+0x675480 after 2 s) and the game's comeback runs; no argument shows the state", run: death_hold },
    Spec { name: "pad", args: "<item>[; <item>...]", help: "ORCH-1: virtual gamepad (pad 0, through XInputGetState): items A, A+DOWN 300 (ms), wait 500, LT/RT [ms], LS x y [ms] / RS x y [ms] (x, y in -1..1); a 120 ms release follows each press; queued after what is waiting", run: |a| if a.is_empty() { Err("usage: pad A; wait 500; DOWN".into()) } else { crate::game::vpad::queue(&a.join(" ")).map(Act::Reply) } },
    Spec { name: "pad_connect", args: "[0|1]", help: "ORCH-1: 1 = pad 0 reports connected and the game rescans its pads (WM_DEVICECHANGE); pad does this by itself", run: |a| Ok(Act::Reply(crate::game::vpad::connect(switch_arg(a, "usage: pad_connect [0|1]")?.unwrap_or(true)))) },
    Spec { name: "pad_clear", args: "", help: "ORCH-1: drop every queued virtual-pad item", run: |_| Ok(Act::Reply(crate::game::vpad::clear())) },
    Spec { name: "pad_status", args: "", help: "ORCH-1: virtual pad: installed or not, XInputGetState calls and thread, items waiting", run: |_| Ok(Act::Reply(crate::game::vpad::status())) },
    Spec { name: "puppet_arm", args: "[0|1]", help: "ORCH-2: 1 = the bot manager's next characters go into the player list as puppet players (use bots 10 after it, offline mode, in an expedition); 0 = bots stay bots; no argument shows the state", run: |a| Ok(Act::Reply(crate::game::puppet::arm_cmd(switch_arg(a, "usage: puppet_arm [0|1]")?))) },
    Spec { name: "puppet_think", args: "[id|keep]", help: "ORCH-2: NpcThink id written into puppet requests (default -1 = none, AFK); keep = the bots' own AI", run: |a| crate::game::puppet::think_cmd(a.first().map(String::as_str)).map(Act::Reply) },
    Spec { name: "puppet_forget", args: "", help: "ORCH-2: the bot manager forgets its 2 characters (they stay), so the next bots request makes 2 more", run: |_| Ok(Act::Reply(crate::game::puppet::forget_cmd())) },
    Spec { name: "peek", args: "<addr|wcm+0x..|exe+0x..> [len]", help: "ORCH-2 research: hex dump of game memory as qwords (read-only; `wcm` = the live WorldChrMan, `exe` = the exe base)", run: |a| crate::game::puppet::peek_cmd(a).map(Act::Reply) },
    Spec { name: "puppet_watch", args: "[slot1|<hex addr>|exec <exe off>|off]", help: "ORCH-2 research: hardware write watch (or execute breakpoint with exec); logs rip (rdx, rcx, return address) and exe return addresses", run: |a| crate::game::puppet::watch_cmd(a).map(Act::Reply) },
    Spec { name: "puppet_session", args: "[keep|null|local]", help: "ORCH-2: what a new puppet's session field (+0x5D0, the one Seamless Co-op checks) becomes: null (default; SC skips it), local (LOCAL's session object) or keep", run: |a| crate::game::puppet::session_cmd(a.first().map(String::as_str)).map(Act::Reply) },
    Spec { name: "puppet_move", args: "<slot> <dx> <dz>", help: "ORCH-2: put the puppet in player slot <slot> at LOCAL's position + (dx, dz) metres", run: |a| crate::game::puppet::move_cmd(a).map(Act::Reply) },
    Spec { name: "puppet_hp", args: "<slot> <hp>", help: "ORCH-2: write a puppet's current HP (0 = as if killed; in a session a player at 0 is downed)", run: |a| crate::game::puppet::hp_cmd(a).map(Act::Reply) },
    Spec { name: "puppet_near_death", args: "[0|1]", help: "ORCH-2: 1 (default) = puppets keep SpEffect 8970061, so every 0 HP is a down like in a session; 0 = only the first", run: |a| Ok(Act::Reply(crate::game::puppet::near_death_cmd(switch_arg(a, "usage: puppet_near_death [0|1]")?))) },
    Spec { name: "puppet_max", args: "[n]", help: "ORCH-2: at most n puppets (0..5, default 5); further bot-manager creations are skipped while armed", run: |a| crate::game::puppet::max_cmd(a.first().map(String::as_str)).map(Act::Reply) },
    Spec { name: "puppet_list", args: "", help: "ORCH-2: every entry of the game's player list (slot, address, vtable, handle, npc id, team, hp, downed state)", run: |_| Ok(Act::Reply(crate::game::puppet::list())) },
    Spec { name: "speffects", args: "", help: "M5-8 research: this game's player's SpEffect ids (revive items 540155 / 6999100 / 6999500 / 8970061, die-at-once 102130, ...)", run: |_| Ok(Act::Reply(crate::game::deathtrace::speffects_cmd())) },
    Spec { name: "flag", args: "<id> [0|1]", help: "M5-4 research: read any event flag; write ours (10007000..=10007999) or the game's 7000 (win), 7001 (loss), 7502 / 7507 (night 1 / 2 done), in this game only", run: flag },
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

fn spectate(args: &[String]) -> Result<Act, String> {
    let on = match args.first().map(String::as_str) {
        None => None,
        Some("0") => Some(false),
        Some("1") => Some(true),
        Some(_) => return Err("expected 0 or 1".into()),
    };
    Ok(Act::Reply(crate::features::night_match::spectate_cmd(on)))
}

fn trace_write(args: &[String]) -> Result<Act, String> {
    let a = args.first().ok_or("usage: trace_write <0xaddr>|0")?;
    let v = a
        .strip_prefix("0x")
        .map_or_else(|| a.parse::<usize>(), |h| usize::from_str_radix(h, 16))
        .map_err(|_| "usage: trace_write <0xaddr>|0")?;
    crate::game::bartrace::set_target(v)?;
    Ok(Act::Reply(format!(
        "trace_write {v:#X}: armed (slot 3); the writers are logged as `T-041 bar_trace` lines"
    )))
}

fn rune_watch(args: &[String]) -> Result<Act, String> {
    let on = match args.first().map(String::as_str) {
        Some("0") => false,
        Some("1") | None => true,
        Some(_) => return Err("expected 0 or 1".into()),
    };
    crate::game::bartrace::set_runes(on);
    Ok(Act::Reply(format!(
        "rune_watch {}: the writers of LOCAL's rune count are logged as `T-041 bar_trace` lines",
        if on { "ON" } else { "OFF" }
    )))
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

fn clock(args: &[String]) -> Result<Act, String> {
    match args.first() {
        None => Ok(Act::Main(MainReq::Clock(ClockCmd::Show))),
        Some(a) => {
            crate::features::clock::parse(a).map(|t| Act::Main(MainReq::Clock(ClockCmd::Set(t))))
        }
    }
}

fn memscan(args: &[String]) -> Result<Act, String> {
    if args.is_empty() {
        return Ok(Act::Reply(crate::game::memscan::last_result()));
    }
    let (kind, rest) = match args[0].as_str() {
        k @ ("utf16" | "ascii" | "hex") => (k, &args[1..]),
        _ => ("utf16", args),
    };
    if rest.is_empty() {
        return Err("usage: memscan [utf16|ascii|hex] <text|hex bytes>".into());
    }
    let needle: Vec<u8> = match kind {
        "hex" => rest
            .join("")
            .as_bytes()
            .chunks(2)
            .map(|c| {
                std::str::from_utf8(c)
                    .ok()
                    .and_then(|t| u8::from_str_radix(t, 16).ok())
                    .ok_or("hex bytes: pairs of hex digits")
            })
            .collect::<Result<_, _>>()?,
        "ascii" => rest.join(" ").into_bytes(),
        _ => crate::game::memscan::utf16_bytes(&rest.join(" ")),
    };
    if needle.len() < 6 {
        return Err("the pattern must be at least 6 bytes".into());
    }
    crate::game::memscan::start_console_scan(needle).map(Act::Reply)
}

fn rival_hit(args: &[String]) -> Result<Act, String> {
    if !args.is_empty() {
        return Err("usage: rival_hit".into());
    }
    crate::game::combat_state::FAKE_RIVAL_HIT.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(Act::Reply(
        "rival_hit: a rival hit on LOCAL is recorded on the next tick".into(),
    ))
}

fn hp0(args: &[String]) -> Result<Act, String> {
    use std::sync::atomic::Ordering;
    if !args.is_empty() {
        return Err("usage: hp0".into());
    }
    crate::game::combat_state::GOD.store(false, Ordering::Relaxed);
    crate::game::combat_state::KILL.store(true, Ordering::Relaxed);
    tracing::info!("console: hp0 requested (god off)");
    Ok(Act::Reply(
        "hp0: LOCAL's HP is written to 0 on the next tick (god off); the result is in the log"
            .into(),
    ))
}

fn god(args: &[String]) -> Result<Act, String> {
    use std::sync::atomic::Ordering;
    let g = &crate::game::combat_state::GOD;
    match args.first().map(String::as_str) {
        None => {}
        Some("0") if args.len() == 1 => g.store(false, Ordering::Relaxed),
        Some("1") if args.len() == 1 => g.store(true, Ordering::Relaxed),
        _ => return Err("usage: god [0|1]".into()),
    }
    let on = g.load(Ordering::Relaxed);
    if !args.is_empty() {
        tracing::info!("console: god {}", if on { "ON" } else { "OFF" });
    }
    Ok(Act::Reply(format!(
        "god {}: {}",
        u8::from(on),
        if on {
            "this game's player is refilled to full HP every tick while up"
        } else {
            "off"
        }
    )))
}

fn night_count(args: &[String]) -> Result<Act, String> {
    let on = match args.first().map(String::as_str) {
        None => None,
        Some("0") if args.len() == 1 => Some(false),
        Some("1") if args.len() == 1 => Some(true),
        _ => return Err("usage: night_count [0|1]".into()),
    };
    Ok(Act::Reply(crate::features::night_match::count_cmd(on)))
}

fn switch_arg(args: &[String], usage: &str) -> Result<Option<bool>, String> {
    match args.first().map(String::as_str) {
        None => Ok(None),
        Some("0") if args.len() == 1 => Ok(Some(false)),
        Some("1") if args.len() == 1 => Ok(Some(true)),
        _ => Err(usage.into()),
    }
}

fn death_trace(args: &[String]) -> Result<Act, String> {
    let on = switch_arg(args, "usage: death_trace [0|1]")?;
    Ok(Act::Reply(crate::game::deathtrace::trace_cmd(on)))
}

fn death_hold(args: &[String]) -> Result<Act, String> {
    let on = switch_arg(args, "usage: death_hold [0|1]")?;
    Ok(Act::Reply(crate::game::deathtrace::hold_cmd(on)))
}

fn flag(args: &[String]) -> Result<Act, String> {
    let id = args
        .first()
        .and_then(|a| a.parse::<u32>().ok())
        .ok_or("flag id must be a number")?;
    let value = match args.get(1).map(String::as_str) {
        None => None,
        Some("1") => Some(true),
        Some("0") => Some(false),
        Some(_) => return Err("value must be 0 or 1".into()),
    };
    Ok(Act::Main(MainReq::Flag(id, value)))
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
        MainReq::Clock(c) => {
            run(Command::Clock(c));
            crate::features::clock::status_line()
        }
        MainReq::NightEnd => {
            run(Command::NightEnd);
            crate::features::nights::status_line()
        }
        MainReq::Flag(f, v) => {
            run(Command::Flag(f, v));
            crate::game::flags::console_reply()
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
    fn clock_parses_and_maps_to_the_clock_command() {
        use crate::game::views::ClockTime;
        assert!(execute_line("clock 25:00").contains("0-23"));
        assert!(execute_line("clock noon").contains("usage: clock"));
        assert!(matches!(
            clock(&[]),
            Ok(Act::Main(MainReq::Clock(ClockCmd::Show)))
        ));
        let night = ClockCmd::Set(ClockTime::new(22, 55, 0).unwrap());
        assert!(matches!(clock(&["night".into()]), Ok(Act::Main(MainReq::Clock(c))) if c == night));
        let mut ran = Vec::new();
        let r = answer(MainReq::Clock(night), &snap(), &mut |c| ran.push(c));
        assert_eq!(ran, vec![Command::Clock(night)]);
        assert!(r.starts_with("clock: "), "{r}");
    }

    #[test]
    fn flag_parses_and_maps_to_its_command() {
        assert!(execute_line("flag x").contains("must be a number"));
        assert!(execute_line("flag 7000 2").contains("0 or 1"));
        assert!(matches!(
            flag(&["7005".into()]),
            Ok(Act::Main(MainReq::Flag(7005, None)))
        ));
        let mut ran = Vec::new();
        answer(MainReq::Flag(7000, Some(true)), &snap(), &mut |c| {
            ran.push(c)
        });
        assert_eq!(ran, vec![Command::Flag(7000, Some(true))]);
    }

    #[test]
    fn night_end_maps_to_its_command() {
        let mut ran = Vec::new();
        let r = answer(MainReq::NightEnd, &snap(), &mut |c| ran.push(c));
        assert_eq!(ran, vec![Command::NightEnd]);
        assert!(r.starts_with("nights: "), "{r}");
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
