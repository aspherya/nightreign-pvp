# T-005: ER engine reference for team type, players, and sessions

Cheat sheet for the NR offset hunt (T-006, T-007). **Nothing here is verified for Nightreign 1.3.3.0** unless it carries a
`[VERIFIED]` tag that says so. Every ER layout below is a **starting hint**, not an NR fact.

## 0. Pins, tag conventions, how offsets were obtained

| Item | Value |
|---|---|
| fromsoftware-rs (`fsrs`) | main @ `59fbd3b3b7daaf14aca47c9f73530493dba6bc79` (2026-09-20, "Merge pull request #325 from vswarte/fix/from-static") [SOURCE: https://api.github.com/repos/vswarte/fromsoftware-rs/commits/main] |
| `from-singleton` crate | v3.0.1, repo main @ `2cd26853780f63a3aa1274d5ec27c2a83d4d8b2b` (2025-11-28) [SOURCE: https://api.github.com/repos/Dasaav-dsv/from-singleton/commits/main] |
| Paramdex | master @ `ff7245e524329bc3eab00036723d2bd53384cedf` (2026-03-06) [SOURCE: https://api.github.com/repos/soulsmods/Paramdex/commits/master] |
| Smithbox | main @ `cbd477a8fd6d436b3e011c8548e1de8fd8876918` (2026-10-01) [SOURCE: https://api.github.com/repos/vawser/Smithbox/commits/main] |

Tag shorthand. `fsrs:<path>#Ln` means `https://github.com/vswarte/fromsoftware-rs/blob/59fbd3b3b7daaf14aca47c9f73530493dba6bc79/<path>#Ln`.
All fsrs paths are under `crates/eldenring/src/` unless they start with `crates/`. So `cs/chr_ins.rs#L154` expands to
`https://github.com/vswarte/fromsoftware-rs/blob/59fbd3b3b7daaf14aca47c9f73530493dba6bc79/crates/eldenring/src/cs/chr_ins.rs#L154`.

**Offsets are derived, not copied from a doc.** fsrs documents no offsets in comments. I compiled a scratch copy of the crate at the
SHA above (stable rustc 1.99.0, `x86_64-pc-windows-msvc`) and printed `core::mem::offset_of!` / `size_of` for the structs below. The
scratch copy is outside the repo and unmodified apart from a throwaway `#[cfg(test)]` module, and the numbers agree with the
`unkNNN` field-name offsets the author embeds. They are tagged `[DERIVED: offset_of! on fsrs@59fbd3b]`. They describe the **ER
2.7.1.x layout** that fsrs targets (fsrs:rva.rs#L25-L26: supported ER versions 2.7.1.0 WW / 2.7.1.1 JP). Hint only.

A path-length gotcha for our own T-001 build: cargo's default `target/` under a very deep directory failed to link (LNK1104). Our repo
path is short, so this is unlikely to bite, but keep the target dir short if it does. [VERIFIED: happened in the scratch build]

---

## 1. Singleton discovery: what works on NR as is?

### 1.1 Where the finder lives

- The FD4 singleton finder is **not in fsrs**. It is the external crate `from-singleton` v3, a dependency of `fromsoftware-shared`
  [SOURCE: fsrs:crates/shared/Cargo.toml#L19] and re-exported as `fromsoftware_shared::FromSingleton`
  [SOURCE: fsrs:crates/shared/src/lib.rs#L36]. fsrs credits Sfix for the approach, Tremwil for the initial finder, Dasaav for the crate
  [SOURCE: fsrs:README.md#L21-L23].
- Exports a mod wants (all from `fromsoftware_shared`):
  - `FromSingleton` trait (supplies the lookup name) [SOURCE: https://github.com/Dasaav-dsv/from-singleton/blob/2cd26853780f63a3aa1274d5ec27c2a83d4d8b2b/src/lib.rs#L20]
  - `#[singleton("Name")]` attribute macro, which implements `FromSingleton` with the given DLRF name [SOURCE: fsrs:crates/shared/macros/src/lib.rs#L22]
  - `FromStatic` trait with `instance_ptr()`, `unsafe instance()`, `unsafe instance_mut()`, returning `InstanceError::{NotFound, Null}`
    [SOURCE: fsrs:crates/shared/src/static.rs#L11], [#L35], [#L85]. `NotFound` means the name is not in the scan result. `Null` means the singleton exists but is not instantiated yet.
  - `Program::current()` (PE view of the host exe via `GetModuleHandleA(NULL)`) [SOURCE: fsrs:crates/shared/src/program.rs#L20]
  - RTTI helpers `find_rtti_classes(program)` and `vftable_classname(program, vftable_va)` [SOURCE: fsrs:crates/shared/src/rtti.rs#L16], [#L82]
- **For T-006 "log every discovered singleton":** call `from_singleton::map()` directly. It returns `name -> address of the static pointer`
  for all found singletons, with no struct definitions needed [SOURCE: from-singleton src/lib.rs#L42]
  (https://github.com/Dasaav-dsv/from-singleton/blob/2cd26853780f63a3aa1274d5ec27c2a83d4d8b2b/src/lib.rs#L42).
  The value is a pointer to the static slot; read it once more to get the instance (null until the game creates it) [SOURCE: same file#L92-L98].

### 1.2 How it works (technique, for the fallback in T-006)

[SOURCE: https://github.com/Dasaav-dsv/from-singleton/blob/2cd26853780f63a3aa1274d5ec27c2a83d4d8b2b/src/find.rs]
- It pattern-scans the exe's `.text` for call-site shapes: `mov edx, imm32` plus a window of `lea rcx / r8 / r9` and `mov r9, rax` instructions
  (`candidates_iter`, find.rs#L404-L469).
  - `FD4Singleton` (plain): the shape is `mov reg,[rip+disp]` (the static slot) / `test` / `jne` / `lea rcx,[rip+disp]` (reflection object) / `call` (find.rs#L310-L378).
    The singleton's **name is obtained by calling a name-getter function** whose address is taken from that same `call`, on the reflection
    object. This **requires the DLRF reflection data to be initialised** (`finish()` is `unsafe` for this reason, find.rs#L207-L228).
  - `FD4DerivedSingleton`: same, but the name is a C string referenced by an `lea r9` (find.rs#L252-L308), so it needs no init.
  - The static slot must lie inside the `.data` section range (find.rs#L67, #L147).
- Runtime: `map()` returns only derived singletons while every found slot is still null; once any slot is non-null it assumes reflection is
  initialised and merges the full set into a cached map (lib.rs#L42-L62). So **call `map()` repeatedly after startup; an early call
  yields a partial list**.
- The wait-for-init in fsrs (`wait_for_system_init`) is ER-specific, see 1.3.

### 1.3 Is it game-agnostic? Can it work on NR's exe as is?

- **The scanner is game-agnostic by construction** (no RVAs, no version table, no product-name check). Its README lists tested games:
  DS3 1.15.0/1.15.2, Sekiro 1.06, ER 1.16, AC6 1.07.1 [SOURCE: https://github.com/Dasaav-dsv/from-singleton/blob/2cd26853780f63a3aa1274d5ec27c2a83d4d8b2b/README.md].
  **Nightreign is not in that list. Untested on NR: UNKNOWN until T-006 runs.** (Looked in: its README, `src/`; no NR issue/changelog text seen.)
- **The rest of the `eldenring` crate is NOT usable on NR as is.** Concrete blockers:
  - `eldenring::rva::get()` panics unless the exe's ProductName normalises to `"elden ring"` and version is 2.7.1.0 (EN) or 2.7.1.1 (JP)
    [SOURCE: fsrs:rva.rs#L21 (NAME), #L44-L50 (panic on error); fsrs:crates/shared/src/game_version.rs#L61 (name compare), #L33 (detect)].
    NR's ProductName is `ELDEN RING NIGHTREIGN` and FileVersion is `1.3.3.0` [VERIFIED: PE version info of `Z:\...\Game\nightreign.exe`, read-only].
    After the crate's normalisation (non-alphanumerics dropped, lowercased) that gives `elden ring nightreign`, which is not equal to `elden ring`, so detection fails. [DERIVED from the compare logic, not executed]
  - `CSTaskImp::wait_for_instance`, `run_recurring`, and `apply_speffect` use ER-only RVAs: `global_hinstance` 0x3d89708, `register_task` 0xeb3e50,
    `chr_ins_apply_speffect` 0x3e8dc0 [SOURCE: fsrs:rva/rva_ww.rs#L93, #L107, #L14; used at fsrs:util/system.rs#L28, fsrs:cs/task.rs#L179, fsrs:cs/chr_ins.rs#L340].
  - `GameMan`, `GameDataMan`, `MultiplayProperties`, `CharacterTypePropertiesTable`, `FieldArea` and others are **RVA-based, not name-based**
    [SOURCE: fsrs:cs/game_man.rs#L205, fsrs:cs/multiplay_type.rs#L296, fsrs:cs/game_data_man.rs#L132 (grep of `load_static_*`)].
  - The `nightreign` crate is only `pub mod param;`: param structs, no engine structs, no RVAs [SOURCE: fsrs:crates/nightreign/src/lib.rs#L1; fsrs:crates/nightreign/Cargo.toml#L8].
- **Implication for T-006.** The portion that can plausibly work unchanged is `from-singleton` (or `fromsoftware-shared`'s re-export of it). We must
  supply our own "game is initialised" wait (poll `from_singleton::map()` until a known name such as `CSTask` or `CSWindow` has a non-null slot) and our own NR-specific
  task hook if we want main-thread callbacks. Do **not** depend on the `eldenring` crate's `rva` module.
- **NR PE layout, which may affect a `.text`/`.data` by-name scan** [VERIFIED: read the first 4 KB of `nightreign.exe` headers on this machine, 9 sections]:
  `.text` (RVA 0x1000, size 0x2BB3C00), `.interpr` (0x2BB5000, 0xCE00), `.rdata` (0x2BC2000), `.data` (0x3985000, vsize 0xD87D0C),
  `.pdata`, `_RDATA`, `.rsrc`, `.reloc`, and a **second section also named `.text`** (RVA 0x4A94000, size 0x31C9600). Both finders pick sections by name
  (find.rs#L39-L47; rtti.rs#L20-L26 for `.text`/`.rdata`). Whether pelite's `by_name` returns the first or last of two same-named sections: UNKNOWN
  (looked at: our reading of `find.rs` only; pelite source not read). `[HYPOTHESIS]` the call sites may live in either `.text`, and the on-disk bytes may differ from the in-memory image
  (the repo notes already expect Arxan; me3 can neutralise it per TECHNICAL_NOTES §2). If `map()` is empty or tiny on NR, check this first.
- The mod pattern in the examples: a `cdylib` whose `DllMain` (on `reason == 1`) spawns a thread, waits for the task singleton, then registers a recurring closure on a
  task group and reads singletons inside it [SOURCE: fsrs:examples/apply-speffect/src/lib.rs#L9-L47; crate-type `cdylib` in fsrs:examples/apply-speffect/Cargo.toml].
  All five examples are ER-only [SOURCE: fsrs:examples/README.md]. `RecurringTask` fakes a native game task and fsrs only claims it works on DS3 and ER
  [SOURCE: fsrs:crates/shared/src/task.rs#L63-L68 doc comment]; NR: UNKNOWN. A read-only logger (T-008) can instead poll from its own thread, but then reads race the game
  (`FromStatic::instance` documents the main-thread requirement, fsrs:crates/shared/src/static.rs#L54-L55, #L70).
- **Name caveat for T-006.** The finder keys by **DLRF reflection names**, not RTTI class names. fsrs uses `"WorldChrMan"`, `"CSSessionManager"`, `"CSNetMan"`, `"CSTask"` [SOURCE: fsrs:cs/world_chr_man.rs#L15,
  fsrs:cs/session_manager.rs#L72, fsrs:cs/net_man.rs#L17, fsrs:cs/task.rs#L139], whereas our RTTI scan found `WorldChrManImp`, `CSSessionManagerImp`, `CSNetManImp`
  (TECHNICAL_NOTES §4). Expect the `*Imp` classes to be the concrete types behind the base-named singletons. Look up the unsuffixed names first. [HYPOTHESIS]

---

## 2. WorldChrMan to local player and other players (ER)

Singleton `WorldChrMan` [SOURCE: fsrs:cs/world_chr_man.rs#L15-L16]. All offsets [DERIVED: offset_of! on fsrs@59fbd3b, ER 2.7.1.x layout].

| Field | Offset | Notes |
|---|---|---|
| `main_player: Option<OwnedPtr<PlayerIns>>` | **+0x1E508** | Doc: "Points to the local player" [SOURCE: fsrs:cs/world_chr_man.rs#L58-L59]. `PlayerIns::local_player()` reads exactly this [SOURCE: fsrs:cs/chr_ins.rs#L948-L957] |
| `sinner_hunter` | +0x1E510 | chr id 30000 player (Spears of the Church boss) [SOURCE: fsrs:cs/world_chr_man.rs#L60-L63]. ER-specific |
| `player_chr_set: ChrSet<PlayerIns>` | **+0x10EE0** | "ChrSet holding the players" [SOURCE: fsrs:cs/world_chr_man.rs#L40-L41] |
| `ghost_chr_set` | +0x10F38 | bloodmessage/bloodstain/replay ghosts [SOURCE: same file#L42-L43] |
| `chr_set_holders[196]` / `chr_sets[196]` | +0x1CC60 / +0x1DED8 | all ChrSets by index [SOURCE: same file#L53, #L55] |
| `net_chr_sync` | +0x1E5E0 | see section 5 [SOURCE: same file#L77] |
| `chr_inses_by_distance` | +0x1F1D0 | `DLVector<{ChrIns*, f32 distance}>` sorted by distance to main player [SOURCE: same file#L99-L100]. Useful for the "nearest N enemies" hotkey (T-008) |

`ChrSet<T>` [SOURCE: fsrs:cs/world_chr_man.rs#L327-L345], size 0x58 [DERIVED]:
`+0x00 vftable`, `+0x08 index (i32)`, `+0x10 capacity (u32)`, `+0x18 entries (ptr to ChrSetEntry array)`, then two `DLMap`s (entity-id and group-id). Iteration
(`characters()`, #L365-L383) is: walk `entries[0..capacity]`, each `ChrSetEntry` is 0x10 bytes `{ chr_ins: *T @+0, chr_load_status: u8 @+8, chr_update_type: u8 @+9, entry_flags: u8, pad }`,
skip null `chr_ins` [SOURCE: fsrs:cs/world_chr_man.rs#L386-L395, #L365-L383; size DERIVED].
- `ChrLoadStatus`: 0 Unloaded, 1 Initializing, 2 **Active**, 3 NetworkInitializing, 4 ReadyForActivation, 5 Unloading [SOURCE: same file#L399-L406].
- `ChrUpdateType`: 0 **Local**, 1-3 Unknown, 4 **Remote** [SOURCE: same file#L410-L416]. Remote players' `PlayerIns` are therefore expected with `chr_update_type == 4`. [HYPOTHESIS for the mapping; the enum is the only source]

**So "the other players" in ER = every non-null `ChrSetEntry.chr_ins` of `WorldChrMan.player_chr_set`**, and the local one is also `main_player`.
Lookup by handle: `chr_sets[handle.selector.container()]` then the ChrSet vftable's `get_chr_ins_by_handle` [SOURCE: fsrs:cs/world_chr_man.rs#L122-L127].
`FieldInsHandle { selector: u32 (bits 0-19 index, bits 20-27 container = ChrSet index), block_id: BlockId(i32) }`, 8 bytes [SOURCE: fsrs:cs/field_ins.rs#L25-L35, #L72-L75].

**NR hunt hints** (all `[HYPOTHESIS]`): (a) `main_player` is a raw pointer in WorldChrMan; scan WorldChrMan for a pointer whose target starts with the `PlayerIns` vftable
(RTTI `PlayerIns` is present in NR). (b) the players' ChrSet vftable is `ChrSet`'s; its `+0x10` capacity should be a small number and `+0x18` should
point to an array of 0x10-byte entries whose first qword is a ChrIns pointer. In a solo SC session expect exactly one non-null entry. (c) NR's WorldChrMan is
certainly not the same size; do not trust the absolute offsets, only the shape.

---

## 3. `team_type` in ChrIns, and how team relations are evaluated (ER)

### 3.1 Location

`ChrIns.team_type: u8` at **ChrIns+0x6C**; `PlayerIns` embeds `ChrIns` at +0 so it is also **PlayerIns+0x6C** [SOURCE: fsrs:cs/chr_ins.rs#L154; fsrs:cs/chr_ins.rs#L871 (`PlayerIns.chr_ins` first field); offset DERIVED].
Neighbouring layout, for pattern-matching in CE [SOURCE: fsrs:cs/chr_ins.rs#L126-L156; offsets DERIVED]:

| Off | Field |
|---|---|
| +0x00 | vftable |
| +0x08 | `field_ins_handle` (8 bytes) |
| +0x10 | `chr_set_entry` (back-pointer to its ChrSetEntry) |
| +0x30 / +0x34 / +0x38 / +0x3C | `block_id`, `block_id_override`, `block_origin_override`, `block_origin` |
| +0x50 / +0x58 | `chr_model_ins`, `chr_ctrl` |
| +0x60 | `npc_param_id` (i32), for enemies = the NpcParam row id |
| +0x64 | `npc_id` (i32, e.g. 8000 Torrent) |
| +0x68 | `chr_type` (i32 enum, -1 None, 0 Local, 1 WhitePhantom, 2 Duelist, 3 Ghost, 5 Npc, 8 GrayPhantom, 13 Arena, ... 22) [SOURCE: fsrs:cs/chr_ins.rs#L1033-L1064] |
| **+0x6C** | **`team_type` (u8)** |
| +0x70 | `p2p_entity_handle` (8 bytes: block id + selector) |
| +0x178 / +0x188 | `special_effect` (SpEffect container ptr) / `character_id` |
| +0x1E4 | `network_authority` (u32) [SOURCE: fsrs:cs/chr_ins.rs#L236] |
| +0x308 | `chr_slot_sys` (0x40 bytes, opaque; see 3.4) |
| +0x580 | (PlayerIns only) `player_game_data` ptr. `ChrIns` size is 0x580; `PlayerIns` is 0x740 |

Cheap CE cross-check for NR: for an enemy ChrIns, `+0x60` should be a valid `NpcParam` row id, and `+0x6C` should equal that row's `teamType`
(NR NpcParam.teamType, section 4). That anchors the offset even if the surrounding layout shifted. [HYPOTHESIS: same in NR]

Other copies of team type that fsrs exposes (could be easier to find):
- `FeMan` HUD tag `team_type: u8` [SOURCE: fsrs:cs/fe_man.rs#L370]; offset not derived.
- `PlayerGameData.quick_match_team: u8` at +0xAA0, doc: "team type for quickmatch, WhiteSign (2) for allies" [SOURCE: fsrs:cs/player_game_data.rs#L200 (offset DERIVED)].
  This hints at a mapping of arena team assignment onto team-type values (WhiteSign = 2 matches "White Ghost" in the enum below).
- `NearEnemyFinder.team_type`: "Team type to be hostile against" [SOURCE: fsrs:cs/world_chr_man.rs#L653-L658]. `CSTargetingSystemOwner::get_team_type(out: &mut i8)` [SOURCE: fsrs:cs/targeting.rs#L22].
  The engine therefore has an "enemy search from team type X" query.

### 3.2 What `team_relation.rs` actually is (correction to TECHNICAL_NOTES §4)

`cs/team_relation.rs` defines four tiny polymorphic filter objects `CSTeamTypeNeutral / Friend / Enemy / Rival` with one vmethod
`validate(&self, info: &TeamRelationTargetInfo, self_target: bool) -> bool`, where `TeamRelationTargetInfo` is three bools
`{ oppose_target, friendly_target, self_target }`
[SOURCE: fsrs:cs/team_relation.rs#L26-L47, #L129-L133]. Enemy accepts `oppose_target`; Friend accepts `friendly_target`; Rival accepts either
[SOURCE: fsrs:cs/team_relation.rs#L74-L126]; Neutral accepts only self. They are static singletons with ER vftable RVAs 0x2a4f868..0x2a4f898
[SOURCE: fsrs:rva/rva_ww.rs#L68-L72].
- This is the **"what relation does this query accept"** layer. It does **not** contain the **team_type x team_type -> {oppose, friendly}**
  matrix, and nothing in fsrs shows how `oppose_target` / `friendly_target` are computed from two `team_type` bytes. **That matrix is UNKNOWN**
  (looked at: all of `cs/*.rs`, `crates/shared`, ER and NR param bindings; also one web search for a published hostility table, no hit).
- TECHNICAL_NOTES §4 row 3 ("`team_type` decides hostility through the team-relation table ... [SOURCE: ... `team_relation.rs`]") overstates what that file shows.
  `team_type` as the hostility key is plausible (the field name, `NearEnemyFinder.team_type`, and Paramdex's description, section 4.2) but the lookup table itself is not mapped.
- Consistent with TECHNICAL_NOTES §4 row 2: NR has no `CSTeamType*` RTTI, so NR's relation code may be a different structure. Fine: the matrix has to be found in NR directly (T-007), or
  inferred empirically (T-012).

### 3.3 How a team type gets assigned to players in ER (partial)

- ER has `ROLE_PARAM_ST` with a `team_type: u8` field [SOURCE: fsrs:crates/eldenring/src/param/generated.rs#L51745-L51746;
  https://github.com/soulsmods/Paramdex/blob/ff7245e524329bc3eab00036723d2bd53384cedf/ER/Defs/RoleParam.xml#L9-L11].
  fsrs computes a role param row id from `chr_type` and vow type in `calculate_role_param_id` [SOURCE: fsrs:cs/chr_ins.rs#L377-L390], so the engine plausibly sets a player's
  `team_type` from the row selected by role. The assignment code itself is not in fsrs. [HYPOTHESIS]
- **`ROLE_PARAM_ST` does not exist in the NR param set**: 0 matches in `fsrs:crates/nightreign/src/param.rs` (226 param structs) and no `Role*` file in `Paramdex/NR/Defs`
  [VERIFIED: grep of the fetched files]. So NR assigns player team types some other way (or the param was renamed). UNKNOWN. Relevant for H1: there may be no param row to edit, which makes the direct write route (T-008) more important.
- Quickmatch (ER arena) is the only fsrs-mapped team-based PvP: `CSQuickMatchingCtrl.quickmatch_team_types: [u8; 10]` "per-slot team assignment: indices 0-4 team-match, 5-9 brawl"
  at +0xC9 of the ctrl object [SOURCE: fsrs:cs/net_man.rs#L306-L308; offset DERIVED]. Team A/B are driven by SpEffects 1110 / 1111 "Team A/B Summon/Respawn" listed in `QuickmatchManager.utility_sp_effects`
  [SOURCE: fsrs:cs/net_man.rs#L183-L199 (doc comment from debug strings)]. ER precedent that "team assignment via SpEffect" is the production mechanism for symmetrical PvP. [HYPOTHESIS: that those SpEffects use `changeTeamType`]
  Whether NR retains any of this (NR RTTI had `CSQuickMatchLobbyContext` in nrsc.dll strings per TECHNICAL_NOTES §3): UNKNOWN.

### 3.4 `ChrTemporaryTeamTypeSlot`

**Not mapped in fsrs.** grep for `Temporary`, `TemporaryTeam` over all of `crates/` returns nothing. [VERIFIED: grep over the fetched tree at the SHA]
The only adjacent thing is the opaque `ChrIns.chr_slot_sys: [u8; 0x40]` at **ChrIns+0x308** [SOURCE: fsrs:cs/chr_ins.rs#L289; offset DERIVED] and a bare `ChrSlotBase { vftable }` with two
vmethods [SOURCE: fsrs:cs/chr_slot.rs#L5-L15]. `[HYPOTHESIS]` the slot objects (including `ChrTemporaryTeamTypeSlot`, whose RTTI exists in NR per TECHNICAL_NOTES §4) hang off that 0x40-byte
block in ER, and an "override team type" slot would be what SpEffect `changeTeamType` populates. Next step for T-007: in CE, take the RTTI vftable of `ChrTemporaryTeamTypeSlot`
from the NR exe, then search a PlayerIns for pointers whose target begins with that vftable.

---

## 4. Team-type enum for NR (the actual values)

### 4.1 Sources (precise files)

| Purpose | File |
|---|---|
| NR enum values for `NpcParam.teamType` | Smithbox `src/Smithbox.Data/Assets/PARAM/NR/Param Enums/TEAM_TYPE.json` @ cbd477a: https://github.com/vawser/Smithbox/blob/cbd477a8fd6d436b3e011c8548e1de8fd8876918/src/Smithbox.Data/Assets/PARAM/NR/Param%20Enums/TEAM_TYPE.json |
| NR enum for `SpEffectParam.changeTeamType` | Smithbox `.../PARAM/NR/Param Enums/SP_EFFECT_CHANGE_TEAM_TYPE.json` (https://github.com/vawser/Smithbox/blob/cbd477a8fd6d436b3e011c8548e1de8fd8876918/src/Smithbox.Data/Assets/PARAM/NR/Param%20Enums/SP_EFFECT_CHANGE_TEAM_TYPE.json) |
| ER equivalents (for diff) | same folder with `ER` in place of `NR` |
| Field definitions | Paramdex `NR/Defs/NpcParam.xml`, `NR/Defs/SpEffectParam.xml`, `NR/Defs/Bullet.xml`, `NR/Defs/NpcThinkParam.xml` @ ff7245e |
| Param bindings | fsrs `crates/nightreign/src/param.rs` (generated from the Paramdex defs, per its header) |

Paramdex itself has **no** enum values: its README says Tdfs ship officially only for Demon's Souls, and `NR/` has only `Defs` and `Names`, no `Tdfs`
[SOURCE: https://github.com/soulsmods/Paramdex/blob/ff7245e524329bc3eab00036723d2bd53384cedf/README.md]. Paramdex NR only names the enum: `NpcParam.xml#L735-L739` has `u8 teamType`, `<Enum>TEAM_TYPE</Enum>`
[SOURCE: https://github.com/soulsmods/Paramdex/blob/ff7245e524329bc3eab00036723d2bd53384cedf/NR/Defs/NpcParam.xml#L735].
Its description (Japanese, my paraphrase) says it sets whether the NPC's attacks hit or miss, and whether it targets or ignores [SOURCE: same file#L737-L739].
Smithbox's own glossary calls team type the alignment that decides "who its AI will attack, who it can damage, and who it can take damage from"; the value set varies per game
[SOURCE: https://github.com/vawser/Smithbox/blob/cbd477a8fd6d436b3e011c8548e1de8fd8876918/src/Smithbox.Data/Assets/Help/Glossary/Team_Type.json].

### 4.2 NR `TEAM_TYPE` values (Smithbox, community-curated, **not official**)

[SOURCE: Smithbox NR TEAM_TYPE.json above, all rows]. Labels are Smithbox's English; some are copies of DS3-era labels.

| Val | Label | | Val | Label | | Val | Label |
|---|---|---|---|---|---|---|---|
| -1 | No Change | | 14 | Neutral | | 29 | Indiscriminate |
| 0 | None | | 15 | Charmed | | 30 | Object |
| **1** | **Player** | | 16 | Invader | | 31 | Co-op Mad Phantom |
| 2 | White Ghost | | 17 | Watchdogs of Farron | | 32 | Invader Mad Phantom |
| 3 | Black Ghost | | 18 | Aldrich Faithful | | 33 | Arch Enemy Team |
| 4 | Grey Ghost | | 19 | Invader - Giant Seed | | 34 | Unknown Enemy |
| 5 | Wandering Ghost | | 20 | Comatose Sleep | | 40 | Unknown Enemy |
| **6** | **Enemy** | | 21 | Watchdogs of Farron - Giant Seed | | 47 | Spirit Summon |
| **7** | **Boss** | | 22 | Aldrich Faithful - Giant Seed | | 48 | Unknown Enemy Team |
| 8 | Ally | | 23 | Enemy | | 49 | Unknown Boss |
| 9 | Hostile Ally | | 24 | Enemy 2 | | 50 | Unknown Enemy Team |
| 10 | Decoy | | 25 | Strong Enemy | | 51 | Enemy 2 Boss |
| 11 | Decoy-like | | 26 | Friendly NPC | | 52 | Unknown Enemy |
| 12 | Battle Ally | | 27 | Hostile NPC | | 53 | Unknown Enemy 2 Team |
| **13** | **Invader** | | 28 | Co-op NPC | | 56 | Unknown Enemy |
| | | | | | | 77 | Near Death |
| | | | | | | 80 | Balancer Raid |

All of the above are **ER/legacy-derived labels used by Smithbox for NR**; whether a value behaves as its label says in NR 1.3.3 is `[HYPOTHESIS]` until T-007/T-012 observe it.
Diff against the ER file (same Smithbox commit) [SOURCE: the two JSON files]:
- NR adds `-1 No Change`, and `34, 40, 49, 50, 52, 53, 56, 77, 80`.
- Value 1 is `Player` in NR but `Live` in ER. Values 17-24 are labelled differently (ER: Invader x2, Host, Co-op, Hostile, Wandering Phantom, Enemy 1, Enemy 2). That relabelling suggests someone edited by hand
  rather than derived NR semantics. Treat 17-22 labels with suspicion (the names are DS3 NPCs).
- `SP_EFFECT_CHANGE_TEAM_TYPE.json` is **byte-for-byte equivalent in content** between NR and ER (same 43 entries). The NR file looks copied from ER, not validated for NR (e.g. ER-only "Host" 19, "Co-op" 20).

The labels most relevant to us (candidates for "hostile to the player" experiments, **no claim on which are hostile to which**):
`1 Player`, `2 White Ghost`, `3 Black Ghost`, `4 Grey Ghost`, `13 Invader`, `16 Invader`, `9 Hostile Ally`, `29 Indiscriminate`, `6 Enemy`, `7 Boss`.
Given `Player=1` for NpcParam, the value a human player carries at runtime may differ (ER assigns players via ROLE_PARAM_ST, section 3.3). Read it in CE before choosing a `hostile_team_type`.

### 4.3 Related NR param fields (for the T-011 backup route; also nice for T-012 design)

- `SP_EFFECT_PARAM_ST.changeTeamType: s8` and `teamOffenseEffectivity: s8` exist in NR. Paramdex NR has them with no description [SOURCE: https://github.com/soulsmods/Paramdex/blob/ff7245e524329bc3eab00036723d2bd53384cedf/NR/Defs/SpEffectParam.xml#L196, #L218;
  fsrs:crates/nightreign/src/param.rs#L51538, #L51560]. ER's def says changeTeamType (default -1) "overwrites with the specified team type" (my paraphrase) [SOURCE: https://github.com/soulsmods/Paramdex/blob/ff7245e524329bc3eab00036723d2bd53384cedf/ER/Defs/SpEffect.xml#L1341-L1344].
  `[HYPOTHESIS: same semantic in NR]`. This is a no-code route to flip a player's team type: apply a SpEffect with `changeTeamType = N`.
- `is_hit_both_team` is a bit of **`BULLET_PARAM_ST`** (`bits_9b`, bit 1), not `ATK_PARAM_ST` [SOURCE: fsrs:crates/nightreign/src/param.rs#L6596 (struct), #L7203 (accessor); Paramdex NR `Bullet.xml#L434`, ER-era description: "hits friend and foe, not wandering ghosts"].
  TECHNICAL_NOTES §4 row 3 lists it next to ATK/EQUIP fields; the flag only applies to bullets. `AtkParam.xml` (NR) has no team field (grep: none).
- `NPC_PARAM_ST.team_type: u8` [SOURCE: fsrs:crates/nightreign/src/param.rs#L40514 (struct), #L40619 (field)]. `NPC_THINK_PARAM_ST.TeamAttackEffectivity` is AI squad-attack throttling, not hostility
  [SOURCE: Paramdex NR `NpcThinkParam.xml#L240`].

---

## 5. Session manager, net manager, player identity, and sync (ER)

### 5.1 `CSSessionManager` (DLRF singleton name `"CSSessionManager"`) [SOURCE: fsrs:cs/session_manager.rs#L72]. Offsets [DERIVED]

| Field | Offset | Notes |
|---|---|---|
| `lobby_state` (u32) | +0x0C | 0 None, 1 TryToCreateSession, 2 FailedToCreate, 3 **Host**, 4 TryToJoin, 5 FailedToJoin, 6 **Client**, 7 OnLeave, 8 FailedToLeave [SOURCE: fsrs:session_manager.rs#L22-L32; names from Sekiro debug menu] |
| `protocol_state` (u32) | +0x10 | 0 None ... 6 **Ingame**, 7 WaitReentryToMap [SOURCE: same file#L36-L69] |
| `players: DLVector<SessionManagerPlayerEntry>` | **+0x70** | `DLVector` = `{allocator@0, first@8, last@0x10, end@0x18}`, 0x20 bytes [SOURCE: fsrs:crates/shared/stl/src/vector.rs#L16-L24]. Entry stride **0x100** |
| `host_player` | +0x90 | a bare `SessionManagerPlayerEntryBase`, "empty" iff `steam_id == 0` [SOURCE: same file#L101, #L199-L204] |
| `session_player_limit` (u32) | +0x170 | doc: 4 open world, 6 quickmatch [SOURCE: same file#L104-L106]. NR-SC lifts to 6 by its own means (TECHNICAL_NOTES §3) |

`SessionManagerPlayerEntryBase` [SOURCE: fsrs:cs/session_manager.rs#L190-L197; offsets DERIVED]: `+0x10 steam_id (u64)`, `+0x18 steam_name` (UTF-16, `DLInplaceStr<..., 64>`, 0xA8 bytes).
`SessionManagerPlayerEntry` adds [SOURCE: same file#L207-L231]: `+0xD0 game_data_index (i32; -1 for host)`, `+0xD4 character_event_id (i32)`, `+0xE0 horse_entity_handle`,
`+0xE8 is_host`, `+0xE9 is_local_player`, `+0xEA join_wait`, `+0xFC pre_ceremony_multiplay_role`.
This is the cleanest ER source for **Steam ID + name per connected player** (T-008 logging). `character_event_id` is the id used by EMEVD/other systems.

### 5.2 From a `PlayerIns` to a Steam ID

- `PlayerIns.player_session_holder` at **PlayerIns+0x598**, a `PlayerSessionHolder` whose `player_network_session` (OwnedPtr) is at **+0x18** of the holder; that `PlayerNetworkSession`
  has `remote_identity: u64` ("Steam ID for PC release") at **+0x8** [SOURCE: fsrs:cs/chr_ins.rs#L875, #L1025-L1031; fsrs:cs/network_session.rs#L84-L87; offsets DERIVED].
  Chain: `PlayerIns +0x598 +0x18 -> ptr; +0x8 = steamid`. Local player probably has no network session. [HYPOTHESIS]
- Alternatively `PlayerIns.session_manager_player_entry` (OwnedPtr to a `SessionManagerPlayerEntryBase`) at **+0x6B8** [SOURCE: fsrs:cs/chr_ins.rs#L914; offset DERIVED]: then `+0x10` = Steam ID, `+0x18` = name.
- `PlayerIns.player_game_data` at **+0x580** [DERIVED] -> `PlayerGameData`: `+0x98 chr_type`, `+0x9C character_name` (UTF-16 x 17) [SOURCE: fsrs:cs/player_game_data.rs#L18, #L57-L58; offsets DERIVED].
- `ChrIns.character_creator_steam_id` is for **summons** (Steam ID of the creator) [SOURCE: fsrs:cs/chr_ins.rs#L251]; not for players.

### 5.3 `CSNetMan` (`"CSNetMan"`) and party info [SOURCE: fsrs:cs/net_man.rs#L17]

`quickmatch_manager` at +0xB0, `breakin_manager` (invasions) at +0xA8 [DERIVED]. `GameMan.party_member_info` -> `PartyMemberInfo` with `party_members: [PartyMemberInfoEntry; 6]` (0x30 each) at +0x28, and counters
`friendly_phantom_count`, `hostile_phantom_count`, `in_world_online_player_count` ... [SOURCE: fsrs:cs/party_member_info.rs#L69-L93; fsrs:cs/game_man.rs#L205 (GameMan is RVA-based, not name-based)].
Note `[PartyMemberInfoEntry; 6]` matches NR's 6-player cap, but is ER's literal. `MultiplayType`/`MultiplayRole` (invader, host, white summon, `BattleRoyale`, ...) pick `ChrType` and `SummonParamType` per join mode
[SOURCE: fsrs:cs/multiplay_type.rs#L17, #L101, #L235-L248]; `MultiplayProperties` is RVA-based [SOURCE: fsrs:cs/multiplay_type.rs#L296]. The pure-ER roles (invasion, red/blue hunters) have no NR counterpart documented.

### 5.4 `NetChrSync` and what is synced (H1-relevant)

- `WorldChrMan.net_chr_sync` (+0x1E5E0) -> `NetChrSync { world_info_owner@0, chr_slot_count@8, net_chr_set_sync[196]@0x10 }`, one `NetChrSetSync` per ChrSet index
  [SOURCE: fsrs:cs/net_chr_sync.rs#L11-L16; offsets DERIVED]. Each `NetChrSetSync` (+0x08 chr_set, +0x10 capacity, +0x30 health readback array, +0x38 update-flags array) is a **receive-side buffer**: P2P tasks write incoming values
  and set flags, and the ChrIns picks them up [SOURCE: fsrs:cs/net_chr_sync.rs#L18-L41 doc comment; offsets DERIVED].
- Synced payloads fsrs maps: **placement** (position, rotation) and **health** `ChrSyncHealthUpdate { current_hp: i32, damage_taken: u32 }` [SOURCE: fsrs:cs/net_chr_sync.rs#L93-L112].
  So in ER, a remote player's HP is pushed as an absolute value plus damage delta from whoever owns that character. [HYPOTHESIS: that the *owner* of a ChrIns is the authority for its HP]
- The session vtable has `send_hit(buffer)` "Sends a 0x120 hit struct to the remote party" [SOURCE: fsrs:cs/network_session.rs#L13 doc]. That implies hit/attack data is sent to the *victim's* owner for resolution. [HYPOTHESIS, taken as ER precedent]
- `ChrIns.network_authority: u32` at +0x1E4 and `ChrSetEntry.chr_update_type` (Local/Remote) are the ER fields to look at when answering E-003's "which client applies the change" question
  [SOURCE: fsrs:cs/chr_ins.rs#L236; fsrs:cs/world_chr_man.rs#L410]. Their semantics are not documented: UNKNOWN.

---

## 6. Cheat Engine starting points for NR 1.3.3.0 (checklist for T-007; all `[HYPOTHESIS]`)

1. Get `WorldChrMan` from the T-006 log (look up `"WorldChrMan"`). Read the pointer in the static slot.
2. Local player: find a pointer inside WorldChrMan whose target begins with the `PlayerIns` RTTI vftable (ER: +0x1E508, `main_player`). Expect many hits to cluster at the tail of the struct
   (`main_player`, `sinner_hunter`, `net_chr_sync` at +0x1E5E0 in ER, i.e. the three are within 0xD8 of each other).
3. On the PlayerIns: check `+0x6C` (u8 team_type), `+0x68` (i32 chr_type; ER values 0 Local, 1 WhitePhantom, 2 Duelist, 8 GrayPhantom, 13 Arena), `+0x60/0x64` (ids), `+0x70` (p2p handle).
   Record the value for self; compare with the enum above (`Player = 1` in NpcParam terms, `2 White Ghost` is what ER's `PlayerGameData.quick_match_team` doc implies for allies).
4. Enemy ChrIns (any non-player in `chr_sets`): `+0x60 npc_param_id` -> look the NpcParam row up (Smithbox) and compare its `teamType` with the byte at `+0x6C`. A match confirms the offset for NR.
5. Players list: ER ChrSet at WorldChrMan+0x10EE0 (`+0x10 capacity`, `+0x18 entries`, 0x10-byte entries). Search for a structure with the same shape right before `ghost_chr_set` (ER delta +0x58). Partner's PlayerIns
   should be one of its non-null entries in an SC session (FS-1).
6. Steam ID / name: `CSSessionManager` (T-006 name `"CSSessionManager"`) `+0x70` DLVector; first entry's `+0x10` should equal the Steam64 ID `76561198342403479` (our pin, TECHNICAL_NOTES §1) for the local player.
   Use that as an anchor to confirm entry stride and layout.
7. `ChrTemporaryTeamTypeSlot`: take its RTTI vftable from the NR exe; search PlayerIns for pointers to objects with that vftable (candidate area ER: ChrIns+0x308..0x348).
8. To change hostility without writing the byte: the SpEffect `changeTeamType` route (4.3). Whether it works in NR is T-011's question.

---

## 7. Corrections and flags for TECHNICAL_NOTES.md (for Opus to apply)

1. **§4 row 3 / `team_relation.rs`**: that file shows relation *filters* (`validate(oppose, friendly, self)`), not a team-type matrix. The matrix is unmapped in fsrs. Reword; keep H1 as is.
2. **§4 row 3 `is_hit_both_team`**: lives on `BULLET_PARAM_ST`, not on ATK/EQUIP params.
3. **§2 "ER crate mapped structures ... closest map"**: true, but the `eldenring` crate **refuses to start on NR** (`rva::get()` panic) and its task/hook helpers use ER RVAs. Only `from-singleton` (and `fromsoftware-shared`'s version-agnostic bits) transfer.
   Also mod bring-up in T-006 needs our own "game initialised" wait.
4. **New**: NR has `SpEffectParam.changeTeamType` and an NR `TEAM_TYPE` enum exists in Smithbox (values in 4.2), but there is **no `ROLE_PARAM_ST` in NR**.
5. **New**: two sections named `.text` in `nightreign.exe` (see 1.3); watch for it in T-006.
6. Naming: finder uses DLRF names (`WorldChrMan`, `CSSessionManager`, `CSNetMan`), our RTTI scan shows `*Imp` names.

## 8. Open questions

1. Does `from-singleton` 3.0.1 find any singletons in NR 1.3.3.0 with its by-name `.text` choice, given there are two `.text` sections? (T-006)
2. How is `team_type x team_type -> hostile/friendly` evaluated in ER and NR? The matrix location/structure is unmapped. Candidate: reverse the caller of `CSTeamType*::validate` in ER, or test empirically (T-012).
3. What `team_type` value does a human player have at runtime in NR (1? 2?), and which values are hostile to it? (T-007, T-012)
4. Does NR retain a role/phantom-based player team assignment (no `ROLE_PARAM_ST`)? What assigns player team types in NR?
5. Where does `ChrTemporaryTeamTypeSlot` live relative to `PlayerIns`, and is it what `changeTeamType` writes to?
6. Are Smithbox's NR `TEAM_TYPE` labels accurate? Their git history for that file was not retrievable (GitHub API rate limit hit when I checked); NR param defs in Paramdex are for an unknown game build vs our 1.3.3.0. (Check Paramdex `NR/` commit date vs regulation.bin in T-011 or T-007.)
7. Does `RecurringTask` (the fsrs closure-as-game-task trick) work on NR's task system? It needs an NR `register_task` equivalent (T-008 may use a thread poll instead).
8. Damage authority (E-003): is HP owned by the victim's client or the attacker's? ER hints (`send_hit`, `damage_taken` in sync) point to victim-owner resolution but are not documented.

---

## Result (for the T-005 card; Opus to paste into TASKS.md, as I was told not to edit it)

Status: `review`. Deliverable written to `.claude/re/er-reference.md`. Key outcomes: (1) fsrs ER layouts documented with derived offsets (WorldChrMan.main_player +0x1E508,
player_chr_set +0x10EE0, ChrIns.team_type +0x6C, session players +0x70, steam id +0x10 in entry); (2) NR `TEAM_TYPE` enum found in Smithbox (community-curated, not official) with Paramdex naming the field; (3) singleton finder is the external
`from-singleton` crate, game-agnostic by design, untested on NR; the `eldenring` crate itself cannot start on NR; (4) corrections to TECHNICAL_NOTES §4 (team_relation.rs is not the hostility matrix; `is_hit_both_team` is on BULLET_PARAM_ST); (5) NR has `SpEffectParam.changeTeamType`
but no `ROLE_PARAM_ST`. Nothing is verified for NR except the PE observations (FileVersion, section table).
