# Bots: AI Nightfarers that play the match (owner doc)

Leonardo, 2026-11-06: a match where bots take the place of missing players. They farm, level up, fight each other and fight
players, as close to a player as possible. Two uses: **test subjects** for mocking whole matches on one PC, and **a real mode**
for players who want to play against bots. **Friends and bots in the same match** (two friends + 4 bots) is a requirement.
This reverses "bots are not a game feature" (DECISIONS 2026-10-24); the decision line is in DECISIONS 2026-11-06.

Engine facts stay where they are: `re/MEMORY_MAP.md` §15 (bot manager), §18 (puppets), TECHNICAL_NOTES §4d, research memo
`archive/research/OFF-3-bots.md`. Cards: TASKS "Bots". This doc owns the design, the plan and the open questions.

## 1. What we already know `[VERIFIED]` unless tagged

| Fact | Proof |
|---|---|
| The game ships its own AI Nightfarers: `CSNpcBotMan` + `NPCBotTableParam` (vanilla design: solo player + 2 AI teammates, never used by a shipped event). The battle AI `029999_battle` (`Common_NPC_AI`) plays a Nightfarer: skills, ultimate, items, guard, dodge, revives. | OFF-3 static, OFF-4 run |
| Event `2003[97](table)` arms the manager, `2003[98]` removes its bots. Our `common.emevd` events run them on our flags. | OFF-4 run 2026-10-22 |
| The manager holds 2 bots, but **writing −1 into a slot makes it forget that bot**: the bot stays with its AI, the next table spawns 2 more. 5 bots kept that way. | OFF-5 runs 1-2 |
| Bots are `CS::PlayerIns` in character set 14 (`WorldChrMan+0x9AF0`), handles `0x…10E0000n`, NpcParam 600040000. **Team 29 written on them → they fought Leonardo, each other and the monsters.** No reverts. | OFF-5 run 2 |
| The game spawns an armed table only once the host is on the ground (held through the hawk ride, spawned at the landing). | OFF-5 run 2 `[timing]` |
| Logic `030000_logic` follows the host player and fights what it sees. Logic `031000_logic` (small-base invaders) guards its camp within 80 m. Neither roams the map. | OFF-3 strings |
| `2003[97]` refuses when `[[exe+0x3C13258]+0xDC8]+0x14` > 1. What it counts: UNKNOWN (party size `[HYPOTHESIS]`). | static |
| A second system exists: **hostile Nightfarer invaders at small bases** (`SmallbaseInvationNpcParam`, NpcParam `6000300x0`, team 6), placed by the map pattern; count override `DebugOverrideInvationNpcNum`. Whether they are synced in multiplayer: UNKNOWN (likely, as map enemies `[HYPOTHESIS]`). | OFF-3 strings |
| Puppets: the same creator can put a PlayerIns into the **player list**, but that needs 3 hardware breakpoints and a guard against Seamless Co-op's sweep. The player build has only **2 free hardware slots** in an expedition (TRAPS). | ORCH-2, TRAPS |

## 2. Design

### 2a. Engine: the game's bot manager, bots stay in set 14 (not the player list)

- Spawn with our `common.emevd` flags and detach-and-re-arm (OFF-5 route, proven). **No permanent hardware breakpoint**, no SC guard.
  The puppet route (player list) is kept for the orchestrator only.
- A host-side **bot roster** (`features/bots/`): handle, hero, team, level, HP, position, alive / defeated, kills. The rules read it next
  to the player list. Bots are not players for Seamless Co-op; they are for **our** rules.
- The game's AI does the fighting. Our code decides **where a bot wants to be** (the "brain") and **how strong it is** (level).

### 2b. The brain: per-bot goals, the game's AI executes

A small state machine per bot, ticked by the host every 50 ms, pure Rust (unit-tested with fakes like `br_match`):

| State | Goal | Leaves when |
|---|---|---|
| Farm | Nearest enemy camp / mob group inside the circle that no other bot farms | the camp is cleared, or a rival comes near, or the circle closes on it |
| Fight | The rival (player or bot) that hit it or is within ~30 m | the rival is dead or far, or HP < 25 % → Flee |
| Flee | Away from the attacker, toward the circle centre | HP back above 60 % or a safe distance |
| Move in | The circle centre (the tide is coming) | inside the next circle |
| Hunt | The nearest living rival (endgame: final circle, or ≤ 3 participants) | the match ends |

How a goal turns into movement is the **one big unknown** (BOT-R1). Candidates, cheapest first, all `[HYPOTHESIS]`:
1. **Redirect the AI's target.** Logic 30000 follows `TARGET_HOSTPLAYER`. If the AI resolves "host" through a handle or pointer we can
   write per bot, then "follow a camp mob" = farm and "follow a rival" = hunt, with the game's own navmesh pathing. One lever for all states.
2. **Our own logic script** in plain-text HavokScript from our package (the game loads `c0000.hks` as text, TECHNICAL_NOTES §4b), reading a
   goal point the DLL writes.
3. **A logic the game already ships** that roams (search the 49 `script/*.luabnd` string tables) switched in by NpcThink id.
4. **Teleport hops** along the navmesh (works anywhere, looks bad; last resort and for tests). **In use since 2026-11-07 as the "hunt"**:
   every minute the farthest living bot beyond 150 m is brought to a living enemy 70-130 m from LOCAL, on the side it already was; its
   own logic (follow the host) then comes for LOCAL. One bot per minute, so they arrive one by one.

### 2c. Levels: a bot grows with the match

- Start at the level of the day, like a player: `NPCBotTableParam` 10..80 (level 1), then the hero tiers 10000 / 20000 / 30000 (levels 6 /
  11 / 13) fit Day 1, Day 2 and the final night.
- **Farming gives XP**: a mob whose last attacker is the bot counts (the dying enemy's `+0x1A0` killer handle where the hits were applied, MEMORY_MAP §21; whether a bot handle shows there is UNKNOWN). Every N
  kills = +1 level. How a level is applied to a live bot is BOT-R2: a SpEffect tier (HP, damage, poise), or a respawn into the next
  CharaInit tier at the same place. Bots do not pick up loot (out of scope; their CharaInit loadout is their build).

### 2d. Match rules for bots (host decides; bots have no game of their own)

- **Bots fill the match to 6** (Solo first: 6 × 1). A bot is a rival like a player: team 29 on every game where it exists.
- **Killed by a rival (player or bot) or by the tide = defeated for good**, body removed after a few seconds. **Killed by a mob or a fall**:
  respawns far from others after the same delay a player has (RW-2 / RW-7 parity), level kept.
- Bots count for "one team left" (RW-1): the human wins when every bot and rival is out.
- A player killed by a bot is a **rival kill** (defeat for good), not a mob kill: the defeat rule must know bot handles.
- Killfeed shows the bot's hero name; spectating a bot and RW-3 drops from bots: later cards.

### 2e. Entry (players): the Bots switch in Matchmaking Settings (built, BOT-5)

Leonardo offered a second card or an "Allow Bots" matchmaking setting. A second card cannot be added: the list shows only the row ids the
game knows (0..16, 18, 100); an added row 17 did not show (2026-11-06), like 101 before. **Leonardo chose the setting (2026-11-06).**

- The stock **"No. of Password Players"** row of Matchmaking Settings (F2) becomes **Bots: Off / On**. The mod has no use for the game's
  password matchmaking, so the row is free. Its label and values are swapped by the existing text hook (`menuflow`, hwbp slot 3; ids
  130002 / 130200 / 130201 -> ours 99100105..107), its help lines point at our texts (CL_LineHelp 701002 / 701011 / 701012), and the game's
  "greyed out without a password" predicate is answered with false while the expedition menu is open and the queue is idle (hwbp slot 0,
  `exe+0x9F91C0`). The value is the game's own settings byte `+0x48`, so **the game remembers it between sessions**.
- **On in both builds as an offline training mode (beta)** (Leonardo 2026-11-07, DECISIONS; it was dev-only for a few hours). Its help
  line says so: Solo against 5 AI bots, no other players.
- **Bots On + Battle Royale card + Solo + Commence** = no search: the game's own solo expedition starts at once and 5 bots fill it
  (same DLL, same zip, normal Battle Royale mode). Bots On with Duo / Trio says "Solo only for now". Bots Off = the normal queue. A running
  queue (searching or a match found) ignores the switch.
- Offline mode (`mode = "offline"`, `bots_fill`) stays as the developers' path.

### 2f. Friends + bots: the hard part (honest verdict)

Bots live on the **host's** game. With a friend in the session, the friend sees them only if the game sends set-14 characters to clients.
**Nobody knows yet.** Three outcomes, decided by BOT-0 (a spike with two `nro` instances, no Leonardo time):

| BOT-0 shows | Then | Size |
|---|---|---|
| Host's bots appear on the client (the game syncs them like enemies) and the gate can be passed | Friends + bots almost free: same engine, team writes on every game | S |
| They do not appear, but the small-base **invaders** are synced | Build bots on the invader system (spawn count, team, our brain on top) | M-L |
| Neither | Our own replication over `net::bus` (position, animation, HP of each bot): weeks, and the own-networking track (NETWORKING.md) first | L+ |

So: **solo + bots ships first** (it does not wait for the answer); friends + bots follows BOT-0's verdict.

## 3. Plan (cards in TASKS "Bots")

| Card | What | Who / test | Depends |
|---|---|---|---|
| **BOT-1** ✅ 2026-11-06 | Bot engine back in the mod (non-dev): fill to N hostile bots in an offline expedition, roster with HP / position / alive, console `bots_fill`, `bots_list` | agent; scenario `bots-fill` (offline, one instance) | — |
| **BOT-0** ✅ 2026-11-06 | Spike: host spawns bots in a 2-instance session; does the client have them? What does the gate count? | agent; `mi-bots-sync` lab | — |
| **BOT-2** ✅ 2026-11-06 (Solo) | Match rules: bots in elimination / win, defeat by a bot = rival kill, bot defeated by rival / tide, mob death respawn | agent; scenarios `bots-win`, `bots-defeat` | BOT-1 |
| **BOT-R1** | RE: how to steer a bot (2b list) | Opus, static + one live probe | BOT-1 |
| **BOT-3** | Brain v1: Farm / Fight / Flee / Move in / Hunt on the chosen lever | agent; scenario `bots-roam` (positions change, bots end in the circle) + Leonardo watches one match | BOT-R1 |
| **BOT-R2 / BOT-4** | Levels: tiers by day + XP from farming | agent; scenario `bots-level` | BOT-1 |
| **BOT-5** ✅ 2026-11-07 | Entry: the Bots switch in Matchmaking Settings (§2e), solo | agent + Leonardo one click test | BOT-2 |
| **BOT-6** | Friends + bots | per BOT-0 | BOT-0, BOT-2 |

Done for "bots v1" = Leonardo plays one Solo match from the card against 5 bots that roam, farm, level and fight, and finds it fun enough.

## 4. Findings log (newest last; one line each, details in testing-log/)

- 2026-11-06: design written (this doc). Old OFF-5 code to restore: git `72f7689` (`features/bot_match.rs`, bot views in `game/views.rs`).
- 2026-11-06: **BOT-1 done**: `bots-fill` PASS, 5 hostile bots kept from the bot set at `WorldChrMan+0x9AF0`, fighting (HP falls). The HUD labels them `?NpcName?` (name text missing: BOT-2 killfeed / names). Their max-HP field reads 1/3 of HP (MEMORY_MAP §18).
- 2026-11-06: **BOT-0 answered** (`mi-bots-sync`): the solo gate counts the session's players; held at 1 the host spawns bots in a session, but
  **the client never gets them**. The small-base **invaders are on both games with the same handles**. So friends + bots = row 2 of §2f:
  build on the invaders (their team written on every game like a rival's, our brain on top). Solo bots keep the bot manager (engine A).
- 2026-11-06: **BOT-2 done for Solo** (`bots-match` PASS): bots count in the elimination (each its own team), a bot's hit on LOCAL is a
  rival hit (so a bot kill is a defeat), a bot killed by LOCAL / another bot / the tide is out, a bot killed by a mob is replaced 14 s later
  by a new one (killer from the bot's `+0x1A0`). **Spread:** each new bot is moved once to a living enemy ~200 m from LOCAL, each in its own
  direction (`game/botspread.rs`). Seen: bots fight their camp, and over time **walk towards LOCAL** (logic 30000 follows the host: 231 → 43 m
  in about a minute for one bot); none warped back. Not done: killfeed names (`?NpcName?`), levels (BOT-4), a real roaming brain (BOT-3).
- 2026-11-07: **BOT-5 done** (`mi-bots-menu` PASS): Bots On in Matchmaking Settings, the Battle Royale card, Commence Solo -> 5 bots
  at 200-300 m, at least ~89 m apart, none near LOCAL at the landing (screenshot `landed`). The extra bot of a round is sent away too.
  The added card row 17 does not show (`bots-card-lab`). Regression `mi-br-card` PASS.
- 2026-11-07: **Leonardo's first match** (player zip, log `nrpvp_2026-11-07_12-44-18.log`): the bots stayed alive at their far camps for
  the whole match and never came; his tide death was judged a mob death (stale last-attacker field, TRAPS) so the game respawned him and
  the match never ended. Fixed: tide by the rain SpEffect (`tide-defeat` PASS), a defeated LOCAL with only bots left loses at once, the
  hunt (`bots-match`), the Bots row dev-only, the row's breakpoint only in the Roundtable (it was arming in the expedition menus).
- 2026-11-07: **Leonardo's second match** (dev build, `dist\logs\nrpvp_2026-11-07_14-18-40.log`, "much better"): 9 hunts, each bringing a
  bot from 170-433 m to 93-124 m; he killed 4, mobs killed 3 (replaced). The WIN came early: the last bot died to a mob as LOCAL died, and
  the replacement could not spawn while LOCAL respawned. Fixed: a bot owed a replacement keeps the match open (`World::bots_pending`), a
  replacement missing after 150 s is given up. `bots-match` PASS.

## 5. How to play against bots (player zip or dev build)

1. Roundtable, the table: Matchmaking Settings (F2) > **Bots: On** (Left / Right), Number of Players **Solo**.
2. Targets (F1): the **Battle Royale** card, then **Commence**. No search: the expedition starts at once.
3. After the hawk crossing 5 bots appear ~200 m away, each at its own enemy camp (at least 80 m apart; never next to you). They farm there
   and over time come for you. Kill them all to win; die to one and you are defeated (watch, F10 leaves). A bot killed by a mob is
   replaced 14 s later by a new one at another camp; a mob kill on you is the vanilla respawn.
4. Dev console: `bots_list` (each bot, alive or dead, HP and distance), `bots_switch [on|off] [solo]`.
