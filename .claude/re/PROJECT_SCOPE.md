# Elden Ring Nightreign — PvP Mod

**This is the product spec (owner: Leonardo).** Requirements are his; the "Game today" marks and the last section are kept in sync with the
code by the agent (last sync 2026-11-01, `main` at the audit PR). Where a requirement and the game differ, the difference is listed in
§ "Scope vs game today" and nothing is silently changed.

## Core Concept
Add two PvPvE modes to Elden Ring Nightreign: Battle Royale and Arena. Both support Solo, Duo, and Trio queues. Players can join public matchmaking or create password-protected private matches with invited Steam friends.

## Engineering Foundation
Achieving this scope depends heavily on reverse engineering Elden Ring Nightreign and Seamless Co-op. The project must discover and verify how the game handles player state, combat, death and revival, inventory, Sites of Grace, expedition progression, multiplayer sessions, and synchronization.
Base implementation decisions on verified Nightreign and Seamless Co-op behavior. Do not assume an interface, hook, or capability exists without evidence.

## Game Modes
Brief descriptions of each mode’s rules and basic structure.

| ID | Mode | Description |
|---|---|---|
| `MODE-01` | **Battle Royale** | Explore Limveld, fight PvE enemies, and gather equipment before competing against rival players or teams. The last surviving player or team wins. |
| `MODE-02` | **Arena** | Fight in the Nightlord arena in Solo (1v1), Duo (2v2), or Trio (3v3), with a loadout chosen at the Sparring Grounds. Best of 3: the first player or team to win two rounds takes the match. |

**Arena is in design (started 2026-11-08, before the Battle Royale release and the launcher are finished).** Battle Royale is the
built and tested mode. The code should be decoupled enough that a new mode is an assembly of existing modules.

## Match Completion
After either mode ends, the winner receives a victory screen. Every participant receives an expedition report and returns to Roundtable Hold.

## Mode Match Structures

| ID | Mode | Queue | Match format | Win condition |
|---|---|---|---|---|
| `MATCH-01` | **Battle Royale** | Solo | 6 players, all competing individually | Be the last player alive |
| `MATCH-02` | **Battle Royale** | Duo | 3 teams of 2 | Be the last team alive |
| `MATCH-03` | **Battle Royale** | Trio | 2 teams of 3 | Be the last team alive |
| `MATCH-04` | **Arena** | Solo | 1v1 (or 1 vs 1 bot, offline) | Best of 3: win 2 rounds |
| `MATCH-05` | **Arena** | Duo | 2v2 | Best of 3: win 2 rounds |
| `MATCH-06` | **Arena** | Trio | 3v3 | Best of 3: win 2 rounds |

## Game Mode: Battle Royale (`MODE-01`)

### 1. General Rules
- PvP is always on, on Day 1 and Night 1 as well as on Day 2 and Night 2. No exceptions.
- **Day 1 and Night 1 are one free phase:** no Night's Tide, no night, no night boss. Day 2 and Night 2 use the vanilla Night's Tide, which shrinks the map.
- A player killed by **another player or team** or by **the Night's Tide** is out for good (**Defeated**): no respawn, no comeback.
- A death caused by a **mob, a boss or a fall** is the vanilla death: the player respawns with their items (runes stay where they died, as in vanilla).
- The match ends the moment one player or team is left, at any time of any day. A night is not a round of its own.
- **Night 2 ends the match:** its last player (team) wins (victory screen); everyone else gets the expedition's failure ending (defeat screen).
- There is no Day 3 and no Nightlord. A night has no boss and no minions.
- **The final circle closes to a point** (Leonardo, 2026-11-07, DECISIONS): Night 2's circle shrinks 20:24 → 23:59:59 from 347.6 m to a point (vanilla 367.7 m to 40 m; the game sets Day 2's first circle about halfway between 700 m and the final radius, so it is 20 m smaller too), so the Night's Tide forces the end instead of a timer, as in Fortnite / Warzone. The tide's damage stays vanilla.
- A player or team can be defeated from the very start of the match, once the bleed-out ends (or the whole team is down).
- Each death and kill counts as a defeat; the defeated player becomes a spectator, or leaves for the Roundtable and searches another match.

### 2. Expedition Phases

#### Day 1 and Night 1 (free phase)
- Start with a **hawk crossing**: every player rides the start hawk on one straight diagonal line across the whole map, like Fortnite or Call of Duty Warzone, and each player chooses where to jump off. The line follows one start direction from the vanilla start point to the far side of the map.
- Players fight PvE enemies, clear camps, obtain equipment and level up, and fight each other.
- No Night's Tide and no night boss; the whole phase is free to explore.
- Surviving players advance to Day 2.

#### Day 2 and Night 2
- Vanilla experience: players keep grinding and exploring.
- The Night's Tide is on and shrinks the map area like the vanilla game does. It drags players together to cause more fights that end in a winner.

### 3. Downed state, defeat and loot

**Downed state (Solo, Duo and Trio alike):** at 0 HP a player is **knocked** for the normal 15 s bleed-out.
- A rival's hit takes 3 s off the bleed-out and never fills the revive bar.
- A teammate can revive a knocked teammate (Duo / Trio). Rivals cannot revive rivals.
- When the bleed-out ends, the player is **defeated**: no comeback, no revive. In Solo there is nobody to revive, so the bleed-out is the whole wait.
- Revive items work as in vanilla (e.g. Wending Grace). How they behave in this ruleset has not been tested yet.

**Defeat and spectating:** a defeated player's body is held and invisible; the camera follows a living player (random at first; mouse buttons or d-pad left / right switch player; F10 leaves). The player can instead leave through the game's menu (Esc or pad Start work while defeated) and search another match.

**Loot from a rival's defeat:**

| Mechanic | Requirement |
|---|---|
| Item drop | The victim's **equipped weapons, talismans and picked-up consumables** (not the starting flask) are dropped where they fell. The weapon's upgrade level is kept. Other item types are not dropped (what the build drops is the whole list; Leonardo accepted it, 2026-11-01) |
| Loot collection | Any player (the killer or anyone else) can pick the drop up from the ground; every player sees it |
| Runes | **Discarded.** Runes are not dropped |
| Defeat by a mob | Nothing drops (vanilla death, the player respawns) |

The first version has no figure or marker on the ground (the "Red Phantom" idea is scrapped: simplicity first).

### 4. Match ending

When one player (or team) is the last standing:
1. The last surviving player or team wins the match.
2. Show **VICTORY ACHIEVED** to the winner (everyone else sees the failure ending).
3. Fade out to the expedition report screen, for every participant.
4. Return to Roundtable Hold after the report.

### 5. Time limit
- If the game has a maximum time per expedition, the match uses it and **everyone loses** when it runs out.
- If the game has none, the mod defines one. **Seen 2026-11-07 (`tide-close-lab`, one PC):** when our night-2 fallback sets 7507 after 600 s, the game does nothing visible: no morning, no Day 3, no ending; the clock stays at 23:59:59 and the circle at its final size, and the expedition goes on. So the game has no limit of its own. **Decision (Leonardo, 2026-11-07): no timer; the final circle closes to a point (§1) and the tide ends the match.** A most-kills tiebreak for edge cases (simultaneous deaths, a stalemate) is wanted but not built. Open: whether the tide still hurts after 7507 (LOCAL lost rain SpEffect 99811 at 7507 while standing outside the circle; god mode was on, so the damage was not seen).

### 6. Rules about information and fairness
- **Rivals are invisible on the HUD:** a rival is on no map, compass, party bar or name tag, and the map pins a rival places are hidden too. Teammates stay visible. Life bars stay vanilla; the lock-on target is named only by the game's own enemy bar (no second name tag).
- **Runes from killing enemies** are shared only inside the own team (Solo: only the killer). A rival's kill gives no runes to the other side.
- **Enemy strength** follows the team size (the vanilla Solo / Duo / Trio values), not the number of players in the session.
- **No permanent equipment:** every match begins with the Nightfarer's default gear.
- **Team behavior:** teammates revive each other and keep the cooperative interactions. Rivals fight each other without disabling normal PvE combat.
- **Respawn far:** a player who respawns after a mob death does so far from the other players.
- **Sites of Grace during PvP:** prevent players from using a Site of Grace while they are actively engaged in PvP. During combat, a Site of Grace must not restore HP or flasks. Restore access once combat is no longer active for that player, including when an opponent dies, the player flees or disengages, or combat otherwise ends.

### 7. Overall game loop

1. Go to `Matchmaking Settings`, choose Solo / Duo / Trio, then in the `Target` list select `Battle Royale` (an option like The Deep of Night, re-using the same component with other content).
2. Queue publicly and join a 6-player match with randomly matched players (6 solos, 3 duos or 2 trios), or enter a private match with a password.
3. Receive a team assignment; in solo queue, each player is their own team.
4. Hawk crossing, then explore, level up, gather loot, fight PvE and engage rivals during the free Day 1 and Night 1.
5. Day 2: prepare and gather equipment; the Night's Tide is now on.
6. Night 2: fight inside the shrinking final circle. Defeated players spectate or leave.
7. The last surviving player or team wins.
8. Show **VICTORY ACHIEVED**, present the expedition report, and return everyone to Roundtable Hold.

## Game Mode: Arena (`MODE-02`, in design)

A short PvP duel mode with no expedition: players land straight in a Nightlord's arena with a build they chose before the match.

### 1. Decided (Leonardo, 2026-11-08)
- **Entry:** a new card in the Roundtable expedition list, **right below the Battle Royale card**. Solo / Duo / Trio as for Battle
  Royale: Solo = 1v1, Duo = 2v2, Trio = 3v3. Commence on the card starts the Arena queue.
- **Bots:** an offline option to play against a bot, at least in 1v1 (Duo / Trio with bots is not needed for now).
- **Place:** a Nightlord's arena (the final boss fight of an expedition), no expedition phase, no Day 1 / Day 2. Which one: below.
- **Spawn:** the players (teams) start facing each other at a medium to long distance, never next to each other.
- **Loadout from the Sparring Grounds:** in the Roundtable's training area, the **Sparring Equipment** lets a player take any weapon
  and talisman. The player builds the loadout there; when an Arena match starts, they enter with that loadout.
- **No loadout chosen:** the player enters with the Nightfarer's default weapon, at legendary tier.
- **Upgrade:** every weapon a player brings is at its **maximum upgrade level**.
- **Level:** every player is always **level 15**.
- **Match:** best of 3 (the first player or team to win two rounds wins). Replaces "win three rounds" of the first draft.
- **Arena:** always **Heolstor's** arena (the Night Aspect, the final Nightlord). Heolstor is not spawned; only the players are there.
- **Between rounds:** everyone goes back to their spawn with full HP, FP and flasks, skill cooldown reset, a short countdown before
  the fight. **The ultimate (Ultimate Art) is available only in the last round** (Leonardo, 2026-11-08; replaces "the gauge
  keeps charging across the rounds"): empty and held empty in rounds 1 and 2, full at the start of round 3 (a 2-0 match has none).
- **Round time limit: 3 minutes**, to be tuned in testing. At the limit the side with more HP (sum in teams) wins the round
  `[the HP tiebreak is the agent's proposal, not objected to]`. Built (ARENA-3, 2026-11-09): HP in percent of each player's
  maximum; within 0.5 % it is a draw, nobody scores and the round is played again `[agent's choice]`; the host judges and sends the
  verdict so both games agree.
  **Overtime (Leonardo, 2026-11-17, DECISIONS):** at the limit the round goes on for up to 60 s while both players lose 2 % of their
  max HP per second; the first at 1 HP loses; after that the HP % rule above decides. The round's seconds left show in the rune
  counter, with a "One minute left!" message.
- **No item drops (Leonardo, 2026-11-09):** a down or a lost match never drops a player's items (the Battle Royale defeat drop,
  RW-3, stays off in Arena; today the Arena match skips the BR rules that ask for it, `features/br_match`).
- **Relics are ON:** each player brings their own relics. The point of Arena is to try any relic build with any weapons and talismans.
- **Flasks:** everyone has the **game's maximum** flask count, refilled each round.
- **Loadout = the whole inventory:** the player enters with everything they put in their inventory at the Sparring Grounds (not only
  what is equipped). The first slice may bring only what is equipped / held; the whole inventory is the target.
- **Match end:** the loser sees the defeat ending, the winner **VICTORY**; then both go back to the Roundtable **out of the match's
  session**, free to search another match or do anything else.

### 2. Agent's proposals Leonardo did not object to (2026-11-08; change them any time)
| Topic | Rule |
|---|---|
| End of a round, Solo | 0 HP ends the round at once (a 15 s bleed-out with nobody to revive is only a wait) |
| End of a round, Duo / Trio | Battle Royale downed rules (teammate revive, rival hit −3 s); the round ends when a whole team is down or defeated; defeated players spectate until the round ends |
| Leaver | A player (team) who leaves loses the match; the rest win |
| Matchmaking | A separate queue per mode; 2 / 4 / 6 players; Duo / Trio fill short parties like Battle Royale (MM-FILL); private code works too |

## Scope vs game today (kept in sync by the agent; 2026-11-01)

| Requirement | Game today |
|---|---|
| Free Day 1 / Night 1, tide from Day 2 | **Built** (`day1_secs`, default 1020 s of slowed clock; scenario `day1-free`) |
| Defeated for good by rival or tide; mob death respawns | **Built** (`defeat-rival`, `defeat-mob`); not seen on two real PCs |
| Match ends when one team is left, win / loss endings | **Built** (`match-win`, `match-loss`); two-PC check left |
| Spectating, leave by Esc / F10 | **Built**; default `defeat_exit_secs = 0` (watch until the match ends); camera smoothness open |
| Downed: rival hit −3 s, teammate revive | **Built** (Nucleus); Duo / Trio matches never run by us |
| Item drop (weapons, talismans, consumables), level kept | **Built** (`mi-rw3-items`); other item types not dropped (accepted); two-PC check left |
| Runes discarded on defeat | **Built** (nothing drops them) |
| Team-only rune sharing | **Built** (`mi-rw8-runes`); Duo / Trio teammate case open |
| HUD hiding of rivals (map, compass, bars, name tags, pins) | **Built** (`mi-rw9-rivals`, `mi-rw9-markers`); pin toast "Ally is showing location" still leaks the area |
| Enemy strength by team size | **Built**, accepted by Leonardo (2026-11-01) |
| Hawk crossing | **Built**, on by default; not run in a Friend Session |
| Solo / Duo / Trio queue, 6-player matches | Solo queue works over real Steam with 2 players; 6 players and Duo / Trio queues only in automated tests (MM-6) |
| Private match by password | `private_code` in `nrpvp.toml` (unit-tested only); the in-game "Multiplayer Password" field is the planned route |
| Sites of Grace blocked in PvP combat | **Built** (GRACE-1): flag 7525 ON while a fight lasts (`grace_combat_secs`, 15), per game; `grace-lock` PASS on one PC; two-PC check in FS-HM1 |
| Respawn far after a mob death | **Built** (RW-7), unit-tested; two-instance check open |
| Time limit | **None by decision** (§5): the closing circle ends the match; the game itself has no limit (the night-2 fallback ends nothing) |
| Final-circle stall-breaker | **Built** (TIDE-MAX): the final circle closes to a point (`tide-close-max`); the tide's damage is vanilla. Most-kills tiebreak not built |
| Revive item behaviour in this ruleset | **Not tested** (TASKS SOLO-REVIVE) |
| Arena | In design (2026-11-08): §Arena. A direct warp into Heolstor's arena works solo (ARENA-0 spike 1); default spawns ±30 m on x |

## References

- [Elden Ring Nightreign Official Guide](https://en.bandainamcoent.eu/elden-ring/news/elden-ring-nightreign-the-official-starter-guide)
- [Elden Ring Nightreign — Phantoms](https://www.thegamer.com/elden-ring-nightreign-white-red-phantoms-gear-explained/)
- [Elden Ring Nightreign — Near-Death](https://eldenring.fandom.com/wiki/Near-Death)
- [Elden Ring Nightreign Fandom Gameplay Wiki](https://eldenring.fandom.com/wiki/Category:Nightreign_Gameplay)
