//! nrcon (CON-2): talk to the nrpvp developer console of running games (CONSOLE.md).
//!
//! Every developer-build game with `dev_mode = true` opens `\\.\pipe\nrpvp-<pid>`. nrcon finds them, sends one line per request and
//! prints the reply (lines up to the end marker, ASCII EOT). Standard library only: a named pipe opens like a file.
//!
//! ```text
//! nrcon list                       running games: index, pid, DLL folder (Nucleus: ...\Instance0\...)
//! nrcon status                     one command to the only running game
//! nrcon -i 1 "team_set 29 rivals"  game number 1 from `nrcon list`
//! nrcon --pid 1234 players         by process id
//! nrcon --all status               every running game, one block each
//! nrcon                            interactive (exit / quit to leave)
//! ```

use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const PIPE_DIR: &str = r"\\.\pipe\";
const PREFIX: &str = "nrpvp-";
const END: &str = "\u{4}";
const OPEN_WAIT: Duration = Duration::from_secs(5);

const HELP: &str = "nrcon - developer console client for nrpvp (CONSOLE.md)

usage:
  nrcon list                      running games: index, pid, DLL folder
  nrcon [target] <command...>     send one command line (several with ';')
  nrcon [target]                  interactive; `exit` or `quit` to leave
targets:
  -i N | --instance N             game N from `nrcon list` (sorted by DLL folder, so Nucleus Instance0 comes first)
  --pid P                         game with process id P
  --all                           every running game
With no target, nrcon uses the only running game. `nrcon help` without a game prints this; with a game, `help` lists its commands.";

#[derive(Debug, PartialEq)]
enum Sel {
    Only,
    Index(usize),
    Pid(u32),
    All,
}

#[derive(Debug, PartialEq)]
struct Args {
    sel: Sel,
    /// The command line to send; empty = interactive.
    line: String,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut sel = Sel::Only;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-i" | "--instance" => {
                let n = args
                    .get(i + 1)
                    .and_then(|v| v.parse().ok())
                    .ok_or("-i needs a number")?;
                sel = Sel::Index(n);
                i += 2;
            }
            "--pid" => {
                let p = args
                    .get(i + 1)
                    .and_then(|v| v.parse().ok())
                    .ok_or("--pid needs a number")?;
                sel = Sel::Pid(p);
                i += 2;
            }
            "--all" => {
                sel = Sel::All;
                i += 1;
            }
            _ => break,
        }
    }
    Ok(Args {
        sel,
        line: args[i..].join(" "),
    })
}

/// A running game: its pipe and what it said about itself.
struct Game {
    pid: u32,
    label: String,
}

impl Game {
    fn path(&self) -> PathBuf {
        PathBuf::from(format!("{PIPE_DIR}{PREFIX}{}", self.pid))
    }
}

/// Pids of every nrpvp pipe currently open.
fn pipe_pids() -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir(PIPE_DIR) else {
        return Vec::new();
    };
    dir.filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str()?.strip_prefix(PREFIX)?.parse().ok())
        .collect()
}

/// Every running game, sorted by DLL folder then pid (Nucleus `Instance0` first).
fn games() -> Vec<Game> {
    let mut out: Vec<Game> = pipe_pids()
        .into_iter()
        .filter_map(|pid| {
            let mut c = Conn::open(pid).ok()?;
            let who = c.ask("whoami").ok()?.join(" ");
            let label = who
                .split_once(' ')
                .and_then(|(_, rest)| rest.split_once(' '))
                .map(|(_, l)| l.to_string())
                .unwrap_or_default();
            Some(Game { pid, label })
        })
        .collect();
    out.sort_by(|a, b| a.label.cmp(&b.label).then(a.pid.cmp(&b.pid)));
    out
}

struct Conn {
    w: File,
    r: BufReader<File>,
}

impl Conn {
    fn open(pid: u32) -> io::Result<Conn> {
        let path = Game {
            pid,
            label: String::new(),
        }
        .path();
        let t0 = Instant::now();
        // 231 = all instances busy (another client is being served), 2 = between instances; both clear within milliseconds.
        let f = loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
            {
                Ok(f) => break f,
                Err(e) if t0.elapsed() < OPEN_WAIT && matches!(e.raw_os_error(), Some(2 | 231)) => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                Err(e) => return Err(e),
            }
        };
        Ok(Conn {
            w: f.try_clone()?,
            r: BufReader::new(f),
        })
    }

    fn ask(&mut self, line: &str) -> io::Result<Vec<String>> {
        self.w.write_all(format!("{line}\n").as_bytes())?;
        self.w.flush()?;
        read_reply(&mut self.r)
    }
}

/// Lines up to the end marker.
fn read_reply(r: &mut impl BufRead) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    loop {
        let mut l = String::new();
        if r.read_line(&mut l)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the game closed the console",
            ));
        }
        let l = l.trim_end_matches(['\r', '\n']);
        if l == END {
            return Ok(lines);
        }
        lines.push(l.to_string());
    }
}

fn pick(sel: &Sel, all: &[Game]) -> Result<Vec<usize>, String> {
    let listing = || {
        all.iter()
            .enumerate()
            .map(|(i, g)| format!("\n  [{i}] pid {} {}", g.pid, g.label))
            .collect::<String>()
    };
    match sel {
        _ if all.is_empty() => {
            Err("no running game with the console (developer build, dev_mode = true)".into())
        }
        Sel::All => Ok((0..all.len()).collect()),
        Sel::Only if all.len() == 1 => Ok(vec![0]),
        Sel::Only => Err(format!(
            "{} games are running; pick one with -i N, --pid P or --all:{}",
            all.len(),
            listing()
        )),
        Sel::Index(i) if *i < all.len() => Ok(vec![*i]),
        Sel::Index(i) => Err(format!("no game {i}:{}", listing())),
        Sel::Pid(p) => all
            .iter()
            .position(|g| g.pid == *p)
            .map(|i| vec![i])
            .ok_or_else(|| format!("no game with pid {p}:{}", listing())),
    }
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&raw) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    let all = games();
    if args.line == "list" {
        if all.is_empty() {
            println!("no running game with the console");
        }
        for (i, g) in all.iter().enumerate() {
            println!("[{i}] pid {} {}", g.pid, g.label);
        }
        return;
    }
    let idx = match pick(&args.sel, &all) {
        Ok(v) => v,
        Err(e) => {
            if matches!(args.line.as_str(), "help" | "-h" | "--help") {
                println!("{HELP}");
                return;
            }
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if args.line.is_empty() {
        interactive(&all[idx[0]]);
        return;
    }
    let mut failed = false;
    for i in &idx {
        let g = &all[*i];
        let res = Conn::open(g.pid).and_then(|mut c| c.ask(&args.line));
        if idx.len() > 1 {
            println!("== [{i}] pid {} {}", g.pid, g.label);
        }
        match res {
            Ok(lines) => lines.iter().for_each(|l| println!("{l}")),
            Err(e) => {
                eprintln!("pid {}: {e}", g.pid);
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}

fn interactive(g: &Game) {
    let mut c = match Conn::open(g.pid) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("pid {}: {e}", g.pid);
            std::process::exit(1);
        }
    };
    println!(
        "connected to pid {} {} (help lists the commands; exit to leave)",
        g.pid, g.label
    );
    let stdin = io::stdin();
    loop {
        print!("nrpvp> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let line = line.trim();
        if matches!(line, "exit" | "quit") {
            return;
        }
        match c.ask(line) {
            Ok(lines) => lines.iter().for_each(|l| println!("{l}")),
            Err(e) => {
                eprintln!("{e}");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn args_select_and_join_the_command() {
        assert_eq!(
            parse_args(&a(&["status"])),
            Ok(Args {
                sel: Sel::Only,
                line: "status".into()
            })
        );
        assert_eq!(
            parse_args(&a(&["-i", "1", "team_set", "29", "rivals"])),
            Ok(Args {
                sel: Sel::Index(1),
                line: "team_set 29 rivals".into()
            })
        );
        assert_eq!(
            parse_args(&a(&["--pid", "77", "players"])),
            Ok(Args {
                sel: Sel::Pid(77),
                line: "players".into()
            })
        );
        assert_eq!(
            parse_args(&a(&["--all"])),
            Ok(Args {
                sel: Sel::All,
                line: String::new()
            })
        );
        assert!(parse_args(&a(&["-i"])).is_err());
    }

    #[test]
    fn replies_end_at_the_marker() {
        let mut r = io::Cursor::new(format!("a\nb\n{END}\nnext\n"));
        assert_eq!(read_reply(&mut r).unwrap(), vec!["a", "b"]);
        let mut r = io::Cursor::new("a\n");
        assert!(read_reply(&mut r).is_err());
    }

    #[test]
    fn picking_a_game() {
        let g = |pid| Game {
            pid,
            label: String::new(),
        };
        let two = vec![g(1), g(2)];
        assert_eq!(pick(&Sel::Only, &[g(5)]), Ok(vec![0]));
        assert!(pick(&Sel::Only, &two).unwrap_err().contains("2 games"));
        assert_eq!(pick(&Sel::Pid(2), &two), Ok(vec![1]));
        assert_eq!(pick(&Sel::All, &two), Ok(vec![0, 1]));
        assert!(pick(&Sel::Index(9), &two).is_err());
        assert!(pick(&Sel::All, &[]).is_err());
    }

    #[test]
    fn the_pipe_directory_can_be_listed() {
        // Holds on any Windows machine; the result is usually empty here (no game running).
        let _ = pipe_pids();
        assert!(std::fs::read_dir(PIPE_DIR).is_ok());
    }
}
