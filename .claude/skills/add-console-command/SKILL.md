---
name: add-console-command
description: Add or change a developer console command in nrpvp (crates/nrpvp/src/console) - registry entry, argument parsing, where it executes (pipe thread vs main loop), test, and the CONSOLE.md table. Use whenever a feature needs to be driven or inspected from nrcon.exe, or a card asks for a new console command.
---

# Add a console command

Every feature that has state or a test switch should be reachable from `nrcon.exe`, so agents can drive and inspect a running game
without asking Leonardo to press anything. Commands are developer-build only (`console/` is behind the `dev` feature).

## 1. Name and shape (CONSOLE.md §3)

- One word with underscores, CS style: `bleed_clock`, `mm_queue_solo`. Values follow after spaces: `team_set 29 rivals`.
- Player commands take an optional target last (`me` default, `all`, `allies`, `rivals`, `p1`..`p6`, `s0`..`s5`): parse it with
  `target::parse_target`, resolve it in the main loop with `target::pick` against the snapshot rows.

## 2. Where it runs

| The command... | Returns | Runs on |
|---|---|---|
| needs no game state (help text, `log`, `mm_status`) | `Act::Reply(text)` | the pipe thread |
| reads or writes game memory, or sends to the matchmaking worker | `Act::Main(MainReq::...)` | the main loop, next tick |

Main-loop work always goes through an existing `overlay::shared::Command` executed by `game::run_command`, so the console uses the same
gated code as everything else. If no `Command` fits, add one there (and its arm in `run_command`) rather than touching memory in
`console/`. A command that needs the player list or several commands gets its own `MainReq` variant and an arm in `console::answer`.

## 3. Steps

1. Add a `Spec { name, args, help, run }` line to `COMMANDS` in `crates/nrpvp/src/console/mod.rs` (the order is the `help` order).
2. Write the `run` fn: validate arguments and return `Err("what is wrong")` on bad input (the usage line is appended for you).
3. Main-loop commands: map to `Command`s in `answer` (or reuse `MainReq::Run(vec![..], "ack text")`). The reply says what happened
   in one line; details go to the log.
4. Test in `console/mod.rs` `mod tests`: parsing errors through `execute_line`, and the `Command`s produced through `answer` with a
   fake `Snapshot` (see `team_commands_map_to_the_existing_main_loop_commands`).
5. Add the row to the "Commands available now" table in `.claude/CONSOLE.md` §0.
6. `cargo test -p nrpvp`, `cargo clippy --workspace --all-targets -- -D warnings`, and the player build:
   `cargo clippy -p nrpvp --no-default-features --all-targets -- -D warnings`.

## 4. Check it live (when a developer game is running)

`dist\nrcon.exe help <name>`, then the command itself; `dist\nrcon.exe log 20` shows the `console: <line> -> <reply>` line and what the
main loop logged.
