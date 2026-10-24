# Traps we already hit

Read when you touch the area named in a bullet, or before debugging something odd. One owner doc for traps; add new ones here (date + what + the rule), not in STATUS.

- **Nucleus `dlllist.txt` must not end with a newline.** It makes Nucleus fail with "WaitForSingleObject failed 258". The deploy script writes it correctly; never edit it by hand.
- **Under Nucleus, keys reach a window only when it has focus** (END unlocks input). Use the console instead: `nrcon --all <cmd>` reaches every instance.
- **Nucleus deletes its instance folders on close.** Logs go to `dist\nucleus-logs\` for that reason.
- **Nucleus does not load our game-data package when it starts the game through Seamless Co-op's launcher** (the plain deploy):
  no no-boss nights, vanilla hawk route. `deploy-nucleus.ps1 -Me3` (start through our launcher / me3) works for window 1 only:
  window 2 stops on ColdLoader "appid not found" (2026-10-23, cause UNKNOWN). Use the plain deploy. Event-script features are tested solo with `scripts\launch.ps1`; under Nucleus use the dev console
  (`flag`, `clock`) instead.
- **Never run the scanner or any tool against the normal game.** Steam launch = vanilla with anti-cheat. Mods only through Nucleus, `nrpvp_launcher.exe`, or `scripts\launch.ps1`.
- **Never touch the vanilla saves** (`%APPDATA%\Nightreign\<id>\NR0000.sl2*`).
- **Player ids change between the lobby and the expedition** (run 21: joiners were `0x6003`/`0x6004` in the lobby, `0x6002`/`0x6003` in
  the expedition). Teams are only meaningful once the expedition has loaded; do not decide or show teams from lobby ids.
- **Addresses change every launch and every area load.** A value found by the scanner is only good for that run; the mod must reach it through the player object (see `re/MEMORY_MAP.md`). A scanner left watching a fixed address after a restart is looking at the wrong memory: stop it.
- **Double-check offsets before any write.** On 2026-10-12 a test wrote to `+0x728/+0x72C` instead of `+0x718/+0x71C` by mistake (no lasting effect).
- **Setting health to 0 does not kill** in a multiplayer session; it downs. Death is the bleed-out clock reaching 0.
- **Elden Ring layouts are hints only.** Several were wrong in Nightreign (list in `re/MEMORY_MAP.md` §9).
- **No `VirtualQuery`-guarded reads on the main loop.** They made each pass take 2–3 s in an expedition (run 15). Use `mem::read` or
  `RegionCache::direct()` there (the guarded `RegionCache::new()` is test-only since R5). The heartbeat line shows the pass time.
- **Only a lobby the game makes can be joined.** Our own `CreateLobby(Public, 6)` gets adopted by Seamless Co-op but joiners' games
  leave it at once, and a direct `JoinLobby` gives Steam membership without the game session (FS-MM2). Steam lobby membership is not
  presence: ALL PRESENT also needs the host's ChrSet to hold the players.
- **`RegionCache::readable_len` is a `VirtualQuery` even on the direct cache.** `flags::locate` used it; 12 flag reads per tick made the
  main loop take 0.6 s (solo) to 2.4 s (Nucleus) per pass (2026-10-23). Read flags only through the fixed `locate` (plain copies).
- **Nucleus has the vanilla night**: minions, the wipe check (both down = game over for both) and the night freeze (a knocked player
  inside the circle never bleeds out). Never put both players on team 6 at night there (1182 counts them out → instant wipe); use
  console `god 1` on the knocking player instead. `clock end` on a fresh day kills everyone outside the closed circle: use
  `clock circle` and walk in.
- **`mm_quiet_invite` and `mm_lobby_trace` both use all four debug registers**; config turns quiet invite off when the trace is on.
- **Sonnet agents can be cut off by usage limits mid-task.** Check `cargo build` and `git status` before trusting the tree; the task card says what was intended.
- **Hardware breakpoints are a shared budget of four** (2026-10-24): hold, gate, reviveblock, menu/matchmaking and, in dev, the puppets (three) all draw on them (`hwbp.rs`). The M5-9 release failed with puppets present because the start breakpoint stayed armed while the gate wanted a slot (fixed). Any new feature that needs a breakpoint states how many slots it holds at peak and frees them when done.
- **Offline solo carries SpEffect 6999500** (a revive item on the script's list), so the die-at-once rule is OFF there; the 102130 kill is only reachable in a real session (TESTING §6, FS-M59).
- **Console tails are unreliable evidence in scenarios:** `log 300` returns the last 300 lines, and event bursts (flag watch) push lines out; turn `ui_flag_watch = false` in scenarios and, when possible, assert on `night_status` / state commands instead of log text.
- **One swing does not down every puppet** (position and facing drift after a morning move): down puppets with `puppet_hp <slot> 0`.
- **`nro` needs `nro desk` in a normal terminal for keys and screenshots** (an agent can start it in the Terminal panel); the game's quit crashes, so runs end with `nro stop`.

