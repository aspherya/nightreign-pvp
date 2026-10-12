# Future Considerations

Planning notes for work whose implementation is deferred. Public matchmaking is a core product goal in
[re/PROJECT_SCOPE.md](re/PROJECT_SCOPE.md); this file records its open feasibility and product
questions. Nothing here gets implemented without a task card in TASKS.md and an active phase in ROADMAP.md.

## Final-day team brawl (Phase 5)
- Flow: surviving teams enter the final day → team-vs-team fight → the winning team chooses **Challenge
  the Nightlord** or **Escape**.
- Feasibility questions: how to move every team into one arena (reuse the existing Nightlord boss
  arena or a day-3 location?); how to suppress or defer the boss spawn (EMEVD flags?); elimination
  rules (downed vs dead, spectating); how to present the choice (reuse an existing menu or interaction
  prompt rather than new UI); how to end the expedition consistently on all clients (victory vs
  escape vs defeat flags); rewards.
- Design intent: escaping is the safe, smaller reward. Fighting the boss is the full-clear risk. Reuse NR
  mechanics; don't build a battle royale system.

## Teams and scale
- Team assignment for more than 2 players: deterministic by Steam ID or lobby order, a host-picked config
  file, or an in-game choice. Needs consistency across clients (SC has no public API).
- SC lobby cap is 6. Reports say 5–6 can be unstable. Realistic targets: 2v2, then 3v3. Anything beyond 6
  needs SC changes we can't make.
- Expedition difficulty scaling with more players (SC has health, damage, and posture scaling settings).

## Friendly fire
- Policy decided after TC-002 shows vanilla SC behaviour. If needed later, choose between team types
  per team and a filter. Avoid damage interception unless a validated requirement needs it.

## Robustness
- Disconnect and reconnect mid-PvP; host migration (does SC support it?).
- Desync detection and logging tooling.
- Performance with 6 players plus a full enemy load.

## Fairness and anti-exploit
- Friends-only sessions make anti-cheat low priority. Basic sanity checks may come later.

## Compatibility and maintenance
- Survive game and SC updates: signature-based discovery, a version guard that refuses to run on unknown
  builds and logs why.
- Coexistence with other me3 mods (FPS unlock, FOV, QoL).

## Presentation
- In-game overlay (hudhook) for team display and a debug panel.
- Team colour or marker on opposing players (reuse invader phantom tinting if the engine still has it?).
- Rewards, rankings, balancing (PvP damage scaling via `vs_player_dmg_correct_rate_*` params).

## Release and permissions
- Before any public release, contact Yui (SC author) about a PvP mod that requires SC. SC terms forbid bundling or modifying it, and the author worries about takedowns and vanilla-matchmaking exposure. Users would install SC themselves from Nexus.

## Known side effects to fix later
- **Downed gauge flicker on a rival hit (2026-10-12, Leonardo: fine for now, improve later).** The game applies the hit to the gauge, then
  the mod puts it back on its next 50 ms pass, on the downed player's game and on the attacker's. Ideas, none tested: (a) run only the
  downed check on a faster pass (5–10 ms); (b) stop the hit from reaching the gauge at all by hooking the function that applies revive
  damage and skipping it for rivals (needs that function found first; it would also close the "one hit removes the whole bar" gap and
  the teams-of-2+ attacker-screen gap, because nothing would need undoing).
- With team_type 29 the player cannot interact with sites of grace (no level-up menu), though passing through still applies the effect. Probably an interaction check on team type. Phase 3 candidate fixes: temporarily restore the team value while near or at a grace, or find the interaction check. (TC-004 run 4, 2026-10-09)

## PvPvE as a selectable expedition mode (idea from Leonardo, 2026-10-10)
- In Roundtable Hold, next to "Solo Mission" / "Up to 6 players", offer a separate **PvPvE** mode. Picking it makes the mod apply the PvPvE rules
  (team types and whatever else is needed) when the expedition starts. Normal co-op stays untouched otherwise.
- To verify later: (a) can we add an entry to the expedition-type menu that SC already modifies (its "Up to 6 players" entry is SC's own), or is a
  toggle in our own overlay at the Roundtable the cheaper first version; (b) how all clients agree on the mode: each client applies team types locally
  (FS-0 run 2), so either everyone selects it or the host's choice is propagated, and SC offers no channel for our data;
  (c) what "the rules" include beyond team types (grace interaction fix, revive rules, team assignment).
- Phase 4 candidate. Not needed for the Phase 0-2 proofs.

## PvP death rules (observed in FS-0 run 2, 2026-10-10; wanted by Leonardo)
- Observed: a player killed by another player goes **downed**; the revive bar appears but **no revive was possible**; the player then **respawns
  near a site of grace**. Probably loses runes and a level, like a normal death (to re-check). The killer gets nothing.
- **Wanted: the killer can loot the player they killed.** To investigate: what the engine drops on a player death (runes at the death spot?),
  whether another player can pick that up, and whether a reward can be granted to the killer directly.
- Also to decide: whether revives between hostile players should be possible at all, and how they work for teammates once teams exist.

## Public matchmaking queues (product requirement; implementation deferred to M8)
- Queues = team size: solo (up to 6 solo players, all rivals), duo (up to 3 teams of 2), trio (up to 2 teams of 3). The mod already takes `team_size`, but queueing and party formation are not implemented.
- Finding players: Steam lobby list filtered by our own lobby tags (mod, queue, mod version); join an open one or create and wait; start when
  full or on a timer. Parties queue together and must stay on one team, so roster building needs party info over the mod channel (S4), not only join order.
- To verify first: can our DLL tag and search the lobby SC creates and join it cleanly (lobby data is owner-only; SC owns the lobby; we may not
  modify SC). Fallback: an external service or Discord bot that distributes invites.
- Blockers: viable lobby discovery / queue transport, reliable team agreement, SC compatibility and permission for a public product; public play also requires basic handling for disconnects and abuse.
