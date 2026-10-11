# nightreign-pvp: agent operating manual

PvPvE mod for ELDEN RING NIGHTREIGN, built on top of Seamless Co-op (SC). The original scope lives in
[.claude/references/project-skeleton.md](references/project-skeleton.md). Read it once. These docs refine it:

| File | Purpose |
|---|---|
| [STATUS.md](STATUS.md) | **Read first.** Current state, what is deployed, next human tasks, traps, decisions |
| [PROJECT_PLAN.md](PROJECT_PLAN.md) | **The plan for scope v2 (Battle Royale):** features, milestones M1–M8, spikes, risks |
| [ROADMAP.md](ROADMAP.md) | Phases, current milestone, acceptance criteria, blockers |
| [TASKS.md](TASKS.md) | Task board: the unit of work for agents. **Start here.** |
| [TECHNICAL_NOTES.md](TECHNICAL_NOTES.md) | Stack decision, verified facts, hypotheses, RE findings |
| [re/MEMORY_MAP.md](re/MEMORY_MAP.md) | **Every memory location we have found**, what it holds, how it behaves, and how it was confirmed |
| [TESTING.md](TESTING.md) | Test environment, test cases, results log |
| [FUTURE_CONSIDERATIONS.md](FUTURE_CONSIDERATIONS.md) | Deferred ideas. Never implement from here without a task. |

## Roles

- **Opus (planner):** owns ROADMAP.md, TASKS.md, experiment design, RE judgement calls, review of every
  Sonnet deliverable, and moving facts from "hypothesis" to "verified" in TECHNICAL_NOTES.md.
- **Sonnet (executor):** runs exactly one task card at a time from TASKS.md, via the `implementer` or
  `researcher` agent in [.claude/agents/](agents/). It does not re-scope, does not start the next card, and
  stops and reports when blocked.
- **Human (Leonardo + test partner):** installs tools, launches the game, runs in-game tests, and reports
  results. Agents cannot play the game. Any acceptance step that needs the game running is a HUMAN step.

## Hard rules

1. **Evidence over assumptions.** Each claim in the docs is tagged `[VERIFIED: how]`, `[SOURCE: link]`, or
   `[HYPOTHESIS]`. Never invent offsets, function names, AOBs, param fields, enum values, or me3 or SC
   behavior. If you don't know, write `UNKNOWN` and propose how to find out.
2. **Never go online with a modded game.** Launch only through SC's launcher or the me3 profile in
   `profiles/`. Both run without EasyAntiCheat. Never launch `start_protected_game.exe` with mods present.
   Back up `%APPDATA%\Nightreign\<steamid>\` before any save-touching experiment.
3. **Do not modify, patch, decompile-for-redistribution, or redistribute SC (`nrsc.dll`).** It is
   closed-source third-party code. We interoperate with it at runtime only.
4. **Pin versions.** Record game build, SC hash, and me3 version in every test result. The current pins
   are in TECHNICAL_NOTES.md §1.
5. **Smallest change that proves the point.** No team system, UI, lobby, friendly-fire filtering, or
   endgame work until ROADMAP.md says that phase is active.
6. **Stay inside the task card.** When you finish one, update its status in TASKS.md, append findings to the
   right doc, and stop.

## Repo layout

- Planning docs (this file, TASKS.md, …): `.claude/`. Research write-ups: `.claude/research/`.
  Reverse-engineering notes and CE tables: `.claude/re/`.
- Code at the repo root: `Cargo.toml`, `crates/nrpvp/`, `profiles/`, `scripts/`.
- Rust lives in `%USERPROFILE%\.cargo\bin`. If `cargo` isn't on PATH in a shell, call
  `~/.cargo/bin/cargo.exe` directly.

## Testing constraint

Day to day, **only Leonardo tests, solo, on one PC**. A friend joins only for scheduled **Friend Sessions
(FS-n)**. Design every card so its acceptance can be checked solo where possible. Anything that needs two
players goes into the next FS runbook (TESTING.md §6), not into the card's own acceptance.

## Paths

- Game: `Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game`
- SC: `<Game>\SeamlessCoop\nrsc.dll`, `nrsc_settings.ini`, launcher `<Game>\nrsc_launcher.exe`
- me3: `Z:\Tools\me3\bin\me3.exe` (on PATH)
- Saves: `%APPDATA%\Nightreign\<steamid64>\` (vanilla `.sl2`, SC uses `.co2`)
- Cheat Engine: `C:\Program Files\Cheat Engine*`

## Build (once T-001 lands)

```powershell
./scripts/build.ps1          # cargo build --release, copies DLL to dist/
./scripts/launch.ps1         # me3 launch with profiles/nrpvp-dev.me3.toml
```

## Conventions

- Rust stable, `x86_64-pc-windows-msvc`, edition 2021. Crate `crates/nrpvp` is a `cdylib`.
- Logging goes through `tracing` to a file next to the DLL (`nrpvp.log`). Every hook logs install success
  or failure.
- Game-memory access lives behind `crates/nrpvp/src/game/`. Each offset or AOB carries a comment
  naming the game version it was verified on and the TECHNICAL_NOTES.md entry that documents it.
- Commit messages: `T-0xx: <what>`.
