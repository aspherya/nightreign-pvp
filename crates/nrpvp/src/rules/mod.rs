//! Rules core: pure game-rule logic with no game-memory access and no clocks.
//!
//! - [`teams`]: deterministic team assignment from per-player ids (T-023, pure-logic part); wired in by T-026.
//! - [`matchstate`]: a deferred Battle Royale rules model (PROJECT_PLAN.md section 4B); not wired into the game.
//!
//! The match-state model is exploratory and does not establish accepted product rules. See
//! `.claude/references/project-scope-v3.md` for product intent and `.claude/PROJECT_PLAN.md` for the
//! detailed planning model. Anything not specified there is marked `ASSUMPTION:`.

// `matchstate` is not wired into the game yet (a later card); until then nothing outside its tests calls it.
#[allow(dead_code)]
pub mod matchstate;
pub mod teams;
