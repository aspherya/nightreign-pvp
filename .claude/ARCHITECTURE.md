# Architecture

How the code is built, the rules that keep it that way, and the refactor plan (§5–§7; accepted 2026-10-20; status per card in §6).
Owner of: code structure, threading, module boundaries, coding rules. Memory offsets live in `re/MEMORY_MAP.md`, not here.

---

## 1. What kind of program this is

Think of it, coming from .NET, as a **hosted background service that runs inside someone else's process**. It is not a game engine:
- We do not own the game loop, the renderer, or the objects. No ECS, scene graph, or update/draw split applies to us.
- We **read** the game's objects by address, **decide** something, and **write** a few bytes back (or set a hardware breakpoint).
- The game, Seamless Co-op and Steam all keep running their own threads around us. Crashing means crashing the game.

So the useful patterns are the ones from backend services: ports and adapters, background workers, mailboxes, state machines.
Patterns built for engines are not.

## 2. As built (2026-11-01; R0, R1, R5, R2-lite, R3-split done, the rest open: §6)

Crates: `nrpvp` (the mod DLL, ~45 k lines incl. tests), `nrpvp-launcher` (player exe), `nrcon` (console client, dev), `nro-core` / `nro` /
`nro-mcp` (orchestrator, dev tooling; `orchestrator/README.md`). `nrscan` (external scanner) was deleted on 2026-10-24 (`git show cbbffa2:crates/nrscan`).

```
DllMain ─► nrpvp-main thread (startup::run, catch_unwind)
            ├─ logging, config, banner
            ├─ console pipe thread (dev build + dev_mode) ──► MainReq ─┐   nrcon.exe talks to it
            ├─ Steam thread  = mm::worker + mm::machine                │   only this thread calls Steam
            │      ▲ mm::send(Cmd)   │ mm::status()                     │
            ├─ menuflow / ui_msg / quietinvite (one thread each)        │
            ├─ hwbp thread (owns debug registers DR0–DR3 of every game thread; primitives in dr.rs)
            ├─ net worker (net/, both builds): net::bus facts ↔ Channel ↔ SteamLink (ISteamNetworkingMessages)
            ├─ memscan thread (on demand, console `memscan`)
            └─ game::run = main loop, 50 ms: singletons → ChrSet → TeamCtl.tick → CombatCtl.tick → Features.tick(World, Ctx); console requests ◄┘
```

| Layer | Modules | State |
|---|---|---|
| Pure rules | `rules/teams`, `rules/matchstate`, `mm/machine` (+`gate`), `mm/proto`, `game/team/roster` | Good: no memory access, tested with fakes |
| Game access | `game/mem`, `game/layout` (every object offset), `rtti`, `singletons`, `image`, `event_flags`, `fmg`, `dr` | Good |
| Feature list (OFF-1, OFF-4/5) | `features/` (`Feature` trait, `Features`, `Ctx` outbox: matchmaking commands, event flags, bot writes; features `mode`, `bots` (dev console), `clock`, `nights`, `night_match`), `game/world` (`World` read model: place, bot manager, watched bots), `game/gamemem` (`GameMem` port + `RealMem`/`FakeMem`), `game/views` (`ChrIns`, `WorldChrMan`, `BotMan`, `ExpeditionMenu`) | New behaviour goes here; the older controllers below move in when a card touches them. **Still outside the Feature list (R3 open):** `game/deathhold`, `menugate`, `hudfilter`, `maphide`, `runeshare`, `multiplay`, `itemdrop` / `defeatdrop`, `orbclaim`, `hawk/` |
| Features | `game/team/` (player list, writes, PvP marking, persist, snapshot), `game/combat_state/` (tracker, downed rule), `game/menuflow`, `game/flags`, `mm/ui_msg`, `steam/quietinvite` | Split by job (R3); files under ~550 lines except tests |
| Dev tooling | `console/`, `steam/lobtrace`, `game/{menutrace,texttrace,flagwatch}`, `game/poswrite` (Task D) | Behind `--features dev`: also `bartrace`, `deathtrace`, `rawcall`, `teleport`, `vpad` (`game/mod.rs`). Compiled in the player build too: `puppet`, `memscan`, `poswrite`, `steam/probe` (runtime dev-only) |

## 3. Current problems, ranked by how much they hurt

This ranking is about **agent throughput**, since agents write the code. A Sonnet agent reads whole files. A 3 600-line file costs
context, slows the agent, and makes two cards collide on the same file.

| # | Problem | Status (2026-10-20) |
|---|---|---|
| P1 | **`TeamCtl` was a god object** (3 567 lines, six jobs) | **Fixed (R3-split):** `game/team/` = chrset, write, roster + TeamCtl's methods in manual / pvpve / view / postest; `mod.rs` 416 lines |
| P2 | **Offsets lived inside feature files** (`+0x5A0` under two names) | **Fixed (R2-lite):** `game/layout.rs` holds every object offset once; modules alias the old names |
| P3 | **Three ways to read memory** (`mem::read`, `RegionCache`, `CombatMem`) | **Open** (R2 rest): one `GameMem` trait + typed views (`PlayerIns<'m>`); touches the downed rule, so it waits for a Nucleus test slot |
| P4 | **Startup is a hand-written composition root** (`if cfg.a && cfg.b` blocks) | **Open** (R3 rest): `Feature` trait + `World` read model |
| P5 | **Config is ~50 flat switches** | **Smaller:** R5 removed the overlay/hotkey/probe keys (old files still load with one warning). Sections: open (R4) |
| P6 | **Dev code shipped to players** | **Fixed (R1 + R5):** feature `dev`; panel, probes, scans, dumps deleted; player DLL 2.0 MB |
| P7 | **`combat_state.rs` mixed tracker, rule and scan driver** | **Fixed (R5 + R3-split):** scan driver deleted; `combat_state/` = memory, tracker, rules |
| P8 | **Layering inversion** (`hwbp` used a dev tracer) | **Fixed (R1):** `dr.rs` |
| P9 | **Ad-hoc globals** | Open (R6) |
| P10 | `mm/machine/gate.rs` 2 634 + `mm/machine.rs` 2 186 (after `cargo fmt`) | Low priority: pure and tested |
| P11 | Files over the 800-line budget without a header reason (2026-11-01): `config.rs` 1756, `features/night_match.rs` 1576, `console/mod.rs` 1332, `steam/queue.rs` 1325, `game/puppet.rs` 1116, `game/mod.rs` 1086, `rules/matchstate.rs` 1007, `steam/probe.rs` 948, `game/poswrite.rs` 939, `game/teleport.rs` 857 | Open; split when a card touches them |

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
| R0 ✅ | **Done 2026-10-20.** `cargo fmt` once; CI on GitHub Actions (fmt, clippy `-D warnings` for dev and player builds, test) | everything | XS |
| R1 ✅ | **Done 2026-10-20 (first pass):** feature `dev` (default on) gates the ImGui panel, `lobtrace`, `menutrace`, `texttrace`, `c1proto`, `flagwatch`; shared helpers moved to `dr.rs` (debug registers), `game/fmg.rs` (text tables), `menuflow`/`flags`; player DLL refuses `dev_mode`; package-release builds the player DLL (3.2 → 2.0 MB). Probes, scans and `poswrite` are still compiled: they go in R5. Original scope: Cargo feature `dev`: move dev tooling under `dev/`, `#[cfg]` out of the player build; `build.ps1` builds with it, `package-release.ps1` without. Move `apply_all` into `hwbp` (P8) | P6, P8, console | M |
| R2 ◐ | **Half done 2026-10-20:** `game/layout.rs` (every object offset, MEMORY_MAP refs). **OFF-1 (2026-10-22):** the `GameMem` port (`game/gamemem.rs`: `RealMem`, `FakeMem`) and the first typed views (`game/views.rs`: `ChrIns`, `ExpeditionMenu`, `MenuSettings`; `menuflow` uses them). **Open:** move `RegionCache` / `CombatMem` users onto the port (a `PlayerIns` view) when a card touches them | P2 ✅, P3 | M |
| R3 ◐ | **Split done 2026-10-20:** `game/team/` and `game/combat_state/` by job (pure moves, same tests). **OFF-1 (2026-10-22):** `Feature` trait + `Features` list built in `startup.rs` and ticked by the main loop with a per-tick `World` (`game/world.rs`) and a `Ctx` outbox; first feature: the game mode. **Open:** move `TeamCtl` / `CombatCtl` / hawk into the list | P1 ✅, P7 ✅, P4 | L |
| R4 ◐ | **Removed keys done (R5):** overlay/hotkey/probe/C1/downed keys gone, old files load with one warning. **Open:** sections | P5 | S |
| R5 ✅ | **Done 2026-10-20:** ImGui panel + hudhook, hotkeys, T-027…T-038 probes and scans, discovery worker, MARK dumps, `c1proto`, the inert downed experiment (~12 k lines). Kept: UI-0/UI-1 tracers (MM-3g RE), `flagwatch`, `poswrite` (Task D) | P6 | M |
| R6 | Thread map + mailbox list in this file; one `core::threads` spawner that names and logs every thread | P9 | S |
| (later) | Split `mm/machine/gate.rs` by state | P10 | M |

Done in order R0 → R1 → CON-1/2 → R5 → R2-lite → R3-split (2026-10-20; SMOKE-2 PASS 2026-10-21). Open (2026-11-01): R2 rest (`PlayerIns` view, `RegionCache` / `CombatMem` users), R3 rest (`TeamCtl`, `CombatCtl`, hawk and the RW controllers into the Feature list), R4 sections, R6. This is the plan of the decoupling refactor (phase 2 of the docs-and-code clean-up): a new mode must be an assembly of existing modules.

**Rules for refactor cards and every card after them:**
- A file over ~800 lines needs a reason in its header; new files aim for under 600.
- Offsets only in `game/layout.rs`, each with game version + MEMORY_MAP section.
- New behaviour comes as a `Feature`, not as another field on an existing controller.
- Dev-only code is `#[cfg(feature = "dev")]`, per module or per item (no `dev/` folder exists).

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
| Where is offset X, and is it verified? | `game/layout.rs` (one line per offset with its MEMORY_MAP section), then `re/MEMORY_MAP.md` |
| How is the player list read, and who is LOCAL? | `game/team/chrset.rs` (`resolve`, `decide_local`) |
| How do rivals get marked hostile? | `game/team/roster.rs` (`plan_pvpve`), `game/team/pvpve.rs` (`pvpve_tick`), `game/team/write.rs` (`write_team_type`) |
| How does the downed rule work? | `game/combat_state/rules.rs` (`Core`), fed by `game/combat_state/tracker.rs` |
| How does matchmaking decide? | `mm/machine.rs` (pure), then `mm/worker.rs` (glue), `MATCHMAKING.md` |
| How do our menus and messages appear in game? | `game/menuflow.rs`, `mm/ui_msg.rs`, `game/flags.rs` + `game/event_flags.rs`, `gamedata/*.json`, `scripts/build-gamedata.ps1` |
| Which settings exist? | `config.rs` (the default toml text at the top lists and explains every key) |
| How does a console command reach the game? | `console/mod.rs` (registry, `answer`), `console/pipe.rs`, the `console::take()` block in `game/mod.rs`; client `crates/nrcon` |
| What is dev-only? | `#[cfg(feature = "dev")]`: `console/`, `steam/lobtrace.rs`, `game/{menutrace,texttrace,flagwatch}.rs`; runtime dev-only: `game/poswrite.rs` |
