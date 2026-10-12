# S9: Public matchmaking, the chosen design

Opus, 2026-10-12. This is the decision. [S8](S8-public-matchmaking.md) (Codex) is the input it answers.
Tags as in the other docs: `[SOURCE]`, `[VERIFIED]`, `[HYPOTHESIS]`, `UNKNOWN`.

## 1. Decision

**Steam is the matchmaker. No server of our own.**

Each queued player's mod joins a small **queue lobby**: a second Steam lobby that our DLL creates and searches for, tagged with the
mode. When the queue lobby is full, its owner is the match host, and every other player's mod joins the host's Seamless Co-op (SC)
lobby through the same Steam path a friend invite uses. From there the game and SC behave exactly as in a private session today.

Order of delivery: **Solo first**, then Duo and Squad. Solo needs no team agreement (everyone is a rival, `team_size = 1` already
works), so it isolates the one hard question: can strangers be put into one SC lobby.

## 2. The hard constraint (never EAC, never Bandai servers)

| Piece | Whose service | Note |
|---|---|---|
| Queue lobby, lobby search | Steam (Valve) | New, ours. `ISteamMatchmaking`, the interface SC already uses `[VERIFIED: nrsc.dll strings]` |
| Joining the host | Steam lobby join | Same path as SC's "Invite Members" |
| Gameplay traffic | Steam P2P via SC | Unchanged |
| EAC | not running | Launch only through `nrpvp_launcher.exe` / SC, as today |
| Bandai matchmaking | blocked by SC and me3 | Unchanged `[SOURCE: T-003]` |

The design adds no new network endpoint except Steam. Guard to build in: the mod refuses to queue unless `nrsc.dll` is loaded in the
process and no EAC module is, and says why on screen.

## 3. Why not the broker in S8

| | Steam queue lobby (chosen) | External broker (S8) |
|---|---|---|
| Hosting, uptime, cost | none | a server we run forever |
| Player identity | Steam's, cannot be faked | client-claimed unless we add Steam ticket checks |
| Getting players into one SC lobby | needed | needed all the same; the broker does not help with it |
| Queue logic (search, fill, timeout) | lobby search + lobby data | custom code, but more flexible |
| Local testing | Nucleus emulates lobbies `[SOURCE: S4 §4]` | needs the service running |

The broker only buys richer queue rules, which a six-player hobby population does not need. It stays as the fallback if Steam lobby
search turns out unusable for this app id.

S8 frames the gate as "merging two SC lobbies". We do not merge. Players move one by one into the host's lobby; a pre-made party is just
several players who move to the same host.

## 4. Facts this rests on

| Fact | Tag |
|---|---|
| SC players join each other through the game's Matchmaking Settings → Invite Members (a Steam invite). SC has no random matchmaking | `[SOURCE: PC Gamer / Gamerant / Deltia SC guides, search 2026-10-12; T-003]` |
| Accepting an invite posts `GameLobbyJoinRequested_t` to the running game | `[SOURCE: partner.steamgames.com/doc/api/ISteamMatchmaking]` |
| Lobby type Invisible: "Returned by search, but not visible to other friends." "A user can be in only one regular lobby, and up to two invisible lobbies." | `[SOURCE: same page]` |
| `LobbyEnter_t` is also received by the creator of a lobby | `[SOURCE: same page]` |
| Lobby search defaults to nearby regions; a distance filter widens it. String filters select on lobby data | `[SOURCE: same page]` |
| `SetLobbyData` is owner only; `SetLobbyMemberData` is per member | `[SOURCE: S4 §2.3]` |
| Call results can be polled with `ISteamUtils::IsAPICallCompleted` / `GetAPICallResult` (documented as the back end of CallResult; "not recommended" but available) | `[SOURCE: partner.steamgames.com/doc/api/ISteamUtils]` |
| Never use `steamworks::Client::init()` in this process; use the flat accessors of the already-loaded `steam_api64.dll` | `[SOURCE: S4 §2.4]` |

## 5. Unknowns, and the spike that settles each

| # | Unknown | Spike | Who can run it |
|---|---|---|---|
| U1 | Do the flat accessors work from our DLL after SC initialised Steam? | MM-1 | Leonardo solo |
| U2 | When does SC create its lobby, what type is it, and can we learn its id? (candidate: our own listener for `LobbyEnter_t` registered with `SteamAPI_RegisterCallback`) | MM-1 | Leonardo solo |
| U3 | Does our own invisible lobby (create, tag, search, join, member list) work next to SC's lobby without disturbing it? | MM-2 | Nucleus, 2–3 instances |
| U4 | **The gate.** Can a player enter the host's SC lobby without a manual invite? Try in order: (a) open `steam://joinlobby/<appid>/<lobby>/<host>` `[HYPOTHESIS: Steam posts the same join request as an accepted invite]`; (b) host's mod calls `InviteUserToLobby`, joiner clicks accept once; (c) joiner's mod calls `JoinLobby` itself and SC reacts to `LobbyEnter_t` | MM-3 | Friend Session (real Steam) |
| U5 | Does U4 work between two accounts that are **not Steam friends**? | MM-3, second half | Friend Session, after unfriending for the test |
| U6 | Can the host's mod start the expedition, or does the host press the button? | later | not needed for v1: the host presses it |
| U7 | Steam ID ↔ in-expedition player (needed for Duo/Squad only) | MM-6 | Nucleus |
| U8 | Six-player stability | M7 | Friend Session |

If U4(a) fails but (b) works, v1 ships with one click per joiner. If all three fail, public matchmaking needs SC's author, and we stop
and ask (see §8).

## 5a. MM-1 result (solo run 2026-10-12, game 1.03.3, SC 1.1.3, me3 0.11.0)

Log: `../experiments/MM-1/2026-10-12_mm_probe_solo.log`. Game and SC behaved normally; no crash.

| Finding | Tag |
|---|---|
| U1 answered: all 13 flat exports resolve; `SteamAPI_GetHSteamUser()` = 1 at DLL start; the four interface accessors return non-null; own Steam ID read | `[VERIFIED: MM-1 log]` |
| Our listen-only callback objects register (flag set) and receive events next to SC, on the standard callback pump | `[VERIFIED: MM-1 log]` |
| U2 answered in part: no lobby exists at launch or on arriving at the Roundtable. **Lobby A** appeared 40 s after the Roundtable loaded: owner = local player, limit 6, `LobbyEnter_t.locked = true`, data `nrsc_lobby_type = nrsc_ynx3_seamless_master_lobby`, `nrsc_lobby_version = 1.1.3`, `inviteParam = 0` | `[VERIFIED: MM-1 log 14:13:37Z]` |
| **Lobby B** appeared 47 s later, 19 s before the expedition map loaded: owner = local player, limit 6, `locked = false`, data only `inviteParam = 0` | `[VERIFIED: MM-1 log 14:14:24Z]` |
| Leonardo's actions in that run: around the time lobby A appeared he **sent an invite to a Steam friend** from Invite Members; he started the expedition with **Solo Mission**. So lobby A is created by the invite flow (opening the screen or sending the invite: not separated), and lobby B is created even for a Solo Mission start | `[VERIFIED: Leonardo's report 2026-10-12 + log times]` |
| Consequence: a host who has invited nobody has **no lobby A**. Whether merely opening Invite Members creates it, and whether starting with "Up to 6 players" alone creates one, is UNKNOWN (2-minute solo check). If only a sent invite creates it, the host's mod needs another way to make SC open its lobby | UNKNOWN |
| Lobby A is SC's joinable lobby and the `nrsc_*` data is what SC checks for its "host is not using seamless co-op" / "version does not match" errors | `[HYPOTHESIS]` |
| Lobby B is the game's own session lobby for the expedition; whether joiners end up in A, B or both | UNKNOWN: MM-3 |
| `locked = true` means "only invited users may join" `[SOURCE: ISteamMatchmaking docs]`. So U4(a) and U4(c) may be refused by Steam for lobby A, and U4(b) (an invite sent by the host's mod) is the path Steam designs for a locked lobby. Extra candidate **U4(d)**: host's mod, as lobby owner, calls `SetLobbyType` to unlock lobby A (changes a Steam setting on SC's lobby at runtime; does not touch SC's files) | `[HYPOTHESIS]`: MM-3 |
| Steam has no callback for our own player leaving a lobby, so whether lobby A still exists during the expedition is UNKNOWN; poll `GetNumLobbyMembers` in MM-3 | UNKNOWN |

## 5b. MM-2 first run in the real game (Leonardo solo, 2026-10-12, dev build with MM-2 + MM-3)

Log: `../experiments/MM-2/2026-10-12_queue_solo.log`. No crash; no report of anything odd on screen.

| Finding | Tag |
|---|---|
| Queue Solo: `RequestLobbyList` → 0 results → `CreateLobby(Invisible, 2)` OK → tags written (`SetLobbyData` all true) → status "solo 1/2 searching". Call results were readable (`LobbyCreated` result 1) | `[VERIFIED: log 15:34:41–42Z]` |
| The repeated search finds our own lobby by its tags (`lobbies_matching=1`), so tag filtering and worldwide search work for this app id | `[VERIFIED: log]` |
| Leave → `LeaveLobby` → Idle; Queue Solo again creates a new lobby. Duo and Squad are refused when alone, with the reason shown | `[VERIFIED: log 15:34:48–59Z]` |
| U3, first half: SC's lobby A (`nrsc_lobby_type`, limit 6) was created at 15:40:55Z **while our Invisible queue lobby was held**, and both were listed with the local player as member for 9 s until Leave. Steam allowed both | `[VERIFIED: log 15:40:55–15:41:04Z]` |
| What Leonardo did at 15:40:55Z (opened Invite Members only, or sent an invite) | UNKNOWN: ask |
| Cosmetic defect: a queue lobby we left keeps being logged every 10 s as `members=[0]` | `[VERIFIED: log]`, fix with the next build |
| Still untested: two real players in one queue lobby, the duplicate-lobby rule, every join method | FS-MM1 |

## 6. Solo flow (v1)

1. Roundtable. Player picks **Solo Battle Royale** (v1: a button in our overlay; later the table entry from S7 route B).
2. Mod searches queue lobbies: `nrpvp=1`, `mode=solo`, `proto=<n>`, `game=<build>`, `sc=<version>`, open slots, worldwide distance.
   Found → join it. None → create one (Invisible, 6 members) and tag it.
3. Two players who both created a lobby: each keeps searching; the owner of the lobby with the higher id leaves and joins the lower.
4. Overlay shows "Searching… 3 / 6".
5. Lobby full, or 90 s passed with at least `min_players` (config, default 2 during development): the owner closes it to new joins
   and writes `state=go`, `host=<steam id>`, `sc_lobby=<id>`, `roster=<steam ids>`.
6. Everyone but the host joins the host's SC lobby (U4). Overlay on all: "Match found, joining…".
7. Host sees "6 / 6 present, start the expedition" and starts it the normal way. Mode on every client is already Solo
   (everyone came from the same queue), so `team_size = 1` is applied with no further agreement.
8. Anyone missing 30 s after `go`: the host starts without them if `min_players` is still met, else everyone returns to step 2.
9. After the match each player leaves the host's SC lobby (back to their own Roundtable) and can queue again.

Threading: Steam calls and the queue state machine run on our own worker thread and pass plain messages to the game loop and overlay.
Nothing on the game loop waits for Steam.

## 7. Duo and Squad (after Solo works)

- A party forms the normal SC way (invite friends), then the leader picks Duo or Squad. The queue ticket is the whole party:
  all of them join the same queue lobby. The leader publishes the queue lobby id as its member data on the SC lobby (id known from U2);
  the members' mods read it and follow. No extra channel needed.
- Queue lobby member data carries `party=<leader steam id>`. Match is complete at 3 parties of 2 or 2 parties of 3. Only complete
  parties queue in the first version; no solo fill.
- Host = owner of the queue lobby; the host's party is already in the right SC lobby; the other parties move in.
- Teams are fixed by the roster (Steam IDs). Each client maps Steam ID → expedition player (U7) and applies the existing team rule.
  Join order must not be used: ids are renumbered at expedition start `[VERIFIED: run 21]`.

## 8. Before any public release

SC's author has said he does not want random matchmaking in SC and forbids bundling or modifying it `[SOURCE: T-003]`. This design
modifies nothing of his and ships nothing of his, but it does give SC sessions public matchmaking. Ask him before a public release.
Private testing with friends is unaffected.

## 9. Task cards

MM-1 … MM-6 in [TASKS.md](../TASKS.md), section "M8 — Public matchmaking".
