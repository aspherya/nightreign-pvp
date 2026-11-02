# Refactor plan: plug-and-play modules (phase 2) and orchestrator (phase 3)

Planning only, written 2026-11-01 against `claude/refactor-plan` @ 5ec1652 (worktree `docs-audit-c341ba`). File:line refs are from
that commit. I did not build or run anything. This plan replaces the open half of ARCHITECTURE §6 (R2 rest, R3 rest, R4, R6) and
changes one of its decisions (see §2.6).

---

## 1. Assessment: what is good, and what actually blocks plug-and-play

### 1.1 What is good (keep it)

- **The match rules are already a pure core.** `NightCore::step(now_ms, &World, &mut flags) -> (acts, lines)`
  (`features/night_match.rs:269-323`) reads no memory and has 25 behaviour-named unit tests (`:1032-1562`). That is the hard part of
  "rules testable without the game", and it is done.
- **The Feature / World / Ctx skeleton exists and works**: `trait Feature { name, tick(&World, &mut Ctx) }` (`features/mod.rs:19-24`),
  a per-tick read model (`game/world.rs:75-109`), and an outbox carried out after all features ran (`Ctx::flush`, `features/mod.rs:50-78`).
- Other pure, tested cores: `rules/teams`, `mm/machine` + `gate`, `game/combat_state/rules.rs`, the judge functions in `hudfilter`
  (`rival_marker`, `marker_ids`, `:222`, `:456`).
- `GameMem` port + `FakeMem`, every offset in `game/layout.rs`, one writer thread for game memory, `catch_unwind` at every entry,
  CI with fmt + clippy + tests for both builds (`.github/workflows`).

### 1.2 The real blockers (ranked)

1. **The composition root is the main loop, not the Feature list.** `game::watch` (`game/mod.rs:266-733`) hand-ticks ten
   controllers outside `Features`, each with its own argument plumbing: `multiplay` (`:572`), `deathhold` (`:630`), `menugate`
   (`:638`), `hudfilter` (`:651`), `runeshare` (`:667`), `defeatdrop` (`:681`), `orbclaim` (`:683`), `maphide` (`:692`), `hawk`
   (`:699`), `reviveblock` (`:708`), plus dev-only `bartrace`, `deathtrace`, `hitkill`. Each one pulls `combat.local_state()` /
   `rival_handles()` again, and the shared "match is running" fact `in_match` is computed inline (`:649-650`). Adding or removing a
   module for a mode means editing this 470-line function. **This is the blocker.**
2. **There is no notion of a ruleset.** `features::mode::GameMode` is *how you enter* (BR queue vs offline solo,
   `features/mode.rs:17-22`), not *which rules apply*. The Battle Royale rules are unconditional: `NightMatch` is always built
   (`features/mod.rs:105`), and the hook modules switch on `in_match` alone. Arena cannot opt out of free Day 1, respawn-far or
   spectating today. The name `mode` also collides with MODE-01/02 in the docs.
3. **`NightCore` is six rules in one struct.** Its fields mix the elimination count (`seen`, `max_teams`, `lone_since`, `decided`,
   `match_over`), LOCAL's defeat (`dead_since`, `judged`, `defeat`, `hold_on`, `menu_on`), spectating (`spec_target`), respawn-far
   (`places`, `place_prev`, `slow_since`, `respawn_pending`, `up_since`), free Day 1 (`day1`) and the event-file fallback (`stuck`)
   (`night_match.rs:181-218`). The step methods are already separate (`local_step`, `count_step`, `spectate_step`, `respawn_step`,
   `day1_step`, `fallback`, `record_place`), so the split is mechanical. Arena wants elimination + defeat + a round reset, and none of
   day1 / respawn-far.
4. **Adding one console command touches five places**: the `COMMANDS` table (`console/mod.rs:92-197`), `MainReq`
   (`console/mod.rs:37-74`), `overlay::shared::Command` (`overlay/shared.rs:10-…`), the closure in the loop (`game/mod.rs:532-568`)
   and the exhaustive `run_command` match (`game/mod.rs:871-934`, which needs dummy arms for intercepted commands, `:884-899`). A
   module cannot own its commands.
5. **Config is one flat struct passed everywhere**: ~45 fields (`config.rs:231-318`) and one 380-line `parse` (`config.rs:581-964`).
   Modules take `&Config` and read what they like. `[hawk]` is the only section (`HawkConfig`, `:323`), which shows the pattern works.
6. **The World is half-filled by the loop.** `World::read` fills only place / bots / clock / revive item; the loop then patches
   `night`, `fighters`, `local_pos`, `local_hits`, `leave_pressed`, `spec_*`, `death_held`, `positions`, `team_size` by hand
   (`game/mod.rs:486-515`), including `GetAsyncKeyState` input edges (`:749-783`). The executor of the match rules' requests,
   `night_acts`, also lives in the loop (`:786-851`) next to `Ctx::flush`.
7. **Smaller things:** dead code `rules/matchstate.rs` (934 lines, its header says "not wired into the game"; only its own tests use
   it, grep: no other user); 281 `static`s in 44 files. Most are legitimate (hooks and VEH handlers run on game threads and need
   lock-free state). The ad-hoc ones are console overrides such as `night_match.rs:839-845` (`COUNT`, `SPECTATE_OVERRIDE`).

### 1.3 Two facts that shape the design (and must not change)

- **Tick order is behaviour.** The World is read (`game/mod.rs:472`) *before* `combat.tick` (`:606`). So the features see the fighters
  of the previous tick, while the hook modules after `:630` see the rivals of the current tick. Moving a hook module into the
  feature list as it stands now would give it rivals that are one tick (50 ms) older. That is small, but it is a behaviour change
  with debounce edges (`DEAD_DEBOUNCE_MS` 800, hold and arm timing) and hardware-slot races (TRAPS "no free hardware slot").
  **The refactor keeps the order exactly** (two feature phases, §2.2).
- **TeamCtl and CombatCtl are not mode features.** Every mode, Arena included, needs rival marking and the downed / revive rule.
  ARCHITECTURE R3 says "move TeamCtl / CombatCtl / hawk into the list". **I recommend against that:** they stay *core services* ticked
  by the loop. Their outputs reach the features through `World`. Moving them buys nothing for plug-and-play and is the riskiest move
  (the downed rule is RE-verified on Nucleus only).

---

## 2. Target structure (the minimum that makes a mode an assembly)

### 2.1 Layers after the refactor

```
startup.rs        build Config, start threads, features = modes::assemble(&cfg)        (unchanged role)
game/mod.rs       the loop only (< 400 lines): discovery -> core services -> World -> features(Rules) -> flush
                  -> combat.tick -> World.after_combat -> features(AfterCombat) -> flush -> heartbeat
game/discovery.rs singletons / RTTI / dump (moved out of game/mod.rs)
game/input.rs     F10 and spectate edges (moved out of game/mod.rs)
game/world.rs     World + one fill function per phase (no hand patching in the loop)
core services     game/team (TeamCtl), game/combat_state (CombatCtl) + reviveblock/bartrace beside it   (unchanged)
features/         Feature trait, Features list, Ctx (+ effects executor), one file per adapter or rule
features/br_match/  Match feature = MatchBook + ordered list of MatchRule (elimination, defeat, spectate, respawn_far,
                    free_day1, flag_fallback) + status
modes/            mod.rs: Ruleset + assemble(); battle_royale.rs: the list. (arena.rs later)
config/           mod.rs (load), parse.rs, typed slices (MatchCfg, TeamCfg, MmCfg, HawkCfg, DevCfg); FILE FORMAT UNCHANGED
console/          registry = concatenation of per-module COMMANDS slices; Act::Main carries a closure
```

### 2.2 Feature trait: small extension

```rust
pub(crate) enum Phase { Rules, AfterCombat }          // where the loop ticks it (keeps today's order)
pub(crate) trait Feature: Any {
    fn name(&self) -> &'static str;
    fn phase(&self) -> Phase { Phase::Rules }
    fn tick(&mut self, world: &World, ctx: &mut Ctx);
    fn status(&self) -> Option<String> { None }      // feeds `status` / a future machine-readable `state` (phase 3 N8)
}
pub(crate) struct Features(Vec<Box<dyn Feature>>);    // + get_mut::<T>() via Any, names(), tick(phase, ..)
```

No DI container, no dynamic subscription. The list is built once, in code, by `modes::assemble`.

### 2.3 Hook modules become thin adapters, not rewrites

`hudfilter`, `runeshare`, `deathhold`, `menugate`, `defeatdrop`, `orbclaim`, `maphide`, `hawk`, `multiplay` keep their module statics
(they must: their hooks run on game threads). Each gets an adapter of about 20 lines:

```rust
struct HudFilter;   // Phase::AfterCombat
impl Feature for HudFilter { fn tick(&mut self, w: &World, _: &mut Ctx) {
    hudfilter::tick(w.base, w.in_match, &w.rivals.handles, &w.rivals.addrs, w.local) } }
```

`World` gains what they read today: `base`, `in_match`, `rivals {handles, addrs}`, `local_state`, `local_team_no`, `team_size`.
`World::after_combat(&CombatCtl)` refreshes them right after `combat.tick`, so AfterCombat adapters see what they see today.
The order inside the list is today's order (deathhold, menugate, hudfilter, runeshare, defeatdrop, orbclaim, maphide, hawk;
`maphide` must stay after `hudfilter`, because it reads `hudfilter::map_hooked()`). `multiplay` is the first Rules feature, since it
ticks today just before `features.tick` (`:572`). `reviveblock`, `bartrace`, `deathtrace` and `hitkill` stay in the loop beside
`combat` (combat instrumentation and the always-on revive rule).

### 2.4 Mode assembly

```rust
// modes/mod.rs
pub(crate) enum Ruleset { BattleRoyale }      // Arena added with MODE-02; no config key until then
pub(crate) fn assemble(cfg: &Config) -> Features { battle_royale::features(cfg) }
// modes/battle_royale.rs: the whole BR product in one readable list
vec![ EnemyScaling, Entry(mode), Bots, Clock, Nights,
      Match::new(cfg.match_cfg(), vec![Elimination, Defeat, Spectate, RespawnFar, FreeDay1, FlagFallback]),
      /* AfterCombat */ DeathHold, MenuGate, HudFilter, RuneShare, DefeatDrop, OrbClaim, MapHide, Hawk ]
```

An Arena list would reuse `Elimination`, `Defeat`, `DefeatDrop`, `HudFilter`, `MenuGate`, `DeathHold`, and add `RoundReset` and
`BestOf(3)` as new `MatchRule`s. That is the acceptance of phase 2: such a list can be written without touching the loop or any
existing module (proved by a compile-only sketch in PR P2-12).

`GameMode` becomes `Entry { Queue, Offline }` in code only. The `mode = "br"|"offline"` key, the console `mode` command and the log
line `mode: br` stay byte-identical.

### 2.5 Match rules: shared book, ordered rules

```rust
pub(crate) struct MatchBook { seen, max_teams, lone_since, decided, match_over, defeat, count_off, in_expedition, … }
pub(crate) trait MatchRule {
    fn step(&mut self, now_ms: u64, w: &World, nf: &NightFlags, local: Fighter, book: &mut MatchBook, out: &mut MatchOut);
    fn reset(&mut self) {}                       // leaving the expedition
    fn describe(&self, out: &mut StatusParts) {} // the night_status line, unchanged text
}
```

The step order stays as in `NightCore::step`: `record_place` (moved to the head of `RespawnFar::step`; it reads no book state and
nothing before `respawn_step` reads `places`, so this is equivalent), then local, count, spectate, respawn, day1, fallback, describe.
The reset keeps `count_off` and the config values, exactly as `night_match.rs:287-295` does. Constants and log texts do not change.

### 2.6 Config: typed slices, same file

**Change to ARCHITECTURE R4 / decision 4:** do **not** move the toml to sections now. `nro` writes top-level keys only
(`apply_config`, SCENARIOS "Step vocabulary"), `forced_config` and preflight P7 check flat `dev_mode` / `mm_queue` / `br_menu`
(`nro/src/session.rs:209-219`), and every scenario sets flat keys. Sections would break all of them for no product gain while core
gameplay is frozen. Instead: split `parse` into per-slice functions that read the same flat keys into `MatchCfg { day1_secs,
defeat_exit_secs, spectate }`, `TeamCfg`, `MmCfg`, `HawkCfg` (exists), `DevCfg`, and give each module only its slice. File sections
come with Arena (`[arena]`) if ever needed.

### 2.7 Console registry per module

`enum Act { Reply(String), Main(Box<dyn FnOnce(&mut Host) -> String + Send>) }`, where `Host` = `{ world, ctx, features, team, combat,
base, size }` on the main thread. Each module exports `pub(crate) const COMMANDS: &[Spec]`. `console/mod.rs` concatenates the slices
in today's help order. `MainReq`, `overlay::shared::Command`, `run_command` and the closure in the loop go away. After that, a module
owns its commands in its own file, and the `add-console-command` skill shrinks to one step. The console is dev-only, so the player DLL
is not touched by this part at all.

---

## 3. Phase 2: ordered PRs (behaviour of Battle Royale must not change)

**Gate for every PR:** `cargo fmt --check`, clippy `-D warnings` for both builds, `cargo test --workspace`, the golden trace of P2-1
byte-identical, the named scenarios PASS (with the phase 3 `smoke` tag once it exists), and the startup log lines `features: [...]`,
hook install lines and `night_match:` lines unchanged in a `day1-free` run (diff of the filtered lines; phase 3 N9 automates this).
Each PR also updates ARCHITECTURE §2 in the same commit. Commit prefix: `R7-n:` (new card family; R0-R6 keep their meaning).

| # | Goal | Files touched | Behaviour-preservation proof | Size | Risk | Opus review looks at |
|---|---|---|---|---|---|---|
| P2-1 | **Characterization harness.** `WorldBuilder` test helper + scripted timelines (rival defeat + hold + exit, mob death + respawn-far, tide on Day 2, 2- and 4-player win / loss, Day 1 host vs client, event-file fallback, leave-expedition reset, spectate next / prev / F10). Each run records per tick (acts, flags, lines, status) into a golden text checked in under `crates/nrpvp/src/features/golden/`. Tests only | new `features/testkit.rs`, `features/golden/*.txt`, `night_match.rs` (test module) | It *is* the proof for later PRs; passes on today's code | M | low | Timelines cover every `NightAct` variant and every branch of `judge`; time steps hit the debounce edges (799 / 800 ms, 30 s windows) |
| P2-2 | **Delete dead code.** `rules/matchstate.rs` (only its own tests use it); the doubled `#[cfg(feature = "dev")]` at `game/mod.rs:41-42` | `rules/`, `game/mod.rs`, ARCHITECTURE §2 | Unit tests, clippy both builds. No scenario needed | XS | none | grep shows no user outside `rules/`; MEMORY_MAP / PROJECT_PLAN §4B links get a git-ref note |
| P2-3 | **World filled in one place.** Move `game/mod.rs:486-515` into `world::fill_expedition(&mut World, &CombatCtl, base, &mut Input)`; F10 / spectate edges into `game/input.rs`; add `base`, `in_match`, `rivals`, `local_state`, `local_team_no` to `World` (filled, not used yet) | `game/world.rs`, new `game/input.rs`, `game/mod.rs` | Golden unchanged; World unit tests with `FakeMem`; scenarios `spectate`, `esc-defeated`, `defeat-rival` | S | low | Same read order inside the tick; `menu_open` gating of spectate edges kept; no extra memory reads per tick |
| P2-4 | **Ctx executes every request.** `night_acts` (`game/mod.rs:786-851`) moves into `features/effects.rs` as part of `Ctx::flush`; `Ctx::night` -> `Ctx::acts` (same enum, renamed `Act`, same variants and log text) | `features/mod.rs`, new `features/effects.rs`, `night_match.rs`, `game/mod.rs` | Golden (acts identical); scenarios `day1-free` (ClockSpeed, GameFlag), `defeat-rival` (DeathHold, MenuGate), `spectate` (Follow), `mi-rw3-items` by name (DropItems) | S | low | Flush order kept: acts before flags before mm before clock, exactly as today (night_acts runs before `fctx.flush`, `:574-580`); the FOLLOW_WARNED once-only warning kept |
| P2-5 | **Feature list becomes a list.** `Features(Vec<Box<dyn Feature>>)`, `Phase`, `get_mut::<T>()`; the console calls for mode / clock / nights / bots go through `get_mut`; same five features in the same order | `features/mod.rs`, `game/mod.rs`, `startup.rs` | Unit test asserts names and order; the `features: [...]` log line is identical | S | low | No `dyn` on hot paths that matters (5-20 virtual calls per 50 ms); `get_mut` cannot fail silently (log on a miss) |
| P2-6 | **Hook modules into the list as AfterCombat adapters**, in today's order; `multiplay` as the first Rules feature; the loop keeps `reviveblock`, `bartrace`, `deathtrace`, `hitkill` beside `combat` | new `features/hooks.rs` (adapters), `game/world.rs` (`after_combat`), `game/mod.rs` | Golden; scenarios `maphide`, `spectate`, `esc-defeated`, `defeat-rival`, `match-loss`; by name `mi-rw9-rivals`, `mi-rw8-runes`, `mi-rw3-items`; log check: hook install lines appear in the same order with the same slot numbers | M | **medium** | Arguments identical to `game/mod.rs:630-706`; AfterCombat sees the post-`combat.tick` rivals; `maphide` after `hudfilter`; the hardware-slot claim order unchanged (deathhold / menugate / defeatdrop / reviveblock); the heartbeat "combat ms" bucket still covers them |
| P2-7 | **Mode assembly.** `modes/mod.rs` (`Ruleset`, `assemble`) + `modes/battle_royale.rs` (the list); `GameMode` -> `Entry` in code (config key, console word and log text unchanged); `startup.rs` calls `assemble` | new `modes/`, `features/mode.rs`, `startup.rs`, `lib.rs` | `features: [...]` line identical; all unit tests; `puppets-smoke` | S | low | Nothing BR-specific is still constructed outside `battle_royale.rs` |
| P2-8 | **Split `night_match` into match rules** (§2.5): `features/br_match/{mod.rs (Match, MatchBook, MatchRule), elimination.rs, defeat.rs, spectate.rs, respawn_far.rs, free_day1.rs, fallback.rs, status.rs}`; constants and log texts kept; console statics stay for now | `features/night_match.rs` -> `features/br_match/`, `modes/battle_royale.rs`, `game/world.rs` (test import) | **Golden byte-identical** + the 25 existing tests moved unchanged (via a `Match::battle_royale(day1, exit, spectate)` constructor) + scenarios `day1-free`, `defeat-mob`, `defeat-rival`, `match-win`, `match-loss`, `spectate` | M-L | **medium-high** | Step order; `record_place` move is equivalent (§2.5); reset keeps `count_off` / config; which rule owns which book field (single writer per field); `night_status` text identical; every file < 400 lines |
| P2-9 | **Typed config slices**, file format unchanged: `config/` folder, `parse` split per slice, features get `MatchCfg` / `TeamCfg` / `MmCfg` / `HawkCfg` / `DevCfg` | `config.rs` -> `config/{mod,parse,slices}.rs`, constructors of features / startup | New snapshot test: `DEFAULT_FILE_TEXT` and the dist dev toml parse to the same `Debug` output as before; the existing config tests unchanged; player-build test `dev_mode_needs`; `puppets-smoke` | M | low-medium | Warnings / rejections of removed keys still logged with the same text; player-mode enforcement (`enforce_player_mode`, `:507`) untouched |
| P2-10a | **Console registry infra**: `Act::Main(closure)`, `Host`, per-module `COMMANDS` slices; migrate the core / team / mm / mode / clock / bots / nights commands | `console/mod.rs`, `game/mod.rs`, `game/team/*`, `features/*` | Console unit tests; a new test: the sorted command name list equals today's; scenarios `puppets-smoke`, `defeat-rival`, `day1-free` | M | medium (dev-only) | Commands that today answer on the pipe thread stay on the pipe thread (no thread change in this PR); `MAIN_WAIT` / `MAX_QUEUED` behaviour kept |
| P2-10b | **Migrate the rest** (hawk, puppet, teleport, orbclaim, net, runeshare, maphide, rivalhud, …) and delete `MainReq`, `overlay::shared::Command`, `run_command`; regenerate CONSOLE.md §3 | `console/`, module files, `overlay/shared.rs`, CONSOLE.md | Same name-list test; `help` text diff only in order where unavoidable; scenarios `maphide`, `mi-rw9-rivals`, `mi-rw8-runes` | M | medium (dev-only) | No command lost; the help text per command unchanged; the player build has no console code (clippy `--no-default-features`) |
| P2-11 | **Loop and discovery split**: `game/discovery.rs` (sections, scan, RTTI, dump, slot lookups); `game/mod.rs` under 400 lines; ARCHITECTURE §2 / §5 / §6 rewritten to as-built; write the `add-feature` skill (Feature skeleton, adapter vs pure rule, phase choice, config slice, COMMANDS slice, golden + scenario) | `game/mod.rs`, new `game/discovery.rs`, ARCHITECTURE.md, `.claude/skills/add-feature/` | Unit tests incl. `rtti_on_real_msvc_binary`; `puppets-smoke`, `day1-free` | S-M | low | Startup log sequence identical (T-006 / T-007a lines); the skill matches the code |
| P2-12 | **Prove the assembly and prepare the next three features**: (a) `#[cfg(test)] modes::arena_sketch()` builds a list without `FreeDay1` / `RespawnFar` and ticks the golden timelines: no ClockSpeed / Teleport acts, elimination still decides; (b) `World::local_in_pvp_combat(window_ms)` from `local_hits` (GRACE-1's input), unit-tested; (c) `team_size` 2 / 3 timelines in the golden set (DUOTRIO-1's pure half: team win, teammate downed is still in) | `modes/`, `game/world.rs`, golden files | Tests only | S | none | (a) compiles without touching loop or rules; (b) uses the same 30 s window semantics as `judge`; (c) results match PROJECT_SCOPE MATCH-02 / 03 |

Order rationale: P2-1 first, because every later PR leans on its golden. P2-2 to P2-5 are low-risk moves that shrink the loop. P2-6 and
P2-8 are the two real-risk PRs; each gets its own scenario round. P2-9 and P2-10 are independent of P2-8 and can go in parallel
branches if two agents work (no shared files except `game/mod.rs` call sites; merge P2-8 first).

**How the three features land afterwards:**
- **GRACE-1** = one new Rules feature `GraceBlock` reading `w.local_in_pvp_combat(..)`, plus a hook adapter for the grace restore
  path (RE still UNKNOWN). Registered in `battle_royale.rs`. Commands in its own `COMMANDS`, test via `WorldBuilder` and one scenario.
- **DUOTRIO-1** = no code at first: golden timelines with `team_size` 2 / 3 (P2-12c) plus the 4-puppet team scenario (needs puppets on
  LOCAL's team, see N10 and §5).
- **SOLO-REVIVE** = a test first (TASKS). If code is needed: one `MatchRule` (`SoloNightDeath`) reading `w.local_revive_item`, which
  already exists (`world.rs:97`).

---

## 4. Phase 3: orchestrator and scenarios, ordered

**Recommendation on timing:** pull **T0 = N1, N2a/b, N3, N4** in *before* P2-6 (about 1-2 days). Phase 2 needs about six scenario
rounds, and at ~40 min of game time per full regression that is the real bill. T0 makes those rounds shorter and trustworthy. The rest
follows phase 2.

| # | Improvement | Files | Proof | Size | Risk | Opus review |
|---|---|---|---|---|---|---|
| N1 (T0) | **Log-file assertions instead of `log 300`.** New steps `wait_log = { contains, absent, timeout_s, since = "step" \| "mark:<name>" \| "run" }` and `expect_log`; `mark = "<name>"` records a byte offset of `runs\<id>\mod\nrpvp.log`. Reads the file directly (the mod truncates it at start, `logging.rs:1-2`), so event bursts can no longer push a line out (TRAPS "console tails are unreliable"). Migrate every `console = "log 300"` wait / expect in `scenarios/*.toml` | `nro-core/src/scenario.rs` (steps + pure matcher with unit tests over text), `nro/src/run.rs`, scenarios, SCENARIOS.md | Unit tests on the matcher (offsets, CRLF, partial last line); re-run `defeat-rival`, `match-loss`, `mi-rw9-rivals` twice each | S | low | Partial-line handling; the offset taken *before* the triggering command; flush latency (the writer flushes, `logging.rs:76`, but how often is not verified: poll at 250 ms) |
| N2a (T0) | **First-key loss after a focus change.** `desk` keys: after `focus()` succeeds (`sys.rs:452`), wait 400 ms, then send one primer key the game ignores (candidate `F24`, VK 0x87; **verify**), then the sequence. `focus` reports whether the focus changed, so the primer goes only after a switch | `nro/src/sys.rs`, `nro/src/desk.rs` | Scripted check: `mi-rw9-markers`' `Q; M` case 10 times with two windows; `pos` menu word after each | S | low | The primer key is truly inert in the menus and in the world (check the key config; ALT is *not* inert: it disembarks the hawk) |
| N2b (T0) | **Keys with a state check and retry**: `keys = { seq, until = { console, contains }, tries = 3 }`. The macros already do this by hand (`invite_members`, the `menu` word of `pos`); make it a step | `nro-core/src/scenario.rs`, `nro/src/run.rs` | Unit test of parsing; `esc-defeated` (Esc opens / closes, checked by `peek` / `pos`), `defeat-rival` end flow | S | low | A retry cannot double-toggle (the check runs before each retry, not only after) |
| N3 (T0) | **Tags and a smoke subset.** `tags = ["smoke", "rules", "hud", "mi", "slow", "lab"]` in each scenario; `regress.ps1 -Tag smoke`; `nro scenarios --tag`; the report prints per-scenario time and a total. Smoke before suite mode = `match-loss` + `day1-free` (~8 min: count, defeat, hold, Day 1 clock and flags) | `nro-core/src/scenario.rs`, `scripts/regress.ps1`, scenarios, SCENARIOS.md | Parse test; one `-Tag smoke` run | XS | none | Labs (`mi-rw3-drop`) tagged `lab` and never in a default run |
| N4 (T0) | **No more wolf deaths.** `navigate = "expedition"` ends with `god 1` unless the scenario says `god = false` (`hp0` already turns god off, CONSOLE help). Remove the hand-written `god 1` steps | `nro/src/run.rs` (`to_expedition`), scenarios | `defeat-mob`, `defeat-rival` (hp0 after god), `match-win` | XS | low | TRAPS: one case where `hp0` was refused after god mode (Erdtree's Favor max HP, MEMORY_MAP 24): the default must not hide that |
| N5 | **Run ids and a run lock.** Id = `<unix_ms>-<name>-<nro pid>` (today `unix seconds-name`, `session.rs:244-252`, can collide); `nro run` takes `runs\.lock` (real-Steam slot, desk input) and fails fast with the holder's pid instead of fighting over `current.txt` / `instances.json`; `nro run` never writes `current.txt` (its State is explicit) | `nro/src/session.rs`, `nro/src/run.rs`, `nro/src/ops.rs` | Unit tests for id and lock (stale lock = dead pid); two `nro run` at once: the second refuses | S | low | **Honest limit:** real parallel runs are bounded by one real-Steam game, ~7 GB RAM per emulated game and one desk input. I recommend serial with a lock, not parallelism |
| N6 | **Headless rules tier (pure Rust, seconds, in CI).** One Rust test per single-game rules scenario, named the same (`sim_defeat_rival`, `sim_match_win`, `sim_day1_free`, …), built on P2-1's `WorldBuilder`: the same commands as events (`hp0`, `rival_hit`, `puppet_hp n 0`, `clock`), the same assertions on `night_status` text and acts. Covers the rules; the game scenarios remain for hooks, UI and timing | `crates/nrpvp/src/features/br_match/sim_tests.rs` (or `features/testkit.rs`), SCENARIOS.md (a column "headless twin") | `cargo test` | M | low | Each twin asserts the same strings as its TOML; no fake that hides game timing (respawn 14 s, comeback hold) without saying so in the test name |
| N7 | **Machine-readable state.** Dev console `state` = `key=value` lines aggregated from `Feature::status()` and core services (phase, defeated, out count, decided winner, mob_deaths, death_held, menugate, rivalhud counters, team_type, world). Scenario `expect = { console = "state", eq = { defeated = "rival", out = "2" } }` | `console/`, features' `status()`, `nro-core/src/scenario.rs` | Unit tests for the format and the matcher; migrate `match-loss`, `defeat-rival` asserts | S-M | low | Keys are stable names, documented in CONTRACT.md; no game reads added (status from the last tick) |
| N8 | **Golden-log checks.** `[golden] file = "scenarios/golden/<name>.txt", prefixes = ["night_match:", "M5-9 deathhold:", "menugate:", "features:"]`: after the run, filtered lines with timestamps and addresses normalised must contain the golden lines *in order* (a subsequence, so extra lines are fine); `nro golden bless <run>` writes it. Also gives phase 2 its log-diff gate | `nro-core` (pure normaliser + subsequence matcher, unit-tested), `nro/src/run.rs`, `scenarios/golden/` | Unit tests; bless on main, then check on a P2 branch | S-M | low | The normaliser removes only volatile parts (time, 0x addresses, ms values); verdict FAIL lists the first missing line |
| N9 | **Suite mode: one launch, many scenarios.** `nro suite --tag smoke`: groups scenarios by *launch signature* (hash of config keys that need a relaunch: `mode`, `mm_queue`, `team_size`, `allow_multi_instance`, …), launches once per group, `title_to_world` once; per scenario: mod console `reset` (new, dev: every feature and hook module back to the Roundtable state, puppets cleared, flag 10007420 off) then `navigate = "expedition"`, then the steps; *live* config via a new console `cfg <key> <value>` for the `MatchCfg` slice (`day1_secs`, `defeat_exit_secs`, `spectate`), which needs P2-9. A scenario that ends in an expedition is left through the GAME_OVER path (flag 7001 + the result screens, as in `defeat-rival`). Target: all single-game scenarios in ~15 min instead of ~40 | `nro/src/run.rs` (suite runner), `nro-core/src/scenario.rs` (`needs`, signature), mod `console` + `features` (`reset`, `cfg`) | Run the whole single-game set as a suite and one by one: same verdicts 3 times in a row | L | **medium** | State bleed between scenarios (module statics: deathhold armed, menugate, orbclaim book, spectate override); the runner checks an invariant before each scenario (`state` shows no defeat, no hold, 0 puppets) and falls back to a relaunch when it fails |
| N10 | **In-process input spike, then pick-ups by command.** Spike: does the game read the mod's virtual pad (`pad`, `game/vpad.rs`, XInputGetState hook, ORCH-1) while its window is *in the background*? If yes, gameplay inputs in `mi-*` go through `pad` (no focus, no lost key), and item pick-up becomes `pad <interact button>` (Xbox layout Y is interact in Elden Ring; **verify for Nightreign**). If no: an RE card for the game's own pick-up call (UNKNOWN today) or the scenarios keep using `drop_gear` / `mob_kill` style test aids. Also needed for DUOTRIO-1: a `puppet_team <slot> <team>` aid so puppets can be LOCAL's teammates (whether a puppet's team value drives the rules is UNKNOWN) | spike notes in MULTI-INSTANCE.md; then `nro/src/run.rs`, scenarios `mi-rw3-items` (talisman / consumable pick-up, which FAILED 3 times by keys) | Spike: `pad_status` counters plus a screenshot while window 2 has focus; then `mi-rw3-items` with pick-ups twice | S (spike) + M | medium | Background input may be filtered by the game (focus loss pauses input in many FromSoftware titles); never fake a pick-up by writing the inventory without saying so in the scenario name |
| N11 | **MCP for agents.** `run_start` (returns run id at once) + `run_status` (steps so far, verdict) instead of one blocking call of 5-9 min; tools `wait_log`, `state`, `suite`, `keys` / `console` with `instance`; `screenshot` returns the image plus `looks_black` | `nro-mcp/src/main.rs`, `nro/src/ops.rs` | Manual MCP session; unit test of the JSON schema | S-M | low | A background run cannot outlive `nro stop`; tool texts stay short |
| N12 | **Content tidy.** Replace fixed `sleep_ms` with waits where a state exists; `defeat_exit_secs` 15 -> 8 where the scenario only needs the screen; per-step time budget in the report; screenshots stay for HUD (RW-9) but every verdict line of a rules scenario must hold without a screenshot (state / golden / log) | scenarios, SCENARIOS.md | Each touched scenario twice | S | low | No scenario loses the negative checks (`panic`, `no free hardware slot`) |

On "verdicts need screenshots to be trusted": N1 + N7 + N8 make every rules verdict rest on state and log evidence. The HUD filter
(RW-9) still needs pixels or the hook counters (`rivalhud` already reports per-site counters). Keep its screenshots, but judge on the
counters.

---

## 5. Pre-flight: risks and what I could not verify

1. **Nothing was built or run** (another process owned cargo). Line numbers are from 5ec1652. The ARCHITECTURE line counts (1576 /
   1756 / 1332) include blank lines; mine (1503 / 1658 / 1245) do not. Same files.
2. **Tick order** (§1.3) is the main behaviour risk of P2-3, P2-4 and P2-6. Its proof relies on the golden for the rules, and on
   scenarios plus hook-install log order for the hook modules. A 50 ms shift would pass most scenarios and still change edge timing:
   a reviewer must check the order by reading, not trust a green run.
3. **Hardware debug slots (DR0-3)** are claimed lazily by `deathhold`, `menugate`, `defeatdrop`, `reviveblock`, `quietinvite`,
   `menuflow` and the traces. Any reorder can turn into "no free hardware slot" only in some sessions. P2-6 must keep the call order;
   add a negative check `absent = "no free hardware slot"` to every touched scenario.
4. **Pipe-thread commands that touch game memory** (`teleport::*`, `puppet::*`, `orbclaim::*`, `menugate::set_active` from
   `console/mod.rs:111-160`) break the "one writer thread" rule today. P2-10 keeps them where they are on purpose (no behaviour change).
   Moving them to the main thread is a separate, later card.
5. **`rules/matchstate.rs` deletion:** PROJECT_PLAN §4B points at it as a design model. Deleting code is fine (git keeps it), but the
   doc link needs a git ref, and Opus should confirm Leonardo does not want it as Arena's starting point. I recommend deleting it: it
   models scope v1 rules (no elimination before sudden death) that contradict shipped BR.
6. **Config sections not done** (§2.6) contradicts ARCHITECTURE decision 4 (2026-10-20). It needs a DECISIONS entry by Opus.
7. **Not verified:** that F24 is inert as a primer key; how often the log writer flushes (N1 polls, so this is low risk); whether the
   game accepts XInput while unfocused (N10 spike); the Nightreign pad button for interact / pick-up; whether a puppet's team value
   drives the mod's team logic (needed for a 4-puppet Duo / Trio scenario); whether leaving an expedition through flag 7001 + result
   screens is reliable enough for suite mode (it works in `defeat-rival`, but has not been run back-to-back).
8. **Suite mode state bleed** is the largest phase 3 risk. Mitigation: invariant check + relaunch fallback (N9). Do not make suite
   mode the only way to run: one scenario per launch stays the reference verdict before a release zip.
9. **Golden traces can freeze bugs.** P2-1 records today's behaviour, warts included. That is right for a frozen RC. When a later
   card changes behaviour on purpose, it re-blesses the golden in the same PR and says so.
10. **Size estimate:** phase 2 is about 12-13 PRs, mostly S/M. P2-6 and P2-8 need real review time. With T0 in place, each needs one
    smoke run plus its named scenarios (about 15-25 min of game time) instead of a full 40-minute regression; the full set runs
    before merging P2-8 and at the end of phase 2.
