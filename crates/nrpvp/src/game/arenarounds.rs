//! ARENA-2 (dev only): best-of-3 rounds of a 1v1 Arena match (TASKS ARENA-2, PROJECT_SCOPE §Arena, DECISIONS "Arena details").
//! Game 1.03.3 / exe 1.3.3.0.
//!
//! Each game runs the same rules on what it sees and acts only on its own player and its own screen:
//! - **Start.** The screen is black from the arrival (`arenacard`); [`START_FADE_IN_MS`] after LOCAL is placed on its spawn it fades
//!   in and round 1 starts.
//! - **A round ends with a down.** LOCAL downed (`PlayerIns+0x710` = 1, or HP 0) = round lost; the rival downed = round won. While
//!   an Arena match is on, the Battle Royale rules, the rival re-down rule and the revive block stand aside
//!   ([`crate::mm::arena_match`]); the downed rule stays, so a rival's hit never revives a downed player.
//! - **Between rounds, behind a black screen.** [`FADE_OUT_MS`] after the down the screen fades to black and the player is frozen
//!   (Fade to Black 2004:77). [`REVIVE_AFTER_MS`] after the down the loser's game asks the game's own get-up (`+0x732` = 1, once a
//!   second until it is up; MEMORY_MAP §6). At [`RESET_AFTER_MS`] each game restores its player (Bonfire-like Recovery 2004:47 on flag
//!   10007410, Refill Estus 2004:87 on flag 10007514; the ultimate gauge is not touched) and puts it on its spawn
//!   ([`super::arenacard::place`]). At [`FADE_IN_MS`] the screen comes back, the player is free, the next round starts. A round counts
//!   a down only once both players have been seen up.
//! - **First to [`ROUNDS_TO_WIN`].** The last round ends the same way (both reset at their spawns), but the screen comes back with the
//!   players still frozen and the endings play: WIN in the winner's game (flag 10007411: banner 4000, 10 s, 7000), LOSE in the
//!   loser's (10007412: banner 4010, 10 s, 7001).

use std::sync::Mutex;
use std::time::Instant;

use crate::game::world::World;

/// Rounds a player needs to win the match (best of 3).
pub(crate) const ROUNDS_TO_WIN: u8 = 2;
/// From LOCAL placed on its spawn to the first fade-in (the move and the camera turn are done by then).
pub(crate) const START_FADE_IN_MS: u64 = 1_000;
/// From the down: fade to black (0.5 s) and freeze.
pub(crate) const FADE_OUT_MS: u64 = 1_000;
/// From the down: the loser's get-up.
pub(crate) const REVIVE_AFTER_MS: u64 = 2_000;
/// From the down: the reset at the spawns (the get-up animation, ~3.5 s, has about finished).
pub(crate) const RESET_AFTER_MS: u64 = 5_000;
/// From the down: the screen comes back.
pub(crate) const FADE_IN_MS: u64 = 6_000;
/// One get-up write per this long: the byte is cleared every frame, but a write every tick showed a burst of the game's own downed
/// messages (Leonardo, 2026-11-08); one write started the get-up within ~1 s.
const REVIVE_EVERY_MS: u64 = 1_000;
/// A reset move refused (a tilted rotation mid-attack) is retried every tick, then written over the rotation after this long.
const PLACE_FORCE_AFTER_MS: u64 = 1_000;

/// Our per-game event flags (`gamedata/emevd-nights.json`, `gamedata/emevd-arena.json`).
const FLAG_HEAL: u32 = 10_007_410;
const FLAG_REFILL_ESTUS: u32 = 10_007_514;
const FLAG_WIN: u32 = 10_007_411;
const FLAG_LOSE: u32 = 10_007_412;
const FLAG_FADE_OUT: u32 = 10_007_515;
const FLAG_FADE_IN: u32 = 10_007_516;
const FLAG_FADE_IN_FROZEN: u32 = 10_007_517;
/// `PlayerIns+0x732`: "revive completed"; the game starts the get-up while it is not 0 (re/MEMORY_MAP.md §6).
const PLAYER_REVIVE_DONE: usize = crate::game::reviveblock::PLAYER_REVIVE_DONE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// No Arena match, or LOCAL not on its spawn yet.
    Off,
    /// LOCAL stands on its spawn behind the black screen since `at_ms`.
    Starting { at_ms: u64 },
    /// A round is on; `armed` once both players have been seen up (a down before that is the last round's).
    Fighting { armed: bool },
    /// The round ended with a down at `at_ms`; `lost` = LOCAL went down; `stage` counts the steps done (fade out, reset).
    RoundOver { lost: bool, at_ms: u64, stage: u8 },
    /// The match is decided; nothing more until the match is over.
    MatchOver,
}

/// What the main loop does (pure output of [`Rounds::step`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Do {
    Message(String),
    FadeOut,
    FadeIn,
    FadeInFrozen,
    Revive,
    Reset,
    Win,
    Lose,
}

/// One player as the rules see it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Seen {
    pub down: bool,
    pub up: bool,
}

impl Seen {
    pub(crate) fn of(state: u8, hp: u32) -> Seen {
        Seen {
            down: state == 1 || hp == 0,
            up: state == 0 && hp > 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rounds {
    pub won: u8,
    pub lost: u8,
    pub phase: Phase,
}

impl Default for Rounds {
    fn default() -> Self {
        Rounds {
            won: 0,
            lost: 0,
            phase: Phase::Off,
        }
    }
}

impl Rounds {
    fn round_no(&self) -> u8 {
        self.won + self.lost
    }

    fn decided(&self) -> bool {
        self.won >= ROUNDS_TO_WIN || self.lost >= ROUNDS_TO_WIN
    }

    /// One tick (pure). `on`: an Arena match is on and LOCAL stands in the arena. `rival` is `None` when this game sees no rival.
    pub(crate) fn step(
        &mut self,
        now_ms: u64,
        on: bool,
        local: Seen,
        rival: Option<Seen>,
    ) -> Vec<Do> {
        if !on {
            *self = Rounds::default();
            return Vec::new();
        }
        let rival = rival.unwrap_or_default();
        match self.phase {
            Phase::Off => {
                self.phase = Phase::Starting { at_ms: now_ms };
                Vec::new()
            }
            Phase::Starting { at_ms } => {
                if now_ms.saturating_sub(at_ms) < START_FADE_IN_MS {
                    return Vec::new();
                }
                self.phase = Phase::Fighting { armed: false };
                vec![Do::FadeIn, Do::Message("Round 1: fight!".into())]
            }
            Phase::Fighting { armed: false } => {
                if local.up && rival.up {
                    self.phase = Phase::Fighting { armed: true };
                }
                Vec::new()
            }
            Phase::Fighting { armed: true } => {
                let lost = if local.down {
                    true
                } else if rival.down {
                    false
                } else {
                    return Vec::new();
                };
                if lost {
                    self.lost += 1;
                } else {
                    self.won += 1;
                }
                self.phase = Phase::RoundOver {
                    lost,
                    at_ms: now_ms,
                    stage: 0,
                };
                vec![Do::Message(format!(
                    "Round {}: you {} ({}-{})",
                    self.round_no(),
                    if lost { "lost" } else { "won" },
                    self.won,
                    self.lost
                ))]
            }
            Phase::RoundOver { lost, at_ms, stage } => {
                let t = now_ms.saturating_sub(at_ms);
                let mut out = Vec::new();
                let mut stage = stage;
                if stage == 0 && t >= FADE_OUT_MS {
                    stage = 1;
                    out.push(Do::FadeOut);
                }
                if stage == 1
                    && lost
                    && (REVIVE_AFTER_MS..RESET_AFTER_MS).contains(&t)
                    && local.down
                {
                    out.push(Do::Revive);
                }
                if stage == 1 && t >= RESET_AFTER_MS {
                    stage = 2;
                    out.push(Do::Reset);
                }
                if stage == 2 && t >= FADE_IN_MS {
                    if self.decided() {
                        self.phase = Phase::MatchOver;
                        out.push(Do::FadeInFrozen);
                        out.push(if self.won >= ROUNDS_TO_WIN {
                            Do::Win
                        } else {
                            Do::Lose
                        });
                    } else {
                        self.phase = Phase::Fighting { armed: false };
                        out.push(Do::FadeIn);
                        out.push(Do::Message(format!(
                            "Round {}: fight!",
                            self.round_no() + 1
                        )));
                    }
                    return out;
                }
                self.phase = Phase::RoundOver { lost, at_ms, stage };
                out
            }
            Phase::MatchOver => Vec::new(),
        }
    }
}

static STATE: Mutex<Option<(Rounds, Instant)>> = Mutex::new(None);
/// When the last get-up write was made, and since when a reset move is waiting (main loop only).
static LAST_REVIVE: Mutex<Option<Instant>> = Mutex::new(None);
static PLACE_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn set_flag(base: usize, flag: u32) {
    if let Err(e) = crate::game::flags::set(base, flag, true) {
        tracing::warn!("arena_rounds: flag {flag} not set: {e}");
    }
}

/// Main loop, every tick (after `arenacard::tick`).
pub(crate) fn tick(base: usize, world: &World) {
    let on = crate::mm::arena_match() && super::arenacard::placed();
    let mut st = lock(&STATE);
    let (rounds, start) = st.get_or_insert_with(|| (Rounds::default(), Instant::now()));
    let now_ms = start.elapsed().as_millis() as u64;
    let mut local = None;
    let mut rival = None;
    for f in world.fighters.iter().flatten() {
        if f.local {
            local = Some((f.addr, Seen::of(f.state, f.hp)));
        } else if rival.is_none() {
            rival = Some(Seen::of(f.state, f.hp));
        }
    }
    let Some((local_addr, local_seen)) = local else {
        return;
    };
    let before = rounds.phase;
    let dos = rounds.step(now_ms, on, local_seen, rival);
    if !on {
        *lock(&PLACE_SINCE) = None;
    }
    if std::mem::discriminant(&rounds.phase) != std::mem::discriminant(&before) {
        tracing::info!(
            "arena_rounds: {before:?} -> {:?} (won {}, lost {})",
            rounds.phase,
            rounds.won,
            rounds.lost
        );
    }
    for d in dos {
        match d {
            Do::Message(m) => {
                tracing::info!("arena_rounds: {m}");
                crate::game::gametoast::show(&m);
            }
            Do::FadeOut => set_flag(base, FLAG_FADE_OUT),
            Do::FadeIn => set_flag(base, FLAG_FADE_IN),
            Do::FadeInFrozen => set_flag(base, FLAG_FADE_IN_FROZEN),
            Do::Revive => {
                let mut last = lock(&LAST_REVIVE);
                if last.is_none_or(|t| t.elapsed().as_millis() as u64 >= REVIVE_EVERY_MS) {
                    *last = Some(Instant::now());
                    match super::mem::write_bytes(local_addr + PLAYER_REVIVE_DONE, &[1]) {
                        Ok(()) => tracing::info!("arena_rounds: get-up asked"),
                        Err(e) => tracing::warn!("arena_rounds: get-up write failed: {e}"),
                    }
                }
            }
            Do::Reset => {
                set_flag(base, FLAG_HEAL);
                set_flag(base, FLAG_REFILL_ESTUS);
                *lock(&PLACE_SINCE) = Some(Instant::now());
                tracing::info!("arena_rounds: reset: recovery, estus, back to the spawn");
            }
            Do::Win | Do::Lose => {
                set_flag(base, if d == Do::Win { FLAG_WIN } else { FLAG_LOSE });
                tracing::info!(
                    "arena_rounds: match over, {} {}-{}",
                    if d == Do::Win { "WIN" } else { "LOSE" },
                    rounds.won,
                    rounds.lost
                );
            }
        }
    }
    let mut pending = lock(&PLACE_SINCE);
    if let Some(t) = *pending {
        let force = t.elapsed().as_millis() as u64 >= PLACE_FORCE_AFTER_MS;
        if super::arenacard::place(base, force) || force {
            *pending = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: Seen = Seen {
        down: false,
        up: true,
    };
    const DOWN: Seen = Seen {
        down: true,
        up: false,
    };

    /// Off -> Starting -> fade in, round 1, armed; returns the next free time.
    fn start(r: &mut Rounds) -> u64 {
        assert!(r.step(0, true, UP, Some(UP)).is_empty());
        assert!(r.step(500, true, UP, Some(UP)).is_empty(), "still black");
        assert_eq!(
            r.step(1_000, true, UP, Some(UP)),
            vec![Do::FadeIn, Do::Message("Round 1: fight!".into())]
        );
        assert!(r.step(1_050, true, UP, Some(UP)).is_empty());
        assert_eq!(r.phase, Phase::Fighting { armed: true });
        2_000
    }

    #[test]
    fn a_player_reads_down_from_the_state_byte_or_zero_hp() {
        assert_eq!(Seen::of(0, 240), UP);
        assert_eq!(Seen::of(1, 0), DOWN);
        assert_eq!(Seen::of(1, 50), DOWN);
        assert_eq!(Seen::of(0, 0), DOWN);
        assert_eq!(
            Seen::of(2, 120),
            Seen {
                down: false,
                up: false
            },
            "getting up: neither"
        );
    }

    #[test]
    fn a_round_end_fades_out_resets_and_fades_back_in() {
        let mut r = Rounds::default();
        let t = start(&mut r);
        assert_eq!(
            r.step(t, true, UP, Some(DOWN)),
            vec![Do::Message("Round 1: you won (1-0)".into())]
        );
        assert!(r.step(t + 900, true, UP, Some(DOWN)).is_empty());
        assert_eq!(r.step(t + 1_000, true, UP, Some(DOWN)), vec![Do::FadeOut]);
        assert!(
            r.step(t + 2_500, true, UP, Some(DOWN)).is_empty(),
            "the winner never revives"
        );
        assert_eq!(r.step(t + 5_000, true, UP, Some(UP)), vec![Do::Reset]);
        assert!(r.step(t + 5_500, true, UP, Some(UP)).is_empty());
        assert_eq!(
            r.step(t + 6_000, true, UP, Some(UP)),
            vec![Do::FadeIn, Do::Message("Round 2: fight!".into())]
        );
    }

    #[test]
    fn the_loser_gets_up_behind_the_black_screen() {
        let mut r = Rounds::default();
        let t = start(&mut r);
        r.step(t, true, DOWN, Some(UP));
        assert_eq!((r.won, r.lost), (0, 1));
        assert_eq!(r.step(t + 1_000, true, DOWN, Some(UP)), vec![Do::FadeOut]);
        assert!(
            r.step(t + 1_900, true, DOWN, Some(UP)).is_empty(),
            "not before 2 s"
        );
        assert_eq!(r.step(t + 2_000, true, DOWN, Some(UP)), vec![Do::Revive]);
        assert_eq!(
            r.step(t + 2_050, true, DOWN, Some(UP)),
            vec![Do::Revive],
            "asked while down (the loop spaces the writes)"
        );
        assert!(
            r.step(t + 3_000, true, Seen::default(), Some(UP))
                .is_empty(),
            "getting up"
        );
        assert_eq!(r.step(t + 5_000, true, UP, Some(UP)), vec![Do::Reset]);
    }

    #[test]
    fn the_last_round_resets_too_then_the_ending_plays_with_the_players_frozen() {
        let mut r = Rounds::default();
        let mut t = start(&mut r);
        for round in 1..=2u8 {
            r.step(t, true, UP, Some(DOWN));
            r.step(t + 1_000, true, UP, Some(DOWN));
            assert_eq!(
                r.step(t + 5_000, true, UP, Some(UP)),
                vec![Do::Reset],
                "round {round}"
            );
            if round == 1 {
                r.step(t + 6_000, true, UP, Some(UP));
                r.step(t + 6_050, true, UP, Some(UP));
                t += 10_000;
            }
        }
        assert_eq!((r.won, r.lost), (2, 0));
        assert_eq!(
            r.step(t + 6_000, true, UP, Some(UP)),
            vec![Do::FadeInFrozen, Do::Win]
        );
        assert_eq!(r.phase, Phase::MatchOver);
        assert!(
            r.step(t + 9_000, true, UP, Some(DOWN)).is_empty(),
            "decided once"
        );
    }

    #[test]
    fn two_lost_rounds_lose() {
        let mut r = Rounds::default();
        let mut t = start(&mut r);
        let mut last = Vec::new();
        for _ in 0..2 {
            r.step(t, true, DOWN, Some(UP));
            r.step(t + 1_000, true, DOWN, Some(UP));
            r.step(t + 5_000, true, UP, Some(UP));
            last = r.step(t + 6_000, true, UP, Some(UP));
            r.step(t + 6_050, true, UP, Some(UP));
            t += 10_000;
        }
        assert_eq!(last, vec![Do::FadeInFrozen, Do::Lose]);
        assert_eq!(r.phase, Phase::MatchOver);
    }

    #[test]
    fn a_down_left_over_from_the_last_round_does_not_count() {
        let mut r = Rounds::default();
        let t = start(&mut r);
        r.step(t, true, UP, Some(DOWN));
        r.step(t + 1_000, true, UP, Some(DOWN));
        r.step(t + 5_000, true, UP, Some(DOWN));
        r.step(t + 6_000, true, UP, Some(DOWN));
        assert_eq!(r.phase, Phase::Fighting { armed: false });
        assert!(
            r.step(t + 6_100, true, UP, Some(DOWN)).is_empty(),
            "the rival still reads down: not armed"
        );
        assert_eq!(r.won, 1);
    }

    #[test]
    fn the_end_of_the_match_clears_the_score() {
        let mut r = Rounds::default();
        let t = start(&mut r);
        r.step(t, true, UP, Some(DOWN));
        assert!(r.step(t + 100, false, UP, Some(UP)).is_empty());
        assert_eq!(r, Rounds::default());
    }
}
