# Orchestrator (`nro`): launch, drive and judge an offline game

Lets Claude Code (or you) launch an **offline** `nightreign-pvp` developer game through me3, drive it (console, keys, screenshots), fill it
with **AFK puppet players** and run scenarios, with no second client and no Nucleus. Lives in this repo: `crates/nro-core` (preflight,
profile, scenario model), `crates/nro` (CLI + desk), `crates/nro-mcp` (MCP server `nightreign`, registered by `.mcp.json`), `scenarios/`,
`pins/nightreign.toml`, `tools/`. Runs go to `runs\<id>\` (gitignored): `report.json`, `watch.log`, `shots\`, a copy of the mod log.

- **Puppet players** are real `PlayerIns` in the game's player list (dev build only, `game/puppet.rs`, MEMORY_MAP §18; how they were found:
  `../research/ORCH-R2-net-player-creation.md`): the game and the mod treat them as rivals p2..p6; they stand still, take hits, go down
  (bar 40, bleed-out ~15 s) and die. Limits: offline mode and a loaded world only, an area change removes them, no AI, no network
  (the other screen's view and SC host/client stay friend-session checks).
- Every launch passes a **preflight** (no "force"): the only route is `me3 launch -g nightreign -e <game>\nightreign.exe -p <run profile>`;
  the profile has no `start_online` / `savefile`; hashes of game, SC, regulation and me3 match `pins/nightreign.toml`; no game, EAC or
  `start_protected_game.exe` running; Steam running; the staged `nrpvp.toml` is dev mode with the public queue off; vanilla saves hashed.
  After launch me3's attach config must say `start_online: false` and list exactly the staged modules, a watchdog kills the game if EAC
  appears, logs every remote TCP endpoint and checks the vanilla saves at the end. This keeps the official servers and EAC out; it cannot
  promise there is no ban risk. Never go online with a modded game (CLAUDE.md hard rule 2).
- The contract with the mod (what `nro` reads from it): [CONTRACT.md](CONTRACT.md). Design history: `../research/ORCH-plan.md`.

## Setup (once)

1. Build the mod dev DLL and game data into `dist\`: `scripts\build.ps1`, `scripts\build-gamedata.ps1`.
2. Build the tools: `cargo build --release -p nro -p nro-mcp`.
3. Steam running and signed in. Start **`target\release\nro.exe desk` in a normal terminal** (an agent can start it in the Terminal panel)
   and leave it open: keys and screenshots need a process on the desktop; launch, console and puppets work without it.

## Use

```text
scripts\regress.ps1 [name ...]       run scenarios\*.toml (all by default) against dist\; one PASS/FAIL line each
nro run scenarios\night1-count.toml  one scenario (about 3-4 min): stage, launch, steps, report.json, stop
nro launch --world --set mode="offline"      launch and press through the title (default mod build: dist\ of this checkout)
nro navigate expedition              Roundtable -> offline Solo expedition, off the hawk
nro cmd "clock end"                  any mod console command (nro cmd help)
nro puppets 3 / puppet_front 1.2 / puppet_ring 4     AFK puppets, placed in front of / around you
nro cmd "puppet_hp 1 0"              puppet in slot 1 to 0 HP = down (use this, not a swing: one swing does not reach every puppet)
nro keys "click 0.5 0.5"             keys and mouse (F interact, Q back, W/A/S/D, ALT off the hawk)
nro shot name / nro log 50 filter / nro stop
```

Scenario catalog, step vocabulary and how to add one: [SCENARIOS.md](SCENARIOS.md). Scenario format (`crates/nro-core/src/scenario.rs`): `[config]` keys written over the staged `nrpvp.toml`, then `[[step]]` entries:
`console`, `wait = {console, contains, timeout_s}`, `expect = {console, contains, absent}`, `keys`, `navigate`, `sleep_ms`, `screenshot`,
`puppets`, `puppet_ring`, `puppet_front`. Put `ui_flag_watch = false` in `[config]` (its event bursts flood `log 300`). Current set:
`puppets-smoke`, `day1-free`, `defeat-mob`, `defeat-rival`, `match-win`, `match-win-end`, `match-loss` (catalog and what each proves: SCENARIOS.md).
