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
./scripts/launch.ps1    # me3 launch -g nightreign -p profiles/nrpvp-dev.me3.toml
```

- `build.ps1` finds `cargo` on PATH or in `%USERPROFILE%\.cargo\bin`. Toolchain: stable MSVC
  (`rust-toolchain.toml`).
- `launch.ps1` finds `me3` on PATH or at `Z:\Tools\me3\bin\me3.exe`, and refuses to run if
  `dist/nrpvp.dll` is missing. It starts the game **without EAC and with official servers blocked**. Never
  launch the protected exe with mods present, and never go online modded.
- The profile `profiles/nrpvp-dev.me3.toml` loads only `dist/nrpvp.dll` (no Seamless Co-op until T-004).
- **Log file:** `dist/nrpvp.log`, next to the DLL (appended across sessions, safe to delete). Expected
  startup lines:
  - `nrpvp 0.1.0 starting (T-001 skeleton)`
  - `unix time <seconds>`
  - `nightreign.exe: base=0x... size=0x...`
  - `exe path: Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game\nightreign.exe`
  - `exe FileVersion: 1.3.3.0`
- Offline checks: `cargo clippy --all-targets`, `cargo test`, `cargo fmt --check`.

_me3 + Seamless Co-op loading: added by T-004._

## Docs
- [CLAUDE.md](CLAUDE.md): working rules for AI agents and humans
- [TASKS.md](TASKS.md): task board
- [TECHNICAL_NOTES.md](TECHNICAL_NOTES.md): stack, findings, decisions
- [TESTING.md](TESTING.md): test cases and results
- [FUTURE_CONSIDERATIONS.md](FUTURE_CONSIDERATIONS.md): deferred ideas

## Known limitations
- PvP feasibility is unproven (experiment E-003 pending).
- SC is closed source with no API. We can only interoperate at runtime.
