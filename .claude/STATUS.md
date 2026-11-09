# Status (updated 2026-11-07)

**The only "now" doc.** Rules: keep it under ~8 KB; replace lines instead of appending history (results go to `testing-log/`, traps to
[TRAPS.md](TRAPS.md), decisions to [DECISIONS.md](DECISIONS.md), tools to [TOOLS.md](TOOLS.md), live cards to [TASKS.md](TASKS.md)).

## 1. What the mod does today

Mode: Battle Royale (`MODE-01`, [re/PROJECT_SCOPE.md](re/PROJECT_SCOPE.md)). **Arena (`MODE-02`) is in design since 2026-11-08** (PROJECT_SCOPE §Arena, TASKS ARENA-0), ahead of the BR release.

| Feature | State |
|---|---|
| PvP between players, PvE alongside; rivals marked hostile automatically; teams from player IDs (`team_size` 1/2) | **Works** (Nucleus, real Steam). `team_size = 2` was tested only for the downed / revive rule (Nucleus); **a Duo party now queues and starts a match as teammates on one PC (`mi-duo-party`, 2026-11-02)**; the RW features between teammates (runes, HUD filter, revive, team win) and `team_size = 3` are still unseen |
| One-click install (`dist\NightreignPvP.zip`), player build with no overlay | **Works on two PCs** (FS-UI2, 2026-10-21) |
| Solo matchmaking over real Steam (queue → match → same Roundtable → expedition as rivals); Battle Royale in the stock table menu | **Finished** (Leonardo, 2026-11-07: works on real Steam with 3 players, no more changes). 6 players only in automated tests |
| **No click after Commence (MM-3g, MM-7):** the host's game makes its lobby by itself, no Steam window, everybody is marked ready, the expedition starts by itself | **Finished, works on real Steam** (2026-11-07, 3 players, as joiner and as host: all present in ~10 s, landed ~70 s after the match was found, no Steam window or notification on any screen) |
| **Battle Royale card in the Roundtable expedition list** (UI-3): the last card, always available; Commence on it starts our queue, any other card is a normal vanilla expedition | **Works on one PC** (2026-11-02; the card is now told by the chosen target list, which also holds on other saves, 2026-11-05). Built into `regulation.bin` (instant). The Deep of Night checked (vanilla Commence). Open: the hawk crossing and other rules also run in normal expeditions (TASKS MODE-SCOPE); not seen on two PCs |
| **Night's Tide death = defeated for good** (fixed 2026-11-07): told by the rain SpEffect on LOCAL, not by the last-attacker field (stale after a fight: a tide death respawned the player in a loop) | **`tide-defeat` PASS on one PC** |
| **The final circle closes to a point (TIDE-MAX, no match timer):** `regulation.bin` final radius 40 → 0 m, tide damage vanilla; Night 2's circle reaches ~0.03 m at 23:59:59 and the tide ends the match. Day 2's first circle is 347.6 m (vanilla 367.7: the game derives it). The game's night-2 fallback (7507 after 600 s) ends nothing | **`tide-close-max` PASS on one PC** (branch `tide-close-max`); not seen on two PCs |
| Downed rule: rival hits take 3 s off the bleed-out; teammates revive; rivals cannot revive rivals | **Works** (Nucleus). Not yet over real two-PC lag |
| Hawk crossing at the start | **Works solo**, on by default; not run in an FS |
| **Match rules (RW-1/2/4):** killed by a rival or the tide = **defeated for good** (body held; default `defeat_exit_secs = 0` = watch until the match ends, F10 leaves; a value above 0 shows the defeat screen after that many seconds; nobody respawns); killed by a mob, boss or fall = the game's own respawn; the match ends the moment one team is left, on any day; **free Day 1** (`day1_secs` = 1020, no tide, no night), Day 2 vanilla | **PASS on one PC** (`day1-free`, `defeat-mob`, `defeat-rival`, `match-win`, `match-win-end`, `match-loss`). Not seen on two PCs (FS-RW1, TESTING §6) |
| **Spectating:** a defeated player watches a living one (held body follows), mouse / d-pad switch player, F10 leaves, Esc / pad Start open the menu; **a hint with these controls shows when the watching starts and every 45 s** (UI-4) | **Works on real Steam** (2026-11-07, 12 min of spectating). **Bug: the hint shows on every screen** (TASKS FS7-1). Camera smoothness open |
| **RW-3 defeat drops:** a rival's kill drops the victim's weapons (level kept), talismans and picked-up consumables where they fell; the host makes the drop, everyone can pick it; **runes are discarded** | **Built, `mi-rw3-items` PASS (2026-11-01)**; the inventory part verified by hand; two-PC check left |
| **RW-8 runes** shared only inside the own team | **Built and accepted on one PC** (`mi-rw8-runes` PASS 3/3, 2026-10-28). Open: Duo/Trio teammate, client-side kill |
| **RW-9 HUD filter:** rivals and their map pins (with the green beam) are off the map, compass, 3D world, party bars and name tags | **Built, seen with two instances** (`mi-rw9-rivals` PASS 2/2, `mi-rw9-markers`, `mi-pin-beam`). The pin toast ("Ally is showing location") is off in every match, for teammates too (UI-5, `mi-toast-solo`); open: Duo/Trio teammates |
| Enemy strength by team size (RW-10) | **Accepted: Leonardo found it balanced (2026-11-01)** |
| Private match (RW-6): `private_code` in `nrpvp.toml` | Unit-tested only; needs two instances / PCs |
| **Orchestrator `nro`** (puppets, keys, screenshots, scenarios, 2-3 real instances with emulated Steam) | **Works**; `scripts\regress.ps1` runs the single-game scenarios, `mi-*` run by name ([orchestrator/SCENARIOS.md](orchestrator/SCENARIOS.md)) |
| **Killfeed (KF-1):** `A downed B`, `A killed B` as toasts, character names ([re/TOAST_KILLFEED.md](re/TOAST_KILLFEED.md)) | **Works on real Steam** (2026-11-07, real names, downed and killed) |
| **K/D scoreboard (SB, [SCOREBOARD.md](SCOREBOARD.md)):** in a match the pause menu (Esc / pad Start) shows your own `Kills: n / Deaths: n` as the game's blinking centre message; back at the Roundtable after a match the game's dialog lists everyone (players and bots) by place with kills and deaths, 3 lines a page, OK for the next | **Works with 3 real players** (2026-11-07). **Bugs: the own score shows on every screen and can show the previous match's numbers** (FS7-1); the ranking after a trade looks wrong (FS7-7) |
| **GRACE-1:** a PvP fight switches Sites of Grace off for that player (flag 7525, 15 s or until no rival is near) | **Works on real Steam** (2026-11-07: ON from a real hit, OFF after 15 s or no rival near) |
| **Bots ([BOTS.md](BOTS.md)), offline training mode (beta):** Matchmaking Settings > Bots: On + Battle Royale card + Solo + Commence = Solo against 5 AI Nightfarers, no other players. They start at separate far camps; every minute the farthest comes to ~100 m of you; they count for the win, a bot kill defeats you (with only bots left you lose at once) | `mi-bots-menu`, `bots-match` PASS on one PC; Leonardo liked his second match (2026-11-07). Not yet: levels, bot names, Duo/Trio with bots, friends + bots |
| Own networking: `net::bus` facts between games | **Built** (NET-1/2); users today RW-3 (on) and RW-5 (off). Replacing Seamless Co-op is a study ([NETWORKING.md](NETWORKING.md)) |

**Launcher app (LAUNCH-1, branch `launcher-app`):** `NightreignPvP.exe` (Tauri 2): first-run wizard (game folder, Seamless Co-op via Nexus, the mod from GitHub releases), update check, Play then the app closes. **Built, unit-tested and tried on a local staged release; not yet tried by Leonardo.** A draft: Leonardo creates the public releases repo; **no release is published before the final test session with more than 3 players** (RELEASE §5b).

**Not built yet** (scope in PROJECT_SCOPE): the most-kills tiebreak for edge cases (no timer, by decision), Solo die-at-once with a revive item.

**Known bug:** the game crashes on quit (0xC0000374, heap corruption) on most runs since 2026-10-10; not proven to be Seamless Co-op's. Runs end with `nro stop` (kill) for this reason.

## 2. What is deployed where

| Place | What |
|---|---|
| `dist\NightreignPvP.zip` in the main checkout | Built 2026-11-07 18:51 UTC from `main` b7c77d2, `-MatchPlayers 3`, Solo, `nrpvp.dll` FAF36FBC275172D3; **installed in Leonardo's `Game\NightreignPvP\` and played by 3 players on 2026-11-07**. `scripts\build.ps1` only updates the repo's `dist\`; the launcher loads `Game\NightreignPvP\` |
| This checkout's `dist\` after `scripts\build.ps1` | Dev build for `launch.ps1` and `nro` (`nrpvp.toml` = dev toml with `mm_queue` / `br_menu` off) |


## 3. Next

**Friend session of 2026-11-07 (3 players, real Steam, testing-log):** host migration works (two in one match, the match went on; also after the host's Alt-F4: HM-1
done), matchmaking is finished (Leonardo), killfeed, grace lock, spectate, defeat and the scoreboard work with real hits. Leonardo:
the mod is close to done; nothing broke the game.

**2026-11-08:** FS7-1..8 merged (#65–#67); their real-Steam checks wait for the next FS. **MM-FILL built (branch `mm-fill`, PR):** Duo / Trio
accept a player alone (or a pair in Trio) and fill the team with random players; the match now takes its teams from the queue roster
(also needed by premade Duo / Trio with 3 teams); `mi-duo-fill`, `mi-duo-party`, `mi-zero-click` PASS. Needs an FS with 4+ players.

**Next: Leonardo's list from that session, TASKS "After the friend session of 2026-11-07" (FS7-1..7):** messages only on the own screen
(also the stale K/D), Battle Royale card first, Bots Off at start, Solo/Duo/Trio from the party size, F10 also leaves the session, the trade ranking, a log line for
the Solo instant death, then the respawn on the hawk (RE first). Then a new zip and Duo / Trio with friends (DUOTRIO-1, never tested on
real PCs). Before that session collect **every** player's logs.

**Arena (MODE-02), started 2026-11-08:** rules in PROJECT_SCOPE §Arena. ARENA-0 spike 1 PASS solo: our warp event (flag 10007500) takes a player from an expedition into Heolstor's arena (`m18`) in ~1 s (branch `arena-0-spike`, MEMORY_MAP §27). **An "Arena" card now shows right after Battle Royale** (row 17 + a breakpoint on the card list loop, MEMORY_MAP §28; dev only). **Commence on the Arena card now lands the player in an empty Heolstor arena (solo PASS, 2026-11-08).** **Two players warped from one BR expedition share the arena, in sync, hits land, and WIN / LOSE banners end the match there (two instances PASS, 2026-11-08).** **ARENA-1 done (2026-11-08): the Arena card queues two players (Solo 1v1, lobby `kind=arena`), both land in the arena 30 m apart facing each other, camera on the opponent; `mi-arena-queue` and `mi-zero-click` PASS.** Next: ARENA-2 (rounds best of 3, end screens, randoms leave the session), then the player build.

**Later:** GFX-1/2 (in-game UI, SB-5), player count (HOST_MIGRATION), SOLO-REVIVE, MODE-SCOPE, MM-8, normal
expeditions (Leonardo has an idea). No match timer: the tide closes to a point (TIDE-MAX).

## 4. Repository state

- `main` has everything above (PRs up to #27).
- Tests: `cargo test --workspace`, clippy `-D warnings` (dev and `--no-default-features`) and fmt; CI runs the same.
- Pins: game 1.03.3 (exe 1.3.3.0), Seamless Co-op v1.1.3, bundled me3 0.11.0 (`TECHNICAL_NOTES.md` §1, `pins/nightreign.toml`).
- Builds: `cargo build --release` = developer DLL (feature `dev`); `--no-default-features` = player DLL (refuses `dev_mode`). MSVC Build Tools needed (`D:\Programs\Microsoft Visual Studio`).
- Friend-session logs are gitignored (`*.log`); they live in `.claude/experiments/<session>/` on Leonardo's PC.
- `.mcp.json` registers the `nightreign` MCP server (`target\release\nro-mcp.exe` of the main checkout): `cargo build --release -p nro -p nro-mcp`.
