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

## 4. Known bugs

Owned by STATUS.md §1 ("Known bug") so there is one list.

## 5. Regression set

Rerun after any game, SC, me3, or nrpvp update: TC-001, plus TC-003 row 3 once it passes.

---

## 6. Friend Session runbooks

### FS-UI2 — In-game Battle Royale UI on the player zip, two PCs (≈40 min). **Status: ready (zip of 2026-10-20 01:28 UTC).**
Tests UI-1 (Number of Players = Solo → Commence → our queue; Stop Searching; the host's Commence once the match is ready) and UI-2 (the
blinking messages) with two players, and folds in FS-UI1 (Seamless Co-op with our game-data package on **both** PCs). Main open question:
`[HYPOTHESIS]` at the match-ready Commence the DLL switches Number of Players from one player to two (byte 2 → 1) so the host's start is a
multiplayer start that takes the joiner along (UI-1 step 3).
**Zip:** `dist\NightreignPvP.zip`, built with `scripts\package-release.ps1 -MatchPlayers 2` (VERSION.txt: commit 71f893d clean, no overlay,
nrpvp.dll `F6B1C1FE9D813141`, gamedata emevd `8B9459487BD2A5A3`, me3 0.11.0). Contains `nrpvp_launcher.exe`,
`HOW TO INSTALL.txt`, `NightreignPvP\` with nrpvp.dll, nrpvp.toml (`matchmaking = true`, `mm_max_players = mm_min_players = 2`), VERSION.txt,
`gamedata\` (event\m10_00_00_00.emevd.dcx + 15 × msg\<lang>\menu_dlc01.msgbnd.dcx) and me3. If the build is rebuilt from a commit, use that zip
and its VERSION line instead; both players need the same zip.
**Before (both, ~5 min):** back up `%APPDATA%\Nightreign\<steamid>\`. Delete the old `NightreignPvP` folder and `nrpvp_launcher.exe` from the
game folder, then drag in both from the new zip (HOW TO INSTALL.txt). Steam running, different accounts, voice chat open, start with
`nrpvp_launcher.exe` only. Leonardo checks once on his PC: `<Game>\NightreignPvP\nrpvp.me3.toml` (written by the launcher) ends with
`[[packages]] id = "nrpvp-gamedata"`, and the log has `br_menu: Commence handler armed (hwbp slot 2)` and, after the Roundtable loads,
`br_menu: our labels found in CL_MenuText_dlc01 ... (hwbp slot 3)`. Write down the time of every step (the logs have the rest).

**0. Labels and Seamless Co-op with the package (both, 3 min; FS-UI1).** Each at their own Roundtable: table → Matchmaking Settings →
Number of Players. Does the list read **Solo / Duo / Trio** on both PCs? Pick **Solo**, back out. Any missing texts (blank or `?` labels)
anywhere in the menus? Pass = both see the three labels.

**1. Stop Searching, one side (Leonardo, 3 min; the friend does not search).** Commence → box "Battle Royale: searching for players (Solo)…"
→ OK. Within ~10 s a blinking "Battle Royale: searching for players…" / "(1 of 2)"; it comes back every ~6 s while searching (as soon as the previous one has faded). The button now reads
**Stop Searching**: press it → box "stopped searching for players" → OK, blinking "Battle Royale: stopped searching." The button reads
Commence again. Pass = all of that, and no match found (the friend was not searching).

**2. Match found (both, 5 min).** Both pick Solo and press Commence within ~30 s of each other, OK the box. Expected messages, in order:
"searching (1 of 2)" (maybe "(2 of 2)") → "Battle Royale: match found!" on both → one player (the host) gets **"Match found! Open
Matchmaking Settings > Invite Members to bring the others in."**, the other "Waiting for the host…" / "Joining the host…". Write down who is
host and every message each side saw (which ones were missing or came late).

**3. Host opens Invite Members (host only, 3 min).** Host: Matchmaking Settings → Invite Members once. **No Steam window should open**
(quiet invite, hwbp slots 0/1, now running next to the Commence/label breakpoints 2/3). The joiner presses nothing: "Joining the host…" →
"Loading in…" → arrives in the host's Roundtable (SC). Then **"Everyone is here. Start the expedition!"** (host) and "Everyone is here. Waiting
for the host to start." (joiner). Does SC's party list at the table look normal on both screens (FS-UI1)?

**4. Host's Commence (host only, 5 min): the hypothesis.** Host: table → Number of Players still **Solo** → first boss → **Commence**.
Pass = the departure starts **for both** and both load into the same expedition (rivals: you can hurt each other; the player build has no overlay any more, so the logs show the rival write).
If it fails, write down exactly what each screen showed (a box? which text? solo expedition for the host only?), then try once:
(a) host sets Number of Players = **Duo** (one of the old "Up to 6 players" options), Commence (the DLL passes it unchanged in this
phase); if that fails, the same with **Trio**;
(b) if that fails too: stop here and write down what each screen showed; Claude reads both logs (`br_menu: Commence pressed … queue phase 2`,
`mm_join:` lines). (The player build has no Insert panel any more: the in-game menu and messages are the only UI.)
After the expedition (or a short walk + quit to the Roundtable), both: does the search start again with Solo → Commence? (One round is enough.)

**5. Friend stops searching (friend, 2 min).** Friend alone: Commence → Stop Searching, same checks as step 1 on the friend's PC.

**After:** both send `<Game>\NightreignPvP\logs` (and the friend's `launcher.log`); Leonardo copies them into
`.claude/experiments/FS-UI2/{host,client}`. Log lines to read: `br_menu: Commence pressed (#n): Number of Players byte …, queue phase …,
decision code …` (step 4: byte 2 on the host and the multiplayer decision), `ui_msg: … shown` / `NOT picked up`, `hwbp: debug registers now
[…] (mask …)` (mask 0b1111 on the host while the quiet invite is armed), the `mm_queue` / gate lines around MATCH READY and ALL PRESENT.
**Stop rules:** a crash → note the time and the step, relaunch, continue from that step. The match-ready start failing in (a) and (b) is a
result, not a blocker: finish steps 5 and the logs.

Finished runbooks (FS-1, FS-MM1, FS-MM2, FS-UI1) were removed on 2026-10-20; their results are in §3 and their text in git
(`git show 339ee04:.claude/TESTING.md`).
