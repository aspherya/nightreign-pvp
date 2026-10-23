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
Owner: OPUS + SONNET/researcher | Status: done; merged into `main` 2026-10-22 (`research/S10-seamless-coop-internals.md`), OPUS review pending | Rule: hard rule 3 (study for interop only; never patch, copy or redistribute `nrsc.dll`)
Questions: (1) how SC creates and joins a session (its Steam lobby wrapper, `MEMORY_MAP` §12, and what it hooks in the game's session
code); (2) where the 6-player cap lives; (3) what happens on host loss and whether the game or SC has any host-migration path; (4) a
sized estimate of our own session layer if SC ever blocks us. Static analysis of `nrsc.dll` exports/imports/strings and the game hooks it
installs, plus runtime traces with our existing hardware-breakpoint tooling. Output: `research/S10-seamless-coop-internals.md` with
evidence tags and a go/no-go on host migration.

## Offline mode (decided 2026-10-22; first new feature, so it carries the open R2/R3 halves)

Leonardo's decision: a separate **Offline** mode for solo play and solo testing, so `br_menu` / `matchmaking` never have to be switched
off again. First step is a solo test mode (no queue, no second player); AI rivals ("bots") come later and start as research (OFF-3).

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

### OFF-3 — Bots: can the game run AI Nightfarers as rivals? (research)
Owner: OPUS + SONNET/researcher | Status: done (2026-10-22, static research) → OFF-4
Result: **GO for offline bots, NO-GO for multiplayer bots for now.** The game ships a complete, dormant NPC bot system: `CSNpcBotMan`,
`NPCBotTableParam` (Nightfarer loadouts per hero, levels 1 / 6 / 11 / 13), a full Nightfarer battle AI (skill, ultimate, revive),
started by event instruction `2003[97](tableId)` and cleared by `2003[98]`; no shipped event script uses them. Start refuses when a
count (likely the party size) is above 1, so solo/offline only. Bots are allies and follow the host as shipped; rivals need a team
flip. Memo: `research/OFF-3-bots.md`; facts in TECHNICAL_NOTES §4d and MEMORY_MAP §15; scripts in `experiments/OFF-3/`.

**OFF-4 done (2026-10-22, solo PASS, TESTING §3):** our `common.emevd` events run the game's bot instructions; console `bots 10|10000`,
`bots_clear` (dev, offline mode). The bots spawn, follow the host and fight enemies; they are allies (team 2), and **the game keeps
at most 2 bots** (vanilla design: solo player + 2 AI teammates; MEMORY_MAP §15). Card removed; text in git (`git show 6fc31c8:.claude/TASKS.md`).

**OFF-5 done (2026-10-22, solo PASS run 2, TESTING §3):** in offline mode, 5 of the game's NPC Nightfarers join after the landing,
hostile to you, each other and monsters (team 29), no console (`offline_bots = 5`). Past the game's 2-bot cap by "detach and re-arm"
(`features/bot_match.rs`). Card removed; text in git (`git show 23b5bb6:.claude/TASKS.md`).

Leonardo's use of bots (2026-10-22): **a sparring partner for testing alone**: combat, the near-death rules and the hawk; a full
battle-royale feel comes after.

### OFF-8 — Near-death state in an offline solo match
Owner: OPUS (RE) → SONNET/implementer → HUMAN solo | Status: ready | Phase: M9 | Blocks: OFF-7
Goal: in offline mode a player whose health reaches 0 goes into the downed state (purple bar, bleed-out) as in a multiplayer
session, instead of dying at once.
Context: Leonardo, REG-1 part B (2026-10-22): in a solo session the player dies at once, no downed state, so the near-death rules
(T-039/T-041) and OFF-7 cannot be tested solo. In multiplayer, health 0 downs (STATUS §6 "Setting health to 0 does not kill").
MEMORY_MAP §6 (downed block, bleed-out), §15 (solo gate).
Steps: (1) static RE: find where the game chooses "downed" or "dead" at 0 health (start from the writers of the downed block in
MEMORY_MAP §6 and the death path; look for a check of the session's player count / multiplayer state / an SpEffect); write what it
reads into MEMORY_MAP §6. (2) Pick the smallest switch that makes the solo player go down in offline mode only (a value the check
reads, or an event-script route if one exists, e.g. a SpEffect). (3) Dev console switch first, then on by default in offline mode.
**Possible conflict:** the NPC bot system only starts when its solo gate count `[[exe+0x3C13258]+0xDC8]+0x14` is ≤ 1 (MEMORY_MAP §15).
If the downed/dead choice reads the same count (or the same "party" object), raising it for the downed state would stop the bots
from spawning; then switch it only after OFF-5's bots are kept, or find a narrower check.
Acceptance (HUMAN solo): offline expedition, let a monster or a bot bring you to 0 health: you are downed (purple bar, bleed-out
clock), and dying from the bleed-out ends as in a session; bots still spawn after the landing. Log line names the switch.

### OFF-7 — Bots count as rivals in the near-death rules
Owner: SONNET/implementer → HUMAN solo | Status: blocked by OFF-8 (no downed state in a solo session) | Phase: M9
Goal: a bot's hit on the downed LOCAL player is handled like a rival player's (T-039/T-041: no progress on the purple bar, 3 s off
the bleed-out), so the near-death rules can be tested solo against bots.
Context: `game/combat_state/rules.rs` (`Core::step`: the attacker handle must be a rival's), `CombatCtl::rival_handles`,
`features/bot_match.rs` (kept bots and their handles), MEMORY_MAP §6/§15.
Steps: give the combat controller the kept bots' handles each tick (from the feature list, through the main loop) and treat them as
rival attackers; the hardware-breakpoint bar block (T-041) uses the same attacker check. Tests with the existing fakes.
Acceptance (HUMAN solo): offline match, get downed by the bots: the bar never fills from their hits and the bleed-out shortens; log
lines `rule: rival hit on downed LOCAL`.
Note: bots left as allies run the game's own rescue behaviour (`ExecRescueAction`), so an ally bot may revive a downed player: a way
to test the teammate revive solo later (not in this card).

### OFF-6 — Bots in the battle royale (sketch, after OFF-7)
In order: spread starts (move each bot to its own drop point along the hawk line when the ride starts, the T-028 teleport on NPCs),
roaming (our own logic script: plain-text logic may load like `c0000.hks`, TECHNICAL_NOTES §4b; or DLL steering), day levels
(tables 10000/20000/30000 = levels 6/11/13), bot deaths counted by the match. Sizes in `research/OFF-3-bots.md`.

## Match flow: nights with no boss (M5; research `research/M5-no-boss-nights.md`)

**M5-1 done (2026-10-22, static research): GO.** The night boss lives in its own map and appears only through two shared events in
`common_func.emevd`; "night done" = flags 7502 / 7507, which everything after the night waits on; the circle and the night run on
the play-area clock. Engine facts: TECHNICAL_NOTES §4e, MEMORY_MAP §16.

**M5-2 done (2026-10-22, solo PASS, TESTING §3):** console `clock [night|circle|end|hh:mm[:ss]]` jumps the play-area clock
(the game applied `clock end` within one frame; the vanilla night followed). Card removed; text in git (`git show 6f21122:.claude/TASKS.md`).

**M5-3 done (2026-10-22, solo PASS nights 1 and 2, TESTING §3):** no boss and no minions on either night (all seven boss/minion
events in `common_func` emptied, `gamedata/emevd-nights-func.json`); each night ends 180 s after 23:59 through `common` events
99100800/99100801 (`gamedata/emevd-nights.json`), then the game's morning; console `night_end`. The 180 s timer is a stop-gap until
M5-5 (a night ends only when one player is left). Taking the Day 3 way out ended in a black screen (out of scope: no Day 3). Card
removed; text in git (`git show 7fa7bdf:.claude/TASKS.md`).

**M5-4 done (2026-10-23, static RE + Nucleus run 2, TESTING §3; answers in memo §6b):** the frozen bleed-out is the vanilla night
rule (SpEffect 102120 inside the circle, `common` 1301 / 1302); the host's DLL 7502 brings every game to the morning (its 1100
passes 7503 / 7505); 7000 / 7001 give each game its own ending; the ~7 s comeback is engine code (not needed: the build holds
eliminated players down instead). Card removed; text in git (`git show b10b303:.claude/TASKS.md`).

**M5-D decided (Leonardo, 2026-10-22; PROJECT_SCOPE §2):** at night a player at 0 HP is **knocked** for the normal 15 s
bleed-out (Solo too); rivals finish a knocked player with hits (−3 s each, the existing T-041 rule); teammates can revive a knocked
teammate (Duo/Trio); **bleed-out over = eliminated**: no ~7 s comeback, the player spectates someone still fighting, and nobody can
revive them until the morning; a team is out when nobody on it stands; the night ends the moment one player (team) is left standing.
Revival items count as a revive (the winner check waits for pending revivals 7010–7012). Day deaths stay vanilla.

### M5-5 / M5-6 / M6-1 — Nights are elimination rounds (`features/night_match.rs`)
Owner: OPUS | Status: **built (branch `claude/great-turing-b2e494`); solo PASS + Nucleus runs 3–5 PASS (TESTING §3, 2026-10-23)**;
open: the night-2 ending chosen by count with two players, and merge (Leonardo)
Built: every game counts the ChrSet players (not 7005–7007); one team left standing for 1 s (no revive item pending, ≥ 2 teams at
nightfall) = night decided. The host sets our 10007400 → our event sets 7502 network-wide (fallback without our event file: the DLL
writes 7502). Only the winner's game sets 10007410 (per-game heal event). Knocked at night = the normal 15 s (event 1302 emptied);
at 1 s left (or 16 s down) **eliminated**: the clock is held at 15 every tick, nobody revives (revive block mode Nobody); after the
decision every knocked player is held too. Morning: anyone down gets up (`+0x732`, rule v1 exempt 12 s), every loser moves to its
own morning spot (last standing position before 12:00 on day 1). Night 2: winner's game 10007411 (banner 4000 → 10 s → 7000), every
other game 10007412 (banner 4010 → 10 s → 7001); fallback writes 7000 / 7001. Emptied: 1180 (wipe), 1302 (night freeze),
`90005910` (Day 3). Console: `night_status`, `night_count 0|1`, `god 0|1` (test aids), `night_end` (CONSOLE §0).
**Not built:** spectating another player (M5-7). Deviation from the card, on purpose: a 10 min fallback timer stays in the night-end
event so a night can never hang if the count fails.
HUMAN (next Nucleus or friend session, about 5 min): land; `flag 7502 1` in the host (straight to Day 2); `god 1` on p1;
`clock circle`, walk in; at night p1 knocks p2 → p1's game `night 2 decided … won` and the win screen, p2's game the loss screen.
Under Nucleus never use team 6 for both players at night (TESTING §3 run 4).

### M5-8 — Research: control the game's death (kill now, stay dead, come back where we say)
Owner: OPUS (static RE, own session) + HUMAN (one Nucleus trace session) | Status: todo (decided 2026-10-23) | Feeds: M5-9, M5-7
Why: Leonardo, 2026-10-23 (PROJECT_SCOPE §2): in **Solo** a kill is a real death; at night 0 HP = dead at once unless the player
carries a revive item; the dead stay dead and spectate until the morning; at the start of Day 2 every dead player respawns at a
random place. Today's elimination (held down, `night_match.rs`) stays for Duo/Trio and as the fallback.
Tools: REA (MCP, `rea`), Ghidra `Z:\Tools\ghidra_12.1.4_PUBLIC`, the unpacked game `Z:\Tools\nr-unpacked\1.03.3` (event files
decoded with `experiments/M5-1/emdec.py`; regulation with WitchyBND, `GameSystemCommonParam` read on 2026-10-23), the cheat-table
leads (MEMORY_MAP §11), `experiments/M5-8/nr.py` (capstone/numpy helpers: `.pdata`, disasm, xrefs). Python: memory note
"python-disasm-env" (numpy added 2026-10-23).
Starting points (memo §6b): per-frame downed function `exe+0x674EB0`; bleed-out death is the player script leaving near-death
(`+0x731` per-frame event type 144); strings `CSRespawnPointMan`, `CS::CSRespawnPointManImp::_FindRespawnPoint` (refs at
`exe+0xBA5225` / `exe+0xBA5426`, a region with no `.pdata`), `WorldChrMan_Respawn`, `RejoinMapEnterStep::STEP_Branch_DyingOrRespawn`,
sound state `LocalPlayerState` `_02_DeathToRespawn`; team type 77 = "dead at night" (set by the engine, read by `common` 1181 /
1182 / 1310); event instruction **`2004[4] Force Character Death`** (used on Nightlord maps); revive items = SpEffects 540155 /
6999100 (flags 7010–7012, `common` 1180 / 1183). Seen: a bled-out player comes back ~7 s later with full HP, by day and at night
(TESTING §3 M5-4 runs 1–2).
Questions, in this order:
- **R0 (enabler):** can the Nucleus instances run our game-data package (start them through me3 instead of SC's launcher, or any
  other way)? Every two-window test today ran without our event files (vanilla minions, wipe check, night freeze).
- **R1 kill on demand:** does `Force Character Death` on the local player (10000) in a per-game event kill a player in a session
  cleanly (death animation, network, SC)? Or a direct engine call? How the DLL tells "carries a revive item" (SpEffects above).
- **R2 the comeback:** which code brings a dead player back after ~7 s, and how to hold it at night and release it at the morning.
  Static first; then a hardware-breakpoint trace in one Nucleus run (e.g. write watch on the dead LOCAL's HP / state / team, RIP +
  return addresses; the T-041 `bar_trace` pattern). Never patch code (hard rule 3 is about SC; for the game: breakpoints/hooks only).
- **R3 the dead state:** with the comeback held: what the dead player sees and can do, team value (77?), alive flags, does SC keep
  the session; does the vanilla 1181 / 1310 logic interfere.
- **R4 respawn places:** the game's own respawn points (`CSRespawnPointMan`, `map:/entryfilelist/RespawnPoint.entryfilelistbnd`,
  `Runtime_RespawnPoint_…`): can the DLL read them to pick a random safe point; where the game itself respawns a player.
Done when: memo `research/M5-8-death.md` answers R0–R4 with evidence tags (or UNKNOWN + how to find out), MEMORY_MAP /
TECHNICAL_NOTES updated, and M5-9 below is rewritten with concrete hooks/offsets. HUMAN: one Nucleus session (~15 min, driven live
by the agent with `nrcon`, one step at a time; Leonardo uses `god 1` on the attacker, never team 6 at night).

### M5-9 — Solo nights: a kill is a real death (build, after M5-8)
Owner: OPUS → SONNET/implementer → HUMAN | Status: todo (blocked on M5-8)
Solo only: at night 0 HP without a revive item = death (R1), the comeback held until the morning (R2), the dead spectate (M5-7),
at Day 2 every dead player respawns at a random point (R4). `night_match.rs` counts the dead as "not standing" (they already are:
HP 0); elimination by holding stays for Duo/Trio. Fallback if R2 fails: kill, then on the comeback move the player away and hold
them down (TASKS history: "option B", 2026-10-23 chat).

### M5-7 — Spectate a player still fighting (camera)
Owner: OPUS (RE) | Status: todo (after M5-5 is merged)
An eliminated player watches a random living player until the morning (choice later). Needs the engine's follow camera: find the
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
Branch `claude/interesting-tharp-1c1237`, not merged. Open: merge (Leonardo), the Spectral Hawk Tree check (step 4 below, not
done yet), and the FS with two players; then remove this card.
Built: `game/hawk/` in both builds (`mod.rs` hook + swap + ride phases; `dev.rs` console and traces, dev build only). Settings
`[hawk]` (`enabled`, `dive_hold`, `dive_secs`, `dive_boost`, `start_cut`; defaults = HAWK-4's accepted values, on) read at
startup; the `hawk_*` commands are session overrides. Only LOCAL: once the main loop knows LOCAL's PlayerIns the hook acts on that
character only (before that, the HAWK-3 rider rule). A native tree ride (Spectral Hawk Tree) ends the lock; the pass-through swap
stays for the session (inert outside start rides). Route file: built by default (`-VanillaRoutes` to skip), required by
`package-release.ps1` and checked by the launcher. **Not a `Feature` yet:** the trait is on the unmerged OFF-1 branch
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
