# Toasts and the killfeed (investigation notes, started 2026-10-27)

**Owner of:** everything we know about the game's on-screen toast / notification messages, the "Ally is showing location" leak, and the
plan to reuse the same mechanism as a **killfeed** (Leonardo's idea, 2026-10-27: a full investigation is planned, nothing here is built).
Memory facts go to [MEMORY_MAP.md](MEMORY_MAP.md) once verified; this file is the working notes. Tags: `[VERIFIED]`, `[SOURCE]`, `[HYPOTHESIS]`, `UNKNOWN`.

## 1. The leak that started this: "Ally is showing location"

- When a player places a map pin, **every other player in the session gets a toast** "Ally is showing location" (bottom right, a few seconds)
  with the **icon of the landmark nearest the pin** `[VERIFIED: two-instance screenshots, 2026-10-27, scenario mi-rw9-markers, testing-log]`.
  It reveals the area, not the pin's exact position. It still appears for a **rival** after the marker filter (RW-9, PR #22), which hides the
  pin itself on the map, compass and in the 3D world.
- Who produces it and how it is triggered is **UNKNOWN**. Candidates: the message-id code at `exe+0x7C67E0` (it reads the marker manager
  `[exe+0x3C17BE0]`), a net message handled when a pin record arrives or changes (the record sync loop: add path `exe+0x911B13`), or the
  pin record handlers `exe+0x90A6B0` / `exe+0x90AC50` `[HYPOTHESIS]`. The marker record names its placer (u16 at `+2` = the placer's map id
  `[PlayerIns+0x5A0]+8`), so a suppression for rivals only is possible once the producer is found (MEMORY_MAP §20 "Map markers").
- Requirement (RW-9): a rival must not be locatable by a notification either. Teammates keep the toast.

## 2. What we already know about showing text on screen (reusable for a killfeed)

- The mod already writes **the game's own on-screen messages**: matchmaking status as blinking messages (UI-2). Each message is an event in the
  Roundtable event script (`gamedata/emevd-events.json`) that waits for a flag (`FLAG_BASE` 10007200 + k) and runs *Display Blinking
  Message* (2007:4) with a text from `EventTextForMap_dlc01` (`gamedata/fmg-entries.json`, text id `TEXT_BASE` 99100500 + k); the DLL sets the
  flag (group 10007 only). Code: `crates/nrpvp/src/mm/ui_msg.rs`; texts in 15 languages by the game-data package. Limits seen: the game may
  drop a new message while one is showing (about one message per 6 s), and our events exist only where our event script is loaded (the
  Roundtable; the expedition has its own scripts: `defeat`, `match` banners use events 4000 / 4010 `[SOURCE: STATUS, TRAPS]`).
- Text is localised per client (FMG), so a killfeed line like "<name> defeated <name>" needs **runtime names**, which static FMG entries
  do not have: UNKNOWN how the game formats a toast with a player name (the ally toast and the "<player> joined" style messages show it exists).

## 3. Investigation plan (killfeed and toast), in order

1. **Find the toast producer.** Observe with two instances (`nro`, scenario `mi-rw9-markers`): place a pin on one, `trace_write` / hardware
   watch the message/FE queue on the other, or hook-log the FE message creation (`CSFeMan` entries, `exe+0x7C67E0` neighbours); use
   Ghidra/REA on `nightreign.exe` (TOOLS.md) from the string / FMG id of "Ally is showing location" (find its text id in the unpacked FMG
   `Z:\Tools\nr-unpacked\1.03.3\`, then xrefs).
2. **Suppress for rivals** (RW-9 acceptance): skip or re-route that toast when the pin's placer is a rival; same style as `hudfilter`.
3. **Learn the toast API**: the function that shows a toast with an icon and a text with parameters (player name, icon id). If callable from
   the mod on the main thread, it is the killfeed output: "A defeated B" with the killer / victim names from `night_match` (hit log, defeat
   state), shown to everyone including the defeated and spectators.
4. **Killfeed rules**: who sees what (rivals' deaths shown to all, no positions), spam limit, localisation (names are data, the sentence is FMG).
5. **Acceptance as scenarios** (`mi-*`): kill a rival (`rival_hit` + `hp0` or a real hit), assert the line on the other instance (screenshot +
   a mod counter), run twice.

## 4. Test route

Two real emulated-Steam instances on one PC: [orchestrator/MULTI-INSTANCE.md](../orchestrator/MULTI-INSTANCE.md), scenarios in
[orchestrator/SCENARIOS.md](../orchestrator/SCENARIOS.md) (`mi-rw9-markers` places the pin; `mclick` = middle click). Real Steam on two PCs
stays a Friend Session check.
