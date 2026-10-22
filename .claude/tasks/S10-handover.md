# S10 handover: what the Seamless Co-op study means for the next work

Written 2026-10-22 for the next planner session or agent. Evidence and details: `research/S10-seamless-coop-internals.md` (the
owner doc for these findings). This file says **what to do with them**. Read `CLAUDE.md` and `STATUS.md` first, as always.

## The goal (Leonardo, STATUS §8, unchanged)

A match where players press Commence once and nothing else: no Steam window, no host role anyone sees, the match starts by itself,
and **a host leaving does not ruin the evening**.

## What S10 changed

| Before S10 | After S10 |
|---|---|
| "Host migration is the goal, work on it as far as it goes" | **True host migration is a no-go.** The host's PC runs the world; the game has no handover code; SC's session code is Themida-protected and cannot be extended. Do not spend work on it |
| MM-3g / MM-7 on hold until S10 is reviewed | **Unblocked.** S10 says stay on SC and build a thin layer on top (1–3 weeks). That layer is exactly MM-3g + MM-7 |
| MM-9 = "end cleanly for now" | MM-9 is **the** answer to host loss: detect within seconds, end it the same way for everyone, requeue everyone automatically (rematch) |
| Replacing SC was an open option | Kept only as a fallback if a game patch breaks SC for good (4–6+ months). Map of what SC changes: S10 doc §1a |
| 6-player cap: who sets it UNKNOWN | Mostly SC (it builds the player container). Plan for 6 stands |

The one thing that could reopen migration: **runbook R2** (S10 doc §6). If the joiner's game keeps a live world after the host's
game dies, the engine already carries on and the plan becomes "keep that working". Run R2 before starting MM-9's code.

## Next work, in order

1. **HUMAN, solo, ~20 min: R2 (host Alt-F4 on Nucleus), then R1 (hook census).** Steps in S10 doc §6. Bring back: what instance 2's
   screen did in each round (words of any message), and the two `census_*.txt` files.
2. **Opus: record R1/R2.** R2 result → S10 doc §4 and MM-9 card. R1 → new MEMORY_MAP section "Seamless Co-op hooks" (map each redirect
   address to a known game function where possible; flag any that sit on code we hook too: `exe+0x27F4D20`, `exe+0x9EEBFB`, `exe+0x7AC800`).
3. **MM-3g + MM-7 (shared spike):** brief `tasks/MM-3g-MM-7-zero-click-autostart.md`, take off hold. Input from S10: a lobby only works when
   the game's own team-session code creates it (`CSTeamSessionManager` / `SessionCreateJob`; `CreateLobby` call site `exe+0x25919A1`
   reads type from a flag and size from `[request+0x68]`), so the target is the menu-side entry that starts that code, not a Steam call.
4. **MM-9 v1 (clean end + rematch):** after R2. Host-loss signals to combine on each joiner, first one wins:
   (a) Steam `LobbyChatUpdate_t` saying the host left the expedition lobby (the second, unlocked lobby; S9 §5c) — note Steam then makes
   someone else the lobby owner automatically, so do not read "owner changed" as "host still here";
   (b) the host's player leaving the ChrSet (`players` count drops, MEMORY_MAP §2);
   (c) the game's own error step `CSEventNetworkErrorReturnTitleStep` running (needs its address; optional).
   Then: show "The host left - match over" (UI-2 message), return to idle, and requeue the survivors together.
5. **MM-8 (host by best connection):** unchanged, independent; makes host loss rarer.

## Doc updates still owed (owner docs, not done by S10)

- STATUS §8: add the decision "host migration no-go; clean end + rematch go (S10, 2026-10-22)". STATUS §3: S10 done.
- TASKS: S10 card → remove after Opus review; MM-3g/MM-7 status → ready; MM-9 text → point here and to S10 §4.
- MEMORY_MAP §12: the `CreateLobby` call-site facts above and the game's session class list (S10 doc §2).

## Rules that matter here

Hard rule 3: `nrsc.dll` is studied read-only; never paste SC code into docs or code; raw dumps stay in
`C:\Users\Leonardo\source\nrsc-study\` (outside the repo). Hard rule 2: the census and every runtime step use a modded launch only
(`scripts\launch.ps1` or Nucleus), never the Steam/EAC launch.
