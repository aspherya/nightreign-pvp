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

## PvP rules

### T-040 — Rival revive: retry the re-down for 2 s, and say why when it fails
Owner: SONNET/implementer → OPUS review → HUMAN (Nucleus, solo) | Status: solo PASS (Nucleus run 2, 2026-10-21); FS watch item next | Depends on: — | Phase: M2 rules (bug from FS-UI2)
Goal: a rival's completed revive is always knocked down again, even when `[PlayerIns+0xB78]` is not readable at the UP moment, and the
player-build log shows enough to explain a miss next time.
Context: TESTING.md §3 FS-UI2 entry ("Defect seen"); the RCA (Leonardo approved fix A + D, 2026-10-21): `Core::apply_rule` /
`try_rule` (`crates/nrpvp/src/game/combat_state/rules.rs:459-518`) run **once** per UP event (`tracker.rs:256` `rule_should_fire`); on
`Err` they log and stop, so the revive stands. In FS-UI2 round 3 (host log 18:36:17.9–18:36:24.1) the one try hit `[PlayerIns+0xB78] is
not a plausible pointer`. Why the pointer was bad is UNKNOWN (leading hypothesis: the tick before, the bar read 0 with the state byte
still 1 and the downed rule wrote 40 back). `early_redown` (retry during state 2) stays **off**. Offsets: `re/MEMORY_MAP.md` §5/§6.
Steps:
1. **Pending re-down (A).** When `try_rule` fails at a rival UP event, keep `pending: Option<PendingRedown { addr, deadline_ms
   (now + 2000), tries, first_err, last_err }>` on `Core`. Each `step` (50 ms tick), before the other rules: if pending, LOCAL is still
   the same `addr` with a PlayerIns vtable, the state byte reads 0, call `try_rule` again (it keeps all its gates: plausible pointer,
   HP > 0 agreeing with mirror/PlayerGameData). On `Ok` → clear, log one line `rule: rival completed the revive meter of LOCAL; downed
   again after N tries (… ms late) (hp a -> 0, readback b)`. Cancel without writing (one line, why) when: the deadline passes; the
   state byte is 1 (a new down) or 2; LOCAL's addr changes / no vtable; HP reads 0 (dead or already down). A success on the first try
   behaves exactly as today (same line). The give-up line names tries, ms and the last error.
2. **Better error text (D, part 1).** The pointer error must carry the raw value: `[PlayerIns+0xB78] = 0x… is not a plausible
   pointer` or `… unreadable`. Same for the HP gate failure (auth / mirror / gamedata values).
3. **LOCAL state diagnostics in the player build (D, part 2).** One line per change of LOCAL's state byte (0/1/2 transitions only, not
   per bar change): `T-040 LOCAL state a -> b: bar …, clock …, pause …, [+0xB78] = 0x…, hp …`. Player build and dev build both; at
   most 30 lines per minute (count the rest, report `(n suppressed)` on the next line). Also one line when the downed rule restores a
   bar that had read ≤ `BAR_HIT_EPS` (the suspicious "bar 0 while state 1" frame). Do not change the existing 5 s rate limit of the
   other rule lines; the pending/give-up lines must **not** be swallowed by it (they are rare; log them always).
4. **Dev-only console test switch.** A console command (use the `add-console-command` skill; dev build only, `#[cfg(feature = "dev")]`
   path) `revive_fail <n>`: the next `n` `try_rule` calls return `Err("forced by console")` without reading memory. `revive_fail` with no
   argument prints the remaining count. Lets a Nucleus run force the first try(s) to fail. CONSOLE.md table updated by the skill.
5. Unit tests in `combat_state/tests.rs` (fake `CombatMem`): pointer null for 3 ticks then valid → re-down on try 4, one success line;
   still invalid at 2 s → give-up line, nothing written; new down (state 1) during pending → cancelled, nothing written; HP 0 during
   pending → cancelled; first try OK → unchanged behaviour (existing test still passes, update the one at `tests.rs:469-479` that locks
   in "log and stop" to the new pending behaviour); state-change diagnostics lines (and the 30/min cap); `revive_fail` count.
Deliverables: `combat_state/{rules,tracker?,tests}.rs`, console command files, `CONSOLE.md` row. No doc edits beyond CONSOLE.md
(Opus updates STATUS/TESTING/MEMORY_MAP).
Acceptance (implementer): `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings` for dev and
`--no-default-features` (player) builds, `cargo fmt --check`; `scripts\build.ps1` OK.
Acceptance (HUMAN, Nucleus 2 instances, ~10 min): deploy with `team_size = 1`. Down player A; `nrcon -i <A> revive_fail 3`; player B
revives A (rival). Pass = A gets up and is knocked down again within ~0.2 s (log `downed again after 4 tries`), plus `T-040 LOCAL state`
lines for 0→1→2→0. Then once without `revive_fail`: normal first-try re-down unchanged. Then `revive_fail 100`: give-up line after
2 s, A stays up (proves the deadline). The real trigger (two-PC lag) goes into the next FS runbook as a watch item.
Out of scope: fix B (not restoring a bar that already read 0) and `early_redown`; any change to the downed rule's hit handling.
Result (implementer + Opus review, 2026-10-21, uncommitted on `t-028-snapback`): `Core::pending_step` / `PendingRedown` in
`combat_state/rules.rs`, `state_diag` + bar-0 line, error text with the raw value, `revive_fail` (dev console; `forced_fails` inert in
the player build). Lines: `… nothing was written yet: <err>; retrying for 2000 ms`, `… downed again after N tries (M ms late) …`,
`… gave up after N tries in M ms …`, `pending re-down of LOCAL cancelled after N tries …: <reason>`, `T-040 LOCAL state a -> b: …`,
`T-040 LOCAL bar read 0 while state 1 …`. `revive_fail 3` = re-down lands on try 4 (the UP-event try counts). Tests 356 → 365 nrpvp,
fmt/clippy (dev + player) clean, `dist\nrpvp.dll` built and deployed to Nucleus with `-TeamSize 1` (18:33 local). Note:
`cargo test -p nrpvp --no-default-features` has 21 failures in config/team tests that assume the `dev` feature; config is untouched by
this card and CI runs only `dev_mode_needs` in that build (pre-existing, not chased). Retries happen on the combat tracker's tick
(~100 ms), so "M ms late" reads in steps of ~50–100 ms.
**Run 1 found the real root cause** (TESTING §3): `[PlayerIns+0xB78]+0x70` is not always the health. Opus added the data-module
route `[[PlayerIns+0x1B8]+0x0]+0x140` (gate: HP > 0, HP and max HP equal to PlayerGameData `+0x10` / `+0x14`; `data_module_hp` in
`rules.rs`, constants in `game/layout.rs`), the old route as fallback; 367 nrpvp tests. Run 2: PASS both directions.

---

### T-041 — No get-up at all: stop a rival's hit from completing the revive bar inside the game
**Handoff (read first): `tasks/T-041-no-getup-handoff.md`** — root cause found (the player script revives when the bar touches 0); next: hook
the hit store `exe+0x66DFC4` so a rival's hit never lowers the bar.
Owner: OPUS (RE) → SONNET/implementer → HUMAN (Nucleus, solo) | Status: dev prototype `revive_block` PASS one direction (Nucleus 2026-10-21, TESTING §3: no stand-up on either screen); always on in both builds since build 1155C5D9204B3ADA (free hardware slot, 0/1 in the player build, while LOCAL is downed); next: other direction, teammate check (`team_size = 2`) |
Depends on: T-040 | Phase: M2 rules
Goal: when a rival's hit would empty a downed player's revive bar, the bar never reaches 0, so the game never starts the get-up
(no "stand up, then drop"), and the gauge never flickers on either screen.
Why: our rule polls every 50 ms and undoes a rival's hit afterwards; a hit big enough to empty the bar makes the game start the get-up
in the same frame, and T-040 run 3 showed an HP write during the get-up does not cut it (TESTING §3).
Steps:
1. **Find the writer (dev trace, Nucleus, ~5 min):** a hardware **data** breakpoint (write) on LOCAL's `PlayerIns+0x714` (revive bar)
   while LOCAL is downed; log RIP, registers and the return-address stack of every write (reuse the `hwbp.rs` VEH pattern; slots 2/3
   are free in an expedition because Commence/labels only matter at the Roundtable — the trace may borrow them, dev only).
2. **Read the function statically** (capstone): where the hit's amount is subtracted, where `bar <= 0` leads to state 2 (`+0x710`), and
   where the attacker handle is at that moment (`+0x1A0` on the owner's game).
3. **Intercept (no code patch):** hardware execute breakpoint just before the subtraction / the `<= 0` branch; in the VEH, if the
   attacker is a RIVAL, keep the bar (and clear the pause, take `rival_hit_seconds` off the clock, as T-039 does today); teammates and
   enemies untouched. The poll-based rule stays as the fallback.
4. Slot budget: execute breakpoint armed only while LOCAL is downed in an expedition; never at the Roundtable (Commence/labels) or while
   the quiet invite needs slots 0/1.
Acceptance (HUMAN, Nucleus): a rival empties the bar in one hit → the downed player stays down, no get-up, no gauge flicker on either
screen; a teammate (team_size 2 run) still revives normally.
Out of scope: changing what a rival's hit does to the bleed-out clock (T-039 rule stays).

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

**Done 2026-10-21 (FS-UI2 PASS, TESTING §3):** **MM-4** (Solo queue, player build), **UI-1** (Battle Royale in the table menu, steps
1–5) and **UI-2** (status as blinking game messages). Cards removed; text in git (`git show c727293:.claude/TASKS.md`). The menu and
text addresses they found are in `re/MEMORY_MAP.md` §13; the flow is in `MATCHMAKING.md` §3. Left over as polish (not cards yet): the
live count inside the searching box (UI-1 C3), and messages that never show because the next state arrives first (TESTING §3, FS-UI2 entry).

### MM-3g — No host click: trigger Invite Members from the mod
Owner: OPUS (RE) → SONNET/implementer → HUMAN (solo) then FS | Status: ON HOLD until S10 is reviewed (brief: `tasks/MM-3g-MM-7-zero-click-autostart.md`) | Depends: MM-3f, UI-0 Q3
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

**Direction 2026-10-21 (STATUS §8):** no Steam window on any screen, everyone arrives ready and the match starts itself, the host is
invisible and chosen by the matchmaker; host leaving = match ends cleanly for now, host migration is the goal. Order: MM-3g and MM-7
share one RE spike (both call game menu code from the mod); MM-8 and MM-9 are independent; S10 runs alongside as research.

### MM-7 — Auto-start: the expedition starts by itself once everyone is here
Owner: OPUS (RE) → SONNET/implementer → HUMAN (solo) then FS | Status: ON HOLD until S10 is reviewed (brief: `tasks/MM-3g-MM-7-zero-click-autostart.md`) | Settles: S9 U6
At `ALL PRESENT` (and every player loaded, `players` = roster), the host's mod starts the departure with no Commence press. Route A:
call the Commence checks `exe+0x9EEBE0` (ordinary code, `re/MEMORY_MAP.md` §13) with a valid menu object, so find what builds that
object without the menu open; route B: an EMEVD instruction that starts a departure (UNKNOWN whether one exists). Also check whether a
SC party has any "ready" state of its own. Players see "Match starting…" (UI-2 message) instead of "host: start the expedition".
Solo-testable part: the call path with a dev console command in a solo SC session (Commence by console = departure).

### MM-8 — Host chosen by the matchmaker (best connection)
Owner: OPUS (spec) → SONNET/implementer → FS | Status: ready | Depends: —
Today the queue lobby owner becomes host (luck). Each member publishes its Steam ping location (`ISteamNetworkingUtils`
`GetLocalPingLocation` → member data); the owner picks the member with the lowest worst-case estimated ping to the others
(`EstimatePingTimeBetweenTwoLocations`), ties by Steam id, and writes `host`. FakeSteam tests for the pick; the player never sees who
hosts (no host wording in messages once MM-3g/MM-7 land). Verify the Steam API calls exist in the game's `steam_api64.dll` first.

### MM-9 — Host leaves mid-match: end the match cleanly (v1), find what SC does
Owner: OPUS (runbook) → HUMAN (FS) → SONNET/implementer | Status: ready (FS observation first)
FS step: in an expedition the host quits (title screen, then Alt-F4 in a second round); log and note what each joiner's screen does
(kicked to title? Roundtable? stays in a broken world?) and what SC shows. Then v1: the joiners' mods detect the host's loss and show
"The host left - match over" and return everyone to idle. Feeds S10 (host migration).

### S10 — Study Seamless Co-op: sessions, the 6-player cap, host loss (research, read-only)
Owner: OPUS + SONNET/researcher | Status: done in its own session, review pending (`tasks/S10-seamless-coop-study.md`) | Rule: hard rule 3 (study for interop only; never patch, copy or redistribute `nrsc.dll`)
Questions: (1) how SC creates and joins a session (its Steam lobby wrapper, `MEMORY_MAP` §12, and what it hooks in the game's session
code); (2) where the 6-player cap lives; (3) what happens on host loss and whether the game or SC has any host-migration path; (4) a
sized estimate of our own session layer if SC ever blocks us. Static analysis of `nrsc.dll` exports/imports/strings and the game hooks it
installs, plus runtime traces with our existing hardware-breakpoint tooling. Output: `research/S10-seamless-coop-internals.md` with
evidence tags and a go/no-go on host migration.

## Hawk crossing (M7, replaces separate spawns; design in PROJECT_PLAN.md §13)

T-028 (teleport) is done: solo run 4 and the Nucleus two-screen check passed on 2026-10-14 (TESTING §3); code merged with `main` on
branch `t-028-snapback`. The F3 team-spawn rule built there is parked (`team_spawns = false`).

HAWK-0 (questions and desk research) is done (2026-10-21): answers and design in PROJECT_PLAN §13, engine facts in TECHNICAL_NOTES §4b
(hawk = player script state) and §4c (routes in `m60_00_00_99.msb`), tools in `experiments/HAWK-0/`.

### HAWK-1b — Long start route, data only (no DLL code)
Owner: OPUS builds → HUMAN (solo, about 5 min) | Status: todo
Build: a script (`experiments/HAWK-0/`) that writes a patched `m60_00_00_99.msb.dcx`: every start route keeps its start point; its
waypoint and end point move along the same direction to 100 m before the map edge, at the start height (no sinking). Recompress with
Oodle Kraken (same DCX header), add it under `map/mapstudio/` in `dist\gamedata` (dev launch only, not the player zip).
HUMAN: launch with `scripts\launch.ps1`, start an expedition, watch the start ride. Report: does the hawk fly past the usual drop
spot, how far and how long (the log has the ride), does it drop you at the end, anything odd (clipping, falling through, a crash on
loading). Acceptance: the hawk follows the long route and drops at its end. If the game ignores or rejects the file: record it, then B2.

### HAWK-A2 — Find what answers `GetBirdEventID` (env 377) in the exe
Owner: OPUS (static, capstone) → HUMAN check later | Status: todo
Find the HKS env dispatcher in `nightreign.exe` 1.3.3.0, the case for 377 (and 395 `GetBirdActionID`), and the object field it reads.
Done when: the field is in MEMORY_MAP with its chain, and a dev console command can read it during a ride (checked in HAWK-1).

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
