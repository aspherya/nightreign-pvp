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

### 2026-10-10 — FS-1 (real Steam, Leonardo hosts via me3; friend joins via the friend kit). Logs: `.claude/experiments/FS-1/{host,client}`
Join worked over real Steam through SC v1.1.3 with nrpvp loaded on both PCs (friend's kit unzipped to `<Game>mod`; launcher and overlay worked on the
second PC). Free-form session with the overlay; values changed often, so behaviour per configuration is from the testers' reports (pending).
**Log findings:** (1) local player = ChrSet entry[0] on both PCs; (2) `ChrIns+0x78` is the same per player on both PCs (host 0x6001FFFFFFFF, joiner
0x6002FFFFFFFF); (3) the friend picked the wrong row with "this is me" (entry[1] = the host), so the button should be replaced by rule (1); (4) no
reverts, no errors on either side; (5) both clients held 29 for both players only briefly (host from 16:49:59; client 16:46:24–16:48:24).
**Reported by Leonardo: PASS.** Both players could damage each other over the real connection (Leonardo lost the fight); hits and HP consistent on
both screens, no lag or desync noticed; nothing odd with values 6, 3, 1, 16; the session ended by Alt+F4 (not a crash).

### 2026-10-10 — FS-0 run 4 (Nucleus, both instances F11 at 15:32:19; auto_team_all off). Logs: `.claude/experiments/FS-0/run4_*`: **PASS**
Both clients held 29 for both players for 6+ min with no reverts. Reported by Leonardo: both players could lock on to and damage each other; HP
consistent on both screens; mechanics such as the Executor parry worked between players; PvE unaffected; nothing odd. PvP death: downed, revive bar shown
but reviving does not work, then respawn with **runes and a level lost** (as a normal death). Log evidence: the local player is ChrSet entry[0] on
both clients; `ChrIns+0x78` (ER p2p handle +8) is the same per player across clients (0x6002… / 0x6001…) [HYPOTHESIS: stable player id].
FS-0 run 3 (auto_team_all = 29) hung both instances ~1 s after Roundtable load; not reproduced with auto off; cause unknown.

### 2026-10-10 — FS-0 run 2 (Nucleus; Instance0 = keyboard+mouse, Instance1 = controller). Logs: `.claude/experiments/FS-0/run2_*`
Instance0 received the hotkeys (both of its copies 2→29 from 14:54:36). Instance1 received **no** key presses (Nucleus binds the keyboard to one
instance), so both of its copies stayed 2. Observed: the Instance0 player could lock on to and damage the other player; the Instance1 player could
not target or aggro the Instance0 player. **Conclusion: hostility is evaluated on the attacker's client from its own copies.** Confirmed by Leonardo: the victim's HP dropped on the victim's own
screen, and the victim **died and respawned** (PvP kill does not break the session). Death detail: the victim went downed, the
revive bar appeared but no revive was possible, and the victim respawned near a site of grace. Rune/level loss to re-check; the killer gets no loot. Follow-up: `auto_team_all = 29` config so keyboard-less instances apply it themselves.

### 2026-10-09 — FS-0 (Nucleus Co-op, 2 instances on Leonardo's PC, Goldberg-emulated Steam + SC v1.1.3 + nrpvp with F11 = all players → 29): **PASS (reported)**
The two players could fight each other **and** enemies (PvPvE) in the same expedition. Per-step matrix details (which client must hold the
flag) were not recorded, and the logs were lost because Nucleus deletes the instance folders on close (fixed by T-018). The overlay did not render under Nucleus (hooks applied, no frames); hotkeys worked.
Pinned issue: the Nucleus handler's `dlllist.txt` must not end with a newline, or Nucleus fails with WaitForSingleObject 258.

### 2026-10-09 — TC-004 run 1 (Leonardo solo, SC profile)
Only value **29** was tested (the instructions wrongly said to press F4 first; selection starts at 6). Write and restore worked; no visible
change with 29 (expected: enemies were already hostile). Value 6 is untested. F4 triggers SC's "Paused". Local team = 2 in the expedition.

### 2026-10-09 — TC-004 run 2 (Leonardo solo): **PASS, core**
F5 2→6: no lock-on, enemies ignore the player and don't hit, the player deals no damage. F6 6→2: normal. No reverts in 25 s.
Still open: persistence through death/revive, grace, night transition, day change (TC-004 steps 4–5), to be done with the T-013 overlay.

(Newest first. Use the header format above.)

### 2026-10-09 — TC-000 (stock SC launcher) and T-004 (me3 sc profile), Leonardo solo
Pins: game 1.03.3 (1.3.3.0), SC v1.1.3 (in-game text; = Nexus "1.13"), me3 0.11.0, nrpvp 0.1.0.
- **TC-000:** SC active. Choosing the "Up to 6 players" expedition while alone fails with a "failed to organize party"-style
  error. Classified `SC-LIMITATION` [HYPOTHESIS: the multiplayer option needs &ge;2 lobby members]. "Solo Mission" not yet tried.
- **T-004:** identical SC behaviour via `launch.ps1` (sc profile). me3 log: nrsc.dll in `early_natives`, "loaded native
  with me2 compatibility shim"; nrpvp.dll loaded after it (`dist/nrpvp.log` 17:55Z, FileVersion 1.3.3.0); `NR0000.co2`
  written. **SC under me3 on NR: VERIFIED.**
- Side finding: the T-001 `dev` profile (no SC) wrote the **vanilla** `NR0000.sl2` (14:45). Offline only, but it should
  use a separate save file. Follow-up in T-004b.

## 4. Known bugs

_None yet._

## 5. Regression set

Rerun after any game, SC, me3, or nrpvp update: TC-001, plus TC-003 row 3 once it passes.

---

## 6. Friend Session runbooks

### FS-1 — First two-player session (≈90 min). **Status: final draft. Ready once T-015 and T-016 are done.**
**Before the session (friend, ~15 min, async):** follow `README-FRIEND.md` in the friend kit: install me3 0.11.0 + SC v1.1.3, back up saves,
run `launch-friend.ps1 -DryRun` and send Leonardo the printed nrsc.dll hash. Leonardo: rebuild, run `scripts/package-friend.ps1`, send the zip.
**Session (voice call; Leonardo hosts):**
1. Both launch (Leonardo `scripts\launch.ps1`, friend `launch-friend.ps1`). Check the overlays show; both press MARK "session start". (5 min)
2. **TC-001:** A hosts "Up to 6 players", B joins, both start the expedition. Check that both see each other and enemies, and fight enemies together for 5 min. (15 min)
   If B can't join via me3, B retries with the stock `nrsc_launcher.exe` (no nrpvp), and we do TC-001 + TC-002 only.
3. **Panel check:** each overlay should list 2 players. Each person confirms which row is LOCAL (use "this is me" if UNKNOWN). Note both team values. (5 min)
4. **TC-003** rows 0–4 + row 4 tail. (45 min)
5. Quit. Both zip their kit or `dist` `logs\` folder; Leonardo copies both into `.claude/experiments/FS-1/` (`host/`, `client/`). (5 min)
**Stop rules:** a crash or disconnect → note the time and MARK, relaunch, continue from the same row. Two crashes on the same row → skip it and move on.
