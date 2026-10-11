# Roadmap

**Current phase:** Phase 1–2, shared world + PvP (Phase 0 closed 2026-10-10: GO)
**Current milestone:** automatic, stable PvPvE rule for 2 players (no debug keys), confirmed over a real network (FS-1).
**Next step:** T-019 (find and fix the auto-apply hang), then T-020 (expedition-only rule + grace workaround); FS-1 when the friend is available.
**Constraint:** solo testing only, day to day. Two-player tests are batched into Friend Session FS-1, which is
booked once the readiness gate in TASKS.md passes (T-004, T-008, T-012 done).
**Blockers:** none yet. Open risks are listed below.

## Phases

**Scope v2 (2026-10-10):** the long-term plan is now [PROJECT_PLAN.md](PROJECT_PLAN.md) (milestones M1–M8). The table below is the original
phase list; Phases 1–2 map to M1, Phase 3 to M1–M3, Phase 4 to M8, Phase 5 to M5–M6.

| Phase | Goal | Exit criterion | Status |
|---|---|---|---|
| 0 Recon | Verify foundations; run E-003 | Phase 0 report answers skeleton §7 Q1–Q8; go/no-go recorded | **done** (GO) |
| 1 Shared world | 2 players, same expedition, launched via our pipeline (me3 + SC + nrpvp) | TC-001 passes on the me3 launch path | **active** |
| 2 PvP | Smallest change that gives reliable two-way PvP | TC-003 row 3 passes, plus death and revive without desync | **active** |
| 3 Expedition stability | PvP across a full day/night cycle, loot, objectives, day transitions, disconnects | A full expedition with PvP doesn't break | todo |
| 4 Playable prototype | Repeatable setup; teams configurable for 2+ players; critical bugs fixed | Fresh-machine setup from README works; regression set green | todo |
| 5 Endgame | Final-day team brawl, then boss-or-escape | Separate feasibility first (see FUTURE_CONSIDERATIONS.md) | deferred |

Phases 1 and 2 overlap on purpose. Phase 0 already proves the me3 + SC launch path (T-004), so Phase 1
mostly confirms it holds across a whole session.

## Key risks (ranked)

1. **SC under me3 doesn't work on NR** (me3 #118 was unresolved). Mitigation: T-004 fallback. Last resort:
   ask the SC author how other DLLs should coexist.
2. **Damage authority:** a hit may be decided on one client and the team-type check may not be
   replicated, which would give one-sided damage. E-003 is designed to reveal this. Mitigation: set team
   types on every client (we would need to sync team assignment; SC has no API, so possibly via deterministic
   assignment by Steam ID).
3. **The engine or SC resets team type** (on spawn, revive, or day transition). Mitigation: re-apply each
   tick, or hook the setter.
4. **Arxan integrity checks** break our hooks. Mitigation: me3 `disable_arxan`; prefer data writes over
   code patches in Phase 0.
5. **Game or SC updates** break offsets. Mitigation: AOB/RTTI-based discovery and the version pins in
   TECHNICAL_NOTES.md §1.
6. **Death and revive in PvP:** NR's downed and revive logic assumes allies, so a PvP kill may leave a
   player stuck or end the run. This is tested in TC-003's tail.
7. **SC reverts or blocks our team-type writes** (ER SC has a "minimalist anticheat" that reverts some runtime edits; NR SC hooks
   `WorldChrManImp`). Detected by TC-004 under the SC profile. Mitigation: persist mode, SpEffect `change_team_type`
   (A1), or EMEVD (A2).
8. **Permission:** SC terms forbid bundling or modification. A public PvP release built on SC needs Yui's blessing. This doesn't affect private
   development.

## Completed work

- 2026-10-10: **FS-1 PASS (real Steam, two PCs):** two-way PvP confirmed over a real connection with the friend kit; local = entry[0] and the per-player id confirmed on both PCs. H1 is confirmed end to end for 2 players.

- 2026-10-09: **FS-0 PASS (reported):** two real SC clients (Nucleus) fought each other and enemies with team 29 set via nrpvp. H1 holds locally: no networking changes needed. Pending: the per-step authority matrix with logs, death/revive in PvP, and FS-1 over a real network.

- 2026-10-09: **H1 prerequisite proven solo:** ChrIns+0x74 controls hostility in NR (TC-004 run 2). The PvPvE candidate value is 29 (Indiscriminate), tested in FS-1.
- 2026-10-09: SC+me3 launch path, singleton discovery, RTTI, local-player resolution, and the gated team write all verified.

- 2026-10-09: environment survey, ecosystem research, stack proposal (ADR-001), task breakdown.

## Phase 0 report

Written 2026-10-10 (T-010). Evidence: TECHNICAL_NOTES.md §3–4, TESTING.md §3, logs in `.claude/experiments/`.

**Verdict: GO.** The core hypothesis H1 holds on one PC with two real clients: setting engine team type 29 for every player on every client gives
synced two-way PvP alongside normal PvE, with **no networking changes and no changes to Seamless Co-op**. Not yet confirmed over real Steam
networking with a second person (FS-1). **Update 2026-10-10: FS-1 passed; confirmed over real Steam between two PCs.**

Answers to the skeleton's Phase 0 questions (§7):
1. *Two players in one shared expedition?* **Yes.** SC v1.1.3 + our DLL, via me3 (solo verified) and via Nucleus (two clients verified).
2. *Can players damage each other?* **Yes**, with team_type 29 (`ChrIns+0x74`) on both players. Lock-on, damage, HP sync, and parry all work.
3. *Does PvP need networking changes?* **No.** Each client decides hostility from its own copies; hits and HP travel through the existing netcode.
   Consequence: every client must apply the values locally, and for teams all clients must agree on who is on which team.
4. *Damage, status, death, revive synced?* Damage, HP, and death: **yes**. Status effects: not specifically tested. Revive: **does not work** between
   hostile players (the bar shows but nothing fills it); the dead player respawns near a grace with runes and a level lost.
5. *Does PvE continue after PvP?* **Yes.** Full solo expedition on 29 incl. night boss; PvPvE fights in FS-0. Side effect: sites of grace can't be
   interacted with on 29.
6. *Hooks, libraries, tooling?* Rust DLL; `from-singleton` (singletons), our RTTI scanner (classes), hudhook (overlay, works under me3, not under
   Nucleus), me3 loader, Nucleus Co-op for local two-client tests. No code hooks needed so far: data reads and one-byte writes only.
7. *Limits going from 2 players to several teams?* 29 is free-for-all: teammates would hurt each other. Teams need a team-type scheme (untested values:
   3, 13, 16, …) or a hostility hook, plus a way for all clients to agree on teams (candidate: the per-player id at `ChrIns+0x78`). SC caps at 6 players
   and reports instability at 5–6. Local/remote detection is only verified for 2 players (entry[0] = local).
8. *Risks before the final-day concept?* Death and revive rules in PvP; grace interaction; team assignment and agreement; the `auto_team_all` hang
   (unexplained); SC updates; permission from SC's author before any public release. The final-day brawl itself is still uninvestigated.

**Next (Phase 1–2, see TASKS.md):** make the rule automatic and safe (fix the auto-apply hang, apply only in expeditions, restore near graces),
confirm over a real network (FS-1), then investigate team values so allies are not hostile.

