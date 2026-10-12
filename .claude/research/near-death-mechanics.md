# Nightreign Near-Death Mechanics

Reference summary for Nightreign's Near-Death system, based on the community wiki article linked below. Use this as a gameplay reference and reverse-engineering lead; verify details against the current game build before treating them as implementation facts.

## Source

- [Near-Death — Elden Ring Wiki (Fandom)](https://eldenring.fandom.com/wiki/Near-Death)
- Page accessed: 2026-10-12.
- The source is community-maintained and may change. The sections below summarize and reorganize its information for this project.

## Normal Near-Death flow

- When a Nightfarer's HP reaches zero, they enter Near-Death for about 15 seconds.
- A downed player cannot attack, but can crawl slowly.
- Allies revive the player by attacking them and dealing enough revive damage to empty the Near-Death gauge. A successful revive restores the player at 50% HP.
- The gauge represents both the remaining bleed-out time and the revive damage still required.
- Attacks that deal revive damage pause the bleed-out timer. If the player receives no revive damage for about 3.5 seconds, the timer resumes and the gauge refills.
- Crawling to a Site of Grace automatically revives the downed player.

## Repeat-down penalties

Repeated Near-Death events make revival harder. The penalty can increase to three levels. The source lists these gauge requirements:

| Penalty level | Gauge bars | Revive damage per bar | Total revive damage |
|---|---:|---:|---:|
| 1 | 1 | 40 | 40 |
| 2 | 2 | 45 | 90 |
| 3 | 3 | 80 | 240 |

The penalty decreases by one level after 180 seconds without another Near-Death event. Visiting a Site of Grace fully resets the penalty.

## Revive damage

The amount of revive damage depends on the weapon or spell and the attack used. Charged attacks can increase the base amount. The wiki lists these weapon base values:

| Base revive damage | Weapon types |
|---:|---|
| 10 | Claws, daggers, fists, torches |
| 12 | Bows, crossbows |
| 14 | Axes, curved swords, flails, hammers, katanas, medium shields, small shields, spears, straight swords, thrusting swords, twinblades, whips |

Additional details from the source:

- Skills scale with the weapon's base revive damage.
- Spell revive damage is determined by the spell, rather than the staff or sacred seal. Charged spells deal more.
- Multi-projectile spells deal their full revive damage when all projectiles connect.
- Revive damage is rounded down, so some attacks may lose fractional damage.

## Death and recovery details

- If the downed player dies, their runes are generally dropped where they first went down, not where they crawled.
- If a mob enemy downed the player, or a mob is nearby, that enemy may hold the runes, shown by a golden light. The runes return to the player when that enemy dies, regardless of who kills it.
- Executor, Guardian, and Revenant can revive nearby allies through their Ultimate Arts. The Revenant's Family can revive allies and can revive the Revenant herself.

## Relevance to this mod

- **Team revives:** preserve the game's teammate revive behavior where it fits the mod's team rules.
- **Rival hits:** the project scope requires rival hits not to advance the revive gauge and each hit to shorten the bleed-out time. The exact in-game behavior must be verified with the current build.
- **Sites of Grace:** the wiki describes crawling to a Site of Grace as an automatic revive and visiting one as a full reset of repeat-down penalties. The project scope separately requires Sites of Grace to be unusable during active PvP, so this interaction must not provide a combat escape or recovery exploit. Access should return after combat ends, per the scope.
- **Rune drops:** the wiki's rune-drop behavior is useful context, but the mod's inventory-drop and rune rules are defined by the project scope and require their own implementation and tests.

## Findings recorded by this project

The project has independently observed and recorded these memory details for the pinned Nightreign build; these are project findings, not claims sourced from the wiki:

- The Near-Death state is tracked in `PlayerIns`: state at `+0x710`, revive gauge at `+0x714`, bleed-out clock at `+0x718`, pause timer at `+0x71C`, down-level decay timer at `+0x720`, and down count at `+0x730`.
- The observed gauge amounts are 40, 90, and 240 for the first, second, and third down levels. The observed bleed-out duration is about 15 seconds, and the pause timer is about 3.5 seconds.
- Writing the bleed-out clock on the owning client's player instance has been observed to affect the downed duration.
- The implementation for rival hits shortening the local player's bleed-out clock is built, but its in-game smoke test is pending. See [`STATUS.md`](../STATUS.md), Task A, and [`TECHNICAL_NOTES.md`](../TECHNICAL_NOTES.md), section 4.

## Open verification points

- Confirm the listed revive-damage values and charged-attack modifiers in the current Nightreign build.
- Confirm whether Sites of Grace automatically revive a crawling player in all relevant expedition states, and how this interacts with the mod's active-PvP lockout.
- Confirm how rune holders and rune recovery behave for player-caused deaths in the mod's PvPvE rules.
- Smoke-test the mod's rival-hit bleed-out rule and verify teammate revives still work.
