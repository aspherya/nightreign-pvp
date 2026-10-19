# Steam matchmaking: how it works (state 2026-10-19)

This explains the matchmaking that exists today: what happens when a player presses **Solo Battle Royale**, which parts are ours, which
are the game's or Seamless Co-op's (SC), and how to read the logs when it goes wrong. The *why* behind each choice, and every test
result, is in [research/S9-matchmaking-decision.md](research/S9-matchmaking-decision.md) (§5c–§5g). Task cards: `TASKS.md` M8 (MM-1 … MM-6).

Pins: game 1.03.3 (exe 1.3.3.0), SC v1.1.3, me3 0.11.0.

## 1. The idea in one paragraph

**Match size (decided by Leonardo 2026-10-19): every match has 6 players**, Seamless Co-op's cap. Solo = 6 players, each on their own;
Duo = 3 teams of 2; Squad (trio) = 2 teams of 3. Until the first release the player zip uses 2-player matches so two people can test
(pre-release flag, §6). Only Solo exists today.

There is no server of ours. Players who press the button meet in a hidden **Steam lobby** (the *queue lobby*) that our mod creates
and tags, so other copies of the mod can find it through Steam's lobby search. When enough players are in it, its owner becomes the
**host**. The host's game then opens a normal **Seamless Co-op lobby** (the game's own multiplayer lobby, taken over by SC), and every other
player's mod sends its game into that lobby. From there it is a normal SC session: they meet at the host's Roundtable, the host starts the
expedition, and our PvP rules apply. The game's anti-cheat (EAC) and Bandai's servers are never used.

## 2. The pieces

| Piece | Who owns it | What it is |
|---|---|---|
| Queue lobby | **our mod** | Steam lobby, type *Invisible*, size `mm_max_players`. Only used to find each other and agree on host and roster. Left as soon as everyone is in the SC lobby |
| SC lobby | **the game + SC** | Steam lobby the game creates when the host presses Matchmaking Settings > Invite Members (`CreateLobby(Public, 6)`); SC adopts it (writes `nrsc_lobby_type`). This is the real session |
| Quiet invite | **our mod** | Stops the Steam invite window that Invite Members normally opens, only while a match is waiting on the host (hardware breakpoint; no game, Steam or SC file is changed) |
| Gate | **our mod** | The step from MATCH READY to everyone in the host's SC lobby and loaded in: publishes the SC lobby id, invites, opens join links, counts who is present |
| PvP rules | **our mod** | Unchanged: rivals get team type 29 once the expedition loads; `team_size` comes from the queue mode (Solo = 1) |

Code: `crates/nrpvp/src/mm/` (`machine.rs` queue state machine, `machine/gate.rs` gate, `worker.rs` Steam thread, `proto.rs` lobby keys),
`crates/nrpvp/src/steam/` (`queue.rs` Steam calls, `probe.rs` callbacks, `quietinvite.rs` window suppression), overlay panel in `overlay/`.
All Steam calls run on our own worker thread through the game's `steam_api64.dll`; nothing on the game loop waits for Steam.

## 3. What happens, step by step (Solo)

| # | Who | What happens | Status line (player build) |
|---|---|---|---|
| 1 | each player | At the Roundtable: Insert → **Solo Battle Royale**. Refused at the title screen or in an expedition | `Searching for players...` |
| 2 | each mod | Searches Steam for queue lobbies with the same tags (§4) and room. Joins the **lowest** lobby id found, else creates one | `Searching for players... 1/2` |
| 3 | lobby owners | If two lobbies exist, the owner of the higher id writes `merge_to`, everyone in it leaves and joins the lower one | |
| 4 | queue owner | Lobby full (6 in the final game; a test setting can start earlier, §5): closes the lobby and writes `host` (itself), `roster`, `state=go` | `Match found!` |
| 5 | host | Asked to press **Matchmaking Settings > Invite Members**. The game creates its lobby; SC adopts it; the quiet invite swallows the Steam window | `Match found! Press Matchmaking Settings > Invite Members to open your lobby (no Steam window will open)` |
| 6 | host's mod | Publishes the SC lobby id (`sc_lobby`) on the queue lobby and sends each roster member a Steam lobby invite | `Match found! Waiting for players to join your lobby (1/2 here)` |
| 7 | joiner's mod | Reads `sc_lobby` and opens `steam://joinlobby/2622380/<lobby>/<host>`. Steam hands it to the running game, which joins | `Match found! Joining the host...` |
| 8 | everyone | In the SC lobby **and** visible in the local player list (`players` = roster size). Queue lobby is left | `Everyone is here - host: start the expedition` / `... waiting for the host to start` |
| 9 | host | Starts the expedition the normal way | `In match` |
| 10 | everyone | Back at the Roundtable after the expedition: the match is over, queue idle again | `Not in a queue` |

Time in FS-MM3: queue to match found 4 s, Invite Members to joiner in the lobby 3 s, loaded in 6 s later.

## 4. Lobby data (queue lobby)

| Key | Written by | Value |
|---|---|---|
| `nrpvp` | creator | `1` (marks our lobbies) |
| `mode` | creator | `solo` / `duo` / `squad` |
| `proto` | creator | protocol version, `1`. Bump it when the lobby format changes, so old and new builds never meet |
| `game` | creator | `1.3.3.0` |
| `sc` | creator | SC version, `1.1.3` |
| `need` | creator | players per match (`mm_max_players`) |
| `merge_to` | owner | lobby id this one dissolves into (step 3) |
| `host`, `roster`, `state=go` | owner | host Steam id; `party:member` pairs; the match is on |
| `sc_lobby`, `present`, `invited` | host | the SC lobby to join; `n/N` in it; who was invited |
| member `nrpvp_party`, `nrpvp_size` | each member | party leader and size (Duo/Squad tickets) |

Search filters = `nrpvp, mode, proto, game, sc, need` equal, free slots ≥ party size, worldwide. Players with different mod settings
(`mm_max_players`) or versions never see each other.

The SC lobby carries `inviteParam = "0"` (written by the game) plus SC's `nrsc_lobby_type = "nrsc_ynx3_seamless_master_lobby"` and
`nrsc_lobby_version`. It is locked and Public, 6 members.

## 5. Timers and give-up rules

| Rule | Value |
|---|---|
| Re-search while waiting | every ~5 s |
| Start with fewer than full (**test setting only**; the final game waits for 6) | `mm_fill_wait_s` (90 s) after the 2nd member, if at least `mm_min_players` |
| Host: start without missing players | 60 s after `sc_lobby` is published: `N/M here - you can start; <names> did not arrive` |
| Joiner gives up | 120 s after `state=go` without being in the host's lobby: `match missed - queue again` |
| Left the Roundtable while searching | dropped from the queue after 3 s (not in MATCH READY: a joiner's world reloads when it joins) |
| Quiet invite armed | only while host + MATCH READY + no SC lobby yet, plus 5 s after the lobby appears |

## 6. Settings (`<Game>\NightreignPvP\nrpvp.toml`)

| Setting | Player zip | Meaning |
|---|---|---|
| `matchmaking` | `true` | Turns on queue + join + quiet invite in the player build. `false` removes it (and the Insert key) |
| `mm_max_players` | `2` (pre-release) | Players per match. **Pre-release flag**: `package-release.ps1 -MatchPlayers N` (default 2) writes N here and in `mm_min_players`; the final game is 6 in every mode. Everyone must have the same number |
| `mm_min_players` | same as `mm_max_players` | Smallest match after the fill wait. Equal to the max in shipped zips, so matches only start full |
| `mm_fill_wait_s` | 90 (default) | See §5 |
| `overlay_toggle_key` | Insert (default) | Opens the matchmaking panel |

Developer-only (`dev_mode = true`; the mmtest zip from `package-release.ps1 -MatchmakingTest`): `mm_queue`, `mm_join`, `mm_join_auto`,
`mm_quiet_invite`, `mm_match_players` (sets min = max), Duo/Squad buttons, and the experiments `mm_probe`, `mm_lobby_trace`,
`mm_sc_create`, `mm_host_create` (do not use the last one: see §8).

## 7. Reading the logs

Logs: `<Game>\NightreignPvP\logs\nrpvp_<UTC time>.log` (one per launch). The two PCs' clocks differ by seconds; line up on `MATCH READY`.

| Grep for | Tells you |
|---|---|
| `mm_queue:` | queue states, search results, lobby data writes, `MATCH READY host=... roster=...`, every status line |
| `mm_join:` | the gate: `host publishes sc_lobby`, `InviteUserToLobby`, `ShellExecuteW(... joinlobby ...)`, `ALL PRESENT`, `match ends` |
| `mm_quiet_invite:` | `ARMED`, `invite dialog suppressed ... call from nightreign.exe+0x258568B`, `disarmed` |
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
| Host line stays on "press Invite Members" | host did not press it, or pressed it before MATCH READY | Press it when the line asks |
| Steam invite window opened anyway | quiet invite not armed (not host, not MATCH READY, or `mm_lobby_trace` on) | Check `mm_quiet_invite:` lines |
| Old SC lobby still around after Leave | Leaving the queue never leaves the game's own lobby (by design: it belongs to the game) | Return to the title or restart before re-queueing if anything looks stuck |

## 9. Not done yet

- **MM-3g**: the host still presses Invite Members. Goal: the mod triggers that game action itself (call chain in `re/MEMORY_MAP.md` §12).
- **MM-6**: Duo (3 teams of 2) and Squad (2 teams of 3) (parties queue together; Steam ID → expedition player mapping for teams). Built partly, dev only, not tested.
- **MM-5**: the "Battle Royale" choice at the expedition table instead of the overlay button.
- More than 2 players: only in automated tests. Boss choice: whatever the host picks.
- Before any public release: ask SC's author (he does not want random matchmaking in SC; S9 §8).
