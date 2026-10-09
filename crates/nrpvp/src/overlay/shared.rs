//! T-013: the only link between the overlay's render thread and the game-facing main loop.
//!
//! The main loop (`game::watch`) publishes a [`Snapshot`] of plain values and drains the [`Command`]
//! queue. The render thread reads the snapshot and pushes commands. **Neither this module nor the
//! render thread ever touches game memory**; every command is executed by the main loop through the same
//! `TeamCtl` / discovery code the hotkeys use.
//!
//! T-015: the snapshot lists every PlayerIns of the ChrSet ([`PlayerRow`]); commands can target one of them
//! by address, and the text box / hotkey MARK is a command too.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Largest number of queued commands; further pushes are dropped (a stuck main loop must not grow memory).
pub(crate) const MAX_QUEUED_COMMANDS: usize = 64;

/// What the overlay can ask the main loop to do. Addresses are PlayerIns addresses taken from the
/// snapshot; the main loop re-validates them against a fresh ChrSet resolve before touching anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// Select this team candidate and write it to the local player.
    Set(u8),
    /// Restore the original value captured before the first write (local player).
    Restore,
    /// Toggle persist mode for the local player.
    TogglePersist,
    /// Run the character discovery (same as the discovery hotkey).
    RunDiscovery,
    /// Run the team_type histogram (same as the histogram hotkey).
    RunHistogram,
    /// T-015: write `value` to this player's local copy of team_type.
    SetFor { addr: usize, value: u8 },
    /// T-015: restore this player's captured original value.
    RestoreFor { addr: usize },
    /// T-015: persist on/off for this player.
    PersistFor { addr: usize, on: bool },
    /// T-015: the HUMAN says this PlayerIns is the local player ("this is me"), kept for the session.
    SetLocalOverride { addr: usize },
    /// T-015: forget the "this is me" choice.
    ClearLocalOverride,
    /// T-015: log `MARK #n: <text>`; `None` = from the hotkey.
    Mark(Option<String>),
}

/// Name of a known `team_type` value (Smithbox NR TEAM_TYPE enum, TECHNICAL_NOTES.md section 4; the
/// hostility semantics are still a HYPOTHESIS). `"?"` for everything else.
pub(crate) fn team_name(v: u8) -> &'static str {
    match v {
        1 => "Player",
        2 => "White Ghost",
        3 => "Black Ghost",
        6 => "Enemy",
        13 | 16 => "Invader",
        29 => "Indiscriminate",
        _ => "?",
    }
}

/// Which PlayerIns is local.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Local,
    Remote,
    /// More than one PlayerIns and nothing tells them apart (and the HUMAN has not chosen).
    Unknown,
}

/// One PlayerIns of the ChrSet, as plain values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlayerRow {
    /// ChrSet entry index.
    pub index: usize,
    pub addr: usize,
    pub role: Role,
    /// Why `role` is what it is ("only PlayerIns", "heuristic: ...", "chosen by you", ...).
    pub basis: String,
    /// True if the HUMAN chose this row with "this is me".
    pub chosen: bool,
    /// Live ChrIns+0x74.
    pub team: Option<u8>,
    /// ChrSet entry bytes +8 and +9 (meaning UNVERIFIED).
    pub state8: u8,
    pub state9: u8,
    /// Original value captured before the first write to this player.
    pub original: Option<u8>,
    /// Persist value, if persist is on for this player.
    pub persist: Option<u8>,
    pub reverts_last_sec: u32,
    pub reverts_total: u64,
}

/// Everything the panel shows, as plain values (no pointers are ever dereferenced from here).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    /// WorldChrMan instance address, 0 = not live.
    pub wcm_instance: usize,
    /// PlayerIns-derived vtables known (RTTI recovered); writes are refused without them.
    pub vtables_known: bool,
    /// Resolved local PlayerIns address (decided, chosen or suggested).
    pub local_player: Option<usize>,
    /// Live `team_type` of the local player (ChrIns+0x74), read by the main loop.
    pub team_type: Option<u8>,
    /// Original value captured before the first write to this local player.
    pub original: Option<u8>,
    /// Currently selected candidate (what the write hotkey applies).
    pub selected: u8,
    /// Configured `team_candidates`.
    pub candidates: Vec<u8>,
    /// Persist is on for the local player.
    pub persist: bool,
    /// Reverts the game made in the last completed second of persist mode (all players).
    pub reverts_last_sec: u32,
    pub reverts_total: u64,
    /// Discovery/histogram worker is running (those buttons are disabled meanwhile).
    pub busy: bool,
    /// Why the local player is not resolved, or empty.
    pub note: String,
    /// T-015: every valid PlayerIns of the ChrSet, entry order.
    pub players: Vec<PlayerRow>,
    /// T-015: how the local player was decided (empty when there is none).
    pub local_basis: String,
    /// T-015: MARK lines logged so far.
    pub marks: u32,
}

/// Bounded FIFO of commands.
#[derive(Default)]
pub(crate) struct CommandQueue {
    q: VecDeque<Command>,
}

impl CommandQueue {
    /// `false` if the queue is full and the command was dropped.
    pub(crate) fn push(&mut self, c: Command) -> bool {
        if self.q.len() >= MAX_QUEUED_COMMANDS {
            return false;
        }
        self.q.push_back(c);
        true
    }

    /// Everything queued so far, oldest first.
    pub(crate) fn drain(&mut self) -> Vec<Command> {
        self.q.drain(..).collect()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.q.len()
    }
}

/// Shared between the overlay and the main loop. Cheap to clone via `Arc`.
#[derive(Default)]
pub(crate) struct Shared {
    snapshot: Mutex<Snapshot>,
    commands: Mutex<CommandQueue>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // Never panic because another thread panicked while holding the lock.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    pub(crate) fn new() -> Arc<Shared> {
        Arc::new(Shared::default())
    }

    /// Main loop: replace the snapshot.
    pub(crate) fn publish(&self, s: Snapshot) {
        *lock(&self.snapshot) = s;
    }

    /// Render thread: copy of the latest snapshot.
    pub(crate) fn snapshot(&self) -> Snapshot {
        lock(&self.snapshot).clone()
    }

    /// Render thread: queue a command. `false` if dropped because the queue is full.
    pub(crate) fn push(&self, c: Command) -> bool {
        lock(&self.commands).push(c)
    }

    /// Main loop: take every queued command.
    pub(crate) fn drain(&self) -> Vec<Command> {
        lock(&self.commands).drain()
    }

    #[cfg(test)]
    pub(crate) fn queued(&self) -> usize {
        lock(&self.commands).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_drain_in_order_and_empty_the_queue() {
        let s = Shared::new();
        assert!(s.drain().is_empty());
        assert!(s.push(Command::Set(6)));
        assert!(s.push(Command::TogglePersist));
        assert!(s.push(Command::Restore));
        assert!(s.push(Command::RunDiscovery));
        assert!(s.push(Command::RunHistogram));
        assert!(s.push(Command::Set(29)));
        assert_eq!(s.queued(), 6);
        assert_eq!(
            s.drain(),
            vec![
                Command::Set(6),
                Command::TogglePersist,
                Command::Restore,
                Command::RunDiscovery,
                Command::RunHistogram,
                Command::Set(29)
            ]
        );
        assert_eq!(s.queued(), 0);
        assert!(s.drain().is_empty());
    }

    #[test]
    fn player_targeted_and_mark_commands_travel_in_order() {
        let s = Shared::new();
        let cmds = vec![
            Command::SetFor { addr: 0x10, value: 6 },
            Command::PersistFor { addr: 0x10, on: true },
            Command::RestoreFor { addr: 0x20 },
            Command::SetLocalOverride { addr: 0x20 },
            Command::Mark(Some("A hits B".to_string())),
            Command::Mark(None),
            Command::ClearLocalOverride,
        ];
        for c in &cmds {
            assert!(s.push(c.clone()));
        }
        assert_eq!(s.drain(), cmds);
    }

    #[test]
    fn queue_is_bounded() {
        let s = Shared::new();
        for _ in 0..MAX_QUEUED_COMMANDS {
            assert!(s.push(Command::Restore));
        }
        assert!(!s.push(Command::Set(1)), "full queue drops the new command");
        assert_eq!(s.drain().len(), MAX_QUEUED_COMMANDS);
        assert!(s.push(Command::Set(1)), "accepts again once drained");
    }

    #[test]
    fn snapshot_publish_and_read() {
        let s = Shared::new();
        assert_eq!(s.snapshot(), Snapshot::default());
        let snap = Snapshot {
            wcm_instance: 0x1234,
            vtables_known: true,
            local_player: Some(0x5678),
            team_type: Some(2),
            original: Some(2),
            selected: 29,
            candidates: vec![6, 29],
            persist: true,
            reverts_last_sec: 3,
            reverts_total: 9,
            busy: true,
            note: String::new(),
            players: vec![PlayerRow {
                index: 1,
                addr: 0x5678,
                role: Role::Local,
                basis: "only PlayerIns".to_string(),
                chosen: false,
                team: Some(2),
                state8: 4,
                state9: 4,
                original: Some(2),
                persist: Some(6),
                reverts_last_sec: 0,
                reverts_total: 1,
            }],
            local_basis: "only PlayerIns".to_string(),
            marks: 3,
        };
        s.publish(snap.clone());
        assert_eq!(s.snapshot(), snap);
        // A later publish replaces the earlier one.
        s.publish(Snapshot::default());
        assert_eq!(s.snapshot(), Snapshot::default());
    }

    #[test]
    fn shared_works_across_threads() {
        let s = Shared::new();
        let producer = Arc::clone(&s);
        let h = std::thread::spawn(move || {
            for v in 0..10u8 {
                producer.push(Command::Set(v));
            }
        });
        h.join().expect("producer thread");
        let got = s.drain();
        assert_eq!(got.len(), 10);
        assert_eq!(got[9], Command::Set(9));
    }

    #[test]
    fn poisoned_locks_do_not_panic() {
        let s = Shared::new();
        let s2 = Arc::clone(&s);
        let _ = std::thread::spawn(move || {
            let _g = s2.commands.lock().expect("lock");
            let _h = s2.snapshot.lock().expect("lock");
            panic!("poison both");
        })
        .join();
        assert!(s.push(Command::Restore));
        assert_eq!(s.drain(), vec![Command::Restore]);
        s.publish(Snapshot::default());
        assert_eq!(s.snapshot(), Snapshot::default());
    }

    #[test]
    fn team_names_match_the_card() {
        let expect = [
            (1, "Player"),
            (2, "White Ghost"),
            (3, "Black Ghost"),
            (6, "Enemy"),
            (13, "Invader"),
            (16, "Invader"),
            (29, "Indiscriminate"),
            (0, "?"),
            (4, "?"),
            (30, "?"),
            (255, "?"),
        ];
        for (v, n) in expect {
            assert_eq!(team_name(v), n, "{v}");
        }
    }
}
