# Roadmap

**Current phase:** Phase 0, Reconnaissance and feasibility
**Current milestone:** answer H1 (do hostile engine team types give synced PvP over SC?) via E-003.
**Next step:** T-000 (install rustup, confirm the second PC and Steam account), then T-001, with T-002, T-003, and T-005
in parallel.
**Blockers:** none yet. Open risks are listed below.

## Phases

| Phase | Goal | Exit criterion | Status |
|---|---|---|---|
| 0 Recon | Verify foundations; run E-003 | Phase 0 report answers skeleton §7 Q1–Q8; go/no-go recorded | **active** |
| 1 Shared world | 2 players, same expedition, launched via our pipeline (me3 + SC + nrpvp) | TC-001 passes on the me3 launch path | todo |
| 2 PvP | Smallest change that gives reliable two-way PvP | TC-003 row 3 passes, plus death and revive without desync | todo |
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

## Completed work

- 2026-10-09: environment survey, ecosystem research, stack proposal (ADR-001), task breakdown.

## Phase 0 report

_To be written in T-010._
