# S6: Team spawn points (move each team's players to different start points)

Researcher: Sonnet | Date: 2026-10-10 | Status: review | Read-only: nothing launched, patched or built. Feeds F3 / T-028.

## Shorthand and pins
- `FS:<file>#Ln` = https://github.com/vswarte/fromsoftware-rs/blob/59fbd3b3b7daaf14aca47c9f73530493dba6bc79/crates/eldenring/src/cs/<file>#Ln . **All fsrs `eldenring` content is ER (ER 2.7.1.x). Hypothesis for NR.** Line numbers are from files fetched on 2026-10-10 at that commit (chr_ins.rs, chr_ins/module/physics.rs, chr_ins/module/fall.rs, net_chr_sync.rs, block_id.rs, position.rs).
- `PT:<file>#Ln` = https://github.com/veeenu/eldenring-practice-tool/blob/cc1a36c6138dba5d387dc7e33a00e0434635fc0b/<file>#Ln (ER practice tool, main on 2026-09-23). **ER.**
- `PD:<file>` = Paramdex @ ff7245e524329bc3eab00036723d2bd53384cedf, `NR/Defs/<file>.xml` or `NR/Names/<file>.txt` (https://github.com/soulsmods/Paramdex/blob/ff7245e524329bc3eab00036723d2bd53384cedf/NR/...). `NRP` = fsrs `crates/nightreign/src/param.rs`.
- `LOG:<n>` = `.claude/experiments/logs/2026-10-09_TC-004_run3_overlay.log` line n (NR 1.3.3.0, singleton map).
- `6PF` = Nightreign6PlayerFixes decompiled events (S3 shorthand).
- NR offsets below are **ER offsets, not shifted** unless stated. Verified NR shift (+8) covers only ChrIns +0x68/+0x70/+0x74/+0x78 (TECHNICAL_NOTES). ER layouts also moved between ER versions (PT:pointers.rs#L352-L367: player offsets 0x6B0/0x6B8/0x6C0 across versions), so treat every number as "where to start looking".

## 0. Bottom line
1. **Vanilla has a table of expedition landing points: `SORTIE_STARTING_POINT_ST`**, 20 named rows in Paramdex, each with a hawk route entity id and a spawn point entity id, gated by map-pattern modifiers. The start is data-driven per map pattern, and it is not our code's to choose. [SOURCE: PD SortieStartingPoint.xml#L10-L23; PD Names/SortieStartingPoint.txt]
2. **Moving the local player by writing the physics-module position (plus the player's block id when the tile changes) is the established ER technique** (practice tool "load position"). It is the preferred route; the engine-warp and EMEVD routes cost much more. [SOURCE: PT position.rs#L59-L77, pointers.rs#L535-L557]
3. **A local write should replicate**, because each client broadcasts its own player's placement and others only read it. Not verified in NR or in a Seamless Co-op session. [SOURCE: FS net_chr_sync.rs#L101-L104 (placement update, MSB space, receive-side buffer)] [HYPOTHESIS]
4. **Source of valid points is the real gap.** Nothing found gives walkable ground coordinates for NR. Engine lists exist as singletons (non-null during an expedition), but their layouts are unknown. Best candidates: (i) circle centre from params, (ii) positions of already-loaded NPCs, (iii) runtime dump of `CSRespawnPointMan` / `CSMsbPointMan`.
5. Timing: no known "landed" flag. Candidates are listed in section 5; the cheapest is a probe, not a guess.

## 1. Vanilla: how an expedition starts

| Item | Finding | Tag |
|---|---|---|
| Hawk/landing | Matches drop in together after class pick. No source states a single shared landing spot for the squad, nor that spots differ per player | [SOURCE secondary: https://en.bandainamcoent.eu/elden-ring/news/elden-ring-nightreign-the-official-starter-guide; web search 2026-10-10 found no explicit statement] UNKNOWN |
| Variation | Reviews say expedition start points are randomised; a guide says spawn locations are random; wiki says campsites "frequently appear near the landing zones at the start of Expeditions" (so landing zones are a named, limited concept) | [SOURCE secondary: https://onemoregame.ph/2025/05/elden-ring-nightreign-review/ ; https://eldenring.wiki.gg/wiki/Nightreign:Locations ; https://deltiasgaming.com/?p=250111 via search snippet only (page returned 405)] |
| Data table | `SORTIE_STARTING_POINT_ST` fields: `areaId, gridXNo, gridZNo, birdRouteEntityId (u32), requireModifier, excludeModifier1/2, spawnGridXNo, spawnGridZNo, spawnPointEntityId (u32)`. fsrs binding has them as `unknown_*` only | [SOURCE: PD SortieStartingPoint.xml#L10-L23; NRP#L50642-L50654] |
| Rows | **20 rows**, named by location and approach direction, e.g. "Stormhill South of Gate from Northwest", "Minor Erdtree from Northeast", "Below Summonwater Hawk from West". Row id looks like `10 42 36 00` (area/grid digits) [HYPOTHESIS on decoding]. Names are ER Limgrave place names, so this is the Limveld base-map landing set | [SOURCE: PD Names/SortieStartingPoint.txt] |
| Modifiers | `requireModifier/excludeModifier` link rows to map-pattern modifiers (same vocabulary as `PLAY_AREA_CREATE_PARAM_ST`), so the pattern restricts which landing rows are legal. The lottery that picks the row: UNKNOWN | [SOURCE: PD fields] [HYPOTHESIS: semantics] |
| Per-class spawn | `PERSONAL_SCENARIO_PARAM_ST.spawnpointEntityId` exists (Paramdex name; fsrs shows `spawnpoint_entity_id` too), and `SCENARIO_PLACEMENT_PARAM_ST` has `charaInitParamId`. Role in the sortie unknown | [SOURCE: PD PersonalScenarioParam.xml#L22; NRP#L45087, #L50058-L50071] UNKNOWN role |
| Event script | No hawk/start warp found in the 6PF decompilation of common/common_func/m12/m18/m19/m30_30. Event 1100 only sets 7500 after `IsGameMode(2)`. Flag 8800 is named "Map point management_Bird movement icon" | [SOURCE: 6PF common.js#L821-L825; Smithbox flags EFID_common 8800] The drop-in is therefore probably engine plus per-tile map events (m60 tiles, not in the decompilation set); UNKNOWN |
| Multi-player placement idiom | Vanilla warps all players to the Spirit Shelter with one call `WarpPlayerWithPlayerNo(12,0,0,0,12002310)`: a base area entity id plus the player number yields a per-player slot | [SOURCE: 6PF common.js#L2184-L2190; EMEDF 2003[93]] The engine has a per-player-number placement mechanism for EMEVD |

Nothing found on how the hawk animation is implemented (cutscene vs gameplay). UNKNOWN. Looked at: wikis, Paramdex, 6PF events.

## 2. Engine: where position lives (ER hints)

ChrIns -> modules -> physics. All ER, derived by reading field order:
- `ChrIns.modules: OwnedPtr<ChrInsModuleContainer>` at ChrIns **+0x190** [SOURCE: FS chr_ins.rs#L211; offset DERIVED by field order, matches PT:pointers.rs#L183]. In `ChrInsModuleContainer`, `physics` is the 14th pointer, **+0x68** [SOURCE: FS chr_ins/module.rs, field order; matches PT:pointers.rs#L527 `0x190, 0x68`].
- `CSChrPhysicsModule.position: HavokPosition` (x,y,z,w f32) at **+0x70** [SOURCE: FS physics.rs#L25; offset DERIVED by field order; PT:pointers.rs#L551-L554 reads x/y/z at +0x70/+0x74/+0x78]. Also `orientation` (quaternion, +0x50), `last_update_position` (+0x80), `chr_proxy_pos_update_requested` [physics.rs#L26-L28], `standing_on_solid_ground`, `touching_solid_ground` [#L29-L30], `is_falling`, `is_touching_ground` [#L67-L68], `gravity_disabled` [#L75].
- Player-only, `PlayerIns`: `block_position: BlockPosition {x,y,z,yaw}` "position within the current block", then `current_block_id: BlockId`, then `safe_block_pos`/`safe_block_id` (last safe-for-saving position, used for saves and signs), `play_region_id` [SOURCE: FS chr_ins.rs#L915-L926]. ER offsets (v2.07): block_position 0x6C0, current_block_id 0x6D0 [PT:pointers.rs#L361-L368, #L350-L356]. NR: +8 would be 0x6C8/0x6D8 [HYPOTHESIS]; team.rs in our repo already uses NR +0x6C0 for the session entry (ER 0x6B8), consistent with the shift.
- `ChrIns` header: `block_id` +0x30, `block_id_override`, `block_origin_override`, `block_origin` (+0x3C) "used for the overworld map chunk positioning and offsets", `chunk_position` (F32Vector4, "global map chunk coordinates") +0x80, `initial_position` (HavokPosition) +0x90 [SOURCE: FS chr_ins.rs#L134-L140, #L158-L160; offsets DERIVED]. ER header offsets +0x30..+0x3C match er-reference §3.
- Coordinate spaces: block-local, "global" world, havok (AABB broadphase, used for the effective player position), map. Block and havok both in metres; **displacements carry between them** [SOURCE: FS position.rs#L1-L14, tests]. So the practice tool's trick is a delta: new_havok = old_havok + (target_global - current_global) [SOURCE: PT position.rs#L63-L64].
- `BlockId` = area (bits 24-31), block, region, index; area 50-88 = overworld (`m60_xx_yy_00`: the "block" and "region" bytes are the tile grid x,z). `BlockId::none() = -1` [SOURCE: FS block_id.rs#L14-L26, #L46-L49]. Note: NR overworld areas appear to include more (BonfireWarpParam etc. use areaNo + gridX/Z + local posX/Y/Z) [SOURCE: PD BonfireWarpParam.xml#L122-L162].
- Discrepancy to settle by logging: fsrs calls `block_position` block-local; the practice tool treats the same field as the world-"global" position (it subtracts it between two tiles). Which is true in NR, and whether tile crossing needs a block-id change, is UNKNOWN. [SOURCE: FS chr_ins.rs#L915 vs PT position.rs#L60-L64]

Fall damage: `CSChrFallModule { fall_timer: f32, force_max_fall_height, disable_fall_motion }` [SOURCE: FS chr_ins/module/fall.rs#L11-L14]. NR param `FALL_CONTROL_PARAM` exists [SOURCE: PD tree listing "FallControlParam.xml"]. A teleport that drops the player will probably accumulate `fall_timer`. [HYPOTHESIS]

## 3. How ER tools teleport

| Tool | Method | Tag |
|---|---|---|
| ER practice tool, "load position" | Writes **physics module chunk position** (x,y,z + two angles) **and** `current_block_id` (`write_map_id`); also moves Torrent's position by the same delta; save stores global position + block id. Has a nudge-up (adds Y to avoid clipping) | [SOURCE: PT position.rs#L45-L77, #L117-L134; pointers.rs#L535-L563] ER |
| ER practice tool, "warp to grace" | **Calls a game function** (found by AOB, `lua_warp`) with two pointers read off `CSLuaEventManager` (+0x18 and +0x08) and the grace's BonfireWarpParam-style id minus 0x3E8. Cross-map capable, engine handles load and fade | [SOURCE: PT warp.rs#L18-L54; pointers.rs#L335, #L620-L622] ER |
| ERTool (C#), Tarnished Tool | Teleport/save position, "no gravity / map collision" features; described as manipulating memory directly (not read) | [SOURCE secondary: search results, nexusmods.com/eldenring/mods/9277 and 4763] not read |
| NR tools | No NR teleport/position tool, Cheat Engine table or source found. Searched: Nexus via search, vgtimes table lists, GitHub search terms. UNKNOWN | -- |
| EMEVD | NR instructions: `WarpPlayer 2003[14]` (areaId, blockId, regionId, indexId, initialAreaEntityId, popup text id; used in m10, m13, m15, m16_11, m35_90, m60_42_39_40), `WarpPlayerWithPlayerNo 2003[93]` (used in common and m12), `SetPlayerRespawnPoint 2003[23]` (**Unused** in vanilla), `WarpCharacterAndCopyFloor 2004[42]`, `...WithFadeout 2004[74]` (adds `useBonfireEffect`, `resetCamera`), `IssueShortWarpRequest 2004[41]`, `WarpCharacterAndSetFloor 2004[40]`. Destinations are entity ids (Area/Character/Object) placed in map MSB, so a new point means shipping edited MSB, or reusing existing entity ids | [SOURCE: nr.emedf.json "Used in" lists; 6PF common_func.js#L5166-L5185, L6104-L6120] |

**Co-op replication.** In ER, a ChrIns receives remote placement through `NetChrSetSync` (receive-side arrays; placement update = position in MSB space + orientation, pending-update bit) [SOURCE: FS net_chr_sync.rs#L18-L41 doc, #L83-L85, #L101-L104]; `ChrIns.net_position_synchronized` is set after a received position update [SOURCE: FS chr_ins.rs#L291-L293]. So the **owning client's local position is what gets sent**; a local write should appear on others as a jump. [HYPOTHESIS] Unknowns: whether the sender sends only when changed beyond a threshold, whether remote interpolation draws a streak, whether the other client has the destination tile loaded (a remote player far away may be unloaded or culled, which is good for separation but might make the ChrIns invisible; see `ChrLoadStatus`). No source states what SC does with teleported players. UNKNOWN.

## 4. Lists of engine points

All singleton facts from `LOG` (NR 1.3.3.0). **They are null at the title screen and in Roundtable, created at expedition load and recreated at every map load** (WorldChrMan also changes instance). Layouts: UNKNOWN for all.

| Singleton | Present in expedition | Hint for contents | Tag |
|---|---|---|---|
| `CSBirdMovePointMan` | yes (null at LOG:70, non-null after load; run4 line 68) | Name = hawk movement points. Best candidate for the hawk drop-in route/landing; flag 8800 "Bird movement icon" fits | [VERIFIED: log read] contents [HYPOTHESIS] |
| `CSRespawnPointMan` | yes | Respawn points (retry points). `GAME_SYSTEM_COMMON_PARAM.retry_point_area_radius` = default radius of MapStudio "retry point" event areas (S5 §2.2) | [VERIFIED: log] [SOURCE: PD GameSystemCommonParam.xml#L362-L364] |
| `CSMsbPointMan` | yes | Name suggests MSB point regions (event regions with positions) | [VERIFIED: log] [HYPOTHESIS] |
| `CSPlayRegionPointMan` | yes | `PLAY_REGION_PARAM`; boss/area ids, not positions per se | [HYPOTHESIS] |
| `CSGroupPlacementPointMan`, `CSWorldMapPointMan`, `CSFastTravelOverrideRegionMan`, `SmallBaseAttachPointMan`, `CSAutoInvadePoint` | yes (LOG:313-361) | Group placement points; world-map pins; small-base attach points (camps/forts) | [HYPOTHESIS] |
| `CSScenarioPlacementManager` (exe+0x4433A98) | non-null from title | Scenario placement; likely tied to `SCENARIO_PLACEMENT_PARAM` / sortie | [HYPOTHESIS] |
| `CSLuaEventMan` / `CSEventMan` | yes | Arguments of the ER warp function came from `CSLuaEventManager` (PT) | [VERIFIED: log] [HYPOTHESIS: NR equivalent] |

Param side (NR):
- **Circle**: `PLAY_AREA_CREATE_COMMON_PARAM_ST` has `initialCircleAreaNo, GridXNo, GridZNo, PosX, PosZ`, `initialCircleRadius`, `secondCircleRadius`, first/second circle start/end times [SOURCE: PD PlayAreaCreateCommonParam.xml#L11-L36; S3 §2.2]. `PLAY_AREA_CREATE_PARAM_ST` has `areaNo, gridXNo, gridZNo, posX, posZ` per candidate final-circle site [PD PlayAreaCreateParam.xml#L11-L16]. The first circle centre and radius are therefore readable without game code; whether they are in the same coordinate space as player positions: UNKNOWN (tile grid + local pos vs havok/global).
- **BonfireWarpParam**: rows have `areaNo, gridXNo, gridZNo, posX, posY, posZ` (**with Y**), `bonfireEntityId`, event flags [SOURCE: PD BonfireWarpParam.xml#L122-L162; NRP#L5256-L5276]. Whether any rows are Limveld expedition points (vs Roundtable) is UNKNOWN; check row list in Smithbox (HUMAN).
- `SORTIE_STARTING_POINT_ST` gives tile + spawn tile + two entity ids, no coordinates. The entity ids resolve to MSB parts/regions: needs MSB (Smithbox map editor) to get coordinates. [HYPOTHESIS]
- `WaypointParam` has only attributes, no positions [PD WaypointParam.xml].
- Valid-ground trick: any loaded NPC `ChrIns` has `initial_position` (spawned on valid ground) [SOURCE: FS chr_ins.rs#L159-L160, ER]. Also enemies' positions via the ChrSets.

## 5. Timing

| Signal | Use | Tag |
|---|---|---|
| `WorldChrMan` instance changes at expedition load; `main_player` / PlayerIns pointer changes (see LOG:313-355, `T-008: new WorldChrMan instance`) | "map loaded": earliest moment a PlayerIns exists | [VERIFIED: LOG] |
| Event flag 7500 (Day 1 started) is set right after `IsGameMode(2)` | "expedition loaded", probably **before** the hawk lands (no timing evidence) | [SOURCE: 6PF common.js#L821-L825] [HYPOTHESIS on ordering] |
| Physics `is_touching_ground`/`standing_on_solid_ground`, `is_falling` | Fall/land detection | [SOURCE: FS physics.rs#L29-L30, #L67-L68] ER |
| Current animation id: practice tool reads `chr modules +0x18 (time_act) -> +0x20` (anim id), time and length | "hawk animation finished" if the drop is an animation | [SOURCE: PT pointers.rs#L511-L513] ER |
| `CSFade` singleton (non-null, LOG:107) | Detect black screen; teleport during fade avoids a visible pop | [VERIFIED: singleton exists] contents UNKNOWN |
| `ChrCtrlModifierActionFlags` / `ChrDebugFlags.disabled_movement` | Detect/disable control | [SOURCE: S5 §3; FS chr_ins.rs#L525-L528 area] ER |
| Day clock `WorldAreaTime` near 07:00 / first-circle start time | Latest sensible time (circle starts around 4:30, S3) | [SOURCE: S3 §1.1] |

Whether the drop is a hawk cutscene (player not controllable, ChrIns moved by the animation) or a gameplay fall: UNKNOWN.

## 6. Ranked routes

### (a) Moving the local player

| Rank | Route | Check | Risks |
|---|---|---|---|
| **1** | **Write physics position (and `current_block_id`/block fields when the target is in another tile) on the local PlayerIns, then pulse the engine's "position changed" flags** (`chr_proxy_pos_update_requested`; `ChrCtrlChrProxyFlags.position_sync_requested`, FS chr_ins.rs#L806-L807). Use the delta method. Zero velocity/fall timer after | T-028 part 1 (solo): hotkey moves player 30 m, then 300 m across a tile; log before/after of block id, havok pos and block_position; confirm model, camera, collision follow. Part 2 (friend): other screen shows the jump, no rubber-band back, no HP/fall damage | Fall through world if target Y has no collision loaded; unloaded destination tile (physics fixes the player to nothing); fall damage (`fall_timer`); camera lag; remote copy streaks; SC or the engine snapping the player back; NR offsets differ from ER (modules +0x190 etc. may be +0x198) |
| 2 | **Call the engine's warp function** (ER practice-tool style) with an NR destination id | Needs an AOB for the NR Lua warp + its arguments from `CSLuaEventMan`; test warp to a Roundtable bonfire id first | Needs RE; destination ids are bonfire entities, none for arbitrary points; may announce to session and interact with SC |
| 3 | **Edit start rows** (`SORTIE_STARTING_POINT` / spawn entity ids) in-memory **before the drop** so the hawk lands each team on a different one of the 20 vanilla landing zones | Dump the table at runtime via the param repository (`SoloParamRepository`/`FD4ParamRepository`, LOG singleton map); watch which row is used | Params may be read before our DLL runs; edits per client must agree; landings are fixed to 20 points of unknown spacing; no EMEVD needed |
| 4 | EMEVD `WarpPlayerWithPlayerNo`/`WarpCharacterAndCopyFloor` from a modified common event, destinations as existing entity ids | Needs event files shipped (S3 §4 route) and an MSB with the entities | Heaviest; only reuses existing entity ids; synced by the game (the plus) |

### (b) Choosing N well-separated start points inside the first circle

| Rank | Route | Check | Risks |
|---|---|---|---|
| **1** | **Ring around the circle centre**: read `initialCircle*` + radius from the param row for the current pattern, place N points on a ring at ~0.6 radius, angles by team index, snapping Y by logging real ground | Log the circle centre/radius; compare with the player's position at drop-in (is the landing inside the circle?) | Coordinates may be in another space; Y unknown; points can fall on cliffs or in water |
| 2 | **Harvest valid ground from loaded NPCs**: take `initial_position` of enemies inside the circle, pick N far apart (greedy farthest-point) and shift away from the enemy | Log every ChrIns initial_position + distance to the centre in a solo run | Camps, bosses and sleeping enemies near the point; list changes while streaming |
| 3 | **Dump `CSBirdMovePointMan` / `CSRespawnPointMan` / `CSMsbPointMan` lists**, filter by circle, take the farthest apart | Pure read-only probe in T-028 (hex dump of struct + first entries) | Layout UNKNOWN; may be per tile only; probably includes non-walkable regions |
| 4 | Fixed table of positions recorded by Leonardo per map pattern (via the position probe) | Walk to the spots, log | Pattern-specific; many patterns; only for a prototype |

### (c) When to trigger

| Rank | Trigger | Check | Risks |
|---|---|---|---|
| **1** | First tick after PlayerIns exists in the new WorldChrMan **and** the player is on the ground and not in the hawk animation (landing signals in §5) and day clock is near the start | Log position, ground flags and animation id every 100 ms from load to +60 s in one solo run; mark when control returns | Landing-signal mismatch; moving during the hawk animation might break the camera |
| 2 | During the loading fade, before the drop-in plays | Probe: is PlayerIns already positioned at load? | The engine may reposition after |
| 3 | After a fixed delay from flag 7500 | Compare delay across runs | Unreliable across hardware |

For all routes, run the move on each client for its own player only and leave allies' positions alone; derive the team index from the per-player id (project rule), so both sides agree on points without messages.

## 7. Open questions
1. NR offsets: `ChrIns.modules`, physics `position`, `PlayerIns.block_position/current_block_id` (ER +0x190/+0x70/+0x6C0 -> NR?). The +8 shift is verified only for +0x68..+0x78.
2. Is `block_position` block-local or world-global in NR, and is a block-id write needed to cross tiles?
3. Is the hawk drop a cutscene or a gameplay state; what marks "landed and controllable"?
4. Which `SORTIE_STARTING_POINT` row is used for a given pattern, and is the landing spot shared by the whole party or per class (`PERSONAL_SCENARIO.spawnpointEntityId`)?
5. Coordinates of the sortie, bird-route and spawn entity ids (need MSB via Smithbox; HUMAN).
6. Layouts of `CSBirdMovePointMan`, `CSRespawnPointMan`, `CSMsbPointMan`, `CSGroupPlacementPointMan`.
7. Does SC or the engine revert or reject a local position write; does a teleport desync the other client; is a distant remote player drawn?
8. First-circle centre/radius values and coordinate space; how far the vanilla landing is from the centre.
9. Do any `BonfireWarpParam` rows describe expedition-world points, and how do they relate to the 20 sortie rows?
10. Fall damage handling on arrival (`fall_timer`, `FallControlParam`); can the player be put down exactly on ground?
