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

## 5c. FS-MM1 first friend run (Leonardo host + fuso, 2 PCs, real Steam, 2026-10-14 02:18–02:38Z, mmtest zip of 2026-10-12)

Logs: `../experiments/FS-MM1/{host,client}/`. fuso's clock is ~13.7 s ahead of Leonardo's (same lobby events). Only step 2 of the
runbook (Solo) was run; baseline, Duo and the method buttons were not. Times below are Leonardo's.

| Finding | Tag |
|---|---|
| Queue Solo on two PCs converged to one queue lobby with one owner/host and the same `roster=` on both, four times (two creates, two joins, incl. host = either player); Leave + re-queue works | `[VERIFIED: both logs 02:22:54, 02:25:39, 02:26:07]` |
| U2 closed: **opening Invite Members alone creates lobby A** (Leonardo: "I had to click Invite Players", nobody picked). At 02:26:18 `LobbyCreated_t` limit 6 locked; the host's mod published `sc_lobby` and sent `InviteUserToLobby` in the same tick (`-> true`) | `[VERIFIED: host log 02:26:18 + Leonardo's report]` |
| Lobby A does not exist until the host opens that screen, and is gone after the expedition (`is gone or we are not in it` 02:37:04). The host step is needed **every match** | `[VERIFIED: host log 02:22:54–02:24:57, 02:37:04]` |
| U4(b) works on SC's locked lobby: joiner gets `LobbyInvite_t`, accepts in the Steam overlay → `GameLobbyJoinRequested_t` → `LobbyEnter_t` lobby A → ALL PRESENT both PCs. MATCH READY → ALL PRESENT took 36 s (22 s of it the human accept) | `[VERIFIED: client log 02:26:33–02:26:57 (its clock)]` |
| U4(a), one data point: the link (`steam://joinlobby/2622380/<A>/<host>`, pressed by the joiner while already a member) was delivered to the **running** game as `GameLobbyJoinRequested_t`; the game left and re-entered lobby A within 1.6 s; no second game copy started | `[VERIFIED: client log 02:27:34–02:27:37 (its clock), host log 02:27:22–23]` |
| Whether the link works for a joiner who was never invited (locked lobby) | UNKNOWN: FS-MM2 |
| Joiners end up in lobby A; at expedition start a further lobby (`…477`, limit 6) is entered by both: the game's lobby B. U5 answered for this path | `[VERIFIED: both logs 02:27:41]` |
| Phase detection (team_type 2) fired on both PCs; teams 0/1 with the remote written 2 → 29 on each side (rivals); the downed rule fired on both PCs with identical values, 7 episodes | `[VERIFIED: both logs 02:28:27–02:36:36]` |
| Defect: after the expedition the status stayed `IN EXPEDITION - match active` with present 0/2 and `wcm=0x0` until the logs end (host 8 s, client ~70 s); the match only ends on a seen Roundtable team_type 1. What the players went to (Roundtable or title) is UNKNOWN | `[VERIFIED: both logs]` |
| Defect: at ALL PRESENT the host leaves the queue lobby, Steam hands its ownership to the joiner, and the joiner logs `not using the published sc_lobby: queue lobby owner … is not the host`. Harmless here (already present) | `[VERIFIED: client log 02:26:57]` |
| Noise: `T-006: timeout after 180 s waiting for WorldChrMan` during the expedition load on both PCs | `[VERIFIED]` |

Decision (Opus, 2026-10-14): the joiner's accept click is removed with `mm_join_auto = "invite+link"` as the mmtest default
(invite first, so the locked lobby admits the joiner; the link replaces the click). The guarded link path is kept (app id + overlay
+ safety guard). The host's Invite Members step stays as an on-screen prompt for MM-4 until MM-3c finds what creates lobby A.

## 5d. MM-3c lobby trace (Leonardo solo, 2026-10-14 22:14–22:18Z, dev build with `mm_lobby_trace`)

Log: `dist\logs\nrpvp_2026-10-14_22-14-09.log` (copy in `../experiments/MM-3c/`). 135 threads armed, 0 failed; no hang or crash reported.

| Finding | Tag |
|---|---|
| Opening Invite Members makes **the game itself** call `ISteamMatchmaking::CreateLobby(type 2 = Public, 6 members)` on its main-loop thread (stack: `nightreign.exe+0x25919A1` ← `+0x2595B68` ← `+0x258E157` ← `+0x25857D4` ← `+0x257F5DA` ← task system `+0xDEC332` ← main loop). No `nrsc.dll` frame | `[VERIFIED: trace 22:18:21.725Z]` |
| The vtable target of `SteamAPI_SteamMatchmaking_v009()`'s `CreateLobby` (slot 13) is at `0x7FF789010B10`, **in no module** (below the exe image), and `this` is a heap object: Seamless Co-op wraps the matchmaking interface with its own proxy. `ISteamFriends` is not wrapped (its slot-33 target is in `steamclient64.dll`) | `[VERIFIED: trace start-up lines]`, proxy owner = SC `[HYPOTHESIS]` |
| 250 ms later `LobbyCreated_t` → lobby A, `locked=true`; 300 ms later the game calls `ISteamFriends::ActivateGameOverlayInviteDialog(lobby A)` from the menu path (`+0x258568B` ← menu functor calls `+0x213B4F`, `+0x80E32A`, `+0x80D743`) | `[VERIFIED: trace 22:18:21.977Z, 22:18:22.292Z]` |
| The online/session functions (`0x257F5DA`…`0x25919A1`, `0x258568B`) have no `.pdata` entries and enter through jump chains (anti-tamper obfuscation): calling them from the mod is not a cheap route | `[VERIFIED: frames.py over nightreign.exe on disk]` |
| So SC's lobby A = the game's own "invite" lobby (Public, 6) as passed through SC's proxy, which locks it and adds `nrsc_*` data. Our queue lobby (`CreateLobby(Invisible, 2)` through the same export) did **not** become an SC lobby | `[HYPOTHESIS]` built on the rows above |

Next (MM-3d): the host's mod calls the proxied `CreateLobby(Public, 6)` itself and checks whether SC adopts it (`nrsc_lobby_type` appears). If it does, and a friend can join it, the Invite Members step goes away without touching game or SC code.

## 5e. MM-3d: the mod's own lobby is adopted (Leonardo solo, 2026-10-14 22:33–22:38Z)

Log: `../experiments/MM-3d/2026-10-14_sc_create_solo.log`. Nothing else on screen changed when the button was pressed (overlay line green).

| Finding | Tag |
|---|---|
| Our `CreateLobby(Public, 6)` from the mod's Steam thread went through the same proxied vtable target as the game's (`0x7FF789010B10`) and the lobby carried `nrsc_lobby_type = nrsc_ynx3_seamless_master_lobby`, `nrsc_lobby_version = 1.1.3`, `locked = true` **in the `LobbyCreated_t` itself** (+0.0 s); our SC detection picked it up | `[VERIFIED: log 22:36:58]` |
| So SC adopts by the call (Public, 6) and not by caller; `CreateLobby(Invisible, 2)` (the queue lobby) is not adopted | `[VERIFIED: this log + FS-MM1]`; exact rule `[HYPOTHESIS]` |
| Difference from the game's lobby: ours has no `inviteParam = 0` (the game writes it on its own lobby) | `[VERIFIED: log 22:36:58 vs 22:37:37]` |
| Opening Invite Members afterwards made the game create a **second** adopted lobby (`…717`, with `inviteParam`); the player was a member of both | `[VERIFIED: log 22:37:37–22:37:49]` |
| Whether a joiner who enters our lobby is connected by SC (Roundtable, expedition) | UNKNOWN: FS-MM2 step 1 |

Decision (Opus, 2026-10-14): MM-3e makes the host's mod create the SC lobby itself at MATCH READY in the mmtest build, with
Invite Members as the fallback prompt after 45 s. The host must not also open Invite Members while our lobby exists (two SC lobbies).

## 5f. FS-MM2 step 1 (Leonardo + SamukaSamukando, 2 PCs, real Steam, 2026-10-19 17:24–17:28Z, mmtest zip of 2026-10-14)

Logs: `.claude/experiments/FS-MM2/{host,client}`. Game 1.03.3, SC 1.1.3, me3 0.11.0, same `nrpvp.dll` on both PCs. The friend's clock
runs about 14.7 s ahead (MATCH READY 17:25:35.57 there, 17:25:20.87 here); times below are Leonardo's. Try 1: friend hosted. Try 2: Leonardo hosted.

| Finding | Evidence |
|---|---|
| Queue matched in under 1 s both tries; host, roster and team size 1 agreed on both PCs | `[VERIFIED: both logs]` |
| The host's mod created the SC lobby and SC adopted it at +0.0 s, both tries, on both PCs | `[VERIFIED: mm_host_create: ADOPTED]` |
| The joiner's link and invite reached the joiner's game (`GameLobbyJoinRequested_t` 0.2 s after the link), but the lobby was "seen" and then "gone or we are not in it" within 0.1 s; the joiner never became a member. Same in both tries | `[VERIFIED: 17:24:28–29 here, 17:25:37 friend]` |
| The friend's manual accept 19 s later gave the same result | `[VERIFIED: friend 17:25:55]` |
| The dev button "Direct" (`JoinLobby` by our mod) put the friend in the Steam lobby: 2/2, `ALL PRESENT` on both PCs | `[VERIFIED: friend 17:26:05 = 17:25:51 here]` |
| The host started the expedition (team_type 2 at 17:27:07) with 1 player in its world; the friend's game never left the Roundtable (his WorldChrMan never changed). The host's world unloaded at 17:27:15 and its log ends at 17:27:32 with no shutdown line | `[VERIFIED: both logs]`; why the host's world ended UNKNOWN (ask Leonardo) |
| Being in the Steam lobby is not being in the SC session. Our lobby has 2 data keys; the game's own lobby has 3 (`inviteParam = 0` too). The game's session code never opened ours, so the joiner's game leaves it at once and the host's game has nothing to bring a member in | `[HYPOTHESIS: strong]`: the game-made lobby worked in FS-MM1 with the same join paths |
| A real SC join shows in the host's ChrSet: FS-MM1 host heartbeat `players=2` at the Roundtable from 02:26:53; here it stayed `players=1` on both PCs | `[VERIFIED: FS-MM1 host log, both FS-MM2 logs]` |
| `ALL PRESENT` (Steam membership) is a false positive when the member is not in the SC session | `[VERIFIED: this run]` |
| Leaving the queue does not leave an adopted SC lobby: the friend stayed in his try-1 lobby `…357` through try 2 | `[VERIFIED: friend log 17:26:03 onwards]` |

Decision (Opus, 2026-10-19): the mod-created lobby (MM-3e) is dropped as the join path; `mm_host_create` goes off in the mmtest build.
The SC lobby must come from the game's own session code. MM-3f: the host's Invite Members (the game's path) creates the lobby, and our
mod stops the Steam invite dialog from opening and sends the invite itself. That is Leonardo's "Start Match" idea of 2026-10-14, applied to
the host. After MM-3f: trigger that game action from the queue without the menu (MM-3g, RE of the call chain in §5d).

## 5g. FS-MM3: Solo works with one host click (2026-10-19 18:01–18:13Z)

Logs `.claude/experiments/FS-MM3/`. With the game's own lobby (host pressed Invite Members, MM-3f skipped the Steam window), the joiner's
automatic link worked: his game briefly saw the lobby, then entered it 1.9 s after the link `[VERIFIED: client 18:02:23–25]`, appeared in the
host's ChrSet (`players=2`) `[VERIFIED]`, and played the expedition as a rival `[VERIFIED: team write on both PCs, downed rule hits]`. The match
ended at the Roundtable by itself on the host `[VERIFIED]`. This confirms §5f: the join path is fine; only a lobby made by the game works.
The one remaining manual step is the host's Invite Members press (MM-3g).

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
