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
| `[PlayerIns+0xB78]+0x70` | u32 | **The real health value** | Writing 0 here on the owner's game downs the player at once | FS-0 run 9 |
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
| `PlayerIns+0x730` | u32 | **Number of downs** | 0, then 1, 2, 3. Decides which bar size is used. Stays at 3 | runs 7, 9, 14 |
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

| Where | What we saw | Status |
|---|---|---|
| `PlayerIns+0x6C0` | Three numbers that move smoothly as the player moves, followed by 1.0 | Candidate. Reading only |
| `PlayerIns+0x6D0` | A second, slightly different set of three | Candidate |
| `[PlayerIns+0x60]+0x1EC`, `[PlayerIns+0x6B0]+0x17C`, `+0x18C` | More position-like sets | Candidates |

Which one actually moves the player when written is **unknown**. The snap-back test for this has not been run. Needed for separate team spawns.

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

## 10. Not found yet

- The table that says which team is hostile to which (we only know the values in §4 from testing).
- Level and rune count (expected inside `CS::PlayerGameData`; not read yet).
- Inventory and the game's "drop item" function.
- The expedition clock and the day/night flags (the flag system's layout checked out structurally; no flag has been read on purpose).
- Each player's Steam ID.
