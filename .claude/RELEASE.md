# How to configure, build and release the mod (player zip)

Owner doc for: the build chain, the settings that matter for a release, the packaging command, and what to hand to the other players.
Short checklist for agents: the `release-zip` skill (`.claude/skills/release-zip/SKILL.md`). Tools overview: [TOOLS.md](TOOLS.md).

The product of a release is **one file, `dist\NightreignPvP.zip`**. Players drop two things into their game folder and start the game with
`nrpvp_launcher.exe`. They need the game (Steam) and Seamless Co-op v1.1.3 already installed. Nothing else is required: me3 is bundled.
Never use the Steam Play button with the mod, and never start `start_protected_game.exe` (CLAUDE.md hard rule 2).

## 1. What you need on the build PC (once)

| Thing | Where / version | Check |
|---|---|---|
| Game | ELDEN RING NIGHTREIGN 1.03.3, exe 1.3.3.0 | pins in [TECHNICAL_NOTES.md](TECHNICAL_NOTES.md) §1; if the game updated, **stop**: offsets must be re-verified |
| Seamless Co-op | v1.1.3 in `<Game>\SeamlessCoop\` | same pins |
| Rust | stable, `x86_64-pc-windows-msvc`, `%USERPROFILE%\.cargo\bin` | `cargo --version` |
| MSVC Build Tools | `D:\Programs\Microsoft Visual Studio` | "link: extra operand" means MSVC is missing |
| me3 | 0.11.0 in `Z:\Tools\me3` (`bin\me3.exe`, `me3-launcher.exe`, `me3_mod_host.dll`, `LICENSE-*`) | the packaging script checks these |
| Game unpack + WitchyBND + Python | `Z:\Tools\nr-unpacked\1.03.3\`, WitchyBND 3.0.1 in `Z:\Tools\` | only for `build-gamedata.ps1` |

## 2. Settings that decide what a release does

Players' settings live in `NightreignPvP\nrpvp.toml` inside the zip. `package-release.ps1` writes it from its parameters, so **do not hand-edit
the zip**: change the parameters and rebuild.

| Script parameter | What it sets | Typical value |
|---|---|---|
| `-MatchPlayers N` (2..6) | `mm_max_players` = `mm_min_players` = N: the queue lobby fills to exactly N players, then the match starts. **Everyone who should meet needs the same N** (lobbies with another N never match) | **2** for a test with one friend; **6** for the real game |
| `-TeamSize N` (1..3) | `team_size`: 1 = every player alone (Solo), 2 = pairs, 3 = trios | **1** (Duo / Trio queues are not built yet, MM-6) |
| `-Me3Dir DIR` | where the me3 binaries come from | default `Z:\Tools\me3` |

The generated file also contains `matchmaking = true` (the Battle Royale Solo queue in the table menu) and `[hawk] enabled = true`.

Settings a player or you may add to `nrpvp.toml` by hand after installing (all optional; defaults in `crates/nrpvp/src/config.rs`, which logs the
effective values at startup):

| Key | Default | Meaning |
|---|---|---|
| `day1_secs` | 1020 | real seconds of the free Day 1 + Night 1 (120..3600); 0 = vanilla Day 1 with tide and night. For a short test use 300 |
| `defeat_exit_secs` | 0 | seconds after a defeat until the defeat screen; 0 = watch until the match ends, F10 leaves |
| `spectate` | true | a defeated player's camera follows a living player |
| `private_code` | "" | players with the same code (letters, digits, `-`, `_`, up to 32) meet in a private queue, outside the public one |
| `mm_fill_wait_s` | 90 | the match starts this long after the 2nd member joined if at least `mm_min_players` are in |
| `matchmaking` | true | `false` turns the queue off entirely |
| `[hawk] enabled` | true | the start hawk crossing |

The player build ignores developer keys (`dev_mode`, the `mm_*` test switches); a developer DLL never goes into the zip.

## 3. Build the zip (the whole procedure)

From the repo root, in PowerShell, with the game **closed** (the build cannot replace a DLL in use):

```powershell
# 0. Is the code in a good state?  (all must pass)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p nrpvp --no-default-features --all-targets -- -D warnings
cargo fmt --all --check

# 1. Only if gamedata\*.json changed or dist\gamedata is missing (a fresh worktree has none):
powershell -ExecutionPolicy Bypass -File scripts\build-gamedata.ps1

# 2. (optional, recommended before a friend session) the automated scenarios; needs `nro desk` in a normal terminal and Steam running
powershell -ExecutionPolicy Bypass -File scripts\regress.ps1

# 3. The zip. 2 players for a test with one friend:
powershell -ExecutionPolicy Bypass -File scripts\package-release.ps1 -MatchPlayers 2
#    For the real game later:   scripts\package-release.ps1 -MatchPlayers 6
#    Test build with the Arena too (not released yet): add -Arena (player DLL with the cargo feature `arena`, full dist\gamedata)
```

`build-gamedata.ps1` writes `dist\gamedata` (everything, dev events included) and `dist\gamedata-player` (no dev-only events: what the
normal zip takes). `-Arena` takes `dist\gamedata` instead; its VERSION line ends in `ARENA test build`.

`package-release.ps1` builds the **player** DLL itself (`--no-default-features`, into `target\player\`) and the launcher, then packs. `scripts/build.ps1` is only for developer launches, not for the zip.

It prints `OK: ...\dist\NightreignPvP.zip (N MB)` and the VERSION line, for example:
`built 2026-11-01 18:30 UTC, commit abc1234 (clean), nrpvp.dll 1A2B3C4D5E6F7788, gamedata emevd ..., hawk routes ..., me3 0.11.0`.
**Build from a clean `main`** so the zip's commit is a real commit (the line says `dirty` otherwise).

### What is in the zip
`nrpvp_launcher.exe`, `HOW TO INSTALL.txt` (the player instructions, from `scripts\release\`), and `NightreignPvP\` with `nrpvp.dll`,
`nrpvp.toml`, `VERSION.txt`, `gamedata\` (15 languages + the Roundtable event script + the hawk line `map\mapstudio\m60_00_00_99.msb.dcx`;
the script refuses an incomplete set) and `me3\` (3 binaries + 2 licences). If you change player-visible rules, update
`scripts\release\HOW TO INSTALL.txt` in the same change (it still describes Solo queue only, which is right today).

## 4. Hand it over and test with one friend (2 players, Solo)

1. **Both PCs:** back up `%APPDATA%\Nightreign\<steamid>\` (never touch the vanilla `NR0000.sl2`). Same game exe 1.3.3.0 and Seamless Co-op v1.1.3;
   stop Steam auto-updates for the session.
2. Send the **same** zip to your friend (a link or file transfer). Each player unzips it and drags `nrpvp_launcher.exe` and the `NightreignPvP`
   folder into the game folder (next to `nightreign.exe` and `nrsc_launcher.exe`); "Replace the files" for an older version.
3. **Compare `NightreignPvP\VERSION.txt` on both PCs**: the `nrpvp.dll` hash must be identical. If not, someone has an old zip.
4. Steam running, start with `nrpvp_launcher.exe` (never the Steam Play button).
5. Both: Roundtable > table > the **Battle Royale** card > Matchmaking Settings > Number of Players = **Solo** > Commence. "searching for players (1 of 2)"
   appears; nobody presses anything after that: the match is found, both are brought together and the expedition starts by itself.
6. Play. For a quick check set `day1_secs = 300` at the top of both `nrpvp.toml` files before starting. The runbook with what to try and what
   to expect (kill, spectate, drops, F10, quit) is [TESTING.md](TESTING.md) §6.
7. After the session, collect from **both** PCs: `NightreignPvP\nrpvp.log`, the `NightreignPvP\logs\` folder, the me3 log folder
   (`%LOCALAPPDATA%\garyttierney\me3\data\logs\`), photos. Put them under `.claude\experiments\<session>\host\` and `...\guest\` (gitignored) and use the
   `test-report` skill to turn them into a `testing-log/` entry.
8. Record the build in [STATUS.md](STATUS.md) §2 (date, `nrpvp.dll` hash, flags, "not installed by an agent") and say which older zips must not be sent.

If the game crashes on quit (0xC0000374) it is the known bug; note the time and send the logs anyway.

## 5. Later: more players (autonomy checklist)

- **Same N everywhere.** Build with `-MatchPlayers 6` and give every player that exact zip. A player on another N or an older build never sees the
  others (lobby filters `need`, `proto`, `game`, `sc`). If a match does not form, compare the `need=` and `proto` lines in the logs (MATCHMAKING.md, lobby data).
- **6 players is verified only in automated tests**; the real queue has been run with 2. Expect to find problems and keep the logs.
- **Duo / Trio** queues do not exist yet (the menu says "coming soon"); `team_size = 2/3` is only for tests (TASKS DUOTRIO-1, MM-6).
- **Private groups:** give every player the same `private_code` line to keep strangers out.
- **After a game or Seamless Co-op update:** re-hash, update the pins (`TECHNICAL_NOTES.md` §1, `pins/nightreign.toml`), re-run `regress.ps1`
  and the `mi-*` scenarios, then rebuild.
- **Rollback:** keep the previous zip (name it by date); a player restores by dropping that zip's files over the game folder. Uninstall = delete
  `nrpvp_launcher.exe` and the `NightreignPvP` folder.

## 5b. Publish for the launcher app (LAUNCH-1)

The launcher app (`crates/nrpvp-app`, `NightreignPvP.exe`) installs and updates the mod from GitHub releases of the **public releases-only
repository** `notgonnaleo/nightreign-pvp-releases` (DECISIONS 2026-11-06; the source repository is private). One command builds the zip, the
launcher and `release.json`, and uploads them:

```powershell
./scripts/publish-release.ps1 -Version 0.2.0 -NotesFile notes.txt -MatchPlayers 2 -DryRun   # stage in dist\release\, upload nothing
$env:NRPVP_RELEASE_DIR = "$PWD\dist\release"; .\dist\release\NightreignPvP-Launcher.exe     # try the staged release locally
./scripts/publish-release.ps1 -Version 0.2.0 -NotesFile notes.txt -MatchPlayers 2           # publish (gh auth login once)
```

- **Do not publish before the final test session with more than 3 players** (Leonardo, 2026-11-06). Until then use `-DryRun` only.
- The newest non-draft release is what every launcher installs: publishing is the release. Never reuse a version number.
- `release.json` pins the game exe and `nrsc.dll` hashes from `pins/nightreign.toml`: the launcher warns a player whose game or Seamless
  Co-op differs. Update the pins when the game or SC changes.
- Players download `NightreignPvP-Launcher.exe` once from the releases page; it shows when a newer launcher is out (no self-update yet).
- Testing without GitHub: `NRPVP_RELEASE_DIR=<folder with release.json + assets>`; another repository: `NRPVP_RELEASES_REPO=owner/name`.
- **Private room** (RW-6): the launcher's Private room tab writes `private_code` into the installed `nrpvp.toml` (before the first table); an update keeps it. Friends save the same code, then queue as usual. Mock: `?room=night-owls`.
- The launcher warns (Install tab and the bottom bar, red) when files of another tool sit in the game folder and make the game load other code: `winmm.dll`, `dlllist.txt`, `OnlineFix*`, `steam_emu.ini`, `ColdClientLoader.ini`, `steam_settings` (an OnlineFix package broke every run on 2026-11-08, TRAPS). It removes nothing. Mock: `?foreign`.
- UI look without building: `.claude/launch.json` config `launcher-ui-mock` serves `crates/nrpvp-app/ui` on a mock (`?fresh`, `?update`).

## 6. Quick troubleshooting

| Symptom | Cause / fix |
|---|---|
| `dist\gamedata incomplete` / "map must hold exactly ..." | run `scripts\build-gamedata.ps1` (without `-VanillaRoutes`) |
| `me3 file missing` | `-Me3Dir` wrong or me3 not 0.11.0 in `Z:\Tools\me3` |
| cargo build of the launcher / player DLL failed | MSVC missing, or a game still running from `dist\` / `target\player\` |
| Launcher says "must be in the game folder" | files dropped in the wrong folder; the game folder holds `nightreign.exe` |
| Launcher says Seamless Co-op was not found | player must install SC v1.1.3 first |
| Players never meet | different `-MatchPlayers`, different mod build, different SC version, or different `private_code` |
| The zip's commit says `dirty` | build from a clean `main` for a session you want to be reproducible |
