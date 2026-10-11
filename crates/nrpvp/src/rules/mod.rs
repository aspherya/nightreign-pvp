//! Rules core: pure game-rule logic with no game-memory access and no clocks.
//!
//! - [`teams`]: deterministic team assignment from per-player ids (T-023, pure-logic part); wired in by T-026.
//! - [`matchstate`]: the scope-v2 match state machine (PROJECT_PLAN.md section 4B).
//!
//! Rule sources: `.claude/references/project-scope-v2.md` (quoted in doc comments) and
//! `.claude/PROJECT_PLAN.md`. Anything not stated there is marked `ASSUMPTION:`.

// `matchstate` is not wired into the game yet (a later card); until then nothing outside its tests calls it.
#[allow(dead_code)]
pub mod matchstate;
pub mod teams;
