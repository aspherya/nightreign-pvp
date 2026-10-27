# Multi-instance `nro`: 2-3 real game instances on one PC (plan, 2026-10-26)

**Owner of:** the design for replacing Nucleus Co-op with `nro` (several real instances, one per player, each seen by Claude).
**Status: PLAN ONLY, nothing implemented.** Decided by Leonardo 2026-10-26 (idea); design by Claude, attacked by an Opus review (§9).
Tags: `[VERIFIED: how]`, `[SOURCE: link]`, `[HYPOTHESIS]`, `UNKNOWN`. Today's tool: [README.md](README.md); mod contract: [CONTRACT.md](CONTRACT.md).

## 1. Goal and non-goals

Goal: `nro run scenarios/<x>.toml` launches N (2-3) **real, networked** dev games through me3, each with the mod and the game-data
package (event files), connects them as Seamless Co-op (SC) host + clients, drives each one (console, keys, screenshots) and judges
from every player's point of view (map, compass, nametags, Duo/Trio teams, revive, SC host/client). Puppets stay for crowd.

Not goals: real Steam matchmaking/lobbies and real lag (stay Friend Sessions, TESTING §6), going online, touching vanilla saves,
touching `nrsc.dll`, shipping any emulator to players.

## 2. What Nucleus does that we need (from the source and the installed handler)

Source: [SplitScreen-Me/splitscreenme-nucleus](https://github.com/SplitScreen-Me/splitscreenme-nucleus) (GPL-3.0, C#, `Master/NucleusGaming/...`),
read at `master` 2026-10-26. Installed handler: `D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN(.js)` v7 by solewalker.

| Need | What Nucleus does | Source |
|---|---|---|
| One Steam account, N identities | A **Steam emulator** (gbe_fork "experimental steamclient"): `steamclient64.dll` + `ColdClientLoader.ini` (`AppId=2622380`) + a per-instance `steam_settings\` folder. The handler writes `account_name`, `account_steamid` (Nucleus' fake ids, e.g. `76561197960287930`), `language` into each instance's `steam_settings\configs.user.ini`. Real Steam is not used (handler text: "Close Steam completely") | handler `.js` `Game.Play`; `Tools/Steam/SteamFunctions.cs` |
| Make the game load the emulator | `steamclient_loader_x64.bat` (in the instance folder) sets `HKCU\Software\Valve\Steam\ActiveProcess\SteamClientDll(64)` to the instance's `steamclient64.dll` and `pid`, starts the exe, **waits 8 s, restores the registry**. The key is **global**, so instances must start one after another (`PauseBetweenStarts = 30`) | installed `steamclient_loader_x64.bat` |
| Separate game folders | `SymlinkGame`/`SymlinkFolders`: each `Instance<N>\` is a symlink farm of the game folder; big files (`data*.bdt`) linked, small ones **copied** (`FileSymlinkCopyInstead`: `steam_api64.dll`, `regulation.bin`, `nrsc_launcher.exe`, ...) and `SeamlessCoop\` **copied** (`DirSymlinkCopyInstead`), because SC's `nrsc_settings.ini` is per instance. `nightreign.exe` is copied (`SymlinkExe = false`) | handler `.js`; `GenericGameHandler.cs` |
| Separate saves | Not by redirecting `%APPDATA%` (the handler does not set `UseNucleusEnvironment`). The save folder is `%APPDATA%\Nightreign\<steamid64>\`, so **a different emulated Steam id is a different save folder** `[HYPOTHESIS, from the folder layout in TECHNICAL_NOTES §1; the 3-player Nucleus runs worked]`. `GraphicsConfig.xml` in `%APPDATA%\Nightreign` is **shared**: the handler rewrites it to windowed + window size (and `ClipCursor OFF`) before launch | handler `.js` |
| Find each other | gbe LAN discovery on `listen_port = 47584` (all instances have the same value, and it works, so the emulator tolerates it) + `auto_send_invite.txt` / `auto_accept_invite.txt` (empty files) in `steam_settings`. Players then use SC's own Invite flow in the Roundtable | installed `steam_settings\configs.main.ini`; handler description |
| Several windows, input | Windows are placed/resized by Nucleus; input goes through **ProtoInput** (EasyHook injection: focus hooks, `BlockedMessages = [WM_KILLFOCUS]`, raw-input/XInput hooks, per-window fake cursor). Our trap "keys only reach a focused window" is the price of not having that | handler `.js` ProtoInput block; `Coop/ProtoInput/*.cs` |
| Our DLL | `dlllist.txt` (no trailing newline) loaded by `winmm.dll`/coldloader; **no me3, so no game-data package** | `deploy-nucleus.ps1`, TRAPS |

Reading: Nucleus is a big GUI/hook machine; what we need is only **(a) a per-instance Steam identity, (b) a per-instance game dir with
its own SC settings, (c) serial launch with a pause**. Everything else (split screen, input virtualisation) we do not need if we drive
windows one at a time.

## 3. Design

### 3.1 Per instance: what must differ

| Item | Differs how | Where |
|---|---|---|
| Steam identity | Emulated (gbe_fork `steamclient64.dll` from the Nucleus handler folder): the **three ids Nucleus already used**, `76561199023125438/439/440` (each has `NR0000.co2` in `%APPDATA%\Nightreign\` `[VERIFIED: dir listing 2026-10-26]`), `account_name = nro<N>`, own `steam_settings\` per instance; never the real account `76561198342403479` | `runs\<id>\i<N>\game\` |
| Game dir | Junction/symlink farm of the pinned game dir; **copied**: `nightreign.exe` (distinct image path per instance, also what pid detection needs), `regulation.bin`, `steam_api64.dll`, `SeamlessCoop\` (own `nrsc_settings.ini`) | `runs\<id>\i<N>\game\` |
| Mod dir | Existing `stage` output per instance: `i<N>\mod\` (own `nrpvp.toml` with `team_size`, `mm_queue`, `mode`, `private_code`, ...; mod logs next to the DLL) | `runs\<id>\i<N>\mod\` |
| me3 profile | One per instance (distinct file name = distinct me3 log dir), `nrsc.dll` of **that instance's** `SeamlessCoop\` | `i<N>\profile.me3.toml` |
| Game process | One `me3 launch -e <i<N>\game\nightreign.exe>`; game pid found by **image path** (today: "the new nightreign.exe", which breaks with 2) | `session.rs launch` |
| Console | `\\.\pipe\nrpvp-<pid>` already per pid `[VERIFIED: pipe.rs]`; `nro` must keep a list of instances | `session.json` `instances[]` |
| Window / keys | Window found by pid (`sys::main_window(pid)` exists). Keys: **focus window N, send, return** (sequential, no ProtoInput). Screenshot per window (`sys::screenshot(hwnd)`) | `desk` |
| Save | Reuse the existing Nucleus id folders and their `NR0000.co2` (no seeding from the real account: it would copy Leonardo's character and FromSoft saves may embed the Steam id `[HYPOTHESIS]`). Back up the three folders before each run, restore after; never touch `76561198342403479` or any `.sl2` | `session::stage_save` |
| Graphics | `GraphicsConfig.xml` is shared in `%APPDATA%\Nightreign`: back up, set windowed 960x540 (3 windows), restore at the end | `session` |

### 3.2 Identity route (the big choice; changed after the Opus review, §9)

- **R2 (primary): Nucleus' own proven mechanism, minus Nucleus.** The handler folder's 17.8 MB `steamclient64.dll` is gbe_fork (contains
  "GSE Saves", reads `steam_settings\configs.*.ini`) and it already ran 3 players `[VERIFIED: Nucleus runs, STATUS]`. Nucleus does not
  use ColdClientLoader to start the exe: its bat sets `HKCU\Software\Valve\Steam\ActiveProcess\SteamClientDll64` (+ `pid`), starts the
  exe, waits 8 s and restores the registry `[VERIFIED: installed steamclient_loader_x64.bat]`. `nro` does the same around `me3 launch`
  (profile with `--skip-steam-init`, as the old `-Me3` mode and `nrpvp-launcher` do). The key is global: **instances start one after
  another**. Improvements: restore the registry when the emulator DLL appears in the game's module list (a fact, not a timer); keep the
  original values in `session.json` and restore them at the next `nro` start if `nro` died; `pid` = the long-lived `nro watch` process
  (what steam_api does with a dead pid is UNKNOWN). The old `-Me3` failure for window 2 (ColdLoader "appid not found") came from
  ColdClientLoader, which `nro` does not use `[HYPOTHESIS]`.
- **R1 (optional later): `steam_api64.dll` replacement.** The `GoldbergEmu\` folder in Nucleus' `utils` is the *original* Mr. Goldberg
  emulator (global txt settings, no `configs.user.ini`), so per-instance ids would silently collapse; R1 needs a gbe_fork
  `steam_api64.dll`, which is not installed (a download, Leonardo's approval first). Not on the spike path.
- Emulator binaries are copied from the installed Nucleus handler at stage time, **never committed**; `pins/nightreign.toml` gets their hashes.

### 3.3 How instances find each other

1. **SC host/client over the emulator (first target):** same as Nucleus: instance 1 is host (Roundtable → Commence → Matchmaking Settings →
   Invite Members; `auto_*_invite.txt`), instances 2-3 join. SC's own "password" (the menu's Multiplayer Password) as the room key, so
   a stray instance on the LAN cannot join `[HYPOTHESIS: the field exists, TECHNICAL_NOTES §4g; which one Commence reads is UNKNOWN]`.
   `nro` presses the keys on the host window; MM-3g (invite from the mod, TASKS) would remove them.
2. **Our own matchmaking (`mm_queue`) over the emulator:** it uses Steam lobbies. gbe implements lobbies over LAN `[HYPOTHESIS]`; Nucleus
   deploys force `mm_queue = false` on the claim "Goldberg has no real Steam lobbies" `[UNVERIFIED claim in deploy-nucleus.ps1]`.
   Test it in spike S3: if it works the whole queue becomes testable on one PC.

### 3.4 Safety (kept from the current preflight, extended)

An **emulated mode** of preflight (the real-Steam mode stays as today). Today P3 refuses `--skip-steam-init` and P6 requires
`steam.exe` `[VERIFIED: preflight.rs]`; in emulated mode (**implemented 2026-10-26** in `nro-core` `preflight.rs` with tests, not yet wired to a launch):
- P3 allows exactly `--skip-steam-init true`; P6 is **flipped**: `steam.exe` must be absent, and the watchdog kills every instance if it
  appears (a real Steam next to a failed emulator load is the "real account" case).
- **P-module:** each game pid must have `steamclient64.dll` loaded from *its own* instance emulator path (module enumeration; the console
  `whoami` only returns `pid <n> <label>`, `pipe.rs:137`, so it cannot prove identity). Else kill.
- **P-lan:** staged `configs.main.ini` has `disable_lan_only = 0` (the handler has `1`, which lifts the emulator's LAN-only block), read back.
- Existing: me3 attach `start_online: false`, profile audit, no EAC, vanilla `.sl2` hashes unchanged at the end. The watchdog loops over all pids.

### 3.5 Code shape

`session.json` gets `instances: [{n, role, pid, me3_pid, dir, steamid}]` (a missing list = today's single instance, so the old CLI/MCP
keep working); every `nro` verb takes `--i N` (default 1, `--all` for cmd/shot/log); `[[instance]]` tables in a scenario
(`role = "host"|"client"`, `[instance.config]`), and any step takes `instance = N`. New module `crates/nro/src/multi.rs`
(stage farm, seed save, launch all, join). Mod side: nothing for the spike; later a console `whoami` field with the Steam id (check
what it prints today).

## 4. Spikes (in order; each ends with a PASS/FAIL line and a testing-log entry)

| Spike | Question | How to judge | Cost |
|---|---|---|---|
| **S0** (no game, read-only) | Symlinks possible on `Z:`/the repo drive? A copied `nightreign.exe` gives a distinct image path? Emulator files in the handler folder (hashes), `ActiveProcess` registry values (read), the emulated-mode preflight drafted as tests | `mklink` in a temp dir, listings, `cargo test` of the new checks | 1 hour |
| **S1** | **One emulated instance through me3** (R2, `--skip-steam-init`, id `…438` with its existing `.co2`), real Steam **closed**, reaches the Roundtable offline | P-module passes, no `steam.exe` appears, console answers, `%APPDATA%\Nightreign\…438` used, vanilla `.sl2` hashes unchanged, me3 attach config ok | 1 session |
| **S2** | **Two instances started one after another** (registry restored between) both reach their worlds offline, own console/window/log; S4 folded in | two pipes, two windows, both `status` = `world loaded`; a screenshot of each; RAM/GPU numbers (31.9 GB RAM, RTX 5070: headroom UNKNOWN) | 1 session |
| **S3** | **Host/client:** instance 2 joins 1 through SC invite over the emulator; then `mm_queue = true` | both consoles list 2 players | 1-2 sessions |
| S4 | Does an unfocused instance keep running and syncing for 60 s? | `players`/heartbeat in the log while window 2 is in the background | with S2 |

If S1 fails: log the cause; stay on Nucleus. R1 only with Leonardo's OK for a download.

## 5. Acceptance scenario (after S3)

Split in two because the host's Invite needs menu key presses (the most fragile part; MM-3g, invite from the mod, would make it robust): **(a) `mi-join`**: host and client connect, both consoles show `players` = 2. **(b) `mi-duo-sc`**:
`scenarios/mi-duo-sc.toml` (new, `[[instance]]` x2): host and client via SC; `team_size = 2`; host `clock`/`flag` to the expedition; wait on
**both** consoles for `players` = 2 and `team ...`; `expect` on the client console that it sees the host as teammate; `shot` from each window.
Next: `mi-rival-hit` (real hit between two clients: replaces most of the FS-RW1 checks that need a second screen), `mi-map-hidden` (RW-9:
the client's map/compass must not show the rival; `shot` + `peek`). Catalog entry in [SCENARIOS.md](SCENARIOS.md) when it exists.

## 6. Risks and unknowns

| # | Unknown / risk | How to find out |
|---|---|---|
| U1 | What the real `steam_api64.dll` does when the `ActiveProcess` pid has died, or the registry is restored early | S1 (pid = `nro watch`) |
| U2 | (resolved) copy `nightreign.exe` per instance: distinct image path | S0 confirms |
| U3 | Steam Input / pads: the game reads pads through Steam Input; with the emulator there is none (keys + mouse only, same as today) | accept |
| U4 | Unfocused window throttling or input loss (Nucleus needs focus hooks) | S4 |
| U5 | `listen_port` clash, firewall prompts, LAN discovery on a PC with Parsec virtual adapter | S3 |
| U6 | 3 games: 31.9 GB RAM, one RTX 5070; a memscan costs 9-12 GiB per process | S2, measure |
| U7 | Hardware breakpoint slots are per process (no clash between instances) | by design `[HYPOTHESIS]` |
| U8 | Whether the mod behaves the same under the emulator's Steam API (`mm_queue` uses `ISteamMatchmaking`) | S3 |
| U9 | (resolved) saves: reuse the 3 Nucleus ids, back up before each run | - |
| U11 | The SC password: the live `nrsc_settings.ini` has no password key (only GAMEPLAY/SCALING/SAVE), so the room key is the in-menu field; who can join on a LAN is bounded by `disable_lan_only = 0` + P-lan | S3 |
| U10 | The crash on quit (STATUS): `nro stop` kills, fine | - |

## 7. Out of scope here

Split screen, virtual pads (ORCH-1), real Steam lobbies, Linux, releasing the emulator to players.

## 9. Opus review (2026-10-26) and what changed

Verified by the reviewer and re-checked here: the `GoldbergEmu\` folder in Nucleus `utils` is the original Goldberg emulator, not gbe_fork (the
17.8 MB `steamclient64.dll` in the handler is gbe_fork); preflight P3/P6 forbid exactly what an emulated run needs; `whoami` has no Steam id;
the three Nucleus ids already have saves. Changes taken: **R2 is primary, R1 optional** (§3.2); emulated-mode preflight with P1/P6 flipped,
module-path check and LAN lock (§3.4); **reuse the Nucleus ids and saves, no seeding** (§3.1); copy `nightreign.exe`; acceptance split into
`mi-join` and `mi-duo-sc` (§5); registry restore on a fact and crash recovery (§3.2). Not taken: parallel start (R2 is serial anyway;
`GraphicsConfig.xml` is shared too). My own point the review did not change: keys stay "focus, send, return" (no ProtoInput) until S4 shows a need.

## 10. S0 result (2026-10-26, read-only on Leonardo's PC)

- **File symlinks need privilege and are refused** (not admin, no developer mode: `mklink` error). **Directory junctions work** (also on `Z:`).
  **Hard links work on `Z:` without privilege** `[VERIFIED: mklink /H of regulation.bin inside the game's volume]`, so each instance dir goes
  **on the game's drive** (e.g. `Z:
ro-instances\<run>\i<N>\`), big files (`data*.bdt`) hard-linked, directories junctioned, small files
  and `nightreign.exe` copied. Not on `C:` (hard links do not cross volumes). Do not create anything inside the Steam game folder itself.
- Emulator files (reference hashes, handler folder): `steamclient64.dll` A3D78CDA6C25D831304AC05040DDBB46D60F6CEB939BC621685F4C4BC407524E,
  `coldloader.dll` 7D358DC69F4E3D8B1544BEAFEEA411E8EA8467B5FA4B7CEE79E2620099807D38.
- `HKCU\Software\Valve\Steam\ActiveProcess` now: `SteamClientDll(64)` = the real Steam's, `pid` 0x3768, `ActiveUser` 0x16c6f597: this is
  what `nro` must save and restore. **Steam is running on this PC now**, so an emulated run needs Leonardo to close it first (P6 flipped).
- The 3 Nucleus id folders hold `NR0000.co2` (19.5 MB) and `.co2.bak`; back up before use.
- Still S0-open: a copied `nightreign.exe` gives a distinct image path (expected, confirmed at S1).

## 11. Implemented for S1 (2026-10-26, not yet run with a game)

`crates/nro/src/emu.rs` + `session.rs`: `nro launch --emulated N` (and `nro preflight --emulated N`) stages `Z:
ro-instances\<run>\i<N>\game`
(hard links, junctions, copied `SeamlessCoop\`, gbe_fork `steamclient64.dll` checked against its hash, `steam_settings\` with the Nucleus ids),
points `ActiveProcess` at it for the start, waits for the instance's own `steamclient64.dll` in the game's module list (P-module, else kill),
restores the registry (also on `nro stop` and at the next launch if `nro` died). `cargo test` / clippy clean. **Dry run** of
`nro preflight --emulated 1` on this PC: staged in 0.7 s, every check passes except P6 (`steam.exe` is running), as designed.
Single instance only; the instance list, `--i N` and per-instance process detection come with S2. Run it: close Steam, then
`target
elease
ro.exe launch --emulated 1 --world --mod-dist <dist>`.

**S1 PASS 2026-10-26** (testing-log/2026-10-20_on.md): emulated id seen by the mod, Roundtable reached with Steam closed, registry restored, vanilla saves unchanged. Next: S2 (two instances one after another, instance list, `--i N`, pid detection per instance).

## 12. S2 attempt (2026-10-26): instance 1 fine, instance 2 dies at attach; single-instance mutex

Built and tested first: per-instance run folders, `runs\instances.json`, `--i N` on every verb, `nro stop --all`, P5 lets another `\nro-instances\` game
run (20 nro-core tests, clippy clean). Run: instance 1 reached the Roundtable (as S1); instance 2's me3 log ends at `Attaching to ELDEN RING NIGHTREIGN`
(never `Host successfully attached`), its `nightreign.exe` is gone within 2 s, no crash event. Resource numbers with one game up: 9.65 GB working set,
5.4 GB of 12.2 GB VRAM, 9.9 GB RAM free of 31.9 GB, so three games fit on RAM but not comfortably.
**Cause (strong, not yet proven): the engine's single-instance mutex.** `nightreign.exe` contains the UTF-16 string `Global\SekiroMutex`
`[VERIFIED: byte scan of the exe]`; the Nucleus handler lists `Game.KillMutex = ["SekiroMutex"]` for exactly this. The second process finds it taken and exits.
Fix options (all close or avoid the handle that instance 1 holds): (a) close it from `nro` in instance 1 like Nucleus' StartGameUtil (needs cross-process handle
duplication; **the first attempt, a PowerShell handle scan, was blocked by the permission classifier, so it needs Leonardo's explicit decision**); (b) inside the
mod, dev build only: on load, close the game's own `Global\SekiroMutex` handle in its own process (in-process, one named handle, `allow_multi_instance` config);
(c) hook `CreateMutexA/W` in instance 2 to rename it. (b) is the smallest and keeps `nro` free of handle tricks; (a) needs no mod change. Open: whether the mutex is the
only blocker (S2 will show).

## 13. S2 continued (2026-10-26): decision (b) built; the second instance still dies, now inside me3's host

**Decision with Opus (b):** the mod closes its own `Global\SekiroMutex` handle (dev build, `allow_multi_instance = true`, console `mutex`; `game/multi_instance.rs`,
unit test with a real named mutex; log line `closed handle … to \BaseNamedObjects\SekiroMutex` seen in instance 1 `[VERIFIED]`). The mutex was real, and is closed, but it was **not the only blocker**.

**What is verified about the second-instance crash** (2 instances started one after the other, Steam closed, the mutex closed):
- The game process of the instance launched *second* dies at once with `0xC0000005`, in **either order** (instance 2 first, then 1: instance 1 dies) and every time (about 10 runs). Each instance **alone works** (S1, and instance 2 alone).
- `nro` attaches a debugger to it (`NRO_DEBUG_ATTACH=1`): the one fatal access violation is a **read in `ntdll.dll+0x165B1D`, called from `me3_mod_host.dll`** (stack scan: me3_mod_host +0x2DAA3, +0x3F58F0, +0xE14FE, +0x409FC0 ...), during the host attach, **before** `nrsc.dll` or `nrpvp.dll` load. Not our code, not the game's.
- Ruled out (each tried, no change): the mutex; `--no-boot-boost true` (me3's shared decrypted-archive cache; now always passed for emulated instances); `--no-mem-patch`; a different gbe listen port per instance; a real copy of `nightreign.exe`; real copies of every file under 100 MB instead of hard links; a private `%LOCALAPPDATA%` for me3 (its cache/logs/config).
- Suspect (**HYPOTHESIS**): a timing-dependent heap race in me3 0.11.0's host. me3 **v0.12.1** (2026-07-19) lists "Use mimalloc v3.4.1 ... which fixes a critical allocation race condition in the host" (release notes of garyttierney/me3). Instance 1 starting first makes the second attach slower or different, which fits a race.
- Other leftovers: `me3 0.13.0` is downloaded and unpacked (data only, never run) at `Z:\Tools\me3-0.13.0\` (zip SHA-256 a8b693c5…8c2b equals the digest GitHub shows for the release); me3 0.11.0 stays the pinned version.

**Next (needs a decision, see the report):** try me3 0.13.0 for multi-instance runs only (own pins file `pins/nightreign-me3-0.13.toml` with its hashes, `me3_dir` Z:\Tools\me3-0.13.0\bin, selected by an env var). Running a downloaded binary was blocked by the permission classifier, so it needs Leonardo's explicit OK. If 0.13.0 does not fix it: report the crash to me3 (`me3-debug-info.zip` of v0.12.1+ has symbols for the stack), or look at the host attach code for state shared between two games.

**Diagnostics added to `nro`** (kept): `NRO_DEBUG_ATTACH=1` (second instance: first access violation, module+offset, stack scan), the game's exit code in `watch.log`, the game pid read from me3's `attaching to process` line (a game that dies right after the attach is never seen by the process poll), `--i N` and `runs\instances.json`.

## 14. me3 0.13.0 tried (2026-10-26, Leonardo's OK): same crash

`NRO_PINS_FILE=nightreign-me3-0.13.toml` (new: `pins/nightreign-me3-0.13.toml`, me3 0.13.0 from `Z:\Tools\me3-0.13.0\bin`, hashes recorded) runs `nro` against the newer me3; the pinned 0.11.0 stays the default. Result: **the second instance dies the same way**, so the mimalloc fix of 0.12.1 is not the cause.
Debugger attach again: the fatal read is at **`ntdll.dll+0x165B1D`**, which lies in the `RtlInterlockedPopEntrySList` code (nearest exports `ExpInterlockedPopEntrySListFault/End` at +0x165AE7/+0x165AF0): a lock-free SLIST pop on a bad list head, called from `me3_mod_host.dll` (0.13.0 stack: +0x447488, +0x31E3F, +0xFA07F, +0x410E18 ...) during the host attach. That points at heap or pool state being corrupt inside the second process, only when another game process (with its own me3 host) is alive; the cause of that coupling is UNKNOWN.
Not yet tried (cheap to expensive): instance 1 and 2 on different me3 versions; instance 1 without our mod and Seamless Co-op; launching instance 2 with `--disable-arxan true`; a second Windows user for instance 2 (different desktop session); and the hybrid below.
**Hybrid fallback (no me3 for clients):** Nucleus runs 3 instances because it never uses me3: its `winmm.dll` + `dlllist.txt` loader starts our DLL in each. `nro` could start the host through me3 (full game data) and the clients the Nucleus way (our DLL, no game-data package: fine for clients that only need the mod's rules). Untested; needs the SC launcher path and the same registry trick.

## 15. Root cause found and fixed (2026-10-26): dearxan's `Local\` section is shared by every game in the session

**Cause `[VERIFIED: source + live probe + debugger]`:** me3's host (0.11.0 and 0.13.0 both use the crate `dearxan` 0.5.2, checksum equal to
me3's `Cargo.lock`) calls `dearxan::disabler::schedule_after_arxan`, whose state lives in a `lazy_global!`: a named pagefile section
`Local\DEARXAN_SCHEDULED_AFTER_ARXAN` holding a *pointer* to a `Ctx` (an `SLIST_HEADER` + flags, 32 bytes) allocated in the process heap
(`dearxan-0.5.2/src/disabler/lazy_global.rs`, `mod.rs`). The crate calls this "process-wide", but `Local\` is the whole logon session. The
first game keeps the handle open for its lifetime; the second game's host opens the same section, sees `value != null`, skips its own setup
(`schedule_after_arxan_inner` never runs: no `dearxan::disabler::game: game path` line in its me3 log) and pushes its callback onto the
SLIST at the first game's address: AV in ntdll's SList code. Evidence: with instance 1 up, `OpenFileMappingW("Local\DEARXAN_SCHEDULED_AFTER_ARXAN")`
from outside returned the section with `value=0x26BD8EBC6A0 size=32`; instance 2 then died with `read of 0x26BD8EBC6A0` at `ntdll+0x165B1D`
(the same address). `Local\DEARXAN_NEUTER_ARXAN_RESULT` does not exist (`disable_arxan` is off). This explains "either order", "alone works",
and why nothing in our files, ports or caches mattered.

**Fix (in the mod, dev build, same switch as the mutex):** `game/multi_instance.rs` now also closes this process's own handles to sections named
`DEARXAN_*` (type index found with an unnamed probe section; unit test with a real named section). Closing the last handle removes the name; the
mapped view that dearxan keeps using stays valid. Log: `closed handle … to \Sessions\1\BaseNamedObjects\DEARXAN_SCHEDULED_AFTER_ARXAN`, then
`multi_instance: ready for another instance` (about 3 s after the game starts). Console `mutex` does both passes.
**Run (me3 0.11.0, pinned default):** instance 1, wait for `ready for another instance` in its `nrpvp.log`, probe: name gone (error 2); instance 2
with `NRO_DEBUG_ATTACH=1`: P-module passed, `no unhandled AV in 20000 ms`, own Steam id 76561199023125439, its own `ready` line; both games
responding 40 s later (≈9-10 GB working set each, 3.7 GB RAM free of 32.7, so a third instance was not tried). Cleanup: all processes gone,
`ActiveProcess` back to Steam's values, vanilla `NR0000.sl2` hash unchanged.

**Limits:** a module that loads in a game *after* the close and uses dearxan would create a fresh section and run its setup again (none known:
SC and nrpvp do not). Instance N+1 must not start before instance N logged `ready` (the name exists from its host attach until then): `nro` should
wait for that line. Upstream: worth reporting to dearxan/me3 (a per-process name, e.g. with the pid, fixes it for everyone).
**Next:** `nro launch --emulated N` waits for `ready for another instance` in every running instance's `nrpvp.log`; then S2 PASS run with
`--world` on both, and S3. The Nucleus-style hybrid (§14) is no longer needed.

## 16. S2 state (2026-10-26, after §15): both instances up; Windows Firewall prompt is the next blocker

- `nro launch --emulated N` now forces `allow_multi_instance = true` and **waits for `multi_instance: ready for another instance`** in the instance's mod log before returning, so the next launch is safe. Two instances launched back to back are both up and answer the console (`status`: title).
- Instance folders are now **stable**: `Z:\nro-instances\i<N>\game` (no run id), refreshed per run (hard links re-made, exe copied only if its size changed). Reason: **Windows Firewall asks once per exe path** ("allow public and private networks to access ELDEN RING NIGHTREIGN"), and that system dialog blocks the game window and our key presses until a human answers it; with a path per run it would come back every run. It is a security setting, so `nro` and the agents never click it. One-time human step per instance (i1, i2, i3): click Allow (Private networks is enough for the emulator's LAN). Old per-run folders under `Z:\nro-instances\1793*` are leftovers and can be deleted (they contain junctions: remove them with `cmd /c rmdir` per junction first, never `Remove-Item -Recurse` on the folder).
- Not yet done for S2: both instances to the Roundtable (`nro --i N navigate title_to_world`), the S4 focus check, then S3 (SC join).

**S2 PASS 2026-10-26** (testing-log): two instances in the Roundtable with their own ids and characters; instance 1 keeps running in the background (S4); RAM is the limit (0.55 GB free with two). S3 (SC join) is next and needs firewall allow rules (§17).

## 17. S3 plan (2026-10-26, research only): SC join over the emulator, firewall, key path, draft `mi-join`

**Firewall: no allow rules are needed; keep the Block rules.** Windows Firewall does not filter traffic between this PC's own addresses
(loopback, and the own `172.29.144.1` vEthernet / `192.168.15.63` Ethernet IPs, which Windows routes internally) `[HYPOTHESIS: known WFP
loopback exemption]`, and the run proves it: with Inbound **Block** rules on `Z:\nro-instances\i1|i2\game\nightreign.exe` (created at the
first prompt, `fw_i2.png` in run `1793057406`), run `1793058324` (i2) holds TCP rows with *ephemeral remote ports* `172.29.144.1:51583` and
`192.168.15.63:51584`, and run `1793058318` (i1) holds `172.29.144.1:51585`: accepted inbound connections on the gbe listen ports, which a
blocked SYN never creates `[VERIFIED: watch.log tcp rows (TCP_TABLE_OWNER_PID_ALL) + Get-NetFirewallRule 2026-10-26]`. The emulators already
found each other before any join. The Block rules also help: they keep real LAN machines out (with `disable_lan_only = 0`) and stop the prompt
from coming back. Why `172.29.144.1`: gbe broadcasts to 255.255.255.255 and every non-loopback adapter's broadcast address (Windows path skips
the loopback adapter), on the default port range and its own port; the first adapter's address becomes the peer address `[SOURCE: gbe_fork
dll/network.cpp send_broadcasts/get_broadcast_info, dev branch]`. Different `listen_port`s (47584/47585) are fine (the docs say "use the same
port", the code broadcasts to a range; the TCP rows confirm). `custom_broadcasts.txt` (`127.0.0.1:47584` etc.) is only a fallback if an adapter
changes; `offline = 0` must stay (it only affects `BLoggedOn`) `[SOURCE: steam_settings.EXAMPLE/configs.main.EXAMPLE.ini]`.

The only human step left: **a rule for i3 before its first launch** (else the prompt blocks the window). Elevated PowerShell:
`New-NetFirewallRule -DisplayName "nro i3 (block inbound)" -Direction Inbound -Action Block -Program "Z:\nro-instances\i3\game\nightreign.exe"`.
Optional cleanup of ~44 stale rules for deleted per-run folders: `Get-NetFirewallApplicationFilter | ? Program -like 'Z:\nro-instances\1793*' | Get-NetFirewallRule | Remove-NetFirewallRule`.
If S3 ever needs an allow rule: Block wins over Allow, so the Block rule must be removed first.

**How the join works under gbe** `[SOURCE: gbe_fork dll/settings_parser.cpp parse_auto_*_invite, dll/steam_friends.cpp]`: an *empty*
`auto_send_invite.txt` = send to anyone, empty `auto_accept_invite.txt` = accept from anyone (both staged, `emu.rs`). Host presses Invite
Members → the game calls `CreateLobby(Public, 6)` and `ActivateGameOverlayInviteDialog(lobby)` (MEMORY_MAP §12) → gbe sends
`InviteUserToLobby` to every LAN peer of the app → the client's gbe posts `GameLobbyJoinRequested_t` with no prompt → the game/SC joins, as
with a Steam invite. That is the Nucleus handler's "other players join automatically". **The client needs no keys.**

**Key path (host only):** Roundtable spawn → `W 5000ms` (to the table, as `to_expedition`) → `F` (expedition menu) → `F2` (Matchmaking
Settings) → the Invite Members row → `F`. The row's position below Number of Players (the selected row, `RIGHT` steps it) is **UNKNOWN**: the
first S3 run takes `screenshot` after `F2` and the agent counts `DOWN`s; then a `navigate = "invite_members"` macro in `run.rs`, ending with
`ESC; ESC`. Opening Invite Members alone creates the lobby `[VERIFIED: S9 memo U2]`.

**What to assert:** `status` on both = `world loaded, 2 player(s)` and `team_type 1` (Roundtable) `[VERIFIED: status_text format,
console/mod.rs]`; `players` = two rows, slot 0 `me` on each `[VERIFIED: MEMORY_MAP §2, Nucleus]`; do not assert ally/rival in the Roundtable.
The client shows `no world` while it reloads into the host's Roundtable, so `wait` with ~90 s.

**Our settings:** nro forces `mm_queue = false`, `br_menu = false` `[VERIFIED: session.rs]`, so the quiet invite (which skips exactly the
`ActivateGameOverlayInviteDialog` call gbe needs for auto-send) never arms; keep `mm_quiet_invite` off for S3. Leave `mode` unset (`br`, menu
hook off). **Password:** leave Multiplayer Password empty: an invite does not go through the password search `[HYPOTHESIS]`, and the Block
rules + LAN lock already bound who can join. **MM-3g** would replace the host keys, but only Invite Members makes a joinable lobby (our own
`CreateLobby` is not joinable, MM-3d), so MM-3g is the robust route, not a quick one; fallback if auto-accept fails: client console
`mm_join invite|direct` on the host's lobby (dev, `mm_join = true`).

**Risks:** Invite Members row position (one screenshot); the invite arriving while the client is not in a world; SC rejecting a gbe
lobby (Nucleus ran 3 players, so low); RAM (0.55 GB free with two); the menu needs focus on the host (`desk` focuses).

**Steps:** (1) nro: `[[instance]]` + `instance = N` in `nro-core` scenario; (2) launch i1, wait ready, launch i2; (3) `title_to_world` on
both; (4) host keys + screenshot, find the row; (5) waits/expects below; (6) testing-log entry; then `mm_queue = true` over gbe (§3.3 #2).

Draft `scenarios/mi-join.toml` (not in `scenarios/` yet):
```toml
name = "mi-join"
about = "S3: instance 2 joins instance 1 as an SC client over the emulator; both consoles see 2 players"
[[instance]]
n = 1
role = "host"
[[instance]]
n = 2
role = "client"
[config]            # every instance
team_size = 1
ui_flag_watch = false
[[step]]
instance = 1
navigate = "title_to_world"
[[step]]
instance = 2
navigate = "title_to_world"
[[step]]
instance = 1
keys = "W 5000ms; wait 1s; F; wait 2s; F2; wait 1s"
[[step]]
instance = 1
screenshot = "mm_settings"
[[step]]
instance = 1
keys = "DOWN; wait 500ms; F; wait 3s; ESC; wait 1s; ESC"   # DOWN count UNKNOWN until mm_settings is read
[[step]]
instance = 2
wait = { console = "status", contains = ["world loaded, 2 player(s)", "team_type 1"], timeout_s = 90 }
[[step]]
instance = 1
wait = { console = "status", contains = "world loaded, 2 player(s)", timeout_s = 30 }
[[step]]
instance = 1
expect = { console = "players", contains = ["me"] }
[[step]]
instance = 2
expect = { console = "players", contains = ["me"] }
[[step]]
instance = 1
screenshot = "host"
[[step]]
instance = 2
screenshot = "client"
```

**S3 PASS 2026-10-26** (testing-log): the client joined the host's Seamless Co-op lobby over the emulator; both consoles list 2 players. Steam lobbies (ISteamMatchmaking) work through gbe_fork on this PC, so `mm_queue = true` between instances is the next experiment (our own matchmaking on one PC). Open: the deterministic key path to Invite Members (`navigate = invite_members`).

## 18. Status 2026-10-27 and what is left

**Done and verified:** S0 to S4 (§10-§16 and testing-log), two emulated instances side by side, SC join over the emulator (S3; the mod's lobby probe lists both members of an `nrsc_ynx3_seamless_master_lobby`). Firewall: the user replaced the auto Block rules with Allow-Private rules for `i1..i3` (admin PowerShell); an instance exe path that is new to Windows triggers a prompt that blocks input, which is why the instance folders are fixed.
**Found on the way:** (a) after `nro stop --all` the killed games hold dearxan's shared section until they have torn down; a launch within seconds crashes in me3's host (instance died after the SC load, even instance 1): `nro stop` now waits for the processes to be gone plus 2 s. (b) `nro` key presses go to the window in front: any window the user opens over the games (Claude app, PowerShell) takes them. (c) focus is retried 6 times (two game windows made the first `SetForegroundWindow` fail often). (d) The Roundtable spawn position differs between runs (the save remembers the last position), so `W 5000ms; D 600ms` does not always reach the table prompt: the host key path to Invite Members is **not yet deterministic**.
**Open (next steps, in order):**
1. ~~A position-independent way to reach the table prompt and the Invite Members row~~: done, §19 (`tp` + menu keys).
2. ~~`[[instance]]` and `instance = N` in the scenario format, `scenarios/mi-join.toml`, SCENARIOS.md entry~~: done, §19.
3. ~~`mm_queue = true` between the two instances~~: done, §20 (`mi-queue` PASS 3/3, queue to expedition).
4. Third instance: **RAM-bound** (§20: ~9.5 GB private per game at the title whatever the graphics settings; 0.4 GB free with three on 32 GB). Needs more RAM, not lower settings.
5. Report the dearxan `Local\` section bug upstream (me3 / dearxan): a per-process name fixes it for everyone.

## 19. `mi-join`: a deterministic host invite (2026-10-27)

**Decision: teleport the host to the table, then the menu keys** (candidate (d) "simpler", picked over (a) steering with W/A/S/D, (b) a
prompt flag, (c) MM-3g). Why: the position write is already verified (MEMORY_MAP §8, T-028 run 4) and the menu path after the prompt is
fixed, so one write removes the only variable part (the spawn the save remembers); steering is a feedback loop with camera-relative keys
(slow, flaky); MM-3g is an open RE spike (the callable Invite Members frame is UNKNOWN, TASKS) and would call game menu code from the mod.
Found live: the prompt also needs the **heading** (at the spot facing away: no prompt), so `tp` writes the heading quaternion at
`Physics+0x50` too (Elden Ring's layout; verified by reading it while facing the table vs away, then by writing it: MEMORY_MAP §8).

**Built:** mod (dev only, `game/teleport.rs`): console `pos` (world position, heading `yaw`, `menu` = `CSMenuMan+0x48`) and `tp x y z [yaw]`
(at most 100 m, the physics module's RTTI vtable checked, the heading written only over a value that reads as one; unit tests). `nro`:
`navigate = "invite_members"` (`run.rs`): Roundtable check, close menus with Q until `menu` = 0, `tp 0.8 10.66 69.5 -170`, check the
landing (1 m), F, check `menu` = 0x200004 (expedition menu), F2 (Matchmaking Settings), DOWN (Team Session: Invite Members), F, then
success only when the mod's lobby probe logs `LobbyCreated_t result=1` after this attempt's `echo` marker; 3 attempts; Q closes the menu.
Scenario format: `[[instance]]` (`n`, `role`, `[instance.config]`) and `instance = N` on any step (`nro-core` `scenario.rs`, parser
tests incl. every file in `scenarios/`); `nro run` launches the instances in order (each waits for the previous one's ready line), runs
the steps on their instance, stops all (last first), writes `report.json` in every run folder. Safety added on the way: `nro desk` sends
keys only while the game window is in front and no window of another program overlaps it (else waits 30 s, 5 times, then refuses), and
stops a sequence the moment the game loses the foreground; screenshots say `COVERED by …`; pid reuse guard in `live_pid`/`stop`.

**Result:** see testing-log 2026-10-27 (`mi-join` PASS runs). Open: `mm_queue = true` over the emulator (§18 item 3), `mi-duo-sc` (§5),
a third instance (RAM).

## 20. Our own matchmaking queue over the emulator (2026-10-27): `mi-queue`

**Safety decision.** The real-Steam rule stays: preflight P7 refuses `mm_queue`/`br_menu` on a real-Steam launch, and `nro` forces both
off there. An **emulated** instance may run the queue, because its lobby calls go to gbe_fork, not Steam: P6 (no `steam.exe`), P6b
(`disable_lan_only = 0`), P-module (the instance's `steamclient64.dll` is in the game, else killed) still hold, plus three new checks:
(1) P7 for emulated refuses `mm_join_auto` containing `link` (the joiner's `steam://joinlobby` goes to the shell, which would **start the
real Steam client**); `nro` forces `mm_join_auto = "invite"`. (2) **Identity gate**: with `mm_queue = true` the launch waits for the mod's
`mm_queue: worker started: own Steam id N` and kills the game unless N is the instance's emulated id (`STEAM_IDS`); `nro cmd mm_queue_*` /
`mm_join` re-check it every time and are refused on real-Steam sessions (`nro-core preflight::queue_identity`, unit tests). (3) The
watchdog kills an emulated game if `steam.exe` appears during the run.

**Result: PASS 3/3** (testing-log 2026-10-27). gbe_fork implements everything the queue uses: `RequestLobbyList` with our string/number
filters and the worldwide distance filter, Invisible lobbies in search results, lobby and member data, `JoinLobby`, `InviteUserToLobby`
with auto-accept. Queue → MATCH READY 1-2 s; Invite Members → ALL PRESENT ~3 s; both commence → same expedition, rivals marked 29.
Nothing the emulator lacks was found for Solo with 2 players.

**What the real flow needs on one PC:** console `mm_queue_solo` on both (the instance that queues first owns the queue lobby and
becomes host); the host's `navigate = "invite_members"`; then **every** party member readies at the table (`navigate = "commence"`, new:
tp, F, F until the menu closes; the party panel counts `n/2`). `mm_host_create` must be false (the mod's own lobby is not joinable).
Not tried yet: `br_menu = true` (Commence in the table menu instead of the console; needs the Number of Players ring on Solo), 3 players,
Duo/Squad, `private_code`.

**Title trap found:** a run killed inside an expedition makes the next start ask "Unable to restore current session … Switch to online
mode?" with OK focused. `title_to_world` now screenshots before every title key and clicks CANCEL when both button frames are seen
(pixel check, `run.rs` `is_restore_dialog`, tests on the real screenshots); the old macro pressed OK ~30 times in run 1 (no internet TCP
seen). TRAPS has the entry.

**Third instance (2026-10-27):** three instances start (all three answer the console at the title), but free RAM falls to 0.5 GB
(default graphics) / 0.4 GB (every quality LOW, 1280x720 window): each game holds ~9.5 GB private bytes at the title either way, so
graphics settings are not the lever. Stopped before the Roundtable; GraphicsConfig.xml restored byte for byte. A third instance needs
about 10 GB more free RAM (or a smaller footprint per game, UNKNOWN how); `mi-join3` waits for that.
