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
Owner: HUMAN + friend (OPUS designs and analyses) | Status: **done** (2026-10-10: PASS, free-form session; see TESTING.md) | Depends on: FS-1 readiness gate | Phase: 0
Goal: answer H1 (TECHNICAL_NOTES.md §4). Can SC players damage each other when they have hostile team
types, and does it stay in sync?
Steps: run the **FS-1 runbook** (TESTING.md §6): TC-001 → TC-002 → TC-003.
Acceptance: all three are recorded, with logs from both PCs under `.claude/experiments/FS-1/`.

### T-010 — Phase 0 feasibility report and go/no-go
Owner: OPUS | Status: **done** (2026-10-10: GO; report in ROADMAP.md) | Depends on: T-002, T-003, T-009 | Phase: 0
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
Owner: SONNET/implementer | Status: **done** (worked on the friend's PC in FS-1; now ships nrpvp.toml) | Phase: 0
Goal: the partner can install and launch our exact build with minimal steps and no repo or Rust toolchain.
Steps: `scripts/package-friend.ps1` builds `dist/friend-kit.zip` containing `nrpvp.dll`, the default `nrpvp.toml`, a me3 profile
template, `launch-friend.ps1`, and `README-FRIEND.md`. The launcher finds the Nightreign game folder (Steam `libraryfolders.vdf`, with a prompt as fallback),
checks that `SeamlessCoop\nrsc.dll` exists and prints its SHA256 for comparison with the host's, writes the profile with absolute paths,
and runs `me3 launch`. The README covers: install me3 (the same version as ours), install SC (the same version as ours, from Nexus), back up saves, never go online,
how to send logs back (`logs\` folder).
Acceptance: the zip builds; the launcher's path detection is unit-tested where possible (a PowerShell function tested on a fake vdf), and a
dry-run mode prints the profile and the command without launching.

### T-017 — FS-0: local dual-instance dry run of FS-1 (Nucleus Co-op)
Owner: HUMAN-solo (OPUS designs and analyses) | Status: **done** (run 4 PASS 2026-10-10, two-way PvP with logs) | Depends on: T-015 | Phase: 0
Goal: run TC-001/TC-002/TC-003 between two real game clients on Leonardo's PC, so the friend session doesn't wait on an available partner.
Setup: Nucleus handler `D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN` (Goldberg ColdClientLoader → nrsc_launcher.exe; `coldloader.dll` loads
the DLLs in `dlllist.txt`). `scripts/deploy-nucleus.ps1` copies dist\nrpvp.dll and nrpvp.toml there and appends nrpvp.dll (`-Remove` undoes it). Each instance logs to
`D:\Programs\Nucleus\content\ELDEN RING NIGHTREIGN\Instance<N>\nrpvp.log` and `logs\`. **Collect logs before relaunching Nucleus** (it rebuilds the instance folders).
Caveats: emulated Steam (Goldberg LAN) instead of real Steam P2P, ~0 latency, shared input. FS-1 with the friend remains the final confirmation.

### T-018 — Logs survive Nucleus (FS-0 evidence)
Owner: SONNET/implementer | Status: **done** (2026-10-10: `log_dir` works; logs in dist
ucleus-logs; the overlay-under-Nucleus investigation was not done) | Phase: 0
Goal: Nucleus deletes `content\...\Instance<N>\` on close, which lost the FS-0 logs. Add config `log_dir` (absolute path; default = DLL dir).
When set, logs go to `<log_dir>\<instance tag>\` where the tag is derived from the DLL's parent folder name (e.g. `Instance0`). `deploy-nucleus.ps1`
sets `log_dir = "<repo>\dist\nucleus-logs"`. Logging is initialised before the config today: read just `log_dir` first, or move the init; keep
the fallback to the DLL dir on any error.
Also: investigate why the overlay draws no frames under Nucleus (hooks apply, but there's never a "context initialised" line). Report findings only; don't fix.

---

## Phase 1–2: automatic PvPvE rule for 2 players

### T-019 — Reproduce and fix the auto-apply hang
Owner: HUMAN-solo (2 min) → SONNET/implementer | Status: ready | Phase: 1-2
Goal: `auto_team_all = 29` must be safe. In FS-0 run 3 both Nucleus instances hung ~1 s after Roundtable load with it enabled (logs:
`.claude/experiments/FS-0/run3_hang_*`); nothing was written (team was 1). Not reproduced with it off. It is unknown whether auto-apply caused it.
Steps: 1. HUMAN: `scripts/deploy-nucleus.ps1` (default AutoTeamAll 29), launch Nucleus, load into Roundtable twice. Hang again → reproducible.
2. If reproducible: implementer reviews what differs when `auto_value` is Some (watch tick path, config load) and adds per-tick heartbeat logging
to locate the stall; fix; re-test. If not reproducible in 3 launches: record it as a one-off and keep auto-apply.
Acceptance: 3 Nucleus launches with auto-apply on reach an expedition and both clients show 29 for both players with no key presses.

### T-020 — Expedition rule v1: automatic, no debug keys
Owner: SONNET/implementer → HUMAN | Status: **done** (PvP mode proven in Nucleus and over real Steam; superseded in detail by T-026 "rivals only") | Depends on: T-019 | Phase: 1-2
Goal: with a single config switch (`pvpve = true`), every client applies team 29 to all players whenever an expedition is running and restores
the original values otherwise. Local player = ChrSet entry[0] (log a warning if the evidence contradicts it). Includes a workaround for sites
of grace: investigate first (read-only logging of what changes when the player stands at a grace on team 2), then propose; don't guess.
Acceptance: HUMAN-solo expedition via me3: PvE normal, value applied and restored automatically; grace behaviour documented. Two-client: FS-0.

### T-021 — Team values research (allies not hostile)
Owner: HUMAN + OPUS (Nucleus) | Status: superseded by T-026 (teammates stay on the normal value 2, only rivals get 29; `team_size = 2` passed with 3 Nucleus instances in FS-0 run 21, 2026-10-12) | Depends on: T-020 | Phase: 2
Goal: find team_type values A and B such that A is hostile to B and to enemies, but A is not hostile to A (needed for 2v2 and up). Test candidates
3, 13, 16, 9 and others from the enum with the overlay/hotkeys in Nucleus (needs 2 clients; ideally 3 for an ally check, otherwise FS-1).

---

## Scope v2 (Battle Royale): see PROJECT_PLAN.md. Milestones M1–M8, spikes S1–S5.

### M1 — Stable free-for-all
- **T-019 / T-020** (above): HUMAN checks of `pvpve = true` in Nucleus and with a friend; watch for the Roundtable freeze.
- **T-022 — Sites of grace on team 29.** Owner: SONNET/implementer → HUMAN | Status: closed, not needed (T-026: the local player stays on 2, so graces work; confirmed in FS-0 run 5) | Depends on: T-020 verified.
  Find out, read-only first, what the grace interaction checks (log the player's state at a grace on team 2 vs 29). Then the smallest fix,
  e.g. restoring team 2 while the player stands at a grace and no rival is near. Acceptance: level-up at a grace works with `pvpve = true`.

### M2 — Teams
- **T-021** (above): team-value research, now with 3–4 Nucleus instances if the PC can run them.
- **T-023 — Team assignment v1.** Owner: SONNET/implementer | Status: **done** via T-026 (teams from player ids; only `team_size = 1` has run in the game) | Depends on: T-021.
  Config `team_size`; team = function of the per-player id (`ChrIns+0x78`) so every client computes the same rosters with no messages.
  Pure-logic module with unit tests; the overlay shows rosters. Acceptance: Nucleus 3+ instances show identical rosters in every log.

### Spikes (SONNET/researcher, read-only, parallel; deliverables in `.claude/research/`)
- **S1 — Player cap.** Status: **done** (2026-10-10). The cap is SC-side and "soft" (author: could go higher, bugs above 5); slots 4–6 needed repeated fixes; whether the engine caps at 6 is UNKNOWN. Decision: plan for 6; ask the author later. What enforces 6 (SC, engine slots, both); public statements; any route beyond 6. → `S1-player-cap.md`
- **S2 — Inventory and item drops.** Status: **done** (2026-10-10). NR has a native Discard-to-ground and dropped items are shared (one pickup removes it for everyone); carry limit ~11 items; `MapItemMan` and `CSGaitem` are findable by name; inventory layout and the Discard function are UNKNOWN on NR. Best route: call the native discard per item on a PvP death. First check (no code): can a team-29 player pick up items at all? Nightreign's own drop-for-teammate mechanic; inventory structures (ER layout in
  fromsoftware-rs as a hint); whether dropped items sync to all players. → `S2-inventory-drops.md`
- **S3 — Day/night, tide, and bosses.** Status: **done** (2026-10-10). The timeline is an event-script flag chain (7502/7507 = night boss defeated → next day; 7000 = win; 7001 = game over when no player is alive at night); the circle is param-driven; modded regulation + event files run next to SC in other mods (6 Player Fixes); a DLL clock mod exists (Storm Timer). **Flag 7001 must be neutralised before PvP night tests.** Event scripts and flags behind the tide, night bosses, and day change (DarkScript3 /
  soulsmods data for NR); whether a night can end without its boss. → `S3-match-flow.md`
- **S4 — Mod-to-mod channel.** Status: **done** (2026-10-10). Recommended: our own Steam Networking Messages on a private channel, addressed by Steam IDs from CSSessionManager (NR offsets UNKNOWN); use raw flat API calls, never `steamworks::Client::init()`; never close sessions. Game-native sync can't carry our data. Experiment E-S4a: hello exchange between two Nucleus instances. Does SC sync event flags; can a DLL use Steam networking messages alongside SC; how other
  mods that depend on SC share state. → `S4-mod-channel.md`
- **S5 — Death and respawn.** Status: **done** (2026-10-10). Revive = allies attack the downed player (gauge 40/90/240, params in GAME_SYSTEM_COMMON_PARAM_ST); the level loss is not param-driven (needs a DLL write to PlayerGameData, location UNKNOWN); protection hints: `invincible_timer_for_net_pc` + ER `disabled_hit`; vanilla boss fights already have "no respawn" (reuse for sudden death); **risk: vanilla ends the run when all players are downed at once**. Engine flow downed → dead → respawn; source of the level loss and of the respawn point. → `S5-death-respawn.md`

- **Rules core (pure logic).** Status: **done** (2026-10-10; `crates/nrpvp/src/rules/{teams,matchstate}.rs`, 37 tests; not wired to the game yet).
  Assumptions to confirm with Leonardo before M6: (1) tie = draw only when the last players die in the same instant, otherwise the first team
  left standing wins; (2) respawn protection 5 s, ends on attacking; (3) a player dead and waiting to respawn when the arena closes is eliminated;
  (4) a downed player still counts as alive; (5) leavers drop nothing, and a team whose members all left or were eliminated is out; (6) rosters are
  frozen at match start (no late joiners).

### T-026 — Relative teams scheme ("rivals only") + roster wiring
Owner: SONNET/implementer → HUMAN (Nucleus) | Status: **done** (2026-10-11: FS-0 run 5 PASS for free-for-all; open defect: rivals can revive each other → T-031)
Goal: each client leaves itself and its allies at the vanilla value and writes the hostile value only to rivals. Config: `pvpve_scheme =
"rivals"` (new default when `pvpve = true`) or `"all"` (today's behaviour), `team_size` (default 1 = free-for-all). Rosters from
`rules::teams` using the per-player id at ChrIns+0x78. The overlay shows each player's team and ALLY/RIVAL. Acceptance (HUMAN, Nucleus 2
instances, no keys): both can lock on to and damage each other; graces, pickups, and chests work; with `team_size = 2` the two are allies and
play vanilla co-op (hits do nothing, revive works).

### T-027 — Read-only probes: timeline flags and player data
Owner: SONNET/implementer → HUMAN-solo | Status: parked (built; not needed for the downed rule. Keep `probes = false` during rule tests: the probes cost ~0.4 s per main-loop pass, FS-0 run 16) | Depends on: T-026 landed (same files) | Phase: M3–M5 groundwork
Goal: (a) log the timeline event flags from S3 (7000, 7001, 7500–7523) and the world clock during a solo expedition; (b) locate the player's
level and rune count (candidate: PlayerIns+0x588 → PlayerGameData, ER layout +8) and log them so the HUMAN can compare with the HUD.
Read-only, guarded reads, every candidate labelled UNVERIFIED.

### S6 — Team spawn points (spike) + T-028 position probe
- **S6.** Owner: SONNET/researcher | Status: **done** (2026-10-10). The game has 20 data-driven start points (`SORTIE_STARTING_POINT_ST`: bird route + spawn point entity + grid). ER hint for position: ChrIns+0x190 → +0x68 → +0x70 (x,y,z), plus a block id on PlayerIns; ER tools teleport by writing the physics position and block id. Decision (Opus): T-028 logs position, ground flags, animation id, and the sortie rows; first route = each client moves its own player after landing; second = give each team a different vanilla start row | Feeds: F3 (moved up from M7 at Leonardo's request, 2026-10-10).
  How the expedition start position is chosen (hawk drop-in, start points per map pattern, event scripts, params), where a character's position
  lives (ER physics module as a hint), how ER/NR mods teleport players and whether that syncs in co-op, and what lists of safe points the
  engine has (`CSRespawnPointMan`, graces, map points). → `.claude/research/S6-team-spawns.md`
- **T-028 — Position probe and self-teleport test.** Owner: SONNET/implementer → HUMAN | Status: todo (the snap-back test is built, T-032 part c; HUMAN run pending: STATUS.md Task D) | Depends on: T-027, S6.
  Read-only first: log the local player's position candidates and the respawn-point list. Then a debug hotkey that moves the **local** player to a
  logged point. Acceptance (HUMAN + friend): the move is clean on both screens. Then the rule: on expedition start each team moves to its own point.

### T-029 — Drag-and-drop install (launcher exe + release zip)
Owner: OPUS (Sonnet agents were rate-limited) → HUMAN | Status: **done** solo (works on Leonardo's PC; second PC pending: STATUS.md Task B)
`crates/nrpvp-launcher` → `nrpvp_launcher.exe` (no console; message boxes for errors): must sit in the game folder; checks nightreign.exe,
SeamlessCoop\nrsc.dll, NightreignPvP\nrpvp.dll, NightreignPvP\me3\me3.exe; writes `NightreignPvP\nrpvp.me3.toml`; runs the **bundled** me3 0.11.0
with `launch -g nightreign -e <Game>\nightreign.exe -p <profile>`; me3 output goes to `NightreignPvP\launcher.log`. `scripts/package-release.ps1`
builds `dist\NightreignPvP.zip` (launcher, mod folder, me3 + its licences, `HOW TO INSTALL.txt`). Logs land in `<Game>\NightreignPvP\logs`.
Acceptance (HUMAN): install from the zip on Leonardo's PC following only the txt guide; the game starts with SC and the panel; then a friend does the same.
Open: the exe is unsigned (SmartScreen prompt, possible antivirus flag); `-e` with a bundled me3 is untested in a real launch.
Interrupted by the rate limit, to restart later: **T-027** (probes; partial `game/probe2.rs` exists, compiles, state unknown) and **S6** (spawn research, nothing delivered).

### T-030 — Player build: PvP always on, no toggles (dev mode for debugging)
Owner: SONNET/implementer → HUMAN | Status: **done** solo (label, no keys; second PC pending: STATUS.md Task B) | Requested by Leonardo 2026-10-10: players must not be able to turn PvP off.
Design: launching with `nrpvp_launcher.exe` **is** the Battle Royale mode. Config `dev_mode` (default false). With dev_mode off: no hotkeys at all,
no interactive panel, PvPvE forced on with the rivals scheme (a `pvpve = false` in the file is ignored with a log line), no pause/resume, no
probes, no automatic discovery dumps; a small read-only on-screen badge ("Battle Royale · team N · K rivals"). With dev_mode on: everything as
today. The player zip ships without dev_mode; `dist\nrpvp.toml`, the Nucleus deploy and the friend kit set `dev_mode = true`.
Note: turning PvP off locally gives no advantage (a client that doesn't mark rivals can't attack them but can still be attacked), so this is UX, not anti-cheat.

### S7 — "Battle Royale" entry in the expedition menu (spike)
Owner: SONNET/researcher | Status: **done** (2026-10-10). The table is an ESD talk script (600001000); SC only relabels the 1/2/3-player options; other mods add entries to that talk list and ship menu text next to SC. Decision (Opus): route A now (launcher = mode + on-screen label, T-030); route B later (a "Battle Royale" entry in the table's talk list that sets a flag and opens the stock menu; needs the mod channel so guests learn the host's choice) | Feeds: F15.
How the Roundtable expedition-type menu (Solo Mission / Up to 6 players under SC) is built: menu and text files (msg FMGs, menu layout),
how SC renames its entries (locale json keys `MODMESSAGE_YUINEXUSBEGIN*`), whether mods have added entries to FromSoftware menus (ER/NR
examples), and how every client would learn the chosen mode. Rank routes from cheapest (relabel an entry under our launcher) to a true third entry.
→ `.claude/research/S7-mode-menu.md`

### T-031 — Downed-state and position discovery (dev probes) + rival-revive experiment switch
Owner: SONNET/implementer → HUMAN (Nucleus) | Status: closed (downed state found in T-032 run 7; the experiment switch is inert; position part moved to T-032 c) | Feeds: F10 (ally-only revive), F3 (spawns)
(a) Downed detection: log, at 100 ms while any value changes, every player's ChrSet entry bytes +8/+9, team_type, and HP candidates, so one
session with a few downs and revives shows which field marks "downed". (b) Experiment switch `downed_rival_team` (dev only, default off): while a
rival's entry +9 == 2, write this value to that rival's copy and put 29 back afterwards, to find a value that stops rival revives.
(c) Position discovery by movement: the ER chain failed; scan PlayerIns (first 0x1000 bytes) and objects one pointer away for f32 triples that
change smoothly while the player walks; log ranked candidates. Read-only except (b).

### T-032 — Find the downed marker (MARK dumps) and the writable position (snap-back test)
Owner: SONNET/implementer → HUMAN (Nucleus) | Status: part (a) **done** (run 7: downed marker, attacker handle, HP found); part (c) snap-back still pending | Feeds: F10, F3. Dev mode only.
(a) F10 MARK additionally dumps, for every ChrSet player, PlayerIns+0..0x1000 and the first 0x200 bytes of its one-level pointees (compact hex,
with pointee vtable RVA and RTTI class name). Protocol: MARK twice with the rival standing, twice with the rival downed; Opus diffs.
(b) RTTI class names next to every pointee vtable in the POSSCAN output.
(c) Snap-back test: a list of position candidates (from FS-0 run 6); F9 selects the next candidate, F7 saves the local player's current values
of all candidates, F6 writes the saved value into the selected candidate only. First new write kind since team_type: dev only, LOCAL player
only, 12 bytes, pointee re-validated by vtable before each write, every write logged.

### T-033 — Finishing blow: rival completes the revive meter → death (PROJECT_PLAN §11)
Owner: SONNET/implementer → HUMAN (Nucleus) | Status: tracker **done** (run 8 PASS); finisher via PlayerGameData HP has no effect (mirror) → T-034 | Feeds: F10
Verified inputs (FS-0 run 7): downed = PlayerIns+0x710; downs = +0x730; last attacker handle = +0x1A0 (victim's client); handle = ChrIns+0x08;
PlayerGameData = [PlayerIns+0x5A0], HP at +0x10. Unknown: how to turn a revive into a death.
(1) State tracker (both modes later; dev first): per player, log down / up / respawn transitions with the last attacker classified as ALLY, RIVAL,
or OTHER. (2) Downed-timer scan (dev, read-only): while the LOCAL player is downed, rank f32/u32 values in PlayerIns and its pointees that count
down in real time. (3) Finisher experiment (dev switch): when the LOCAL player is revived and the last attacker is a rival, write HP = 0 and log
what the game does next (re-downed with a higher down count, or dead).

### T-034 — Finishing blow, round 2: authoritative HP and the bleed-out timer
Owner: SONNET/implementer → HUMAN (Nucleus) | Status: **done** (run 9: authoritative HP found; HP 0 = re-down, not death; wide scan too slow → T-035) | Feeds: F10. Dev mode only.
(1) Finisher modes (F5 cycles): `hp0-all` writes u32 0 to all three HP locations seen in run 7 (PlayerGameData+0x10, PlayerIns+0xBA0,
[PlayerIns+0xB78]+0x70), each validated; log readbacks and the state at +100 ms … +5 s. Also `hp0-downed`: the same write while the LOCAL
player is still downed and a rival hits (to see whether a downed player can be killed that way).
(2) Wide timer scan while LOCAL is downed: PlayerIns two levels deep and the first 0x2000 bytes of every live singleton; rank values counting
down (or up) at a steady real-time rate that started at the DOWN transition, and values that jump when a hit lands (the revive meter).

### T-035 — Rival revive rule v1 in the player build + fast bleed-out timer scan
Owner: SONNET/implementer → HUMAN | Status: **done** (rule v1 fired correctly in FS-0 runs 15, 18, 19; it is now the fallback behind T-039) | Feeds: F10
(1) **Rule v1 (player and dev mode, always on):** on the LOCAL player's revive (downed 1→0 with hp > 0) whose last attacker is a RIVAL, write
u32 0 to the authoritative HP `[PlayerIns+0xB78]+0x70` once, so the player is downed again instead of revived; allies revive as in vanilla.
Verified offsets only (FS-0 runs 7–9). The badge shows nothing new. Remove the hp0/hp0-all/hp0-downed experiment modes.
(2) **Fast timer scan (dev, read-only):** the wide scan took ~800 ms per pass. Rewrite the sampler to copy each region with one guarded bulk
read, drop the all-singletons sweep to a short list plus PlayerIns two levels deep, and keep every pass under 50 ms; run it off the main loop.
Goal: locate the bleed-out clock (≈ 15 s) and the revive meter, so a rival-completed meter can end the clock at once (instant death).

### T-036 — Free-for-all: HP 0 = death (no downed state) via the in-memory near-death params
Owner: SONNET/implementer → HUMAN | Status: dropped (decision 2026-10-12: instant death on 0 health rejected, auto-revive items would defeat it; replaced by T-039) | Feeds: F9/F10. Requested by Leonardo 2026-10-12.
(1) Param access (read-only first): locate `GAME_SYSTEM_COMMON_PARAM_ST` in memory through the param repository singleton (ER layout in
fromsoftware-rs as a hint; field offsets from the NR param struct); validate by `required_revive_progress_level1/2/3` == 40/90/240; log every
near-death/revive field. (2) Rule: when `team_size == 1`, write 0 to the near-death duration fields on this client (both modes), re-checking
periodically whether something restores them; when `team_size >= 2`, leave them alone. Dev key to toggle for A/B testing.
Acceptance (HUMAN, Nucleus or friend): in free-for-all a player at 0 HP dies and respawns immediately on both screens; no revive is possible.

### T-037 — nrscan: external memory scanner for the bleed-out clock and the revive bar
Owner: SONNET/implementer → OPUS runs it during a HUMAN Nucleus session | Status: **done** (used live in FS-0 runs 10–14) (built 2026-10-12: dist
rscan.exe; self-tests pass; live run pending) | Feeds: F10 (PROJECT_PLAN §11)
Why: three in-mod scans failed (narrow regions, slow passes) and a manual Cheat Engine search is too slow for a 15 s bleed-out (2.5 billion
candidates). Design: a separate CLI (`crates/nrscan`, read-only, ReadProcessMemory from outside the game; no mod change). It tails the target
instance's nrpvp log for `T-033 STATE who=LOCAL … DOWN` / `UP`, then scans committed private read-write memory for f32 in a plausible range and
keeps the values whose change matches elapsed time; survivors are tracked until the player leaves the downed state; a second mode tracks
values that step when hits land. Finally it explains each surviving address relative to the PlayerIns and singleton addresses in the log
(containing region, pointers to it, offsets).

### T-038 — nrscan round 2: write test, object identification, route search, fast bar scan
Owner: SONNET/implementer → OPUS runs it with the HUMAN | Status: **done** (used live in FS-0 run 14) (built 2026-10-12: class names, route search, clock write test, fast bar scan, peek/poke; 26 tests; live runs pending) | Feeds: F10 (PROJECT_PLAN §11)
Approach agreed with Leonardo 2026-10-12: finish the investigation with the external scanner (candidates **and** confirmations) before any
further mod changes. Questions: (1) is the count-up f32 from run 10 the bleed-out clock (write test); (2) where is the revive bar; (3) is the
clock writable both ways; (4) can the bar be restored after a hit; (5) which object holds each value (RTTI class) and what stable route
leads to it from PlayerIns / WorldChrMan.

### T-039 — Downed rule (Leonardo's design): rival hits shorten the bleed-out and never move the revive bar
Owner: SONNET/implementer → HUMAN | Status: **done** for free-for-all (FS-0 runs 15–20, 2026-10-12, TESTING.md §3: rival hits never keep gauge progress on either screen, 3 s off the clock per hit, death after 4–5 hits; 282 tests). Follow-ups in the Result below | Feeds: F10. Inputs verified in FS-0 run 14 (TECHNICAL_NOTES §4).
On the LOCAL player's own client, while `PlayerIns+0x710 == 1`: when the bar (`+0x714`) drops and the last attacker (`+0x1A0`) is a RIVAL →
write the bar back to its value before the hit, subtract `rival_hit_seconds` (default 3.0) from the clock (`+0x718`, floor 0.05), and set the
pause timer (`+0x71C`) to 0. Hits by ALLY/SELF/OTHER: untouched. Replaces rule v1 (re-down on a rival revive), which stays only as a safety net
if a rival's hit ever completes the bar before the mod reacts. Both modes; no keys; remove nothing else.

**Result (2026-10-12, Opus, FS-0 runs 15–20):** done for free-for-all. What it took beyond the card:
- Main-loop reads moved from `VirtualQuery` to `ReadProcessMemory` (`mem::read`, `RegionCache::direct`): a pass went from 2–3 s to 1–5 ms.
- `deploy-nucleus.ps1` writes `probes = false` (the probes still cost ~0.4 s per pass).
- `Core::remote_step`: each client also restores its copy of a downed rival that has no teammate, so the attacker's gauge is right.
- `Core::getup_step` (early re-down during the get-up) is built but **off** (`early_redown = false`): never exercised in the game.

Open follow-ups (no card yet; write one when the phase needs it):
1. **Gauge flicker** (wanted by Leonardo, low priority): a rival hit shows on the gauge for up to one 50 ms pass on both screens. See
   FUTURE_CONSIDERATIONS.md "Known side effects to fix later".
2. **One hit that removes the whole bar** (first down, bar 40; runs 18 and 19): the player gets up and rule v1 downs them again 3.7 s
   later. Accepted by Leonardo 2026-10-12 as long as the player ends up downed again. `early_redown` would close it; untested.
3. **Teams of 2+:** the attacker-side restore cannot tell a teammate's hit from a rival's on a remote copy (`+0x1A0` is not valid there).
   Still open after STATUS.md Task C (FS-0 run 21 passed the rule itself with teams of two; the lone rival's gauge was not reported).

## M8 — Public matchmaking (design: `research/S9-matchmaking-decision.md`)

Decision (Opus, 2026-10-12): Steam queue lobby + join the host's SC lobby; no server of ours; Solo first. Never EAC, never Bandai servers.
Cards run in order. MM-1 and MM-2 are developer-build only and change nothing in the player zip.

### MM-1 — Steam access probe: flat interfaces and SC's lobby id
Owner: SONNET/implementer → HUMAN-solo | Status: **done** (2026-10-12, solo run, log `experiments/MM-1/2026-10-12_mm_probe_solo.log`; findings in S9 §5a) | Settles: S9 U1, U2
Dev build only, behind `probes`-style switch `mm_probe = true`. New module `crates/nrpvp/src/steam/` (raw externs to the loaded
`steam_api64.dll` via `GetProcAddress`; never Init / ManualDispatch / Shutdown; no `steamworks` crate).
1. Log whether each accessor resolves and returns non-null: `SteamAPI_SteamMatchmaking_v009`, `SteamAPI_SteamUtils_v010` (or the version
   the DLL exports: list the exports and log the one found), `SteamAPI_SteamUser_v021`, `SteamAPI_SteamFriends_v017`. Log own Steam ID.
2. Register listeners with `SteamAPI_RegisterCallback` for `LobbyEnter_t`, `LobbyChatUpdate_t`, `GameLobbyJoinRequested_t`,
   `LobbyDataUpdate_t` (hand-built `CCallbackBase`; listen only, never consume or answer). Log every event with lobby id.
3. For each lobby id seen: log owner, member count, member limit, and all lobby data keys/values (`GetLobbyDataCount` / `ByIndex`).
Acceptance (HUMAN-solo, `scripts\launch.ps1`): launch, reach the Roundtable, open Matchmaking Settings, open Invite Members, close it,
start and finish/quit a solo expedition. Send the log. Game and SC must behave normally throughout.
Stop and report if any accessor is null or registering a listener breaks SC (no invites, no lobby).
Result (implementer): config `mm_probe` (dev only, forced off in player mode), `crates/nrpvp/src/steam/{mod,callback,probe}.rs`, thread spawned from
`startup.rs` only when `dev_mode && mm_probe`; `dist\nrpvp.toml` has `mm_probe = true`. Build OK, 293 tests pass (was 282), clippy clean.
Ids/layouts (500+4/5/6/13, 300+33/37, pack 8, struct sizes 24/24/32/16/16/264) and flat signatures verified from SDK headers bundled by steamworks-rs.
UNKNOWN: MSVC slot order of the two `Run` overloads (both slots point at one function, so it does not matter). Log lines: `grep mm_probe nrpvp.log`.

### MM-2 — Queue lobby (Solo / Duo / Squad) with hotkeys and overlay
Owner: SONNET/implementer → HUMAN + friend (real Steam, 2 PCs) | Status: **Scenario 1 PASS (FS-MM1, 2026-10-14); Scenario 2 (Duo) awaiting FS-MM2** | Depends: MM-1 | Settles: S9 U3
Dev mode only, switch `mm_queue = true`. Reuse `crates/nrpvp/src/steam/`; add the write-side calls there, each documented.
1. Pure state machine in `crates/nrpvp/src/mm/` (unit-tested, Steam behind a trait): `Idle → Searching → InQueue → MatchReady`,
   plus `Leave`. A ticket is `(mode, party)`: Solo = 1 player; Duo/Squad = the local player's current SC party (see 4).
2. Steam side on its own worker thread (never the game loop): `RequestLobbyList` with string filters `nrpvp=1`, `mode`, `proto`,
   `game`, `sc`, distance filter worldwide; `JoinLobby` if one has room for the whole ticket, else `CreateLobby(Invisible, 6)` +
   `SetLobbyData`. Call results polled with `ISteamUtils::IsAPICallCompleted` / `GetAPICallResult` (no manual dispatch).
   Duplicate-lobby rule: the owner of the higher lobby id leaves and joins the lower. Heartbeat re-search every few seconds.
3. `MatchReady` when the lobby holds `mm_match_players` players (config, default 2 for tests; 6 later) made of complete tickets.
   The owner then sets `SetLobbyJoinable(false)` and writes `state=go`, `host=<owner steam id>`, `roster=<steam id:party,...>`.
   Every member logs the same roster line. Nothing joins SC yet (that is MM-3).
4. Party for Duo/Squad: members of the SC lobby the local player is in (lobby with `nrsc_lobby_type` data seen by the MM-1 listener).
   The party leader (that lobby's owner) queues; it writes `nrpvp_q=<queue lobby id>` with `SetLobbyMemberData` on the SC lobby and
   the other members' mods read it and join the same queue lobby with the same `party` value. Member data is per member and is
   not SC's data; never call `SetLobbyData` on SC's lobby.
5. Mode sets the team rule at runtime: Solo → team size 1, Duo → 2, Squad → 3, overriding `team_size` from the file for that
   match only (find where `team_size` reaches `rules::teams::assign_teams` and add a runtime override).
6. Controls: hotkeys (Nucleus has no overlay) and overlay buttons: queue Solo / Duo / Squad, leave queue. Overlay line:
   "Queue: solo 1/2 searching" → "MATCH READY host=<name or id>".
Safety guard (S9 §2): refuse to queue unless `nrsc.dll` is loaded and no EasyAntiCheat module is loaded in the process; log why.
Acceptance (REAL Steam, Leonardo + friend, 2 PCs, `scripts\package-release.ps1 -MatchmakingTest` zip, `mm_match_players = 2`; Nucleus dropped):
Scenario 1: both press Queue Solo (overlay button; F1 backup) within ~10 s → both logs show the same queue lobby id, one owner/host, the same
`roster=` and `MATCH READY`; leave (Leave queue / F5) + re-queue works; both pressing at the same moment converges to one lobby (log shows
`duplicate lobby ... dissolving`); an SC co-op session between the two still works afterwards. Scenario 2: Leonardo invites the friend the
normal Seamless Co-op way, the leader presses Queue Duo → the friend's mod follows via `nrpvp_q` (log `following party leader`), both in one
queue lobby with the same party, `team size override = 2`. If a Steam call fails (lobby search, call-result polling, CreateLobby with SC's
lobbies already open), report exactly which `mm_queue:` line says so instead of working around it.
Result (implementer, 2026-10-12): `crates/nrpvp/src/mm/{mod,machine,proto,guard,worker}.rs`, `steam/queue.rs` (write side behind `QueueLobby`/`JoinTarget`/`ScLobby`
types), config `mm_queue` / `mm_match_players` / `hotkey_mm_*` (F1 Solo, F2 Duo, F3 Squad, F5 Leave; bound only when `mm_queue`), overlay block, `Command::MmQueue/MmLeave`,
team-size override (`mm::effective_team_size`, read by `TeamCtl::team_size_now`), `package-release.ps1 -MatchmakingTest` -> `dist\NightreignPvP-mmtest.zip`.
Build OK, 324 tests pass in `nrpvp` (was 293), clippy clean. All 18 Steam exports present in the game's `steam_api64.dll`. UNKNOWN: whether the game's
callback pump consumes call results before our poll (log says so if a handle is "unknown to Steam"); whether Steam allows our Invisible lobby next to SC's two lobbies.

### MM-3 — The gate: enter the host's SC lobby (friend session build)
Owner: SONNET/implementer → HUMAN + friend | Status: **method (b) PASS (FS-MM1, 2026-10-14)**; fixes in MM-3a | Depends: MM-2 | Settles: S9 U4, U5, §5a unknowns
Facts from MM-1 (S9 §5a): SC's lobby A is locked and is created by the invite flow; a second lobby B appears at expedition start.
Build (dev mode, `mm_join = true`), on top of the MM-2 `MatchReady` state:
1. Host side: if an SC lobby (data `nrsc_lobby_type`) is known → write its id into the queue lobby (`sc_lobby=`). If none → overlay
   says so ("open Invite Members once") and keeps checking.
2. Join methods, each a separate dev button and each logged with every lobby event that follows, on both PCs:
   (b) host: `InviteUserToLobby(sc_lobby, joiner)`; joiner accepts in the Steam overlay.
   (a) joiner: open `steam://joinlobby/2622380/<sc_lobby>/<host>` (only after checking `ISteamUtils::GetAppID() == 2622380`
       and that the Steam overlay is enabled for this process, so Steam cannot start a second, vanilla copy of the game).
   (b+a) host invites, joiner's mod opens the link itself (no click).
   (d) host: `SetLobbyType` to unlock lobby A, then (a). Changes a Steam setting on SC's lobby at runtime; logged, dev only.
   (c) joiner: `JoinLobby(sc_lobby)` from our DLL. Last, least likely.
3. Log on both sides, always: the full MM-1 event stream, so a **normal manual invite** (baseline, run first) shows which callback
   the joiner receives and which lobby (A or B) each player ends up in.
4. `scripts\package-release.ps1 -Dev` (or a sibling script): a friend zip with dev mode, `mm_probe`, `mm_queue`, `mm_join` on.
FS runbook → TESTING.md §6. Scenario 1: both queue Solo, same match, rivals. Scenario 2: Leonardo invites the friend the normal
way, leader queues Duo with `mm_match_players = 2`, both land as teammates. Then repeat the join methods after unfriending.
Result (implementer, 2026-10-12): built, awaiting friend run (FS-MM1, TESTING.md §6). Config `mm_join` (default false, dev + `mm_queue` only, forced off in the player build)
and `mm_join_auto` = `invite` (default) | `invite+link` | `link` | `none`. New: `mm/join.rs` (URL, link guard, auto mode, phase), `mm/machine/gate.rs` (the gate on `MatchReady`),
`steam/queue.rs` (`InviteUserToLobby`, `SetLobbyType`, `GetAppID`, `IsOverlayEnabled`, `PublishedScLobby`, `ScType`, `ShellExecuteW` link), `LobbyInvite_t` listener and per-lobby
membership lines (`mm_join: lobby ...`) in `steam/probe.rs`, overlay row of buttons (a) (b) (c) (d) + relock. Friend zip `dist\NightreignPvP-mmtest.zip` and `dist\nrpvp.toml` set `mm_join = true`.
Build OK, 351 tests pass (was 324), clippy clean. With `mm_join = false` a ready match behaves as in MM-2 (tested: no writes at all). Match end: Leave, or team_type 1 after having seen 2 for 5 s.
UNKNOWN (the session answers them): which lobby (A or B) joiners end up in; whether Steam accepts invite/link/JoinLobby/SetLobbyType on SC's locked lobby; what `GameLobbyJoinRequested_t` /
`LobbyInvite_t` an invited player's game receives; whether SC's lobby A exists for a host that has not opened Invite Members.

FS-MM1 (2026-10-14, step 2 only): MM-2 Scenario 1 **PASS**; MM-3 method (b) **PASS** (host opened Invite Members, joiner accepted);
rivals in the expedition **PASS**. Duo (Scenario 2), baseline and the method buttons not run yet → FS-MM2. Findings: S9 §5c.

### MM-3a — Fixes from FS-MM1 and the invite+link default
Owner: SONNET/implementer → HUMAN + friend (FS-MM2) | Status: built, awaiting FS-MM2 | Depends: MM-3 | Evidence: S9 §5c, logs `experiments/FS-MM1/`
Only `crates/nrpvp/src/mm/`, `steam/probe.rs`, the startup warning in `game/` (item 5), `scripts/`, `scripts/release/`. The T-028 position work
lives on branch `t-028-snapback`; on `main` only item 6 touches the (older, committed) snap-back keys.
1. **Match end after the expedition.** Today the match ends only on Leave or 5 s of team_type 1 after an expedition; in FS-MM1 both
   PCs stayed on `IN EXPEDITION - match active` with `wcm=0x0` and lobby A gone. Add to `gate_step`: once `saw_expedition`, if the
   phase is not `Expedition` AND the player is in no SC lobby (`env.sc` is None, or not the published/target lobby) continuously for
   15 s → `end_match(.., "expedition over: Seamless Co-op lobby gone")`. Keep the Roundtable rule. Unit tests: ends after 15 s; does
   not end while `Expedition` even if the SC lobby read flickers; does not end before any expedition.
2. **No false trust warning.** Step 2 (joiner reads `sc_lobby`): when a verified `published` value already exists, an `Err` from
   `PublishedScLobby::from_queue` (owner moved after the host left) keeps the old value and logs nothing above debug. Unit test with
   the owner moved after a first good read.
3. **Joiner status with links on.** With `mm_join_auto` = `invite+link`/`link`, after the automatic link the joiner's line reads
   `joining the host's lobby... (if a Steam invite pops up, accept it)` instead of the accept-the-invite text. Test.
4. **Stop logging lobbies we left.** `steam/probe.rs` keeps printing every lobby ever seen every 10 s (`members=[0]` / `[0, 0]`).
   Drop a lobby from the periodic `mm_join: lobby ...` lines once our own id is no longer a member.
5. **Load-screen warning.** `T-006: timeout after 180 s waiting for WorldChrMan` fires during later loads once WorldChrMan had
   been found before. After the first success, log that case at info, worded "WorldChrMan null for N s (loading)".
6. **mmtest package.** `scripts\package-release.ps1 -MatchmakingTest` writes `mm_join_auto = "invite+link"`, and leaves the T-028
   position test keys (F6/F7/F9) unbound in that zip's `nrpvp.toml` (find the switch in `config.rs`; if there is none, add
   `pos_test = false` default and bind those keys only when true). Update `scripts\release\MATCHMAKING TEST.txt`: the joiner no longer
   accepts an invite unless one pops up; the "(a) link" warning becomes "do not press any Join methods button".
Acceptance (implementer): `cargo test` and `cargo clippy` clean; the new zip exists and its `nrpvp.toml` shows the two settings.
Acceptance (HUMAN + friend): FS-MM2 in TESTING.md §6.
Result (implementer, 2026-10-14): items 1-6 built. Files: `mm/machine/gate.rs` (15 s "SC lobby gone" end after an expedition, kept published sc_lobby on a later Err, link-on joiner text; +5 tests, 1 updated), `steam/probe.rs` (LobbyWatch takes own id, periodic lines only while a member; +1 test), `game/mod.rs` (WorldChrMan null after a first success logs info "WorldChrMan null for N s (loading)" after 10 s; the T-006 warn only before the first success), `scripts/package-release.ps1` (mmtest `mm_join_auto = "invite+link"`; F6/F7/F9 were already unbound there via `hotkey_pos_* = ""`, so no config.rs change), `scripts/release/MATCHMAKING TEST.txt`. Tests 351 -> 357 pass, clippy clean, release build OK. Zip `dist/NightreignPvP-mmtest.zip` 2026-10-14 00:14 local (03:14 UTC), toml checked. UNKNOWN: whether the 15 s rule behaves in the real game; phase None while loading counts as "not Expedition", so a very long load with no SC lobby after an expedition could end the match early.

### MM-3c — What creates Seamless Co-op's lobby A (removes the host's Invite Members step)
Owner: SONNET/implementer → HUMAN-solo (5 min) → OPUS analysis | Status: built, awaiting HUMAN-solo run | Depends: none (run after MM-3a in one build)
Fact: lobby A appears only when the host opens Matchmaking Settings > Invite Members, every match (S9 §5c). Goal: find the call
chain that creates it, so the host's mod can trigger the same game or SC path itself. Dev-only probe `mm_lobby_trace = true`
(forced off in player and mmtest builds). Interop only: **no byte of `nrsc.dll` or the game is patched.**
1. Hardware breakpoint (debug register DR0, execute) on the code that `ISteamMatchmaking::CreateLobby` runs: the vtable slot of the
   `SteamAPI_SteamMatchmaking_v009()` object (slot index from the SDK header bundled by steamworks-rs; write the index and the source line
   into the code comment) AND the flat export `SteamAPI_ISteamMatchmaking_CreateLobby` (DR1). Set on every thread of the process
   (Toolhelp snapshot + Get/SetThreadContext), re-applied every 2 s for new threads, cleared after 3 hits or 10 min.
2. A vectored exception handler catches `EXCEPTION_SINGLE_STEP` from those addresses only (else continue search), logs the thread id,
   the arguments (rcx/rdx/r8/r9 → lobby type and max members), then walks the stack from the CONTEXT with `RtlLookupFunctionEntry` +
   `RtlVirtualUnwind` (max 32 frames) and logs every frame as `module+offset` (module from `GetModuleHandleExW(FROM_ADDRESS)`). Resume
   with RF set in EFlags. Our own queue lobby `CreateLobby` calls will also hit: label them by thread (our worker's id is known).
3. Same treatment, logging only, for `ISteamFriends::ActivateGameOverlayInviteDialog` (DR2), to see whether the screen opens Steam's dialog.
Acceptance (HUMAN-solo, `scripts\launch.ps1`): reach the Roundtable, open Matchmaking Settings > Invite Members, close it, quit. Game and
Seamless Co-op must behave normally. Send `dist\nrpvp.log`. Opus then reads the frames in `nrsc.dll` / `nightreign.exe`.
Stop and report if setting the thread contexts fails or the game hangs.
Result (implementer, 2026-10-14): built, awaiting HUMAN-solo run. New `steam/lobtrace.rs` (+ `mm_lobby_trace` in `config.rs`, spawn in `startup.rs`, `note_steam_thread` call in `probe.rs`, features `Win32_System_Diagnostics_ToolHelp` + `Win32_System_Kernel`). Grep prefix `mm_lobby_trace:` (hits: `mm_lobby_trace: HIT n/3 <label> at 0x.. thread=.. rcx=.. rdx=.. r8=.. r9=.. [..]` then `mm_lobby_trace:   #i module+0xOFF (0xADDR)`). Switch `mm_lobby_trace` default false, forced off in the player build and written `false` in the mmtest toml (`package-release.ps1`, zip NOT rebuilt); `dist\nrpvp.toml` has it `true`. Armed: DR0 flat `SteamAPI_ISteamMatchmaking_CreateLobby`, DR1 the live vtable target behind it, DR2 flat `SteamAPI_ISteamFriends_ActivateGameOverlayInviteDialog`, DR3 its vtable target. The SDK headers are NOT on this machine (steamworks-rs is not a dependency), so no slot index is hard-coded: the displacement is decoded at run time from the export's first bytes (`mov rax,[rcx]; jmp [rax+disp]`) and the target read from the live interface object; the log prints the 24 raw bytes of each export and the decoded slot (disp/8). If the thunk has another shape only DR0/DR2 are armed (the bytes are logged to decode by hand). Order differs from the card (DR0 = flat, DR1 = vtable). VEH is alloc/lock-free (static 16-slot ring, CAS claim, CONTEXT + 8 KiB stack copy, RF set); unwinding (`RtlLookupFunctionEntry`/`RtlVirtualUnwind`, 32 frames, stack-pointer rebasing into a scratch copy) and logging run on the tracer thread. Threads suspended only around Get/SetThreadContext with no allocation; re-applied every 2 s; per address disarmed after 3 hits; all DRs cleared on every thread when all are done or after 10 min, then the VEH is removed. Build OK, 366 tests pass (was 357; +8 lobtrace: DR7, hit counting/disarm, finish rule, thunk decoder, formatting, rebase, handler ignores foreign exceptions, real-stack unwind; +1 config), clippy clean. Left for the HUMAN: the run above. UNKNOWN/risk: Arxan/EAC-free anti-debug may notice debug registers or the VEH (use `scripts\launch.ps1`, same as always); the game may call the vtable of a different Steam interface version than `_v009`/`_v017` returns (then DR1/DR3 never fire; check the `thunk jumps through` line); a call made before the tracer armed (it waits for Steam init) is missed, so open Invite Members only after the Roundtable loads; our own queue-lobby CreateLobby (mm_queue) also hits and is labelled `OUR Steam thread`; frames in the first hit beyond 8 KiB of stack are cut.

### MM-3d — Does Seamless Co-op adopt a lobby the mod creates the game's way?
Owner: SONNET/implementer → HUMAN-solo (5 min) | Status: built, awaiting HUMAN-solo run | Depends: MM-3c (done, S9 §5d)
Fact (S9 §5d): Invite Members makes the game call `CreateLobby(Public, 6)` through SC's proxied `ISteamMatchmaking`; that lobby
becomes SC's lobby A. Question: if the host's mod makes the same call through the same interface, does SC adopt it?
Dev only, switch `mm_sc_create = true` (default false; forced off in player and mmtest builds; on in `dist\nrpvp.toml`).
1. Overlay dev button **"create SC lobby (test)"** in the Queue block (no hotkey). Refuse, with the reason logged, if the safety guard
   fails or the player is already in an SC lobby (`env.sc` is Some).
2. Call `SteamAPI_ISteamMatchmaking_CreateLobby(SteamAPI_SteamMatchmaking_v009(), type, 6)` from the Steam worker thread, the
   same way the queue lobby is created; `type` from `mm_sc_create_type` = `"public"` (default) | `"friends"` | `"private"`. Poll the
   `LobbyCreated_t` call result with the existing poller and log result + lobby id.
3. For 20 s after creation log, every 2 s: owner, members, limit, `locked` (from `LobbyEnter_t`), and every data key/value of that
   lobby, plus whether the worker's SC-lobby detection now reports it (`env.sc`). Prefix `mm_sc_create:`.
4. Write nothing to that lobby (no `SetLobbyData`, no type changes). If after 20 s it carries no `nrsc_lobby_type`, it is ours alone:
   `LeaveLobby` it and log `not adopted`. If it carries `nrsc_lobby_type`, leave it alone and log `ADOPTED by Seamless Co-op`.
5. Keep `mm_lobby_trace` working next to it (the trace will also show our call on our thread; fine).
Acceptance (implementer): tests for the refuse rules and the adopted/not-adopted decision with `FakeSteam`; `cargo test`, clippy clean;
dev DLL built; do NOT rebuild the mmtest zip.
Acceptance (HUMAN-solo, `scripts\launch.ps1`): at the Roundtable press the button once, wait 30 s, then open Invite Members and close
it, quit. Send the log. Also say if anything on screen changed when the button was pressed.
Result (implementer, 2026-10-14): built, awaiting HUMAN-solo run. New `mm/sccreate.rs` (state machine `ScCreate`: press -> `CreateLobby(type, 6)` -> poll `LobbyCreated_t` -> watch 20 s, sample every 2 s -> verdict), `steam/queue.rs` (`QueueSteam::begin_create_sc`, `ScType::Friends` = 1 + `parse_create`/`create_name`, FakeSteam `adopt_created`), `mm/worker.rs` (`Cmd::ScCreate`, SC-lobby detection `env.sc` passed to the sampler), `mm/mod.rs` (`Cmd::ScCreate`, `Status.sc_create_ui/sc_create_line`), `overlay/{mod,shared}.rs` + `game/mod.rs` (`Command::MmScCreate`), `config.rs` (`mm_sc_create` default false, `mm_sc_create_type` = public|friends|private, dev + `mm_queue` only, forced off in the player build), `startup.rs`, `scripts/package-release.ps1` (mmtest toml `mm_sc_create = false`; zip NOT rebuilt). Button: **"create SC lobby (test)"** in the Queue block of the debug overlay (no hotkey), with a coloured status line under it. Log prefix `mm_sc_create:` (lines: `pressed`, `LobbyCreated result=.. lobby=..`, `+N.Ns lobby=.. owner=.. members=[..] limit=.. locked=.. data=[k="v",..] env.sc=.. nrsc_lobby_type=..` every 2 s, then `ADOPTED by Seamless Co-op` or `not adopted ... LeaveLobby`). Refusals (`mm_sc_create: REFUSED: ...`): safety guard fails, already in an SC lobby (`env.sc` Some), a test already running. Writes nothing to the lobby: no set_* call exists in the module; the only call besides reads is `leave` (takes the `QueueLobby` from our own CreateLobby result, refuses again at call time if `nrsc_lobby_type` is present), made only when the marker is absent after 20 s. Dev `dist\nrpvp.toml`: `mm_sc_create = true`, `mm_sc_create_type = "public"`; `dist\nrpvp.dll` rebuilt. 377 tests pass (was 366; +8 sccreate, +1 config, +1 queue, +1 refuse-rule), clippy clean. Left for the HUMAN: the run in the Acceptance line. UNKNOWN/risk: if the game's callback pump consumes our `LobbyCreated_t` the poll reports an invalid handle and the lobby is not tracked (a stray lobby could stay open until quit; the log shows the `event LobbyCreated` line with its id); the verdict is the marker on the lobby within 20 s only; `mm_lobby_trace` hits on our own call are labelled `OUR Steam thread`.

### MM-3e — Host's mod creates the SC lobby at MATCH READY (no Invite Members)
Owner: SONNET/implementer → HUMAN + friend (FS-MM2) | Status: FAIL in FS-MM2, superseded by MM-3f | Depends: MM-3d (ADOPTED, S9 §5e)
Build on `mm/sccreate.rs` and the gate (`mm/machine/gate.rs`). New config `mm_host_create` (dev + `mm_join` only, default false,
forced off in the player build; **true in the mmtest zip**).
1. Host, MATCH READY, `env.sc` is None, guard OK: create the lobby with the MM-3d call (`Public`, 6) once. Wait for the adopted lobby
   (`nrsc_lobby_type` present). From then the gate runs as today (publish `sc_lobby`, invite, ALL PRESENT) with that lobby.
2. If the created lobby is not adopted within 10 s → `LeaveLobby` it (ours only, as MM-3d) and fall back to today's prompt
   ("open Matchmaking Settings > Invite Members ..."). Also fall back if no roster member has entered within 45 s of publishing, but
   then keep our lobby (do not leave it) and change the host line to: `<names> did not arrive: open Invite Members and invite them`.
3. Never create a second lobby in the same match. If `env.sc` already exists (a lobby the game made) use it and create nothing.
4. Host status while creating: `creating the Seamless Co-op lobby...`. Log prefix `mm_host_create:`.
5. Two adopted SC lobbies at once (the host opened Invite Members anyway): the worker's `env.sc` must stay on the lobby the gate
   published (`sc_lobby`); log the second one as `second SC lobby ignored`. Test it.
6. `scripts\package-release.ps1 -MatchmakingTest`: `mm_host_create = true` (keep `mm_join_auto = "invite+link"`, `mm_sc_create = false`,
   `mm_lobby_trace = false`). Rebuild the mmtest zip. Update `scripts\release\MATCHMAKING TEST.txt`: Solo test = nobody opens any menu;
   only if the host's line asks for it, the host opens Invite Members.
Acceptance (implementer): FakeSteam tests for 1–3 and 5; `cargo test`, clippy clean; zip rebuilt and its toml checked.
Acceptance (HUMAN + friend): FS-MM2 (TESTING.md §6), step 1.
Result (implementer, 2026-10-14): built, awaiting FS-MM2. Config `mm_host_create` (`config.rs`, `startup.rs`, `worker.rs` Setup, `MachineCfg.host_create`; dev + `mm_join`, forced off in the player build, true in the mmtest toml). `mm/machine/gate.rs`: `HostCreate` state (Todo, Creating, Watching, Adopted, Done) run by `host_create_step` on the first gate pass of a host with `env.sc` None and the guard OK: `begin_create_sc(Public, 6)` once, poll `LobbyCreated_t` (15 s), adopted when `nrsc_lobby_type` appears, `LeaveLobby` (our QueueLobby only) if not adopted within 10 s; never a second create (also none if `env.sc` exists, on failure, or after expedition); no-arrival line after 45 s since `sc_lobby` was published when no other roster member is in (lobby kept); `abandon_create` on Leave/match end (pending create becomes an orphan, a not-adopted lobby is left, an adopted one is untouched). `machine.rs`: `preferred_sc_alive`; `worker.rs`: `on_event` ignores a second SC lobby while the match's lobby is alive (log `mm_host_create: second SC lobby ignored`) and `current_sc` stays on the match's lobby. Log prefix `mm_host_create:`. Host lines, automatic path: `MATCH READY host=..` -> `creating the Seamless Co-op lobby...` -> `MATCH READY - waiting for <name> to enter your Seamless Co-op lobby (1/2 present)` -> `ALL PRESENT 2/2 - host: start the expedition`; joiner: `MATCH READY - waiting for the host's Seamless Co-op lobby` -> `joining the host's lobby... (if a Steam invite pops up, accept it)` -> `MATCH READY - in the host's lobby, waiting for the others` -> `ALL PRESENT - waiting for the host to start`. Fallback (not adopted in 10 s): host `host has no Seamless Co-op lobby yet: open Matchmaking Settings > Invite Members and invite <name>`; (no arrival 45 s): host `<name> did not arrive: open Invite Members and invite them`. Tests 377 -> 385 (+7 gate `host_create_tests`, +1 config), clippy clean, release build OK. Zip `dist\NightreignPvP-mmtest.zip` 2026-10-14 19:46 local (22:46 UTC), toml checked (`mm_host_create = true`, `mm_join_auto = "invite+link"`, `mm_sc_create = false`, `mm_lobby_trace = false`, F6/F7/F9 unbound); `MATCHMAKING TEST.txt` Test 2 updated (nobody opens a menu; Invite Members only if the host's line asks). Left for the HUMAN: FS-MM2 step 1 (two PCs). Risks: MM-3d proved adoption only solo (+0.0 s); with a real queue lobby open next to it, or with the joiner's invite/link on a lobby the game never "opened" via the UI, behaviour is UNKNOWN; if Steam rejects the invite on our created lobby the 45 s line appears; a `LobbyCreated_t` consumed by the game's pump leaves an untracked lobby (log `no LobbyCreated result`).

FS-MM2 (2026-10-19): **FAIL, superseded by MM-3f.** Creation and adoption worked on both PCs, but the joiner's game would not stay in a lobby the game never opened, and a direct `JoinLobby` gave Steam membership without the SC session (S9 §5f).

### MM-3f — Host's Invite Members creates the lobby; the mod suppresses the Steam invite dialog
Owner: SONNET/implementer → HUMAN (solo) then FS-MM3 | Status: PASS (solo + FS-MM3) | Depends: S9 §5d, §5f
Goal: keep the lobby the game's own session code makes (with `inviteParam`), but without the Steam overlay on the host.
1. `mm_host_create` off in the mmtest toml (`scripts\package-release.ps1 -MatchmakingTest`); code stays, dev only.
2. New dev config `mm_quiet_invite` (dev + `mm_join`, default false, forced off in the player build, true in the mmtest zip). While a match
   is in MATCH READY and we are the host, a hardware execute breakpoint (reuse `steam/lobtrace.rs`: DR slot, arming on every thread, VEH)
   on the flat `SteamAPI_ISteamFriends_ActivateGameOverlayInviteDialog` skips the call: in the VEH, only when the return address is in
   `nightreign.exe`, set RIP = [RSP], RSP += 8 (the function returns void). No code bytes are written. Outside MATCH READY the
   breakpoint is disarmed and the dialog works as normal (Duo and Squad parties still use it).
   Log `mm_quiet_invite: invite dialog suppressed (lobby <id>)`.
3. Host status at MATCH READY when `env.sc` is None: `MATCH READY - press Matchmaking Settings > Invite Members (no Steam window will open)`.
   When the game's lobby appears, the gate runs as today: publish `sc_lobby`, `InviteUserToLobby`, joiner link.
4. Joiner: no `Direct` button in the mmtest build. Joiner line unchanged.
5. `ALL PRESENT` must not rely on Steam membership alone: it also needs the local ChrSet to hold at least the roster size in players
   (main thread's player count, passed to the gate). Until then: host `in the lobby 2/2, waiting for <names> to load in`, joiner
   `in the host's lobby, loading in...`. Signal `[VERIFIED: FS-MM1 players=2 on a real join; FS-MM2 stayed 1]` (S9 §5f).
6. Leave/abandon: if we created nothing, leave nothing (the game's lobby belongs to the game).
Acceptance (implementer): tests for 2 (gate on/off by state, return-address filter as a pure function) and 3; `cargo test`, clippy clean;
mmtest zip rebuilt, toml checked.
Acceptance (HUMAN, solo): queue Solo with `mm_match_players = 1` is not possible, so: dev button "test quiet invite" arms it, Leonardo
presses Invite Members: no Steam window, log shows the suppressed call and an adopted lobby with `inviteParam`.
Acceptance (FS-MM3): host presses Invite Members at MATCH READY; the joiner arrives with no clicks and is at the host's Roundtable;
expedition with both.
Result (implementer, 2026-10-19): built, awaiting the HUMAN-solo check. New `steam/quietinvite.rs` (pure fns `return_in_image`, `decide`, `arm_wanted`, `keep_armed`, `skip_registers`; VEH; arming thread `nrpvp-quietinvite`), `steam/lobtrace.rs` (only `pub(super)` on `apply_all`, `TARGETS`, RF/continue constants), `steam/mod.rs`, `config.rs` (`mm_quiet_invite`: dev + `mm_join`, default false, off in the player build, switched off with a warning when `mm_lobby_trace = true` because both own DR0-DR3 on every thread), `startup.rs` (spawn with the exe base/size), `mm/worker.rs` (`Setup.quiet_invite`, `quiet_tick`: arm decision each tick, 5 s linger after the SC lobby appears because the game opens the window ~300 ms after `LobbyCreated_t` and env.sc refreshes every 500 ms; `Cmd::QuietTest` = 60 s window; status line), `mm/mod.rs` (`Cmd::QuietTest`, `Status.quiet_ui/quiet_line`, `LOCAL_PLAYERS`), `mm/machine.rs` + `mm/machine/gate.rs` (`MachineCfg.quiet_invite`, `Machine::quiet_invite_wanted(own)` = host of a MATCH READY match, no `env.sc`, no ALL PRESENT / expedition yet; host line of step 3; `Env.players`, `everyone_loaded`, the "load in" lines of step 5), `game/team.rs` (stores `res.players.len()` next to `note_local_team`), `game/mod.rs`, `overlay/{mod,shared}.rs` (button "test quiet invite" + coloured line in the Queue block; the "(c) direct JoinLobby" button is hidden whenever `mm_quiet_invite` is on). **Deviation from step 2, on evidence:** the MM-3c trace shows the game calls the VTABLE target of `ISteamFriends::ActivateGameOverlayInviteDialog` (`0x7FFD1F0A2070`, steamclient64.dll; DR3 hit, the flat export DR2 never hit; log 22:18:22.292Z), so the mod arms BOTH the flat export (DR0) and the vtable target (DR1, decoded at run time from the flat thunk and the live `ISteamFriends` object exactly as `lobtrace` does); a flat-only breakpoint would never fire. The return-address filter (caller inside the nightreign.exe image) applies to both. Logs: `mm_quiet_invite: DR0/DR1 = ...`, `ready`, `test armed for 60 s`, `ARMED (n thread(s)...)`, `invite dialog suppressed (lobby <id>) - call from nightreign.exe+0x...`, `left alone: return address ... is not in nightreign.exe`, `disarmed`. Status lines: host `MATCH READY - press Matchmaking Settings > Invite Members (no Steam window will open)`; both in the Steam lobby but the ChrSet holds fewer than the roster: host `in the lobby 2/2, waiting for <names> to load in`, joiner `in the host's lobby, loading in...`; then `ALL PRESENT ...` as before. Overlay: `quiet invite: test armed, N s left: press Matchmaking Settings > Invite Members`, then green `quiet invite: SUPPRESSED 1 Steam window(s)`. Package: `-MatchmakingTest` toml `mm_host_create = false`, `mm_quiet_invite = true`, `mm_join_auto = "invite+link"`, `mm_sc_create = false`, `mm_lobby_trace = false` (checked inside the zip); `MATCHMAKING TEST.txt` Test 2 rewritten (host presses Invite Members when the line asks, no Steam window, joiner does nothing; "load in" lines explained). Zip: C:/Users/Leonardo/source/repos/nightreign-pvp-main/dist/NightreignPvP-mmtest.zip (THIS worktree's dist), 2026-10-19 14:44 local (17:44 UTC), 7.4 MB, nrpvp.dll E3F8E7E8E9532E87; dev DLL dist/nrpvp.dll is the same build. Tests 385 -> 396 (+5 quietinvite incl. a VEH test that fires the handler on a fake CONTEXT, +5 gate, +1 config), `cargo clippy --all-targets` clean, release build OK. Left for the HUMAN: the solo check in the Acceptance (HUMAN) line (dev toml needs `mm_queue = true`, `mm_join = true`, `mm_quiet_invite = true`; press "test quiet invite", then Invite Members; send the log) and FS-MM3. Risks / UNKNOWN: (1) skipping the vtable target is safe only if it is a void function with no stack-cleanup duty: true for `ISteamFriends::ActivateGameOverlayInviteDialog(CSteamID)` per the SDK signature, but not yet seen running; (2) hardware breakpoints on every thread are the same mechanism as MM-3c (135 threads, no hang reported) but now stay armed for long stretches while the host waits; (3) a second Invite Members press more than 5 s after the lobby exists shows the Steam window again, by design; (4) the player count is the main thread's ChrSet count; if it does not reach the roster size on a real join the lines stay at "load in" and ALL PRESENT never comes (FS-MM1 logged players=2 on the host only); (5) the vtable target could be shared with another call site in nightreign.exe (identical code folding); the log line shows every skipped call-site offset.
HUMAN solo (2026-10-19): **PASS**: game lobby with `inviteParam` created, invite dialog skipped (call from `nightreign.exe+0x258568B`), no Steam window, no hang (TESTING.md). Next: FS-MM3.
FS-MM3 (2026-10-19): **PASS**: Solo match with one host click (Invite Members), joiner automatic, expedition as rivals, match end at the Roundtable (TESTING.md, S9 §5g).

### MM-4 — Solo queue end to end, player build
Owner: SONNET/implementer → HUMAN (solo) then FS-MM4 | Status: built; solo steps 1–4 PASS 2026-10-19, awaiting FS-MM4 (incl. step 5) | Feeds: F1, F15
S9 §6 for Solo in the **player build** (`dev_mode = false`), with the join path proven in FS-MM3 (host presses Invite Members, quiet
invite, joiner link + invite, ALL PRESENT needs ChrSet players).
1. Config: new player setting `matchmaking` (default true). In the player build it turns on what Solo matchmaking needs: the queue, the
   join step with `invite+link`, and the quiet invite. Everything else stays forced off (`mm_probe` logging, `mm_host_create`,
   `mm_lobby_trace`, `mm_sc_create`, manual join buttons). If the queue or gate depends on callbacks the probe registers, keep the
   callbacks and drop only the probe's logging. `matchmaking = false` turns it all off.
2. Lobby size and start rule (S9 §6 step 5): `mm_max_players` (default 6) is the queue lobby size; the match starts when the lobby is full,
   or when `mm_fill_wait_s` (default 90) has passed since the lobby got its 2nd member and it holds at least `mm_min_players` (default 2).
   Keep `mm_match_players` working as before for dev/mmtest (it sets min = max); the mmtest zip keeps 2 so tests stay quick.
   Gate and invites must handle up to 5 joiners: FakeSteam tests with 3 and 6 players.
3. Missing players (step 8): host, 60 s after publishing `sc_lobby`, with present ≥ min: `N/M here - you can start; <names> did not
   arrive`. A joiner not in the host's lobby 120 s after `state=go`: give up, leave, back to idle with `match missed - queue again`.
4. Leave the queue by itself when the player leaves the Roundtable or the world unloads while Searching or InQueue (not in MatchReady:
   a joiner's world reloads when it enters the host's lobby, S9 §5g). Status `Queue: left (you left the Roundtable)`.
5. Player overlay Queue block: one button `Solo Battle Royale` and `Leave`; Duo/Squad hidden (MM-6) and no dev buttons. Lines in player
   words: `Searching for players... 3/6`, `Match found!`, host `Match found! Press Matchmaking Settings > Invite Members to open your
   lobby (no Steam window will open)`, joiner `Match found! Joining the host...`, `Everyone is here - host: start the expedition`,
   `In match`. Dev builds keep today's detailed lines.
6. Packaging: the default player zip (`scripts\package-release.ps1` without `-MatchmakingTest`) ships with `matchmaking = true`;
   update its how-to text (Solo: both press the button; the host presses Invite Members when asked; nobody else does anything).
Acceptance (implementer): tests for 1–4 (FakeSteam), `cargo test`, clippy clean; both zips rebuilt and their tomls checked.
Acceptance (HUMAN, solo): player zip: button, `Searching for players... 1/6`, Leave, and quitting to the title leaves the queue.
Acceptance (FS-MM4): two PCs on the player zip: Solo match with only the host's Invite Members press; expedition; back at the Roundtable.
Result (implementer, 2026-10-19): built, awaiting HUMAN-solo. Config (`config.rs`): player setting `matchmaking` (default true; in the player build it forces `mm_queue`, `mm_join`, `mm_quiet_invite` to follow it and `mm_join_auto = "invite+link"`; `mm_probe`, `mm_host_create`, `mm_lobby_trace`, `mm_sc_create` stay forced off; no effect in dev mode), `mm_max_players` 6 (2..=6, the queue lobby size/`need` tag), `mm_min_players` 2 (2..=max), `mm_fill_wait_s` 90 (5..=600); `mm_match_players` in the file sets min = max (mmtest keeps 2). The default `nrpvp.toml` text no longer sets `mm_match_players` (it would override the new defaults). Machine (`mm/machine.rs`): owner starts when `complete_players >= max`, or when the lobby has had >= 2 members for `fill_wait` and holds >= `min` (both need all tickets complete); `world_tick` leaves Searching/Creating/Joining/InQueue after 3 s away (never MatchReady) with idle line `Queue: left (you left the Roundtable)`; `MachineCfg.friendly` gives player words. Gate (`mm/machine/gate.rs`): host line `N/M here - you can start; <names> did not arrive` 60 s after publishing `sc_lobby` when >= min are here AND loaded (here = min(Steam members, ChrSet players)); joiner not in the host's SC lobby 120 s after `state=go` (not after ALL PRESENT/expedition) leaves, idle line `match missed - queue again`; `Gate.pline` = player-words line. Signal for step 4 (`mm/mod.rs`): `World::{Never,Gone,Present(phase)}` from the time of the game loop's last local-player read (`Gone` = no read for max(4 s, 3 x watch_interval_ms)). Worker also refuses a queue request with `load into the Roundtable first` (no world) / `go back to the Roundtable first` (expedition). Player UI (`overlay/`, `startup.rs`, `config.rs::binding`): the player build keeps the read-only badge and gains ONE hotkey (the overlay toggle, default Insert, hotkey thread starts only for it) that opens a small interactive panel `Battle Royale matchmaking` with the status line, `Solo Battle Royale` and `Leave` (no Duo/Squad, no dev buttons); the badge gets a third line with the queue status or `[Insert] Battle Royale matchmaking`; the game loop accepts only `MmQueue(Solo)`/`MmLeave` from the player overlay. Probe (`steam/probe.rs`): `spawn(queue, evidence, verbose)`; player build verbose = false (callbacks registered, startup dump and per-event lobby dumps dropped, one `mm_probe: quiet mode` line); mm_queue/mm_join/mm_quiet_invite/mm_host_create log lines unchanged. Tests now 419 (418 pass, 1 ignored hudhook smoke; the MM-3f card counted 396) with new ones for config, lobby size/fill wait, 3 and 6 player gate, start line, give-up, player words, auto-leave, world state, overlay tones, command whitelist, `cargo clippy --all-targets` clean, release build OK. Zips (this worktree's `dist\`, 2026-10-19 15:49 local = 18:49 UTC, nrpvp.dll 5AA3CF20AF48B3A9): `NightreignPvP.zip` toml = `team_size = 1`, `matchmaking = true`, `team_candidates`; `NightreignPvP-mmtest.zip` toml unchanged (dev_mode, `mm_match_players = 2`, `mm_quiet_invite = true`, `mm_host_create = false`, `mm_lobby_trace = false`, `mm_sc_create = false`). How-to text: new FIND PLAYERS section (the mmtest zip carries the same HOW TO INSTALL.txt). Left for the HUMAN: the solo check in the Acceptance (HUMAN) line, then FS-MM4. Risks/UNKNOWN: (1) the leave-by-itself signal is the `team_type` read once per watcher period; a long stall of the game loop in the Roundtable could look like `Gone` (3 s debounce after 4 s); (2) the player panel needs the Insert key, which Seamless Co-op or another mod might also use (configurable through `overlay_toggle_key`); (3) up to 5 invites/links are only proven by FakeSteam, the real 2-player path is FS-MM3's; (4) dev builds without `mm_match_players` in their toml now get lobby size 6 / min 2 / 90 s instead of 2.

### UI-0 — In-game UI instead of ImGui: map the Roundtable menus, messages and flags
Owner: OPUS → researcher/implementer → HUMAN (screenshots, solo checks) | Status: **DONE 2026-10-19** (step 6 PASS solo; two-player part = FS-UI1) | Feeds: MM-3g, UI-1, UI-2
Plan, questions Q1–Q8, steps 1–7 and results log: `research/UI-0-ingame-ui.md`. Decision behind it (Leonardo, 2026-10-19): no ImGui for
players in the final product; we must be able to add/replace game menu options and change their behaviour.
Acceptance: as in that file §7 (Q1–Q7 answered with evidence tags, the round-trip + "Battle Royale (test)" table entry proven or ruled
out, cards UI-1, UI-2 and MM-3g written).
Result (Opus, 2026-10-19): answers table in `research/UI-0-ingame-ui.md` §10. Toolchain in `Z:\Tools\` (Nuxe, WitchyBND, esdtool, DarkScript3,
Smithbox), unpack in `Z:\Tools\nr-unpacked\1.03.3\`. Proven solo: a talk list at the table before the stock expedition menu, our own
EventTextForTalk texts, our flag 10007000 set by the list and read by the DLL, SC unaffected (table, menu, Solo start). Learned: SC hooks
`LookupEntry` (`exe+0x27F4D20`) and swaps the Number of Players labels; no talk command opens Matchmaking Settings / Invite Members;
EMEVD Generic Dialog with EventTextForMap ids is the status-text route. Dev tools added: `ui_text_trace`, lobby-flag watch, flag-group census,
`experiments/UI-0/map_text.py`, profiles `nrpvp-ui0a/ui0b`. Two-player check: FS-UI1 (TESTING.md §6).

### UI-1 — Battle Royale inside the stock expedition menu (Leonardo's flow)
Owner: OPUS (RE) → SONNET/implementer → HUMAN (solo), then FS | Status: investigating A/B/C | Depends: UI-0 | Replaces: MM-5
**Flow (Leonardo, 2026-10-19, "the best flow possible, we are not giving it up"):** spawn at the Roundtable → press the table → the stock boss
list → **Matchmaking Settings → Number of Players shows Solo / Duo / Trio** → back, pick the **first boss** (only that one for now) →
**Commence Expedition** → the game searches for players (**our** queue) → when the match is full the expedition starts by itself. Cancel =
the game's own "Cancel matchmaking". Battle Royale only (no normal expedition while the mod is installed). On a roadblock: talk to
Leonardo about alternatives that keep this flow.
**Why it needs the DLL:** the menu is native code (`OpenCommenceExpeditionMenu`, menu 34), not a script; under me3 the game is offline, so
Commence with more than one player shows 506023 "Cannot start multiplayer session in offline mode" and then "Matchmaking failed"; no
event flag changes (step 0 result below). We must **never** make the game think it is online (that is Bandai's servers).
**Static findings (2026-10-19, `experiments/UI-0/imm_xrefs.py`, `disfunc.py`, game 1.03.3)** `[VERIFIED: disassembly; meaning HYPOTHESIS until the trace]`:
- The expedition menu code is ordinary (has `.pdata`): `0x9EE750` "Cancel matchmaking?" 506012 / "canceled" 506011 (tail obfuscated);
  `0x9EEBE0` **Commence checks**: 506040 not installed, 506070 too many relics, 506024 Deep of Night offline, then 506023 offline-multiplayer,
  decided from **byte `+0x49` of the settings object `[menu+0xFC8]`** (`<=1` with online, or `==2` → the matchmaking branch) and
  `0x6AEAE0` = "is online" (`[exe+0x3C13258]+0xB40`); `0x9EF3F0` Commence button label (130304 Solo / 130305 Two-Player); `0x9FB020`
  **Number of Players list** (130408/130417/130409; settings `[rbx+0x3E08]`, same `+0x49` byte, reset to 0 in some cases); `0x60A1C0` a
  table matchmaking-result → dialog (506020..506030 family).
- The "Matchmaking..." ids (336900..336905) are computed, not constants: the matchmaking screen's code is found from the stack (trace).
**A (Solo/Duo/Trio in Number of Players):** read/write the `+0x49` byte (values per option from the trace); labels: SC swaps 130409/130417/
130408 through its `LookupEntry` hook, so our labels need a later hook or FMG ids that SC does not touch (to decide).
**B (Commence starts our queue):** catch the Commence check sequence (`0x9EEBE0`) when the setting is Duo/Trio (or our Solo), before the
offline dialog, and start `MmQueue` instead (hardware breakpoint + skip, as MM-3f does), so the offline dialog never shows.
**C (the game's own matchmaking screen and cancel):** find the screen's state from the stack of the 336900 lookups; drive "Matchmaking..."
and the "<remaining> / <total>" counter (336905, tags `matchRemainingCount` / `matchTotalCount`) from our queue; cancel through `0x9EE750`.
Fallback if C is blocked: our own text (UI-2) with the same flow; talk to Leonardo first.
0. **Step 0 result (2026-10-19 22:26–22:30 UTC, log `experiments/UI-0/ui1-step0-nrpvp.log`)**: "Up to 6 players" → Commence: 506023
   offline, 336900 "Matchmaking...", 336905 counter, 506020 "Matchmaking failed"; no Steam lobby event; **no event flag changed** (50 ms
   watcher, `ui_flag_watch`). Solo Commence: departure cutscene flags 2307/2330 ON.
1. **Trace for A/B/C (built 2026-10-19):** `game/menutrace.rs`, dev config `ui_menu_trace = true`: DR0 `0x9EEBE0`, DR1 `0x9FB020`, DR2
   `0x9EE750`, DR3 `LookupEntry` only for 336900..336905 / 506011 / 506012 / 506020..506023; logs registers + exe return candidates from the
   stack; dumps the settings object candidates and logs every byte change of them every 100 ms. `ui_flag_watch` stays on. 430 tests, clippy clean.
   **Runbook (Leonardo, solo, ~5 min, `.\scripts\launch.ps1`)**, press **F10** before each step:
   1. Roundtable → table → boss list. 2. Matchmaking Settings → Number of Players: select each of the three options in turn, ~2 s each,
   end on the **first "Up to 6 players"**. 3. Back, **Commence Expedition** → the offline message → OK. 4. Number of Players → the
   **second "Up to 6 players"** → Commence → OK. 5. Number of Players → **Solo Mission** → Commence; if a cancel is offered, cancel once,
   then Commence again and let it load. 6. Quit. Then tell Claude "menu trace done".
2..n. Implementation steps are written after the trace answers A/B/C.

### UI-2 — Matchmaking status in the game's own messages
Owner: OPUS (spec) → SONNET/implementer → HUMAN (solo) | Status: ready after UI-1 | Depends: UI-1 (flag writes)
Goal: "Searching 3/6", "Match found", the host prompt, "Joining the host", "Everyone is here", "Left the queue" without ImGui.
1. **Spike first: which instruction draws in the Roundtable while the player walks around**, and how we author EMEVD. Options in
   order: (a) an EMEVD event in `m10_00_00_00.emevd.dcx` that waits on our flag and runs Display Status Message / Display Blinking Message /
   Display Generic Dialog (2007[3]/[4]/[1]) with our EventTextForMap entries, then clears the flag (DarkScript3 is a GUI: find out whether
   its library or Smithbox can save EMEVD without hands; else Leonardo saves it once and we keep the source in `gamedata/`); (b) the
   same message through ESD `OpenGenericDialog` (only while talking: not enough alone). Test solo with a dev hotkey that sets the flag.
2. Texts as EventTextForMap entries in our range (one per count for "Searching N/6", N = 1..6), all languages English for now.
3. DLL: one flag per message (10007100..), set by the matchmaking worker on each status change, rate-limited (one message on screen at a time).
4. Then remove the Insert panel and the badge from the player build (keep the dev overlay).
Acceptance (HUMAN, solo): queue from the table; the in-game message shows "Searching for players 1/6"; Leave shows "Left the queue".

### MM-3g — No host click: trigger Invite Members from the mod
Owner: OPUS (RE) → SONNET/implementer → HUMAN (solo) then FS | Status: ready (RE spike first) | Depends: MM-3f, UI-0 Q3
UI-0 settled that **no talk command opens Matchmaking Settings or Invite Members** (all NR ESDs + esdtool's command list), so the data route
is closed. The action is the game's menu code: S9 §5d has the chain (menu functor calls `+0x80D743` → `+0x80E32A` → `+0x213B4F`, session
code `+0x258568B` / `+0x257F5DA`, the latter obfuscated with no `.pdata`).
1. Spike (Opus, read-only): with the MM-3c trace and `experiments/UI-0/disfunc.py`, find the highest frame that is "Invite Members was
   chosen" and has `.pdata` (`+0x80D743` / `+0x80E32A` are candidates) and what its arguments are (menu object, item id). A hardware-breakpoint
   trace on it while Leonardo presses Invite Members gives the live registers (reuse `ui_text_trace`'s VEH pattern).
2. If a callable entry exists: the host's mod calls it on the main thread at MATCH READY (dev flag first), with MM-3f's quiet invite armed.
   If not: fall back to opening the Matchmaking Settings menu for the host (still one click) and record why.
Acceptance (HUMAN, solo): dev button "invite now (test)" makes the game's lobby (`inviteParam`) with no menu and no Steam window.

### MM-5 — Table entry "Battle Royale" (S7 route B) replaces the overlay button
Owner: — | Status: **superseded by UI-1** (2026-10-19)

### MM-6 — Duo and Squad with rivals: Steam ID → expedition player
Owner: OPUS to card in detail | Status: later | Settles: S9 U7
Match sizes (Leonardo, 2026-10-19): Duo = 3 teams of 2, Squad = 2 teams of 3, always 6 players; complete parties only.
With more than one team in a Duo/Squad match the roster (Steam IDs) must be mapped to expedition players on every client.
