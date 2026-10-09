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
_Added by T-001 / T-004._

## Docs
- [CLAUDE.md](CLAUDE.md): working rules for AI agents and humans
- [TASKS.md](TASKS.md): task board
- [TECHNICAL_NOTES.md](TECHNICAL_NOTES.md): stack, findings, decisions
- [TESTING.md](TESTING.md): test cases and results
- [FUTURE_CONSIDERATIONS.md](FUTURE_CONSIDERATIONS.md): deferred ideas

## Known limitations
- PvP feasibility is unproven (experiment E-003 pending).
- SC is closed source with no API. We can only interoperate at runtime.
