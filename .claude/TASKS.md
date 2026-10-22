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

**Done 2026-10-22: T-040 and T-041 accepted** (Leonardo: "worked very well"; TESTING §3). A rival's hit on a downed player is now
cancelled inside the game (hardware breakpoint on the bar store `exe+0x66DFC4`): no stand-up, no gauge flicker, the bleed-out penalty
stays; teammates revive normally (3-instance run, `team_size = 2`). Fallbacks still on: the revive-event block (`exe+0x675159`) and the
T-040 re-down (data-module HP, 2 s retry). Cards removed; text in git (`git show 3384bca:.claude/TASKS.md`); full story and addresses in
`tasks/T-041-no-getup-handoff.md` and MEMORY_MAP §6. Follow-ups (no card yet): a player zip with this build for the next FS (real
two-PC lag); confirm in the player build that the hooks' two hardware slots (0/1, only while downed) never clash with the quiet invite (0/1, only at
the Roundtable at match ready).

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

### HAWK-1b — Long start route: land check
Owner: OPUS → HUMAN | Status: open (2026-10-22)
Done: the game loads a patched `m60_00_00_99.msb.dcx`; the hawk flies the long route at ~17 m/s and the engine drops the player at its
end (TESTING §3, runs 1–2; tool `experiments/HAWK-0/patch_routes.py` via `scripts\build-gamedata.ps1 -HawkRoutes`, ends 50 m inside
the land outline; `package-release.ps1` refuses `map\`). Open: 2 of 3 drops fell to y −26 and the second killed Leonardo (run 1 off the island; run 2 ride B ended
among vanilla drop spots, so likely ground collision not loaded far from the vanilla drop, HYPOTHESIS). **Decision 2026-10-22
(Leonardo): one single route, always: the map diagonal, bottom to top** (every start route gets the same points). Next build: that
diagonal, ending exactly on a vanilla drop spot; acceptance: three rides in a row land on ground. Parked until the wrap-up and the
Offline with Bots mode are in place.

### HAWK-A2 — Find what answers `GetBirdEventID` (env 377) in the exe
Owner: OPUS (static, capstone) → HUMAN check later | Status: todo
Find the HKS env dispatcher in `nightreign.exe` 1.3.3.0, the case for 377 (and 395 `GetBirdActionID`), and the object field it reads.
Done when: the field is in MEMORY_MAP with its chain, and a dev console command can read it during a ride (checked in HAWK-1).

### HAWK-1 — Ride probe (dev build, solo)
Owner: SONNET/implementer → HUMAN | Status: todo (unblocked: `t-028-snapback` is in `main`, SMOKE-3 PASS 2026-10-21).
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
