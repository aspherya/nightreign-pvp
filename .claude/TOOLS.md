# Tools and how to run them

Owner doc for build, deploy, test tooling and logs. What the mod does is in STATUS; test results in TESTING / `testing-log/`.

## Which route tests what

| I need to check... | Route | Owner doc |
|---|---|---|
| a rule or feature that needs our event files (nights, hold/release, Roundtable menu, hawk line), with other players | **Orchestrator `nro`** (offline game through me3, AFK puppet players) | [orchestrator/README.md](orchestrator/README.md) |
| a quick solo look at something | `scripts\launch.ps1` (SC + dev DLL + game data) and `dist\nrcon.exe` | CONSOLE.md |
| plain rules between two real clients on one PC (downed rule, teams, revive); no event files | **Nucleus, secondary tool** (`scripts\deploy-nucleus.ps1`) | below |
| real networking: Steam matchmaking, SC host/client, the other screen's view, lag | **Friend session** with the player zip | TESTING.md §6 |

## Commands

| Tool | Command | Note |
|---|---|---|
| Build the dev mod | `powershell -NoProfile -File scripts\build.ps1` | `dist\nrpvp.dll`; fails to copy while a game uses it |
| Build game data | `powershell -ExecutionPolicy Bypass -File scripts\build-gamedata.ps1` | `dist\gamedata` + `dist\gamedata-player` (needs the 1.03.3 unpack in `Z:\Tools\nr-unpacked\1.03.3\`, Python) |
| Build the orchestrator | `cargo build --release -p nro -p nro-mcp` | `target\release\nro.exe`, `nro-mcp.exe` (`.mcp.json` registers the MCP server `nightreign`) |
| Orchestrator desk | `target\release\nro.exe desk` in a normal terminal (an agent can start it in the Terminal panel) | needed for keys and screenshots |
| Run scenarios | `powershell -ExecutionPolicy Bypass -File scripts\regress.ps1 [name ...]` | all `scenarios\*.toml` by default, one PASS/FAIL line each; reports in `runs\<id>\report.json` (gitignored) |
| By hand | `nro launch --world`, `nro navigate expedition`, `nro cmd "<console line>"`, `nro puppets 3`, `nro shot name`, `nro log 50 filter`, `nro stop` | `nro` with no argument lists them; default mod build = `dist\` of this checkout |
| Console of any dev game | `dist\nrcon.exe list / status / players / log 30 / -i N <cmd> / --all <cmd>` | commands: CONSOLE.md §0 |
| Player zip | `powershell -ExecutionPolicy Bypass -File scripts\package-release.ps1 [-MatchPlayers N] [-TeamSize N]` | `release-zip` skill; builds the player DLL itself |
| Tests | `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo clippy -p nrpvp --no-default-features --all-targets -- -D warnings`, `cargo fmt --all --check` | the same checks run in GitHub Actions |

## Nucleus (secondary tool)

`scripts\deploy-nucleus.ps1 [-TeamSize N]` copies the dev DLL into the Nucleus handler (`D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN`), close Nucleus first; it rewrites the handler's settings; logs go to `dist\nucleus-logs\Instance<N>\`; `dist\nrcon.exe --all <cmd>` reaches every instance. It starts the game through Seamless Co-op's launcher, so **our game-data package is not loaded** (no no-boss nights, no Roundtable menu, no hawk line): use the console (`flag`, `clock`, `god`) as stand-ins. Keys reach a window only when it has focus (END unlocks input). The old `-Me3` mode (start through me3) failed for window 2 and was removed (`research/` notes in the git history of `claude/nucleus-me3`). Traps: TRAPS.md.

## Logs

The mod writes `nrpvp.log` (current session) and `logs\nrpvp_<UTC>.log` (newest 30) next to the DLL; orchestrator runs keep a copy in `runs\<id>\mod\nrpvp.log`. me3's own logs: `%LOCALAPPDATA%\garyttierney\me3\data\logs\<profile>\` (check that `nrsc.dll` loaded early and `nrpvp.dll` after it).

## How the in-game UI works (pointer)

UI-1 uses hardware breakpoints (shared owner `src/hwbp.rs`: slots 0/1 quiet invite, 2 Commence, 3 labels), no code patched; UI-2 appends our events to the Roundtable event script and the DLL sets flags 10007200+k. The game-data package `dist\gamedata` (texts in 15 languages + the event scripts) is built from `gamedata\*.json`. Details: `research/UI-0-ingame-ui.md`, MATCHMAKING.md.
