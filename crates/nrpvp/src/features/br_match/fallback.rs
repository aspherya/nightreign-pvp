//! Our flag still ON after [`FALLBACK_MS`] = our event file is not loaded: write the game's flag instead.

use super::{Act, MatchBook, MatchOut, MatchRule, Tick, FLAG_LOSE, FLAG_WIN};

/// The game's own flags written when our event file is missing.
pub(crate) const GAME_CLEAR: u32 = 7000;
pub(crate) const GAME_OVER: u32 = 7001;
/// Our flag still ON this long after we set it = our event file is not loaded: write the game's flag ourselves.
pub(crate) const FALLBACK_MS: u64 = 1_500;

/// The rule: write the game's own win / lose flag when ours is stuck.
#[derive(Debug, Default)]
pub(crate) struct FlagFallback {
    /// Since when each of our flags (win, lose) has been ON.
    stuck: [Option<u64>; 2],
}

impl MatchRule for FlagFallback {
    fn step(&mut self, t: &Tick, _book: &mut MatchBook, out: &mut MatchOut) {
        let (now_ms, nf) = (t.now_ms, &t.nf);
        let cases = [
            (nf.our_win, FLAG_WIN, GAME_CLEAR),
            (nf.our_lose, FLAG_LOSE, GAME_OVER),
        ];
        for (slot, (on, flag, game)) in self.stuck.iter_mut().zip(cases) {
            if !on {
                *slot = None;
                continue;
            }
            let since = *slot.get_or_insert(now_ms);
            if now_ms.saturating_sub(since) >= FALLBACK_MS {
                *slot = None;
                out.lines.push(format!(
                    "night_match: our flag {flag} still ON after {FALLBACK_MS} ms (event file not loaded?): writing the game's {game}"
                ));
                out.flags.push((flag, false));
                out.acts.push(Act::GameFlag(game, true));
            }
        }
    }

    fn reset(&mut self) {
        *self = FlagFallback::default();
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;
    use crate::game::world::NightFlags;

    #[test]
    fn a_stuck_win_flag_is_written_to_the_game_after_the_fallback_delay() {
        let mut c = core();
        let nf = NightFlags {
            our_win: true,
            ..NightFlags::default()
        };
        let w = world(&[up(ME, true)], nf);
        run(&mut c, 0, &w);
        let (acts, flags, _) = run(&mut c, FALLBACK_MS, &w);
        assert_eq!(flags, vec![(FLAG_WIN, false)]);
        assert!(acts.contains(&Act::GameFlag(GAME_CLEAR, true)));
    }
}
