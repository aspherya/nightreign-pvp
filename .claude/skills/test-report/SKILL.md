---
name: test-report
description: Turn a test Leonardo just ran (solo, Nucleus, or a friend session) into evidence - find and read the right logs, judge PASS/FAIL against the card or runbook, write the TESTING.md entry, and update the owner docs. Use when Leonardo says he ran a test, pastes results, or says "check the logs".
---

# Test report

Leonardo runs the game; agents cannot. When he reports a run, the job is to turn it into evidence in the right docs without asking
him to repeat anything the logs already say.

## 1. Find the logs (newest first)

| Run type | Where |
|---|---|
| Orchestrator run (`nro run`, one PC) | `runs\<id>\report.json` (steps with replies), `runs\<id>\mod\nrpvp.log`, `runs\<id>\shots\*.png` (look at the screenshots) |
| Nucleus (dev, secondary tool) | `dist\nucleus-logs\Instance<N>\nrpvp.log` (current) and `...\logs\nrpvp_<UTC>.log` (one per session). One folder per instance |
| `scripts\launch.ps1` (dev, one PC) | `dist\nrpvp.log`, `dist\logs\` |
| Player zip on Leonardo's PC | `<Game>\NightreignPvP\logs\` (`<Game>` = `Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game`) |
| Friend's PC | Only what Leonardo copies in; ask for `<Game>\NightreignPvP\logs` if missing |
| me3 load problems | `%LOCALAPPDATA%\garyttierney\me3\data\logs\<profile>\` |

Pick the session file by its UTC timestamp against the time Leonardo gives. The banner at the top states build, exe version and
config; check the DLL is the build under test before judging anything.

## 2. Read them in this order

1. **Banner + `effective config` line**: right build, right switches (`dev_mode`, `team_size`, `matchmaking`). If a developer game is still running, `dist\nrcon.exe --all status`
   and `log 50` read it directly.
2. **`heartbeat:` lines**: main-loop pass time must stay at a few ms; seconds = a slow-read regression (STATUS traps).
3. **The card's own log lines** (each card and runbook names them: rule lines, `MATCH READY`, `ALL PRESENT`, `MARK #n`).
4. **Errors and panics**: `ERROR`, `panicked`, `failed`.
5. Multi-instance runs: line the instances up on time; the rule must agree on every game.

Report numbers and quote short log lines; never infer what is not in the log. What only Leonardo saw on screen comes from his message
and is marked as such.

## 3. Write it down (one owner per fact, see CLAUDE.md)

- **`testing-log/` (the newest file; index in TESTING.md §3)**: add one entry in the existing shape:
  `### YYYY-MM-DD — <run name> (<who>, <build/zip>, nrpvp.dll <hash16>). Logs: <path>: **PASS|PARTIAL|FAIL**` followed by short
  bullets: what was checked, the evidence, defects.
- **Copy the logs** into `.claude/experiments/<session>/` (`host/`, `client/`, or `Instance<N>/`); they are gitignored and stay local.
- **TASKS.md**: tick the card's acceptance; when a card is fully accepted, remove it (git keeps it) and say so in STATUS.
- **STATUS.md**: §1 feature state, §3 next step, §6 a new trap if one was hit.
- **re/MEMORY_MAP.md**: only when a memory location was confirmed or disproved, with the run as proof.
- **MATCHMAKING.md**: only when the matchmaking flow or a failure pattern changed.

## 4. Reply to Leonardo

Short: PASS/FAIL per check, the one or two lines of evidence that matter, what changes next. If something must be re-run, give
player-side steps (what to press), not code.
