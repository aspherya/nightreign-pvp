# Own networking instead of Seamless Co-op (design, 2026-10-30)

**Owner doc for this topic.** Decision of Leonardo, 2026-10-30: replace Seamless Co-op (SC) with our own solution. First need: cross-game
data, such as RW-5's last-hitter verdict (only the game that applied the hits knows who landed them, and our event flags do not sync
between games). This doc is research and design, not a rewrite. Cards: [TASKS.md](TASKS.md) "Own networking (NET)".
Tags: `[VERIFIED: how]`, `[SOURCE: link]`, `[HYPOTHESIS]`, UNKNOWN. **ER** = Elden Ring fact, a hint only for NR.
Raw agent report with all file/line citations: `side-session-notes/.../net-research-agent.md` (outside the repo); the facts that matter are copied below.
Pins: game 1.03.3 (exe 1.3.3.0), SC 1.1.3, me3 0.11.0.

## 0. Bottom line

1. **Two separate problems, one word.** (a) A *data channel* between our mods ("transport"): cheap, no SC dependency, buildable in days.
   (b) The game's *session layer* (remote players, their sync, hits, HP, the 6-slot container): this is what SC really gives us, and what
   "replacing SC" means. It is months of work with high risk. Do (a) now, decide (b) after three cheap studies (§6, gate G1).
2. **Smallest step for RW-5 (NET-1, NET-2):** a `net` module with a `PeerLink` trait whose real implementation uses
   `ISteamNetworkingMessages` on a private channel, plus a fallback implementation on Steam lobby member data. A `Verdict` message
   (boss id, killer team) rides on it. It works next to SC today and keeps working after SC is gone, because the game itself never
   uses that Steam interface.
3. **Replacing the session layer** is only realistic by driving the game's own session code (`CSSessionManager`, `JoinSessionJob`, ...)
   without SC's ~40 patches and Bandai-server bypass. Replicating players ourselves (puppets) means re-implementing animation,
   hits, HP and SpEffect sync: not recommended. Verdict and gates in §5-§6.
4. **Honest recommendation:** hybrid for a long time (SC keeps the session, our channel carries our data). Full replacement only if the
   studies NET-3 to NET-5 show the game's session code runs with a small, readable set of changes. Reason to still replace: independence
   from SC updates and from its author's permission (it is closed, no license, no redistribution). Reason not to: it is the highest-risk
   item of the project and gives players nothing they see.

## 1. What SC does for us today

| Area | What SC does | Our dependency | Tag |
|---|---|---|---|
| Lobby | No session layer of its own. The game's `CSTeamSessionManager` calls `ISteamMatchmaking::CreateLobby` (`exe+0x25919A1`, size from `[request+0x68]`); SC proxies `ISteamMatchmaking`, tags it `nrsc_lobby_type = nrsc_ynx3_seamless_master_lobby`, `nrsc_lobby_version`, locks it. A second unlocked lobby appears at expedition start | `mm/` queue gate reads these tags (`steam/queue.rs`, `mm/worker.rs`, `mm/machine/gate.rs`); the host must press Invite Members; the joiner opens `steam://joinlobby` | `[SOURCE: MEMORY_MAP §12; S10 §2]` `[VERIFIED: MM-1, FS-MM3]` |
| Bandai servers | Cut out: no `RequestCreateSession`/QuickMatch; EAC not running | Required (rule: never online) | `[SOURCE: S10 §2]` |
| Player slots | Builds the 6-slot `CSChrSet<PlayerIns>` (`WorldChrMan+0x9A40`) itself; sweeps slots 1..5 every frame and destroys a PlayerIns whose `[chr+0x5D0]+8` matches no active member (`nrsc.dll+0x62AB4`) | all of `game/team/*`, `rules/teams.rs` (teams = sorted session ids `0x600N`), puppets (`game/puppet.rs` must dodge the sweep) | `[VERIFIED: MEMORY_MAP §2, §18]` |
| Remote players and sync | Remote `PlayerIns` come from the game: pending `CSSessionManager` entry -> `exe+0xC43B94` -> `exe+0xC445A0` -> `WorldChrMan::CreateNetPlayer exe+0x518280` -> `ChrSet<PlayerIns>::CreateNetPlayer exe+0x4AC240`. The member table `[[exe+0x3C078D0]+0x10] + idx*0x8A0` has room for idx <= 1 (vanilla = 3 players); SC widens it | PvP (hits, HP, death, positions) works only because the game syncs these players; H1 | `[SOURCE: ORCH-R2 L11-15]` `[VERIFIED: FS-0/FS-1 PvP]` |
| Host authority | Host runs world and enemies, sends EMEVD event sync (flag 7502 reaches clients); no host migration anywhere in the exe | `night_match.rs` relies on it; MM-9 is "end the match" | `[SOURCE: S10 §4]` `[VERIFIED: Nucleus run 2]` |
| Saves and settings | `.co2` save extension, `nrsc_settings.ini` (scaling keys, save ext), "Up to 6 players" menu ring, 4-6 slot data fixes | profiles (no me3 `savefile`), `nro` copies an ini per instance | `[VERIFIED: grep profiles/, emu.rs]` |
| Load order | `nrsc.dll` `load_early`; `nrpvp.dll` loads after it | `nrpvp-launcher`, `pins/nightreign.toml` | `[VERIFIED]` |
| Not used | SC password, SC chat | the game's own Password menu fields exist, nothing reads them | `[VERIFIED: grep]` |

How hostile it is to read: SC is Themida-virtualised; none of its Steam interface strings is referenced from plain code
`[VERIFIED: S10 §1]`. Only run-time observation (our hardware breakpoints) works, and hard rule 3 forbids patching or copying it.

**Everything cross-client in our mod works without messages today**: each game reads its own copies of the player list; teams, host and
match end are computed per game (`rules/teams.rs`, `features/night_match.rs`) `[VERIFIED: B1 of the agent report]`. RW-5 is the first
feature that cannot: the hit is applied in one game only (RW-8 found the same: the other game reads attacker `-1`).

## 2. How the game's own netcode works underneath

| Fact | Tag |
|---|---|
| The exe initialises and pumps Steam itself (`SteamAPI_Init`, `RunCallbacks` imports) and fetches interfaces by version string. Its P2P transport is the **old `SteamNetworking006`** (`SendP2PPacket`/`ReadP2PPacket`). No `SteamNetworkingMessages`, `Sockets` or `Utils` strings in the exe | `[VERIFIED: pefile + string scan, 2026-10-30]` |
| SC adds `SteamNetworking005`, **`SteamNetworkingMessages002`**, `SteamNetworkingUtils003`, `SteamMatchMaking009`, `SteamFriends017`, `SteamUser021`, `SteamUtils010`. Which channels SC uses, UNKNOWN (virtualised) | `[VERIFIED: nrsc.dll strings]` |
| EOS SDK is delay-imported, but only login and the **`EOS_AntiCheatClient_*` peer set** (RegisterPeer, MessageToPeer, ...): EAC in peer mode, not a transport. Correction to TECHNICAL_NOTES §3 ("not EOS") - true for transport only. How SC neutralises the EAC peer path, UNKNOWN | `[VERIFIED: delay-import table]` |
| Server: `https://cl-steam.fromsoftware-game.net:443` over HTTP with a Steam encrypted app ticket; `FromNet` has 75 classes (QuickMatch, Multiplay, Ghost, DeathBox, ~38 telemetry log requests) | `[VERIFIED: strings/RTTI]` |
| 127 network RTTI classes: `CSSessionManagerImp`, `CSTeamSessionManagerImp` (NR only), `CSNetManImp`, `SessionCreateJob`, `JoinSessionJob`, `SessionLeaveJob`, `PlayerNetworkSession`, `NetChrSetSync`, `CSChrBehaviorSyncModule`, ... | `[VERIFIED: RTTI scan]` |
| ER layouts (leads only): typed 1-byte packets (8 chr data, 11 chr type, 12 equipment, 60/61 SpEffect, 90 area, 104 EMEVD sync), explicit `send_hit` with a 0x120-byte hit struct, `p2p_send_queue` of `{steam id, bytes, length, type}`, lobby states Host=3 / Client=6, `NetChrSync` = placement + health only. `set_flag` does not network | `[SOURCE: fromsoftware-rs session_manager.rs, network_session.rs]` **ER** |
| fromsoftware-rs `nightreign` crate has params only; no NR session structs published. NR `CSSessionManager` at `exe+0x3C21EC8`, `CSNetMan` at `exe+0x3C046A8`, layouts unmapped | `[SOURCE: GitHub tree]` `[VERIFIED: T-006]` |

**What we can drive or hook directly (no SC):** (1) Steam via the flat API (done: `steam/`); (2) a second transport on
`ISteamNetworkingMessages` (new, §3); (3) the game's session jobs (`JoinSessionJob`, `SessionCreateJob`) by call or hook: MM-3g
research already has the Invite Members call chain; (4) `SendP2PPacket`/`ReadP2PPacket` call sites for tracing (NET-3), possibly to
inject our own typed packets into the game's stream (risky: the peer's packet dispatcher must know the type).

### 2a. Findings from the first investigation round (2026-10-30, static)

| Finding | Effect on the plan | Tag |
|---|---|---|
| **The Bandai-server block is not SC's job.** me3 (`start_online = false`, the default) sets the game property `Menu.IsEnableOnlineMode = false` through the game's own `CSAutoControlAPI::SetGameProperty` (me3 `host/game_properties.rs`). `CSAutoControlAPI` exposes only `SetGameProperty`, `SignIn`, `Sleep` (no session control). | UNKNOWN 5 is half answered: without SC the server path is already switched off by a stock game switch. Whether the game's *session jobs* then work (friend-invite co-op) is the live question | `[SOURCE: me3 v0.11.0 lib.rs L174, game_properties.rs]` `[VERIFIED: exe strings, method table at +0x2CBA028..]` |
| The remote-member table is a **hard-coded 2 entries**: the getter at `exe+0x1FE530` is `cmp ecx,1; ja -> null; idx*0x8A0 + [[exe+0x3C078D0]+0x10]`, and the key lookup at `exe+0x1FE560` loops `idx < 2`. Vanilla Nightreign co-op is 3 players (you + 2). So **a game without SC already does up to 3 players by design**; widening needs the table, its two bounds and the allocation, not a new protocol | Two-player PvP (what we ship now: `-MatchPlayers 2`) would not need the widening, but see §2b: the sign-in gate stops a game without SC before any session starts | `[VERIFIED: static disassembly, 2026-10-30]`; the "no SC needed for 2-3 players" hypothesis is **refuted for now** by §2b |
| `imul reg,reg,0x8A0` (the member stride) occurs at only **9 sites**, all in `exe+0x1FC3E8..0x200A68` (about 5 functions) | Counting the code that indexes the member array is small; the size of the widening is bounded by this region plus the allocation and the sync code that reads the member count (not yet counted) | `[VERIFIED: byte search; stride 0x8A0 also appears in unrelated constants, 236 hits of the raw immediate]` |


### 2b. Live results without SC (2026-10-30, two emulated instances on one PC)

Run with `NRO_NO_SC=1` (profile without `nrsc.dll`, `savefile = NRNOSC.sl2` seeded from each emulated id's own `.co2`; real player saves untouched, hashes checked by `nro`).
Scenario `scenarios/mi-nosc-join.toml`; manual probes with the new dev console command `rawcall` (`game/rawcall.rs`) and `poke`/`peek`.

| Result | Tag |
|---|---|
| **The game boots, loads the Roundtable and runs our mod without SC**, in both instances (me3, offline game property, emulated Steam). Nothing in the boot path needs SC. A fresh save shows Bandai's privacy-policy dialog (not clicked: the seeded save avoids it) | `[VERIFIED: run 1793383721, both `title_to_world` steps ok]` |
| Without SC the table menu says **"Three players"** (SC: "Up to 6") and **Invite Members is dead**: F does nothing, no `LobbyCreated_t`, no dialog | `[VERIFIED: screenshots, run 1793384109]` |
| **The gate is a sign-in state machine.** Three singletons (`exe+0x3C22048`, `exe+0x3C21E50`, `CSSessionManager exe+0x3C21EC8`) each hold a state word and a flag byte (+0x20 inside the state object). SC runs: state **2**, flag **1**. No SC after boot: state 0, flag 0. The game's own `CSAutoControlAPI::SignIn(0)` (`exe+0xF18D30`, SC calls the same method; me3 finds it by name) drives the step function `exe+0xDEC110`: state already 2 = done; **flag 0 = state 9 ("offline", failed)**; flag 1 = builds a sign-in request (platform user data, 0x41-byte profile) and goes on to state 3 (in flight) and finally 2 when an answer arrives. That answer is what Bandai's server gives in vanilla and what SC fakes | `[VERIFIED: live diff of SC vs no-SC dumps; SignIn run live (no SC: 0 -> 9; SC: reset to 0 -> 3 and stuck, no answer)]`; the reading of 2/3/9 as signed-in/in flight/offline `[HYPOTHESIS from the step function]` |
| Forcing the states by hand (`poke` state = 2, flag = 1) **kicks the game back to the title** (twice); the session manager's state alone changes nothing visible. The three objects are not independent | `[VERIFIED: runs 1793384326]` |
| **Player limit field located: `CSSessionManager+0x170` = 3 without SC, 6 with SC** (also `+0x18`, `+0xB8`... differ but are pointers). This is the number SC raises; the member table (§2a) is the other half | `[VERIFIED: live diff, 2 launches]` |

**What it means.** SC's core trick is not networking, it is **making the game believe it is signed in to Bandai without the server**, then letting the game's own session code run with a raised limit. Everything else we measured (server block by me3, table width, limit field) is small and located. The unknown is the sign-in completion: which function turns state 3 into 2 and with which data. If that is one hook, S1 is weeks; if the sign-in feeds many later server calls (create session, ranking, quick match), SC's ~40 patches exist for a reason and S1 is months.

## 3. Designs for the transport (problem a)

| # | Design | Verdict | Flaws |
|---|---|---|---|
| T1 | **`ISteamNetworkingMessages` on a private channel**, flat accessor `SteamAPI_SteamNetworkingMessages_SteamAPI_v002`, peers = Steam IDs of the SC lobby members (we already read the lobby: `LobbyApi`). Never `Client::init()` (would switch the process to manual dispatch and shut Steam down on drop: S4 §2.4). Poll with sends on a timer so no callbacks are needed; sends accept the peer's session | **Chosen.** Reliable, up to 512 KB per message, SDR relay/NAT handled by Valve `[SOURCE: steamnetworkingtypes.h; ISteamNetworkingMessages doc]` | SC may poll a range of channels (UNKNOWN, pick an odd number, log); never `CloseSessionWithUser`; no delivery notice (we add acks/seq); idle timeout (1 Hz heartbeat); gbe_fork support is by source reading only until NET-1 runs; Goldberg relay availability reported "Unknown" for ER SC `[SOURCE: goldberg issue 1]` |
| T2 | **Lobby data / member data / chat** as a message bus. Member data = one value per member (<= 8192 B, key <= 255), chat <= 4 KB | **Fallback**, same trait. Already proven under gbe_fork (`mi-queue`) and real Steam (FS-MM3) | Goes through Steam's back end (latency, rate limits UNKNOWN), no ordering guarantee beyond data updates, the lobby id is the hard part (no "current lobby" getter; for SC's lobby we have it from the probe) |
| T3 | Hook `SendP2PPacket` and add our packet type to the game's stream | Not now | The receiver must understand the type, or the game drops or errors on it; NR packet ids unknown |
| T4 | Own server / broker | Rejected (S9 §3) | Uptime, cost, identity |
| T5 | Raw UDP/TCP sockets | Rejected | NAT traversal by hand, no identity, firewall prompts |

**Security (applies to every transport):** a message comes from any Steam user who can reach us. Accept only from Steam IDs in the
current roster; parse with strict length and field caps (never trust a length); a message may request nothing but "record this
observation" (no code, no file, no memory addresses); version byte so mixed builds ignore each other (like lobby `proto`). Fuzz test
the decoder (`cargo test`), no `unsafe` in the codec.

## 4. The data channel as built (NET-1, NET-2)

Seamless Co-op stays. The channel only lets our mods tell each other about events; RW-5 is its first user ("boss B died, last hitter team
T"), the loot drop itself stays local and still needs its own RE (TASKS RW-5). Code: `crates/nrpvp/src/net/`, in every build.

```text
feature  --publish(topic,key,value)-->  net::bus  -->  worker thread  -->  Channel<PeerLink>  -->  SteamLink (ISteamNetworkingMessages)
feature  <--take(topic)---------------  net::bus  <--  worker thread  <--  Channel  <-- roster = members of the SC lobby (+ net_peer)
```

- **What a feature touches:** only `net::bus` - `publish(topic, key, value)` and `take(topic) -> Vec<Received{from, fact}>`. A fact is three
  plain numbers. A new kind of message = a new `topic` constant in `bus.rs` (with what key and value mean); the wire format, the worker and
  the tests do not change. `topic::BOSS_VERDICT` (1): key = boss id, value = team of the last hitter.
- **Guarantees (`net/facts.rs`, unit-tested with fake time):** a fact is resent every 0.5 s to each roster peer until it acks, and stays
  for peers who join later (up to 1 h or until the lobby is left); a receiver delivers it **once** however often it arrives and acks it
  (a fact refused because of a limit is *not* acked, so the sender keeps trying). If two machines publish the same key with different
  values, the **lowest publisher Steam id wins** on every machine: the loser receives the winner's fact (a later fact for a key
  supersedes the earlier one) and stops resending its own. A second `publish` of a key on one machine is refused. Keys are forgotten when
  the SC lobby is left (the next match), so reuse across matches is fine, within a match it is not. Facts of a topic this build does not
  know are dropped without recording or ack. Bounds: 256 pending, 4096 keys, 1024 unread.
- **Delivered to the other machines only;** the publisher already knows its value.
- **Roster (`net/feed.rs`):** the Steam thread (`steam/probe.rs`) asks once a second which Seamless Co-op lobbies (data key
  `nrsc_lobby_type`) we are a member of and publishes their members; getters only, nothing is ever written to a lobby. Console `net_peer`
  adds test peers. Only roster members are heard (everything else is counted as `strangers`).
- **Wire (`net/msg.rs`):** `[version][type][body]`, little endian, at most 256 bytes, strict decoder (exact length, known version and
  type, never allocates from a length field). Messages: `Hello`, `Ping`, `Pong`, `Fact`, `Ack`. Facts only, nothing asks the receiver to run
  or write anything. `PeerLink` is the port (`SteamLink` real, `FakeLink` in tests), so another transport or an own session layer can
  replace the link without touching features.
- **Switch:** config `net`. Developer builds: the file (default true; the worker idles until Steam and a roster exist). Player build: follows
  `matchmaking`. Console: `net_status`, `net_peer`, `net_ping`, `net_verdict`, `net_publish` (CONSOLE.md).
- **Not covered:** two real PCs (relay, latency, Steam rate limits): next Friend Session. Messages larger than 256 bytes or streaming.

**Open for RW-5:** a stable cross-game *boss id* (`key`). Handles/addresses differ per game; candidates are the map entity id of the boss
(EMEVD entity), its NpcParam id plus map, or the boss-map flag (`XXXX0200`/`XXXX0800`, TECHNICAL_NOTES §4e). UNKNOWN which is readable
from a live `ChrIns`; it needs a boss in a live session (`mob_kill` aids exist, RW-8). The channel treats the key as an opaque number.

## 5. Designs for the session layer (problem b)

| # | Design | Effort | Verdict | Risk |
|---|---|---|---|---|
| S0 | **Keep SC** for the session, our channel for our data | days | **Default for the next months** | SC update can break our hooks (pins), no redistribution, author's permission still needed before a public release |
| S1 | **Drive the game's session code without SC**: we create/join the Steam lobby through the game's own jobs (MM-3g style), neutralise the Bandai server path and the EAC peer path ourselves, widen the member table and the 6-slot container ourselves (the ~40 SC patches' effect, written by us from our own observations; no SC code copied) | S10 estimate 4-6+ months for the whole; first proof (two instances in one lobby, no SC) maybe 2-4 weeks of RE `[HYPOTHESIS]` | **The only real replacement.** Keeps all native sync (anims, hits, HP) | Server/EAC paths we do not understand yet (UNKNOWN 5), the remote-join handler and `CreateNetPlayer` limits, host-only world, an entire class of desyncs we meet for the first time; each game patch moves the sites |
| S2 | **Our own replication** onto puppet `PlayerIns` (position, animation id, HP, hits, SpEffects, equipment, death) over T1 | 6+ months, never finished | Rejected | Re-implements the game's sync; animation and hit timing; the puppet infrastructure proves local creation, not remote fidelity |
| S3 | Hybrid S0 now, S1 as an experiment branch behind gates | | **Recommended** | |

What can be tested on one PC with `nro` (two emulated-Steam instances, gbe_fork): everything in T1/T2 (messages, lobby data), S1's first
proof (no `nrsc.dll` in the profile; instances create/join with the game's jobs over the emulator's LAN lobbies) and the packet
trace. Not on one PC: real NAT/relay behaviour, latency/jitter, Steam rate limits, EAC-less behaviour with real peers - these go
into a Friend Session (TESTING §6).

## 6. Phased plan and gates

| Phase | Content | Cards | Gate to pass |
|---|---|---|---|
| N0 | `net` module, `PeerLink`, Messages link + lobby-data link, Ping/Pong over the SC lobby roster, `FakeLink` tests | NET-1 | `mi-net-ping` PASS (two instances exchange ping/pong both ways, > 100 messages, no SC traffic disturbed: a normal expedition still loads) |
| N1 | `Verdict` message and the boss id question; RW-5 can start | NET-2 | `mi-net-verdict` PASS |
| N2 | Observe the game's own netcode: `SendP2PPacket` histogram (type, size, rate), NR `CSSessionManager` layout | NET-3, NET-4 | Logs from two instances with counts per type; layout table in MEMORY_MAP |
| N3 | SC-less boot probe: me3 profile without `nrsc.dll`; where does the session stack stop (server step, EAC peer, member table)? | NET-5 | A written list of blockers, each with an exe address, or "two instances in one lobby" |
| **G1** | **Decision: S1 yes/no**, with the NET-3..5 evidence, a size estimate, and the answer of SC's author on permission | NET-6 (HUMAN/OPUS) | Leonardo decides (DECISIONS) |
| N4+ | Only if G1 = yes: own lobby create/join through the game's jobs, member table widening, container, saves/ini, launcher; every step with an `mi-*` scenario | cards written then | |

## 7. Legal and safety

| Topic | Position | Tag |
|---|---|---|
| Never online with a modded game | Unchanged (rule 2). Our transport is Steam P2P between players, not Bandai's servers; it must stay off when `nrsc.dll`-less and EAC-less runs could reach the official servers: the guard (`mm/guard.rs`) must be rebuilt as "no EAC module, official-server path blocked" when SC goes | `[VERIFIED: guard.rs]` |
| Anti-cheat | NR ships EAC (kernel level, press); we launch only through me3/`nrpvp_launcher` without EAC, as SC does. Press reports bans (180 days) for modified files online; no official Bandai/FromSoftware statement on offline mods found; EULA names EAC. Official policy: UNKNOWN (fetch the NR EULA and Bandai support text before any public release) | `[SOURCE: wccftech EULA, 80.lv, PC Gamer]` secondary |
| SC | No modification, patching, copying or redistribution of `nrsc.dll`; we may observe it at run time. Replacing it removes the dependency but not the duty: until S1 ships, ask the author before a public release (S9 §8). `BLOCKSYNC` in his release repo is a per-SC-version on/off table, not a flag list (corrects S4 §1) | `[SOURCE: Nexus terms; release repo]` |
| Valve | Steam P2P and lobbies are used inside the game's own app id 2622380; no Spacewar id | `[VERIFIED: join link]` |
| Peers are untrusted | §3 Security | |
| Saves | Back up `%APPDATA%\Nightreign\<steamid>\` before save-touching steps; S1 changes the save extension story (`.co2`) - a vanilla `.sl2` is never touched | CLAUDE.md |

### 7a. Ban-safety evidence and the decision of 2026-10-30

Leonardo: proceed only with enough proof that the path does not get players banned. **There is no proof, and none can exist from our side**: neither Bandai Namco nor FromSoftware has published a rule on offline mods or on third-party Steam traffic (UNKNOWN 8; searched again 2026-10-30, only press and guides). What we can show:

| Evidence | Tag |
|---|---|
| Every run launched through me3 with `start_online = false`: no `start_protected_game.exe`, no EasyAntiCheat module (nro's watchdog kills the game if EAC appears), same posture as Seamless Co-op today | `[VERIFIED: preflight P1-P6, L1 in every run]` |
| In all runs of 2026-10-30 (no-SC, rawcall, net ping) the only remote TCP endpoints were the emulator's own LAN addresses (172.25.x, 192.168.x); none to Bandai (`cl-steam.fromsoftware-game.net`) or any public address | `[VERIFIED: watch.log "tcp remote" lines of runs 1793383721, 1793384326]` |
| The data channel is Steam networking between the two players' own games (our private channel number), the same Valve service the game already uses for lobbies; it carries no account, save or Bandai data | `[VERIFIED: NET-1 code and scenario]` |
| Reports of bans concern modified files used **online** with EAC; SC's own author says there is no ban risk unless connecting with vanilla users (secondary sources) | `[SOURCE: press/guide search, 2026-10-30]` |

Not shown: that any of this is safe under Bandai's rules. Decision: **NET-1 / NET-2 (the data channel) go ahead**: they add no Bandai contact and no EAC interaction, and sit next to SC exactly as SC's own traffic does. **NET-7 (faking the sign-in) is on hold** until we have either an official statement or Leonardo accepts the residual risk in writing in DECISIONS: it changes how the game believes it is signed in to Bandai, and a mistake there (flag byte set, server reachable) could produce a real sign-in request. If it is ever run: emulated Steam ids only, `Menu.IsEnableOnlineMode` stays false, firewall-blocked game exe, and nro's TCP watch must show no public endpoint.

## 8. Open unknowns

| # | Unknown | How to find out (card) |
|---|---|---|
| 1 | NR `CSSessionManager` layout (state, players vector, steam id offset, send queue) | dump with two instances (NET-4) |
| 2 | NR packet ids and what each syncs; SC's own channel numbers | `SendP2PPacket`/`ReadP2PPacket` and Messages breakpoints (NET-3) |
| 3 | Does gbe_fork deliver `ISteamNetworkingMessages` between instances, and does SC mind our channel? | NET-1 |
| 4 | Stable cross-game boss id | NET-2 |
| 5 | What SC does about the Bandai server path and the EAC peer path | NET-5 |
| 6 | Who writes 6 into `[request+0x68]`, how the member table beyond idx 1 is sized | S10 runbook R1 hook census, NET-5 |
| 7 | Is session id `0x6001` the host from the engine or from SC? | after NET-4 |
| 8 | Official Bandai/FromSoftware policy on offline mods and P2P | read the EULA / support pages (NET-6) |
| 9 | Real-network behaviour of Messages (relay, latency, idle timeouts) | next Friend Session |
