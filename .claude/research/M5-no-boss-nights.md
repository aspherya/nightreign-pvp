# M5-1: Nights with no night boss (static research)

Researcher: Opus | Date: 2026-10-22 | Status: done, waiting for the in-game checks in TASKS M5-2 / M5-3
Scope: game 1.03.3 (exe 1.3.3.0), the real event files `Z:\Tools\nr-unpacked\1.03.3\event\*.emevd.dcx` (all 278 decompressed with
WitchyBND 3.0.1.0 and read with our own decoder `experiments/M5-1/emdec.py`, instruction names from DarkScript3 3.6.3
`nr-common.emedf.json`), `regulation.bin` (WitchyBND, `PlayAreaCreateCommonParam`), and `nightreign.exe` (capstone,
`experiments/M5-1/m5_clock.py`). Nothing was launched or patched. Event numbers below are the real 1.03.3 ones; they match S3's
6PF-based reading where S3 had them. Tags as in TECHNICAL_NOTES.

---

## 0. Bottom line

**GO.** A night with no boss is a two-event change in `common_func.emevd` plus one new event in `common.emevd`, all through the
event-script tooling we already ship (`scripts/build-gamedata.ps1`). The night itself, the circle and everything after the night
(flasks refilled, the interval grace, Day 2 at 07:00, Day 3) keep working, because they hang off the "night boss defeated" flags
(7502 / 7507), not off the boss.

| Question (Leonardo) | Answer |
|---|---|
| How do we stop the boss from spawning? | Every night-boss map but one (35 of the 36 maps `m47_70`..`m52_13`, one per boss; `m52_12` starts its night in its own events `52122800`/`52122801`) starts the same two common events: **`90055000`** (night minions; sets 7501 / 7506) and **`90055001`** (sets the map's own "boss fight starts" flag `XXXX0200` when the minions are dead or after 120 s). The boss character is disabled at map load and only enabled when `XXXX0200` turns on (per-boss events, e.g. `90065001`). Replace the bodies of `90055000` (keep the minions disabled, end) and `90055001` (end) and no minion and no boss ever appears, on every boss map, from one file. `[SOURCE: common_func 90055000/90055001, m47_70 event 0, common_func 90065001; this memo §2]` |
| Does the night still play out with the shrinking circle as the arena? | **Yes** `[HYPOTHESIS, strong]`. The circle and the night run on the play-area clock, not on the boss: the final circle shrinks 20:24 → 24:00 to a 40 m radius (`PlayAreaCreateCommonParam` row 0), the clock **holds itself at 23:59:59** (`exe+0x698430`), and "night" (flag 7515) is set by `common` event 1107 whenever the clock reads 23:59. Nothing there reads a boss flag. To check in game: M5-3. |
| What sets the "night done" flag, and when? | In vanilla only the boss's death: `90065900` (boss-defeat handler) sets **7502** (Night 1) / **7507** (Night 2), and `common` 1190 / 1191 set them again when any boss map's defeat flag `XXXX0800` / `XXXX0850` turns on. Nothing else, no timer. So with no boss **we set it**: a new `common` event waits for night (7515), then N seconds (our night length) or our flag, then sets 7502 or 7507 with `Set Network-Connected Event Flag` like the game does. |
| Does anything else wait on the boss? | Listed in §3. What follows 7502 / 7507 is untouched (flask refill `common` 1303/1304, interval 7503 → the interval grace `90005580` → 20 s → `Reset Time of Day` → Day 2; after 7507 the Day 3 warp `90005910`). Lost with the boss: its health bar and boss music (never start), the "night boss felled" banner, `IncrementTeamBossesKilled`, the boss's drops, and the heal `90065900` gives (`SpEffect 102121` + `Bonfire-like Recovery`; our event can do both). The vanilla game-over check `common` 1180 (multiplayer only, everyone down at night) is not about the boss and stays as it is. |

The **clock jump** for testing (Leonardo's request) is built: console `clock`, `clock night`, `clock circle`, `clock end`,
`clock hh:mm[:ss]` (CONSOLE §0). It uses a set-time mailbox the game itself reads every frame (§4). No event instruction can set
an arbitrary time: the only clock instruction, `2003[89] Reset Time of Day`, always goes to the next morning (its hour arguments
are unused, §4).

---

## 1. The night timeline in the 1.03.3 event files

All of this is host-side (`Player Is In World Type: Own World`) and uses `Set Network-Connected Event Flag` (2003[69]), so the host's
game decides and the flags reach the others. `[SOURCE: common 1100, 1103, 1107]`

| Step | Event (file) | What it does |
|---|---|---|
| Day 1 starts | 1100 (common) | expedition mode (`3[41]` = 2) → 7500 |
| Night minions | `90055000` (common_func), started by each boss map's event 0 with (minion group `XXXX5200`, day 1 / 2, boss area `XXXX2200`, timer flag `XXXX0201`) | waits clock 23:00–23:59 **and** (a player in the boss area **or** clock 23:59) → 7501 (day 1) / 7506 (day 2), enables the minions; 120 s later sets the timer flag |
| Boss fight starts | `90055001` (common_func), same map, (minion group, day, timer flag, **battle flag `XXXX0200`**) | waits the prelim flag, clock 23:59, and (all minions dead **or** the timer flag) → `XXXX0200`, 7504 / 7509 |
| Boss appears | per-boss events in the boss map (e.g. `90065001` for `m47_70`) | disable the boss at load, wait `XXXX0200`, enable it, health bar, boss music |
| Night | 1107 (common) | clock 23:59:00–23:59:59 → 7515 on; back to ≤ 23:58:59 → 7515 off |
| Boss dies | `90065900` (common_func), (defeat flag `XXXX0800`, day 1 / 2, …, banner text, …) | banner, `IncrementTeamBossesKilled`, boss music off, then **7502** / **7507**, `SpEffect 102121`, `Bonfire-like Recovery` |
| | 1190 / 1191 (common) | any boss map's `XXXX0800` → 7502; any `XXXX0850` → 7507 (a second path to the same flags) |
| Flasks | 1303 / 1304 (common) | 7502 / 7507 → refill flasks |
| Interval | 1100 | 7502 → 7503 (skipped when the special event flag 7700 is on); 7507 → 7508 (unless 7720) |
| Interval grace | `90005580` | 7503 → the site of grace appears |
| Day 2 | 1103 | 7503 → 20 s → sound, SFX, 2 s → **`Reset Time of Day`** (2003[89], duration 4 s); 1100 sets 7505 once the clock reads ≤ 22:59 |
| Day 3 | `90005910` | 7508 → 4 s → an action button on an asset → warp to the Spirit Shelter, 7521 |

Boss maps and their day: `90065900`'s second argument is 1 for 18 boss maps and 2 for the other 18 (`m47_70` = 1, `m48_20` = 2, …). The flags `XXXX0000` / `XXXX0001` that choose day 1 / day 2 in each boss map's event 0 are set by no event
script: by the engine's map-pattern lottery `[HYPOTHESIS]`.

Map variations: event 0 of each boss map also starts `90035000` (variation 1: battle flag on flag 8035, "night bosses back to back",
set by common 1120 15 s after 7502 when a special event is on) and `90015442` (variation 2, meaning UNKNOWN). Variation 0 is the
normal expedition. The v1 change covers variation 0 only; variations 1 and 2 need the same treatment if we ever see them in a match.

## 2. How to remove the boss (the build, M5-3)

1. **`common_func.emevd`, event `90055000`** → new body: `Change Character Enable State(X0, Disabled)`, `Set Character AI State(X0,
   Disabled)`, `END`. (The minions exist enabled in the map data: the vanilla body disables them in every branch where they are not
   wanted.) No 7501 / 7506 any more: only 1102 / 1105 (they just wait and end) and two special maps (`m49_29`, `m52_12`) read them.
2. **`common_func.emevd`, event `90055001`** → new body: `END`. No `XXXX0200`, so no boss, no health bar, no boss music; no 7504 /
   7509 (read only by a few map-object animations: `90015009`, `90045000`, `90045005`).
3. **`common.emevd`, new events** (our id range 991007xx):
   - Night 1 end: IF 7515 ON AND 7505 OFF AND 7502 OFF AND own world → WAIT N s (or our flag `10007400`) → `Set Network-Connected
     Event Flag(7502, ON)`, `Set SpEffect(20000, 102121)`, `Bonfire-like Recovery` → END.
   - Night 2 end: the same with 7505 ON, 7507 OFF → 7507.
   N comes from `gamedata/` (start: 180 s, tune from play). The DLL sets `10007400` for the console command `night_end` (tests), and
   later the match logic can end a night early (last team standing).

**As built (M5-3, 2026-10-22; Leonardo: no boss and no minions at all):** the replacement also covers every other path to a boss
fight flag found in `common_func`: the wave version on `m52_12` (`90065251` minion waves → disable X12, `90065252` → disable X0,
`90065253` → END), the back-to-back variant (`90035000` → END) and map variation 2 (`90015442` → END). The night-end events also set
8020 when the special event 7700 / 7720 is on, because `common` 1100 then waits for 8020 instead of the night flag.

Our build pipeline only appended events before M5-3. M5-3 adds "replace an event's instructions" to `scripts/emevd_add_events.py` (same
technique: new instructions and parameter entries at the end of each table, the event header pointed at them; every existing offset
stays valid).

## 3. What else waited on the boss

| Thing | With no boss | Action |
|---|---|---|
| Health bar, boss music, banner text | never start | none |
| Heal at night end (`90065900`: `SpEffect 102121`, `Bonfire-like Recovery`) | gone | our night-end event does both (decide: PvP may not want a full heal; Leonardo) |
| Flask refill (1303 / 1304) | still runs (on 7502 / 7507) | none |
| `IncrementTeamBossesKilled` | not counted | none for the battle royale; vanilla rewards at the end may be lower `[HYPOTHESIS]` |
| Boss drops (relics, runes) | gone | none (F14 says no kill rewards) |
| Boss arena walls / assets | `90065900` enables the asset `X8` on defeat (0 = none on the boss maps read) | check in game that nothing blocks the arena |
| Game over (1180) | unchanged: in multiplayer, everyone's "alive" flag off at night → 7001 | known PvP issue (S3 §6), not part of this card |
| Day 3 (Nightlord) | unchanged after 7507 | M6 (finale) decides |

## 4. The play-area clock (engine, static reading of exe 1.3.3.0)

Full table with offsets: MEMORY_MAP §16. In short:

- `WorldPlayAreaManager` (FD4 singleton, slot `exe+0x3C13158`) holds the clock as a `DLDateTime` at `+0xF8` (u64 time value, then a
  packed u64 at `+0x100`: hour bits 34–38, minute 39–44, second 45–50). The event condition `1[7] IF Play Area Current Time In Range`
  (`exe+0x603070`) reads exactly these bits.
- The per-frame update `exe+0x698430` advances it by frame time × speed (`+0x118`, f32), and when an advance would pass midnight it
  puts the old time back and sets **23:59:59**: the night clock never runs out by itself.
- In the same update: if the three i32 at `+0x124`, `+0x128`, `+0x12C` hold a valid hour / minute / second, the game sets its clock
  there with its own setter `exe+0x68FF30`, writes −1 into all three, and when `CSSessionManager` (`exe+0x3C21EC8`) `+0xC` is 3
  packs the time and passes it on (`exe+0xDDF410`, a network send `[HYPOTHESIS]`). This is the mailbox our `clock` command fills.
- `2003[89] Reset Time of Day` (`exe+0x5FEA00` → `exe+0x68C1B0`) ignores its hour arguments: it requests a transition to the morning
  hour from a param (default 07:00) over its duration (`+0x13C`, `+0x140` = 1). It cannot jump forward.
- Circle schedule (`PlayAreaCreateCommonParam` row 0, the normal expedition): first circle 12:30 → 16:09, final circle 20:24 →
  00:00, final radius 40 m, initial radius 700 m; rain SpEffects 99810 / 99811. Whether the circle follows a clock jump at once
  (computed from the clock) or catches up: M5-2 shows it.

Why not a hook or a direct write of the time value: the mailbox is the game's own request path, on its own thread, with its own
range check and its own sync; writing the `DLDateTime` from our thread would race the per-frame update.

## 5. Risks

1. **Event-file distribution:** every player needs the same `common_func` / `common` (already true: our game-data package is in the
   player zip). Seamless Co-op is not affected (it reads flags, it does not ship event files; S3 §5).
2. **Clock jump in a session:** only the host's clock matters (the game sends it on); a client's jump would be overwritten
   `[HYPOTHESIS]`. The console command is dev-only.
3. **Night length:** the night lasts as long as our timer; in vanilla it lasts as long as the fight. The circle stays closed at its
   final size the whole time `[HYPOTHESIS]`.
4. **Variations 1 / 2** (§1) are not covered by v1.
5. **The decoder is ours:** argument layouts come from the EMEDF; the boss-flow reading was cross-checked against S3's 6PF
   decompilation (same events, same flags).

## 6. M5-4: death at night, the alive count, and the two endings (static, 2026-10-22)

Leonardo's design (PROJECT_SCOPE §2): no respawn at night, the dead spectate, a night ends when one player is left, only the
winner is healed, night 2 ends the match (win / failure), no Day 3. What the 1.03.3 event files say:

| Finding | Evidence | Tag |
|---|---|---|
| **Team type 77 = "dead at night, still in the match".** No event script ever sets 77; the engine does. `common` 1184 treats 1, 2, 20, 29, 77 and 0 as "still a participant" (anything else clears that slot's alive and revival flags); 1182 and 1310 test 77 on the local player | `common` 1182, 1184, 1310 | `[VERIFIED: static]`; which engine code sets 77 and when it is cleared: UNKNOWN (Nucleus run) |
| **Alive flags 7005 / 7006 / 7007 (player no. 1 / 2 / 3).** Event 50 starts 1182 in each game for its own player number only (`Is Player No`), so **each game keeps its own player's flag**, network-wide. 1182: ON at start; at night (7515) or Day 3 (7510) OFF when the player's team leaves {1, 2, 20, 29}; back ON when it returns. Revival flags 7010–7012 (1183) follow the revive items (SpEffects 540155 / 6999100) | `common` 50, 1182, 1183 | `[VERIFIED: static]` |
| Only 3 slots | 7005–7007 cover players 1–3 (vanilla trio). Six players need our own count: the DLL already reads every player's team value (the T-008 team controller); 77 and HP 0 mean "out" | — | design |
| **The wipe check** | `common` 1180 (multiplayer, run by every game): night or Day 3, all of 7005–7007 OFF, no revival pending → SpEffect 102140, banner 4010, 7516, 5 s, **7001 game over**. With PvP this fires on a double KO at night; it must be replaced (M5-5) | `common` 1180 | `[VERIFIED: static]` |
| Solo | `common` 1181: a solo player at 0 HP (or team 77) at night → the same failure ending, unless a revive item is held. Matches Leonardo's run on 2026-10-22 (died at night solo → back to the Roundtable) | `common` 1181; TESTING §3 M5-2 | `[VERIFIED: static + run]` |
| **Win ending = local flag 7000**: every Nightlord map does `Display Text Effect(4000)` (victory banner) → 15 s → `Set Event Flag(7000)`, a plain local flag (not network-connected). `Finalize Expedition` (2003[103]) runs only in play mode 2 | `m16`/`m18`/`m19` x002590, `m18` 18002620 | `[VERIFIED: static]`; that 7000 set outside a Nightlord map gives the result screen: Nucleus/solo run |
| **Failure ending = local flag 7001** after banner 4010 and 7516 (1180 / 1181) | `common` 1180, 1181 | `[VERIFIED: static + solo run]` |
| ⇒ **The endings can differ per game:** both flags are set by each game for itself, so the winner's game can play the win while the others play the failure | — | `[HYPOTHESIS, strong]`; Nucleus run |
| Night heal | `90065900` (vanilla) is **not** own-world gated, so every game heals its own player (`Bonfire-like Recovery`). Our M5-3 night-end event **is** host-only, so in a session only the host would be healed: fine for "only the winner", but the heal must move to an event that runs in every game and checks "my player is alive" | `common_func` 90065900; `gamedata/emevd-nights.json` | `[VERIFIED: static]` |
| Morning revival | Nothing in the event scripts revives a dead (77) player at the morning; vanilla relies on the engine (or on `Bonfire-like Recovery`) | — | UNKNOWN: Nucleus run |
| Spectator camera | Not in the event scripts (only `Change Camera` for special maps, `90075165`); the engine's dead-player camera | — | UNKNOWN: Nucleus run |
| Day 3 | `90005910` (after 7508) offers the warp; replace it with END in M6-1 | `common_func` 90005910 | `[VERIFIED: static]` |

**Nucleus caveat:** the Nucleus handler starts each instance through Seamless Co-op's launcher, not me3, so **our event files are
not loaded there** (bosses come back). The dev console `flag <id> [0|1]` (reads any flag; writes ours and 7000 / 7001 / 7502 /
7507 in one game) lets a Nucleus run end a night and play the endings without them. Whether a flag written by the DLL in the host's
game reaches the other games is itself one of the questions (S4).

**Nucleus run 1 (2026-10-22, TESTING §3):** the alive flags are shared and drop on **downed**, not on death; **a bled-out player
comes back by itself ~7 s later with full HP, by day and at night** (no spectator phase before the boss fight), so "out until the
morning" needs us to stop that comeback at night (where the game does it: UNKNOWN, next RE); all standing players down at once at
night = the vanilla wipe (game over for both games).

**What the Nucleus run answers** (runbook: TASKS M5-4): (1) what a player who dies at night sees (spectator camera on whom?), their
team value (77?) and alive flag; (2) whether the dead player comes back at the morning when the host's game sets 7502, and where;
(3) whether 7502 set by the host's DLL reaches the client (7503 / 7505 there); (4) the endings: `flag 7000 1` in one game and
`flag 7001 1` in the other.

## 7. Sizes and cards

| Card | What | Size |
|---|---|---|
| **M5-2** | `clock` console command (built here) — Leonardo's solo check that the jump works and what the circle does | built; 10 min of play |
| **M5-3** | No-boss nights v1: replace `90055000` / `90055001`, add the two night-end events and the `night_end` console command | ½ day code + 20 min solo test (with `clock end`) |
| M5-4 | Night 2 arena polish (final circle size / keep players inside, F7) — after M5-3 shows what the circle does | later |

Cards in TASKS.md (M5 section).
