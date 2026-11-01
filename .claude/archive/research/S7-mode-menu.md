# S7: "Battle Royale" entry in the Roundtable expedition menu

Researcher: Sonnet | Date: 2026-10-10 | Status: review
Scope: NR unless labelled ER. Read-only: nothing launched, patched, installed or built; no game archive opened; `nrsc.dll` read only through plain-string listing; `SeamlessCoop\locale\english.json` read.
Tags: `[SOURCE: ...]`, `[VERIFIED: how]`, `[HYPOTHESIS]`, `UNKNOWN` (+ where I looked). ER facts are labelled ER and are hints only.
Shorthand: `fsrs-param` = https://raw.githubusercontent.com/vswarte/fromsoftware-rs/main/crates/nightreign/src/param.rs ; `PD` = https://github.com/soulsmods/Paramdex/tree/master/NR ; `SBX` = https://github.com/vawser/Smithbox ; `6PF` = https://github.com/Umgak/Nightreign6PlayerFixes (branch `mod`) ; `FR` = Free Reign, https://www.nexusmods.com/eldenringnightreign/mods/141 ; `BA` = Boss Arena, https://www.nexusmods.com/eldenringnightreign/mods/449 (Nexus pages read in the Browser pane, public, no login).

---

## 0. Bottom line

1. **The Roundtable table is an ESD talk script, with a native menu behind it.** Smithbox's NR script-name list names ESD `600001000` "Expedition Menu" (and 601..610001000 for Character Selection, Relic Rites, Visual Codex, Journal, Sparring, ...). [SOURCE: `SBX/Documentation/Game Information/NR/esd_script_names.txt`] Free Reign "replaces the expedition menu with a selection of freeplay options" at the Roundtable and "selecting Expedition will open the standard expedition menu instead", and it ships `script/talk/m10_00_00_00.talkesdbnd.dcx`, `event/m10_00_00_00.emevd.dcx`, `regulation.bin` and `msg/<lang>/menu_dlc01.msgbnd.dcx` through an me3 profile. [SOURCE: FR description; FR file-contents preview, read 2026-10-10] So **a pre-menu list with extra choices that then falls through to the stock expedition menu has a shipped NR precedent** (solo, no SC).
2. **Text is a plain FMG override and has an SC-compatible precedent.** 6PF ships `msg/<lang>/menu_dlc01.msgbnd.dcx`, adds entries to `EventTextForMap.fmg` (FMG id 34) and edits `CL_MenuText.fmg` (id 200), and is built to run next to SC. [SOURCE: 6PF repo `project/msg/*/menu_dlc01.msgbnd.dcx`; `project/.smithbox/Workflow/Exported Text/msg_en.json`; S3 section 3]
3. **SC relabels, it does not add.** SC resolves its three strings through its own `game_message_repository` layer from `locale\<lang>.json`; the three keys map 1:1 to the vanilla 1/2/3-player options and two of them have identical text. A third SC-independent label therefore cannot come from SC's json (we must not touch it) and is not safely done by an FMG edit of an ID SC overrides (shadowing risk, section 2).
4. **Which option the host picked is not readable today.** Candidate carriers exist (section 4) but none is verified. The robust design is host-authoritative: the host's DLL decides the mode and tells guests over the S4 channel; the menu is only the input device.
5. **Recommended route:** A now (launcher = mode, plus a visible label and an S4 handshake), then an **ESD pre-menu entry "Battle Royale"** (route B below) as the first "real" in-menu choice. Relabelling an SC-owned slot (route C) and editing the native GFx layout (route D) rank lower.

---

## 1. How the menu is built in NR

### 1.1 Layers (what we can name, with evidence)

| Layer | Finding | Tag |
|---|---|---|
| Roundtable map | Lobby is map `m10_00_00_00`; FR ships the `m10` EMEVD and talkesdbnd; EFID names for m10 include lobby flags (`10002700` "For lobby menu: Matching menu", `10002701` Hero selection, `...2705` Personal scenario, `10001800` Lobby event flag, `69990` "set when you enter the lobby ... if you play multiplayer with this flag set, the game start event will run") | [SOURCE: FR preview; `SBX/Documentation/Game Information/NR/event_flags/english/NR-Test -- EFID_m10_00_00_00` and `EFID_common`] [HYPOTHESIS: identity of the map from content] |
| Table interaction | ESD talk `600001000` "Expedition Menu", sibling ESDs 601..610001000. The talk file is `script/talk/m10_00_00_00.talkesdbnd.dcx` | [SOURCE: `esd_script_names.txt`; FR preview] |
| Lobby menu rows | `LOBBY_MENU_PARAM_ST` (PD `LobbyMenuParam`): fields `unknown_0, name_id, unknown_event_flag_1..4, icon_id (u16), MapLayer (u16), unknown_7`. Row names: 1000 Expeditions, 1010 Character Selection, 1020 Change Garb, 1030 Relic Rites, 1040 Visual Codex, 1050 Journal, 1060 Small Jar Bazaar, 1065 Collector Signboard, 1070 Garden, 1080 Shore, 1090 Waiting Room, 1100 Crypt, 1110 Sparring Grounds. A sibling `LOBBY_MENU_NPC_PARAM_ST` has `icon_id, icon_id_2, arrival_event_flag, show_map_icon_event_flag, hide_map_icon_event_flag` (rows = lobby NPCs: Wylder, Guardian, ... Traitor). This is the lobby map-point list, with `name_id` = a text id | [SOURCE: fsrs-param#L32533, #L32574; PD `Defs/LobbyMenuParam.xml`, `Names/LobbyMenuParam.txt`] [HYPOTHESIS: `name_id` indexes `CL_MenuText`, and the rows are map markers rather than the in-menu buttons] |
| Expedition (Nightlord) list | `NIGHT_BOSS_MENU_PARAM_ST`: `expedition_name_id, boss_icon_id, unlock_event_flag, defeat_event_flag, boss_name_id, defeated_boss_icon_id, sort_id, description_id, expedition_background_id, large_boss_icon_id`. This is the Nightlord picker, not the party-size control | [SOURCE: fsrs-param#L38451] |
| Other menu params | `MAIN_SCENARIO_MENU_PARAM_ST` (3 unknown s32), `MENU_COMMON_PARAM_ST`, `MENU_WINDOW_PARAM_ST`, `MENU_PROPERTY_LAYOUT/SPEC`, `MESSAGE_BOX_PARAM_ST` (1 unknown), `TALK_PARAM_ST` (`msgId`, `msgId_female`, voice/motion, `eventId`, ~40 unknowns), `OPERATION_GUIDE_PARAM_ST`, `NETWORK_MSG_PARAM_ST` | [SOURCE: fsrs-param struct list; PD `Defs/`] |
| No party-size param | No NR param with "sortie", "quickmatch", "party size" or "expedition type" in its name exists in fsrs-param (grep of all 59,023 lines; the only hits are `SORTIE_STARTING_POINT_ST` (spawn points) and `SESSION_RANDOM_ASSET_TEX_REPLACE_PARAM_ST`). `crates/nightreign` in fromsoftware-rs contains only `param.rs` and `lib.rs`; no engine structs for NR | [SOURCE: fsrs-param grep; https://github.com/vswarte/fromsoftware-rs/tree/main/crates/nightreign] [VERIFIED: tree listing 2026-10-10] |
| Vestigial BR field | `NETWORK_MSG_PARAM_ST` carries a column `battleRoyal` among ER-style invasion/summon text ids (`normalWhite`, `redHunterVisitor`, ..., `npc1..16`). It is a leftover from the ER struct; NR has no vanilla battle royale mode | [SOURCE: PD `Defs/NetworkMsgParam.xml`] [HYPOTHESIS: vestigial] |
| Text containers | NR menu text lives in `msg/<lang>/menu_dlc01.msgbnd.dcx`; Smithbox's NR registry lists these FMGs in it: Talk Messages (1), Talk Messages Female (4), Network Messages (31), Action Button Text (32), Event Text for Talk (33), Event Text for Map (34), Menu Text (200), Line Help (201), Key Guide (202), System Messages (203), Dialogue (204), Loading/Tutorial text (205-208), Personal Scenario text (211-213), plus DLC1 duplicates (500-516). Item text is `item_dlc01.msgbnd.dcx`. Both FR and 6PF ship only the `*_dlc01` bundles | [SOURCE: `SBX/Smithbox.Release/Output/Assets/FMG/NR/Fmg Descriptor Registry.json`; FR preview; 6PF tree] |
| Layout / GFX | Menu layout is Scaleform `.gfx` files in the game `menu` folder; existing mods replace them as loose files (Smaller HUD: `01_000_fe.gfx`, `01_001_fe_soul.gfx`; UI aspect-ratio mods use a Flash decompiler). I did not find the file that draws the expedition-type control, and found no `.sblyt` mention for NR | [SOURCE: Nexus NR mods 823, 161, 160 (search snippets, pages not opened)] UNKNOWN for the expedition screen |
| Engine singletons present in NR | `CSMenuMan` (exe+0x3C149D8), `MsgRepository` (exe+0x3C246A8, derived slot), `MsgTagMan`, `CSWorldTalkMan` (exe+0x4433DF0), `CSResidentTalkFileMan` (exe+0x4433A18), `CSSessionManager`, `CSTeamSessionManager`, `CSTeamSessionDataMan`. No `CSQuickMatch*` or "Sortie" singleton | [VERIFIED: `.claude/experiments/logs/2026-10-09_TC-004_runs1-2.log`] |
| ER shape of the menu/talk layer (hint only) | ER crate has `MsgRepositoryImp` (empty struct, singleton "MsgRepository"), `CSMenuManImp`, `TalkScript`/`CSNpcTalkIns` with `NpcMenuState { current_open_menu: MenuType }`, and `MenuType` (ER: TalkList=1, Warp=2, ... ArenaMenu=34) | [SOURCE: fromsoftware-rs `crates/eldenring/src/cs/{msg_repository,menu_man,talk_script,menu_type}.rs`] ER; NR values UNKNOWN |

### 1.2 What I could not establish

- The text ids (and FMG) of the vanilla party-size strings. No published NR id list was found (web search; the Smithbox export in 6PF contains only 6PF's own two edited FMGs). A human must extract `menu_dlc01.msgbnd.dcx` and search "Solo" / "Duo" / "Trio" / "player". UNKNOWN.
- Whether party size is a button row, a tab, or a toggle in the native menu. Press descriptions disagree on tab names ("the label varies"); one says "set the number of players to be one, two, or three", PC Gamer says a switch at the bottom of the menu from multiplayer to solo. [SOURCE: https://www.gamesradar.com/games/action-rpg/elden-ring-nightreign-multiplayer-join-friends/ ; https://www.pcgamer.com/games/rpg/elden-ring-nightreign-multiplayer-crossplay-solo/ (secondary, via search summary)] UNKNOWN. Duo was added in patch 1.02. [SOURCE: https://nightreignwiki.com/guides/duo-mode/ (secondary)]

---

## 2. How SC presents its entries

| Fact | Tag |
|---|---|
| `english.json` has exactly these menu strings: `YUINEXUSBEGINONEPLAYER` "Solo Mission", `...TWOPLAYER` "Up to 6 players", `...THREEPLAYER` "Up to 6 players", `...PLAYERCOUNTREMAINING` "{} / {}", plus not-seamless / wrong-version / paused / resumed | [VERIFIED: read `...\Game\SeamlessCoop\locale\english.json`] |
| All eight keys are literal strings inside `nrsc.dll`, with the prefix `MODMESSAGE_`; also `nrsc\cs\game_message_repository/../../mod/message_repository/locale.h`, the error text "Expected JSON object in translation file for locale", and RTTI `CSQuickMatchLobbyContext`, `EcTestSortieMemberNum`, `EcTestPlayerNo`, `EcTestTeamType`, `EcTestChrDead`, `EcTestEventFlagState`, `PartyMemberInfo` | [VERIFIED: `grep -a -o -E '[ -~]{6,}' nrsc.dll`, 2026-10-10] |
| So SC has a module that wraps the **game's message repository** (`cs\game_message_repository`) and feeds it strings from per-language json (one json per game language, `locale\`). Since `MsgRepository` is a game singleton, the likely mechanism is a hook on the text lookup that returns the json string for chosen message ids. Which game ids are intercepted is UNKNOWN | [VERIFIED: strings] [HYPOTHESIS: mechanism] |
| SC changelog 1.04: menu text changed "to make it more clear that the Two player and Three player options ... are no longer relevant" (SC ignores the 2/3 split; both mean "a lobby of up to 6") | [SOURCE: Nexus NR mod 3 changelog 1.04, as quoted in `.claude/research/T-003-seamless-coop.md` section 6; search snippet of https://www.nexusmods.com/eldenringnightreign/mods/3] |
| Since 0.03 there is no ini player count: "the party just starts the expedition when ready" | [SOURCE: Nexus NR mod 3 changelog 0.03; T-003] |
| `EcTest*` names mirror EMEVD condition kinds (`IfIsPlayerNo` 4[38], `IfCharacterHasTeamType` 4[36], flag state, character dead). `...SortieMemberNum` has no counterpart in the NR EMEDF I have, so it is a condition kind used by game scripts to test the number of members in the sortie | [SOURCE: nr-emedf (`https://soulsmods.github.io/emedf/nr-emedf.html`): no instruction with Sortie/Member in its name] [HYPOTHESIS: "Ec" = event condition; SC hooks these to lift the 3-player cap] |

**Reading.** [HYPOTHESIS, medium-high confidence] The vanilla menu still has three distinct party-size choices (1, 2, 3). SC keeps them and changes the labels: 1 = "Solo Mission", 2 and 3 both "Up to 6 players". It does not add or remove entries.

**Implications for a third distinct choice**
- *Reusing the 2- or 3-player slot as "Battle Royale".* The slot already exists and SC treats both identically, so repurposing one is functionally free. **But** (a) its label is SC's (we cannot edit `english.json`: hard rule 3 and the author's FAQ "do not modify, redistribute, or bundle this mod" [SOURCE: T-003 section 5]); (b) if SC substitutes the label at lookup time, an FMG override of the same id would be overwritten by SC or would overwrite it depending on hook order, outcome UNKNOWN; (c) we do not know whether our DLL can tell slot 2 from slot 3 after SC processed the click, and what the lobby does when the user picks one (UNKNOWN: SC may normalise both to the same value).
- *Drop-in text via our own DLL.* We hold the `MsgRepository` singleton, so a DLL could hook the same lookup and substitute the string after SC. That is a second hook on a function SC already hooks: not recommended before the cheaper routes fail. [HYPOTHESIS]
- *A pre-menu entry avoids all three issues* (section 5, route B): it sits before the stock expedition menu, in a layer SC does not touch (to our knowledge).

---

## 3. Precedent for adding or relabelling entries

| Item | What it shows | Tag |
|---|---|---|
| **Free Reign** (TKGP, mod 141, v1.1.2, "Supports NR 1.03.2 // Calibrations 1.03.4") | NR Roundtable table interaction replaced by a freeplay list, the stock "Expedition" item still opens the normal menu. Ships `FreeReign.me3` (108 B), `Mod/event/` (common_func, m10, m16, m18, m19 EMEVD), `Mod/map/...`, `Mod/msg/<15 languages>/{item,menu}_dlc01.msgbnd.dcx`, `Mod/regulation.bin`, `Mod/script/talk/m10_00_00_00.talkesdbnd.dcx`, `Mod/sd`, `Mod/sfx`. "Installation with me3 is required" (UXM outdated). Known issues include "the menu map always shows Roundtable Hold" | [SOURCE: FR Nexus page and file preview, read 2026-10-10] |
| **Boss Arena** (PositronCannon, mod 449) | Adds a boss-selection menu and Boss Rush at the Roundtable ("changes map data, events, talk scripts, text"); me3 profiles only; **"Seamless Co-op is inherently incompatible"** because it replaces expeditions; separate save files | [SOURCE: BA Nexus description, read 2026-10-10; S3 section 3] |
| **6PF** | Edits `menu_dlc01` text (adds IDs to Event Text for Map, edits Menu Text 401320 for a version line) and ships events/regulation, with SC `load_early` | [SOURCE: 6PF repo; README] |
| **Fun is Allowed** (mod 49) | Param, HKS and UI overhaul with SC profiles | [SOURCE: S3 section 3, Nexus] |
| SC itself | Relabels via json (section 2). Also edits the Roundtable display ("party member visibility in the Round Table Hold") | [SOURCE: T-003 section 1; Nexus mod 3] |
| ER (hints) | ER mods add custom ESD menus (talk lists) and edit FMG text; the Souls modding wiki documents that dialogue lines come from ESD files and `TalkParam`, and that the TalkParam id does not always match the FMG id | [SOURCE: https://soulsmodding.wikidot.com/tutorial:intro-to-elden-ring-emevd (search summary)] ER |

No mod was found that adds a button to the **native** expedition-type screen (the GFX layer). Searched: Nexus NR search results, web search ("expedition menu GFX/Scaleform", "Solo/Duo/Trio"). UNKNOWN (absence not proven; Nexus search is JS-driven).

### Tools (NR support as of today)

| Tool | NR support | Use here | Tag |
|---|---|---|---|
| Smithbox (vawser) | NR is a project type; Text Editor edits FMGs without unpacking; params; has NR row names, esd names and flag lists | Extract/inspect `menu_dlc01` text, add new FMG entries, edit `LobbyMenuParam` | [SOURCE: SBX README/releases; NR assets in repo] |
| ESDLang (`esdtool.exe`, thefifthmatt) | "DS1, DS2, Bloodborne, DS3, Sekiro, Elden Ring, AC6, and Nightreign are supported" | Decompile/recompile `m10_00_00_00.talkesdbnd` ESD scripts | [SOURCE: https://github.com/thefifthmatt/ESDLang README] |
| WitchyBND | "Basic initial support for ELDEN RING NIGHTREIGN"; FMG, BND4, DCX, PARAM; reads NR TAEs | Unpack `.msgbnd.dcx` / `.talkesdbnd.dcx` containers and FMG | [SOURCE: https://github.com/ividyon/WitchyBND README and changelog] |
| DarkScript3 3.6.3 | NR EMEVD | Only if a flag-setting EMEVD is needed | [SOURCE: S3 section 4] |
| Nuxe 1.2.1 | Unpacks NR archives (use unpack-only, never its exe patcher) | Getting the vanilla files out | [SOURCE: S3 section 4] |

Soulstruct is also listed by the Souls modding wiki for ER; NR support not checked. UNKNOWN.

### me3 packaging
me3 packages override game files by game-relative path (`msg/engus/menu_dlc01.msgbnd.dcx`, `script/talk/m10_00_00_00.talkesdbnd.dcx`, `event/...`, `regulation.bin`). Proven in the wild by FR (all four kinds) and 6PF (msg, event, regulation) on NR 1.03.x. [SOURCE: FR and 6PF file lists; S3 section 4] **Overrides replace the whole file.** Consequences: FR/BA and our `m10` talk/event files cannot coexist; 6PF and ours cannot both own `menu_dlc01.msgbnd`; a game patch means re-extracting and re-applying our edit. [HYPOTHESIS: whole-file replacement, consistent with all examples]

---

## 4. How our DLL could learn the chosen option

| # | Carrier | Reasoning | Tag |
|---|---|---|---|
| 1 | **An event flag set by our ESD (or EMEVD) when "Battle Royale" is picked**, read locally by the DLL (host side) | We define the semantics; the flag lives in the m10 flag group (EFID m10 names flags `10002700`.. etc.; 6PF used `11007000`..`11008100` for its own markers, so the `11xxxxxx` range is evidently usable for mod flags) | [SOURCE: 6PF common.js#L14, globalFlags.js; EFID_m10] [HYPOTHESIS: a given id is unused in our build] Reading a flag from the DLL needs the NR `CSEventFlagMan` layout; `probe2.rs` already does read-only ER-layout flag probes marked UNVERIFIED (T-027) |
| 2 | **Host announces over the S4 channel** (own `ISteamNetworkingMessages` channel, addressed by `CSSessionManager` Steam IDs), guests accept the host's value | Needed anyway because a guest's local flag is not the host's choice; flags are not networked by the setter | [SOURCE: S4 sections 0-1, `event_flag.rs` doc "Does not inherently network set flags"] |
| 3 | Game-synced party-size state: `CSTeamSessionDataMan`, `CSQuickMatchLobbyContext` (RTTI inside SC), `PartyMemberInfo`, the value behind `EcTestSortieMemberNum` | The game must agree on the expedition type among members, so it plausibly sits in session/lobby data. Layouts, offsets and even whether SC keeps the vanilla value are UNKNOWN. Discovery experiment: hex-dump `CSTeamSessionDataMan` (exe+0x3C21F48) and `CSTeamSessionManager` (exe+0x3C21FC8) instances at the Roundtable while toggling 1P / 2P / 3P; diff. Cheap, solo, read-only | [VERIFIED: slots in T-006 log] UNKNOWN: contents |
| 4 | Lobby / member data via Steam matchmaking | SC runs a Steam lobby; our DLL cannot get its id without RE | [SOURCE: S4 section 2.3] |
| 5 | Watch SC's `YUINEXUS...` strings in memory | Brittle, tied to SC internals | [HYPOTHESIS] not recommended |

Do guests know the host's choice natively? UNKNOWN. In vanilla, the party leader's choice reaches matched players through FromSoftware's servers; under SC the lobby is SC's Steam lobby, so the mechanism is SC's. Treat guest knowledge as "none until our handshake".

---

## 5. Ranked routes (cheapest first)

### A. Launcher = mode, plus a visible label and handshake (no menu change)
- **What:** keep "whoever launched with our launcher". Show an on-screen label ("Battle Royale mode ON, N players have it") from the overlay; the host broadcasts `mode=BR, version, config hash` over the S4 channel; a guest without the DLL simply never answers, so the host's label turns red.
- **Effort:** small (overlay already exists in `crates/nrpvp/src/overlay/`; S4 channel is the dependency).
- **Risk:** low. No game file touched. The mode is not a visible menu choice; a player can forget to launch with our launcher (the handshake catches it).
- **In-game check:** solo: label shows after loading into the Roundtable and again in an expedition. Nucleus 2 instances: instance B launched without the DLL, A's label shows "peer missing"; both with the DLL: both labels show the same mode and version. FS: same over real Steam.

### B. ESD pre-menu entry "Battle Royale" (Free Reign pattern; recommended first real menu change)
- **What:** edit ESD `600001000` ("Expedition Menu") in `m10_00_00_00.talkesdbnd.dcx` so the table first offers a short list: "Expedition" (unchanged path into the stock menu) and "Battle Royale" (sets our flag, then opens the same stock expedition menu). Add the new label as a **new FMG id** in a free range of `Event Text for Talk` (33) or `Talk Messages` (1) inside `menu_dlc01.msgbnd.dcx`, in every language we ship (english first, others copy english). The DLL reads the flag (carrier 1) and announces it (carrier 2). The party-size options and SC's labels stay exactly as SC sets them.
- **Why it is the best "real entry":** it avoids SC's relabelled strings, needs no GFX work, and has NR precedent (FR). Guests do not need the edit to join, since the host broadcasts the mode.
- **Effort:** medium. HUMAN extracts `script/talk/m10_00_00_00.talkesdbnd.dcx` and `msg/engus/menu_dlc01.msgbnd.dcx` (Smithbox or Nuxe unpack-only, scratch folder), decompiles with ESDLang, finds the call that opens the expedition menu, adds a list choice and a `SetEventFlag`. Researcher/implementer then diff-checks and writes the me3 package. FMG edit in Smithbox's text editor.
- **Risk:** medium. (1) SC may depend on the unmodified ESD flow in the lobby (UNKNOWN; it shows party members in the Roundtable by hooks). (2) Whole-file replacement of the talk bundle conflicts with Free Reign/Boss Arena and ties us to build 1.3.3.0 (re-extract after each patch). (3) ESDLang recompile fidelity on NR is untested by us. (4) The ESD edit runs locally, so nothing is shared by itself.
- **In-game check (solo, via the me3 profile with SC `load_early`):** save backup first; (i) interact with the table: list shows both entries; (ii) "Expedition" leads to the stock menu with SC's "Solo Mission / Up to 6 players" intact and starting a solo expedition works; (iii) "Battle Royale" leads to the same menu; DLL log shows the flag flip and `mode=BR`; (iv) nothing else in the Roundtable (character select, relic rites, sparring) changed; (v) Nucleus 2 instances: host picks BR, guest's DLL reports `mode=BR` from the S4 message. Failure modes to log: SC lobby fails, menu does not open, table does nothing.

### C. Relabel an SC-owned slot as "Battle Royale" and treat that selection as the mode
- **What:** repurpose the 3-player (or 2-player) option, change its label to "Battle Royale", and read which slot was chosen.
- **Effort:** medium-high, and **blocked on three unknowns**: the text id/FMG of that option, whether SC's message hook overrides our FMG edit (or ours overrides SC's), and where the selected slot is stored after SC acts (section 4, carrier 3).
- **Risk:** high. Relabelling SC's 2P/3P slot by FMG may be shadowed; a DLL text hook is a second hook on a function SC hooks; and picking the slot may be normalised by SC. Also a visible side effect: the slot SC labels "Up to 6 players" is the one a normal SC user would pick.
- **In-game check:** (1) Human screenshot of the expedition menu under SC (does it show three entries, two reading "Up to 6 players"?). (2) Extract the text, find which FMG ids hold "Solo" / "Up to ..." equivalents. (3) With an FMG edit of only the 3P text id, look at what the menu shows (SC string or ours). (4) Dump `CSTeamSessionDataMan` while toggling the slots (carrier 3) and see whether slot 2 and slot 3 differ in memory.

### D. A true third button in the native expedition menu (GFX / layout edit)
- **What:** add a control to the expedition screen's Scaleform layout and wire it.
- **Effort:** high. The file that draws the control is unidentified; GFX edits need a Flash decompiler; logic wiring (what the button does in ESD/engine) is unknown.
- **Risk:** high: breaks on patches; no NR precedent found; SC's own UI edits may overlap.
- **In-game check:** only after A/B have shipped and shown the layer is worth the cost. Not recommended in this phase.

### Ranking summary

| Rank | Route | Effort | Risk | First verifying step |
|---|---|---|---|---|
| 1 | A launcher = mode + label + S4 handshake | S | Low | Nucleus: missing-peer warning |
| 2 | B ESD pre-menu entry + flag | M | Medium | Table list visible, SC lobby unaffected |
| 3 | C relabel SC slot | M-H (blocked) | High | Menu screenshot + text-id extraction + memory diff |
| 4 | D native GFX button | H | High | Not scheduled |

---

## 6. Constraints check

- **SC untouched:** routes A and B add files only in our own me3 package (`msg/`, `script/talk/`, optionally `event/`) and our own DLL; none edits `nrsc.dll`, `nrsc_settings.ini` or `locale\english.json`. Route C-by-json would edit SC's locale file (not allowed) and is excluded. [SOURCE: CLAUDE.md hard rule 3; T-003 section 5]
- **me3 packaging:** supported and shipped by FR and 6PF (section 3). We could still not confirm that an `m10` ESD/EMEVD replacement coexists with SC (FR and BA do not claim to).
- **Overlap with other mods:** FR and BA replace the same Roundtable files; our README must say "do not combine".
- **Agreement among players:** host-authoritative via S4 (carrier 2), never via the menu alone.

---

## 7. What a human does next (read-only, cheap)

1. Screenshot the Roundtable table and expedition menu **under SC**: list every entry and show where "Solo Mission" and "Up to 6 players" appear (button, tab, or start button).
2. Extract (Smithbox, scratch project, never saving over the game) `msg/engus/menu_dlc01.msgbnd.dcx` and `script/talk/m10_00_00_00.talkesdbnd.dcx`. Search the FMG text for "Solo", "Duo", "Trio", "Expedition", "player"; record FMG name and ids. Decompile ESD `600001000` with ESDLang and record where it opens the menu.
3. Run the memory-diff probe at the Roundtable: dump `CSTeamSessionDataMan` / `CSTeamSessionManager` while toggling 1P / 2P / 3P; save the logs under `.claude/experiments/logs/`.
4. Log which flags in `10002700..10002707` change when each lobby menu is opened (read-only flag probe from T-027).

## Open questions

1. What are the vanilla text ids and FMG for the party-size options, and does SC override them by id (shadowing) or does it draw its own text?
2. Does the expedition screen show three entries or a single control? Does the game store the party size in `CSTeamSessionDataMan`/`CSQuickMatchLobbyContext`, and does SC overwrite it?
3. Does ESD `600001000` simply call an "open expedition menu" function that our added list can safely precede? Is the same ESD also triggered for guests?
4. Does an `m10` talk/EMEVD replacement coexist with SC (neither FR nor BA claims it)? Does SC watch the lobby event flags (`10002700` family) or `69990`?
5. Do guests receive any host expedition parameters natively under SC (Nightlord, depth), which would let the mode ride on an existing synced value?
6. Is a flag in the `11xxxxxx` range free in our build, and what does `CSEventFlagMan` look like in NR (T-027 is ER-layout, UNVERIFIED)?
7. Is the base game's menu text really only in `menu_dlc01.msgbnd` (as FR/6PF and Smithbox imply), or also in a base `menu.msgbnd` that a game archive holds?
8. ESDLang and WitchyBND round-trip on NR `talkesdbnd` and `msgbnd` files: untested by us.
9. The author's stance (T-003 section 5): does a mod that adds a PvP entry to the SC-patched Roundtable need his permission before public release? Unchanged open item.
