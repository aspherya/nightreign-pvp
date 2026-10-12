# Project Plan: Nightreign Battle Royale PvP Mod

Product intent and scope boundaries: [re/PROJECT_SCOPE.md](re/PROJECT_SCOPE.md). This document preserves the detailed feature breakdown and feasibility planning; it is subordinate to project scope and the current ROADMAP/TASKS. Treat proposed mechanics and feasibility routes here as planning until supported by current evidence and an active task.

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
| F1 | Public solo/duo/trio queues, initially up to 6 players | SC supports 6 players, reported shaky at 5–6. Public queueing, random matching, and party handling are not implemented | Matchmaking route and reliable six-player sessions; do not assume the ceiling can be raised | **Very high.** See §3 and S4/S7 |
| F2 | Teams: allies can't hurt each other, rivals can | 29 is free-for-all | Team values or a hostility hook `[UNKNOWN]`; all clients agreeing on teams `[LEAD: derive from the per-player id]` | High |
| F3 | Teams spawn at different positions | Nothing | How spawn points are chosen `[UNKNOWN]` | Medium |
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

The product target is public solo/duo/trio matchmaking. The initial capacity target is **6 players**: up to six solo players, three duos, or two trios. Private SC sessions and Nucleus are the current development test path, not the intended public product.

SC and the engine currently constrain the session to six players. Do not spend effort on more than six without new evidence and an explicit scope decision. Public matchmaking is not implemented, and access to SC's lobby, queue discovery, party membership, and match start remains unresolved. Investigate these when the roadmap activates matchmaking; do not assume SC exposes a public API or that its lobby can be reused without permission.

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
| **M7 Scale and spawns** | Separate team spawns; validate intended formats up to 6 players | F1, F3 | The six-player configuration completes a match |
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
