# Elden Ring Nightreign PvPvE Mod — Project Skeleton

> **Refinements (2026-10-09).** This file is the original scope and stays authoritative for *intent*.
> Operational detail now lives in the repo-root docs: `CLAUDE.md` (agent rules and roles), `TASKS.md`
> (task cards), `ROADMAP.md`, `TECHNICAL_NOTES.md` (stack decision ADR-001, verified facts),
> `TESTING.md`, and `FUTURE_CONSIDERATIONS.md`. Clarifications added after review:
>
> 1. **Expedition timeline terms.** An expedition is Day 1 → Day 2 → final day (the Nightlord). Each of
>    Days 1–2 has a *day phase* (exploration, with the circle shrinking) and a *night phase* (night boss).
>    "Earlier Expedition Days" in §3.1 means **both the day and night phases of Days 1–2**. PvP must stay
>    possible during the day grind and during night-boss fights. The "Final Day" in §3.2 is the Nightlord day.
> 2. **Stack.** A Rust native DLL loaded by me3 next to Seamless Co-op. fromsoftware-rs is the
>    reverse-engineering reference. Rationale and alternatives are in TECHNICAL_NOTES.md §2.
> 3. **Core technical hypothesis (H1).** PvP may need no networking changes at all: giving opposing
>    players hostile *engine team types* on every client may be enough, because NR inherits ER's
>    invasion-era combat netcode. Experiment E-003 (TASKS.md T-009) settles this before any other gameplay work.
> 4. **Test environment.** Two PCs and two Steam accounts that each own the game, the same SC build on both,
>    and saves backed up. Never run the modded game online (no EAC, no official servers).
> 5. **Workflow.** Opus plans, designs experiments, and reviews. Sonnet executes one task card at a time
>    (`.claude/agents/implementer.md`, `researcher.md`). Humans run every in-game step.
> 6. **Third-party code.** Seamless Co-op is closed source. We never modify or redistribute it, and we
>    interoperate only at runtime.

## 1. Project Overview

Develop a PvPvE multiplayer mod for Elden Ring Nightreign that preserves the core vanilla expedition experience while introducing multiple competing teams into the same world.

The preferred starting point is the Nightreign-specific Seamless Co-op mod, subject to technical investigation.

The long-term vision is to combine Nightreign's PvE exploration and progression with emergent PvP encounters, culminating in a competitive final-day confrontation inspired in part by The Duskbloods.

Development must begin with a small, functional proof of concept rather than the complete vision.

## 2. Core Design Principles

1. **Preserve vanilla gameplay.** Keep the existing expedition structure, combat, exploration, loot, progression, and pacing wherever possible.
2. **Shared world.** Multiple players must inhabit the same expedition rather than separate instances.
3. **Emergent PvP.** Players can encounter and fight opposing players while continuing to engage with the PvE world.
4. **Incremental development.** Validate technical assumptions before implementing dependent features.
5. **Minimal initial scope.** Two-player testing is sufficient for the first prototype.
6. **Document future requirements.** Preserve the long-term vision without prematurely implementing it.
7. **Evidence over assumptions.** Verify game behavior, networking capabilities, APIs, and technical limitations before designing around them.

## 3. Gameplay Vision

### 3.1 Earlier Expedition Days — Shared PvPvE

Multiple teams participate in the same expedition.

Players can:

* Explore the same world.
* Fight the same PvE enemies.
* Collect loot and improve their characters.
* Progress through ordinary expedition objectives.
* Encounter opposing players organically.
* Engage in PvP while PvE combat remains active.
* Avoid opposing players when they prefer not to fight.

PvP should introduce risk and unpredictability without replacing the original exploration and progression loop.

### 3.2 Final Day — Competitive Endgame

The long-term preferred direction is to replace the conventional final-day Nightlord confrontation with a decisive team-versus-team brawl.

The proposed flow is:

1. Surviving teams enter the final-day confrontation.
2. Teams fight until a winning team is determined.
3. The winning team chooses between two outcomes:

   * **Challenge the final boss:** Continue into a final PvE encounter and pursue full expedition completion.
   * **Escape:** End the expedition without fighting the final boss.

The initial preferred victory condition is the last surviving team, although the exact rules remain open to investigation and design.

This concept is a future gameplay objective, not a requirement for the initial prototype.

Do not implement the final-day brawl, boss-or-escape choice, custom victory conditions, or associated rewards until the underlying multiplayer functionality has been proven.

### 3.3 Design Intent

The final-day design should create a meaningful risk-versus-reward decision.

Escaping should represent a deliberate, lower-risk conclusion after winning the PvP confrontation. Challenging the boss should offer the opportunity for a fuller PvE victory at additional risk.

The implementation should reuse Nightreign's existing mechanics where practical rather than introducing an unrelated battle royale system.

## 4. Initial Scope

The first development milestone is a two-player multiplayer proof of concept.

Required:

* Both players can join the same multiplayer session.
* Both players can inhabit the same world.
* Both players can participate in ordinary PvE combat.
* Opposing players can damage each other.
* Combat state remains sufficiently consistent across clients.
* The expedition remains functional during and after PvP combat.

Not required:

* A full lobby.
* Dedicated servers.
* Public matchmaking.
* A custom lobby browser.
* Custom team-selection UI.
* Custom rewards or rankings.
* Custom final-day mechanics.
* Custom boss encounters.
* Advanced anti-cheat systems.
* Large-scale performance optimization.
* Friendly-fire prevention.

Do not implement features merely because they may be useful later. Every initial feature must contribute to proving the core concept.

## 5. Friendly Fire

Custom friendly-fire prevention is out of scope for the initial prototype.

Do not add custom team-aware damage filtering, damage interception, or status-effect filtering solely to prevent teammates from harming one another.

First investigate and document the existing game's and Seamless Co-op's behavior.

If friendly fire already occurs, it may remain enabled during testing. If it is already prevented by the underlying framework, preserve that behavior rather than modifying it unnecessarily.

Only investigate custom prevention if it becomes necessary for a validated gameplay requirement.

## 6. Networking Foundation

Evaluate the Nightreign-specific Seamless Co-op project as the preferred networking foundation.

Do not assume that its existing features automatically support the intended PvPvE game mode.

Before implementing gameplay mechanics, investigate:

* Supported game and mod versions.
* Source availability and licensing.
* Session creation and joining.
* Player synchronization.
* Existing player-count limits.
* Party membership assumptions.
* Damage and combat synchronization.
* PvE enemy synchronization.
* Death and revival behavior.
* Boss synchronization.
* Disconnect and reconnect behavior.
* Available modding hooks and extension points.
* Compatibility with other required modding tools.

Determine whether the framework can support opposing players within a shared expedition without requiring a major networking rewrite.

Distinguish between the framework's networking party and the mod's conceptual gameplay teams. They may be different things, but do not implement a separate team system until investigation establishes that it is necessary.

Prefer extending existing functionality over replacing it.

Do not modify or redistribute third-party code without verifying the applicable license and permissions.

## 7. Technical Investigation

Phase 0 must answer the following questions:

1. Can two players reliably participate in one shared Nightreign expedition?
2. Can players from separate intended teams damage each other?
3. Does PvP require changes to the underlying networking framework?
4. Are player damage, status effects, death, and revival synchronized correctly?
5. Can the game continue normal PvE progression after player combat?
6. What game hooks, modding libraries, and tooling are available?
7. What limitations would prevent expanding from two players to several teams?
8. Which technical risks must be resolved before the final-day concept is feasible?

Use the actual project documentation, source code where available, and reproducible in-game experiments.

Do not invent API names, game hooks, networking capabilities, or undocumented behavior.

If the preferred networking foundation cannot support a required feature, document the evidence and compare viable alternatives before committing to a workaround.

## 8. Development Roadmap

### Phase 0 — Reconnaissance and Feasibility

The current game and mod installation is found at: Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game

Inspect the game modding ecosystem and the candidate networking foundation.

Deliverables:

* Technical architecture summary.
* Identified tools and extension points.
* Feasibility assessment.
* Known limitations and uncertainties.
* Proposed proof-of-concept implementation.
* Acceptance tests for the next phase.

No large-scale implementation should begin before the major architectural assumptions have been investigated.

### Phase 1 — Shared-World Proof of Concept

Using two players, verify that both can:

* Join the same session.
* Exist in the same world.
* See and interact with the PvE environment.
* Engage in combat without immediately breaking the session.

Acceptance criterion: both players can participate in the same expedition environment and ordinary PvE remains functional.

### Phase 2 — Player-versus-Player Combat

Implement the smallest necessary changes to enable reliable PvP between the two players.

Validate:

* Player damage registration.
* Damage and status-effect synchronization.
* Death and revival.
* Continued PvE functionality.
* Recovery from common multiplayer errors.

Acceptance criterion: two players can fight one another in a shared expedition without critical desynchronization or progression failures.

### Phase 3 — Expedition Stability

Validate the PvP functionality across a meaningful portion of the vanilla expedition.

Test:

* Exploration and movement.
* Ordinary enemy encounters.
* Loot collection.
* Expedition objectives.
* Day transitions.
* Player death and revival.
* Disconnection and recovery.

Acceptance criterion: PvP does not consistently break the vanilla expedition loop.

### Phase 4 — Playable Prototype

Integrate the validated functionality into a repeatable testing setup.

Focus on critical defects, reproducible failures, synchronization, and progression stability.

Do not expand into large-lobby support or custom final-day gameplay until this milestone is reliable.

### Phase 5 — Future Competitive Endgame

Investigate and implement the final-day brawl and boss-or-escape choice only after the prototype is stable.

This phase requires separate feasibility work for:

* Transitioning from the ordinary expedition into the PvP endgame.
* Determining which teams participate.
* Defining victory and elimination conditions.
* Handling player deaths and disconnections.
* Determining when the winning team may escape or fight the boss.
* Reusing or modifying the existing Nightlord encounter.
* Completing the expedition consistently across clients.
* Handling rewards and completion states.

The exact implementation must be based on verified game mechanics and networking behavior.

## 9. Testing Strategy

The initial testing environment consists of two people. Do not require more participants to validate the early milestones.

Every milestone must have reproducible acceptance tests.

For each test, document:

* Preconditions.
* Steps to reproduce.
* Expected behavior.
* Actual behavior.
* Relevant logs or debugging evidence.
* Known limitations.

Prefer real in-game multiplayer testing over relying on compilation, static analysis, or isolated unit tests alone.

Use automated tests where appropriate for any custom logic that can be tested independently.

Record defects and distinguish between:

* Confirmed defects in the mod.
* Existing game behavior.
* Networking framework limitations.
* Unverified assumptions.

## 10. Repository Documentation

Maintain the following files as the project develops. In addition, `CLAUDE.md` (agent operating rules)
and `TASKS.md` (task cards: the unit of work handed to executor agents) were added on 2026-10-09.

### `README.md`

Include:

* Project overview.
* Current implementation status.
* Required tools and dependencies.
* Setup instructions.
* How to launch a test session.
* Known limitations.

### `ROADMAP.md`

Track:

* Development phases.
* Current milestone.
* Acceptance criteria.
* Completed work.
* Known blockers.
* Next implementation step.

### `TECHNICAL_NOTES.md`

Record:

* Networking architecture findings.
* Game and modding APIs.
* Relevant hooks and synchronization behavior.
* Verified technical constraints.
* Important design decisions and their rationale.

### `TESTING.md`

Record:

* Test environment.
* Reproduction steps.
* Expected behavior.
* Actual results.
* Known bugs.
* Regression tests.

### `FUTURE_CONSIDERATIONS.md`

Preserve deferred requirements and longer-term ideas, including:

* Final-day team brawl.
* Winner's choice between the final boss and escape.
* Larger player counts.
* Additional teams.
* Team identification and custom team management.
* Friendly-fire policy.
* Disconnect and reconnect handling at scale.
* Performance under higher player counts.
* Anti-exploit measures.
* Compatibility with game updates and other mods.
* Future rewards, rankings, and balancing.

These are planning notes, not implicit commitments to implement every feature.

## 11. Engineering Guidelines

* Inspect the existing repository before changing its structure.
* Work in small, reviewable increments.
* Prefer minimal changes that are easy to test and revert.
* Reuse existing game and networking functionality when practical.
* Avoid unnecessary abstractions and premature architecture.
* Keep assumptions separate from verified facts.
* Document technical decisions that affect future development.
* Do not claim success without evidence from the relevant tests.
* Do not implement deferred features without a clear reason.
* Preserve vanilla behavior unless a specific requirement justifies changing it.
* Report blockers with concrete evidence and possible alternatives.
* Do not allow speculative future features to delay the core proof of concept.

## 12. Definition of Success

The initial prototype is successful when two players can participate in the same Nightreign expedition, engage in PvP, and continue playing without critical synchronization failures or broken PvE progression.

The long-term vision is a multi-team PvPvE expedition in which the final day becomes a competitive team brawl, with the winning team choosing whether to escape or risk a final boss encounter.

The immediate objective is to prove the networking and combat foundation needed to make that vision possible.

## 13. Instructions for the Coding Agent

Start with Phase 0.

Inspect the relevant project, verify the candidate networking foundation, identify the major technical risks, and produce a concrete implementation plan for the smallest proof of concept.

Do not implement the entire mod immediately.

Do not invent missing technical details. If a decision depends on undocumented game behavior, investigate it and report the findings.

After the investigation, identify the first experiment that can conclusively validate the architecture and proceed incrementally from there.
