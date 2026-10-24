# Testing

Every result records: **date, testers, game build, SC hash, me3 version, nrpvp commit or version, launch
path (SC launcher or me3 profile)**. Classify each problem as one of: `MOD-DEFECT`, `GAME-BEHAVIOUR`, `SC-LIMITATION`, or
`UNVERIFIED`.

## 1. Test environment

| Item | Value |
|---|---|
| PC A (host) | Leonardo's PC, Steam ID 76561198342403479. Used for all solo testing |
| PC B (client) | Friend's PC, available **only during scheduled Friend Sessions** (§6) |
| Network | Internet (Steam P2P via SC) |
| Pins | see TECHNICAL_NOTES.md §1 |

Two instances on one PC **work via Nucleus Co-op** (Goldberg-emulated Steam, SC loaded in both): this is FS-0 (TASKS T-017), a local dry run. Real-network confirmation is still FS-1.

**Safety checklist before every session:** launch only via `nrsc_launcher.exe` or `scripts/launch.ps1`;
saves backed up; both PCs on the same SC build and the same nrpvp build.

---

## 2. Test cases

TC-000…TC-004 are the Phase 0 cases. The overlay buttons and F-keys they mention were deleted on 2026-10-20 (R5); the same
actions are console commands now (`team_set`, `team_restore`, `team_persist`, `mark`: CONSOLE.md §0).

### TC-000 — Solo SC baseline (HUMAN-solo)
Preconditions: saves backed up; stock `nrsc_launcher.exe`.
Steps:
1. Launch via the SC launcher. Note any SC version text (title screen, menus, popups).
2. Host an SC session alone. Note every SC-specific menu entry, option, or item in the inventory or shop,
   especially anything about PvP, friendly fire, teams, or rules.
3. Play a solo expedition through the first night boss (or ~15 min).
4. Confirm a `.co2` save appeared in `%APPDATA%\Nightreign\<steamid>\`.
Expected: normal play. Record the list of SC-specific UI and items found.

### TC-001 — Join and shared expedition (baseline, FS)
Preconditions: both PCs on the same SC build, launched via the path under test.
Steps:
1. A hosts an SC session. B joins.
2. Both choose characters and start an expedition.
3. Play until the first night circle closes (or ~10 min). Both fight the same enemies and pick up loot.
4. A kills an enemy that B is also attacking. Check that both see it die.
Expected: both are in the same world and see each other move; enemies, deaths, and loot stay consistent;
no disconnects.
Record: any desync (enemy HP differences, ghost enemies), disconnects, and the SC version.

### TC-002 — Friendly-fire probe (vanilla SC)
Preconditions: TC-001 session running.
Steps: A hits B with (a) a melee weapon, (b) a projectile or bow, (c) an AoE skill or ultimate art,
(d) a status buildup source (poison or bleed), and (e) a fire pot or throwable, if available.
Then check SC's in-game items and menus for any PvP or friendly-fire option, and repeat with it on.
Expected: unknown. This test establishes the baseline. Record for each attack type: hit reaction yes/no,
HP loss on B's screen, HP loss on A's view of B, and status buildup.

### TC-003 — E-003: PvP via team 29 (FS-1, decisive)
Preconditions: both PCs launched through the me3 SC path with the same nrpvp build (compare the `VERSION.txt` from the friend kit) and the same nrsc.dll hash.
Both are in the same **"Up to 6 players"** expedition (A hosts, B joins). Overlay visible on both. Stand together, away from enemies.
Everyone uses **MARK** before and after each step (e.g. `MARK: row2 A hits B`).
Each row: A attacks B 3 times (melee), then B attacks A 3 times. Record **on both screens**: lock-on possible? hit reaction? HP bar drops (attacker's view and victim's view)?

| Row | On A's PC | On B's PC | Question answered |
|---|---|---|---|
| 0 | nothing (both team 2) | nothing | Baseline: is there friendly fire at all? (= TC-002) |
| 1 | — | B sets **own** (LOCAL) to 29 | Victim-side flag enough? |
| 2 | A sets **B's copy** (REMOTE) to 29 | B restores own to 2 | Attacker-side flag enough? |
| 3 | A sets B's copy to 29 | B sets own to 29 | Both sides agree on B |
| 4 | A sets **own** to 29 + B's copy to 29 | B sets own to 29 + A's copy to 29 | **Target config: free-for-all** (both hostile both ways) |

Row 4 tail (≈10 min): both fight each other **and** nearby enemies (PvPvE). Then A kills B: record downed/death state, whether A can or must revive B,
whether B respawns, whether the expedition continues for both, whether team values survive (watch the overlay). Then let the night circle close, if time allows.
Pass criterion for H1: row 3 or row 4 gives two-way damage that is visible and consistent on both screens, without a disconnect.

### TC-004 — E-002: team type vs AI hostility (HUMAN-solo)
Preconditions: T-008 and T-013 done; `./scripts/build.ps1`, then `./scripts/launch.ps1` (SC + nrpvp; never online); solo SC
"Solo Mission"/expedition; `dist\nrpvp.log` open in a tail window (`Get-Content dist\nrpvp.log -Wait -Tail 50`). The game
window must be in the foreground. **Delete `dist\nrpvp.toml` first** so the new defaults are regenerated (an old file works, but
its missing keys fall back to the defaults silently). The effective config and `overlay: ...` lines are logged at startup.
Primary method: the **overlay** (ImGui panel, Direct3D 12). It appears on the first rendered frame, visible, top-left; **Insert**
hides/shows it (`overlay_toggle_key`). Panel: status block (WorldChrMan, local player, live `team_type`, captured original,
selected candidate, persist + reverts in the last second, busy flag), one button per candidate (`6 Enemy`, `29 Indiscriminate`,
`3 Black Ghost`, `16 Invader`, `13 Invader`, `2 White Ghost`, `1 Player`; the selected one is green): **click = select and write
to the local player**, `Restore original`, a `Persist` checkbox, `Discovery` and `Histogram` buttons (disabled while busy) and a
scrollable log tail. Clicks on the panel are not passed to the game while the cursor is over it. If the mouse cannot reach the
panel in-game, use the hotkey fallback below and report it. Every overlay action also logs as before, with `overlay` in place of the
key name (e.g. `T-008: overlay team write PlayerIns 0x... +0x74: N -> V (readback V)`).
Fallback hotkeys (game window in the foreground; rebindable in `nrpvp.toml`; **F4 is not bound**, SC uses it for pause): F5 write
the selected candidate, F6 restore, F7 persist on/off, F8 histogram, F9 discovery. Cycling the selection is overlay-only (set
`hotkey_cycle` to bind it). The selection starts at the first candidate (6). All log lines start with `T-008:`.
Steps (overlay; each has a hotkey equivalent):
0. About 30 s after the world loads, check the panel's status block (or the log): `WorldChrMan: live`, `Local player: PlayerIns 0x...`
   and `Local team_type (live): N`. The log has `ChrSet ... capacity=6`, six `entry[i] raw=...` lines, `local player = 0x...
   (reason)` and `new WorldChrMan instance ...: local player 0x... team_type = N at start`. Record N. If the panel says
   "not resolved" or the log says "ambiguous"/"NONE", stop and report: the candidate buttons and F5-F7 will refuse to write.
1. Stand near a group of normal enemies, out of aggro range. Click `Histogram` (or F8) and record the histogram lines in the log
   (`F8 all:` and the per-class lines); the enemies' value is expected to be 6, the local player's is the N from step 0.
2. Click the first candidate button you want (do the candidates in list order, one at a time, steps 2-3 per candidate): expect
   `overlay team write PlayerIns 0x... +0x74: N -> V (readback V)` and the status block showing `Local team_type (live): V`, with
   `Original (captured): N`. Walk into the group. Record: do they aggro? Do your attacks still damage them? Can you lock on? Does
   the watcher (`watch: PlayerIns ... team_type=`) show V staying put?
3. Click `Restore original` (F6): expect `overlay restore ... V -> N (readback N)`. Repeat the observation from step 2 (they should
   behave as vanilla again). Then go back to step 2 with the next candidate. (Persist must be OFF for the restore to stick.)
4. With a candidate applied and persist mode **off**, do each of the following and watch the log tail / status block for resets (the
   watcher prints a `watch:` line with `(was ... team_type=V)` whenever the value changes): dodge roll, take damage, die and get
   revived (or the solo equivalent), use the ultimate art, rest or level up at a site of grace, fight the night-boss transition, day
   change. A new area/map load logs `WorldChrMan instance is now ...` and a new `new WorldChrMan instance ...: local player ...
   team_type = N at start` line, and the status block shows `Original (captured): none yet`: the applied value is probably gone
   with the old objects; click the candidate again if so.
5. Repeat step 4 with persist mode **on**: click a candidate once, then tick `Persist` (F7; log `persist ON`). The status block shows
   `reverts in last 1 s: n   total: m`; the log prints `persist: N reverts in last 1 s, last seen value X` once per second only
   while the game is reverting the value (silence means it holds). Note any stutter, crash, or odd AI behaviour. Untick `Persist`
   (`persist OFF (K revert(s) counted)`), then `Restore original`.
6. Repeat steps 2-3 for the remaining candidates (the card's list in order: 29, 3, 16, 13, 2, 1) and record, per value, the
   behaviour of nearby enemies. Click `Histogram` once at the end to confirm it is back to normal after the restore.
Also record for T-013: does Insert toggle the panel, does the mouse reach it, is the game stable for 10 minutes with it visible.
Expected: unknown. This is what we're testing. Record the result for every step. Attach the `T-008:` lines of the log.

---

## 3. Results log

One entry per test run: date, who, build (game / SC / me3 / nrpvp commit + dll hash), logs or screenshots as evidence, steps with what was seen, verdict **PASS / FAIL / PARTIAL**, and what it changed in the docs. Entries live in `testing-log/` (newest file last; add new entries at the top of the newest file):

| File | Covers |
|---|---|
| [testing-log/2026-10-09_to_19.md](testing-log/2026-10-09_to_19.md) | FS-0, FS-1, TC-00x, T-0xx, MM, UI (frozen) |
| [testing-log/2026-10-20_on.md](testing-log/2026-10-20_on.md) | R5 onward: UI-1/UI-2 sessions, hawk, offline, M5 nights, M5-8/M5-9, puppets, orchestrator scenarios |

Automated acceptance: `scenarios/*.toml` run with `scripts\regress.ps1` (orchestrator, one PC; `.claude/orchestrator/README.md`). A card whose acceptance is a scenario is done when that scenario is PASS in a log entry.

## 4. Known bugs

Owned by STATUS.md §1 ("Known bug") so there is one list.

## 5. Regression set

Rerun after any game, SC, me3 or nrpvp update and before every player zip:
1. **Automated, one PC:** `scripts\regress.ps1` (all scenarios, catalog in `orchestrator/SCENARIOS.md`); needs `nro desk`.
2. **Player-zip smoke, 5 minutes, Leonardo** (the zip installed as a player would; REG-1 of 2026-10-22 was retired because its bot checks no longer exist):
   Roundtable > Matchmaking Settings > Number of Players lists **Solo / Duo / Trio**; Solo > Commence shows "searching for players (Solo)" and the button reads Stop Searching (press it: "stopped searching"); Duo > Commence says "coming soon".
   Optional offline check (dev only, not the zip): `mode = "offline"` in `nrpvp.toml`, Solo > Commence starts the game's own solo expedition.
3. TC-001 (Seamless Co-op alone still works).
Past regression runs: `testing-log/`.

## 6. Friend Session runbooks

### FS-M59 — what only two real PCs can show (M5-9 and the night flow; one page; about 70 min, two matches)
**Zip:** `dist\NightreignPvP.zip` of 2026-10-24, built from `main` d1c3264, `nrpvp.dll` 64E49188380B5C54, `-MatchPlayers 2` (STATUS §2). It has our event files, no console, `team_size = 1`. Both PCs compare `NightreignPvP\VERSION.txt` and the Seamless Co-op version (v1.1.3); the game must be exe 1.3.3.0 (stop Steam auto-updating overnight: an update means stale offsets and no version gate yet). Never EAC.

**Pre-flight, both PCs (10 min):** back up `%APPDATA%\Nightreign\<steamid>\` (`.co2` and `.sl2`); Leonardo installs the zip himself and does a 5-minute smoke run (Roundtable table menu shows Solo / Duo / Trio, Solo > Commence searches); both know where `NightreignPvP\nrpvp.log` and `NightreignPvP\logs\` are and how they will send them.

**How a match runs:** play from the table (Solo, host presses Invite Members, both land by hawk). Nothing needs a console: a night starts by itself at 23:00 of the play clock, ends when one player is left, and has no timer. **Escape hatch:** a dead player cannot open Esc; if the dead player is still dead 30 s after the night was decided (no `RELEASED` in the log), Alt-F4 and say so. After every quit write down the time and any Windows "Application Error" in Event Viewer > Windows Logs > Application (Event ID 1000 / 1001; known quit crash 0xC0000374).

**Match A (about 35 min).** Night 1: the **host** kills the **guest** (steps 1 to 3, do this first: if it fails, everything after is about the fallback). Night 2: the **host** dies (the guest must win) (steps 4 and 5a).
**Match B (about 25 min).** Night 1: the **host** dies to the guest or a monster, the guest stays up (step 5b: the host's death released at the morning). Then free play (step 7) until you are done.
1. **A real kill at night.** At night the host brings the guest to 0 HP. **Expect:** the guest dies at once (death animation, "YOU DIED"), no 15 s knock; guest's log `night_match: die-at-once ON` before and `M5-9 deathhold: start step HELD` after; `night 1 decided: … won/lost` on both. If the log says `die-at-once OFF (LOCAL holds a revive item)`, the guest was knocked as before: say so, the rest of the steps still run (knocked, eliminated, held).
2. **What the other screen shows.** Killer: is the victim's body on the floor, standing, or gone; does the victim stay on the compass / map? Victim: YOU DIED, camera only, can the menu open (Esc)? A photo of each screen.
3. **Morning.** After the decision the game's morning follows. **Expect** on the victim: `RELEASED … gate`, respawn at full HP, then (Day-2 clock reads 07:00) `moved to a random day-1 place`; no storm death. The winner is healed (`flag 10007410`). Killer: does the victim blink away and appear, or walk in? Note how long after the morning the victim is standing.
4. **Night 2 by count, both endings.** Day 2, at night the host dies: the guest's screen shows the win ending, the host's the loss ending. **Photos of both screens, from the moment the night is decided until the Result screen.**
5. **Host dies.** (a) Night 2 of match A, (b) night 1 of match B. Does the session stay up, does the guest get the decision (and, for (b), the morning), does the host's death come back at the morning? Send both logs.
6. **Vanilla multiplayer events at night.** No minions, no boss; does anything throw a dead or held player out ("Disconnected", back to the Roundtable) or show a game-over for both? Note the time of anything odd.
7. **Free play.** Fight normally (PvP, downs, revives by day). Anything odd: time + what happened.

**Send back, both PCs:** `NightreignPvP\nrpvp.log` (copy it right after each match, before the next start), `NightreignPvP\logs\`, the me3 log folder (`%LOCALAPPDATA%\garyttierney\me3\data\logs\`), the photos, and a line per step: pass / what you saw. Put them under `.claude\experiments\FS-M59\host\` and `...\guest\`. I read the logs (`grep -E "night_match|deathhold|T-033 STATE|die-at-once|hwbp"`); pass criteria are the **Expect** lines, anything else is a finding for the results log.

**Things the logs must answer (unknown today, from the 2026-10-24 review):** (1) does a real session carry SpEffect 6999500 (is die-at-once ON)? (2) is the held function `exe+0x5C4810` hit for characters other than LOCAL (the player build has no console: count `start step HELD` lines on each PC, and look for `RELEASED`, `dropped` or `no free hardware slot` on the PC whose player did not die)? (3) are our flags 10007420 / 10007410 per game, or do the two games fight over them (repeated `features: flag 10007420 ON/OFF (was ...)` or `10007410` lines flipping back and forth in a log mean the games fight over it)? (4) what Seamless Co-op does when the host dies (steps 5a / 5b). (5) any `no free hardware slot` line (the player build has only two free debug-register slots in an expedition).

**No-go trigger:** if the guest is still dead after the night was decided and the Alt-F4 recovery, stop the M5-9 steps (they cannot pass); spend the rest of the session on free play and the hawk crossing, and send the logs. The player DLL has never armed deathhold in a game (every one-PC run was the dev DLL), and Leonardo's solo smoke run cannot reach a Solo night, so match A night 1 is its first real run.

Finished runbooks (FS-1, FS-MM1, FS-MM2, FS-UI1 on 2026-10-20; FS-UI2 on 2026-10-21) were removed; their results are in §3 and their text in git
(`git show 339ee04:.claude/TESTING.md`; FS-UI2: `git show c727293:.claude/TESTING.md`).
