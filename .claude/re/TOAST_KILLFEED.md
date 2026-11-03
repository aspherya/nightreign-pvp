# Toasts and the killfeed (investigation notes, started 2026-10-27)

**Owner of:** everything we know about the game's on-screen toast / notification messages, the "Ally is showing location" leak, and the
**killfeed** built on the same mechanism (Leonardo's idea, 2026-10-27; built 2026-11-03).
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

## 2. The killfeed (KF-1, built 2026-11-03, Claude's design on Leonardo's delegation, DECISIONS)

**What the player sees:** at the bottom right, the game's own toasts, stacked: `Leo downed Bob`, then `Ana killed Bob` or `Bob was consumed by the tide`. The own team's names are blue, everyone else's red. No count of who is left (Leonardo, 2026-11-03). No line for a mob's or a fall's death: the game's own respawn follows, nobody is out.

**The rules:** a `downed` line when a rival's hit (last 2 s) puts a player down; a `killed` line when `defeat::Defeat` judges them defeated by a rival, naming the **last rival who hit** (the window of the defeat rule, 30 s: a player who knocks someone and a third player who touches them before the bleed-out ends: the third player gets the kill; there is no finishing blow in our rules). `Player1 + Player3 killed Player2` is not done.

**How it works** `[VERIFIED: mi-killfeed PASS, photos read, 2026-11-03]`:
- **Who did it:** only the victim's game knows (`PlayerIns+0x1A0`). `features/br_match/kill_report.rs` raises the event; `features/killfeed.rs` publishes it on `net::bus` (topic 6) and shows it; the other games receive it in under half a second.
- **Names:** `PlayerGameData+0x9C` of each player's copy (MEMORY_MAP §5), read once a second and kept. No Steam name, no id to Steam id map.
- **The screen:** `game/gametoast.rs`. On the game thread (a call site of the `CSFeMan` update, no hardware slot): write the line over a placeholder text (`99100110..117`), point row 6120 at it, call the toast function, put the row back. Row 6120 is the ally pin toast that is off in a match anyway. The row is known from `toastfilter`'s scan (about 30 s after launch), a line waits for it.
- A name goes through `gametoast::clean`: the toast reads markup, a name must not bring any.

**Open:** a real hit naming the killer on two PCs (the path is the same `classify` that `T-033 hit` used with real hits in FS-RC1, but the killer's name from it has only been seen with the console's `rival_hit`: TESTING §6 FS-HM1 item 8); whether a rival's hit on a player already down updates the attacker field before the T-041 hook runs (if not, the kill goes to the last rival whose hit took HP). The sentence is English in every language, as our other texts. A name in an alphabet the game's language has no font for shows boxes (CJK in an English game). **The game's own "Near death" toast** (no name) also reaches the rival's screen; it sits above our `downed` line. Vanilla; left alone.

## 3. Ideas left
- An icon on the row (`toast_icon <id>` tries one; none looks clean).
- The own name highlighted differently from a teammate's.
- More than 6 players: the toasts stack and fade by themselves, so nothing here depends on the player count.
- The `downed` line could go away in Solo, where it only announces the kill 15 s ahead. Kept: it is what the big battle royales show.

## 4. Test route

Two real emulated-Steam instances on one PC: [orchestrator/MULTI-INSTANCE.md](../orchestrator/MULTI-INSTANCE.md), scenarios in
[orchestrator/SCENARIOS.md](../orchestrator/SCENARIOS.md) (`mi-rw9-markers` places the pin; `mclick` = middle click). Real Steam on two PCs
stays a Friend Session check.
