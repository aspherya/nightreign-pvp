# UI-0: in-game UI instead of ImGui (handoff and plan, 2026-10-19)

**Status:** started (plan + first facts). **Owner:** Opus plans; a researcher/implementer runs the steps; Leonardo gives screenshots and
in-game checks. **Card:** `TASKS.md` UI-0. **Read first:** this file, then `research/S7-mode-menu.md` (the earlier menu research: layers,
tools, ranked routes A–D), `MATCHMAKING.md` (what the menu must drive), `re/MEMORY_MAP.md` §12.

## 1. Why this exists (Leonardo's decisions)

- **No ImGui for players in the final product** (2026-10-19). Every choice and every status message lives in the game's own UI: menus,
  talk lists, on-screen messages. The Insert overlay panel is a test stop-gap; the overlay stays a developer tool.
- **We must be able to change the game's menus**: add new options, replace existing ones, change what they do.
- **Target flow** (2026-10-12, `re/PROJECT_SCOPE.md` §7): at the Roundtable table the expedition list offers **Solo / Duo / Squad Battle
  Royale** next to "Solo Mission" / "Up to 6 players". The player picks one, picks the Nightlord, starts; matchmaking runs by itself; the
  match starts by itself. No invites, no menu visits, no host action.
- **Every match has 6 players** (2026-10-19): Solo 6 × 1, Duo 3 × 2, Squad 2 × 3.
- Hard rules still apply: never EAC (`start_protected_game.exe`), never Bandai servers, never modify or redistribute Seamless Co-op
  (`nrsc.dll`, its ini, its `locale\*.json`), never touch vanilla saves, evidence tags on every claim.

## 2. Where we stand today

| Piece | Today | Must become |
|---|---|---|
| Mode choice | Insert panel → "Solo Battle Royale" (ImGui) | A choice at the Roundtable table |
| Status ("Searching 1/2", "Match found", "press Invite Members") | ImGui panel and badge | Game on-screen messages (banner / dialog / talk text) |
| Host step | host opens Matchmaking Settings > Invite Members by hand | nothing (MM-3g) |
| Join, PvP rules, match end | automatic (FS-MM3 PASS) | unchanged |

## 3. The key idea: most menu behaviour is game data, not exe code

What the Roundtable shows and does is mostly in files the game loads, which **me3 can replace with our own copies** (our package, by
game-relative path; nothing in the game folder or in SC changes):

| Layer | File (game-relative) | Holds | Precedent |
|---|---|---|---|
| Talk scripts (ESD) | `script/talk/m10_00_00_00.talkesdbnd.dcx` | The table's lists ("Expedition", …) and what each choice does. ESD `600001000` = "Expedition Menu" | Free Reign replaces the table menu with its own list and falls through to the stock menu `[SOURCE: S7 §0]` |
| Text (FMG) | `msg/<lang>/menu_dlc01.msgbnd.dcx` | Every label and message (Talk Messages 1, Event Text for Talk 33, Menu Text 200, System Messages 203, …) | 6PF adds/edits FMG entries and runs next to SC `[SOURCE: S7 §0]` |
| Event scripts (EMEVD) | `event/m10_00_00_00.emevd.dcx`, `event/common.emevd.dcx` | Logic on flags; on-screen messages | FR, 6PF ship EMEVD `[SOURCE: S7 §3]` |
| Params | `regulation.bin` | e.g. `LobbyMenuParam` (table map points), `TalkParam`, `MenuCommonParam` | FR, 6PF ship it |
| Layout (GFX, Scaleform) | `menu/*.gfx` | How native screens are drawn | Only UI-resize mods; S7 route D, last resort |

The DLL closes the loop: a script choice sets an **event flag**, the DLL sees it and acts (start the queue, etc.); the DLL can set flags that
an EMEVD/ESD reacts to (show a message). Where data cannot do it, the DLL can call the game's own functions (as MM-3f already steps around one).

**Costs** (S7 §3): overrides replace whole files, so (a) re-extract and re-apply after every game patch, (b) they clash with other mods
replacing the same files (FR, Boss Arena, 6PF's `menu_dlc01`), (c) coexistence with SC is untested for `m10` talk/event files.

## 4. First facts gathered (2026-10-19)

| Fact | Evidence |
|---|---|
| The game data is packed: `data0..3.bdt/.bhd`, `dlc01.bdt/.bhd` in `<Game>`; no loose `script/`, `msg/`, `menu/` folders | `[VERIFIED: directory listing of Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game]` |
| Oodle is present for DCX decompression: `<Game>\oo2core_9_win64.dll` | `[VERIFIED: same listing]` |
| No unpacker or ESD/FMG tool is installed on Leonardo's PC (Z:\Tools has me3, Nucleus, Me3 Manager, EldenRingSaveManager, Flawless Widescreen) | `[VERIFIED: listing of Z:\Tools and Program Files]` |
| Tool support claimed for NR: Smithbox (text, params, NR names), ESDLang `esdtool` (NR talk ESD), WitchyBND (BND4, DCX, FMG), DarkScript3 3.6.3 (NR EMEVD), Nuxe 1.2.1 (unpack archives; **unpack only, never its exe patcher**) | `[SOURCE: S7 §3 "Tools"]`; none tried by us |
| Engine singletons for menus/talk exist: `CSMenuMan` exe+0x3C149D8, `MsgRepository` exe+0x3C246A8, `CSWorldTalkMan` exe+0x4433DF0, `CSResidentTalkFileMan` exe+0x4433A18 | `[VERIFIED: TC-004 log, S7 §1.1]` |
| Lobby flags exist: `10002700` "For lobby menu: Matching menu", `10002701` hero selection, `69990` lobby multiplayer start | `[SOURCE: Smithbox EFID lists, S7 §1.1]` |

## 5. Questions UI-0 must answer

| # | Question | Why it matters |
|---|---|---|
| Q1 | Can we unpack the game, and decompile + recompile (round-trip, byte-identical where possible) the table's talk ESD and the menu FMGs? | Everything below depends on it |
| Q2 | In ESD `600001000`, what opens the stock expedition menu? Can a list choice be added before it ("Battle Royale" → Solo/Duo/Squad) that sets a flag and then opens the same menu? | UI-1 (mode choice at the table) |
| Q3 | Which ESD talk commands exist in NR that open other menus (matchmaking settings, invite, party)? Is there one that does what Invite Members does? | MM-3g zero-click, maybe without RE of exe code |
| Q4 | How does NR show on-screen text? Candidates `[HYPOTHESIS: ER names]`: EMEVD `DisplayBanner`, `DisplayGenericDialog`, blinking/system messages; ESD talk text and generic dialogs; FMG System Messages (203). Can text change at run time (e.g. "Searching 3/6")? | UI-2 (status without ImGui) |
| Q5 | Can the DLL read and write event flags in NR? (`CSEventFlagMan` layout is ER-based in T-027, UNVERIFIED) | The data ↔ DLL channel |
| Q6 | Does SC still work with our `m10` talk/event files (its party list at the table, Invite Members, joining, expedition start)? | Go / no-go for the whole route |
| Q7 | Is the party-size choice (Solo Mission / Up to 6 players) a talk list, a native screen, or both? What text ids does SC override (shadowing)? | Where "Battle Royale" can live (S7 routes B vs C) |
| Q8 | What does a game patch do to our files (diff after the next update)? | Maintenance cost |

## 6. Plan (steps in order; each ends with a written result in this file)

**Step 1, toolchain (needs Leonardo's OK to download).** Pick and install, into `Z:\Tools\` (never the game folder): an unpacker (Nuxe
unpack-only, or Smithbox's own unpack if it has one; decide by checking each project's current NR notes), WitchyBND, ESDLang `esdtool`,
DarkScript3. Record versions and download URLs here. Smithbox is a GUI: Leonardo runs it if needed.

**Step 2, extract read-only copies** into a scratch folder (e.g. `Z:\Tools\nr-unpacked\1.03.3\`), never over the game: `script/talk/m10_00_00_00.talkesdbnd.dcx`,
`msg/engus/menu_dlc01.msgbnd.dcx` (and `menu.msgbnd.dcx` if it exists, Q7 in S7), `event/m10_00_00_00.emevd.dcx`, `event/common.emevd.dcx`,
and list `menu/` (GFX names only). Record file sizes and hashes. Nothing extracted goes into git (game content); only our notes and, later,
our own edits as diffs.

**Step 3, map the table (Q2, Q3, Q7).** Decompile all ESDs in the m10 talk bundle. Document ESD `600001000`: its states, the talk list
entries and their text ids, the call that opens the expedition menu, and every other "open menu" call in m10 (list them with ids).
Search FMG text for "Solo", "Up to", "Expedition", "Matchmaking", "Invite", "player"; record FMG + id. Leonardo: screenshots of the table
interaction, the expedition menu under SC, and Matchmaking Settings (`.claude/experiments/UI-0/`).

**Step 4, messages (Q4).** From EMEVD (DarkScript3) and ESD, list the instructions that put text on screen and the FMG each uses. Pick
the best candidate for live status lines and for "Match found" style prompts.

**Step 5, flags (Q5).** Verify the NR `CSEventFlagMan` layout read-only from the DLL (dev build): read known lobby flags `10002700..10002707`
while Leonardo opens the menus; then pick a free flag range for us (S7 suggests `11xxxxxx`; verify unused).

**Step 6, smallest proof (Q1, Q6), solo.** Round-trip the m10 talk bundle unchanged → load it through our me3 package → table works,
SC works (Leonardo solo: table, expedition start; then a friend join). Then the smallest real edit: a new entry "Battle Royale (test)"
in the table list that sets our flag and opens the stock menu; the DLL logs the flag. Back up `%APPDATA%\Nightreign\<id>\` first.

**Step 7, write the UI-1 / UI-2 / MM-3g cards** from what steps 3–6 found.

## 7. Done when

UI-0 is done when Q1–Q7 each have an answer with an evidence tag (or a clear UNKNOWN plus the next experiment), step 6 has passed or failed
with a reason, and cards UI-1 (mode choice at the table), UI-2 (in-game status) and an updated MM-3g are written in `TASKS.md`.

## 8. Risks and guard rails

- Modified game files only through our me3 package and launcher; never in the game folder; never with `start_protected_game.exe`.
- Extracted game files are third-party content: keep them out of git and out of release zips except the specific files our mod must
  override (as FR and 6PF do). Before a public release, review that with SC's author question (S9 §8).
- SC coexistence is the main unknown (Q6). If an `m10` override breaks SC, fall back to: text-only changes (FMG), EMEVD-only messages,
  and DLL-driven calls of game functions.
- Patch fragility: pin every extracted file to game 1.03.3 (exe 1.3.3.0) with hashes.

## 9. Results log

(append findings per step here, newest last)

### Step 1, toolchain (2026-10-19, done)

Leonardo approved the full standard set (asked for the best-documented tools). Each tool is in its own folder in `Z:\Tools\`; zips stay in
`Z:\Tools\_downloads\`. Every download comes from the project's GitHub "latest release". The SHA-256 below is the zip's.

| Tool | Folder | Release / asset | SHA-256 (zip) | Role |
|---|---|---|---|---|
| Nuxe 1.2.1 | `Z:\Tools\Nuxe-1.2.1` | github.com/JKAnderson/Nuxe v1.2.1 `Nuxe.1.2.1.zip` | `023D3C2A…78267A` | Unpack archives. **Unpack only; never Patch/Restore** |
| WitchyBND 3.0.1.0 | `Z:\Tools\WitchyBND-3.0.1.0` | github.com/ividyon/WitchyBND 3.0.1.0 `-win-x64.zip` | `A3E6B2A0…AF50364` | BND4/DCX/FMG on the command line |
| ESDLang esdtool 0.5.1 | `Z:\Tools\ESDLang-0.5.1` | github.com/thefifthmatt/ESDLang v0.5.1 `esdtool-v0.5.1.zip` | `DEB613A5…49CB91C` | Talk ESD decompile/compile |
| ESDLang dump 0.5.1 | `Z:\Tools\ESDLang-0.5.1-esd-dump` | same release, `extra-all-esds-dump-v0.5.1.zip` | `3F74955F…78CC9DE` | Pre-decompiled ESDs incl. `nr\` (142 files) |
| DarkScript3 3.6.3 | `Z:\Tools\DarkScript3-3.6.3` | github.com/AinTunez/DarkScript3 3.6.3 `Darkscript3_6_3.zip` | `84FE6E2B…F49638FCA` | EMEVD (step 4) |
| Smithbox 2.2.6 | `Z:\Tools\Smithbox-2.2.6` | github.com/vawser/Smithbox 2.2.6 `Smithbox_2_2_6_2026_09_20_b.zip` | `9A147C31…19994B3D7` | Text/params editor (GUI, Leonardo); fallback |

Full hashes: `Get-FileHash Z:\Tools\_downloads\*.zip`. .NET 6/8/9/10 runtimes are already installed `[VERIFIED: dotnet --list-runtimes]`.

**Unpack without touching the game folder.** Nuxe's Basic unpack writes next to the exe, so `Z:\Tools\nr-unpacked\1.03.3\` holds NTFS
**hard links** to `data0..3` and `dlc01` `.bhd/.bdt` (no extra space; unpacking only reads them) `[VERIFIED: fsutil hardlink list]`. Run Nuxe's
Advanced unpack on that folder. Game exe `1.3.3.0` `[VERIFIED: FileVersion]`. Do not edit the linked archives: they are the game's files.

### Step 3 head start from the ESDLang dump (2026-10-19)

Source: `extra-all-esds-dump-v0.5.1\nr\` (published 2025-08-16, so its game version may predate 1.03.3; recheck against our unpack).
All facts below are `[SOURCE: ESDLang v0.5.1 NR dump]` until rechecked.

- **Q2: ESD `t600001000` (the table) has no talk list.** When the "Commence expedition" action button (6900) is pressed, it calls
  `OpenCommenceExpeditionMenu()` and waits while menu type 34 is open. Flag `10002700` ("For lobby menu: Matching menu") is used as the "menu
  requested" input, and the script turns it off afterwards. Only the Executor hero gets a short talk line first (`x5`).
  ⇒ The Solo/Up-to-6 choice lives **inside the native menu 34** (Q7: a native screen, not a talk list). A talk list ("Battle Royale" / "Expedition")
  can still be put **before** `OpenCommenceExpeditionMenu()`. Other NR ESDs build lists with `AddTalkListData*` + `OpenConversationChoicesMenu`
  (30 uses), so the commands exist in NR.
- **Q3: no ESD command opens matchmaking settings, invite, or the party screen.** NR scripts use these menu openers: CommenceExpedition,
  ChangeCharacter, Relic, VisualCodex, Journal, Jar/Garb/Enhance/Regular/Catalog/LegendaryEnhance shops, ReplicateArmament, SparringEquipment,
  SparringSettings, ConversationChoices, GenericDialog. esdtool's full talk command list (all games) has nothing named for matchmaking, party, or
  invite either. NR also uses **unnamed** commands (`c1_163..181`, `c5_166..172`, `f301..316`); any of them could be online-related: UNKNOWN.
  ⇒ MM-3g stays a DLL job (call the game function) unless one of those unnamed commands turns out to be it.

### Step 2, extraction (2026-10-19, done)

Leonardo ran Nuxe 1.2.1 Advanced → Unpack on `Z:\Tools\nr-unpacked\1.03.3\`: 32,228 files plus folders `action … sound`. The game folder is unchanged:
it has no new folders and no file newer than 10/10 `[VERIFIED: listing after unpack]`. `regulation.bin` is also hard-linked there, because esdtool
loads params for names (SHA-256 `876A3CA2…2C91E0268`). Work copies are in `Z:\Tools\nr-unpacked\work\` (`esd\`, `msg\`, `rt\`). None of this goes into git.

| File (game 1.03.3, exe 1.3.3.0) | Bytes | SHA-256 |
|---|---|---|
| `script/talk/m10_00_00_00.talkesdbnd.dcx` | 335760 | `43D5005F1934A37121CA2589C58684E73964F2440B1D13BB79443AC689796CD1` |
| `script/talk/m10_00_00_00_dlc01.talkesdbnd.dcx` | (also present; 32 ESDs) | not hashed yet |
| `event/m10_00_00_00.emevd.dcx` | 23232 | `A4D87CAEBE834496F66B958E1FCDE0190006CB97ACE6805AB1A02A00C642D55B` |
| `event/common.emevd.dcx` | 13056 | `0018635B07959514CD9F1DF4E4889959B649C04E175567E6D8A8E2F11B0E2805` |
| `msg/engus/menu_dlc01.msgbnd.dcx` | 158976 | `E33F1C338878DA423D95CF54589072D99425F2A92C10603E5C93BDA9323184DB` |
| `msg/engus/item_dlc01.msgbnd.dcx` | 209904 | `A4F14491D8D3F34BAAF9CC57ECE763BA433B3647C40B2817068F36B926ADA0F2` |

- **S7 open question 7 answered:** `msg/engus/` holds only `item_dlc01`, `menu_dlc01`, `ngword` msgbnds. There is **no** base `menu.msgbnd`
  `[VERIFIED: unpacked listing]`. Every menu string is in `menu_dlc01`.
- **Lobby GFX** (names only, `menu/`): `02_170_lobbymenutop`, `02_171_lobbyselect`, `02_200_matching`, `02_230_matching_selectoption`,
  `02_600_heroselect`, `03_063_talkchoice`, `01_010_messagebox`, `01_011/013_messagebox_small(_nb)`, `01_950_systemtoast`,
  `01_080_emergencynotice`, `01_090_summonmessage`, `01_060_caption` `[VERIFIED: unpacked listing]`. The messagebox, systemtoast and caption files
  are candidates for Q4.

### Step 3, the table (2026-10-19, tools part done; Leonardo's screenshots pending)

Tools: esdtool needs `oo2core_6_win64.dll` (copied from `ELDEN RING\Game` into the esdtool folder) and `-basedir` with `regulation.bin`.
Command: `esdtool.exe -nr -basedir Z:\Tools\nr-unpacked\1.03.3 -i <talkesdbnd> -writepy <dir>\%e.py`. Do not pipe it into `select -First`,
because that kills it partway.

- **Q1 (round-trip): yes, semantically.** `m10_00_00_00` decompiles to 87 ESDs and `_dlc01` to 32. Recompiled `t600001000` is 9356 bytes vs 9674
  original, so **not byte-identical**, but decompiling it again gives the same script (ignoring the name and whitespace) `[VERIFIED: rt\ diff]`.
  FMG via WitchyBND: `CL_MenuText.fmg` repacks to 90628 vs 90632 bytes. The only change is entries 3020/3021 (" " → ""). Witchy trims
  whitespace-only text `[VERIFIED: rt\fmg xml diff]`. Use Smithbox for FMG edits if that matters, or keep those entries out of our edits.
  The in-game proof is still step 6.
- **The 2025-08 dump vs 1.03.3:** `t600001000` is identical. `t100401000, t101001000, t201001000, t605001000, t617001000, t618001000` changed, and
  the whole `_dlc01` bundle is new `[VERIFIED: hash compare]`. Use our own decompile from now on.
- **Q2 confirmed on 1.03.3:** see the head start above. Pattern for a list, as the game itself does it (`t121201000`):
  `ClearPreviousMenuSelection(); ClearTalkListData(); AddTalkListDataAlt(1, <EventTextForTalk id>, -1, 0, False); …;
  OpenConversationChoicesMenu(0); if GetTalkListEntryResult() == 1: …`. Yes/no prompts use `OpenGenericDialog(DialogBoxType.CenterBottom1, <id>,
  DialogResult.Left, DialogBoxStyle.OrnateNoOptions, 1)`. So the step 6 edit is: in `t600001000_x3`, before `x4`, show a list
  "Battle Royale (test)" / "Expedition". The first sets our flag, then both call `x4()`.
- **Q3 confirmed on 1.03.3:** menu openers in m10 + dlc01 are CommenceExpedition, ChangeCharacter, Relic, VisualCodex, Journal, Jar/Garb/Catalog shops,
  SparringEquipment, SparringSettings, ConversationChoices, GenericDialog. There is no matchmaking, invite, or party opener.
- **Q7: the party-size choice is native menu 34** (`OpenCommenceExpeditionMenu`, GFX probably `02_170/171/230`, `[HYPOTHESIS]` which one).
  Vanilla text `[VERIFIED: CL_MenuText]`: 130407 "Number of Players", 130409 "One player", 130417/130200 "Two players", 130408/130201
  "Three players", 130410 "Matchmaking Settings", 130401 "Invite Members", 130300/130302 "Commence Expedition", 130304 "Commence Solo
  Expedition", 130305 "Commence Two-Player Expedition", 40340 "Perform Matchmaking", 40341 "No Matchmaking", 336900 "Matchmaking...",
  336901 "Matchmaking complete", 336902 "Ready to commence expedition", 336904 "Matchmaking cancelled". CL_Dialogues: 506011 "Matchmaking has
  been canceled", 506012 "Cancel matchmaking?", 506020–22 "Matchmaking failed…". CL_LineHelp 701005 "Invite players to team session".
  ActionButtonText 1882 "Commence expedition". Full hit list: `work\msg\search.tsv` (211 rows).
  **SC shadowing:** `SeamlessCoop\locale\english.json` has 4 keys only: `MODMESSAGE_YUINEXUSBEGINONEPLAYER` "Solo Mission",
  `…TWOPLAYER`/`…THREEPLAYER` "Up to 6 players", `…PLAYERCOUNTREMAINING` "{} / {}" `[VERIFIED: file read]`. So "Solo Mission / Up to 6
  players" is SC text set at run time, over vanilla ids `[HYPOTHESIS]`: 130304/130305 + a three-player id, or the 130409/130417/130408
  set. An FMG edit to those ids would be hidden under SC. Our "Battle Royale" label must therefore be a **new** id (talk list, `EventTextForTalk`), not those.
- **Leonardo, still needed for step 3:** screenshots into `.claude/experiments/UI-0/`: (1) "Commence expedition" prompt at the table,
  (2) the expedition menu under SC, (3) Number of Players list, (4) Matchmaking Settings, (5) the "Matchmaking…" status while searching.

### Step 5, flags and text ids (2026-10-19, built; Leonardo's run pending)

Leonardo's screenshots (`experiments/UI-0/s0`, `s3`, `s4`) are kept but **not read**, to save tokens (Leonardo, 2026-10-19). The text-id
logger below replaces them and gives exact ids, which a picture cannot.

**Text lookup found** (static, game 1.03.3 / exe 1.3.3.0; scripts `experiments/UI-0/msgrepo_xrefs.py`, `disfunc.py`):
- 140 code sites load the `MsgRepository` slot `exe+0x3C246A8`. Per-category getters (e.g. `exe+0xE5B250`, called with `edx = 130011`
  "Please set group password" and `110330` "Enter player name") call **`exe+0x27F4D20` = `LookupEntry(repo, slot, category, id)`**
  `[VERIFIED: disassembly]`: bounds `slot < [repo+0x10]`, `category < [repo+0x14]`, FMG = `[[repo+8] + slot*8] + category*8`, then a tail jump to
  **`exe+0x27F5710`, the FMG search `(fmg, id)`**. Same shape as ER `MsgRepositoryImp::LookupEntry` `[SOURCE: fsrs msg_repository.rs]`.
- **`category` is the FMG file id inside the msgbnd** `[VERIFIED: callers pass 203, 210, 400–428, 504–515; same ids as the WitchyBND
  manifests]`: 1 TalkMsg, 32 ActionButtonText, 33 EventTextForTalk, 200 CL_MenuText, 201 CL_LineHelp, 203 CL_System_Message_win64,
  204 CL_Dialogues, 500+ the `_dlc01` twins (504 CL_MenuText_dlc01). Full list in `game/texttrace.rs::category_name`.
- Getter `0xE5B250` tries category 504 first and falls back to a second, **obfuscated** path (`exe+0xF9C630` via another global,
  `edx = 200`). So not every lookup goes through `0x27F4D20`, which is why the FMG search `0x27F5710` is traced too.

**Built (dev only), nothing patched:**
- `game/texttrace.rs`, config `ui_text_trace = true`: hardware execute breakpoints (DR0 `0x27F4D20`, DR1 `0x27F5710`) plus a VEH, the same
  family as MM-3f. Before arming it checks the entry bytes; a jump there (an SC hook) is logged with the target module, and that answers
  "does SC hook `LookupEntry`?" directly. Each `(category, id)` is logged once: `ui_text_trace: TEXT <FMG> <cat> id <id>`. FMG-search
  hits are mapped back to a category through the repository table. It runs 20 min, then disarms. It takes the debug registers, so it
  forces `mm_quiet_invite` / `mm_lobby_trace` off.
- `probe2.rs` flag watch (needs `probes = true`): adds 6000 (always OFF) / 6001 (always ON) as a check of the ER layout on NR, plus 10002700..07,
  10003045, 69990. Once per manager instance it logs **every flag group that has storage** (`flag groups with a descriptor …`), for
  picking our own flag range (a flag in a group with no block cannot be set).
- `experiments/UI-0/map_text.py <nrpvp.log>`: prints each traced id with its English text from the extracted FMGs, plus the MARK
  lines. Ids that come out `<not in FMG>` are candidates for SC's own strings.
- Build: `cargo test` 425 pass (1 ignored); clippy clean; `dist\nrpvp.dll` built, and `dist\nrpvp.toml` has `ui_text_trace = true`, `probes = true`.

**Runbook for Leonardo (solo, about 5 min; the game may stutter in menus while the trace runs, which is expected):**
1. `.\scripts\launch.ps1` (SC + dev build). Load into the Roundtable.
2. Press **F10** (MARK), wait 3 s. (Baseline: what the Roundtable itself looks up.)
3. **F10**, go to the table, press "Commence expedition", wait on the expedition screen 5 s.
4. **F10**, open **Number of Players**, move over each option, close it.
5. **F10**, open **Matchmaking Settings**, move over each entry (do not press Invite Members), close it.
6. **F10**, start a **Solo Mission** expedition and let it load (this records the start and the expedition flags).
7. Quit the game. Tell Claude "step 5 run done". Claude reads `dist\nrpvp.log` through `map_text.py`.

#### Step 5 results (Leonardo's run, 2026-10-19 21:51–21:54 UTC, SC + dev build, game 1.03.3)

Read with `map_text.py dist\nrpvp.log`: 234 unique texts, 0 dropped, all through `LookupEntry` (FMG-search hits were repeats).
- **SC hooks `LookupEntry`** `[VERIFIED: entry bytes]`: `E9 BC C2 5F FD` (a 5-byte jump, the rest of the function unchanged) to `0x7FF704910FE1`,
  which is in no module, so a hook trampoline. Which DLL put it there is `[HYPOTHESIS: SC]` (S7: SC wraps the message repository);
  following the trampoline names the owner (next trace run). Our breakpoint at the entry still fires, before SC's code.
- **Every lookup asks the `_dlc01` FMG first, then the base one** (504 then 200, 505 then 203, 507 then 33) `[VERIFIED: log order]`.
- **Q7 answered:** the expedition screen looks up 1882 "Commence expedition" (ActionButtonText), then CL_MenuText 102000 "Expeditions",
  130416 "Target", 130410 "Matchmaking Settings", 130404–130406 passwords, 130300 "Commence Expedition". **Number of Players** looks up
  **130409 "One player", 130417 "Two players", 130408 "Three players"**, 130407, LineHelp 701010. Leonardo's screen shows "Solo Mission" /
  "Up to 6 players" there, so SC's `LookupEntry` hook swaps exactly those ids `[VERIFIED: ids looked up; HYPOTHESIS: SC swaps 130409/130417/130408,
  the only labels on that list]`. Matchmaking Settings: 130400 "Team Session", 130401 "Invite Members", 130200/130201, 130002, LineHelp 701002/5/6/7.
  Start: 130304 "Commence Solo Expedition" / 130305 "Commence Two-Player Expedition", then 336902 "Ready to commence expedition".
  In me3 the game is **offline**: choosing Two players shows CL_Dialogues 506023 "Cannot start multiplayer session in offline mode…" (SC's
  "Up to 6 players" path does not go through that check).
- **Q5 answered: reading flags works** `[VERIFIED]`: the ER layout's checks all pass; 6001 "always ON" went ON when the save loaded and
  6000 stayed off; 7500 (timeline) went ON when "DAY I" showed. The lobby flags 10002700..07 / 10003045 / 69990 never read ON (sampled
  once per second: a short pulse would be missed). Divisor 1000; **143 groups have storage**: `0..9 60..99 1009 1019 … 1209 (every 10)
  10000..10005 10007..10009 11000..11005 11007..11009 12000..12005 12007..12009 16000..16005 16007..16009 16100..16105 16107..16115
  16117..16119 18000..18005 18007..18009 19000..19005 19007..19009`.
- **Our flag range: group 10007 (flags 10007000..10007999)** `[VERIFIED: has storage; 0 references in all 191 NR talk ESDs and in the raw
  u32 words of all 278 EMEVDs; DarkScript3's NR flag names list nothing there]`. 10008 is the same (spare). Still UNKNOWN: whether the exe or
  params use it, and whether it is saved to the `.co2` (step 6 shows the latter).

### Step 4, on-screen text (2026-10-19, answered from data, no GUI needed)

- **EMEVD** (DarkScript3 `nr-common.emedf.json` + a parse of all 278 decompressed EMEVDs): NR has the ER message instructions:
  2007[1] Display Generic Dialog, [3] Display Status Message, [4] Blinking Message, [9] Full Screen Message, [10] Generic Dialog and Set Event
  Flags, [12] Blinking Message with Priority, [16] Network Message, [15] Show Tutorial Popup, [19] Text Effect, and 2003[111] Show Floating Message.
  The game uses: Generic Dialog 11× (m10: message ids 70010..70100), Generic Dialog + flags 4× (m10, 70040/70100), Blinking 1×, Tutorial
  Popup 114×, Text Effect 86×, Floating Message 15× `[VERIFIED: instruction table parse]`. **The m10 Generic Dialog ids are EventTextForMap
  (FMG 34)** ("Hark, the murmuring of waves…"), the FMG 6PF adds entries to.
- **ESD**: `OpenGenericDialog(type, <id>, …)` (64 uses), `TalkToPlayer`, talk lists (`AddTalkListData(n, <EventTextForTalk id>, -1)`).
- **Pick for UI-2:** fixed messages as **our own EventTextForMap entries**, shown by an EMEVD event that waits on our flag (group 10007), and the
  DLL sets the flag (no hook). "Searching 3/6" = one entry per count (6 entries). Live text without fixed entries would need the DLL to point
  one of our FMG entries at its own string `[HYPOTHESIS: FMG string offsets are 64-bit and relative, as in DS3-format FMGs]`, or a hook like
  SC's, which we avoid. Whether a Generic Dialog / Status Message draws in the Roundtable and during matchmaking: UNKNOWN until UI-2.

### Step 6, smallest proof (2026-10-19, built; Leonardo's run pending)

Built from the 1.03.3 unpack; packages in `dist\` (gitignored, game content); save folders backed up to `Z:\Tools\save-backups\2026-10-19-ui0-step6`.
- **6a** `dist\ui0-pkg-a\script\talk\m10_00_00_00.talkesdbnd.dcx` (340304 bytes): esdtool recompile of the **unchanged** `t600001000` (the other
  86 ESDs passed through). Profile `profiles\nrpvp-ui0a.me3.toml` = SC profile + `[[packages]]`.
- **6b** `dist\ui0-pkg-b`: the talk bundle with `t600001000_x6`, a list "Battle Royale (test)" (EventTextForTalk 99100000) / "Expedition"
  (99100001) shown after "Commence expedition". Choice 1 sets **flag 10007000 ON**, choice 2 sets it OFF; both then open the stock menu
  (`x4`); cancel opens nothing. Plus `msg\engus\menu_dlc01.msgbnd.dcx` with those two entries added. Only `EventTextForTalk.fmg` differs
  from vanilla `[VERIFIED: unpack + byte compare of all FMGs]`, so the 3020/3021 whitespace issue does not apply. Edited source:
  `Z:\Tools\nr-unpacked\work\ui0-step6\b\t600001000.py`. The DLL watches 10007000 (probe line `flag 10007000: off -> ON`).
- `scripts\launch.ps1 -Profile ui0a | ui0b`.

**Runbook for Leonardo (solo, about 6 min):**
1. `.\scripts\launch.ps1 -Profile ui0a`. Roundtable loads? Go to the table, "Commence expedition": does the normal expedition menu
   open? Start a Solo Mission; does it load? Quit.
2. `.\scripts\launch.ps1 -Profile ui0b`. At the table, "Commence expedition": is there a list "Battle Royale (test)" / "Expedition"?
   Pick **Battle Royale (test)**: does the expedition menu open? Close it. Again, pick **Expedition**: same? Once more and press Back on the
   list. Then start a Solo Mission from the menu; does it load? Quit.
3. Tell Claude what you saw in each. Claude reads `dist\nrpvp.log` (flag 10007000 lines, `ui_text_trace` ids 99100000/1, me3 package lines).
If the game crashes or the table does nothing, quit and say at which step. The vanilla files are untouched; the next normal launch is unaffected.
