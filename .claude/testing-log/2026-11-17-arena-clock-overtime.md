# 2026-11-17 Arena round clock, one-minute message and overtime (solo, against the bot)

**One PC, Leonardo, dev DLL of branch `arena-round-timer`** (game 1.03.3 / exe 1.3.3.0, SC 1.1.3, me3 0.11.0; game data = the
2026-11-17 HP1 build, `common.emevd.dcx` 3A70D40971F6CEDA, no saved-loadout talk file: ESDLang needs .NET, not installed). Bots On +
Solo + the Arena card. Log: the branch's `dist\nrpvp.log`.

**Clock (ARENA-CLOCK), first run 16:26 UTC: PASS (Leonardo: "the result is nice").** `arena_clock: on; rune count 0 kept`; the rune
counter counted the round down. Round 1 ran the full 3:00 and was judged by HP % (the old rule): `bout 1 timed out: Won`, `Round 1:
time! you won (1-0)`; round 2 started at the full limit after the bot was replaced.

**One-minute message and overtime (ARENA-OT), second run 16:46 UTC: PASS.** Round 1 fight at 16:46:54; `One minute left!` at
16:48:54 (the game's message, text 99100111); at 16:49:54 `Overtime! HP drains until someone falls` (99100112) and
`arena_ot: overtime: HP drains`; the bot reached 1 HP at 16:50:46 (52 s, 2 %/s from about full), the round ended 0.8 s later:
`bout 1 timed out: Won for the host`, `Round 1: time! you won (1-0)`. Unit tests 692 PASS, clippy dev and player builds clean.

Not seen yet: two players in overtime (both games agreeing on the host's verdict), the rune count put back after WIN / LOSE.
