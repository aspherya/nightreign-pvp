# Nightreign PvP Orchestrator: plan v3 (2026-10-23)

> **Outcome (2026-10-23, after review).** Leonardo: Nucleus is not an option; make several AFK fake players that the game and the
> mod see as players, in one game. Done: **puppet players** (§3 approach D, built a different way than E3 planned: the bot
> manager's PlayerIns creator redirected into the player list, Seamless Co-op's sweep guarded, near-death SpEffect managed),
> plus M1 (launch, console, navigation, input, screenshots, scenarios, MCP). Results and limits: `docs/STATUS.md`; the engine
> work: `docs/research/R2-net-player-creation.md`. The rest of this document is the plan as reviewed.

Brief: [`plan.md`](../plan.md). This plan replaces plan v2 (commit `cc65f2e`), which built its own launcher and lab DLL apart from
`nightreign-pvp`. The brief asks for the reverse: use the mod's own build, launch and console, and keep only orchestration here.

Tags, the same as the mod repo uses (`nightreign-pvp/.claude/CLAUDE.md` hard rule 1):
`[VERIFIED: how]` checked in the repo or on this PC today · `[PROJECT: file §]` stated by the mod repo's docs (they were verified there,
not re-run by me) · `[SOURCE: link]` external · `[HYPOTHESIS]` · `UNKNOWN`. Paths starting with `pvp/` are under
`C:\Users\Leonardo\source\repos\nightreign-pvp\`.

Nothing in `nightreign-pvp` was changed. Nothing was launched. The only things run were reads of files, `me3 --help`/`--version`,
and a process list.

---

## 0. Summary

1. **There is no known way today to make a second real player inside one game.** In this mod a "player" is an entry in the game's
   player list (`WorldChrMan+0x9A40`, 6 slots, `pvp/.claude/re/MEMORY_MAP.md` §2). The entry is backed by a network session through
   Seamless Co-op (SC). Every rule that involves two players reads that list: teams, the downed rule, night elimination. Today the only
   way to fill a second slot is a second game client, through Nucleus or a friend's PC. `[PROJECT: STATUS §1, MEMORY_MAP §2]`
2. **Two partial substitutes already work in one instance.**
   - **NPC bots.** The game ships its own AI Nightfarers. The mod spawns 5 of them as hostile rivals in an offline expedition
     (`[PROJECT: TESTING §3 OFF-5 run 2]`).
   - **Unit tests.** The mod's pure rules have 484 tests built on fakes (`[PROJECT: STATUS §7]`).
   Both are useful, but neither is a player. Details in §3.
3. **One route could give a real one-instance fake player, and it is untested.** The game would create a player object for a
   "remote" player that has no peer behind it. These classes exist in `nightreign.exe`: `PlayerNullSession`, `PlayerDebugSession`,
   `CSDebugBankPlayerMng`, and about ten pre-made player objects. `[VERIFIED: exe strings today; PROJECT: S10 §2, MEMORY_MAP §2]`
   **Feasibility: UNKNOWN.** Milestone 0 has one measurable experiment to settle it (E3).
4. **Launcher: use the same me3 + profile path, not `nrpvp_launcher.exe` itself, for developer tests.** The launcher is a player
   tool. It is silent: its errors are message boxes. It runs whatever is installed in the game folder, and it passes
   `me3-args.txt` straight through to me3, so a stray `--online true` would go through too. The orchestrator should build the same
   profile itself and launch me3 directly, behind a preflight, from a staged copy of `dist\`. Keep `nrpvp_launcher.exe` for one
   scenario only: the player-zip regression. Details in §4.
5. **Recommended architecture: a separate Rust repository with a small, explicit contract to the mod.** The contract is the mod's
   scripts, its console pipe and its logs. Add a few dev-only test hooks to the mod through normal mod cards. The core logic sits
   behind both a CLI and an MCP server. Details in §5–§6.
6. **First milestone: experiments (M0), then M1.** M1 is a fully autonomous solo scenario: build → safe launch → expedition → hostile
   bots → evidence → verdict → teardown. It removes the human from solo tests, and removes Nucleus for every test whose second party
   only needs to be an opponent, not a player. What happens to Nucleus for player-vs-player rules depends on E3. Details in §7.

---

## 1. Current testing workflow

### 1.1 What is verified, built-but-unverified, and how

| Area | State | Evidence |
|---|---|---|
| Build | `scripts\build.ps1` → `dist\nrpvp.dll`, `.pdb`, `nrcon.exe`. Fails if a running game holds the DLL | `[VERIFIED: read pvp/scripts/build.ps1]` `[PROJECT: STATUS §5]` |
| Game data package | `scripts\build-gamedata.ps1` → `dist\gamedata` (texts, event scripts, hawk route). Needs the unpack in `Z:\Tools\nr-unpacked\1.03.3` and Python | `[PROJECT: STATUS §3]` |
| Developer launch | `scripts\launch.ps1` = `me3 launch -g nightreign -p profiles\nrpvp-sc.me3.toml` (SC early + `nrpvp.dll` after it + `dist\gamedata`) | `[VERIFIED: read script + profile]` |
| Player launch | `nrpvp_launcher.exe` in the game folder → bundled me3 with a profile it writes | `[VERIFIED: read pvp/crates/nrpvp-launcher/src/main.rs]` |
| Control | `dist\nrcon.exe` over `\\.\pipe\nrpvp-<pid>` (one line in, reply lines + EOT; local clients only; DACL = the user). About 50 commands: `status`, `players`, `log`, `mode`, `bots`, `bots_hostile`, `clock`, `flag`, `god`, `night_*`, `death_*`, `team_*`, `hawk_*` | `[VERIFIED: read pvp/crates/nrpvp/src/console/pipe.rs]` `[PROJECT: CONSOLE §0; SMOKE-1 PASS]` |
| Observation | `dist\nrpvp.log` + `dist\logs\nrpvp_<UTC>.log` (line prefixes per feature), me3 logs in `%LOCALAPPDATA%\garyttierney\me3\data\logs\`, `nrscan` (outside reader, Nucleus only) | `[PROJECT: STATUS §5]` |
| Who plays | **Leonardo plays every in-game step**: "Agents cannot play the game. Any acceptance step that needs the game running is a HUMAN step" | `[PROJECT: CLAUDE.md "Roles"]` |
| Solo-testable | Event-script features (nights, hawk, offline mode, bots, clock, death hold): solo PASS runs on 2026-10-22/23 | `[PROJECT: TESTING §3]` |
| Needs 2+ players | Rival hostility, downed rule (rival hit, revive block), teams, night elimination by count, death hold in a session | `[PROJECT: STATUS §1, TESTING §3 FS-0, T-040, T-041, M5-5, M5-9 N1']` |
| Built, not verified | `team_size = 3`; night 2 decided by count with two players; die-at-once kill in a session; T-041 over real lag; `early_redown`; whether teammates can hurt each other | `[PROJECT: STATUS §1, §3, §4]` |

### 1.2 How Nucleus is used

1. `scripts\deploy-nucleus.ps1 [-TeamSize N] [-Me3]` copies the dev DLL and a rewritten `nrpvp.toml` into the Nucleus handler. It
   writes `dlllist.txt` with no trailing newline, and forces `mm_queue = false` and `br_menu = false`. `[VERIFIED: read script]`
2. Leonardo starts 2 or 3 instances in the Nucleus GUI. The handler uses `coldloader.dll` for Steam emulation, `SymlinkGame = true`,
   `MaxPlayers = 6` and XInput hooks. `[VERIFIED: read handler .js]`
3. The instances meet through SC's Invite / Join and start an expedition. Leonardo plays both windows, switching focus. Keys reach a
   window only when it has focus. `[PROJECT: STATUS §6]`
4. Agents drive both windows with `nrcon --all`. Logs land in `dist\nucleus-logs\Instance<N>\`. `[PROJECT: STATUS §6, CONSOLE §0]`

### 1.3 What costs the most time

| Blocker | Why it hurts | Evidence |
|---|---|---|
| A human must play every step | Every card waits for Leonardo, and each run is 5–40 minutes of his time | `[PROJECT: CLAUDE.md; REG-1 ≈ 40 min]` |
| Two-player rules need Nucleus or a friend | Leonardo controls 2–3 windows at once, a friend comes only to scheduled sessions | `[PROJECT: CLAUDE.md "Testing constraint"]` |
| **Nucleus does not load the game-data package** | Event-script features cannot be tested in two windows. `-Me3` fails on window 2 ("ColdLoader appid not found", cause UNKNOWN), so the DLL falls back to flag writes | `[PROJECT: STATUS §6, TESTING M5-9 N0]` |
| No downed state solo | Solo, the player dies at 0 HP, so the near-death rules cannot be tested solo or against bots (OFF-8 blocks OFF-7) | `[PROJECT: TASKS OFF-8]` |
| Crash on quit (0xC0000374) | Every teardown may leave a crash record, so automation must tolerate it | `[PROJECT: STATUS §1 "Known bug"]` |
| Leftover processes, symlink pollution | Leftover `me3.exe` processes lock instance folders. `SymlinkGame` makes Nucleus runs write into the real game folder (see §1.4) | `[PROJECT: TESTING M5-9 N0]` `[VERIFIED: today]` |

### 1.4 Observed on this PC today (for Leonardo; not changed)

- **The real game folder's `NightreignPvP\` is not the release candidate.** It holds:
  - `me3.exe` **0.13.0** (the pin is 0.11.0) `[VERIFIED: --version]`
  - `me3-args.txt` with `--skip-steam-init true`, plus `env.txt` and `steam_appid.txt` (Nucleus `-Me3` leftovers)
  - an `nrpvp.dll` dated 2026-10-23 14:38
  - a `launcher.log` written by a Nucleus `Instance1` launch

  Its `VERSION.txt` still names the 2026-10-22 RC. Likely cause: the handler's `SymlinkGame = true` makes instance folders point into
  the game folder `[HYPOTHESIS]`. Double-clicking `nrpvp_launcher.exe` there right now would pass `--skip-steam-init` to me3 0.13.0.
- **The Nucleus handler is still in `-Me3` mode**: `LauncherExe = "nrpvp_launcher.exe"` and `dlllist.txt` has no `nrpvp.dll`.
  STATUS §2 says the plain deploy is active. `[VERIFIED: read files]`
- **`me3.exe` (pid 27348) and `me3-launcher.exe` (pid 14348)** from `D:\Programs\Nucleus\content\…\Instance1\` were running with no
  game. Steam was running. No EasyAntiCheat or `start_protected_game.exe` process was running. `[VERIFIED: Get-Process, 2026-10-23]`

These are exactly the conditions the orchestrator's preflight (§4.3) must catch.

---

## 2. Feature-to-test workflow

The orchestrator does not write mod code. Claude does that in `nightreign-pvp`, following that repo's rules: task cards, one owner doc
per topic, and evidence tags. The orchestrator provides the steps in **bold**:

1. **Understand:** Claude reads `pvp/.claude/STATUS.md` and the owning doc, then writes or updates a task card.
2. **Scenario:** Claude writes a scenario file in this repo (§6.3): setup, actions, expected observations, and which kind of
   participant each role needs (§3).
3. **Capability check:** `scenario_check` says whether every role and step can be done with what exists (bots, ghosts, puppets,
   Nucleus, human). Anything that cannot be done becomes a `HUMAN` step, or the scenario stops before launch with the reason.
4. Claude makes the code change in `nightreign-pvp` and runs `cargo test --workspace`, fmt and clippy, as that repo requires.
5. **Build and stage:** run `scripts\build.ps1`, plus `build-gamedata.ps1` when `gamedata\*.json` changed. Copy `dist\nrpvp.dll`,
   `.pdb`, `nrpvp.toml` and `gamedata\` into the run folder, so the next build is never blocked by a running game.
6. **Preflight, then launch** (§4). Refuse with reasons if any check fails.
7. **Reach the scenario:** title → Roundtable → table → Commence. Then use console commands (`mode offline`, `bots_hostile 1`,
   `clock night`, `god 1`, …) and pad input.
8. **Act and observe:** pad input, console commands, log watches, screenshots, and the mod's `state` dump.
9. **Judge:** check the scenario's expectations (log patterns, console replies, state values, optional screenshot checks) and give
   `PASS` / `FAIL` / `INCONCLUSIVE`. Each problem is classed in the mod's own vocabulary: `MOD-DEFECT`, `GAME-BEHAVIOUR`,
   `SC-LIMITATION`, `UNVERIFIED` (`pvp/.claude/TESTING.md` header).
10. **Evidence bundle:** a run folder with the pins (exe, SC, me3, nrpvp.dll hashes, commit), the preflight report, the console
    transcript, mod and me3 logs, screenshots and the verdict. Claude turns it into a TESTING §3 entry with the mod's `test-report`
    skill.
11. **Teardown:** quit through the menu or kill the process, tolerating the known crash on quit. Clean up leftover me3 processes that
    this run started. Check that the real `.sl2` saves are untouched.
12. Claude fixes the code and runs the same scenario again (`scenario_rerun <run>`).

**Example from the brief:** "a player's loot is left on the ground when another player kills them".

- Steps 1–2: the research is in `pvp/.claude/research/S2-inventory-drops.md`. The scenario needs a **killer** and a **victim**.
- Step 3, with today's capabilities:
  - **"killed by a bot"** can be fully automated: LOCAL is the victim, a hostile bot is the killer. That needs OFF-8 (a downed/death
    path in solo) and the drop code.
  - **"LOCAL kills another player and sees their loot"** needs a second real player: Nucleus or a friend today, a puppet only if E3
    is a GO.

  The capability check must say this plainly instead of passing a bot run off as the player case.

---

## 3. Fake participants in one game

### 3.1 What "player" means to the mod

These code paths decide whether a participant counts as a player:

- **The player list** (`WorldChrMan+0x9A40`): slot 0 is LOCAL, the others are in join order. The mod's `World.fighters` is filled
  from it (`pvp/crates/nrpvp/src/game/world.rs`).
- **Per-player memory**: the team value `ChrIns+0x74`, the id `ChrIns+0x78` (`0x600N…`), HP, the downed block `PlayerIns+0x710`,
  and the revive bar (MEMORY_MAP §2–§6).
- **Hit attribution**: the downed rule checks that the attacker's handle belongs to a rival (`combat_state/rules.rs`, TASKS OFF-7).
- **Game events that count players**: `EcTestSortieMemberNum`, `EcTestPlayerNo`, the wipe check, and the night flags 7502/7503
  (S10 §1a, STATUS §6).
- **Network**: each other player's game runs its own copy of the rules. The host runs the world (host-authoritative, S10 §0). The
  mod's design (each game marks only rivals) is per-client.

### 3.2 Approaches

| # | Approach | Exists? | Exercises | Cannot validate | Replaces which Nucleus workflow | Evidence / experiment |
|---|---|---|---|---|---|---|
| A | **Pure simulation**: the mod's unit tests with fake `GameMem` / fighters | Yes, 484 tests | Rule decisions (downed rule, teams, night match) | Any game memory, engine reaction, timing, network | None (it already sits before Nucleus) | `[PROJECT: STATUS §7]` |
| B | **Ghost roster**: a dev-only console command adds N synthetic fighters to `World.fighters` (id, team, "standing / downed / eliminated"), with LOCAL real | No (mod card) | Feature logic in the live loop against real LOCAL and real flags: night-end-by-count, team assignment from ids, win/lose banners, elimination bookkeeping | Anything the ghost would do in the world: hits, revives, bodies, the game's own player counting, other screens | Only the "count / decide" half of M5-5 (night 2 by count, the 5-minute check in STATUS §3) | Small. Acceptance: night 2 ends by count with 1 ghost "eliminated" `[HYPOTHESIS: World is the single read path for night_match, true per world.rs comment]` |
| C | **NPC bots** (`CSNpcBotMan`, event `2003[97]`) as hostile participants | **Yes**: 5 rivals kept, team 29, full Nightfarer AI, solo only | Real combat between LOCAL and a Nightfarer-shaped NPC: damage, hostility writes on non-player ChrIns, death or knock of LOCAL by another character, drops on LOCAL's death, clock and night flow with "opponents" | Player-list entries, player ids, other players' downed block and revive bar, SC party, game player counts, other clients' view. Bots are not in the player list (they are in character set 14). AFK: as shipped they follow and fight; making them stand still is UNKNOWN (NpcThink swap or our own logic script, OFF-6 notes) | Tests where LOCAL is the victim and the opponent's player-ness does not matter, **once OFF-8 + OFF-7 land** | `[PROJECT: OFF-3 memo; TESTING OFF-4/OFF-5; MEMORY_MAP §15]`. Experiment **E4**: bots AFK |
| D | **Puppet players**: extra `PlayerIns` entries in LOCAL's player list, created by the game's own remote-player path with a null/debug session instead of a peer | **No; UNKNOWN** | Everything that reads the player list and per-player memory **in LOCAL's game**: rival marking, LOCAL hitting a downed rival (bar block, bleed-out −3 s), night elimination by count with real bodies, death hold. Puppets stay AFK by default (no input source) | Network sync and the other client's own rules (in Nucleus each window runs the mod). The host/joiner role split. SC's own per-slot fixes | Most of FS-0-style "one rival standing there" runs, **if** a puppet takes hits and has a working downed block | Leads: `PlayerNullSession`, `PlayerDebugSession`, `CSDebugBankPlayerMng` in the exe `[VERIFIED: strings today]`; ~10 pre-made player-class objects (team 30) at `WorldChrMan+0x9C20..+0xA170` `[PROJECT: MEMORY_MAP §2]`; SC rebuilds the player set itself `[PROJECT: S10 §1a]`. Experiment **E3** |
| E | **Simulated network clients**: a headless peer over Steam P2P speaking the game's and SC's protocol | No | In principle everything, network included | — | — | **No-go.** SC's Steam and P2P logic is Themida-virtualised and cannot be read statically (S10 §1); it would be a second game client written from scratch; it conflicts with hard rule 3's spirit and needs a second Steam identity |
| F | **Multiple local instances** (Nucleus) | Yes, 2–3 windows | Real clients over emulated Steam: the network path, each client's rules, both screens | Event-script features (package not loaded), real latency | Itself | `[PROJECT: FS-0 runs; STATUS §6]`. Not the target. The orchestrator can still automate its edges (deploy, `nrcon --all`, log collection) as a fallback adapter |
| G | **Real humans** (friend sessions) | Yes | Real network, real people | — | Final acceptance; stays | `[PROJECT: FS-UI2]` |

**What AFK means here:** a participant that is present in the role the test needs (rival or teammate), does not move or act unless
the scenario tells it to, and can be hit, downed and eliminated.

- B is AFK by construction: it has no body.
- C is not AFK as shipped.
- D would be AFK by default.
- F is AFK when nobody touches the window.

### 3.3 Verdict on "one instance, several AFK fake players"

- **Today:** not possible as real players. C gives up to 5 non-player opponents, and A/B cover decision logic.
- **Limiting factor:** a player-list entry is created by the game's join path, which is driven by a network session (SC proxies the
  game's own session code, S10 §2). Nobody has yet shown that the path can run without a peer.
- **Smallest experiment: E3** (§7). It ends in GO or NO-GO for approach D.

---

## 4. Launcher integration

### 4.1 How `nrpvp_launcher.exe` works `[VERIFIED: read main.rs]`

1. It finds its own folder, which must be the game folder.
2. It checks that `nightreign.exe`, `SeamlessCoop\nrsc.dll`, `NightreignPvP\nrpvp.dll`, `NightreignPvP\me3\me3.exe`, the event file
   and the hawk route file are present.
3. It writes `NightreignPvP\nrpvp.me3.toml`: SC `load_early = true` with no initializer, `nrpvp.dll` `load_after` SC, and the
   `nrpvp-gamedata` package. The shape was verified in T-004.
4. It runs `me3 launch -g nightreign -e <Game>\nightreign.exe -p <profile>`, **plus every word of `me3-args.txt`**, with no window
   and stdout/stderr going to `launcher.log`.
5. It waits 8 s for an early me3 failure and shows a message box. It has no command-line options, no exit code to read and no ready
   signal.

### 4.2 Recommendation

| Use | Path | Why |
|---|---|---|
| Developer scenarios (almost everything) | **Orchestrator → `me3 launch -g nightreign -e <exe> -p <run>\profile.me3.toml`**, with the profile in the launcher's shape, pointing at the **staged** copy of the dev DLL and package | The same loader, load order and package as both documented paths (`launch.ps1`, the launcher). The orchestrator controls every argument (no `me3-args.txt`). The build is never blocked by a running game (STATUS §5). me3's attach-config log line confirms `start_online: false` after launch (seen in today's `launcher.log`) |
| Player-zip regression (REG-1 part A) | `nrpvp_launcher.exe` in the game folder, **after** preflight checks of that folder (no `me3-args.txt`, no `env.txt`, bundled me3 version = pin, `VERSION.txt` = the zip being tested) | That is what players run |
| Never | `start_protected_game.exe`, Steam's Play button, `steam://run`, Nucleus' launcher outside the Nucleus adapter | `[PROJECT: CLAUDE.md hard rule 2, README]` |

- **Possible later change in the mod** (a mod card, not invented behaviour): a launcher option that prints its checks and the
  profile it would write, so the orchestrator could reuse its checks directly.
- **Which me3 to use** is decided at review (Q4):
  - `Z:\Tools\me3\bin\me3.exe` 0.11.0 is the pin used by `launch.ps1`.
  - The game folder's bundled me3 is now 0.13.0.

### 4.3 Offline boundary and preflight

**What "offline" can mean here** (the brief says "without connecting to official matchmaking or game services"):

- `[PROJECT/SOURCE]`
  - me3 blocks the official matchmaking servers by default. `start_online` / `--online` turn that off.
    [me3 configuration reference](https://github.com/garyttierney/me3/blob/main/docs/configuration-reference.md),
    `me3 launch --help` ("Re-enable online matchmaking? (ban risk)").
  - SC "does not connect to the matchmaking servers" (S10 §2 step 6).
  - EAC starts only through `start_protected_game.exe`, and both documented paths start `nightreign.exe` directly (README).
- **However, a dev launch still uses the network:**
  - Steam: SC creates a Steam lobby once the Roundtable loads (TECHNICAL_NOTES §3, MM-1) and uses Steam P2P.
  - GitHub: SC fetches `VERSION` and data files from `yuiamoroll/NightreignSeamlessCoopRelease` (TECHNICAL_NOTES §3).

  So the honest guarantee is **"no FromSoftware/Bandai services and no EAC"**, not "no network". Whether that is acceptable is Q1.

**Preflight checks.** Every check must pass. A check that cannot be made counts as failed, and a failure is a refusal that names the
check and what to fix.

| # | Check | Source of truth |
|---|---|---|
| P1 | Launch route is one of the two in §4.2; the command line is built by the orchestrator, never from a file | brief, hard rule 2 |
| P2 | Generated profile has no `start_online`, no `savefile` with SC, and natives/packages only from the staged run folder or `SeamlessCoop\nrsc.dll` | `pvp/profiles/*.toml` comments |
| P3 | me3 args contain no `--online`; for the player route, `NightreignPvP\me3-args.txt` and `env.txt` are absent | launcher main.rs |
| P4 | Pins: `nightreign.exe` SHA-256, `nrsc.dll` SHA-256, me3 version, regulation hash match the pins table (`pvp/.claude/TECHNICAL_NOTES.md` §1) | TECHNICAL_NOTES §1 |
| P5 | No running `nightreign.exe`, `start_protected_game.exe`, `EasyAntiCheat*`, or leftover `me3*.exe` (leftovers: refuse and name the pids; never kill processes this run did not start without asking) | STATUS §6, today's observation |
| P6 | Steam is running (the game needs it; `--skip-steam-init` is Nucleus-only) | launcher message, deploy-nucleus.ps1 |
| P7 | Staged `nrpvp.toml` has `dev_mode = true` for the console (dev route) and no keys that start the public queue unless the scenario asks (`mm_queue`, `br_menu`) | dist/nrpvp.toml, CONSOLE §0 |
| P8 | Saves: real `NR0000.sl2*` hashed before and after and never opened; `.co2` backed up into the run folder before a save-touching scenario | hard rule 2, STATUS §6 |
| **After launch** | | |
| L1 | me3 log attach config says `start_online: false` and lists exactly the expected natives and packages; otherwise kill the game | `launcher.log` format seen today |
| L2 | Watchdog every 2 s: no EAC process or module; the game's remote TCP endpoints are only Steam/loopback ranges `[HYPOTHESIS: which ranges SC needs; measure in E1]`; if a violation is seen, kill the game and record it | — |

**Gaps that must be closed before launch automation is called safe:**

- L2's allowed-endpoint list is not known. E1 measures it read-only.
- No document lists the FromSoftware/Bandai hostnames. Without that list, L2 can only flag endpoints and cannot block them.
- Nothing can promise no ban, and the tool must say so.

---

## 5. Architecture and stack

### 5.1 Options

| Option | For | Against |
|---|---|---|
| 1. Standalone orchestrator, mod untouched | Clean separation | It cannot press buttons without focus, cannot dump state, cannot make ghosts or puppets. Screen-scraping only |
| 2. Extend the mod's tooling (`nrcon`, scripts) | Shortest path | Breaks the brief ("do not put orchestrator code in nightreign-pvp"). Mixes test harness and product |
| **3. Combined (recommended)** | Orchestration, scenarios, evidence, safety and MCP live here. The mod gains a few **dev-feature-only** test hooks through its own cards and rules (the `dev` cargo feature is already the place for dev-only code, ARCHITECTURE §7) | Two repos to keep in step, handled by a versioned contract (§5.3) |

### 5.2 Repository structure (Rust workspace, MSVC; matches the mod's toolchain, TECHNICAL_NOTES §1)

```
nightreign-orchestrator/
  crates/
    nro-core/      pure: scenario model + runner state machine, preflight policy, verdict rules, evidence manifest, pins (no Win32)
    nro-win/       Win32 adapters: processes and hashes, me3 launch, nrcon pipe client, log tail, window capture, watchdog
    nro/           CLI: env, preflight, launch, status, cmd, pad, shot, run <scenario>, stop, evidence
    nro-mcp/       MCP server (stdio), a thin mapping of the same calls; no logic. `rmcp` crate [SOURCE: github.com/modelcontextprotocol/rust-sdk] (must be downloaded)
  scenarios/       *.toml scenario files (versioned with the mod contract they need)
  pins/            nightreign.toml (exe, SC, me3, regulation hashes)
  docs/            PLAN.md, CONTRACT.md (what we use from nightreign-pvp), STATUS.md
  runs/            (gitignored) one folder per run: staged mod, profile, logs, shots, report.json
```

### 5.3 Integration boundary with `nightreign-pvp` (`docs/CONTRACT.md`)

**Reads/runs:**

- `scripts\build.ps1`, `scripts\build-gamedata.ps1`, and `scripts\package-release.ps1` (for zip scenarios)
- `dist\` outputs: `nrpvp.dll`, `.pdb`, `nrpvp.toml`, `gamedata\`, `nrcon.exe`
- `pvp/.claude/TECHNICAL_NOTES.md` §1 pins, which are copied into `pins/` on change
- the console pipe protocol: `\\.\pipe\nrpvp-<pid>`, one line in, reply lines + `\u{4}`, and `whoami`
- log line prefixes

**Mod-side test hooks to propose as mod cards (dev build only, each with console access per the mod's `add-console-command` skill):**

| Hook | Purpose | Size guess |
|---|---|---|
| `state` | One-line JSON of `World`: place, local HP/team/downed, fighters, bots, clock, night flags, menu/title indicator | S |
| `pad <buttons> <ms>` / `stick` | Inject a virtual pad state into the game's own input read (XInput), so input works without focus. The handler already sets `XInputEnabled = true`, so the game reads XInput `[HYPOTHESIS: hook point]` | M (RE for the hook point) |
| `ghost add/rm/set` | Approach B | S–M |
| OFF-8 / OFF-7 (existing cards) | Downed state solo; bots count as rivals | existing |
| `kill`, `down` (CON-3, planned) | Put a participant into a state fast | existing plan |
| `puppet …` | Approach D, only after an E3 GO | L |
| `capabilities` | Lists hooks and the contract version, so the orchestrator refuses a scenario the build cannot run | S |

---

## 6. Claude interface

### 6.1 MCP tools (every tool is also a CLI subcommand that returns the same JSON)

| Group | Tools |
|---|---|
| Discover | `capabilities` (orchestrator + mod contract version + participant kinds available), `env` (pins, paths, processes, what preflight would say) |
| Session | `preflight`, `build {gamedata?}`, `launch {route: dev\|player_zip, scenario?}`, `wait_for {condition, timeout}`, `stop {mode: menu\|kill}`, `session_status` |
| Inspect | `state` (mod `state` JSON), `players`, `console {line}` (allow-list of commands; free form only with `dev_unsafe`), `log_tail {n, prefix?}`, `log_wait {regex, timeout}`, `screenshot {region?}` (PNG path) |
| Control | `pad {sequence}`, `navigate {to: roundtable\|table_menu\|expedition}` (scripted pad paths checked by `state` + screenshot), `participants {add: bot\|ghost\|puppet, n, team, afk}`, `clock`, `flag`, `god` (thin wrappers of console commands) |
| Scenarios | `scenario_list`, `scenario_check {file}` (capability fit, human steps), `scenario_run {file, params}`, `scenario_rerun {run}` |
| Evidence | `evidence {run}` (manifest), `report {run}` (verdict, failures with log lines and shots, pins), `compare {run_a, run_b}` |

`console` can never send a command that is not in the mod's `help` list. Launch-type operations always go through `preflight`; there
is no "force" parameter.

### 6.2 CLI

`nro preflight`, `nro launch --route dev`, `nro run scenarios\bots-kill-local.toml`, `nro cmd "clock night"`,
`nro pad "A 100ms; wait 2s; DOWN"`, `nro shot`, `nro stop`, `nro report runs\<id>`.

### 6.3 Scenario file sketch

```toml
name = "bots-kill-local"
needs = { contract = ">=1", participants = [{ kind = "bot", n = 1, team = "rival" }], hooks = ["state", "pad"] }
[setup]   console = ["mode offline", "bots_hostile 1"]
[[steps]] navigate = "expedition"
[[steps]] wait_for = { state = "place == Expedition && bots.kept >= 1", timeout = "180s" }
[[steps]] console = "god 0"
[[expect]] log = "rule: .* downed LOCAL"   # example only
[teardown] stop = "menu"
```

---

## 7. Milestones

### M0: experiments (each ends in a short memo with numbers; about one week in all)

| # | Question | Method | Pass measure | Human |
|---|---|---|---|---|
| E1 | Can the orchestrator launch safely and know when the game is ready? | `nro preflight` + `nro launch --route dev` from a staged run folder; read me3's attach-config line; poll the pipe (`whoami`, `status`); watchdog logs every remote endpoint (read-only) for one Roundtable visit and one expedition; teardown | 5 of 5 runs: preflight report, L1 confirmed, pipe ready in < 90 s, endpoint list recorded, real `.sl2` hashes unchanged; refusal tests (fake `me3-args.txt` with `--online`, running leftover me3, wrong exe pin) all refuse | Steam signed in; watch the first run |
| E2 | Can input reach the game without a human? | (a) `SendInput` with the window focused, (b) the mod `pad` hook (mod card); script title → Roundtable → table → Commence (offline mode) | (b) reaches the expedition 5 of 5 with the window unfocused; (a) recorded as a fallback | none after the card |
| E3 | **Can one game hold a puppet player?** | One last 2-window Nucleus run with a hardware write watch on player-list slot 1 in the host (the mod's `hwbp` slots) to catch the function that fills it and its session argument; then static reading of that function and of `PlayerNullSession` / `PlayerDebugSession` users; then one solo try (dev console) to call it with a null/debug session | **GO** if, solo, a second player-list entry appears with a model, HP, team, a working downed block, takes LOCAL's hits and stays 120 s with no crash; otherwise **NO-GO** with the blocking reason | Leonardo starts Nucleus once; the solo try is watched |
| E4 | Can bots be AFK participants? | Bot NpcThink swap (a param row with an idle logic, or our plain-text logic, TECHNICAL_NOTES §4b) | 5 bots stand still for 120 s, take hits, die | none |

### M1: first end-to-end workflow (no human in the loop, solo)

- **Scope:** E1 + E2 implemented, plus the `state` hook. Scenario `bots-kill-local` and one regression scenario (`smoke-offline`:
  hawk cruise, drop, bots arrive, clock jump, night end) run from `nro run` and from MCP, with an evidence bundle and a verdict.
- **What it removes:** Leonardo from every solo test, and Nucleus from every test whose opponent can be an NPC once OFF-8/OFF-7 land.
- **Done when:** both scenarios pass 3 runs in a row unattended, and a seeded failure (a mod build with a known bug) gives `FAIL`
  with the log line and screenshot that show it.

### M2: depends on E3

- **E3 GO:** the `puppet` hook in the mod and `participants {kind: puppet}`. Ports FS-0 run 19 (the downed rule, one rival) and M5-5
  (night elimination by count) to one window. This is the milestone that **replaces Nucleus** for in-LOCAL-game player rules.
- **E3 NO-GO:** ghosts (B) for count logic, bots (C) for combat, and a **Nucleus adapter** (deploy, wait for both pipes,
  `nrcon --all`, collect `nucleus-logs`) for the rest, with the Nucleus GUI start as the one human step. The plan then says plainly
  that two-client rules still need two clients.

---

## 8. Acceptance criteria, risks, open questions

### 8.1 Acceptance criteria (for the M1 build)

- **Refusal:** with any P1–P8 check failing, `launch` does not start a process and returns the failed checks and their fixes.
  Covered by unit tests in `nro-core` for every check and one live refusal per check class.
- **Post-launch guard:** an L1 mismatch, or an EAC process seen by the watchdog, kills the game within 2 s and marks the run
  `ABORTED (safety)`.
- **Repeatability:** the same scenario on the same build gives the same verdict 3 of 3 times, or is classed `INCONCLUSIVE` with the
  reason.
- **Evidence:** every run folder has pins, preflight, console transcript, logs, shots, and `report.json`. A report never says
  `PASS` without at least one positive observation named in the scenario.
- **Honesty:** `scenario_check` refuses to map a "player" role onto a bot or ghost unless the scenario allows that substitution
  explicitly.
- **Boundaries:** no file outside `runs\` and the staged folders is written. The real saves' hashes are unchanged.

### 8.2 Risks

- **Game or SC update:** pins fail and preflight refuses, which is correct behaviour but stops all tests until the mod is re-pinned.
- **Input hook RE (E2b) takes longer than planned:** fallback to `SendInput` with focus. The pad hook stays dev-only.
- **The puppet path (E3) may crash or need SC internals:** stop at NO-GO. Never patch SC (hard rule 3).
- **The crash on quit** pollutes Windows crash records; teardown by kill is accepted.
- **Steam/GitHub traffic** remains, so "offline" must be defined (Q1).
- **Usage limits mid-run** (STATUS §6): runs must be resumable from their folder.

### 8.3 Human-required steps

- Steam running and signed in.
- Approving the mod cards for the test hooks.
- The single Nucleus start for E3.
- Friend sessions for real-network checks.
- Cleaning the game-folder drift in §1.4, or approving me to do it.

### 8.4 Open questions for review

1. **Offline definition:** is "no FromSoftware/Bandai services and no EAC, Steam lobby/P2P and SC's GitHub fetch allowed" the
   policy? If not, the dev route must use `-Profile dev` (no SC), and most features would not load.
2. **Mod test hooks:** may Claude add the dev-only hooks in §5.3 to `nightreign-pvp` as normal task cards (`state`, `pad`, `ghost`,
   `capabilities`)?
3. **Save:** use the current SC `.co2` save with a backup per run, or a dedicated test character?
4. **me3 for dev runs:** keep `Z:\Tools\me3` 0.11.0 (the pin), or move the pin to 0.13.0?
5. **E3 priority:** run the puppet experiment in M0 as proposed, or after M1?

---

## 9. Repository

Name **`nightreign-orchestrator`**, path **`C:\Users\Leonardo\source\repos\nightreign-orchestrator`**. The folder already exists. It
is a git repo whose only commit holds plan v2; v2's files are deleted in the working tree and this plan replaces them. Nothing is
built until the plan and M0/M1 are approved.

## Sources (external)

- me3 configuration reference (`start_online`, `savefile`): https://github.com/garyttierney/me3/blob/main/docs/configuration-reference.md
- `me3 launch --help`, me3 0.11.0 and 0.13.0 binaries on this PC (`--online`, `--skip-steam-init`).
- Running the game without EAC by starting the main exe, offline only (community guide, not authoritative):
  https://rutab.net/b/games/2025/06/03/elden-ring-nightreign-kak-otklyuchit-eac-dlya-odinochnogo-rezhima.html
- Nucleus Co-op overview: https://github.com/nucleuscoop/nucleuscoop/blob/master/README.md
- Rust MCP SDK (`rmcp`): https://github.com/modelcontextprotocol/rust-sdk
- No public material found on fake players or puppet players for Nightreign; searches for prior art returned nothing relevant.
