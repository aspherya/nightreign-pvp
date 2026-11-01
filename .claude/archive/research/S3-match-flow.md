# S3: Day/night, tide, and bosses (match flow)

Researcher: Sonnet | Date: 2026-10-10 | Status: review
Scope: NR only unless labelled ER. Read-only: nothing launched, patched, installed or built; no regulation.bin or game archive opened.
Tags as in TECHNICAL_NOTES.md. Shorthand (all downloaded 2026-10-10 into the session scratchpad, not the repo):
- `fsrs-param` = https://raw.githubusercontent.com/vswarte/fromsoftware-rs/main/crates/nightreign/src/param.rs (59,023 lines)
- `PD` = https://raw.githubusercontent.com/soulsmods/Paramdex/master/NR/Defs
- `EMEDF` = https://raw.githubusercontent.com/AinTunez/DarkScript3/master/DarkScript3/Resources/nr-common.emedf.json and https://soulsmods.github.io/emedf/nr-emedf.html (the "Used in" lists)
- `FLAGS` = https://github.com/vawser/Smithbox/tree/main/Documentation/Game%20Information/NR/event_flags/english (community flag names; `EFID_common` = file "NR-Test -- EFID_common -- Row Names.txt")
- `6PF` = https://github.com/Umgak/Nightreign6PlayerFixes (branch `mod`, MIT). Its `project/event/*.emevd.dcx.js` are **DarkScript3 decompilations of the real NR event files** (common, common_func, m12, m18, m19, m30_30, ...), edited only where the mod needed. Line numbers below refer to those files. Unedited vanilla logic is visible in them, but it is the mod's snapshot (commit 2026-07-09), not necessarily our game build.

---

## 0. Bottom line for the planner

1. **The expedition timeline is an EMEVD state machine driven by event flags 7500-7512 and by one clock, the "play area" clock** (`PlayAreaCurrentTime`). Day = clock 00:00-22:59 in event terms, night/boss phase = 23:00-23:59. [SOURCE: 6PF common.js#L823-L1000; EMEDF] The names of the flags are community labels (Day 1 started, night boss defeated, interval started, Day 2 started, ..., Day 3 Nightlord defeated, game clear 7000, game over 7001). [SOURCE: FLAGS EFID_common 7000-7523]
2. **The circle itself is probably engine + param driven, not EMEVD**: `PLAY_AREA_CREATE_COMMON_PARAM_ST` has `first/second_circle_start/end_hour/minute`, `initial_circle_radius`, `second_circle_radius`, per-day rain SpEffects. [SOURCE: fsrs-param#L46038; PD/PlayAreaCreateCommonParam.xml] No EMEVD in the 6PF decompilation shrinks a circle (EMEVD only *queries* it: `InsidePlayArea`, `InsidePlayAreaByTime`). [SOURCE: 6PF, grep] [HYPOTHESIS: engine reads the params and the clock]
3. **A night can end without its boss by setting the "night boss defeated" flag (7502 for Night 1, 7507 for Night 2).** In vanilla only the boss-defeat event sets them. Everything after (interval flag, Day 2 start, clock reset to 07:00) hangs off those flags. [SOURCE: 6PF common.js#L848-L862, L902, common_func.js#L13143-L13174] Whether the *game* accepts a flag set from outside EMEVD (our DLL) and syncs it is UNKNOWN (S4).
4. **An expedition ends as a win when event flag 7000 ("game clear") turns on.** In vanilla it is set 15 s after the Day 3 Nightlord is defeated (flag 7512), by a map event in `m18_00_00_00`. `FinalizeExpedition` (EMEVD 2003[103]) is used only in m18. [SOURCE: 6PF m18_00_00_00.js#L56-L64, #L109-L113; EMEDF page "Used in m18_00_00_00"]
5. **Modified `regulation.bin` + a modified `event/` folder are already used with Seamless Co-op in the wild** (Nightreign 6 Player Fixes, Fun is Allowed) via me3 profiles with SC `load_early`. This answers the "does SC block modded regulation/events" question in the favour of the EMEVD/param route, with the caveat that both are *mods the author of SC knows of* (SC even has an event-flag handshake with 6PF). [SOURCE: Nexus NR mods 596 and 49; 6PF common.js#L14] Sections 5-6.
6. **A DLL that controls the day clock already exists and works in co-op** (Nightreign Storm Timer v1.6.x: speed, freeze, jump, restart the day; host controls, clients follow). So the clock is hookable and host-authoritative. [SOURCE: Nexus NR mod 923] (Closed-source DLL; mechanism not read.)

---

## 1. Vanilla structure (what the game does)

### 1.1 Public description

| Item | Finding | Tag |
|---|---|---|
| Length | 3 in-game days. Days 1 and 2 are about 15 min each and end with a night boss. Day 3 is only the Nightlord | [SOURCE: https://eldenring.wiki.gg/wiki/Nightreign:Expeditions] (page summarised by the fetch tool; stub-level wiki, numbers are community timings) |
| Circle phase 1 | Large circle appears at about 4:30, shrinks over about 3:00 (ends about 7:30) | same |
| Circle phase 2 | At about 11:00 the second prompt, shrinks faster over about 3:00, funnelling to the boss arena (white tree) | same |
| Night boss | Once the ring is close, a wave of small enemies, then the boss; "the night does not progress until the boss is defeated" | same; [SOURCE: https://deltiasgaming.com/?p=247780 via search summary, secondary] |
| Day transition | Boss death -> HP/FP/flasks refilled -> about 30 s -> Day 2, map reset to full size and rearranged | same |
| Day 3 | After Night 2 boss: Spirit Shelter, then the Nightlord arena; entirely a boss fight | same |
| Rain/storm outside circle | Gradual damage, accelerates after about 2 min outside | same |
| Special events | One per expedition; Day 1 events about 6-8 min in, Day 2 about 2-8 min | [SOURCE: https://eldenring.wiki.gg/wiki/Nightreign:Special_Events] |

### 1.2 What the event scripts show (primary, decompiled)

The play-area clock runs 07:00 -> 23:59 each day (a Storm Timer feature "start the day over (clock back to 07:00)" and EMEVD `ResetTimeOfDay(7,0,0,4)` agree). [SOURCE: 6PF common.js#L902; Nexus NR mod 923]

| Step | Mechanism | Tag |
|---|---|---|
| Day 1 start | Event `1100` waits `IsGameMode(2)` (expedition) then sets **7500** | [SOURCE: 6PF common.js#L821-L825] |
| Night begins | Clock enters 23:00-23:59. Night flag 7515 is set when clock is in 23:59:00-23:59:59 | [SOURCE: common.js#L933-L939] |
| Preliminary enemies | Common event `90055000`: when clock >= 23:00 **and** (a player is in the boss area **or** clock is 23:59) sets **7501** (Day 1 prelim) or **7506** (Day 2 prelim), spawns the minions | [SOURCE: common_func.js#L10647-L10731] |
| Night boss starts | Common event `90055001`: prelim flag AND clock 23:59 AND (minions dead or timer flag) -> sets **7504** (Day 1) / **7509** (Day 2) "night boss battle started" | [SOURCE: common_func.js#L10733-L10756] |
| Night boss dies | Common event `90065900` (boss-defeat handler, takes `value` 1 or 2): waits the boss flag, shows text, `IncrementTeamBossesKilled(1)`, then sets **7502** (value 1) or **7507** (value 2), then `BonfirelikeRecovery` | [SOURCE: common_func.js#L13133-L13185] |
| Interval | `1100`: after 7502 -> sets **7503** "interval started" | [SOURCE: common.js#L826-L841] |
| Day 2 start | `1103`: after 7503 -> 20 s -> **`ResetTimeOfDay(7,0,0,4)`**. `1100` sets **7505** (Day 2 started) once the clock reads <= 22:59 again | [SOURCE: common.js#L842-L846, L879-L903] |
| Night 2 | Same chain with 7506/7509/7507/7508. After 7507 the interval flag 7508 is set | [SOURCE: common.js#L848-L862] |
| Day 3 | m12 (Spirit Shelter) events set **7510** (Day 3 started) after all players are in the area; `TargetingBoss(TargetBoss.X)` picks the Nightlord. Manual warp flags **7522/7523**; common `1200` warps players to m12 | [SOURCE: 6PF m12.js#L26-L92; common.js#L2184-L2191] |
| Nightlord dies | m18 event `18002800`: boss HP <= 0 -> `IncrementTeamBossesKilled`, sets **7512** (Day 3 boss defeated) | [SOURCE: m18_00_00_00.js#L124-L150] |
| Win | m18 `18002590`: waits 7512 -> `DisplayTextEffectId(4000)` -> 15 s -> **`SetEventFlag(7000)`** (game clear). `18002620`: after ending flags -> `SetEventFlag(6020)` + **`FinalizeExpedition()`**. Common `1100` ends on 7000 | [SOURCE: m18_00_00_00.js#L56-L64, #L109-L113; common.js#L864] |
| Loss | Common `1180`: in multiplayer, once 7515 (night) or 7510 (Day 3) and **no player "I am alive" flag** (7005-7007, and 11007005+ in 6PF) and no revive, then SpEffect 102140, text, **7516 (game over confirmed)**, 5 s, **7001 (game over)**. Skipped if the boss just died (7513) | [SOURCE: common.js#L1795-L1830, L1935-L1960] |
| "Alt" night | Map variation 1 uses flag 8020 "night bosses appear consecutively" instead of 7502/7507 | [SOURCE: common_func.js#L13133-L13148; FLAGS 8020, 7518] |

Notes:
- Flags are written with `SetNetworkconnectedEventFlagID` and guarded by `PlayerIsInOwnWorld()`, i.e. **the host (own world) sets match-flow flags and they propagate**. [SOURCE: common.js#L823-L862] [HYPOTHESIS: `PlayerIsInOwnWorld` = host/solo]
- `EFID_common` also names `2200 "Time progress stop: Shrink"` and `2201 "Time progress stop: Appearance"` (flags that freeze the clock during circle events) and `9052 "Warped to Tower of God"`. Consumers of 2200/2201 were not found in the decompiled files, so they may be engine-side. [SOURCE: FLAGS; grep of 6PF] [HYPOTHESIS]
- `EFID_common` 7650-7699 are "Final reduction day1/day2 <map tile>" flags (about 50, one per `m60_xx_xx` tile). Not referenced in the decompiled common events. [SOURCE: FLAGS; grep] [HYPOTHESIS: they are `PLAY_AREA_CREATE_PARAM_ST.day1Flag/day2Flag` values, i.e. which tile is the final circle]
- Flag ranges: 7000-7020 per-player alive/revival/mission, 7500-7523 timeline, 7600-7649 shifting-earth, 7700-7739 sudden events. 150-160 "3-Day Table Mode: Defeat ..." and 8900-8914 "[1 day try] ..." exist: **NR has 1-day and 3-day sub-modes in its data**; not explored.

---

## 2. Which systems implement it

### 2.1 EMEVD

Files that matter (from the decompilation set in 6PF and the EMEDF "Used in" lists):

| File | Role | Tag |
|---|---|---|
| `common.emevd` | Timeline chain 1100-1108, game-over 1180, warp to Spirit Shelter 1200, clock reset 1103 | [SOURCE: 6PF common.js] |
| `common_func.emevd` | Reusable bosses/prelim/defeat events (`90055000`, `90055001`, `90065252/3/4`, `90065900`), raids, sudden events | [SOURCE: 6PF common_func.js] |
| `m12_00_00_00` | Spirit Shelter / Day 3 start (7510), Nightlord selection | [SOURCE: m12.js#L26-L92] [HYPOTHESIS: map identity from content] |
| `m18_00_00_00` | Nightlord arena: 7512, 7000, 6020, `FinalizeExpedition`, `SetCharacterTeamType` on arena NPCs | [SOURCE: m18_00_00_00.js; EMEDF "Used in"] [HYPOTHESIS: map identity from content] |
| `m10_00_00_00` (+dlc01) | Roundtable Hold / lobby (missions, scenario battle, menu flags). Not the expedition | [SOURCE: EMEDF "Used in" list: missions, `WarpPlayerToScenarioBattle`] [HYPOTHESIS: identity] |
| `m30_00_00_00`, `m30_30_00_00` | Use `InsidePlayArea` / `PlayAreaCurrentTime`; probably the Limveld base map(s) | [SOURCE: EMEDF page] [HYPOTHESIS: identity] |
| `m60_*` tiles | Per-tile events (weather, small bases, final-circle tiles) | [SOURCE: EMEDF page "ChangeWeather" used in m60_42_38_10 ...] |

Relevant NR EMEVD instructions (class[index]; args from `nr-common.emedf.json`):
- Clock/time: `1[6] IfPlayAreaCurrentTime`, `1[7] IfPlayAreaCurrentTimeInRange(h,m,s,h,m,s)`, `1[4]/1[5] IfTimeOfDay(InRange)`, `2003[89] ResetTimeOfDay(unusedH,unusedM,unusedS,duration)` (used in `common`).
- Circle: `3[45] IfInsidePlayArea(entityId, thresholdDistance, inPlayArea)`, `3[48] IfInsidePlayAreaByTime(entityId, thr, in, h, m, s)` (used in common_func, m53_05, m30*, m49_42, common).
- Expedition: `2003[103] FinalizeExpedition(dummy)` (m18 only), `2003[101] IncrementTeamBossesKilled`, `3[54] IfCompareTeamBossesKilled`, `3[57] IfIsCampaignSession`, `2003[117/118] Increment/SetMissionProgress`, `2003[109/110] StealPlayerLevel / GainSoulsFromStolenLevels` (common_func, relevant to F9 "level loss"), `2003[23] SetPlayerRespawnPoint(areaEntityId)` (unused in vanilla), `2003[74] IssueEndOfPseudoMultiplayerNotification` (unused), `2004[4] ForceCharacterDeath`, `2003[96] RandomlySetNetworkconnectedEventFlagInRange` (pattern/lottery in common_func and some m60/m52 tiles).
- The EMEDF enum `Scenario Section` (Wylder, Guardian, ...) is the per-class personal scenario, **not** the expedition day. [SOURCE: EMEDF enum 47]
- Weather: `2003[68] ChangeWeather(weather, lifespan, immediately)`; `IfWeatherActive`, `IfWeatherLotActive`.

### 2.2 Params (candidate list; field names from fsrs-param, with PD names where they differ)

| Param (struct) | Fields of interest | Role (tag) |
|---|---|---|
| `PLAY_AREA_CREATE_COMMON_PARAM_ST` (fsrs-param#L46038) | `first_circle_start_hour/minute`, `first_circle_end_hour/minute`, `second_circle_start_hour/minute`, `second_circle_end_hour/minute`, `initial_circle_radius`, `second_circle_radius`, `initial_circle_area_no/grid_x_no/grid_z_no/pos_x/pos_z`, `sp_effect_first_day`, `sp_effect_second_day` (PD: `day1RainSpEffect`, `day2RainSpEffect`); ~20 `unknown_*` incl. f32 `unknown_2..4`, `unknown_19..21` | Circle schedule and radii, storm SpEffect per day. Hours are probably play-area-clock hours [HYPOTHESIS]; meaning of the unknown f32s UNKNOWN |
| `PLAY_AREA_CREATE_PARAM_ST` (#L46243) | `areaNo, gridXNo, gridZNo, posX, posZ` (PD only), `day1Flag, day2Flag, requireModifier1/2, excludeModifier1/2, bossAttachPoint, extraBossAttachPoint, connectColEntityId`; fsrs-param exposes only `world_map_point_id_1/2` | A candidate final-circle centre per map tile, flags 7650+ [HYPOTHESIS] |
| `PLAY_AREA_CREATE_DEFAULT_PARAM_ST` (#L46222) | all `unknown_*` (3 f32, 5 s32) | UNKNOWN |
| `LOT_RESULT_PLAYAREA_PARAM_ST` (PD: LotResultPlayAreaParam) | `patternId, playArea1, playArea2, bossId1, bossId2, extraBossId1/2, bossModifier1/2, extraBossModifier1/2` (fsrs-param exposes only `play_area_create_id`) | Per-pattern result: final area and night boss for day 1 / day 2 [HYPOTHESIS from names] |
| `MAP_PATTERN_SET_PARAM_ST`, `MAP_PATTERN_COMMON_ST`, `MAP_PATTERN_MAP_LOT_CONDITION_ST` (`targetBoss1/2`), `LOT_BASE_MAP_PATTERN_FLAG_ST` (`eventFlag, weight, require/excludeModifier`) | The lottery that picks the map pattern per Nightlord | [SOURCE: PD]; mostly `unknown_*`, [HYPOTHESIS: roles] |
| `PLAY_REGION_PARAM_ST` (#L46287) | `boss_area_id`, `boss_id_1..16`, `match_area_id`, warp-item permit/prohibit bonfire ids and event-flag ids | Per-region boss registry/permissions |
| `NIGHT_BOSS_MENU_PARAM_ST` (#L38451) | `expedition_name_id, boss_icon_id, unlock_event_flag, defeat_event_flag, boss_name_id, sort_id, ...` | **UI/unlock list**, not logic |
| `CEREMONY_PARAM_ST` (#L7940) | `event_layer_id, map_studio_layer_id, multi_play_area_offset, override_map_place_name_id` | Seasonal event layers (`IfCeremonyActive` is unused in vanilla event scripts) |
| `ACROSS_DAY_CORRECT_PARAM_ST` (#L14), `CLEAR_COUNT_CORRECT_PARAM_ST` (#L9219: hp/atk/def/soul rates), `LIMITED_DAY_ASSET_PARAM_ST` (#L31923) | scaling and day-limited assets | Names suggest Day 2 stronger enemies [HYPOTHESIS]; `across_day` fields are all `unknown_*` |
| `MISSION_MANAGEMENT_PARAM_ST`, `PERSONAL_SCENARIO_PARAM_ST`, `SORTIE_STARTING_POINT_ST`, `SCENARIO_PLACEMENT_PARAM_ST` (`chara_init_param_id`) | missions, sortie start points (F3), per-scenario spawn | not circle/time |
| `WEATHER_*`, `CUTSCENE_GPARAM_TIME_PARAM_ST` | weather lots | cosmetic/storm look |

No param was found that sets the **day length** or the **night boss trigger time**; EMEVD hard-codes the 23:00/23:59 thresholds, and the clock speed is not a param here. [SOURCE: grep of fsrs-param struct list, 59,023 lines] [HYPOTHESIS: clock rate is engine/code]

### 2.3 Engine singletons (from `.claude/experiments/logs/2026-10-09_TC-004_runs1-2.log`, 235 names, game 1.3.3.0)

| Singleton | Slot | Instance state | Use for S3 |
|---|---|---|---|
| `WorldAreaTime` | exe+0x3C125D8 | non-null in expedition | The clock. Probable target for speed/freeze/jump [HYPOTHESIS; Storm Timer proves a clock is hookable] |
| `WorldPlayAreaManager` | exe+0x3C13158 | non-null | Circle/play area state [HYPOTHESIS] |
| `WorldAreaWeather` / `CSWeatherLotRegionMan` | +0x3C12648 / +0x3C124B0 | non-null / null at start, set after load | weather |
| `CSEventFlagMan` | exe+0x3C115A8 | non-null | Event flag storage: read the timeline flags; write 7502/7507/7000 [HYPOTHESIS: layout] |
| `CSEventMan`, `CSEmevdRepository`, `EventNonActiveBlockManager`, `CSEventState`, `CSLuaEventMan` | -- | -- | Event script runtime and repository (where loaded EMEVD lives) |
| `CSScenarioPlacementManager` | exe+0x4433A98 | non-null | Scenario placement (F3 spawns) [HYPOTHESIS] |
| `CSPlayRegionPointMan`, `CSFastTravelOverrideRegionMan`, `CSRespawnPointMan`, `CSAutoInvadePoint` | -- | some null at start | respawn/play-region data (F9, F11) |
| `CSTeamSessionManager`, `CSTeamSessionDataMan`, `CSSessionManager`, `CSNetMan`, `CSNetworkUserManager`, `CSPlatformNetworkMan`, `CSBlockPlayerList` | -- | -- | session/team layer |
| `CSRegulationManager`, `FD4ParamRepository`, `SoloParamRepository`, `SystemParamRepository` | -- | -- | game's own regulation/param loading (not SC's `YuiNexusRegulationManager`) |
| `CSShareClearDataInsMan`, `CSShareClearDataPointMan`, `CSTrophy` | -- | -- | clear data (expedition result?) [HYPOTHESIS: name only] |

No singleton named Nexus, Hamari, Balancer (beyond `CSLoadBalancer`, a render/memory loader), or quickmatch exists in the 235. `CSHamariSimulateChrIns` is a ChrIns subclass (replay), not a singleton. [VERIFIED: log read, 235 names] ER analogue: `WorldAreaTime` also exists as an ER singleton (ER, reference only; fsrs-param `eldenring` crate), so a field map may be portable [HYPOTHESIS].

---

## 3. Existing mods that touch timeline / bosses

| Mod | What it does | How | With SC? | Tag |
|---|---|---|---|---|
| **Nightreign Storm Timer** (v1.6.3) https://www.nexusmods.com/eldenringnightreign/mods/923 | Countdown by phase (Tide Approaches/Closes/Returns, Night Descends, Nightfall) **plus storm control**: speed up/down to 10x, freeze, jump 30 min, restart day at 07:00 | DLL (Elden Mod Loader); reads the game's own day clock and hooks its speed ("found the game clock, the window and the speed hook" in its log) | **Yes, co-op ready**: only the host's keys change the storm; partners' timers measure the host's real speed and follow. Boss night shows "Time stands still" | [SOURCE: Nexus page] |
| **Time Stop** (slop mod, v?, mod 917) https://www.nexusmods.com/eldenringnightreign/mods/917 | Freezes enemies, projectiles, **storm and day clock** | me3 native DLL, `disable_arxan = true`, tested on 1.03.3 / me3 0.11.0 | **No**: "in co-op only your own game freezes and the session falls out of sync" | [SOURCE: Nexus page] |
| **Nightreign 6 Player Fixes** https://www.nexusmods.com/eldenringnightreign/mods/596 , source https://github.com/Umgak/Nightreign6PlayerFixes | Rewrites many events for >3 players, including the Defeat/game-over event ("not trigger until all players are down"), raids, Nightlords | Modified `event/*.emevd`, `map/mapstudio/*.msb`, `script/*.lua`, `regulation.bin`; plus LilyHook.dll (patches Libra clone cap) | **Yes, intended for SC.** me3 profile with SC `load_early = true` and the mod as a `[[package]]` | [SOURCE: Nexus page; repo tree; README] |
| **Fun is Allowed** v31 https://www.nexusmods.com/eldenringnightreign/mods/49 | Overhaul (param, HKS, UI) | me3 profiles `FIA_SeamlessCoop.me3` and `FIA_ServerRedirector.me3` (SC copied into its `native` folder in the old guide) | Yes | [SOURCE: Nexus page] |
| **Boss Arena** (PositronCannon, v3.1.12) https://www.nexusmods.com/eldenringnightreign/mods/449 | Boss select menu in Roundtable, Boss Rush mode, expedition-less | "changes map data, events, talk scripts, text"; me3 profiles only | **Incompatible with SC** ("designed for solo; SC is designed for real Expeditions") | [SOURCE: Nexus page] |
| **Nightreign Randomizer and Derandomizer** (thefifthmatt) | Re-seeded map patterns, boss placement, spawn points; options let the night boss appear anywhere | Not documented in what I could read (Nexus page not located). Press says it replaces the map-pattern templates and is compatible with SC | Reported yes (French outlet) | [SOURCE secondary: https://www.pcgames.de/Elden-Ring-Nightreign-Spiel-75093/News/Randomizer-Mod-neue-Herausforderungen-1477714/ ; https://www.gameblog.fr/jeu-video/ed/news/elden-ring-nightreign-mod-map-random-699102] Method UNKNOWN. A GamesRadar line (via search summary, **unverified**) says Night 1 and 2 bosses must be distinct "otherwise progression will softlock" |
| Deep of Night "Chance Setter" (mod 735), "(OFFLINE) ALLBoss Enable with CE" (915) | not read | Nexus list only | -- | UNKNOWN (titles only: https://www.nexusmods.com/eldenringnightreign/mods/735 , /915) |

No mod was found that "skips the night boss" or "lengthens days" via param/EMEVD in a way that works under SC. Searched: Nexus NR listing pages, web search (4 queries). Nexus search is JS-driven and returned only the newest list, so absence is **not proven**. UNKNOWN.

---

## 4. Tools and loading

| Question | Finding | Tag |
|---|---|---|
| Read NR EMEVD today | DarkScript3 3.6.3 (NR 1.03 instructions, "a few unknowns") opens `.emevd.dcx` and decompiles to JS. The 6PF repo shows the working result (header `@docs nr-common.emedf.json`, `@version 3.6.2`) | [SOURCE: https://github.com/AinTunez/DarkScript3/releases ; 6PF common.js#L1-L8] |
| Get the files out | Our game folder has no loose `event/`; files are in `data*.bdt` / `dlc01.bdt`. Smithbox File Browser, or **Nuxe v1.2.1** (JKAnderson, supports NR; unpack plus a loose-load exe patcher, we would **only** unpack to a scratch dir, never patch). Smithbox has no EMEVD editor | [SOURCE: T-011 section 3; https://github.com/JKAnderson/Nuxe README, release v1.2.1 2026-08-28; VERIFIED earlier directory listing] |
| Param editing | Smithbox 2.2.6 (NR supported) edits `regulation.bin`; WitchyBND regulation support for NR UNKNOWN (T-011) | [SOURCE: T-011] |
| Load a modified regulation / EMEVD with me3 | `[[packages]]` folder with the files at their game-relative paths (`regulation.bin`, `event/common.emevd.dcx`, `event/common_func.emevd.dcx`, `map/mapstudio/...`). Both 6PF and Boss Arena ship `event` folders through me3 profiles; me3 docs show `regulation.bin` as a package asset | [SOURCE: Nexus mods 596, 449; https://raw.githubusercontent.com/garyttierney/me3/main/docs/user-guide/creating-mod-profiles.md] This **closes T-011 open question 8 as "works in practice"** [SOURCE: two shipped mods]; not tested on our build |
| Profile syntax used with SC | `profileVersion = "v1"`, `[[supports]] game = "nightreign"`, `[[package]] id/path`, `[[natives]] path = ".../nrsc.dll" load_early = true`, extra `[[natives]]` for DLLs | [SOURCE: Nexus mod 596 install section] |
| Multiplayer rule | Every player needs identical event/regulation files. 6PF uses an in-game check (a custom SpEffect row 98400 that must exist; otherwise flag 11008100 shows a warning) and per-player install flags. This is a mod-level check, **not** an SC one | [SOURCE: 6PF common.js#L3660-L3680, L2443-L2453] [HYPOTHESIS: SC itself does not compare them, since the mod author had to build his own] |

---

## 5. Seamless Co-op interactions

| Fact | Tag |
|---|---|
| SC says other mods are fine "so long as they do not interfere with the multiplayer aspect or edit any of the items the mod uses to function". It uses its own launcher; "Mod-compatibility (due to EAC not running)" is a feature | [SOURCE: Nexus NR mod 3 description] |
| SC downloads from GitHub (`yuiamoroll/NightreignSeamlessCoopRelease`, last commit 2026-07-07) six data files: `BLOCKSYNC` (empty), `ENDLESSNIGHT` (`>1, 999, 1999, 3999, 5999, 10000`), `SUPERNIGHTLORD`, `SUPERNIGHTLORD2`, `SUPERNIGHTLORD3` (short integer lists, some prefixed `>5`), `VERSION` (list of versions with a 0/1 flag). Only numbers, no params, events or code. Meaning UNKNOWN; they replace what FromSoft's matchmaking servers supply for Everdark/Endless Night | [SOURCE: repo raw files, fetched 2026-10-10; Nexus mod 3] [HYPOTHESIS: roles] |
| Implication: those files do not carry regulation or event content, so SC **is not shipping a modded param/EMEVD set that ours would clash with**; the clash risk is limited to anything SC edits at runtime (scaling) | [HYPOTHESIS] |
| `YuiNexusRegulationManager` stays unexplained, but the evidence now tilts to "runtime scaling code, not a lobby gate": two shipped regulation mods work with SC, and 6PF built its own regulation check | [SOURCE: Nexus 596/49] [HYPOTHESIS] |
| SC reads at least one **event flag set by a modded EMEVD**: 6PF's common event 0 sets flag `11007000` "communicates to Seamless that I am here and that it is safe to use uncapped player count/numbers". So SC has code that watches event flags and the SC author coordinated with a mod. For us: SC can read flags, so assume it may also react to timeline flags (7500+) | [SOURCE: 6PF common.js#L14, globalFlags.js#L1-L2] [HYPOTHESIS: reaction scope] |
| SC's host pause key (F4) "halts night progress" (added 1.07). So SC already stops the clock/night itself; our clock control could fight it | [SOURCE: T-003 line 38, Nexus mod 3 changelog] [HYPOTHESIS: mechanism] |
| SC caps player numbers (event `IsPlayerNo`, 3 vanilla slots, flags 7005-7007) unless the 6PF handshake is present: relevant to F1/S1, not here | [SOURCE: 6PF README, common.js] |
| Quick-match/Endless Night is server-controlled in vanilla (Nexus note), so "Endless Night" is **not** an in-file mode we can reuse | [SOURCE: Nexus mod 3] |

---

## 6. Ranked routes

Common prerequisites for any EMEVD route: a **HUMAN** extracts `common.emevd.dcx` and `common_func.emevd.dcx` (and `m12`, `m18`) from our build with Smithbox/Nuxe (unpack-only into a scratch folder), opens them in DarkScript3, and diffs them against the 6PF decompilation to see how far our build has drifted. Without this the line numbers above are not binding.

### (a) A night with no boss that still advances

| Rank | Route | Mechanics | Must verify in-game |
|---|---|---|---|
| **1** | **Edited `common_func` / `common` event files** (me3 package, identical on all clients). In the `90055000/1` prelim and `90065252..4` boss spawn events, skip the spawn for the target day; add a trigger (e.g. clock >= 23:59 and the PvP phase over, or a custom flag) that sets **7502** (Night 1) via `SetNetworkconnectedEventFlagID`, so `1100/1103` run the interval, `ResetTimeOfDay(7,0,0,4)` and Day 2 start unchanged | The flow after 7502 is entirely EMEVD, so we only touch the cause. Precedent: 6PF edits these same files under SC | (1) Day 2 starts, map reshuffles, clock back to 07:00 and **the circle restarts** (the circle may be engine-gated by the boss flag, not only by the clock); (2) no reward/`BonfirelikeRecovery` side effects missing (heal, rewards from `90065900` are skipped if we skip the handler: decide which to keep); (3) late joiners and both peers agree on flags; (4) no softlock if we do not spawn the boss (the randomizer author reports softlocks when bosses are wrong) |
| **2** | **DLL sets flag 7502/7507** through `CSEventFlagMan` at the moment the match logic says "night over"; no file changes. Needs the prelim enemies not to spawn (they only spawn after clock >= 23:00 or a player in the boss area), so use it with a clock hold (route b.2) or accept the prelim wave | No regulation/EMEVD distribution problem. Risks: write API unknown, the flag may need the engine's network-connected setter to sync; `PlayerIsInOwnWorld` gating means only the host should write | (1) Writing the flag from the DLL triggers the same EMEVD continuation; (2) clients receive it (S4); (3) SC does not revert it |
| 3 | **Drive the clock only** (hook `WorldAreaTime`, Storm Timer style): jump the clock to 07:00 for Day 2 without the flag chain | Cheap, proven to exist as a feature. But Day 2 flags (7505, 7503) and map reshuffle come from the flag chain, so this likely leaves the world in Night 1's state. Use only as a helper | Whether Day 2 content appears at all |
| 4 | Param: make `LOT_RESULT_PLAYAREA_PARAM_ST.bossId1` empty | Highest softlock risk (unknown semantics of boss id 0), changes the lottery for everyone, and the boss registry also feeds `PLAY_REGION_PARAM_ST.boss_id_*` | Not recommended before the above fail |

### (b) A boss-less final arena with a controlled circle (Night 2)

| Rank | Route | Mechanics | Must verify in-game |
|---|---|---|---|
| **1** | **Param edit of `PLAY_AREA_CREATE_COMMON_PARAM_ST` (circle times, radii) in a modded `regulation.bin`**, plus EMEVD edit of the Day 2 boss spawn (as in a.1) and a custom end-of-match flow in EMEVD or the DLL | Radii and start/end times are exposed as named fields; `second_circle_radius` may define the final size [HYPOTHESIS]. Timing and detection use the existing clock conditions (`PlayAreaCurrentTimeInRange`, `InsidePlayAreaByTime`) | (1) Read real row values in Smithbox before editing (HUMAN); (2) what each field really does by changing one at a time in a solo run; (3) the final circle location and whether "boss area" scenery (white tree, gauntlet spawns) can be removed; (4) the clock freeze at 23:59: what stops it in vanilla (boss flag? flags 2200/2201?) and whether the circle stays closed with no boss |
| **2** | **DLL-controlled clock** (host sets `WorldAreaTime`, Storm Timer style): hold at the "closing" phase, slow the shrink, or run it fast; and a DLL damage/respawn rule for "fully closed" (F11) | No file distribution. Precedent for a host-authoritative clock that clients follow. SC's F4 pause shows SC touches the same clock | (1) Does a changed clock value shrink the circle continuously (circle derived from clock) or is it stepped from events; (2) clients follow (S4); (3) conflict with SC pause |
| 3 | **EMEVD-only**: new events using `InsidePlayAreaByTime` and `SetSpEffect` to simulate the shrink/damage, ignoring the engine circle | The most control but heavy, and the engine circle and rain would still run in parallel | Only if 1 and 2 fail |

Also note for (b): the vanilla game-over event `1180` fires when **no player is alive at night** (7515) even if the last standing players are in different PvP teams; with PvP this can end the expedition by accident (a simultaneous double KO, or the last team downed with others "alive" flags cleared). It must be neutralised (edit or flag) before any PvP night test. [SOURCE: common.js#L1795-L1830, L1935-L1960] [HYPOTHESIS: exact PvP interaction]

### (c) Ending the expedition as a win without the Nightlord

| Rank | Route | Mechanics | Must verify in-game |
|---|---|---|---|
| **1** | **EMEVD edit**: a new event in `common` (not a map file) that, on a custom "winner chose to leave" flag, mirrors what m18 does after the Nightlord: `DisplayTextEffectId(4000)`, wait, `SetEventFlag(7000)`, `SetEventFlag(6020)`, `FinalizeExpedition()` | Uses only vanilla instructions in the vanilla order; also possible to set 7512 first so any other 7512 listeners fire. Precedent for editing `common`/`common_func` with SC | (1) `FinalizeExpedition` works outside m18 (EMEDF lists it only in m18); (2) relics/Murk are awarded on a "win"; (3) save state afterwards (.co2) is sane; (4) works with two players, one of whom chose "end" while others are dead/respawning |
| **2** | **DLL writes flag 7000** (and 6020) | Smallest change; 7000 is in the global range, no map dependence. What consumes 7000 is not in the event files (only `common` 1100 waits on it and ends), so the consumer is engine code [HYPOTHESIS] | (1) A bare 7000 ends the expedition with a win screen; (2) clients agree (7000 is written by the host's own-world event in vanilla) |
| 3 | **Keep the Nightlord path for the other choice**: warp winners to m12/m18 using the existing `7522/7523` manual-warp flags and event `1200`; "end now" = route c.1/c.2 | The "fight the Nightlord" choice is native and needs no new code beyond the prompt | Who triggers the warp (host only); `TargetingBoss` picks the Nightlord from the pattern |

Unranked helper for all three: **`IncrementTeamBossesKilled(1)`** and **`IfCompareTeamBossesKilled`** exist (used in common_func); in vanilla they likely drive rewards/unlocks, so decide whether we call them.

---

## 7. What a human should do first (cheap, read-only)

1. **Smithbox, read-only, scratch project:** open `PlayAreaCreateCommonParam`, `PlayAreaCreateParam`, `LotResultPlayAreaParam`, `PlayRegionParam` rows; record all `first/second_circle_*`, radii, `sp_effect_*day` values and the number of rows. Never save over the game. (T-011 section 7 has the same safety note.)
2. **Extract** `event/common.emevd.dcx`, `common_func.emevd.dcx`, `m12_00_00_00`, `m18_00_00_00` from our game archives into a scratch folder (Smithbox File Browser, or Nuxe unpack-only; Nuxe's exe patching is a game-file modification and should not be used). Decompile with DarkScript3 3.6.3; diff against 6PF; confirm the flag chain of section 1.2 on our build.
3. **Watch the timeline flags live** (our DLL can poll `CSEventFlagMan` for 7500-7523, 7000, 7001, 2200/2201, 7650-7699 during a solo expedition and log transitions). This is a read-only probe and the cheapest way to turn most `[HYPOTHESIS]` rows above into `[VERIFIED]`. Needs the flag-storage layout first (RE of `CSEventFlagMan`; ER layout as a hint only).
4. **Read the play-area clock** from `WorldAreaTime` the same way (value and rate) and correlate with circle phases.

---

## Open questions

1. Which fields of `PLAY_AREA_CREATE_COMMON_PARAM_ST` really set the circle schedule and the final radius? What are `unknown_2..4`, `unknown_19..21`? (Needs row values and single-field experiments.)
2. What stops the clock at 23:59 until the boss dies (flags 2200/2201? a boss flag? engine code), and what happens to the circle with no boss?
3. Does `FinalizeExpedition` run from `common`, or only from m18? What exactly consumes flag 7000 (engine result screen)?
4. Can our DLL write event flags (`CSEventFlagMan` layout in NR), and do such writes propagate to clients and survive SC (S4)?
5. Are `PLAY_AREA_CREATE_PARAM_ST.day1Flag/day2Flag` the same flags as `EFID_common` 7650-7699 ("Final reduction ...")?
6. Does `LOT_RESULT_PLAYAREA_PARAM_ST.bossId1/2` mean Day 1 / Day 2 night boss? Is there a clean "no boss" value?
7. How closely does the 6PF snapshot (July 2026) match our build 1.3.3.0 / regulation dated 2026-04-01 (the 6PF event files may already depend on a newer regulation)?
8. Do the SC data files (`ENDLESSNIGHT`, `SUPERNIGHTLORD*`) mean anything for the timeline, and does SC's pause (F4) write the same clock our DLL would?
9. What do the "3-Day Table Mode", "[1 day try]" and `IsMapVariation(1)` (flag 8020) paths in the event files do? A 1-day or continuous-night variant might already be a cheaper base for "two days, two nights".
10. The randomizer's actual method (param vs EMEVD vs MSB) was not read; the Nexus page was not located.
