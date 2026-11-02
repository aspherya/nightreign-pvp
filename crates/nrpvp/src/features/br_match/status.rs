//! The `night_status` text (console) made from the book and the parts the rules add with [`MatchRule::describe`](super::MatchRule).

use std::sync::{Mutex, PoisonError};

use super::MatchBook;
use crate::game::world::NightFlags;

static STATUS: Mutex<String> = Mutex::new(String::new());

/// What only a rule knows; each rule fills its part in [`MatchRule::describe`](super::MatchRule::describe).
#[derive(Debug, Default)]
pub(crate) struct StatusParts {
    /// [`FreeDay1`](super::free_day1::FreeDay1): the clock is slowed.
    pub free_day1: bool,
    /// [`Spectate`](super::spectate::Spectate): the watched player.
    pub spec_target: Option<u64>,
    /// [`RespawnFar`](super::respawn_far::RespawnFar): places recorded.
    pub places: usize,
}

pub(super) fn render(
    now_ms: u64,
    players: usize,
    nf: &NightFlags,
    book: &MatchBook,
    parts: &StatusParts,
) -> String {
    let out = book.seen.values().filter(|s| s.out(now_ms)).count();
    let phase = if book.match_over {
        "match over".to_string()
    } else if nf.night {
        format!("night {}", if nf.day2 { 2 } else { 1 })
    } else if parts.free_day1 && !nf.day2 {
        "free day 1".to_string()
    } else {
        format!("day {}", if nf.day2 { 2 } else { 1 })
    };
    let who = match book.decided {
        Some(d) => format!(
            ", decided (winner {})",
            d.winner.map_or("none".into(), |w| format!("{w:#X}"))
        ),
        None => String::new(),
    };
    format!(
        "{phase}{who}; {players} player(s), {} team(s) seen, {out} out; LOCAL {}; mob deaths {}; places {}",
        book.max_teams,
        match book.defeat {
            Some(d) => format!(
                "defeated by {}{}",
                d.cause.text(),
                parts
                    .spec_target
                    .map_or(String::new(), |t| format!(", watching {t:#X}"))
            ),
            None => "in".into(),
        },
        book.mob_deaths,
        parts.places
    )
}

/// Keep the text for the console (the feature's tick, once per pass).
pub(super) fn publish(status: &str) {
    let mut s = STATUS.lock().unwrap_or_else(PoisonError::into_inner);
    if *s != status {
        status.clone_into(&mut s);
    }
}

/// `night_match: <phase>; …` (console `night_status`).
pub(crate) fn status_line() -> String {
    let s = STATUS.lock().unwrap_or_else(PoisonError::into_inner);
    format!(
        "night_match: {}{}",
        if s.is_empty() {
            "not in an expedition"
        } else {
            &s
        },
        crate::game::deathhold::status_short().map_or(String::new(), |d| format!("; {d}"))
    )
}
