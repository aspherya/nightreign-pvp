# Task Board

The unit of work is the **task card**. Opus writes and reviews cards. Sonnet executes one card at a time.
The Human runs anything that needs the game open.

**Status values:** `todo` · `ready` (all dependencies done) · `in-progress` · `review` (Sonnet done, waiting for Opus) ·
`done` · `blocked` (with reason).
**Owner values:** `SONNET/implementer` · `SONNET/researcher` · `OPUS` · `HUMAN` · combinations, where the
first owner does the work and the second verifies.

**Testing constraint:** day to day, only Leonardo tests, solo. Two-player checks are batched into
**Friend Sessions** (FS-1, FS-2, …). Their runbooks are in TESTING.md §6. A card is `done` when its
**solo** acceptance passes. Its multiplayer checks then sit in the FS runbook.

## How to run a card

- **From an Opus session:** "Run T-00X." Opus spawns the matching agent from `.claude/agents/` with the card
  text, reviews the result, and updates this board. Several agents may run in parallel, so **agents do not
  edit this file**. They write their Result into their deliverable or reply, and Opus copies it here.
- **From a Sonnet session directly:** "Execute T-00X from .claude/TASKS.md, following .claude/CLAUDE.md." When
  it's done, it sets the status to `review` and fills in Result. Opus moves the card to `done`.
- **HUMAN steps:** follow the card. Paste logs and observations into chat or into TESTING.md, and an agent tidies them up.

## Card template

```
### T-XXX — <title>
Owner: … | Status: … | Depends on: … | Phase: …
Goal: one sentence, the question answered or the capability added.
Context: links to the doc sections the executor must read.
Steps: numbered and concrete.
Deliverables: files created or changed.
Acceptance: objective checks; mark the HUMAN-solo checks; two-player checks go to an FS runbook.
Out of scope: what NOT to do.
Result: (filled in after execution: what happened, evidence, follow-ups)
```

---

## Phase 0: Reconnaissance and feasibility

Critical path (all solo): **T-001 → T-004 → T-006 → T-007 → T-008 → T-012**. Then **FS-1** (T-009), then T-010.
Research cards T-003, T-005, and T-011 run in parallel from the start.

**FS-1 readiness gate:** T-004, T-008, and T-012 are done and the FS-1 runbook (TESTING.md §6) is final.
Opus tells Leonardo when to book the friend session.

---

### T-000 — Human prerequisites
Owner: HUMAN | Status: **done** (2026-10-09) | Phase: 0
Result: Rust 1.99.0 stable msvc is installed (`~/.cargo/bin`). The repo is on GitHub (`notgonnaleo/nightreign-pvp`).
The second PC and Steam account are only available during Friend Sessions. Back up saves before the first
launch through me3 (T-001 HUMAN step).

### T-001 — Repository and DLL skeleton
Owner: SONNET/implementer → HUMAN | Status: in-progress | Depends on: T-000 | Phase: 0
Goal: a buildable `nrpvp.dll` that loads in the game and writes a log line, plus scripts to build and launch.
Context: .claude/CLAUDE.md, .claude/TECHNICAL_NOTES.md §2.
Steps:
1. Cargo workspace at the **repo root** (`Cargo.toml`, `rust-toolchain.toml` pinned to stable, `.gitignore` for
   `target/`, `dist/`, `*.log`).
2. `crates/nrpvp`: a `cdylib`. `DllMain` on `DLL_PROCESS_ATTACH` spawns a thread (no work under the
   loader lock). The thread sets up a `tracing` file logger at `<dll dir>/nrpvp.log` and logs: the crate
   version, the `nightreign.exe` module base and size, the exe FileVersion, and a timestamp.
3. `scripts/build.ps1`: `cargo build --release`, then copy the DLL to `dist/`.
4. `profiles/nrpvp-dev.me3.toml`: a me3 profile for Nightreign that loads **only** `dist/nrpvp.dll` for
   now. **Look up the exact profile schema in the me3 docs (https://me3.help) for v0.11. Do not guess
   keys.** Note in a comment which doc page you used.
5. `scripts/launch.ps1`: runs `me3 launch` with that profile (exact CLI from `me3 --help`).
Deliverables: the files above, plus a "Build & launch" section in .claude/README.md.
Acceptance: `./scripts/build.ps1` succeeds (agent-verifiable). **HUMAN-solo:** launch via `launch.ps1`; the game
reaches the title screen, and `nrpvp.log` has the startup lines.
Out of scope: SC loading, hooks, game structures.
Result (2026-10-09, Sonnet; reviewed by Opus): the workspace, `crates/nrpvp` (lib/startup/logging/sysinfo),
`profiles/nrpvp-dev.me3.toml`, and `scripts/{build,launch}.ps1` are created. build, clippy (`-D warnings`), test, and fmt are
clean. `me3 profile show` parses the profile. A smoke test via LoadLibrary outside the game wrote the log
correctly. The me3 schema comes from the me3 repo at tag v0.11.0 docs (me3.help returned 403).
**Status: awaiting HUMAN-solo launch.** Expect `dist/nrpvp.log` to contain `exe FileVersion: 1.3.3.0`. If
there's no log, check `%LOCALAPPDATA%\garyttierney\me3\data\logs`. Arxan is not handled yet; if the game
crashes at startup, that becomes a T-004 question.

### T-002 — Baseline SC behaviour
Owner: HUMAN (+ OPUS writes up) | Status: ready | Depends on: — | Phase: 0
Goal: establish vanilla SC behaviour before we change anything.
Solo part (now): launch with the stock `nrsc_launcher.exe` and host an SC session alone. Note the SC version
string shown anywhere in-game, and any SC-specific items, menus, or options (especially PvP, friendly-fire,
or "rulebook"-style items). Start a solo expedition and confirm it plays normally. Record this as TC-000.
Two-player part: TC-001 and TC-002 in **FS-1**.
Acceptance: TC-000 is recorded with pins.

### T-003 — Research: Nightreign SC features, settings, and terms
Owner: SONNET/researcher | Status: in-progress | Phase: 0
Goal: know everything SC officially offers that relates to PvP, teams, friendly fire, player cap, mod
loaders, and permissions, before we build around it.
Steps:
1. Find the NR SC Nexus page, readme or changelog, the author's statements, and community wikis or
   Discords that are publicly indexed.
2. Answer: the current version and its date; all `nrsc_settings.ini` keys (especially `[GAMEPLAY]`); any
   PvP or friendly-fire toggle or item; player cap and stability notes; how SC should be loaded with
   ModEngine/me3; the license and permission terms (redistribution, use as a dependency).
3. Compare with ER SC's PvP features as context only. Label everything clearly as ER.
Deliverables: `.claude/research/T-003-seamless-coop.md` (Opus merges the key facts into TECHNICAL_NOTES.md §3).
Acceptance: every claim has a link. There are no guessed ini keys.
Out of scope: binary analysis of nrsc.dll beyond plain strings.

### T-004 — me3 loads SC and nrpvp together
Owner: SONNET/implementer → HUMAN | Status: todo | Depends on: T-001 | Phase: 0
Goal: one launch path where SC is active **and** our DLL is loaded.
Steps:
1. Extend the me3 profile so it loads `nrsc.dll` from the game folder as a native, with whatever the me3 docs
   specify for `modengine_ext_init` initializers and early loading (`load_early`, `load_before`). Load
   `nrpvp.dll` after SC.
2. Document the profile in .claude/README.md.
3. **Fallback**, used only if step 1 fails in HUMAN testing: investigate whether SC's own launcher can load
   an extra DLL (for example a proxy DLL in the game folder). Write the findings up before implementing
   anything.
Acceptance (**HUMAN-solo**): launched via `launch.ps1`, SC is clearly active (same SC-specific UI or items
as TC-000, uses `.co2` saves), a solo SC session can be hosted, and `nrpvp.log` is written.
Two-player check (**FS-1**): a friend joins through this launch path (TC-001).
Out of scope: changing SC files.

### T-005 — Research: ER engine reference for team type, players, and sessions
Owner: SONNET/researcher | Status: in-progress | Phase: 0
Goal: a precise cheat sheet that tells the NR offset hunt where to look.
Context: .claude/TECHNICAL_NOTES.md §4.
Steps:
1. In fromsoftware-rs (`crates/eldenring/src/cs/` and `crates/shared`), document with file and line
   references:
   - how `WorldChrMan` reaches the local player and other players (`PlayerIns` array or list);
   - where `team_type` sits in `ChrIns` and how it's read in team-relation checks;
   - `ChrTemporaryTeamTypeSlot` if mapped;
   - how singletons are located (the FD4 singleton finder) and what `fromsoftware-shared` exports to use it;
   - session manager and net_chr_sync basics (player handles, Steam IDs).
2. Find the **team-type enum names and values** for NR (Paramdex or Smithbox NR `NpcParam.teamType` enum,
   or another published source). Cite the source. If no NR source exists, give the ER one and mark it
   `[HYPOTHESIS: same in NR]`.
Deliverables: `.claude/re/er-reference.md`.
Acceptance: every offset or name has a link. Nothing is presented as verified for NR.
Out of scope: writing code.

### T-006 — Singleton discovery in NR
Owner: SONNET/implementer → HUMAN | Status: todo | Depends on: T-001, T-005 | Phase: 0
Goal: prove we can locate engine singletons (WorldChrMan and others) in NR 1.3.3 without hardcoded
addresses.
Steps:
1. Try `fromsoftware-shared`'s singleton finder against NR. If it doesn't apply, implement the same FD4
   singleton-scan technique documented in T-005, with a source link.
2. At startup, after the game finishes initialising (wait or poll; document the approach), log every
   discovered singleton name and address.
Acceptance (**HUMAN-solo**): the log lists singletons including `WorldChrMan` (or the NR equivalent name)
with plausible addresses. Results go into TECHNICAL_NOTES.md §4 as `[VERIFIED: log]`.
Out of scope: reading fields inside singletons.

### T-007 — Map local player → team type in NR (CE session)
Owner: OPUS + HUMAN | Status: todo | Depends on: T-006 | Phase: 0
Goal: verified pointer path from `WorldChrMan` to each `PlayerIns` and its `team_type` (and
`ChrTemporaryTeamTypeSlot` if reachable) on NR 1.3.3.
Steps: Opus prepares a Cheat Engine plan using the ER layout as a starting hint. The Human runs it solo
(vanilla-offline-style SC solo session), recording the team-type values for self, normal enemies, a field
boss, and a night boss if reachable. Save the CE table to `.claude/re/nrpvp.CT`.
Acceptance (**HUMAN-solo**): the path and observed values are written into TECHNICAL_NOTES.md §4 as
`[VERIFIED: CE]` with the game version.
Two-player check (**FS-1**): the partner's PlayerIns is reachable on the same path, and its team type is
recorded from both host and client views.

### T-008 — Debug controls: read and write team type
Owner: SONNET/implementer → HUMAN | Status: todo | Depends on: T-007 | Phase: 0
Goal: a reproducible way to run the experiments without Cheat Engine on both PCs.
Steps:
1. Every 1 second, log each player's handle or name, Steam ID if known, and team type, but only when it
   changes. Also log the team type of the nearest N enemies on request (hotkey F8).
2. Config `nrpvp.toml` (next to the DLL) with `hostile_team_type = <value from T-007>` and hotkeys:
   F5 sets the local player to `hostile_team_type`, F6 restores the original value, and F7 re-applies
   every tick (persist mode on/off).
3. Every write is logged with old → new values.
Acceptance (**HUMAN-solo**): F5, F6, and F7 work as logged and nothing crashes. Behaviour checks are in T-012.
Out of scope: any network messages of our own, UI overlay, team assignment logic.

### T-012 — E-002: does team type drive hostility in NR? (solo)
Owner: HUMAN (OPUS designs and analyses) | Status: todo | Depends on: T-008 | Phase: 0
Goal: before involving a friend, prove solo that (a) our write mechanism works, (b) the engine uses the
player's team type for hostility in NR, and (c) the value survives game events, or find out what resets it.
Steps: run **TC-004** in TESTING.md.
Acceptance: TC-004 is recorded. Opus decides whether the value set and persistence approach for FS-1 is
sound, or whether a different mechanism (T-011 alternatives) is needed first.

### T-009 — E-003: the decisive PvP experiment (FS-1)
Owner: HUMAN + friend (OPUS designs and analyses) | Status: todo | Depends on: FS-1 readiness gate | Phase: 0
Goal: answer H1 (TECHNICAL_NOTES.md §4). Can SC players damage each other when they have hostile team
types, and does it stay in sync?
Steps: run the **FS-1 runbook** (TESTING.md §6): TC-001 → TC-002 → TC-003.
Acceptance: all three are recorded, with logs from both PCs under `.claude/experiments/FS-1/`.

### T-010 — Phase 0 feasibility report and go/no-go
Owner: OPUS | Status: todo | Depends on: T-002, T-003, T-009 | Phase: 0
Goal: answer the 8 Phase 0 questions in the skeleton (§7). Choose the Phase 1–2 approach (H1, or A1–A3),
and write the Phase 1–2 cards.
Deliverables: the "Phase 0 report" section in ROADMAP.md, an updated ADR log, new cards.

### T-011 — Research: param and EMEVD routes to hostility (backup path)
Owner: SONNET/researcher | Status: in-progress | Phase: 0 (parallel, low priority)
Goal: be ready for H1 failing.
Steps: for NR specifically, find out whether SpEffectParam or AtkParam have team-changing or "hit both
teams" semantics (field docs from Paramdex). Find whether NR's EMEDF has a set-team-type instruction
(DarkScript3 or soulsmods sources). Find whether Smithbox, WitchyBND, and DarkScript3 currently support
NR 1.3.x.
Deliverables: `.claude/research/T-011-alt-hostility.md` (Opus merges the key facts into TECHNICAL_NOTES.md §4).
Out of scope: making a modded regulation.bin.
