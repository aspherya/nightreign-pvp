# Testing

Every result records: **date, testers, game build, SC hash, me3 version, nrpvp commit or version, launch
path (SC launcher or me3 profile)**. Classify each problem as one of: `MOD-DEFECT`, `GAME-BEHAVIOUR`, `SC-LIMITATION`, or
`UNVERIFIED`.

## 1. Test environment

| Item | Value |
|---|---|
| PC A (host) | Leonardo's PC, Steam ID 76561198342403479. Specs TBD |
| PC B (client) | TBD (T-000) |
| Network | TBD (same LAN or internet) |
| Pins | see TECHNICAL_NOTES.md §1 |

**Safety checklist before every session:** launch only via `nrsc_launcher.exe` or `scripts/launch.ps1`;
saves backed up; both PCs on the same SC build and the same nrpvp build.

---

## 2. Test cases

### TC-001 — Join and shared expedition (baseline)
Preconditions: both PCs on the same SC build, launched via the path under test.
Steps:
1. A hosts an SC session. B joins.
2. Both choose characters and start an expedition.
3. Play until the first night circle closes (or ~10 min). Both fight the same enemies and pick up loot.
4. A kills an enemy that B is also attacking. Check that both see it die.
Expected: both are in the same world and see each other move; enemies, deaths, and loot stay consistent;
no disconnects.
Record: any desync (enemy HP differences, ghost enemies), disconnects, and the SC version.

### TC-002 — Friendly-fire probe (vanilla SC)
Preconditions: TC-001 session running.
Steps: A hits B with (a) a melee weapon, (b) a projectile or bow, (c) an AoE skill or ultimate art,
(d) a status buildup source (poison or bleed), and (e) a fire pot or throwable, if available.
Then check SC's in-game items and menus for any PvP or friendly-fire option, and repeat with it on.
Expected: unknown. This test establishes the baseline. Record for each attack type: hit reaction yes/no,
HP loss on B's screen, HP loss on A's view of B, and status buildup.

### TC-003 — E-003 hostile team type matrix (decisive)
Preconditions: T-004 and T-008 done; me3 launch on both PCs; both in the same expedition, standing close
together, away from enemies.
Matrix: for each candidate value `V` from T-007 (start with the value normal enemies use, then
invader-like values if any exist):

| Row | A (attacker) sets self to V | B (victim) sets self to V | Result |
|---|---|---|---|
| 1 | no | yes | |
| 2 | yes | no | |
| 3 | yes | yes | |

Per row record: does A's lock-on target B; hit reaction on B (both screens); HP loss (both screens); status
buildup; can B hit A back; how normal enemies now treat A and B; does the value persist through a dodge, death,
revive, and the next day phase (watch the log); does anything crash or disconnect.
After the matrix: if any row gives two-way damage, play 10 minutes of PvPvE and kill B. Check death,
revive or downed state, and whether the expedition continues for both.

---

## 3. Results log

(Newest first. Use the header format above.)

_No results yet._

## 4. Known bugs

_None yet._

## 5. Regression set

Rerun after any game, SC, me3, or nrpvp update: TC-001, plus TC-003 row 3 once it passes.
