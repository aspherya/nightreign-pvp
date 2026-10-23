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
| Toolchains present | Rust stable 1.99.0 (rustup, MSVC), Python 3.12.10 (not on PATH; capstone, pefile), `gh`, VS 2022 Community 17.14, CMake, vcpkg (`C:\vcpkg`), .NET SDK, git, Node | [VERIFIED: 2026-10-20] |
| RE and modding tools | Cheat Engine; WitchyBND 3.0.1 and the game 1.03.3 unpack in `Z:\Tools\`; Nucleus Co-op in `D:\Programs\Nucleus` | [VERIFIED] |
| me3 loads our native DLL into NR 1.3.3.0 (no SC), no crash at startup, no `disable_arxan` needed for a passive DLL | [VERIFIED: T-001 HUMAN-solo run, dist/nrpvp.log 2026-10-09] |
| Saves | `%APPDATA%\Nightreign\76561198342403479\NR0000.sl2`, no `.co2` yet (SC not run yet) | [VERIFIED] |

When the game or SC updates, re-hash, update this table, and re-run the regression tests in TESTING.md.

---

## 2. Stack decision (ADR-001, accepted)

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
| `windows` 0.62 | Win32 APIs (DllMain, threads, memory, debug registers, named pipe, security descriptors) |
| `tracing`, `tracing-subscriber`, `tracing-appender` | File logging (+ the in-memory tail the console's `log` reads) |
| `serde`, `toml` | `nrpvp.toml` config |
| `from-singleton` 3.0.1 | FD4 singleton call-site patterns (validated on NR by T-006) |

As used on 2026-10-20 (initially planned `retour`, `pelite` and `fromsoftware-shared` were never needed; `hudhook` (ImGui overlay) was
used from T-013 and removed by R5 when the console replaced the panel). `nrcon`, `nrscan` and the launcher use std (+ `windows` for
`nrscan`).

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
| Our DLL can use Steam's flat API in the SC process without initialising Steam, and can register its own listen-only callbacks. A Steam lobby (limit 6, locked, data `nrsc_lobby_type`, `nrsc_lobby_version`, `inviteParam`) appears some time after the Roundtable loads, and a second unlocked lobby (only `inviteParam`) shortly before the expedition loads. Details and open points: `research/S9-matchmaking-decision.md` §5a | [VERIFIED: MM-1 solo run 2026-10-12, `experiments/MM-1/`] |
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

**Every memory location (offsets, layouts, team values, downed block, HP, flags) is owned by [re/MEMORY_MAP.md](re/MEMORY_MAP.md)**
since 2026-10-20. Code comments that say "TECHNICAL_NOTES §4" refer to facts that now live there (§2 players, §3 ChrIns, §4 team
values, §5–6 HP and downed state). The full former table is in git: `git show 339ee04:.claude/TECHNICAL_NOTES.md`.

Facts that are not memory locations:

| Finding | Tag |
|---|---|
| **H1 holds:** giving rivals a hostile team type on every client is enough for synced two-way PvP through SC's networking, with no networking changes. Hostility is decided per client from that client's own copies of `team_type` | [VERIFIED: FS-0 runs 2 and 4, FS-1 over real Steam, 2026-10-10] |
| RTTI has `ChrTemporaryTeamTypeSlot`, `EcTestTeamType`, `CSSessionManagerImp`, `CSNetManImp`; `CSTeamType*` classes are absent, so the team-relation code may differ from ER | [VERIFIED: RTTI string scan] |
| `team_relation.rs` (fromsoftware-rs) is not a hostility matrix; the team_type × team_type table is unmapped | [SOURCE: T-005] |
| Param and event-script routes to hostility (`SP_EFFECT_PARAM_ST.change_team_type`, EMEDF `2004[2] SetCharacterTeamType`, `dmg_level_vs_player`, `is_hit_both_team` on bullets only) | [SOURCE: `research/T-011-alt-hostility.md`] |
| The `eldenring` crate cannot run on NR (its RVA table checks ER versions and panics); its layouts are hints only | [SOURCE: T-005] |
| SC binds F4 to its pause; our keys must avoid it | [VERIFIED: TC-004 run 1] |
| Nightreign vanilla has no PvP; the director stated none was planned | [SOURCE: [gfinityesports](https://www.gfinityesports.com/article/no-pvp-messages-spirit-ashes-in-elden-ring-nightreign-according-to-director)] |

### 4b. The hawk ride is a player behavior state (HKS)

Sources: the player action script `action/script/c0000.hks` as plain Lua text, (1) shipped by the Nexus mod "Spectral hawk anywhere"
(`Z:\Tools\nr-mods\Spectral haw flying\mod\`, study only: its permissions forbid modifying or re-uploading it); (2) the community
decompile [El-Fonz0/EldenRingNightreignHKS](https://github.com/El-Fonz0/EldenRingNightreignHKS) at `b2ccd62` ("Update for 1.03.1") and
`197b182` ("changes from 1.03.4"). The mod's file is that decompile (its "Version 11, 1.03.1") plus a whole mod pack's edits (Daybreak
Team), not vanilla plus one line. **All 24 hawk functions are identical in the 1.03.1 and 1.03.4 decompiles and in the mod, except
the one 707144 line** `[VERIFIED: function-by-function diff, 2026-10-21; scripts and route dump in experiments/HAWK-0/]`. So every row
below is vanilla behaviour, and the hawk code did not change between patches.
**Warning:** that mod pack's script calls `os.execute(...)` to open web pages, so a mod's action script can run programs on the PC.
Never install a script mod we have not read.
Which decompile matches our exe (1.3.3.0) exactly is not proven (string and number fingerprints did not separate 1.03.1 from 1.03.4).
It only matters if we ship our own `c0000.hks`.

| Finding | Where in that file |
|---|---|
| Both hawks are the same state family of the player character, not a separate vehicle. The engine tells the script which ride starts through `env(GetBirdEventID)`: **1 = tree hawk** → `W_BirdAct_MoveStart`; **4 = start hawk** → `W_Departure_MoveStart` (`ExecBirdAct`) | `ExecBirdAct` |
| **The mod's whole trick is one added line:** if the player has SpEffect **707144**, start `W_BirdAct_MoveStart` (a tree-hawk ride anywhere). 707144 is not set by the mod's event script, so its regulation.bin applies it (probably through an item); not checked | `ExecBirdAct` |
| **Start hawk loop (`Departure_MoveLoop_onUpdate`) reads no input.** It ends only when the engine sets `GetBirdEventID == 2` (end of the route) → `W_BirdAct_SeparationFallStart`. This is why the start hawk cannot be left early | `Departure_MoveLoop_onUpdate` |
| **Tree hawk loop exits:** hold the action button (`ACTION_ARM_ACTION`) > 500 ms → `W_BirdAct_FallStart`; **L3** → `W_BirdAct_SeparationFallStart`; off the route (`GetBirdPathEscapeTime`) ≥ 10 → SpEffect 102615, ≥ 2000 → fall; `GetBirdEventID == 2` → separation fall. SpEffect **99230** turns the exits off and the speed up to ×1.3 | `BirdAct_MoveLoop_onUpdate` |
| Steering on a hawk: the engine reports `GetBirdMovementDirectionTime(0..3)` (up, down, left, right); the script feeds them to blend variables | `BirdActCommonFunction` |
| Shared rules in `BirdActCommonFunction`: `GetBirdActionID == 1` → separation fall; `== 2` → fall end; dying on the hawk → separation fall; landing → `W_BirdAct_FallEnd` / `W_Departure_FallEnd`; damage type "death by falling" → `W_FallDeath` (the void) | `BirdActCommonFunction` |
| The start-hawk fall has its own states (`Departure_FallStart/FallLoop/FallEnd`, a 180° turn `W_BirdDepartureFall180` under SpEffect 102610) and a 100 s safety timer that forces a separation fall | `Departure_*`, `ExecDashTurn` |
| The game loads `c0000.hks` as **plain Lua source** from a mod package (the Nexus mod ships text, not bytecode; vanilla 1.03.3 ships bytecode) | file header; `Z:\Tools\nr-unpacked\1.03.3\action\script\c0000.hks` starts with `LuaQ` |
| `BirdActCommonFunction` (run every frame by every hawk state, the start-hawk loop included) calls `ExecBirdAct()`, so a 377 answer of 1 or 4 **mid-ride** starts that ride type at once. `BirdAct_MoveStart` (the tree ride's start animation) moves to `W_BirdAct_MoveLoop` when `IsAnimEnd(0)` (env **339**) or `GetEventEzStateFlag(0)` (env **301**) is TRUE. SpEffect **102615** (added every frame off the route) is only a 0.05 s VFX (4050), no movement: the descent off the route is engine-side | studied script; SpEffectParam row via WitchyBND (2026-10-22) |
| Engine ids of the hawk values (the number the script passes to `env`): `GetBirdEventID` = **377**, `GetBirdPathEscapeTime` = 384, `GetBirdMovementDirectionTime` = 394, `GetBirdActionID` = **395**. The exe code that answers these ids is where the engine keeps "which ride / route ended" (needed for A2, PROJECT_PLAN §13) | decompile constants (`197b182`) |
| Numbers in the compiled vanilla script are stored as big-endian f32 | `vanilla c0000.hks` byte search |

### 4c. Hawk routes in the map data

`[VERIFIED: read from the 1.03.3 unpack, 2026-10-21; tools and full dump in experiments/HAWK-0/]`

| Finding | Detail |
|---|---|
| All hawk routes are in one map-wide file, **`map/mapstudio/m60_00_00_99.msb.dcx`** (`_98` holds the 3 routes of the other map variant, landing rows 110xxxxxx) | `SORTIE_STARTING_POINT_ST.birdRouteEntityId` values are found only there |
| A route is an EVENT entry of **type 25** whose type data holds 4 ints (0, 0, 6001, 0 for start routes; 7601–7605 for tree routes: probably gating flags, UNKNOWN) and then a list of **POINT indices** (i16, ended by -1) | Names: `出撃ルート` = sortie (start hawk) routes, 29 (20 used by the landing table, 9 marked unused); `鳥移動ルート` = bird-move routes (the Spectral Hawk Tree roads), 11; `鳥移動_レア` = rare variants, 5 |
| A route point is a POINT entry: position x, y, z (f32) at **+0x14**, heading (deg) at **+0x24**. Point names end in `出撃` (start), `通常N` (waypoint N), `終了` (end) | e.g. route `00_000`: (-205.6, 238.5, 845.2) → (-85.0, 218.4, 783.9) → (-23.9, 160.7, 754.5) |
| **Every start route is 3 points, 190–203 m** (matches the ~210 m ride of T-028 run 4). Tree routes: 6–11 points, 240–760 m. An unused rare route has 25 points over 1,465 m, so long many-point routes exist in the data | `routes_m60_00_00_99.txt` |
| Frame of this file: tiles are **256 m**. The box guess "Limveld spans x −334..1202, z 256..1792" is **wrong at the edges**: HAWK-1b run 1 ended a route at (1052, 1134) inside it and the player fell into the void. Known land (every vanilla start-route end + every Spectral Hawk Tree route point, 167 points) spans x −24..1032, z 467..1541; `patch_routes.py` uses their convex hull | `[VERIFIED: HAWK-1b run 1, 2026-10-22]` |
| A patched `m60_00_00_99.msb.dcx` in an me3 package **is loaded**: the start hawk flies the moved points in a straight line at 17.4 m/s, holds the point height exactly, and the engine's route end starts the drop | `[VERIFIED: HAWK-1b run 1, TESTING §3]` |
| World frame (`Physics+0x70` + origin) vs this file: a ride runs in the **opposite** direction (180° about the vertical; 2 rides). The offset is **not** global: run 1 fit world = (647.5 − x, y, 1404 − z) and predicted run 2 ride A within 10 m (same route), but ride B (route 01_000) was off by (789, 261) m, so the origin is per area | `[VERIFIED: rotation, HAWK-1b; offset per area: HYPOTHESIS]` |
| Each landing row also names a `spawnPointEntityId`: a POINT named `出撃ルートNN 復帰用ポイント①` ("return point") in the tile file `m60_XX_YY_00/10/20`, not the landing spot | tile MSBs |
| **Tile frame → this file's frame** (Limveld base tiles `m60_XX_YY_00`, X 41..46, Y 35..39): **x = (X−42)·256 + 128 + local x, z = (Y−34)·256 + 128 + local z, y unchanged** | `[VERIFIED: static fit, 2026-10-22, experiments/HAWK-0/fit_frame2.py]`: 7 of the 9 start-route return points (`出撃ルートNN 復帰用ポイント①`) lie within 14 m of the predicted landing (route end + 52 m forward); heights agree (route end ≈ 39 m above its return point) |
| Ground and the playable island | Every part of the base tiles gives a height sample (`ground_map.py` → `ground_cells.json`, 16 m cells; `hawk_atlas.png` draws land, all routes, drops, falls). Land in tile rows Y 36..39: **x 16..1008, z 528..1520** (≈ 1 km square). Layers `_10.._50` are the Shifting Earth variants (the `_40` crater floor sits at y −668); clearance must use the highest variant (`diagonal.py`) |
| **The crossing line** (HAWK-1b run 3) | Every start route set to (858, 210, 678) → (512, 250, 1024) → (166, 448, 1370), 979 m: the game spawns the player on the hawk at the moved start, flies the line the same way every time, and the drop from y 448 (≈ 250 m above the ground) does no damage | `[VERIFIED: HAWK-1b run 3, 3 rides, Leonardo]` |
| HAWK-1b falls on the atlas | Run 1 (1052, 1134): open sea, the void. Run 2 ride B (650, 549): **over land** (ground y ≈ 78), about 20 m inside the island's bottom edge, yet the player fell to y −26: ground not solid there at that moment (not loaded, or a cliff lip) `[HYPOTHESIS]`. Keep route ends ≥ 150 m inside the land box | `[VERIFIED: static, atlas]` for the positions |
| WitchyBND 3.0.1 decrypts `regulation.bin` and writes NR params as XML, but cannot serialize NR MSBs; `msb.py` walks them (sections: MODEL, EVENT, POINT, ROUTE, LAYER, PARTS, version 80) | `experiments/HAWK-0/` |

### 4d. NPC bots: the game's own AI Nightfarers (OFF-3)

`[VERIFIED: static, 1.03.3, 2026-10-22]` unless tagged. Full memo: `research/OFF-3-bots.md`; addresses: `re/MEMORY_MAP.md` §15.

| Finding | Detail |
|---|---|
| Event instruction **`2003[97]`** (one i32: `NPCBotTableParam` table id) | Arms `CSNpcBotMan` with every row of that table (NpcParam, NpcThink, CharaInit); the manager's update then spawns them. Does nothing when the solo gate count is above 1 |
| Event instruction **`2003[98]`** (no args) | Removes every spawned bot and clears the table |
| No shipped event script uses 97 or 98 (279 `.emevd` scanned; bank 2003 ids 0..130 occur) | The system is dormant in vanilla play |
| Bot data | `NPCBotTableParam` tables 10..80 (2 heroes each, level 1) and 10000 / 20000 / 30000 (8 heroes, levels 6 / 11 / 13); `NpcParam 600040000` team 2 (ally); `NpcThinkParam 600040000` logic 30000 (follow the host) + battle 29999 (`Common_NPC_AI`: plays a Nightfarer: skill, ultimate, items, revives) |
| Hostile cousins | `SmallbaseInvationNpcParam`: Nightfarer-style invaders at small bases (team 6, logic 31000 = guard home, same battle AI) |
| In game (OFF-4 run, 2026-10-22) | `2003[97]` from our `common.emevd` event spawns the bots in an expedition: they appear, follow the host and fight enemies; the host cannot hurt them (team 2, allies). `2003[98]` removes them. **At most 2 bots** per manager (list sized to 2 in the constructor; table store holds 2 rows): the vanilla design is solo player + 2 AI teammates `[VERIFIED: OFF-4 run + static]` |
| Custom AI logic | The game loads `c0000.hks` as plain Lua from a package (§4b), so a plain-text roaming logic script for bots may load the same way `[HYPOTHESIS]` |

### 4e. The night and its boss in the event scripts (M5-1)

`[VERIFIED: static, the 278 real 1.03.3 event files decoded, 2026-10-22]`. Full memo: `research/M5-no-boss-nights.md`; the clock's
memory: `re/MEMORY_MAP.md` §16.

| Finding | Detail |
|---|---|
| In game (M5-2 run, 2026-10-22) | After `clock end`: 7515 + 7501 ON 0.4 s later; 120 s later 7504 and the boss map's `49240200` / `49240201` (`m49_24`) ON and the boss appeared, as read |
| The night boss lives in its own map | 36 boss maps `m47_70`..`m52_13`, one per boss. 35 start the common events **`90055000`** (minions; 7501 / 7506) and **`90055001`** (sets the map's `XXXX0200` "fight starts" when the minions die or 120 s pass). The boss is enabled only on `XXXX0200` |
| "Night N done" = flags **7502 / 7507** | Set only by the boss-defeat handler `90065900` and by `common` 1190 / 1191 (any boss map's `XXXX0800` / `XXXX0850`). No timer sets them |
| Everything after hangs off 7502 / 7507 | Flask refill (1303 / 1304), interval 7503 / 7508 (1100), interval grace (`90005580`), Day 2 = `Reset Time of Day` 20 s after 7503 (1103), Day 3 warp after 7508 (`90005910`) |
| Night flag 7515 | `common` 1107: on while the play-area clock reads 23:59, which the engine holds once reached (MEMORY_MAP §16) |
| No event instruction sets the clock to a chosen time | `2003[89] Reset Time of Day` always goes to the morning; its hour arguments are unused |
| Night freeze (M5-4) | `common` 1301: local flag **9015** while night (7515) and the local player is inside the play area (and on Day 3); 1302: 9015 → SpEffect **102120** on the local player (bleed-out held at 15 s, MEMORY_MAP §6), 9015 off → 102121. Our event file empties 1302 (M5-5) `[VERIFIED: static + Nucleus run 2]` |
| Alive flags and the wipe | 1182: each game keeps its own player's flag 7005–7007; at night it drops when the player is downed **or its team is not 1 / 2 / 20 / 29** (team 6 = out); 1180 (multiplayer) ends the expedition for all when all three are off. Our event file empties 1180 (M5-5) `[VERIFIED: static + Nucleus runs 1, 2, 4]` |
| The morning reaches everyone from the host | A DLL write of 7502 in the host stays local, but the host's 1100 then sets **7503 / 7505 network-wide** and both games reach Day 2 `[VERIFIED: Nucleus run 2]` |
| Endings are per game | 7000 / 7001 are plain local flags; set in two games at once, each played its own result (the 7000 game the first-win cutscene) and both were consumed within 5 s `[VERIFIED: Nucleus run 2]` |
| Our decoder | `experiments/M5-1/emdec.py` (any NR `.emevd`; names and argument layouts from DarkScript3's `nr-common.emedf.json`) |

---

## 5. Design decisions log

| ID | Decision | Status |
|---|---|---|
| ADR-001 | Rust cdylib + me3 + fromsoftware-rs reference (§2) | Accepted (in use since 2026-10-09) |
| ADR-002 | One SC party; hostility via engine team types only (§3) | Working assumption |
| ADR-003 | Debug control via hotkeys + config + log file first; overlay only if needed | Superseded by ADR-004 |
| ADR-004 | Developer control through a text console (named pipe + `nrcon`, usable by agents); the ImGui panel and F-keys go away. `CONSOLE.md` | Accepted 2026-10-20 |
| ADR-005 | Code structure: ports and adapters for game memory and Steam, typed memory views, feature modules, a per-tick read model, dev code behind a cargo feature. `ARCHITECTURE.md` | Accepted 2026-10-20 |
| ADR-006 | One owner doc per topic; the doc map in `CLAUDE.md` is the index | Accepted 2026-10-20 |
