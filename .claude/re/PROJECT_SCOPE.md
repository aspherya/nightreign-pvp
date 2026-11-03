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
| `MODE-02` | **Arena** | Fight in the Nightlord arena in Solo (1v1), Duo (2v2), or Trio (3v3). The first player or team to win three rounds takes the match. |

**Arena is still under refinement and comes after the first release. Only Battle Royale is built and tested.** The code should be decoupled enough that a new mode is an assembly of existing modules.

## Match Completion
After either mode ends, the winner receives a victory screen. Every participant receives an expedition report and returns to Roundtable Hold.

## Mode Match Structures

| ID | Mode | Queue | Match format | Win condition |
|---|---|---|---|---|
| `MATCH-01` | **Battle Royale** | Solo | 6 players, all competing individually | Be the last player alive |
| `MATCH-02` | **Battle Royale** | Duo | 3 teams of 2 | Be the last team alive |
| `MATCH-03` | **Battle Royale** | Trio | 2 teams of 3 | Be the last team alive |
| `MATCH-04` | **Arena** | Solo | 1v1 | Win 3 rounds |
| `MATCH-05` | **Arena** | Duo | 2v2 | Win 3 rounds |
| `MATCH-06` | **Arena** | Trio | 3v3 | Win 3 rounds |

## Game Mode: Battle Royale (`MODE-01`)

### 1. General Rules
- PvP is always on, on Day 1 and Night 1 as well as on Day 2 and Night 2. No exceptions.
- **Day 1 and Night 1 are one free phase:** no Night's Tide, no night, no night boss. Day 2 and Night 2 use the vanilla Night's Tide, which shrinks the map.
- A player killed by **another player or team** or by **the Night's Tide** is out for good (**Defeated**): no respawn, no comeback.
- A death caused by a **mob, a boss or a fall** is the vanilla death: the player respawns with their items (runes stay where they died, as in vanilla).
- The match ends the moment one player or team is left, at any time of any day. A night is not a round of its own.
- **Night 2 ends the match:** its last player (team) wins (victory screen); everyone else gets the expedition's failure ending (defeat screen).
- There is no Day 3 and no Nightlord. A night has no boss and no minions.
- The final circle keeps the game's size (40 m radius).
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
- If the game has none, the mod defines one. **Open:** whether vanilla Nightreign ends an expedition on its own at the end of Night 2 is not verified (Day 3 and the Nightlord are removed); until it is, nothing ends a stalled match except the Night's Tide damage inside the final circle. No limit is built.

### 6. Rules about information and fairness
- **Rivals are invisible on the HUD:** a rival is on no map, compass, party bar or name tag, and the map pins a rival places are hidden too. Teammates stay visible. The lock-on target keeps its name tag and life bars stay vanilla.
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

## Arena (`MODE-02`, later)
Players begin inside the Nightlord arena, with no expedition phase.
- Solo: 1v1; Duo: 2v2; Trio: 3v3.
- Starting loadout: players start at level 15 with their default weapons at legendary tier.
- Victory: the first player or team to win three rounds wins the match.

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
| Time limit | **Not built**, rule open (§5) |
| Final-circle stall-breaker | Only the vanilla tide damage |
| Revive item behaviour in this ruleset | **Not tested** (TASKS SOLO-REVIVE) |
| Arena | Not started, after the first release |

## References

- [Elden Ring Nightreign Official Guide](https://en.bandainamcoent.eu/elden-ring/news/elden-ring-nightreign-the-official-starter-guide)
- [Elden Ring Nightreign — Phantoms](https://www.thegamer.com/elden-ring-nightreign-white-red-phantoms-gear-explained/)
- [Elden Ring Nightreign — Near-Death](https://eldenring.fandom.com/wiki/Near-Death)
- [Elden Ring Nightreign Fandom Gameplay Wiki](https://eldenring.fandom.com/wiki/Category:Nightreign_Gameplay)
