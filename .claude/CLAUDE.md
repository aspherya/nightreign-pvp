# nightreign-pvp: agent operating manual

PvPvE mod for ELDEN RING NIGHTREIGN, built on top of Seamless Co-op (SC). **Do not load the whole documentation tree.** Read this file,
then [STATUS.md](STATUS.md) (≈6 KB, the only "now" doc), then only the doc that owns the topic you need. **Each topic has exactly one
owner doc.** When a fact changes, update it there and nowhere else; other docs link to it instead of repeating it. The "Read when"
column says when a doc is worth its tokens.

| File | Owns | Read when |
|---|---|---|
| [STATUS.md](STATUS.md) | What works, what is deployed, what is next (≤ 8 KB) | **Always, first** |
| [TASKS.md](TASKS.md) | Live task cards only (finished cards are removed; git keeps them) | You are given or pick a card |
| [TOOLS.md](TOOLS.md) | Build, deploy, which test route tests what, logs, Nucleus (secondary) | Building, testing, deploying |
| [TRAPS.md](TRAPS.md) | Mistakes we already made (hardware slots, Nucleus, saves, console tails…) | Touching an area, or debugging something odd |
| [DECISIONS.md](DECISIONS.md) | Decisions with date and who decided | Before re-arguing or changing a direction |
| [re/PROJECT_SCOPE.md](re/PROJECT_SCOPE.md) | Product intent and scope boundaries (Leonardo's spec) | Designing a feature |
| [PROJECT_PLAN.md](PROJECT_PLAN.md) | Feasibility, milestones, parked ideas (§12) | Planning a milestone |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Code structure, threads, module boundaries, coding rules | Writing or moving code |
| [REFACTOR_PLAN.md](REFACTOR_PLAN.md) | Phase 2 (plug-and-play modes, 13 PRs) and phase 3 (orchestrator, scenarios) plan with acceptance per PR | Doing or reviewing the refactor |
| [CONSOLE.md](CONSOLE.md) | Developer console design and command table | Adding or using a console command |
| [re/MEMORY_MAP.md](re/MEMORY_MAP.md) | **Every memory location we found**, behaviour, how it was confirmed | Touching game memory |
| [TECHNICAL_NOTES.md](TECHNICAL_NOTES.md) | Version pins, stack, Seamless Co-op facts, non-memory engine facts, ADRs | Version or engine questions |
| [NETWORKING.md](NETWORKING.md) | Replacing Seamless Co-op: what SC does, game netcode, transport and session options, phased plan, gates | Cross-game data, anything about SC or sessions |
| [HOST_MIGRATION.md](HOST_MIGRATION.md) | Host migration and player count: findings, design, plan, budgets | Anything about a host leaving or more players |
| [BOTS.md](BOTS.md) | AI bots that fill a match: design, plan, open questions (friends + bots) | Anything about bots |
| [MATCHMAKING.md](MATCHMAKING.md) | How Steam matchmaking works today: flow, lobby keys, logs, failures | Matchmaking work |
| [TESTING.md](TESTING.md) | Test environment, cases, regression set, Friend Session runbooks (§6) | Planning or judging a test |
| [testing-log/](testing-log/) | Results log, one entry per run, newest file last (append-only) | Looking up what a past run showed |
| [orchestrator/GUIDE.md](orchestrator/GUIDE.md) | **How to use `nro`**: setup, commands, examples, scenarios, two instances | Testing with `nro` |
| [orchestrator/README.md](orchestrator/README.md), [CONTRACT.md](orchestrator/CONTRACT.md) | `nro` design, puppet players, safety checks | Understanding `nro` |
| [RELEASE.md](RELEASE.md) | **How to configure, build and release the player zip**, test with a friend, scale to more players | Releasing |
| [orchestrator/SCENARIOS.md](orchestrator/SCENARIOS.md) | Catalog of automated one-PC scenarios, how to run, how to add one | **Before changing or adding a feature** (what to re-run, which test to write) |
| [../README.md](../README.md) | Build, package, install (developers and players) | Releasing |

Project skills (`.claude/skills/`): `test-report` (turn a reported test run into evidence in the right docs), `release-zip`
(build, verify and record a player zip or Nucleus deploy) and `add-console-command` (every feature gets console access). Planned:
"add a feature" (once the `Feature` trait exists, ARCHITECTURE §6 R3).

Frozen history, read only when a card links it: [archive/INDEX.md](archive/INDEX.md) lists it (spike memos, hand-overs, `archive/er-reference.md`,
scripts and logs of finished runs, which doc owns each surviving fact). It is not kept up to date. `.claude/experiments/` is only the
gitignored drop folder for friend-session logs.

**Language:** code, comments and docs are English only (Leonardo, 2026-11-01); chat with him may be Portuguese.

**How a feature is specified (spec-driven, light):** a card in TASKS.md is the spec: goal, rules (link `re/PROJECT_SCOPE.md`), build
steps and an **acceptance that is a scenario** (`scenarios/<name>.toml`, run with `scripts\regress.ps1`) wherever one PC can show it;
what only two real PCs can show goes into the next Friend Session runbook (TESTING §6). Done = scenario PASS + a `testing-log/` entry +
the card removed. Doc hygiene: STATUS stays under ~8 KB (replace, do not append), results go to `testing-log/`, new traps to
TRAPS.md, new decisions to DECISIONS.md.

## Roles

- **Opus (planner):** owns STATUS.md, TASKS.md, experiment design, RE judgement calls, review of every
  Sonnet deliverable, and moving facts from "hypothesis" to "verified" in re/MEMORY_MAP.md / TECHNICAL_NOTES.md.
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
   endgame work without a card in TASKS.md.
6. **Stay inside the task card.** When you finish one, update its status in TASKS.md, put findings in the doc that owns the
   topic (table above), and stop. When a card is done and accepted, remove it from TASKS.md.
7. **Follow ARCHITECTURE.md** for where code goes (offsets, features, dev-only code, file size).

## Repo layout

- Planning docs (this file, TASKS.md, …): `.claude/`. Frozen research and hand-overs: `.claude/archive/` (live RW-3 hand-over notes still in `.claude/research/`).
  Reverse-engineering notes and CE tables: `.claude/re/`.
- Code at the repo root: `Cargo.toml`, `crates/nrpvp/` (the mod), `crates/nrcon`, `crates/nrpvp-launcher`, `crates/nro*` (orchestrator), `scenarios/`, `pins/`, `tools/`, `profiles/`, `scripts/`.
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

## Build

```powershell
./scripts/build.ps1          # cargo build --release, copies DLL to dist/
./scripts/launch.ps1         # me3 launch: SC + nrpvp.dll + dist/gamedata (profiles/nrpvp-sc.me3.toml)
./scripts/package-release.ps1  # player zip dist/NightreignPvP.zip (builds the player DLL: --no-default-features)
./scripts/regress.ps1        # orchestrator scenarios on one PC (needs `nro desk`, TOOLS.md)
```

## Conventions

- Rust stable, `x86_64-pc-windows-msvc`, edition 2021. Crate `crates/nrpvp` is a `cdylib`.
- Logging goes through `tracing` to a file next to the DLL (`nrpvp.log`). Every hook logs install success
  or failure.
- Game-memory access lives behind `crates/nrpvp/src/game/`. Each offset or AOB carries a comment
  naming the game version it was verified on and the re/MEMORY_MAP.md section that documents it (older code still says
  "TECHNICAL_NOTES §4": that content moved to MEMORY_MAP).
- Commit messages: `T-0xx: <what>`.
