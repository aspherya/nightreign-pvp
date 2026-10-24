# Status (updated 2026-10-24)

**The only "now" doc.** Rules: keep it under ~8 KB; replace lines instead of appending history (results go to `testing-log/`, traps to
[TRAPS.md](TRAPS.md), decisions to [DECISIONS.md](DECISIONS.md), tools to [TOOLS.md](TOOLS.md), live cards to [TASKS.md](TASKS.md)).

## 1. What the mod does today

| Feature | State |
|---|---|
| PvP between players, PvE alongside; rivals marked hostile automatically; teams from player IDs (`team_size` 1/2) | **Works** (Nucleus, real Steam). `team_size = 3` not run |
| One-click install (`dist\NightreignPvP.zip`: launcher + mod + bundled me3 + game data), player build with no overlay | **Works on two PCs** (FS-UI2, 2026-10-21) |
| Solo matchmaking over real Steam (queue → match → same Roundtable → expedition as rivals); Battle Royale in the stock table menu, status as the game's own messages | **Works with one host click**, 2 players (FS-MM3, FS-UI2); 6 players only in automated tests |
| Downed rule: rival hits take 3 s off the bleed-out and never fill the bar; teammates revive; rivals cannot revive rivals (T-040/T-041) | **Works, clean** (Nucleus). Not yet over real two-PC lag |
| Hawk crossing at the start (one line, dive, drop) | **Works solo**, on by default (HAWK-2, PR #2); not run in an FS |
| **Nights are elimination rounds** (M5-3/5/6, M6-1): no boss, no minions, knocked 15 s then eliminated, night decided when one team is left, winner healed, losers get up at the morning, night 2 = win/loss ending | **PASS** solo, Nucleus and one-PC orchestrator runs incl. night 2 by count (2026-10-24) |
| **Solo real death (M5-9):** at night 0 HP = dead at once (SpEffect 102130 via our event 99100805), held until the morning (breakpoint `exe+0x5C4810`), released through a gate, moved to a random day-1 place after the Day-2 clock | **PASS on one PC** (hold, release, move, no slot starvation). **Waiting for a friend session (FS-M59, TESTING §6):** the 102130 kill (offline solo carries a revive effect, so it stays off there), the other screen's view, SC host/client, banners |
| Offline mode (`mode = "offline"`): Commence Solo runs the game's own solo expedition | **Works**; the test base of the orchestrator |
| **Orchestrator `nro` + puppet players** (in this repo since 2026-10-24): offline launch through me3, AFK players the mod counts as rivals, keys/screenshots, scenarios | **Works**; `scripts\regress.ps1` runs `night1-count`, `night1-local-dies`, `night2-win`, `night2-loss`, `puppets-smoke` |

Not built yet: spectating another player (M5-7), keep-level / inventory drop on death, final arena as its own stage (today night 2 is the whole final fight, decided by count, no contraction or sudden-death tie-break), Duo/Trio
matchmaking (MM-6), host-migration work (MM-8/9, S10). `PROJECT_PLAN.md` §2 and §9.

**Known bug:** the game crashes on quit (0xC0000374, heap corruption) on every run since 2026-10-10; not proven to be Seamless Co-op's. The
first quit on a post-R5 build did not crash (SMOKE-2), Alt-F4 on the older zip did. A player zip from `main` is the real check. Runs end
with `nro stop` (kill) for this reason.

## 2. What is deployed where

| Place | What |
|---|---|
| **Zip for FS-M59, 2026-10-24:** `dist\NightreignPvP.zip` (main checkout), `nrpvp.dll` 64E49188380B5C54, gamedata emevd 8B9459487BD2A5A3, hawk routes 5ACA00E4047EB8C2, `-MatchPlayers 2`, `team_size = 1`, built from `main` d1c3264 (clean, after the clean-up PR #12) | Player build: Solo Battle Royale, nights, die-at-once, no console, no bots or puppets. **Not** installed in Leonardo's game folder by an agent. Every older zip and `dist\friend-kit.zip` are superseded: do not send them |
| This checkout's `dist\` after `scripts\build.ps1` | Dev build for `launch.ps1` and `nro` (`nrpvp.toml` = dev toml with `mm_queue` / `br_menu` off) |
| Nucleus handler `D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN` | Plain deploy (SC's launcher), secondary tool (TOOLS.md) |

## 3. Next, in order

1. **Friend session FS-M59** with the 2026-10-24 zip (TESTING §6). Afterwards Claude reads both PCs' logs (`test-report` skill) and fixes what failed.
2. **M5-7** spectate camera for the dead/held player (it must also allow quitting: Esc is blocked while dead). TASKS.
3. Matchmaking: MM-3g + MM-7 (no Invite click, auto-start), MM-8, MM-9, S10; then MM-6 (Duo/Trio). TASKS, MATCHMAKING.md.
4. Death rules: keep level, starting items, respawn protection; inventory drop (`research/S5-death-respawn.md`, `S2-inventory-drops.md`).
5. The open R2/R3 halves of the refactor (typed views + one `GameMem` trait, `Feature` trait) go in with the next feature (ARCHITECTURE §6).
6. Orchestrator follow-ups: `puppet_revive`, scenarios that wait on state instead of the log tail, `scripts\regress.ps1` before every zip (TASKS "Test tooling").

## 4. Repository state

- `main` has everything above incl. M5-9, the puppets, `hp0` (console: LOCAL to 0 HP), the hold-slot fix, the orchestrator and the bot clean-up (PR of 2026-10-24).
- Tests: `cargo test --workspace` → 464 nrpvp + 4 nrcon + 3 launcher + 16 nro-core + 2 nro (1 ignored); clippy `-D warnings` (dev and `--no-default-features`) and fmt clean; CI runs the same.
- Pins: game 1.03.3 (exe 1.3.3.0), Seamless Co-op v1.1.3, bundled me3 0.11.0 (`TECHNICAL_NOTES.md` §1, `pins/nightreign.toml`).
- Builds: `cargo build --release` = developer DLL (feature `dev`); `--no-default-features` = player DLL (refuses `dev_mode`). MSVC Build Tools needed (`D:\Programs\Microsoft Visual Studio`).
- Friend-session logs are gitignored (`*.log`); they live in `.claude/experiments/<session>/` on Leonardo's PC.
- `.mcp.json` registers the `nightreign` MCP server (`target\release\nro-mcp.exe` of the main checkout): build it with `cargo build --release -p nro -p nro-mcp`.
