# Status and handoff (updated 2026-10-20)

Read this first when picking the project up again. **This is the only doc that says what is true now**: what works, what is built but
untested, what comes next, the traps we already fell into, and the decisions made. Other docs own their topic (doc map in `CLAUDE.md`);
this file points to them.

---

## 1. What the mod does today

| Feature | State |
|---|---|
| PvP between players, PvE alongside, no networking changes | **Works.** Proven in Nucleus and with a friend over real Steam |
| Automatic: each player's game marks rivals hostile, no keys | **Works** (Nucleus, 2 players). Teammates and yourself stay on the normal value, so sites of grace and pickups work |
| Teams from player IDs (`team_size` 1, 2 or 3) | **Works** for `team_size` 1 and 2 (Nucleus run 21: 3 players, 2 v 1, all three games agreed). `team_size = 3` not run |
| Teammate revive | **Works** (Nucleus run 21: 3 of 3) |
| One-click install (`dist\NightreignPvP.zip`: launcher + mod + bundled loader + game-data package) | **Works** on Leonardo's PC. Not yet tried on a second PC |
| Player build: PvP always on, no keys, **no overlay** (2026-10-20: the in-game menu and messages are the only UI) | Built; untested in game (FS-UI2) |
| **Solo matchmaking over real Steam (queue → match found → same Roundtable → expedition as rivals → back to idle)** | **Works with one host click** (FS-MM3, 2026-10-19, 2 PCs): the host presses Invite Members when asked; everyone else does nothing. Started from the table (UI-1) in the player zip since 2026-10-20; 2-player matches until release, 6 in the final game; 3–6 players only in automated tests |
| Battle Royale in the stock table menu (UI-1) and status as the game's own messages (UI-2) | **Works solo** (dev build); in the player zip, FS-UI2 next |
| A rival can't revive a rival (rule v1: bar filled by a rival → knocked down again) | **Works** (Nucleus) |
| **Downed rule (Leonardo's design): a rival's hit never keeps progress on the purple bar and takes 3 s off the bleed-out; a teammate's hit fills the bar as normal** | **Works in free-for-all (Nucleus runs 15–20, accepted by Leonardo 2026-10-12).** Right on both screens, with a brief flicker per hit. **Works with teams of 2 (Nucleus run 21, 3 instances, 2026-10-12):** teammates revived, rivals finished, both in the same down. Not handled: the attacker's gauge when the downed rival has a teammate |

Not started: separate spawns, keeping level on death, inventory drop, no-boss nights, final arena, sudden death, winner's choice.
See `PROJECT_PLAN.md` §2 and §9.

**Known bug:** the game crashes on quit (0xC0000374, heap corruption) on every run since 2026-10-10. Not proven to be Seamless Co-op's;
our thread that rewrites debug registers and the overlay's teardown were the suspects. The overlay is gone since R5 (2026-10-20): if
quits stop crashing on the new builds, it was the overlay; if not, the debug-register thread is next. Investigate before any public zip.

## 2. What is deployed where

| Place | Mode | Settings |
|---|---|---|
| Nucleus handler `D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN` (build of 2026-10-12 02:31; redeploy for SMOKE-2) | developer (logs to `dist\nucleus-logs\Instance<N>\`; after redeploy `dist\nrcon.exe --all` reaches every instance) | `team_size = 2` (set for run 21), PvP on |
| `dist\NightreignPvP.zip` (2026-10-20, nrpvp.dll 0881264E03245444, `-MatchPlayers 2`) | player (no keys, no overlay; in-game menu + messages) | `team_size = 1`, `matchmaking = true`, `br_menu` on. **Not** installed in Leonardo's game folder by the agent |
| `dist\nrpvp.dll` + `dist\nrpvp.toml` (used by `scripts\launch.ps1`) + `dist\nrcon.exe` | developer + console (no panel since R5). Branch `eng/cleanup-r5-r2-r3`: R5, R2-lite, R3-split; **SMOKE-2 pending** | `team_size = 1` |

`dist\friend-kit.zip` is the **old** kit from 2026-10-10. Do not send it; send `NightreignPvP.zip`.

## 3. Next, in order

**RESUME HERE (session paused 2026-10-21 at the usage limit; branch `eng/cleanup-r5-r2-r3`):**
1. Verify the last commit ("console reads fresh"): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --release -- -D warnings`,
   the same with `-p nrpvp --no-default-features`, `cargo test --workspace --release`. It is NOT built or tested yet.
2. When the game is closed: `scriptsuild.ps1` (puts the fix in `dist
rpvp.dll`).
3. Finish SMOKE-2 (below). **Part A step 2 PASSED** in the Roundtable on 2026-10-20 23:5x (TESTING §3 entry still to write):
   `list/status/players/team_set 6/team_restore/pos_next/log` all fine, readback 1 -> 6 -> 1, ticks 0 ms, menu Commence/Stop fine.
   Found: a chained `team_set 6; players` showed the old value (players list cached for 100 ms): that is the fix in step 1.
   Still to run: part A step 3 (solo expedition: team_set 6 / team_restore / pvp 0 / pvp 1), step 4 (crash on quit?), part B (Nucleus
   downed rule + Task D).
4. Then: TESTING §3 entry for SMOKE-2 (skill `test-report`), merge the branch into `main` (fast-forward), update the memory files
   (`dev-console-and-architecture.md`: R5/R2-lite/R3-split done).

1. **FS-UI2** (HUMAN + friend; runbook TESTING.md §6): both PCs on the new zip, match started from the table, checks the match-ready
   Commence `[HYPOTHESIS]` (one player is switched to two first) and our game files next to SC on two PCs.
2. **SMOKE-2: the cleanup branch `eng/cleanup-r5-r2-r3` (HUMAN, ~25 min; Claude drives the console).** Built: R5 (panel, hotkeys,
   probes, scans, discovery and dumps deleted, ~12 k lines), R2-lite (`game/layout.rs`), R3-split (`team/`, `combat_state/` by job).
   `dist\nrpvp.dll` and `dist\nrcon.exe` are this build. SMOKE-1 (console + R0/R1) passed on 2026-10-20 (TESTING §3).
   **Part A, solo (~10 min):**
   1. `.\scripts\launch.ps1`, load to the Roundtable. Table menu: Solo / Duo / Trio, Commence shows "searching", Stop Searching stops.
   2. Say "game is up": Claude runs `status`, `players`, `log 30`, `team_set 6` / `team_restore`, `pos_next`.
   3. Start a solo expedition. Claude runs `team_set 6` (enemies should ignore you), `team_restore` (back to normal), `pvp 0`, `pvp 1`.
   4. Quit. Note whether the game **crashed on quit** (the overlay is gone now: see the known bug in §1).
   **Part B, Nucleus 2 instances (~15 min): the downed rule after the split, and Task D.**
   1. Claude runs `scripts\deploy-nucleus.ps1 -TeamSize 1` (Nucleus closed). Start 2 instances, host + join, start an expedition.
   2. Say "both in": Claude runs `nrcon --all status` (both games, rivals marked).
   3. Down one player with the other (rival). Hit the downed one 2–3 times: the purple bar must not fill and the clock gets shorter, as in
      runs 15–20; then let them die. Claude checks the rule lines in both logs.
   4. Task D (T-028, needed for spawns), on instance 0: stand still, say "save" (Claude runs `pos_save`), walk ~20 m, say "go" (Claude
      runs `pos_go`). Tell Claude whether you snapped back and what the other screen showed. Claude runs `pos_next` and you repeat, up
      to 6 candidates.
   Rollback if anything is off: `git checkout main`, `scripts\build.ps1`. The FS-UI2 zip is untouched; the new player DLL gets its own
   check before the next zip.
3. **MM-3g** (no Invite Members click), then **MM-6** (Duo/Trio). Later polish: live count inside the searching box (UI-1 C3).
4. **Task D: position snap-back test** (T-028, needed for separate spawns): folded into SMOKE-2 part B (console `pos_save` / `pos_go` /
   `pos_next` instead of F7 / F6 / F9).

Open from FS-0 run 21: whether teammates can damage each other (not visible in the logs), and what the lone rival's screen showed on the
gauge of a downed enemy who has a teammate.

How the in-game UI works: UI-1 uses hardware breakpoints (shared owner `src/hwbp.rs`: slots 0/1 quiet invite, 2 Commence, 3 labels), no
code patched; UI-2 appends our events to the Roundtable event script and the DLL sets flags 10007200+k. The game-data package
`dist\gamedata` (texts in 15 languages + the event script) is built from `gamedata\*.json` by `scripts\build-gamedata.ps1` (needs the
1.03.3 unpack in `Z:\Tools\nr-unpacked\1.03.3\` and Python). Details: TASKS.md UI-1 / UI-2, `research/UI-0-ingame-ui.md`.

## 4. Next building work

1. Tune `rival_hit_seconds` (default 3.0) from play feel.
   Accepted for now (Leonardo, 2026-10-12): a single rival hit that removes the whole bar (first down, bar 40; runs 18 and 19) lets the
   player get up; rule v1 downs them again 3.7 s later. `Core::getup_step` would do it at once but is **off** (`early_redown = false`):
   never exercised in the game, effect UNKNOWN.
   Gauge flicker (up to 50 ms per rival hit, both screens): improvement ideas in `PROJECT_PLAN.md` §12.
   Attacker-side gauge with teams of 2+: a copy cannot tell who hit, so `remote_step` only acts for a downed rival with no teammate.
   With teams, a rival's hit on a downed enemy will again stay on the attacker's gauge until this is solved. Run 21 confirmed the rule
   itself is right with teams; what that gauge looked like on the lone rival's screen was not reported.
   New in run 21: the combat step peaked at 63–225 ms around rival hits with three games running (cause UNKNOWN, rule still fired).
2. Separate team spawns: needs Task D, then "move each team to its own point after landing" (`research/S6-team-spawns.md`).
3. Death rules: keep level, starting items, respawn protection (`research/S5-death-respawn.md`; level and runes are not located yet).
4. Inventory drop on death (`research/S2-inventory-drops.md`).
5. No-boss nights, final arena, sudden death, winner's choice (`research/S3-match-flow.md`, `S4-mod-channel.md`).
6. Matchmaking: how it works and its logs are in `MATCHMAKING.md`; design history in `research/S9-matchmaking-decision.md` §5c–5g.
   Next: MM-3g (trigger the game's Invite Members from the queue so no one clicks; call chain in S9 §5d), then MM-6 Duo/Squad.
7. Structure: R0, R1, R5 done; R2 and R3 half done (offsets in `game/layout.rs`; `team/` and `combat_state/` split by job). The open
   halves (typed views + one `GameMem` trait; `Feature` trait + `World` read model) go in with the first new feature. `ARCHITECTURE.md` §6.

## 5. Tools and how to run them

| Tool | Command | Note |
|---|---|---|
| Build the mod | `powershell -NoProfile -File scripts\build.ps1` | Fails to copy if a game using `dist\nrpvp.dll` is open |
| Deploy to Nucleus | `powershell -ExecutionPolicy Bypass -File scripts\deploy-nucleus.ps1 [-TeamSize N]` | Close Nucleus instances first. Resets the handler's settings |
| Build the player zip | `powershell -ExecutionPolicy Bypass -File scripts\package-release.ps1 [-TeamSize N]` | Builds the player DLL itself (`--no-default-features`, no dev tooling) into `target\player\` |
| Developer launch (one PC) | `.\scripts\launch.ps1` | SC + developer DLL + game-data package |
| Console | `dist\nrcon.exe list / status / players / log 30 / -i N <cmd> / --all <cmd>` | Talks to every running developer game (`dev_mode = true`). Commands: `CONSOLE.md` §0 |
| Scanner | `dist\nrscan.exe list / peek / poke / clock / bar / explain` | Reads (and with `poke`, writes) a **Nucleus** game from outside. Refuses to write to any other install |

Watch a player's downed block live (replace the address with `PlayerIns+0x710` from the instance log line `local = 0x…`):
`dist\nrscan.exe peek --instance 0 0x<addr> --hex 36 --interval-ms 300`

Logs: the mod writes `nrpvp.log` (current session) and `logs\nrpvp_<UTC>.log` (newest 30) next to the DLL; me3's own logs are in
`%LOCALAPPDATA%\garyttierney\me3\data\logs\<profile>\` (check that `nrsc.dll` loaded early and `nrpvp.dll` after it).

## 6. Traps we already hit

- **Nucleus `dlllist.txt` must not end with a newline.** It makes Nucleus fail with "WaitForSingleObject failed 258". The deploy script writes it correctly; never edit it by hand.
- **Under Nucleus, keys reach a window only when it has focus** (END unlocks input). Use the console instead: `nrcon --all <cmd>` reaches every instance.
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
  `RegionCache::direct()` there (the guarded `RegionCache::new()` is test-only since R5). The heartbeat line shows the pass time.
- **Only a lobby the game makes can be joined.** Our own `CreateLobby(Public, 6)` gets adopted by Seamless Co-op but joiners' games
  leave it at once, and a direct `JoinLobby` gives Steam membership without the game session (FS-MM2). Steam lobby membership is not
  presence: ALL PRESENT also needs the host's ChrSet to hold the players.
- **`mm_quiet_invite` and `mm_lobby_trace` both use all four debug registers**; config turns quiet invite off when the trace is on.
- **Sonnet agents can be cut off by usage limits mid-task.** Check `cargo build` and `git status` before trusting the tree; the task card says what was intended.

## 7. Repository state

- `main`: docs, R0, R1, CON-1/2 (2026-10-20). Branch `eng/cleanup-r5-r2-r3`: R5, R2-lite, R3-split, waiting for SMOKE-2. Leonardo pushes.
- Branch `t-028-snapback` (Leonardo): T-028 snap-back/teleport runs and F3 team spawns, 3 commits not on `main`. Resolve when spawns start.
- Tests: `cargo test --workspace` → 385 pass (353 nrpvp, 4 nrcon, 3 launcher, 25 nrscan; ignored: the timing-dependent `nrscan`
  self-scan `poke_add_is_followed`). R5 removed ~110 tests together with the code they tested. fmt, clippy `-D warnings` (dev and player builds) clean; the same checks
  run in GitHub Actions (`.github/workflows/ci.yml`) on every push.
- Builds: `cargo build --release` = developer DLL (feature `dev`, default: console, RE tracers); `--no-default-features` = player DLL
  (refuses `dev_mode`). Building needs the MSVC Build Tools (Visual Studio in `D:\Programs\Microsoft Visual Studio`).
- Pins: game 1.03.3 (exe 1.3.3.0), Seamless Co-op v1.1.3, bundled me3 0.11.0.
- Friend-session logs are gitignored (`*.log`); they live in `.claude/experiments/<session>/` on Leonardo's PC only.

## 8. Decisions made (so they are not re-argued)

- Stack: Rust DLL loaded through me3 next to Seamless Co-op; nothing in Seamless Co-op is modified.
- **The table offers only Solo / Duo / Trio Battle Royale** (Leonardo, 2026-10-19): no normal expedition while the mod is installed. The
  choice opens the stock boss menu; pressing start begins our matchmaking; the game's own "Cancel matchmaking" stops it (no Leave entry). UI-1.
- **No ImGui for players in the final product** (Leonardo, 2026-10-19): menus and status messages must be the game's own UI. The
  Insert panel was a stop-gap; **removed from the player build on 2026-10-20** (the overlay is a developer tool only).
- **Every match has 6 players** (Seamless Co-op's limit; Leonardo, 2026-10-19): Solo 6 × 1, Duo 3 × 2, Squad 2 × 3. Pre-release zips use 2 for testing.
- PvP is always on for rivals; players cannot switch it off. Launching with `nrpvp_launcher.exe` is the mode, until a menu entry exists.
- Hostility: each game keeps itself and teammates on the normal value and marks only rivals (value 29).
- Downed players: teammates fill the purple bar; rivals shorten the bleed-out and never move the bar. Instant death on 0 health was
  rejected (auto-revive items would defeat it).
- Investigation first, then building: confirm memory locations with the scanner before adding mod code.
- Matchmaking joins through the game's own lobby (Invite Members path), never a lobby our mod creates; never EAC, never Bandai servers.
- Leonardo only does smoke tests; Claude decides design and order, and does not build tools that are not needed for the goal.
- **Developer control is a text console** (2026-10-20): commands over a named pipe with `nrcon`, so agents can drive a running game;
  the ImGui panel and F-keys were deleted (R5, 2026-10-20). `CONSOLE.md`.
- **Code structure** (2026-10-20): ports and adapters, typed memory views, feature modules, dev code behind a cargo feature; refactor
  before the next big features. `ARCHITECTURE.md`.
- **One owner doc per topic** (2026-10-20); the map is in `CLAUDE.md`. Finished task cards and runbooks leave the docs (git keeps them).
