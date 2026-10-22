# Developer console

**Status (2026-10-20): CON-1, CON-2 and CON-4 done.** The console works against a live game (SMOKE-1); the ImGui panel and the F-key
hotkeys are deleted (R5) and their remaining functions are console commands. CON-3 (`kill`, `down`, `bleed_clock`, flags, settings)
and CON-5 (in-game line, autoexec) are planned. Developer builds only (`dev` feature + `dev_mode = true`): the player zip never has it.

## 0. Use it now

Start a developer game (`.\scripts\launch.ps1`, or Nucleus after `scripts\deploy-nucleus.ps1`), then in any terminal:

```
dist\nrcon.exe list                      running games (Nucleus: index 0 = Instance0)
dist\nrcon.exe status                    the only running game
dist\nrcon.exe -i 1 players              game 1 from `list`
dist\nrcon.exe --all "echo hello"        every game
dist\nrcon.exe                           interactive prompt (exit to leave)
```

Every command and its reply is also logged (`console: <line> -> <reply>`). From Git Bash, a leading `/` gets rewritten into a path:
leave it out there (`kill`, not `/kill`).

### Commands available now

The source of truth is the registry in `crates/nrpvp/src/console/mod.rs` (`help` prints the same list).

| Command | Args | What it does |
|---|---|---|
| `help` | `[name]` | List the commands, or one command's help |
| `echo` | `text…` | Write `console: echo: text` to the log (a note for whoever reads the log) |
| `mark` | `[text…]` | Write a numbered `MARK #n: text` line to the log |
| `log` | `[n]` | The last n log lines (default 20, at most 300), straight from the game |
| `status` | | World loaded or not, local player and its team value, PvP mode, game mode (`mode: br` / `mode: offline`), matchmaking line, pid |
| `mode` | `[br\|offline]` | OFF-1 game mode. `br`: Commence starts the queue. `offline`: Commence with Solo starts the game's own solo expedition at once (Duo/Trio say "solo only"); switching to it leaves the queue. Asked during an expedition, the change waits for the Roundtable. No argument shows the mode. Logs `mode: <mode>` |
| `bots` | `[10\|30\|50\|10000]` | OFF-4, offline mode only: sets our flag so the game's own NPC bot instruction `2003[97]` runs with `NPCBotTableParam` table 10 / 30 / 50 (2 Nightfarers, level 1: heroes 1+2, 3+4, 5+6) or 10000 (2 of the 8 base Nightfarers, level 6; the game holds 2 bots). No argument: the last request, the bot manager's state and the OFF-5 match line (`bot_match: <stage>, n of 5 kept`). Logs `bots:`. Dev only |
| `bots_hostile` | `[0\|1]` | Offline match bots attack you (1, default: team 29, they also fight each other) or fight on your side (0: the game's ally team 2). Takes effect at once for the bots already there and stays for the next expeditions until the game restarts. No argument shows the bot lines. Dev only |
| `bots_clear` | | OFF-4 spike, offline mode only: runs `2003[98]`, which removes every bot the game spawned. Dev only |
| `clock` | `[night\|circle\|end\|hh:mm[:ss]]` | M5: the expedition's play-area clock (`clock: 14:03:10 (running, speed …)`). With a time the game jumps its own clock there on its next frame (its set-time mailbox, MEMORY_MAP §16): `circle` = 20:20 (final circle starts 20:24), `night` = 22:55 (night events from 23:00), `end` = 23:58:30 (night flag at 23:59). Expedition only. Logs `clock:` (one line per game hour, and "the game applied …" after a jump). Dev only |
| `night_end` | | M5-3: end the current night now. Sets our flag 10007400, which the no-boss night events (`gamedata/emevd-nights.json`) wait for besides their 180 s timer; the game's own morning follows (heal, flasks, grace, next day). Acts only at night and only in the host's game. Logs `nights:`. Dev only |
| `flag` | `<id> [0\|1]` | M5-4 research: read any event flag (`flag 7005`); with a value, write one of ours (10007000..=10007999) or the game's 7000 (game clear), 7001 (game over), 7502 / 7507 (night 1 / 2 done), in this game only (for Nucleus runs, where our event files are not loaded). Logs `flag:`. Dev only |
| `players` | | Every player in this game's ChrSet: slot, `pN`, id, team value, relation (me / ally / rival) |
| `team_set` | `<value> [target]` | Write team_type on this game's copy of the target's players (default `me`) |
| `team_restore` | `[target]` | Put back the team_type captured before the first write (default `me`) |
| `team_persist` | `0\|1 [target]` | Keep re-writing the last written team_type on the target's players (1), or stop (0) |
| `pvp` | `0\|1` | Resume (1), or pause and put back every player's captured original (0; the old F12) |
| `mm_queue_solo` / `mm_queue_duo` / `mm_queue_squad` | | Join that matchmaking queue |
| `mm_leave` | | Leave the queue or the ready match |
| `mm_status` | | The matchmaking status line |
| `mm_join` | `link\|invite\|direct\|unlock_public\|unlock_invisible\|relock` | One join-method attempt for the ready match (dev, `mm_join = true`) |
| `mm_sc_create` | | Create a lobby the game's way; does Seamless Co-op adopt it? (dev, `mm_sc_create = true`) |
| `mm_quiet_test` | | Suppress the Steam invite window for 60 s (dev, `mm_quiet_invite = true`) |
| `revive_fail` | `[n]` | T-040 test switch: the next n rival re-down tries fail ("forced by console") without reading memory; no argument shows how many are left |
| `early_redown` | `[0\|1]` | T-040 follow-up experiment: knock a rival-revived player down during the get-up (1) instead of after it (0, default); no argument shows the state. Dev only, resets at launch |
| `bar_trace` | `[0\|1]` | T-041 step 1 trace: while LOCAL is downed, a hardware write watch (slot 3, only if free) logs which instruction writes the revive bar, as `T-041 BAR WRITE #n: rip=exe+0x… value=… tid=…`; no argument shows ON/OFF and the hit count. Dev only, resets at launch |
| `hawk_env` | | HAWK-3: logs the env object (`exe+0x3C1D3D8`) and whose code its switch at `+0x10` is (game RVA, ours, or another module's). Reads only. Dev only |
| `hawk_trace` | `[0\|1\|all]` | HAWK-3: swaps the env switch for our pass-through and logs every change of the hawk answers 377/384/394/395 per character (`HAWK-3 0x… (LOCAL) env 377 0 -> 4`); `all` also logs every other id the rider asks (noisy ids are muted). No argument shows the state. Dev only |
| `hawk_drop` | | HAWK-3: answers `GetBirdEventID` = 2 ("route ended") to the rider for 1.5 s, so the start hawk drops the player. Dev only |
| `hawk_tree` | `0\|1` | HAWK-3: start-hawk rides (377 = 4) answer 1 (tree hawk) instead; set before the ride. Acts on every rider (solo tests only). Dev only |
| `hawk_cruise` | `0\|1` | HAWK-4 (on by default since HAWK-2: `[hawk] enabled` in `nrpvp.toml`; this overrides it for the session): start rides run as tree rides (answer 1 for 4) with 394 answered 0 (stick hidden), so the hawk holds the route; once the real 394 reaches the dive hold, the stick passes through and 384 is capped at 1999 for the dive time, then the vanilla off-route fall. Set before the ride. Log lines `HAWK-4 phase …`. Dev only |
| `hawk_dive` | `[hold] [ms]` | HAWK-4 (defaults from `[hawk] dive_hold` / `dive_secs`): dive trigger in 394 units (default 400; 394 rises about 1,600 per second of stick) and dive length in ms (default 7000 since 2026-10-22); no argument shows the state. Dev only |
| `hawk_start` | `0\|1\|2` | HAWK-4: ride start under cruise/tree: 0 shows the tree start animation, **1 cuts it** (default: `IsAnimEnd` answered TRUE for 0.4 s), 2 keeps the vanilla start and switches to the tree ride 2 s later (with the cut). Dev only |
| `hawk_boost` | `0\|1` | HAWK-4: during a dive the script's hawk speed goes to x1.3 (`GetSpEffectID(99230)` answered TRUE on the speed check only; default 1). Dev only |
| `hawk_off` | | HAWK-3: everything off (the HAWK-2 cruise too, until the next launch) and the game's switch put back. Dev only |
| `revive_block` | `[0\|1]` | T-041 prototype: while LOCAL is downed, a hardware breakpoint on `exe+0x675159` clears the "revive completed" flag when the reviver is a rival, so no get-up starts; no argument shows ON/OFF and the block count. **On by default** (both builds run the block); the command only switches it off for a comparison run. Dev only |
| `pos_next` / `pos_save` / `pos_go` | | Snap-back test (Task D): select a candidate, save your values, write them back (you only) |
| `whoami` (pipe only) | | `pid <pid> <DLL folder>`; `nrcon list` uses it |

Targets: `me`, `all`, `allies`, `rivals`, `p1`…`p6`, `s0`…`s5` (§3.1).

---

## 1. Where the weight actually is

You asked for "an in-game command line, simpler and lightweight". Two parts of that need correcting first:

- **The heavy part is not the panel, it is the renderer.** To draw anything over the game (a panel or a console line) we hook DirectX 12
  through hudhook/ImGui. A CS2-style in-game console would keep exactly that hook. Turning buttons into a text box saves code, not weight.
- **The in-game overlay does not draw under Nucleus** (STATUS traps). Nucleus is where every multi-player test on your PC runs. An
  in-game-only console would be blind in the setup you use most.

So the valuable part is the **command layer**: one place that turns `kill`, `team_set 29`, `mm_queue_solo` into actions. The window you
type in is just a frontend. We already have half of the layer: `overlay::shared::Command` is the list of actions the panel's buttons
send to the main loop.

## 2. Proposal

```
 frontends                          command core (dev feature)                 executors
┌───────────────────────┐          ┌───────────────────────────────┐          ┌────────────────────────┐
│ nrcon.exe  (pipe)     │──text──► │ parse → look up in registry → │──Cmd───► │ main loop (game memory)│
│ in-game console (F1)  │──text──► │ check args → build Command    │──Cmd───► │ Steam thread (mm)      │
│ autoexec.cfg          │──text──► │ reply text ◄──────────────────│◄─reply── │ console itself (help)  │
└───────────────────────┘          └───────────────────────────────┘          └────────────────────────┘
```

**Frontend 1, first: external console over a named pipe.** The DLL opens `\\.\pipe\nrpvp-<pid>` (local user only). A small
`nrcon.exe` (a new `nrscan` subcommand or its own binary) attaches to one or all running games:

```
nrcon                      # interactive, picks the only running game
nrcon -i 1 "kill"          # Nucleus instance 1
nrcon --all "status"       # every running instance
```

Why it comes first:
- It works under Nucleus, in both windows at once, with no focus juggling (the END-key trap goes away).
- **Claude can drive it.** An agent can type `status`, `players` or `mm_status` into a running game and read the reply, instead of
  asking you to press F9 and paste a log. That removes human steps from every RE and test loop, and is the biggest payoff in this
  document.
- No rendering, no input capture, nothing new hooked in the game.

**Frontend 2, later: in-game console.** A one-line ImGui input plus the log tail, toggled with a key (CS uses `~`; SC already binds F4).
It reuses hudhook in dev builds only. Nice for solo play; not needed for testing.

**Frontend 3: `autoexec.cfg`.** Commands next to the DLL, run on world load (`mm_queue_solo`, `rival_hit_seconds 2`). It makes
repeatable test setups a file instead of instructions.

## 3. Command shape (design; built commands are in §0)

CS style (Leonardo, 2026-10-20): **a command name is one word with underscores, never spaces** (`mm_queue_solo`, `team_set`,
`bleed_clock`). Only values follow after a space, as in CS (`sv_cheats 1`, `team_set 29 all`). `;` chains commands, quotes wrap text
with spaces. A setting ("cvar") is typed by its own name: alone it prints the value, with a value it sets it (`rival_hit_seconds` /
`rival_hit_seconds 2`). Every command has a help line and runs on exactly one executor.

| Command | Does | Executor | Exists today as |
|---|---|---|---|
| `help [name]` | List commands / one command's help | console | — |
| `echo "text"` | Write `MARK #n: text` to the log (your `console.log`) | console | `Command::Mark` |
| `status` | Build, world state, player count, loop time, queue state | main loop | heartbeat line |
| `players` | ChrSet table: slot, id, team, HP, downed, rival/ally | main loop | overlay table |
| `team_set <v> [slot\|all]` / `team_restore [slot\|all]` | Write or restore team_type | main loop | `Set`, `SetAll`, `SetFor`, `Restore*` |
| `pvp 0\|1` | Pause or resume the automatic rival marking | main loop | `SetPvpve` |
| `kill` | Local player dies (§7 Q2) | main loop | — (HP 0 = down only) |
| `down` | Local player goes down (HP 0) | main loop | rule v1's write |
| `bleed_clock <s>` | Set own bleed-out seconds while downed | main loop | nrscan `poke` |
| `mm_queue_solo` / `mm_queue_duo` / `mm_queue_squad` / `mm_leave` / `mm_status` | Matchmaking | Steam thread | `MmQueue`, `MmLeave` |
| `flag_get <id>` / `flag_set <id> 0\|1` | Read or set an event flag | main loop | `game::flags` |
| `msg_show <k>` | Show matchmaking message k in game | main loop | `ui_msg` flags |
| `<setting> [value]`, `cvar_list` | Read or change a live setting (`rival_hit_seconds`, intervals) | main loop | toml only, restart |
| `pos_save` / `pos_go` | Snap-back test (T-028) | main loop | F7 / F6 |
| ~~`dump_players` / `discover`~~ | Dropped: R5 deleted the discovery scanner and the dumps (findings are in MEMORY_MAP) | — | — |

`kill` and every other write keep today's gates (PlayerIns vtable check, own client only).

### 3.1 Targets (Leonardo's idea, 2026-10-20)

Player commands take an optional target, CS style: `kill`, `kill p2`, `team_set 29 rivals`, `down all`. Default is `me`.

| Target | Means |
|---|---|
| `me` | The local player (ChrSet slot 0) |
| `p1`…`p6` | A player by session number: `p2` = id `0x6002…` (MEMORY_MAP §3), the same on every game once the expedition has loaded |
| `s0`…`s5` | A ChrSet slot on *this* game (differs per game; for debugging) |
| `allies` / `rivals` / `all` | From the current roster |
| a name | **Later.** No name is located in memory yet; it needs the Steam ID ↔ player mapping (MM-6). Until then `players` shows ids |

**The catch: some writes only work on the owner's game.** HP, the bleed-out clock and the downed rule only take effect on the
victim's own client (MEMORY_MAP §5–6); `team_type` is per client by design. So:
- `team_set`, `team_restore`, `players`, reads: run locally against any target.
- `kill`, `down`, `bleed_clock` on someone else: **routed by `nrcon`**, not by the game. `nrcon kill p2` asks every running instance
  for its local player id and sends `kill` to the one whose `me` is `p2`. That covers Nucleus and anything else on this PC. Inside the
  game, `kill p2` answers "p2 is not this game's player; use `nrcon kill p2`".
- Across two PCs it needs the mod-to-mod channel (S4), which does not exist yet. Until then, remote kills between PCs are out of scope.

## 4. Where it lives in the code

```
crates/nrpvp/src/console/        (behind the `dev` cargo feature, see ARCHITECTURE.md)
  mod.rs        registry: name → { help, args spec, executor, fn(args) -> Result<Cmd, Usage> }
  parse.rs      tokenizer (quotes, `;`); pure, unit-tested
  pipe.rs       named-pipe server thread; one line in, reply lines out
  autoexec.rs   reads autoexec.cfg, queues lines on world load
crates/nrcon/  (or `nrscan con`)  the client
```

The executors stay where they are. The main loop already drains a command queue each tick; the console adds a reply channel so text
comes back to whoever sent it. The ImGui panel's buttons and the F-key hotkeys are deleted once their commands exist (the hotkey
list in `config.rs` shrinks by about 20 keys).

## 5. Rules

- Dev builds only (`--features dev`). The player DLL has no pipe and no parser.
- The pipe accepts the current user only and is local only. No TCP.
- Every console command is logged (`console: <line> -> <reply>`), so test logs show what was typed.
- Commands never block the main loop: they are queued and run on the next tick (50 ms).

## 6. Plan (cards)

| Card | Content | Acceptance |
|---|---|---|
| CON-1 | Command core + parser + registry, mapped onto the existing `Command`s; `help`, `echo`, `status`, `players`, `team_*`, `pvp`, `mm_*` | Unit tests; solo: `nrcon status` answers from a running game |
| CON-2 | Pipe server + `nrcon` with `-i` / `--all` | Solo in Nucleus: one command reaches both instances |
| CON-3 | New commands: `kill`, `down`, `bleed_clock`, `flag_*`, `msg_show`, settings, `pos_*` | Solo: each one, checked on screen |
| CON-4 ✅ | Delete the ImGui panel buttons and the F-key hotkeys (done with R5, 2026-10-20) | Dev build has no hotkey thread |
| CON-5 (optional) | In-game console line (ImGui) + `autoexec.cfg` | Solo |

**Agents and skills:** no new agent; CON cards fit the existing `implementer`. Once CON-1 lands, a short **skill "add a console
command"** (registry entry, executor, help text, test, log line) is worth writing, because every later feature will want one. Not
before: it would describe code that does not exist yet.

## 7. Decisions (2026-10-20)

Leonardo delegated these; Claude decided.

1. **Frontend order:** pipe + `nrcon` first (CON-1, CON-2). The in-game ImGui console line (CON-5) comes later in dev builds, for the
   CS feel when playing solo. It is the same command layer, so nothing is thrown away.
2. **`kill` and `down` are separate commands.** `down` = HP 0 (bleed-out runs); `kill` = HP 0, then the bleed-out clock to 0 once
   downed. Both use only verified writes (MEMORY_MAP §5–6) on the local player.
3. **Player build: no console at all.** It lives behind the `dev` cargo feature.
4. **Syntax: CS style, underscores in names** (Leonardo: `mm_queue_solo`, no spaces inside a name; values after a space; `;` chains).
   A leading `/` is accepted and ignored, so `/kill` works too.
5. Command list: §3 as written; new commands are added with their feature from now on.
6. **Targets** (§3.1): `me` / `p1`…`p6` / slots / `allies` / `rivals` / `all`. Owner-only writes on another player are routed
   by `nrcon` to that player's game; names come after MM-6. Targets ship in CON-1 (parser) and CON-3 (routing).
