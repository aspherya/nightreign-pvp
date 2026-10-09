# nightreign-pvp: friend kit

Thanks for helping test! Please follow these steps in order. Total time: about 15 minutes.

**Golden rules: never launch the game online with this mod, and never mix it with vanilla play in the same
session.** The mod runs without anti-cheat on purpose. Only start the game with `launch-friend.ps1`.

1. **Install me3 0.11.0.** Download the installer from
   <https://github.com/garyttierney/me3/releases/tag/v0.11.0> and run it. Use exactly 0.11.0.
2. **Install Seamless Co-op for Nightreign v1.1.3** (Nexus Mods lists it as "1.13") into your game folder, as
   its page describes. You should end up with `SeamlessCoop\nrsc.dll` inside the game's `Game` folder.
   The game version must be 1.03.3.
3. **Back up your saves.** Copy `%APPDATA%\Nightreign\` (paste that into the Explorer address bar) to somewhere
   safe, for example your Desktop.
4. **Unzip this kit anywhere** (for example `C:\nrpvp-friend-kit`). Keep all the files together in one folder.
5. **Close the game and Steam's overlay games, but leave Steam running.** Then right-click `launch-friend.ps1`
   and pick **Run with PowerShell**. If that is blocked, open PowerShell in the kit folder and run:
   `powershell -ExecutionPolicy Bypass -File launch-friend.ps1`
   - It finds your game folder (or asks you for it), checks Seamless Co-op, and starts the game.
   - It prints the SHA256 of your `nrsc.dll`. The host's is:
     `@@NRSC_SHA256@@`
     If you see a WARNING that they differ, tell me before playing.
   - To preview without launching: add `-DryRun`.
6. **In game:** press **Insert** to show or hide the mod overlay. Follow the host's instructions for the session.
7. **After the session, send me the logs:** zip the `logs` folder in the kit folder (and `nrpvp.log` if present) and
   send it to me. If the game crashed, also include the newest file from
   `%LOCALAPPDATA%\garyttierney\me3\data\logs`.

Troubleshooting: if the script says me3 is not found, close and reopen PowerShell after installing it. Any other
problem: send me a screenshot of the PowerShell window.
