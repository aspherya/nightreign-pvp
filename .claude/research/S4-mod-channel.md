# S4: Mod-to-mod channel (how our DLLs share match state across clients)

Researched 2026-10-10. Tags: `[SOURCE: ...]`, `[VERIFIED: how]` (read-only, this machine), `[HYPOTHESIS]`, `UNKNOWN` (+ where I looked).
ER facts are labelled **ER** and are hints only. Nexus returns 403 to WebFetch, so Nexus-only claims are not re-read here.

## 0. Bottom line

1. **Best channel: our own `ISteamNetworkingMessages` messages on a private channel number, addressed by Steam ID read from the engine's `CSSessionManager` player list.** It needs no lobby id, no callbacks if both sides send first, and does not touch SC's state. Use a flat-API accessor (`SteamAPI_SteamNetworkingMessages_SteamAPI_v002`). **Do not use `steamworks::Client::init()`** (it would switch the process to manual callback dispatch and shut Steam down on drop; see §2.4).
2. **Do not rely on game sync as a data carrier.** Event flags are not networked by the setter (fromsoftware-rs says so) and SC's flag sync is undocumented. SpEffect sync exists in the engine (packets 60/61) but whether a custom SpEffect on a remote player replicates under SC is untested. It is a cheap bonus experiment, not the foundation.
3. **Lobby data/chat is a viable fallback or complement** (tiny, host-authoritative), but the lobby id is the hard part: Steam has no "current lobby" getter.
4. **Under Nucleus (Goldberg/gbe_fork) the Networking Messages path appears to work** (source implements it, flat accessors exported); lobbies are implemented too. Both still need an in-game run.
5. No public mod that shares custom state over SC was found (UNKNOWN; see §3).

---

## 1. What the game and SC already sync

### 1.1 Engine packets (fromsoftware-rs, **ER** layouts, NR is an ER-engine fork)

| Fact | Tag |
|---|---|
| `NetworkSession` vtable has `broadcast_packet(buffer, length, packet_type: u8)`, `receive_packet(.., packet_type)`, `receive_latest_packet`, `kick`, `request_leave`, `remote_identity()` ("for PC release this'll be the steam ID"). The engine's multiplayer is a typed-packet protocol over Steam, one `PlayerNetworkSession` per remote player holding `remote_identity: u64` (Steam ID). | [SOURCE: https://raw.githubusercontent.com/vswarte/fromsoftware-rs/main/crates/eldenring/src/cs/network_session.rs] |
| Known packet type ids from doc comments: 8 base character data, 11 character type, 12 equipment, 60 SpEffect initialize, 61 SpEffect sync data, 90 (0x5A) multiplay start area id, 104 EMEVD event sync (host to clients, per-player opt-out `disable_emk_sync`). | [SOURCE: network_session.rs; session_manager.rs; s2 `game_data_man.rs` in the same repo] |
| `CSFD4VirtualMemoryFlag::set_flag` doc: "Sets the event flag bit ... **Does not inherently network set flags.**" So writing an event flag from our DLL is local only. | [SOURCE: https://raw.githubusercontent.com/vswarte/fromsoftware-rs/main/crates/eldenring/src/cs/event_flag.rs] |
| `NetChrSync` / `NetChrSetSync` only carry **placement and health** readback buffers per ChrIns (`ChrSyncUpdateFlags` has bits for placement and health). Not a general state channel. | [SOURCE: .../cs/net_chr_sync.rs] |
| `CSSessionManager` (singleton) has `players: DLVector<SessionManagerPlayerEntry>`; each entry begins with `SessionManagerPlayerEntryBase { .., steam_id: u64 at +0x10, steam_name: UTF-16 }`, then `game_data_index`, `character_event_id`, `is_host`, `is_local_player`. Emptiness check reads the u64 at +0x10 (matches the "Steam ID at entry +0x10" hint). Also `host_player` and `session_player_limit` (4 open world / 6 quickmatch in ER). | [SOURCE: .../cs/session_manager.rs#L60-L260] **ER layout**; NR offsets UNKNOWN |
| NR has `CSSessionManager` at exe+0x3C21EC8 (singleton found), `CSNetMan` at exe+0x3C046A8, plus NR-only `CSTeamSessionManager`/`CSTeamSessionDataMan`. | [VERIFIED: T-006 log, TECHNICAL_NOTES §4] |
| ER battle-royale/arena QM uses SpEffects 1110/1111 (team summon/respawn), 1130 (death), 1140 (crown for 1st place), etc., applied "to the players during battle", plus `received_world_flag_sync` and per-slot `quickmatch_team_types` in `QuickmatchManager`. Precedent that SpEffect markers are a native game idiom. | [SOURCE: .../cs/net_man.rs#L170-L330] |

**Answers to the card's questions**

- *Does ER/NR sync event flags?* **ER:** vanilla syncs some flags via host-driven EMEVD event sync (packet 104) and world flag sync at join. The exact set is UNKNOWN to me; no primary doc found (Nexus ER SC article 241 returned 403). **ER SC:** its author's post (via search snippet only, unverified) says the update syncs all flags on load and lets the author choose which flags come from local vs host, and that live event sync is still imperfect. [SOURCE: web search snippet of https://nexusmods.com/eldenring/articles/241, not opened]. **NR SC:** UNKNOWN. Clues only: author's release repo has data files `BLOCKSYNC` (a flag/ID list, purpose unconfirmed) and `nrsc.dll` has RTTI `EcTestEventFlagState` [VERIFIED: strings of nrsc.dll]. [HYPOTHESIS: SC syncs a subset of flags at join and blocks some; runtime flag writes by a client are not broadcast.]
- *Are applied SpEffects replicated?* The engine has SpEffect init/sync packets (60/61) [SOURCE above], so at least some SpEffects on a player are synced to peers **in ER**. Which ones (by param flag, by source, by lifetime) is UNKNOWN. We never need a replicated one for hostility. **NR/SC behaviour: UNKNOWN.**
- *Character state / world time:* death/downed state is observable on each client from the synced ChrIns (health sync is native, PvP death was seen on both clients in FS-0/FS-1 [VERIFIED: TECHNICAL_NOTES §4]). World time/tide phase: UNKNOWN whether readable (no struct located; `world_area_time.rs` exists in fromsoftware-rs ER, not examined). Both are inputs to a state machine, not a place to store winner/choice.

Conclusion: **game-native sync can feed observations (deaths, positions, probably the tide), but cannot carry mod-defined values reliably.**

---

## 2. Using Steam directly from our DLL, next to SC

### 2.1 What SC itself loads (fact base)

`nrsc.dll` plain strings include `SteamNetworkingMessages002`, `SteamMatchMaking009`, **`SteamNetworking005`**, `SteamNetworkingUtils003`. [VERIFIED: `grep -a` of nrsc.dll 2026-10-10] So SC uses new Networking Messages, the lobby API, and the old P2P API. Its channel numbers and whether it calls `AcceptSessionWithUser`/`CloseSessionWithUser` are UNKNOWN (no decompilation allowed).

### 2.2 `ISteamNetworkingMessages` semantics

All from https://partner.steamgames.com/doc/api/ISteamNetworkingMessages (read 2026-10-10) unless noted.

| Point | Detail |
|---|---|
| Channels | `SendMessageToUser(identity, data, size, flags, nRemoteChannel)`; receiver must call `ReceiveMessagesOnChannel(sameNumber)`. Small integers recommended. Multiple channels to one user share one connection. Reliable on one channel is in-order and exactly-once; no ordering across channels. |
| Implicit session | The first send creates a session implicitly (handshake first). **Sending to a user also implicitly accepts** any incoming session from them. |
| Incoming first contact | Without prior session you get `SteamNetworkingMessagesSessionRequest_t` and cannot read messages until `AcceptSessionWithUser` or you send to that user. If ignored, the callback repeats while they keep sending. |
| No delivery notice | No success callback; failure shows via `SteamNetworkingMessagesSessionFailed_t`. Design acks into the protocol. |
| Broken session | Send returns `k_EResultNoConnection` unless `k_nSteamNetworkingSend_AutoRestartBrokenSession` is set. |
| Closing | `CloseSessionWithUser` frees the **whole session (all channels)**; `CloseChannelWithUser` closes one channel; session ends when no channels remain. **Never call `CloseSessionWithUser` ourselves** (would cut SC's traffic); never call it on SC's channels. |
| Idle timeout | Sessions time out after idle periods with no callback; send a heartbeat (e.g. 1 Hz) anyway. |
| Size limit | Not stated on this page. Fragmentation/reassembly is supported. UNKNOWN numeric limit; keep messages under ~1 KB. [HYPOTHESIS: the sockets limit of 512 KB applies; not verified] |

**Can it coexist with SC? [HYPOTHESIS, high confidence]** The interface is a process-wide singleton, channels are separate routing numbers, and messages are demultiplexed per channel. So a second module that sends and receives on its own channel should not disturb SC, provided: (a) we choose a channel number SC does not use (pick something unusual, e.g. 0x4E52, and log); (b) we never close sessions; (c) we do not drain SC's channels; (d) our first send to a peer also accepts SC's session if SC has not (harmless). Risk: if SC loops `ReceiveMessagesOnChannel` over a range of channels rather than fixed ones. UNKNOWN.

**Callbacks.** `SessionRequest`/`SessionFailed` arrive through Steam's callback pump, which SC runs. Registering our own receiver needs `SteamAPI_RegisterCallback` with a hand-built `CCallbackBase` (C++ vtable) [SOURCE: steam_api.h convention; HYPOTHESIS that this works from Rust]. **Avoid it**: if both clients send to each other on a timer, each send accepts the other's session, so `ReceiveMessagesOnChannel` can be polled with no callbacks. Failure is detected by `Send` return codes. [HYPOTHESIS: sufficient; the doc says send accepts pending requests.]

### 2.3 `ISteamMatchmaking` (lobby data, member data, chat)

From https://partner.steamgames.com/doc/api/ISteamMatchmaking (read 2026-10-10):

| Point | Detail |
|---|---|
| Lobby data | `SetLobbyData` **owner only**; keys up to `k_nMaxLobbyKeyLength` = **255** chars; value cap is `k_cubChatMetadataMax` (value not stated on that page; I recall 8192 from the SDK header: [HYPOTHESIS], UNKNOWN until checked in `steamclientpublic.h`). Returns false if key/value too long. Owner changes automatically when the owner leaves. |
| Member data | `SetLobbyMemberData` sets only your own per-user data; same caps; returns void. Everyone reads others' with `GetLobbyMemberData`. Good for "I am eliminated / my vote". |
| Chat | `SendLobbyChatMsg`: text or binary, **up to 4 KB**; received via `LobbyChatMsg_t` callback then `GetLobbyChatEntry`. Goes through Steam back-end; Valve recommends the networking API for volume. |
| Current lobby | **No getter.** Track the id from `LobbyCreated_t`/`LobbyEnter_t`/`LobbyChatMsg_t`/`LobbyDataUpdate_t`/`LobbyInvite_t` callbacks. `GetLobbyByIndex` only for search results. |
| Members | `GetNumLobbyMembers`, `GetLobbyMemberByIndex`, `GetLobbyOwner` need you to be in the lobby (you are, via SC) and a valid lobby id. |

**Can we get SC's lobby id? options (none verified on NR)**
1. Poll `ISteamFriends::GetFriendGamePlayed(friendId, &info)`; `FriendGameInfo_t` has a lobby id field [HYPOTHESIS from memory of the SDK; the doc page I fetched did not show the struct]. Works only for Steam friends and only if SC sets the game's lobby as joinable/rich presence. Fine for friend sessions, bad as a general design.
2. Hook-free memory read: SC/engine may hold the lobby id (the engine's `manager_impl_steam` pointer at CSSessionManager+0x60 is a Steam wrapper [SOURCE: session_manager.rs]). Offsets for NR UNKNOWN; would need RE. Not recommended.
3. Register a `LobbyEnter_t` callback before SC joins. Impossible for the joiner (SC joins at lobby entry; our DLL loads earlier, but still needs the C++ callback object trick) and fragile.
4. Skip the lobby entirely: use Networking Messages with the Steam IDs from `CSSessionManager`. **Preferred.**

### 2.4 Rust crates

| Fact | Tag |
|---|---|
| `steamworks` 0.13 (Noxime/steamworks-rs), SDK 1.64, `steamworks-sys` 0.13.0 (raw bindings, optional bindgen). Wraps `networking_messages()` (`send_message_to_user`, `receive_messages_on_channel`, `session_request_callback`, `SessionRequest::accept`) and `matchmaking()`. | [SOURCE: https://github.com/Noxime/steamworks-rs; src/lib.rs and src/networking_messages.rs on master] |
| `Client::init()` calls `SteamAPI_InitFlat`, then **`SteamAPI_ManualDispatch_Init()`**; its callback loop uses `SteamAPI_ManualDispatch_RunFrame/GetNextCallback`; `Drop` for the manager calls **`SteamAPI_Shutdown()`**. There is no "attach to existing init" constructor. | [SOURCE: steamworks-rs src/lib.rs `init`, `run_callbacks_raw`, `impl Drop for Manager` (fetched 2026-10-10)] |
| Consequence: using the high-level `Client` in a process where SC already initialised Steam is dangerous: manual-dispatch mode is global to the process, so SC's `SteamAPI_RunCallbacks` could stop firing and our drain would steal its callbacks; and shutdown on drop would kill SC's Steam. | [HYPOTHESIS, derived from the above and from Valve's description of manual dispatch as an alternative to RunCallbacks: https://partner.steamgames.com/doc/sdk/api] |
| The docs do not say whether `SteamAPI_Init` may be called twice. | [SOURCE: same page] UNKNOWN |
| Safe approach: depend on `steamworks-sys` (or declare the few externs ourselves), **never call Init/ManualDispatch/Shutdown**, and obtain the interface with the flat accessor `SteamAPI_SteamNetworkingMessages_SteamAPI_v002()` from the already-loaded `steam_api64.dll`. The flat function names exist in the SDK flat header and gbe_fork exports them. The identity struct can be built with `SteamAPI_SteamNetworkingIdentity_SetSteamID64` (flat). Requires that the accessor works after another module's init: [HYPOTHESIS; the accessors use the global Steam client context created by SC's init]. | [SOURCE: gbe_fork dll/flat.cpp L6051 for the export; HYPOTHESIS for the semantics] |

---

## 3. Existing mods that share state through SC

- **ER/NR:** I found no public mod that adds its own player-to-player channel on top of SC. Searches: web queries on NR mods requiring SC and custom Steam networking, me3 discussions #118, the earlier T-003 sweep. The "47 mods using this mod" list on Nexus did not render (T-003), so **UNKNOWN**. Known SC-dependent mods (e.g. "Nightreign 6 Player Fixes") change events/params locally and rely on SC's own sync [SOURCE: T-003 §1].
- Implication: we are probably first to need this; no tested precedent to copy. Evidence for feasibility is therefore generic Steamworks semantics (§2), not NR-specific.

---

## 4. Nucleus Co-op (Goldberg-based) behaviour

| Fact | Tag |
|---|---|
| The project's Nucleus handler uses Goldberg via ColdClientLoader; FS-0 PvP worked through it, so SC's own Steam traffic works under the emulator. | [VERIFIED: TASKS T-017; TECHNICAL_NOTES §4] |
| gbe_fork (maintained Goldberg fork) implements `Steam_Networking_Messages` with `SendMessageToUser` (Steam-ID identities only; IP-address identities return `NoConnection` "TODO"), `ReceiveMessagesOnChannel`, `AcceptSessionWithUser`, and session-request callback. Messages are carried by Goldberg's own LAN transport (reliable flag honoured). | [SOURCE: https://raw.githubusercontent.com/Detanup01/gbe_fork/dev/dll/steam_networking_messages.cpp#L165-L270] |
| gbe_fork implements lobbies: `SetLobbyData`, `SetLobbyMemberData`, `SendLobbyChatMsg`, `GetLobbyChatEntry`, `GetLobbyOwner`, member listing. | [SOURCE: .../dll/steam_matchmaking.cpp L812, L936, L986, L1004, L1186] |
| gbe_fork exports the flat accessors `SteamAPI_SteamMatchmaking_v009` and `SteamAPI_SteamNetworkingMessages_SteamAPI_v002`. | [SOURCE: .../dll/flat.cpp L1358, L6051] |
| Caveats: whether the Nucleus handler ships gbe_fork or an older Goldberg (the original 2019 build lacked these) is UNKNOWN; check the DLL actually in the instance folder. Goldberg `SendMessageToUser` returns OK without any real delivery guarantee; there are no per-session timeouts to test. Nucleus instances each get a distinct emulated Steam ID, but the per-instance IDs are UNKNOWN. Message size limits in Goldberg are not enforced in the code I read (UNKNOWN). | [SOURCE: same files; UNKNOWN for handler contents] |

**Takeaway:** Nucleus can test the whole channel locally with 2+ instances; real Steam (FS) is still needed to confirm session/handshake behaviour.

---

## 5. Ranked options and verification experiments

| Rank | Option | Why | Cost | Tiny experiment |
|---|---|---|---|---|
| **1** | **Own Networking Messages channel, addressed by `CSSessionManager` Steam IDs**; host-authoritative match state, 1 Hz heartbeat plus event messages, acks, sequence numbers | No lobby id, no callbacks if both send; independent of SC's internals; works in Goldberg | Low-medium: needs the `CSSessionManager.players` walk on NR (offsets) and 3-4 FFI calls | **E-S4a (solo-checkable parts, then Nucleus).** In the DLL thread: log `GetProcAddress("SteamAPI_SteamNetworkingMessages_SteamAPI_v002")` result and the non-null interface pointer. Then read the `CSSessionManager.players` vector and log each `steam_id` (+0x10 of entry, **ER layout**, verify on NR by comparing with the local Steam ID from `SteamAPI_ISteamUser_GetSteamID`). Then, in Nucleus with 2 instances, each sends `"hello <seq>"` to the other's ID every second on channel 0x4E52 and polls `ReceiveMessagesOnChannel`; log round trips. Pass: both log the peer's messages and SC's co-op play is unaffected. Repeat on real Steam in an FS. |
| 2 | **Lobby member data + lobby data** (owner writes `phase`/`winner`; each member writes `eliminated`) | Persistent, auto-delivered to late joiners, owner migrates | Medium: needs lobby id (hard) and polling of `GetLobbyData` | **E-S4b.** In FS with a friend: call `GetFriendGamePlayed` on the partner and log any lobby id; if non-zero, call `GetNumLobbyMembers`, `GetLobbyOwner`, and write/read `SetLobbyMemberData("nrpvp","1")`. Pass: partner reads it via `GetLobbyMemberData`. Solo cannot test. |
| 3 | **SpEffect marker** on a player (e.g. a spare param row applied on death/win) read by peers via the synced ChrIns SpEffect list | No networking code, uses engine sync (packets 60/61) | Needs a modified regulation or an existing otherwise-unused SpEffect id; SC may check regulation (`YuiNexusRegulationManager`); SC may filter | **E-S4c.** Apply an existing vanilla SpEffect with an unmistakable effect (e.g. a visible aura) to the local player from our DLL on key press; confirm the other Nucleus instance sees it and, if possible, reads it in its `SpecialEffect` list on that ChrIns (struct from `sp_effect.rs`, **ER** layout). Note which id works. Small for flags (a handful of bits), not a general channel. |
| 4 | **Event-flag piggyback**: set a flag locally on every client and hope SC syncs it | `set_flag` is explicitly local; NR SC sync rules unknown | Low code, high uncertainty | **E-S4d.** Set an unused event flag on instance A and read it on instance B in Nucleus; also test with the flag being host-set at a boss defeat. If B never sees it, drop. |
| 5 | **Lobby chat messages** (`SendLobbyChatMsg`, binary 4 KB) | Delivered to all members via Steam back-end | Needs lobby id and a `LobbyChatMsg_t` callback (C++ callback object from Rust) | Only if rank 2 works and pushing event-style messages is needed. |
| 6 | Hooking SC/engine `broadcast_packet` to add our own `packet_type` | Native path | **Forbidden/inadvisable**: hooks the engine network layer; likely detectable by SC's integrity checks; unknown packet-type whitelist | Not recommended. |

Recommendation: implement rank 1 as the primary, with host-authoritative match state (host = `is_host` entry in `CSSessionManager.players`) and a state hash in the heartbeat so any client that diverges re-requests a snapshot. Keep ranks 3 and 4 as free experiments to run during a Nucleus session.

---

## 6. Open questions

1. Does the NR `CSSessionManager` player vector have the same layout as ER (vector at +0x70, entry Steam ID at +0x10)? Needs RE or a dump experiment; the singleton address is known.
2. Which channel numbers does SC use on `SteamNetworkingMessages002`, and does it ever call `CloseSessionWithUser`? (Strings do not show; runtime: log `SteamNetworkingMessagesSessionRequest_t`/failure events from our side after a first send.)
3. Does the flat accessor `SteamAPI_SteamNetworkingMessages_SteamAPI_v002` return a usable pointer when the app's Steam init was done by another module (SC/the game)? Verify in E-S4a. Not guaranteed by Valve docs.
4. Message size limit of `ISteamNetworkingMessages` (page silent). Check `steamnetworkingtypes.h` (`k_cbMaxSteamNetworkingSocketsMessageSizeSend`) and gbe_fork behaviour.
5. `k_cubChatMetadataMax` value (needs the SDK header), and whether `FriendGameInfo_t` exposes the lobby id.
6. Does the Nucleus handler ship gbe_fork or an old Goldberg (affects E-S4a under Nucleus)? Inspect `steam_api64.dll` in an instance folder and look for networking-messages strings.
7. Does NR SC sync event flags at all, and what is `BLOCKSYNC`? Requires an in-game flag experiment (E-S4d); the Nexus ER article about flag sync was not readable (403).
8. Is a SpEffect applied by our DLL on a player replicated to peers in NR under SC (E-S4c)?
9. Does SC's author accept a second DLL sending on Steam networking? It does not modify SC, so likely fine, but tie it to the pre-release permission request (T-003 §5).
