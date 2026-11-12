# The game's own UI files (Scaleform GFx) and how we edit them

**Owner of:** everything about editing the game's menus and HUD as files: the format, the tool (JPEXS), the workflow, what loads, what
is verified, and what is still missing for live values (a number that changes in a match). Started 2026-11-07 (Leonardo's question
after the K/D scoreboard: "can JPEXS give us a more professional UI?"). Tags: `[VERIFIED: how]`, `[SOURCE]`, `[HYPOTHESIS]`, `UNKNOWN`.
Uses: [SCOREBOARD.md](../SCOREBOARD.md) §8b (why), TASKS SB-5.

## 1. Verdict (2026-11-07)

- **Changing the look of a menu or the HUD, and adding a new static element: viable, verified, no human needed.** A text field we
  added to the HUD file showed in the game (§4). Everything runs from the command line (`ffdec-cli.exe` + a Python script); nobody has
  to click in JPEXS.
- **A new element whose value changes during the match (a live kill counter): not solved yet.** The game fills every HUD element from
  its C++ code, by name; it knows nothing about an element we add, so nothing writes into it (§5). That part is reverse engineering
  with an unknown cost; §6 is the plan.

## 2. What the files are `[VERIFIED: directory listing and JPEXS dumps of game 1.03.3]`

- `Game\menu\*.gfx` in the unpacked game (`Z:\Tools\nr-unpacked\1.03.3\menu\`, 328 files): Scaleform GFx (SWF with a `GFX` header),
  **ActionScript 3** (`DoABC2`), stage 1920x1080, 30 fps. Images are external (`DefineExternalImage2` -> `.tga` names in the `.tpf`
  texture files); the font comes from `font.swf` (`ImportAssets2`), class `MenuFont_01`.
- One file per screen. The ones that matter here:

| File | What | Notes |
|---|---|---|
| `01_000_fe.gfx` (2.3 MB) | **The in-game HUD** ("front end"): `PlayerHUD`, `EnemyTag0..7`, `RemotePCTag0..5`, `BossList`, `LockOn`, `ItemPin`, `ActionGuide`... | 27 text fields, 339 scripts; root timeline has one frame |
| `01_930_keyguide.gfx` | A button bar (`KeyGuide` text field, `StatusBar`) | **Not** the pause menu's bar (§4) |
| `02_000_ingametop.gfx` | The pause menu's top | |
| `02_013_equip.gfx` | The pause menu's Equipment screen, **with its own `KeyGuide`** | every menu screen has its own key guide (50 files contain one) |
| `01_950_systemtoast.gfx` | The toasts (our killfeed) | |
| `05_100_result.gfx`, `05_110_reward.gfx` | The end-of-expedition result and reward screens | a place for a future scoreboard screen |

- The scripts are timeline code only (`stop`, `gotoAndPlay` per frame); there are no functions the game calls. The game's C++ side
  finds elements by name and writes into them: the names are plain strings in `nightreign.exe` (`PlayerHUD`, `SoulLv`, `EnemyTag`,
  `KeyGuide`; `RemotePCTag%d` is built) `[VERIFIED: strings in the exe]`. The exe holds the Scaleform runtime (`Scaleform::Render::D3D12`,
  `ScaleformValue`, `ScaleformSwfPlayer`, `GFx_PlaceObject3Loader`...) `[VERIFIED: strings]`.

## 3. The tool

- **JPEXS Free Flash Decompiler 26.4.0** at `Z:\Tools\FFDJPEXS` (Leonardo downloaded it, 2026-11-07); needs Java (Java 25 is installed).
  `ffdec-cli.exe` is the command line, `ffdec.exe` the window (only for looking).
- It reads the game's GFx files (tags, scripts, text fields) and writes them back: **`-swf2xml` then `-xml2swf` gives a byte-identical
  file** (`01_930_keyguide.gfx`, header `GFX` kept) `[VERIFIED: cmp, 2026-11-07]`.

## 4. Workflow (verified end to end on 2026-11-07)

```powershell
$ff = 'Z:\Tools\FFDJPEXS\ffdec-cli.exe'
& $ff -swf2xml Z:\Tools\nr-unpacked\1.03.3\menu\01_000_fe.gfx fe.xml        # ~80 MB of XML for the HUD
& $ff -dumpSWF  Z:\Tools\nr-unpacked\1.03.3\menu\01_000_fe.gfx > fe.txt      # tag list with names (where is what)
& $ff -export script out Z:\Tools\nr-unpacked\1.03.3\menu\01_000_fe.gfx      # the AS3 scripts, to read
python tools\gfx\add_text_field.py fe.xml fe_mod.xml --id 713 --depth 900 --name NrpvpKills --text "KILLS 3" --x 1660 --y 100
& $ff -xml2swf fe_mod.xml 01_000_fe.gfx
# put it in the game-data package at menu\01_000_fe.gfx (dist\gamedata\menu\ for a dev test)
```

- **me3 loads a `.gfx` from our package**: its log reads `override=...\gamedata\menu\01_930_keyguide.gfx` (and `01_000_fe.gfx`)
  `[VERIFIED: me3.log of runs 1794073582 and 1794073742]`. No change to the me3 profile is needed: the package is already mounted.
- **A new text field on the HUD shows in the game**: "KILLS 3" in the game's font under the rune counter, top right
  `[VERIFIED: screenshot hud.png, run 1794073742-gfx-override-lab]`. Scenario `scenarios/gfx-override-lab.toml` (a lab: put the
  modified file in `dist\gamedata\menu\` first, remove it afterwards).
- The first try changed `01_930_keyguide.gfx` (red text, bar 100 px up) and nothing changed in the pause menu: that screen draws its
  own key guide from `02_013_equip.gfx` (TRAPS).
- The title background now has a build step (§8): `scripts\build-gamedata.ps1` generates it from the pinned game file and our image.
  Other future GFx edits should follow that pattern; never commit the game's `.gfx` itself.

## 5. Why a live value is the hard part

The HUD's numbers (HP, runes, level) are written by the game's C++ code into elements it knows by name. An element we add is never
touched by that code, so it keeps its initial text. Ways to make it live, cheapest first:

1. **Our DLL writes the text through the Scaleform runtime.** Find the HUD movie (the `GFx::Movie` / root `GFx::Value` the game uses
   for `01_000_fe.gfx`) and call the same value API the game uses (`GetMember("NrpvpKills")`, then set its text). Lead: the code that
   looks up `"SoulLv"` / `"PlayerHUD"` by name (string xrefs) holds the root value and the API calls. `[HYPOTHESIS: Scaleform 4 API
   shape]`. Cost UNKNOWN; budget one Opus session to find the root and one set-text call, stop and report if not found.
2. **Borrow a text the game already writes and that we can steer**, then show it in our element: the game writes some texts from
   pointers we can redirect (fromsoftware-rs `MenuString`: `proc_status_message`, tag `role_string`, SCOREBOARD §8b) or from FMG ids we
   rewrite at run time (verified for the blinking message). An AS3 `enterFrame` in our element could copy such a field's text. Fragile:
   the field must be one the game keeps updated while our counter should show.
3. Our element in a menu screen instead of the HUD (the pause menu, the result screen): same problem, only shown less often.

## 6. Plan (TASKS SB-5)

1. **GFX-1 (RE, Opus):** route 1 of §5: find the HUD's root value and how the game sets a text; prove it with a dev console command
   that sets `NrpvpKills` to any text in a running game (screenshot). Stop at the budget.
2. **GFX-2:** if GFX-1 works, the live counter: the patch in `build-gamedata.ps1` (from the pinned `01_000_fe.gfx` + `add_text_field.py`),
   the scoreboard writes LOCAL's numbers on a change; the pause-menu message can stay or go (Leonardo).
3. Later, same tools: a real results screen (edit `05_100_result.gfx` or add a page), the Battle Royale look of menus.

## 7. Human needed?

No, for everything above: the files are made and checked from the command line, and `nro` takes the screenshots. Leonardo only looks
at the result and decides the design (where the counter goes, how it looks).

## 8. Custom title background (2026-11-12)

`[VERIFIED: mod-only game launch, captured title screen, user confirmed "it worked"]`
`menu/05_000_title.gfx` is the title command menu. The supplied square image is stretched over its full 1920x1080 stage, behind
the original Continue / Load Game / New Game / System controls. Source image and preprocessed bitmap live in `gamedata/title/`.
The supplied `.png` filename actually held WebP; the original bytes are stored as `background.webp`.

`tools/gfx/title_background.py` embeds a lossless ARGB bitmap (`DefineBitsLossless2`) and a rectangular bitmap-filled shape
(`DefineShape3`). It inserts the background at root depth 1 and shifts the three existing root placements up one depth.
All original scripts, names, nested timelines and controls are preserved. Bitmap X/Y scaling differs intentionally, filling
the stage without letterboxing. Inline lossless bitmaps work in Nightreign's GFx runtime; a separate TPF is unnecessary.

The patcher verifies the source movie SHA-256 in `gamedata/title/background.json`, checks image/bitmap integrity and rejects
unexpected root operations or masks. `scripts/build-gamedata.ps1` runs it before copying the player package, so both developer
and player builds include the generated menu file. The vanilla game archives/executable are never patched.

To change the picture, run `python tools/gfx/prepare_title_bitmap.py IMAGE` (ffmpeg + ffprobe), then rebuild game data.
Regular builds use the committed bitmap and only need Python's standard library. The original game GFx stays out of git.
The installed test override is `Game/NightreignPvP/gamedata/menu/05_000_title.gfx`; removing that one file restores the stock menu.
Evidence and version hashes are recorded in `testing-log/2026-11-12-title-background.md`.
