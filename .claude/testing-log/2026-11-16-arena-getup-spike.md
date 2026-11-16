# 2026-11-16 Arena get-up spike (one PC, two emulated instances)

Why: on real Steam (2026-11-15, Samuel's log `nrpvp_2026-11-15_14-11-37.log`, Leonardo's `nrpvp_2026-11-15_14-59-55.log`, Leonardo's
clock ~23 s behind) Samuel lost round 1 to a down, the get-up was written at +2.04 s (no effect) and again at +4.57 s; the state went
1 -> 0 with HP 670 and no state 2, he died at +6.9 s (round 2 "lost" 0.9 s after its start, match LOSE 0-2) and the level dropped to 14
at +12.0 s. The 2.5 s retry spacing of ARENA-FS12 (TRAPS) did not prevent it.

Build: branch `arena-getup-spike` from `dev` 178d185, dev DLL, dev game data (the saved-loadouts talk bundle not built: `esdtool`
needs .NET 6, not installed). Game 1.03.3, emulated Steam, me3 0.11.0. Lab `scenarios\mi-arena-getup-lab.toml` (PASS, games kept:
`runs\1794857897/907-mi-arena-getup-lab-i1/i2`), then downs of instance 2 by console `hp0`, `arena_bo 50` / `200` so the match never
ends, the get-up method switched with the new dev console `arena_getup <byte|bar|none> <after_ms>`. Every down is timed by the new
`arena_getup:` lines (LOCAL state changes from the down, `SKIP` = 1 -> 0 with no state 2, `DEATH` = HP 0 in state 0).

| Method | Written at (after the down) | Downs | Clean (1 -> 2 -> 0) | SKIP | Death |
|---|---|---|---|---|---|
| byte `+0x732` (today) | 2.0 s | 8 | 8 (state 2 at +3.0 s every time) | 0 | 0 |
| byte | 0.3 s (and again at 2.8 s) | 6 | 6 (the 0.3 s write did nothing; state 2 at +3.0 s) | 0 | 0 |
| byte | 4.0 s | 4 | 0 | **4** (+4.06 s) | 0 seen |
| byte | 4.5 s | 4 | 0 | **4** | **3** (+7.4..7.9 s) |
| bar to 0 + pause 3.5 | 0.3 s (every tick) | 4 | 4 (state 2 at +3.0 s) | 0 | 0 |
| bar | 2.0 s | 4 | 4 (state 2 ~0.95 s after) | 0 | 0 |
| bar | 4.0 s | 4 | 4 (state 2 50 ms after) | 0 | 0 |
| bar | 4.5 s | 4 | 4 (state 2 50 ms after) | 0 | 0 |
| none (the reset's Recovery 2004:47 at +5 s) | - | 2 | 2 (state 2 at +5.2 s, up at +8.8 s) | 0 | 0 |

- **Samuel's bug reproduced on one PC, every time:** a `+0x732` write later than ~3.5 s after the down skips the get-up (8 of 8) and
  the player dies ~3.3 s later. No get-up can start before ~3.0 s (the fall); a byte written before that is acted on at +3.0 s.
  On real Steam the window moved (his +2.04 s write did nothing): a rival hit on the downed player at +0.15 s is the likely cause
  `[HYPOTHESIS]`. Cause in the code (disassembly, MEMORY_MAP §6): the byte makes the code lead (`exe+0x66F3B0(chr, 2)` sets state 2
  and HP 50 % and asks for no animation); in state 2 the per-frame function drops to state 0 on the first frame without `+0x732`,
  which only the revival animation sets; when the player script is not ready to play it, the player is left in the downed script
  with the bleed-out clock at 0.
- **The bar route never skipped** (16 of 16, the late writes included): the player script sees the empty bar, plays the revival, the
  animation sets `+0x732`, state 2 follows. The downed rule must leave that bar alone (it restores a bar a rival "hit" lowered): the
  spike switches it off while the bar route writes.
- **Immortality PoC (Set Character Immortality 2004:12, our flag 10007520, event 99100920):** player 2 at 60 HP took two real hits
  from player 1 and stayed at ~1 HP, standing, no down, no round lost (shot `runs\1794857907-mi-arena-getup-lab-i2\shots\imm_p2b.png`).
  `hp0` (a direct HP write) is not stopped by it.
- **The fix (same branch, `nrpvp.dll` 1C8A36783E07B354):** the bar route is the only get-up (the byte write is gone), the downed rule
  leaves that bar alone, and a skip or a death still seen holds the player's rounds 12 s and puts it back on its spawn. Lab again
  (`runs\1794859092/099`): 8 downs (get-up at the default 2 s and at 4.5 s) all 1 -> 2 -> 0, no SKIP, no death; one down with two
  real hits from player 1 before the get-up (`T-041 ... 1 rival hit(s) on downed LOCAL cancelled`): state 2 at +3.04 s, up at +6.67 s.
- Not covered: two real PCs.
