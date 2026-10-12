//! MM-2: public matchmaking queue (S9). Developer mode only, switch `mm_queue = true`.
//!
//! - [`machine`]: the pure queue state machine (`Idle -> Searching -> InQueue -> MatchReady`, plus `Leave`), written against
//!   the `QueueSteam` trait so it is unit-tested with a fake Steam.
//! - [`proto`]: the lobby data protocol (tags, roster text, ticket completeness).
//! - [`guard`]: the safety guard (Seamless Co-op loaded, no EasyAntiCheat module).
//! - [`worker`]: the glue that runs inside the Steam thread (`steam::probe`): real Steam, guard, hub commands, SC party tracking.
//!
//! The game loop and the overlay never call Steam. They only exchange plain values with the worker through this module's
//! globals: [`send`] (commands in), [`status`] (state out) and [`effective_team_size`] (the team-size override).

pub(crate) mod guard;
pub(crate) mod join;
pub(crate) mod machine;
pub(crate) mod proto;
pub(crate) mod worker;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

/// Queue mode. The mode sets the team size of the match (S9 §6/§7): Solo 1, Duo 2, Squad 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Mode {
    Solo,
    Duo,
    Squad,
}

impl Mode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Mode::Solo => "solo",
            Mode::Duo => "duo",
            Mode::Squad => "squad",
        }
    }
    pub(crate) fn parse(s: &str) -> Option<Mode> {
        match s {
            "solo" => Some(Mode::Solo),
            "duo" => Some(Mode::Duo),
            "squad" => Some(Mode::Squad),
            _ => None,
        }
    }
    /// Players per team for this mode (also the party size a ticket needs).
    pub(crate) fn team_size(self) -> usize {
        match self {
            Mode::Solo => 1,
            Mode::Duo => 2,
            Mode::Squad => 3,
        }
    }
}

/// What the overlay / hotkeys ask the worker to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cmd {
    Queue(Mode),
    Leave,
    /// MM-3 (dev buttons): one join method attempt.
    Join(join::JoinCmd),
}

/// What the overlay shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Status {
    /// One line, e.g. `Queue: solo 1/2 searching` or `MATCH READY host=...`.
    pub line: String,
    /// In some queue state other than Idle (the Leave button is enabled, the Queue buttons are not).
    pub active: bool,
    /// MM-3 (`mm_join`): show the join-method buttons.
    pub join_ui: bool,
    /// MM-3: the host's Seamless Co-op lobby id is known (buttons (a) (b) (c) usable).
    pub sc_known: bool,
    /// MM-3: we own that Seamless Co-op lobby (button (d) usable).
    pub sc_owner: bool,
}

struct Hub {
    cmds: Mutex<VecDeque<Cmd>>,
    status: Mutex<Status>,
}

/// Commands queued but not yet taken; further sends are dropped (a stuck worker must not grow memory).
const MAX_CMDS: usize = 16;

static HUB: OnceLock<Hub> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);
/// 0 = no override.
static TEAM_OVERRIDE: AtomicUsize = AtomicUsize::new(0);
/// MM-3: the local player's team_type as last read by the game loop (0 = never read). Only written while the feature is enabled.
static LOCAL_TEAM: AtomicU8 = AtomicU8::new(0);

fn hub() -> &'static Hub {
    HUB.get_or_init(|| Hub { cmds: Mutex::new(VecDeque::new()), status: Mutex::new(Status::default()) })
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Called once at startup when `dev_mode && mm_queue`. Until then every function below is inert.
pub(crate) fn enable() {
    ENABLED.store(true, Ordering::SeqCst);
}

pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// Queue a command for the worker. `false` when the queue feature is off or the queue is full.
pub(crate) fn send(c: Cmd) -> bool {
    if !enabled() {
        return false;
    }
    let mut q = lock(&hub().cmds);
    if q.len() >= MAX_CMDS {
        return false;
    }
    q.push_back(c);
    true
}

/// Worker: take everything queued.
pub(crate) fn take_cmds() -> Vec<Cmd> {
    lock(&hub().cmds).drain(..).collect()
}

/// Worker: publish the state for the overlay.
pub(crate) fn publish(s: Status) {
    *lock(&hub().status) = s;
}

/// Overlay: the latest published state.
pub(crate) fn status() -> Status {
    lock(&hub().status).clone()
}

/// Worker: set (`Some(n)`) or clear (`None`) the team-size override. Set on `MatchReady`, cleared on leave / back to Idle.
pub(crate) fn set_team_override(n: Option<usize>) {
    TEAM_OVERRIDE.store(n.unwrap_or(0), Ordering::SeqCst);
}

/// Game loop: the local player's current `team_type` (read by `TeamCtl::watch_tick`). Inert unless the matchmaking feature is on.
pub(crate) fn note_local_team(team: u8) {
    if enabled() {
        LOCAL_TEAM.store(team, Ordering::Relaxed);
    }
}

/// Worker: the local player's phase according to the last `team_type` the game loop read (`None` = never read / not 1 or 2).
pub(crate) fn local_phase() -> Option<join::Phase> {
    join::Phase::from_team(LOCAL_TEAM.load(Ordering::Relaxed))
}

/// The team size the rules must use: the match override when one is set, else the configured value. With `mm_queue` off
/// nothing ever sets the override, so this returns `configured` unchanged.
pub(crate) fn effective_team_size(configured: usize) -> usize {
    pick_team_size(configured, TEAM_OVERRIDE.load(Ordering::SeqCst))
}

pub(crate) fn pick_team_size(configured: usize, override_value: usize) -> usize {
    if override_value == 0 {
        configured
    } else {
        override_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_map_to_team_sizes_and_labels() {
        assert_eq!(
            [Mode::Solo, Mode::Duo, Mode::Squad].map(|m| (m.label(), m.team_size())),
            [("solo", 1), ("duo", 2), ("squad", 3)]
        );
        for m in [Mode::Solo, Mode::Duo, Mode::Squad] {
            assert_eq!(Mode::parse(m.label()), Some(m));
        }
        assert_eq!(Mode::parse("trio"), None);
    }

    #[test]
    fn team_size_override_applies_only_when_set() {
        assert_eq!(pick_team_size(3, 0), 3, "no override: the configured value, unchanged");
        assert_eq!(pick_team_size(1, 2), 2);
        assert_eq!(pick_team_size(3, 1), 1);
    }

    #[test]
    fn global_override_is_zero_until_set() {
        // Not set here: the global is shared with the rest of the test binary (team rule tests), so only the default is checked.
        assert_eq!(effective_team_size(2), 2);
    }

    #[test]
    fn commands_do_nothing_until_enabled_and_are_bounded() {
        // `enable` is process-global, so this is the only test that calls it.
        assert!(!enabled());
        assert!(!send(Cmd::Leave));
        enable();
        assert!(take_cmds().is_empty());
        assert!(send(Cmd::Queue(Mode::Solo)));
        assert!(send(Cmd::Leave));
        assert_eq!(take_cmds(), vec![Cmd::Queue(Mode::Solo), Cmd::Leave]);
        for _ in 0..MAX_CMDS {
            assert!(send(Cmd::Leave));
        }
        assert!(!send(Cmd::Leave));
        assert_eq!(take_cmds().len(), MAX_CMDS);
        publish(Status { line: "x".into(), active: true, ..Default::default() });
        assert_eq!(status(), Status { line: "x".into(), active: true, ..Default::default() });
    }
}
