# Nightreign PvP

PvPvE / Battle Royale mod for ELDEN RING NIGHTREIGN. It runs next to Seamless Co-op, with no anti-cheat and no connection to
the official matchmaking servers.

**The mod only loads when the game is started with `nrpvp_launcher.exe` (players) or `scripts\launch.ps1` (developer).**
Steam's Play button starts the normal game with anti-cheat and without the mod; that is how to play the normal game.

## Build (developer PC)

Needs: Rust (stable, `x86_64-pc-windows-msvc`), me3 0.11.0 in `Z:\Tools\me3`, the game with Seamless Co-op 1.1.3 installed.
Close the game first: the build cannot replace a DLL that is in use.

1. Build the mod. Output: `dist\nrpvp.dll`.

```powershell
powershell -NoProfile -File scripts\build.ps1
```

2. Make the zip to send. Pick one:

| Zip | Command | For |
|---|---|---|
| `dist\NightreignPvP.zip` | `powershell -ExecutionPolicy Bypass -File scripts\package-release.ps1` | Normal player build: PvP always on, no queue |

The packaging script builds its own **player** `nrpvp.dll` (without the developer tooling) into `target\player\`, so step 1 is only
needed for developer launches (`scripts\launch.ps1`, Nucleus). Developers drive a running developer game with `dist\nrcon.exe`
(see `.claude/CONSOLE.md`).
`NightreignPvP\VERSION.txt` inside the zip says when and from which commit it was built.

3. Send the zip. **Both players must install the same zip**, including you.

To try a build alone on the developer PC without installing a zip:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\launch.ps1
```

## Install (every player, including the developer)

Needs: ELDEN RING NIGHTREIGN on Steam and Seamless Co-op for Nightreign, the same version on every PC (1.1.3 today).

1. Back up `%APPDATA%\Nightreign` (the Seamless Co-op saves live there).
2. Open the game folder: in Steam, right-click the game > Manage > Browse local files, then open `Game`.
   It is the folder with `nightreign.exe` and `nrsc_launcher.exe`.
3. If an older version is there, delete the `NightreignPvP` folder and `nrpvp_launcher.exe`.
4. Drag `nrpvp_launcher.exe` and the whole `NightreignPvP` folder from the zip into the game folder.
5. Start Steam, then double-click `nrpvp_launcher.exe`.

The zip also contains `HOW TO INSTALL.txt` with the same steps for players, and the test zip adds `MATCHMAKING TEST.txt`
with what to press during a matchmaking test.

Uninstall: delete `nrpvp_launcher.exe` and the `NightreignPvP` folder. Nothing else is changed.

## After a test

Each player sends the folder `<game folder>\NightreignPvP\logs`. The matchmaking test steps and what to write down are in
[.claude/TESTING.md](.claude/TESTING.md) §6 (the current friend-session runbook).
