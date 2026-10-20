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
