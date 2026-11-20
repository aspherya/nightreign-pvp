# 2026-11-17 Arena: one ultimate per round, consumables back every round (ARENA-R17)

**Leonardo, solo, one PC, Arena against the bot** (dev DLL of branch `arena-round-ult` b07b281 merged with `dev`; game 1.03.3, SC
1.1.3, me3 0.11.0; `dist\gamedata` rebuilt in full, the saved-loadouts ESD with `DOTNET_ROLL_FORWARD=Major` because esdtool wants
.NET 6). One match, **WIN 2-0. Leonardo: "everything is fine"; PR aspherya/nightreign-pvp#15 merged into `dev` (039cb29).**

From `dist\nrpvp.log`:
- Round 1: `ultimate filled for this round (0.0 -> 1000.0)` at the start, `ultimate used (300.0 / 1000.0)` 18 s in, no recharge
  after it. Reset: `ultimate filled for this round (300.0 -> 1000.0)`. Round 2: used again 10 s in.
- Consumables: 2 of `4000050A` in the pouch at the start; both used in round 1; the reset gave them back (`arena_refill: 1 good(s)
  to give back [(4000050A, 2)]`, pouch slot 32). Round 2's reset: `0 good(s)` (none used).
- After a use the gauge stays at 300 / 1000 (30 %), not 0: the game leaves that much, and our hold only stops it rising. Leonardo
  accepted it as is.
- Seen again (not this change): at round 2's reset the spawn move was refused for ~1 s (`Physics+0x50 does not read as a
  heading`), then forced.

Not covered: two players (a rival's ultimate), `mi-arena-bot` not run (it now waits for the new log lines).
