# S10: Seamless Co-op internals: sessions, the 6-player cap, host loss

Researcher: Opus, 2026-10-21. Card: TASKS.md S10 (Matchmaking). Read-only study for interop (hard rule 3): no SC code is
copied here. This doc gives addresses, class names, counts and conclusions only. Raw dumps (strings, disassembly) stay outside the
repo in `C:\Users\Leonardo\source\nrsc-study\` and are never committed. Scripts: `../experiments/S10/`.

Pins: game 1.03.3 (`nightreign.exe` SHA256 `9545e8732919bb6a…`), SC 1.1.3 (`nrsc.dll` SHA256 `243eec929a97b71e…`, PE time
`0x6A4679A9`), me3 0.11.0. Tags: `[VERIFIED: how]`, `[SOURCE: link]`, `[HYPOTHESIS]`, UNKNOWN.

---

## 0. Answers in one screen

| # | Question | Answer |
|---|---|---|
| 1 | How does SC create and join a session? | SC does not have its own session layer. It reuses the game's: the game's own session code (`CSSessionManager`, `CSTeamSessionManager`, `JoinSessionJob`, …) creates the Steam lobby and joins it. SC sits in between: it proxies the game's `ISteamMatchmaking` (and very likely the other Steam interfaces it names), tags and locks the lobby, cuts out the Bandai servers, and patches about 40 game locations it finds by signature. Its Steam and session logic is **Themida-virtualised**, so it cannot be read statically. Only what it touches can be seen |
| 2 | Where does the 6-player cap live? | **Both, and mostly SC.** The game passes the lobby size from a runtime field (`[request+0x68]`), not a constant. SC rebuilds the player character container (`CSChrSet<PlayerIns>`) itself, and its 4–6 slot fixes are SC changelog items. Steam's own lobby limit is 250. The engine still assumes 1–3 in data (events, powers, menus) |
| 3 | Host loss, any migration path? | The game's network model is host-authoritative (the host runs the world and enemies and sends event sync). No host-migration code or text exists in the exe (0 hits among 6,863 class names and all strings). The only recovery path in vanilla is **server-backed "session recovery"** (rejoin your own ongoing session through Bandai's servers), which SC removes and we must not use. Steam lobby ownership does move to another member by itself, but that is only the lobby, not the game world |
| 4 | Our own session layer if SC blocks us? | Thin layer **on top of SC** (drive the game's own session jobs, MM-3g style): 1–3 weeks. **Replacing SC**: 4–6+ months, high risk (see §5) |
| — | **Go / no-go on host migration** | **No-go** for real host migration (the match carries on with the same world under a new host). **Go** for the cheaper goal: detect host loss fast, end the match cleanly for everyone, and requeue (MM-9). One cheap Nucleus check (runbook R2) could still change this, if joiners' games already carry on under SC when the host drops |

---

## 1. What `nrsc.dll` is, statically

| Fact | Tag |
|---|---|
| Packed with **Themida** (section `.themida`, 7.4 MB of 9.2 MB). Only one import per DLL is visible (`kernel32!GetModuleHandleA`, `steam_api64!SteamAPI_GetHSteamPipe`, `ws2_32`, `crypt32`, `wldap32`, …); the real import table is rebuilt at run time | `[VERIFIED: pe_overview.py]` |
| Exports: `modengine_ext_init`, `me3_load_early` | `[VERIFIED: pe_overview.py]` |
| A normal `.text` (1.1 MB, 4,626 `.pdata` functions) holds the plain C++ parts: JSON/locale, formatting, crashpad, libcurl (GitHub version check), signature scanning, hook installation, WorldChrMan code | `[VERIFIED: fn_strrefs.py]` |
| **None** of the Steam interface version strings (`SteamMatchMaking009`, `SteamNetworkingMessages002`, `SteamNetworking005`, `SteamFriends017`, `SteamUser021`, `SteamUtils010`, `SteamNetworkingUtils003`, `STEAMAPPS_INTERFACE_VERSION008`, `STEAMUSERSTATS_INTERFACE_VERSION012`) is referenced from any `.pdata` function. The code that uses them lives in Themida-virtualised code | `[VERIFIED: fn_strrefs.py, xrefs.py]` |
| Consequence: SC's Steam proxy and P2P message logic **cannot be read statically**. Only runtime observation works (our hardware-breakpoint traces, the hook census below) | `[VERIFIED: above]` |
| Source files named in the binary: `hooks\hooks.cpp`, `signatures\signatures.cpp`, `cs\world_chr_man\world_chr_man.cpp`, `cs\bloodstain_man\…`, `cs\auto_control_api\…`, `cs\game_message_repository\…`, `mod\param_repository\…`, `mod\server_manager\…` (`YuiNexusNetRankingDB`), `mod\preferences\…`, `mod\message_repository\locale.h`, `dantelion\memory\memory.cpp` | `[VERIFIED: strings]` |
| SC's only player-facing texts: Solo Mission / Up to 6 players, "host is not using seamless co-op", "host version … does not match", Paused / Resumed, "{} / {}". **No text about a host leaving** | `[VERIFIED: SeamlessCoop/locale/english.json + strings]` |

### 1a. What SC touches in the game (counts from static reading)

| Item | Count / names | Tag |
|---|---|---|
| Game classes SC finds by RTTI name (vtables) | 14: `CSAutoControlAPI`, `PartyMemberInfo`, `CSQuickMatchLobbyContext`, `CSChaosMatchRateDecreaseJob`, `CSChaosMatchRateUpdateJob`, `CSLuaEventScriptImitation`, `ChrFinderImp` (in `WorldChrManImp::_UpdateMultiDoping`), `PlayerDebugSession`, `PlayerNetworkSession`, `EcTestPlayerNo`, `EcTestChrDead`, `EcTestTeamType`, `EcTestEventFlagState`, `EcTestSortieMemberNum`; plus `CSEzUpdateTask<…GameMan>` | `[VERIFIED: one SC function at nrsc+0x655A0 references all of them, then "Failed to find signature"]` |
| Byte-pattern signatures | about 40 checks in the signature function (`nrsc+0x6F570`: 34 "Failed to find signature" sites, 6 "incompatible game version: inconsistent pattern result") | `[VERIFIED: fn_strrefs.py]` |
| Hook install sites (allocation checks tagged `hooks.cpp`) | 28: 11 in one init function (`nrsc+0x1EDB0`) and 2–3 in seven others | `[VERIFIED: fn_strrefs.py]`; exact hook count needs runbook R1 |
| Player container | SC's `world_chr_man.cpp` code builds the `CSChrSet` for `PlayerIns` (and for `NecroBuddy`) itself ("Cannot create container for CSChrSet.PlayerIns!!", next to `WorldChrManImp`). The game has no such text | `[VERIFIED: strings + xref nrsc+0x26830 / +0x26B30]`; meaning: SC resizes the player set `[HYPOTHESIS: strong]` |
| Already known hook | `LookupEntry` `exe+0x27F4D20` starts with a `jmp` into private memory (MEMORY_MAP §13) | `[VERIFIED: UI-0]` |
| Steam matchmaking proxy | the `CreateLobby` vtable slot points into private memory below the exe; `ISteamFriends` is not wrapped | `[VERIFIED: MM-3c, MEMORY_MAP §12]` |

Reading the list: `PlayerNetworkSession` (per-player network session), `PartyMemberInfo` (party list), `EcTestSortieMemberNum`
(the "how many players departed" check used by events and params), `EcTestPlayerNo` (player slot checks) and the event-flag and
team-type tests are exactly what a 3→6 player change has to touch. `CSQuickMatchLobbyContext` and the two `CSChaosMatchRate*Job`
classes belong to the vanilla matchmaking/rating server path that SC disables. `[HYPOTHESIS: strong]`. The names are facts, what SC does with each is not.

## 2. How a session is made and joined (Q1)

The game's own session classes (RTTI in `nightreign.exe`) `[VERIFIED: game_rtti grep, 6,863 classes]`:
`CSSessionManagerImp` (FD4 singleton), `CSTeamSessionManagerImp`, `CSTeamSessionDataManImp`, `CSNetManImp`, `CSPlatformNetworkManImp`,
`CSNetworkUserManagerImp`, `CSSessionQos`; jobs `SessionCreateJob`, `SessionLeaveJob`, `JoinSessionJob`, `JoinSessionWaitJob`,
`JoinSessionByTitleFlowJob`, `CheckSessionExistenceJob`, `CSUpdateSessionJob`, `CSMultiplayNotifyLeaveJob`, `UpdateUntilPcLeaveJob`;
player side `PlayerSession`, `PlayerNetworkSession`, `PlayerNullSession`, `PlayerSessionHolder`, `PartyMemberInfo`; error steps
`CSEventNetworkErrorReturnTitleStep`, `CSEventNetworkErrorLeaveOnlineModeStep`; vanilla matchmaking `CSQuickMatchingCtrl` + `CSQuickMatch*Job`
+ `FromNet::FNQuickMatch`; server calls `FromNet::RequestCreateSessionParams`, `RequestDeleteSessionParams`, `RequestLeaveMultiplayParams`,
`RequestDisconnectP2PLogParams`. ER's equivalent of `CSSessionManager` has a `lobby_state` (Host / Client / OnLeaveSession …), a
`host_player`, a `players` list and `session_player_limit` `[SOURCE: fromsoftware-rs eldenring/src/cs/session_manager.rs]`; for NR that
layout is a lead only.

Flow under SC, put together from MM-1…FS-MM3 (S9 §5a–5g) and this study:

1. Host opens Invite Members → the game's team-session code calls `ISteamMatchmaking::CreateLobby` at `exe+0x25919A1`. The call site
   reads **lobby type from a flag** (Public 2 or Invisible 3) and **`cMaxMembers` from `[request+0x68]`** of a request object.
   It is not a constant `[VERIFIED: disasm_at.py exe+0x2591950..0x25919A1]`. Under SC that field is 6 `[VERIFIED: MM-3c trace]`.
2. The call goes into SC's proxy (private memory). The lobby comes back locked, with `nrsc_lobby_type` / `nrsc_lobby_version` already
   set in `LobbyCreated_t`. The game adds `inviteParam` `[VERIFIED: MM-3c, MM-3d]`.
3. The joiner's game gets `GameLobbyJoinRequested_t` (invite or our `steam://joinlobby` link), the game's `JoinSessionJob` enters the
   lobby, and SC's proxy checks the `nrsc_*` tags (the "not using seamless co-op" and "version does not match" texts)
   `[VERIFIED: FS-MM1, FS-MM3]` / tag check by SC `[HYPOTHESIS]`.
4. Session traffic: the game uses `SteamNetworking006` (old P2P API), SC carries `SteamNetworking005` and
   `SteamNetworkingMessages002`. SC very likely runs its own message channel next to the game's `[HYPOTHESIS]`.
5. At departure the host's game makes a second, unlocked lobby (the expedition session) and everyone moves into it
   `[VERIFIED: MM-1, FS-MM1, FS-MM3]`.
6. The vanilla server steps (`RequestCreateSession`, quick-match registration) are skipped. SC: "does not connect to the matchmaking
   servers" `[VERIFIED: string]`.

Why our own `CreateLobby(Public, 6)` was not joinable (FS-MM2) fits this picture: a lobby is only a session when the game's
session code (step 1's request object, `CSTeamSessionManager` state) made it. So MM-3g has to **start that game code path**. Calling
Steam is not enough. `[HYPOTHESIS: strong]`

## 3. The 6-player cap (Q2)

| Layer | What we know | Tag |
|---|---|---|
| Steam | `cMaxMembers` "can not be above 250" | `[SOURCE: partner.steamgames.com/doc/api/ISteamMatchmaking]` |
| Game lobby creation | size comes from `[request+0x68]` at run time (6 under SC). Who writes 6 there (game data or an SC patch) | UNKNOWN: runbook R1 (is there a hook or patch near the writer) |
| Game player container | `CSChrSet<PlayerIns>` capacity 6 under SC (MEMORY_MAP §2); SC has its own code that builds this container | `[VERIFIED]` / SC sizes it `[HYPOTHESIS: strong]` |
| Game data | events, powers, menus are built for 1–3 (SC's slot 4–6 fixes; the game still has "Two/Three player" menu entries that SC relabels) | `[SOURCE: SC changelog via S1]` `[VERIFIED: english.json]` |
| ER reference | ER has `session_player_limit` (4 world / 6 arena) and an override field | `[SOURCE: fromsoftware-rs]`, NR lead only |

Conclusion: 6 is an SC decision layered on a 3-player game. Steam and the lobby code would allow more. The player container,
sync buffers and the game's per-slot data are where more than 6 would break. Nothing here changes "plan for 6" (S1).

## 4. Host loss (Q3)

| Fact | Tag |
|---|---|
| Host-authoritative engine: ER's session keeps a `host_player`, the host sends event (EMEVD) sync to clients, `game_data_index` is −1 for the host | `[SOURCE: fromsoftware-rs session_manager.rs]`, same design in NR `[HYPOTHESIS: strong]` |
| No host-migration code in NR: no class or string with `migrat`, `NewHost`, `ChangeHost`, `HostLeave` and similar among 6,863 RTTI names and all exe strings. One script-style name `ReportHostLeaveArea` (likely the "stay in the multiplay area" warp, ER's `CSStayInMultiplayAreaWarpData`) | `[VERIFIED: grep]` / meaning `[HYPOTHESIS]` |
| Vanilla **session recovery** exists: "Attempting session recovery…", "Current session found. Press OK to restore session", "Unable to restore current session as it has already ended" (CL_System_Message 401820–401827), plus debug server calls `DBGForceFailRestoreSession` / `DBGGetSessionClientID`. It brings **your own** crashed/disconnected game back into the session that is still running. It goes through Bandai's server | `[VERIFIED: FMG text 1.03.3 + exe strings]` / server dependency `[HYPOTHESIS: strong]` |
| Public reports (vanilla): if a player drops, the other two continue; rejoin from the main menu after a crash. Nothing reliable on what happens when the **host** drops | `[SOURCE: steamcommunity.com/app/2622380/discussions/0/592889306000928255; insider-gaming.com/elden-ring-nightreign-leave-penalty-explained]` UNKNOWN for the host |
| Steam lobby: "if the current owner leaves, another user in the lobby will become the owner automatically" | `[SOURCE: partner.steamgames.com/doc/api/ISteamMatchmaking]` |
| SC: no host-loss text, its session logic is virtualised. What a joiner's screen does under SC when the host leaves | UNKNOWN: MM-9 (friends) and runbook R2 (Nucleus, solo) |

**Verdict: no-go on true host migration.** It would mean promoting a client's copy of the world (enemies, night circle, events,
items on the ground, everyone's state) to the authoritative world, inside obfuscated game code that has no such path, under an SC
we cannot extend. That is engine work far beyond the project's size. The vanilla recovery feature is a rejoin, not a migration,
and it needs Bandai's servers.

**What is realistic, in order:** (1) pick the host with the best connection (MM-8) so host loss is rare; (2) detect host loss
within seconds on every joiner (Steam `LobbyChatUpdate_t` for the host on the expedition lobby, plus the ChrSet `players` drop; the
game's own `CSEventNetworkErrorReturnTitleStep` firing is a third signal) and end the match the same way for everyone (MM-9);
(3) "rematch": the survivors' mods requeue together at once, so the cost is one lost round, not a lost evening.

**One open door:** if runbook R2 shows the joiners' expedition **keeps running** after the host's game dies (vanilla-style
continuation that SC keeps), the engine already does a form of migration and the verdict flips to "keep it working with our mod".
Cheap to check, so it is in the runbook.

## 5. Our own session layer if SC ever blocks us (Q4)

| Route | What it means | Size | Main risks |
|---|---|---|---|
| A. Thin layer on SC (recommended direction) | Drive the game's own team-session jobs from the mod (MM-3g / MM-7), our queue lobby for matchmaking, our clean host-loss end (MM-9). SC still does the session, cap and slot fixes | **1–3 weeks** | Session code has no `.pdata` and is obfuscated (S9 §5d); every game or SC update can move it |
| B. Replace SC | Our own: server-free session creation (skip `RequestCreateSession` / FromNet), lobby and join, 6-slot `CSChrSet` and its sync buffers, slot 4–6 fixes for events/powers/relics, separate save, scaling, a P2P message channel, version check | **4–6+ months** (one planner + agents, with friend sessions), likely more | Everything in A, plus the Arxan-protected online code in the exe, the 3-player data assumptions SC fixed one by one over a year (changelog 1.02–1.12), and keeping up with game patches alone. Also: SC's save and launcher conventions would no longer apply, so players lose their SC progress |
| C. Hybrid | Keep SC for 6 slots and saves; replace only the lobby/join step | 3–6 weeks | Depends on SC accepting a session it did not see created (FS-MM2 says it does not today) |

Recommendation: stay on A. B only if SC stops working with a game patch and its author stops updating it. Then start from
this doc's §1a list, which is the map of what SC had to change.

## 6. Runbooks (HUMAN, Leonardo solo)

**R1: hook census (about 5 min, dev build, read-only).** Run the commands from the repo folder that has `experiments\S10\` (the
`s10-sc-study` worktree `C:\Users\Leonardo\source\repos\nightreign-pvp-s10` until that branch is merged). Start the game with `.\scripts\launch.ps1`. At the Roundtable, run
`%LOCALAPPDATA%\Programs\Python\Python312\python.exe .claude\experiments\S10\hook_census.py --out .claude\experiments\S10\census_roundtable.txt`.
Start a Solo expedition. Once landed, run it again with `--out …\census_expedition.txt`. Quit. Send both files (addresses and module
names only). The census reads memory and never writes. Pass = files written. Claude then maps every redirect to a game function and
writes the list to MEMORY_MAP.

**R2: host loss on Nucleus (about 15 min, 2 instances).** With the current Nucleus deploy (any dev build), start 2 instances,
join instance 2 to instance 1 the usual way, start an expedition. Once both have landed and can see each other, **close instance 1's
window (Alt-F4)**. Watch instance 2 for 60 s: does it keep playing, show a message (write down the words), go to the title screen or
the Roundtable? Then `dist\nrcon.exe -i 1 log 60` (or keep `dist\nucleus-logs\Instance1\`). Round 2: same, but instance 1 leaves
through the menu (Return to Title). Pass = both rounds noted. If instance 2 keeps a live world with enemies and can still progress,
call it out: that is the open door in §4. Goldberg is not real Steam, so MM-9 repeats this with a friend.

## 7. Follow-ups this study suggests

- MEMORY_MAP §12: add the `CreateLobby` call-site facts (type flag, `[request+0x68]` size) and the game's session class list.
- After R1: list of SC hooks in MEMORY_MAP (new section), so a future conflict between our hooks and SC's is visible at once.
- Optional dev-console probe: dump which `ISteamMatchmaking` / `ISteamNetworking*` vtable slots point outside `steamclient64.dll`. That
  gives the exact set of Steam calls SC intercepts.
- Locate NR's `CSSessionManager` (lobby state, host entry, players) for MM-8/MM-9. It is the cleanest host-loss signal.
