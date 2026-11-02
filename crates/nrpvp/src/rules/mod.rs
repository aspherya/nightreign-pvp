//! Rules core: pure game-rule logic with no game-memory access and no clocks.
//!
//! - [`teams`]: deterministic team assignment from per-player ids (T-023, pure-logic part); wired in by T-026.
//!
//! The deferred match-state model that used to live here (`matchstate.rs`) was deleted in R7-2; the shipped match rules are
//! `features/br_match/`. `git log --diff-filter=D -- crates/nrpvp/src/rules/matchstate.rs` finds it.

pub mod teams;
