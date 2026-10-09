# Testing

Every result records: **date, testers, game build, SC hash, me3 version, nrpvp commit or version, launch
path (SC launcher or me3 profile)**. Classify each problem as one of: `MOD-DEFECT`, `GAME-BEHAVIOUR`, `SC-LIMITATION`, or
`UNVERIFIED`.

## 1. Test environment

| Item | Value |
|---|---|
| PC A (host) | Leonardo's PC, Steam ID 76561198342403479. Used for all solo testing |
| PC B (client) | Friend's PC, available **only during scheduled Friend Sessions** (§6) |
| Network | Internet (Steam P2P via SC) |
| Pins | see TECHNICAL_NOTES.md §1 |

Running two game instances on one PC was considered and rejected for now. SC needs two real Steam accounts
over Steam networking, which would mean a VM with GPU passthrough or a second Steam client: too much
headache for the value.

**Safety checklist before every session:** launch only via `nrsc_launcher.exe` or `scripts/launch.ps1`;
saves backed up; both PCs on the same SC build and the same nrpvp build.

---

## 2. Test cases

### TC-000 — Solo SC baseline (HUMAN-solo)
Preconditions: saves backed up; stock `nrsc_launcher.exe`.
Steps:
1. Launch via the SC launcher. Note any SC version text (title screen, menus, popups).
2. Host an SC session alone. Note every SC-specific menu entry, option, or item in the inventory or shop,
   especially anything about PvP, friendly fire, teams, or rules.
3. Play a solo expedition through the first night boss (or ~15 min).
4. Confirm a `.co2` save appeared in `%APPDATA%\Nightreign\<steamid>\`.
Expected: normal play. Record the list of SC-specific UI and items found.

### TC-001 — Join and shared expedition (baseline, FS)
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

### TC-004 — E-002: team type vs AI hostility (HUMAN-solo)
Preconditions: T-008 done; me3 launch with SC + nrpvp; solo SC expedition; `nrpvp.log` open in a tail
window (`Get-Content nrpvp.log -Wait`).
Steps:
1. Stand near a group of normal enemies, out of aggro range. Press F8 and record the enemies' team types.
2. Press F5 (self → `hostile_team_type`). Walk into the group. Record: do they aggro? Do your attacks
   still damage them? Can you lock on?
3. Press F6 (restore). Repeat step 2.
4. With F5 active and persist mode **off**, do each of the following and watch the log for resets: dodge
   roll, take damage, die and get revived (or the solo equivalent), use the ultimate art, rest or level up
   at a site of grace, fight the night-boss transition, day change.
5. Repeat step 4 with persist mode **on** (F7). Note any stutter, crash, or odd AI behaviour.
6. Optional, if T-007 found other candidate values: repeat steps 2–3 per value.
Expected: unknown. This is what we're testing. Record the result for every step.

---

## 3. Results log

(Newest first. Use the header format above.)

_No results yet._

## 4. Known bugs

_None yet._

## 5. Regression set

Rerun after any game, SC, me3, or nrpvp update: TC-001, plus TC-003 row 3 once it passes.

---

## 6. Friend Session runbooks

### FS-1 — First two-player session (≈90 min). **Status: draft. Opus finalises it at the readiness gate.**
Preconditions: both PCs have the same SC build (compare `nrsc.dll` hash), the same `dist/nrpvp.dll` + `nrpvp.toml`
(zip them from `dist/` for the friend), saves backed up on both, and a voice call for coordination.
Leonardo hosts.
1. **TC-001** through the me3 launch path (≈20 min). If me3 fails for the friend, fall back to the stock SC
   launcher for TC-001/TC-002 and skip TC-003.
2. **TC-002** friendly-fire probe (≈10 min).
3. **T-007 FS check:** the partner's team type is visible in the log on both PCs (≈5 min).
4. **TC-003** E-003 matrix (≈40 min), including the death/revive tail.
5. Collect `nrpvp.log` from both PCs into `.claude/experiments/FS-1/` (rename to `host.log` / `client.log`).

