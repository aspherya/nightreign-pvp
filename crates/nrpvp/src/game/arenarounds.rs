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
//! - **Round time limit ([`ROUND_LIMIT_MS`], ARENA-3).** A round with no down by then goes to the side with more HP in percent of
//!   its maximum (a draw within [`DRAW_WITHIN`]: nobody scores, the round is played again). The **host** judges and publishes the
//!   verdict (`net::bus` topic `ARENA_ROUND`, key = the bout number, value 1 host won / 2 host lost / 3 draw) so both games agree;
//!   the other game takes it, or judges by itself after [`VERDICT_WAIT_MS`] more without one.
//! - **First to [`ROUNDS_TO_WIN`].** The last round ends the same way (both reset at their spawns), but the screen comes back with the
//!   players still frozen and the endings play: WIN in the winner's game (flag 10007411: banner 4000, 10 s, 7000), LOSE in the
//!   loser's (10007412: banner 4010, 10 s, 7001).
//! - **ARENA-4, against a bot.** In an Arena match against a bot ([`crate::mm::arena_bots`]) the rival is the bot the bots feature
//!   publishes ([`set_arena_bot`]): down at 0 HP, its HP in percent of the HP it had at its spawn. LOCAL judges the round limit (no
//!   session). The round events ([`bot_events`]) tell the bots feature when to park, heal, replace or remove the bot, and every fade-in
//!   that starts a round waits (at most [`RIVAL_WAIT_MS`]) until the bot stands on its spawn.

use std::sync::Mutex;
use std::time::Instant;

use crate::game::world::World;

/// Rounds a player needs to win the match (best of 3).
pub(crate) const ROUNDS_TO_WIN: u8 = 2;
/// ARENA-3: a round with no down ends after this long (PROJECT_SCOPE §Arena: 3 minutes, to be tuned).
pub(crate) const ROUND_LIMIT_MS: u64 = 180_000;
/// The non-host game waits this long past the limit for the host's verdict, then judges by itself.
pub(crate) const VERDICT_WAIT_MS: u64 = 3_000;
/// HP fractions closer than this are a draw.
pub(crate) const DRAW_WITHIN: f32 = 0.005;
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
/// ARENA-4: a round start waits at most this long for the bot on its spawn (then goes on without it).
pub(crate) const RIVAL_WAIT_MS: u64 = 30_000;
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
/// ARENA-4: while ON this game's player holds SpEffect 8970061 (event 99100919): offline a solo player at 0 HP in the arena dies at
/// once ("YOU DIED", `mi-arena-bot` 2026-11-10); with it 0 HP is a down. A down uses it up: OFF at each reset, ON at each round start.
const FLAG_NEAR_DEATH: u32 = 10_007_519;
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
    /// Fill LOCAL's ultimate (the reset before round 3).
    UltFull,
    /// The host's verdict on a timed-out bout (key, outcome from the host's side) for the other game.
    Verdict(u8, Outcome),
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

/// How a bout ended for LOCAL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Won,
    Lost,
    Draw,
}

impl Outcome {
    /// The bus value from the host's side (1 won, 2 lost, 3 draw).
    pub(crate) fn wire(self) -> u32 {
        match self {
            Outcome::Won => 1,
            Outcome::Lost => 2,
            Outcome::Draw => 3,
        }
    }

    /// The other game's outcome from the host's bus value.
    pub(crate) fn from_host_wire(v: u32) -> Option<Outcome> {
        match v {
            1 => Some(Outcome::Lost),
            2 => Some(Outcome::Won),
            3 => Some(Outcome::Draw),
            _ => None,
        }
    }
}

/// ARENA-3: what the round timer needs from the world this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Timer {
    /// This game is the session host (it judges).
    pub host: bool,
    /// `(LOCAL, rival)` HP as a fraction of each one's maximum.
    pub hp: Option<(f32, f32)>,
    /// The host's verdict for the current bout, as LOCAL's outcome.
    pub verdict: Option<Outcome>,
    /// The round limit; 0 = [`ROUND_LIMIT_MS`] (the console's `arena_limit` sets a shorter one for tests).
    pub limit_ms: u64,
    /// ARENA-4: the rival (a bot) is not on its spawn yet: a fade-in that starts a round waits (at most [`RIVAL_WAIT_MS`]).
    pub rival_missing: bool,
}

/// Console override of the round limit (ms; 0 = the default).
static LIMIT_OVERRIDE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `arena_limit [secs]`: set (0 = back to the default) or show the round limit.
pub(crate) fn limit_cmd(args: &[String]) -> Result<String, String> {
    use std::sync::atomic::Ordering;
    if let Some(a) = args.first() {
        let s: u64 = a.parse().map_err(|_| "seconds must be a number")?;
        LIMIT_OVERRIDE_MS.store(s * 1000, Ordering::Relaxed);
    }
    let v = LIMIT_OVERRIDE_MS.load(Ordering::Relaxed);
    Ok(format!(
        "arena_limit: {} s{}",
        if v == 0 { ROUND_LIMIT_MS } else { v } / 1000,
        if v == 0 {
            " (default)"
        } else {
            " (console override)"
        }
    ))
}

/// The time-limit judgement (pure): more HP in percent wins, a near tie or unknown HP is a draw.
pub(crate) fn judge(hp: Option<(f32, f32)>) -> Outcome {
    match hp {
        Some((l, r)) if (l - r).abs() >= DRAW_WITHIN => {
            if l > r {
                Outcome::Won
            } else {
                Outcome::Lost
            }
        }
        _ => Outcome::Draw,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rounds {
    pub won: u8,
    pub lost: u8,
    pub phase: Phase,
    /// When the current round's fight started (its fade-in).
    pub fight_since_ms: u64,
    /// Bouts ended so far, draws included (the verdict key).
    pub bouts: u8,
}

impl Default for Rounds {
    fn default() -> Self {
        Rounds {
            won: 0,
            lost: 0,
            phase: Phase::Off,
            fight_since_ms: 0,
            bouts: 0,
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

    /// The ultimate is held empty in rounds 1 and 2 (DECISIONS "Ultimate only in the last round"): from the start of the match until
    /// two rounds are played. Round 3 starts with it full ([`Do::UltFull`]) and leaves it alone.
    pub(crate) fn ult_held(&self) -> bool {
        !matches!(self.phase, Phase::Off | Phase::MatchOver) && self.won + self.lost < 2
    }

    /// One tick with no round timer input (pure; the tests of the down rules).
    #[cfg(test)]
    pub(crate) fn step(
        &mut self,
        now_ms: u64,
        on: bool,
        local: Seen,
        rival: Option<Seen>,
    ) -> Vec<Do> {
        self.step_timed(now_ms, on, local, rival, Timer::default())
    }

    /// One tick (pure). `on`: an Arena match is on and LOCAL stands in the arena. `rival` is `None` when this game sees no rival.
    pub(crate) fn step_timed(
        &mut self,
        now_ms: u64,
        on: bool,
        local: Seen,
        rival: Option<Seen>,
        timer: Timer,
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
                let t = now_ms.saturating_sub(at_ms);
                if t < START_FADE_IN_MS
                    || (timer.rival_missing && t < START_FADE_IN_MS + RIVAL_WAIT_MS)
                {
                    return Vec::new();
                }
                self.phase = Phase::Fighting { armed: false };
                self.fight_since_ms = now_ms;
                vec![Do::FadeIn, Do::Message("Round 1: fight!".into())]
            }
            Phase::Fighting { armed: false } => {
                if local.up && rival.up {
                    self.phase = Phase::Fighting { armed: true };
                }
                Vec::new()
            }
            Phase::Fighting { armed: true } => {
                let mut out = Vec::new();
                let (outcome, timed) = if local.down {
                    (Outcome::Lost, false)
                } else if rival.down {
                    (Outcome::Won, false)
                } else {
                    let t = now_ms.saturating_sub(self.fight_since_ms);
                    let limit = if timer.limit_ms == 0 {
                        ROUND_LIMIT_MS
                    } else {
                        timer.limit_ms
                    };
                    let o = if let Some(v) = timer.verdict {
                        v
                    } else if t < limit {
                        return Vec::new();
                    } else if timer.host {
                        let o = judge(timer.hp);
                        out.push(Do::Verdict(self.bouts + 1, o));
                        o
                    } else if t >= limit + VERDICT_WAIT_MS {
                        judge(timer.hp)
                    } else {
                        return Vec::new();
                    };
                    (o, true)
                };
                self.bouts += 1;
                match outcome {
                    Outcome::Won => self.won += 1,
                    Outcome::Lost => self.lost += 1,
                    Outcome::Draw => {}
                }
                self.phase = Phase::RoundOver {
                    lost: outcome == Outcome::Lost,
                    at_ms: now_ms,
                    stage: 0,
                };
                let what = match outcome {
                    Outcome::Won => "you won",
                    Outcome::Lost => "you lost",
                    Outcome::Draw => "draw, played again",
                };
                out.push(Do::Message(format!(
                    "Round {}: {}{what} ({}-{})",
                    self.round_no() + u8::from(outcome == Outcome::Draw),
                    if timed { "time! " } else { "" },
                    self.won,
                    self.lost
                )));
                out
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
                    if self.won + self.lost == 2 && !self.decided() {
                        out.push(Do::UltFull);
                    }
                }
                let rival_wait =
                    !self.decided() && timer.rival_missing && t < FADE_IN_MS + RIVAL_WAIT_MS;
                if stage == 2 && t >= FADE_IN_MS && !rival_wait {
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
                        self.fight_since_ms = now_ms;
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

/// ARENA-4: the types shared with the bots feature (defined there: that feature is in both builds, this module is dev only).
use crate::features::bots::fill::{ArenaBot, BotEvents};

static ARENA_BOT: Mutex<Option<ArenaBot>> = Mutex::new(None);
static BOT_EVENTS: Mutex<BotEvents> = Mutex::new(BotEvents {
    round_ends: 0,
    resets: 0,
    decided: 0,
});

/// ARENA-4 (bots feature, every tick): the bot the Arena fights, `None` = none (yet).
pub(crate) fn set_arena_bot(b: Option<ArenaBot>) {
    *lock(&ARENA_BOT) = b;
}

/// ARENA-4 (bots feature): the round events so far.
pub(crate) fn bot_events() -> BotEvents {
    *lock(&BOT_EVENTS)
}

static STATE: Mutex<Option<(Rounds, Instant)>> = Mutex::new(None);
/// SB-ARENA: LOCAL's rounds `(won, lost)` while an Arena match is on (`None` = no Arena match); the scoreboard shows them instead of
/// kills and deaths (an Arena round ends with a down, which the Battle Royale kill count never sees).
pub(crate) fn score() -> Option<(u8, u8)> {
    lock(&STATE)
        .as_ref()
        .filter(|(r, _)| r.phase != Phase::Off)
        .map(|(r, _)| (r.won, r.lost))
}

/// The host's verdicts received this match: `(bout key, LOCAL's outcome)`.
static VERDICTS: Mutex<Vec<(u32, Outcome)>> = Mutex::new(Vec::new());

/// A player's HP as a fraction of its maximum (`PlayerGameData+0x10` / `+0x14`, MEMORY_MAP §29).
fn hp_fraction(player: usize) -> Option<f32> {
    let pgd = super::mem::read::<usize>(player + crate::game::layout::PLAYER_GAMEDATA)
        .filter(|p| super::mem::is_ptr(*p))?;
    let hp = super::mem::read::<u32>(pgd + 0x10)?;
    let max = super::mem::read::<u32>(pgd + 0x14).filter(|m| *m > 0)?;
    Some(hp as f32 / max as f32)
}
/// When the last get-up write was made, and since when a reset move is waiting (main loop only).
static LAST_REVIVE: Mutex<Option<Instant>> = Mutex::new(None);
static PLACE_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// ARENA-4 (pure): whether LOCAL should hold SpEffect 8970061 ([`FLAG_NEAR_DEATH`]): in a match against a bot, from the arrival to the
/// reset after each down (the reset clears it, the next round start gives it back), never once the match is decided.
pub(crate) fn near_death_wanted(vs_bot: bool, on: bool, phase: Phase) -> bool {
    vs_bot
        && on
        && match phase {
            Phase::Starting { .. } | Phase::Fighting { .. } => true,
            Phase::RoundOver { stage, .. } => stage < 2,
            Phase::Off | Phase::MatchOver => false,
        }
}

/// The value last written to [`FLAG_NEAR_DEATH`] (main loop only; `None` = never written).
static NEAR_DEATH: Mutex<Option<bool>> = Mutex::new(None);

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
    let mut rival_addr = None;
    let vs_bot = crate::mm::arena_bots();
    for f in world.fighters.iter().flatten() {
        if f.local {
            local = Some((f.addr, Seen::of(f.state, f.hp)));
        } else if rival.is_none() && !vs_bot {
            rival = Some(Seen::of(f.state, f.hp));
            rival_addr = Some(f.addr);
        }
    }
    let Some((local_addr, local_seen)) = local else {
        return;
    };
    // ARENA-4: the bot is the rival (down at 0 HP; its HP in percent of its spawn HP for the time limit).
    let arena_bot = if vs_bot { *lock(&ARENA_BOT) } else { None };
    let bot_hp = arena_bot.and_then(|b| world.bot(b.handle).map(|c| (b, c.hp)));
    if vs_bot {
        rival = bot_hp.map(|(_, hp)| Seen::of(0, hp));
    }
    // ARENA-3: the host's verdicts on timed-out bouts (key = bout number).
    let mut verdicts = lock(&VERDICTS);
    if !on {
        verdicts.clear();
    }
    for r in crate::net::bus::take(crate::net::bus::topic::ARENA_ROUND) {
        if let Some(o) = Outcome::from_host_wire(r.fact.value) {
            tracing::info!(
                "arena_rounds: the host's verdict for bout {}: {o:?} for us",
                r.fact.key
            );
            verdicts.push((r.fact.key, o));
        }
    }
    let next_bout = u32::from(rounds.bouts) + 1;
    let rival_hp = if vs_bot {
        bot_hp
            .filter(|(b, _)| b.full_hp > 0)
            .map(|(b, hp)| (hp as f32 / b.full_hp as f32).min(1.0))
    } else {
        rival_addr.and_then(hp_fraction)
    };
    let timer = Timer {
        host: vs_bot || super::defeatdrop::is_host(base),
        hp: hp_fraction(local_addr).zip(rival_hp),
        verdict: verdicts.iter().find(|v| v.0 == next_bout).map(|v| v.1),
        limit_ms: LIMIT_OVERRIDE_MS.load(std::sync::atomic::Ordering::Relaxed),
        rival_missing: vs_bot && !bot_hp.is_some_and(|(b, hp)| b.ready && hp > 0),
    };
    drop(verdicts);
    let before = rounds.phase;
    let dos = rounds.step_timed(now_ms, on, local_seen, rival, timer);
    if !on {
        *lock(&PLACE_SINCE) = None;
    }
    if std::mem::discriminant(&rounds.phase) != std::mem::discriminant(&before) {
        if matches!(rounds.phase, Phase::RoundOver { .. }) {
            lock(&BOT_EVENTS).round_ends += 1;
        }
        tracing::info!(
            "arena_rounds: {before:?} -> {:?} (won {}, lost {})",
            rounds.phase,
            rounds.won,
            rounds.lost
        );
    }
    if before == (Phase::Fighting { armed: false })
        && rounds.phase == (Phase::Fighting { armed: true })
    {
        tracing::info!("arena_rounds: both up: a down counts from now");
    }
    // ARENA-4: offline, 0 HP is a down only with SpEffect 8970061.
    let want = near_death_wanted(vs_bot, on, rounds.phase);
    let mut nd = lock(&NEAR_DEATH);
    if *nd != Some(want) && (want || nd.is_some()) {
        match crate::game::flags::set(base, FLAG_NEAR_DEATH, want) {
            Ok(_) => {
                tracing::info!(
                    "arena_rounds: SpEffect 8970061 (0 HP downs) {}",
                    if want { "given" } else { "taken back" }
                );
                *nd = Some(want);
            }
            Err(e) => tracing::warn!("arena_rounds: flag {FLAG_NEAR_DEATH} not set: {e}"),
        }
    }
    drop(nd);
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
                // ARENA-4: the last round's reset already ends the match for the bot (no new bot behind the black screen).
                let mut ev = lock(&BOT_EVENTS);
                if rounds.decided() {
                    ev.decided += 1;
                } else {
                    ev.resets += 1;
                }
                drop(ev);
                set_flag(base, FLAG_HEAL);
                set_flag(base, FLAG_REFILL_ESTUS);
                *lock(&PLACE_SINCE) = Some(Instant::now());
                tracing::info!("arena_rounds: reset: recovery, estus, back to the spawn");
            }
            Do::UltFull => match super::ultgauge::set(local_addr, 1.0) {
                Ok((b, a)) => {
                    tracing::info!("arena_rounds: round 3: ultimate filled ({b:.1} -> {a:.1})")
                }
                Err(e) => tracing::warn!("arena_rounds: ultimate not filled: {e}"),
            },
            Do::Verdict(key, o) => {
                let sent = crate::net::bus::publish(
                    crate::net::bus::topic::ARENA_ROUND,
                    u32::from(key),
                    o.wire(),
                );
                tracing::info!(
                    "arena_rounds: bout {key} timed out: {o:?} for the host (published {sent})"
                );
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
    // The ultimate stays empty until round 3 (only written when it holds a charge).
    if on && rounds.ult_held() {
        if let Some((_, now, _)) = super::ultgauge::read(local_addr) {
            if now > 0.0 {
                match super::ultgauge::set(local_addr, 0.0) {
                    Ok((b, _)) => {
                        tracing::debug!("arena_rounds: ultimate held empty ({b:.1} -> 0)")
                    }
                    Err(e) => tracing::warn!("arena_rounds: ultimate not emptied: {e}"),
                }
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
    fn the_ultimate_is_held_in_rounds_one_and_two_and_filled_for_round_three() {
        let mut r = Rounds::default();
        assert!(!r.ult_held(), "no match");
        let mut t = start(&mut r);
        assert!(r.ult_held(), "round 1");
        // Round 1 won, round 2 lost: 1-1.
        r.step(t, true, UP, Some(DOWN));
        r.step(t + 1_000, true, UP, Some(DOWN));
        assert_eq!(r.step(t + 5_000, true, UP, Some(UP)), vec![Do::Reset]);
        r.step(t + 6_000, true, UP, Some(UP));
        r.step(t + 6_050, true, UP, Some(UP));
        assert!(r.ult_held(), "round 2");
        t += 10_000;
        r.step(t, true, DOWN, Some(UP));
        r.step(t + 1_000, true, DOWN, Some(UP));
        assert_eq!(
            r.step(t + 5_000, true, UP, Some(UP)),
            vec![Do::Reset, Do::UltFull],
            "the reset before round 3 fills it"
        );
        assert!(!r.ult_held(), "round 3 is free");
    }

    #[test]
    fn a_two_nil_match_never_fills_the_ultimate() {
        let mut r = Rounds::default();
        let mut t = start(&mut r);
        for _ in 0..2 {
            r.step(t, true, UP, Some(DOWN));
            r.step(t + 1_000, true, UP, Some(DOWN));
            assert!(!r.step(t + 5_000, true, UP, Some(UP)).contains(&Do::UltFull));
            r.step(t + 6_000, true, UP, Some(UP));
            r.step(t + 6_050, true, UP, Some(UP));
            t += 10_000;
        }
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

    fn timed(host: bool, hp: Option<(f32, f32)>, verdict: Option<Outcome>) -> Timer {
        Timer {
            host,
            hp,
            verdict,
            limit_ms: 0,
            rival_missing: false,
        }
    }

    #[test]
    fn against_a_bot_local_holds_the_near_death_speffect_from_each_round_start_to_the_reset() {
        let p = |stage| Phase::RoundOver {
            lost: true,
            at_ms: 0,
            stage,
        };
        assert!(near_death_wanted(true, true, Phase::Starting { at_ms: 0 }));
        assert!(near_death_wanted(
            true,
            true,
            Phase::Fighting { armed: false }
        ));
        assert!(
            near_death_wanted(true, true, p(1)),
            "downed: keep it until the reset"
        );
        assert!(
            !near_death_wanted(true, true, p(2)),
            "the reset takes it back"
        );
        assert!(!near_death_wanted(true, true, Phase::MatchOver));
        assert!(
            !near_death_wanted(false, true, Phase::Starting { at_ms: 0 }),
            "PvP: a session downs anyway"
        );
        assert!(!near_death_wanted(
            true,
            false,
            Phase::Fighting { armed: true }
        ));
    }

    fn missing(m: bool) -> Timer {
        Timer {
            rival_missing: m,
            ..Timer::default()
        }
    }

    #[test]
    fn the_first_fade_in_waits_for_the_bot_on_its_spawn() {
        let mut r = Rounds::default();
        assert!(r.step_timed(0, true, UP, None, missing(true)).is_empty());
        assert!(
            r.step_timed(5_000, true, UP, None, missing(true))
                .is_empty(),
            "no bot yet: still black"
        );
        assert_eq!(
            r.step_timed(5_050, true, UP, Some(UP), missing(false)),
            vec![Do::FadeIn, Do::Message("Round 1: fight!".into())]
        );
        // Never seen: the round starts anyway after the wait.
        let mut r = Rounds::default();
        r.step_timed(0, true, UP, None, missing(true));
        let end = START_FADE_IN_MS + RIVAL_WAIT_MS;
        assert!(r
            .step_timed(end - 1, true, UP, None, missing(true))
            .is_empty());
        assert_eq!(
            r.step_timed(end, true, UP, None, missing(true))[0],
            Do::FadeIn
        );
    }

    #[test]
    fn the_next_round_waits_for_the_new_bot_but_the_ending_does_not() {
        let mut r = Rounds::default();
        let t = start(&mut r);
        // The bot is down (dead): round won; at the reset the bots feature brings a new one.
        r.step_timed(t, true, UP, Some(DOWN), missing(false));
        assert_eq!(
            r.step_timed(t + 1_000, true, UP, Some(DOWN), missing(false)),
            vec![Do::FadeOut]
        );
        assert_eq!(
            r.step_timed(t + 5_000, true, UP, Some(DOWN), missing(false)),
            vec![Do::Reset]
        );
        assert!(
            r.step_timed(t + 6_000, true, UP, None, missing(true))
                .is_empty(),
            "the new bot is not on its spawn yet"
        );
        assert_eq!(
            r.step_timed(t + 7_500, true, UP, Some(UP), missing(false)),
            vec![Do::FadeIn, Do::Message("Round 2: fight!".into())]
        );
        r.step_timed(t + 7_550, true, UP, Some(UP), missing(false));
        assert_eq!(r.phase, Phase::Fighting { armed: true });
        // Round 2 won too: the ending plays without waiting for a bot.
        let t = t + 10_000;
        r.step_timed(t, true, UP, Some(DOWN), missing(false));
        r.step_timed(t + 1_000, true, UP, Some(DOWN), missing(false));
        r.step_timed(t + 5_000, true, UP, Some(DOWN), missing(false));
        assert_eq!(
            r.step_timed(t + 6_000, true, UP, None, missing(true)),
            vec![Do::FadeInFrozen, Do::Win]
        );
    }

    #[test]
    fn the_host_judges_a_timed_out_round_by_hp_percent_and_publishes_it() {
        let mut r = Rounds::default();
        start(&mut r);
        let t0 = r.fight_since_ms;
        let at = |d| t0 + d;
        assert!(r
            .step_timed(
                at(ROUND_LIMIT_MS - 1),
                true,
                UP,
                Some(UP),
                timed(true, Some((0.6, 0.4)), None)
            )
            .is_empty());
        let out = r.step_timed(
            at(ROUND_LIMIT_MS),
            true,
            UP,
            Some(UP),
            timed(true, Some((0.6, 0.4)), None),
        );
        assert_eq!(out[0], Do::Verdict(1, Outcome::Won));
        assert!(matches!(&out[1], Do::Message(m) if m.contains("time!") && m.contains("you won")));
        assert_eq!((r.won, r.lost, r.bouts), (1, 0, 1));
    }

    #[test]
    fn the_other_game_takes_the_hosts_verdict_or_judges_itself_after_the_wait() {
        let mut r = Rounds::default();
        start(&mut r);
        let t0 = r.fight_since_ms;
        // No verdict yet: waits past the limit.
        assert!(r
            .step_timed(
                t0 + ROUND_LIMIT_MS + 100,
                true,
                UP,
                Some(UP),
                timed(false, Some((0.9, 0.1)), None)
            )
            .is_empty());
        // The host's verdict wins over its own HP view.
        r.step_timed(
            t0 + ROUND_LIMIT_MS + 200,
            true,
            UP,
            Some(UP),
            timed(false, Some((0.9, 0.1)), Some(Outcome::Lost)),
        );
        assert_eq!((r.won, r.lost), (0, 1));
        let mut r = Rounds::default();
        start(&mut r);
        let t0 = r.fight_since_ms;
        r.step_timed(
            t0 + ROUND_LIMIT_MS + VERDICT_WAIT_MS,
            true,
            UP,
            Some(UP),
            timed(false, Some((0.9, 0.1)), None),
        );
        assert_eq!((r.won, r.lost), (1, 0));
    }

    #[test]
    fn a_draw_scores_nothing_and_the_round_is_played_again() {
        let mut r = Rounds::default();
        start(&mut r);
        let t0 = r.fight_since_ms;
        let out = r.step_timed(
            t0 + ROUND_LIMIT_MS,
            true,
            UP,
            Some(UP),
            timed(true, Some((0.5, 0.502)), None),
        );
        assert_eq!(out[0], Do::Verdict(1, Outcome::Draw));
        assert_eq!((r.won, r.lost, r.bouts), (0, 0, 1));
        assert!(matches!(&out[1], Do::Message(m) if m.contains("Round 1") && m.contains("draw")));
        assert_eq!(judge(None), Outcome::Draw);
        assert_eq!(
            Outcome::from_host_wire(Outcome::Won.wire()),
            Some(Outcome::Lost)
        );
    }

    #[test]
    fn a_down_before_the_limit_still_ends_the_round_normally() {
        let mut r = Rounds::default();
        start(&mut r);
        let t0 = r.fight_since_ms;
        let out = r.step_timed(
            t0 + 10_000,
            true,
            UP,
            Some(DOWN),
            timed(true, Some((0.1, 0.9)), None),
        );
        assert!(matches!(&out[0], Do::Message(m) if !m.contains("time!") && m.contains("you won")));
    }
}
