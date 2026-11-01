# Archive (frozen history, 2026-11-01)

Nothing here is kept up to date. Each file is a spike memo, hand-over or run log that finished its job; the **owner doc** column says where
the surviving facts live now. Open a file only when a card or a code comment links it. Old paths (`research/...`, `tasks/...`,
`experiments/...`, `re/er-reference.md`) in `testing-log/` and in git history map to `archive/<same path>` (exception: `re/er-reference.md` is `archive/er-reference.md`).

| File | What it was | Surviving facts live in |
|---|---|---|
| `research/S1-player-cap.md` | 6-player cap in Seamless Co-op | NETWORKING.md, MEMORY_MAP §12 |
| `research/S2-inventory-drops.md` | Inventory drop research (before RW-3) | MEMORY_MAP, `research/RW3-*` (live) |
| `research/S3-match-flow.md` | Match flow, nights, boss handling | DECISIONS.md, TECHNICAL_NOTES §4e |
| `research/S4-mod-channel.md` | Mod-to-mod channel options | NETWORKING.md §4 (built as `net::bus`) |
| `research/S5-death-respawn.md` | Death, respawn, level loss | MEMORY_MAP §16/§17, DECISIONS.md |
| `research/S6-team-spawns.md` | Team spawns (replaced by the hawk crossing) | PROJECT_PLAN §13 |
| `research/S7-mode-menu.md` | Mode menu in the Roundtable | MATCHMAKING.md, MEMORY_MAP §12 |
| `research/S8-public-matchmaking.md`, `S9-matchmaking-decision.md` | Matchmaking options and the decision | MATCHMAKING.md |
| `research/S10-seamless-coop-internals.md` | Seamless Co-op internals study | NETWORKING.md, MEMORY_MAP |
| `research/T-003-seamless-coop.md` | Seamless Co-op first study | TECHNICAL_NOTES §3 |
| `research/T-011-alt-hostility.md` | Hostility values (team 29) | TECHNICAL_NOTES, MEMORY_MAP |
| `research/M5-no-boss-nights.md`, `M5-8-death.md`, `near-death-mechanics.md` | Night without boss, die-at-once, near death | TECHNICAL_NOTES §4e/§4f, MEMORY_MAP §16/§17 |
| `research/OFF-3-bots.md` | Game bot manager (puppets' engine) | MEMORY_MAP §15/§18 |
| `research/ORCH-plan.md`, `ORCH-R2-net-player-creation.md` | Orchestrator design | orchestrator/README.md, MULTI-INSTANCE.md |
| `research/UI-0-ingame-ui.md` | In-game UI research | MEMORY_MAP §19/§20, TOOLS.md |
| `research/hawk/*` | Hawk tree notes | PROJECT_PLAN §13, TECHNICAL_NOTES §4b/§4c |
| `tasks/*` | MM-3g brief, S10 hand-over and study, T-041 hand-off | MATCHMAKING.md, MEMORY_MAP §12, TRAPS.md |
| `experiments/*` | Scripts and logs of finished runs (FS-*, HAWK-*, M5-*, MM-*, OFF-3, S10, SMOKE-3, T-0xx, UI-0). Reusable scripts: `M5-1/emdec.py`, `UI-0/disfunc.py`, `UI-0/emevd_dump.py`, `HAWK-0/msb.py` | TESTING.md, testing-log/ |
| `er-reference.md` | Elden Ring hints (not Nightreign facts) | n/a |
| `TESTING-phase0-cases.md` | TC-000..TC-004, overlay-era test cases | testing-log/2026-10-09_to_19.md |

Still live (not archived): `research/RW3-handover.md`, `RW3-red-phantom-game.md`, `RW3-red-phantom-static.md` until the RW-3 card is removed.
`.claude/experiments/` stays as the gitignored drop folder for friend-session logs (`*.log`).
