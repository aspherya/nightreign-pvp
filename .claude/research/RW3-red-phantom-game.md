# RW-3: the red phantom, game-side understanding (what the game does, from public sources)

Written 2026-10-31 on branch `feature/player-red-phantom-loot`. Companion notes: static code reading `RW3-red-phantom-static.md` (exe analysis), later an in-game research log. **Nothing here is verified in our own game yet.** Every row is `[SOURCE: link]` (secondary press and guide sites, read through search summaries) or `[HYPOTHESIS]`. Why this exists: Leonardo wants the defeated player's items (and later runes) on the ground as a **red phantom**, and nobody on the team knows this vanilla object well.

## 1. What the player sees (vanilla)

| Fact | Tag |
|---|---|
| A **red phantom** is a **static red figure kneeling** at the spot where another Nightfarer died. It marks that a player died there; you cannot see how they died (unlike ER's bloodstain replays). | [SOURCE: https://www.thegamer.com/elden-ring-nightreign-white-red-phantoms-gear-explained/ , Bandai starter guide via search summary] |
| **Interacting** with it makes it **drop all the gear that player was carrying** at that spot; the gear is then picked up like any ground item, and any Nightfarer can use it. Interaction key: the item/interact button (Triangle on PlayStation, Y on Xbox; F-style prompt on PC). | [SOURCE: thegamer (above); search summary of guides] |
| They are **common** (the game is hard), one guide says near a Day boss's spawn point. | [SOURCE: thegamer; search summary] |
| Gear looted from a red phantom carries a **Grief** passive: two bonus stats tied to the **two strongest stats of the character class the dead player used** (e.g. Duchess Grief = Dexterity + Intelligence; Wylder's = Strength + Dexterity). It appears **only** on weapons looted from red phantoms. Bandai's guide says the grief "grows", raising the bonus for the next person who gets it (meaning not understood). | [SOURCE: https://gaggula.pages.dev/posts/all-character-grief-passives-stat-bonuses-in-elden-ring-nightreign ; thegamer; Bandai starter guide via search summary] |
| **White phantoms** are a different thing: static ones in the Roundtable Hold show the gear a character used to beat a Nightlord (no effect); rare live ones in Limveld replay another player's actions (walking through refills the Arts Gauge). | [SOURCE: thegamer] |

## 2. Where they come from: the surprise

Several sources say a red phantom is **the corpse of a player who died in ANOTHER match** (an earlier expedition, yours or someone else's), not of a teammate dying in the current one: "red phantoms, which are corpses of players who died in other matches" (gaggula); GamesRadar's headline says you can loot **your own corpse on your next run** to get your best gear back; PC Gamer calls it the first evolution of the bloodstain since Demon's Souls. [SOURCE: gaggula page above; https://www.gamesradar.com/games/roguelike/i-hope-youre-not-squeamish-because-elden-ring-nightreign-lets-you-find-and-loot-your-own-corpse-to-steal-back-your-best-gear-on-your-next-run-i-couldnt-have-even-imagined-that-was-possible (headline only readable) ; https://www.pcgamer.com/games/roguelike/my-favorite-little-elden-ring-nightreign-feature-is-the-first-evolution-of-fromsoftwares-iconic-bloodstains-since-2009s-demons-souls-and-has-roots-in-a-roguelike-from-way-back-in-1987 (headline only readable)]

Consequences for us (all `[HYPOTHESIS]`):
- The vanilla feature is probably **a record of a past death** (position / map, class, gear list) that is stored somewhere (the FromSoftware server, or our local save) and **read back by a spawner at the start of a later expedition**, not an object created live when someone dies next to you. That would explain why our held defeat produced nothing, and why a *live* "dead teammate" phantom may not exist at all in vanilla.
- If so, the real questions are: what is the record, who writes it, where is it stored (offline mode has no server), and **can we write one ourselves and make the game spawn it during a match** (for the other players' games), or create the phantom object directly with our own data.
- Offline-mode check (no server): our test setup runs offline / emulated Steam. If red phantoms appear there at all, the record store is local (the save file); if they never appear, it is server-fed and we may have to spawn the object ourselves.

## 3. Runes (separate system)

A real death drops all runes (and the runes of the lost level) at the death spot; the dead player recovers them there, and they are lost for good if the player dies again first; some enemies can pick them up (kill the enemy to get them back). Not mentioned for red phantoms: **runes are not part of the phantom's loot**. [SOURCE: https://beebom.com/elden-ring-nightreign-death-explained/amp/ ; https://deltiasgaming.com/?p=250892 ; search summaries] This matches Leonardo's decision to do items first and runes later.

## 4. Things the sources do not tell us (to find out)

1. Is the phantom created from a **stored record** (cross-match) or also live for a death in the current session (same expedition)? Is there one per dead player?
2. What is in the record: the **inventory** (6 weapons / 2 talismans / 3 consumables in NR?), relics, which gear counts as "carried", the class (for Grief), the death position and map?
3. Who can **interact**: any player? any team type (our rivals are team 29, hostile)? Can the original owner loot their own?
4. After the interaction: are the dropped items **ordinary world items** (shared, first pickup wins) or per-player copies? Does the phantom disappear afterwards?
5. How is it **replicated** in a session (do all players see the same phantom)?
6. How does the Grief value get attached to the dropped weapon (an item-instance field? a SpEffect? a param row)?
7. Limits: how many phantoms exist per expedition, and how they are chosen.

## 5. Next

Static code reading (the exe: object, creation, interaction, item drop) is in `RW3-red-phantom-static.md`; the in-game research that settles §4 comes after it. Public-source caveat: most pages were readable only as search summaries; treat every row as a lead, not a fact.

## 6. White phantoms (added 2026-10-31; why they matter to us)

Public descriptions are thin and agree with each other [SOURCE: https://www.thegamer.com/elden-ring-nightreign-white-red-phantoms-gear-explained/ ; https://rutab.net/b/games/2025/06/04/chto-delayut-fantomy-v-elden-ring-nightreign-polnyy-gayd.html]:
- **Static white phantoms** stand at the entrance of the Roundtable Hold and **show the gear of a player who beat a Nightlord**. No interaction beyond looking.
- **Live white phantoms** are rare in Limveld and **replay another player's actions**; walking through one fully refills the Arts Gauge. They are less common than in the original game because Limveld's map is randomised.
- No source says where the data comes from (server, past runs, local save) or how it is generated. [UNKNOWN]

Why they may help us [HYPOTHESIS]:
1. A static white phantom **displaying a player's gear in the Hold** is a **second consumer of a "player gear snapshot"** record. If both phantoms read the same record type (gear list + class + appearance), the Hold phantoms can be seen in **our offline game** (the Roundtable is reachable there), so we can find the record store, its format and the spawner **in-game without needing a red phantom to appear**. This is the cheapest first in-game experiment.
2. The record that makes a white phantom look like a player (appearance, armor, weapons) is the same data a red phantom would need to show a kneeling figure of the dead player; the red one adds "loot on interaction" and the Grief tag.
3. If the Hold phantoms exist offline, the record is local (save file or a bundled/cached list); if they never show, they are server-fed.
4. Live white phantoms replaying actions suggest a **recording / playback** system for another player's character (a PlayerIns-like puppet driven by recorded data). Our own puppet players (`game/puppet.rs`) already create such characters; a phantom may use a similar character-creation path, which would be a template for a kneeling figure.

In-game research to add (later, when asked): visit the Roundtable Hold entrance offline and look; list the characters / entities there (`peek`, `puppet_list`-style listing); check the save for stored gear records.
