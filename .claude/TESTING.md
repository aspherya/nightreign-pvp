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

### 2026-10-19 — FS-MM2 step 1 (Leonardo + SamukaSamukando, real Steam, mmtest zip 2026-10-14 19:46). Logs: `.claude/experiments/FS-MM2/{host,client}`
Game 1.03.3, SC 1.1.3, me3 0.11.0. **FAIL** (step 1, automatic join). Queue and host election: PASS in under 1 s, both tries. Host's mod
created the SC lobby and SC adopted it (+0.0 s), both tries. The joiner's link and invite reached the game, which entered and left the
lobby at once, both tries; manual accept did the same. The friend got into the Steam lobby only with the dev "Direct" button, which showed
`ALL PRESENT`, but his game stayed at the Roundtable while Leonardo started the expedition alone; Leonardo's world ended 8 s later and his log
stops at 17:27:32Z. Steps 2–3 not run. Analysis and decision: S9 §5f (the lobby must be made by the game; MM-3f).

### 2026-10-19 — MM-3f solo check (Leonardo, build 121749e dirty, nrpvp.dll E3F8E7E8E9532E87). Log: `.claude/experiments/MM-3f/2026-10-19_quiet_invite_solo.log`
Game 1.03.3, SC 1.1.3, me3 0.11.0. **PASS.** Test button armed DR0/DR1 on 137 threads (0 failed). Invite Members: the game created its own
lobby (`inviteParam = 0`, `nrsc_lobby_type`, adopted) and the invite-dialog call from `nightreign.exe+0x258568B` was skipped 0.4 s later; no Steam
window, no hang. Disarmed after 60 s on 133 threads. Not done: pressing Invite Members again after disarm (Leonardo quit right after).

### 2026-10-19 — FS-MM3 (Leonardo host on MM-3f build E3F8E7E8E9532E87; SamukaSamukando joiner on the 2026-10-14 mmtest build). Logs: `.claude/experiments/FS-MM3/{host,client}`
Game 1.03.3, SC 1.1.3, me3 0.11.0. **PASS (Solo, one host click).** Both pressed Queue Solo; match in 4 s. Host line asked for Invite Members;
one press: game lobby (`inviteParam`), Steam window skipped, mod invite sent. The joiner's mod opened the link; his game entered the lobby 1.9 s
later with no clicks (Leonardo: "only I had to click invite members"). Joiner in the host's world 6 s later (`players=2`), then ALL PRESENT;
quiet invite disarmed on 137 threads. Expedition with both, rival write 2 -> 29 on both PCs, downed rule fired on rival hits. Back at the
Roundtable after ~10 min: host status back to idle by itself. Not covered: the joiner ran the old build, so the joiner-side "loading in" line
was not exercised (joiner join code is the same in both builds); friend's log ends at 18:13:45 while loading back.

### 2026-10-19 — MM-4 solo check (Leonardo, player zip, nrpvp.dll 5AA3CF20AF48B3A9). Log: `.claude/experiments/MM-4/2026-10-19_player_solo.log`
Game 1.03.3, SC 1.1.3, me3 0.11.0. **PASS (steps 1–4).** `mode: PLAYER`, only the Insert key bound, probe in quiet mode. Insert opened the panel;
Solo Battle Royale: queue lobby (Invisible, 6), `Searching for players... 1/6`; Leave: `Not in a queue`. Not run: step 5 (auto-leave on quit to
title, refusal at the title); check it in FS-MM4.

### 2026-10-19 — UI-0 step 5 text trace + step 6 table list (Leonardo, solo, dev build, me3 profiles sc / ui0a / ui0b). Logs: `.claude/experiments/UI-0/step5-nrpvp.log`, `dist
rpvp.log` (ui0b)
Game 1.03.3, SC 1.1.3, me3 0.11.0. **PASS.** Step 5: 234 text ids traced (SC hooks `LookupEntry`), flags read right (6001 ON, 7500 ON at
DAY I). Step 6a: recompiled table script through an me3 package, normal menu, Solo Mission loaded. Step 6b: list "Expedition" / "Battle
Royale (test)" at the table, each opens the stock menu, flag 10007000 ON then off. Details: `research/UI-0-ingame-ui.md` §9–10.

### 2026-10-10 — T-029/T-030 player zip, Leonardo solo: **PASS**
Installed from `dist\NightreignPvP.zip` by the txt guide; `nrpvp_launcher.exe` started the game (bundled me3 with `-e`); the BATTLE ROYALE badge
showed top-right and changed to "Team 1 - 0 rivals" in a Solo Mission; sites of grace and item pickups work (local player stays on team 2).
Two-player check pending (Nucleus next, friend later).

### 2026-10-11 — FS-0 run 5 (Nucleus, 2 instances, dev build with scheme "rivals", team_size 1, probes on). Logs: `.claude/experiments/FS-0/run5_*`
**PvP: PASS.** No keys pressed. Each client wrote 29 once, only to its copy of the other player (`pvpve team write who=REMOTE[1] … 2 -> 29`,
write #1 on both); local players stayed on 2. Reported: both could fight and down each other; sites of grace and levelling work.
**Defect vs scope:** rivals can **revive** each other (attacking a downed rival fills the revive gauge). Under the old "all 29" scheme they could not.
**Log findings:** per-player ids are session slot numbers (0x6001, 0x6002, and 0x6003 after a rejoin), so they are not always 1..N but still order
consistently; ChrSet entry byte +9 went 04→02→04 for ~5 s for one player near the end, seen on both clients (candidate "downed" flag, unconfirmed);
`CSEventFlagMan` ER-layout structural checks ALL PASS; position probe: **ER physics chain does not hold on NR** (modules pointer at +0x190 is
readable but +0x68 is not); floats that look like coordinates sit around PlayerIns+0x6C0..0x6D8. No Roundtable freeze.

### 2026-10-11 — FS-0 run 6 (Nucleus, T-031 build). Logs: `.claude/experiments/FS-0/run6_*`
**Downed experiment: not exercised.** F8 cycled 0→6→14→30→2 on both clients, but no `downed rival` line was ever logged: the trigger (ChrSet
entry byte +9 == 2) never fired during several downs and revives. **Entry byte +9 is not the downed marker** (retracts the earlier candidate).
Reported: rivals could still down, kill, and revive each other with every value (expected, since nothing was written).
**Position scan: useful.** Smooth f32 triples while moving (incl. the hawk flight): `PlayerIns+0x6C0` (x,y,z,1.0) and a second vector at
`+0x6D0`; `[PlayerIns+0x60]+0x1EC` (pointee vtable exe+0x2C0A898); `[PlayerIns+0x6B0]+0x17C` and `+0x18C` (vtable exe+0x2C8FD20);
`[PlayerIns+0x5F0]+0x64`. Which one is authoritative (moves the player when written) is UNKNOWN → T-032.

### 2026-10-11 — FS-0 run 7 (Nucleus, T-032 MARK dumps; Part 1 only). Logs: `.claude/experiments/FS-0/run7_*`
Sequence: both standing (marks 1–3 on Instance0), player 0x6001 downed by 0x6002 (mark 4), revived (5), downed again (6), bled out / respawned (7, 8).
Diffing the PlayerIns dumps of the victim, on its own client and on the rival's client:
- **Downed marker: `PlayerIns+0x710` == 1** while downed, 0 otherwise; also `+0x1F9` bit 4 (0x10). Identical on both clients for both downs.
- **Down counter: `PlayerIns+0x730`** = 0, then 1 after the first down, 2 after the second; same on both clients.
- **team_type does not change when downed** (stays 2 locally / 29 on the rival's copy).
- **Last-hit-by: `PlayerIns+0x1A0`** (and `+0x198`, which clears later) holds the attacker's handle, i.e. the 8 bytes at the attacker's `ChrIns+0x08`
  (host `0xFFFFFFFF10C00000`, joiner `0xFFFFFFFF10C00001`). Seen on the victim's own client only (0xFF… / 0 on the rival's copy).
- **PlayerGameData: `PlayerIns+0x5A0`** → object with RTTI class `CS::PlayerGameData` (vtable exe+0x2BE6E28); **HP = u32 at +0x10** (240 standing,
  0 downed, 120 right after a revive = 50%); mirrored at `PlayerIns+0xBA0`. Same offset as ER's PlayerGameData.
- ChrSet entry byte +9 stayed 04 throughout (confirms it is not a downed marker).
Part 2 (snap-back position test) not run yet.

### 2026-10-11 — FS-0 run 8 (Nucleus, T-033 build). Logs: `.claude/experiments/FS-0/run8_*`
**Tracker: PASS.** DOWN/UP transitions for 0x6002 logged on both clients at matching times (downs 1, 2, 3; hp 0 / 120); the victim's client
classified the last attacker as RIVAL id 0x6001 (handle 0xFFFFFFFF10C00001) every time.
**Finisher (hp0): fired once, no effect.** `T-033 FINISHER hp 120 -> 0 (readback 120)`; +100 ms…+2 s still hp=120, downed=0. So
`PlayerGameData+0x10` is a **mirror** of HP that the game rewrites, not the authoritative value. Other places that held the same HP pattern in run 7:
`PlayerIns+0xBA0` and `[PlayerIns+0xB78]+0x70`.
**Timer scan:** no countdown found in PlayerIns or its one-level pointees over a 26 s downed window (only up-counters: +1.0/s floats at
`[PlayerIns+0x5A8]+0x1C8` and `[PlayerIns+0x760]+0xE4`; a frame counter at `PlayerIns+0x560`). Snap-back test not run.

### 2026-10-11 — FS-0 run 9 (Nucleus, T-034 build). Logs: `.claude/experiments/FS-0/run9_*`
**Authoritative HP found: `[PlayerIns+0xB78]+0x70` (u32).** Writing 0 there on a rival-completed revive held (readback 0); `PlayerIns+0xBA0`
and `PlayerGameData+0x10` already read 0 a moment later (mirrors). **Effect: the player is downed again at once** (downs stays capped at 3),
not killed; reported by Leonardo as "got up but got downed again instantly". Fired twice (04:57:46, 04:58:11), same result.
**Death signature:** leaving the downed state with hp=0, then "HP restored" to full ~6 s later = bled out and respawned (04:56:57 after 16.9 s
untouched; 04:58:28, 15.6 s after a re-down). A revive leaves with hp = 50%. **Each re-down restarts the bleed-out clock** (~13–17 s).
**Wide timer scan: failed for performance** (passes took 520–920 ms instead of 250 ms, ≤ 10 samples per window, no ranking; it also delayed the
main loop). Bleed-out timer still unlocated.

### 2026-10-12 — FS-0 run 10 (Nucleus, external scanner `nrscan clock`). Output: `.claude/experiments/FS-0/nrscan/run10_clock_instance0.out`
Tool fixes during the session: a full pass over ~7 GB took 16 s (too slow), and regions > 1 GB were skipped although the game heap holding
PlayerIns is one ~1.1–1.2 GB private RW region. With `--near-mb 256` (653 MB around the PlayerIns addresses) pass 1 took 0.70 s.
**Result (one untouched bleed-out, Instance0 LOCAL, 15.2 s scan window):** 8,200,440 candidates → 2 survivors, both f32 counting **up** at
1.0/s: 2.815 at +0.71 s … 16.962 at +14.78 s (so ≈ 0 about 2 s before the scan started, ≈ 17.4 at death). Addresses that run:
0x7FF43A4444E4 and 0x7FF43A445BDC (0x16F8 apart, values a constant 0.019 apart). The first lies at +0x2B4 of an object at 0x7FF43A444230 that
many pointers reference; no path from PlayerIns or a named singleton was found. **Unknown whether it is the bleed-out clock or a co-running
timer (animation, HUD).** Next: write test (add seconds while downed, measure the time of death).

### 2026-10-12 — FS-0 runs 11–13 (Nucleus, `nrscan clock`, read-only). Outputs: `.claude/experiments/FS-0/nrscan/run11`, `run12`
- Run 12 (one untouched bleed-out, f32 mode, 1028 MB near the player, 15.9 s): 87 survivors, all f32 counting up at 1.0/s but already at 9–19
  when the down happened; classes seen on holders: `CS::CSPhysSysIns` → `CS::HitIns`. **No value started at the down.** The count-up pair from
  run 10 did not reappear, so it was not the bleed-out clock (retracted).
- Run 13 (one untouched bleed-out, u32 mode, range 1–60000, scales ×1/×30/×60/×1000): 32,272,345 candidates → 0 after 6.3 s.
- Run 11 (several downs with hits): inconclusive (the filter drops any value that pauses).
**Conclusion: the bleed-out clock is not an f32 or u32 that changes at 1/s, 30/s, 60/s or 1000/s within ~256 MB of PlayerIns.** Not searched:
a normalised fraction (1 → 0 over ~15 s), other rates, memory farther from the player. Search stopped at Leonardo's request (no new tooling).

### 2026-10-12 — FS-0 run 14 (Nucleus, live reads and writes with `nrscan peek`/`poke`). Outputs: `.claude/experiments/FS-0/nrscan/run14`
**The near-death block is in PlayerIns, next to the downed flag** (first seen in the run-7 MARK dumps; confirmed live on Instance0):
- `PlayerIns+0x710` u8 state: 0 = normal, **1 = downed**, 2 = getting up after a revive (≈3.5 s).
- `PlayerIns+0x714` f32 **revive bar, amount left**: 40 / 90 / 240 at the 1st / 2nd / 3rd down; each hit subtracts (240 → 229 after one hit); 0 → revived.
- `PlayerIns+0x718` f32 **bleed-out clock, seconds left**: starts ≈ 14.9, counts down in real time; **death exactly at 0** (watched 14.21 → 0.02 → dead).
- `PlayerIns+0x71C` f32 **pause timer**: set to ≈ 3.4–3.5 on a hit and counts down; while > 0 the clock does not move.
- `PlayerIns+0x720` f32: down-level decay timer (−1 before the first down; 180 → counts down while up; frozen while downed).
- `PlayerIns+0x730` = number of downs (already known).
**Clock write: works.** At 16:26:01 the clock was set from 9.97 to 2.0 (`poke`, readback 2.0); it kept counting (1.65, 1.00, 0.20) and the
player died at 16:26:04.9, 8 s after the down instead of ≈15.
Not yet tested: writing the bar and the pause timer (same block, same kind of write). A first attempt wrote to +0x728/+0x72C by mistake
(two 0xFFFFFFFF words; the game had reset them by the next check; no effect seen).
Why the scans missed it: the clock pauses on hits and the bar counts **down** from its start value, not up from zero.

### 2026-10-11 — FS-0 run 15 (Nucleus, 2 instances, T-039 downed-rule build of 13:33, team_size 1). Logs: `.claude/experiments/FS-0/run15_*`: **PARTIAL**
- PvP, downing and finishing a downed rival work. Leonardo: some rival hits still moved the purple bar and a rival could sometimes revive.
- Log evidence: the rule fired (`rule: rival hit on downed LOCAL`, 20 times on Instance0), but late. Heartbeats show the main loop taking
  2–3 s per pass with 2 players in an expedition (`slowest tick` 1928–3085 ms; 12–22 ms at the title screen), not the intended 50 ms.
  Several hits landed between two passes: `bar 3 -> 40 restored`, `bar 12 -> 90 restored`. On each instance's first down (bar 40) the
  bar was completed before the next pass; rule v1 then downed the player again 5–8 s later (it only sees the revive after the 3.5 s
  get-up, state 2 → 0).
- The writes themselves work: bar restore sticks, clock −3 s per hit, pause cleared, death after 4 hits. [VERIFIED]
- Cause of the slow loop: [HYPOTHESIS] each `VirtualQuery`-guarded read costs about 10 ms inside the game's heap (roughly 0.35 s for the
  ~25 reads between the combat tick and the heartbeat line). The loop has been this slow in every Nucleus log since 2026-10-11 03:03.
- Fix built 2026-10-12 01:32 local: main-loop reads use `ReadProcessMemory` on our own process (`mem::read`, `RegionCache::direct`);
  the heartbeat line now also shows where the slowest pass went (`team / combat / overlay`). Not yet run in the game.

### 2026-10-12 — FS-0 runs 16–18 (Nucleus, 2 instances, team_size 1; builds of 01:32 and 01:57). Logs: `.claude/experiments/FS-0/run18_*` (16 and 17 not kept)
- Run 16 (01:32 build, handler still had `probes = true`): team/combat/overlay parts of the loop 0 ms, whole pass still 0.4–1 s. The
  probes were the rest. Confirms the `VirtualQuery` cost. [VERIFIED]
- Run 17 (`probes = false`): pass time 1–3 ms (median), max 95 ms. Every rival hit undone within 50 ms on the owner's game.
  Leonardo: only the FIRST hit on a downed player still shows on the gauge.
- Run 18 (01:57 build with `T-039 BLOCK` lines): Leonardo: it shows on the **attacker's** screen only. The log explains it: the
  remote copy of the downed player holds the same block (+0x710/+0x714/+0x718/+0x71C) and is refreshed only when a hit lands, with
  the owner's values at that moment (`bar 40 -> 3 (clock 12.677819)` on the copy; the owner logged the same clock). It never
  receives the owner's restore, so it stays at the first hit's value; later hits bring nearly the same value. [VERIFIED]
- Also in run 18: one hit removed the whole first-down bar (40 → 0, state 2 at 05:04:28.9). Rule v1 downed the player again 3.7 s
  later (at state 2 → 0, hp 120). The single-hit gap is real at this damage level (hits removed 37–38 of 40).
- Fix built after run 18: each game also restores bar/clock/pause on its copy of a downed rival that has no teammate in the session
  (`Core::remote_step`). Not yet run in the game.

### 2026-10-12 — FS-0 run 19 (Nucleus, 2 instances, team_size 1, 02:10 build). Logs: `.claude/experiments/FS-0/run19_*`: **PASS**
- Attacker-side gauge fix works: `rule: rival hit on downed REMOTE[1] (display copy)` for every hit on both instances, same values as
  the owner's line. Leonardo: only a brief flicker on both screens. Pass time 2–5 ms median. [VERIFIED]
- The single-hit gap again, same place as run 18: first down (bar 40), hits removed 37, 38, then the third hit of the chain removed
  all 40 (state 1 → 2 at 05:15:38.3). Rule v1 downed the player again at 05:15:42.0 (hp 120 → 0), 3.7 s later. Leonardo: "revived
  but died right after". Reproducible with that weapon's third hit.
- Built after run 19, Nucleus only (the player zip stays on the 02:10 build): `Core::getup_step` makes rule v1's store as soon as
  the get-up starts (state 2) when the last attacker is a rival. EXPERIMENT, effect in the game UNKNOWN.

### 2026-10-12 — FS-0 run 20 (Nucleus, 2 instances, team_size 1, 02:20 build with the early re-down on). Logs: `.claude/experiments/FS-0/run20_*`: **PASS (rule), early re-down not exercised**
- Six downs, every rival hit undone on both instances, death after 3–4 hits; one untouched down bled out in 15 s. Pass time 1 ms median.
- No hit completed the bar this time, so `getup_step` never ran. Its effect in the game is still UNKNOWN; it was switched off
  (`early_redown = false`) in the final build of 02:31, which is what Nucleus, `dist\nrpvp.dll` and the player zip now hold.
- Leonardo accepts the current behaviour: a brief gauge flicker per hit, and a player who gets up after a full-bar hit is downed again.

### 2026-10-12 — FS-0 run 21 (Nucleus, **3 instances, team_size 2**, STATUS Task C). Logs: `.claude/experiments/FS-0/run21_*`: **PASS**
Pins: game 1.3.3.0, SC v1.1.3, Nucleus/Goldberg transport. Leonardo's report: "worked exactly how we wanted": teammates revive downed
friends, rivals finish rivals. The logs agree:
- **Teams agreed on all three games.** Expedition ids: Instance2 = `0x6001`, Instance0 = `0x6002`, Instance1 = `0x6003`. Team A = 6001 + 6002,
  team B = 6003 alone. Team writes: Instance0 and Instance2 each marked one player (6003), Instance1 marked two. Nobody marked a teammate.
- **Teammate revive, 3 of 3:** 12:51:48 (6002 down, bar 40 → 20 → 0, up at 110 hp), 12:53:31 (6001 down, 40 → 26 → 12 → 0, up at 120 hp),
  12:56:46 (6002 down, 40 → 20 → 0). The rule left every teammate hit alone.
- **Rival finish, 4 of 4 with hits:** every rival hit was undone and took 3 s off the clock (15 `rule:` lines on the victims' games, the
  same hits mirrored on the attackers' copies). Deaths 5.9–8.5 s after the down instead of 15 s. Two untouched downs bled out in 15 s.
- **Both halves in one down (12:57:22, 6002 downed, bar 90):** two hits of 20 stayed on the bar (90 → 70, 82.8 → 62.8) while three hits of
  25–26 were undone and cost 3 s each; the player died at 12:57:31. `[HYPOTHESIS]` the kept hits were the teammate's: the rule treats every
  hit not attributed to a rival that way, and 20 is the hit size 6001 showed elsewhere in the run (6003 hits for 25).
- **Revive and finish in one fight (12:56:46–54):** 6001 revived teammate 6002 (2 hits) and then killed downed rival 6003 (4 hits).
- **Attacker's gauge:** `(display copy)` lines ran on Instance0 and Instance2 for downed 6003 (a rival with no teammate). Instance1 has
  none for downed 6001/6002, as designed (T-039 follow-up 3); what Instance1's screen showed there was not reported.
- **Player ids change between the lobby and the expedition:** before the expedition the two joiners were `0x6003` and `0x6004`; in the
  expedition the three players were `0x6001`, `0x6002`, `0x6003`. No team write happened before the expedition (first write 12:51:08).
- **Bar grows back:** on the owner's game the bar rose about 9 per second between hits while the pause timer was 0 (70 → 78.2 in 0.9 s,
  78.7 → 82.8 in 0.5 s). First seen here.
- Log wording: a death is logged as `UP (hp=0)` followed by `HP restored` about 7.4 s later; a revive is `UP (hp=110)`.
- Pass time: usually 0–2 ms, but the combat step peaked at 63–225 ms in the 10 s windows that contain rival hits (all three instances).
  Cause UNKNOWN (three games on one PC; run 20 with two games had none). The rule still reacted on every hit.
- Warnings: one 180 s WorldChrMan timeout per game while sitting in the menus, and one `ChrSet capacity 0` per area change. Both harmless.
- Not shown by the logs: that teammates cannot damage each other (nothing logs damage).

### 2026-10-14 — FS-MM1 step 2 only (real Steam, 2 PCs: Leonardo host, fuso joiner; mmtest zip of 2026-10-12 12:51). Logs: `.claude/experiments/FS-MM1/{host,client}`: **PASS (Solo, with two manual steps)**
Game 1.03.3 (exe 1.3.3.0), Seamless Co-op v1.1.3, bundled me3 0.11.0. Baseline, Duo and the join-method buttons were not run.
- **Queue Solo: PASS.** Both pressed Queue Solo; one queue lobby, same host and roster on both PCs, four matches in a row (either player as host).
- **Gate: PASS with two human steps.** The host had to open Matchmaking Settings > Invite Members (opening it was enough: lobby A appeared and the
  host's mod invited fuso by itself), and fuso had to accept the Steam invite. ALL PRESENT on both PCs 36 s after MATCH READY.
- **Expedition as rivals: PASS.** Teams 0/1 on both PCs, remote written 2 → 29, downed rule fired 7 times with identical values on both PCs.
- fuso pressed "(a) link" once while already in the lobby: Steam delivered it to the running game (left and re-entered lobby A in 1.6 s); no second copy.
- Defects: status stuck on `IN EXPEDITION` after the expedition ended; false `not trusting its sc_lobby` warning on the joiner at ALL PRESENT.
  Fix card MM-3a. Full findings: `research/S9-matchmaking-decision.md` §5c.

### 2026-10-20 — SMOKE-1: branch `eng/r0-r1-console` (Leonardo solo + Claude over nrcon, dev build 2026-10-20 20:43, `scripts\launch.ps1`, game 1.03.3, SC 1.1.3, me3 0.11.0). Log: `dist\nrpvp.log` (23:47–23:53 UTC): **PASS**
- **Console (CON-1/CON-2), driven by Claude from a terminal:** `nrcon list` found the game (pid 10332); `status`, `players`,
  `mm_status`, `echo` answered. `team_set 6` → `players` showed 6 → `team_restore` → 1, both writes with readback in the log
  (`T-008: console team write … 1 -> 6 (readback 6)`, `console restore … 6 -> 1`). `team_set 29 p2` with one player: clear error.
  Asked 20 s after launch, `status` said "no world" while the Roundtable was still loading (WorldChrMan live at 23:47:50); the
  local player appeared 2 s after the world, team 1 as in earlier runs.
- **No regression from R0/R1 (Leonardo, on screen + log):** table menu shows Solo / Duo / Trio, Commence started the Solo queue with
  the searching message, Stop Searching left it (`br_menu: Commence -> stop searching`, `mm_queue: state InQueue -> Idle`). Debug
  registers armed on 132 threads, 0 failed; main-loop ticks 4–267 ms (the 267 ms around unloading).
- Follow-up done the same day: two reply wordings fixed ("rivals rivals", "Session(2)" → "p2"), commit `2dadcb2`.
- Not covered: the R1 player DLL (`--no-default-features`) in game; it gets its own check before the next player zip.

### 2026-10-21 — SMOKE-2: cleanup branch `eng/cleanup-r5-r2-r3` (Leonardo + Claude over nrcon; dev build 2026-10-21 08:00, nrpvp.dll 66824444A97739C5; game 1.03.3, SC 1.1.3, me3 0.11.0). Logs: `dist\nrpvp.log`, `dist\nucleus-logs\Instance{0,1}\`: **PASS**
- **Part A, solo (`scripts\launch.ps1`), Roundtable:** console `list/status/players/team_set 6/team_restore/pos_next/log` answered on
  2026-10-20 (build before the fresh-read fix) and again on 2026-10-21; menu Solo/Duo/Trio, Commence → searching, Stop → idle (log
  `br_menu: Commence -> stop searching`, `mm_queue: state InQueue -> Idle`). Found and fixed: a chained `team_set 6; players` showed the
  value from before the write (players list cached 100 ms); with the fix `players` showed 6 at once (2026-10-21).
- **Step 3 (solo expedition) not run, by design:** with `br_menu` on, Commence starts the Solo queue (it waited at `solo 1/2 searching`),
  so no solo expedition can start; in a solo expedition there are no rivals anyway. Left with `mm_leave`; value restored 6 -> 1.
- **Quit: no crash** (Leonardo). First clean quit noted since 2026-10-10 (0xC0000374), and the first build without the overlay. One
  run is not proof: watch the next quits (STATUS §1 known bug).
- **Part B, Nucleus 2 instances (deploy with `mm_queue = false`, `br_menu = false`):** `nrcon --all status` showed both games, each
  keeping itself on 2 and marking the other 29 (`rivals marked 29, 1 writes` on each). Downed rule: Instance0 (p2) downed by RIVAL p1 at
  11:16:14 (bar 40, clock 14.98); two rival hits reached the rule: `bar 3 -> 40 restored, clock 7.77 -> 4.77` and `bar 2 -> 40
  restored, clock 3.69 -> 0.69`; death at clock 0 (11:16:23). Instance1's display copy logged the same values at the same times.
  Leonardo reported 3 hits and that the bar never filled; the logs show 2 hits reaching the rule (the third likely landed as the clock
  ran out). 0 errors / panics; ticks 1–4 ms, peaks 62 / 114 ms at the hits (the known run-21 spike, not new).
- **Task D (snap-back) skipped** (Leonardo): still open, console `pos_save` / `pos_go` / `pos_next` ready.

### 2026-10-13 — T-028 run 1 (solo, `scripts\launch.ps1`, snap-back test with `Physics+0x70`). Log: `.claude/experiments/T-028/run1_solo_teleport.log`
Game 1.03.3, Seamless Co-op v1.1.3, mod build 2026-10-13 22:30 (branch `t-028-snapback`). One save, nine writes, first candidate only.
**Position write moves the player: PASS for x and z** (within 2 cm, never snapped back). **Height: FAIL** (1.8 m to 7.7 m below the saved
height; after write 9 the player fell out of the world). Leonardo: "it kinda worked, with some bugs". Cause found in runs 2-3.

### 2026-10-14 — T-028 runs 2 and 3 (solo). Logs: `.claude/experiments/T-028/run2_solo_teleport.log`, `run3_solo_walk.log`
Run 2: grass worked; rock → grass and cliff top arrived beside the saved spot and fell. The physics position is measured from an origin
that moves. Run 3 (walk, no teleport): the origin is `PlayerIns+0x80` (`re/MEMORY_MAP.md` §8).

### 2026-10-14 — T-028 run 4 (solo, origin-aware teleport, build 173f4fa). Log: `.claude/experiments/T-028/run4_solo_teleport.log`: **PASS**
Leonardo: "it worked". Four save/teleport pairs, including a cliff top from 16 m below and saves 40–250 m apart. Height held within 0.6 m
2 s after every write. The log also holds the full vanilla hawk ride (MEMORY_MAP §8).

### 2026-10-14 — T-028 two screens (Nucleus, same build): **PASS (reported by Leonardo, logs not kept)**
The other player saw the jump; both screens agreed; both could still hit each other afterwards.

### 2026-10-21 — FS-UI2: Battle Royale from the table, player zip, two PCs (Leonardo "leo" + friend "Samuka Games"; zip 71f893d, nrpvp.dll F6B1C1FE9D813141; game 1.03.3, SC 1.1.3, me3 0.11.0). Logs: `.claude/experiments/FS-UI2/{host,client}` (host = Leonardo's PC, client = the friend's): **PASS (logs; on-screen answers pending)**
- Same build and config on both (`mode: PLAYER`, `matchmaking=true mm_max_players=2 mm_min_players=2 team_size=1`, overlay off). 0 ERROR /
  panic. Heartbeat worst passes 74 ms (combat step, in expeditions), otherwise 0–17 ms. The friend's clock is ~16 s ahead of Leonardo's.
- **Step 0 (labels, FS-UI1):** both: `our labels found in CL_MenuText_dlc01 … Number of Players shows Solo / Duo / Trio (hwbp slot 3)`;
  SC lobby and Roundtable worked with our package on both PCs. On-screen check of the labels: not reported yet.
- **Step 2/3/4, three full rounds, both directions:** round 1 18:18–18:27 (Leonardo host), round 2 18:28–18:33 (friend host), round 3
  18:33–18:37 (Leonardo host). Each: `MATCH READY` under 1 s after the second Commence → host `invite dialog suppressed` (quiet invite,
  mask 0b1111) + `method (b) InviteUserToLobby -> true` → joiner `method (a) link` → `ALL PRESENT 2/2` 8–9 s after the invite → both
  `local player is in an expedition (team_type 2)` within ~1 s → rivals (heartbeat `team 29, n writes`), downed rule fired (`rule: rival
  hit on downed LOCAL …` 7× on Leonardo, 2× on the friend) → back at the Roundtable `state MatchReady -> Idle`, next Commence queued again.
- **Hypothesis (host Commence switches byte 2 → 1): not exercised.** No `br_menu: Commence pressed` at match ready in any round; the
  expedition started for both anyway. So the start with a SC party needs nothing from us (whether it bypasses `exe+0x9EEBFB` or hit it
  with an unreadable settings pointer: UNKNOWN). Fallbacks (a)/(b) not needed.
- **Messages (UI-2):** shown on both: searching (0), "(1 of 2)" (20, repeating every 6 s), host Invite Members (5), waiting for players
  (6), waiting for the host (7), loading in (9), everyone is here host/joiner (10/11). **Never shown:** "match found!" (4) and "joining
  the host…" (8): the next state arrived inside the 6 s gap. "Everyone is here" kept repeating through the departure (`NOT picked up …
  not in the Roundtable?` 4–5× per round, harmless).
- Friend, before round 3: Commence twice with Number of Players byte 0 → "Duo and Trio coming soon" box (`decision code 0x120`), then
  Solo → queued. The byte was 0 after his round as host (the friend may have changed it; ask).
- **Steps 1 and 5 (Stop Searching) not run**: no `stop searching` line on either PC. Already PASS solo (SMOKE-2 Part A, UI-2 solo).
- **Defect seen:** `rule: rival completed the revive meter of LOCAL but nothing was written: [PlayerIns+0xB78] is not a plausible
  pointer` (Leonardo, round 3 18:36:24): the re-down did not happen once.
- **Crash 0xC0000374 (ntdll heap corruption) twice on Leonardo's PC** (Windows Application log 1000): 18:14:59 UTC and 18:37:14 UTC; both
  were Alt-F4 quits (Leonardo saw no crash), so Alt-F4 hits the quit crash too. This zip predates R1/R5 (hudhook still linked, overlay off by
  config), so it neither clears nor convicts the overlay; STATUS §1 known bug.

### 2026-10-21 — T-040 runs 1 and 2: rival revive re-down (Leonardo, Nucleus 2 instances, dev build, `team_size = 1`). Logs: `.claude/experiments/T-040/`: **PASS (run 2)**
- **Run 1 (21:52–21:59 UTC, build 18:33, retry only):** `revive_fail 3` on Instance0; Instance1 revived it. The retry ran as designed
  (`gave up after 40 tries in 2023 ms`), but every try after the forced ones failed the HP gate: `authoritative 4294967295, mirror
  1781645456, PlayerGameData 120`. **The revive stood: the FS-UI2 bug reproduced.** Live reads on both games: `[PlayerIns+0xB78]`
  pointed into a list of same-class objects, `+0x70` = 0xFFFFFFFF, and `+0xBA0` held half a pointer; the health was at
  `[[PlayerIns+0x1B8]+0x0]+0x140` (240 / 260, = PlayerGameData). Leonardo's `nrscan poke` of 0 there downed Instance0 at once
  (22:03:37, `STATE … DOWN`). Root cause: **the re-down wrote to an address that is not always the health** (worked in FS-0 runs
  9–19 and on 2026-10-21 00:54, not here); the single try was a second, smaller problem.
- **Run 2 (22:12–22:17 UTC, build 19:06, nrpvp.dll 5324D6D949A6A449: data-module route first, old route as fallback):** both directions
  re-downed on the first try: Instance1 22:16:05.2 and Instance0 22:16:46.5 `rule: rival completed the revive meter of LOCAL; downed
  again (hp 130 -> 0, readback 0)`, next `STATE … DOWN (downs=2)` within 0.1 s. Both then bled out under further rival hits
  (expected). 0 errors.
- Seen by Leonardo and expected (STATUS §4 item 1): when a rival's last hit empties the bar in one go, the game starts the get-up
  (~3.6 s, state 2) before the downed rule can undo it, so the player stands up and is then knocked down again.
  Instance1 22:16:01.5 logged it: `T-040 LOCAL bar read 0 while state 1 (before 40)`.
- **Run 3 (22:24–22:30 UTC, build 19:22, console `early_redown 1` on both): the early write does NOT shorten the get-up.** Instance0:
  the last rival hit emptied the bar (`bar read 0 while state 1`, 22:29:15.2), the mod set health 0 `0 ms in` to the get-up
  (22:29:15.4, `hp 100 -> 0, readback 0`), yet the game played the whole get-up and only then went `state 2 -> 1` (22:29:19.0, 3.6 s
  later, bar 90). Leonardo: "the rival gets up and then dies", same as without it. Instance1's down ended in a bleed-out (no get-up).
  Conclusion `[VERIFIED]`: once state 2 has started, an HP write cannot cut it; the get-up must be prevented before it starts.

### 2026-10-21 — T-041: revive-bar trace and the revive block (Leonardo, Nucleus 2 instances, dev build, `team_size = 1`). Logs: `.claude/experiments/T-041/`: **PASS (block v2, one direction)**
- **Trace (22:38–22:44 UTC, `bar_trace 1`):** no crash; writers of `PlayerIns+0x714`: `exe+0x675149` (every frame, bar regrowth),
  `exe+0x66DFCC` (a reviver's hit), `exe+0x66F5ED` (get-up start), `exe+0x67533C` / `exe+0x67521E` (end of get-up / bleed-out), plus our
  own rule (`nrpvp.dll`). Disassembly: the get-up is started by the byte `PlayerIns+0x732`, not by the bar (a get-up began with the bar
  at 40, 22:42:36). MEMORY_MAP §6.
- **Block v1 (23:00–23:01, clear `+0x732`):** blocked, but the game took its "neither flag" branch and stood the player up at once
  (`state 1 -> 0`, HP 0), then downed them again 3.7 s later. FAIL.
- **Block v2 (23:09–23:13, build a646288c59f2b1b0: jump to the game's own no-event exit `exe+0x67539F`):** Instance0 downed by the
  rival; the hit at 23:11:37.0 emptied the bar; from 23:11:37.8 the revive event arrived every frame for 3.6 s (131 frames dropped,
  all from the rival's handle) and **state stayed 1**; the player bled out at 23:11:46. **Leonardo: no stand-up on either screen**
  (the rival's screen also showed the player staying down). Other downs: Instance1's bar never emptied (bled out, nothing to block);
  an enemy down (attacker not a player) was left alone.
- **Hit hook (2026-10-22 00:14–00:15 UTC, build 745ECEF467581CC3: breakpoint on the bar store `exe+0x66DFC4`, a rival's hit keeps
  the current bar): PASS, both directions.** Instance0 downed: 3 rival hits `bar stays 40, clock 14.45 -> 11.45 … 4.96 -> 1.96,
  pause … -> 0`, bled out at 00:14:47.6 still on its first down. Instance1 downed: 4 hits, same, bled out 00:15:24.5. No revive
  event reached the game (`0 rival revive frame(s) dropped`), no extra down. **Leonardo: "it worked very well"**: no stand-up on
  either screen, and the near-death gauge no longer flickers on rival hits (the old poll-based restore caused it).
- **Teams, 3 instances, `team_size = 2` (2026-10-22 00:34–00:40 UTC, same build): PASS.** Teams agreed on all three games: 6001 + 6002
  vs 6003. Rival downed (6003, Instance0): 4 rival hits cancelled (`bar stays 40`), bled out at 00:37:14. **Teammate revives work:**
  6001 (Instance2) revived by ally 6002 at 00:38:06 (`revive_block: allowed a revive of LOCAL by a non-rival`, up at 120 HP); 6002
  (Instance1) revived by ally 6001 at 00:40:21 (up at 130 HP). **Both in one down** (6001, second down, 00:39:02): the teammate's hits
  lowered the bar normally (75.9, 65, 72.7 between regrowth) while 3 rival hits were cancelled and cost 3 s each; died at 00:39:15.
  Leonardo: "it worked". T-040 and T-041 accepted.
- Superseded (block v2 entries above): the downs counter rose to 2
  (bar 90) during the blocked window without a new down on screen (source UNKNOWN; probably the rival's game, which believed the
  player was getting up).

### 2026-10-21 — SMOKE-3: teleport on `main` (Leonardo solo + Claude over nrcon; `scripts\launch.ps1`, dev build `main` d3d1eb8, nrpvp.dll 8036B4B61971B2E4; game 1.03.3, SC 1.1.3, me3 0.11.0; `br_menu = false` in `dist\nrpvp.toml` so Commence starts a vanilla solo expedition). Log: `.claude/experiments/SMOKE-3/nrpvp_2026-10-22_01-00-43.log`: **PASS**
- **Jump 1 (flat ground, ~33 m away):** saved `Physics+0x70=(17.105,91.550,22.102)` origin (80,0,-272) at 01:02:28; `pos_go` at 01:02:41
  wrote `(-14.895,91.550,14.102)` (the origin had moved by (32,0,8): same world spot), readback equal, unchanged at +100 ms / +500 ms /
  +2 s; `PlayerIns+0x6C0` followed within 1 m.
- **Jump 2 (cliff, 13 m below the saved spot):** saved `(13.231,93.485,-6.404)` origin (40,0,-216); `pos_go` from `(6.738,80.127,1.251)`
  wrote the saved value, held at +100 ms / +500 ms / +2 s (`PlayerIns+0x6C0` y 93.42 → 93.40). Leonardo: "working fine", no fall.
- 0 errors / panics; slowest tick 12 ms. The T-028 teleport works unchanged on `main`.

### 2026-10-22 — HAWK-1b run 1, long start routes (Leonardo solo, dev `main` d3d1eb8 + `dist\gamedata` with `-HawkRoutes` box margin 150 m, nrpvp.dll 8036B4B61971B2E4). Logs: `experiments/HAWK-1b/`: **PASS (mechanism), route end over the void**
- Trace: `ride_sampler.sh` sent `pos_save` once a second; world position = `Physics+0x70` + origin. br_menu off, solo Commence.
- **The game loads our `m60_00_00_99.msb.dcx` from the me3 package** and the hawk flies the patched route: route `08_000` (start height
  198.9, 552 m after the patch, vanilla 203 m). Samples 01:18:43 → 01:19:14: world (108.8, 198.83, 84.6) → (−404.5, 198.83, 270.1),
  a straight 546 m leg in 31.3 s = **17.4 m/s, height constant to 0.01 m** (no sinking, no clipping reported).
- **The engine's route end drops the player:** the fall starts between 01:19:14.3 and 01:19:15.4 (y 198.8 → 189 → 158 → 105 → 47 → −9 →
  −26). No input, no DLL code.
- **Defect: the end point was off the island.** Leonardo: fell into the void. The game put him back on land at world (−304.8, 17.8, 441.7)
  by 01:19:25 and play went on. Cause: the route end (1052, 1134) in map-file coordinates is inside the old box guess (TECHNICAL_NOTES §4c)
  but outside the island. Fix built the same day: route ends now stop 50 m inside the outline of known land points (TASKS HAWK-1b).
- Frame: the leg's direction in the world samples is the exact opposite of the route's direction in the map file, so world = map file
  rotated 180° about the vertical, plus an offset (TECHNICAL_NOTES §4c, one ride).
- 0 errors / panics.

### 2026-10-22 — HAWK-1b run 2, land-safe route ends (Leonardo solo, dev `main` d3d1eb8 + `dist\gamedata` with `-HawkRoutes`, ends 50 m inside the land outline, nrpvp.dll 8036B4B61971B2E4). Logs: `experiments/HAWK-1b/nrpvp_2026-10-22_01-25-33.log`: **PARTIAL**
- Two expeditions in one session. 0 errors / panics.
- **Ride A, route 08_000 (346 m):** 01:26:41 → 01:27:01, y 198.8 constant, drop at the route end, **landed on ground** (y 199 → 45 by
  01:27:06, then walking at y ≈ 45).
- **Ride B, route 01_000 (987 m):** 01:31:49 → 01:32:46, 963 m in 56 s = 17.1 m/s. Height not constant: y 274.1, climbs to 294.1 at
  01:32:03–06 (the route points are all at 274.1), then sinks slowly to 274 by 01:32:47. Drop at the route end, then a fall from y 274
  to **y −25.8** (01:32:54), the same floor as the run 1 void fall; the world unloaded at 01:32:59 (death). Yet the route end (650, 549) in map
  coordinates lies 57–78 m from four vanilla drop spots, i.e. inside land. Leonardo (his retest): **the route end killed him**.
- Open: was ride B a fall into the sea or a fall through ground that was not loaded yet? Run 1 (void) ended ~350 m past the vanilla
  drop, ride B ~790 m past it, ride A (fine) ~140 m past it.
- Frame: ride B also ran opposite to its map-file direction (180° holds), but the offset differs from run 1 by (789, 261) m, so
  `Physics+0x70` + origin is not one global frame (TECHNICAL_NOTES §4c).

### 2026-10-22 — HAWK-3 run 1, env swap probe (Leonardo solo, dev uncommitted `main` + `game/hawk.rs`, vanilla routes, nrpvp.dll 96B0D469D120687E). Logs: `experiments/HAWK-3/nrpvp_2026-10-22_11-57-04.log`: **PASS**
- One session, 11:57-12:07 UTC, four expeditions. No crash, no ERROR or panic; heartbeat 0-15 ms throughout with the swap on.
- `hawk_env`: env object `exe+0x3C1D3D8` → switch at `+0x10` = **`exe+0x408030`** (game code, not Seamless Co-op). Swap ON at 11:58:36,
  re-found on each of the 4 area loads, swap OFF at 12:05:31 with the original put back.
- **`r8` of the env call is LOCAL's PlayerIns** (every expedition: "rider = LOCAL PlayerIns … (r8 of env is the PlayerIns)"). Six
  other characters also ask for 377 in the Roundtable (answer 0); in the expeditions only LOCAL does.
- **377 `GetBirdEventID` on the vanilla start ride:** −1 idle → **4** for under 100 ms at ride start (11:59:22.28) → −1 → 0 (11:59:30)
  → **2 at the route end** (11:59:34.41, 12 s after the start). On the Spectral Hawk Tree rides: 0 blips and 2 at each end
  (12:00:59, 12:02:06); the start value 1 was never sampled (shorter than one 50 ms tick).
- **394 `GetBirdMovementDirectionTime`:** stays 0 on the start ride; climbs about 1,600/s while steering a tree hawk and falls back when
  the stick is released (12:01:25). **384 `GetBirdPathEscapeTime`:** −1 on the route, counts up 16 → 1,721 off the route, back to −1.
- `hawk_drop` (12:04:00.98, rider LOCAL): **359 answers of 2** in 1.5 s, no error. It came about 11 s into a 12 s ride (route end
  expected ≈ 12:04:01.7), so the drop beat the route end by under 1 s; the engine's own 2 never followed (it did on the other rides).
  The world unloaded 17 s later (12:04:18). **Leonardo: `hawk_drop` worked.** A rerun early in the ride is still worth one look.
- `hawk_tree 1` (set 12:04:42 in the Roundtable): the next start ride answered **1 instead of 4 four times**, and during that start
  ride **394 rose with the stick (12:05:03, 12:05:10)**, which it never does on a vanilla start ride: the tree-hawk loop ran and read
  steering. 384 stayed −1 (never off the route). **Leonardo: `hawk_tree 1` worked**, but the ride felt much shorter than it
  should and he landed at the start of the map: the run used the vanilla 200 m start route (`dist\gamedata` had no map files),
  and a tree ride leaves the route as soon as the stick moves. Design answer: PROJECT_PLAN §13 v1 (cruise lock, dive, long line).
- Trace `all`: ids 326 and 1116 alternate within a tick (an argument-dependent id) and flooded the log (≈ 4,000 lines); 224 tracks the
  camera or stick angle. Button id for "hold action" not identified yet.

### 2026-10-22 — HAWK-1b run 3, one diagonal line "B" for every start route (Leonardo solo, dev uncommitted `main`, `dist\gamedata` with `-HawkRoutes` line (858, 210, 678) → (512, 250, 1024) → (166, 448, 1370), nrpvp.dll 96B0D469D120687E). Logs: `experiments/HAWK-1b/nrpvp_2026-10-22_12-33-22.log`: **PASS**
- Three expeditions in one session (loaded 12:34:06, 12:36:21, 12:38:48 UTC). No ERROR or panic; slowest main-loop pass 12 ms.
- **Leonardo: all three rides were perfect, the same route each time, no fall damage.** The moved start point still puts the player on
  the hawk, and the drop from the line's end (y 448 over ground ≈ y 194, about 250 m) does no damage.
- Each expedition stayed loaded 90–100 s after it started (no reload mid-ride or right after the drop). No position samples were taken
  this run (the ride sampler was not running), so length and speed of the ride are from Leonardo's report and the route data (979 m).

### 2026-10-22 — HAWK-4 run 1, cruise lock and dive (Leonardo solo, branch `hawk-crossing` + HAWK-4 uncommitted, line B in `dist\gamedata`). Logs: `experiments/HAWK-3/nrpvp_2026-10-22_12-57-44.log`: **PASS (two polish points)**
- Four expeditions, no ERROR or panic. Cruise held the line (stick taps ignored); dives started on a stick hold (real 394 = 401–418)
  and ended after 4.0 s (three times) and 7.0 s (once, `hawk_dive 400 7000`), then the vanilla fall; the other rides ended through the
  drop buttons. **Leonardo: "most of it is perfect"; overall result excellent.**
- Polish 1: the tree-hawk **start animation** (the hawk grabbing the player) shows at the ride start; he does not want it seen.
- Polish 2: the dive should **last longer and carry the player farther**.

### 2026-10-22 — HAWK-4 run 2, start-animation cut and faster dive (Leonardo solo, branch `hawk-crossing`, line B in `dist\gamedata`, nrpvp.dll EA76EE9D80AFE6FA). Logs: `experiments/HAWK-3/nrpvp_2026-10-22_13-21-27.log`: **PASS**
- Three expeditions, no ERROR or panic. Each ride logged "tree start: skipping the start animation" (default `hawk_start 1`); two
  dives of 7.0 s (13:23:07–14, 13:27:16–24) with the x1.3 boost on (default), then the vanilla fall.
- **Leonardo: good enough as it is.** `hawk_start 2` and `hawk_boost 0` were not tried (not needed). The one thing that may change
  later from player feedback: the dive's time and distance.

### 2026-10-22 — HAWK-2 solo smoke, hawk crossing always on (Leonardo solo, branch `claude/interesting-tharp-1c1237` 447aa1f, `scripts\launch.ps1` from the worktree: developer DLL in PLAYER mode, `matchmaking = false`, no `[hawk]` table, line B in `dist\gamedata`, nrpvp.dll E58D614C024A9FE2). Logs: `experiments/HAWK-2/nrpvp_2026-10-22_15-05-19.log`: **PASS**
- No console (player mode) and no command: `[hawk] HawkConfig { enabled: true, dive_hold: 400, dive_secs: 7.0, ... }` →
  `HAWK-2 hawk crossing ON` at startup, `HAWK-3 swap ON` 8 s later (exe+0x408030 → env_hook).
- Ride: `rider = LOCAL PlayerIns`, start animation cut, `phase none -> cruise` (15:06:14), `cruise -> dive (real 394 = 400)`
  (15:06:22), `dive -> dive over` 7.0 s later. No ERROR or panic; heartbeat 0–26 ms.
- In the Roundtable 8 other characters asked hawk ids (`(other)`); none was a rider, and the overrides now act on LOCAL only.
- **Leonardo: "it worked now, it's how I wanted it."** A first try the same day ran the old `hawk-crossing` DLL from the main
  checkout's `dist` (cruise off until `hawk_cruise 1`, no HAWK lines in the log): not a defect of this build.
- Not checked: a Spectral Hawk Tree ride after the crossing (must stay vanilla), and two players (next FS).

### 2026-10-22 — OFF-1 Offline mode (Leonardo solo, branch `claude/offline-mode-ccabe0` 711d6fa, worktree `dist\` with `br_menu = true`, `mode = "br"`). Logs: `experiments/OFF-1/nrpvp_2026-10-22_15-25-21.log`: **PASS**
- Start: `build mode: DEV`, `mode: br (from nrpvp.toml)`, `features: ["mode"]`. No ERROR or panic.
- `mode offline` (15:28:30) → Commence Solo (press #1, decision Pass): **the expedition started at once**, no searching box (Leonardo).
- `mode br` during the expedition (15:30:17) → "stays until the expedition ends"; applied on the way back (15:30:26, "after the
  expedition").
- Back in the Roundtable, mode br: Commence Solo (#2) → searching (queue phase 1); Commence again (#3) → stopped. As before.
- Not exercised: Offline + Duo ("solo only" box). Press #4 was Duo in **br** mode (decision 0x120 = "Duo/Trio coming soon"), as
  before; the Offline + Duo path is covered by unit tests only. Crash on quit: not reported.

### 2026-10-22 — OFF-4 bot spike (Leonardo solo, branch `claude/off-4-bots` 6fc31c8, worktree `dist\`, `mode = "offline"`). Logs: `experiments/OFF-4/nrpvp_2026-10-22_16-48-10.log`: **PASS**
- **Leonardo: the bots spawn.** They follow him and fight enemies; he could not fight them (they are allies, team 2: as shipped).
- Our `common.emevd` events run in the expedition: each flag set (`features: flag 10007300 ON`) is followed within 50 ms by the
  manager's new table id (10, then 10000). `bots_clear` (flag 10007302) → table −1 and both bot slots back to −1 within 1 s.
- **The manager holds exactly 2 bots.** Its bot list has 2 entries from the start (both −1); after `2003[97]` they hold the
  characters' handles `0xFFFFFFFF10E00000` / `…01`. Table 10000 (8 rows) also gave 2 handles. The constructor sizes the list to a
  constant 2 (`exe+0x38CC05..`, static), and the table store has room for 2 rows: **the vanilla system is "solo player + 2 AI
  teammates"** (MEMORY_MAP §15).
- The `+0x4` "armed" byte read 0 at every 50 ms sample: the update consumes it within one frame.
- No ERROR or panic. One WARN at the area unload (ChrSet capacity 0), as on every quit.

### 2026-10-22 — OFF-5 run 1, offline match with rival bots (Leonardo solo, branch `claude/off-4-bots` bee0399, worktree `dist\`, `mode = "offline"`, `offline_bots = 5`). Logs: `experiments/OFF-5/nrpvp_2026-10-22_17-07-38.log`: **PARTIAL (4 of 5)**
- **Leonardo: the bots fight him, each other and the monsters.** No console was used.
- **Detach and re-arm works:** after the manager's slots were set to −1, the bots stayed in the world (and fought), and the next
  `2003[97]` spawned 2 more at once (round 3, 1 s after round 2). Handles grow per spawn: `0x…10E00000` .. `…03`.
- **Found by handle:** every bot was in WorldChrMan character set **14** (the ER encoding: bits 20..27 of the handle), `ChrIns+0x68`
  = **600040000** (its NpcParam id), team 2 → written 29 at once; no reverts logged.
- **Why 4:** "landed" fired 4 s after the expedition loaded (the ground byte changes at the ride start too), so round 1 (table 10)
  was armed during the ride; the manager spawned nothing for 15 s and the director moved on. Round 2 (armed 17:08:58) spawned at
  17:09:03, at the landing: **the manager spawns only once the host is on the ground** `[HYPOTHESIS from timing]`. Fix (same day):
  a round waits up to 120 s.
- Ticks of 55–82 ms in the windows with a flag write (`flags::set` walks the flag tree); otherwise 0–16 ms. No ERROR or panic.

### 2026-10-22 — OFF-5 run 2, 5 rival bots (Leonardo solo, branch `claude/off-4-bots` 23b5bb6, worktree `dist\`, `mode = "offline"`). Logs: `experiments/OFF-5/nrpvp_2026-10-22_17-18-38.log`: **PASS**
- Round 1 (table 10) was armed during the hawk ride (17:19:43); **the game held it and spawned both bots at the landing** (17:20:03).
  Rounds 2 (table 30) and 3 (table 50) spawned within 0.1 s of their flag. **5 of 5 kept**, all found in set 14 and set to team 29;
  the 6th bot of round 3 was removed by the clear. No console used.
- Ticks of 82–103 ms in the windows with flag writes (as in run 1). No ERROR or panic.

## 4. Known bugs

Owned by STATUS.md §1 ("Known bug") so there is one list.

## 5. Regression set

Rerun after any game, SC, me3, or nrpvp update: TC-001, plus TC-003 row 3 once it passes.

---

## 6. Friend Session runbooks

No open runbook. Finished runbooks (FS-1, FS-MM1, FS-MM2, FS-UI1 on 2026-10-20; FS-UI2 on 2026-10-21) were removed; their results are in §3 and their text in git
(`git show 339ee04:.claude/TESTING.md`; FS-UI2: `git show c727293:.claude/TESTING.md`).
