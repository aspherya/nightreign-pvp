# Project Plan: Nightreign Battle Royale PvP Mod

Product intent and scope boundaries: [re/PROJECT_SCOPE.md](re/PROJECT_SCOPE.md). This document owns the feature-by-feature feasibility,
the milestones, and the parked ideas (§12). It is subordinate to the scope and to STATUS.md / TASKS.md. Treat proposed mechanics and
feasibility routes here as planning until supported by current evidence and an active task. §1, §6 and §9 are the 2026-10-10
snapshot; what works today is in STATUS.md §1.

Written 2026-10-10, after Phase 0. Tags: `[PROVEN]` tested by us · `[LEAD]` evidence it may work, untested · `[UNKNOWN]` needs a spike.

---

## 1. Where we start from

| Capability | State |
|---|---|
| Two players in one expedition, launched with our mod | `[PROVEN]` me3 path, friend kit, Nucleus |
| Players damage each other, with PvE alongside | `[PROVEN]` team type 29 on all players, set on every client. Two PCs over real Steam |
| No networking changes, no changes to Seamless Co-op | `[PROVEN]` each client decides hostility from its own copies |
| Identify "me" and every other player the same way on all clients | `[PROVEN for 2 players]` local = first slot; per-player id at `ChrIns+0x78` |
| Automatic rule without debug keys (`pvpve = true`) | Built (T-020), not yet run in-game |
| PvP death | `[PROVEN]` downed → no revive → respawn near a grace, runes and one level lost |
| Test bench | Nucleus Co-op runs 2 instances on one PC; the handler allows up to 6 `[LEAD]` |

## 2. The scope, feature by feature

Each row says what the game or our tests already give us, and what has to be found out or built.

| # | Scope v3 feature | What we have | What's missing | Risk |
|---|---|---|---|---|
| F1 | Public solo/duo/trio queues, 6 players per match in every mode | SC supports 6 players, reported shaky at 5–6. Public queueing, random matching, and party handling are not implemented | Matchmaking route and reliable six-player sessions; do not assume the ceiling can be raised | **Very high.** See §3 and S4/S7 |
| F2 | Teams: allies can't hurt each other, rivals can | 29 is free-for-all | Team values or a hostility hook `[UNKNOWN]`; all clients agreeing on teams `[LEAD: derive from the per-player id]` | High |
| F3 | **Hawk crossing** (2026-10-21, replaces "teams spawn at different positions"): the start hawk crosses the whole map, each player jumps off where they want | `[PROVEN]` moving the local player (world position kept, seen on both screens); the vanilla ride is a straight ~17 m/s line (T-028 run 4) | How the ride works (hawk object, what ends it), holding a player in the air, a safe fall from any point, a drop button. §13 | Medium-high |
| F4 | PvP active immediately, PvE continues | `[PROVEN]` | Automatic rule verified in-game; sites of grace broken on 29 | Low |
| F5 | Night 1: tide shrinks, **no boss** | The tide exists in vanilla | How to skip the night boss and still advance to Day 2 `[UNKNOWN]` (event scripts and flags) | High |
| F6 | Day 2: map opens again, stronger enemies | Vanilla does this | Probably nothing | Low |
| F7 | Night 2: all teams in one arena that shrinks, no boss | The tide and the circle exist | Remove the boss, control the circle's final size, keep everyone inside `[UNKNOWN]` | High |
| F8 | Death drops the entire inventory as loot anyone can take | Nightreign lets players drop items for teammates `[LEAD]`; runes drop on death in vanilla | Read the inventory, drop every item on death, make the drops visible and lootable by rivals `[UNKNOWN]` | **Very high** |
| F9 | Respawn with starting items, level kept, away from combat, brief protection | Vanilla respawns near a grace and removes a level | Stop the level loss, choose the respawn point, protection that ends on attack `[UNKNOWN]` | High |
| F10 | Downed players can be revived by teammates only | Revive works in vanilla co-op; between hostile players it does not `[PROVEN]` | Should come for free once teammates are non-hostile `[LEAD]`; verify | Medium |
| F11 | Sudden death: no respawns once the arena is fully closed; stall-breaker damage | The tide deals damage outside the circle | Detect "fully closed", block respawns, mark eliminated players, escalate damage `[UNKNOWN]` | High |
| F12 | Win detection: last team with living players | Player state is readable (`ChrSet` entries) | A match state machine all clients agree on | Medium |
| F13 | Winner's choice: escape with loot or fight the Nightlord | The Nightlord fight exists | A choice prompt, ending the expedition as a win without the boss, rewards `[UNKNOWN]` | High |
| F14 | Balancing rules (no kill rewards, default gear, loot stays on the ground, final-arena supply) | Mostly follow from F8–F9 | Tuning, after the mechanics exist | Low |
| F15 | Mode selection in Roundtable Hold | SC already edits that menu | Our own menu entry or a panel toggle; all clients agreeing on the mode | Medium |
| F16 | Install and launch for normal players | Friend kit with a script | A launcher; permission from SC's author before any public release | Medium |

## 3. Public queues and initial player cap

The product target is public solo/duo/trio matchmaking. Every match has **6 players** (decided by Leonardo 2026-10-19): six solo players, three duos, or two trios (squads). This fits Seamless Co-op's cap, so no work goes into raising it. Private SC sessions and Nucleus are the current development test path, not the intended public product.

SC and the engine currently constrain the session to six players. Do not spend effort on more than six without new evidence and an explicit scope decision. Public matchmaking is not implemented, and access to SC's lobby, queue discovery, party membership, and match start remains unresolved. Investigate these when the roadmap activates matchmaking; do not assume SC exposes a public API or that its lobby can be reused without permission.

The detailed matchmaking/session-formation proposal and the Solo, Duo, and Squad queue formats are in [S8: Public matchmaking and party/session formation](research/S8-public-matchmaking.md). It is exploratory; the first exit condition is still proving how independently formed players can enter one shared SC session.

## 4. Two cross-cutting pieces

**A. A way for our mod to talk to itself across players (spike S4).** Hostility needs none: each client applies it locally. Team assignment can
be derived from the per-player id, also with no messages. But match state (arena closed, who is eliminated, who won, the winner's choice) must be
identical everywhere. Preferred order: (1) derive it from state the game already syncs (the tide's phase, player deaths); (2) carry it in something
the game already syncs for us (event flags, if SC syncs them `[UNKNOWN]`); (3) our own small messages over Steam networking between lobby members.

**B. The match rules as a pure state machine.** Phases, team rosters, alive/eliminated, win check, respawn permission. Written in Rust with no game
access, so it is fully unit-tested without launching anything. Game code feeds it events and applies its decisions.

## 5. Milestones

Each milestone starts with a spike (find out, cheaply) and ends with a test on real clients. Order is by dependency and by how much each one
de-risks the rest.

| Milestone | Goal | Covers | Exit test |
|---|---|---|---|
| **M1 Stable free-for-all** | The automatic rule works for a whole expedition with 2 players, no keys; graces usable | F4 | Nucleus 2 instances + friend: full Day 1 → Night 1 with `pvpve = true`, no freeze, graces work |
| **M2 Teams** | Allies are safe from each other and can revive; rivals are hostile; every client agrees on teams | F2, F10 | Nucleus 3–4 instances: 2v1 or 2v2, ally hits do nothing, ally revive works, rival hits land |
| **M3 Death and respawn rules** | Level kept, starting items, respawn point chosen by us, protection window | F9 | Kill → respawn with the right state on both screens, repeated 10 times |
| **M4 Inventory drop** | A PvP death leaves the victim's items in the world; anyone can pick them up | F8, F14 | A kills B, A picks up B's weapon, both screens agree |
| **M5 Match flow** | Two days and two nights, no Night 1 boss, Night 2 arena without a boss | F5, F6, F7 | A full match timeline runs start to finish with no boss fights |
| **M6 Finale** | Sudden death, eliminations, win detection, winner's choice | F11, F12, F13 | 2 teams: the arena closes, respawns stop, last team wins, both endings reachable |
| **M7 Scale and spawns** | Hawk crossing (§13); validate intended formats up to 6 players | F1, F3 | The six-player configuration completes a match |
| **M8 Product and public matchmaking** | Queue selection, random matching, party/team formation, release permissions, player install flow | F15, F16 | A player installs, queues solo/duo/trio, and completes a match with random players |

M1 and M2 are next. M3–M6 can only be ordered after their spikes, because each could turn out trivial (the game already does it) or very hard.

## 6. Spikes (research before building)

Read-only research tasks for Sonnet researchers, each producing a short sourced memo. They can run in parallel with M1.

| Spike | Question | Feeds |
|---|---|---|
| S1 Player cap | What sets the 6-player limit (SC, engine slots, both)? Any public statement or path beyond 6? | §3, M7 |
| S2 Inventory and drops | How does Nightreign's own "drop item for a teammate" work? Where is the inventory stored (ER layout as a hint)? Are dropped items synced to every player? | M4 |
| S3 Day/night and bosses | Which event scripts and flags drive the tide, the night boss, and the day change? Can a night end without its boss? | M5, M6 |
| S4 Mod-to-mod channel | Does SC sync event flags? Can our DLL send its own Steam networking messages to lobby members without clashing with SC? | §4A, M6 |
| S5 Death and respawn | What does the engine do between downed, death, and respawn? Where do level loss and the respawn point come from? | M3 |

## 7. How we work

Unchanged: Opus plans and reviews, Sonnet executes one card at a time, humans run the game. New since Phase 0:
- **Nucleus is the daily two-client bench.** Try 3–4 instances for M2 (the handler allows 6; the PC's limits are untested).
- **Friend sessions** confirm each milestone over a real connection, using the friend kit.
- **Every milestone ships behind a config switch**, so a broken feature can be turned off without rebuilding.

## 8. Biggest risks

1. **Public matchmaking may not be accessible through SC's lobby.** Mitigation: investigate a compatible lobby-discovery and mod-to-mod Steam channel before committing to queue architecture; do not modify SC.
2. **Inventory drop (F8)** is the scope's central mechanic and the least understood. Mitigation: spike S2 before promising it; fallback designs
   (drop runes and consumables only; or the killer receives a reward directly).
3. **Controlling the expedition's script (F5, F7, F13)** may need modified event files, which SC may check or overwrite. Mitigation: spike S3 and S4.
4. **Agreement between clients** on match state. Mitigation: derive from synced game state first.
5. **Game or SC updates** break offsets. Mitigation: discovery by names and patterns, version pins, a startup check.
6. **Permission and public operation:** SC's terms and the author's stance may constrain a public PvP product; public play also introduces disconnect and abuse requirements. Resolve these before public launch; private development tests are unaffected.

---

## 9. After the spikes (2026-10-10): what changed

Memos: `research/S1-player-cap.md` … `S5-death-respawn.md`. Everything below is from sources or Elden Ring hints unless marked `[PROVEN]`.

| Feature | Before | Now |
|---|---|---|
| F1 players | Very high risk | Decided: build for 6. The cap is SC's own soft choice; slots 4–6 already needed fixes |
| F2 teams | Unknown values | **New idea, cheap to test:** each client leaves itself and its allies on the vanilla co-op value (2) and sets only *rivals* to 29. Allies then behave exactly as in vanilla (revive, graces, pickups), and no "team value pairs" are needed. Works per client because hostility is per client `[PROVEN]` |
| F5/F7 no bosses, arena | Unknown | The timeline is a chain of event flags; the circle comes from params. Other mods already ship changed event and param files next to SC. Route: an event-script edit that skips the boss and sets the "night done" flag |
| F8 inventory drop | Very high risk | The game has a native discard-to-ground with shared pickups; ~11 items per player. Route: call the native discard per item on death. Needs the inventory and the function located |
| F9 respawn rules | Unknown | Level loss isn't a setting: save and restore it from the DLL. A native invulnerability timer exists for protection. Respawn point: move the player after respawn |
| F11 sudden death | Unknown | Vanilla boss fights already disable respawns: find that switch |
| F12/F13 win and choice | Unknown | Flag 7000 = expedition won; an event can replay the ending. Mod-to-mod messages over Steam for agreement between clients |
| New risks | | (1) Vanilla ends the run when **no player is alive at night** (flag 7001): must be neutralised. (2) Interactions (graces, maybe pickups) fail for a player on team 29: the "rivals only" scheme avoids it |

## 10. Stable v1 (the first build worth a real play session)

1. **Teams, automatic:** rosters from the per-player id (`team_size` in the config; 1 = free-for-all); each client keeps its own team on the
   vanilla value and marks rivals hostile. No keys.
2. **Vanilla behaviour inside a team:** revive, graces, pickups all work.
3. **PvP death = vanilla death** for now (downed, no rival revive, respawn, runes and a level lost).
4. **The run can't end by accident** when everyone is down at night (flag 7001 handled), or the limitation is documented if it can't be handled yet.
5. Works in Nucleus (2–3 instances) and over real Steam with the friend kit.

After v1, in order: M3 death rules (keep level, protection) → M4 inventory drop → M5 no-boss nights → M6 finale → M7 spawns → M8 product.
The data needed by M3 and M4 is the same (where level, runes, and inventory live), so one discovery task feeds both.

## 11. Downed players: design decision (Leonardo's idea, adopted 2026-10-11)

A downed player keeps the vanilla revive meter and anyone can hit them. **Who lands the hit that completes the meter decides:** a teammate →
the player revives (vanilla); a rival → the player **dies** (finishing blow). Both sides can be filling the same meter at once; the completing
hit wins, so there is no tie rule. This replaces the earlier goal "rival hits must not count on a downed player".
Needs, all `[UNKNOWN]` on NR: (1) a downed marker (T-032 dumps); (2) the attacker of the completing hit, decided on the victim's own client
(`[LEAD]` ER's ChrIns keeps a last-hit-by handle); (3) a way to turn that revive into a death (`[LEAD]` EMEVD `Force Character Death`
2004[4]; setting HP to 0 may only re-down the player). Fallback if (3) is impossible: rival hits do not count and the timer decides.

**§11 update (2026-10-11, after FS-0 run 9 and Leonardo's refinement).** Target design: a downed player bleeds out on the vanilla clock;
**every hit by a rival shortens that clock** (hits by teammates fill the revive meter as in vanilla), so rivals finish a downed player faster and
the body is a race between the clock and the meter. Needs the bleed-out clock located and writable `[UNKNOWN]` (T-035 fast scan).
Interim rule v1, shipping now: when a rival's hit completes the revive meter, the victim's client sets HP to 0 again (verified: re-downed at
once), so a rival can never revive; the player dies when the bleed-out (≈15 s, restarting on each down) runs out.

**§11 status (2026-10-12, updated after FS-0 run 14).** The clock and the bar are located (PlayerIns+0x718 and +0x714, with the pause timer at +0x71C) and the clock is writable. Leonardo's target rule is buildable on the victim's own client: on a hit whose attacker is a RIVAL, restore the bar to its value before the hit, subtract a chunk from the clock, and clear the pause; hits by teammates are left alone.

**§11 update 2 (2026-10-12, Leonardo): free-for-all has no downed state.** With `team_size = 1`, a player whose HP reaches 0 must die and
respawn at once: no bleed-out, no revive. Route `[LEAD]`: set the game's own near-death duration to 0 in the in-memory param table
(`GAME_SYSTEM_COMMON_PARAM_ST`: `near_death_grace_period_timer_default`, `boss_arena_near_death_duration`; validate the row by
`required_revive_progress_level1/2/3` = 40/90/240) on every client. Team modes (team_size ≥ 2) keep the vanilla bleed-out; the "rival hits
shorten the clock" idea applies there and is deferred to team play. Rule v1 (re-down on a rival revive) stays as the team-mode fallback.

## 12. Parked ideas (merged from FUTURE_CONSIDERATIONS.md on 2026-10-20)

Nothing here is built without a task card. Items already done or decided (menu entry, queues, team values, friendly fire, grace
interaction, the downed rule) were dropped; their history is in git.

- **Downed gauge flicker on a rival hit** (2026-10-12, Leonardo: fine for now). The game applies the hit to the gauge, then the mod puts it
  back on its next 50 ms pass, on both screens. Ideas, none tested: (a) run only the downed check on a faster pass (5–10 ms); (b) hook the
  function that applies revive damage and skip it for rivals. (b) would also close the "one hit removes the whole bar" gap and the
  teams-of-2+ attacker-screen gap (STATUS.md §4).
- **Separate team spawns** (F3 before 2026-10-21, replaced by the hawk crossing, §13). Built on `t-028-snapback`, never run: at the
  vanilla drop each game moves its own player `spawn_radius` m from the drop point, one direction per team (`team_spawns = true`).
  Kept only as a fallback if the crossing fails.
- **Robustness:** disconnect and reconnect mid-match; host migration (does SC support it?); desync detection; performance with 6 players
  and a full enemy load.
- **Compatibility:** a version guard that refuses to run on an unknown game build and logs why; coexistence with other me3 mods (FPS
  unlock, FOV, QoL).
- **Presentation:** a team colour or marker on rivals (invader phantom tint?); PvP damage scaling through
  `vs_player_dmg_correct_rate_*`; rewards and rankings.
- **Fairness:** sessions are friends-only today, so anti-cheat is low priority. Public queues will need basic sanity checks and abuse
  handling (see §8 risk 6).

## 13. Hawk crossing (Leonardo's idea, adopted 2026-10-21; replaces separate team spawns)

**Design.** At the start of an expedition every player is on the hawk, as in vanilla. Instead of a short ride that drops everyone at one
landing zone, the hawk keeps flying on the **same straight line** (vanilla start point, vanilla direction) until it reaches the far side of
the map. Each player chooses when to jump off and lands where they want. Players who never jump are dropped automatically at the end
of the line. Teammates are not forced together (they coordinate by voice, as in Fortnite). Whether everyone stays on one line or each
team gets its own line can be decided later. The design puts one line through the whole map.

**What we know (T-028 runs 3-4, 2026-10-14, solo; `re/MEMORY_MAP.md` §8).**
- Moving the local player works: write the physics-module position, keeping world position (physics − origin). Other players see the
  move (Nucleus, 2026-10-14). `[VERIFIED]`
- The vanilla ride (run 4): the player starts about 160 m up and moves on a **straight line** at 16–18 m/s for about 12 s (about 210 m),
  sinking slowly (y 162 → 108). While riding, the game marks the player as standing (ground byte 1). At release the ground byte goes
  to 0. The player keeps the same horizontal speed and falls about 84 m in 3 s, then lands with **no damage**. `[VERIFIED: run 4 log]`
- Each vanilla landing row (`SORTIE_STARTING_POINT_ST`, 20 rows) names a **hawk route** (`birdRouteEntityId`, a map entity). The game
  has a `CSBirdMovePointMan` that appears at expedition load. `[SOURCE: S6]` The map is 6 × 6 tiles (`m60_41..46_35..40`). If NR
  tiles are 256 m like Elden Ring's, that is about 1.5 km across `[HYPOTHESIS]`, so the full crossing takes about 90 s at the vanilla speed.

**Leonardo's answers (2026-10-21, from playing vanilla).** (1) The start hawk cannot be left early. (2) Each player has their own
hawk. (3) No fall damage, except falling into the void. (4) The vanilla button: to be traced with nrcon/nrscan in the next session.
(5) No glide: after the drop the player falls straight to the spot. Leonardo's point: only the start hawk is locked (fixed direction,
fixed drop). The hawks from **Spectral Hawk Trees** let the player steer for a while.
- Consequences: we must provide the drop ourselves (1); each game moves only its own player, which matches how we already work (2); a
  release from any height over solid ground is safe, so route 1's biggest risk goes away, but the line must stay over the map (3; the
  void at the edges kills) `[HYPOTHESIS until HAWK-1 c]`.

**Spectral Hawk Trees** (wiki extract `research/hawk/spectral-hawk-tree.md`): a tree summons a hawk that flies a **fixed route shown on the
map** ("spectral road"). The player can steer off the route, which makes the hawk descend; with no input it flies the whole route. The
trees are at fixed places and the start hawk also has a route (`birdRouteEntityId`), so both very likely use the same route system
(`CSBirdMovePointMan`, flag 8800 "Bird movement icon") `[HYPOTHESIS]`. A Nexus mod, **Spectral hawk anywhere** (Dhafer, 2026-10-03;
`research/hawk/spectral-hawk-anywhere-mod.md`), summons a tree hawk anywhere, at any time, so summoning a hawk without a tree is
possible. The page does not say how it works. Its permissions forbid modifying or re-uploading the file, so we may only study it and
write our own version (or ask the author).

**What the Nexus mod showed (2026-10-21; facts in `TECHNICAL_NOTES.md` §4b).** The hawk is not a vehicle. Riding it is a state of the
player character, run by the player action script `c0000.hks`. The engine tells the script which ride starts (1 = tree hawk, 4 = start
hawk) and when the route ends (2). The **start hawk loop reads no input at all**, which is why it can't be left early. The **tree hawk
loop can be left by holding the action button for 0.5 s or by pressing L3**, and the player can steer it. The Nexus mod adds one line
that starts a tree-hawk ride whenever the player has SpEffect 707144, and the game accepts the script as plain text from a mod package.

**The design splits into two jobs.**

*Job A: the drop button on the start hawk.*
- A1 (data): ship our own `c0000.hks` (text) that adds the tree-hawk exits (hold action 0.5 s, L3) to `Departure_MoveLoop_onUpdate`.
  Same buttons as vanilla tree hawks. A community decompile exists for 1.03.1 and 1.03.4 and its hawk code is identical in both
  (TECHNICAL_NOTES §4b), but whether either is an exact match for our exe is unproven. A wrong file can break any other move. Risks:
  the whole 1 MB script is replaced (it conflicts with any mod that ships one) and every patch needs a re-check. Licence of the decompile
  repo: not checked.
- A2 (DLL, preferred): when the player presses the drop button, make the engine answer "route end" (2) to `GetBirdEventID` (env 377)
  for this player. The vanilla script then starts the vanilla separation fall. Nothing is shipped and the script stays vanilla.
  Needs: the exe code that answers env 377, and the field it reads (static reading with capstone, then a check in the game).

*Job B: a route across the whole map.* **Found 2026-10-21 (TECHNICAL_NOTES §4c):** every start route is 3 points (start, waypoint,
end) in `m60_00_00_99.msb`, about 200 m long. Limveld is about 1.5 km square in that file's frame.
- B1 (data, preferred): ship a changed `m60_00_00_99.msb.dcx` in our game-data package. Keep each start route's start point and move
  its waypoint and end point along the same direction to 100 m before the map edge. That is an in-place float edit with the same file
  size, so no MSB writer is needed. Extended lengths: 600–1,530 m, 36–90 s at the vanilla speed (`experiments/HAWK-0/`). The engine's
  own end-of-route signal then drops everyone left on the hawk at the far side, for free. Variant: also move the start point back to
  the near edge, so every ride is a full edge-to-edge crossing. Open: does the hawk keep its speed and height over a long leg (the
  vanilla routes sink about 80 m over 200 m; we can keep the end point high)? Does it clip through hills (keep points high)? Does a far
  end point load correctly?
- B2 (DLL): keep the vanilla route and move the player ourselves along the extended line. Fallback only.

Likely v1: **B1 + A2**. A cheap first test needs no DLL code: a game-data package with one patched MSB, a start ride, and a look at
whether the hawk flies the long route and drops at its end (HAWK-1b).

Earlier ideas, dropped: summoning a tree hawk at the start (the start ride is already the right state; the Nexus mod's trick only
starts a tree-hawk ride); flying a separate hawk object (the hawk is not an object, it is the player's own state).

Plan: HAWK-0 done (2026-10-21). Next: HAWK-1b (route patch test, data only) and HAWK-1 (ride probe, drop-button trace). Then build
HAWK-2. Cards in `TASKS.md`.

**What happens to the earlier work.** The T-028 teleport (`game/poswrite.rs`, layout §8 offsets) and the hawk-ride detector
(`game/spawn.rs::DropDetector`) are the B2 fallback and the ride logger of HAWK-1. The F3 "move each team 200 m apart at the drop" rule (`team_spawns`, off by
default) stays in the code as a parked fallback (§12) until the crossing works, then it is deleted.
