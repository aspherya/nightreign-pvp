# OFF-3: can the game run AI Nightfarers as rival bots in an offline match? (research memo, 2026-10-22)

Game 1.03.3 (`nightreign.exe` 1.3.3.0), regulation from `Z:\Tools\nr-unpacked\1.03.3\regulation.bin` (WitchyBND 3.0.1, NR
paramdex). Static work only; nothing was run in the game. Scripts: `experiments/OFF-3/` (`off3_bots.py` reproduces the code facts).

## Verdict: GO for offline bots (spike first), NO-GO for bots in multiplayer matches for now

**The game already contains a complete, dormant "NPC bot" system that spawns AI Nightfarers.** It has its own manager
(`CSNpcBotMan`), its own param table (`NPCBotTableParam`), ready-made Nightfarer loadouts for every hero at three level tiers (6, 11, 13), a
210 KB AI script that plays a Nightfarer (weapon skill, ultimate art, items, spells, guarding, reviving downed allies), and two
event-script instructions that start and stop it. No shipped event script calls them, so nobody sees bots in vanilla play. Our
game-data package already appends events to the game's event scripts (UI-2), so we can call the game's own instructions without
patching code.

What is not known yet, and only a game run answers: whether bots actually appear in an expedition (models, navmesh spawn point),
how many spawn per table, and what the "≤ 1" gate counts. Multiplayer stays out: the start instruction refuses when that count is
above 1 (likely the party size), and whether other players' games would even see a local NPC is UNKNOWN.

## Evidence

### 1. Params (the data side) `[VERIFIED: regulation 1.03.3, WitchyBND]`

| Param | What it holds |
|---|---|
| `NPCBotTableParam` (`npcParamId`, `npcThinkId`, `charaInitParamId`, weight) | Tables of bots. Rows share an id = one table. Tables **10..80**: 2 rows each (NpcParam/NpcThink `600040000`, CharaInit 90000..90007, two heroes per table). Tables **10000 / 20000 / 30000**: 8 rows each, CharaInit 401xx..408xx / 4xx01 / 4xx02 = the 8 base heroes (heroId 1..8) at soul level **6 / 11 / 13** (one tier per table: roughly day 1, day 2, final night) |
| `CharaInitParam` 90000..90007, 40100..40802, 30000..30900 | Full Nightfarer loadouts: `heroId` (1..10), weapons, armour, `characterSkillWeapon`, `ultimateArtWeapon`, stats, starting items, model scale. 30000..30900 cover heroes 1..10 (incl. the DLC pair) |
| `NpcParam 600040000` | Bot body: `teamType` **2** (same as players in an expedition = ally), HP 1551, `disableRespawn` 1, `npcPlayerWeightType` 2 |
| `NpcThinkParam 600040000` | AI: `logicId` **30000**, `battleGoalID` **29999** |
| `SmallbaseInvationNpcParam` + `NpcParam 6000300x0` (`teamType` 6 = enemy) + `NpcThink 6000300x0` (logic 31000, battle 29999) | A second, separate system: hostile Nightfarer-style **invaders at small bases** (one row per hero, CharaInit 30000..30900). Its count is a map-pattern hot-spot setting (exe string `Game.Map.Pattern.HotSpot.DebugOverrideInvationNpcNum`) |

### 2. AI scripts (`script/*.luabnd.dcx`, HavokScript bytecode; read through their string tables) `[VERIFIED: strings]`

- `029999_battle` "CommonNPCTest29999_Battle" (210 KB): `Common_NPC_AI`, hero build patterns `PLAN_SP_EFFECT_HERO_{BALANCE, TANK,
  POWER, SPEED, TECHNICAL, SHOOTER, MAGIC, NECROMANCER}` and `PLAN_BUILD_PATERN_*_1..3`, `NPC_ATK_Skill`, `NPC_ATK_Ult`,
  `ExecRescueAction`, `TEAM_TYPE_DyingPC`, weapon/spell/item/skill selection, guard, parry, dash, jump. It plays a Nightfarer.
- `030000_logic` (the bots' logic): `TARGET_HOSTPLAYER`, `IsApproachingHost`: **follow the host player**, fight what it sees.
- `031000_logic` (the invaders' logic): search, approach, return home within 80 m (guards its camp).
- Exe strings next to the AI manager: "バディ小隊" (buddy platoon) and "NpcBot小隊" (**NpcBot platoon**).

### 3. Code (`nightreign.exe` 1.3.3.0) `[VERIFIED: static disassembly, 2026-10-22]`

| What | Where | Behaviour |
|---|---|---|
| `CSNpcBotMan` | FD4 singleton, slot `exe+0x3C0B518`; re-created on every area load (singleton log) | Ticked every frame by the world update (`exe+0x523728` → `exe+0x38D280`) |
| Event-script bank dispatcher | `exe+0x587470` (`edx = bank − 2001`, table `exe+0x587844`) | Bank **2003** → `exe+0x58CBA0` (`ecx = id − 3`, table `exe+0x5901E0`, 131 cases) |
| **`2003[97]` (int `tableId`)** | case `exe+0x58F556` → `exe+0x607950` → `CSNpcBotMan` method `exe+0x38CF70(this, tableId)` | Refuses if `[[exe+0x3C13258]+0xDC8]+0x14` **> 1** or `tableId < 0`. Otherwise reads every `NPCBotTableParam` row with that id, stores (NpcParam, NpcThink, CharaInit) triples in the manager (`+0x0C`, 12 bytes each), sets `+0x0` = tableId, `+0x4` = 1 (armed), `+0x40` = −1.0 (timer) |
| **`2003[98]` (no args)** | case `exe+0x58F56A` → `exe+0x60B090` → `exe+0x38D110(this)` | For each spawned bot (list `+0x28`..`+0x30`, 16-byte entries): sets its byte `+9` = 1, finds the character (`exe+0x518F30`) and calls `exe+0x3CD800` on it (removal: [HYPOTHESIS]); then clears the table (`+0x0` = −1, `+0x4` = 0) |
| Update `exe+0x38D280` | when armed (`+0x4`) | Walks the table and calls `exe+0x38DA10` / `exe+0x38E980` / `exe+0x38E730`, which use **WorldChrMan**, **CSWorldNvmManager** (navmesh), **CSHavokMan** and **CSWorldAiManager**: spawn on the navmesh and register an AI platoon [HYPOTHESIS from the callees; not traced] |
| Use in shipped event scripts | all 279 `event/*.emevd.dcx` | **None.** Bank 2003 ids 0..130 occur; 97 and 98 never (96 and 99 do) |

`exe+0x3C13258` is not a named singleton in our log. `+0xDC8 → +0x14` reads like a member count: in solo it should be ≤ 1
`[HYPOTHESIS]`. Offline mode (OFF-1) is solo, so the gate should pass.

### 4. The other leads

- `CSRebornBuddyMan` / `CSChrRebornBuddyModule` and `CSNecroBuddyMan` / `CSChrNecroBuddyModule` (+ `AI.NecroBuddy.*` debug
  switches): character-owned summons (the Revenant's family and one more summon kind), not free-standing Nightfarers. Not bots.
- `CSEzSelectBot.*` strings are the debug boot menu ("bot" = a menu helper), unrelated.
- Nexus NPC/summon mods: not studied this round (the game's own system made them unnecessary for the go/no-go).

## Likely mechanism for our rival bots

1. Offline match (OFF-1), solo. Our game-data package appends an event to `common.emevd` (runs on every map) that waits for one
   of our flags (group 10007) and runs `2003[97](tableId)`; another flag runs `2003[98]`. The DLL sets the flag (console first,
   later at the hawk drop).
2. The game spawns the table's Nightfarers near the host with the full Nightfarer AI. As shipped they are **allies** (team 2) and
   **follow the host**.
3. Rivals: the DLL writes team_type on the bots' ChrIns (`CHR_TEAM +0x74`, the same write we use for players; recognise bots by
   `CHR_ID +0x68` = 600040000) to the rival value; or the package ships a modified regulation with hostile rows (teamType 6) and
   our own tables. The DLL route needs no regulation edit.
4. Battle-royale behaviour (roam the map, ride the hawk, level up by day) is later work: logic 30000 only follows the host, which
   for a hostile bot means "hunt the player". Good enough for a first fight; tiers 10000/20000/30000 give day-scaled builds. The
   game loads `c0000.hks` as plain Lua from a package (TECHNICAL_NOTES §4b), so our own plain-text logic script for bots (roam
   toward a point, then fight) may load the same way `[HYPOTHESIS]`.

## Size estimate

| Step | Size | Who |
|---|---|---|
| OFF-4 spike: event in `common.emevd` + console `bots <table>` / `bots_clear` + a log line per bot ChrIns seen | S (1 day) | agent, then Leonardo solo 10 min |
| OFF-5 rival bots: team flip of bot ChrIns, a table of 5 rivals for a 6-player Solo match, bots start at the hawk drop | M (2–3 days) | agent + 2 solo runs |
| OFF-6 BR behaviour: roaming logic (our own logic script or DLL steering), day tiers, bot deaths counted by the match | L (1–2 weeks) | agent + several runs; scope after OFF-5 |

Multiplayer bots (fill empty slots of a real match): not before the gate and the network side are understood; separate research.

## First build card

**OFF-4 — Bot spike: spawn the game's own NPC bots in an offline expedition** (text in `TASKS.md`).
