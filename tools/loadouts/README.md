# Saved loadouts

Arena builds add **Save loadout** and **Load loadout** to the existing **Change sparring equipment** interaction outside in the
Sparring Grounds. This uses the existing interaction, not a newly spawned NPC. There are ten numbered slots; each shows Empty or
Saved. Saving an occupied slot asks before overwriting. Back/cancel does not overwrite anything.

A preset stores the six weapon placements (including upgrade, affinity, custom row and skill), and quick items with quantities
and their relative pouch order. Every Nightfarer can save and load every slot. Loading applies equipment to the current character,
without changing characters or opening a character selector. Existing presets from the character-specific version remain usable;
their old hero metadata is ignored and is omitted the next time the file is saved.

Loading replaces selected equipment rather than accumulating the old selection. The game manages the starting flask, relics,
armour and stats. Arena arrival still uses the existing maximum-upgrade/level-15 rules. A dedicated new NPC is not implemented.

Presets persist in `%LOCALAPPDATA%/NightreignPvP/loadouts.toml`, separately from game saves and mod updates. A malformed or newer
file disables preset operations and is preserved; the reason goes to the log. Saves stage and sync the complete file before an
atomic replacement. Presets are local to the PC and shared across its local game profiles. Matchmaking must be idle to save/load.

## Build and checks

`build-gamedata.bat -EsdTool <ESDLang-0.5.1/esdtool.exe>` compiles the dialogue, which is included in the full `dist/gamedata`
package and Arena releases. The BR-only `gamedata-player` package keeps the original interaction. Keep ESDLang's `dist/` folder
beside its executable and install the .NET 6 x64 runtime. `scripts/build-loadouts.ps1` can build the dialogue alone.

The builder checks the original NR 1.03.3 bundle's pinned SHA-256, unpacks with WitchyBND, decompiles only `t607001000`, generates
our additional state machines, compiles and repacks. All other 86 ESD payloads are checked unchanged before repacking. No game
script is committed to source control. Dialogue source is understood by ESDLang, not executed as Python.

Run `python -m unittest discover -s tools/loadouts -p 'test_*.py'` and `cargo test -p nrpvp`. The `saved-loadouts` scenario uses
`saved_loadouts sandbox` first, so it cannot overwrite real presets. It tests generic equipment restoration through the developer command; it does not prove NPC menu navigation.

## Manual acceptance (pending)

Use the pinned offline/Seamless launcher and back up the game save before the first native-inventory test.

1. At the training interaction, check that normal equipment selection still works and that the ten slots scroll on keyboard and pad.
2. Equip different weapons in all six hand slots, a custom skill, and several consumables. Save slots 1 and 10; cancel one overwrite,
   then accept another. Confirm the slot summaries refresh.
3. Change the equipment and quantities. Load twice: verify all six placements, empty hands, skill, quantities, pouch order and no
   extra selected items. Cancel loading an empty slot. Confirm a full pouch reports failure instead of success.
4. Save a slot on one Nightfarer, switch to another, and load it. Verify the current character stays selected and the weapons/items
   are restored without a character selector. Overwrite that slot from the second Nightfarer, then load it on the first one.
5. Restart the game and load both saved slots. Check that a corrupted preset file is left intact and the log explains the error.
6. Immediately queue Arena after loading. Verify the restored weapons/skills/items arrive with the existing arena upgrades, including
   after a round reset. Check that saving/loading is unavailable in a match or while matchmaking is active.

No in-game acceptance has been run for this feature yet.
