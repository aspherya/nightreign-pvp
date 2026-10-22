# Nightreign memory map: what we found

Everything here was observed on **ELDEN RING NIGHTREIGN 1.03.3** (`nightreign.exe` 1.3.3.0) with Seamless Co-op v1.1.3.
A game update can move any of these. Each row says how it was confirmed; the test logs are under `.claude/experiments/`.

How to read an address: `PlayerIns+0x718` means "0x718 bytes into the player's object". `[PlayerIns+0xB78]+0x70` means "read the
pointer stored at PlayerIns+0xB78, then go 0x70 bytes into what it points to". `exe+0x…` is an offset from where the game is loaded.

Confidence: **Confirmed** = seen behaving this way in the live game. **Likely** = fits everything we saw, not tested on purpose.

---

## 1. Finding things at startup

| What | Where | Behaviour | Confirmed by |
|---|---|---|---|
| Game systems by name ("singletons") | found with the `from-singleton` scan, 235 of them | Each has a fixed slot that holds a pointer to the live object. The pointer is empty until the game creates the object | T-006 log |
| `WorldChrMan` (the character manager) | slot at `exe+0x3C0F0A8` | Empty at the title screen. **Re-created on every area load** (Roundtable → expedition), so always re-read it | T-006, every session |
| `CSSessionManager` | slot at `exe+0x3C21EC8` | Session and players. Layout not mapped | T-006 |
| `CSNetMan` | slot at `exe+0x3C046A8` | Networking. Layout not mapped | T-006 |
| Class of any object | the object's first 8 bytes point to a table; the game's built-in class names can be read from there | Lets us check "is this really a player?" before touching it | T-007a (8,813 classes) |
| Player class `CS::PlayerIns` | table at `exe+0x2C307F8` | Every player character, local and remote | T-007a |
| Enemy class `CS::EnemyIns` | table at `exe+0x2C18120` | Every enemy | T-007a |
| `CS::PlayerGameData` | table at `exe+0x2BE6E28` | Stats block of a player | FS-0 run 7 |

## 2. The list of players

| What | Where | Behaviour | Confirmed by |
|---|---|---|---|
| Player list | `WorldChrMan+0x9A40` | A fixed list with **6 slots** (Seamless Co-op's player limit). Slot count at `+0x10`, pointer to the slots at `+0x18`. Each slot is 16 bytes and starts with a pointer to a player | T-007a, every session |
| Which slot is "me" | the first filled slot | On every player's own game, **slot 0 is the local player**; others follow in join order | Nucleus and real Steam, 2 players. Not tested with 3+ |
| Number of filled slots = players loaded in this world | same place | Goes from 1 to 2 on both games a few seconds after a friend really joins the Seamless Co-op lobby; stays 1 when the friend is only a Steam lobby member. Matchmaking uses it as "present" | FS-MM1, FS-MM2, FS-MM3 (2 players) |
| Slot count during loading | same place | Briefly reads 0 while an area loads. Ignore it and retry | every session |
| Other pre-made player objects | `WorldChrMan+0x9C20` … `+0xA170` | About ten extra player-class objects with team 30 ("Object"). **Not real players**; ignore them | T-007a |

## 3. Inside a character (`ChrIns`; players and enemies share this start)

| Offset | Type | What it is | Behaviour | Confirmed by |
|---|---|---|---|---|
| `+0x08` | 8 bytes | **Handle**: the character's ID inside this game | Host was `0xFFFFFFFF10C00000`, the joiner `0xFFFFFFFF10C00001`. Other objects refer to a character by this value | FS-0 run 7 |
| `+0x68` | u32 | Character / NPC id | 0 for real players, e.g. 10000010 for an enemy, 1000000 for the "object" players | T-007a |
| `+0x70` | u32 | Character type | 0 in Roundtable, 13 in an expedition for players | TC-004 |
| `+0x74` | u8 | **Team type**: decides who is hostile to whom | See §4. Each game decides from **its own copy**; it is not synced | TC-004, FS-0, FS-1 |
| `+0x78` | 8 bytes | **Session player id** | Same value for the same player on everyone's game: `0x6001FFFFFFFF` host, `0x6002…` next to join; a rejoin gets a new number (`0x6003…`). With 3 players the expedition ids were `0x6001`/`0x6002`/`0x6003` on all three games; in the lobby before it the two joiners were `0x6003`/`0x6004`, so **ids are renumbered when the expedition starts** | FS-0, FS-1, run 21 |

Nightreign's layout here is Elden Ring's shifted by 8 bytes (Elden Ring has team type at `+0x6C`).

## 4. Team type values (`ChrIns+0x74`)

The names come from the community's list; the behaviour column is what we saw.

| Value | Name | What we saw |
|---|---|---|
| 1 | Player | A player in Roundtable Hold |
| 2 | White Ghost (co-op) | A player during an expedition. The normal value |
| 6 | Enemy | Most enemies. **A player set to 6 is ignored by enemies and cannot lock on to or damage them** |
| 7 | Boss | Bosses |
| 26 | Friendly NPC | Some NPCs |
| 29 | Indiscriminate | **A player marked 29 can be attacked by a normal (2) player.** If you mark *yourself* 29, sites of grace stop working, so the mod only marks rivals |
| 30 | Object | The extra pre-made player objects |
| 48 | Unknown enemy team | Some enemies |

How the mod uses it: on each player's game, the local player and teammates stay on 2, and every rival's copy is set to 29. A value
written this way was never reset by the game or by Seamless Co-op (held 15+ minutes, through deaths and the night boss).

## 5. A player's health

| Where | Type | What it is | Behaviour | Confirmed by |
|---|---|---|---|---|
| `[PlayerIns+0xB78]+0x70` | u32 | **The real health value** — **NOT reliable**: on 2026-10-21 (T-040 Nucleus, both games) `+0xB78` held a pointer into a list of same-class objects (vtables `exe+0x2C340D0` / `exe+0x2C33520`) and `+0x70` read 0xFFFFFFFF; FS-UI2 had it implausible too. Worked in FS-0 runs 9/15/18/19 and Nucleus 2026-10-21 00:54 | Writing 0 here on the owner's game downs the player at once (when it is the health) | FS-0 run 9; disproved as a fixed route by T-040 run 1 |
| `[[PlayerIns+0x1B8]+0x0]+0x140` / `+0x144` | u32 | Health / max health in the character data module (vtable `exe+0x2C10B88`), the module the game's own HP write `exe+0x428CDD` targets (§ "Character data" row below) | Matched PlayerGameData HP on both Nucleus games (240/240, 260/260) | `[VERIFIED: read on both games + poke 0 downed the player, T-040 run 1; the mod's re-down wrote it in both directions, T-040 run 2, 2026-10-21]` |
| `PlayerIns+0xBA0` | u32 | Copy of health | Follows the real value. Writing it does nothing lasting | FS-0 runs 7–9 |
| `[PlayerIns+0x5A0]+0x10` | u32 | Copy of health inside `CS::PlayerGameData` | Follows the real value; a write is overwritten immediately | FS-0 run 8 |

Seen values: 240 full, 0 while downed, 120 right after a revive (half), full again after a respawn.

## 6. Downed state, revive bar, and bleed-out (all inside the player)

| Offset | Type | What it is | Behaviour | Confirmed by |
|---|---|---|---|---|
| `PlayerIns+0x710` | u8 | **State** | 0 = normal, **1 = downed**, 2 = getting up after a revive (about 3.5 s) | runs 7, 14 |
| `PlayerIns+0x714` | f32 | **Revive bar (the purple one): amount left to fill** | Starts at **40 / 90 / 240** for the 1st / 2nd / 3rd down. Each hit subtracts (240 → 229 after one hit). At 0 the player gets up. 0 when not downed | runs 7, 14 |
| `PlayerIns+0x718` | f32 | **Bleed-out clock: seconds left** | Starts at about 14.9 and counts down in real time. **The player dies when it reaches 0.** 0 when not downed | run 14 |
| `PlayerIns+0x71C` | f32 | **Pause timer** | Set to about 3.5 when a hit lands, then counts down. **While it is above 0 the bleed-out clock is frozen** | run 14 |
| `PlayerIns+0x720` | f32 | Down-count cooldown | −1 before the first down. After a down, counts down from 180 while the player is up; frozen while downed. Likely the "penalty wears off after 3 minutes" rule | Likely (runs 7, 14) |
| `PlayerIns+0x730` | u8 (read as a byte by the game) | **Number of downs** | 0, then 1, 2, 3. Decides which bar size is used. Stays at 3 | runs 7, 9, 14; byte reads at `exe+0x66DF0B`, `exe+0x675033` (T-041) |
| `PlayerIns+0x731` | u8 | "Go down" request: in state 0 the per-frame function calls `exe+0x66F3B0(chr, 1)` when it is set; cleared every frame (`exe+0x67539F`) | Who sets it: UNKNOWN | `[VERIFIED: disassembly exe+0x674F21, T-041]` |
| `PlayerIns+0x732` | u8 | **"Revive completed"**: in state 1, on the owner's game, `exe+0x675159` reads it and calls `exe+0x66F3B0(chr, 2)` = start the get-up; cleared every frame (`exe+0x67539F`). **The bar does not decide the get-up**: one started with the bar at 40 (T-041 trace, Nucleus 22:42:36) | Who sets it: UNKNOWN (next trace) | `[VERIFIED: disassembly + trace, T-041]` |
| `PlayerIns+0x728` | u64 | Handle of the player whose hit emptied the bar (`exe+0x66E028`); all-ones when cleared | | `[VERIFIED: disassembly, T-041]` |
| Code: revive hit | `exe+0x66DEC0(victim, reviver, hit)` | bar -= max(1, trunc(hit+0x150 × downs factor × hit+0x258)); store at `exe+0x66DFC4`; bar 0 → `+0x728` = reviver handle; then pause `+0x71C` | runs on the victim's own game (main thread) | `[VERIFIED: trace RIP exe+0x66DFCC + disassembly, T-041]` |
| Hook: cancel a rival's revive hit | breakpoint on the store `exe+0x66DFC4` (`movss [rdi+0x714], xmm1`, bytes `F3 0F 11 8F 14 07 00 00`); rdi = victim, `[rsi+8]` = reviver handle; xmm1 low 32 bits = current bar for a RIVAL reviver | The bar never drops, the player script never sees 0, no revival animation; teammates untouched | `[VERIFIED: Nucleus 2026-10-22, both directions + 2 v 1, T-041]` |
| Code: who sets `+0x732` / `+0x731` | `exe+0x40D390` event dispatcher (163 types, jump table RVA `0x40E67C`, index = type−1): type **145** → `exe+0x40E5F1` `mov byte [rsi+0x732], 1`; type **144** → `exe+0x40E5E8` sets `+0x731`. The only other store is the PlayerIns constructor (`exe+0x66698B`). The revival animation emits type 145 every frame it plays | | `[VERIFIED: byte search + disassembly, T-041]` |
| Script: what starts the revival | `c0000.hks` `NearDeathCommonFunction` / `NearDeathStart_onUpdate`: `env(GetNearDeathHealth) <= 0` (env id 372, very likely the bar `+0x714`) and no SpEffect **102455** → `W_NearDeathRevival` (the start state latches it until its EzState flag 0). SpEffect 102455 disables this revival | handoff `tasks/T-041-no-getup-handoff.md` | `[SOURCE: community decompile; env 372 = bar HYPOTHESIS]` |
| Code: per-frame downed update | `exe+0x674EB0(chr, dt)` | state 1: pause countdown, bleed clock, bar regrows toward the max for this down (`exe+0x675141`, every frame), then the `+0x732` check; state 2 end → state 0 (`exe+0x67532E`) | | `[VERIFIED: trace RIP exe+0x675149 ×200/down + disassembly, T-041]` |
| `PlayerIns+0x1F9` | u8 | Flag byte | Bit 0x10 is set while downed | run 7 |
| `PlayerIns+0x728`, `+0x72C` | u32 ×2 | Unknown | Both read `0xFFFFFFFF`. Not used | run 14 |

**What writing does (owner's game only):**
- Clock: setting it to 2.0 while downed made the player die 3 seconds later instead of 10. The game keeps counting from the new value. **Confirmed.**
- Bar: writing the pre-hit value back sticks; the next hit subtracts from the restored value (run 15: `bar 58 -> 90 restored`, then 57, 55). **Confirmed.**
- Pause timer: writing 0 lets the clock run again at once (run 15: the clock kept falling between rival hits). **Confirmed.**

**Seen in run 21 (3 players, owner's game):**
- The bar grows back by about 9 per second while the pause timer is 0 and no hit lands (70 → 78.2 in 0.9 s). Whether it stops at the
  starting size: UNKNOWN. Our rule sets the pause to 0, so after a rival hit the growth starts at once.
- The amount one hit takes off the bar depends on the attacker: 14, 20 and 25–26 for the three players of run 21 (11 in run 14).

**Remote copies (another player's PlayerIns on my game):** the same block is used. State follows the owner. Bar, clock and pause are
refreshed only when a hit lands, with the owner's values at that moment; a value the owner writes afterwards is not sent. The copy's
bar is what the attacker's screen shows. **Confirmed** (run 18). Whether the copy's clock counts down by itself between hits: UNKNOWN.

**How a down ends:**
- State goes 1 → 0 with health still 0: the player **bled out**. Full health returns about 6 seconds later (respawn).
- State goes 1 → 2 → 0 with health at half: the player was **revived**.
- Each new down restarts the clock at about 14.9.

## 7. Who hit me

| Offset | Type | What it is | Behaviour | Confirmed by |
|---|---|---|---|---|
| `PlayerIns+0x1A0` | 8 bytes | **Last attacker's handle** | Holds the `+0x08` handle of whoever hit this player last. Stays set afterwards. Only meaningful on the victim's **own** game | runs 7–9 |
| `PlayerIns+0x198` | 8 bytes | Current attacker's handle | Same, but clears back to all-ones (`0xFFFF…`) later. Does not change when the same attacker hits again | runs 7, 12 |
| `PlayerIns+0x4A8`, `+0x538` | u32 | Hit pulse | Flip 0 → 1 → 0 for a moment on each hit | Likely (T-035 log) |

To tell teammate from rival: read the handle, find which player in the list has that handle at `+0x08`, and look up that player's team.

## 8. Position (not finished)

**The physics module holds the position. Writing it moves the player**. **A teleport that keeps world position (physics − origin, below) lands on the saved spot and stays: confirmed solo, run 4 (2026-10-14); other players see the move (Nucleus, 2026-10-14).** Code: `game/layout.rs` (position group), `game/poswrite.rs`. (T-028 run 1, solo, 2026-10-13; log `.claude/experiments/T-028/run1_solo_teleport.log`).

**What run 1 showed (9 writes, local player, solo expedition):**
- The chain is `[[PlayerIns+0x1B8]+0x68]+0x70`. The owner at `+0x08` is the PlayerIns. **Confirmed.**
- x and z arrive within 2 cm every time and are **not snapped back**. **Confirmed.**
- Height is not reliable. Saved while standing at y 193.35; after the write the ground byte went to 0 and the player fell to 191.58 (1.8 m lower) and stood there. Later writes ended at 188.2 and 185.7, and after the last one the player fell out of the world (y 185 to -171 in 9 s) and the expedition ended. Cause UNKNOWN.
- `+0x92` and `+0x93` read 1 while standing and 0 while falling. **Confirmed** as "on the ground".
- **The numbers are relative to an origin that moves** (run 2, log `run2_solo_teleport.log`). x and z stay within about -32..+32; when the player walks past the edge the value jumps back by about 32 m while the player keeps moving smoothly (hawk flight: z went 2.7 → 29.2 → 4.4 nine times in a row). y is not re-centred. So a saved x, y, z is only valid until the next jump: writing it later puts the player 32 m (or a multiple) away, often in the air. This explains the "wrong height" and "fell through" results of runs 1 and 2. **The origin is `PlayerIns+0x80`** (x, y, z, 1.0 as f32; y stays 0; `+0xE0` holds a copy). **World position = physics position − origin.** Confirmed in run 3 (walk, log `run3_solo_walk.log`): local minus origin is a smooth path across all 12 jumps (hawk flight and on foot). The origin moves in steps of 8 m (seen: 24, 32 m) and kept growing across a map-tile change, so one frame covers the whole expedition map. Elden Ring has a "chunk position" at the same offset.
- `PlayerIns+0x90` = the start position of this life in the same local frame (it shifts with the origin). `[HYPOTHESIS]` (Elden Ring `initial_position`).
- `PlayerIns+0x38` … `+0x44` hold map-tile ids in the Elden Ring format, e.g. `0x3C2B2800` = m60_43_40_00, which changed to `0x3C2C2700` when the player crossed into the next tile. Not needed for a move inside one expedition map.
- The start of the expedition (before the hawk) reads (0.00, 10.66, 67.00).
- **The hawk ride** (run 4, world position, samples every ~0.2 s): starts at y ≈ 162; a straight line (direction about (-0.6, 0, -0.8)
  on that map) at 16–18 m/s for about 12 s / 210 m, sinking to y ≈ 108. Ground byte `+0x92` = 1 for the whole ride. At release it turns 0,
  the horizontal speed stays about 17 m/s, and the player falls about 84 m in 3 s to the ground (y ≈ 24) with no damage. Anim id at
  `[time act]+0x20` read 9 throughout (no use as a ride marker). **Confirmed** (one run, one landing zone). What carries the player
  (hawk object, animation, attachment) and what ends the ride: UNKNOWN (PROJECT_PLAN §13).

Facts from the exe on disk:

| What | Where | How we know |
|---|---|---|
| Class of the object that holds the position | `CS::CSChrPhysicsModule`, table at `exe+0x2C13720` | `[VERIFIED: RTTI of nightreign.exe 1.3.3.0 on disk]` |
| Position x, y, z (f32) | `+0x70`, `+0x74`, `+0x78` in that object | `[VERIFIED: code at exe+0x46E5C1 reads [rbx+0x70]]`. Same offset as Elden Ring. The community table (§11) edits these three to move the player |
| "Standing on solid ground" byte | `+0x92` in that object | `[HYPOTHESIS]` The same code tests `[rcx+0x92]`; the name is Elden Ring's |
| Owner (the character) | `+0x08` in that object | Confirmed, run 1 |
| The character's module list | `[PlayerIns+0x1B8]`: a block of 57 pointers, no class | `[HYPOTHESIS]` Seen in the run 7 dump. Elden Ring has it at `+0x190`; in Nightreign `+0x190` is `CS::SpecialEffect`, which is why the T-028 probe found nothing |
| Which entry of the list is the physics module | `+0x68` (same as Elden Ring) | Confirmed, run 1 |

Older candidates (FS-0 run 6: numbers that move smoothly with the player). Reading only; none was written yet.

| Where | What it is |
|---|---|
| `PlayerIns+0x6C0` | Three numbers followed by 1.0 |
| `PlayerIns+0x6D0` | A second, slightly different set |
| `[PlayerIns+0x60]+0x1EC` | Inside `CS::PlayerCtrl` |
| `[PlayerIns+0x6B0]+0x17C`, `+0x18C` | Inside `CS::CSFD4LocationBodyScaleModifier` (a model/bone object: unlikely to move the player) |
| `[PlayerIns+0x5F0]+0x64` | Object with no class |

Answered later: the wrong height and the falls came from the moving origin (above); other players **do** see the move (Nucleus, 2026-10-14,
reported by Leonardo).
Still UNKNOWN: how the map tile
is stored and whether a far move must change it; what marks "landed after the hawk".

## 9. Things that turned out to be wrong

Kept here so nobody repeats them.

| Earlier belief | What is true |
|---|---|
| Elden Ring's path to the local player (`WorldChrMan+0x1E508`) works in Nightreign | It doesn't. Use the player list at `+0x9A40` |
| Byte +9 of a player-list slot marks "downed" | It doesn't change on a down. The state is `PlayerIns+0x710` |
| `WorldChrMan+0x1C820` points to the main player | It held text. Not a player pointer |
| Health in `PlayerGameData` is the real health | It's a copy. The real one is `[PlayerIns+0xB78]+0x70` |
| A stopwatch counting up from the moment of a down (found by the scanner) is the bleed-out clock | It was a short-lived effect timer and did not reappear. The clock counts **down** at `PlayerIns+0x718` |
| The revive bar starts at 0 and fills up | It starts at the required amount and counts **down** to 0 |
| Elden Ring's route to a character's position works | It doesn't. See §8 |
| Setting health to 0 kills a player | In a multiplayer session it only downs them. Death comes from the bleed-out clock reaching 0 |

## 11. Leads from a community cheat table (not yet seen in the live game)

Source: `nightreign v1.01.4.CT` by Matthew80 (written for exe 1.1.2.0 / 1.1.3.0), read on 2026-10-12. The table was **not run**.
Its byte patterns were searched in the 1.03.3 `nightreign.exe` file on disk. "Code at" is where the pattern sits in 1.3.3.0 (one
match unless noted). The offsets are the table author's; what each object is and how to reach it from `PlayerIns` is `[HYPOTHESIS]`.

| What | Offsets (table) | Code at (1.3.3.0) | Pattern | Our guess at the object |
|---|---|---|---|---|
| Health / max health, FP / max FP, stamina / max stamina | `+0x140` / `+0x14C`, `+0x150` / `+0x154`, `+0x15C` / `+0x160` | read `exe+0x7CBEBC`; **write** `exe+0x428CDD` (`mov [rdi+0x140],eax`) | `8B 88 40 01 00 00 89 8F`; `89 87 ?? ?? ?? ?? 33 D2 49 8B CE` | The character's data module (Elden Ring has the same fields 8 bytes lower). Relation to `[PlayerIns+0xB78]+0x70`: UNKNOWN |
| Position x, y, z (the table edits these to move the player) | `+0x70`, `+0x74`, `+0x78` | `exe+0x46E5C1` (`movups xmm3,[rbx+0x70]`) | `0F 10 5B 70 F3 0F 10 93 98 01 00 00` | `CS::CSChrPhysicsModule` (§8). The table hooks this code and stores `rbx` (the object) with no filter, so its three "pos" rows point at whichever character ran the code last |
| Runes, level | `+0x6C`, `+0x68` | `exe+0x830E3D` (`mov eax,[r14+0x6C]`) | `41 8B 46 6C 89 86 1C 01 00 00` | `CS::PlayerGameData` = `[PlayerIns+0x5A0]` (same offsets as Elden Ring) |
| Murk, sovereign sigil (kept between runs: **never write**) | `+0xD0`, `+0x5C` | same | same | same object |
| Vigor … arcane (8 values) | `+0x34` … `+0x50` | pattern not found in 1.3.3.0 | n/a | same object |
| "No hit" flag | byte `+0xF0`, bit `0x40`, in the object at `[X+0x60]` | `exe+0xC3676A` (the game clears the bit here) | `48 8B 40 60 80 A0 F0 00 00 00 BF` | `[PlayerIns+0x60]` (already a §8 pointer). What the table writes: UNKNOWN |
| Damage of a hit being applied | `+0x258` in the table, **`+0x280` in 1.3.3.0** | `exe+0x43BFB1` | `41 8B 96 ?? ?? ?? ?? 85 D2 0F 9F C0` | A per-hit damage record |
| "Use an item" removes it from the inventory | a call | `exe+0x1F0840` | `E8 ?? ?? ?? ?? 44 8B F0 FF C7` | The called function is a lead for §10 "inventory" |
| Ultimate charge now / needed; skill cooldown | `+0x14` / `+0x18`; `+0x24`, `+0x28` | `exe+0x48B859`; `exe+0x2E0972` and `exe+0x48C03B` | `F3 0F 10 43 14 48 83`; `F3 0F 58 77 24 0F` | UNKNOWN |
| Enemies ignore the player ("stealth") | byte `+0xD4` | `exe+0x2F5D02` | `74 ?? C6 81 ?? ?? ?? ?? ?? E9 ?? ?? ?? ?? 80 B9` | UNKNOWN |
| Night's Tide speed multiplier | a float constant multiplied into the tide timer, and a rate at `+0x118` | **pattern not found in 1.3.3.0** | n/a | n/a |

## 12. Online: Steam lobbies and the game's multiplayer code (matchmaking)

Not memory values but code and Steam facts the matchmaking depends on. How they are used: `.claude/MATCHMAKING.md`. Proofs: S9 §5c–§5g.

| What | Where | Behaviour | Confirmed by |
|---|---|---|---|
| Steam API the game uses | the game's own `steam_api64.dll`; flat exports found by name (`SteamAPI_ISteamMatchmaking_CreateLobby`, …) | Each flat export is a 2-instruction jump: `mov rax,[rcx]; jmp [rax+disp]` into the interface's table in `steamclient64.dll` | MM-3c log 2026-10-14 |
| `ISteamMatchmaking::CreateLobby` | table slot 13 (`disp 0x68`) | Calls from our mod pass through `nrsc.dll+0x2BEC2` first: SC wraps the matchmaking interface | MM-3c (call stack of our own queue lobby) |
| `ISteamFriends::ActivateGameOverlayInviteDialog` | table slot 33 (`disp 0x108`) | **The game calls the table target directly**; the flat export is never hit. Takes the lobby id, returns nothing. Skipping it (return at once) leaves no side effect we could see | MM-3c; MM-3f solo; FS-MM3 |
| Invite Members creates the multiplayer lobby | call to `CreateLobby(Public, 6)` at `exe+0x25919A1`, called from `+0x2595B68` ← `+0x258E157` ← `+0x25857D4` ← `+0x257F5DA` ← `+0xDEC332` (return addresses) | Runs when the player picks Matchmaking Settings > Invite Members and no lobby exists yet. SC then writes its tags on it | MM-3c |
| Invite window call | returns to `exe+0x258568B` | About 0.3 s after the lobby is created. This is the call MM-3f skips | MM-3c; MM-3f; FS-MM3 |
| This code region | around the call sites above (`exe+0x257F5DA` … `+0x25919A1`) | No unwind data (`.pdata`) and jump-chain obfuscation: static reading is slow. A live trace (hardware breakpoints) works | MM-3c, Python/capstone read of the exe |
| Lobby made by the game | Steam lobby data `inviteParam = "0"` (the game), `nrsc_lobby_type = "nrsc_ynx3_seamless_master_lobby"` and `nrsc_lobby_version = "1.1.3"` (SC); Public, 6, locked | Joinable: other games enter and load into the host's Roundtable | FS-MM1, FS-MM3 |
| Lobby made by **our** `CreateLobby(Public, 6)` | SC adopts it too (writes its 2 tags at once), but no `inviteParam` | **Not joinable**: a joiner's game enters and leaves within 0.1 s; a direct `JoinLobby` gives Steam membership but no session | MM-3d, FS-MM2 |
| Second, unlocked lobby at expedition start | host creates it ~50 s before the expedition loads; the other player enters it 0.7 s later | Purpose UNKNOWN (likely the expedition's own session). Our mod ignores it | FS-MM3 |
| Phase from team type | local player `ChrIns+0x74` (§4): 1 at the Roundtable, 2 in an expedition | The gate uses it for "match started" and "match over" | FS-MM1, FS-MM3 |
| Join link | `steam://joinlobby/2622380/<lobby>/<host>` | Steam passes it to the running game as `GameLobbyJoinRequested_t`; the game joins by itself (works only for a lobby made by the game) | FS-MM1, FS-MM3 |

## 13. Text and event flags (UI-0, game 1.03.3)

| What | Where | Behaviour | Proof |
|---|---|---|---|
| `LookupEntry(repo, slot, category, id)` | `exe+0x27F4D20` | Returns the text of FMG `category` (the FMG file id in the msgbnd: 200 CL_MenuText, 33 EventTextForTalk, 34 EventTextForMap, 500+ the `_dlc01` twins) entry `id`. Every lookup asks the `_dlc01` FMG first, then the base one. **Seamless Co-op hooks it** (first 5 bytes = `jmp` into memory outside any module) and swaps the Number of Players labels 130409/130417/130408 | `[VERIFIED: disassembly + UI-0 step 5 trace]`; which ids SC swaps `[HYPOTHESIS]` |
| FMG search `(fmg, id)` | `exe+0x27F5710` | `LookupEntry` tail-jumps here; not hooked | `[VERIFIED: entry bytes]` |
| `MsgRepository` table | `[exe+0x3C246A8]`: `+0x8` table, `+0x10` slot count, `+0x14` category count; FMG = `[[+8] + slot*8] + category*8` | Read by `game/texttrace.rs` to map FMG pointers back to categories | `[VERIFIED: LookupEntry bounds checks; trace mapped every FMG]` |
| `CSEventFlagMan` | slot `exe+0x3C115A8` | ER layout holds: divisor 1000, a red-black map of group → block; 143 groups have storage (list in `research/UI-0-ingame-ui.md` step 5). Flag 6001 ON / 6000 OFF, 7500 ON at DAY I | `[VERIFIED: UI-0 step 5 probe]` |
| Our flags | group **10007** (10007000..10007999), 10008 spare | Storage present, no use in any NR talk ESD or EMEVD. 10007000 set by our table list and read by the DLL | `[VERIFIED: UI-0 step 6]`; use by the exe or params UNKNOWN |
| Commence checks | `exe+0x9EEBE0` (ordinary function, has `.pdata`), called once per Commence press from `exe+0x9E81AE` / `exe+0xA4246E` with `rcx` = the menu; our breakpoint sits at `exe+0x9EEBFB` (first instruction after the prologue, `r14` = menu) | Checks 506040 / 506070 / 506024 / 506023 in turn. Jumping to `exe+0x9EEC1D` runs the game's own 506040 notice dialog and leaves through the normal epilogue. **FS-UI2: the host's start of the expedition with a Seamless Co-op party (3 rounds) logged no press here** `[HYPOTHESIS: SC's party start takes another path, or the settings pointer was not readable]` | `[VERIFIED: UI-1 trace + C1 prototype 2026-10-19; FS-UI2 2026-10-21]` |
| Number of Players byte | `[[menu+0xFC8]+0x49]` (the list `exe+0x9FB020` reaches the same object as `[rcx+0x3E08]`) | **2 = one player, 1 = two, 0 = three.** Commence with 0/1 offline = 506023 "Cannot start multiplayer session in offline mode"; with 2 = departure. Seen at 0 again on the friend's PC after a round as host (FS-UI2; cause UNKNOWN) | `[VERIFIED: UI-1 trace; 0x9EF3F0 picks 130304 / 130305 from it]` |
| Commence dialog text | CL_Dialogues (category 204) entry 506040's offset slot: `[[fmg+0x18]+idx*8]`, string = `fmg + offset` | One aligned 8-byte store points the dialog at our own UTF-16 text; the open dialog copies the text when it opens (does not follow later changes) | `[VERIFIED: C1 prototype, Leonardo saw the text]` |
| CL_MenuText getter | `exe+0x7AC800` (`edx` = id) | Breakpoint here swaps 130409/130417/130408 → our 99100100..102 before SC's `LookupEntry` hook sees them: the list reads Solo / Duo / Trio | `[VERIFIED: UI-1 step 4 solo; FS-UI2 on two PCs]` |

## 14. The hawk ride: the script's `env()` answers (HAWK-A2/HAWK-3, game 1.03.3)

| What | Where | Behaviour | Proof |
|---|---|---|---|
| Lua `env` stub | `exe+0x1624170` | `eax = [exe+0x467AFE0](L)`, then pushes `eax` (32-bit) as the answer | `[VERIFIED: static, 2026-10-22]` |
| Env callback | `exe+0xD403F0` (put in the slot by the constructor `exe+0xD400B0`) | Reads the id (Lua arg 1), then **tail-jumps** to `[[exe+0x3C1D3D8] + 0x10]` with `ecx` = id, `rdx` = L, `r8` = `[[ctx]+0x28]` (`ctx` from `exe+0x15C93C0(L)`) | `[VERIFIED: static, 2026-10-22]`; `r8` = the PlayerIns (row below) |
| Env object | global `exe+0x3C1D3D8` (made at `exe+0xD5775A`) | `+0x10` = the env switch, **`exe+0x408030`**, set at run time (where: UNKNOWN). Stays the same across area loads. `game/hawk/` swaps it for a pass-through (one atomic store); no crash, 0 ms cost | `[VERIFIED: HAWK-3 run 1, 2026-10-22]` |
| `r8` of the switch | | **The character's PlayerIns** (LOCAL's in every expedition) | `[VERIFIED: HAWK-3 run 1]` |
| 377 `GetBirdEventID` | answered by the switch | −1 idle; **4** for one or two frames when the start ride begins; 0 blips during rides; **2** at the route end (one or two frames). Answering 2 ourselves is accepted (359 answers, no error); answering 1 instead of 4 makes the start ride run the tree-hawk loop (394 then reacts to the stick) | `[VERIFIED: HAWK-3 run 1]`; on-screen effect pending |
| 394 / 384 | | 394 direction time: rises ~1,600/s while the stick is held on a tree-hawk ride, 0 on a vanilla start ride. 384 escape time: −1 on the route, counts up while off it | `[VERIFIED: HAWK-3 run 1]` |

## 15. NPC bots: `CSNpcBotMan` (OFF-3, game 1.03.3)

Dormant in vanilla play (no shipped event script starts it). Story and params: `research/OFF-3-bots.md`.

| What | Where | Behaviour | Proof |
|---|---|---|---|
| `CSNpcBotMan` | FD4 singleton, slot `exe+0x3C0B518` | Re-created on every area load. Ticked every frame by the world update (`exe+0x523728` → `exe+0x38D280`) | `[VERIFIED: singleton log 2026-10-22 + static]` |
| Bot table state | `+0x0` table id (−1 none), `+0x4` armed byte, `+0x0C` (NpcParam, NpcThink, CharaInit) triples (12 bytes each; room for 2), `+0x40` f32 timer (−1.0 when armed) | Filled by `exe+0x38CF70` (event `2003[97]`), cleared by `exe+0x38D110` (`2003[98]`). The table id shows in game within 50 ms of our flag; the armed byte is consumed within one frame (always 0 at 50 ms samples) | `[VERIFIED: static 2026-10-22; OFF-4 run 2026-10-22]` |
| Spawned bots | vector `+0x28`..`+0x30`, 16-byte entries: `+0x0` the bot character's handle (−1 = empty; seen `0xFFFFFFFF10E00000` / `…01`), byte `+9` = "remove". **Exactly 2 entries**: the constructor `exe+0x38CB40` sizes it to the constant 2 | `2003[98]` finds each character (`exe+0x518F30`) and calls `exe+0x3CD800` on it: both bots gone, entries back to −1 within 1 s. **Writing −1 into a slot handle** makes the manager forget that bot: the bot stays in the world with its AI, and the next `2003[97]` fills the slot with a new bot | `[VERIFIED: static + OFF-4 run + OFF-5 run 1, 2026-10-22]` |
| Character sets (handle lookup) | `[WorldChrMan+0x170B8 + i*8]`, i = 0..=0x84 | The game's handle lookup `exe+0x518F30` picks a set and an entry from the handle (decode tables filled at run time); our bots' handles `0x…10E0000n` are in **set 14** (bits 20..27, the ER encoding). OFF-5 searches the sets with the player ChrSet layout (`+0x10` count, `+0x18` entries, 16 bytes each) and accepts an entry only when its `ChrIns+0x08` handle matches. Bot ChrIns: `+0x68` = 600040000 (its NpcParam id), team 2 as spawned | `[VERIFIED: static + OFF-5 run 1, 2026-10-22: 4 bots found in set 14]` |
| Solo gate | `[[exe+0x3C13258]+0xDC8]+0x14` (i32) | `2003[97]` does nothing when it is above 1. What it counts: UNKNOWN (party size `[HYPOTHESIS]`) | `[VERIFIED: static]` |

## 10. Not found yet

- The table that says which team is hostile to which (we only know the values in §4 from testing).
- Level and rune count (expected inside `CS::PlayerGameData`; not read yet).
- Inventory and the game's "drop item" function.
- The expedition clock (the day/night flags are readable since UI-0: 7500 ON at DAY I, §13).
- Each player's Steam ID inside the game (needed to map the queue roster to expedition players for Duo/Squad teams, MM-6). The
  Steam IDs themselves are known from the lobbies.
- The game function behind Invite Members that we could call ourselves (MM-3g); only its call chain is known (§12).
