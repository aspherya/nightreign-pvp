# Status and handoff (updated 2026-10-22)

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
| One-click install (`dist\NightreignPvP.zip`: launcher + mod + bundled loader + game-data package) | **Works** on two PCs (FS-UI2: Leonardo + friend, game data next to SC on both) |
| Player build: PvP always on, no keys, **no overlay** (2026-10-20: the in-game menu and messages are the only UI) | **Works on two PCs** (FS-UI2, 2026-10-21, zip 71f893d) |
| **Solo matchmaking over real Steam (queue → match found → same Roundtable → expedition as rivals → back to idle)** | **Works with one host click** (FS-MM3, 2026-10-19, 2 PCs): the host presses Invite Members when asked; everyone else does nothing. Started from the table (UI-1) in the player zip since 2026-10-20; 2-player matches until release, 6 in the final game; 3–6 players only in automated tests |
| Battle Royale in the stock table menu (UI-1) and status as the game's own messages (UI-2) | **Works with two players** (FS-UI2, 2026-10-21): 3 rounds, each PC host at least once: Solo → Commence → match → host's Invite Members (no Steam window) → same expedition as rivals → back to idle. The host starts the expedition the normal way; our menu code is not involved. Polish: "match found!" and "joining the host…" never show (the next state arrives within the 6 s gap) |
| **Downed rule (Leonardo's design): a rival's hit never keeps progress on the purple bar and takes 3 s off the bleed-out; a teammate's hit fills the bar as normal; a rival can never revive a rival** | **Works, clean (T-041, Nucleus 2026-10-22, free-for-all and 2 v 1):** the rival's hit is cancelled inside the game (hook on the bar store), so no stand-up and no gauge flicker on any screen; teammates revive normally. Fallbacks: revive-event block and the T-040 re-down. Not yet in a player zip or over real two-PC lag |

In progress: the hawk crossing (PROJECT_PLAN §13 v1): **works solo in the dev build** (one diagonal line, cruise, dive, drop;
HAWK-1b/3/4 PASS 2026-10-22, branch `hawk-crossing`). **HAWK-2 merged into `main` 2026-10-22** (PR #2, 8aca550): on by default in both builds (`[hawk]` in `nrpvp.toml`, no console needed), route file in the player zip. **Solo
smoke PASS 2026-10-22** (TESTING §3: cruise, dive, drop with no command; Leonardo: "how I wanted it"). Next: a new player zip, then an FS. Not started: keeping level on death, inventory drop, no-boss nights, final arena, sudden death, winner's choice.
See `PROJECT_PLAN.md` §2 and §9.

**Known bug:** the game crashes on quit (0xC0000374, heap corruption) on every run since 2026-10-10. Not proven to be Seamless Co-op's;
our thread that rewrites debug registers and the overlay's teardown were the suspects. The overlay is gone since R5 (2026-10-20): if
quits stop crashing on the new builds, it was the overlay; if not, the debug-register thread is next. **2026-10-21: the first quit on
the new build did not crash** (SMOKE-2); keep watching before calling it fixed. FS-UI2 (same day, the older zip 71f893d: hudhook
still linked, overlay off by config): both Windows crash records on Leonardo's PC were his Alt-F4 quits (Leonardo saw no crash), so
Alt-F4 hits it too. A player zip from `main` (post-R5, no hudhook) is the real check.

## 2. What is deployed where

| Place | Mode | Settings |
|---|---|---|
| Nucleus handler `D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN` (build of 2026-10-21 18:33 with T-040, uncommitted on `t-028-snapback`; `mm_queue`/`br_menu` off) | developer (logs to `dist\nucleus-logs\Instance<N>\`; after redeploy `dist\nrcon.exe --all` reaches every instance) | `team_size = 1` (T-040 test), PvP on |
| **Release candidate 2026-10-22:** `.claude\worktrees\offline-mode-ccabe0\dist\NightreignPvP.zip` from `main` 0145f72 (clean), nrpvp.dll DB60DAF8E06D743C, hawk routes 5ACA00E4047EB8C2, `-MatchPlayers 2` | player: Solo Battle Royale (Duo/Trio "coming soon"), hawk crossing always on, PvP + downed rules, offline mode via `mode = "offline"`; **no bots** (dev only) | `team_size = 1`, `matchmaking = true`. **Not** installed by the agent; regression REG-1 (TESTING §5) open |
| `dist\NightreignPvP.zip` (2026-10-20, nrpvp.dll 0881264E03245444, `-MatchPlayers 2`): **superseded** by the 2026-10-22 release candidate | player (no keys, no overlay; in-game menu + messages) | `team_size = 1`, `matchmaking = true`, `br_menu` on. **Not** installed in Leonardo's game folder by the agent |
| Worktree `.claude\worktrees\offline-mode-ccabe0\dist\` (dev build of `main` 0145f72, 2026-10-22; that folder's own `scripts\launch.ps1` uses it) | developer + console: offline mode + bots (REG-1 part B) | copy of `main`'s dev toml with `br_menu = true`, `mode = "br"`; copy of `dist\gamedata` (hawk line B) |
| `dist\nrpvp.dll` + `dist\nrpvp.toml` (used by `scripts\launch.ps1`) + `dist\nrcon.exe` | developer + console (no panel since R5). branch `hawk-crossing` (`main` + hawk console: `hawk_*`, CONSOLE §0), built 2026-10-22, nrpvp.dll EA76EE9D80AFE6FA: HAWK-4 run 2 PASS. `dist\gamedata` also holds the hawk line B (`-HawkRoutes`) | `team_size = 1`, `team_spawns = false`, **`br_menu = false`** (2026-10-21: so Commence starts a vanilla solo expedition; with it on, Commence waits for a 2nd player). Set it back to `true` for queue tests |

HAWK-2 worktree `.claude\worktrees\interesting-tharp-1c1237\dist` (2026-10-22, built from the HAWK-2 code): `nrpvp.dll`
(developer DLL), `gamedata` (with the line), `nrpvp.toml` = player mode + `matchmaking = false` for the solo smoke, and a check
build of `NightreignPvP.zip` (dirty, not for players). The main checkout's `dist\nrpvp.dll` is the same HAWK-2 build since
2026-10-22 (the `hawk-crossing` build is kept as `dist\nrpvp.dll.hawk-crossing.bak`).

`dist\friend-kit.zip` is the **old** kit from 2026-10-10. Do not send it; send `NightreignPvP.zip`.

## 3. Next, in order

1. **FS-UI2: PASS (2026-10-21, TESTING §3).** MM-4, UI-1, UI-2 accepted and removed from TASKS. Its rival-revive miss is fixed
   by **T-040** (Nucleus PASS 2026-10-21: the re-down now writes the character data module's HP, the old `+0xB78` route was not
   always the health; retry 2 s; diagnostics in the player build). **T-041** (2026-10-22): accepted, no stand-up and no flicker
   (STATUS §1). Next for players: a new zip from this branch for the next FS. Still open: a new player zip (post-R5, with T-040) for the next FS.
2. **SMOKE-2: PASS (2026-10-21, TESTING §3).** R5, R2-lite, R3-split merged into `main`. Next engineering step: the open halves of R2/R3
   with the first new feature (ARCHITECTURE §6).
3. **MM-3g + MM-7** (no Invite Members click, auto-start; one shared RE spike), **MM-8** (host by best connection), **MM-9** (host
   leaves: FS observation, then a clean match end), **S10** (study SC toward host migration); then **MM-6** (Duo/Trio). Later polish: live count inside the searching box (UI-1 C3).
4. **Hawk crossing** (PROJECT_PLAN §13, TASKS HAWK-0..2). The teleport (T-028) passed on 2026-10-14 (solo + Nucleus). `t-028-snapback`
   was merged into `main` on 2026-10-22 (Leonardo, with T-040/T-041); **SMOKE-3 PASS** (2026-10-21, TESTING §3: both jumps, flat and
   cliff, landed and held on `main` d3d1eb8; the teleport needs no change). HAWK-0 research is done
   (2026-10-21: the hawk is a player script state; the start routes are 3-point, 200 m routes in `m60_00_00_99.msb`). HAWK-1b (2026-10-22, TESTING §3): **the game flies our long start
   route and its own route end drops the player** (B1 works); 1 of 3 rides landed on ground, 2 fell to y −26 (sea, or ground not loaded
   far from the vanilla drop: open). `dist\gamedata` holds the single diagonal line B (dev only, 2026-10-22). Next: HAWK-1b land check, HAWK-A2
   (static RE), HAWK-1 (ride probe).
   **HAWK-3 run 1 PASS (2026-10-22, TESTING §3):** the DLL swaps the game's hawk-answer pointer safely; `hawk_drop` and `hawk_tree`
   work (Leonardo). Design v1 (cruise on one diagonal line, stick-held dive, vanilla drop buttons): PROJECT_PLAN §13. **HAWK-1b PASS
   (2026-10-22, run 3):** one diagonal line for every start route, three perfect rides, no fall damage. **HAWK-4 PASS (2026-10-22, runs
   1-2):** cruise lock, stick-held dive (7 s, ×1.3 speed), vanilla drop buttons, start animation cut; Leonardo: good enough. All
   on branch `hawk-crossing` (dev console only). **HAWK-2 solo smoke PASS (2026-10-22), merged (PR #2):** always on, no console; next: a new player zip, then an FS (both players cruise, dive, drop; each sees the other land).

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
   Gauge flicker: **solved by T-041** (2026-10-22): the rival's hit is cancelled before the bar changes.
   Attacker-side gauge with teams of 2+: a copy cannot tell who hit, so `remote_step` only acts for a downed rival with no teammate.
   With teams, a rival's hit on a downed enemy will again stay on the attacker's gauge until this is solved. Run 21 confirmed the rule
   itself is right with teams; what that gauge looked like on the lone rival's screen was not reported.
   New in run 21: the combat step peaked at 63–225 ms around rival hits with three games running (cause UNKNOWN, rule still fired).
2. **Offline mode** (decided 2026-10-22): **OFF-1 done** (solo PASS 2026-10-22, TESTING §3; branch `claude/offline-mode-ccabe0`,
   already synced with `main` incl. HAWK-2; next: merge it): `mode = "br" | "offline"` in `nrpvp.toml` + console `mode`; Offline lets Commence Solo run
   the game's own solo expedition. It brought the first `Feature`, the `World` read model, the `GameMem` port and typed views
   (ARCHITECTURE §6). **OFF-3 done (2026-10-22): bots GO for offline** — the game ships a
   dormant NPC bot system (AI Nightfarers, started by event instruction `2003[97]`, solo only; `research/OFF-3-bots.md`). Next:
   **OFF-4 bot spike PASS** (2026-10-22, branch `claude/off-4-bots`, not merged; console `bots 10|10000`, `bots_clear`; our events in
   `common.emevd`): bots spawn in the expedition, follow you and fight monsters; allies as shipped, **at most 2** (game design: solo +
   2 AI teammates). **OFF-5 done** (solo PASS run 2, 2026-10-22): in offline mode 5 hostile bots join after the landing, no
   console; they fight you, each other and monsters. Next: OFF-7 (bots count as rivals in the near-death rules), then OFF-6 (bots in
   the battle royale); OFF-2 (Solo/Duo/Trio (Offline) menu entries) later. `TASKS.md`.
3. Death rules: keep level, starting items, respawn protection (`research/S5-death-respawn.md`; level and runes are not located yet).
4. Inventory drop on death (`research/S2-inventory-drops.md`).
5. **No-boss nights (M5, Leonardo 2026-10-22: the nights become a player brawl): M5-1 research GO** (2026-10-22,
   `research/M5-no-boss-nights.md`): replace two shared events in `common_func.emevd` (no minions, no boss) and add a timed
   "night done" (7502 / 7507) in `common.emevd`; the circle and the rest of the night run on the clock. **M5-2** console `clock`
   (jump the play-area clock through the game's own set-time mailbox, MEMORY_MAP §16): **solo PASS 2026-10-22** (TESTING §3).
   **M5-3** (no boss, no minions; nights end 180 s after 23:59 with the normal morning): **solo PASS nights 1 and 2, 2026-10-22**.
   **M5-4** (research) in progress: Nucleus run 1 showed the alive flags drop on downed and a bled-out player comes back after ~7 s
   even at night; open: the night-end sync, the per-game endings, the comeback's code. **M5-D** decided (knock 15 s, then eliminated +
   spectate). Then M5-5 / M5-6 / M6-1 (TASKS). Final arena, sudden death, winner's choice later (`research/S3-match-flow.md`, `S4-mod-channel.md`).
   **OFF-8** (downed state in an offline solo match) now blocks OFF-7.
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
- **Nucleus does not load our game-data package** (it starts the game through Seamless Co-op's launcher, not me3): no no-boss
  nights, vanilla hawk route. Event-script features are tested solo with `scripts\launch.ps1`; under Nucleus use the dev console
  (`flag`, `clock`) instead.
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

- `main`: docs, R0, R1, CON-1/2, R5, R2-lite, R3-split, T-028 teleport, T-040/T-041 (SMOKE-1 2026-10-20, SMOKE-2 and SMOKE-3 2026-10-21). Leonardo pushes.
- `t-028-snapback` (T-028 teleport, parked F3 team-spawn rule, T-040/T-041) is merged into `main` (3ea3168, 2026-10-21); the branch
  can be deleted (SMOKE-3 PASS: the teleport works on `main`).
- Tests: `cargo test --workspace` → 404 pass on `main` d3d1eb8 (372 nrpvp, 4 nrcon, 3 launcher, 25 nrscan; ignored: the timing-dependent `nrscan`
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
- **The crossing line** (Leonardo, 2026-10-22): one line for every start route, diagonal "B" of the route atlas (the longer and higher;
  the "bottom to top" rule is dropped). TASKS HAWK-1b.
- **Match start = hawk crossing** (Leonardo, 2026-10-21): the start hawk flies one straight line across the whole map and each player
  jumps off where they want (Fortnite style). Replaces separate team spawns. `PROJECT_PLAN.md` §13.
- **Matchmaking direction** (Leonardo, 2026-10-21): stay on Seamless Co-op as long as it carries us, with three must-haves: no
  Steam window or invite ever on a player's screen, everyone arrives ready and the match starts by itself, and the "host" stops being a
  player-facing role. Asking SC's author is out of scope. Studying how SC works (read-only, for interop; never patched, copied or
  redistributed, hard rule 3) is approved, to prepare our own matchmaking/session layer if SC becomes the roadblock.
- **Nights are elimination rounds** (Leonardo, 2026-10-22; `re/PROJECT_SCOPE.md` §2): no boss, no minions; no respawn at night,
  the dead spectate; a night ends only when one player (team) is left, no timer; only that winner is healed; the others respawn
  spread out at the morning; night 2's winner wins the match, the rest lose; no Day 3 / Nightlord; the 40 m final circle stays.
  At night 0 HP = knocked for the normal 15 s (Solo too; rival hits −3 s, teammates revive); bleed-out over = eliminated until
  the morning, spectating (M5-D, 2026-10-22).
  Research and tests come before code: Leonardo runs Nucleus multi-instance checks for it before the first UAT with his friend.
- **Host leaving mid-match** (Leonardo, 2026-10-21): for now accept that the match ends, and end it cleanly; the real goal is **host
  migration** (Destiny / old CoD style), worked on as far as it goes. The host is picked by the matchmaker (best connection), never by
  luck, and has no player-facing role. **A server host (a PC running the game with no player) is rejected** for now.
- **One owner doc per topic** (2026-10-20); the map is in `CLAUDE.md`. Finished task cards and runbooks leave the docs (git keeps them).
