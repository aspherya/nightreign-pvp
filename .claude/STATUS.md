# Status and handoff (written 2026-10-12)

Read this first when picking the project up again. It says what works, what is built but untested, what to test next, and the
traps we already fell into. Details live in the other docs; this file points to them.

| Doc | Use it for |
|---|---|
| `re/MEMORY_MAP.md` | Every memory location found, with behaviour and proof |
| `PROJECT_PLAN.md` | The Battle Royale plan (scope v2), milestones, §11 = the downed-player design |
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
| Teams from player IDs (`team_size` 1, 2 or 3) | Built. Only `team_size = 1` (free-for-all) has run in the game |
| One-click install (`dist\NightreignPvP.zip`: launcher + mod + bundled loader) | **Works** on Leonardo's PC. Not yet tried on a second PC |
| Player build: PvP always on, no keys, small "BATTLE ROYALE" label | **Works** (solo check) |
| A rival can't revive a rival (rule v1: bar filled by a rival → knocked down again) | **Works** (Nucleus) |
| **Downed rule (Leonardo's design): a rival's hit restores the purple bar and takes 3 s off the bleed-out; a teammate's hit fills the bar as normal** | **Built 2026-10-12, 278 tests pass, deployed. NOT yet run in the game** |

Everything else in scope v2 is not started: separate spawns, keeping level on death, inventory drop, no-boss nights, final arena,
sudden death, winner's choice, menu entry, queues. See `PROJECT_PLAN.md` §2 and §9.

## 2. What is deployed where (all the same build, 2026-10-12 13:33 local)

| Place | Mode | Settings |
|---|---|---|
| Nucleus handler `D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN` | developer (panel does not draw under Nucleus; logs to `dist\nucleus-logs\Instance<N>\`) | `team_size = 1`, PvP on, probes off |
| `dist\NightreignPvP.zip` | player (no keys, label only) | `team_size = 1` |
| `dist\nrpvp.dll` + `dist\nrpvp.toml` (used by `scripts\launch.ps1`) | developer, full panel | `team_size = 1`, probes on |

`dist\friend-kit.zip` is the **old** kit from 2026-10-10. Do not send it; send `NightreignPvP.zip`.

## 3. Next human tasks, in order

### Task A: smoke-test the downed rule (Nucleus, 2 instances, ~10 min)
1. Launch both instances from Nucleus, join the same expedition, daytime, away from enemies.
2. A downs B.
3. A hits B slowly, one hit every 2 seconds.
4. Expected: **the purple bar does not fill** (it may flicker for a split second per hit), and **B dies after about 4–5 hits**, well before 15 seconds.
5. Also do one untouched down: B should bleed out in the normal ~15 seconds.
6. Close the games, then tell Claude what happened. Logs: `dist\nucleus-logs\Instance0` and `Instance1` (look for `rule: rival hit on downed LOCAL`).

If the bar still fills: the write to the bar or the pause timer is not sticking (only the clock write was proven). Say so; that is the first thing to check.

### Task B: play with a friend on the player zip
1. Send `dist\NightreignPvP.zip`. They follow `HOW TO INSTALL.txt` inside it (drag two things into the game folder, double-click the launcher).
2. Leonardo installs the same zip the same way (replace the old `NightreignPvP` folder and `nrpvp_launcher.exe` in the game folder).
3. Host "Up to 6 players", friend joins, nobody presses anything.
4. Check: both labels show "1 rival"; both can fight; graces and pickups work; the downed rule behaves as in Task A.
5. Afterwards each sends the `logs` folder from `<Game>\NightreignPvP\`.

### Task C: three players, teams of two (needs 3 people or 3 Nucleus instances)
The only way to see both halves of the downed rule at once: a teammate filling the bar while a rival shortens the clock.
Set `team_size = 2` (Nucleus: `scripts\deploy-nucleus.ps1 -TeamSize 2`; zip: `scripts\package-release.ps1 -TeamSize 2`).
Also the first test of: teammates can't hurt each other, teammate revive works, player IDs with 3 players.

### Task D: position snap-back test (needed for separate spawns)
Never run. Developer build in Nucleus: stand still, **F7** (save spot), walk ~20 m, **F6** (try to return), note the result, **F9** (next
candidate), repeat for 6 candidates. Tell Claude which one snapped the player back and what the other screen showed.

## 4. Next building work (after Task A passes)

1. Tune `rival_hit_seconds` (default 3.0) from play feel. Make the check faster if the bar flicker is visible.
2. Separate team spawns: needs Task D, then "move each team to its own point after landing" (`research/S6-team-spawns.md`).
3. Death rules: keep level, starting items, respawn protection (`research/S5-death-respawn.md`; level and runes are not located yet).
4. Inventory drop on death (`research/S2-inventory-drops.md`).
5. No-boss nights, final arena, sudden death, winner's choice (`research/S3-match-flow.md`, `S4-mod-channel.md`).
6. "Battle Royale" menu entry and queues (`research/S7-mode-menu.md`, `FUTURE_CONSIDERATIONS.md`).
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
- **Addresses change every launch and every area load.** A value found by the scanner is only good for that run; the mod must reach it through the player object (see `re/MEMORY_MAP.md`). A scanner left watching a fixed address after a restart is looking at the wrong memory: stop it.
- **Double-check offsets before any write.** On 2026-10-12 a test wrote to `+0x728/+0x72C` instead of `+0x718/+0x71C` by mistake (no lasting effect).
- **Setting health to 0 does not kill** in a multiplayer session; it downs. Death is the bleed-out clock reaching 0.
- **Elden Ring layouts are hints only.** Several were wrong in Nightreign (list in `re/MEMORY_MAP.md` §9).
- **Sonnet agents can be cut off by usage limits mid-task.** Check `cargo build` and `git status` before trusting the tree; the task card says what was intended.

## 7. Repository state

- Last commit: `64ed3ac`. **About 30 files are changed or new since then and not committed**, including `crates/nrscan`, `crates/nrpvp-launcher`,
  the downed rule, the scripts, and these docs. Leonardo commits; nothing was committed by Claude.
- `scripts/package-friend.ps1` and `scripts/friend-kit/` were deleted (by Leonardo); `package-release.ps1` replaces them.
- Tests: `cargo test` → 278 pass in `nrpvp`, 26 in `nrscan`, 3 in the launcher.
- Pins: game 1.03.3 (exe 1.3.3.0), Seamless Co-op v1.1.3, bundled me3 0.11.0.

## 8. Decisions made (so they are not re-argued)

- Stack: Rust DLL loaded through me3 next to Seamless Co-op; nothing in Seamless Co-op is modified.
- Target: 6 players (Seamless Co-op's limit); design for N teams of M.
- PvP is always on for rivals; players cannot switch it off. Launching with `nrpvp_launcher.exe` is the mode, until a menu entry exists.
- Hostility: each game keeps itself and teammates on the normal value and marks only rivals (value 29).
- Downed players: teammates fill the purple bar; rivals shorten the bleed-out and never move the bar. Instant death on 0 health was
  rejected (auto-revive items would defeat it).
- Investigation first, then building: confirm memory locations with the scanner before adding mod code.
- Leonardo only does smoke tests; Claude decides design and order, and does not build tools that are not needed for the goal.
