# M5-8 — Controlling the game's death (kill now, stay dead, come back where we say)

Card: TASKS M5-8 (done 2026-10-23). Feeds M5-9 (Solo real death) and M5-7 (spectate). Game 1.03.3 (exe 1.3.3.0), Seamless Co-op
1.1.3, me3 0.11.0. Static reading with `experiments/M5-8/nr.py` (+ `rtti.py`), the player script decompile
(`Z:\Tools\nr-mods\Spectral haw flying\mod\action\script\c0000.hks`, a community decompile of the vanilla `c0000.hks`), the 1.03.3
event files (decoded with `experiments/M5-1/emdec.py`) and the regulation (WitchyBND). Live checks: Leonardo solo, 2026-10-23
(TESTING §3 "M5-8 solo death trace"), logs `experiments/M5-8/nrpvp_2026-10-23_02-*.log`.

**Verdict:** GO for M5-9. All three pieces exist in the game and the DLL can drive them without patching code: a SpEffect that makes
0 HP a real death, a hold on the comeback that the DLL can release later (proven solo), and the game's own respawn at a point. Not
proven yet: the same in a multiplayer session (Nucleus, with our event files now loaded through me3; M5-9 acceptance).

## R0 — Our game-data package in the Nucleus windows

| Finding | Tag |
|---|---|
| Nucleus starts every window with `Game.LauncherExe` from its handler (`D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN.js`, was `nrsc_launcher.exe`) inside that window's own copy of the game folder (`Instance<N>\`, the handler folder is copied in; the exe is copied, not linked). Steam is emulated inside the game (`winmm.dll` → `coldloader.dll` from `dlllist.txt`), so it does not care who starts the exe | `[SOURCE: the handler script]` |
| Our player launcher `nrpvp_launcher.exe` already runs me3 with `-e <its folder>\nightreign.exe` and SC + nrpvp.dll + our package from `NightreignPvP\` next to it. So the handler only has to name our launcher. `scripts\deploy-nucleus.ps1 -Me3` (new) does that: the player-zip layout in the handler, nrpvp.dll out of `dlllist.txt` (me3 loads it), `LauncherExe = "nrpvp_launcher.exe"`, and `NightreignPvP\me3-args.txt` = `--skip-steam-init true` (new optional launcher file: Steam is closed under Nucleus). A deploy without `-Me3` (or `-Remove`) puts SC's launcher back | `[VERIFIED: deployed 2026-10-23; files checked]` |
| That the windows start, join each other and load our event files this way | **Window 1 yes, window 2 no**: "Failed to read coldloader.ini: appid not found in coldloader.ini or steam_appid.txt" (Nucleus's Steam emulator, before me3 attaches); a `steam_appid.txt` in the loader folders did not help. `[VERIFIED: Nucleus 2026-10-23, TESTING §3]`. Cause UNKNOWN; Nucleus stays on SC's launcher, and event-file features are tested solo or in a friend session |

## R1 — Kill a player on demand

| Finding | Tag |
|---|---|
| **SpEffect 102130 = "die at once, no downed state".** The player script decides death vs downed at 0 HP: `ExecNearDeath()` returns false (no `W_NearDeathStart`) when `IsDirectDeath()` is true, and `IsDirectDeath()` is true when the character has **102130** (also: no hero, a fall into a death zone, or solo play outside a boss fight without 8970061). `NearDeathCommonFunction()` also ends an existing downed state at once with 102130 (`W_NearDeathEnd`) | `[SOURCE: c0000.hks decompile, IsDirectDeath / ExecNearDeath / NearDeathCommonFunction]` |
| Vanilla uses it: `m31_90_00_00` event 31902265 `Set SpEffect(20000, 102130)`. Row: `effectEndurance -1` (stays until cleared), category 20, no state, no icon | `[VERIFIED: event file + regulation]` |
| 102130 overrides revive items in the script, so the DLL must give it only to a player **without** one. The items the script respects: SpEffects **540155, 6999100, 6999500, 8970061** (the solo direct-death check; 540155 / 6999100 are also the vanilla revival flags 7010–7012 of `common` 1183) | `[SOURCE: decompile; VERIFIED: 1183]` |
| The DLL reads a character's SpEffects itself: list `[[PlayerIns+0x190]+8]`, id at node `+8`, next at node `+0x38` (the game's own walk `exe+0x509150`) | `[VERIFIED: disassembly + solo run: 20 ids read, 6999500 among them]` |
| **The switch works in game:** our event 99100805 (`gamedata/emevd-nights.json`, per game) gives the local player 102130 while our flag **10007420** is ON and clears it when it goes OFF. Solo run: ON → 102130 in the list within 0.8 s, OFF → gone | `[VERIFIED: solo run 2026-10-23 02:14 (console flag + speffects)]` |
| That a 0-HP hit then kills in a session (death animation on every screen) | `[HYPOTHESIS, strong]` (solo play already dies directly, so solo cannot show it): M5-9 Nucleus step |
| Alternatives (not needed): event `2004[4] Force Character Death` is never used on players in vanilla; its body is `exe+0x601410(chr, runes)` → `exe+0x3CE6D0`. Bank 2004 dispatcher `exe+0x587DE0` (case table `exe+0x58AA04`, index id−1); per-character add / clear SpEffect `exe+0x609F40(chr, id)` / `exe+0x601210(chr, id)` | `[VERIFIED: disassembly]` |

## R2 — The comeback, and holding it

| Finding | Tag |
|---|---|
| Solo by day the player comes back ~9–11 s after death, 13–90 m away, full HP | `[VERIFIED: solo runs 1–3]` |
| **The comeback is a chain of timed callbacks on the Lua event sequencer** (`CS::CSLuaEventProxy`, vtable `exe+0x2C259F0`), built by the death handler `exe+0x5C2770` (it reads flags 9017 / 9015 and picks the text 3000/3010 "YOU DIED") with `exe+0x5BBD90(seq, 0xFA6, callback, delay)`: `exe+0x5C3510` waits, then **`exe+0x5C4810(ctx, seq)`** queues the fade `exe+0x5C3DB0` (0.5 s), which queues **"begin respawn" `exe+0x5C3F80`** → `exe+0x66E4A0(LOCAL)` (sets `+0x738` = 1, `+0x1F4` bit 0x40, sends net message 0x1392), which after 1.0 s queues **`exe+0x5C4360`** → `exe+0x66E8E0(LOCAL, pos, rot, flag)` (HP refilled by `exe+0x426B4F`, position set) | `[VERIFIED: disassembly + solo trace: write watch on HP and +0x1F4, then execute breakpoints on the three steps, run 3/4]` |
| Remote copies come back through `exe+0x675480` (per-frame, `+0x738` / `+0x734` timer) after the owner's net message, or through the net handler `exe+0x4F3570` (types 0/1/2 → `exe+0x66E4A0` / `exe+0x66E8E0` / `exe+0x66E3D0`) | `[VERIFIED: disassembly]` |
| `CS::RejoinMapEnterStep` (`STEP_Branch_DyingOrRespawn` `exe+0xC3F0F0`, `STEP_RespawnEnter` `exe+0xC3F230`) is **not** on the solo death path (no hit in run 1); it is the session rejoin path | `[VERIFIED: solo run 1]` |
| **Hold:** an execute breakpoint on `exe+0x5C4810` whose handler returns at once (RIP = return address, RSP + 8; the step returns nothing) keeps the dead player dead: no fade, nothing after it is queued. The step was reached 4.5 s after the death; held 94 s | `[VERIFIED: solo run 4, 2026-10-23 02:37]` |
| **Release:** call the held step with its saved `(ctx, seq)` on a game thread: the dev tool redirected the next entry of `exe+0x675480` (a per-frame character update) into `exe+0x5C4810` with a return into the gate, then put the gate's arguments back. The game's own comeback followed (begin respawn 2.5 s later, respawn 1.1 s after that, full HP, 90 m away); Leonardo: "respawned normally, no issues" | `[VERIFIED: solo run 4]` |
| The release ran on thread 22468 while the sequencer runs on 22184. It worked, but M5-9 should call from the sequencer's own thread: the Lua event proxy's update `exe+0x5BCE10(proxy, x)` (called from `exe+0x5B7430`), which owns the steps | `[HYPOTHESIS]` (that 0x5BCE10 runs every frame on 22184): M5-9 logs the thread of both |

## R3 — What a dead (held) player sees and can do

| Finding | Tag |
|---|---|
| Solo, held: "YOU DIED" shows, the camera stays on the body and can be turned; no other input, **the Esc menu does not open** (vanilla: no menu while dead). HP 0, state byte 0, team stays 2, `+0x1F4` low byte 0x80 | `[VERIFIED: solo run 4 + log]` |
| In a session, by day (Nucleus 2026-10-23): held 55 s, team stayed 2, SC kept the session, **the other window shows no body**; released → normal respawn on both screens | `[VERIFIED: Nucleus N1']` |
| At night in a session: team 77? does `common` 1310 react? | UNKNOWN: next friend session (needs our event files) |
| Esc while dead (Leonardo asked, 2026-10-23): not now. The spectator card M5-7 must at least let a dead player quit | decision |

## R4 — The game's respawn points

| Finding | Tag |
|---|---|
| `CSRespawnPointMan` (FD4 singleton, slot `exe+0x3C17C88`): `exe+0xBA2B80` picks a point (`_FindRespawnPoint` `exe+0xBA5100`; fallback `exe+0xBA5530`) and stores block id `+0x18`, position `+0x20`, rotation `+0x30`, valid byte `+0x40`. The search walks the points (MSVC map at `+8`, point at node `+0x28`), keeps only points **inside the current play area** (`WorldPlayAreaManager` check `exe+0x68B750`) and takes the **nearest** to the player (height weighted by `GameSystemCommonParam +0x220`), preferring points whose flag condition passes | `[VERIFIED: disassembly]` |
| So the game respawns a player at the nearest respawn point inside the circle (seen 13–90 m) | `[VERIFIED: solo runs]` |
| **For "a random place at Day 2":** keep the game's respawn (safe ground) and then move the player with the proven teleport (T-028) to a random place: a pool of ground positions that players stood on during the day (`PhysicsModule +0x92` = 1), at least 150 m from every other standing player. Reading the point objects themselves needs their position getter `exe+0xE417B0` (not read) | design; `[HYPOTHESIS]` that day-1 ground samples are always safe (they were standing there) |

## What M5-9 builds (card in TASKS)

At night in Solo: every game keeps 10007420 ON (102130) while its player stands without a revive item; a 0-HP hit then kills.
A dead player's comeback is held at `exe+0x5C4810`; at the morning each game releases its own player (gate call), the game respawns
the player at its nearest point, and the DLL then teleports them to a random day spot. Duo/Trio keep the knock-and-revive and the
M5-6 hold. If the hold fails in a session: fallback (TASKS history "option B"): let the comeback run, then move the player away and
hold them down with the M5-6 clock hold until the morning.
