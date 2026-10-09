# Technical Notes

Tag legend: `[VERIFIED: how]` checked on this machine or in-game · `[SOURCE: link]` from external
documentation or code, not yet checked here · `[HYPOTHESIS]` unproven, needs an experiment · `UNKNOWN`.

---

## 1. Environment and version pins (as of 2026-10-09)

| Component | Value | Evidence |
|---|---|---|
| Game | `nightreign.exe` FileVersion **1.3.3.0**, Steam buildid **22818764**, SHA256 prefix `9545E8732919BB6A` | [VERIFIED: PE version info, appmanifest_2622380.acf, Get-FileHash] |
| regulation.bin | SHA256 prefix `876A3CA279A4561D`, dated 2026-04-01 | [VERIFIED] |
| Seamless Co-op | `nrsc.dll` SHA256 prefix `243EEC929A97B71E`, installed 2026-10-09. No PE version resource; the version number is UNKNOWN (see T-003) | [VERIFIED] |
| SC launcher | `nrsc_launcher.exe` SHA256 prefix `2726DEA099A54F00` | [VERIFIED] |
| me3 | 0.11.0 (2026-02-28) at `Z:\Tools\me3` | [VERIFIED: `me3 --version`] |
| Toolchains present | VS 2022 Community 17.14, CMake, vcpkg (`C:\vcpkg`), .NET SDK, git, Node | [VERIFIED] |
| Toolchains missing | Rust (rustup), Python, `gh` CLI | [VERIFIED] |
| RE tools present | Cheat Engine | [VERIFIED] |
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
| Yui removed the game's player cap and has mentioned PvPvE-style modes as a *possible* future, while also saying there are no plans for PvP or matchmaking | [SOURCE: [rutab.net](https://rutab.net/b/video-games/2025/08/05/mod-dlya-elden-ring-nightreign-teper-podderzhivaet-kooperativ-do-6-igrokov.html)] |
| Networking uses **Steam** (`SteamNetworkingMessages002`, `SteamMatchMaking009`), not EOS | [VERIFIED: import strings in nrsc.dll] |
| Exports or expects `modengine_ext_init`, and says *"If you're using ModEngine, please make sure seamless co-op is loaded early"* | [VERIFIED: strings in nrsc.dll] |
| Config keys are **not stored as plain text**, so hidden options can't be found statically (even `health_scaling` isn't a literal) | [VERIFIED: string search] |
| `nrsc_settings.ini` has an **empty `[GAMEPLAY]` section**. In ER SC this section held PvP and invasion options. NR keys UNKNOWN | [VERIFIED] [HYPOTHESIS: keys exist] |
| nrsc.dll RTTI references game classes `EcTestTeamType`, `EcTestChrDead`, `EcTestPlayerNo`, `PlayerNetworkSession`, `PartyMemberInfo`, `CSQuickMatchLobbyContext`. SC touches team-type event checks and party membership | [VERIFIED: RTTI strings] |
| ER SC had in-game PvP and friendly-fire toggles (the Judicator's Rulebook item). NR SC equivalent UNKNOWN | [SOURCE: ER community] |
| me3 + SC: me3 v0.5.0 added "Support loading mods that rely on modengine_ext_init" / "Support Seamless Coop". An earlier NR-specific report (me3 discussion #118, 2025-06-09) was **unresolved**. Current behavior on 0.11 is UNKNOWN (T-004) | [SOURCE: me3 CHANGELOG; [discussion #118](https://github.com/garyttierney/me3/discussions/118)] |

**Party vs team:** SC's *party* is the networking group (everyone in the lobby). Our *teams* are a gameplay
concept (who may damage whom). Phase 0 assumes everyone shares one SC party and hostility comes only
from engine team types. No separate team system until an experiment proves one is needed.

---

## 4. Engine findings relevant to PvP

| Finding | Tag |
|---|---|
| `nightreign.exe` RTTI has `ChrIns`, `PlayerIns`, `WorldChrManImp`, `CSSessionManagerImp`, `CSNetManImp`, `EcTestTeamType`, `EcTestChrSosInvadeType`, and **`ChrTemporaryTeamTypeSlot`** | [VERIFIED: RTTI string scan] |
| `CSTeamType*` class names were **not** found as RTTI in NR, so the team-relation implementation may differ from ER. UNKNOWN | [VERIFIED: absent from scan] |
| NR params keep PvP damage scaling: `ATK_PARAM_ST.dmg_level_vs_player`, `EQUIP_PARAM_WEAPON_ST.vs_player_dmg_correct_rate_*`, and an `is_hit_both_team` flag | [SOURCE: fromsoftware-rs `nightreign/src/param.rs`] |
| In ER, `ChrIns.team_type: u8` decides hostility through the team-relation table. Players become hostile when their team types are opposed (host vs invader) | [SOURCE: fromsoftware-rs eldenring `chr_ins.rs`, `team_relation.rs`] |
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

- **A1, param:** an SpEffect or Atk param route (`is_hit_both_team`, team-change effects if present in NR).
  No code needed, but it ships a modified `regulation.bin` through me3.
- **A2, EMEVD:** an event instruction that sets a character's team type (ER has one). Needs DarkScript3
  NR support.
- **A3, damage hook:** intercept the hit-validation or team-relation check and force "hostile" for
  player pairs on different gameplay teams. This is the most invasive option, so use it last.

---

## 5. Design decisions log

| ID | Decision | Status |
|---|---|---|
| ADR-001 | Rust cdylib + me3 + fromsoftware-rs reference (§2) | Pending review |
| ADR-002 | One SC party; hostility via engine team types only (§3) | Working assumption |
| ADR-003 | Debug control via hotkeys + config + log file first; overlay only if needed | Accepted |
