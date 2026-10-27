# Status (updated 2026-10-24)

**The only "now" doc.** Rules: keep it under ~8 KB; replace lines instead of appending history (results go to `testing-log/`, traps to
[TRAPS.md](TRAPS.md), decisions to [DECISIONS.md](DECISIONS.md), tools to [TOOLS.md](TOOLS.md), live cards to [TASKS.md](TASKS.md)).

## 1. What the mod does today

| Feature | State |
|---|---|
| PvP between players, PvE alongside; rivals marked hostile automatically; teams from player IDs (`team_size` 1/2) | **Works** (Nucleus, real Steam). `team_size = 3` not run |
| One-click install (`dist\NightreignPvP.zip`: launcher + mod + bundled me3 + game data), player build with no overlay | **Works on two PCs** (FS-UI2, 2026-10-21) |
| Solo matchmaking over real Steam (queue → match → same Roundtable → expedition as rivals); Battle Royale in the stock table menu, status as the game's own messages | **Works with one host click**, 2 players (FS-MM3, FS-UI2); 6 players only in automated tests. **On one PC** between two emulated-Steam instances: queue → same expedition as rivals, `mi-queue` PASS 3/3 (2026-10-27, orchestrator/MULTI-INSTANCE §20) |
| Downed rule: rival hits take 3 s off the bleed-out and never fill the bar; teammates revive; rivals cannot revive rivals (T-040/T-041) | **Works, clean** (Nucleus). Not yet over real two-PC lag |
| Hawk crossing at the start (one line, dive, drop) | **Works solo**, on by default (HAWK-2, PR #2); not run in an FS |
| **Match rules after the first real match (RW-1/2/4, 2026-10-24):** killed by a rival (hit log, 30 s) or the tide = **defeated for good** (body held, defeat screen after `defeat_exit_secs` = 20 s, then the Roundtable); killed by an enemy or a fall = vanilla respawn; the match ends the moment one team is left, on any day (a player is out after 30 s dead or 6 s gone); **free Day 1** (`day1_secs` = 1020: slowed clock to 11:50, no tide, no night, then the game's morning); Day 2 vanilla | **PASS on one PC** (`day1-free`, `defeat-mob`, `defeat-rival`, `match-win`, `match-loss`). Not seen on two PCs: the hold after a bleed-out death, the other screen, real hits (FS-RW1, TESTING §6) |
| **Spectating (RW-1 / M5-7):** a defeated player (`defeat_exit_secs = 0`, `spectate = true`) watches: the held, invisible body follows a living player once the hold hides it, mouse buttons / pad d-pad left-right switch player (not while a menu is open), F10 leaves | **Works in Nucleus (3 players, Leonardo 2026-10-25)**. Esc and pad Start open the menu for a dead player (`esc-defeated` PASS, pad confirmed on one PC; FS confirmation left, TASKS); camera smoothness open |
| Enemy strength by team size (RW-10): `MultiPlayCorrectionParam` rewritten in memory (Solo = no co-op SpEffect, Duo / Trio = that size's) | Table found and rewritten on one PC (2 copies, 168 rows); the effect on enemy health is not measured |
| Private match (RW-6): `private_code` in `nrpvp.toml` puts players with the same code in their own queue room (lobby tag `room`, hashed) | Unit-tested only (FakeSteam); needs two PCs |
| Offline mode (`mode = "offline"`): Commence Solo runs the game's own solo expedition | **Works**; the test base of the orchestrator |
| **Orchestrator `nro` + puppet players** (in this repo since 2026-10-24): offline launch through me3, AFK players the mod counts as rivals, keys/screenshots, scenarios | **Works**; `scripts\regress.ps1` runs `night1-count`, `night1-local-dies`, `night2-win`, `night2-loss`, `puppets-smoke` |

Not built yet: **RW-3** defeat item drops, **RW-5** boss drops to the last hitter, **RW-8** runes shared only inside a team, **RW-9** (built: rivals and the map pins they place are off the map, compass, 3D world, party bars and name tags, seen with two instances 2026-10-27; open: the pin toast, Duo / Trio teammate run, TASKS), the other three need a second real client to observe, final arena contraction and sudden death, Duo/Trio matchmaking (MM-6), host migration (MM-8/9, S10).

**Known bug:** the game crashes on quit (0xC0000374, heap corruption) on every run since 2026-10-10; not proven to be Seamless Co-op's. The
first quit on a post-R5 build did not crash (SMOKE-2), Alt-F4 on the older zip did. A player zip from `main` is the real check. Runs end
with `nro stop` (kill) for this reason.

## 2. What is deployed where

| Place | What |
|---|---|
| **Zip for FS-RW1, 2026-10-24:** `dist\NightreignPvP.zip` (main checkout, built from branch `rw-rules` commit 841d18b, clean), `nrpvp.dll` F8645865520E09CF, gamedata emevd 8B9459487BD2A5A3, hawk routes 5ACA00E4047EB8C2, `-MatchPlayers 2`, `team_size = 1` | Player build with the RW rules (defeat for good, free Day 1, scaling, private code). **Not** installed in Leonardo's game folder by an agent. Every older zip (the FS-M59 one, 1BA13128659811B7, too) and `dist\friend-kit.zip` are superseded: do not send them |
| This checkout's `dist\` after `scripts\build.ps1` | Dev build for `launch.ps1` and `nro` (`nrpvp.toml` = dev toml with `mm_queue` / `br_menu` off) |
| Nucleus handler `D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN` | Plain deploy (SC's launcher), secondary tool (TOOLS.md) |

## 3. Next, in order

The list Leonardo gave on 2026-10-24 (fixes F1-F4, design changes D1-D7) is tracked in [TASKS.md](TASKS.md) ("Leonardo's list"), with state and test route per item.

1. **F2 / RW-8: runes shared only inside the own team** (not built; started in its own session 2026-10-27). Then **D2 / RW-3** (defeat drops) and **D4 / RW-5** (boss drops to the last hitter): both need RE.
2. **Two-instance checks** (orchestrator `mi-*` scenarios, one PC, no friend needed): respawn far (F1), enemy scaling measured (F4), the other screen of a defeat and spectating (D1), private match (D7), Duo/Trio teammates for RW-9.
3. **RW-9 leftovers:** the pin toast "Ally is showing location" and the killfeed idea built on the same mechanism ([re/TOAST_KILLFEED.md](re/TOAST_KILLFEED.md)).
4. **Friend session FS-RW1** (TESTING §6) with a fresh zip: the real two-PC check of everything above (lag, the other screen, real Steam). Zip of §2 is older than the RW-9 work: rebuild first.
5. Matchmaking: MM-3g + MM-7 (no Invite click, auto-start), MM-8, MM-9, S10; then MM-6 (Duo/Trio).
6. Death rules still open: keep level, starting items, respawn protection (`research/S5-death-respawn.md`). Spectator camera smoothness.

## 4. Repository state

- `main` has everything above incl. M5-9, the puppets, `hp0` (console: LOCAL to 0 HP), the hold-slot fix, the orchestrator and the bot clean-up (PR of 2026-10-24).
- Tests: `cargo test --workspace` → 464 nrpvp + 4 nrcon + 3 launcher + 16 nro-core + 2 nro (1 ignored); clippy `-D warnings` (dev and `--no-default-features`) and fmt clean; CI runs the same.
- Pins: game 1.03.3 (exe 1.3.3.0), Seamless Co-op v1.1.3, bundled me3 0.11.0 (`TECHNICAL_NOTES.md` §1, `pins/nightreign.toml`).
- Builds: `cargo build --release` = developer DLL (feature `dev`); `--no-default-features` = player DLL (refuses `dev_mode`). MSVC Build Tools needed (`D:\Programs\Microsoft Visual Studio`).
- Friend-session logs are gitignored (`*.log`); they live in `.claude/experiments/<session>/` on Leonardo's PC.
- `.mcp.json` registers the `nightreign` MCP server (`target\release\nro-mcp.exe` of the main checkout): build it with `cargo build --release -p nro -p nro-mcp`.
