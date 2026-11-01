# R2: how the game creates a remote player (static, nightreign.exe 1.3.3.0)

Goal: make extra **players** (entries of the player ChrSet `WorldChrMan+0x9A40`, class `CS::PlayerIns`) inside one game, without a
second client. Tools: `tools/re/` (`nr.py` from the mod repo's M5-8 helpers, `vt.py` RTTI finder, `summ.py`). All RVAs are `exe+`.

## Chain (VERIFIED: static disassembly, 2026-10-23)

| RVA | What | Evidence |
|---|---|---|
| `0x2C307F8` | `CS::PlayerIns` vtable; constructor `0x6667F0` (object size `0x7A0`, 16-aligned alloc) | RTTI + `lea` at `0x666830` |
| `0x3E58E0` | **Create a network PlayerIns**: formats the name `"%s_%04d_Net"` (`%04d` = `desc+0x20`), allocates `0x7A0`, calls the constructor with `desc`, then `vcall +0x40`, links the chr into the world (`0x3D0100`, `+0x138`, `0x3D75B0`, `0x3D5ED0`, `0x3D5CB0`). Args: `rcx` = holder, `rdx` = &ChrSet entry, `r8` = name base, `r9d` = id, `[rsp+0x28]` = `desc` | disasm |
| `0x4AC240` | **`ChrSet<PlayerIns>::CreateNetPlayer(set, u32 id, desc*)`**: first free entry with index ≥ 1 (slot 0 = local), builds the name with `0x63FE50([WorldChrMan+0x9910]+0x120, slot)`, calls `0x3E58E0` for that entry. Returns the PlayerIns or 0 when full | disasm |
| `0x518280` | **`WorldChrMan::CreateNetPlayer(wcm, u32 id, desc*)`**: `rcx += 0x9A40` → `0x4AC240`; then `player->vcall[+0x610](wcm[+0x17560])` | disasm |
| `0xC445A0` | Remote-join handler (no `.pdata`): `id = session[+0x98]` if it is in −1..0x17 (`0x20BB20`), `desc` = `0x3E4830(&desc, session, &item[+0x10])` (`desc+0x20 = session[+8]`, `+0x24 = 3`, `+0x28 = session`, `+0x30 = key`), calls `0x518280(WorldChrMan, id, &desc)`, then `0x3D5FE0(player, id)` (writes `player+0x70 = id`), `0x66F870(player, item)`, `0x3D7490(player, 5)`, module refreshes through `player[+0x1B8]`, and registers with `[exe+0x3C078D0 …]+0x40` | disasm |
| `0xC43B94` | Caller: once per update, when `CSSessionManager` (slot `exe+0x3C21EC8`) has a pending entry (`[mgr+8] != 0`), pops it (`0xDF1C20`), maps its key to a member index (`0xDF0470`), gets the member object (`0x1FE530`: `[[exe+0x3C078D0]+0x10] + idx*0x8A0`, idx ≤ 1) and calls `0xC445A0` | disasm |

Other creators of `PlayerIns`: `0x4AC170` (slot 0 only: the local player), `0x4AB970` → `0x3E4AD0` (`"%s_%04d"`, from map data),
`0x4ABC40` → `0x3E51F0` (the generic ChrSet creator the NPC bot manager uses, `0x38E980`), `0x3E56A0` → `CSJamaisVuReplayPlayerIns`.

## Consequence for puppets [HYPOTHESIS until the live test]

Calling `0x518280` (or `0x4AC240`) from our DLL **on the game thread**, with a `desc` built the same way, should put a real
`PlayerIns` into a free player slot. Unknowns the live test must answer:

1. Which session object to pass (the member table holds 2 entries; reuse LOCAL's, or a zeroed copy).
2. Whether the new player stays in `NetworkInitializing` waiting for network data; if so, copy LOCAL's `PlayerGameData` into it and
   set the entry's load status.
3. Whether it can be hit, gets a downed block (`+0x710`), and whether SC or the game removes it after a while.

## What was built instead, and what the live runs showed (2026-10-23, solo, offline mode)

The network path above needs a session member to bind to, so the puppets use the **bot manager's creator** and change only its
target set. Everything below is `[VERIFIED: live, dev build of nightreign-pvp branch claude/orchestrator-puppets]` unless tagged.

| Step | Where | What happens |
|---|---|---|
| Bots are PlayerIns | `CSNpcBotMan` spawn step calls `exe+0x4ABC40(WorldChrMan+0x9AF0, request)` at `exe+0x38F2B3`; that creator builds `CS::PlayerIns` (`exe+0x3E51F0` → ctor `exe+0x6667F0`) | static |
| Redirect | execute breakpoint at `exe+0x38F2B3`: `rcx` := `WorldChrMan+0x9A40` (only when a player slot 1..5 is free; the creator scans 20 entries whatever the capacity); request `+0x44` (NpcThink) := −1 → no AI, the puppet stands still | handles `0x…10C0000N` = the player set, same as LOCAL `0x…10C00000` |
| The bot manager must forget it | at `exe+0x38F2EE` (just after it stores the handle at `[r15+[rdi+0x28]]`) write −1 there; otherwise it destroys the character it cannot find in its own set | without it the puppet was gone within 50 ms |
| **Seamless Co-op destroys it** | SC's per-frame sweep (live code in `nrsc.dll` around `+0x62644`, read-only): for each player slot k = 1..5 it reads `[chr+0x5D0]` and looks for its `+8` key among its 5 session members (`+8` key, `+0x128` bit 0 = active); no match → `call [vtbl+0x50]` = the game's **destroy character `exe+0x51B2B0(WorldChrMan, chr)`**. A null `+0x5D0` is skipped by SC, but the game's player update calls `[chr+0x5D0]->vf[0x30]` every frame (`exe+0x676227`) → crash. In solo all 5 SC members are inactive | write watch on the slot (`exe+0x397BFB` clears it, called from `exe+0x51B453`) + execute trace on `exe+0x51B2B0` (return address `nrsc.dll+0x62AB4`) |
| Guard (no SC change) | execute breakpoint on `exe+0x51B2B0`: when the return address is inside `nrsc.dll` and `rdx` is a puppet → return at once (pop the return address). Every other destroy runs | puppets stable for minutes; "SC destroys skipped" ≈ 4 per frame per puppet |
| `+0x5D0` | puppets keep the game's own `PlayerNullSession` (static object, vtable `exe+0x2CA4A60`, key 0); LOCAL has a `PlayerDebugSession` (vtable `exe+0x2CA49C8`, key 0) | peek |
| Player id | `ChrIns+0x78` := `0x600N_FFFF_FFFF` (N = slot + 1) so the mod's team rules see distinct players (the creator copies LOCAL's `0x6001…`) | `players`: p2..p6 rivals |
| Load status | the entry's status byte reads 3, then 4 by itself within a second or two; forcing 4 earlier crashed (`exe+0x404FE5`, null +0x30) | live |
| Position | puppets share LOCAL's physics frame (`[[chr+0x1B8]+0x68]+0x70`); a puppet's own `+0x80` is **not** an origin. In an expedition they appear ~2 m from LOCAL; in the Roundtable at the map origin | peek; `puppet_move` |
| Hits | the local player's sword damages them (720 → 662 …) | live |
| **Down instead of death** | solo play kills at 0 HP (`IsDirectDeath`, research/M5-8-death.md); SpEffect **8970061** turns that off but also stands the player up after ~5 s (the solo auto-revive). So the puppet holds 8970061 while up and loses it while downed: the add (`exe+0x609F40(chr, id)`) / remove (`exe+0x601210(chr, id)`) calls are made on the game thread by turning SC's per-frame destroy call into a tail call to them (and once at creation, by redirecting the creation-site thread into the add with a return to the same instruction) | down: state 1, bar 40, clock 14.97 → 0, then state 0 with HP 0 (dead, stays dead); the mod's T-033 `DOWN (downs=1 …)` / `UP`, T-039 BLOCK; the game's HUD shows "Near death" |
