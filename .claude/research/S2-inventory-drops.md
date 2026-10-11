# S2: Inventory and item drops (spike for F8 / F9 / F14)

Question: scope v2 wants "a PvP death drops the victim's entire inventory into the world, lootable by anyone". What does the game already
give us, and what is the cheapest route? Nothing below is verified for Nightreign (NR) 1.3.3.0 unless tagged `[VERIFIED]`. ER = Elden Ring.
ER facts are `[HYPOTHESIS]` for NR.

Pins: fromsoftware-rs (`fsrs`) @ `59fbd3b3b7daaf14aca47c9f73530493dba6bc79` (same as `.claude/re/er-reference.md`). `fsrs:X#Ln` =
`https://github.com/vswarte/fromsoftware-rs/blob/59fbd3b3b7daaf14aca47c9f73530493dba6bc79/crates/eldenring/src/cs/X#Ln` unless the path starts with `crates/`.
Paramdex @ `ff7245e524329bc3eab00036723d2bd53384cedf`. ER practice tool (`veeenu/eldenring-practice-tool`) @ `cc1a36c6138dba5d387dc7e33a00e0434635fc0b`.

---

## 0. Bottom line

1. NR already has a **native "drop an item on the ground for a teammate" action** (inventory Discard puts the item in the world) and world items are
   **shared and single-instance** (first pickup removes it for the whole team). That is exactly the behaviour F8 wants. [SOURCE: Bandai Namco beginner tips, section 2/3; secondary guides, section 1]
2. The NR inventory is **small by default** (6 weapons, 2 talismans, 3 consumables) so "drop everything" is at most about 11 items plus runes. [SOURCE: Bandai Namco beginner tips]
3. The singleton list we already captured has **`MapItemMan`, `CSGaitem`, `CSItemGetMenuMan`, `BloodStainMan`, `CSDeathBoxInsMan`, `CSRegulationManager`** (and no `GameDataMan`, no `ItemLot*`).
   [VERIFIED: `.claude/experiments/logs/2026-10-09_TC-004_run4_full-expedition-29.log` singleton map, lines 111, 129, 253 and 62, 81]
   fsrs binds `MapItemMan` and `CSGaitem` **by DLRF name**, so they are findable in NR without RVAs. [SOURCE: fsrs:item.rs#L1-L4, fsrs:gaitem.rs#L12-L14]
4. Biggest unknowns, none answerable by reading: (a) the NR function the Discard menu action calls; (b) where NR's `PlayerGameData` / inventory lives (ER uses an RVA-based `GameDataMan`, absent from NR's
   singleton list); (c) whether a team-29 player can use the pickup action at all (grace interaction is blocked on 29, so the action-button path may be too). All three need the game running.
5. Recommended first route: **reuse the native discard/drop path** (Route 1). Cheapest decisive experiment is a solo HUMAN test (section 6, E1) that needs no code.

---

## 1. Q1: Nightreign's own item sharing (drop, ping, pick up)

| Fact | Tag |
|---|---|
| There is **no trade system**. The way to give an item to a teammate is to discard it near them and let them pick it up. Menu: open inventory, select item, Discard (PC key shown as "-" in guides). | [SOURCE: https://deltiasgaming.com/elden-ring-nightreign-how-to-give-items-to-teammates/ via search summary, secondary; page itself returned HTTP 405 to WebFetch, so steps are unconfirmed] |
| Bandai Namco's own guide: gear and items **are not instanced per player**; if you pick up a weapon it is removed for the whole team; players are encouraged to drop items they don't need, especially for a teammate who can use them. | [SOURCE: https://en.bandainamcoent.eu/elden-ring/news/beginner-tips-elden-ring-nightreign (sections "As you Land in Limveld")] |
| Default carry limits: 6 weapons, 2 talismans, 3 consumables. | [SOURCE: same Bandai page] |
| Chests and crates throw items on the ground; those drops are shared (one player takes it, the others can't). Special boss reward ("Dormant Power") is per-player choice; basic loot shared. | [SOURCE: https://deltiasgaming.com/?p=247122 via search summary, secondary; page not fetchable] |
| Rune drops from kills are **split equally** among teammates regardless of who killed. | [SOURCE: Bandai Namco page, same section] |
| Pings: PC default "X" drops a pin; near a ground item the ping highlights that item on teammates' HUDs. Holding the action/loot button plus the ping button flags an item. Holding Triangle/Y + L3 marks an item on consoles. | [SOURCE: https://www.shacknews.com/article/144457/ping-map-and-items-elden-ring-nightreign and the Deltia guide via search summaries, secondary] |
| Dropping applies to ordinary inventory items. Which item types are blocked is **UNKNOWN**. Params carry per-item flags: `isDiscard` ("can the item be discarded"), `isDrop` ("can the item be placed on the spot"), `isDeposit`, `disableMultiDropShare` on goods, and equivalents on weapon and protector rows. | [SOURCE: Paramdex NR `Defs/EquipParamGoods.xml#L309, #L316, #L365, #L533` (Japanese descriptions, my paraphrase); fsrs:crates/nightreign/src/param.rs `is_discard` #L14687 / #L16230 / #L18872, `is_drop` #L14985 / #L16240 / #L18882, `disable_multi_drop_share` #L14767 / #L16250 / #L18862 (goods / protector / weapon respectively, by struct range: EQUIP_PARAM_GOODS_ST starts #L14258, PROTECTOR #L15334, WEAPON #L17575)] |
| Who can see and pick up: all teammates in the session (shared single instance). Whether a **hostile** player can: **UNKNOWN**. | [SOURCE: above] / UNKNOWN |

Not found: patch notes or a wiki page specifying drop rules (looked at: Bandai tips, shacknews, Deltia via search summaries, eldenring.wiki.gg root page returned 404). A Steam discussion exists on the topic
("Stop picking up items that are bad for you but good for a teammate", https://steamcommunity.com/app/2622380/discussions/0/686368182958621010/), not read.

## 2. Q2: Death in NR

| Fact | Tag |
|---|---|
| At 0 HP in co-op you are **downed** (near-death gauge); teammates revive by attacking you. Day: timer runs; night: no timer while others live. Real death: **you lose all runes (dropped at the death spot) and one level**, then respawn at the last grace or nearby safe spot. | [SOURCE: https://beebom.com/elden-ring-nightreign-death-explained/amp/ ; Bandai tips] |
| Dropped runes are recoverable by the dead player; an enemy may pick them up, and killing that (golden-aura) enemy returns them. Nothing says a teammate can take them. | [SOURCE: Beebom; Bandai tips] |
| The sources say **nothing about items or equipment** being lost or dropped on death. Our own FS-0 run: a PvP death gave "downed, no revive possible, respawn with runes and a level lost". | [SOURCE: Beebom (silence)] [VERIFIED: TECHNICAL_NOTES §4 row "Two-way PvP works...", FS-0 run 4] |
| Implication: the **rune-bloodstain mechanic already fires on PvP death** (runes lost) so the engine already creates a world pickup at the death spot for runes. Whether a rival can collect it: **UNKNOWN**. In ER the bloodstain is a single record in the local `GameDataMan` (`has_bloodstain`, `bloodstain`, `bloodstain_entity_id`; `BloodstainData{ pos, rune_count, block_id }`), i.e. per-player and local. | [SOURCE: fsrs:game_data_man.rs#L53-L55, #L137-L145] [HYPOTHESIS for NR] |
| NR has a `BloodStainMan` singleton. | [VERIFIED: singleton map log line 62] |

## 3. Q3: Where inventory and equipment live (ER layout as hints)

Chain in ER (all `[SOURCE: fsrs ...]`, offsets not derived by me; use `offset_of!` as T-005 did before relying on any):

- `PlayerIns.player_game_data` -> `PlayerGameData` [fsrs:chr_ins.rs#L872]. The er-reference derived this at PlayerIns+0x580 for ER. For NR (ChrIns layout is ER shifted +8) the guess is **+0x588**. [HYPOTHESIS; er-reference.md §3.1; TECHNICAL_NOTES §4 "NR = ER shifted by +8"]
- Also reachable from the global `GameDataMan { main_player_game_data, player_game_data_list [5], session_player_game_data_list [40], gaitem_game_data, bloodstain... }`, but `GameDataMan` is **RVA-based in fsrs** (`load_static_indirect(rva.game_data_man)`) and **not in NR's DLRF singleton list**. [SOURCE: fsrs:game_data_man.rs#L42-L55, #L126-L134] [VERIFIED: absent from log]
  Finding it in NR: RTTI scan for `GameDataMan` / `PlayerGameData` vtables (we already have an RTTI resolver, T-007a) then pointer-scan for it. [HYPOTHESIS]
- `PlayerGameData`: stats incl. `rune_count` [fsrs:player_game_data.rs#L46], `equipment: EquipGameData` [#L135], `storage: Option<OwnedPtr<EquipInventoryData>>` (stash box, in ER) [#L138], `is_main_player` [#L143]. Remote players also get a `PlayerGameData` in ER (`session_player_game_data_list`, equipment synced in "packet 12"). [SOURCE: fsrs:game_data_man.rs#L12-L24, #L45-L50]
- `EquipGameData` [fsrs:player_game_data.rs#L421-L451]: `equipment_item_idx_list [22]`, `chr_asm` (equipped slots, `gaitem_handles [22]`, #L1213-L1218), `equip_inventory_data`, `equip_item_data` (`quick_slots[10]`, `pouch_slots[6]`, `great_rune`; #L1033-L1042), `last_add_item_result` (enum Success / UniqueItemDuplicate / InventoryFull; #L442-L458: the vendor of "add item" returns this).
- `EquipInventoryData` [#L907-L927] -> `items_data: InventoryItemsData` [#L499-L570] with three arrays (normal items, key items, multiplayer key items), each `{capacity, head ptr, used length}`, plus an item-id hash map. Entries are `EquipInventoryDataListEntry { gaitem_handle, item_id, quantity, sort_id, is_new, pot_group }` [#L991-L1008]; an entry is empty when `item_id` is invalid (#L1010-L1014). Use `normal_entries()` etc. semantic: **the arrays can contain gaps**, iterate the whole capacity. [SOURCE: #L509-L514 comments]
- `CSGaitem` (DLRF singleton, `CSGaitemImp`): 5120 `gaitems[]` (`Option<OwnedPtr<CSGaitemIns>>`), each `{ gaitem_handle, item_id }`; weapons (`CSWepGaitemIns`) and gems carry extra data (reinforcement, gem slots / ashes of war). Handle bitfield: index = bits 0-15, bit 23 = "indexed" (weapons, protectors, gems), category = bits 28-30 (Weapon 0, Protector 1, Accessory 2, Goods 3, Gem 4). `CSGaitemGameData` (inside GameDataMan) is a 14000-slot "already acquired" table. [SOURCE: fsrs:gaitem.rs#L12-L26, #L58-L65, #L73-L98, #L120-L128, #L163-L173, #L202-L206]
- Item id format: `ItemId` / `OptionalItemId` (category in the top bits; goods = 4, weapon = 0 in the `ItemCategory` enum while the gaitem handle uses a different category number set 0-4). [SOURCE: fsrs:item_id.rs#L23-L32, fsrs:gaitem.rs#L120-L128]
- Upgrade level / affinity are encoded **in the weapon item id** (the ER practice tool adds `upgrade + affinity` to the base id when spawning). [SOURCE: veeenu practice-tool `practice-tool/src/widgets/item_spawn.rs#L209-L222`, commit cc1a36c]

NR: all of it `[HYPOTHESIS]`. NR has `CSGaitem` by name, so at least that struct can be probed (check `gaitems[0..]` entries for a plausible `item_id` and category bits). NR weapons have no ashes of war, so `CSWepGaitemIns` likely differs. UNKNOWN.

NR params useful for decoding ids: `EQUIP_PARAM_WEAPON_ST`, `EQUIP_PARAM_PROTECTOR_ST`, `EQUIP_PARAM_ACCESSORY_ST`, `EQUIP_PARAM_GOODS_ST`, `ITEMLOT_PARAM_ST`, `ITEMTABLE_PARAM_ST` all exist. [SOURCE: fsrs:crates/nightreign/src/param.rs#L13998 (accessory), #L14258 (goods), #L15334 (protector), #L17575 (weapon), #L30605 (itemlot), #L31278 (itemtable)] (NR has no `ITEMLOT_PARAM_ST` map/enemy split visible in the bindings: one struct; how the game picks tables: UNKNOWN.)

## 4. Q4: How ER mods spawn or give items; NR equivalents

| Technique | Evidence | NR status |
|---|---|---|
| **"Item give" via `MapItemMan`**: call a game function with (`*MapItemMan`, `&SpawnRequest { one=1, item_id, qty, durability=-1, gem=-1 }`, `&mut out_u32`, `0`). Used by the ER practice tool's item spawner. | [SOURCE: veeenu/eldenring-practice-tool `practice-tool/src/widgets/item_spawn.rs#L340-L377`; function RVAs `func_item_spawn: 0x548c20`, `func_item_inject: 0x54e570`, `map_item_man: 0x3c51cf0` in `lib/libeldenring/src/codegen/base_addresses.rs#L250-L262`; all @cc1a36c, ER only] | `MapItemMan` singleton **exists in NR** (instance non-null in expedition). Function RVA and signature: **UNKNOWN**; needs an AOB search or xrefs from the pickup flow. |
| Whether that call puts the item in the **player's inventory** or on the **ground**: the practice tool uses it as "Spawn item" for the player; I could not find documentation saying which, so treat as unknown. | UNKNOWN (looked at: item_spawn.rs, pointers.rs; no README text on it) | Test in-game. |
| Item debug tools in C#: Nordgaren's `Elden-Ring-Debug-Tool` has `GibItemCommand` / `RemoveItemCommand` (give and remove inventory items) built on `Erd-Tools`. Implementation not read. | [SOURCE: https://github.com/Nordgaren/Elden-Ring-Debug-Tool/tree/main/src/Elden-Ring-Debug-Tool-ViewModels/Commands/ItemViews ; https://github.com/Nordgaren/Erd-Tools-CPP] | ER only, unread. |
| **Param route**: put items in an `ITEMLOT_PARAM_ST` row and let the game award it (map pickup / treasure / EMEVD "AwardItemLot"). fsrs has `ItemLotUtil { item_lot: u32, is_map_item_lot: bool }`, i.e. an ER helper object for "lot id + map-or-enemy". Not documented for spawn. | [SOURCE: fsrs:item_lot.rs#L1-L11] | NR param exists (above). Lots are **static** (set in regulation), so they cannot encode a dying player's inventory; only fixed-content drops. Needs regulation.bin modding, which SC may check (TECHNICAL_NOTES §3, `YuiNexusRegulationManager`). |
| NR `ITEMLOT_PARAM_ST` has `canExecByFriendlyGhost` / `canExecByHostileGhost` bits ("also roll when I am a co-op / hostile phantom") and a per-player `getItemFlagId`. Suggests a lot can be restricted by the receiver's multiplayer role; the semantics under SC are unknown. | [SOURCE: Paramdex NR `Defs/ItemLotParam.xml#L297, #L493, #L500`] | UNKNOWN effect. |
| `fsrs/examples/spawn-asset` spawns an asset/object (not an item) in ER, showing the crate can drive object spawning. Not read. | [SOURCE: tree listing `examples/spawn-asset/src/lib.rs` @59fbd3b] | ER only; the crate does not run on NR (er-reference §1.3). |
| Cheat Engine tables (Grand Archives, Hexinton etc.) do "give item" by calling the same game routine. | [SOURCE: press search summaries only; not verified] | ER only. |

Warning: GitHub search for "elden ring item spawner" is flooded with repos that look like malware/cheat-seller spam ("Undetected EAC bypass"). I did not open any of them; do not either.

Other NR singletons worth knowing for this spike [VERIFIED: singleton map log]: `CSItemGetMenuMan` (item-acquired popup), `CSMenuMan`, `CSActionButtonMan` (interaction prompts; pickup goes through
action buttons in ER, to be confirmed), `CSDeathBoxInsMan`, `CSShareClearDataInsMan`, `CSRespawnPointMan`, `CSRegulationManager`, `CSEventFlagMan`, `CSNetworkUserManager`. Whether any of these is
involved in the pickup or drop: UNKNOWN.

## 5. Q5: Is a world item drop networked or per-player?

| Evidence | Tag |
|---|---|
| NR: items dropped or found are **shared**: picking up a weapon removes it for the team. So the engine has a **replicated world-item object** (pickup state is synced). | [SOURCE: Bandai Namco tips] (Note: this is vanilla matchmaking; SC reuses the same netcode, TECHNICAL_NOTES §3) |
| ER vanilla: unknown whether map pickups are per-player. A Steam forum anecdote says summoned players can loot enemy drops but not open chests; unverified. | UNKNOWN (looked at: search summaries only) |
| ER SC (the mod): "items are unique to every player", boss drops and world items are given to everyone. Press description of the mod, not the code. | [SOURCE: press search summary of the Seamless Co-op ER description (techraptor.net)] [HYPOTHESIS for NR SC] |
| NR SC: nothing found on how item pickups are handled. Check the Nexus page comments/bugs. | UNKNOWN (looked at: search results for NR SC; no source) |
| `disableMultiDropShare` per-item flag shows the engine has a notion of "multi-drop sharing" of items across players. | [SOURCE: Paramdex NR `EquipParamGoods.xml#L365`] meaning UNKNOWN |

Design consequence: two cases. If the discard-spawned object is a replicated entity, one client creating it is enough (Route 1). If each client creates and owns its own local copy (the ER SC pattern), a
free-for-all needs our own arbitration (section 7, Route 2).

---

## 6. Experiments (all HUMAN, solo first, no code except where noted)

- **E1, solo, no code (do first).** SC solo expedition. Discard a weapon and a consumable on the ground. Does it appear? Walk away and return: does it persist? Pick it up: does it return with upgrade level intact? Then repeat with `pvpve = true` (team 29) and via the overlay with team 2 vs 29. **If the pickup action does not work on 29**, that is the new blocker (compare the grace issue T-022).
- **E2, Nucleus two instances.** A discards a weapon, B sees it and picks it up. Does it vanish on A's screen? What if B is on team 29 and A on 2 (client views disagree)? Does a hostile player see it, take it, and does it sync? Record both logs. (Also feasible per client, to see whether copies exist on both.)
- **E3, code, read-only.** Add a probe: log `CSGaitem` (via singleton name) entries and, once found, `PlayerGameData` -> `equip_inventory_data` entries (item id, quantity). Method: RTTI scan for `PlayerGameData` / `GameDataMan` / `EquipInventoryData` vtables, then probe PlayerIns+0x588 (hypothesis), validating each pointer with the guarded-read helpers. Acceptance: logged list matches the in-game inventory (6 weapons, 2 talismans, 3 consumables at most).
- **E4, code, read-only.** Log a call-trace of the Discard path without hooking by hand: with the debugger / CE, break on writes to `MapItemMan` fields or xref the string "discard" menu handler. Goal: the function the Discard action calls, its args, and whether it takes the item from the inventory in the same call.
- **E5, code, one write.** Call the discard function (found in E4) from our thread once, for a known inventory slot, on the main player, in a solo expedition. Does an item appear? Run only after E1 and E4.
- **E6, runes.** In FS-0, kill A with B. Where do A's runes appear, and can B walk over and take them? (Rune pickup is the closest existing "death drop" and costs nothing.)

## 7. Recommended routes (ranked)

### Route 1: Reuse the native Discard-to-ground action on death (best)
Mechanism: on a PvP death, the victim's client (the authority for its own inventory) runs the same internal function the Discard menu entry calls, once per inventory entry (and per equipped item), placing items around the death point. The engine then replicates the world object as it does for teammate gifts.
- Pros: native pickup UX, native replication ("vanishes for the team", Bandai), items keep upgrades/affinity, no custom netcode.
- Verify: E1 (pickup works on team 29 and after PvP), E2 (hostile client sees and can take it, one winner), E4 (function found, callable from our thread without a menu), E5, plus: does the drop work when the player is downed or dead, does it need the player entity alive? What about items with `isDiscard = 0` or `isDrop = 0`. Count of drops per death is limited by the small NR inventory (about 11 plus runes).
- Risks: function may require UI state; ground items might spawn stacked; SC may reject the action; anti-dupe (victim's own inventory must lose the item exactly once; make the removal part of the same call).

### Route 2: Spawn world items with the `MapItemMan` give/spawn call (ER practice-tool style) on every client
Mechanism: all clients observe the death (we already see player state through `ChrSet` entries; F12 state machine) and each spawns the same item at the same position through the `MapItemMan` function (NR RVA/signature to be found). First pickup wins, so pickups must be arbitrated: either a deterministic rule (the closest player claims, via the per-player id `ChrIns+0x78`, which is identical across clients [VERIFIED: TECHNICAL_NOTES §4]) or our own message channel (spike S4).
- Verify: whether the call creates a ground object or adds to inventory (E-new: call it once with a harmless good and watch), whether the spawned object is local-only, whether pickup of a locally spawned object tells other clients, and whether the victim's removal is separate (needs inventory write: `EquipInventoryData` entry edit plus hash map update, per `update_item_id_mapping` / `remove_item_id_mapping`, fsrs:player_game_data.rs#L810, #L863).
- Cons: duplication risk, heavy reverse engineering, needs both the spawn function and inventory removal.

### Route 3: "Virtual loot" with our own pickup (fallback that always works if we can read and edit inventory)
Mechanism: on death, snapshot the victim's inventory (E3), remove it locally, publish a loot record `{pos, items}` through our own channel (S4) or deterministically on all clients, and mark the spot with a visible world effect (bloodstain-like SFX, spawned asset). A rival standing in range presses our key/overlay button to claim the record; the claimant's client calls an add-item function to grant the items.
- Verify: an add-item function that works under SC (`last_add_item_result` exists in ER, fsrs:player_game_data.rs#L442-L458), claim arbitration, syncing of records, effect spawning.
- Cons: not "in the world", own UI, own netcode; but independent of whether discarded items replicate or can be picked up on team 29.

### Fallback designs if full inventory drop is infeasible (F8 / F14)
1. **Runes only.** The engine already drops runes at the death spot (our FS-0 saw rune loss). If a rival cannot collect them, route them to the killer by editing `rune_count` (fsrs:player_game_data.rs#L46) on kill. Cheap, no item handling.
2. **Consumables and weapons only.** Drop 6 weapons + 3 consumable slots, skip talismans and key items (reduces the `isDiscard` and unique-item edge cases).
3. **Killer reward directly.** On kill, grant the killer the victim's items via an add-item call and clear the victim (Route 3 minus the world step).
4. **Loot from the arena.** Spawn fixed loot with static `ITEMLOT_PARAM_ST` rows (regulation edit, subject to SC regulation checks) or by an EMEVD award, ignoring victim gear. Weakest fit for F8.

## 8. Open questions

1. Does the Discard action in NR create a replicated world entity, and is its pickup limited by team type (team 29), by session role, or by distance only? (E1, E2)
2. What is the function the Discard menu calls, and can it be called on demand from our thread while the player is alive, downed, or dead? (E4, E5)
3. NR location of `PlayerGameData` / `GameDataMan` (RTTI scan; PlayerIns+0x588 guess) and whether `EquipInventoryData` still matches ER. (E3)
4. Does the `MapItemMan` function at `(MapItemMan*, request, out, 0)` give to inventory or spawn a ground item in ER, and what is its NR equivalent? Where could we read ER's function body for this, given we do not decompile or reproduce code? (Treat as AOB/signature search by us on NR.)
5. Which item types are undroppable in NR (key items, relics, unique/boss items)? Check `isDiscard`/`isDrop`/`isDeposit` per row in NR params (Smithbox) and compare with E1.
6. Can a rival collect the victim's rune bloodstain (E6)? If yes, runes are already F8-complete.
7. Are NR world pickups per-player copies under SC (as in ER SC per press) or shared single instances as in vanilla (Bandai)? E2 settles it.
8. Does SC revert inventory edits or intercept item flow (it hooks `WorldChrManImp` and has a param repository, TECHNICAL_NOTES §3)? Part of E5.
9. Starting items on respawn (F9): where does NR get the class loadout at expedition start, and can we re-grant it (needs an add-item function; same question as Route 3).

## Sources used (all accessed 2026-10-10)
- https://github.com/vswarte/fromsoftware-rs @59fbd3b (`cs/item.rs`, `cs/item_lot.rs`, `cs/item_id.rs`, `cs/gaitem.rs`, `cs/game_data_man.rs`, `cs/player_game_data.rs`, `crates/nightreign/src/param.rs`)
- https://github.com/soulsmods/Paramdex @ff7245e (`NR/Defs/EquipParamGoods.xml`, `NR/Defs/ItemLotParam.xml`)
- https://github.com/veeenu/eldenring-practice-tool @cc1a36c (item spawner + base addresses)
- https://en.bandainamcoent.eu/elden-ring/news/beginner-tips-elden-ring-nightreign (primary)
- https://beebom.com/elden-ring-nightreign-death-explained/amp/ (secondary)
- Deltia's Gaming, Shacknews, Steam discussion (search summaries only; secondary, pages not fetched)
- `.claude/experiments/logs/2026-10-09_TC-004_run4_full-expedition-29.log` (singleton map)
