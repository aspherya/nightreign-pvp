# Project health review (2026-10-19)

A snapshot of what the repo holds, which tools we use and why, how the code is laid out, and what to clean up. Nothing was
changed in code for this review. Findings marked **H-n** are candidate task cards; none is scheduled yet.

Measured on `main` @ `339ee04`: `cargo test --workspace --release` → **481 pass** (452 nrpvp, 3 launcher, 26 nrscan; 1 ignored),
`cargo clippy --workspace --all-targets` → **0 warnings**, `cargo fmt --check` → **fails in 29 files**.

---

## 1. Verdict

The project is in good shape for its age (10 days, 52 commits). Three things are especially solid:
- **Evidence discipline.** Offsets carry their game version and a doc reference, `re/MEMORY_MAP.md` records how each one was proven,
  and the docs separate verified facts from hypotheses.
- **Pure logic is separated and tested.** `rules/` has no memory access, and `mm/machine` runs against a `QueueSteam` trait with a
  fake Steam. Most of the 481 tests sit there.
- **Fail-safe runtime.** `DllMain` only spawns a thread, the mod thread runs inside `catch_unwind` with `panic = "unwind"`, and every
  hook logs whether it installed. Logs survive crashes (unbuffered, one file per session, newest 30 kept).

The debt is what you'd expect after a fast research phase:
1. Investigation code ships inside the player DLL.
2. A few files have grown into god files.
3. The docs have drifted from each other.
4. Nothing enforces the quality gates automatically.

None of it blocks FS-UI2. All of it gets more expensive the longer it waits.

---

## 2. Inventory

### 2.1 Repository

| Area | What | Size |
|---|---|---|
| `crates/nrpvp` | The mod (cdylib loaded by me3) | ~32 k lines Rust, 64 files |
| `crates/nrpvp-launcher` | `nrpvp_launcher.exe`: writes an me3 profile, starts the bundled me3 | 231 lines |
| `crates/nrscan` | External read-mostly memory scanner (Nucleus instances only) | ~4.9 k lines |
| `gamedata/` | Our texts (`fmg-entries.json`) and Roundtable events (`emevd-events.json`) | 2 JSON sources |
| `scripts/` | build, launch, package-release, deploy-nucleus, build-gamedata (PowerShell) + `emevd_add_events.py` | 6 scripts |
| `profiles/` | me3 profiles `sc` (default) and `dev` | 2 |
| `.claude/` | Planning, RE notes, research spikes, agent defs, experiment scripts | ~3.3 k lines of core docs, 13 research write-ups |
| `dist/` (gitignored) | DLL, PDB, player zip, gamedata package, logs | ~20 MB of logs |

### 2.2 Inside `nrpvp`, by role

| Role | Modules | Lines (approx.) | In the player build? |
|---|---|---|---|
| Entry and infra | `lib`, `startup`, `logging`, `logring`, `sysinfo`, `config`, `hwbp` | 2.9 k | yes |
| Game memory core | `game/mod` (main loop), `mem`, `image`, `rtti`, `singletons`, `team`, `combat_state`, `downed`, `flags` | 9.0 k | yes |
| Rules (pure) | `rules/teams`, `rules/matchstate` (not wired yet) | 1.3 k | `teams` yes |
| Matchmaking | `mm/*` (machine, gate, proto, guard, worker, join, ui_msg, sccreate) | 5.7 k | yes |
| Steam glue | `steam/mod`, `callback`, `queue`, `quietinvite` | 1.9 k | yes |
| In-game UI | `game/menuflow`, `mm/ui_msg` | 0.7 k | yes |
| **Investigation / dev only** | `probe2`, `discover`, `wide_scan`, `probe_scan`, `probe_pos`, `probe_down`, `mark_dump`, `mark`, `menutrace`, `texttrace`, `flagwatch`, `c1proto`, `steam/lobtrace`, `steam/probe` (dump parts), `poswrite` (snap-back test) | **~11 k** | compiled in, switched off at runtime |
| Dev UI | `overlay/*` (hudhook/ImGui), `hotkey` | 2.0 k | compiled in, switched off at runtime |

About **40 % of the DLL is code that players never run**. A runtime `dev_mode = false` keeps it off, but it is still linked in, along
with hudhook/imgui. One edit to `nrpvp.toml` turns all of it back on.

---

## 3. Tools: what and why

| Tool | Used for | Why this one | Keep? |
|---|---|---|---|
| **Rust stable, MSVC** (`rust-toolchain.toml`) | Mod, launcher, scanner | Memory-safe FFI, same ecosystem as fromsoftware-rs, one toolchain for DLL + exe | Yes |
| `windows` 0.62 | Win32 API | The official bindings | Yes |
| `tracing` + `tracing-subscriber` + `tracing-appender` | File logs, ring buffer for the overlay | Structured, levelled, cheap | Yes |
| `from-singleton` 3.0.1 | FD4 singleton call-site patterns | Deliberately not the `eldenring` crate (its RVA table panics on NR) | Yes |
| `serde` + `toml` | `nrpvp.toml` | me3 uses TOML too | Yes |
| `hudhook` =0.9.3 (imgui 0.12, DX12) | Developer overlay | Fastest route to a debug UI | Dev only (H-1) |
| **me3** 0.11.0 | Loads nrsc.dll early, then nrpvp.dll and our `[[packages]]` | Supported loader, no EAC, no file patching | Yes, pinned and bundled |
| **Seamless Co-op** 1.1.3 | Networking, 6-player sessions | Only working P2P layer. Closed source, runtime interop only | Yes, pinned |
| **Nucleus Co-op** | 2–3 game instances on one PC | Lets Leonardo test multiplayer solo | Yes |
| `nrscan` (ours) | Find and confirm memory locations from outside | Faster than Cheat Engine for our repeated questions; refuses non-Nucleus installs | Yes, but freeze its scope |
| Cheat Engine | Ad-hoc RE | Standard | Occasional |
| WitchyBND 3.0.1 | Unpack and repack `msgbnd` | Standard Souls tool | Yes (gamedata build) |
| Python 3.12 (+ capstone, pefile) | `emevd_add_events.py`, disassembly helpers in `experiments/` | Quickest way to script binary formats | Yes; pin in a `requirements.txt` (H-9) |
| DarkScript3 `emedf` | EMEVD instruction reference | Reference only, not a dependency | — |
| PowerShell scripts | Build, package, deploy | Native on Windows | Yes; centralise paths (H-8) |
| Claude agents (`implementer`, `researcher`) | Task-card execution | Opus plans, Sonnet executes | Yes |

**Missing on purpose and fine:** a server, a database, a web backend. Matchmaking uses Steam lobbies (S9), so there is no
infrastructure to run.

---

## 4. Architecture review

### 4.1 Threads and data flow (as built)

```
DllMain ─► nrpvp-main thread (startup::run, catch_unwind)
            ├─ logging, config, banner
            ├─ hotkey thread (dev)            ──► commands ─┐
            ├─ overlay thread (dev, hudhook)  ◄── Snapshot ─┤
            ├─ Steam thread (steam::probe)  = mm::worker + mm::machine
            │      ▲ mm::send(Cmd)   │ mm::status()          │
            ├─ quietinvite / menuflow / ui_msg / flag helpers (one thread each)
            ├─ hwbp thread (owns DR0–DR3 for every game thread)
            └─ game::run  = main loop (50 ms): singletons → ChrSet → team writes, downed rule, snapshot
```

About 19 spawned threads and roughly 58 `static` atomics or mutexes. Modules talk through small global mailboxes (`mm::send`,
`mm::status`, `Shared`), not shared ownership. That is a sensible pattern for a DLL living inside someone else's process. It is not
written down anywhere yet, though (H-6).

### 4.2 What is good
- **Steam has one owner.** Only the Steam thread calls Steam; the game loop and UI exchange plain values. This rule prevents a whole
  class of re-entrancy bugs.
- **Writes are gated.** `write_team_type` re-reads the vtable and checks it against RTTI before writing a single byte.
- **Patching is avoided.** Hardware breakpoints and data writes only, no code bytes changed, which keeps Arxan quiet.
- **Discovery uses RTTI and patterns,** not hardcoded addresses, so a game patch costs less.

### 4.3 Problems

| # | Finding | Where | Why it matters |
|---|---|---|---|
| A1 | **Dev and investigation code is compiled into the player DLL** (§2.2). Gated only by runtime config; there are 0 `#[cfg(feature)]`s | whole crate | Bigger attack and crash surface; ImGui/DX12 hooks linked for players; cognitive load; a player editing one line in the toml gets the probes |
| A2 | **God files.** `team.rs` 3.6 k (the `TeamCtl` impl alone is ~1 k lines and mixes resolve, persist, PvPvE, the downed rule, overlay snapshot, and poswrite), `combat_state.rs` 2.2 k, `mm/machine/gate.rs` 2.1 k, `config.rs` 1.8 k, `mm/machine.rs` 1.6 k | game, mm, config | Hard to review, and Sonnet cards touching them get slow and conflict-prone |
| A3 | **`config.rs` is a flat bag of about 60 switches** with interlocks in code ("quiet invite off when lobby trace on", "player build forces X") | config, startup | Each feature adds 2–4 flags. Startup is a long `if cfg.a && cfg.b && !cfg.c` chain |
| A4 | **Layering inversion.** `hwbp` (core, used by player features) calls `steam::lobtrace::apply_all` and `TARGETS` from a dev-only tracer | `hwbp.rs:19` | Removing the tracer would break the player build; the debug-register owner should own its own primitive |
| A5 | **Debug-register contention is a config rule, not a type.** Four tracers grab DR0–DR3 directly while others use `hwbp` slots | hwbp, tracers | Every new breakpoint feature has to remember the rule |
| A6 | **The heap-corruption crash on quit (0xC0000374)** has happened on every run since 2026-10-10, marked "pre-existing" | — | Not proven to be SC's. Our suspend/SetThreadContext loop, hudhook teardown, and threads still running at `DLL_PROCESS_DETACH` are all candidates. Players will see it |
| A7 | **Main loop owns everything.** `game::run` drives singletons, team, downed, probes, snapshot, and commands | `game/mod.rs` | Fine today. The next features (spawns, death rules, inventory, match flow) need a place to plug in |

---

## 5. Process and quality gates

| Item | State | Note |
|---|---|---|
| Unit tests | ✅ 481 pass, fast (<10 s) | Strong for a mod |
| Clippy | ✅ clean (default lints) | |
| rustfmt | ❌ 29 files drift: all of MM/UI and recent work (gate.rs 110 hunks, machine.rs 91, queue.rs 50 …) | Newer code was never formatted. A single `cargo fmt` commit fixes it (H-3) |
| CI | ❌ none (GitHub remote exists, no `.github/`) | Windows runner: fmt + clippy + test on push (H-4) |
| Pre-commit hook | ❌ | Optional once CI exists |
| Versioning | ⚠️ always `0.1.0`; `VERSION.txt` has commit + DLL hash | Good enough to trace a zip. Players and logs would benefit from a visible build number (H-10) |
| Branches | ⚠️ `t-028-snapback` holds 3 commits not on `main` (snap-back, F3 team spawns) | Decide: merge into the spawns work or archive (H-11) |
| Hardcoded paths | ⚠️ `Z:\…` in launch.ps1, package-release.ps1, build-gamedata.ps1, the SC profile | One `scripts/paths.ps1` (or env vars) (H-8) |
| Unsafe | ~180 sites, concentrated in Steam FFI (`steam/queue.rs` 48) and tracers | Expected; keep it behind small wrappers with `// SAFETY:` notes |

---

## 6. Documentation drift

The docs are the project's memory, and several now contradict each other. Agents read them first, so this matters more than
usual (H-2).

| Doc | Problem |
|---|---|
| `.claude/README.md` | Says "Phase 0, nothing playable"; documents `package-friend.ps1` / friend kit (script no longer exists) and T-020 F11/F12 keys; "PvP feasibility unproven" |
| `README.md` (root) | Offers a `-MatchmakingTest` zip with "debug panel with Queue buttons"; the player build has no overlay now |
| `ROADMAP.md` | "Current milestone" is still run 21 (2026-10-12); next step = Task B; no MM/UI phases |
| `STATUS.md` | Mostly current, but §1 and §3 Task E still mention the Insert panel next to "no overlay"; §3 Tasks B/C/E are superseded |
| `TECHNICAL_NOTES.md §1` | Lists Rust and Python as missing; ADR-001 still "pending review" |
| `TASKS.md` | 972 lines; done cards are mixed with live ones |
| `rules/mod.rs` | Links `.claude/references/project-scope-v3.md`, which does not exist (it's `re/PROJECT_SCOPE.md`) |
| `game/team.rs` header | "The only memory write in the project". No longer true (`mem::write_bytes`, `write_u64_aligned`, flags, c1proto) |
| `config.rs` comments | "the player build forces it off" on `mm_queue`/`mm_join`/`mm_quiet_invite`, but MM-4 turns them on through `matchmaking` |

Suggested rule: **STATUS.md is the only "now" doc.** ROADMAP keeps phases only, READMEs keep how-to only, and finished cards move
to `TASKS_DONE.md`.

---

## 7. Data and logging: do we need a database?

**Short answer: not now, and never inside the DLL.**

Today: `tracing` text lines, one file per session, newest 30 kept; players send `<Game>\NightreignPvP\logs` after a test; `nrscan`
already has a log parser (`logparse.rs`). The questions we ask of logs are about one session ("did all three games agree on teams",
"how long did MATCH READY → ALL PRESENT take").

| Option | Verdict |
|---|---|
| SQL DB (Postgres etc.) / telemetry server | **No.** It needs a server and an upload, which breaks the "no server of ours" decision (S9) and adds privacy questions. |
| SQLite written by the DLL | **No.** It adds a C dependency and disk I/O inside the game process for no current question. |
| **Structured events in the log (JSON lines) + an offline analyzer** | **Yes, when the next multi-player debugging pain shows up.** Emit key events (`match_ready`, `all_present`, `team_assign`, `down`, `rival_hit`, `death`) as one `tracing` JSON line each, next to the human log. A small `nrlog` tool (or a `nrscan` subcommand) merges several players' logs on the timeline and prints a match report. If cross-session questions appear later ("average queue time over 20 tests"), the analyzer can load the JSON into SQLite or DuckDB **on the dev PC**. (H-7) |

---

## 8. Dev QoL ideas, ranked by payoff

1. **CI + fmt** (H-3, H-4): catches agent cut-offs (STATUS §6 trap) without Leonardo checking.
2. **`dev` cargo feature** (H-1): makes player and dev builds two artifacts, and makes clean-up mechanical.
3. **One-command test kit:** `scripts/dev.ps1 build|launch|nucleus|zip|logs`. A single entry point with tab completion replaces
   five scripts with different flags; `logs` opens the newest session log for every instance.
4. **Multi-log match report** (H-7): turns "send me your logs folder" into a one-screen verdict.
5. **Game-patch checklist:** a script that re-hashes the exe, SC, and the unpacked files against the pins and lists which offsets and
   AOBs to re-verify. Today the pins sit in three places (TECHNICAL_NOTES §1, build-gamedata.ps1, the code comments).

---

## 9. Proposed cards (not scheduled)

| Card | What | Size | When |
|---|---|---|---|
| **H-1** | Cargo feature `dev` (default on for `build.ps1`, off for `package-release.ps1`). Move probes, tracers, overlay, hotkeys, and `nrscan`-support dumps behind it. Player DLL drops hudhook | M | After FS-UI2 |
| **H-2** | Doc pass: fix §6 drift, archive done cards to `TASKS_DONE.md`, make ROADMAP phases-only | S | Now (docs only) |
| **H-3** | `cargo fmt` the workspace in one commit, nothing else | XS | Now (no behaviour change, but it is a code-file change: needs your OK) |
| **H-4** | GitHub Actions on `windows-latest`: fmt check, clippy `-D warnings`, test | S | After H-3 |
| **H-5** | Split `team.rs` (`resolve`, `pvpve`, `persist`, `snapshot`) and move `apply_all` from `steam::lobtrace` into `hwbp` (A4). Group config into sections (`[matchmaking]`, `[dev]`, `[rules]`) with a migration for old flat keys | M | After H-1 |
| **H-6** | `ARCHITECTURE.md`: thread map, ownership rules (Steam thread, main loop, hwbp), globals list, how to add a feature | S | With H-5 |
| **H-7** | Structured event lines + an offline multi-log match report | M | Before the next 3+ player session |
| **H-8** | `scripts/paths.ps1` (or env vars) for the `Z:\` paths; the launcher already has none | XS | Any time |
| **H-9** | `requirements.txt` for the Python helpers | XS | Any time |
| **H-10** | Visible build id: version from date or git describe, in the log banner and zip name | XS | With the next release |
| **H-11** | Resolve the `t-028-snapback` branch (merge into the spawns card or archive) | XS | When spawns start |
| **H-12** | Investigate the quit crash (A6): quit with SC only, then SC + nrpvp with each thread disabled; check DLL detach | M | Before a public release |
| **H-13** | Delete investigation code whose question is answered (keep the findings in MEMORY_MAP): the T-031–T-038 probes, wide/probe scans, `c1proto`, UI-0 tracers | M | After H-1 makes it safe |

Suggested order: **H-2 → H-3 → H-4** (cheap, protects everything after), FS-UI2, **H-1 → H-13 → H-5/H-6**, then feature work
(spawns, death rules). H-7 before the next large multi-player session; H-12 before anyone outside the test group gets a zip.
