# T-041 — No get-up after a rival's revive: handoff (2026-10-21, end of the Opus session)

Read this first if you pick T-041 up in a new session. Card summary: `.claude/TASKS.md` (PvP rules, T-041). Evidence:
`.claude/TESTING.md` §3 (entries "T-040 runs 1 and 2", "T-041: revive-bar trace and the revive block"). Addresses: `.claude/re/MEMORY_MAP.md`
§6 (downed player). Logs: `.claude/experiments/T-040/`, `.claude/experiments/T-041/`.

## The problem (Leonardo, product)

Solo PvP: when a rival hits a downed player and one hit empties the purple revive bar, the downed player **plays the stand-up
animation** (~3.6 s), then our rule knocks them down again. Players hate it. Leonardo wants **no stand-up at all** on any screen.
Teammates (Duo/Trio) must still revive normally.

## What is now known (all game 1.03.3, exe 1.3.3.0)

1. **The player script decides the revive, from the bar.** `c0000.hks` (readable decompile: `/z/Tools/nr-mods/Spectral haw flying/mod/action/script/c0000.hks`,
   a modded copy of [El-Fonz0/EldenRingNightreignHKS](https://github.com/El-Fonz0/EldenRingNightreignHKS); compiled vanilla in
   `Z:\Tools\nr-unpacked\1.03.3\action\script\c0000.hks`):
   - `NearDeathCommonFunction` (line ~10305): `if env(GetNearDeathHealth) <= 0 and env(GetSpEffectID, 102455) == FALSE then
     ExecEventAllBody("W_NearDeathRevival")`. `GetNearDeathHealth` = env id **372**.
   - `NearDeathStart_onUpdate` (line ~11477): while the down-start animation plays, `GetNearDeathHealth <= 0` (with SpEffect 102100,
     without 102455) **latches** `ISENABLE_NEARDEATHREVIVAL = TRUE`; when the start animation's EzState flag 0 fires, it plays
     `W_NearDeathRevival`. This is why a get-up started with the bar back at 40 (22:42:36): the bar had touched 0 one frame earlier.
   - **SpEffect 102455 disables the "gauge depleted → revive" path** in both places `[SOURCE: decompile; untested in game]`.
   - `GetNearDeathHealth` is almost certainly the revive bar `PlayerIns+0x714` `[HYPOTHESIS: name + behaviour; the exe handler of env 372 not read yet]`.
2. **The revival animation sets the flag, not the other way round.** The only real writer of `PlayerIns+0x732` ("revive completed") is
   `exe+0x40E5F1` (`mov byte [rsi+0x732], 1`), case **145** of a 163-case event dispatcher `exe+0x40D390` (jump table at RVA
   `0x40E67C`, index = type−1). Case **144** (`exe+0x40E5E8`) sets `+0x731` ("go down"). The events come from the animation, every frame
   it plays. The other match (`exe+0x66698B`) is the PlayerIns constructor zeroing the field. `[VERIFIED: byte search + disassembly]`
3. **The game's state machine** (`exe+0x674EB0`, per frame, state byte `+0x710`): `+0x732` → `exe+0x66F3B0(chr, 2)` = state 2 (get-up);
   neither flag on an event frame → state 0 at once. MEMORY_MAP §6.
4. **The hit on the bar**: `exe+0x66DEC0(victim=rcx, reviver=rdx→rsi, hit=r8)`; the store is `exe+0x66DFC4`
   (`movss [rdi+0x714], xmm1`, xmm1 = new bar, already clamped at 0); then if the bar is 0, `+0x728` = reviver handle `[rsi+8]`; then the
   pause `+0x71C` and a call `exe+0x3D7610(reviver, victim)` when the hit was > 0. Runs on the **victim's own game**, main thread
   (trace RIP `exe+0x66DFCC`). `[VERIFIED: trace + disassembly]`
5. **Health address fixed by T-040**: `[[PlayerIns+0x1B8]+0x0]+0x140` (the old `[PlayerIns+0xB78]+0x70` is not always health).

## What was tried, and why it failed

| Try | Result |
|---|---|
| T-040 retry the re-down 2 s | Works (re-down after the stand-up). Kept as the fallback |
| `early_redown` (HP 0 during the get-up) | Get-up still plays in full (run 3) |
| `revive_block` v1: clear `+0x732` at `exe+0x675159` | "Neither flag" branch → instant stand-up |
| `revive_block` v2: jump to the no-event exit `exe+0x67539F` | State stays 1, but **the animation still plays** (the HKS already chose `W_NearDeathRevival`); 169 frames dropped; downs counter +1 after it |
| Data watch on `+0x730..+0x733` | **Killed the game silently** (no WER record) at arm time. Do not repeat; `bar_trace` is now off by default |
| Data watch on the bar `+0x714` | Safe; found the writers |

## Recommended fix (next step)

**Stop a rival's hit from ever taking the bar to 0, inside the hit itself.** Hardware execute breakpoint on `exe+0x66DFC4` (the store
in `exe+0x66DEC0`). In the VEH: if `rdi` == LOCAL and the reviver's handle `[rsi+8]` is a RIVAL (same handle list `revive_block`
publishes), set `xmm1` = the current bar `[rdi+0x714]` (no decrease). The HKS then never sees `GetNearDeathHealth <= 0` from a rival,
so no revival animation, no `+0x732`, no extra down, no gauge flicker. Teammates: untouched.
- The T-039 penalty (clock −`rival_hit_seconds`, pause 0) was applied by the 50 ms poll on a *bar drop*; with no drop it must come from
  the VEH: count rival hits in an atomic (and write nothing else there), and let the main loop apply the clock/pause penalty when the
  count changes. Do not skip the game's pause write or `exe+0x3D7610` (unknown side effects).
- Check bytes at `exe+0x66DFC4` (`F3 0F 11 8F 14 07 00 00`) before arming, like `reviveblock.rs` does. Arm only while LOCAL is downed
  (state 1), in a free slot (`hwbp::is_free`; 0/1 in the player build).
- Keep `revive_block` (`game/reviveblock.rs`, always on) and the T-040 re-down as fallbacks until the new hook passes in Nucleus.
- Alternative (data only, Solo mode): give the downed player SpEffect 102455 so the HKS never revives. Needs a way to add a SpEffect
  (EMEVD `SetSpEffect` from our m10/event package, or the game's add-SpEffect function); blocks teammates too, so Duo/Trio need the
  hook anyway. Keep as plan B.

**Test (Leonardo, Nucleus 2 instances, `team_size = 1`, ~5 min, no console):** down one player, the other hits the bar until it would
empty. Pass = no stand-up on either screen, the bar never drops on rival hits, the clock still loses 3 s per hit. Instance1's
character empties the 40 bar in one hit; Instance0's normal hits do not (use a heavy attack). Then `team_size = 2` (teammate revive works).

## State of the code (uncommitted, branch `t-028-snapback`)

- `game/combat_state/`: T-040 (data-module HP route, 2 s retry, `T-040 LOCAL state` diagnostics, `early_redown` console, off).
- `game/reviveblock.rs`: always-on revive block v2 (both builds). `game/bartrace.rs`: dev trace, **off by default** (on = `bar_trace 1`;
  it watches `+0x730` now — that target crashed; switch the offset back to `layout::PLAYER_REVIVE_BAR` before using it again).
- `dr.rs`/`hwbp.rs`: write watches (`set_write`), `is_free`. Console: `revive_fail`, `early_redown`, `bar_trace`, `revive_block`.
- Nucleus deploy: build `43928771ED3FCB71` (trace off, revive block v2 on), deployed 2026-10-21.
- Tests 372 nrpvp + 4/3/25; clippy dev + player clean at the time of writing.

## Rules to keep (from this session)

- Leonardo tests alone; give him builds that work with **no live console commands**; tell him exactly when to land and what to hit.
- Never leave an experimental trace on by default in a build he plays. Data watches on the flags dword are known to kill the game.
- Report in plain words; quote 1–2 log lines; mark HYPOTHESIS vs VERIFIED.
