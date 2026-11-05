# Steam matchmaking: how it works (state 2026-11-05)

This explains the matchmaking that exists today: what happens when a player presses **Solo Battle Royale**, which parts are ours, which
are the game's or Seamless Co-op's (SC), and how to read the logs when it goes wrong. The *why* behind each choice, and every test
result, is in [archive/research/S9-matchmaking-decision.md](archive/research/S9-matchmaking-decision.md) (§5c–§5g). Open task cards: `TASKS.md` MM-3g, MM-6 … MM-9.

Pins: game 1.03.3 (exe 1.3.3.0), SC v1.1.3, me3 0.11.0.

## 1. The idea in one paragraph

**Match size (decided by Leonardo 2026-10-19): every match has 6 players**, Seamless Co-op's cap. Solo = 6 players, each on their own;
Duo = 3 teams of 2; Squad (trio) = 2 teams of 3. Until the first release the player zip uses 2-player matches so two people can test
(pre-release flag, §6). Only Solo exists today.

There is no server of ours. Players who press the button meet in a hidden **Steam lobby** (the *queue lobby*) that our mod creates
and tags, so other copies of the mod can find it through Steam's lobby search. When enough players are in it, its owner becomes the
**host**. The host's game then opens a normal **Seamless Co-op lobby** by itself (the game's own multiplayer lobby, taken over by SC), and every
other player's mod sends its game into that lobby. From there it is a normal SC session: everyone is marked ready by the mod and the expedition
starts by itself, and our PvP rules apply. After Commence nobody presses anything (MM-3g, MM-7). The game's anti-cheat (EAC) and Bandai's servers are never used.

## 2. The pieces

| Piece | Who owns it | What it is |
|---|---|---|
| Queue lobby | **our mod** | Steam lobby, type *Invisible*, size `mm_max_players`. Only used to find each other and agree on host and roster. Left as soon as everyone is in the SC lobby |
| SC lobby | **the game + SC** | Steam lobby the game creates (`CreateLobby(Public, 6)`) when `game/matchstart.rs` calls the job behind Invite Members; SC adopts it (writes `nrsc_lobby_type`). This is the real session |
| Quiet invite | **our mod** | Stops the Steam invite window of a pressed Invite Members while a match waits on the host. Only for the fallback prompt now: the mod's own lobby job never opens that window (hardware breakpoint; no game, Steam or SC file is changed) |
| Start | **our mod + the game** | At ALL PRESENT each game is marked ready (`game/matchstart.rs`: what Commence does in a party), the host sends the settings first; the game departs when all are ready |
| Gate | **our mod** | The step from MATCH READY to everyone in the host's SC lobby and loaded in: publishes the SC lobby id, invites, opens join links, counts who is present |
| PvP rules | **our mod** | Unchanged: rivals get team type 29 once the expedition loads; `team_size` comes from the queue mode (Solo = 1) |

Code: `crates/nrpvp/src/mm/` (`machine.rs` queue state machine, `machine/gate.rs` gate, `worker.rs` Steam thread, `proto.rs` lobby keys),
`crates/nrpvp/src/steam/` (`queue.rs` Steam calls, `probe.rs` callbacks, `quietinvite.rs` window suppression), the table menu in
`game/menuflow.rs` and the on-screen messages in `mm/ui_msg.rs`. Also in `mm/`: `guard.rs` (never-online guard), `join.rs`, `sccreate.rs`; `steam/callback.rs`; the data channel `net/` (NETWORKING.md) reads the SC lobby roster through `steam/probe.rs`.
All Steam calls run on our own worker thread through the game's `steam_api64.dll`; nothing on the game loop waits for Steam.

## 3. What happens, step by step (Solo)

| # | Who | What happens | Status line (player build) |
|---|---|---|---|
| 1 | each player | At the Roundtable table: boss menu → pick the **Battle Royale** card (last in the list, UI-3) and Matchmaking Settings → Number of Players **Solo** → **Commence** (UI-1; any other card starts the normal expedition). Refused at the title screen or in an expedition | `Searching for players...` |
| 2 | each mod | Searches Steam for queue lobbies with the same tags (§4) and room. Joins the **lowest** lobby id found, else creates one | `Searching for players... 1/2` |
| 3 | lobby owners | If two lobbies exist, the owner of the higher id writes `merge_to`, everyone in it leaves and joins the lower one | |
| 4 | queue owner | Lobby full (6 in the final game; a test setting can start earlier, §5): closes the lobby and writes `host` (itself), `roster`, `state=go` | `Match found!` |
| 5 | host's mod | Asks the game for its lobby on the next frame (no menu, no Steam window); SC adopts it | `Match found! Creating your lobby...` |
| 6 | host's mod | Publishes the SC lobby id (`sc_lobby`) on the queue lobby. Sends a Steam lobby invite only to a player the link has not brought in after 25 s (`link+invite`) | `Match found! Waiting for players to join your lobby (1/2 here)` |
| 7 | joiner's mod | Reads `sc_lobby` and opens `steam://joinlobby/2622380/<lobby>/<host>`. Steam hands it to the running game, which joins | `Match found! Joining the host...` |
| 8 | everyone | In the SC lobby **and** visible in the local player list (`players` = roster size). Queue lobby is left | `The match is starting` |
| 9 | each mod | 2 s later marks its player ready, the host sending the settings first (Battle Royale card, row 9). All ready: the game departs by itself, about a minute to the expedition | `In match` |
| 10 | everyone | Back at the Roundtable after the expedition: the match is over, queue idle again | `Not in a queue` |

Time in FS-MM3: queue to match found 4 s, Invite Members to joiner in the lobby 3 s, loaded in 6 s later.
FS-UI2 (2026-10-21, player zip, table menu, 3 rounds, each PC host at least once): second Commence to MATCH READY under 1 s; match
ready to the host's Invite Members 11–12 s; invite to ALL PRESENT 8–9 s; both players entered the expedition within about 1 s of each
other (the two PCs' clocks differ by ~16 s; lined up on the invite/link lines).
One PC, `mi-zero-click` (2026-11-05): second Commence to MATCH READY 1 s, lobby job under 1 s, ALL PRESENT 4 s, ready 6 s, in the expedition about 65 s
after the second Commence. The Commence paths are in `re/MEMORY_MAP.md` §13.

## 4. Lobby data (queue lobby)

| Key | Written by | Value |
|---|---|---|
| `nrpvp` | creator | `1` (marks our lobbies) |
| `mode` | creator | `solo` / `duo` / `squad` |
| `proto` | creator | protocol version, `2`. Bump it when the lobby format changes, so old and new builds never meet |
| `game` | creator | `1.3.3.0` |
| `sc` | creator | SC version, `1.1.3` |
| `need` | creator | players per match (`mm_max_players`) |
| `room` | creator | RW-6 private match: hash of `private_code` (`p-…`); absent = public. Part of the search filter, so different codes never meet |
| `merge_to` | owner | lobby id this one dissolves into (step 3) |
| `host`, `roster`, `state=go` | owner | host Steam id; `party:member` pairs; the match is on |
| `sc_lobby`, `present`, `invited` | host | the SC lobby to join; `n/N` in it; who was invited |
| member `nrpvp_party`, `nrpvp_size` | each member | party leader and size (Duo/Squad tickets) |

Search filters = `nrpvp, mode, proto, game, sc, need, room` equal, free slots ≥ party size, worldwide. Players with different mod settings
(`mm_max_players`) or versions never see each other.

The SC lobby carries `inviteParam = "0"` (written by the game) plus SC's `nrsc_lobby_type = "nrsc_ynx3_seamless_master_lobby"` and
`nrsc_lobby_version`. It is locked and Public, 6 members.

## 5. Timers and give-up rules

| Rule | Value |
|---|---|
| Re-search while waiting | every ~5 s |
| Start with fewer than full (**test setting only**; the final game waits for 6) | `mm_fill_wait_s` (90 s) after the 2nd member, if at least `mm_min_players` |
| Go ahead without missing players | 60 s after `sc_lobby` is known, if at least `mm_min_players` are here: every player in the lobby is marked ready, on the same rule |
| Host's lobby did not appear | the game is asked 3 times, 5 s apart; 20 s after MATCH READY the host gets the old prompt (`Press Matchmaking Settings > Invite Members`) as a fallback |
| Ready mark | 2 s after ALL PRESENT, then twice more 10 s apart (no effect when nothing changed); only the first try sends the settings |
| Host invites a player the link did not bring | 25 s after `sc_lobby` is published |
| Joiner gives up | 120 s after `state=go` without being in the host's lobby: `match missed - queue again` |
| Left the Roundtable while searching | dropped from the queue after 3 s (not in MATCH READY: a joiner's world reloads when it joins) |
| Quiet invite armed | only while host + MATCH READY + no SC lobby yet, plus 5 s after the lobby appears |

## 6. Settings (`<Game>\NightreignPvP\nrpvp.toml`)

| Setting | Player zip | Meaning |
|---|---|---|
| `matchmaking` | `true` | Turns on queue + join + quiet invite + the automatic lobby and start + the table menu (`br_menu`) in the player build. `false` removes it |
| `mm_max_players` | `2` (pre-release) | Players per match. **Pre-release flag**: `package-release.ps1 -MatchPlayers N` (default 2) writes N here and in `mm_min_players`; the final game is 6 in every mode. Everyone must have the same number |
| `mm_min_players` | same as `mm_max_players` | Smallest match after the fill wait. Equal to the max in shipped zips, so matches only start full |
| `mm_fill_wait_s` | 90 (default) | See §5 |

Developer-only (`dev_mode = true`, developer DLL): `mm_queue`, `mm_join`, `mm_join_auto` (the player build uses `link+invite`), `mm_auto_start` (off by default in the developer build, so the older `mi-*` scenarios keep their manual steps), `mm_quiet_invite`, `mm_match_players` (sets
min = max), and the experiments `mm_probe`, `mm_lobby_trace`, `mm_sc_create`, `mm_host_create` (do not use the last one: see §8).
Driven from the console: `mm_queue_solo|duo|squad`, `mm_leave`, `mm_status`, `mm_join <method>`, `mm_sc_create`, `mm_quiet_test`, `mm_open_lobby`, `expedition_start`.

## 7. Reading the logs

Logs: `<Game>\NightreignPvP\logs\nrpvp_<UTC time>.log` (one per launch). The two PCs' clocks differ by seconds; line up on `MATCH READY`.

| Grep for | Tells you |
|---|---|
| `mm_queue:` | queue states, search results, lobby data writes, `MATCH READY host=... roster=...`, every status line |
| `mm_join:` | the gate: `host publishes sc_lobby`, `InviteUserToLobby`, `ShellExecuteW(... joinlobby ...)`, `ALL PRESENT`, `match ends` |
| `matchstart:` | `lobby job started from state N`, `ready sent (leader true/false ...)`, or why the game was not asked |
| `mm_quiet_invite:` | `ARMED`, `invite dialog suppressed ... call from nightreign.exe+0x258568B` (only after a pressed Invite Members), `disarmed` |
| `LobbyEnter_t`, `LobbyChatUpdate_t` | Steam saw us enter a lobby / someone enter or leave ours |
| `heartbeat: ... players=N` | how many players the game has loaded in this world |
| `team write ... -> 29` | the PvP rule marked a rival in the expedition |

A healthy joiner shows: `the host published sc_lobby` → `method (a) link` → `GameLobbyJoinRequested_t` → a brief `seen` / `gone` (normal) →
`LobbyEnter_t ... locked=true response=1` → `ALL PRESENT`.

## 8. Known failure patterns

| Symptom | Cause | What to do |
|---|---|---|
| Joiner's game enters the host's lobby and leaves within 0.1 s, every time | The lobby was **not** made by the game (our own `CreateLobby`, MM-3e). The game will not keep a session in it | Never create the SC lobby from the mod; the host must use Invite Members (FS-MM2, S9 §5f) |
| `2/2` in the lobby but the other player never appears; host starts alone | Steam membership without the game session (e.g. a direct `JoinLobby`) | ALL PRESENT now waits for `players` in the world; if it stays on "load in", the join did not really happen |
| Never matched | different `mm_max_players`, mod version (`proto`), game or SC version | Both use the same zip |
| Host line asks for Invite Members | the game made no lobby in 20 s (read the `matchstart:` lines), or a build without `mm_auto_start` | Press it when the line asks |
| `Preparing n/m` stays, no departure | a player was not marked ready (`matchstart: ready sent` missing on that game) or pressed the game's Cancel | That player presses Commence |
| Steam invite window opened anyway | quiet invite not armed (not host, not MATCH READY, or `mm_lobby_trace` on) | Check `mm_quiet_invite:` lines |
| Old SC lobby still around after Leave | Leaving the queue never leaves the game's own lobby (by design: it belongs to the game) | Return to the title or restart before re-queueing if anything looks stuck |

## 8b. On one PC (emulated Steam, 2026-10-27)

The whole Solo flow runs between two gbe_fork instances on one PC (`scenarios/mi-queue.toml`, PASS 3/3, orchestrator/MULTI-INSTANCE
§20): the emulator's LAN lobbies support our search filters, Invisible lobbies, lobby/member data, `InviteUserToLobby` (auto-accepted).
Queue to MATCH READY 1-2 s, lobby to ALL PRESENT ~3 s. Each member must be marked ready (`Preparing n/2`, then `Matchmaking complete 2/2`); the emulated run
takes the link out (`mm_join_auto` with `link` would start the real Steam client), so the joiner comes in by the invite there, which on real Steam is only the fallback. **The link path and what a joiner's screen shows are only for an FS.**

## 9. Not done yet

- **Two real PCs**: the zero-click flow over real Steam (FS-HM1 item 9). The table menu stays open during the departure (a `Cancel` button shows; pressing it takes the ready mark back, the mod marks again up to its 3 tries).
- **MM-6**: Duo (3 teams of 2) and Squad (2 teams of 3) (parties queue together; Steam ID → expedition player mapping for teams). Built partly, dev only, not tested.
- More than 2 players: only in automated tests. Boss choice: whatever the host picks.
- Seamless Co-op's author is not contacted: we do not modify or redistribute SC (DECISIONS 2026-11-01).
