# Elden Ring Nightreign — PvPvE Battle Royale Mod

## Core concept

Create a public-matchmaking PvPvE mode for Elden Ring Nightreign. Players queue for a match with random players, are assigned as teammates or rivals, explore Limveld, fight PvE enemies, gather equipment, and compete in a final PvP arena. Solo-queue players have no teammates; every other player is a rival.

The match ends when one player or team wins the final arena. The winner receives the victory screen and the expedition report, then returns to Roundtable Hold. **Losers get the same expedition report and return to Roundtable Hold too** (Leonardo, 2026-10-24). There is **no winner's choice**: winning ends the match, nothing is offered.

## Engineering foundation

Achieving this scope depends heavily on reverse engineering Elden Ring Nightreign and Seamless Co-op. The project must discover and verify how the game implements player state, combat, death and revival, inventory, Sites of Grace, expedition progression, multiplayer sessions, and synchronization, then use those findings to implement the required mode. Base decisions on verified Nightreign and Seamless Co-op behavior; do not assume an interface, hook, or capability exists without evidence.

## 1. Match structure

| Match setting | Requirement |
|---|---|
| Players | **6 players per match, in every mode** (Seamless Co-op's cap) (decided by Leonardo 2026-10-19) |
| Queue types | Solo (6 players, everyone a rival), Duo (3 teams of 2), Squad/Trio (2 teams of 3) |
| Matchmaking | Public queue that matches players with random participants and assigns teammates and rivals |
| Solo teams | Each player is an individual team; all other players are rivals |
| Team queues | Duo and trio players are assigned to teams of the selected size; players on other teams are rivals |
| Duration | Approximately 40–45 minutes, similar to a standard expedition |
| Winner | The last surviving player or team after respawns are disabled |

## 2. Expedition phases

**Nights are elimination rounds (Leonardo, 2026-10-22; supersedes the Night 1 / Night 2 / sudden-death / Nightlord rules below
where they differ):**
- A night has **no boss and no minions**; the night is only the player fight inside the closed circle (built: M5-3).
- **No respawns at night.** At 0 HP a player is **knocked** for the normal 15 s bleed-out (Solo too); a rival's hit takes 3 s
  off; a teammate can revive a knocked teammate (Duo/Trio). When the bleed-out ends the player is **eliminated** until the morning:
  no comeback, no revive, and they **spectate** a player still fighting (a random one for now; choosing whom to watch comes later)
  (decided 2026-10-22).
- **Solo: a kill is a real death (Leonardo, 2026-10-23; supersedes "knocked … Solo too" for Solo).** At night a player at 0 HP
  **dies at once** unless they carry a revive item (then the item works as in vanilla). The dead stay dead until the morning and
  **spectate**; at the start of Day 2 every dead player **respawns at a random place on the map**. No downed bodies in a Solo arena.
  "I want the players to feel the reward of killing a player, not just hiding it." Duo/Trio keep the knock (teammates revive).
  Needs research first: how the game kills a character, keeps them dead and brings them back (TASKS M5-8).
- A kill feed (notifications in the bottom-right corner of the screen) is planned by Leonardo as its own system later; not part
  of the night work.
- A night ends **only when one player is left** (one team, once Duo/Trio exist): no timer.
- **Only the night's winner is healed** at the end of the night. The others **respawn at the morning, spread over different
  places**.
- **Night 2 ends the match:** its last player (team) wins (victory screen); everyone else gets the expedition's failure ending.
  **No Day 3 and no Nightlord.**
- The final circle keeps the game's size (40 m radius) for now: "the idea is to be a mess"; change it only if players complain.
- Day rules are unchanged (respawning on, PvE, loot).

### Day 1 — Exploration and looting

- Start with a **hawk crossing** (Leonardo, 2026-10-21; replaces "spawn teams at different positions"): every player rides the
  start hawk on one straight line across the whole map, like Fortnite's battle bus, and each player chooses where to jump off.
  The line follows the vanilla start direction from the vanilla start point to the far side of the map.
- Let players fight PvE enemies, clear camps, obtain equipment, and level up.
- Enable PvP from the beginning of the expedition.
- On PvP death, drop the player's carried inventory and allow them to respawn.

### Night 1 — Survival

- The Night's Tide shrinks the playable area.
- Do not spawn a nighttime boss.
- Allow players to continue fighting each other and PvE enemies.
- Keep respawning enabled.
- Advance surviving players to Day 2.

### Day 2 — Final preparation

- Expand the playable area again.
- Let teams improve their loadouts and farm runes.
- Place strong PvE enemies near valuable equipment.
- Keep PvP active. PvP deaths continue to drop inventory and allow respawning.
- Prepare all remaining players for the final arena.

### Night 2 — Battle Royale finale

- Bring all remaining teams into the same PvP arena.
- Progressively shrink the arena.
- Keep respawning enabled until the arena reaches its final size.
- Permanently disable respawning when the arena finishes closing.
- Determine the winner as the last surviving player or team.

## 3. PvP death and inventory system

Every PvP death transfers the victim's entire carried inventory into the world as loot.

| Mechanic | Requirement |
|---|---|
| Inventory drop | Drop all equipped weapons, equipment, consumables, and carried items at the death location as a lootable phantom containing the player's inventory |
| Loot collection | Allow the killer or any other player to collect the dropped items |
| Unspent runes | Drop on death |
| Respawning | Allow respawn until the final arena finishes closing |
| Respawn equipment | Restore only the character's initial starting items |
| Character level | Retain the player's level after respawn |
| Teammate revive | Allow teammates to revive a downed player before the death is finalized |
| Rival attacks on downed players | Rival hits must not advance the revive meter; each rival hit shortens the bleed-out time |
| Solo-queue death | With no teammates available to revive, reaching zero HP results in death without a downed wait |
| Team elimination | Make elimination permanent only after respawns are disabled in the final arena |

### Death sequence

1. Player A defeats Player B.
2. Player B's carried inventory becomes loot at the death location.
3. Player B respawns with initial items only and keeps their character level.
4. Player B can recover loot, gather new equipment, or fight again.

To discourage spawn camping, respawn players away from active combat. Give them a short protection period that ends immediately when they attack.

## 4. Night 2 — Final arena rules

The final arena has two stages.

### Stage 1 — Arena contraction

- Keep all remaining teams fighting while the arena shrinks.
- Keep respawning enabled.
- On death, drop the player's inventory and respawn them with initial items while retaining their level.
- Place respawned players at a safe position inside the current playable area.
- Keep dropped equipment available for recovery.

### Stage 2 — Sudden death

- Disable respawns for every player when the arena reaches its final size.
- Make deaths permanent.
- Continue the battle until only one player or team has living players.
- If combat stalls, apply escalating environmental damage to force an eventual winner.
- Keep existing character abilities and teammate revives usable where applicable.
- Do not spawn PvE bosses inside the final PvP arena.
- Maintain enough arena space for combat.

| Arena state | Respawn rule |
|---|---|
| Open arena | Respawns enabled |
| Shrinking arena | Respawns enabled |
| Final arena | No respawns; deaths are permanent |

A team wins once every opposing team has been eliminated after respawns are disabled.

## 5. Match ending

After the final Night 2 player-versus-player battle:

1. The last surviving player or team wins the match.
2. Show **VICTORY ACHIEVED** to the winner.
3. Fade out to the expedition report screen.
4. Return the player to Roundtable Hold after the report.

## 6. Additional balancing rules

- **No kill-farming rewards:** Kills transfer existing inventory; they do not grant extra levels or free high-tier loot.
- **No permanent equipment:** Every match begins with the Nightfarer's default gear.
- **Respawn anti-camping:** Spread respawns across safe areas of the current zone, away from active combat.
- **Loot persistence:** Keep dropped items on the ground for a reasonable duration so the victim's squad or rivals can recover them.
- **No formal alliances:** Rival squads remain hostile even if they temporarily cooperate against PvE threats.
- **Final-arena supplies:** Keep limited neutral loot accessible so players who respawn shortly before sudden death can rebuild a basic loadout.
- **Team behavior:** Teammates can revive each other and retain useful cooperative interactions. Rival players can fight one another without disabling normal PvE combat.
- **Sites of Grace during PvP:** Prevent players from using a Site of Grace while they are actively engaged in PvP. During combat, a Site of Grace must not restore HP or flasks. Restore access once combat is no longer active for that player, including when an opponent dies, the player flees or disengages, or combat otherwise ends.

## 7. Overall game loop

1. Choose solo, duo, or trio matchmaking.
2. Queue publicly and join a 6-player match with randomly matched players (6 solos, 3 duos or 2 trios).
3. Receive a team assignment; in solo queue, each player is their own team.
4. Explore, level up, gather loot, fight PvE, and engage rivals during Day 1.
5. Survive Night 1 while the Night's Tide shrinks the area and no night boss appears.
6. Prepare and gather equipment during Day 2.
7. Fight in the Night 2 arena while it contracts and respawns remain enabled.
8. Enter sudden death when the arena closes; respawns stop and deaths become permanent.
9. Win as the last surviving player or team.
10. Show **VICTORY ACHIEVED**, present the expedition report, and return the winner to Roundtable Hold.

**Entry flow wanted for steps 1–2 (Leonardo, 2026-10-12):** at the Roundtable table the expedition type list offers **Solo / Duo / Squad
Battle Royale** next to "Solo Mission" and "Up to 6 players". The player picks one, picks the Nightlord as for any expedition, and
starts. Matchmaking then runs by itself and the match begins once its criteria are met. No overlay buttons, no invite, no menu visit,
no host action.
**No ImGui for players (Leonardo, 2026-10-19):** the final product shows nothing from our overlay to players. Every choice and every
status message lives in the game's own UI (menus, talk lists, on-screen messages), by changing or adding game menu options and
behaviour. The overlay stays a developer tool. The mod itself must create or obtain the session and bring every matched player into it. How it
works today: `MATCHMAKING.md` (the table menu starts the queue, UI-1; one host click remains until MM-3g).

The defining rule is that a death before sudden death costs the player their carried inventory but does not eliminate them. Only the final no-respawn stage determines the winner.

## References

- [Elden Ring Nightreign Official Guide](https://en.bandainamcoent.eu/elden-ring/news/elden-ring-nightreign-the-official-starter-guide)
- [Elden Ring Nightreign — Phantoms](https://www.thegamer.com/elden-ring-nightreign-white-red-phantoms-gear-explained/)
- [Elden Ring Nightreign — Near-Death](https://eldenring.fandom.com/wiki/Near-Death)
- [Elden Ring Nightreign Fandom Gameplay Wiki](https://eldenring.fandom.com/wiki/Category:Nightreign_Gameplay)
