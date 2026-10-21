# Task Board

The unit of work is the **task card**. Opus writes and reviews cards. Sonnet executes one card at a time.
The Human runs anything that needs the game open.

**Status values:** `todo` · `ready` (all dependencies done) · `in-progress` · `review` (Sonnet done, waiting for Opus) ·
`done` · `blocked` (with reason).
**Owner values:** `SONNET/implementer` · `SONNET/researcher` · `OPUS` · `HUMAN` · combinations, where the
first owner does the work and the second verifies.

**Testing constraint:** day to day, only Leonardo tests, solo. Two-player checks are batched into
**Friend Sessions** (FS-1, FS-2, …). Their runbooks are in TESTING.md §6. A card is `done` when its
**solo** acceptance passes. Its multiplayer checks then sit in the FS runbook.

## How to run a card

- **From an Opus session:** "Run T-00X." Opus spawns the matching agent from `.claude/agents/` with the card
  text, reviews the result, and updates this board. Several agents may run in parallel, so **agents do not
  edit this file**. They write their Result into their deliverable or reply, and Opus copies it here.
- **From a Sonnet session directly:** "Execute T-00X from .claude/TASKS.md, following .claude/CLAUDE.md." When
  it's done, it sets the status to `review` and fills in Result. Opus moves the card to `done`.
- **HUMAN steps:** follow the card. Paste logs and observations into chat or into TESTING.md, and an agent tidies them up.

## Card template

```
### T-XXX — <title>
Owner: … | Status: … | Depends on: … | Phase: …
Goal: one sentence, the question answered or the capability added.
Context: links to the doc sections the executor must read.
Steps: numbered and concrete.
Deliverables: files created or changed.
Acceptance: objective checks; mark the HUMAN-solo checks; two-player checks go to an FS runbook.
Out of scope: what NOT to do.
Result: (filled in after execution: what happened, evidence, follow-ups)
```

---

**Board cleaned 2026-10-20:** done, dropped and superseded cards were removed. Their full text is in git:
`git show 339ee04:.claude/TASKS.md`. Findings from them live in `re/MEMORY_MAP.md`, `MATCHMAKING.md`, `TESTING.md` §3 and
`research/`.

---

## Engineering (accepted 2026-10-20; cards are written out in full when they start)

Done (on `main`, SMOKE-1 PASS 2026-10-20): **R0** (fmt + CI), **R1** first pass (dev feature), **CON-1, CON-2** (console + `nrcon.exe`).
Done on `main` (SMOKE-2 PASS 2026-10-21): **R5**, **CON-4**, **R2** half (`game/layout.rs`), **R3** half
(split by job), **R4** removed keys. Open: the R2/R3 halves (with the first new feature), R4 sections, R6, CON-3, CON-5. Order was: SMOKE-1 → FS-UI2 → R5 → R2 → R3 → R4 → R6 → CON-3, CON-4 → (CON-5 optional).
- **R0–R6 — Refactor plan.** Behaviour-preserving; scope, acceptance and rules in `ARCHITECTURE.md` §6–§7. Owner: SONNET/implementer →
  OPUS review → HUMAN solo smoke run.
- **CON-1–CON-5 — Developer console.** Scope and acceptance in `CONSOLE.md` §6–§7. Owner: SONNET/implementer → HUMAN solo.
- **VER-1 — Version gate.** `startup.rs` logs the exe FileVersion but never acts on it. Compare it with the pinned build
  (`TECHNICAL_NOTES.md` §1); on a mismatch, log a warning and start no hooks, debug registers or memory writes. Reads and logging
  still run. Rule source: ARCHITECTURE.md §6 "Modding rules". Owner: SONNET/implementer → HUMAN solo (normal launch must be
  unchanged). Fits after R1.

## Matchmaking and in-game UI (M8)

### MM-4 — Solo queue end to end, player build
Owner: SONNET/implementer → HUMAN (solo) then FS-MM4 | Status: built; solo steps 1–4 PASS 2026-10-19, awaiting FS-MM4 (incl. step 5) | Feeds: F1, F15
S9 §6 for Solo in the **player build** (`dev_mode = false`), with the join path proven in FS-MM3 (host presses Invite Members, quiet
invite, joiner link + invite, ALL PRESENT needs ChrSet players).
1. Config: new player setting `matchmaking` (default true). In the player build it turns on what Solo matchmaking needs: the queue, the
   join step with `invite+link`, and the quiet invite. Everything else stays forced off (`mm_probe` logging, `mm_host_create`,
   `mm_lobby_trace`, `mm_sc_create`, manual join buttons). If the queue or gate depends on callbacks the probe registers, keep the
   callbacks and drop only the probe's logging. `matchmaking = false` turns it all off.
2. Lobby size and start rule (S9 §6 step 5): `mm_max_players` (default 6) is the queue lobby size; the match starts when the lobby is full,
   or when `mm_fill_wait_s` (default 90) has passed since the lobby got its 2nd member and it holds at least `mm_min_players` (default 2).
   Keep `mm_match_players` working as before for dev/mmtest (it sets min = max); the mmtest zip keeps 2 so tests stay quick.
   Gate and invites must handle up to 5 joiners: FakeSteam tests with 3 and 6 players.
3. Missing players (step 8): host, 60 s after publishing `sc_lobby`, with present ≥ min: `N/M here - you can start; <names> did not
   arrive`. A joiner not in the host's lobby 120 s after `state=go`: give up, leave, back to idle with `match missed - queue again`.
4. Leave the queue by itself when the player leaves the Roundtable or the world unloads while Searching or InQueue (not in MatchReady:
   a joiner's world reloads when it enters the host's lobby, S9 §5g). Status `Queue: left (you left the Roundtable)`.
5. Player overlay Queue block: one button `Solo Battle Royale` and `Leave`; Duo/Squad hidden (MM-6) and no dev buttons. Lines in player
   words: `Searching for players... 3/6`, `Match found!`, host `Match found! Press Matchmaking Settings > Invite Members to open your
   lobby (no Steam window will open)`, joiner `Match found! Joining the host...`, `Everyone is here - host: start the expedition`,
   `In match`. Dev builds keep today's detailed lines.
6. Packaging: the default player zip (`scripts\package-release.ps1` without `-MatchmakingTest`) ships with `matchmaking = true`;
   update its how-to text (Solo: both press the button; the host presses Invite Members when asked; nobody else does anything).
Acceptance (implementer): tests for 1–4 (FakeSteam), `cargo test`, clippy clean; both zips rebuilt and their tomls checked.
Acceptance (HUMAN, solo): player zip: button, `Searching for players... 1/6`, Leave, and quitting to the title leaves the queue.
Acceptance (FS-MM4): two PCs on the player zip: Solo match with only the host's Invite Members press; expedition; back at the Roundtable.
Result (implementer, 2026-10-19): built, awaiting HUMAN-solo. Config (`config.rs`): player setting `matchmaking` (default true; in the player build it forces `mm_queue`, `mm_join`, `mm_quiet_invite` to follow it and `mm_join_auto = "invite+link"`; `mm_probe`, `mm_host_create`, `mm_lobby_trace`, `mm_sc_create` stay forced off; no effect in dev mode), `mm_max_players` 6 (2..=6, the queue lobby size/`need` tag), `mm_min_players` 2 (2..=max), `mm_fill_wait_s` 90 (5..=600); `mm_match_players` in the file sets min = max (mmtest keeps 2). The default `nrpvp.toml` text no longer sets `mm_match_players` (it would override the new defaults). Machine (`mm/machine.rs`): owner starts when `complete_players >= max`, or when the lobby has had >= 2 members for `fill_wait` and holds >= `min` (both need all tickets complete); `world_tick` leaves Searching/Creating/Joining/InQueue after 3 s away (never MatchReady) with idle line `Queue: left (you left the Roundtable)`; `MachineCfg.friendly` gives player words. Gate (`mm/machine/gate.rs`): host line `N/M here - you can start; <names> did not arrive` 60 s after publishing `sc_lobby` when >= min are here AND loaded (here = min(Steam members, ChrSet players)); joiner not in the host's SC lobby 120 s after `state=go` (not after ALL PRESENT/expedition) leaves, idle line `match missed - queue again`; `Gate.pline` = player-words line. Signal for step 4 (`mm/mod.rs`): `World::{Never,Gone,Present(phase)}` from the time of the game loop's last local-player read (`Gone` = no read for max(4 s, 3 x watch_interval_ms)). Worker also refuses a queue request with `load into the Roundtable first` (no world) / `go back to the Roundtable first` (expedition). Player UI (`overlay/`, `startup.rs`, `config.rs::binding`): the player build keeps the read-only badge and gains ONE hotkey (the overlay toggle, default Insert, hotkey thread starts only for it) that opens a small interactive panel `Battle Royale matchmaking` with the status line, `Solo Battle Royale` and `Leave` (no Duo/Squad, no dev buttons); the badge gets a third line with the queue status or `[Insert] Battle Royale matchmaking`; the game loop accepts only `MmQueue(Solo)`/`MmLeave` from the player overlay. Probe (`steam/probe.rs`): `spawn(queue, evidence, verbose)`; player build verbose = false (callbacks registered, startup dump and per-event lobby dumps dropped, one `mm_probe: quiet mode` line); mm_queue/mm_join/mm_quiet_invite/mm_host_create log lines unchanged. Tests now 419 (418 pass, 1 ignored hudhook smoke; the MM-3f card counted 396) with new ones for config, lobby size/fill wait, 3 and 6 player gate, start line, give-up, player words, auto-leave, world state, overlay tones, command whitelist, `cargo clippy --all-targets` clean, release build OK. Zips (this worktree's `dist\`, 2026-10-19 15:49 local = 18:49 UTC, nrpvp.dll 5AA3CF20AF48B3A9): `NightreignPvP.zip` toml = `team_size = 1`, `matchmaking = true`, `team_candidates`; `NightreignPvP-mmtest.zip` toml unchanged (dev_mode, `mm_match_players = 2`, `mm_quiet_invite = true`, `mm_host_create = false`, `mm_lobby_trace = false`, `mm_sc_create = false`). How-to text: new FIND PLAYERS section (the mmtest zip carries the same HOW TO INSTALL.txt). Left for the HUMAN: the solo check in the Acceptance (HUMAN) line, then FS-MM4. Risks/UNKNOWN: (1) the leave-by-itself signal is the `team_type` read once per watcher period; a long stall of the game loop in the Roundtable could look like `Gone` (3 s debounce after 4 s); (2) the player panel needs the Insert key, which Seamless Co-op or another mod might also use (configurable through `overlay_toggle_key`); (3) up to 5 invites/links are only proven by FakeSteam, the real 2-player path is FS-MM3's; (4) dev builds without `mm_match_players` in their toml now get lobby size 6 / min 2 / 90 s instead of 2.

### UI-1 — Battle Royale inside the stock expedition menu (Leonardo's flow)
Owner: OPUS (RE) → SONNET/implementer → HUMAN (solo), then FS | Status: solo PASS (dev); in the player zip since 2026-10-20, FS-UI2 next | Depends: UI-0 | Replaces: MM-5
**Flow (Leonardo, 2026-10-19, "the best flow possible, we are not giving it up"):** spawn at the Roundtable → press the table → the stock boss
list → **Matchmaking Settings → Number of Players shows Solo / Duo / Trio** → back, pick the **first boss** (only that one for now) →
**Commence Expedition** → the game searches for players (**our** queue) → when the match is full the expedition starts by itself. Cancel =
the game's own "Cancel matchmaking". Battle Royale only (no normal expedition while the mod is installed). On a roadblock: talk to
Leonardo about alternatives that keep this flow.
**Why it needs the DLL:** the menu is native code (`OpenCommenceExpeditionMenu`, menu 34), not a script; under me3 the game is offline, so
Commence with more than one player shows 506023 "Cannot start multiplayer session in offline mode" and then "Matchmaking failed"; no
event flag changes (step 0 result below). We must **never** make the game think it is online (that is Bandai's servers).
**Static findings (2026-10-19, `experiments/UI-0/imm_xrefs.py`, `disfunc.py`, game 1.03.3)** `[VERIFIED: disassembly; meaning HYPOTHESIS until the trace]`:
- The expedition menu code is ordinary (has `.pdata`): `0x9EE750` "Cancel matchmaking?" 506012 / "canceled" 506011 (tail obfuscated);
  `0x9EEBE0` **Commence checks**: 506040 not installed, 506070 too many relics, 506024 Deep of Night offline, then 506023 offline-multiplayer,
  decided from **byte `+0x49` of the settings object `[menu+0xFC8]`** (`<=1` with online, or `==2` → the matchmaking branch) and
  `0x6AEAE0` = "is online" (`[exe+0x3C13258]+0xB40`); `0x9EF3F0` Commence button label (130304 Solo / 130305 Two-Player); `0x9FB020`
  **Number of Players list** (130408/130417/130409; settings `[rbx+0x3E08]`, same `+0x49` byte, reset to 0 in some cases); `0x60A1C0` a
  table matchmaking-result → dialog (506020..506030 family).
- The "Matchmaking..." ids (336900..336905) are computed, not constants: the matchmaking screen's code is found from the stack (trace).
**A (Solo/Duo/Trio in Number of Players):** read/write the `+0x49` byte (values per option from the trace); labels: SC swaps 130409/130417/
130408 through its `LookupEntry` hook, so our labels need a later hook or FMG ids that SC does not touch (to decide).
**B (Commence starts our queue):** catch the Commence check sequence (`0x9EEBE0`) when the setting is Duo/Trio (or our Solo), before the
offline dialog, and start `MmQueue` instead (hardware breakpoint + skip, as MM-3f does), so the offline dialog never shows.
**C (the game's own matchmaking screen and cancel):** find the screen's state from the stack of the 336900 lookups; drive "Matchmaking..."
and the "<remaining> / <total>" counter (336905, tags `matchRemainingCount` / `matchTotalCount`) from our queue; cancel through `0x9EE750`.
Fallback if C is blocked: our own text (UI-2) with the same flow; talk to Leonardo first.
0. **Step 0 result (2026-10-19 22:26–22:30 UTC, log `experiments/UI-0/ui1-step0-nrpvp.log`)**: "Up to 6 players" → Commence: 506023
   offline, 336900 "Matchmaking...", 336905 counter, 506020 "Matchmaking failed"; no Steam lobby event; **no event flag changed** (50 ms
   watcher, `ui_flag_watch`). Solo Commence: departure cutscene flags 2307/2330 ON.
1. **Trace for A/B/C (built 2026-10-19):** `game/menutrace.rs`, dev config `ui_menu_trace = true`: DR0 `0x9EEBE0`, DR1 `0x9FB020`, DR2
   `0x9EE750`, DR3 `LookupEntry` only for 336900..336905 / 506011 / 506012 / 506020..506023; logs registers + exe return candidates from the
   stack; dumps the settings object candidates and logs every byte change of them every 100 ms. `ui_flag_watch` stays on. 430 tests, clippy clean.
   **Runbook (Leonardo, solo, ~5 min, `.\scripts\launch.ps1`)**, press **F10** before each step:
   1. Roundtable → table → boss list. 2. Matchmaking Settings → Number of Players: select each of the three options in turn, ~2 s each,
   end on the **first "Up to 6 players"**. 3. Back, **Commence Expedition** → the offline message → OK. 4. Number of Players → the
   **second "Up to 6 players"** → Commence → OK. 5. Number of Players → **Solo Mission** → Commence; if a cancel is offered, cancel once,
   then Commence again and let it load. 6. Quit. Then tell Claude "menu trace done".
   **Trace result (Leonardo solo, 2026-10-19 22:56–22:59 UTC, log `experiments/UI-0/ui1-menutrace-nrpvp.log`):**
   - **A** `[VERIFIED]`: the Number of Players list (`0x9FB020`) uses settings `[rcx+0x3E08]`; the byte `+0x49` changed 2 → 0 when Leonardo
     changed the option. Commence with **0** → 506023 offline + "Matchmaking failed"; Commence with **2** → departure (flags 2307/2330),
     so **2 = One player**, 0 and 1 = the two multiplayer options (which is Two vs Three: not needed). Commence reads the same byte through
     `[menu+0xFC8]` (the menu object is new each time the menu opens).
   - **B** `[VERIFIED]`: DR0 `0x9EEBE0` hit once per Commence with `rcx` = the menu; called from `exe+0x9E81AE` / `exe+0xA4246E` (vtable
     calls). It is the place to start our queue before the offline dialog.
   - **C** `[VERIFIED: stacks]`: the "Matchmaking..." (336900) and counter (336905) lookups come from `exe+0xB4E80D` / `0xB4E892` /
     `0x133816D` / `0x12E0567`, which have **no `.pdata` (obfuscated)**, like the S9 session code. Driving the game's own matchmaking screen
     is a roadblock. Alternatives put to Leonardo: C1 the game's own Commence dialog with our live text ("Searching for players 3/6", its
     button = cancel), C2 an on-screen message (UI-2), C3 keep digging (later polish). Recommended C1.
   - Not exercised: Cancel matchmaking (`0x9EE750`, no hit: offline fails at once, there is nothing to cancel).
   **Decision (Leonardo, 2026-10-19):** C1 now (C3 later as polish; C1's queue start / cancel / hand-off carry over, only the searching
   screen would change). Nucleus may help later for two-window tests of our side (real Steam matchmaking still needs two accounts).
2. **C1 prototype (built 2026-10-19, `game/c1proto.rs`, dev `ui_c1_proto = true`), no code patched:**
   - Redirect: DR0 at `exe+0x9EEBFB` (first instruction after the Commence checks' prologue; bytes checked). If `[[r14+0xFC8]+0x49] != 2`
     (a multiplayer option) `RIP = exe+0x9EEC1D`: the game's own 506040 notice-dialog branch, which builds, shows and cleans up an OK
     dialog and leaves through the normal epilogue `0x9EF0B4` (the caller `0x9E8110` ignores the result) `[VERIFIED: disassembly]`.
   - Live text: FMG string = `fmg + offsets[idx]` (`0x27F5710`: `[[fmg+0x18]+idx*8] + fmg`); CL_Dialogues (204; 506040 has no
     `_dlc01` twin) entry 506040's offset slot ← `buffer - fmg`, one aligned 8-byte store (`mem::write_u64_aligned`); the DLL rewrites the
     UTF-16 buffer every second ("Searching for players n/6", n = 1..6). Groups at `fmg+0x28` (16 bytes: offset index, first id, last id),
     group count `fmg+0xC` `[VERIFIED: 0x27F5710 disassembly]`.
   - Open questions it answers: does the dialog show our text? does the open dialog follow the buffer (live counter) or keep the text it
     had when it opened? does the menu behave normally after OK?
   **Runbook (Leonardo, solo, ~4 min, `.\scripts\launch.ps1`):** 1. Roundtable → table → Matchmaking Settings → Number of Players → an
   **"Up to 6 players"** option → back. 2. **Commence Expedition**: a dialog should say "Battle Royale (prototype) / Searching for players
   n/6 / Press OK to stop searching". Watch it ~8 s: **does the number change while it is open?** Press OK. 3. Commence again once,
   OK, then leave the menu. 4. Number of Players → **Solo Mission** → Commence: the normal expedition should start. 5. Quit. Tell Claude what
   the dialog showed and whether the number changed.
   **Prototype results (Leonardo solo, 2026-10-19 23:11–23:30 UTC; logs `experiments/UI-0/c1proto-run2.log`, `-run3.log`):**
   - Redirect + text **work** `[VERIFIED: Leonardo saw the box with our text; log: byte 0/1 redirected, byte 2 (Solo) left alone and the
     expedition started]`.
   - **The box does not follow the text while open**: it copies the text when it opens. Even rewriting the digits in place in all copies
     found (19 copies UTF-16/UTF-8 in 8 GB of private heap, 10 still matching, updated every second from 23:29:33) left it frozen
     `[VERIFIED: Leonardo]`. A live counter inside the box needs close/reopen (unknown API) or the real matchmaking screen (C3).
   - Crash on quit (`0xC0000374` heap corruption in ntdll) on every run: **pre-existing**, logged by Windows every day since 2026-10-10 (5–17
     per day), not caused by UI-1. Separate issue.
   Options put to Leonardo: (1) box "Searching… you will be told" + live status as on-screen game messages (UI-2) + Commence again = "Stop
   searching?"; (2) close/reopen the box on each change; (3) the real matchmaking screen. Recommended 1 (2/3 later).
   **Decision (Leonardo, 2026-10-19): option 1**, then on-screen messages (UI-2), then MM-3g. Options 2/3 are later polish.
3. **Option 1 built (2026-10-19), dev `br_menu = true` (needs `mm_queue`; player build forces it off until tested):**
   - `game/menuflow.rs`: hardware breakpoint at `0x9EEBFB` (as the prototype). Number of Players byte: **2 = one player, 1 = two, 0 =
     three** `[VERIFIED: 0x9EF3F0 picks 130304 "Commence Solo Expedition" for 2 and 130305 "Two-Player" for 1]`. Pure `decide()`:
     idle + one player → queue Solo, dialog "searching… press Commence again to stop"; idle + two/three → dialog "Duo and Trio coming
     soon"; searching → leave, dialog "stopped searching"; match ready → Commence runs unchanged (one player is switched to two first, so
     the host's start is a multiplayer one `[HYPOTHESIS, FS check]`); matchmaking off → dialog saying so. The dialog text is picked by
     pointing entry 506040's FMG offset slot at one of four DLL-owned UTF-16 texts just before the game builds the dialog. The handler
     records a request in an atomic; the module thread calls `mm::send(Queue(Solo) / Leave)` (the overlay's path).
   - `hwbp.rs`: one owner of DR0..DR3 for every thread; MM-3f quiet invite now claims slots 0/1 through it, `br_menu` slot 2, so both run
     together. The all-register dev tracers (`mm_lobby_trace`, `ui_text_trace`, `ui_menu_trace`, `ui_c1_proto`) switch `br_menu` off.
   - `mm`: lock-free queue phase (`mm::phase()`: idle / searching / matched), written by the worker every tick.
   - 442 tests pass (decide table, config rules, hwbp mask), `clippy --all-targets` clean; `dist\nrpvp.dll` built, `dist\nrpvp.toml`
     has `br_menu = true`, `ui_c1_proto = false`.
   **Runbook (Leonardo, solo, ~4 min, `.\scripts\launch.ps1`):** 1. Roundtable → table → Matchmaking Settings → Number of Players =
   **Solo Mission** → back → **Commence**: dialog "Battle Royale: searching for players (Solo)…"; OK; the Insert panel's status line
   should say the queue is searching. 2. **Commence again**: "stopped searching"; the panel goes back to idle. 3. Number of Players =
   **Up to 6 players** → Commence: "Duo and Trio … coming soon". 4. Quit. Tell Claude what each step showed.
   Not solo-testable: the match-ready Commence (host starts the expedition with the others): next FS.
   **Result (Leonardo solo, 2026-10-19 23:47–23:49 UTC, log `experiments/UI-0/ui1-option1-nrpvp.log`): PASS.** hwbp armed slot 2 on 47
   threads, 0 failed. Commence with two players (byte 1) and three players (byte 0) → "coming soon"; one player (byte 2) → queue Solo
   sent → `solo 1/2 searching` (dev `mm_match_players = 2`); Commence again → stop sent → `Queue: idle`; again start, again stop. No
   crash during play. Still open: the match-ready Commence (FS), labels, player build.
4. **Labels Solo / Duo / Trio (built 2026-10-19):**
   - Game-data package: `gamedata/fmg-entries.json` (our texts, in git) + `scripts/build-gamedata.ps1` → `dist\gamedata\msg\<lang>\
     menu_dlc01.msgbnd.dcx` for all 15 languages (English text everywhere for now), only the listed FMGs rewritten (refuses FMGs with
     whitespace-only texts), pinned to the 1.03.3 engus hash. Labels live in **CL_MenuText_dlc01 (504)**, ids 99100100 Solo / 99100101 Duo
     / 99100102 Trio. engus check: only CL_MenuText_dlc01.fmg differs from vanilla, the three entries present `[VERIFIED]`.
   - DLL (`menuflow.rs`, hwbp slot 3): breakpoint at the CL_MenuText getter `exe+0x7AC800` (`edx` = id; bytes checked) swaps
     130409→99100100, 130417→99100101, 130408→99100102 before the lookup, so SC's `LookupEntry` hook never sees those ids. Armed only once
     99100100 and 99100102 are found in the live CL_MenuText_dlc01 FMG (no package → SC's labels stay). All four hardware slots are now
     used (0/1 quiet invite, 2 Commence, 3 labels).
   - Profile: the package is in the **default** profile `profiles/nrpvp-sc.me3.toml` (Leonardo, 2026-10-19: one profile; `br`, `ui0a`,
     `ui0b` removed; `launch.ps1` builds `dist\gamedata` when missing). 443 tests, clippy clean.
   **Runbook (Leonardo, solo, ~3 min):** `.\scripts\launch.ps1 -Profile br`. 1. Table → Matchmaking Settings → Number of Players: the
   three options should read **Solo / Duo / Trio** (also check how the chosen value shows on the settings page). 2. Pick Solo → back →
   Commence: the searching box; Commence again: stopped. 3. Quit; tell Claude what the list showed.
5. **Player build switch-on (built 2026-10-20):** `config.rs` `enforce_player_mode`: `br_menu = matchmaking` (was forced off); the dev
   tools (`ui_text_trace`, `ui_menu_trace`, `ui_c1_proto`, `ui_msg_test`, `mm_lobby_trace`, `ui_flag_watch`) stay forced off, so all four
   hardware slots go through `hwbp.rs` (0/1 quiet invite, 2 Commence, 3 labels; the quiet invite's VEH and `menuflow`'s match only their own
   addresses). `startup.rs` starts `menuflow` on `br_menu && mm_queue` (no dev check). `ui_messages` stays as written (default true).
   `nrpvp_launcher` writes a third profile entry `[[packages]] id = "nrpvp-gamedata" path = <Game>\NightreignPvP\gamedata` (same shape
   as `profiles/nrpvp-sc.me3.toml`) and refuses to start without `gamedata\event\m10_00_00_00.emevd.dcx`. `package-release.ps1` copies
   `dist\gamedata` into `NightreignPvP\gamedata` (checks 1 emevd + 15 languages; VERSION.txt names the emevd hash); HOW TO INSTALL.txt
   describes the table flow (Solo → Commence → messages → host Invite Members → everyone here → host Commence; Insert panel kept as
   the old way). 452 + 3 + 26 tests, `clippy --workspace --all-targets -D warnings` clean. Zip rebuilt with `-MatchPlayers 2`
   (2026-10-20 01:18 UTC, nrpvp.dll `0881264E03245444`; contents checked: 16 game-data files identical to `dist\gamedata`, toml
   `matchmaking = true`, min = max = 2). Two-player check: **FS-UI2** (TESTING.md §6; FS-UI1 folds in), including the match-ready
   Commence `[HYPOTHESIS]`. Not run in game yet.
6..n. Duo/Trio queues (MM-6).

### UI-2 — Matchmaking status in the game's own messages
Owner: OPUS (spec) → SONNET/implementer → HUMAN (solo) | Status: solo PASS (dev); in the player zip since 2026-10-20, FS-UI2 next | Depends: UI-1 (flag writes)
Goal: "Searching 3/6", "Match found", the host prompt, "Joining the host", "Everyone is here", "Left the queue" without ImGui.
1. **Spike first: which instruction draws in the Roundtable while the player walks around**, and how we author EMEVD. Options in
   order: (a) an EMEVD event in `m10_00_00_00.emevd.dcx` that waits on our flag and runs Display Status Message / Display Blinking Message /
   Display Generic Dialog (2007[3]/[4]/[1]) with our EventTextForMap entries, then clears the flag (DarkScript3 is a GUI: find out whether
   its library or Smithbox can save EMEVD without hands; else Leonardo saves it once and we keep the source in `gamedata/`); (b) the
   same message through ESD `OpenGenericDialog` (only while talking: not enough alone). Test solo with a dev hotkey that sets the flag.
2. Texts as EventTextForMap entries in our range (one per count for "Searching N/6", N = 1..6), all languages English for now.
3. DLL: one flag per message (10007100..), set by the matchmaking worker on each status change, rate-limited (one message on screen at a time).
4. Then remove the Insert panel and the badge from the player build (keep the dev overlay).
Acceptance (HUMAN, solo): queue from the table; the in-game message shows "Searching for players 1/6"; Leave shows "Left the queue".
**Spike built (2026-10-19), event-script route** (the direct route, EMEVD's message functions, sits in obfuscated code: the bank-2007
dispatch near `0x79D3DD` jumps into protected code):
- `scripts/emevd_add_events.py` appends events to a decompressed EMEVD in place (events, instructions, args appended; event 0's
  Initialize Event list copied to the end with ours added; params / linked files / strings copied). Argument packing checked against
  m10's own instructions (3:0, 2003:66, 1000:4, 2000:0, 2007:1) `[VERIFIED: byte compare]`; result re-read by `emevd_dump.py`;
  WitchyBND DCX round trip byte-identical `[VERIFIED]`.
- `gamedata/emevd-events.json`: three test events in the Roundtable (`event/m10_00_00_00.emevd.dcx`, pinned 1.03.3 hash): wait flag
  10007100 / 101 / 102 → Display Generic Dialog / Blinking Message / Status Message (texts 99100200..202 in EventTextForMap_dlc01) →
  clear the flag → restart. `build-gamedata.ps1` builds it into `dist\gamedata\event\` (needs Python 3).
- DLL: `game/flags.rs` get/set (writes only 10007000..=10007999, one-byte read-modify-write); dev `ui_msg_test = true` sets the three
  flags in turn 25 s after the world loads (10 s apart, twice) and reads each back 2 s later (OFF again = our event ran).
  `dist\nrpvp.toml` for the spike: `ui_msg_test = true`, `ui_text_trace = true` (shows which FMG each message asks for), `br_menu = false`
  (debug registers). 446 tests, clippy clean.
**Runbook (Leonardo, solo, ~3 min, `.\scripts\launch.ps1`):** load into the Roundtable and stand still about 1 minute. Three test texts
should appear one after the other, twice: "Battle Royale test 1 of 3 (dialog)", "… 2 of 3 (blinking message)", "… 3 of 3 (status
message)". Note which ones appeared and how each looked (popup with a button? text that fades? a line at the bottom?). Quit; tell Claude.
Risk: our m10 event file replaces the game's (with everything kept); SC with it is untested until this run.
**Spike result (Leonardo solo, 2026-10-20 00:41–00:43 UTC): PASS.** All three events ran (each flag was OFF again 2 s after the DLL set it)
and each asked EventTextForMap_dlc01 for its text (`ui_text_trace`); Leonardo read the **blinking message** fine. SC and the Roundtable
worked with our m10 event file. Decision: blinking messages.
**Messages built (2026-10-20):** `mm/ui_msg.rs`: 34 messages, message k = flag 10007200+k = text and event id 99100500+k, all blinking:
searching, "searching for players (N of M)" for M = 2..6, stopped (only after a search), left the Roundtable, missed, match found, host:
open Invite Members, waiting for the others / the host, joining, loading in, everyone is here (host: start / joiner: waiting), not
everyone arrived, problem. Pure mapping from the queue view (`Machine::queue_view`, the gate's player line) with tests; the worker posts
one message per change; a sender thread sets the flag (at most one per 3 s, newest wins) and clears all message flags once the flag manager
is live. `gamedata/*.json` hold the 37 events / texts; a unit test checks them against the Rust table. Config `ui_messages = true`
(default; needs the queue and the package). 450 tests, clippy clean; `dist\gamedata` (37 events) and `dist\nrpvp.dll` rebuilt; dev toml back
to `br_menu = true`, `ui_text_trace = false`, `ui_msg_test = false`.
**Runbook (Leonardo, solo, ~3 min, `.\scripts\launch.ps1`):** Roundtable → table → Number of Players = Solo → Commence (the searching
box) → OK. Within a few seconds: "Battle Royale: searching for players..." then "... (1 of 2)". Press Stop Searching: "Battle Royale:
stopped searching." Quit; tell Claude which messages appeared. The match messages need two players (next friend session).
**Solo result (Leonardo, 2026-10-20 00:53–00:56 UTC, log `experiments/UI-0/ui2-solo-nrpvp.log`): PASS.** Commence (Solo) → queue →
message 0 "searching for players...", 3 s later message 20 "(1 of 2)"; Stop Searching → message 1 "stopped searching"; again search;
quit to title → queue left ("you left the Roundtable"; the world unloaded before that message could show). hwbp slots 2+3 on 131 threads,
0 failed. Not yet seen: the match messages (two players).
   Leonardo asked whether messages stopped showing: **yes** — by design each message showed once per change, so nothing was on screen for
   28 s mid-search, and the first "(1 of 2)" (3 s after "searching...") and "stopped searching" left no flag change at 50 ms sampling
   (possibly dropped while the previous blinking text was showing). **Fix (2026-10-20):** gap 6 s, ongoing-state messages (searching,
   N of M, host Invite Members, waiting, everyone is here, not everyone arrived) repeat every 15 s while current, the worker posts `None`
   to end a repeat, and every message is read back after 1 s (log `shown` / `NOT picked up ... cleared`). 452 tests, clippy clean;
   `dist\nrpvp.dll` rebuilt. Solo check: search ~40 s → the search message every ~15 s; Stop Searching → "stopped" with a `shown` line.
**Player build (2026-10-20):** the zip now carries the package (`NightreignPvP\gamedata`, loaded by the launcher as `[[packages]]`) and
`br_menu` is on with `matchmaking` (UI-1 step 5); `ui_messages` was already on there. The match messages (match found, host Invite Members,
joining, everyone is here) are checked in **FS-UI2** (TESTING.md §6). Step 4 (remove the Insert panel and badge from the player build) is
not done: the panel stays as the fallback until FS-UI2 passes.

### MM-3g — No host click: trigger Invite Members from the mod
Owner: OPUS (RE) → SONNET/implementer → HUMAN (solo) then FS | Status: ready (RE spike first) | Depends: MM-3f, UI-0 Q3
UI-0 settled that **no talk command opens Matchmaking Settings or Invite Members** (all NR ESDs + esdtool's command list), so the data route
is closed. The action is the game's menu code: S9 §5d has the chain (menu functor calls `+0x80D743` → `+0x80E32A` → `+0x213B4F`, session
code `+0x258568B` / `+0x257F5DA`, the latter obfuscated with no `.pdata`).
1. Spike (Opus, read-only): with the MM-3c trace and `experiments/UI-0/disfunc.py`, find the highest frame that is "Invite Members was
   chosen" and has `.pdata` (`+0x80D743` / `+0x80E32A` are candidates) and what its arguments are (menu object, item id). A hardware-breakpoint
   trace on it while Leonardo presses Invite Members gives the live registers (reuse `ui_text_trace`'s VEH pattern).
2. If a callable entry exists: the host's mod calls it on the main thread at MATCH READY (dev flag first), with MM-3f's quiet invite armed.
   If not: fall back to opening the Matchmaking Settings menu for the host (still one click) and record why.
Acceptance (HUMAN, solo): dev button "invite now (test)" makes the game's lobby (`inviteParam`) with no menu and no Steam window.

### MM-6 — Duo and Squad with rivals: Steam ID → expedition player
Owner: OPUS to card in detail | Status: later | Settles: S9 U7
Match sizes (Leonardo, 2026-10-19): Duo = 3 teams of 2, Squad = 2 teams of 3, always 6 players; complete parties only.
With more than one team in a Duo/Squad match the roster (Steam IDs) must be mapped to expedition players on every client.

## Hawk crossing (M7, replaces separate spawns; design in PROJECT_PLAN.md §13)

T-028 (teleport) is done: solo run 4 and the Nucleus two-screen check passed on 2026-10-14 (TESTING §3); code merged with `main` on
branch `t-028-snapback`. The F3 team-spawn rule built there is parked (`team_spawns = false`).

### HAWK-0 — Questions for Leonardo and desk research
Owner: OPUS + HUMAN (answers only, no test) | Status: in progress (answers 2026-10-21 in PROJECT_PLAN §13)
1. **Done 2026-10-21.** Leonardo's answers (PROJECT_PLAN §13). The Nexus mod (`Z:\Tools\nr-mods\Spectral haw flying\`, study only)
   showed how the hawk works: a player behavior state in `c0000.hks` (TECHNICAL_NOTES §4b). Leaving a tree hawk early: hold the action
   button 0.5 s or press L3 (from the script; confirm in HAWK-1 d).
2. Opus, offline: decompress the `m60_*` map files (Oodle KRAK, `oo2core_9_win64.dll` from the game folder) and read the hawk route
   entities named by `SORTIE_STARTING_POINT_ST.birdRouteEntityId`: points, length, height. Also find the tile size, which gives the map
   edges in the world frame we write.
3. Opus, offline: get an exact decompile of the 1.03.3 `c0000.hks` (the ividyon/EldenRingHKS repo, or DSLuaDecompiler on our unpack),
   diff it against the Nexus mod's file to see exactly what the mod changed, and confirm that the start-hawk loop reads no input in
   vanilla. Needed for A1 (shipping our own script).
Done when: answers and findings are in PROJECT_PLAN §13 / MEMORY_MAP §8.

### HAWK-1 — Ride probe (dev build, solo)
Owner: SONNET/implementer → HUMAN | Status: todo | Depends on: `t-028-snapback` merged into `main`.
Build (console, `add-console-command` skill): `pos_shift <dx> <dy> <dz>` moves the LOCAL player by metres in the world frame (world
position kept, as `pos_go`). `pos_hold <secs>` holds the current world position every tick, then lets go. A ride logger records world
position, ground byte and speed every 100 ms from expedition load to 30 s after landing, and lists every character within 30 m of the
player during the ride (to find the hawk). The logger also runs on a Spectral Hawk Tree ride, to compare the two hawks.
HUMAN (solo, about 15 min, two expeditions): (a) during the start ride, `pos_shift 50 0 0`: does the player stay moved, does the hawk
follow, does the drop still happen on time? (b) during the start ride, `pos_hold 30`: is the player held past the 12 s ride, what does it
look like, and what happens at release? (c) after landing, `pos_shift 0 100 0`: confirm no fall damage. (d) ride one Spectral Hawk Tree
with no input, then a second one steering off the route, then try every button to leave it early (the trace shows which one works).
Acceptance: the answers and the log. They choose route 0, 1 or 2 of §13.

### HAWK-2 — Hawk crossing v1
Owner: OPUS to card in detail after HAWK-1 | Status: later
The first new feature, so the open halves of R2/R3 (typed views, `Feature` trait; ARCHITECTURE §6) are built with it.
