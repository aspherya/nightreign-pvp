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

### FS-RW1 — the new match rules on two real PCs (about 60 min, two or three matches)
**Zip:** `dist\NightreignPvP.zip` (STATUS §2 has the build, the `nrpvp.dll` hash and what `VERSION.txt` says), `-MatchPlayers 2`, `team_size = 1`. Both PCs compare `NightreignPvP\VERSION.txt`, Seamless Co-op v1.1.3 and game exe 1.3.3.0 (stop Steam auto-updates). Never EAC. **Pre-flight:** back up `%APPDATA%\Nightreign\<steamid>\`; Leonardo installs the zip himself and sees Solo / Duo / Trio in the table menu. For a shorter Day 1 both PCs add the line `day1_secs = 300` at the top of `NightreignPvP\nrpvp.toml` (default 1020 s = 17 min; both the same, the host ends the day).

**How a match runs:** table > Number of Players = Solo > Commence; the host presses Invite Members; both land by hawk. Nothing needs a console. **Escape hatch:** a defeated player watches until the match ends and can press F10 to leave; if F10 does nothing, Alt-F4 and say so. Write down the time of every quit and any Windows "Application Error".

1. **Free Day 1.** Play Day 1 to its end (no circle, no storm, no night). **Expect** both logs: `free Day 1: clock speed`, host `Day 1 is over at 11:50`, both `Day 2 has begun`; then the game's morning (clock 07:00) and the normal tide on Day 2. Photo of the clock around 11:50 and of the morning.
2. **A real kill.** (Match A, Day 1 or Day 2.) The host brings the guest to 0 HP and the bleed-out runs out (or the host finishes them). **Expect** guest log: `LOCAL defeated by a rival`, `spectating player …`, then `M5-9 deathhold: start step HELD` (no respawn); the camera follows the other player until the match ends; F10 gives `the leave key was pressed`, the defeat screen, Result, Roundtable. Host log: `match decided … won` about 31 s after the guest's death (30 s dead timer), then the win ending. **Unknown today:** is `start step HELD` reached after a bleed-out death in a session (the one-PC runs used a direct death)? If the guest respawns anyway, say how long they stood before the defeat screen.
3. **A non-player death.** (Match B.) The guest dies to an enemy or a fall. **Expect** `LOCAL died to an enemy or a fall`, respawn after about 14 s with items, no `match decided` in the host's log. Optional: the guest dies more than 30 s after the last rival hit.
4. **Tide death on Day 2** (optional): stand outside the closed circle until dead. **Expect** `defeated by the tide`.
5. **Enemy strength.** Solo match: fight the first camp. Does it feel like a solo run? Logs: `multiplay: enemy scaling rows set for team size 1` on both PCs (the scan takes about a minute after the game starts; do not start the expedition before it).
6. **Private code.** Both PCs set `private_code = "test1"` in `nrpvp.toml` and queue: they must meet (the queue log shows a `room` tag `p-…`). Then one PC changes it to `other` and both queue for 60 s: they must **not** meet.
7. **Observations for RW-3 / 5 / 8 / 9 (no pass or fail, photos and a line each):** (a) runes: one player kills enemies; does the other's rune counter rise (photo of both counters before and after)? (b) the map: is the other player's marker visible (photo of the map on both)? (c) the compass bar and the nametag at about 10 m and 50 m (photos); does a lock-on show a name and a life bar? (d) if a boss dies: who gets the Dormant Power or weapon choice? (e) what the defeated player's screen shows from the death on: the camera should follow the other player (spectating) and F10 should leave; and what the OTHER screen shows of the defeated player's body (invisible? a body hovering next to them?).

**Send back, both PCs:** `NightreignPvP\nrpvp.log`, `NightreignPvP\logs\`, the me3 log folder, the photos and a line per step. Put them under `.claude\experiments\FS-RW1\host\` and `...\guest\`. Greps: `night_match|deathhold|multiplay|room|T-033 STATE`.

**No-go:** if a defeated guest can still fight after their defeat (respawns and walks about), stop the kill steps, send the logs and play free for the rest.

Finished runbooks (FS-1, FS-MM1, FS-MM2, FS-UI1 on 2026-10-20; FS-UI2 on 2026-10-21) were removed; their results are in §3 and their text in git
(`git show 339ee04:.claude/TESTING.md`; FS-UI2: `git show c727293:.claude/TESTING.md`).
