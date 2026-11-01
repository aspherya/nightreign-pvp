# S8: Public matchmaking and party/session formation

## Purpose

This document proposes how to queue individual players and pre-formed teams into the same Nightreign expedition. It is a design discussion, not an implementation specification. The intent is to compare architecture options and identify the feasibility work before implementation.

The product needs to cover all three Battle Royale formats:

| Mode | Match composition at six players | Queue unit |
|---|---|---|
| **Solo Battle Royale** | Six players; each player is their own team | One player |
| **Duo Battle Royale** | Three teams of two | A pre-formed duo; whether solo players may fill an incomplete duo is an open product decision |
| **Squad Battle Royale** | Two teams of three | A pre-formed three-player squad |

The requested example is the Squad flow: three friends form a Seamless Co-op party, their leader selects **Squad Battle Royale** and presses **Start Expedition**, then that squad is matched with another three-player squad. The same matchmaking/session design should support the Solo and Duo compositions above.

For the first playable queue prototype, keep the queue rules simple: Solo accepts one-player tickets; Duo accepts complete two-player parties; Squad accepts complete three-player parties. Filling incomplete Duo/Squad teams with solo tickets is a later product decision. The initial match target is six players, though smaller sessions can be used for development tests.

## Executive summary

Use a small matchmaking service to match **queue tickets**, while retaining Seamless Co-op as the game-session and gameplay transport if it can host the combined match. A ticket represents one player in Solo, a two-player party in Duo, or a three-player party in Squad. The total match target is six players for all three formats: six solo teams, three duos, or two squads.

The service can choose *which* parties should play together. It cannot, on its own, merge two existing Seamless Co-op lobbies or move their players into one game session. That session-joining step is the first feasibility gate and should be proven before building a full matchmaking backend.

If Seamless Co-op can merge/join the selected players and parties through its supported flow, matchmaking is a modest service plus client integration. If it cannot, session integration or replacing the networking/session layer becomes the central project. Avoid building custom gameplay packet transport until this question is answered.

## Findings from the current repository

The following are documented in `../TECHNICAL_NOTES.md`, `../ROADMAP.md`, and the current Rust code:

- Seamless Co-op is the current networking foundation. The repo notes identify Steam networking interfaces in `nrsc.dll`, a six-player lobby, and no public Seamless Co-op extension API. See [technical notes, Seamless Co-op](../TECHNICAL_NOTES.md#3-seamless-co-op-networking-foundation).
- The project has verified two-player PvP over real Steam networking between two PCs. Existing game networking synchronizes combat; custom matchmaking is not currently part of that proof. See [roadmap](../ROADMAP.md).
- The repo distinguishes a Seamless Co-op **party** (the players in one network session) from a gameplay **team** (who is allied or hostile). A matchmaker must produce a shared party/session; the mod must separately make every client agree on gameplay teams.
- The current mod is a Rust native DLL. Its game-facing loop polls game state and processes overlay commands. There is no matchmaking client, queue protocol, backend service, or supported Seamless Co-op API in the current implementation.
- The existing product planning already names public Solo, Duo, and Trio queues for six players: up to six solo players, three duos, or two trios. This proposal uses **Squad** in player-facing text for the three-player team format; “trio” remains an equivalent internal term. See [PROJECT_PLAN.md, public queues](../PROJECT_PLAN.md#3-public-queues-and-initial-player-cap) and [FUTURE_CONSIDERATIONS.md, public matchmaking](../FUTURE_CONSIDERATIONS.md).
- The current overlay is mainly a development/debug surface. In player mode, the badge is read-only; it is not yet a production matchmaking UI. See `crates/nrpvp/src/overlay/mod.rs` and `crates/nrpvp/src/game/mod.rs`.
- Player IDs observed in the lobby can change when the expedition loads. The documented expedition IDs were consistent across clients, but lobby IDs for joiners differed from their expedition IDs. Team assignment should therefore not be keyed only by a lobby-time in-game ID. See [technical notes, player IDs](../TECHNICAL_NOTES.md#4-engine-findings-relevant-to-pvp).
- The mod's PvP work shows that each client applies team/hostility values locally. Every client needs the same roster-to-team mapping. Free-for-all team value 29 has been used in experiments, but it does not provide squad allies. Squad-specific team semantics remain a separate gameplay requirement.
- The repo notes identify Seamless Co-op networking as Steam-based. Avoiding Bandai's matchmaking servers and EAC does not itself remove the need for a game transport/session. The proposed design still relies on Seamless Co-op/Steam unless that layer is deliberately replaced.

These are repository findings, not a claim that Seamless Co-op officially supports arbitrary matchmaking or merging pre-existing lobbies.

## Proposed architecture

### Matchmaking discovery options

The project's existing planning already suggests Steam lobby discovery with mod/mode/version tags, or an external service/Discord as a fallback ([FUTURE_CONSIDERATIONS.md](../FUTURE_CONSIDERATIONS.md)). This note keeps both options open:

- **Use Steam lobby discovery/invites:** potentially the fewest moving parts if the mod can safely inspect, tag, search, and join the lobby Seamless Co-op already owns. The repo has not established that this is possible, or that modifying lobby metadata is safe/allowed.
- **Use an external matchmaking broker:** gives control over queues, party tickets, ready checks, timeouts, and future regional/version filters. It still needs a way to direct clients into one real Seamless Co-op/game session; it does not replace that session layer.
- **Use manual/Discord invites temporarily:** useful for testing the team/match flow while automated discovery is unresolved, but it is not the intended public matchmaking experience.

The recommendation is not to commit to the broker before the session spike. First test the Steam/Seamless join path and determine whether lobby discovery is usable. Then choose the simplest coordinator that can produce the required six-player session.

### 1. Matchmaking coordinator

Run a small external service, separate from the injected DLL, if direct Steam lobby discovery is not viable. Its job is limited to queue coordination:

- accept and expire party tickets;
- match queue tickets by mode, number of players, team/party size, game/mod protocol version, and optionally region;
- create one match assignment and a ready check;
- designate a destination/host party if the session technology needs one;
- return the same match assignment to all members of each party;
- cancel or remake an assignment if a member fails the ready check or disconnects before launch.

The service does **not** need to simulate the game, decide hits, or relay game packets if Seamless Co-op continues to carry gameplay traffic.

For a first local prototype, the service can run on one developer machine. A public/always-on deployment can come later, after the session join flow is proven.

### 2. Mode- and party-aware client

One player/client (or a party leader for a pre-formed team) creates a queue ticket. Solo tickets represent one player. Duo/Squad tickets should keep their pre-formed members together. Whether incomplete teams can be filled with solo-queued players is not yet decided. Tickets should include at minimum:

```text
mode                 solo_battle_royale | duo_battle_royale | squad_battle_royale
team_size            1 | 2 | 3
player_count          equal to team_size for initial Duo/Squad tickets; 1 for Solo
party_id             stable id for the pre-formed group (Duo/Squad)
protocol_version     versioned mod matchmaking protocol
game_version         Nightreign version
mod_version          nrpvp version
party/session handle whatever stable join target the session layer supports
```

The service should not need personal data beyond what is necessary to identify the player/party and help members join the same match. A stable platform identity may be needed for roster mapping, but its source and privacy implications need to be decided.

### 3. Session formation

After tickets totaling six players are matched, those players/parties must end up in the **same Seamless Co-op session**. Possible mechanisms, ordered by preference:

1. Use a documented or ordinary Seamless Co-op invite/join flow, if it supports bringing the second party into the first party's lobby.
2. Use the game's/Steam's existing lobby join mechanism only if experiments establish it can safely coexist with Seamless Co-op and the resulting session is recognized by the game and Seamless Co-op.
3. If neither works, investigate a supported Seamless Co-op extension or a different session/networking design before committing to a backend implementation.

Do not assume that knowing a Steam lobby ID or creating a separate Steam lobby automatically joins Nightreign's gameplay session. Seamless Co-op owns or participates in session state, and the game must recognize all players in one expedition.

### 4. Ready check and expedition start

Recommended state flow:

```text
Idle
  -> QueueUnitReady
  -> Queued
  -> MatchFound
  -> ReadyCheck
  -> JoiningDestinationSession
  -> AllPlayersPresent
  -> ExpeditionStarting
  -> InMatch
  -> Complete / Cancelled
```

Before the host starts, verify that the expected six players are present in the same session and all accepted the match. Give assignments a short expiry and return a party to the queue if the other party cannot join. The initial prototype can use a fixed six-player requirement; backfill and variable player counts can be deferred.

The matchmaker should not issue “start expedition” to every client. One designated session host should start it after the shared lobby is complete, or the game should retain its existing host-start behavior.

### 5. Roster and gameplay team assignment

The match result should define teams according to the selected format: six one-player teams in Solo, three two-player teams in Duo, or two three-player teams in Squad. It should identify each player with an identity that remains meaningful across the transition from lobby to expedition. At expedition load, each mod client should:

1. resolve the actual in-expedition player roster;
2. associate those players with the match roster;
3. apply the same team mapping locally;
4. report whether mapping succeeded, and avoid silently applying a partial/ambiguous mapping.

The existing notes say lobby IDs can change at expedition start. The expedition player IDs were consistent across clients in the observed experiments, but the association between platform identity and in-game expedition ID still needs a reliable method. Do not rely on lobby order or lobby IDs until tested across hosts and clients.

The team/hostility values required for two allied squads also remain to be established. This is independent of matchmaking: a queue can successfully produce six players while the gameplay team model is still wrong. Keep those as separate milestones.

## Suggested minimum protocol

A prototype can use HTTPS requests plus polling or a WebSocket for updates. Keep the protocol versioned from the first test.

Possible operations:

- `POST /queue`: player submits a Solo ticket or a party leader submits a Duo/Squad ticket; returns a queue ticket ID.
- `GET /queue/{ticket_id}` or WebSocket events: queued, match found, ready check, session details, cancelled/expired.
- `POST /match/{match_id}/ready`: each player or party reports ready, with the match assignment tracking all expected members.
- `POST /queue/{ticket_id}/cancel`: leader cancels.
- `POST /match/{match_id}/session-status`: report that the assigned session has the expected roster, if the service needs to coordinate launch.

Backend correctness basics: tickets need expiry/heartbeats, cancellation must be idempotent, and a player/party must never be assigned to two matches at once. Do not trust a client-provided player count as proof that all members are present; verify the roster in the eventual shared game session before launch.

## Feasibility spike: do this first

Before implementing matchmaking UI or a hosted service, test the actual session-merge path:

1. Start two independent three-player Seamless Co-op parties for the Squad case. Also check whether six independent players and three two-player parties can form a session for Solo and Duo.
2. Select one session as the proposed destination/host.
3. Try to bring the other party/players into that same Seamless session using the normal supported join/invite flow.
4. Confirm all six are visible to every client in the pre-expedition lobby and in the expedition, regardless of whether the match is six solos, three duos, or two squads.
5. Confirm the expedition can start and remain stable with six players.
6. Record whether joining requires the second party to disband/re-form, whether the host can invite players, and what happens if a player disconnects.

If this passes, implement a local matchmaking broker that returns the destination session, team grouping, and a ready check. If it fails, stop and investigate session integration alternatives; a queue server alone cannot make the user flow work.

## Implementation phases

### Phase A: manual session proof

Prove that players from separate starting groups can become one six-player Seamless Co-op session. Start with two three-player parties for Squad, then verify three two-player parties for Duo and six individuals for Solo. Capture logs and exact manual steps. No matchmaking service needed yet.

### Phase B: local broker proof

Create a small service and a simple client/companion that can enqueue tickets, assemble six-player matches for each format, show ready state, and return a destination session plus team groupings. Keep the join itself manual if necessary. This validates queue behavior without entangling game-memory code.

### Phase C: client integration

Add an appropriate player-facing queue UI and session join action. A companion launcher is a practical first UI if the existing overlay is intended to remain a debug panel. If queue controls go in the overlay, keep all game-memory interaction on the existing game loop thread; network I/O should run on a separate worker and pass plain messages to the game loop/UI.

### Phase D: in-game roster/team bridge

Map the match roster to expedition players, apply team assignments consistently on all clients, and handle missing/ambiguous identities. Validate the gameplay team model separately from queue matching.

### Phase E: remote service and resilience

Host the broker, add reconnects, queue timeouts, region/version policy, basic abuse controls, service metrics, and user-visible failure reasons. Cheating prevention can remain out of scope for the first concept prototype, as requested, while still validating message shapes and capacity.

## Main concerns and risks

1. **Session merging is the critical unknown.** Seamless Co-op's existence and six-player capacity do not prove that two already-formed lobbies can be joined into one.
2. **Unclear Seamless Co-op API surface.** Repo notes say closed source and no public extension API. Directly invoking Steam interfaces may conflict with the game's or Seamless Co-op's session lifecycle; it is an experiment, not the default assumption. A Steam lobby directory may be simpler than an external broker if our code can safely discover/join the lobby SC creates, but that is itself unproven.
3. **Team assignment identity bridge.** Lobby IDs change at expedition load. Stable player identity must be linked to in-match players without assuming ordering.
4. **Gameplay teams still need a valid ally/enemy scheme.** Matchmaking does not solve hostility, friendly fire, revive rules, or team-colored UI.
5. **Six-player stability.** Six is the documented cap, not proof that every six-player PvP match is stable. The repo also records reports of instability at five to six players; validate this early across all formats.
6. **Native DLL reliability.** Do not block the game loop on network requests. Run networking asynchronously and make it safe for the service to be unavailable.
7. **Party departure and host loss.** Define whether a match is cancelled, remade, or allowed to start short when a player leaves during queue/join/ready phases. For the first test, cancellation/rematch is simplest.
8. **Scope of “offline.”** This proposal avoids Bandai matchmaking and EAC but, as currently understood, still depends on Steam networking through Seamless Co-op. If the goal is to avoid Steam too, that is a separate transport project.
9. **Distribution and dependency constraints.** Existing project notes say not to bundle, modify, or hook inside `nrsc.dll`, and to seek the author's permission before public release. Keep the design outside Seamless Co-op binaries unless this constraint is revisited.

## Questions for review

1. Is a central matchmaking broker the right first design, or should the first implementation use Steam lobby discovery/tags and invites? Is an external service only a fallback if we cannot safely use the SC-created lobby?
2. What is the simplest supported way to join a second Seamless Co-op party into the destination party's session?
3. Would a companion launcher be a better place for queue/session UI than adding networking to the injected DLL?
4. What identity can reliably correlate a queued player with that player's in-expedition `PlayerIns` across every client?
5. Should Duo and Squad accept only complete pre-formed teams initially, or should solo-queued players be allowed to fill open team slots? What should happen if a party is smaller than the selected team size?
6. How should a match behave if one player leaves after a match is found but before the expedition starts?
7. Is six the right first target for each mode, given the documented cap and stability concerns, or should initial tests use fewer players (for example, two duos) before targeting six? **Decided 2026-10-19 (Leonardo): six in every mode (6 solos, 3 duos, 2 squads of 3); smaller test matches only through the pre-release flag (`MATCHMAKING.md` §6).**
8. Is there a viable alternative that avoids relying on Steam/Seamless Co-op session formation without rebuilding the full game networking layer?

## Recommendation

Treat this as two linked but separate projects:

1. **Session formation:** prove that independently formed groups can share one Seamless Co-op expedition.
2. **Matchmaking:** select Solo players, Duo teams, or Squad teams and coordinate readiness once session formation is known to work.

Start with the first. It is the largest technical unknown and determines whether the rest of the proposed flow is practical.
