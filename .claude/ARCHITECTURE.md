# Architecture

How the code is built, the rules that keep it that way, and the refactor plan (§5–§7; **accepted 2026-10-20, not started**).
Owner of: code structure, threading, module boundaries, coding rules. Memory offsets live in `re/MEMORY_MAP.md`, not here.

---

## 1. What kind of program this is

Think of it, coming from .NET, as a **hosted background service that runs inside someone else's process**. It is not a game engine:
- We do not own the game loop, the renderer, or the objects. No ECS, scene graph, or update/draw split applies to us.
- We **read** the game's objects by address, **decide** something, and **write** a few bytes back (or set a hardware breakpoint).
- The game, Seamless Co-op and Steam all keep running their own threads around us. Crashing means crashing the game.

So the useful patterns are the ones from backend services: ports and adapters, background workers, mailboxes, state machines.
Patterns built for engines are not.

## 2. As built (2026-10-20)

Three crates: `nrpvp` (the mod DLL, ~32 k lines), `nrpvp-launcher` (player exe), `nrscan` (external scanner, dev tool).

```
DllMain ─► nrpvp-main thread (startup::run, catch_unwind)
            ├─ logging, config, banner
            ├─ hotkey thread (dev)            ──► Command ─┐
            ├─ overlay thread (dev, hudhook)  ◄── Snapshot ─┤
            ├─ Steam thread  = mm::worker + mm::machine     │   only this thread calls Steam
            │      ▲ mm::send(Cmd)   │ mm::status()          │
            ├─ menuflow / ui_msg / quietinvite (one thread each)
            ├─ hwbp thread (owns debug registers DR0–DR3 of every game thread)
            └─ game::run = main loop, 50 ms: singletons → ChrSet → TeamCtl.tick → CombatCtl.tick → Snapshot
```

| Layer | Modules | State |
|---|---|---|
| Pure rules | `rules/teams`, `rules/matchstate`, `mm/machine` (+`gate`), `mm/proto` | Good: no memory access, tested with fakes |
| Game access | `game/mem`, `rtti`, `singletons`, `image` | Good |
| Features | `game/team` (hostility, persist, roster, snapshot, pos test), `game/combat_state` (downed rule), `game/menuflow`, `mm/ui_msg`, `steam/quietinvite` | Mixed: see §3 |
| Dev tooling | probes, scans, tracers, dumps, overlay, hotkeys | ~13 k lines, compiled into the player DLL |

## 3. Current problems, ranked by how much they hurt

This ranking is about **agent throughput**, since agents write the code. A Sonnet agent reads whole files. A 3 600-line file costs
context, slows the agent, and makes two cards collide on the same file.

| # | Problem | Evidence | Cost today |
|---|---|---|---|
| P1 | **`TeamCtl` is a god object.** 50 fields, ~55 methods, six jobs: player resolution, debug writes and restores, persist, PvPvE marking, overlay snapshot, snap-back test | `game/team.rs` 3 567 lines | Every hostility, roster or UI card touches it |
| P2 | **Offsets live inside feature files.** The near-death block and HP chain are constants in `combat_state` (and again in `wide_scan`), the ChrSet shape and `+0x5A0` in `team` (`+0x5A0` in both) | grep `0x5A0`, `0x710` | Not badly scattered yet, but every new feature that needs a player field has to import from another feature or copy the constant |
| P3 | **Three ways to read memory.** `mem::read` (guarded, slow), `RegionCache`, and the `CombatMem` trait (only combat uses an adapter) | `game/mem.rs`, `combat_state.rs` | Only one feature is testable without the game; STATUS trap "no guarded reads on the main loop" exists because of this |
| P4 | **Startup is a hand-written composition root.** 15 `if cfg.a && cfg.b && !cfg.c { spawn }` blocks | `startup.rs` | Every feature edits it; flag interlocks live in two places |
| P5 | **Config is ~60 flat switches**, dev and player mixed, interlocks in code | `config.rs` 1 840 lines | Each feature adds 2–4 flags; comments drifted (`mm_queue` "forced off" is false) |
| P6 | **Dev code ships to players** (gated only at runtime, 0 `#[cfg(feature)]`) | §2 | Crash surface, hudhook linked, agents read dead code |
| P7 | **`combat_state.rs` mixes tracker, rule and scan driver** | 2 235 lines | Same as P1, smaller |
| P8 | **Layering inversion:** `hwbp` uses `steam::lobtrace::apply_all` | `hwbp.rs:19` | Deleting a dev tracer breaks the player build |
| P9 | **Ad-hoc globals:** ~58 statics, mailboxes per module | grep `static` | No single picture of who talks to whom |
| P10 | `mm/machine/gate.rs` 2 063 + `mm/machine.rs` 1 608 | — | Big, but pure and tested: **low priority** |

What is **not** a problem: Rust as a language, the thread-per-concern model, panic handling, the test suite, the pure rules core. Keep
them.

## 4. Patterns: which ones, and which to avoid

Mapped to what you know from .NET. The rule: a pattern goes in only where it removes a P-number above.

| Pattern | .NET analogue | Where it applies here | Fixes |
|---|---|---|---|
| **Ports and adapters** | Interfaces in Domain, implementations in Infrastructure | One `GameMem` trait (read, typed read, gated write) with a real adapter and a fake for tests. Same for Steam (`QueueSteam` already does this) | P3, makes P1/P7 testable |
| **Typed memory views** | EF entity configuration, DTO mapping | `PlayerIns<'m>` with `team()`, `set_team()`, `downed()`, `bleed_clock()`, `hp()`: each offset written **once**, in `game/layout/`, with its MEMORY_MAP reference | P2 |
| **Feature modules** | `IHostedService` + DI registration | `trait Feature { fn name(); fn enabled(&Config) -> bool; fn tick(&mut self, &World, &mut Ctx) }`, a list in one place. Startup becomes a loop | P4, P1 (split TeamCtl into features) |
| **Read model per tick** | CQRS read side | Main loop reads the world **once** per tick into `World` (players, states, ids, roster); features read that and emit writes | P3, perf trap |
| **Command / mediator** | MediatR | Already exists: `Command` enum + main-loop queue. The console (`CONSOLE.md`) makes it the only input | P4, P9 |
| **State machine** | Stateless library, sagas | Already the core of matchmaking. Next: match flow (`rules/matchstate`) | — |
| **Strategy** | `IStrategy` + DI | Only where there are real variants, and as a Rust `enum` + `match`, not a trait hierarchy: team scheme, revive policy. Already done | — |

**Avoid:** DI containers, inheritance trees, generic repositories, an async runtime (tokio) in the DLL, ECS, event buses with
dynamic subscribers. Each adds indirection that agents must learn and that hides the thread a piece of code runs on.

## 5. Target structure

```
crates/nrpvp/src/
  lib.rs, startup.rs            composition root: build Config, start threads, register features (short)
  config/                       mod.rs (load, validate), player.rs, dev.rs: sectioned [rules] [matchmaking] [dev]
  core/                         logging, logring, sysinfo, hwbp (owns apply_all), threads
  game/
    mem.rs                      raw access; GameMem trait + RealMem + FakeMem (tests)
    layout/                     one file per engine class: chr_set.rs, player_ins.rs, near_death.rs, flags.rs   (offsets live ONLY here)
    world.rs                    World read model: one snapshot per tick
    discovery/                  singletons, rtti, image
  features/                     each implements Feature; each < ~600 lines
    hostility.rs                rival marking (from TeamCtl)
    downed_rule.rs              rival hits shorten the clock (from combat_state)
    roster.rs                   who is ally or rival (rules/teams glue)
    menuflow.rs, ui_msg.rs      in-game UI
    matchmaking.rs              bridge to the Steam thread
  rules/                        pure: teams, matchstate (unchanged)
  mm/                           pure machine + Steam worker (gate split by state, later)
  steam/                        FFI adapters
  dev/            #[cfg(feature = "dev")]   console, probes, tracers, dumps, overlay, pos test
```

## 6. Refactor plan (proposal)

Every R-card is **behaviour-preserving**: same log lines, same tests green, plus one solo smoke run (Roundtable → expedition → down
→ rule fires). No feature work mixed in.

| Card | What | Unblocks | Size |
|---|---|---|---|
| R0 | `cargo fmt` once; CI on GitHub Actions (fmt, clippy `-D warnings`, test) | everything | XS |
| R1 ✅ | **Done 2026-10-20 (first pass):** feature `dev` (default on) gates the ImGui panel, `lobtrace`, `menutrace`, `texttrace`, `c1proto`, `flagwatch`; shared helpers moved to `dr.rs` (debug registers), `game/fmg.rs` (text tables), `menuflow`/`flags`; player DLL refuses `dev_mode`; package-release builds the player DLL (3.2 → 2.0 MB). Probes, scans and `poswrite` are still compiled: they go in R5. Original scope: Cargo feature `dev`: move dev tooling under `dev/`, `#[cfg]` out of the player build; `build.ps1` builds with it, `package-release.ps1` without. Move `apply_all` into `hwbp` (P8) | P6, P8, console | M |
| R2 | `game/layout/` + typed views + one `GameMem` trait; migrate `team`, `combat_state`, `downed` to it | P2, P3 | M |
| R3 | `World` read model + `Feature` trait; split `TeamCtl` into hostility/roster/persist-debug and `combat_state` into tracker/rule; startup loop | P1, P4, P7 | L |
| R4 | Sectioned config with old flat keys still accepted (migration + log warning) | P5 | S |
| R5 | Delete answered investigation code (T-031…T-038 probes, wide/probe scans, `c1proto`, UI-0 tracers); findings stay in MEMORY_MAP | P6 | M |
| R6 | Thread map + mailbox list in this file; one `core::threads` spawner that names and logs every thread | P9 | S |
| (later) | Split `mm/machine/gate.rs` by state | P10 | M |

Order: **R0 now → FS-UI2 → R1 → R5 → R2 → R3 → R4 → R6**, then feature work (spawns, death rules). The console (CON-1) comes after R1
because it lives in `dev/`.

**Rules for refactor cards and every card after them:**
- A file over ~800 lines needs a reason in its header; new files aim for under 600.
- Offsets only in `game/layout/`, each with game version + MEMORY_MAP section.
- New behaviour comes as a `Feature`, not as another field on an existing controller.
- Dev-only code goes under `dev/` behind the feature flag.

**Modding rules (every card that touches game memory or hooks):** what native mod communities (SKSE/CommonLib, fromsoftware-rs,
Reloaded-II) converge on. DllMain, hook logging, saves and offline play are in `CLAUDE.md` and `agents/implementer.md`; not repeated here.
- **Typed access only.** Feature code calls accessors (`player.hp()`), never `base + 0x…`. Pointer math lives in `game/layout/`.
- **Locate by signature where possible.** A new function address is an AOB scan or RTTI lookup; a fixed RVA needs a byte check at the
  site (as `menuflow` does) and a note in MEMORY_MAP saying why no signature was used.
- **Never block, never unwind into the game.** No locks, sleeps or Steam calls inside a hook, VEH handler or game-thread callback;
  every thread entry, hook body and FFI callback is wrapped in `catch_unwind`, logs and returns the safe default.
- **Share the process.** Assume other mods and SC hook the same code: chain to the original, never overwrite a hook or debug register
  that is not ours (check DR7 ownership before claiming a slot), and restore what we changed on shutdown.
- **Unknown build means stay passive.** Writes and hooks run only on a game build listed in `TECHNICAL_NOTES.md` §1; on any other build
  log it and run read-only (card VER-1).
- **Rules stay testable.** A decision that can be made without the game goes in `rules/` or behind the `GameMem` port with a fake,
  with a unit test.

**Agents and skills:** no new agent. The `implementer` agent gets these rules (a short "Architecture rules" block pointing here).
Once R3 lands, one skill "add a feature" (Feature skeleton, config section, layout use, test with `FakeMem`) is worth writing. Not
before.

## 7. Decisions (2026-10-20)

Leonardo asked for Claude's recommendation; these are it.

1. **Patterns:** the five in §4 (ports and adapters, typed memory views, feature modules, a per-tick read model, the existing commands
   and state machines). Strategy only as Rust enums. Nothing else until a named problem needs it.
2. **Timing:** R0 now (agents only, no game). R1 right after FS-UI2. R5, R2 and R3 before spawns and death rules: those features would
   otherwise land in `team.rs` and `combat_state.rs` and the bill grows.
3. **Size budget:** new files aim for under 600 lines; a file over ~800 needs a reason in its header. Refactor cards bring the big files
   under it.
4. **Config (R4):** no migration. Every player zip ships its own `nrpvp.toml` and the install steps replace the folder, so old flat
   keys are simply rejected with a clear log line naming the new key.
5. **Deletions (R5):** yes. Investigation code whose question is answered is deleted; MEMORY_MAP keeps the finding, git keeps the code.

## 8. Where to look (for agents and for humans new to the code)

Every module starts with a `//!` header saying what it owns, which card added it, and which doc section backs its facts. Read the
header before the body.

| Question | Start at |
|---|---|
| What runs at startup and in which thread? | `startup.rs`, then §2 of this file |
| Where is offset X, and is it verified? | `re/MEMORY_MAP.md`, then `game/layout/` (after R2; today `game/team.rs`, `game/combat_state.rs`) |
| How do rivals get marked hostile? | `game/team.rs` (`plan_pvpve`, `write_team_type`); after R3 `features/hostility.rs` |
| How does the downed rule work? | `game/combat_state.rs` (`Core`); after R3 `features/downed_rule.rs` |
| How does matchmaking decide? | `mm/machine.rs` (pure), then `mm/worker.rs` (glue), `MATCHMAKING.md` |
| How do our menus and messages appear in game? | `game/menuflow.rs`, `mm/ui_msg.rs`, `gamedata/*.json`, `scripts/build-gamedata.ps1` |
| Which settings exist? | `config.rs` (the default toml text at the top lists and explains every key) |
