# T-011: Param and EMEVD routes to hostility (backup path for H1)

Researcher: Sonnet | Date: 2026-10-09 | Status: review
Scope: NR only unless a line is labelled ER. ER information is a hypothesis for NR. Read-only. Nothing was
launched, patched or built. No modded regulation.bin was made.

Tag legend as in TECHNICAL_NOTES.md. Shorthand used below:
- `fsrs-param` = https://raw.githubusercontent.com/vswarte/fromsoftware-rs/main/crates/nightreign/src/param.rs
  (downloaded 2026-10-09, 59,023 lines). Line numbers refer to that download.
- `PD` = https://github.com/soulsmods/Paramdex (master; last commit 2026-03-06 is a README edit; the last NR
  `Defs` commit is 2025-07-17).
- `SBX` = https://github.com/vawser/Smithbox (main).
- `DS3` = https://github.com/AinTunez/DarkScript3 (master).

---

## 0. Bottom line for the planner

1. **A param route to "change a player's team type" exists on paper in NR:** `SP_EFFECT_PARAM_ST.changeTeamType`
   (s8). A SpEffect carrying it is documented in ER as "overwrite with the specified team type". Whether the NR
   engine honours it on players is **UNKNOWN**.
2. **An EMEVD route exists in NR's EMEDF:** instruction `2004[2] SetCharacterTeamType(uint entityId, byte teamType)`,
   plus the condition `4[36] IfCharacterHasTeamType`. Vanilla NR uses `SetCharacterTeamType` in `common_func`,
   `m10_00_00_00`, `m10_00_00_00_dlc01`, `m13_00_00_00` and `m18_00_00_00`. Whether it can target a **player**
   entity is **UNKNOWN**.
3. **`isHitBothTeam` is on `BULLET_PARAM_ST` (Bullet), not `ATK_PARAM_ST`.** It only affects projectiles and
   spells. A weapon melee route would use AtkParam's `opposeTarget` / `friendlyTarget` / `selfTarget` flags
   instead. Both are real fields in NR. Neither is proven to affect player-vs-player.
4. **All three tools support NR in current releases** (Smithbox 2.2.6, WitchyBND 3.0.1.0, DarkScript3 3.6.3),
   with caveats in section 3.
5. **me3 can ship a modified `regulation.bin` for NR** (official docs show exactly this). EMEVD is plausible but
   undocumented. Both depend on the game's files being replaced on **all** clients identically.
6. **SC risk:** `nrsc.dll` contains the string `YuiNexusRegulationManager`. Its purpose is UNKNOWN, but it
   could interact with a modded regulation (section 5).
7. **Param/EMEVD are static routes.** They cannot express "hostile only to players on the other gameplay
   team" without something that applies a SpEffect or event per player at runtime. Backup path A1/A2 therefore
   still needs a trigger. Section 6 covers this.

---

## 1. Param field semantics (NR), compared with ER

### 1.1 `changeTeamType` (SpEffectParam)

| Item | Finding | Tag |
|---|---|---|
| NR field exists: `SP_EFFECT_PARAM_ST.change_team_type: i8` | Struct at line 51414, field at line 51538, accessor `change_team_type()` / `set_change_team_type()` at lines 53365-53371 | [SOURCE: fsrs-param#L51538] |
| NR Paramdex def | `<Field Def="s8 changeTeamType" />`. In `NR/Defs/SpEffectParam.xml` the field has **no** DisplayName, Enum, description or default | [SOURCE: https://raw.githubusercontent.com/soulsmods/Paramdex/master/NR/Defs/SpEffectParam.xml#L196] |
| ER Paramdef (labelled ER) | `s8 changeTeamType = -1`, Enum `SP_EFFECT_CHANGE_TEAM_TYPE`, range -1..99, description (Japanese) "指定したチームタイプに上書きする" = "overwrite with the specified team type" | [SOURCE: https://raw.githubusercontent.com/soulsmods/Paramdex/master/ER/Defs/SpEffect.xml#L1341] (ER) |
| NR Smithbox has the same enum `SP_EFFECT_CHANGE_TEAM_TYPE` | Values are identical to the ER one (0 None, 1 Live, 2 White Ghost ... 19 Host, 20 Co-op, 21 Hostile, 29 Indiscriminate, 47 Spirit Summon ...). It looks copied from ER rather than NR-derived | [SOURCE: https://raw.githubusercontent.com/vawser/Smithbox/main/Smithbox.Release/Output/Assets/PARAM/NR/Param%20Enums/SP_EFFECT_CHANGE_TEAM_TYPE.json] |
| **Is the default -1 ("no change") in NR?** | The NR def has no default, so the NR param row default may be 0. **A row written by hand must set -1 explicitly** unless the base row says otherwise | UNKNOWN (inspect real rows in Smithbox) |
| Does the NR engine apply `changeTeamType` to **players**, in multiplayer, on all peers? | No source found | UNKNOWN. Looked at: Paramdex, Smithbox annotations (the NR English `SP_EFFECT_PARAM_ST.json` has no entry for `changeTeamType`), web search (no hits) |
| How vanilla NR uses it (which rows set it, and to what) | Not checked: needs the real `regulation.bin` opened in Smithbox (HUMAN, read-only). Filter SpEffectParam on `changeTeamType != -1/0` | UNKNOWN |

Related, not team-changing: `team_offense_effectivity: i8` (line 51560) = ER description "overwrite the target's
team attack effectivity". It is an AI attack-token value, not hostility. [SOURCE: ER def
https://raw.githubusercontent.com/soulsmods/Paramdex/master/ER/Defs/SpEffect.xml#L1494] (ER).
`NPC_THINK_PARAM_ST.team_attack_effectivity` (line 43487) is the same family: "how many team members can attack at
once", AI coordination only [SOURCE: https://raw.githubusercontent.com/soulsmods/Paramdex/master/NR/Defs/NpcThinkParam.xml#L240].

SpEffect target filters, which also matter for any SpEffect-based design (NR def):
`effectTargetSelf/Friend/Enemy/Player/AI/Live/Ghost`, `effectTargetOpposeTarget`, `effectTargetFriendlyTarget`,
`effectTargetSelfTarget`, `effectTargetAttacker` [SOURCE: https://raw.githubusercontent.com/soulsmods/Paramdex/master/NR/Defs/SpEffectParam.xml#L121-L130 and #L189;
English labels: https://raw.githubusercontent.com/vawser/Smithbox/main/src/Smithbox.Data/Assets/PARAM/NR/Param%20Annotations/English/SP_EFFECT_PARAM_ST.json].

### 1.2 `isHitBothTeam` (Bullet) and the AtkParam target flags

| Item | Finding | Tag |
|---|---|---|
| `isHitBothTeam` lives in **`BULLET_PARAM_ST`**, bit 1 of byte `bits_9b` (bit 0 is `is_penetrate_map`) | Struct at line 6596; accessors `is_hit_both_team()` / `set_is_hit_both_team()` at lines 7203-7210 | [SOURCE: fsrs-param#L7203] |
| NR Paramdex def | `u8 isHitBothTeam:1`, DisplayName "敵味方共にあたる？", description (Japanese) = "Does it hit both friend and foe? (does not hit wandering ghosts)" | [SOURCE: https://raw.githubusercontent.com/soulsmods/Paramdex/master/NR/Defs/Bullet.xml#L434] |
| Smithbox English annotation | "Are you an enemy or an ally? (Not a wandering ghost)" (a rough machine translation of the same text) | [SOURCE: https://raw.githubusercontent.com/vawser/Smithbox/main/src/Smithbox.Data/Assets/PARAM/NR/Param%20Annotations/English/BULLET_PARAM_ST.json] |
| ER equivalent | The ER Paramdex has no `Bullet.xml` under that name (HTTP 404). `isHitBothTeam` is not present in the ER defs I fetched, so I cannot compare it | UNKNOWN (ER file name not located) |
| **AtkParam does not have `isHitBothTeam`** | `ATK_PARAM_ST` (line 2094) has no such field. The bit appears only in `BULLET_PARAM_ST` | [VERIFIED: grep of the downloaded fsrs-param, one hit at line 7203 inside Bullet] |
| AtkParam target flags (NR and ER, identical) | `opposeTarget:1 = 1` ("Target: hostile"), `friendlyTarget:1` ("Target: allies"), `selfTarget:1` ("Target: self"). Their Japanese descriptions are only the labels | [SOURCE: https://raw.githubusercontent.com/soulsmods/Paramdex/master/NR/Defs/AtkParam.xml#L480-L497; English: https://raw.githubusercontent.com/vawser/Smithbox/main/src/Smithbox.Data/Assets/PARAM/NR/Param%20Annotations/English/ATK_PARAM_ST.json] |
| `isGhostAtk:1` | "Used for spirit-body (ghost) damage judgement" | [SOURCE: https://raw.githubusercontent.com/soulsmods/Paramdex/master/NR/Defs/AtkParam.xml#L452] |
| `NPC_PARAM_ST.isCalculatePvPDamage:1` | "Decides whether damage is calculated as 'player' ... if disabled, treated as 'enemy'" | [SOURCE: https://raw.githubusercontent.com/soulsmods/Paramdex/master/NR/Defs/NpcParam.xml#L1268] |
| `ATK_PARAM_ST.dmg_level_vs_player: i8`, `EQUIP_PARAM_WEAPON_ST.vs_player_dmg_correct_rate_*` | PvP damage scaling fields exist in NR | [SOURCE: fsrs-param#L2263 and #L17756-L17763] |
| Does `friendlyTarget=1` on a player's attack make it hit a same-team player? | The semantics read that way, but it is not documented anywhere I found. **The hit-validation code path for players (and whether a "friend" check exists for player-vs-player at all) is unknown** | [HYPOTHESIS] |

**Correction to TECHNICAL_NOTES.md §4 line 115:** the row groups `is_hit_both_team` with the
`ATK_PARAM_ST` / `EQUIP_PARAM_WEAPON_ST` fields. It is a **Bullet** flag. The task-card text also says
"AtkParam (e.g. isHitBothTeam)". Opus should note that when merging.

### 1.3 `teamType` (NpcParam) and the NR enum

| Item | Finding | Tag |
|---|---|---|
| Field: `NPC_PARAM_ST.team_type: u8` (line 40619). Description (Japanese): "the NPC's attacks hit / do not hit, targeting / not targeting setting". Enum `TEAM_TYPE` | Same in ER | [SOURCE: fsrs-param#L40619; https://raw.githubusercontent.com/soulsmods/Paramdex/master/NR/Defs/NpcParam.xml#L735] |
| Paramdex ships **no** `TEAM_TYPE` enum for NR or ER (`Tdfs` folder has no TEAM_TYPE) | Enum names come from Smithbox | [SOURCE: https://api.github.com/repos/soulsmods/Paramdex/contents/ER/Tdfs (listing); NR has no Tdfs folder] |
| Smithbox NR `TEAM_TYPE` enum (commit dates 2026-07-24/26) | See table below. It is an ER-derived list with extras at the end. NR-specific entries look like **77 "Near Death"** and **80 "Balancer Raid"**. Entries 17-22 and 34-56 look inherited from other games (Farron/Aldrich) and are probably not NR-valid | [SOURCE: https://raw.githubusercontent.com/vawser/Smithbox/main/Smithbox.Release/Output/Assets/PARAM/NR/Param%20Enums/TEAM_TYPE.json] |
| DarkScript3 NR EMEDF `Team Type` enum | Same ER list, with additional **"Unknown 67 / 70 / 77 / 79"** compared with the ER EMEDF (which has 67 and 70 only) | [SOURCE: https://raw.githubusercontent.com/AinTunez/DarkScript3/master/DarkScript3/Resources/nr-common.emedf.json (enum "Team Type")] |
| **Which value is hostile to a player is not in any param or EMEDF doc.** It is encoded in a team-relation table inside the exe (T-005 territory) | | UNKNOWN. Looked at: Paramdex, Smithbox meta, EMEDF html |

Common core values (Smithbox NR `TEAM_TYPE` vs EMEDF NR `Team Type`; both ER-derived; labels differ for some):

| Value | Smithbox NR TEAM_TYPE | EMEDF NR Team Type |
|---|---|---|
| -1 | No Change | Default |
| 0 | None | Disabled |
| 1 | Player | Human |
| 2 / 3 / 4 / 5 | White / Black / Grey / Wandering Ghost | White / Black / Gray / Wandering Phantom |
| 6 | Enemy | Enemy |
| 7 | Boss | Strong Enemy |
| 8 / 9 / 12 | Ally / Hostile Ally / Battle Ally | same |
| 13 | Invader | Invader |
| 14 | Neutral | (absent) |
| 15 | Charmed | Charmed |
| 16 | Invader | (absent) |
| 19 | "Invader - Giant Seed" | **Host** |
| 20 | "Comatose Sleep" | **Co-op** |
| 21 | "Watchdogs of Farron - Giant Seed" | **Hostile** |
| 29 | Indiscriminate | Indiscriminate |
| 30 | Object | Object |
| 31 / 32 | Co-op Mad Phantom / Invader Mad Phantom | White / Red Berserker |
| 47 | Spirit Summon | (absent) |
| 77 | Near Death | Unknown 77 |
| 80 | Balancer Raid | (absent) |

So the two NR sources **disagree on 16-22** (the Smithbox NR TEAM_TYPE enum is inconsistent with its own
`SP_EFFECT_CHANGE_TEAM_TYPE` enum). EMEDF 19/20/21 = Host/Co-op/Hostile matches the Smithbox SpEffect enum and
the ER layout. The set most plausible for "opposed players" in an ER-derived engine is
{1 Player/Human, 13 Invader, 19 Host, 20 Co-op, 21 Hostile, 29 Indiscriminate}. **Do not use any of them
without observing it in-game (T-007/T-012).** [HYPOTHESIS: the ER host/invader pairing carries over to NR].

### 1.4 Vestigial invasion fields still present in NR params (supporting evidence for H1, not proof)

NR `GameSystemCommonParam` still defines `enemyDetectionSpEffect_BulletId_byInvadeOrb_None`,
`..._byCoopRing_RedHunter`, `autoInvadePoint_generateDist/cancelDist` [SOURCE:
https://raw.githubusercontent.com/soulsmods/Paramdex/master/NR/Defs/GameSystemCommonParam.xml#L495-L503, #L852-L859].
`NetworkParam` still has versus/wander-ghost fields (`updateVersusGhostIntervalTime`, `distanceOfBeginRecordVersus`,
line 763-803) [SOURCE: https://raw.githubusercontent.com/soulsmods/Paramdex/master/NR/Defs/NetworkParam.xml#L763].
`MultiplayerState` in the NR EMEDF still lists `Invasion = 5` and `InvasionPending = 6`, with no "used in" annotation
(unused by vanilla NR scripts) [SOURCE: https://soulsmods.github.io/emedf/nr-emedf.html, enum MultiplayerState].
Field presence does not show the code is live. It only shows the paramdef inherited the invasion layout.
NR also has `SmallbaseInvationNpcParam`, which is PvE NPC "invasions" of small bases, not player invasion
[SOURCE: https://raw.githubusercontent.com/vawser/Smithbox/main/src/Smithbox.Data/Assets/PARAM/NR/Param%20Meta/SmallbaseInvationNpcParam.xml (exists); purpose HYPOTHESIS from name].

---

## 2. EMEVD (NR)

| Item | Finding | Tag |
|---|---|---|
| **`SetCharacterTeamType` exists in NR** | Instruction `2004[02]`: `SetCharacterTeamType(uint entityId, byte<TeamType> teamType)` | [SOURCE: https://soulsmods.github.io/emedf/nr-emedf.html#SetCharacterTeamType; https://raw.githubusercontent.com/AinTunez/DarkScript3/master/DarkScript3/Resources/nr-common.emedf.json (class 2004, index 2)] |
| Used by vanilla NR in | `common_func`, `m10_00_00_00`, `m10_00_00_00_dlc01`, `m13_00_00_00`, `m18_00_00_00` | [SOURCE: soulsmods emedf page, same anchor] |
| Condition counterpart | `4[36] IfCharacterHasTeamType(group, entityId, teamType)` and the skip/goto/end variants via the condition function `CharacterHasTeamType` | [SOURCE: soulsmods emedf page, `IfCharacterHasTeamType` anchor] |
| Other relevant instructions in NR | `2004[8] SetSpEffect(entityId, spEffectId)`, `2004[21] ClearSpEffect`, `4[5] IfCharacterHasSpEffect`, `2004[28] SetNetworkUpdateAuthority(entityId, authorityLevel 0 Normal / 8192 Forced)`, `4[31] IfCharacterInvadeType`, `2009[8] SendInvadingPhantomsHome`, `2009[11] SendAllPhantomsHome` | [SOURCE: nr-common.emedf.json] |
| ER also has `SetCharacterTeamType` at the same index (labelled ER); the NR instruction list is a superset (478 vs 362 instructions) | | [VERIFIED: parsed both EMEDF json files from DarkScript3 master] |
| **Can it target players?** The `entityId` is a generic `uint`, with no documented restriction. How a player is addressed in NR event scripts (a fixed entity id like ER's 10000, or an entity group) is not in the EMEDF docs | | UNKNOWN. Check by decompiling NR `common_func` / `m10_00_00_00` with DarkScript3 and reading how the 5 vanilla callers pass the entity |
| Does EMEVD run on each peer, or host-authoritative, in SC sessions? | Not documented | UNKNOWN. SC's RTTI includes `EcTestTeamType` (an event-condition class), so SC is aware of team-type checks (TECHNICAL_NOTES §3, [VERIFIED: RTTI]) |
| MSB event type `TeamFight` exists in NR MSB meta: fields `TargetEntityGroupID` ("Enemy Entity Group ID"), `LeaderEntityID`, 3 item lots | Looks like an AI group-fight feature, not player hostility | [SOURCE: https://raw.githubusercontent.com/vawser/Smithbox/main/Smithbox.Release/Output/Assets/MSB/NR/Meta/Event/TeamFight.xml]. Purpose [HYPOTHESIS] |

---

## 3. Tool support status (as of 2026-10-09)

| Tool | Latest | NR support | Caveats | Tag |
|---|---|---|---|---|
| **Smithbox** | 2.2.6, 2026-09-20 | Listed as supported (README: "Elden Ring: Nightreign" among supported games). Param, text, map, model, havok editors cover NR. 2.2.0 (2026-06-04): "Added support for NR 1.03.5 regulation to upgrader". 2.2.4: "Improved the Nightreign meta and row names". NR param defs/enums last touched 2026-07-24/26 | **No EMEVD editor** in its feature list. Param file reading needs no pre-unpacking ("You no longer need to unpack a game"). README says .NET 7 Desktop Runtime (this PC has 6, 8, 9, 10 WindowsDesktop; 7 absent. Check the actual release notes before installing) | [SOURCE: https://github.com/vawser/Smithbox/releases ; https://raw.githubusercontent.com/vawser/Smithbox/main/README.md] |
| **WitchyBND** | 3.0.1.0, 2026-07-06 | "Basic initial support for Nightreign" added in 2.15.0.0 (2025-05-30); TAE reading in 2.15.1.0. README format list does not name NR | I found **no changelog line** saying that NR `regulation.bin` 1.03.x unpack/repack works, nor any NR-specific param fix. Needs a test on a *copy* of regulation.bin | [SOURCE: https://github.com/ividyon/WitchyBND/releases ; https://raw.githubusercontent.com/ividyon/WitchyBND/main/README.md]. Regulation support UNKNOWN |
| **DarkScript3** | 3.6.3, 2026-03-08 | NR since 3.6 (2025-07-21). 3.6.1 = NR 1.02 instructions; 3.6.2 (2025-12-11) = "Update for Nightreign DLC", NR 1.03 instructions "with a few unknowns"; 3.6.3 = more NR DLC fixes and map names | Event 90015446 decompiles alternately; some instructions still unknown. Needs .NET 6 Desktop and ASP.NET Runtimes (this PC has both, [VERIFIED: `dotnet --list-runtimes`]) | [SOURCE: https://github.com/AinTunez/DarkScript3/releases] |
| **Paramdex** | NR defs last changed 2025-07-17 | NR paramdefs exist (226 files) | Behind Smithbox's own bundled defs (updated 2026-07-26) and behind the DLC. **Layouts for the current regulation may differ**; verify field offsets against fsrs-param, which tracks the paramdef XMLs | [SOURCE: https://api.github.com/repos/soulsmods/Paramdex/commits?path=NR/Defs] |

**Version gap to flag:** our pins say exe FileVersion 1.3.3.0 and a `regulation.bin` dated 2026-04-01
(TECHNICAL_NOTES §1). The newest regulation Smithbox names is 1.03.5. Bandai Namco's own notes say regulation
1.03.3 shipped with app hotfix 1.03.1 (secondary reading: the notes are only found through a search summary;
original: https://en.bandainamcoent.eu/elden-ring/news/elden-ring-nightreign-hotfix-notes-version-1031).
The mapping "exe FileVersion 1.3.3.0 ↔ regulation version" is **UNKNOWN**. The regulation version is shown on the
title screen in-game (HUMAN check) and is also readable by Smithbox/Witchy. Record it as a pin.

How to get EMEVD/params off disk: Smithbox's File Browser extracts from the game archives [SOURCE: Smithbox
README "File Browser"]. The game folder has loose `regulation.bin` and **no loose `event/` folder**, so event
files are inside `data*.bdt` / `dlc01.bdt` archives [VERIFIED: directory listing of
`Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game`: `regulation.bin` 1,974,720 bytes, `data0-3.bdt`, `dlc01.bdt`,
no `event` dir].

---

## 4. me3: loading a modified regulation.bin / EMEVD for NR

| Item | Finding | Tag |
|---|---|---|
| Official profile guide for **Nightreign** says "Place asset files (e.g. `regulation.bin`, `parts/` folders) in `mod`" and uses `[[supports]] game = "nightreign"`, `[[packages]] id/path`, `[[natives]] path`. Its example installs the Nexus mod *Fun Is Allowed*, which ships a `regulation.bin` | Packages are loose-file overrides; paths are relative to the `.me3` file | [SOURCE: https://raw.githubusercontent.com/garyttierney/me3/main/docs/user-guide/creating-mod-profiles.md] |
| Disk-to-disk overrides "like regulation.bin" were added to the override mapping | | [SOURCE: me3 CHANGELOG commit 105ce74, https://raw.githubusercontent.com/garyttierney/me3/main/CHANGELOG.md#L1716] |
| NR file overrides cover all non-Wwise files (PR #111) | A path inside a package that matches a game-archive path replaces it. This is the mechanism an `event/*.emevd.dcx` override would use | [SOURCE: CHANGELOG#L1334-L1345]. EMEVD specifically: **not documented** → [HYPOTHESIS: works like any other archive file] |
| Package-level keys `id`, `path`, and (per the schema) load order options | Full schema lives in `schemas/mod-profile.md`, which I did not fetch | UNKNOWN: read the schema before writing the profile (T-001 already did this for natives) |
| me3 versions | Pinned 0.11.0 (2026-02-28). Repo CHANGELOG top entry is **v0.13.0 (2026-08-11)**; there are newer releases than our pin | [SOURCE: CHANGELOG#L7 and #L227]. Note for Opus: pins in TECHNICAL_NOTES §1 are not the newest |
| Multiplayer consequence | A modded regulation must be **identical on both PCs** (the engine syncs by param data on each peer). Exact SC/game check on regulation mismatch is not documented | [HYPOTHESIS]; UNKNOWN |
| me3 does not need a launcher flag to enable online (`start_online` default off; "not needed for Seamless Co-op") | | [SOURCE: https://raw.githubusercontent.com/garyttierney/me3/main/docs/configuration-reference.md] |

---

## 5. Seamless Co-op interaction risks for the param/EMEVD path

- `nrsc.dll` contains the plain string **`YuiNexusRegulationManager`**, alongside `YuiNexusNetRankingDB` and
  `MODMESSAGE_YUINEXUS*` messages [VERIFIED: plain string scan of `...\SeamlessCoop\nrsc.dll` on this machine, read-only]. The class is
  not described anywhere. [HYPOTHESIS: SC applies its `[SCALING]` settings (`health_scaling`, `damage_scaling`,
  `posture_scaling` in `nrsc_settings.ini`, [VERIFIED: ini]) by editing params at runtime. If so, a regulation
  mod could be overwritten or could conflict. Possibly also a regulation version/hash gate in the lobby]. This is
  the single biggest unknown for the param route.
- No strings matching `emevd`, `paramdef`, `teamtype`, `hostile`, `pvp`, `invad` were found in `nrsc.dll` in a
  plain ASCII/UTF-16 scan (only RTTI `EcTestTeamType` and `YuiNexusRegulationManager`) [VERIFIED].
- ER SC compatibility with regulation mods is outside this card's evidence (not researched).

---

## 6. What the backup routes can and cannot do

| Route | Mechanism | Can express "hostile only across gameplay teams"? | Blockers / unknowns |
|---|---|---|---|
| **A1a: SpEffect with `changeTeamType`** | Row in SpEffectParam, applied to the player by an item/gear effect, or by EMEVD `SetSpEffect` | Only if something applies the right effect per player at runtime (our DLL or event). The param alone cannot assign teams | Is the field honoured on players? Do remote peers see the change? Does SC reset it? All UNKNOWN. Ships a modified `regulation.bin` identical on both PCs |
| **A1b: `friendlyTarget` / `opposeTarget` on AtkParam, `isHitBothTeam` on Bullet** | Edit weapon/skill attack rows | These make *attacks* hit more target classes. They are global to the attack, not per player pair. Likely to hit enemy NPCs' allies too, and teammates | No evidence a player-vs-player friend check exists to be bypassed. Sync model unknown. Many rows (all weapons/skills/spells) to edit |
| **A2: EMEVD `SetCharacterTeamType`** | Modified `common`/map `.emevd.dcx` event | Event code can branch per player, but the event runs in the *game's* event system (peer behaviour unknown) | Targeting a player entity is UNKNOWN. EMEVD packaging through me3 is [HYPOTHESIS]. DarkScript3 NR works |
| **H1 (current plan): write `team_type` from our DLL** | Direct memory write | Yes, full control | Subject of T-007/T-008/T-012 |

Practical reading: params/EMEVD are best treated as a way to make **our DLL's team-type choice stick** or to
**mirror it on remote peers** if H1 shows a reset problem, not as a replacement for per-player assignment. If T-012
shows the engine resets `team_type` every tick (or on an event), the SpEffect route (a permanent buff carrying
`changeTeamType`) is the natural counter, because it is the engine's own supported override path. [HYPOTHESIS]

---

## 7. Cheap follow-up checks (all read-only; none done here)

1. **HUMAN, 15 min:** install Smithbox, open the project at the game folder (read-only), then in NR SpEffectParam
   sort on `changeTeamType`. List which rows carry a value and the values. Also check NpcParam `teamType`
   distribution (which values vanilla NR uses for normal enemies, bosses, night bosses). Gives real NR numbers
   for T-007's comparison. (Smithbox can mutate a project; use "Open project" with a scratch project folder, do not
   save over the game.)
2. **HUMAN:** DarkScript3 3.6.3 → open NR `common_func.emevd.dcx` (extracted via Smithbox's File Browser) and
   search `SetCharacterTeamType`. Record the entity argument and what it targets (player?).
3. Read `schemas/mod-profile.md` in the me3 repo for package keys before drafting any profile.
4. Run WitchyBND on a *copy* of `regulation.bin` to see whether it unpacks cleanly (decides whether it is usable
   next to Smithbox).
5. Determine the actual regulation version (title screen) and add it to the §1 pin table.

---

## Open questions

1. Does the NR engine honour `SP_EFFECT_PARAM_ST.changeTeamType` on player characters, and is the change
   visible to remote peers in an SC session?
2. Can `SetCharacterTeamType` address a player entity in NR, and does it run per peer?
3. What are NR's real hostile-to-player team-type values (team relation table is in the exe, not in params)?
   Which of Smithbox's NR `TEAM_TYPE` (16-22) vs the EMEDF names is right?
4. Does `friendlyTarget=1` / `isHitBothTeam` actually enable damage between two players?
5. What does `YuiNexusRegulationManager` do? Does SC overwrite params at runtime, or compare regulation
   versions/hashes between peers?
6. Which regulation version does our exe 1.3.3.0 + `regulation.bin` (2026-04-01) correspond to? (Smithbox mentions 1.03.5.)
7. Does WitchyBND open/repack the current NR `regulation.bin`?
8. Does me3 override `event/*.emevd.dcx` (inside `data*.bdt`) on NR the same way it does `regulation.bin`?
9. Is the SpEffect `changeTeamType` default for a new NR row 0 or -1 (the NR paramdef has no default)?
10. ER precedent for "players hostile via SpEffect/EMEVD in co-op" was not found in any indexed source.
