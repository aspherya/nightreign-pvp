# Toasts and the killfeed (investigation notes, started 2026-10-27)

**Owner of:** everything we know about the game's on-screen toast / notification messages, the "Ally is showing location" leak, and the
plan to reuse the same mechanism as a **killfeed** (Leonardo's idea, 2026-10-27: a full investigation is planned, nothing here is built).
Memory facts go to [MEMORY_MAP.md](MEMORY_MAP.md) once verified; this file is the working notes. Tags: `[VERIFIED]`, `[SOURCE]`, `[HYPOTHESIS]`, `UNKNOWN`.

## 1. The leak that started this: "Ally is showing location"

- When a player places a map pin, **every other player in the session gets a toast** "Ally is showing location" (bottom right, a few seconds)
  with the **icon of the landmark nearest the pin** `[VERIFIED: two-instance screenshots, 2026-10-27, scenario mi-rw9-markers, testing-log]`.
  It reveals the area, not the pin's exact position. It still appears for a **rival** after the marker filter (RW-9, PR #22), which hides the
  pin itself on the map, compass and in the 3D world.
- **How it works `[VERIFIED: static reading of exe 1.3.3.0 + two-instance trace, scenario `mi-toast-solo`, 2026-11-02]`:** the toasts are rows of
  `UserDispLogParam` (32 bytes: the first field is a CL_MenuText id). Row 6120 = text 337750 "Ally is showing location", row 6130 = 337720
  "Ally is also showing location", rows 6100 / 6110 = the placer's own "Location shown" / "You also showed location"; rows 60xx = the
  equipment pings. One function (`exe+0x86833F`, no direct callers) shows a toast from its row id and does nothing when the row's text id is
  negative (`exe+0x86844F`: `cmp dword [row], 0; jl <exit>`). The text ids 3377xx are computed (CL_MenuText category 200 via the getter
  `exe+0x7AC800`), which is why searching the exe for the number found nothing. The pin's owner is not an argument we know of.
- **The fix (UI-5, `game/toastfilter.rs`):** while a Battle Royale match runs (`in_match`, any team size) both rows' text ids are written -1 in memory (found by
  scanning for rows 6100 / 6110); outside a match (normal expedition, Roundtable, offline) they stay the game's own. A teammate's toast goes too
  (Leonardo, 2026-11-02: simplest rule, the pin itself still shows on the map). **Telling a rival's pin from a teammate's was tried and failed:** switching the
  rows off from `hudfilter`'s map sync hook (`exe+0x9040E0`, which judges a new or changed rival record) comes too late, the toast function had already read its
  row (player 2 still asked text 337750 although the hook had fired for that pin). The toast function (`exe+0x86833F`: row id, an object with a vtable, two small
  values) has no direct callers and no obvious owner argument; the way left would be an entry detour on it that skips rival-owned pins (hardware slots are too scarce,
  TRAPS). Dev tool: `toast_trace = true` logs who asks for toast texts (`game/toasttrace.rs`).
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
