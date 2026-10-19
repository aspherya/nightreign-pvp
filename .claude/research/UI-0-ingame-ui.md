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
