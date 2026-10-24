# Scenario catalog (automated one-PC tests)

**Owner of:** which scenarios exist, what each proves, how to run them and how to add one. Tool usage: [README.md](README.md); what `nro`
reads from the mod: [CONTRACT.md](CONTRACT.md). Files are `scenarios/<name>.toml`; the name is the file name.

**Before you add or change a feature:** read the table, run the scenarios your change can break, and add or extend one for the new
behaviour (a TASKS card's acceptance should name its scenario). Done = `scripts\regress.ps1 <name>` PASS and an entry in `testing-log/`.

Run (one PC, Steam running, `nro desk` started in a normal terminal, dev build in `dist\`: TOOLS.md):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\regress.ps1                           # every scenario, one PASS/FAIL line each
powershell -ExecutionPolicy Bypass -File scripts\regress.ps1 night1-count night2-win   # some of them
target\release\nro.exe run scenarios\night1-count.toml                                 # one, full JSON report (runs\<id>\report.json)
```

Each run launches its own game (about 2.5-4 min), so run only what the change touches; run all before a release zip. `PASS` = every
`expect` / `wait` held; a failed `expect` makes the run FAIL, a timed-out `wait` ABORTS it. Reports, screenshots and the mod log of a run:
`runs\<id>\` (gitignored).

## Catalog

All use `mode = "offline"`, `mm_queue = false` (set in `[config]`) and a dev DLL; all start with `navigate = "title_to_world"` and
`navigate = "expedition"` (the offline Solo expedition, off the hawk). Puppets = the AFK players of `game/puppet.rs`.

| Scenario | Proves | Setup and key steps | Time |
|---|---|---|---|
| `puppets-smoke` | Puppet players exist and behave as players: listed as rivals p2..p5 (`players`, `puppet_list`), a puppet at 1 HP goes **down** after one swing, the mod logs `DOWN` / `T-039 BLOCK`, it bleeds out (`UP (downs=1, hp=0`), Seamless Co-op's destroy sweep is skipped | `god 1`, 4 puppets, `puppet_ring`, screenshot, `puppet_front`, `puppet_hp n 1`, one `click`, wait on `puppet_list` / `log` | 2.5 min |
| `night1-count` | **Night 1 is decided by count** (M5-5): with LOCAL + 3 puppets, when the puppets are down the night ends (`night 1 decided … last one standing … won`), the winner heal flag fires, `night 1 is over; morning` follows | `god 1`, 3 puppets in front, `clock end`, wait `night_status` = night 1, `puppet_hp 1..3 0`, wait on `log` | 3 min |
| `night1-local-dies` | **Solo real death hold and release** (M5-9) with puppets present (hardware-slot budget): `hp0` at night → `start step HELD` while 3 puppets stand → `night_end` → `RELEASED` through the gate → the morning move happens only after the clock reads 07:00 → alive, `night_status` = day 2 / death released; no `no free hardware slot` | `god 1`, 3 puppets in a ring, `clock end`, `hp0`, `night_end`, waits on `log`, `expect clock` = 07: | 3.5 min |
| `night2-win` | **Night 2 decided by count, LOCAL last** (M5-5 night-2 item): night 1 via `night_end`, Day 2, `clock end`, puppets 1..3 to 0 HP → `night 2 decided … (this game's player won, host)` → the game's ending (Result screen), `night_status` = match over | as `night1-count`, then `night_end`, wait Day 2 and the 07: clock, `clock end`, screenshots of the ending | 3.5 min |
| `night2-loss` | **Night 2 loss with LOCAL dead and held**: puppets 1-2 down, `hp0` on LOCAL, puppet 3 is the last → `night 2 decided … lost`, `start step HELD` (never released), the Result screen | as `night2-win` but `hp0` instead of downing puppet 3 | 3.5 min |
| `night2-win-end` | As `night2-win`, then the **end flow**: Result → Reward → `F` (Next) ×4 → back at the Roundtable (`status` team_type 1); screenshots `end1..4`, `roundtable` | `night2-win` steps + `keys = "F; wait 3s"` and screenshots, `wait status` = `team_type 1` | 5 min |
| `night2-loss-end` | As `night2-loss` (LOCAL dead and held), then the same end flow; the loser reaches the same Roundtable (reward is smaller) | `night2-loss` steps + the end-flow steps | 5 min |

Not covered by any scenario (needs two real PCs, see TESTING §6): the 102130 kill itself (offline solo LOCAL carries SpEffect 6999500, so
die-at-once stays off), what the other screen shows, Seamless Co-op host/client, banners of both screens, real latency.

## Step vocabulary (`crates/nro-core/src/scenario.rs`)

`[config]` keys are written over the staged `nrpvp.toml` (top-level keys only). Steps run in order:

| Step | Meaning |
|---|---|
| `console = "<line>"` | one mod console command (CONSOLE.md §0); the reply is recorded |
| `wait = { console, contains, timeout_s, every_ms }` | repeat a console line until the reply contains the text (all needles, case-insensitive); timeout = ABORT |
| `expect = { console, contains, absent }` | one console line whose reply must match; a miss = FAIL, the run continues |
| `keys = "ENTER; E*3; click 0.5 0.5; wait 1500ms"` | keys and mouse for the game window (needs `nro desk`) |
| `navigate = "title_to_world"` or `"expedition"` | built-in macros |
| `puppets = n`, `puppet_ring = r`, `puppet_front = d` | make 1..5 puppets; place them around / in front of LOCAL (metres) |
| `screenshot = "name"` | `runs\<id>\shots\name.png` (open it with the Read tool and look) |
| `sleep_ms = n` | pause |

## Writing a new scenario

1. Copy the closest scenario above; keep the three `[config]` lines and `ui_flag_watch = false` (its event bursts flood `log 300`).
2. Reach the state with **commands, not keys** where you can: `clock end` / `clock circle`, `night_end`, `god 1`, `hp0`, `puppet_hp <slot> 0`
   (a sword swing does not reach every puppet), `flag <id> [0|1]`. Keys are for the title screen and for what has no command.
3. Assert on **state commands** (`night_status`, `clock`, `puppet_list`, `players`, `status`) first, and on `log 300` text only for a
   line that must have been written (`night 1 decided`, `RELEASED`); the 300-line tail can lose a line during event bursts.
4. Add a negative check (`absent = [...]`) for the failure you fear (`no free hardware slot`, `panic`).
5. Run it twice; a step that is flaky after two attempts goes to the friend-session list instead (TESTING §6), not into the set.
6. Add a row to the catalog above (what it proves, key steps, time), name the scenario in the card, and log the run (`testing-log/`).
   A new console command a scenario needs: `add-console-command` skill.
