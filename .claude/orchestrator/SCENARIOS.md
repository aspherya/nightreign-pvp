# Scenario catalog (automated one-PC tests)

**Owner of:** which scenarios exist, what each proves, how to run them and how to add one. Tool usage: [README.md](README.md); what `nro`
reads from the mod: [CONTRACT.md](CONTRACT.md). Files are `scenarios/<name>.toml`; the name is the file name.

**Before you add or change a feature:** read the table, run the scenarios your change can break, and add or extend one for the new
behaviour (a TASKS card's acceptance should name its scenario). Done = `scripts\regress.ps1 <name>` PASS and an entry in `testing-log/`.

Run (one PC, Steam running, `nro desk` started in a normal terminal, dev build in `dist\`: TOOLS.md):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\regress.ps1                           # every scenario, one PASS/FAIL line each
powershell -ExecutionPolicy Bypass -File scripts\regress.ps1 day1-free match-win   # some of them
target\release\nro.exe run scenarios\day1-free.toml                                    # one, full JSON report (runs\<id>\report.json)
```

Each run launches its own game (about 2.5-4 min), so run only what the change touches; run all before a release zip. `PASS` = every
`expect` / `wait` held; a failed `expect` makes the run FAIL, a timed-out `wait` ABORTS it. Reports, screenshots and the mod log of a run:
`runs\<id>\` (gitignored).

## Catalog

Single-game scenarios: all use `mode = "offline"`, `mm_queue = false` (set in `[config]`) and a dev DLL; all start with `navigate = "title_to_world"` and
`navigate = "expedition"` (the offline Solo expedition, off the hawk). Puppets = the AFK players of `game/puppet.rs`.

| Scenario | Proves | Setup and key steps | Time |
|---|---|---|---|
| `puppets-smoke` | Puppet players exist and behave as players: listed as rivals p2..p5 (`players`, `puppet_list`), a puppet at 1 HP goes **down** after one swing, the mod logs `DOWN` / `T-039 BLOCK`, it bleeds out (`UP (downs=1, hp=0`), Seamless Co-op's destroy sweep is skipped | `god 1`, 4 puppets, `puppet_ring`, screenshot, `puppet_front`, `puppet_hp n 1`, one `click`, wait on `puppet_list` / `log` | 2.5 min |
| `day1-free` | **Free Day 1** (RW-4): the clock runs slowed (`day1_secs = 120` here: 07:00 to 11:50 in 120 s), no night and no tide; the host then sets 7502, the game's morning brings Day 2 at 07:00 and the speed comes back after the reset | `[config] day1_secs = 120`, waits on `night_status` = free day 1, log `Day 1 is over`, `clock` 07:, `night_status` = day 2 | 4 min |
| `defeat-mob` | **A non-player death is vanilla** (RW-2): `flag 10007420 1` (die at once, so offline's revive effect does not catch it) + `hp0` with no rival hit → `died to an enemy`, nothing held, the game's own respawn, `LOCAL in`, `mob deaths 1`, never `defeated` | 3 puppets in a ring, `flag 10007420 1`, `hp0`, waits | 3.5 min |
| `defeat-rival` | **A rival's kill is a defeat for good** (RW-1): `rival_hit` then `hp0` → `defeated by a rival`, `start step HELD`, `death held`, after `defeat_exit_secs` (15 here) the defeat screen (`FLAG_LOSE`), then Result, Reward, Next presses and the Roundtable | as above plus the end flow of `match-win-end` | 5.5 min |
| `spectate` | **A defeated player watches** (RW-1 / M5-7): after `rival_hit` + `hp0` the held body is moved next to the nearest living puppet every 100 ms (`night_status` = `watching 0x…`), the follow camera shows that puppet from behind (screenshot `watching2`), F10 asks for the defeat screen and the run leaves the expedition | 3 puppets in a ring, `flag 10007420 1`, `rival_hit`, `hp0`, waits on `night_status`, `keys = "F10"` | 3.5 min |
| `esc-defeated` | **A defeated, held player can open the Esc menu** (RW-1): as `spectate`, then Esc opens the menu (`CSMenuMan+0x48` = 0x11, screenshot `esc_open`), Esc again closes it | `rival_hit`, `hp0`, wait `the Esc menu is unlocked`, `keys = "ESC"`, `peek` | 3.5 min |
| `maphide` | **No other player on the map / compass in a Solo match** (RW-9): `pvpve = true`, with 3 puppets the automatic mode empties the other-players list (`maphide` = `0 item(s), hidden by us`), `maphide 0` gives the 2 items back, `maphide auto` empties it again; the vanilla `exe+0x1FE4C0` bytes are checked; screenshots `compass_free` / `compass_hidden` (a puppet is not a network player, so no pin shows by itself: forcing one is by hand, MEMORY_MAP §20) | `peek`, `wait` on `maphide`, `maphide 0` / `auto`, screenshots | 3 min |
| `match-win` | **The match ends when one team is left, on Day 1** (RW-1): LOCAL + 3 puppets, the puppets at 0 HP for 30 s → `match decided … won` (no night needed), the game's ending | 3 puppets in front, `puppet_hp 1..3 0`, wait on `log` | 3.5 min |
| `match-win-end` | As `match-win`, then the end flow: Result → Reward → Next ×4 → back at the Roundtable | `match-win` steps + `keys = "F; wait 3s"` and screenshots, `wait status` = `team_type 1` | 5.5 min |
| `match-loss` | **LOCAL defeated while two puppets are also out**: the last puppet wins → `match decided … lost`, `start step HELD`, no second defeat screen | `puppet_hp 1 0`, `puppet_hp 2 0`, `flag 10007420 1`, `rival_hit`, `hp0` | 4 min |

**Multi-instance (`mi-*`, MULTI-INSTANCE.md):** two emulated-Steam games on one PC (`[[instance]]` tables). Different setup: **Steam closed**,
`nro desk` running, ~7 GB free RAM per game, run by name only (`regress.ps1` skips `mi-*` unless named), and `nro` must run from a normal
shell, not an agent's sandboxed Bash (it cannot open the game process, so the P-module check fails).

| Scenario | Proves | Setup and key steps | Time |
|---|---|---|---|
| `mi-join` | **Seamless Co-op host/client on one PC**: instance 2 joins instance 1's SC lobby over the gbe_fork emulator with no client keys; both consoles `world loaded, 2 player(s)`, `team_type 1`, `players` = me + rival; the host's lobby probe lists both Steam ids | `[[instance]]` 1 host, 2 client; `title_to_world` on both; `navigate = "invite_members"` on 1 (tp to the table, F, F2, DOWN, F; checks the menu word and `LobbyCreated_t`); waits on both `status`, the host's `log 300` | 6 min |
| `mi-queue` | **Our own Solo matchmaking on one PC**: both instances `mm_queue_solo`, they meet in our queue lobby over gbe_fork, MATCH READY, the host's Invite Members, the joiner joins by the mod's invite, ALL PRESENT on both, both commence, same expedition, rivals marked 29, `mm_status` IN EXPEDITION | `[[instance]]` 1 host, 2 client; config `mm_queue`/`mm_join`/`mm_quiet_invite` on, `mm_match_players = 2`, `mm_host_create = false` (emulated only: preflight P7 and nro's identity gate); `navigate = "invite_members"` on 1, `navigate = "commence"` on both | 6 min |
| `mi-rw9-rivals` | **RW-9 with two real instances**: `mi-queue`'s flow to one expedition as rivals, then `rivalhud` shows the three hooks active, `hawk_drop` on both, `tp_player 1 0 -4` puts player 2 in front of player 1; every site `hiding a rival now` on both; `rivalhud 0` brings party bar / name tag / compass back (screenshots `p*_hidden`, `p*_shown`); lock-on (`Q`) on player 2 shows only the target's name tag (`p2_locked_on`) | `[[instance]]` × 2, `mm_queue_solo`, `navigate` invite_members / commence, `rivalhud`, `hawk_drop`, `tp_player`, `keys = "Q"` | ~8 min |
| `mi-rw9-markers` | **RW-9 map markers with two real instances**: `mi-rw9-rivals`' flow to one expedition as rivals; player 1 pins the map (M, `mclick` = middle button), keeps it (`map list [1]`), player 2 gets the record (`records [1]`) but its map list stays empty and both marker hooks are `hiding a rival now`; the same the other way; `rivalhud 0` on player 2 brings the pin back (`p2_*_shown`), a moved pin is hidden again | `[[instance]]` × 2, as `mi-rw9-rivals` up to `tp_player`, then `keys = "M 300ms"`, `keys = "mclick fx fy"`, `rivalhud` waits with `contains` / `absent` | ~9 min |
| `mi-nosc-join` | **NET-5, NO Seamless Co-op** (research scenario, expected to FAIL at Invite Members until NET-7): run with env `NRO_NO_SC=1` (profile without `nrsc.dll`, `savefile = NRNOSC.sl2` per emulated id; seed that file from the instance's own `.co2` or the game shows Bandai's privacy dialog). Both instances reach the Roundtable; Invite Members is dead (sign-in gate, NETWORKING §2b) | `NRO_NO_SC=1`, `navigate = "invite_members"` on 1, then `world loaded, 2 player(s)` on both | ~4 min |
| `mi-net-ping` | **NET-1 data channel** (PASS 2026-10-30, also with `NRO_NO_SC=1`): two instances, `net = true`, `net_peer` each other, `net_ping 120` both ways over `ISteamNetworkingMessages` channel 0x4E52; `pongs=120`, no errors, no strangers, no undecodable | `net_peer`, `net_ping`, `net_status` | ~4 min |
| `mi-net-join` | NET-1 **next to a live SC session** (PASS 2026-10-30): `mi-join`'s flow, then the same 120 pings each way; both still list 2 players | as `mi-join` + net steps | ~6 min |
| `mi-net-verdict` | **NET-2 facts** next to a live SC session: the roster comes from the SC lobby (no `net_peer`), instance 1 publishes `boss 4012 team 1` three times, instance 2 records it once and acks it and cannot overwrite it | as `mi-join` + `net_verdict` | ~6 min |
| `mi-rw3-items` | **RW-3 defeat drops, two real instances** (PASS 2026-11-01): `mi-queue`'s flow to one expedition as rivals, hawk drop to the ground; `drop_gear dry` shows each game's drop list (weapons and inventory items) and who is the host; the host drops its own gear (`drop_gear`: `itemdrop: on the game thread ... rax 0x1`, new `MapItemMan` slots); player 2 is defeated by a "rival" (`flag 10007420 1`, `rival_hit`, `hp0`): its game publishes the drop (topic 5), the host queues and makes it (`rax 0x1`) | `drop_gear`, `rival_hit`, `hp0`, `night_status`, screenshots | ~9 min |
| `mi-rw3-drop` | **Lab, not a test** (RW-3 E1): `teardown = "keep"`, no asserts; two instances reach one expedition as rivals and stay open for `item_drop` by hand. Skipped by `regress.ps1` | as `mi-queue` | ~5 min |
| `mi-rw8-runes` | **RW-8 runes only inside the team, two real instances** (PASS 3/3, 2026-10-28): `mi-queue`'s flow to one expedition as rivals, `runeshare` shows both credit hooks; player 1 `mob_kill` (test aid: LOCAL as the enemy's attacker + HP 0) with player 2 4 m away; player 1 `+N` / `0 blocked`, player 2 `+0` / `1 blocked = N rune(s)` and `Unknown -> BLOCKED` in its log; control `runeshare 0` on player 2 → the next kill pays it (`+N`) | `runes_mark`, `mob_kill`, `runeshare`, screenshots | ~8 min |
| `mi-rw5-orb` | **Parked feature (RW-5 is off by default, the scenario sets `orb_auto 1`).** RW-5 first-to-loot, two real instances (PASS 2026-10-31): both kill the same Dormant Power boss (`boss_orb`, lock-on Q, hits); instance 1 `orb_go` + F opens the screen = claim; instance 2 logs `(rival) -> off`, its `orb_list` says OFF; `net_status` facts 1/1 | `boss_orb go/hp`, `keys`, `orb_go`, waits on `orb_list` / `log` | ~9 min |

Not covered by any scenario (needs two real PCs, see TESTING §6): the rival kill with real hits (the scenarios fake it with `rival_hit`; offline solo LOCAL carries SpEffect 6999500, so a death needs `flag 10007420 1`), what the other screen shows, Seamless Co-op host/client, banners of both screens, real latency.

## Step vocabulary (`crates/nro-core/src/scenario.rs`)

`[config]` keys are written over the staged `nrpvp.toml` (top-level keys only). Steps run in order:

| Step | Meaning |
|---|---|
| `console = "<line>"` | one mod console command (CONSOLE.md §0); the reply is recorded |
| `wait = { console, contains, absent, timeout_s, every_ms }` | repeat a console line until the reply contains the text (all needles, case-insensitive) and none of `absent`; timeout = ABORT |
| `expect = { console, contains, absent }` | one console line whose reply must match; a miss = FAIL, the run continues |
| `mark = "<name>"` | remember the byte length of this instance's `runs\<id>\mod\nrpvp.log` now (marks are per instance) |
| `wait_log = { contains, absent, timeout_s, since }` | poll the mod log FILE (not the console tail) until the text after `since` has all `contains` (case-insensitive, complete lines only); `absent` text after `since` aborts at once; timeout = ABORT. `since` = `"step"` (default: the log length when the previous step finished), `"run"` (whole log) or `"mark:<name>"` |
| `expect_log = { contains, absent, since }` | the same, one look, no timeout; a miss = FAIL, the run continues. `contains` may be empty (only `absent`) |
| `keys = "ENTER; E*3; click 0.5 0.5; mclick 0.6 0.4; M 300ms; mousemove 200 0; wait 1500ms"` | keys and mouse for the game window (needs `nro desk`; `mclick` = middle button, `K 300ms` holds a key: the map needs `M 300ms`; `mousemove dx dy` = relative mouse-look in counts, turns the game camera) |
| `navigate = "title_to_world"`, `"expedition"`, `"invite_members"` or `"commence"` | built-in macros (`expedition` ends by sending the console `god 1` so no wolf kills the character; `god = false` on that step skips it, for scenarios where a mob should kill: `defeat-mob`; `hp0` turns god off by itself; `invite_members`: Roundtable host → Invite Members pressed, position-independent; `commence`: the Commence flow, used by `mi-queue`) |
| `[[instance]]` `n`, `role`, `[instance.config]` | emulated instances 1..3, launched in file order; instance config goes over `[config]` |
| `instance = N` (in any step) | the instance the step acts on (default: the first `[[instance]]`); report lines start with `[iN]` |
| `puppets = n`, `puppet_ring = r`, `puppet_front = d` | make 1..5 puppets; place them around / in front of LOCAL (metres) |
| top level: `name`, `about`, `tags = ["smoke", "rules", "hud", "mi", "slow", "lab"]` (filter with `nro scenarios --tag X` / `regress.ps1 -Tag X`; the default run skips `mi` and `lab`), `teardown = "kill"` (default) or `"keep"` | the scenario's title and one-line purpose; `keep` leaves the games running after the last step (labs) |
| `screenshot = "name"` | `runs\<id>\shots\name.png` (open it with the Read tool and look) |
| `sleep_ms = n` | pause |

## Writing a new scenario

1. Copy the closest scenario above; keep the three `[config]` lines and `ui_flag_watch = false` (its event bursts flood the log).
2. Reach the state with **commands, not keys** where you can: `clock end` / `clock circle`, `night_end`, `god 1`, `hp0`, `puppet_hp <slot> 0`
   (a sword swing does not reach every puppet), `flag <id> [0|1]`. Keys are for the title screen and for what has no command.
3. Assert on **state commands** (`night_status`, `clock`, `puppet_list`, `players`, `status`) first, and on log text (`wait_log` / `expect_log`, which read the log file) only for a
   line that must have been written (`night 1 decided`, `RELEASED`); the console's 300-line tail can lose a line during event bursts, the file does not.
4. Add a negative check (`absent = [...]`) for the failure you fear (`no free hardware slot`, `panic`).
5. Run it twice; a step that is flaky after two attempts goes to the friend-session list instead (TESTING §6), not into the set.
6. Add a row to the catalog above (what it proves, key steps, time), name the scenario in the card, and log the run (`testing-log/`).
   A new console command a scenario needs: `add-console-command` skill.
