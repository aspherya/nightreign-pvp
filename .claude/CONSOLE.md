# Developer console

**Status (2026-10-20): CON-1 and CON-2 built** (command core, named pipe, `nrcon.exe`); not yet run against a live game. CON-3..5
planned. Replaces the ImGui debug panel and the F-key hotkeys with text commands, CS2 style. Developer builds only (`dev` feature +
`dev_mode = true`): the player zip never has it.

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
| `mark` | `[text…]` | Write `MARK #n: text` to the log; dev mode also starts the MARK dump (the old Mark key) |
| `status` | | World loaded or not, local player and its team value, PvP mode, matchmaking line, pid |
| `players` | | Every player in this game's ChrSet: slot, `pN`, id, team value, relation (me / ally / rival) |
| `team_set` | `<value> [target]` | Write team_type on this game's copy of the target's players (default `me`) |
| `team_restore` | `[target]` | Put back the team_type captured before the first write (default `me`) |
| `pvp` | `0\|1` | Pause (0) or resume (1) the automatic rival marking |
| `mm_queue_solo` / `mm_queue_duo` / `mm_queue_squad` | | Join that matchmaking queue |
| `mm_leave` | | Leave the queue or the ready match |
| `mm_status` | | The matchmaking status line |
| `discover` | | Run the character discovery and dump (the old F9) |
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
| `dump_players` / `discover` | Discovery and dumps | discovery worker | F9, histogram |

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
| CON-4 | Delete the ImGui panel buttons and the F-key hotkeys | Dev build has no hotkey thread |
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
