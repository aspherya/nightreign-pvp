# Building on Windows

The batch files in the repository root work from any working directory and do not use the original developer's drive letters.
Double-click one to run it; the window stays open so you can read the result. Outputs go into `dist/`.

Install **Windows x64 Rust (MSVC)** from [rustup](https://rustup.rs) and **Visual Studio Build Tools** with the
**Desktop development with C++** workload and a Windows SDK. The scripts find Cargo even when it is not on PATH,
and initialize the installed Visual Studio x64 tools automatically. Close the particular executable or DLL you are rebuilding.

| Batch file | Builds / output | Extra requirements |
|---|---|---|
| `build-launcher.bat` | `dist/NightreignPvP-Launcher.exe` and the game-folder helper `dist/nrpvp_launcher.exe` | Rust + C++ tools only |
| `build-mod.bat` | Player DLL without developer tooling: `dist/nrpvp.dll` | Rust + C++ tools only; `-Arena` enables the Arena test feature |
| `build-all.bat` | Entire developer workspace: launcher, developer DLL, console and orchestrator binaries in `dist/` | Rust + C++ tools; use `build-mod.bat` for the player DLL |
| `build-gamedata.bat` | `dist/gamedata/` and `dist/gamedata-player/`, including the title background | Pinned game unpack, WitchyBND, Smithbox 2.2.6, Python 3 and .NET 10 SDK |
| `build-release.bat` | Mod zip, launcher and updater `release.json` in `dist/release/` | Generated player game data, pinned me3 0.11.0, Rust + C++ tools; asks for a version |
| `prepare-ci-inputs.bat` | `dist/ci-inputs.zip` and its SHA-256 for GitHub Actions | Generated player game data and pinned me3 0.11.0 |

`build-release.bat` only stages local files. It does not upload or publish a release. Its defaults match CI: 6 maximum players,
2 minimum players after the fill wait. The existing pinned game/tool versions still apply; a different game build needs verification.
You do not need the game, Python, me3 or a GitHub token to compile just the launcher or player DLL.

## Machine-specific game-data tools

Copy `build.local.example.psd1` to `build.local.psd1` and fill in your locations. Paths can be absolute or relative to this repo.
The real file is gitignored. Alternatively, run a batch file and it prompts for missing game-tool paths once and saves them there.
For me3, choose the distribution root that contains `bin/`, `LICENSE-MIT` and `LICENSE-APACHE`.

Command-line options override the local file:

```bat
build-gamedata.bat -Unpacked "D:\Nightreign unpack\1.03.3" -Witchy "D:\Mod tools\WitchyBND.exe" -Smithbox "D:\Mod tools\Smithbox" -Python "C:\Python314\python.exe"
build-release.bat -Version 0.3.0 -Me3Dir "D:\Mod tools\me3"
prepare-ci-inputs.bat -Me3Dir "D:\Mod tools\me3"
build-mod.bat -Arena
```

You can also place tools under `tools/local/` (gitignored): `me3/`, `unpacked/`, `WitchyBND/WitchyBND.exe`, `Smithbox/`.
The scripts discover these locations automatically. Python can be `python.exe` or the Windows `py.exe` launcher.
The game unpack and third-party tools are not bundled with the source repository.

## Terminal / automation use

Use `-CheckOnly -NonInteractive` to check prerequisites without compiling or prompting. Set `NRPVP_NO_PAUSE=1` to prevent
the batch window from waiting for a key. Exit code 0 means success; a failed dependency check or build returns a nonzero code.

```bat
set NRPVP_NO_PAUSE=1
build-launcher.bat -CheckOnly -NonInteractive
build-launcher.bat -NonInteractive
```

For release publication and uploading the CI input bundle, see [.claude/RELEASE.md](.claude/RELEASE.md).
