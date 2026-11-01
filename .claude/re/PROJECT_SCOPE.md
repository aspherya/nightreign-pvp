# Elden Ring Nightreign — PvP Mod

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

**Arena still under refinement, Only work with Battle Royale Mode.**

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
- Day 1 and Night 1 are one free phase, No night's tide shrinking closing the map. 
- Day 2 and Night 2 night's tide shrinking is applied. 
- A player killed by a player or the tide is out for good (Defeated).
- The match ends when one player or team is left alive/Determine the winner as the last surviving player or team. (Last man standing wins).
- Nights are basically elimination rounds. Each eliminated player or team results in **defeat**.
- A night has **no boss and no minions**; (related task M5-3).
- **No respawns at night.** At 0 HP a player is **knocked** for the normal 15 s bleed-out (Solo/Duo/Trio).
- A rival's hit takes 3 s off; 
- A teammate can revive a knocked teammate (Duo/Trio). 
- When the bleed-out ends the player is **eliminated**: no comeback, no revive, and they **spectate** a player still fighting.
- **Solo: a kill is a real death** At night a player at 0 HP  **dies at once** unless they carry a revive item (then the item works as in vanilla). The dead stay dead and **spectate**.
- A night ends **only when one player is left** (one team, once Duo/Trio exist).
- If a match passes the maximum vanilla time everyone loses.
- **Night 2 ends the match:** its last player (team) wins (victory screen); everyone else gets the expedition's failure ending (defeat screen).
- **No Day 3 and no Nightlord.**.
- The final circle keeps the game's size (40 m radius).
- Players (or teams) can be defeated right on the start of the match if they die after the near-death/bleed timer runs out (or gets the entire team wiped).
- Defeated players drop their inventory and runes for other players to loot.
- Defeated players loot should be interacted by other players through "Red Phantoms" which will be at the same spot where the player died.
- PvP is always-on. No exceptions, both 1st Day/Night and 2nd Day/Night.

### 2. Expedition Phases

#### 1st Day & Night
- Start with a **hawk crossing** every player rides the
  start hawk on one straight diagonal line across the whole map, like Fortnite or Call of Duty Warzone, and each player chooses where to jump off.
  The line follows a specific start direction from the vanilla start point to the far side of the map.
- Let players fight PvE enemies, clear camps, obtain equipment, and level up.
- No night's tide shrinking, the entire day is free for players to explore.
- Still no night's tide shrinking, the entire day and night is free for players to explore.
- Do not spawn a nighttime boss.
- Allow players to continue fighting each other and PvE enemies.
- Advance surviving players to Day 2.

#### 2nd Day & Night
- Similar Day 1 and Vanilla experience, Players continue grinding and exploring.
- Night's tide now is enabled and should shrink the map area like the vanilla game already does.
- Night's tide shrinking **is be responsible for dragging players together to cause more fights which will result in someone winning/losing.**

### 3. PvP death and inventory system

Every PvP death transfers the victim's entire carried inventory into the world as loot.

| Mechanic | Requirement |
|---|---|
| Inventory drop | Drop all equipped weapons, equipment, consumables, and carried items at the death location as a lootable phantom containing the player's inventory |
| Loot collection | Allow the killer or any other player to collect the dropped items |
| Unspent runes | Drop on death |
| Teammate revive | Allow teammates to revive a downed player before the death is finalized, default bleed-timer behavior when playing with teams. |
| Rival attacks on downed players | Rival hits must not advance the revive meter; each rival hit shortens the bleed-out time |
| Solo-queue death | With no teammates available to revive, reaching zero HP results in death after the bleed-timer runs out. |
| Team elimination | Each death and kill count as a defeat, defeated player becomes spectator and can watch the match or leave and search for another match. |

#### Death sequence

1. Player A defeats Player B.
2. Player B's carried inventory becomes loot at the death location through "Red Phantom" on spot.
3. Player B becomes a spectator of any random player that still on the match.
4. Player B can watch other players or quit the match and search another one.

### 4. Match ending

When one player (or team) is the last standing on the match: 

1. The last surviving player or team wins the match.
2. Show **VICTORY ACHIEVED** to the winner.
3. Fade out to the expedition report screen.
4. Return the player to Roundtable Hold after the report.

### 6. Additional balancing rules

- **No permanent equipment:** Every match begins with the Nightfarer's default gear.
- **Team behavior:** Teammates can revive each other and retain useful cooperative interactions. Rival players can fight one another without disabling normal PvE combat.
- **Sites of Grace during PvP:** Prevent players from using a Site of Grace while they are actively engaged in PvP. During combat, a Site of Grace must not restore HP or flasks. Restore access once combat is no longer active for that player, including when an opponent dies, the player flees or disengages, or combat otherwise ends.

### 7. Overall game loop

1. Go to `Matchmaking Settings` and set if it's going to be Solo/Duo/Trio and then go to `Target` list and select the option `Battle Royale` (The option can be similar to the The Deep of Night option, we can re-use the same component and just changing the content).
2. Queue publicly and join a 6-player match with randomly matched players (6 solos, 3 duos or 2 trios).
3. Receive a team assignment; in solo queue, each player is their own team.
4. Explore, level up, gather loot, fight PvE, and engage rivals during Day 1.
5. Survive Night 1 while the Night's Tide shrinks the area and no night boss appears.
6. Prepare and gather equipment during Day 2.
7. Fight in the Night 2 arena while it contracts and respawns remain enabled.
8. Enter sudden death when the arena closes; respawns stop and deaths become permanent.
9. Win as the last surviving player or team.
10. Show **VICTORY ACHIEVED**, present the expedition report, and return the winner to Roundtable Hold.

## Arena
Players begin inside the Nightlord arena, with no expedition phase.
- Solo: 1v1
- Duo: 2v2
- Trio: 3v3
- Starting loadout: Players start at level 15 with their default weapons at legendary tier.
- Victory: The first player or team to win three rounds wins the match.


## References

- [Elden Ring Nightreign Official Guide](https://en.bandainamcoent.eu/elden-ring/news/elden-ring-nightreign-the-official-starter-guide)
- [Elden Ring Nightreign — Phantoms](https://www.thegamer.com/elden-ring-nightreign-white-red-phantoms-gear-explained/)
- [Elden Ring Nightreign — Near-Death](https://eldenring.fandom.com/wiki/Near-Death)
- [Elden Ring Nightreign Fandom Gameplay Wiki](https://eldenring.fandom.com/wiki/Category:Nightreign_Gameplay)
