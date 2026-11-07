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
`archive/research/`.

---

## PvP rules

**Done 2026-10-22: T-040 and T-041 accepted** (Leonardo: "worked very well"; TESTING §3). A rival's hit on a downed player is now
cancelled inside the game (hardware breakpoint on the bar store `exe+0x66DFC4`): no stand-up, no gauge flicker, the bleed-out penalty
stays; teammates revive normally (3-instance run, `team_size = 2`). Fallbacks still on: the revive-event block (`exe+0x675159`) and the
T-040 re-down (data-module HP, 2 s retry). Cards removed; text in git (`git show 3384bca:.claude/TASKS.md`); full story and addresses in
`archive/tasks/T-041-no-getup-handoff.md` and MEMORY_MAP §6. Follow-ups (no card yet): a player zip with this build for the next FS (real
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
- **VER-1 — Version gate** (refined in the 2026-10-24 review). `startup.rs` only logs the exe FileVersion. Build a pure `version::gate(file_version, pinned) -> Open | Closed(reason)` in `startup::run`; `Closed` (also when unreadable): one log warning and **no** `hwbp::set`, VEH, memory write, event flag, `poswrite` or teleport; the Steam worker and read-only `World` may run. `deathhold` and `reviveblock` already byte-check their RVAs; `menuflow`, `quietinvite`, `hawk` and the flag writes do not, so the gate is what protects a patched exe. Acceptance: unit tests, a dev override env `NRPVP_FAKE_EXE_VERSION` and a scenario `version-mismatch` (gate line logged, no `hwbp:` lines). Owner: SONNET/implementer. Rule source: ARCHITECTURE §6 "Modding rules".
- **HWBP-1 — Claim/release API for the debug registers** (2026-10-24 review; ties into the decoupling refactor). `hwbp::set` silently overwrites a slot's holder, `is_free` then `set` is not atomic, and features mix fixed slots (quiet invite 0/1, Commence 2, labels 3) with "first free" (deathhold, reviveblock, puppets, itemdrop). Build `hwbp::claim(owner, addr) -> Result<Slot, Busy>` (CAS per slot, owner beside the address), `release(Slot)` by the owner only, `holders()` for the log and `status`; a refused claim logs the holder. Write the peak budget table into ARCHITECTURE (player build worst case: menuflow 2 + reviveblock 2 = 4, so a death hold during a downed state has no slot). Acceptance: unit tests (double claim, release by a non-owner, budget exhausted), `defeat-rival` and `match-loss` PASS, `hwbp:` log shows owners. Owner: OPUS design, SONNET/implementer.

## Test tooling

The orchestrator (`nro`, scenarios, puppet players) is in this repo and is the primary test route: `TOOLS.md`, `orchestrator/README.md`.
ORCH-1 (virtual pad) is blocked and parked (the game scans XInput pads only on a device change; keyboard and mouse through `nro desk`
work). Open ideas: `puppet_revive`, puppets that attack, scenarios that wait on the log file instead of the 300-line console tail.
Every card whose acceptance can be a scenario names it (`scenarios/<name>.toml`) and is done when `scripts\regress.ps1 <name>` is PASS.

### NRO-MI — `nro` launches 2-3 real instances (Nucleus replacement)
Owner: OPUS (plan) → SONNET/implementer | Status: S0-S3 done; `mi-join` PASS 3/3 (2026-10-27); open: `mi-duo-sc` (team_size 2 teammate run), never written | Depends on: none
Goal: Claude drives and sees every player's view (host/client, Duo/Trio) with no human and no Nucleus.
Context: full design, spikes S0-S4, risks: [orchestrator/MULTI-INSTANCE.md](orchestrator/MULTI-INSTANCE.md).
Acceptance: `scenarios/mi-join.toml` (PASS) then `mi-duo-sc.toml` PASS (both consoles `players` = 2) + a `testing-log/` entry.
Out of scope: real Steam lobbies, real lag, shipping an emulator.

## Matchmaking and in-game UI (M8)

**Done 2026-10-21 (FS-UI2 PASS, TESTING §3):** **MM-4** (Solo queue, player build), **UI-1** (Battle Royale in the table menu, steps
1–5) and **UI-2** (status as blinking game messages). Cards removed; text in git (`git show c727293:.claude/TASKS.md`). The menu and
text addresses they found are in `re/MEMORY_MAP.md` §13; the flow is in `MATCHMAKING.md` §3. Left over as polish (not cards yet): the
live count inside the searching box (UI-1 C3), and messages that never show because the next state arrives first (TESTING §3, FS-UI2 entry).

### MODE-SCOPE — Mod rules only in the Battle Royale expedition (Leonardo, 2026-11-02, not urgent)
Owner: OPUS (design) | Status: idea, not scheduled
The normal expeditions started from the other cards are vanilla in every checked way, except that the hawk crossing at the start (and maybe other rules: free Day 1, PvP team writes, HUD filter) also runs in them although it is meant for Battle Royale. Decide which
rules follow "this expedition came from our card" and which stay always on; the card signal exists (`menuflow::decide_for_card`), the expedition itself would need a flag set at Commence. First check what really changes in a normal expedition (one short run).

### MM-6 — Duo and Squad with rivals: Steam ID → expedition player
Owner: OPUS to card in detail | Status: **started 2026-11-02**: the menu queues Duo / Trio (`br_menu`), a Duo party of 2 reaches an expedition as teammates (`mi-duo-party`); open: the 6-player 3-team match, Trio, messages for a party member / no party, the roster → team mapping with several teams | Settles: S9 U7
Match sizes (Leonardo, 2026-10-19): Duo = 3 teams of 2, Squad = 2 teams of 3, always 6 players; complete parties only.
With more than one team in a Duo/Squad match the roster (Steam IDs) must be mapped to expedition players on every client.

**Direction 2026-10-21:** no Steam window on any screen, everyone arrives ready and the match starts itself, the host is invisible and chosen by the matchmaker; host leaving = host migration (HOST_MIGRATION.md).

**Done 2026-11-05: MM-3g and MM-7** (`mi-zero-click` PASS 3/3; `game/matchstart.rs`, MEMORY_MAP §12). Their real-Steam check is FS-HM1 item 9. Left as polish, no card: the table menu stays open during the departure. Cards removed; text in git.

### MM-8 — Host chosen by the matchmaker (best connection)
Owner: OPUS (spec) → SONNET/implementer → FS | Status: ready | Depends: —
Today the queue lobby owner becomes host (luck). Each member publishes its Steam ping location (`ISteamNetworkingUtils`
`GetLocalPingLocation` → member data); the owner picks the member with the lowest worst-case estimated ping to the others
(`EstimatePingTimeBetweenTwoLocations`), ties by Steam id, and writes `host`. FakeSteam tests for the pick; the player never sees who
hosts. Verify the Steam API calls exist in the game's `steam_api64.dll` first.

## Host migration and 12 players (top priority: migration first, player count later; Leonardo 2026-11-02; plan and budgets in [HOST_MIGRATION.md](HOST_MIGRATION.md))

### HM-1 — Host migration: read it on real Steam
Owner: HUMAN (session) then OPUS | Status: **everything one PC can show is done (`mi-host-migration` PASS, HOST_MIGRATION §1a). Open: FS-HM1, TESTING §6, with a fresh zip**
Past a budget in HOST_MIGRATION §6: stop and ask Leonardo.
Out of scope: more than 6 players, NET-7.

## Own networking (NET; decided 2026-10-30, design and gates in [NETWORKING.md](NETWORKING.md))

Order: NET-1 and NET-2 are done (2026-10-30; scenarios `mi-net-ping`, `mi-net-join`, `mi-net-verdict`; facts on `net::bus`, NETWORKING §4); users today: RW-3 `ITEM_DROP` (topic 5, on) and the RW-5 skeleton (`ORB_CLAIM`, topic 2, off). NET-5 round 1 is done (NETWORKING §2b) and continues as NET-7; NET-3..5 are read-only studies that feed gate G1 (NET-6: replace SC's session layer, yes or no). Hard rule 3 holds for all of them (observe `nrsc.dll` at run time only).

### NET-3 — Study: the game's own P2P traffic (read-only)
Owner: SONNET/researcher + OPUS review | Status: todo | Depends on: none (better after NET-1)
Goal: packet type / size / rate histogram of the game's `SendP2PPacket`/`ReadP2PPacket` (SteamNetworking006) and of SC's Messages channels, per phase (Roundtable, loading, expedition, a rival hit).
Steps: hardware-breakpoint trace like `steam/lobtrace.rs` on the interface's send/read vtable targets (dev build only), log `(peer, size, channel, first byte)` aggregates; `ISteamNetworkingMessages` send/receive likewise to learn SC's channel numbers. Two instances, `mi-join` flow.
Acceptance: `scenarios/mi-net-trace.toml` produces logs with non-zero histograms for each phase; findings table in NETWORKING §2/MEMORY_MAP (type ids, which carries hits/HP/SpEffects); UNKNOWN 2 and the channel question answered or restated.
Out of scope: injecting packets, changing any game or SC byte.

### NET-4 — Study: NR `CSSessionManager` layout (read-only)
Owner: SONNET/researcher + OPUS review | Status: todo | Depends on: none
Goal: map the singleton at `exe+0x3C21EC8` (state, `players` vector, steam id offset, send queue, limit fields) and `CSTeamSessionManager`.
Steps: dump the object in a 2-instance session (`memscan`/`peek`), match each instance's Steam id (user getter), cross-check with ORCH-R2 (`[[exe+0x3C078D0]+0x10]+idx*0x8A0`) and the ER layout; answer whether `0x6001` is the engine's or SC's numbering.
Acceptance: table in `re/MEMORY_MAP.md` with a `[VERIFIED]` tag per offset; console `session` dump command; scenario `mi-session-dump` logs both players with their Steam ids.
Out of scope: writes.

### NET-7 — Sign-in gate: make the game "signed in" without Bandai's server (the decider for replacing SC)
Owner: OPUS + SONNET/researcher | Status: **released with guards for the lab stage (DECISIONS 2026-11-02), runs as HM-2 only if HM-1 shows SC blocks migration**; real Steam needs a separate go | Depends on: NET-5 round 1 | Budget: 3 to 5 working days, then stop and decide (NET-6)
Goal: find what takes the three sign-in state objects from 3 (request in flight) to 2 (done), and complete it ourselves with local data, so that Invite Members works in a game without `nrsc.dll`.
Steps: (1) in an SC run (the oracle) find the writer of state 2 and the function that handles the sign-in answer: reset the state, run `rawcall F18D30 0`, trace writes with hardware breakpoints on the state words and the step function `exe+0xDEC110` (static: its callee `0xDEBA10`, the request builder, and the 0x41-byte profile at `0xE01370`); (2) call that completion from our mod after our own conditions (never with the real server reachable: `Menu.IsEnableOnlineMode` stays false, flag byte handling decided by what (1) shows); (3) set `CSSessionManager+0x170` to 6 and look at what the table menu and the lobby do; (4) try `mi-nosc-join` again.
Acceptance: `mi-nosc-join` PASS: Invite Members creates a lobby (`LobbyCreated_t result=1`) and instance 2 joins it, both consoles show `world loaded, 2 player(s)`. **Stop rule:** if after 5 days the sign-in cannot be completed locally, or the next gate is a server call that needs a real answer, record the blocker and take NET-6 with "no": the data channel (NET-1, NET-2) stays the plan.
Out of scope: widening the member table beyond 2 remotes (only after this passes), real Steam, any connection to Bandai servers.

### NET-6 — Gate G1: replace the session layer, yes or no
Owner: OPUS → Leonardo decides | Status: blocked on NET-3..5
Goal: a decision in DECISIONS.md with evidence, size estimate, the read EULA/support text (UNKNOWN 8) and, if still wanted, the question to SC's author.
Out of scope: starting S1.

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

**Bots were cancelled on 2026-10-24 and are back as a feature on 2026-11-06:** see the "Bots" section and [BOTS.md](BOTS.md).

## Bots (Leonardo, 2026-11-06; design, plan and open questions in [BOTS.md](BOTS.md))

### BOT-R1 — RE: how to steer a bot
Owner: OPUS | Status: after BOT-1
Rank the four levers of BOTS.md §2b with static reading (AI target "host" field of logic 30000, plain-text logic loading, shipped
roaming logics) and one live probe each for the best two. Result into MEMORY_MAP and BOTS.md; BOT-3 is written from it.

### BOT-3 / BOT-4 / BOT-6
BOT-0 done 2026-11-06: host bots are not on a client; small-base invaders are (BOTS.md §4). BOT-6 builds friends + bots on the invaders.
BOT-2 done for Solo 2026-11-06 (`bots-match`); bots are on the killfeed as "Bot N" since 2026-11-07 (BOT-KF); their HP bar still reads `?NpcName?`. The early WIN while a replacement was owed is fixed (2026-11-07).
BOT-5 done 2026-11-07: the Bots switch in Matchmaking Settings (`mi-bots-menu`).
Brain v1, levels, friends + bots: written out when they start (BOTS.md §3 holds scope and order).

## Match flow: nights with no boss (M5; research `archive/research/M5-no-boss-nights.md`, `archive/research/M5-8-death.md`)

**Built earlier, since replaced by the RW rules (2026-10-24, below):** M5-1..M5-4 (no boss, no minions, clock console; engine facts TECHNICAL_NOTES §4e/§4f, MEMORY_MAP §16/§17), the elimination rounds M5-5 / M5-6 / M6-1, M5-8 research and M5-9 Solo real death. What stays: `features/br_match/` (match state), `game/deathhold.rs` (keeps a defeated player dead; SpEffect 102130 = die at once, the comeback can be held and released). Rule decisions: `DECISIONS.md`. A Solo die-at-once rule is open again: see "Solo night death" below.

### M5-9 / M5-5 — superseded
The elimination rounds (night = a round, losers held until the morning, die-at-once in Solo nights) were replaced on 2026-10-24 by the RW
rules (DECISIONS "After the first real match"; section "Rework after the first real match" below). The hold (`game/deathhold.rs`) stays: it
keeps a defeated player dead. What FS-M59 left open (the hold in a real session, the other screen) is in the FS-RW1 runbook (TESTING §6).

## Hawk crossing (M7, replaces separate spawns; design in PROJECT_PLAN.md §13)

T-028 (teleport) is done: solo run 4 and the Nucleus two-screen check passed on 2026-10-14 (TESTING §3); code merged with `main` on
branch `t-028-snapback`. The F3 team-spawn rule built there is parked (`team_spawns = false`).

HAWK-0 (questions and desk research) is done (2026-10-21): answers and design in PROJECT_PLAN §13, engine facts in TECHNICAL_NOTES §4b
(hawk = player script state) and §4c (routes in `m60_00_00_99.msb`), tools in `archive/experiments/HAWK-0/`.

HAWK-A2 / HAWK-3 are done (2026-10-22, TESTING §3 HAWK-3 run 1): the env switch is found and swapped safely, `r8` = PlayerIns,
`hawk_drop` and `hawk_tree` work in the game (Leonardo). Design v1 that follows from it: PROJECT_PLAN §13 "v1 design". HAWK-1 (ride
probe) is dropped: the env trace answered what it was for. HAWK-1b is done (2026-10-22, TESTING §3 run 3: one diagonal line "B"
for every start route, three rides PASS); HAWK-4 is done (2026-10-22, TESTING §3 HAWK-4 runs 1-2: cruise lock, dive, start
animation cut; Leonardo: good enough). The line is built by `scripts\build-gamedata.ps1` (by default since HAWK-2; TECHNICAL_NOTES §4c).

### HAWK-2 — Hawk crossing v1 (player feature)
Owner: OPUS → SONNET/implementer → HUMAN | Status: **solo smoke PASS 2026-10-22** (TESTING §3; Leonardo: "how I wanted it").
Merged (PR #2). Open: the Spectral Hawk Tree check (a tree ride must behave as in vanilla), and the FS with two players; then remove this card.
Built: `game/hawk/` in both builds (`mod.rs` hook + swap + ride phases; `dev.rs` console and traces, dev build only). Settings
`[hawk]` (`enabled`, `dive_hold`, `dive_secs`, `dive_boost`, `start_cut`; defaults = HAWK-4's accepted values, on) read at
startup; the `hawk_*` commands are session overrides. Only LOCAL: once the main loop knows LOCAL's PlayerIns the hook acts on that
character only (before that, the HAWK-3 rider rule). A native tree ride (Spectral Hawk Tree) ends the lock; the pass-through swap
stays for the session (inert outside start rides). Route file: built by default (`-VanillaRoutes` to skip), required by
`package-release.ps1` and checked by the launcher. **Not a `Feature` yet** (ARCHITECTURE R3 open); moving it there = `configure` → constructor, `tick` → `Feature::tick` with `local` from the `World`.
What: PROJECT_PLAN §13 v1 as a player feature (cruise lock + dive + start cut with the accepted defaults; the console
commands stay dev-only): `hawk.rs` leaves the dev-only build, `[hawk]` settings in `nrpvp.toml`, on by default
in Battle Royale and offline mode, only LOCAL changed, swap removed when the expedition ends. The route file goes into the player
game-data package (lift the `map\` refusal for that one file). The first new feature, so the open halves of R2/R3 (typed views,
`Feature` trait; ARCHITECTURE §6) are built with it.
HUMAN: one solo smoke; then the next FS: both players cruise, dive and drop where they want; each sees the other land.

## Leonardo's list (2026-10-24, tracked here; update the state column when something changes)

Test routes: **1 PC** = automated scenario (`nro`, one game), **2 inst** = two real emulated-Steam instances on one PC (`nro`, `mi-*` scenarios,
[orchestrator/MULTI-INSTANCE.md](orchestrator/MULTI-INSTANCE.md)), **FS** = Friend Session on two PCs (TESTING §6).

| # | Item | Card | State (2026-11-01) | Still to test |
|---|---|---|---|---|
| F1 | Respawn far from all players | RW-7 | Built, unit-tested | Two instances: where each one starts and respawns after a mob death (2 inst), then FS |
| F2 | Runes shared only inside the own team (solo: himself) | RW-8 | **Built, `mi-rw8-runes` PASS 3/3 (2026-10-28, PR #25)** | Duo / Trio teammate case, a client-side kill, FS photo of both rune counters |
| F3 | No remote location (map, compass), name tags only on the target | RW-9 | **Built and seen with 2 inst**: map, compass, party bar, name tag, edge arrow, placed map pins with their 3D marker and green beam hidden for rivals (`mi-pin-beam`), pin toast off in every match (UI-5) | Duo/Trio teammates; FS photo |
| F4 | Enemy difficulty by team size (Solo / Duo / Trio values) | RW-10 | **Accepted: Leonardo found enemy strength balanced (2026-11-01).** Table rewritten on one PC (2 copies, 168 rows) | none beyond FS |
| D1 | Defeat / loss when killed by a player or the tide, then spectate or quit to the Roundtable | RW-1, M5-7 | **Done on 1 PC** (`defeat-rival`, `match-loss`, `spectate`, `esc-defeated`); spectate seen in Nucleus | 2 inst: the other screen (held body, defeat screen, spectate), real hits; spectator camera smoothness |
| D2 | Defeated players drop items (red phantom) on the ground; runes are discarded (DECISIONS) | RW-3 | **Weapons, talismans and picked-up consumables built** (`mi-rw3-items` PASS 2026-11-01; inventory part verified by hand, weapon level kept); runes discarded | 2 inst |
| D3 | Death by mob / boss respawns normally with items | RW-2 | **Done on 1 PC** (`defeat-mob`) | none beyond FS |
| D4 | Boss dormant powers / weapons only to the last hitter's team | RW-5 | **Parked 2026-10-31, vanilla (see RW-5)** (`disableMultiDropShare` param hint) | RE first, then 2 inst with a boss |
| D5 | Day 1 / night 1 free of the tide, tide starts on Day 2 | RW-4 | **Done on 1 PC** (`day1-free`) | FS (real clock over the network) |
| D6 | Match ends when one team is left, even on Day 1 | RW-1 | **Done on 1 PC** (`match-win`, `match-win-end`, `match-loss`) | 2 inst with real defeats; FS |
| D7 | Private match by password, outside matchmaking | RW-6 | Unit-tested only (`private_code` in `nrpvp.toml`); the in-game "Multiplayer Password" field is the planned player route (RW-6 follow-up) | 2 inst: same code meets, different codes do not (`mm_queue` works over the emulator, `mi-queue`) |

Backlog since 2026-11-02 (Leonardo: nothing here is scheduled; UI improvements come first, only simple work with a valid result). Order when picked up again (2026-11-01): **Sites of Grace blocked in PvP combat**, **Duo and Trio first tests**, then the 2-instance checks of F1, D1, D7. Open ideas without schedule: Solo night death, match time limit. Arena comes after the first release.

## Not built yet (Leonardo, 2026-11-01; rules in `re/PROJECT_SCOPE.md`)

### DUOTRIO-1 — First Duo and Trio tests (Leonardo 2026-11-02; now the official Duo / Trio work)
Owner: HUMAN + OPUS | Status: **in progress**: `mi-duo-party` (party, queue, expedition as `ally`) PASS 2026-11-02; next scenarios on that base (two teammates): runes shared, revive, HUD / pins of a teammate, a teammate's defeat; then a puppet team scenario for 2 vs 2 / win by team | Depends on: none
Duo / Trio have never been tested by us (teams of 2 and 3, teammate revive, shared runes, HUD filter, win by team). Write `mi-duo-sc` (NRO-MI) and a 4-puppet team scenario; list what breaks. MM-6 covers matchmaking for teams.

### SOLO-REVIVE — Solo night death with a revive item (BACKLOG, Leonardo 2026-11-02)
Owner: OPUS | Status: idea, no schedule (DECISIONS 2026-11-01)
PROJECT_SCOPE §1 wants a Solo kill at night to be a real death at once unless the player carries a revive item (Wending Grace, goods `0x400002BC`) that then works as in vanilla. First step is only a test: with and without the item, does the current build already behave so? Only then a card for code.

### TIMELIMIT-1 — Match time limit (idea)
Owner: OPUS | Status: idea, no schedule. PROJECT_SCOPE §1: "if a match passes the maximum vanilla time everyone loses". Not built.

### MODE-02 — Arena (later)
Not for the first release (PROJECT_SCOPE). The refactor goal (phase 2 of the clean-up) is code decoupled enough that a new mode is an assembly of existing modules.

## Rework after the first real match (RW; Leonardo's notes 2026-10-24, rules in DECISIONS)

Built and tested on one PC 2026-10-24 (scenarios `day1-free`, `defeat-mob`, `defeat-rival`, `match-win`, `match-win-end`, `match-loss`;
testing-log): **RW-1** defeat for good + early win, **RW-2** a non-player death is vanilla, **RW-4** free Day 1 (the code is in
`features/br_match/mod.rs`, header comment). Built, unit-tested, not yet seen on two PCs: **RW-6** private match, **RW-7** respawn far, **RW-10**
enemy scaling. Their two-PC checks are in the FS-RW1 runbook (TESTING §6); the cards come off when FS-RW1 passes. Open, each waiting for
what a second real client shows (research tools: console `memscan`, `peek`; findings so far below):

- **RW-3 — Defeat drops.** **Slice 1 built, `mi-rw3-items` PASS on one PC (2026-11-01):** a rival-caused defeat drops the victim's equipped **weapons** at its spot; the host makes the drop with the game's own function on the game thread (`game/itemdrop.rs`, `game/defeatdrop.rs`, bus topic 5), the game shows it to every player and any can pick it. **Slice 2 (2026-11-01):** talismans and picked-up consumables are read from the inventory list and dropped with the weapons (not the flask); verified by hand on the host as victim, not in a scenario (key presses into the second window are unreliable). **Left:** a scenario for the inventory part, weapon art / Grief effect (the weapon LEVEL is kept: it is part of the id, verified 2026-11-01), item lifetime, the victim's inventory emptying, the client-initiated drop path, the two-PC check (FS-RW1). Runes are discarded, not dropped (DECISIONS 2026-11-01). The red phantom figure is scrapped (DECISIONS 2026-11-01). Research: `research/RW3-handover.md`, `RW3-red-phantom-*.md`, MEMORY_MAP §24.
- **RW-5 — Boss orb: first to loot — PARKED, vanilla ships** (Leonardo, 2026-10-31). Built and seen on two instances but **off by default**: `game/orbclaim.rs`, console `orb_auto 1` + `orb_list / orb_off / orb_on / orb_claim / orb_go / boss_orb`, topic `ORB_CLAIM`, scenario `mi-rw5-orb` (switches it on). **Why parked:** a client's pick needs the host's copy of the orb, so nulling the host's copy blocks a client claimer too (MEMORY_MAP §22); the fix is a hook at the game's interaction call site (static RE; a hardware read watch on the slot crashes the game, TRAPS). Also open: second kill overwrites slot +0x58 (an unpicked orb is lost, the rescue-by-moving experiment failed), orb lifetime, nulled slot at expiry, two real PCs. **If resumed:** time-box the call-site hook spike; if no clean block point, stay vanilla.
- **RW-8 — Runes only inside the own team.** **Built and accepted on one PC** (PR #25, 2026-10-28): every game credits its own player for every death it processes (`exe+0x3DE710` → `exe+0x668380`, no killer check, MEMORY_MAP §21); the killer's game records the attacker (`ChrIns+0x1A0`), the other game reads `-1` (it applied no hit). `game/runeshare.rs` (both builds) passes amount 0 when the last attacker is a rival, or — with no teammate (Solo) — when none is recorded. Console `runeshare`, `runes_mark`, test aids `mobs`, `mob_go`, `mob_kill`. **Acceptance:** `scenarios/mi-rw8-runes.toml` **PASS 3/3** (runs `1793219963/-969`, `1793220173/-180`, `1793220436/-441`, 2026-10-28): player 1 `+1040` / `0 blocked`; player 2 `+0` / `1 blocked = 100 rune(s)` and `Unknown -> BLOCKED`; control `runeshare 0` pays player 2 (`+1040` / `+1560`). **Open:** Duo / Trio teammates (an unrecorded killer stays vanilla there — it may be a teammate's; needs the killer from the owner's sync, UNKNOWN); the message credit `exe+0x51DC0B` (enemy not loaded here) is counted, not filtered; FS-RW1 step 7(a) on two real PCs (photos of both rune counters). Remove the card when the FS check passes. **Review 2026-10-30 (code only, 494 unit tests pass):** hooks are checked before patching and fail safe; **untested risk:** in Solo a *client* who kills an enemy owned by the host sees no attacker recorded (`Unknown`) and gets no runes — the scenario only has the owner kill. Check with a client-side real kill (or find where the host's verdict reaches the client) before FS-RW1.
- **RW-9 — No remote map / compass position, nametags only on the target.** **Built for every team size (the Duo / Trio teammate case is still open), seen with two real instances on one PC (2026-10-27, branch `test/rw9-multi`):** `game/hudfilter.rs` hooks three HUD lookups (MEMORY_MAP §20 "Rival filter") so a rival is on no map pin, compass mark, party bar or name tag (incl. the edge arrow); the lock-on target has no name tag either (the game's enemy bar names it, 2026-11-06); teammates are not rivals and stay. `maphide` (Solo, whole list) is only the fallback. Console `rivalhud`, test aid `tp_player`. Life bars from lock-on / hits stay vanilla (DECISIONS). Seen: with the filter on, no party bar / name tag / edge arrow on both screens (run `1793065578`, `h1_hawk`, `h2_hawk`); compass and map pin gone with `maphide` (run `1793064664`). **Acceptance:** `scenarios/mi-rw9-rivals.toml` PASS 2/2 (2026-10-27). **Map markers (2026-10-27, branch `rw9-markers`):** a pin a rival places arrived and showed on the map, compass and as a 3D marker; two more hooks (MEMORY_MAP §20 "Map markers") now keep a rival's pins off all three (own and teammates' stay), `scenarios/mi-rw9-markers.toml` PASS (testing-log 2026-10-27). Still shown: the "Ally is showing location" toast with a landmark icon (producer UNKNOWN). **Left after that:** the toast, Duo / Trio teammate case (needs `team_size = 2` over the queue or a 4-instance PC), FS photo on two PCs. Remove the card when a teammate run shows the teammate still marked. **Open:** the toast "Ally is showing location" still reveals the area to a rival; notes and plan (and the killfeed idea built on the same mechanism): [re/TOAST_KILLFEED.md](re/TOAST_KILLFEED.md).
- **Spectating (M5-7) is built in v1** (STATUS; DECISIONS 2026-10-25): the held body is moved next to a living player. Camera smoothness and the two-PC view are the open items below.
- **RW-6 follow-up:** the game's own Matchmaking Settings has a "Multiplayer Password" field; the text lives in a `CS::CSMenuSimpleSaveDataChunk<3, MENU_INPUT_HISTORY>` object (input history, found with `memscan`) and in a second structure; which one the menu reads at Commence is UNKNOWN. Reading it would replace `private_code` in the toml (no config editing for players).
- **Spectator camera smoothness (next):** the moved body makes the camera jump about 20 times a second. Idea: find what writes `ChrExFollowCam+0xA0..` (the followed position, newest entry; console `trace_write <addr>`) and feed it another player's position or target directly, leaving the body where it died. Fallback: keep the moved body (committed, 2026-10-25).
- **ESC / pad while defeated (works on one PC, 2026-10-25; scenario `esc-defeated` PASS, pad confirmed by Leonardo).** `game/menugate.rs`: Esc or pad Start opens the game's menu for a defeated, held player (sets the open request, MEMORY_MAP §19); pad d-pad right / left switch the watched player (mouse buttons too; all off while a menu is open). The pad is read from the game's own input bits (XInput is blind inside the game). Quit through the menu while dead works (one PC, Leonardo). Left: a Nucleus / FS confirmation, other pad layouts (bits measured on a Steam Input Xbox pad). Remove this card after that.

## Ideas from FS-RC1 (2026-11-01, testing-log; none scheduled)
- **Decide the win at once:** publish a `DEFEAT` fact on `net::bus` from the victim's game so the winner does not wait the 30 s `OUT_DEAD_MS` after a rival kill (needs a topic, key = team, a test in the golden traces).
- **Quiet the `mm_join: gate:` log** after the expedition has started (about 150 identical lines per match).
- **Double press of Commence** cancels the search (toggle); consider ignoring a second press within a second.


## Oddities recorded by the golden traces (R7-1, 2026-11-01; behaviour kept as-is, decide later whether any is a bug)

1. The event-file fallback re-fires `GameFlag(7000/7001)` every 1500 ms while our flag stays ON in the world.
2. With no positions known, the spectator logs "spectator switched to player..." but sends no `Follow`.
3. A mob-death respawn with no positions of others logs "respawned inf m from the nearest player; no move".
4. The mob-death log prints "last rival hit None ms ago" when there was none.
5. Night flag ON on Day 1 with the clock past 11:50 makes the host set 7502 at once.
6. A LOCAL dead (HP 0, not downed) for 30 s counts as *out* even when a mob's death would respawn it (`OUT_DEAD_MS`, by design).
7. After the expedition-leave reset `status` is empty until the next in-expedition tick.
8. `DeathHold(false)` and `MenuGate(false)` are emitted on every leave, even when neither was on.
