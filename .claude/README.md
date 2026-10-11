# nightreign-pvp

A PvPvE mod for **ELDEN RING NIGHTREIGN**: multiple teams in one expedition, fighting each other and the
world, with a future final-day team brawl. Built on top of **Seamless Co-op** (by Yui) for networking.

> **Status:** Phase 0, reconnaissance. Nothing playable yet. See [ROADMAP.md](ROADMAP.md).

## Requirements
- ELDEN RING NIGHTREIGN (Steam), build pinned in [TECHNICAL_NOTES.md §1](TECHNICAL_NOTES.md)
- Nightreign Seamless Co-op (from Nexus Mods). It is **not** included in this repo and never will be.
- [me3](https://me3.help) 0.11+
- Rust stable (`rustup`, MSVC toolchain) and Visual Studio 2022 C++ build tools
- Two PCs with two Steam accounts for multiplayer tests

## Safety
Modded play runs **without EasyAntiCheat and never on official servers**. Only launch through SC's
launcher or this repo's me3 profile. Back up your saves (`%APPDATA%\Nightreign\`). SC uses separate
`.co2` saves.

## Build and launch
All commands run from the repo root in PowerShell.

```powershell
./scripts/build.ps1     # cargo build --release, copies nrpvp.dll (+ .pdb) to dist/
./scripts/launch.ps1    # default -Profile sc: Seamless Co-op + nrpvp.dll (profiles/nrpvp-sc.me3.toml)
./scripts/launch.ps1 -Profile dev   # nrpvp.dll only, no SC (profiles/nrpvp-dev.me3.toml)
```

- `build.ps1` finds `cargo` on PATH or in `%USERPROFILE%\.cargo\bin`. Toolchain: stable MSVC
  (`rust-toolchain.toml`).
- `launch.ps1` finds `me3` on PATH or at `Z:\Tools\me3\bin\me3.exe`, and refuses to run if
  `dist/nrpvp.dll` is missing. It starts the game **without EAC and with official servers blocked**. Never
  launch the protected exe with mods present, and never go online modded.
- Profiles (me3 v0.11.0 schema; the `path` of a native is relative to the profile file unless absolute):
  - `profiles/nrpvp-dev.me3.toml`: loads only `dist/nrpvp.dll`. No Seamless Co-op.
  - `profiles/nrpvp-sc.me3.toml` (default): loads `nrsc.dll` **in place** from
    `Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game\SeamlessCoop\nrsc.dll` (absolute path; SC files are
    never copied or modified), then `dist/nrpvp.dll`. The SC entry uses `load_early = true` and **no
    `initializer`**: with no initializer, me3 calls SC's `modengine_ext_init` export itself, and setting an
    `initializer` would skip that call. `nrpvp.dll` has `load_after = [{ id = "nrsc.dll", optional = false }]`
    (the id is the DLL file name) and is a normal (non-early) native, so it always loads after SC. The comments in
    the profile cite the me3 docs and source lines behind every setting.
  - **Which save each profile uses** (both in `%APPDATA%\Nightreign\<steamid64>\`):
    - `dev` sets `savefile = "nrpvp-dev.sl2"`, so it reads and writes `nrpvp-dev.sl2` (plus `nrpvp-dev.sl2.bak`) and
      never touches the vanilla `NR0000.sl2`. On the first dev launch me3 seeds `nrpvp-dev.sl2` by **copying**
      `NR0000.sl2` (read only). To start from a blank save, delete `nrpvp-dev.sl2` only with the game closed.
    - `sc` sets no `savefile`: Seamless Co-op uses its own `NR0000.co2`, which me3 does not redirect (it only
      redirects `.sl2`).
    - Rationale and source citations are in the comments of `profiles/nrpvp-dev.me3.toml`. `me3 profile show -f
      profiles\nrpvp-dev.me3.toml` lists the `Savefile` property.
  - `launch.ps1 -DisableArxan` adds `--disable-arxan` for troubleshooting (below).
- **Log file:** `dist/nrpvp.log`, next to the DLL (appended across sessions, safe to delete). Expected
  startup lines:
  - `nrpvp 0.1.0 starting (T-001 skeleton)`
  - `unix time <seconds>`
  - `nightreign.exe: base=0x... size=0x...`
  - `exe path: Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game\nightreign.exe`
  - `exe FileVersion: 1.3.3.0`
- Offline checks: `cargo clippy --all-targets`, `cargo test`, `cargo fmt --check`.

### Friend kit (T-016)
```powershell
./scripts/build.ps1             # package-friend.ps1 does NOT rebuild
./scripts/package-friend.ps1    # -> dist/friend-kit.zip
```
- The zip holds `nrpvp.dll`, `nrpvp.pdb`, `launch-friend.ps1`, `README-FRIEND.md` and `VERSION.txt` (build time, git
  commit, dirty flag, DLL hash) inside `nrpvp-friend-kit\`. Sources are in `scripts/friend-kit/`. `nrpvp.toml` is not
  shipped: the DLL writes the default on first run. The host's `nrsc.dll` SHA256 is embedded into the launcher and
  the README at package time (`-NrscPath` overrides the source path); `nrsc.dll` itself is never packaged.
- Pins for the partner: me3 **0.11.0**, Seamless Co-op for Nightreign **v1.1.3** (Nexus "1.13"), game 1.03.3.
- `launch-friend.ps1` supports `-GameDir`, `-DryRun` (prints the profile and command, writes and launches nothing)
  and `-SelfTest` (tests the `libraryfolders.vdf` parser). It writes `nrpvp-friend.me3.toml` next to itself, so the
  logs (`nrpvp.log`, `logs\`) land in the kit folder.

### PvPvE mode (T-020)
- Off by default. While on, every client rewrites its own copy of `team_type` for every player whose value is 2 (the
  expedition default) to `pvpve_team` (29), every `pvpve_interval_ms` (250). Other values (1 in the Roundtable) are never
  touched. **Every client in the session must run it.**
- **me3 path (`launch.ps1`):** edit `dist\nrpvp.toml` (created on the first launch) and set `pvpve = true`, then restart
  the game. The effective values are logged at startup (`T-008: effective config: ... pvpve=true pvpve_team=29`).
- **Nucleus path:** `scripts\deploy-nucleus.ps1` writes `pvpve = true` (use `-Pvpve:$false` to turn it off). **Friend kit:**
  `package-friend.ps1` ships `pvpve = true`.
- F12 (restore all) pauses the mode, F11 (write all) resumes it; the overlay has a "PvPvE mode" checkbox and a status
  line (`PvPvE: ON/PAUSED/OFF`, team value, automatic writes this session). `auto_team_all = N` still works as a
  deprecated alias (logs a warning).
- The local player is the first valid PlayerIns in the ChrSet (entry[0]); there is no "this is me" button any more.
- `nrpvp.log` has a `heartbeat:` line every 10 s from the main loop (T-019).

### Troubleshooting me3 + Seamless Co-op
- **me3 logs:** `%LOCALAPPDATA%\garyttierney\me3\data\logs\<profile name>\<timestamp>.log`, for example
  `...\logs\nrpvp-sc.me3\2026-10-09_15-00-00.log`. Open the newest one. It shows the resolved profile and attach
  config (check that `early_natives` lists `nrsc.dll` and `natives` lists `nrpvp.dll`), Arxan messages, and any
  `failed to load native mod` warning.
- **SC loaded through the shim:** the log should contain `loaded native with me2 compatibility shim` with
  the `nrsc.dll` path. If that line is missing, me3 did not initialise SC.
- **SC's own errors** come from SC, not me3: for example `Method "SignIn" is not available ...
  auto_control_api.cpp` (me3 issue #621, SC loaded too late) or SC's message *"If you're using ModEngine, please
  make sure seamless co-op is loaded early"*. Both point at load timing, so check that `load_early = true` is on
  the `nrsc.dll` entry.
- **Arxan:** `disable_arxan` defaults to **off** in me3 0.11.0, and the me3 docs do not say natives on Nightreign
  need it. T-001's `nrpvp.dll` already loaded fine with it off. If the game crashes at startup or hooks misbehave,
  relaunch with `./scripts/launch.ps1 -DisableArxan` (or uncomment `disable_arxan = true` in the profile) and
  compare the logs.
- **Nothing happens or the game exits:** make sure Steam is running, and that `dist/nrpvp.dll` exists
  (`./scripts/build.ps1`).
- **No `nrpvp.log`:** it is written next to the DLL in `dist/`. If it is missing, look for a native load warning in
  the me3 log.
- Do not set me3's `start_online` or `savefile` for the SC profile: SC does not need `start_online` and picks its
  own `.co2` save. (`savefile` is set only in the dev profile; see "Which save each profile uses".)

## Docs
- [CLAUDE.md](CLAUDE.md): working rules for AI agents and humans
- [TASKS.md](TASKS.md): task board
- [TECHNICAL_NOTES.md](TECHNICAL_NOTES.md): stack, findings, decisions
- [TESTING.md](TESTING.md): test cases and results
- [FUTURE_CONSIDERATIONS.md](FUTURE_CONSIDERATIONS.md): deferred ideas

## Known limitations
- PvP feasibility is unproven (experiment E-003 pending).
- SC is closed source with no API. We can only interoperate at runtime.
