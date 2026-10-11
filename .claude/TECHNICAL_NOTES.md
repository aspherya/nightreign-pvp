# Technical Notes

Tag legend: `[VERIFIED: how]` checked on this machine or in-game · `[SOURCE: link]` from external
documentation or code, not yet checked here · `[HYPOTHESIS]` unproven, needs an experiment · `UNKNOWN`.

---

## 1. Environment and version pins (as of 2026-10-09)

| Component | Value | Evidence |
|---|---|---|
| Game | `nightreign.exe` FileVersion **1.3.3.0**, Steam buildid **22818764**, SHA256 prefix `9545E8732919BB6A` | [VERIFIED: PE version info, appmanifest_2622380.acf, Get-FileHash] |
| regulation.bin | SHA256 prefix `876A3CA279A4561D`, dated 2026-04-01 | [VERIFIED] |
| Seamless Co-op | `nrsc.dll` SHA256 prefix `243EEC929A97B71E`, installed 2026-10-09. No PE version resource; it is **SC 1.13** (Nexus upload 2026-07-02, "compatible with game 1.03.3"; sizes match the package and the DLL contains "1.13") | [VERIFIED: hash/size] [SOURCE: Nexus NR SC mod 3; T-003] |
| SC launcher | `nrsc_launcher.exe` SHA256 prefix `2726DEA099A54F00` | [VERIFIED] |
| me3 | 0.11.0 (2026-02-28) at `Z:\Tools\me3` | [VERIFIED: `me3 --version`] |
| Toolchains present | VS 2022 Community 17.14, CMake, vcpkg (`C:\vcpkg`), .NET SDK, git, Node | [VERIFIED] |
| Toolchains missing | Rust (rustup), Python, `gh` CLI | [VERIFIED] |
| RE tools present | Cheat Engine | [VERIFIED] |
| me3 loads our native DLL into NR 1.3.3.0 (no SC), no crash at startup, no `disable_arxan` needed for a passive DLL | [VERIFIED: T-001 HUMAN-solo run, dist/nrpvp.log 2026-10-09] |
| Saves | `%APPDATA%\Nightreign\76561198342403479\NR0000.sl2`, no `.co2` yet (SC not run yet) | [VERIFIED] |

When the game or SC updates, re-hash, update this table, and re-run the regression tests in TESTING.md.

---

## 2. Stack decision (ADR-001, **pending review with Leonardo**)

**Decision:** write the mod as a **Rust native DLL (`cdylib`) loaded by me3**, next to SC, using the
**fromsoftware-rs** ecosystem as the reverse-engineering foundation.

### Why Rust

1. **Best reverse-engineering head start.** [fromsoftware-rs](https://github.com/vswarte/fromsoftware-rs)
   (Apache-2.0/MIT, active, last commit 2026-09-20) has an `eldenring` crate with mapped structures for
   exactly the systems we need: `chr_ins` (it has a `team_type: u8` field), `team_relation`
   (CSTeamTypeFriend/Enemy/Rival/Neutral hostility logic), `world_chr_man`, `session_manager`, `net_man`,
   `network_session`, `net_chr_sync`, `player_game_data`. Nightreign is a fork of the ER engine, so these
   are the closest available map. [SOURCE: repo tree] The same repo has a `nightreign` crate, but it holds
   **only param definitions** (`lib.rs` = `pub mod param;`), so we port the ER layouts we need and verify
   each one against NR. [VERIFIED: fetched crates/nightreign/src/lib.rs]
2. **The loader is Rust-native.** me3 is Rust. It loads native DLLs, calls initializers, controls load
   order, and can neutralise Arxan (`disable_arxan`, via `dearxan`). [SOURCE: me3 CHANGELOG #457, #484, #501, #634]
3. **Proven pattern for FromSoft runtime tools.** Rust with `hudhook` (ImGui overlay for DX12) is the
   established pattern for Souls practice tools, which gives us an in-game debug panel cheaply once we
   need one.
4. **Sonnet writes idiomatic Rust well,** and the compiler catches FFI and memory mistakes that would
   otherwise surface as game crashes.

### Alternatives considered

- **C++ (MSVC + vcpkg + MinHook/safetyhook + ImGui):** the toolchain is already installed and there is a
  lot of ER DLL-mod prior art. There is no maintained NR/ER structure library of the same quality (libER
  is ER-only and less complete), so we would hand-map everything. **This is the fallback** if a Rust
  blocker appears (for example, fromsoftware-rs utilities don't work on NR and we end up hand-mapping
  anyway).
- **C#:** good for offline tools. Injecting a managed runtime into the game process is avoidable
  complexity. **Rejected for the in-game DLL.** Allowed for side tooling if useful.

### Crates (initial; add only when a task needs one)

| Crate | Use |
|---|---|
| `windows` | Win32 APIs (DllMain, threads, memory, keyboard state) |
| `retour` | Function detours, used only when a task needs a hook |
| `pelite` | PE parsing and AOB pattern scanning in `nightreign.exe` |
| `tracing`, `tracing-subscriber`, `tracing-appender` | File logging |
| `serde`, `toml` | `nrpvp.toml` config (hotkeys, experiment toggles) |
| `fromsoftware-shared` | Singleton finder and shared engine utilities. **Must be validated on NR** (T-006) |
| `hudhook` | ImGui overlay. **Deferred** until hotkeys and logs aren't enough |

### Tools

| Tool | Role | Status |
|---|---|---|
| me3 0.11 | Launch the game with no EAC, load `nrsc.dll` and `nrpvp.dll`, Arxan handling | installed |
| Seamless Co-op (NR) | Networking foundation: lobby, Steam P2P, up to 6 players | installed |
| rustup (stable, msvc) | Build | **to install** (HUMAN) |
| Cheat Engine | Live memory inspection, finding offsets, quick write experiments | installed |
| Ghidra (or IDA Free) | Static analysis of `nightreign.exe` | to install when needed |
| ReClass.NET | Interactive struct mapping | optional |
| Smithbox | Param, EMEVD and map viewing (NR support must be checked per release) | to install when needed |
| WitchyBND | Unpack BND/DCX archives | to install when needed |
| DarkScript3 | EMEVD event scripts (alternative team-type path) | to install when needed |

---

## 3. Seamless Co-op (networking foundation)

| Fact | Tag |
|---|---|
| Author: Yui (LukeYui). Closed source, distributed on Nexus. No public API or extension points. | [SOURCE: press coverage; no source in install] |
| Has its own launcher (`nrsc_launcher.exe`), its own save extension (`co2`), and blocks official servers | [VERIFIED: files + ini] [SOURCE: guides] |
| Lobby up to **6 players** | [VERIFIED: locale `english.json` "Up to 6 players"] |
| **NR SC has no PvP, friendly-fire, team, or invasion features.** The author calls it feature complete and in maintenance mode, with no plans for PvP (PC Gamer, 2025-08-05) | [SOURCE: Nexus NR SC page, changelog; T-003] |
| The full ini key set is `[GAMEPLAY]` (empty), `health_scaling`, `damage_scaling`, `posture_scaling`, `save_file_extension`. Nexus lists the 1.13 ini at 246 B, the same as ours, so there are no hidden keys | [SOURCE: Nexus file preview; T-003] |
| SC is WinLicense-protected and finds game code by AOB. It hooks `WorldChrManImp` (incl. `_UpdateMultiDoping`) and has a param repository. In ER, SC reverts some runtime edits ("minimalist anticheat"). **Risk: SC may revert our team-type writes** | [VERIFIED: strings] [HYPOTHESIS: NR behaviour] |
| SC fetches Everdark/Endless Night data and a VERSION file from GitHub (`yuiamoroll/NightreignSeamlessCoopRelease`) | [SOURCE: T-003] |
| **Terms:** no redistribution, modification, or bundling without permission; the GitHub release repo has no license. Use as a dependency isn't addressed, but 47 Nexus mods declare SC as a requirement. Policy: never bundle, patch, or hook inside nrsc.dll; users install SC from Nexus; ask Yui before any public release | [SOURCE: Nexus permissions/FAQ; T-003] |
| Networking uses **Steam** (`SteamNetworkingMessages002`, `SteamMatchMaking009`), not EOS | [VERIFIED: import strings in nrsc.dll] |
| Exports or expects `modengine_ext_init`, and says *"If you're using ModEngine, please make sure seamless co-op is loaded early"* | [VERIFIED: strings in nrsc.dll] |
| Config keys are **not stored as plain text**, so hidden options can't be found statically (even `health_scaling` isn't a literal) | [VERIFIED: string search] |
| nrsc.dll RTTI references game classes `EcTestTeamType`, `EcTestChrDead`, `EcTestPlayerNo`, `PlayerNetworkSession`, `PartyMemberInfo`, `CSQuickMatchLobbyContext`. SC touches team-type event checks and party membership | [VERIFIED: RTTI strings] |
| ER SC had in-game PvP and friendly-fire toggles (Judicator's Rulebook, `allow_invaders`). **NR SC has no equivalent** | [SOURCE: T-003] |
| me3 + SC: me3 v0.5.0 added "Support loading mods that rely on modengine_ext_init" / "Support Seamless Coop". An earlier NR-specific report (me3 discussion #118, 2025-06-09) was **unresolved**. Current behavior on 0.11 is UNKNOWN (T-004) | [SOURCE: me3 CHANGELOG; [discussion #118](https://github.com/garyttierney/me3/discussions/118)] |

**Loading SC through me3 (T-004):** the Dec-2025 SC update requires early loading. Without it you get
`Method "SignIn" is not available`. me3 v0.10+ provides `load_early = true` for this (issue #621, PR #624).
A native with **no** `initializer` gets me3's ModEngine2 shim, which calls `modengine_ext_init`. Setting an
explicit initializer bypasses the shim (the maintainer warned it would crash). `disable_arxan` is a top-level
profile key, default false, forced on only for DS3. NR natives don't need it so far: T-001 ran fine with
dearxan's default path. [SOURCE: me3 v0.11.0 `crates/mod-host/src/host.rs`, `lib.rs`, `cli/src/db/profile.rs`;
issue #621] **[VERIFIED: 2026-10-09 HUMAN-solo; me3 log "loaded native with me2 compatibility shim", `.co2` written]**

**Party vs team:** SC's *party* is the networking group (everyone in the lobby). Our *teams* are a gameplay
concept (who may damage whom). Phase 0 assumes everyone shares one SC party and hostility comes only
from engine team types. No separate team system until an experiment proves one is needed.

---

## 4. Engine findings relevant to PvP

| Finding | Tag |
|---|---|
| `nightreign.exe` RTTI has `ChrIns`, `PlayerIns`, `WorldChrManImp`, `CSSessionManagerImp`, `CSNetManImp`, `EcTestTeamType`, `EcTestChrSosInvadeType`, and **`ChrTemporaryTeamTypeSlot`** | [VERIFIED: RTTI string scan] |
| `CSTeamType*` class names were **not** found as RTTI in NR, so the team-relation implementation may differ from ER. UNKNOWN | [VERIFIED: absent from scan] |
| NR params keep PvP damage scaling: `ATK_PARAM_ST.dmg_level_vs_player`, `EQUIP_PARAM_WEAPON_ST.vs_player_dmg_correct_rate_*`. `is_hit_both_team` is on **BULLET_PARAM_ST** (projectiles only), not AtkParam. AtkParam has `opposeTarget`/`friendlyTarget`/`selfTarget` | [SOURCE: fromsoftware-rs `nightreign/src/param.rs#L7203`; T-011] |
| `SP_EFFECT_PARAM_ST.change_team_type` (s8) exists in NR. ER semantics: "overwrite with the specified team type", default -1. NR semantics on players UNKNOWN | [SOURCE: param.rs#L51538; T-011] |
| NR EMEDF has `2004[2] SetCharacterTeamType(entityId, teamType)` and `4[36] IfCharacterHasTeamType`. Vanilla NR uses it in `common_func`, m10, m13, m18. Whether it can target players is UNKNOWN | [SOURCE: DarkScript3 NR EMEDF; T-011] |
| Team-type enum candidates (ER-derived Smithbox enum, conflicting on 16–22): 19 Host, 20 Co-op, 21 Hostile per EMEDF and the SpEffect enum. NR extras: 77 "Near Death", 80 "Balancer Raid". Hostile-to-player candidates: 1, 13, 19, 20, 21, 29 | [HYPOTHESIS until T-007/T-012; SOURCE: Smithbox, EMEDF via T-011] |
| Leftover invasion fields in NR (`MultiplayerState.Invasion = 5`, `autoInvadePoint_*`, versus-ghost NetworkParam fields). This shows the layout survived, not that the code is live | [SOURCE: T-011] |
| `nrsc.dll` contains `YuiNexusRegulationManager`. SC may edit params at runtime or gate lobbies on regulation, which is a risk for any modded-regulation route | [VERIFIED: string scan] [HYPOTHESIS: purpose] |
| In ER, `ChrIns.team_type: u8` is at **+0x6C** (chr_type +0x68, npc_param_id +0x60). `WorldChrMan.main_player` +0x1E508, `player_chr_set` +0x10EE0. Full hint sheet in `.claude/re/er-reference.md`. **ER layout only; a CE starting point for NR** | [SOURCE: fromsoftware-rs fbd3b, offset_of!; T-005] |
| `team_relation.rs` is **not** a hostility matrix. It only defines Neutral/Friend/Enemy/Rival filters over an `{oppose, friendly, self}` result. The team_type × team_type hostility table is unmapped: UNKNOWN | [SOURCE: T-005] (corrects an earlier note) |
| The singleton finder is the external `from-singleton` crate (re-exported by `fromsoftware-shared`): a game-agnostic `.text` pattern scan returning DLRF names (`WorldChrMan`, `CSSessionManager`, …). Untested on NR. NR has **two `.text` sections** plus `.interpr` | [SOURCE: from-singleton v3.0.1; VERIFIED: NR PE headers] |
| Singleton discovery works on NR 1.3.3.0: 235 DLRF singletons; `from-singleton` stock `map()` is sufficient (all call sites in the first `.text`). WorldChrMan slot exe+0x3C0F0A8, CSSessionManager exe+0x3C21EC8, CSNetMan exe+0x3C046A8, plus NR-only `CSTeamSessionManager`, `CSTeamSessionDataMan`. WorldChrMan is re-created on area change | [VERIFIED: T-006 log 2026-10-09] |
| ER offset WorldChrMan+0x1E508 (main_player) is **wrong on NR** (null or garbage) | [VERIFIED: T-006 probe] |
| RTTI is plain in NR (8813 classes). ChrIns-derived vtables: ChrIns exe+0x2C0BA90, **PlayerIns exe+0x2C307F8**, **EnemyIns exe+0x2C18120**, ReplayGhostIns exe+0x2C1A450, ReplayEnemyIns exe+0x2C19D58, CSJamaisVuReplayPlayerIns exe+0x2C17578, CSJamaisVuReplayEnemyIns exe+0x2C16F10, CSHamariSimulateChrIns exe+0x2CD7D08. WorldChrManImp vtable exe+0x2C1B930. Hierarchy: PlayerIns/EnemyIns > ChrIns > FieldInsBase | [VERIFIED: T-007a log 2026-10-09] |
| **Character layout (NR = ER shifted by +8):** ChrIns+0x68 u32 = NpcParam/character id (enemies 10000010…, objects 1000000); ChrIns+0x70 = chr_type-like (5); **ChrIns+0x74 u8 = team_type**: enemies 6 (Enemy), 26 (Friendly NPC), 48 (Unknown Enemy Team), 0 (None); 4 pre-allocated PlayerIns objects = 30 (Object). Values match the Smithbox NR TEAM_TYPE enum | [VERIFIED: T-007a dumps, 8 enemies + 4 PlayerIns] [HYPOTHESIS: +0x74 is the field the engine uses for hostility, to be confirmed by TC-004] |
| **Local player:** WorldChrMan+0x9A40 = inline ChrSet (ER shape: capacity +0x10 = **6**, matching SC's cap; entries ptr +0x18; 0x10-byte entries, first qword PlayerIns*). Also referenced via pointers at +0x16440 and +0x17118. WorldChrMan+0x1C820 → stride-8 array holding 1 PlayerIns (main-player candidate). Big mixed ChrIns arrays at +0x9BB8 (cap 1600), +0x17518, +0x17548. Ten more PlayerIns are direct members in pairs at +0x9C20…+0xA170 (team 30 = Object, not the player) | [VERIFIED: T-007a run 2, solo expedition] |
| Local player team_type: **1 in Roundtable, 2 during an expedition** (+0x70 = 13 in the expedition). ChrSet entry bytes +8 = `04 04`. The F5 write 2→29 worked (readback 29) and F6 restored it. The F8 histogram in an expedition shows **PlayerIns with team 6 (Enemy)** among 34 PlayerIns (mostly 30 = Object), so the engine has player-type characters it treats as enemies. The ChrSet is momentarily capacity 0 during area loads (handled) | [VERIFIED: TC-004 run 1, 2026-10-09] |
| **ChrIns+0x74 IS the engine's hostility field on NR.** Local player 2→6 (Enemy): enemies ignore the player, the player can't lock on to or damage them; F6 (6→2) restores normal combat. The value held 25 s with no revert (persist off). With 29 (Indiscriminate), enemies stayed hostile | [VERIFIED: TC-004 run 2, 2026-10-09, log 19:04:23–19:04:48] |
| The overlay (hudhook 0.9.3 DX12) works alongside SC under me3: the panel renders and mouse clicks reach it. Writes 2↔6↔29 held with no reverts (29 held 33 s until the world unloaded). ChrSet entry byte +9 toggled 04→02→04 for ~5 s (a state flag? downed/dead?). WorldChrMan+0x1C820 held UTF-16 text, so it is **not** a player pointer | [VERIFIED: TC-004 run 3, 2026-10-09] |
| **Team 29 (Indiscriminate) on the local player through a full solo expedition:** lock-on, dealing and taking damage, deaths and revives, field bosses, and the night boss all behave normally (like team 2). The value **held 15.5 min with no reverts and persist OFF** (19:26:50–19:42:24, until the expedition ended). Side effect: **sites of grace can't be interacted with** (level-up menu unavailable), though passing through still applies the grace effect | [VERIFIED: TC-004 run 4, 2026-10-09] |
| **Hostility is decided per client from that client's own copies of team_type.** With one client holding 29 for both players and the other holding 2 for both, only the first could target and damage the other player | [VERIFIED: FS-0 run 2, 2026-10-10, two Nucleus instances] |
| **Two-way PvP works between two SC clients with team 29 set on both clients for both players:** lock-on, damage, HP sync, parry mechanics, PvE alongside. PvP death = downed → no revive possible → respawn with runes and a level lost | [VERIFIED: FS-0 run 4, 2026-10-10; Nucleus/Goldberg transport, not real Steam] |
| **Local player = ChrSet entry[0] on each client**, and **`ChrIns+0x78` is a per-player id identical on all clients** (host 0x6001FFFFFFFF, joiner 0x6002FFFFFFFF) | [VERIFIED: FS-0 (Nucleus) and FS-1 (real Steam, 2 PCs), 2026-10-10] [HYPOTHESIS: beyond 2 players] |
| SC binds F4 to its pause ("Paused" on screen); our hotkeys must avoid it | [VERIFIED: TC-004 run 1] |
| The `eldenring` crate can't run on NR: its RVA table checks ProductName and ER versions and panics. We use its struct layouts only as reference and write our own init-wait and main-thread hook | [SOURCE: T-005] |
| NR Smithbox `TEAM_TYPE` enum (community-curated): 1 Player, 2 White Ghost, 3 Black Ghost, 4 Grey Ghost, 6 Enemy, 7 Boss, 13/16 Invader, 29 Indiscriminate, plus NR-only 77 Near Death, 80 Balancer Raid. Conflicts with the EMEDF labels on 16–24 | [SOURCE: Smithbox PARAM/NR/Param Enums/TEAM_TYPE.json; T-005] [HYPOTHESIS: hostility] |
| ER arena (quickmatch) assigns PvP teams per slot via SpEffects 1110/1111. This is the closest precedent for symmetric team PvP | [SOURCE: T-005] |
| Nightreign vanilla has no PvP; the director stated none was planned | [SOURCE: [gfinityesports](https://www.gfinityesports.com/article/no-pvp-messages-spirit-ashes-in-elden-ring-nightreign-according-to-director)] |

### Core hypothesis (H1), to be tested by E-003

> Because NR inherits ER's invasion-era combat netcode, **giving opposing players hostile team types on
> all clients** is enough for hits to register and sync through SC's existing networking, with no
> networking changes.

Sub-questions E-003 must answer:
- Does the change need to happen on the attacker's client, the victim's client, or both? This tells us
  the damage authority model.
- Which team-type values are hostile to a player? Get candidates from Paramdex/Smithbox NR enum names for
  NpcParam `teamType`. Do not guess the numbers.
- Does the engine or SC reset team type (on spawn, day change, death, or revive)? If it does, we need a
  hook or a periodic re-apply.
- Is `ChrTemporaryTeamTypeSlot` a cleaner override mechanism than writing `team_type` directly?

### Alternative mechanisms (if H1 fails or is unstable)

- **A1, param:** an SpEffect with `change_team_type`, applied to a player by our DLL or by EMEVD. Engine-native override, so a good counter if `team_type` keeps getting reset. Bullet `isHitBothTeam` only covers projectiles.
  Ships a modified `regulation.bin` via a me3 `[[packages]]` folder. Watch for SC regulation checks.
- **A2, EMEVD:** `SetCharacterTeamType` exists in NR (2004[2]); DarkScript3 3.6.3 supports NR. Needs a check that it can target player entities and runs per peer. EMEVD file overrides via me3 are undocumented.
- **A3, damage hook:** intercept the hit-validation or team-relation check and force "hostile" for
  player pairs on different gameplay teams. This is the most invasive option, so use it last.

---

## 5. Design decisions log

| ID | Decision | Status |
|---|---|---|
| ADR-001 | Rust cdylib + me3 + fromsoftware-rs reference (§2) | Pending review |
| ADR-002 | One SC party; hostility via engine team types only (§3) | Working assumption |
| ADR-003 | Debug control via hotkeys + config + log file first; overlay only if needed | Accepted |
