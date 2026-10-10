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

**FS-1 readiness gate:** T-004, T-008, T-012 (done), T-015, and T-016 are done and the FS-1 runbook (TESTING.md §6) is final.
Opus tells Leonardo when to book the friend session.

---

### T-000 — Human prerequisites
Owner: HUMAN | Status: **done** (2026-10-09) | Phase: 0
Result: Rust 1.99.0 stable msvc is installed (`~/.cargo/bin`). The repo is on GitHub (`notgonnaleo/nightreign-pvp`).
The second PC and Steam account are only available during Friend Sessions. Back up saves before the first
launch through me3 (T-001 HUMAN step).

### T-001 — Repository and DLL skeleton
Owner: SONNET/implementer → HUMAN | Status: **done** (2026-10-09) | Depends on: T-000 | Phase: 0
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
**HUMAN-solo PASSED 2026-10-09:** log showed `nightreign.exe base=0x7FF791550000 size=0x7C5D600`, `exe FileVersion: 1.3.3.0`. No crash; Arxan was not an issue for a passive native. Expect `dist/nrpvp.log` to contain `exe FileVersion: 1.3.3.0`. If
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
Owner: SONNET/researcher | Status: **done** (2026-10-09) | Phase: 0
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
Result: `.claude/research/T-003-seamless-coop.md`, merged into TECHNICAL_NOTES.md §1 and §3. SC 1.13; no PvP/FF/team features; no hidden ini keys;
me3 `load_early` with no initializer is field-reported working; restrictive terms (never bundle, patch, or hook SC; ask Yui before a public release).
New risk: SC may revert runtime edits (as ER SC does), which TC-004 will expose.

### T-004 — me3 loads SC and nrpvp together
Owner: SONNET/implementer → HUMAN | Status: **done** (2026-10-09; SC loads under me3, see TESTING.md results) | Depends on: T-001 | Phase: 0
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
Result (2026-10-09, Sonnet; reviewed by Opus): new `profiles/nrpvp-sc.me3.toml` loads nrsc.dll (absolute path, `load_early = true`, no
initializer, so me3 calls `modengine_ext_init` via its ME2 shim) and then nrpvp.dll (`load_after` nrsc.dll). `launch.ps1 -Profile sc|dev`
(default sc) and `-DisableArxan`. Basis: me3 v0.11.0 source; issue #621 (SC Dec 2025 update needs early load,
fixed by `load_early` in me3 v0.10). Parsed by `me3 profile show`. **Untested in-game.**
HUMAN-solo checks: SC UI present; `.co2` save; no "loaded early" or `Method "SignIn"` error; solo session hostable;
fresh `dist/nrpvp.log`; me3 log (`%LOCALAPPDATA%garyttierneyme3dataogs
rpvp-sc.me3`) shows nrsc.dll under
early_natives plus "loaded native with me2 compatibility shim" and no "failed to load native mod". If it fails, retry with
`-DisableArxan`, then use step 3 (fallback).

### T-004b — Protect the vanilla save in the dev profile
Owner: SONNET/implementer | Status: review (awaiting HUMAN-solo; optional, low priority) | Depends on: T-004 | Phase: 0
Goal: the `dev` profile (no SC) must not write the vanilla `NR0000.sl2`.
Steps: use me3's `savefile` profile option (check the v0.11.0 docs/source for exact semantics) to point the dev profile at a separate save name.
Document it in README.
Acceptance: `me3 profile show` reflects it. **HUMAN-solo:** after a dev launch, `NR0000.sl2` keeps its mtime and the new save file appears.
Result (2026-10-09, Sonnet; reviewed by Opus): `savefile = "nrpvp-dev.sl2"` (me3 v0.11.0: a bare name in the vanilla save dir; seeded by copying NR0000.sl2 on first use;
panics rather than falling back if the redirect fails). `me3 profile show` confirms it. Baseline: NR0000.sl2 mtime 14:45:17.

### T-005 — Research: ER engine reference for team type, players, and sessions
Owner: SONNET/researcher | Status: **done** (2026-10-09) | Phase: 0
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
Result: `.claude/re/er-reference.md` (pinned to fromsoftware-rs 59fbd3b). Merged into TECHNICAL_NOTES.md §4. ER offsets: team_type +0x6C, main_player +0x1E508.
Use `from-singleton` for T-006; the eldenring crate can't run on NR; the hostility matrix is unmapped.

### T-006 — Singleton discovery in NR
Owner: SONNET/implementer → HUMAN | Status: **done** (2026-10-09) | Depends on: T-001, T-005 | Phase: 0
Goal: prove we can locate engine singletons (WorldChrMan and others) in NR 1.3.3 without hardcoded
addresses.
Steps:
1. Try `fromsoftware-shared`'s singleton finder against NR. If it doesn't apply, implement the same FD4
   singleton-scan technique documented in T-005, with a source link.
2. At startup, after the game finishes initialising (wait or poll; document the approach), log every
   discovered singleton name and address.
3. (Added by Opus after T-005.) **Read-only ER-offset probe**, to make T-007 cheaper: every 2 s, using
   guarded reads (VirtualQuery-checked; never write), try ER's path `WorldChrMan → +0x1E508 main_player →
   +0x6C team_type` (plus chr_type +0x68). Log only on change, clearly labelled `UNVERIFIED-ER-OFFSET`.
   It must never crash the game: any unreadable pointer is logged once and skipped.
Result (2026-10-09, Sonnet; reviewed by Opus): `crates/nrpvp/src/game/{mem,image,singletons,probe,mod}.rs`. Uses from-singleton 3.0.1's pattern iterators with our own driver that
scans **all** executable sections (the stock driver only scans the first `.text`), and also runs the stock `map()` for comparison. FD4 names are resolved by calling the game's
name-getter (chosen by majority vote), like stock does: this is the one crash risk. The probe logs `UNVERIFIED-ER-OFFSET`. 24 unit tests; clippy clean.
**HUMAN-solo PASSED 2026-10-09:** 235 singletons (identical to stock `map()`; all hits in the first `.text`; getter exe+0x2153380, 231/231 votes).
WorldChrMan slot exe+0x3C0F0A8 (live after ~28 s, re-created on area change); CSSessionManager exe+0x3C21EC8; CSNetMan exe+0x3C046A8;
NR-specific CSTeamSessionManager / CSTeamSessionDataMan. ER probe **failed**: +0x1E508 is null or garbage on NR. Solo Mission works under SC+me3.
Acceptance (**HUMAN-solo**): the log lists singletons including `WorldChrMan` (or the NR equivalent name)
with plausible addresses. Results go into TECHNICAL_NOTES.md §4 as `[VERIFIED: log]`.
Out of scope: reading fields inside singletons.

### T-007 — Map local player → team type in NR
Owner: SONNET/implementer → HUMAN → OPUS analyses | Status: **done via T-007a** (local player's own team value gets logged by T-008)
**Re-scoped 2026-10-09 (Opus):** automated RTTI/pointer-scan discovery in the DLL first (T-007a below); manual CE is the fallback only. | Depends on: T-006 | Phase: 0
Goal: verified pointer path from `WorldChrMan` to each `PlayerIns` and its `team_type` (and
`ChrTemporaryTeamTypeSlot` if reachable) on NR 1.3.3.
Steps: Opus prepares a Cheat Engine plan using the ER layout as a starting hint. The Human runs it solo
(vanilla-offline-style SC solo session), recording the team-type values for self, normal enemies, a field
boss, and a night boss if reachable. Save the CE table to `.claude/re/nrpvp.CT`.
Acceptance (**HUMAN-solo**): the path and observed values are written into TECHNICAL_NOTES.md §4 as
`[VERIFIED: CE]` with the game version.
Two-player check (**FS-1**): the partner's PlayerIns is reachable on the same path, and its team type is
recorded from both host and client views.

### T-007a — Automated character discovery (RTTI + pointer scan + memory dumps)
Owner: SONNET/implementer → HUMAN-solo → OPUS | Status: **done** (2026-10-09; run 2 after the hang fix: scans take 0.2–0.4 s; results in TECHNICAL_NOTES §4: team_type = ChrIns+0x74, local player = ChrSet @ WorldChrMan+0x9A40) | Depends on: T-006 | Phase: 0
Goal: find, without CE, (a) where WorldChrMan holds the local PlayerIns (and the player/enemy ChrSets), and (b) raw
PlayerIns/EnemyIns bytes so Opus can identify the `team_type` offset.
Steps:
1. Resolve vtables from MSVC RTTI in nightreign.exe: TypeDescriptor name → CompleteObjectLocator(s) → vtable (COL ptr at vtable[-1]).
   At least `PlayerIns`, `ChrIns`, and every `.?AV*Ins@CS@@` class whose ClassHierarchyDescriptor has ChrIns as a base. Log class → vtable RVA.
2. Guarded scan of the WorldChrMan instance (up to 0x40000 bytes, 8-aligned): log every offset holding a pointer to an object whose vtable
   is a ChrIns-derived class (direct members). Then one level deeper: offsets holding a pointer to an array whose 0x10- or 0x8-stride
   entries point to ChrIns-derived objects (report offset, stride, count found, class histogram). These are ChrSet candidates.
3. Hotkey F9 (GetAsyncKeyState polling on our thread; edge-triggered): re-run step 2 and hex-dump the first 0x400 bytes of every
   PlayerIns found and of up to 8 non-player ChrIns, each labelled with class name and address. Also run once automatically 10 s
   after WorldChrMan becomes live or changes.
4. Remove or disable the T-006 ER probe (it is proven wrong); keep the singleton map dump.
Acceptance: build/clippy/tests clean. **HUMAN-solo:** in a Solo Mission, press F9 (i) alone in a quiet spot and (ii) next to a group of
enemies. Opus then reads the log and records the PlayerIns location and the team_type offset candidates in TECHNICAL_NOTES.md §4.
Out of scope: any memory writes or hooks.

### T-008 — Debug controls: read and write team type
Owner: SONNET/implementer → HUMAN | Status: **done** (2026-10-09; write and restore verified in-game) | Depends on: T-007 | Phase: 0
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
**Spec update (Opus, 2026-10-09, after T-007a):** local player = entries of the inline ChrSet at WorldChrMan+0x9A40 (capacity +0x10,
entries +0x18, 0x10-byte entries, first qword PlayerIns* checked against the PlayerIns vtable exe+0x2C307F8 via RTTI). team_type = **ChrIns+0x74 (u8)**.
To identify "local" in multiplayer, log every entry's full 0x10 bytes (ER has chr_update_type Local=0/Remote=4 there). Also log +0x1C820's first PlayerIns.
Hotkeys: F4 cycles `hostile_team_type` through the config list `team_candidates = [6, 29, 3, 16, 13, 2, 1]` (log the new choice);
F5 writes it to the local player; F6 restores the value captured before the first write; F7 toggles persist (re-apply each 50 ms tick and
count/log how often the game reverted it); F8 logs a histogram of team_type over all discovered ChrIns plus the local player value. Every write logs
old → new. The write is a single guarded u8 store, only to a PlayerIns that passes the vtable check.

### T-012 — E-002: does team type drive hostility in NR? (solo)
Owner: HUMAN (OPUS designs and analyses) | Status: **done** (2026-10-09: team 6 = allied with enemies; team 29 = normal PvE that persists all expedition; grace interaction blocked under 29)
Goal: before involving a friend, prove solo that (a) our write mechanism works, (b) the engine uses the
player's team type for hostility in NR, and (c) the value survives game events, or find out what resets it.
Steps: run **TC-004** in TESTING.md.
Acceptance: TC-004 is recorded. Opus decides whether the value set and persistence approach for FS-1 is
sound, or whether a different mechanism (T-011 alternatives) is needed first.

### T-013 — In-game debug overlay (ImGui)
Owner: SONNET/implementer → HUMAN-solo | Status: **done** (2026-10-09; the panel and mouse work in-game with SC) | Depends on: T-008 | Phase: 0
Goal: replace blind F-key control with an on-screen panel showing live state, controls, and a log tail, so solo and FS tests are fast and unambiguous.
Requested by Leonardo after TC-004 run 1 (F4 collides with SC's pause; no on-screen feedback).
Steps: `hudhook` (DX12) overlay toggled by a config key (default Insert). Panel: WorldChrMan/local player status; local team_type live;
buttons per team candidate + Restore + Persist checkbox (with revert counter); Discovery (F9) and Histogram (F8) buttons; last ~200 log lines
(in-memory ring buffer tracing layer). Hotkeys stay as a fallback but default bindings avoid F4.
Acceptance: build, clippy, and tests clean. **HUMAN-solo:** the overlay toggles, buttons do what the log says, and the game is stable for 10 min.
Out of scope: anything that isn't a debug tool.

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
Owner: SONNET/researcher | Status: **done** (2026-10-09) | Phase: 0 (parallel, low priority)
Goal: be ready for H1 failing.
Steps: for NR specifically, find out whether SpEffectParam or AtkParam have team-changing or "hit both
teams" semantics (field docs from Paramdex). Find whether NR's EMEDF has a set-team-type instruction
(DarkScript3 or soulsmods sources). Find whether Smithbox, WitchyBND, and DarkScript3 currently support
NR 1.3.x.
Deliverables: `.claude/research/T-011-alt-hostility.md` (Opus merges the key facts into TECHNICAL_NOTES.md §4).
Out of scope: making a modded regulation.bin.
Result: `.claude/research/T-011-alt-hostility.md`. Merged into TECHNICAL_NOTES.md §4. Key points: SpEffect `change_team_type` and EMEVD
`SetCharacterTeamType` both exist in NR, but their effect on players is UNKNOWN. Smithbox 2.2.6, DarkScript3 3.6.3, and WitchyBND 3.0.1 support NR.
The team-type enum is ER-derived with conflicts. SC has a `YuiNexusRegulationManager`. Note: me3 0.13.0 is out and our pin is 0.11.0; don't
upgrade mid-Phase-0 unless needed.

### T-014 — Per-session log files
Owner: SONNET/implementer | Status: **done** (deployed 2026-10-09; confirmed on the next launch) | Phase: 0
Goal: never lose test evidence. Each game launch writes `dist/logs/nrpvp_<UTC timestamp>.log`, `dist/nrpvp.log` stays a copy of or link to
the latest one for quick tailing, and only the newest 30 session logs are kept. Logs worth keeping for the record are copied to
`.claude/experiments/logs/` (committed) by Opus when it reviews a test.

### T-015 — FS-1 tooling: all-players panel, remote writes, MARK notes
Owner: SONNET/implementer → HUMAN-solo | Status: review (code done 2026-10-09, 118 tests, 1 known flaky logging test; first in-game check in FS-0) | Depends on: T-013 | Phase: 0
Goal: make FS-1 runnable and self-documenting. Each client sees every player in the session and can change any player's *local copy*
of team_type, which is how we test whether hostility is decided on the attacker's side, the victim's side, or both.
Steps:
1. The overlay lists every valid PlayerIns in the ChrSet at WorldChrMan+0x9A40: index, address, LOCAL or REMOTE, team_type (+0x74), the entry
   state bytes (+8, +9) and, if cheaply available, the character name or Steam ID (from CSSessionManager; skip it if unverified). The local player is
   decided by a rule that must work with two or more players. Investigate the entry bytes and any ChrIns field that differs between local and
   remote (ER: ChrSetEntry update type Local=0/Remote=4; ChrIns network authority). Log the evidence and **do not guess**: if you can't
   tell them apart from verified data, show "UNKNOWN" and let the HUMAN pick the local one via a "this is me" button saved for the session.
2. Per-player buttons: set any candidate value, or restore, on that player's local copy (same gated write; PlayerIns vtable check).
   Persist works per player.
3. A **MARK** button plus a text box: writes `MARK: <text>` to the log with a running counter, so testers can annotate moments
   ("A hits B", "B died").
4. Log every write as `who=LOCAL|REMOTE[i]`, so both PCs' logs can be lined up by timestamp.
Acceptance: build, clippy, and tests clean. **HUMAN-solo:** the panel lists 1 player as LOCAL, the per-player buttons work, and MARK lines appear.
Remote rows can only be verified in FS-1.

### T-016 — Friend kit (packaging for the FS-1 partner)
Owner: SONNET/implementer | Status: review (done 2026-10-09: dry-run verified; nrpvp.toml is not shipped (the DLL generates it); repackage after each rebuild) | Phase: 0
Goal: the partner can install and launch our exact build with minimal steps and no repo or Rust toolchain.
Steps: `scripts/package-friend.ps1` builds `dist/friend-kit.zip` containing `nrpvp.dll`, the default `nrpvp.toml`, a me3 profile
template, `launch-friend.ps1`, and `README-FRIEND.md`. The launcher finds the Nightreign game folder (Steam `libraryfolders.vdf`, with a prompt as fallback),
checks that `SeamlessCoop\nrsc.dll` exists and prints its SHA256 for comparison with the host's, writes the profile with absolute paths,
and runs `me3 launch`. The README covers: install me3 (the same version as ours), install SC (the same version as ours, from Nexus), back up saves, never go online,
how to send logs back (`logs\` folder).
Acceptance: the zip builds; the launcher's path detection is unit-tested where possible (a PowerShell function tested on a fake vdf), and a
dry-run mode prints the profile and the command without launching.

### T-017 — FS-0: local dual-instance dry run of FS-1 (Nucleus Co-op)
Owner: HUMAN-solo (OPUS designs and analyses) | Status: run 1 PASS (reported) 2026-10-09; repeat with logs after T-018 | Depends on: T-015 | Phase: 0
Goal: run TC-001/TC-002/TC-003 between two real game clients on Leonardo's PC, so the friend session doesn't wait on an available partner.
Setup: Nucleus handler `D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN` (Goldberg ColdClientLoader → nrsc_launcher.exe; `coldloader.dll` loads
the DLLs in `dlllist.txt`). `scripts/deploy-nucleus.ps1` copies dist\nrpvp.dll and nrpvp.toml there and appends nrpvp.dll (`-Remove` undoes it). Each instance logs to
`D:\Programs\Nucleus\content\ELDEN RING NIGHTREIGN\Instance<N>\nrpvp.log` and `logs\`. **Collect logs before relaunching Nucleus** (it rebuilds the instance folders).
Caveats: emulated Steam (Goldberg LAN) instead of real Steam P2P, ~0 latency, shared input. FS-1 with the friend remains the final confirmation.

### T-018 — Logs survive Nucleus (FS-0 evidence)
Owner: SONNET/implementer | Status: todo | Phase: 0
Goal: Nucleus deletes `content\...\Instance<N>\` on close, which lost the FS-0 logs. Add config `log_dir` (absolute path; default = DLL dir).
When set, logs go to `<log_dir>\<instance tag>\` where the tag is derived from the DLL's parent folder name (e.g. `Instance0`). `deploy-nucleus.ps1`
sets `log_dir = "<repo>\dist\nucleus-logs"`. Logging is initialised before the config today: read just `log_dir` first, or move the init; keep
the fallback to the DLL dir on any error.
Also: investigate why the overlay draws no frames under Nucleus (hooks apply, but there's never a "context initialised" line). Report findings only; don't fix.
