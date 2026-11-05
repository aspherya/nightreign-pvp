//! UI-2: matchmaking status as the game's own on-screen messages (no ImGui for players).
//!
//! Each message `k` is one event appended to the Roundtable's event script (`gamedata/emevd-events.json`): it waits for flag
//! [`FLAG_BASE`]` + k`, shows Display Blinking Message (2007:4, the kind Leonardo could read in the UI-2 spike) with text
//! [`TEXT_BASE`]` + k` (EventTextForMap_dlc01, `gamedata/fmg-entries.json`), clears the flag and restarts. This module maps the queue
//! state to `k` ([`for_state`], pure) and a thread sets the flag through `game::flags` (our group 10007 only):
//! - at most one message every [`MIN_GAP`] (about one blinking text; the game may drop a new one while one is showing, UI-2 solo run);
//! - a message for an ongoing state ([`repeats`]: searching, waiting, everyone is here...) is shown again every [`REPEAT_EVERY`] while
//!   it is still the current one, so the status never disappears for long; the worker posts `None` when no message applies;
//! - [`READBACK_AFTER`] after setting a flag it is read back: OFF = our event ran; still ON = no event picked it up (e.g. outside the
//!   Roundtable, where our events do not exist): logged and cleared so it cannot show up stale later.
//!
//! The tables here and the two JSON files are checked against each other by a unit test. Log lines start with `ui_msg:`.

use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Flag of message 0 (our group 10007; 10007200..=10007299 are message flags).
pub(crate) const FLAG_BASE: u32 = 10_007_200;
/// Text (EventTextForMap_dlc01) and event id of message 0.
pub(crate) const TEXT_BASE: u32 = 99_100_500;
const MIN_GAP: Duration = Duration::from_secs(6);
/// Leonardo (2026-10-20): repeat as soon as the previous blinking text is gone, so the player never feels stuck.
const REPEAT_EVERY: Duration = MIN_GAP;
const READBACK_AFTER: Duration = Duration::from_secs(1);

pub(crate) const SEARCHING: u32 = 0;
pub(crate) const STOPPED: u32 = 1;
pub(crate) const LEFT_ROUNDTABLE: u32 = 2;
pub(crate) const MISSED: u32 = 3;
pub(crate) const MATCH_FOUND: u32 = 4;
pub(crate) const HOST_INVITE: u32 = 5;
pub(crate) const WAITING_OTHERS: u32 = 6;
pub(crate) const WAITING_HOST: u32 = 7;
pub(crate) const JOINING: u32 = 8;
pub(crate) const LOADING_IN: u32 = 9;
pub(crate) const ALL_HERE_HOST: u32 = 10;
pub(crate) const ALL_HERE_JOINER: u32 = 11;
pub(crate) const SOME_MISSING: u32 = 12;
pub(crate) const PROBLEM: u32 = 13;
pub(crate) const STARTING: u32 = 14;
/// First "Searching for players: N of M" message (M = 2..=6, N = 1..=M; see [`count_msg`]).
pub(crate) const COUNT_FIRST: u32 = 20;
/// Number of messages (k < COUNT).
pub(crate) const COUNT: u32 = 40;

/// "Searching for players: `n` of `m`" (pure). `None` outside 1..=m, 2..=6.
pub(crate) fn count_msg(n: u32, m: u32) -> Option<u32> {
    if !(2..=6).contains(&m) || n == 0 || n > m {
        return None;
    }
    // Messages for m = 2 start at COUNT_FIRST; each m takes m slots.
    let before: u32 = (2..m).sum();
    Some(COUNT_FIRST + before + (n - 1))
}

/// The message for one of the gate's player lines (`mm::machine::gate`, pure). `None` = no message (e.g. "In match").
pub(crate) fn for_gate_line(line: &str) -> Option<u32> {
    if line.is_empty() || line == "In match" {
        None
    } else if line == "The match is starting" {
        Some(STARTING)
    } else if line.starts_with("Everyone is here - host") {
        Some(ALL_HERE_HOST)
    } else if line.starts_with("Everyone is here - waiting") {
        Some(ALL_HERE_JOINER)
    } else if line.contains("did not arrive: press")
        || line.starts_with("Match found! Press Matchmaking Settings")
    {
        Some(HOST_INVITE)
    } else if line.contains("you can start;") {
        Some(SOME_MISSING)
    } else if line.contains("to load in") || line == "Match found! Loading in..." {
        Some(LOADING_IN)
    } else if line.starts_with("Match found! Waiting for players")
        || line.starts_with("Match found! In the host's lobby")
    {
        Some(WAITING_OTHERS)
    } else if line == "Match found! Waiting for the host..." {
        Some(WAITING_HOST)
    } else if line == "Match found! Joining the host..." {
        Some(JOINING)
    } else {
        Some(MATCH_FOUND)
    }
}

/// What the queue state asks for (pure; the machine fills the inputs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum QueueView {
    Idle {
        error: Option<String>,
        left_note: Option<String>,
    },
    Searching,
    InQueue {
        members: u32,
        need: u32,
    },
    Matched {
        gate_line: String,
    },
}

/// The message for a queue view, given the previous one (pure). Stopping is a transition: active → idle without a note or error.
pub(crate) fn for_state(prev: Option<&QueueView>, cur: &QueueView) -> Option<u32> {
    match cur {
        QueueView::Idle { error, left_note } => {
            let was_active = prev.is_some_and(|p| !matches!(p, QueueView::Idle { .. }));
            if !was_active {
                return None;
            }
            let was_searching =
                prev.is_some_and(|p| matches!(p, QueueView::Searching | QueueView::InQueue { .. }));
            match (error, left_note) {
                (Some(e), _) if e.contains("missed") => Some(MISSED),
                (Some(_), _) => Some(PROBLEM),
                (None, Some(_)) => Some(LEFT_ROUNDTABLE),
                // A finished match also goes back to idle: only a search that ends without a reason is "stopped".
                (None, None) if was_searching => Some(STOPPED),
                (None, None) => None,
            }
        }
        QueueView::Searching => Some(SEARCHING),
        QueueView::InQueue { members, need } => count_msg(*members, *need).or(Some(SEARCHING)),
        QueueView::Matched { gate_line } => {
            for_gate_line(gate_line).or_else(|| (gate_line.is_empty()).then_some(MATCH_FOUND))
        }
    }
}

/// Whether message `k` describes an ongoing state and is shown again every [`REPEAT_EVERY`] while current (pure).
pub(crate) fn repeats(k: u32) -> bool {
    matches!(
        k,
        SEARCHING
            | HOST_INVITE
            | WAITING_OTHERS
            | WAITING_HOST
            | ALL_HERE_HOST
            | ALL_HERE_JOINER
            | SOME_MISSING
            | STARTING
    ) || (COUNT_FIRST..COUNT).contains(&k)
}

/// The sender's state between ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Sched {
    /// The current message (the worker's latest), if any.
    pub current: Option<u32>,
    /// A message waiting to be shown (newest wins).
    pub pending: Option<u32>,
    /// When the last message was shown.
    pub last_sent: Option<Instant>,
}

/// What to show now, if anything (pure): a pending message once [`MIN_GAP`] has passed, else the current message again when it
/// [`repeats`] and [`REPEAT_EVERY`] has passed.
pub(crate) fn next_to_show(s: &Sched, now: Instant) -> Option<u32> {
    let since = s.last_sent.map(|t| now.saturating_duration_since(t));
    if since.is_some_and(|d| d < MIN_GAP) {
        return None;
    }
    if let Some(k) = s.pending {
        return Some(k);
    }
    match (s.current, since) {
        (Some(k), Some(d)) if repeats(k) && d >= REPEAT_EVERY => Some(k),
        _ => None,
    }
}

struct Hub {
    /// `Some(update)` = the worker posted since the last tick; the update is the new current message or `None`.
    posted: Mutex<Option<Option<u32>>>,
}

static HUB: OnceLock<Hub> = OnceLock::new();

/// Worker: the message for the new queue state (`None` = no message; stops any repeat). No-op until [`spawn`] ran.
pub(crate) fn post(k: Option<u32>) {
    if let Some(h) = HUB.get() {
        *h.posted.lock().unwrap_or_else(PoisonError::into_inner) = Some(k);
    }
}

/// Start the sender (`ui_messages`; needs the game-data package, default launch profile / player zip). `base`: `nightreign.exe`.
pub(crate) fn spawn(base: usize) {
    let hub = HUB.get_or_init(|| Hub {
        posted: Mutex::new(None),
    });
    let r = std::thread::Builder::new().name("nrpvp-uimsg".into()).spawn(move || {
        tracing::info!(
            "ui_msg: sender started (flags {FLAG_BASE}..{}, texts {TEXT_BASE}.., gap {} s, repeat {} s)",
            FLAG_BASE + COUNT - 1,
            MIN_GAP.as_secs(),
            REPEAT_EVERY.as_secs()
        );
        let mut cleared = false;
        let mut sched = Sched::default();
        // (message, flag set at) waiting for its read-back.
        let mut check: Option<(u32, Instant)> = None;
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let now = Instant::now();
            if let Some(update) = hub.posted.lock().unwrap_or_else(PoisonError::into_inner).take() {
                sched.current = update;
                sched.pending = update;
            }
            // Clear our message flags once the flag manager is live (a crash could leave one ON in memory or the save).
            if !cleared {
                if crate::game::flags::get(base, FLAG_BASE).is_err() {
                    continue;
                }
                for k in 0..COUNT {
                    let _ = crate::game::flags::set(base, FLAG_BASE + k, false);
                }
                cleared = true;
                tracing::info!("ui_msg: message flags cleared");
            }
            if let Some((k, at)) = check {
                if now.saturating_duration_since(at) >= READBACK_AFTER {
                    check = None;
                    match crate::game::flags::get(base, FLAG_BASE + k) {
                        Ok(false) => tracing::info!("ui_msg: message {k} shown (our event cleared flag {})", FLAG_BASE + k),
                        Ok(true) => {
                            tracing::info!(
                                "ui_msg: message {k} NOT picked up within {} s (flag {} still ON; not in the Roundtable?); cleared",
                                READBACK_AFTER.as_secs(),
                                FLAG_BASE + k
                            );
                            let _ = crate::game::flags::set(base, FLAG_BASE + k, false);
                        }
                        Err(e) => tracing::info!("ui_msg: message {k} read-back failed: {e}"),
                    }
                }
            }
            let Some(k) = next_to_show(&sched, now) else { continue };
            let repeat = sched.pending.is_none();
            match crate::game::flags::set(base, FLAG_BASE + k, true) {
                Ok(_) => {
                    tracing::info!(
                        "ui_msg: message {k}{} (flag {}, text {})",
                        if repeat { " again" } else { "" },
                        FLAG_BASE + k,
                        TEXT_BASE + k
                    );
                    sched.pending = None;
                    sched.last_sent = Some(now);
                    check = Some((k, now));
                }
                Err(e) => {
                    // The world may be loading: keep the message pending and clear the flags again once the manager is back.
                    tracing::info!("ui_msg: message {k} not shown: {e}");
                    cleared = false;
                    sched.last_sent = Some(now);
                }
            }
        }
    });
    if r.is_err() {
        tracing::warn!("ui_msg: could not spawn the sender thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_keeps_the_gap_and_repeats_ongoing_states() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        // Nothing shown yet: a pending message goes at once.
        let s = Sched {
            current: Some(SEARCHING),
            pending: Some(SEARCHING),
            last_sent: None,
        };
        assert_eq!(next_to_show(&s, t0), Some(SEARCHING));
        // Within the gap nothing goes, even a new pending message.
        let s = Sched {
            current: Some(count_msg(1, 2).unwrap()),
            pending: count_msg(1, 2),
            last_sent: Some(t0),
        };
        assert_eq!(next_to_show(&s, at(3)), None);
        assert_eq!(next_to_show(&s, at(6)), count_msg(1, 2));
        // An ongoing message repeats as soon as the gap has passed; a one-off does not.
        let s = Sched {
            current: count_msg(1, 2),
            pending: None,
            last_sent: Some(t0),
        };
        assert_eq!(next_to_show(&s, at(5)), None);
        assert_eq!(next_to_show(&s, at(6)), count_msg(1, 2));
        let s = Sched {
            current: Some(STOPPED),
            pending: None,
            last_sent: Some(t0),
        };
        assert_eq!(next_to_show(&s, at(60)), None);
        // No current message: nothing repeats.
        let s = Sched {
            current: None,
            pending: None,
            last_sent: Some(t0),
        };
        assert_eq!(next_to_show(&s, at(60)), None);
    }

    #[test]
    fn ongoing_states_repeat_and_one_offs_do_not() {
        for k in [
            SEARCHING,
            HOST_INVITE,
            WAITING_OTHERS,
            WAITING_HOST,
            ALL_HERE_HOST,
            ALL_HERE_JOINER,
            SOME_MISSING,
            COUNT_FIRST,
            COUNT - 1,
        ] {
            assert!(repeats(k), "{k}");
        }
        for k in [
            STOPPED,
            LEFT_ROUNDTABLE,
            MISSED,
            MATCH_FOUND,
            JOINING,
            LOADING_IN,
            PROBLEM,
        ] {
            assert!(!repeats(k), "{k}");
        }
    }

    #[test]
    fn counts_cover_two_to_six_without_overlap() {
        let mut seen = std::collections::HashSet::new();
        for m in 2..=6 {
            for n in 1..=m {
                let k = count_msg(n, m).expect("in range");
                assert!((COUNT_FIRST..COUNT).contains(&k), "{n}/{m} -> {k}");
                assert!(seen.insert(k), "{n}/{m} duplicates {k}");
            }
        }
        assert_eq!(seen.len(), 20);
        assert_eq!(count_msg(0, 6), None);
        assert_eq!(count_msg(3, 2), None);
        assert_eq!(count_msg(1, 7), None);
    }

    #[test]
    fn gate_lines_map_to_messages() {
        assert_eq!(for_gate_line("In match"), None);
        assert_eq!(
            for_gate_line("Everyone is here - host: start the expedition"),
            Some(ALL_HERE_HOST)
        );
        assert_eq!(
            for_gate_line("Everyone is here - waiting for the host to start"),
            Some(ALL_HERE_JOINER)
        );
        assert_eq!(
            for_gate_line("Match found! Press Matchmaking Settings > Invite Members to open your lobby (no Steam window will open)"),
            Some(HOST_INVITE)
        );
        assert_eq!(
            for_gate_line(
                "Bob did not arrive: press Matchmaking Settings > Invite Members and invite them"
            ),
            Some(HOST_INVITE)
        );
        assert_eq!(
            for_gate_line("2/3 here - you can start; Bob did not arrive"),
            Some(SOME_MISSING)
        );
        assert_eq!(
            for_gate_line("Match found! Everyone is in your lobby, waiting for Bob to load in..."),
            Some(LOADING_IN)
        );
        assert_eq!(
            for_gate_line("Match found! Loading in..."),
            Some(LOADING_IN)
        );
        assert_eq!(
            for_gate_line("Match found! Waiting for players to join your lobby (1/2 here)"),
            Some(WAITING_OTHERS)
        );
        assert_eq!(
            for_gate_line("Match found! In the host's lobby, waiting for the others (1/2 here)"),
            Some(WAITING_OTHERS)
        );
        assert_eq!(
            for_gate_line("Match found! Waiting for the host..."),
            Some(WAITING_HOST)
        );
        assert_eq!(
            for_gate_line("Match found! Joining the host..."),
            Some(JOINING)
        );
        assert_eq!(
            for_gate_line("Match found! Creating your lobby..."),
            Some(MATCH_FOUND)
        );
        assert_eq!(for_gate_line("Match found!"), Some(MATCH_FOUND));
    }

    #[test]
    fn queue_states_map_to_messages() {
        let idle = QueueView::Idle {
            error: None,
            left_note: None,
        };
        assert_eq!(for_state(None, &idle), None);
        assert_eq!(for_state(Some(&idle), &idle), None);
        assert_eq!(
            for_state(Some(&idle), &QueueView::Searching),
            Some(SEARCHING)
        );
        assert_eq!(
            for_state(
                None,
                &QueueView::InQueue {
                    members: 1,
                    need: 2
                }
            ),
            count_msg(1, 2)
        );
        assert_eq!(for_state(Some(&QueueView::Searching), &idle), Some(STOPPED));
        let left = QueueView::Idle {
            error: None,
            left_note: Some("you left the Roundtable".into()),
        };
        assert_eq!(
            for_state(Some(&QueueView::Searching), &left),
            Some(LEFT_ROUNDTABLE)
        );
        let missed = QueueView::Idle {
            error: Some("match missed - queue again".into()),
            left_note: None,
        };
        assert_eq!(
            for_state(
                Some(&QueueView::Matched {
                    gate_line: String::new()
                }),
                &missed
            ),
            Some(MISSED)
        );
        let err = QueueView::Idle {
            error: Some("lobby create failed".into()),
            left_note: None,
        };
        assert_eq!(for_state(Some(&QueueView::Searching), &err), Some(PROBLEM));
        assert_eq!(
            for_state(
                None,
                &QueueView::Matched {
                    gate_line: String::new()
                }
            ),
            Some(MATCH_FOUND)
        );
        // The match ended: back to idle without a message.
        assert_eq!(
            for_state(
                Some(&QueueView::Matched {
                    gate_line: "In match".into()
                }),
                &idle
            ),
            None
        );
    }

    /// Every message has its event (one line per event: id = text id, flag, text, blinking) in gamedata/emevd-events.json and its text
    /// in gamedata/fmg-entries.json (plain-text checks: the files keep one entry per line).
    #[test]
    fn gamedata_has_every_message() {
        let events = include_str!("../../../../gamedata/emevd-events.json");
        let texts = include_str!("../../../../gamedata/fmg-entries.json");
        let used: Vec<u32> = [
            SEARCHING,
            STOPPED,
            LEFT_ROUNDTABLE,
            MISSED,
            MATCH_FOUND,
            HOST_INVITE,
            WAITING_OTHERS,
            WAITING_HOST,
            JOINING,
            LOADING_IN,
            ALL_HERE_HOST,
            ALL_HERE_JOINER,
            SOME_MISSING,
            PROBLEM,
            STARTING,
        ]
        .into_iter()
        .chain(COUNT_FIRST..COUNT)
        .collect();
        for k in used {
            let line = format!(
                "{{ \"id\": {}, \"flag\": {}, \"message\": {}, \"display\": \"blinking\" }}",
                TEXT_BASE + k,
                FLAG_BASE + k,
                TEXT_BASE + k
            );
            assert!(
                events.contains(&line),
                "no event line for message {k}: {line}"
            );
            assert!(
                texts.contains(&format!("\"{}\": \"", TEXT_BASE + k)),
                "no text for message {k}"
            );
        }
    }
}
