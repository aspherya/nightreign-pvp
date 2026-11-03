# Status (updated 2026-11-02)

**The only "now" doc.** Rules: keep it under ~8 KB; replace lines instead of appending history (results go to `testing-log/`, traps to
[TRAPS.md](TRAPS.md), decisions to [DECISIONS.md](DECISIONS.md), tools to [TOOLS.md](TOOLS.md), live cards to [TASKS.md](TASKS.md)).

## 1. What the mod does today

Mode: Battle Royale (`MODE-01`, [re/PROJECT_SCOPE.md](re/PROJECT_SCOPE.md)). Arena (`MODE-02`) is wanted later, not for the first release.

| Feature | State |
|---|---|
| PvP between players, PvE alongside; rivals marked hostile automatically; teams from player IDs (`team_size` 1/2) | **Works** (Nucleus, real Steam). `team_size = 2` was tested only for the downed / revive rule (Nucleus); **a Duo party now queues and starts a match as teammates on one PC (`mi-duo-party`, 2026-11-02)**; the RW features between teammates (runes, HUD filter, revive, team win) and `team_size = 3` are still unseen |
| One-click install (`dist\NightreignPvP.zip`), player build with no overlay | **Works on two PCs** (FS-UI2, 2026-10-21) |
| Solo matchmaking over real Steam (queue → match → same Roundtable → expedition as rivals); Battle Royale in the stock table menu | **Works with one host click**, 2 players (FS-MM3, FS-UI2). One PC, two emulated-Steam instances: `mi-queue` PASS 3/3 (2026-10-27). 6 players only in automated tests |
| **Battle Royale card in the Roundtable expedition list** (UI-3): the last card, always available; Commence on it starts our queue, any other card is a normal vanilla expedition | **Works on one PC** (2026-11-02, `testing-log/2026-10-20_on.md`). Built into `regulation.bin` (instant). The Deep of Night checked (vanilla Commence). Open: the hawk crossing and other rules also run in normal expeditions (TASKS MODE-SCOPE); not seen on two PCs |
| Downed rule: rival hits take 3 s off the bleed-out; teammates revive; rivals cannot revive rivals | **Works** (Nucleus). Not yet over real two-PC lag |
| Hawk crossing at the start | **Works solo**, on by default; not run in an FS |
| **Match rules (RW-1/2/4):** killed by a rival or the tide = **defeated for good** (body held; default `defeat_exit_secs = 0` = watch until the match ends, F10 leaves; a value above 0 shows the defeat screen after that many seconds; nobody respawns); killed by a mob, boss or fall = the game's own respawn; the match ends the moment one team is left, on any day; **free Day 1** (`day1_secs` = 1020, no tide, no night), Day 2 vanilla | **PASS on one PC** (`day1-free`, `defeat-mob`, `defeat-rival`, `match-win`, `match-win-end`, `match-loss`). Not seen on two PCs (FS-RW1, TESTING §6) |
| **Spectating:** a defeated player watches a living one (held body follows), mouse / d-pad switch player, F10 leaves, Esc / pad Start open the menu; **a hint with these controls shows when the watching starts and every 45 s** (UI-4) | **Works** (Nucleus 3 players; `spectate`, `esc-defeated` PASS). Camera smoothness open |
| **RW-3 defeat drops:** a rival's kill drops the victim's weapons (level kept), talismans and picked-up consumables where they fell; the host makes the drop, everyone can pick it; **runes are discarded** | **Built, `mi-rw3-items` PASS (2026-11-01)**; the inventory part verified by hand; two-PC check left |
| **RW-8 runes** shared only inside the own team | **Built and accepted on one PC** (`mi-rw8-runes` PASS 3/3, 2026-10-28). Open: Duo/Trio teammate, client-side kill |
| **RW-9 HUD filter:** rivals and their map pins are off the map, compass, 3D world, party bars and name tags | **Built, seen with two instances** (`mi-rw9-rivals` PASS 2/2, `mi-rw9-markers`). The pin toast ("Ally is showing location") is off in every match, for teammates too (UI-5, `mi-toast-solo`); open: Duo/Trio teammates |
| Enemy strength by team size (RW-10) | **Accepted: Leonardo found it balanced (2026-11-01)** |
| Private match (RW-6): `private_code` in `nrpvp.toml` | Unit-tested only; needs two instances / PCs |
| **RW-5 boss orbs** | **Parked, vanilla** (everyone can loot; DECISIONS 2026-10-31). A first-to-loot skeleton is in the code, off by default (`orb_auto 1`) |
| Offline mode (`mode = "offline"`) | **Works**; the test base of the orchestrator |
| **Orchestrator `nro`** (puppets, keys, screenshots, scenarios, 2-3 real instances with emulated Steam) | **Works**; `scripts\regress.ps1` runs the single-game scenarios, `mi-*` run by name ([orchestrator/SCENARIOS.md](orchestrator/SCENARIOS.md)) |
| **GRACE-1:** a PvP fight switches Sites of Grace off for that player (flag 7525, 15 s or until no rival is near) | **Works on one PC** (`grace-lock`, `mi-grace-local` PASS, 2026-11-03). A real hit between players not seen: FS-HM1 |
| Own networking: `net::bus` facts between games | **Built** (NET-1/2); users today RW-3 (on) and RW-5 (off). Replacing Seamless Co-op is a study ([NETWORKING.md](NETWORKING.md)) |

**Not built yet** (scope in PROJECT_SCOPE): A match time limit, final arena contraction and sudden death, Solo die-at-once with a revive item, Duo/Trio matchmaking (MM-6).

**Known bug:** the game crashes on quit (0xC0000374, heap corruption) on most runs since 2026-10-10; not proven to be Seamless Co-op's. Runs end with `nro stop` (kill) for this reason.

## 2. What is deployed where

| Place | What |
|---|---|
| `dist\NightreignPvP.zip` in the main checkout | **Built 2026-10-24 from branch `rw-rules` (841d18b): older than RW-3, RW-8, RW-9. Rebuild from `main` before FS-RW1** (`release-zip` skill). Not installed in Leonardo's game folder by an agent. Every older zip and `dist\friend-kit.zip` are superseded |
| This checkout's `dist\` after `scripts\build.ps1` | Dev build for `launch.ps1` and `nro` (`nrpvp.toml` = dev toml with `mm_queue` / `br_menu` off) |
| Nucleus handler `D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN` | Plain deploy, parked (TOOLS.md) |

**Release candidate (2026-11-01):** the build of `main` at the audit PRs was tested with a friend in a 2-player Solo match; Leonardo accepted it as the release candidate, core gameplay frozen (DECISIONS). Three checks left for a finished Battle Royale: Grace in combat, Duo/Trio, the revive item.

## 3. Next (Leonardo, 2026-11-03, in this order)

1. **Finish the match start:** no Invite Members press (MM-3g), the match starts at once (MM-7), no Steam invite on the overlay.
2. **Killfeed:** first lines without names ("a team was eliminated, N left"), then names ([re/TOAST_KILLFEED.md](re/TOAST_KILLFEED.md)).

**Ongoing, Leonardo reports when he has the logs:** the sessions with friends: **FS-HM1** (TESTING §6, host migration on real Steam: `mi-host-migration` PASS on one PC, a crash on real Steam not proven; [HOST_MIGRATION.md](HOST_MIGRATION.md)), Duo / Trio rules between real players (`mi-duo-party` shows the party flow), SOLO-REVIVE (the vanilla revive item in Solo). Player count later, on good results. **Dropped:** card polish (Effective line, side panel, own icon). **Later:** normal expeditions (Leonardo has an idea), MODE-SCOPE, rest of matchmaking (MM-8, the 3-team Duo match), FS-RW1.

Clean-up plan around the next features: docs 1:1 with the product (done in this pass), then a refactor so a new mode is an assembly of existing modules (ARCHITECTURE §6), then better orchestrator scenarios and tooling.

## 4. Repository state

- `main` has everything above (PRs up to #27).
- Tests: `cargo test --workspace`, clippy `-D warnings` (dev and `--no-default-features`) and fmt; CI runs the same.
- Pins: game 1.03.3 (exe 1.3.3.0), Seamless Co-op v1.1.3, bundled me3 0.11.0 (`TECHNICAL_NOTES.md` §1, `pins/nightreign.toml`).
- Builds: `cargo build --release` = developer DLL (feature `dev`); `--no-default-features` = player DLL (refuses `dev_mode`). MSVC Build Tools needed (`D:\Programs\Microsoft Visual Studio`).
- Friend-session logs are gitignored (`*.log`); they live in `.claude/experiments/<session>/` on Leonardo's PC.
- `.mcp.json` registers the `nightreign` MCP server (`target\release\nro-mcp.exe` of the main checkout): `cargo build --release -p nro -p nro-mcp`.
