# Task Board

The unit of work is the **task card**. Opus writes and reviews cards. Sonnet executes one card at a time.
The Human runs anything that needs the game open.

**Status values:** `todo` · `ready` (all dependencies done) · `in-progress` · `review` (Sonnet done, waiting for Opus) ·
`done` · `blocked` (with reason).
**Owner values:** `SONNET/implementer` · `SONNET/researcher` · `OPUS` · `HUMAN` · combinations, where the
first owner does the work and the second verifies.

## How to run a card

- **From an Opus session:** "Run T-00X." Opus spawns the matching agent from `.claude/agents/` with the card
  text, reviews the result, and sets the status.
- **From a Sonnet session directly:** "Execute T-00X from TASKS.md, following CLAUDE.md." When done, set the
  status to `review`. Opus moves it to `done`.
- **HUMAN steps:** follow the card. Paste logs and observations into the result section (or into
  TESTING.md for test cases). An agent then tidies them up.

## Card template

```
### T-XXX — <title>
Owner: … | Status: … | Depends on: … | Phase: …
Goal: one sentence, the question answered or the capability added.
Context: links to the doc sections the executor must read.
Steps: numbered and concrete.
Deliverables: files created or changed.
Acceptance: objective checks. Mark the HUMAN checks.
Out of scope: what NOT to do.
Result: (filled in by the executor: what happened, evidence, follow-ups)
```

---

## Phase 0: Reconnaissance and feasibility

Dependency graph (critical path in bold):
**T-001 → T-004 → T-006 → T-007 → T-008 → T-009 → T-010**. T-002, T-003, and T-005 run in parallel early.

---

### T-000 — Human prerequisites
Owner: HUMAN | Status: ready | Phase: 0
Goal: unblock all build and test work.
Steps:
1. Install rustup (https://rustup.rs). Use the default stable `x86_64-pc-windows-msvc` toolchain. VS 2022
   C++ build tools are already present.
2. Confirm the **two-player test setup**: two PCs, two Steam accounts that each own Nightreign, and the same SC
   build on both. Record it in TESTING.md §1.
3. Back up `%APPDATA%\Nightreign\76561198342403479\` somewhere outside the game folders.
4. Optional: let an agent run `git init` in the repo (asked separately).
Acceptance: `cargo --version` works in a new terminal, and TESTING.md §1 is filled in.

### T-001 — Repository and DLL skeleton
Owner: SONNET/implementer → HUMAN | Status: todo | Depends on: T-000 (rustup) | Phase: 0
Goal: a buildable `nrpvp.dll` that loads in the game and writes a log line, plus scripts to build and launch.
Context: CLAUDE.md, TECHNICAL_NOTES.md §2.
Steps:
1. Cargo workspace at the repo root (`Cargo.toml`, `rust-toolchain.toml` pinned to stable, `.gitignore` for
   `target/`, `dist/`, `*.log`).
2. `crates/nrpvp`: a `cdylib`. `DllMain` on `DLL_PROCESS_ATTACH` spawns a thread (no work under the
   loader lock). The thread sets up a `tracing` file logger at `<dll dir>/nrpvp.log` and logs: the crate
   version, the `nightreign.exe` module base and size, the exe FileVersion, and a timestamp.
3. `scripts/build.ps1`: `cargo build --release`, then copy the DLL to `dist/`.
4. `profiles/nrpvp-dev.me3.toml`: a me3 profile for Nightreign that loads **only** `dist/nrpvp.dll` for
   now. **Look up the exact profile schema in the me3 docs (https://me3.help) for v0.11. Do not guess
   keys.** Note in a comment which doc page you used.
5. `scripts/launch.ps1`: runs `me3 launch` with that profile (exact CLI from `me3 --help`).
Deliverables: the files above, plus a README.md "Build & launch" section.
Acceptance: `./scripts/build.ps1` succeeds (agent-verifiable). **HUMAN:** launch via `launch.ps1`; the game
reaches the title screen, and `nrpvp.log` has the startup lines.
Out of scope: SC loading, hooks, game structures.

### T-002 — Baseline SC session and friendly-fire behaviour
Owner: HUMAN (+ OPUS writes up) | Status: ready | Depends on: T-000 | Phase: 0
Goal: establish vanilla SC behaviour before we change anything.
Steps: run TESTING.md test cases **TC-001** (join and shared expedition) and **TC-002** (friendly-fire
probe) with the stock SC launcher. Record the SC version shown in-game or on Nexus, and any in-game SC
items or menus about PvP or friendly fire.
Acceptance: TC-001 and TC-002 results are recorded with pins.
Out of scope: me3, our DLL.

### T-003 — Research: Nightreign SC features, settings, and terms
Owner: SONNET/researcher | Status: ready | Phase: 0
Goal: know everything SC officially offers that relates to PvP, teams, friendly fire, player cap, mod
loaders, and permissions, before we build around it.
Steps:
1. Find the NR SC Nexus page, readme or changelog, the author's statements, and community wikis or
   Discords that are publicly indexed.
2. Answer: the current version and its date; all `nrsc_settings.ini` keys (especially `[GAMEPLAY]`); any
   PvP or friendly-fire toggle or item; player cap and stability notes; how SC should be loaded with
   ModEngine/me3; the license and permission terms (redistribution, use as a dependency).
3. Compare with ER SC's PvP features as context only. Label everything clearly as ER.
Deliverables: an update to TECHNICAL_NOTES.md §3 with `[SOURCE: link]` tags. Unanswered items stay
`UNKNOWN`.
Acceptance: every claim has a link. There are no guessed ini keys.
Out of scope: binary analysis of nrsc.dll beyond plain strings.

### T-004 — me3 loads SC and nrpvp together
Owner: SONNET/implementer → HUMAN | Status: todo | Depends on: T-001 | Phase: 0
Goal: one launch path where SC networking works **and** our DLL is loaded.
Steps:
1. Extend the me3 profile so it loads `nrsc.dll` from the game folder as a native, with whatever the me3 docs
   specify for `modengine_ext_init` initializers and early loading (`load_early`, `load_before`). Load
   `nrpvp.dll` after SC.
2. Document the profile in README.md.
3. **Fallback**, used only if step 1 fails in HUMAN testing: investigate whether SC's own launcher can load
   an extra DLL (for example a proxy DLL in the game folder). Write the findings up before implementing
   anything.
Acceptance (**HUMAN**): launched via `launch.ps1`, the SC lobby works (TC-001 passes with two players), and
`nrpvp.log` is written on both PCs.
Out of scope: changing SC files.

### T-005 — Research: ER engine reference for team type, players, and sessions
Owner: SONNET/researcher | Status: ready | Phase: 0
Goal: a precise cheat sheet that tells the NR offset hunt where to look.
Context: TECHNICAL_NOTES.md §4.
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
Deliverables: `docs/re/er-reference.md`.
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
Acceptance (**HUMAN** run): the log lists singletons including `WorldChrMan` (or the NR equivalent name)
with plausible addresses. Results go into TECHNICAL_NOTES.md §4 as `[VERIFIED: log]`.
Out of scope: reading fields inside singletons.

### T-007 — Map local player → team type in NR (CE session)
Owner: OPUS + HUMAN | Status: todo | Depends on: T-006 | Phase: 0
Goal: verified pointer path from `WorldChrMan` to each `PlayerIns` and its `team_type` (and
`ChrTemporaryTeamTypeSlot` if reachable) on NR 1.3.3.
Steps: Opus prepares a Cheat Engine plan using the ER layout as a starting hint. The Human runs it solo,
then as SC host, then as SC client. Record the team-type values seen for self, a partner, normal enemies,
and a boss. Save the CE table to `docs/re/nrpvp.CT`.
Acceptance: the path and observed values are written into TECHNICAL_NOTES.md §4 as `[VERIFIED: CE]` with
the game version.

### T-008 — Debug controls: read and write team type
Owner: SONNET/implementer → HUMAN | Status: todo | Depends on: T-007 | Phase: 0
Goal: a reproducible way to run E-003 without Cheat Engine on both PCs.
Steps:
1. Every 1 second, log each player's handle or name, Steam ID if known, and team type, but only when it
   changes.
2. Config `nrpvp.toml` (next to the DLL) with `hostile_team_type = <value from T-007>` and hotkeys:
   F5 sets the local player to `hostile_team_type`, F6 restores the original value, and F7 re-applies
   every frame or tick (persist mode on/off).
3. Every write is logged with old → new values.
Acceptance (**HUMAN**, solo): pressing F5 changes the logged team type, and normal enemies' behaviour toward
the player is observed and noted. F6 restores the original. Nothing crashes.
Out of scope: any network messages of our own, UI overlay, team assignment logic.

### T-009 — E-003: the decisive PvP experiment
Owner: HUMAN (OPUS designs and analyses) | Status: todo | Depends on: T-004, T-008 | Phase: 0
Goal: answer H1 (TECHNICAL_NOTES.md §4). Can SC players damage each other when they have hostile team
types, and does it stay in sync?
Steps: run test case **TC-003** in TESTING.md (the matrix covers who flips: victim, attacker, or both, and which
values are used).
Acceptance: the matrix is fully recorded, with logs from both PCs attached under `docs/experiments/E-003/`.

### T-010 — Phase 0 feasibility report and go/no-go
Owner: OPUS | Status: todo | Depends on: T-002, T-003, T-009 | Phase: 0
Goal: answer the 8 Phase 0 questions in the skeleton (§7). Choose the Phase 1–2 approach (H1, or A1–A3),
and write the Phase 1–2 cards.
Deliverables: the "Phase 0 report" section in ROADMAP.md, an updated ADR log, new cards.

### T-011 — Research: param and EMEVD routes to hostility (backup path)
Owner: SONNET/researcher | Status: ready | Phase: 0 (parallel, low priority)
Goal: be ready for H1 failing.
Steps: for NR specifically, find out whether SpEffectParam or AtkParam have team-changing or "hit both
teams" semantics (field docs from Paramdex). Find whether NR's EMEDF has a set-team-type instruction
(DarkScript3 or soulsmods sources). Find whether Smithbox, WitchyBND, and DarkScript3 currently support
NR 1.3.x.
Deliverables: TECHNICAL_NOTES.md §4 "Alternative mechanisms" expanded with sources.
Out of scope: making a modded regulation.bin.
