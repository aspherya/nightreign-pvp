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

## Test tooling

The orchestrator (`nro`, scenarios, puppet players) is in this repo and is the primary test route: `TOOLS.md`, `orchestrator/README.md`.
ORCH-1 (virtual pad) is blocked and parked (the game scans XInput pads only on a device change; keyboard and mouse through `nro desk`
work). Open ideas: `puppet_revive`, puppets that attack, scenarios that wait on the log file instead of the 300-line console tail.
Every card whose acceptance can be a scenario names it (`scenarios/<name>.toml`) and is done when `scripts\regress.ps1 <name>` is PASS.

### NRO-MI — `nro` launches 2-3 real instances (Nucleus replacement)
Owner: OPUS (plan) → SONNET/implementer | Status: todo, waits for Leonardo's go (spike S0 first) | Depends on: RW-9 merged
Goal: Claude drives and sees every player's view (host/client, Duo/Trio) with no human and no Nucleus.
Context: full design, spikes S0-S4, risks: [orchestrator/MULTI-INSTANCE.md](orchestrator/MULTI-INSTANCE.md).
Acceptance: `scenarios/mi-join.toml` then `mi-duo-sc.toml` PASS (both consoles `players` = 2) + a `testing-log/` entry.
Out of scope: real Steam lobbies, real lag, shipping an emulator.

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
Owner: OPUS + SONNET/researcher | Status: done; merged into `main` 2026-10-22 (`research/S10-seamless-coop-internals.md`), OPUS review pending | Rule: hard rule 3 (study for interop only; never patch, copy or redistribute `nrsc.dll`)
Questions: (1) how SC creates and joins a session (its Steam lobby wrapper, `MEMORY_MAP` §12, and what it hooks in the game's session
code); (2) where the 6-player cap lives; (3) what happens on host loss and whether the game or SC has any host-migration path; (4) a
sized estimate of our own session layer if SC ever blocks us. Static analysis of `nrsc.dll` exports/imports/strings and the game hooks it
installs, plus runtime traces with our existing hardware-breakpoint tooling. Output: `research/S10-seamless-coop-internals.md` with
evidence tags and a go/no-go on host migration.

## Offline mode (decided 2026-10-22; first new feature, so it carries the open R2/R3 halves)

Leonardo's decision: a separate **Offline** mode for solo play and solo testing, so `br_menu` / `matchmaking` never have to be switched
off again. The solo test mode (no queue, no second player) is done; rivals for tests are the orchestrator's puppets.

**OFF-1 done (2026-10-22, solo PASS, TESTING §3):** `mode = "br" | "offline"` + console `mode`; Offline lets Commence Solo run the
game's own solo expedition. Card removed; text in git (`git show 4e0180c:.claude/TASKS.md`).

### OFF-2 — Offline mode in the game menu (players)
Owner: OPUS (RE) → SONNET → HUMAN | Status: later (after OFF-1)
A player-visible way to pick Offline in the stock expedition menu (e.g. an extra Number of Players entry or a Roundtable message
choice, UI-1/UI-2 tools). Design after OFF-1 is in use.
Sketch (2026-10-22, not designed): the Number of Players list already shows our Solo / Duo / Trio through the label hook
(`menuflow`, hwbp slot 3). The cheapest entry is a fourth choice there is none of: so either (a) reuse a value: Commence pressed
twice within a few seconds with Solo = Offline (no new UI, but hidden), or (b) a Roundtable event (UI-2 tools) that asks "Battle
Royale or Offline?" when the player first walks to the table and sets `mode` through a flag the DLL reads (flag group 10007).
(b) is the player-friendly one; it needs a yes/no dialog event (2007 family) whose answer lands in a flag: check that first.

**Bots are not a game feature** (Leonardo, 2026-10-24): OFF-3 (research, done), OFF-6, OFF-7 and OFF-8 are cancelled; the automatic
offline rival bots (OFF-5) were removed from the code. The game's bot manager stays as the engine of the orchestrator's puppet players
(`game/puppet.rs`, console `bots` / `bots_clear` / `puppet_*`); `research/OFF-3-bots.md` and MEMORY_MAP §15 / §18 keep the facts.

## Match flow: nights with no boss (M5; research `research/M5-no-boss-nights.md`, `research/M5-8-death.md`)

**Built earlier (the rounds were replaced 2026-10-24, see RW below; engine facts TECHNICAL_NOTES §4e/§4f, MEMORY_MAP §16/§17):** M5-1..M5-4 (no boss,
no minions, clock console, night freeze facts), **M5-5 / M5-6 / M6-1 nights are elimination rounds** (`features/night_match.rs`: one team
left = night decided, only the winner healed, losers held down until the morning, night 2 win/loss endings; solo, Nucleus and
orchestrator puppets PASS incl. night 2 by count, 2026-10-24), **M5-8** research (SpEffect 102130 = die at once; the comeback can be
held and released), **M5-9 Solo real death** (`features/night_match.rs`, `game/deathhold.rs`: die-at-once at night, held until the
morning, released through the gate, moved to a random day-1 place after the Day-2 clock; one-PC PASS 2026-10-24).
Rule decisions (M5-D): `DECISIONS.md`.

### M5-9 / M5-5 — superseded
The elimination rounds (night = a round, losers held until the morning, die-at-once in Solo nights) were replaced on 2026-10-24 by the RW
rules (DECISIONS "After the first real match"; section "Rework after the first real match" below). The hold (`game/deathhold.rs`) stays: it
keeps a defeated player dead. What FS-M59 left open (the hold in a real session, the other screen) is in the FS-RW1 runbook (TESTING §6).

### M5-7 — Spectate a player still fighting (camera)
Owner: OPUS (RE) | Status: todo (a defeated player (RW-1) is held dead and gets the defeat screen after `defeat_exit_secs`; with this camera they could watch instead)
A defeated player watches a living player until the match ends or they choose to leave. Must let a dead
player open the menu at least to quit (vanilla blocks Esc while dead; Leonardo 2026-10-23). Needs the engine's follow camera: find the
camera's target (the local PlayerIns) and whether pointing it at another PlayerIns copy is safe. Static RE first (camera classes in
RTTI / FD4 names), then a solo dev command that swaps the target to a bot (offline mode) as the proof.

## Hawk crossing (M7, replaces separate spawns; design in PROJECT_PLAN.md §13)

T-028 (teleport) is done: solo run 4 and the Nucleus two-screen check passed on 2026-10-14 (TESTING §3); code merged with `main` on
branch `t-028-snapback`. The F3 team-spawn rule built there is parked (`team_spawns = false`).

HAWK-0 (questions and desk research) is done (2026-10-21): answers and design in PROJECT_PLAN §13, engine facts in TECHNICAL_NOTES §4b
(hawk = player script state) and §4c (routes in `m60_00_00_99.msb`), tools in `experiments/HAWK-0/`.

HAWK-A2 / HAWK-3 are done (2026-10-22, TESTING §3 HAWK-3 run 1): the env switch is found and swapped safely, `r8` = PlayerIns,
`hawk_drop` and `hawk_tree` work in the game (Leonardo). Design v1 that follows from it: PROJECT_PLAN §13 "v1 design". HAWK-1 (ride
probe) is dropped: the env trace answered what it was for. HAWK-1b is done (2026-10-22, TESTING §3 run 3: one diagonal line "B"
for every start route, three rides PASS); HAWK-4 is done (2026-10-22, TESTING §3 HAWK-4 runs 1-2: cruise lock, dive, start
animation cut; Leonardo: good enough). The line is built by `scripts\build-gamedata.ps1` (by default since HAWK-2; TECHNICAL_NOTES §4c).

### HAWK-2 — Hawk crossing v1 (player feature)
Owner: OPUS → SONNET/implementer → HUMAN | Status: **solo smoke PASS 2026-10-22** (TESTING §3; Leonardo: "how I wanted it").
Merged (PR #2). Open: merge (Leonardo), the Spectral Hawk Tree check (step 4 below, not
done yet), and the FS with two players; then remove this card.
Built: `game/hawk/` in both builds (`mod.rs` hook + swap + ride phases; `dev.rs` console and traces, dev build only). Settings
`[hawk]` (`enabled`, `dive_hold`, `dive_secs`, `dive_boost`, `start_cut`; defaults = HAWK-4's accepted values, on) read at
startup; the `hawk_*` commands are session overrides. Only LOCAL: once the main loop knows LOCAL's PlayerIns the hook acts on that
character only (before that, the HAWK-3 rider rule). A native tree ride (Spectral Hawk Tree) ends the lock; the pass-through swap
stays for the session (inert outside start rides). Route file: built by default (`-VanillaRoutes` to skip), required by
`package-release.ps1` and checked by the launcher. **Not a `Feature` yet:** the trait exists on `main` (OFF-1)
(`claude/offline-mode-ccabe0`); moving it there = `configure` → constructor, `tick` → `Feature::tick` with `local` from the `World`.
Solo smoke (Leonardo, player terms): from the worktree folder run `scripts\launch.ps1` (its `dist\nrpvp.toml` is player mode with
`matchmaking = false`, so there is no console and Commence starts a normal solo expedition). (1) Commence Solo; the hawk takes
you without the grab animation and flies the long diagonal line; moving the stick briefly does nothing. (2) Hold the stick in any
direction: the hawk leaves the line, goes faster and down; steer to a spot; after about 7 s it drops you. (3) Second run: hold
action (or L3) during the cruise: you drop at once. (4) In the expedition, ride a Spectral Hawk Tree if one is near: it must
behave as in vanilla (steerable, no lock). PASS = all of it with no nrcon command; `dist\nrpvp.log` shows `HAWK-2 hawk crossing
ON`, `HAWK-3 swap ON`, `HAWK-4 phase none -> cruise` (and `-> dive`).
What: PROJECT_PLAN §13 v1 as a player feature (cruise lock + dive + start cut with the accepted defaults; the console
commands stay dev-only): `hawk.rs` leaves the dev-only build, `[hawk]` settings in `nrpvp.toml`, on by default
in Battle Royale and offline mode, only LOCAL changed, swap removed when the expedition ends. The route file goes into the player
game-data package (lift the `map\` refusal for that one file). The first new feature, so the open halves of R2/R3 (typed views,
`Feature` trait; ARCHITECTURE §6) are built with it.
HUMAN: one solo smoke; then the next FS: both players cruise, dive and drop where they want; each sees the other land.

## Leonardo's list (2026-10-24, tracked here; update the state column when something changes)

Test routes: **1 PC** = automated scenario (`nro`, one game), **2 inst** = two real emulated-Steam instances on one PC (`nro`, `mi-*` scenarios,
[orchestrator/MULTI-INSTANCE.md](orchestrator/MULTI-INSTANCE.md)), **FS** = Friend Session on two PCs (TESTING §6).

| # | Item | Card | State (2026-10-27) | Still to test |
|---|---|---|---|---|
| F1 | Respawn far from all players | RW-7 | Built, unit-tested | Two instances: where each one starts and respawns after a mob death (2 inst), then FS |
| F2 | Runes shared only inside the own team (solo: himself) | RW-8 | **Not built** (engine split code UNKNOWN) | Needs the rune-count offset and the share path; test with 2 inst (a second player kills, watch both rune counts) |
| F3 | No remote location (map, compass), name tags only on the target | RW-9 | **Built and seen with 2 inst**: map, compass, party bar, name tag, edge arrow, placed map pins and their 3D marker hidden for rivals (PRs #21, #22) | Pin toast "Ally is showing location" still leaks the area ([re/TOAST_KILLFEED.md](re/TOAST_KILLFEED.md)); lock-on name tag; Duo/Trio teammates; FS photo |
| F4 | Enemy difficulty by team size (Solo / Duo / Trio values) | RW-10 | Table found and rewritten on one PC (2 copies, 168 rows); effect not measured | Measure enemy HP / damage in Solo vs 2-player sessions (2 inst, console `peek`), then FS |
| D1 | Defeat / loss when killed by a player or the tide, then spectate or quit to the Roundtable | RW-1, M5-7 | **Done on 1 PC** (`defeat-rival`, `match-loss`, `spectate`, `esc-defeated`); spectate seen in Nucleus | 2 inst: the other screen (held body, defeat screen, spectate), real hits; spectator camera smoothness |
| D2 | Defeated players drop items (red phantom) and runes on the ground | RW-3 | **Not built** (route: `research/S2-inventory-drops.md` §7) | RE first, then 2 inst |
| D3 | Death by mob / boss respawns normally with items | RW-2 | **Done on 1 PC** (`defeat-mob`) | none beyond FS |
| D4 | Boss dormant powers / weapons only to the last hitter's team | RW-5 | **Not built** (`disableMultiDropShare` param hint) | RE first, then 2 inst with a boss |
| D5 | Day 1 / night 1 free of the tide, tide starts on Day 2 | RW-4 | **Done on 1 PC** (`day1-free`) | FS (real clock over the network) |
| D6 | Match ends when one team is left, even on Day 1 | RW-1 | **Done on 1 PC** (`match-win`, `match-win-end`, `match-loss`) | 2 inst with real defeats; FS |
| D7 | Private match by password, outside matchmaking | RW-6 | Unit-tested only (`private_code` in `nrpvp.toml`); the in-game "Multiplayer Password" field is the planned player route (RW-6 follow-up) | 2 inst: same code meets, different codes do not (`mm_queue` works over the emulator, `mi-queue`) |

Next in this list: **F2 / RW-8 (runes)**, then D2 / RW-3 and D4 / RW-5 (both need RE), then the 2-instance checks of F1, F4, D1, D7.

## Rework after the first real match (RW; Leonardo's notes 2026-10-24, rules in DECISIONS)

Built and tested on one PC 2026-10-24 (scenarios `day1-free`, `defeat-mob`, `defeat-rival`, `match-win`, `match-win-end`, `match-loss`;
testing-log): **RW-1** defeat for good + early win, **RW-2** a non-player death is vanilla, **RW-4** free Day 1 (the code is in
`features/night_match.rs`, header comment). Built, unit-tested, not yet seen on two PCs: **RW-6** private match, **RW-7** respawn far, **RW-10**
enemy scaling. Their two-PC checks are in the FS-RW1 runbook (TESTING §6); the cards come off when FS-RW1 passes. Open, each waiting for
what a second real client shows (research tools: console `memscan`, `peek`; findings so far below):

- **RW-3 — Defeat drops** (items through a red phantom, runes on the ground). Route and unknowns: `research/S2-inventory-drops.md` §7. Nothing built.
- **RW-5 — Boss drops (and the "GREAT ENEMY FELLED" banner) only for the last hitter's team.** Branch `rw5-boss-drops`, state 2026-10-30, **no working filter yet**. Test aids (dev console): `boss_list`, `boss_go n|0xhandle`, `boss_kill`, `boss_info`, `boss_lots`, `boss_hp <n|handle> <hp>` (note: HP 1 hides a boss from the HP > 1000 list; poke the HP instead), `hitkill`, `god` (now raises max HP to 1,000,000), `trace_read`; `scenarios/tmp-rw5-explore.toml` keeps two games up. **Observed (two instances, Leonardo and Claude, 2026-10-30):** (1) a **real damage kill** (boss at 1 HP, one hit) makes the Dormant Power orb (glowing pickup with a marker) and the "GREAT ENEMY FELLED" text appear on **both** games, rivals included; an HP-write kill (`boss_kill`, `hitkill`) showed no orb; (2) rune credit works as RW-8 says (killer's game +32,499, the other +0, `1 blocked`); (3) blanking the boss's `NpcParam` reward lots (`+0x6C`, `+0x2D8`, `+0x338`; `game/runeshare.rs`, on the blocked game) in the death-credit hook did **not** stop the orb on that game: the drop is not read from those fields at that moment; (4) killing the Nightlord-class bosses (npc 46400010, 75700010; HP 12017 / 22656) ends the expedition and the game exits: never `boss_kill` index 0 blindly; (5) the read watch (`trace_read`, DR7 RW = 11) crashed the game every time the watched field was read, even after the handler accepted RW 11 (cause UNKNOWN, do not use it). **Static finding:** the banner and the drop come from event scripts that every game runs for itself (Network Sync disabled): `common_func` 90015002 (boss defeated: `Display Text Effect` banner, `Increment Team Bosses Killed`, network flag) and the 28 events with `Handle Miniboss Defeat` `2003[15]` (HP <= 0, wait 4 s, `Play SE`, dead, **Handle Miniboss Defeat**, set flag) in `m34_*`, `m35_90`, `m46_50/60/80` and the overworld tiles `m60_*`. **Plan:** (a) extend `scripts/emevd_add_events.py` with a "guard" patch that inserts `SKIP IF Event Flag(local flag F OFF, 1 line)` before the banner and before `Handle Miniboss Defeat` in those events (parameters re-indexed), ship the changed map files in the game-data package; (b) the DLL sets local flag F at the boss's death-credit hook (4 s before the events reach the instruction) to "this team is credited"; (c) the last hitter must reach every game: a game records attackers only for hits it applied (MEMORY_MAP §21) and the boss owner is whoever fought it, not always the host (seen: the owner was player 2 once, player 1 once), so the owner's verdict needs a transport (lobby member data written per boss handle on each attacker change, or a network-synced event flag set from one of our own events); until then a Solo player who hits a boss owned by a rival's game gets nothing. Open question: which of the two event families the castle bosses (npc 40210030, 46900020) use. Param hint: `disableMultiDropShare` exists on goods / protector / weapon rows (`Paramdex NR`), the sharing itself is engine code UNKNOWN.
- **RW-8 — Runes only inside the own team.** **Built and accepted on one PC** (branch `rw8-runes`, 2026-10-28): every game credits its own player for every death it processes (`exe+0x3DE710` → `exe+0x668380`, no killer check, MEMORY_MAP §21); the killer's game records the attacker (`ChrIns+0x1A0`), the other game reads `-1` (it applied no hit). `game/runeshare.rs` (both builds) passes amount 0 when the last attacker is a rival, or — with no teammate (Solo) — when none is recorded. Console `runeshare`, `runes_mark`, test aids `mobs`, `mob_go`, `mob_kill`. **Acceptance:** `scenarios/mi-rw8-runes.toml` **PASS 3/3** (runs `1793219963/-969`, `1793220173/-180`, `1793220436/-441`, 2026-10-28): player 1 `+1040` / `0 blocked`; player 2 `+0` / `1 blocked = 100 rune(s)` and `Unknown -> BLOCKED`; control `runeshare 0` pays player 2 (`+1040` / `+1560`). **Open:** Duo / Trio teammates (an unrecorded killer stays vanilla there — it may be a teammate's; needs the killer from the owner's sync, UNKNOWN); the message credit `exe+0x51DC0B` (enemy not loaded here) is counted, not filtered; FS-RW1 step 7(a) on two real PCs (photos of both rune counters). Remove the card when the FS check passes. **Review 2026-10-30 (code only, 494 unit tests pass):** hooks are checked before patching and fail safe; **untested risk:** in Solo a *client* who kills an enemy owned by the host sees no attacker recorded (`Unknown`) and gets no runes — the scenario only has the owner kill. Check with a client-side real kill (or find where the host's verdict reaches the client) before FS-RW1.
- **RW-9 — No remote map / compass position, nametags only on the target.** **Built for Solo, Duo and Trio, seen with two real instances on one PC (2026-10-27, branch `test/rw9-multi`):** `game/hudfilter.rs` hooks three HUD lookups (MEMORY_MAP §20 "Rival filter") so a rival is on no map pin, compass mark, party bar or name tag (incl. the edge arrow); LOCAL's lock-on target keeps its name tag; teammates are not rivals and stay. `maphide` (Solo, whole list) is only the fallback. Console `rivalhud`, test aid `tp_player`. Life bars from lock-on / hits stay vanilla (DECISIONS). Seen: with the filter on, no party bar / name tag / edge arrow on both screens (run `1793065578`, `h1_hawk`, `h2_hawk`); compass and map pin gone with `maphide` (run `1793064664`). **Acceptance:** `scenarios/mi-rw9-rivals.toml` PASS twice (not run yet: Steam was running), incl. the lock-on step (`PlayerIns+0x768` not yet read live). **Map markers (2026-10-27, branch `rw9-markers`):** a pin a rival places arrived and showed on the map, compass and as a 3D marker; two more hooks (MEMORY_MAP §20 "Map markers") now keep a rival's pins off all three (own and teammates' stay), `scenarios/mi-rw9-markers.toml` PASS (testing-log 2026-10-27). Still shown: the "Ally is showing location" toast with a landmark icon (producer UNKNOWN). **Left after that:** the toast, Duo / Trio teammate case (needs `team_size = 2` over the queue or a 4-instance PC), FS photo on two PCs. Remove the card when the scenario passes and a teammate run shows the teammate still marked. **Open:** the toast "Ally is showing location" still reveals the area to a rival; notes and plan (and the killfeed idea built on the same mechanism): [re/TOAST_KILLFEED.md](re/TOAST_KILLFEED.md).
- **Spectating (M5-7)**: built in v1 without camera RE (see STATUS): the held dead body, which other players do not see (Nucleus 2026-10-23), is moved next to a living player and the follow camera goes with it. F10 leaves. Not seen on two PCs.
- **RW-6 follow-up:** the game's own Matchmaking Settings has a "Multiplayer Password" field; the text lives in a `CS::CSMenuSimpleSaveDataChunk<3, MENU_INPUT_HISTORY>` object (input history, found with `memscan`) and in a second structure; which one the menu reads at Commence is UNKNOWN. Reading it would replace `private_code` in the toml (no config editing for players).
- **Spectator camera smoothness (next):** the moved body makes the camera jump about 20 times a second. Idea: find what writes `ChrExFollowCam+0xA0..` (the followed position, newest entry; console `trace_write <addr>`) and feed it another player's position or target directly, leaving the body where it died. Fallback: keep the moved body (committed, 2026-10-25).
- **ESC / pad while defeated (works on one PC, 2026-10-25; scenario `esc-defeated` PASS, pad confirmed by Leonardo).** `game/menugate.rs`: Esc or pad Start opens the game's menu for a defeated, held player (sets the open request, MEMORY_MAP §19); pad d-pad right / left switch the watched player (mouse buttons too; all off while a menu is open). The pad is read from the game's own input bits (XInput is blind inside the game). Quit through the menu while dead works (one PC, Leonardo). Left: a Nucleus / FS confirmation, other pad layouts (bits measured on a Steam Input Xbox pad). Remove this card after that.
