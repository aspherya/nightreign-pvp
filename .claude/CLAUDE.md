# nightreign-pvp: agent operating manual

PvPvE mod for ELDEN RING NIGHTREIGN, built on top of Seamless Co-op (SC). Do not load the whole documentation tree by default:
read STATUS.md, then only the doc that owns the topic you need. **Each topic has exactly one owner doc.** When a fact changes, update
it there and nowhere else; other docs link to it instead of repeating it.

| File | Owns |
|---|---|
| [STATUS.md](STATUS.md) | **Read first.** The only "now" doc: what works, what is deployed, what is next, traps, decisions |
| [TASKS.md](TASKS.md) | Live task cards only (finished cards are removed; git keeps them) |
| [re/PROJECT_SCOPE.md](re/PROJECT_SCOPE.md) | Product intent and scope boundaries (Leonardo's spec) |
| [PROJECT_PLAN.md](PROJECT_PLAN.md) | Feature-by-feature feasibility, milestones, parked ideas (§12) |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Code structure, threads, module boundaries, coding rules, refactor plan |
| [CONSOLE.md](CONSOLE.md) | Developer console design (commands, pipe, `nrcon`) |
| [re/MEMORY_MAP.md](re/MEMORY_MAP.md) | **Every memory location we have found**, what it holds, how it behaves, how it was confirmed |
| [TECHNICAL_NOTES.md](TECHNICAL_NOTES.md) | Version pins, stack, Seamless Co-op facts, non-memory engine facts, design decisions log (ADRs) |
| [MATCHMAKING.md](MATCHMAKING.md) | How Steam matchmaking works today: flow, lobby keys, settings, logs, failure patterns |
| [TESTING.md](TESTING.md) | Test environment, test cases, results log, open friend-session runbooks |
| [../README.md](../README.md) | Build, package, install (developers and players) |

Project skills (`.claude/skills/`): `test-report` (turn a reported test run into evidence in the right docs), `release-zip`
(build, verify and record a player zip or Nucleus deploy) and `add-console-command` (every feature gets console access). Planned:
"add a feature" (once the `Feature` trait exists, ARCHITECTURE §6 R3).

Frozen history, read only when a card links it: `research/` (spike memos), `re/er-reference.md` (Elden Ring hints), `experiments/`
(logs and scripts from test runs). They are not kept up to date.

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

## Build

```powershell
./scripts/build.ps1          # cargo build --release, copies DLL to dist/
./scripts/launch.ps1         # me3 launch: SC + nrpvp.dll + dist/gamedata (profiles/nrpvp-sc.me3.toml)
./scripts/package-release.ps1  # player zip dist/NightreignPvP.zip (builds the player DLL: --no-default-features)
```

## Conventions

- Rust stable, `x86_64-pc-windows-msvc`, edition 2021. Crate `crates/nrpvp` is a `cdylib`.
- Logging goes through `tracing` to a file next to the DLL (`nrpvp.log`). Every hook logs install success
  or failure.
- Game-memory access lives behind `crates/nrpvp/src/game/`. Each offset or AOB carries a comment
  naming the game version it was verified on and the re/MEMORY_MAP.md section that documents it (older code still says
  "TECHNICAL_NOTES §4": that content moved to MEMORY_MAP).
- Commit messages: `T-0xx: <what>`.
