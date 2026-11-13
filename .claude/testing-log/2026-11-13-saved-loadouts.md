# Saved loadouts: implementation validation (2026-11-13)

Added ten local presets to the existing Sparring Equipment interaction for Arena builds. Presets store the current Nightfarer,
hand placements/custom skills and quick goods. Cross-character loads open the native selector and continue after selecting the
saved Nightfarer. A new NPC and automatic character switching are not implemented.

The pinned 1.03.3 Roundtable talk bundle was extracted read-only from the installed game's data archive (SHA-256
43D5005F1934A37121CA2589C58684E73964F2440B1D13BB79443AC689796CD1). WitchyBND 3.0.1.0 unpacked it;
ESDLang 0.5.1 decompiled the actual t607001000, compiled the generated additions and Witchy repacked the bundle. The other
86 ESD payloads matched their original hashes. Tool runs used .NET roll-forward because this machine has x64 .NET 8/9/10.

Rust unit tests cover slot serialization, persistence/restart/overwrite, corruption/version validation, failed writes and console
argument routing. Python tests cover generated save/load/cancel branches, text coverage and protocol bounds.

No game was launched. No Seamless Co-op or me3 runtime behavior was tested. The manual UI/inventory/character-swap checklist is
in tools/loadouts/README.md; scenarios/saved-loadouts.toml was authored with an isolated temporary preset store and has not been run.
The new native quantity-delta call is supported by static disassembly on exe 1.3.3.0, not a live call by this implementation.

Validation results:

- Developer build: 679 Rust unit tests pass; three Python menu-generation tests pass; formatting passes.
- Arena player and BR-only builds pass Clippy. Strict developer Clippy encounters the existing needless_range_loop warning in
  unchanged game/bartrace.rs:184 on Rust 1.100; the developer check passes with only that lint allowed. Cargo also reports the
  existing tracing-appender manifest warning.
- Full game-data rebuild passes: 23 files in gamedata, 21 in gamedata-player; all 15 languages include the added text entries.
- Arena release DLL builds via scripts/build-local.ps1 -Task Mod -Arena -NonInteractive and is staged as dist/nrpvp.dll.
- An extra full no-default-features + arena unit-test run had 21 failures in unchanged config/team tests that expect developer
  configuration behavior (607 passed). This is not the CI-prescribed player test filter; the new loadout-specific player tests pass.
- In-game scenario/manual acceptance remains pending. No deployment into the installed game was performed.

## Character-independent presets (user revision, 2026-11-13)

Removed character capture, matching, hero slot labels and the character selector from preset saving/loading. Every slot now
stores weapons/items only, applies to the current Nightfarer and shows Saved/Empty. Existing version-1 files with hero metadata
remain readable: that field is discarded on read and omitted on subsequent saves. Added migration coverage for legacy hero
values 1, 4, 8 and 12, and assertions that generated menus contain no GetChrHero or character selector.

Validation: 680 developer Rust tests and three Python tests pass; developer Clippy passes with the previously recorded
bartrace lint allowed; Arena player Clippy passes. The Arena release DLL and full game data rebuilt successfully. Installed
and hash-verified the DLL and 23 game-data files in the user's Game/NightreignPvP folder after confirming the game was closed.
The previous installed DLL/data were backed up in target/manual-mod-backups. Existing preset/config files were not edited.
Cross-character gameplay acceptance still needs an in-game check; no game was launched by the agent.

## In-game check on the CI release v0.1.41 (2026-11-13, Leonardo, solo)

Build: v0.1.41 from aspherya/nightreign-pvp-releases, built by the release workflow from commit a0b1b38 (main), installed by the
launcher's auto-update. Game 1.03.3, Seamless Co-op 1.1.3, me3 0.11.0. Installed VERSION.txt: nrpvp.dll CA9A5FC3A71BBEB9; the
game-data folder holds 23 files including script/talk/m10_00_00_00.talkesdbnd.dcx (checked on disk by the agent).

Before this release, every published zip lacked the feature (the player DLL was built without the old `arena` flag and the BR-only
game data had no talk bundle); Leonardo's earlier "Y does nothing" was the 2026-11-12 manual Arena build, which predates it.

Result: **PASS** (Leonardo: "funcionou certinho"). The save-loadout option now appears at the Sparring Equipment and works. The
README checklist steps were not reported one by one: cross-character load, restart persistence and the Arena hand-off remain unconfirmed.
