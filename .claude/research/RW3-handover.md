# RW-3 hand-over: defeat drops (red phantom / items on the ground)

Written 2026-10-31 for the next session. Branch `feature/player-red-phantom-loot` (from `main` 6a80166). Read in this order: this file, then `RW3-red-phantom-static.md` (exe reading, addresses), `RW3-red-phantom-game.md` (what the game does for the player), `.claude/CLAUDE.md` rules (evidence tags, never go online with a modded game, never touch Seamless Co-op).

## 1. Goal and decisions so far

**Goal (Leonardo, 2026-10-31, in his words):** downed players, after the bleed-out time runs out, drop all their items on the spot where they died, so other players can loot them. Trigger = the bleed-out reaching 0 (the real death), not the downing. "All items" = what the game's own death record covers (up to 6 weapons, 5 talismans, 10 consumables); runes later. **ANSWERED (Leonardo, 2026-10-31): items drop only on bleed-out deaths caused by a rival/other player; enemy and fall deaths stay a vanilla respawn with items kept (RW-2).** Build for the rival case (`NightAct` on "defeated by a rival") and keep the trigger condition in one place.

- **Product rule** (`re/PROJECT_SCOPE.md` §3, Leonardo): a player defeated by a rival drops their gear where they fell; the killer or anyone can take it. Runes later. Nobody respawns (a rival's kill is already "defeated for good": body held, defeat screen / spectating).
- **2026-10-31 Leonardo rejected** the first slice (runes as an invisible record on the data channel; branch `claude/rw3-runes-drop-dd0fca`, commit 5a2633a, scenario `mi-rw3-runes` PASS, never pushed; it works and can be mined for ideas: `game/runedrop.rs`, bus topics 3/4, aids `rune_give` / `rune_drops` / `rune_drop_go`). He wants **items first, through the red phantom or at least items on the ground**; runes come on top later.
- **2026-10-31 Leonardo's leaning:** drop-only (items on the ground, no phantom figure) "is not a bad idea"; Claude's opinion: ~70% workable as the final solution, decided by the first live experiment (below). The real red phantom is optional polish.
- Not decided: whether the phantom figure is required for the product. Ask Leonardo after experiment E1.

## 2. What we know (details and evidence in the two research notes)

1. **Vanilla red phantom = the game's online "DeathBox" feature** (`CSDeathBoxInsManImpl`, `CSDeathBoxUploadJob`, ...). The dying player's game uploads a gear record to FromSoftware's server; other games **download** records and show one as a kneeling red phantom (nearest within 50 m). It is never made from a death in your own session. So the vanilla path cannot run for us (needs online mode, server; rule 2).
2. **The hold is not why we saw nothing.** The upload starts at `exe+0x5C296B` (death handler) before the step we hold and returns at once unless online mode (`[exe+0x3C078D0]+0xD8 == 2`). `[VERIFIED: static disasm]`, meaning `[HYPOTHESIS]`.
3. **The record** (`exe+0xB0DF60` fills it from LOCAL's `PlayerGameData`): up to 6 weapons, 5 accessories (id | `0x20000000`), 10 quick items with counts, name, position, character id. **No runes.**
4. **The loot step** (`exe+0x53D070`): a client sends session message 0x6E to the host, the host sets "opened" and broadcasts 0x6F, then calls **`exe+0x576110(MapItemMan, &vector<0x50-byte entries>, &pos, &mapId, yaw)`** which drops the items as ordinary `MapItemMan` records (same container as the boss orb, MEMORY_MAP §22). Entries built by `exe+0x56DED0` (init) + `exe+0x578A60(entry, &id, count)`; weapon entries via the named-weapon (Grief) DB lookup `exe+0xB12C80` + `exe+0xB0E390`.
5. **The prompt:** ActionButtonParam 1030 (1031 variant) through `CSActionButtonRegionSystemImp`; the dead player's name goes into text tag 0x147. No team check.
6. **White phantoms:** Roundtable static ones (`CSShareClearDataInsManImp`) use the same `CSDisplayGhost` class and are also server-fed; live Limveld ones are `ReplayGhostIns` fed by `CSNetWanderingGhostDb` (replay a recorded character, no loot).
7. **Our own earlier observation** (`mi-rw3-observe`, MEMORY_MAP §23): after `rival_hit` + `hp0` the victim keeps all runes, "YOU DIED" shows, nothing appears on the killer's screen.
8. **RW-5 lesson** (MEMORY_MAP §22): on a client, picking a map item needs the **host's copy** of the record; so who creates the drop matters.

## 3. The plan (cheapest first; stop and report after each stage)

| Stage | What | Gate |
|---|---|---|
| **E0 read-only** | `peek` the online-mode byte `[[exe+0x3C078D0]+0xD8]` / `+0xDE` offline; find the FD4 singleton of `CSDeathBoxInsMan` and `[CSNetMan+0x48]` (list size at `+0x48`); check whether the Roundtable Hold white phantoms show in our offline game (walk to the entrance, screenshot); read `[CSSessionManager]+0xC` on host and client (expected 3 / 6) | all expected empty offline; tells us if the display half exists offline |
| **E1 the key experiment** | New dev console aid (use the `add-console-command` skill, offsets tagged 1.03.3 + MEMORY_MAP section) that calls `exe+0x576110` **on the host** with ONE plain item (a weapon id with a known upgrade level, then a consumable) at LOCAL's world position. Check with `peek` on `MapItemMan` slots +0x40..+0x100 (§22), a screenshot, and a pick on **both** games (two `nro` instances: host makes the drop, client picks; then the reverse) | does it appear? does the client see and pick it? does the weapon keep its level? coordinate convention OK? |
| **E2 drop-only slice** (only if E1 passes) | Victim's game reads its gear exactly as `exe+0xB0DF60` does, sends it over `net::bus` (new topic; the key/value are `u32`, so several facts per item like `game/runedrop.rs` did on the rune branch), the host builds entries and calls `exe+0x576110` at the victim's world position; trigger = the existing `NightAct` for a rival's defeat (`features/br_match/defeat.rs`, as the rune branch's `DropRunes`). Scenario `mi-rw3-items.toml` (victim defeated by `rival_hit` + `hp0`, items appear, the other game picks one). Victim's inventory need not be emptied (defeated for good; expedition items probably vanish at the end, `[HYPOTHESIS]`: verify) |
| **E3 polish** | Real phantom: inject a record on every game (layout in the static note §1) and trigger the instance rebuild (`exe+0x53DE40`), or our own marker + prompt. Only after E1/E2; medium-high risk | Leonardo decides if the figure is worth it |
| later | runes (reuse `game/runedrop.rs` idea or put them in the same drop), the tide's defeat, Duo/Trio, FS-RW1 two-PC check | |

**If E1 fails (the client does not see the host's drop):** fallback = every game spawns its own copy of the drop from the bus data, first pick wins, closed by a bus fact (the RW-5 pattern, `game/orbclaim.rs`).

## 4. Open questions to settle live

Does `exe+0x576110` work offline and on a client? Its exact signature and vector type (MSVC `std::vector` allocated with the game's allocator?), coordinate / map-id convention (`+0x0C` map id, `+0x10` position inside the block; compare with `pos`). How weapon upgrade levels and relics are encoded in the item entry (the record holds only ids + counts; ER put upgrade in the id). Which param is #0x29 (SFX ids of the phantom). What happens to a dropped item's lifetime (RW-5: 36 s to minutes, cause unknown). Whether the singleton at `exe+0x3C078D0` is the online-state object (name via its RTTI).

## 5. Lab setup (all in TRAPS.md / TOOLS.md; the short version)

- Steam must be **closed** (`steam.exe -shutdown`); one set of games at a time (`tasklist` for leftover `nightreign` / `nro`).
- Build `cargo build --release -p nro -p nrpvp -p nrcon`; copy the desk: `copy target\release\nro.exe runs\nro-desk.exe`, run `runs\nro-desk.exe desk` minimised; build the mod with `scripts\build-mod.ps1` while the desk runs.
- `dist\` is gitignored: copy **`dist\gamedata` AND `dist\nrpvp.toml`** from the main checkout (`C:\Users\Leonardo\source\repos\nightreign-pvp\dist`). **Without `nrpvp.toml` PvP stays OFF** (`pvpve=OFF`, rival never becomes team 29): lost an hour on this.
- Run a scenario: `target\release\nro.exe run scenarios\<name>.toml --mod-dist dist` (PowerShell tool, not the sandboxed Bash for launches).
- Template for two-instance scenarios: `scenarios/mi-rw8-runes.toml` (setup through hawk drop). The rune branch's `scenarios/mi-rw3-runes.toml` is a working defeat flow to copy.
- Aborts happen: window not in front (rerun once), rival not team 29 in 30 s (timeout 90 s helped).

## 6. Traps found this session (also in TRAPS.md on the rune branch; port them)

- **`hp0` is refused after `god 1`** (`gate failed (hp 524287, max 1000000 ...)`): the god mode leaves the data module's max HP inflated, not fixed by `god 0`. The victim of a defeat scenario never uses god mode.
- **Scenario `contains` matching is case-insensitive** (`TAKEN` matched `taken here`): match whole phrases.
- A `mob_kill` target can be worth 0 runes (npc 43810110); use a deterministic aid.
- Existing traps: no `trace_read` on hot engine arrays (crashes the game), no `tp` hops over unloaded terrain, one key into a window in the back can be lost, desktop must stay awake.

## 7. Where things are

- This branch: `.claude/research/RW3-red-phantom-game.md`, `-static.md`, this file. No code yet.
- Rune-record slice (rejected, kept): branch `claude/rw3-runes-drop-dd0fca` commit 5a2633a (`crates/nrpvp/src/game/runedrop.rs`, `net/bus.rs` topics RUNE_DROP / RUNE_TAKE, `NightAct::DropRunes`, scenario `mi-rw3-runes`, MEMORY_MAP §23, testing-log entry 2026-10-31).
- The static researcher's scratch worktree: `.claude/worktrees/agent-ad0c199866f75a067` (safe to delete; its report is copied here).
- Ghidra (REA) (2026-10-31): `open_binary` on the exe returned in about a minute and `binary_overview` worked (540,742 procedures, 61,941 strings, image base 0x140000000), so a first analysis is NOT hours here. But the saved snapshot `Z:\Tools\ghidra-projects\nightreign\nightreign-1.03.3.snapshot` is only 34 KB (overview evidence, no analysis database; Ghidra deletes its temporary project on close). Treat it as useless: just `open_binary` again when you want decompiled code (e.g. `procedure_pseudo_code` on `exe+0x576110`), and keep it open for the session. Python + capstone (`Z:\Tools\rea\elden-ring-nightreign\tools\nr.py`) remains the fallback.

## 8. Update 2026-11-01 (live session: E0 done, E1 PASSED host -> client)

Details: MEMORY_MAP section 24, testing-log 2026-11-01. **The drop works.** `exe+0x576110`, called on the game thread (aid `item_drop`, `game/itemdrop.rs`, lab scenario `mi-rw3-drop`), creates a visible, pickable ground item on the host with the right id and position; the client gets the record, sees the same prompt and can pick it into its inventory. The earlier crash was the call running on the wrong thread (null per-thread allocator). Leonardo's answer to the open question: items drop only on rival-caused bleed-outs.
**Still open before/while building E2:** (1) client-initiated drop (the victim may be the client: the plan is that the victim's game sends the gear over `net::bus` and the HOST drops, so this may not be needed; confirm with a host-side drop triggered by a bus message); (2) weapon level / relics encoding in the entry (plain id = level 0); (3) lifetime of the item; (4) how to run the drop from the host's game thread inside the feature (reuse the breakpoint-handler trick; it needs a free hardware slot, check the other features with `hwbp::is_free`) or find a cleaner game-thread hook; (5) reading the gear exactly as the upload filler does (`exe+0xB0DF60`; the id list layout is in MEMORY_MAP section 24) and an inventory-remove for the victim; (6) the red phantom figure stays optional polish.
**Recommended next:** E2 as in section 3 (victim -> bus -> host drops at the victim's position, trigger = rival bleed-out), plus a scenario `mi-rw3-items`.

**Update 2026-11-01 (final): weapon level checked and kept** (it is part of the id, MEMORY_MAP 24: Raider's Greataxe+3 drops as +3).
**Update 2026-11-01 (end): slice 2 is built too** (talismans and consumables from the inventory, see MEMORY_MAP 24 and the testing-log; weapon level, lifetime, armour, runes, the client-as-victim with a consumable and a scenario for the inventory part are open).
**Update 2026-11-01 (late): E2 slice 1 is built** (weapons only, `mi-rw3-items` PASS, commit 6fec3a4, see TASKS RW-3). Next pieces and what is known: (a) **consumables:** the starting Crimson flask must NOT be dropped (Leonardo); picked-up consumables become inventory items; a valid test item is Wending Grace `0x400002BC` (spawn with `item_drop 400002BC` at the feet, pick with F); reading the inventory (where goods and counts live) is not done: after picking one, find the entry with `memscan hex BC 02 00 40` and read its neighbours (the quick-item slots `PGD+0x1D8` are the 10 pouch slots, the flask is one of them: skip it by id once known). (b) **talismans:** equipped ids sit behind gaitem handles (`PGD+0x1D8+0xF4..`, `0x908002C2..`); a valid talisman id is still needed (the table's accessory list is mostly Elden Ring leftovers); easiest is for Leonardo to pick a real one in a match and read its id. (c) weapon level: add the level to the id (table script). (d) the table's `exe+0x574A6C` drops at the player's feet without a position; ours (`576110`) takes a position, which the defeat drop needs.
