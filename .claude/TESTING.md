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

Two instances on one PC: `nro` multi-instance scenarios (`scenarios/mi-*.toml`, emulated Steam, [orchestrator/MULTI-INSTANCE.md](orchestrator/MULTI-INSTANCE.md)). Nucleus is parked. The real network (lag, the other PC's screen) is only seen in a Friend Session (§6).

**Safety checklist before every session:** launch only via `nrsc_launcher.exe`, `nrpvp_launcher.exe`, `scripts/launch.ps1` or `nro`;
saves backed up; both PCs on the same SC build and the same nrpvp build.

---

## 2. Test cases

The Phase 0 cases TC-000..TC-004 (overlay buttons and F-keys, deleted in R5) are archived: [archive/TESTING-phase0-cases.md](archive/TESTING-phase0-cases.md). The one that still matters is TC-001 (Seamless Co-op alone still works, §5).

---

## 3. Results log

One entry per test run: date, who, build (game / SC / me3 / nrpvp commit + dll hash), logs or screenshots as evidence, steps with what was seen, verdict **PASS / FAIL / PARTIAL**, and what it changed in the docs. Entries live in `testing-log/` (newest file last; append new entries at the end of the newest file):

| File | Covers |
|---|---|
| [testing-log/2026-10-09_to_19.md](testing-log/2026-10-09_to_19.md) | FS-0, FS-1, TC-00x, T-0xx, MM, UI (frozen) |
| [testing-log/2026-10-20_on.md](testing-log/2026-10-20_on.md) | R5 onward: UI-1/UI-2 sessions, hawk, offline, M5 nights, M5-8/M5-9, puppets, orchestrator scenarios, multi-instance, NET-1/2/5, RW-1..RW-9 |

Automated acceptance: `scenarios/*.toml` run with `scripts\regress.ps1` (orchestrator, one PC; `.claude/orchestrator/README.md`). A card whose acceptance is a scenario is done when that scenario is PASS in a log entry.

## 4. Known bugs

Owned by STATUS.md §1 ("Known bug") so there is one list.

## 5. Regression set

Rerun after any game, SC, me3 or nrpvp update and before every player zip:
1. **Automated, one PC:** `scripts\regress.ps1` (all scenarios, catalog in `orchestrator/SCENARIOS.md`); needs `nro desk`.
2. **Player-zip smoke, 5 minutes, Leonardo** (the zip installed as a player would; REG-1 of 2026-10-22 was retired because its bot checks no longer exist):
   Roundtable > Matchmaking Settings > Number of Players lists **Solo / Duo / Trio**; Solo > Commence shows "searching for players (Solo)" and the button reads Stop Searching (press it: "stopped searching"); Duo > Commence says "coming soon".
   Optional offline check (dev only, not the zip): `mode = "offline"` in `nrpvp.toml`, Solo > Commence starts the game's own solo expedition.
3. TC-001 (Seamless Co-op alone still works).
Past regression runs: `testing-log/`.

## 6. Friend Session runbooks

### FS-RW1 — the new match rules on two real PCs (about 60 min, two or three matches)
**Zip:** `dist\NightreignPvP.zip` (**rebuild it from `main` first** with the `release-zip` skill: the zip of 2026-10-24 predates RW-3, RW-8 and RW-9; STATUS §2 records the build, the `nrpvp.dll` hash and what `VERSION.txt` says), `-MatchPlayers 2`, `team_size = 1`. Both PCs compare `NightreignPvP\VERSION.txt`, Seamless Co-op v1.1.3 and game exe 1.3.3.0 (stop Steam auto-updates). Never EAC. **Pre-flight:** back up `%APPDATA%\Nightreign\<steamid>\`; Leonardo installs the zip himself and sees Solo / Duo / Trio in the table menu. For a shorter Day 1 both PCs add the line `day1_secs = 300` at the top of `NightreignPvP\nrpvp.toml` (default 1020 s = 17 min; both the same, the host ends the day).

**How a match runs:** table > Number of Players = Solo > Commence; the host presses Invite Members; both land by hawk. Nothing needs a console. **Escape hatch:** a defeated player watches until the match ends and can press F10 to leave; if F10 does nothing, Alt-F4 and say so. Write down the time of every quit and any Windows "Application Error".

1. **Free Day 1.** Play Day 1 to its end (no circle, no storm, no night). **Expect** both logs: `free Day 1: clock speed`, host `Day 1 is over at 11:50`, both `Day 2 has begun`; then the game's morning (clock 07:00) and the normal tide on Day 2. Photo of the clock around 11:50 and of the morning.
2. **A real kill.** (Match A, Day 1 or Day 2.) The host brings the guest to 0 HP and the bleed-out runs out (or the host finishes them). **Expect** guest log: `LOCAL defeated by a rival`, `spectating player …`, then `M5-9 deathhold: start step HELD` (no respawn); the camera follows the other player until the match ends; F10 gives `the leave key was pressed`, the defeat screen, Result, Roundtable. Host log: `match decided … won` about 31 s after the guest's death (30 s dead timer), then the win ending. **Unknown today:** is `start step HELD` reached after a bleed-out death in a session (the one-PC runs used a direct death)? If the guest respawns anyway, say how long they stood before the defeat screen.
2b. **RW-3 drops.** The guest is killed by the host with a real rival kill. **Expect** the guest's weapons, talismans and picked-up consumables lie where they fell, visible to both; the host picks one up; the guest's runes are gone (discarded, not dropped); the guest's log shows no error from the drop. Photo of the drop and of the host's inventory.
3. **A non-player death.** (Match B.) The guest dies to an enemy or a fall. **Expect** `LOCAL died to an enemy or a fall`, respawn after about 14 s with items, no `match decided` in the host's log. Optional: the guest dies more than 30 s after the last rival hit.
4. **Tide death on Day 2** (optional): stand outside the closed circle until dead. **Expect** `defeated by the tide`.
5. **Enemy strength (optional; Leonardo already found it balanced, 2026-11-01).** Check only that the log line is there. Logs: `multiplay: enemy scaling rows set for team size 1` on both PCs (the scan takes about a minute after the game starts; do not start the expedition before it).
6. **Private code.** Both PCs set `private_code = "test1"` in `nrpvp.toml` and queue: they must meet (the queue log shows a `room` tag `p-…`). Then one PC changes it to `other` and both queue for 60 s: they must **not** meet.
7. **Observations for RW-3 / 5 / 8 / 9 (no pass or fail, photos and a line each):** (a) runes: one player kills enemies; does the other's rune counter rise (photo of both counters before and after)? (b) the map: is the other player's marker visible (photo of the map on both)? (c) the compass bar and the nametag at about 10 m and 50 m (photos); does a lock-on show a name and a life bar? (d) if a boss dies: both players can loot the Dormant Power orb (vanilla by decision, 2026-10-31); (e) what the defeated player's screen shows from the death on: the camera should follow the other player (spectating) and F10 should leave; and what the OTHER screen shows of the defeated player's body (invisible? a body hovering next to them?).

**Send back, both PCs:** `NightreignPvP\nrpvp.log`, `NightreignPvP\logs\`, the me3 log folder, the photos and a line per step. Put them under `.claude\experiments\FS-RW1\host\` and `...\guest\`. Greps: `night_match|deathhold|multiplay|room|T-033 STATE`.

**No-go:** if a defeated guest can still fight after their defeat (respawns and walks about), stop the kill steps, send the logs and play free for the rest.

Finished runbooks (FS-1, FS-MM1, FS-MM2, FS-UI1 on 2026-10-20; FS-UI2 on 2026-10-21) were removed; their results are in §3 and their text in git
(`git show 339ee04:.claude/TESTING.md`; FS-UI2: `git show c727293:.claude/TESTING.md`).
