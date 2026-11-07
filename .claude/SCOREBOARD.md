# K/D scoreboard (owner doc)

**Owner of:** the kill / death scoreboard: what counts as a kill and a death, what each player sees during and after a match, how
every game gets the numbers, the UI options and their cost, the plan (TASKS "Scoreboard"). Started 2026-11-07 (design only, nothing built).
The killfeed it builds on is owned by [re/TOAST_KILLFEED.md](re/TOAST_KILLFEED.md); bots by [BOTS.md](BOTS.md); match rules by
`features/br_match/` and [re/PROJECT_SCOPE.md](re/PROJECT_SCOPE.md). Tags: `[VERIFIED: how]`, `[SOURCE]`, `[HYPOTHESIS]`, `UNKNOWN`.

## 1. What Leonardo asked (2026-11-07)

1. **During a match: own numbers only.** A player sees their own kills and deaths, updated as they happen, and nobody else's.
2. **When the match ends: everyone.** The kills and deaths of every participant: players, and the bots in the bots training mode.
3. Bots count like players (rivals named "Bot N").

**Hard requirement (Leonardo, 2026-11-07): as integrated in the game's own UI as possible.** The two shapes he wants: (a) press a
button and the game shows a text / banner with your kills, or (b) a number always on screen. Mockups of both over a real screenshot
were shown to him (2026-11-07). **His choice (2026-11-07): option C, the game's centre message; then "every time the player opens
the menu (Esc / pad Start) they see their kills"** instead of a separate key. Deaths count mob and fall deaths too (Claude's
recommendation of Q1, built that way; Leonardo did not object). Priority: the scoreboard first; then Arena (MODE-02, designed separately); the match timer is parked.

## 2. What already exists (the scoreboard adds no new detection)

- **Every rival kill and tide death is already an event on every game.** `br_match/kill_report.rs` raises a `KillEvent` on the victim's
  game (`Downed`, `Killed` with the last rival who hit, `Tide`); `features/killfeed.rs::report` publishes it on `net::bus` topic
  `KILL_FEED` and every other game receives it in under half a second `[VERIFIED: mi-killfeed PASS 2026-11-03, both logs]`.
- **Only the victim's game knows the attacker** (`PlayerIns+0x1A0`, MEMORY_MAP §7). So a kill is credited by the victim's game, never by
  the killer's. `[VERIFIED: MEMORY_MAP §7, kill_report.rs]`
- **Bots' deaths are local events** (`killfeed::local_event`, bots live on this game only), with the killer named when it is LOCAL or
  another bot; a bot killed by a mob has no event and is replaced by a new bot with a new handle, so today a new name ("Bot 6")
  `[VERIFIED: bots/fill.rs kill_event, bots-match PASS 2026-11-07]`.
- **LOCAL's mob / fall deaths are judged** (`defeat.rs`, `MatchBook::mob_deaths`, console `night_status` "mob deaths N")
  `[VERIFIED: defeat-mob scenario]` but are **not** on the bus: other games do not know about them.
- The `Feed` in `killfeed.rs` already keeps names (`PlayerGameData+0x9C`, "Bot N"), teams and who is out, and drops a second report
  of the same player's defeat.
- Bus fact format: key = victim << 16 | seq << 2 | kind (2 bits: 0 Downed, 1 Killed, 2 Tide); **kind 3 is free** and an older build
  decodes it as `None` and ignores it `[VERIFIED: killfeed.rs decode + its unit test decode(3, 0) == None]`.

## 3. Counting rules (proposed)

| Thing | Rule | Why |
|---|---|---|
| **Kill** | +1 for the rival named in a `Killed` event: the last rival (player or bot) who hit the victim in the 30 s before the death (`defeat::RIVAL_WINDOW_MS`). Exactly the killfeed's "A killed B". | One rule for the feed and the board: they can never disagree. No finishing blow in our rules (a third player who touches a downed player last gets the kill; KF-1 design). |
| Down | **Not a kill and not a death.** Not shown in v1. | A downed player can be revived (Duo/Trio). A "knocks" column is a later idea. |
| Tide death | Death +1 for the victim, **nobody** gets a kill. | No rival did it. |
| Mob / fall death (respawns) | **Recommended: death +1** (see question Q1). Nobody gets a kill. | Under the current rules a rival's kill or the tide puts you out for good, so "deaths" from rivals alone is always 0 or 1 per match: a K/D with D in {0, 1} says little. Counting every death gives D a meaning (how often the PvE punished you). The alternative (D = out by rival/tide only) is listed in Q1. |
| Teammate | Never credited: `rival_id` only records rivals (`game/combat_state/mod.rs`, `attacking_rival`), so a teammate cannot get a kill on a teammate. A Duo/Trio team's kills = the sum of its members'. | |
| Left the match (Alt-F4, F10 before being out, crash) | The leaver is marked "left" in the end summary; **no kill for anyone** (the victim's game is gone, nobody else knows the last attacker). | Honest flaw: a rage-quit denies the kill. Fixing it needs every game to keep "who hit whom", a bigger change; not worth it now. |
| Bot | Same rules. A bot killed by LOCAL or another bot: kill to that killer, death to the bot. A bot killed by a mob: death +1 (if Q1 = every death) and **the replacement bot takes over its row and name** ("Bot 2" comes back), like a player's respawn. | Parity with players (BOTS §2d). Today the replacement gets a new name: a small change in `bots/fill.rs` (the round armed for a mob death hands its first kept handle the dead bot's slot). |
| A bot kills LOCAL | Kill to the bot, death to LOCAL. | Already in the event (`bots-match`). |

Ratio: the board shows **counts** ("3 kills, 1 death"), not a ratio; with D = 0 a ratio is undefined and in one match it means little.

## 4. Where the numbers come from (every game, no new message for kills)

- A pure `Tally` (new module `features/scoreboard.rs`): per short id, kills, deaths, out (yes / no, cause), left. It is fed **every event the
  `Feed` accepts** (local events, bus facts, bot events), in `KillFeed::tick`, so the bus topic keeps one reader. Same dedupe as the feed
  (a defeat reported twice counts once).
- Because the victim's game publishes every kill and every game receives it, **every game can build the full table by itself**; the
  numbers agree on all games as long as the bus delivers every fact (`[HYPOTHESIS]`: no fact loss seen so far; a lost fact = one game
  shows one kill fewer).
- **Mob / fall deaths (only if Q1 = every death):** the victim's game publishes one more fact, kind 3 `Died` (no killer), when `Defeat`
  judges a death that is not a defeat. It gives no toast line on the killfeed (the feed ignores it) and an older build ignores it.
- The table freezes when the expedition ends (the killfeed resets there today; the scoreboard keeps a copy as "last match" for the end
  summary). A player who leaves the session early (defeat screen, F10) stops receiving facts, so their copy is "as of when you left".
- Friends + bots later: bot events would have to be published too (bots live on one game). Not needed for Solo vs bots.

## 5. In-match UI: own numbers only

Constraint: **no ImGui for players** (DECISIONS 2026-10-19). Today the mod talks to players through three channels: the game's toast
(`game/gametoast.rs`, any text, 120 UTF-16 units with markup `[VERIFIED: KF-1]`), the blinking message (EMEVD 2007:4, fixed text from
`gamedata/fmg-entries.json`, e.g. the spectator hint 99100806) and a dialog box (CL_Dialogues 506040 through the Commence handler,
Roundtable menu only, `game/menuflow.rs`).

| Option | What the player sees | Cost | Risk / flaw |
|---|---|---|---|
| **A. Own toast on change (recommended)** | After each of your kills or deaths, on your screen only, one more toast in gold: `Your score: 3 kills · 1 death`, right after the killfeed line. | Small: `gametoast::show` exists; ~40 lines + tests. | Not persistent: you see the number when it changes, not on demand. Toasts stack and fade by themselves. |
| B. Own count on the spectator hint | The hint a defeated player sees every 45 s gets `You: 3 kills · 1 death`. | Small after the SB-4a spike (needs a text rewritten at run time in EventTextForMap_dlc01, the same technique as the toast's placeholders). | Only once you are out. A nice extra, not a counter. |
| **C. Button → the game's banner (Leonardo's shape a)** | A key / pad button shows `Kills: 3 · Deaths: 1` for ~4 s as the game's own centre message (the blinking message 2007:4, the same line the spectator hint uses `[VERIFIED: spectate screenshots]`). | Small. Text: either fixed entries per count in `fmg-entries.json` (no RE at all, the matchmaking messages work this way) or one placeholder rewritten at run time (SB-4a check). Trigger: a key read like spectate's. | Which pad button is free: UNKNOWN (the game uses nearly all; a hold or a combo may be needed). The game's own messages may queue behind or replace it. |
| D. **Persistent HUD counter (RE spike, separate)** | A number always on screen, like the rune counter. | **Unknown.** The spike must first find: (1) a HUD text element always drawn in an expedition whose string the game reads by text id each frame (then a placeholder rewritten at run time is a live counter at no draw cost), or (2) an unused HUD element we can show and fill; (3) that writing it is safe from the main loop (thread). Adding a new element means editing the game's menu layout files: a big job. Budget: one Opus session; stop and report if (1) finds nothing. | Could take days with no result. The game's own HUD may have no free text slot. |

**Recommendation (revised 2026-11-07 after Leonardo's hard requirement):** C is near-certain and fully in the game's style: build it.
Before committing to D, run **D step 0** (cheap, agent time only, no game rounds for Leonardo): one `nro` run in an expedition with
`ui_text_trace = true` (`game/texttrace.rs`, already built) lists the text ids the HUD looks up; for the best always-visible candidate,
a dev write of `TEST 3` over its text and a screenshot. The result is a real in-game picture (or a clear "no slot"), so the cost of the
full D is known before paying it. A stays as the cheap fallback.

## 6. End of match: everyone

**When:** when the match is decided (`elimination.rs`: `FLAG_WIN` / `FLAG_LOSE`, the WIN / LOSE banner 4000 / 4010, then after 10 s the
ending 7000 / 7001 and the Result / Reward screens back to the Roundtable, `gamedata/emevd-nights.json` 99100803/4). In the bots mode a
defeated LOCAL with only bots left loses at once, which freezes the bots' numbers there.

**What:** one row per participant, the winning team first, then by place (later out = better), then by kills:

```
Battle Royale results
#1  Leo      3 kills  1 death   winner
#2  Bot 4    1 kill   1 death
#3  Ana      1 kill   1 death   left
...
```
Own row highlighted (blue as the killfeed's own team), rivals red. Duo/Trio: rows grouped by team with the team's total.

| Option | Cost | Risk / flaw |
|---|---|---|
| **E1. A dialog at the Roundtable after the return (recommended)** | A new EMEVD event in `m10_00_00_00` (Roundtable; `gamedata/emevd-events.json` already has `display: dialog`, 2007:1) with one long placeholder text in EventTextForMap_dlc01; the mod writes the summary over it, then raises the flag once the player stands in the Roundtable. | Unknown until the SB-4a spike: does the dialog read its text when shown (the toast does, `[VERIFIED]` for CL_MenuText; for EventTextForMap `[HYPOTHESIS]`), how many lines it shows, whether colour markup works in a dialog. The game is calm there, the box stays until OK, everyone can read it. |
| E2. Toasts during the 10 s WIN / LOSE banner | No RE: 7 toasts (header + 6 rows) at 400 ms apart. | Toasts fade in a few seconds and their stack height is UNKNOWN; the banner may cover them. Readable for 3 players, not for 6 (12 later). Kept as the fallback if E1 fails. |
| E3. A dialog inside the expedition at the decision | The same as E1 but in `common.emevd`. | Unknown whether a dialog draws over the banner and what the ending's warp does to an open dialog. More risk for no gain. |

**Text limits:** the toast placeholder is 120 UTF-16 units (`gametoast::TEXT_UNITS`); a row with a coloured name is ~80 units, so a
toast holds one row. The dialog placeholder is ours to size: **1024 units** fits 12 rows with markup. The dialog box's own limit and
wrapping: UNKNOWN (SB-4a). Names go through `gametoast::clean` (no markup from players). English only, like our other texts.

## 7. Plan (cards in TASKS "Scoreboard"; each piece has its scenario)

1. **SB-1 Tally + console `score`** (no UI): the rules of §3 as a pure `Tally` with unit tests, fed from the killfeed; the replacement bot
   keeps its slot's name. Scenario `score-bots` (one game, bots mode).
2. **SB-2 Own toast** (option A). Scenario `score-bots` checks the toasts; `score-self` checks own deaths (mob, then rival).
3. **SB-3 Mob / fall deaths on the bus** (kind 3; only if Q1 = every death). Scenario `mi-score` (two instances).
4. **SB-4a Spike: run-time text in a Roundtable dialog** (dev command, screenshot of an 8-line coloured dialog). Then **SB-4b End
   summary** (E1). Scenarios `score-bots-end` (one game) and `mi-score` (both games show the same rows).
5. **SB-5 (optional, Leonardo decides) HUD counter spike** (option D).

## 8. Open questions for Leonardo

- ~~Q1~~ Mob and fall deaths count (built that way, 2026-11-07).
- **Q2** End summary at the Roundtable in a dialog (recommended), or in toasts during the WIN / LOSE banner?
- ~~Q3~~ In-match: the blinking centre message on Esc / Start stays; a more professional in-match UI comes later from editing the
  game's GFx menus with JPEXS (Leonardo, 2026-11-07; §8b).
- **Q4** Per match only (recommended for now), or also a lifetime K/D kept across matches (a local file; another card)?

## 8b. Existing tools for editing the game's UI (research 2026-11-07, Leonardo's question)

- **The game's menus and HUD are Scaleform GFx files**, as in Elden Ring: `Z:\Tools\nr-unpacked\1.03.3\menu\` holds 328 files, among them
  `01_930_keyguide.gfx` (the key guide bar), `02_000_ingametop.gfx` (most likely the in-game HUD), `01_950_systemtoast.gfx`
  `[VERIFIED: directory listing]`. Elden Ring modders edit these with **JPEXS Free Flash Decompiler** and ship them through Mod Engine
  (menu replacers, a Sekiro-style HUD layout) `[SOURCE: nexusmods.com/eldenring/mods/2261, /6892]`. No Nightreign HUD edit was found.
  This is the "proper" route to a new, always-visible HUD text: a GFx edit adds the field; the open question is what feeds it a value
  (the game's C++ side sets HUD values through `CSFeMan`, not by FMG id).
- **fromsoftware-rs `eldenring` crate, `cs/fe_man.rs`** names the HUD manager's fields (Elden Ring layout, not Nightreign's):
  `blinking_message_id` / `_priority` (the blinking message by text id, writable from a DLL with no EMEVD), `FrontEndViewValues`
  (`sword_arts_name_string`, `proc_status_message` as `MenuString` = a UTF-16 pointer the HUD reads), tag data with `chr_name` /
  `role_string` `[SOURCE: docs.rs/crate/eldenring/latest/source/src/cs/fe_man.rs]`. A `MenuString` pointer is a candidate for a
  persistent text if Nightreign's HUD shows one at all times.
- **ERNativeUI** (Elden Ring only): a host DLL that adds mod pages to the game's own Game Options menu with native controls (no overlay)
  `[SOURCE: nexusmods.com/eldenring/mods/10767]`. A pattern for a future in-game settings page, not for a counter.
- **Gideon** (UI texture / layout deployer, CC BY-NC-ND: cannot be modified) `[SOURCE: github.com/ividyon/Gideon]`. Not useful here.
- Verdict: nothing ready to reuse for "a number on the HUD". The GFx route (JPEXS) is real but is its own spike (download a tool,
  learn the menu's ActionScript and its data source).
- **JPEXS spike done 2026-11-07: [re/GFX_UI.md](re/GFX_UI.md).** Editing and adding HUD elements works (a new text showed on the HUD);
  a live value needs the DLL to write through the Scaleform runtime (GFX-1, unknown cost).

## 9. Findings log (newest last, one line each)

Kept short; the run details are in `testing-log/2026-10-20_on.md`.

- 2026-11-07: design written (this file); nothing built.
- 2026-11-07: **built:** `Tally` (`features/scoreboard.rs`), `KillKind::Died` (kind 3, mob / fall death, on the bus, no feed line),
  console `score` / `score_show`, the message on the pause menu's opening (`CSMenuMan+0x48` 0 -> 0x11). `score-bots` PASS: the run-time
  text works in the centre message `[VERIFIED: screenshot]`; with the pause menu open it shows too `[VERIFIED: Leonardo watched the run;
  the single screenshot missed it because the message blinks]`.
- 2026-11-07: **Option B tried and reverted:** pointing CL_KeyGuide 110001 ("Close", category 202) at our text put
  `F:Switch  Q:Close   Kills: 1 | Deaths: 1   2:Discard ...` on the menus' button bar while a menu is open `[VERIFIED: score-bots run
  1794069384, screenshot score_pause_menu1]`. Leonardo chose to keep only the blinking message and to build a proper UI later with
  JPEXS (GFx); the code is not in the branch (it is in this log only: `fmg_for(base, 202)`, probe "Close", aligned u64 write of
  `text - fmg` into the offset slot, put back on leaving the expedition).
- 2026-11-07: **SB-4 built: the end-of-match summary.** When the expedition ends, the table is frozen (`scoreboard::summary`: teams by
  place, still standing first, then the one out last; kills inside a team; `(you)`, `(left)`, `- Winner` when one team stands). Once
  LOCAL stands in the Roundtable with no menu for 5 s, the Roundtable's event 99100310 shows it as the game's generic dialog, its text
  (placeholder 99100602, 1024 units) written at run time `[VERIFIED: score-bots-end]`. **The dialog shows 3 lines and cuts a long line**
  `[VERIFIED: score-dialog-lab run 1794071916]`, so the summary comes in pages (title + 2 rows, then 3 rows a page; the next page when
  the dialog closed: the menu word reads 0x4 while it is open, it must read so 0.5 s to count). A page closed in under 1.5 s (a key
  press left over from the result screens opened the table menu, word 0x200004) is shown once more. Gone participants: a player
  missing 6 s = left; a bot dead 2 s with no kill report = killed by a mob (its body stays in the world, so absence never comes).
  `score-bots-end` PASS (runs 1794071602 one page cut, 1794072818 three pages); Leonardo watched the three pages.
  Not done: Duo/Trio team totals, colours in the dialog, two real players.
