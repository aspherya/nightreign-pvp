//! ARENA-CLOCK: the Arena round's time left on screen, shown in LOCAL's rune counter (top right of the HUD). Game 1.03.3 / exe 1.3.3.0.
//!
//! The HUD draws the rune count from `[PlayerIns+0x5A0]+0x6C` and a plain `u32` store there changes the number on screen
//! (MEMORY_MAP §23). Runes have no use in the Arena (level 15 is the cap, nothing to buy), so while an Arena match is on the counter
//! shows the seconds left in the round ([`seconds_left`]): the limit while a round has not started yet, counting down while the
//! round is fought, frozen at the down until the reset behind the black screen, then the full limit again for the next round. The
//! rune count from before the clock's first write is put back when the match is over (the endings) or ends, if LOCAL's
//! `PlayerGameData` is still the same. Console `arena_clock [0|1]`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use super::arenarounds::{Phase, Rounds};

/// `PlayerGameData` -> rune count (MEMORY_MAP §23).
const RUNES: usize = super::runeshare::GAMEDATA_RUNES;

/// The clock is shown (console `arena_clock`; on by default).
static ON: AtomicBool = AtomicBool::new(true);
/// LOCAL's `PlayerGameData` and its rune count from before the clock's first write.
static SAVED: Mutex<Option<(usize, u32)>> = Mutex::new(None);

fn ceil_secs(ms: u64) -> u32 {
    ms.div_ceil(1000).min(u64::from(u32::MAX)) as u32
}

/// The seconds the clock shows (pure); `None` = no clock (no Arena match, or the match is over).
pub(crate) fn seconds_left(r: &Rounds, now_ms: u64, limit_ms: u64) -> Option<u32> {
    match r.phase {
        Phase::Off | Phase::MatchOver => None,
        Phase::Starting { .. } => Some(ceil_secs(limit_ms)),
        Phase::Fighting { .. } => Some(ceil_secs(
            limit_ms.saturating_sub(now_ms.saturating_sub(r.fight_since_ms)),
        )),
        // Frozen at the down until the reset (stage 2), then the next round's full limit.
        Phase::RoundOver { at_ms, stage, .. } => Some(if stage >= 2 {
            ceil_secs(limit_ms)
        } else {
            ceil_secs(limit_ms.saturating_sub(at_ms.saturating_sub(r.fight_since_ms)))
        }),
    }
}

fn pgd_of(local: usize) -> Option<usize> {
    super::mem::read::<usize>(local + super::layout::PLAYER_GAMEDATA)
        .filter(|p| super::mem::is_ptr(*p))
}

fn write_runes(pgd: usize, v: u32) -> Result<(), &'static str> {
    super::mem::write_bytes(pgd + RUNES, &v.to_le_bytes())
}

/// Main loop, every tick of `arenarounds::tick`: show `secs` in LOCAL's rune counter, or put the old count back when `None`.
pub(crate) fn show(local: usize, secs: Option<u32>) {
    let pgd = pgd_of(local);
    let mut saved = SAVED.lock().unwrap_or_else(|e| e.into_inner());
    let secs = secs.filter(|_| ON.load(Ordering::Relaxed));
    let Some(secs) = secs else {
        if let Some((was_pgd, runes)) = saved.take() {
            if pgd == Some(was_pgd) {
                match write_runes(was_pgd, runes) {
                    Ok(()) => tracing::info!("arena_clock: off; rune count {runes} put back"),
                    Err(e) => tracing::warn!("arena_clock: rune count not put back: {e}"),
                }
            } else {
                tracing::info!(
                    "arena_clock: off; LOCAL's PlayerGameData changed, rune count left alone"
                );
            }
        }
        return;
    };
    let Some(pgd) = pgd else {
        return;
    };
    let Some(now) = super::mem::read::<u32>(pgd + RUNES) else {
        return;
    };
    if saved.is_none() {
        *saved = Some((pgd, now));
        tracing::info!(
            "arena_clock: on; rune count {now} kept, the counter shows the round's seconds left"
        );
    }
    if now != secs {
        if let Err(e) = write_runes(pgd, secs) {
            tracing::debug!("arena_clock: write failed: {e}");
        }
    }
}

/// Console `arena_clock [0|1]`: show or hide the round clock (no argument: the state).
pub(crate) fn cmd(args: &[String]) -> Result<String, String> {
    if let Some(a) = args.first() {
        match a.as_str() {
            "0" => ON.store(false, Ordering::Relaxed),
            "1" => ON.store(true, Ordering::Relaxed),
            _ => return Err("arena_clock: 0 or 1".into()),
        }
    }
    Ok(format!(
        "arena_clock: {} (the Arena round's seconds left in the rune counter)",
        if ON.load(Ordering::Relaxed) {
            "on"
        } else {
            "off"
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: u64 = 180_000;

    fn rounds(phase: Phase, fight_since_ms: u64) -> Rounds {
        Rounds {
            phase,
            fight_since_ms,
            ..Rounds::default()
        }
    }

    #[test]
    fn no_clock_outside_a_match_or_after_it() {
        assert_eq!(seconds_left(&rounds(Phase::Off, 0), 5, LIMIT), None);
        assert_eq!(seconds_left(&rounds(Phase::MatchOver, 0), 5, LIMIT), None);
    }

    #[test]
    fn the_full_limit_shows_before_the_first_round() {
        let r = rounds(Phase::Starting { at_ms: 0 }, 0);
        assert_eq!(seconds_left(&r, 500, LIMIT), Some(180));
    }

    #[test]
    fn a_fought_round_counts_down_and_stops_at_zero() {
        let r = rounds(Phase::Fighting { armed: true }, 10_000);
        assert_eq!(seconds_left(&r, 10_000, LIMIT), Some(180));
        assert_eq!(seconds_left(&r, 10_001, LIMIT), Some(180));
        assert_eq!(seconds_left(&r, 11_000, LIMIT), Some(179));
        assert_eq!(seconds_left(&r, 10_000 + LIMIT - 1, LIMIT), Some(1));
        assert_eq!(seconds_left(&r, 10_000 + LIMIT, LIMIT), Some(0));
        assert_eq!(seconds_left(&r, 10_000 + LIMIT + 3_000, LIMIT), Some(0));
    }

    #[test]
    fn the_clock_freezes_at_the_down_then_shows_the_next_rounds_limit_from_the_reset() {
        let over = |stage| {
            rounds(
                Phase::RoundOver {
                    lost: false,
                    at_ms: 70_000,
                    stage,
                },
                10_000,
            )
        };
        assert_eq!(seconds_left(&over(0), 72_000, LIMIT), Some(120));
        assert_eq!(seconds_left(&over(1), 75_000, LIMIT), Some(120));
        assert_eq!(seconds_left(&over(2), 76_000, LIMIT), Some(180));
    }

    #[test]
    fn a_shorter_test_limit_is_used() {
        let r = rounds(Phase::Fighting { armed: false }, 0);
        assert_eq!(seconds_left(&r, 0, 30_000), Some(30));
    }
}
