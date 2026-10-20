# Status and handoff (written 2026-10-12, matchmaking updated 2026-10-19)

Read this first when picking the project up again. It says what works, what is built but untested, what to test next, and the
traps we already fell into. Details live in the other docs; this file points to them.

| Doc | Use it for |
|---|---|
| `re/MEMORY_MAP.md` | Every memory location found, with behaviour and proof |
| `re/PROJECT_SCOPE.md` | Canonical product intent and scope boundaries |
| `PROJECT_PLAN.md` | Detailed feature feasibility/milestone plan; §11 records downed-player design decisions |
| `MATCHMAKING.md` | How the Steam matchmaking works, its settings, and how to read its logs |
| `TESTING.md` §3 | The result of every test session, newest last |
| `TASKS.md` | Task cards and their state |
| `TECHNICAL_NOTES.md` | Stack, Seamless Co-op facts, verified engine facts |
| `FUTURE_CONSIDERATIONS.md` | Ideas parked for later (menu entry, queues, loot rules) |

---

## 1. What the mod does today

| Feature | State |
|---|---|
| PvP between players, PvE alongside, no networking changes | **Works.** Proven in Nucleus and with a friend over real Steam |
| Automatic: each player's game marks rivals hostile, no keys | **Works** (Nucleus, 2 players). Teammates and yourself stay on the normal value, so sites of grace and pickups work |
| Teams from player IDs (`team_size` 1, 2 or 3) | **Works** for `team_size` 1 and 2 (Nucleus run 21: 3 players, 2 v 1, all three games agreed). `team_size = 3` not run |
| Teammate revive | **Works** (Nucleus run 21: 3 of 3) |
| One-click install (`dist\NightreignPvP.zip`: launcher + mod + bundled loader) | **Works** on Leonardo's PC. Not yet tried on a second PC |
| Player build: PvP always on, no keys, small "BATTLE ROYALE" label | **Works** (solo check) |
| **Solo matchmaking over real Steam (queue → match found → same Roundtable → expedition as rivals → back to idle)** | **Works with one host click** (FS-MM3, 2026-10-19, 2 PCs): the host presses Invite Members when asked; everyone else does nothing. Player build has it (MM-4: Insert panel, Solo Battle Royale; 2-player matches until release, 6 in the final game); 3–6 players only in automated tests |
| A rival can't revive a rival (rule v1: bar filled by a rival → knocked down again) | **Works** (Nucleus) |
| **Downed rule (Leonardo's design): a rival's hit never keeps progress on the purple bar and takes 3 s off the bleed-out; a teammate's hit fills the bar as normal** | **Works in free-for-all (Nucleus runs 15–20, accepted by Leonardo 2026-10-12).** Right on both screens, with a brief flicker per hit. **Works with teams of 2 (Nucleus run 21, 3 instances, 2026-10-12):** teammates revived, rivals finished, both in the same down. Not handled: the attacker's gauge when the downed rival has a teammate |

Most later Battle Royale mechanics are not started: separate spawns, keeping level on death, inventory drop, no-boss nights, final arena,
sudden death, winner's choice, menu entry, queues. See `PROJECT_PLAN.md` §2 and §9.

## 2. What is deployed where (all the same build, 2026-10-12 02:31 local)

| Place | Mode | Settings |
|---|---|---|
| Nucleus handler `D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN` | developer (panel does not draw under Nucleus; logs to `dist\nucleus-logs\Instance<N>\`) | `team_size = 2` (set for run 21), PvP on, probes off |
| `dist\NightreignPvP.zip` | player (no keys, label only) | `team_size = 1` |
| `dist\nrpvp.dll` + `dist\nrpvp.toml` (used by `scripts\launch.ps1`) | developer, full panel | `team_size = 1`, probes on |

`dist\friend-kit.zip` is the **old** kit from 2026-10-10. Do not send it; send `NightreignPvP.zip`.

## 3. Next human tasks, in order

### Where the in-game UI stands (2026-10-20, dev build; details in TASKS.md UI-1 / UI-2)
- **UI-1 (Leonardo's flow) works solo:** table → boss menu → Matchmaking Settings → Number of Players shows **Solo / Duo / Trio** →
  **Commence** starts our Solo queue and the game's own box says so → the button reads **Stop Searching** while searching → pressing it
  stops. Duo/Trio say "coming soon" (MM-6). Once a match is ready Commence runs normally (host starts; one player is switched to two first:
  [HYPOTHESIS], check in the friend session). Mechanism: hardware breakpoints (shared owner `src/hwbp.rs`: slots 0/1 quiet invite,
  2 Commence, 3 labels), no code patched.
- **UI-2 works solo:** matchmaking status as the game's blinking messages ("searching (N of M)", "stopped", "match found", host "open
  Invite Members", "everyone is here"...): our events appended to the Roundtable event script + flags 10007200+k set by the DLL.
- **Game-data package** `dist\gamedata` (texts in all 15 languages + the Roundtable event script), built from `gamedata\*.json` by
  `scripts\build-gamedata.ps1` (needs the unpack and Python). One launch profile: `.\scripts\launch.ps1` loads it.
- **Not in the player zip yet:** `br_menu` is forced off in the player build and the zip/launcher do not carry `dist\gamedata`.
- **Next building work, in order:** (1) player zip with `br_menu` on + the package (launcher loads it) and a friend-session runbook
  (Solo search on both → match found → host Invite Members → everyone here → host Commence; watch the messages; FS-UI1 folds in);
  (2) MM-3g (no Invite Members click); (3) MM-6 Duo/Trio. Later polish: live count inside the searching box / the game's real
  matchmaking screen (UI-1 C3).
- Known: the game crashes on quit (0xC0000374) on every run since 2026-10-10; pre-existing, separate issue.
UI-0 is DONE (2026-10-19, `research/UI-0-ingame-ui.md` §10). Modding tools live in `Z:\Tools\`,
the 1.03.3 unpack in `Z:\Tools\nr-unpacked\1.03.3\`. Launch: `.\scripts\launch.ps1` (default profile = SC + DLL + our game-data package `dist\gamedata`, built by `scripts\build-gamedata.ps1`; the UI-0 test profiles were removed). Two-player check of our game
files with SC: FS-UI1 (TESTING.md §6), add it to the next friend session.

### Task E (next): FS-MM4, two PCs on the player zip
Zip: `dist\NightreignPvP.zip` (MM-4 with the pre-release 2-player flag `package-release.ps1 -MatchPlayers 2`, nrpvp.dll 86A7396A604E94FD, also installed in Leonardo's game folder; build with `-MatchPlayers 6` for the final game). Both: Insert → Solo Battle
Royale. Host: Matchmaking Settings > Invite Members when the line asks. Expedition, back to the Roundtable. Also MM-4 solo step 5 (quit to
title while queued: queue left by itself). Logs: both `<Game>\NightreignPvP\logs`.

### Task A: done (downed rule, free-for-all). Results: `TESTING.md` §3, FS-0 runs 15–20

### Task B: play with a friend on the player zip
1. Send `dist\NightreignPvP.zip`. They follow `HOW TO INSTALL.txt` inside it (drag two things into the game folder, double-click the launcher).
2. Leonardo installs the same zip the same way (replace the old `NightreignPvP` folder and `nrpvp_launcher.exe` in the game folder).
3. Host "Up to 6 players", friend joins, nobody presses anything.
4. Check: both labels show "1 rival"; both can fight; graces and pickups work; the downed rule behaves as in Task A.
5. Afterwards each sends the `logs` folder from `<Game>\NightreignPvP\`.

### Task C: done (3 Nucleus instances, teams of two). Results: `TESTING.md` §3, FS-0 run 21
Still open from it: whether teammates can damage each other (not visible in the logs), and what the lone rival's screen showed on the
gauge of a downed enemy who has a teammate. Original card: the only way to see both halves of the downed rule at once: a teammate filling the bar while a rival shortens the clock.
Set `team_size = 2` (Nucleus: `scripts\deploy-nucleus.ps1 -TeamSize 2`; zip: `scripts\package-release.ps1 -TeamSize 2`).
Also the first test of: teammates can't hurt each other, teammate revive works, player IDs with 3 players.

### Task D: position snap-back test (needed for separate spawns)
Never run. Developer build in Nucleus: stand still, **F7** (save spot), walk ~20 m, **F6** (try to return), note the result, **F9** (next
candidate), repeat for 6 candidates. Tell Claude which one snapped the player back and what the other screen showed.

## 4. Next building work

1. Tune `rival_hit_seconds` (default 3.0) from play feel.
   Accepted for now (Leonardo, 2026-10-12): a single rival hit that removes the whole bar (first down, bar 40; runs 18 and 19) lets the
   player get up; rule v1 downs them again 3.7 s later. `Core::getup_step` would do it at once but is **off** (`early_redown = false`):
   never exercised in the game, effect UNKNOWN.
   Gauge flicker (up to 50 ms per rival hit, both screens): improvement ideas in `FUTURE_CONSIDERATIONS.md`, "Known side effects".
   Attacker-side gauge with teams of 2+: a copy cannot tell who hit, so `remote_step` only acts for a downed rival with no teammate.
   With teams, a rival's hit on a downed enemy will again stay on the attacker's gauge until this is solved. Run 21 confirmed the rule
   itself is right with teams; what that gauge looked like on the lone rival's screen was not reported.
   New in run 21: the combat step peaked at 63–225 ms around rival hits with three games running (cause UNKNOWN, rule still fired).
2. Separate team spawns: needs Task D, then "move each team to its own point after landing" (`research/S6-team-spawns.md`).
3. Death rules: keep level, starting items, respawn protection (`research/S5-death-respawn.md`; level and runes are not located yet).
4. Inventory drop on death (`research/S2-inventory-drops.md`).
5. No-boss nights, final arena, sudden death, winner's choice (`research/S3-match-flow.md`, `S4-mod-channel.md`).
6. "Battle Royale" menu entry: UI-1 (proven route, UI-0) and public queues. **Queue design decided 2026-10-12:**
   `research/S9-matchmaking-decision.md` (Steam queue lobby, no server of ours, Solo first). Cards MM-1 … MM-6 in `TASKS.md`.
   **State 2026-10-19:** Solo works end to end with one human step (FS-MM3). Path: queue lobby match → host presses Invite Members →
   the game makes the Seamless Co-op lobby → MM-3f skips the Steam invite window (hardware breakpoint, no patch) → host's mod invites, the
   joiner's mod opens the join link → joiner's game enters → ALL PRESENT once the host's world shows the players. A lobby created by our
   mod does **not** work (FS-MM2, S9 §5f). MM-4 brings it to the player build (solo steps 1–4 PASS). Next: FS-MM4 (Task E), then MM-3g
   (trigger the game's Invite Members from the queue so no one clicks; call chain in S9 §5d), then MM-6 Duo/Squad and MM-5 table entry.
   Findings: S9 §5c–5g.
7. Clean-up: the mod still carries a lot of investigation code (probes, dumps, the in-mod timer scans, snap-back test, experiment
   switches). It is all developer-mode only, but it should be trimmed once the features that needed it are done.

## 5. Tools and how to run them

| Tool | Command | Note |
|---|---|---|
| Build the mod | `powershell -NoProfile -File scripts\build.ps1` | Fails to copy if a game using `dist\nrpvp.dll` is open |
| Deploy to Nucleus | `powershell -ExecutionPolicy Bypass -File scripts\deploy-nucleus.ps1 [-TeamSize N]` | Close Nucleus instances first. Resets the handler's settings |
| Build the player zip | `powershell -ExecutionPolicy Bypass -File scripts\package-release.ps1 [-TeamSize N]` | Packages the current `dist\nrpvp.dll` |
| Developer launch (one PC) | `.\scripts\launch.ps1` | Full debug panel (Insert toggles) |
| Scanner | `dist\nrscan.exe list / peek / poke / clock / bar / explain` | Reads (and with `poke`, writes) a **Nucleus** game from outside. Refuses to write to any other install |

Watch a player's downed block live (replace the address with `PlayerIns+0x710` from the instance log line `local = 0x…`):
`dist\nrscan.exe peek --instance 0 0x<addr> --hex 36 --interval-ms 300`

## 6. Traps we already hit

- **Nucleus `dlllist.txt` must not end with a newline.** It makes Nucleus fail with "WaitForSingleObject failed 258". The deploy script writes it correctly; never edit it by hand.
- **Our overlay does not draw inside Nucleus.** The mod still runs. Under Nucleus, keys reach a window only when it has focus; press END to unlock input so one key press reaches both.
- **Nucleus deletes its instance folders on close.** Logs go to `dist\nucleus-logs\` for that reason.
- **Never run the scanner or any tool against the normal game.** Steam launch = vanilla with anti-cheat. Mods only through Nucleus, `nrpvp_launcher.exe`, or `scripts\launch.ps1`.
- **Never touch the vanilla saves** (`%APPDATA%\Nightreign\<id>\NR0000.sl2*`).
- **Player ids change between the lobby and the expedition** (run 21: joiners were `0x6003`/`0x6004` in the lobby, `0x6002`/`0x6003` in
  the expedition). Teams are only meaningful once the expedition has loaded; do not decide or show teams from lobby ids.
- **Addresses change every launch and every area load.** A value found by the scanner is only good for that run; the mod must reach it through the player object (see `re/MEMORY_MAP.md`). A scanner left watching a fixed address after a restart is looking at the wrong memory: stop it.
- **Double-check offsets before any write.** On 2026-10-12 a test wrote to `+0x728/+0x72C` instead of `+0x718/+0x71C` by mistake (no lasting effect).
- **Setting health to 0 does not kill** in a multiplayer session; it downs. Death is the bleed-out clock reaching 0.
- **Elden Ring layouts are hints only.** Several were wrong in Nightreign (list in `re/MEMORY_MAP.md` §9).
- **No `VirtualQuery`-guarded reads on the main loop.** They made each pass take 2–3 s in an expedition (run 15). Use `mem::read` or
  `RegionCache::direct()` there; `RegionCache::new()` is for the discovery and probe scans. The heartbeat line shows the pass time.
- **Keep `probes = false` for rule tests.** The probes still use the slow reads and cost about 0.4 s per pass. Until 2026-10-12 the
  Nucleus handler had `probes = true` (copied from `dist\nrpvp.toml`); `deploy-nucleus.ps1` now writes `probes = false` unless `-Probes $true`.
- **Only a lobby the game makes can be joined.** Our own `CreateLobby(Public, 6)` gets adopted by Seamless Co-op but joiners' games
  leave it at once, and a direct `JoinLobby` gives Steam membership without the game session (FS-MM2). Steam lobby membership is not
  presence: ALL PRESENT also needs the host's ChrSet to hold the players.
- **`mm_quiet_invite` and `mm_lobby_trace` both use all four debug registers**; config turns quiet invite off when the trace is on.
- **Sonnet agents can be cut off by usage limits mid-task.** Check `cargo build` and `git status` before trusting the tree; the task card says what was intended.

## 7. Repository state

- `main`: MM-3f committed as `0872bc9`; MM-4 merged on top (2026-10-19). Leonardo pushes.
- Branch `t-028-snapback` (Leonardo): T-028 snap-back/teleport runs and F3 team spawns, 3 commits not on `main` and not yet pushed.
- Tests: `cargo test` → 418 pass in `nrpvp` (1 ignored hudhook smoke).
- Pins: game 1.03.3 (exe 1.3.3.0), Seamless Co-op v1.1.3, bundled me3 0.11.0.
- Friend-session logs are gitignored (`*.log`); they live in `.claude/experiments/<session>/` on Leonardo's PC only.

## 8. Decisions made (so they are not re-argued)

- Stack: Rust DLL loaded through me3 next to Seamless Co-op; nothing in Seamless Co-op is modified.
- **The table offers only Solo / Duo / Trio Battle Royale** (Leonardo, 2026-10-19): no normal expedition while the mod is installed. The
  choice opens the stock boss menu; pressing start begins our matchmaking; the game's own "Cancel matchmaking" stops it (no Leave entry). UI-1.
- **No ImGui for players in the final product** (Leonardo, 2026-10-19): menus and status messages must be the game's own UI. The
  Insert panel is a stop-gap for testing.
- **Every match has 6 players** (Seamless Co-op's limit; Leonardo, 2026-10-19): Solo 6 × 1, Duo 3 × 2, Squad 2 × 3. Pre-release zips use 2 for testing.
- PvP is always on for rivals; players cannot switch it off. Launching with `nrpvp_launcher.exe` is the mode, until a menu entry exists.
- Hostility: each game keeps itself and teammates on the normal value and marks only rivals (value 29).
- Downed players: teammates fill the purple bar; rivals shorten the bleed-out and never move the bar. Instant death on 0 health was
  rejected (auto-revive items would defeat it).
- Investigation first, then building: confirm memory locations with the scanner before adding mod code.
- Matchmaking joins through the game's own lobby (Invite Members path), never a lobby our mod creates; never EAC, never Bandai servers.
- Leonardo only does smoke tests; Claude decides design and order, and does not build tools that are not needed for the goal.
