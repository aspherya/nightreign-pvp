# S5: Death and respawn (downed, revive, respawn, level loss)

Spike S5, read-only desk research, 2026-10-10. Feeds M3 (F9, F11) and M2 (F10). Nothing here was run in game. NR game 1.3.3.0.

## 0. Shorthand and pins

- `NRP:<line>` = https://github.com/vswarte/fromsoftware-rs/blob/59fbd3b3b7daaf14aca47c9f73530493dba6bc79/crates/nightreign/src/param.rs#L<line> (fsrs main at the T-005 pin, NR param bindings).
- `ERC:<line>` = https://github.com/vswarte/fromsoftware-rs/blob/59fbd3b3b7daaf14aca47c9f73530493dba6bc79/crates/eldenring/src/cs/chr_ins.rs#L<line> (**ER** layout).
- `PD:<file>` = Paramdex NR defs at `ff7245e524329bc3eab00036723d2bd53384cedf` (https://github.com/soulsmods/Paramdex/blob/ff7245e524329bc3eab00036723d2bd53384cedf/NR/Defs/<file>).
- `WIKI` = https://eldenring.wiki.gg/wiki/Near-Death_Gauge (community wiki, secondary; numbers may be patch-dependent).
- Everything labelled **ER** is a hypothesis for NR.

## 1. Vanilla NR rules (secondary sources)

| Rule | Claim | Tag |
|---|---|---|
| Downed state | Only in multiplayer. A downed player crawls, cannot attack or use items. Solo play has no near-death state: HP 0 means death at once | [SOURCE: https://www.pcgamesn.com/elden-ring-nightreign/revive; https://beebom.com/elden-ring-nightreign-death-explained/amp/] |
| Gauge | Circle of 3 uneven segments (not equal): 1 bar = 40 "Revival HP", 2 bars = 90 (+45), 3 bars = 240 (+80). A teammate revives by **attacking the downed player**; drain speed depends on weapon and attack type, not attack rating | [SOURCE: WIKI] |
| Revive result | Downed player stands with half HP (WIKI). One guide says 40% | [SOURCE: WIKI; https://eip.gg/nightreign/guides/how-to-revive-teammates] (conflict, UNKNOWN which is current) |
| Gauge regrowth | If not hit for 3.5 s, the gauge refills (2/s at 1 bar, 9/s at 2 bars, 40/s at full) | [SOURCE: WIKI] |
| Repeated downs | Each down adds a segment (harder revive). Penalty decays after 3 consecutive minutes alive and is fully reset at a Site of Grace | [SOURCE: WIKI] |
| Timer (day) | Outer ring: 15 s. Revive progress pauses it. Expiry = death | [SOURCE: WIKI; deltiasgaming/videogamer agree on about 15 s] |
| Timer (night boss / Nightlord / day-end bosses) | **No timer, no respawn.** Teammates can revive at leisure. All three downed at once = run lost | [SOURCE: WIKI; https://beebom.com/elden-ring-nightreign-death-explained/amp/] |
| Death penalty | Respawn nearby, lose **one level** (display turns red until regained) and **drop all runes** at the death point. The drop includes enough runes to regain the level. Never more than one level lost; second death before pickup loses the runes for good. Enemies may grab the drop (kill them to recover) | [SOURCE: WIKI] |
| Respawn point | Reports differ: last Site of Grace visited, or a safe spot inside the circle near teammates when outside the tide's circle | [SOURCE: beebom, sportskeeda] (conflicting, UNKNOWN exact rule) |
| Grace shortcut | A downed player who reaches a grace with no enemies nearby and no healing-disabling tide is revived at full HP and penalties reset | [SOURCE: WIKI] |
| Survival items | Wylder passive and "Wending Grace" cheat death once, including in boss fights | [SOURCE: eip.gg guide] |

Our FS-0/FS-1 PvP observation fits the day rules: downed, revive bar shown, nobody could revive, respawn near a grace with runes and one level lost [VERIFIED: FS-0 run 4 per TECHNICAL_NOTES §4].

## 2. Engine side

### 2.1 Does a downed player switch team type?

UNKNOWN. Evidence for and against:

- NR `TEAM_TYPE` has **77 "Near Death"** (and 80 "Balancer Raid") [SOURCE: Smithbox NR TEAM_TYPE.json per er-reference §4.2]. The label suggests the downed player is moved to 77, which would explain why allies' attacks "hit" them. This is [HYPOTHESIS]: no source describes the assignment.
- In the TC-004 logs the local player showed 1 (Roundtable) and 2 (expedition); no log contains a downed sample [VERIFIED: T-007a/TC-004 notes in TECHNICAL_NOTES §4]. The 5 s flip of ChrSet entry byte +9 (04→02→04) was never tied to a down. In ER that byte is `chr_update_type` (0 Local, 4 Remote) [SOURCE: er-reference §2], and byte +8 is `chr_load_status` [SOURCE: same]. A 5 s excursion is at least as consistent with an area or load transition as with a down. Treat as unexplained. [HYPOTHESIS]
- **Interaction with our hostility writes:** our mod rewrites only value 2 (to 29). If a down sets 77 on ChrIns+0x74, the hook sees 77, leaves it alone, and on revive/respawn the engine sets some value (probably 2) which we would re-rewrite to 29. If the engine instead only sets a flag, nothing changes. Either way, the observed FS-0 behaviour (no revive between 29-vs-29 players) is the baseline to explain: revive hits probably need the attacker to be "friendly" to the downed player, or the downed player's relation to be 77-based. UNKNOWN which. [HYPOTHESIS]
- There is no documentation that identifies who consumes 77. Looked at: Smithbox enum, EMEDF enum, Paramdex NpcParam/SpEffectParam descriptions (none describe it). The EMEDF `Team Type` enum in NR adds "Unknown 77" [SOURCE: T-011 line 92].

### 2.2 Params that govern near-death, revive, death (NR)

All in `GAME_SYSTEM_COMMON_PARAM_ST` (a single global param; struct starts NRP:26211) unless stated. Names come from fsrs; Paramdex at the T-005 pin has most of them as `unknown_*` or without descriptions, so **meaning is inferred from names**.

| Field | Type | Where | Likely meaning | Tag |
|---|---|---|---|---|
| `required_revive_progress_level1/2/3` | i32 | NRP:26532-26534 | Revival HP for 1/2/3 bars (wiki: 40/90/240 cumulative, steps 40/45/80 [SOURCE: WIKI]) | [HYPOTHESIS: mapping] (PD: `RequiredReviveProgress_Level1..3` at `GameSystemCommonParam.xml#L1550-L1552`) |
| `revive_rate_decay_percent_level1/2/3` | f32 | NRP:26535-26537 | Gauge regrowth rates per level (wiki 2/9/40 per s) | [HYPOTHESIS] (PD `#L1553-L1556`) |
| `near_death_grace_period_timer_default` | f32 | NRP:26498 | Outer-ring timer (wiki 15 s). Fits the day timer | [HYPOTHESIS] (PD `#L1513`) |
| `boss_arena_near_death_duration` | f32 | NRP:26497 | Near-death duration in boss arenas; semantics (0 = unlimited?) UNKNOWN | [HYPOTHESIS] (PD `#L1512`) |
| `near_death_level_decay_time` | u16 | NRP:26485 | Possibly the "3 minutes alive reduces penalty" decay (180 s). Not named in Paramdex | [HYPOTHESIS] |
| `retry_point_area_radius` | f32 | NRP:26262 | Default radius of MapStudio "retry point" event areas (PD `#L362-L364` description, translated: used when a retry-point event has no radius or region). This is the respawn-area concept | [SOURCE: PD GameSystemCommonParam.xml#L362-L364] |
| `respawn_special_effect_active_checker_sp_effect_id` | i32 | NRP:26267 | A SpEffect that notifies "revive special effect fired", used mainly for multiplayer sync | [SOURCE: PD `#L403-L405`, translated] |
| `invincible_timer_for_net_pc_init_sync`, `invincible_timer_for_net_pc` | u8 | NRP:26357-26358 | Invincibility timers for freshly synced remote player characters (max 60; second default 10; unit probably seconds, unconfirmed) | [SOURCE: PD `#L910-L921`, translated] [HYPOTHESIS: unit] |
| `retry_point_*`, `tutorial_flag_on_access_retry_point` | various | PD `#L512-L545` | Retry point UI/tutorial. Not rules | [SOURCE: PD] |
| `menu_common` `solo_play_death_to_fade_out_time`, `party_ghost_death_to_fade_out_time` | f32 | MENU_COMMON_PARAM_ST NRP:34989-34990 | Fade timing only | [SOURCE: NRP] |

Other params:

- `EQUIP_PARAM_WEAPON_ST.revive_damage` (f32) NRP:17804, PD EquipParamWeapon `#L1891`: per-weapon revive damage. `ATK_PARAM_ST.revive_damage_correction` (i32) NRP:2286, PD AtkParam `#L1404`: per-attack correction. These fit "weapon and attack type determine drain speed" (WIKI) and are the knobs for gauge tuning. [HYPOTHESIS: semantics]
- `NPC_PARAM_ST.disable_respawn` (bit) NRP:42029 (PD NpcParam `#L1017` "forbid respawn"). **NPC only**; no player equivalent found.
- `SP_EFFECT_PARAM_ST`: `no_dead` bit NRP:52870 (PD SpEffectParam `#L146`), `is_ignore_no_damage` (PD `#L195`), `dont_delete_on_dead` (NRP:53866), `is_disable_net_sync` (NRP:53886), `change_team_type` NRP:51538, per-damage-type `*_damage_cut_rate` NRP:51422-51428 (0 = immune), `soul` NRP:51474 and `soul_rate`/`have_soul_rate` NRP:51471,51476 (rune grants/modifiers). `no_dead` has the ER debug meaning "cannot die" [HYPOTHESIS for NR].
- **No param field found that holds a level-loss amount, a "lose level on death" switch, or a respawn-point rule.** Searched NRP for revive, resurrect, near_death, dying, level_down, respawn, reborn, rebirth, downed, bloodstain, death, dead, lose; Paramdex NR defs for PlayerCommonParam, MultiPlayCorrectionParam, CommonSystemParam, NetworkParam, EnemyCommonParam, CharaInitParam, HeroParam, others. The level loss is therefore **not param-driven as far as we can see** (code or EMEVD). UNKNOWN.

### 2.3 Event scripts (EMEVD)

NR EMEDF (DarkScript3) instructions that touch these topics. Indices from the scratch copy of `nr.emedf.json` (from the T-011 source, https://github.com/AinTunez/DarkScript3 `nr-common.emedf.json`) [SOURCE: that file; I did not re-download this session, the local copy is dated 2026-10-10]:

| Instr | Args | Note |
|---|---|---|
| `2003[23]` Set Player Respawn Point | Respawn Area Entity ID | Defines where the player respawns. Lever for "choose the respawn point" if we can run script or call the same function |
| `2003[109]` Steal Player Level / `2003[110]` Gain Souls From Stolen Levels | Levels / Souls Multiplier | Names suggest a level-transfer mechanic. Unknown whether the death penalty uses it. [HYPOTHESIS] |
| `2003[14]` Warp Player; `2003[93]` Warp Player With Player No; `2003[77]` Set Pseudo Multiplayer Return Position | | Placement tools |
| `2004[4]` Force Character Death | Entity ID, Should Receive Runes | Kill |
| `2004[15]` Set Character Invincibility | Entity ID, Disabled/Enabled | Spawn protection candidate. Whether it targets players: UNKNOWN |
| `2004[2]` Set Character Team Type; `4[36]` IF Character Has Team Type | | See T-011 |
| `4[0]` IF Character Dead/Alive; `4[13]` IF Player's Soul Level | | Conditions |
| `2004[47]` Bonfire-like Recovery | | Grace-like heal |

Which vanilla script (common_func etc.) implements the down/revive flow is UNKNOWN. I did not find any published decompile of NR near-death scripts. Looked at: web search (no hits), EMEDF. Shipping an EMEVD override via me3 is undocumented (TECHNICAL_NOTES A2).

### 2.4 Singletons that look relevant (from the 235-entry map)

Source: the "singleton map" section of `experiments/logs/2026-10-09_TC-004_run3_overlay.log` lines 59-295 [VERIFIED: read]. All exist in NR 1.3.3.0; instance is non-null unless noted. Purpose is guessed from names only [HYPOTHESIS].

| Name | Why it matters |
|---|---|
| `CSRespawnPointMan` | Name suggests it owns respawn/retry points. First place to look for "choose the respawn point" |
| `BloodStainMan` | Death markers / bloodstains (rune drop pile may live here or in `MapItemMan`) |
| `CSDeathBoxInsMan` | "Death box" instances. Unknown, probably death-triggered hit areas |
| `CSRebornBuddyMan`, `CSNecroBuddyMan` | Spirit/ally revival (buddy) managers. Probably not player revive, but check |
| `CSTeamSessionManager`, `CSTeamSessionDataMan` (NR-only) | Team/party state for expeditions. Candidates for "all dead" tracking |
| `CSNpcBotMan` | Bots for solo/duo fill-in |
| `CSEventFlagMan`, `CSEventMan`, `CSEmevdRepository` | Flags and scripts. Hook points for flag-based mode switches |
| `DmgMan`, `DmgHitRecordMan`, `CSBattleGroupMan` | Damage pipeline |
| `WorldChrManDbg` | ER has a flag block right after it with `player_no_dead`, `all_no_damage`, ... (see 3). It exists in NR |
| `WanderGhostMan`, `CSShareClearDataInsMan` | Ghost / shared clear data. Peripheral |
| `GameDataMan`, `GameMan` | **Not in the map** (ER finds them by RVA, not name). Player level/runes (PlayerGameData) must be found another way (CE or AOB) [VERIFIED: absent from log lines 59-295] |

## 3. ER hints for invulnerability and death flags (all **ER**, pinned commit 59fbd3b)

| Hint | Location | Use |
|---|---|---|
| `ChrIns.debug_flags: ChrDebugFlags` (u32). Bit 3 `disabled_hit` = "Makes character ignore all incoming damage" | ERC:325, ERC:525-528 | Per-character no-damage switch, the cleanest protection window |
| `ChrInsFlags1c5` (u8 at ChrIns+0x1C5 by the field order; offset not derived): bit 4 `is_invincible` ("used in lua events while loading in area"), bit 7 `death_flag` ("whether the character is dead") | ERC:226-229, ERC:440-451 | Invincible bit also a protection candidate; `death_flag` is a state to read (detect dead) |
| `ChrCtrlFlags` bit 1 `disable_hit`, bit 0 `disable_player_collision` | ERC:781-787 | Alternative; disables hits entirely (also outgoing), so likely too blunt |
| `PlayerIns.invincibility_timer_for_net_player: f32` "Set on player spawn and maybe on arena respawn? Players cannot be hurt if this is above 0" | ERC:896-898 | Native "spawn protection". Matches the NR params `invincible_timer_for_net_pc*` above. Writing a larger value is a candidate |
| `ChrCtrlModifierActionFlags` bit 5 `temporary_death_state` (set by TAE ChrActionFlag 40) | ERC:750-751 | The downed/dying animation may use this. [HYPOTHESIS] |
| `CSChrDataModule`: `hp`, `max_hp`, `recoverable_hp` | `crates/eldenring/src/cs/chr_ins/module/data.rs#L29-L40` at the pin | Read HP, force HP |
| `WorldChrManDbgFlags` (follows `WorldChrManDbg`): `player_no_dead` ("prevents death by setting HP to 1"), `all_no_damage`, `all_no_hit`, `all_no_attack` | `crates/eldenring/src/cs/world_chr_man_dbg.rs#L100-L156` at the pin | Global (not per-player) debug properties. Too coarse for a PvP window, but shows the engine's own no-dead path |
| `PlayerGameData.level: u32`, `rune_count: u32`, `rune_memory: u32` (adjacent fields) | `crates/eldenring/src/cs/player_game_data.rs#L45-L47` at the pin | Level and runes to snapshot/restore. ER offsets not derived here; NR layout must be found in CE |
| `ChrIns.chr_type` enum has `BloodstainGhost = 10` | ERC:1051 | Irrelevant to players |
| ER fsrs has **no** structures for death handling, respawn, revive, or downed state (grep for dead/death/revive/respawn/retry/rebirth over the cs/ tree) | all of `crates/eldenring/src` at the pin | The flow is not mapped anywhere public. [VERIFIED: grep of the checked-out files] |

## 4. Existing NR mods (secondary; Nexus pages returned HTTP 403 to my fetch, so only search snippets)

| Mod | What it does | How | Tag |
|---|---|---|---|
| "Easier Revives - Normalized Values and No Regeneration" (NR Nexus mods/334) | Makes the revive-cost increase linear (40/80/120) and turns off gauge regrowth | Almost certainly param edits (`required_revive_progress_*`, `revive_rate_decay_percent_*`) in a modified regulation. Method is **not stated** in what I could read | [SOURCE: search snippet of https://www.nexusmods.com/eldenringnightreign/mods/334] [HYPOTHESIS: method] |
| "Scaling for Duos" (NR mods/40, deprecated) | Mentions lowering the 2nd-down revive damage from 90 to 80 | Param edit | [SOURCE: snippet of https://www.nexusmods.com/eldenringnightreign/mods/40] |
| "Boss Arena" (NR mods/449) | Changelog mentions custom defeat handling, a bug in the "infinite revives" cheat being applied in boss rush, and quitting while dying makes you die again at the Roundtable | Unknown (probably EMEVD+params). Evidence that some mods already change defeat flow | [SOURCE: snippet of https://www.nexusmods.com/eldenringnightreign/mods/449] |
| "Nightreign Revive" (an **ER** mod using ER SC) | Brings NR-style revive to ER | Needs an early-load bootstrap DLL plus a main DLL (see T-003) | [SOURCE: https://www.nexusmods.com/eldenring/mods/11175] |
| "Coop Revival" (**ER**) | Alpha-stage revive allies like NR | Unknown | [SOURCE: https://www.nexusmods.com/eldenring/mods/8131 snippet] |

No NR mod found that removes the level loss or changes the respawn point (searched: Nexus via search engine, "no level loss", "death penalty", "respawn"). UNKNOWN whether one exists; the Nexus NR section was not browsable. The ER "Disable Rune Loss On Death" exists but is ER and runes only [SOURCE: https://www.pcgamesn.com/elden-ring/mods-best].

## 5. Ranked routes

Common context: all in-game checks need Nucleus (2-4 instances) or an FS; a solo check is noted where possible. Each route's verification steps are numbered V1...

### 5.1 Ally-only revive (F10)

1. **Keep teammates friendly, rivals hostile (the M2 hostility design) and let the vanilla gauge do the rest.** Reason: vanilla revive is "ally attacks the downed player"; FS-0 showed no revive between two 29-vs-29 players. If the revive check is a friend-relation check, any design where teammates are non-hostile but non-ally-blocking gets revive free. [HYPOTHESIS]
   - V1 (log): while a player is downed in Nucleus, log ChrIns+0x74 and ChrSet entry bytes +8/+9 every 100 ms from every client. Answers 2.1 (is it 77?).
   - V2 (Nucleus, 3 instances: A, B teammates, C rival): down A; B attacks A (does the gauge drain, does A stand?), then C attacks A (must not). Repeat with the down caused by C.
   - V3: with identical team value for A and B (2 vs 2, no 29), does B's attack even register as a hit on A? If hits are blocked outright, revive may rely on a special path and a separate "revive hit" must be allowed.
2. If the team-type route fails: use the params as the lever (`revive_damage`, `required_revive_progress_*`) only to tune; they do not select who may revive.
3. Fallback, DLL-driven: on a rival's hit on a downed player, nothing special; on an ally's hit, write the revive ourselves (set HP to half, clear down state). Needs the down-state fields: UNKNOWN. Last resort.

### 5.2 Keep the level (F9)

1. **Snapshot level and runes at the down, restore after the respawn (DLL write to `PlayerGameData`).** Needs: locate NR `PlayerGameData` (not via singleton; ER fsrs `level`/`rune_count` adjacent at player_game_data.rs#L45-L47), detect respawn (HP/`death_flag` transition, position jump). Rune drop: also restore or leave dropped (our rules: F8 drops items; runes are a separate decision).
   - V1 (solo, CE): find level and rune count; die to an enemy in a Nucleus 2-instance expedition (so a down exists); observe the value changes and the order (level at death or at respawn?).
   - V2: write level back after respawn; check the HUD (red level display clears?), stats (HP/FP/stamina scale), that the level-up menu shows the right cost, and that SC does not revert it.
   - V3: does the change survive area change and sync to the other client (level shows on the other screen)?
2. SpEffect with `soul` / EMEVD `Gain Souls From Stolen Levels`: unproven; the penalty may not use these. Needs reading the vanilla script. Lower priority.
3. Param: no field found (see 2.2). Not available.

### 5.3 Choose the respawn point (F9)

1. **Teleport after respawn (DLL)**: wait for the respawn, then move the player to a computed safe location. Needs: a position write (ER fsrs has a module path, not read in this spike) or a call to the engine warp. V1: measure what the engine does in the first seconds after respawn (position, fade, flags) and whether a teleport desyncs the remote copy. V2: ten repeats (M3 acceptance).
2. **Steer the engine's own respawn point**: `CSRespawnPointMan` singleton (inspect in CE: points list, current selection) or EMEVD `2003[23] Set Player Respawn Point`. V1: dump `CSRespawnPointMan` before and after touching a grace; compare. V2: write the chosen point, die, check. Ranks above 1 if the structure is writable, because the engine then handles the fade and sync.
3. EMEVD / MSB retry points: needs shipping scripts or map data via me3 (undocumented). Last.

### 5.4 Spawn protection that ends on attack (F9)

1. **Per-character no-damage flag (ER `ChrDebugFlags.disabled_hit`, bit 3 of u32 at `debug_flags`)** set for N seconds after respawn, cleared when the player attacks. Pair with `invincibility_timer_for_net_player` (float; protects remote hits by the engine's own rule). Reason: no regulation edit, so no SC conflict.
   - V1 (solo + Nucleus): set the flag on the local player after respawn; get hit by an enemy and by the other instance; does the victim's client decide the hit (H1 damage authority)? Protection must be applied on the **victim's client**, and ideally the attacker's too (so the hit doesn't show).
   - V2: find the NR offsets of `debug_flags`, `chr_flags1c5` and `invincibility_timer_for_net_player` (ER layout is a hint only; NR ChrIns is shifted +8 in the header per TECHNICAL_NOTES §4).
   - V3: attack detection for the "ends on attack" rule. Not researched: UNKNOWN. Candidates are an animation/state id, a stamina drop, or `IF Player Attacking`-style state. Needs a dump of attack state.
2. SpEffect applied by the DLL (damage cut 0, or `no_dead`, or `is_ignore_no_damage`): needs a custom row in regulation (A1 risk: SC `YuiNexusRegulationManager`). Alternative: reuse an existing vanilla invulnerability SpEffect id; none identified. UNKNOWN.
3. EMEVD `2004[15] Set Character Invincibility`: only if EMEVD can ship; unknown for players.

### 5.5 Disable respawn (F11, sudden death)

1. **Reuse the vanilla "no respawn" mode** (day-end bosses and the Nightlord have no near-death timer and no respawn [SOURCE: WIKI]). Find what switches it (a flag, a state in `CSTeamSessionManager`, or a param read like `boss_arena_near_death_duration`) and set it for everyone at sudden death. Cheapest, because the engine handles the UI and consequences (everyone downed = run ends, which is exactly an elimination test if the mode is per team).
   - V1: in a real boss fight, diff event flags and the two team-session singletons against a field state (CE/dump). Cheap and solo-checkable.
   - V2: in Nucleus, apply the switch in the field and let a player die: no respawn? downed forever?
2. **Kill loop**: after every respawn, force the player to dead again, or mark them eliminated and put them in a spectator-like state (camera, no collision). Crude, needs HP write, may flash UI. 
3. EMEVD `2004[4] Force Character Death`: needs script shipping.
4. Ending state: all players of a team downed means a run end in vanilla. Our own state machine (PROJECT_PLAN section B) should decide eliminations, not the engine's "run lost" screen. Check what the engine does when all downed players are on rival teams (it may treat it as a wipe, which is a hazard for 2 teams).

## 6. Open questions

1. Is a downed player set to team 77, or is the down only a flag? (log V1 in 5.1)
2. What exactly feeds the gauge: attack events from "friendly" attackers? Why did 29-vs-29 fail? (V3 in 5.1)
3. Where does the level loss happen (at death, at respawn, a script, or code)? No param found.
4. NR `PlayerGameData` location and layout (no singleton; GameDataMan is not in the map).
5. What vanilla switch disables near-death timer and respawn in boss fights? Is it per player, per team, or global?
6. Respawn rule: last grace vs safe spot inside the circle (sources conflict); what `CSRespawnPointMan` holds.
7. Does the DLL-set invulnerability bit hold when SC syncs or reverts character state? (SC reverts some runtime edits in ER, TECHNICAL_NOTES §3.)
8. Unit of `invincible_timer_for_net_pc` and what `near_death_level_decay_time` does.
9. The 04→02→04 flip of ChrSet byte +9 (5 s): downed or area load? Log it against a known down.
10. Nexus mod pages were not fetchable (403); how "Easier Revives" and "Boss Arena" implement their changes is unconfirmed. A human could open the pages (permissions/requirements/file list).
