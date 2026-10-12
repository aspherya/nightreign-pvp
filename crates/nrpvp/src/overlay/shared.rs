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
    /// FS-0: write this value to every PlayerIns of the ChrSet (this client's copies).
    SetAll(u8),
    /// FS-0: restore every PlayerIns that has a captured original.
    RestoreAll,
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
    /// T-020: resume (`true`) or pause (`false`) the PvPvE mode. Ignored when the mode is off in the config.
    SetPvpve(bool),
    /// T-015: log `MARK #n: <text>`; `None` = from the hotkey.
    Mark(Option<String>),
    /// T-032 (dev only): select the next snap-back position candidate.
    PosCycle,
    /// T-032 (dev only): save the local player's values of every position candidate.
    PosSave,
    /// T-032 (dev only): write the saved values into the selected candidate, local player only.
    PosWrite,
    /// MM-2 (dev only, `mm_queue`): queue for a match in this mode. The main loop only forwards it to the Steam worker thread.
    MmQueue(crate::mm::Mode),
    /// MM-2 (dev only): leave the queue / the ready match.
    MmLeave,
    /// MM-3 (dev only, `mm_join`): one join-method attempt (buttons (a) (b) (c) (d)).
    MmJoin(crate::mm::join::JoinCmd),
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

/// Which PlayerIns is local (the first valid one in ChrSet entry order, T-020).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Local,
    Remote,
}

/// T-026: short form of a player id for the overlay: the first four hex digits, `0x6001..`; `?` if unreadable.
/// (ASCII only: the default ImGui font has no ellipsis glyph.)
pub(crate) fn short_id(id: Option<u64>) -> String {
    match id {
        None => "?".to_string(),
        Some(v) => {
            let hex = format!("{v:X}");
            if hex.len() > 4 {
                format!("0x{}..", &hex[..4])
            } else {
                format!("0x{hex}")
            }
        }
    }
}

/// T-026: how a player relates to the LOCAL player according to the rosters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Relation {
    Local,
    Ally,
    Rival,
}

impl Relation {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Relation::Local => "LOCAL",
            Relation::Ally => "ALLY",
            Relation::Rival => "RIVAL",
        }
    }
}

/// One PlayerIns of the ChrSet, as plain values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlayerRow {
    /// ChrSet entry index.
    pub index: usize,
    pub addr: usize,
    pub role: Role,
    /// Why `role` is what it is ("first PlayerIns in the ChrSet", "not the local player").
    pub basis: String,
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
    /// T-026: per-player id (ChrIns+0x78), if readable.
    pub id: Option<u64>,
    /// T-026: 0-based team number from the rosters (None when the rosters are unavailable).
    pub team_no: Option<u8>,
    /// T-026: relation to the local player (None when there is no local player).
    pub relation: Option<Relation>,
    pub reverts_total: u64,
}

pub(crate) const BADGE_TITLE: &str = "BATTLE ROYALE";
pub(crate) const BADGE_WAITING: &str = "waiting for expedition";

/// T-030: the two lines of the player badge. Line 1 is fixed; line 2 is `waiting for expedition` until the local
/// player is resolved with at least one player row, then `Team N` (1-based, `?` if the rosters are unavailable) and
/// the rival count.
pub(crate) fn badge_lines(s: &Snapshot) -> (String, String) {
    let in_expedition = s.wcm_instance != 0 && s.local_player.is_some() && !s.players.is_empty();
    if !in_expedition {
        return (BADGE_TITLE.to_string(), BADGE_WAITING.to_string());
    }
    let team = s
        .players
        .iter()
        .find(|p| p.relation == Some(Relation::Local))
        .and_then(|p| p.team_no)
        .map_or("?".to_string(), |n| (u32::from(n) + 1).to_string());
    let rivals = s
        .players
        .iter()
        .filter(|p| p.relation == Some(Relation::Rival))
        .count();
    (
        BADGE_TITLE.to_string(),
        format!(
            "Team {team} - {rivals} {}",
            if rivals == 1 { "rival" } else { "rivals" }
        ),
    )
}

/// T-020: state of the PvPvE mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PvpveState {
    /// Not enabled in the config (`pvpve = false`).
    #[default]
    Off,
    /// Enabled and applying.
    On,
    /// Enabled but paused (F12 or the checkbox).
    Paused,
}

impl PvpveState {
    pub(crate) fn label(self) -> &'static str {
        match self {
            PvpveState::Off => "OFF",
            PvpveState::On => "ON",
            PvpveState::Paused => "PAUSED",
        }
    }
}

/// Everything the panel shows, as plain values (no pointers are ever dereferenced from here).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    /// WorldChrMan instance address, 0 = not live.
    pub wcm_instance: usize,
    /// PlayerIns-derived vtables known (RTTI recovered); writes are refused without them.
    pub vtables_known: bool,
    /// Resolved local PlayerIns address (ChrSet entry[0] rule).
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
    /// T-020: PvPvE mode state, its team value and how many automatic writes were made this session.
    pub pvpve: PvpveState,
    pub pvpve_team: u8,
    pub pvpve_writes: u64,
    /// T-026: scheme label ("rivals" / "all"), team size, and a warning when the rosters fell back to free-for-all.
    pub pvpve_scheme: String,
    pub team_size: u8,
    pub roster_warning: String,
    /// T-031/T-032: downed-rival experiment value (0 = off); INERT since T-032 (the downed marker is unknown).
    pub downed_rival_team: u8,
    /// T-032 (dev only): selected snap-back position candidate `"text (i/n)"` (empty in the player build) and whether values are saved.
    pub pos_selected: String,
    pub pos_saved: bool,
    /// T-033 (dev only): one compact combat-state line per player, and the rival revive status text (empty in the player build).
    pub combat_lines: Vec<String>,
    pub revive_text: String,
    /// T-027: read-only probes are enabled (`probes = true`).
    pub probes_enabled: bool,
    /// T-027: the latest probe candidates, one plain line each (all UNVERIFIED).
    pub probe_lines: Vec<String>,
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
            Command::SetFor {
                addr: 0x10,
                value: 6,
            },
            Command::PersistFor {
                addr: 0x10,
                on: true,
            },
            Command::RestoreFor { addr: 0x20 },
            Command::Mark(Some("A hits B".to_string())),
            Command::Mark(None),
            Command::SetPvpve(false),
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
                team: Some(2),
                state8: 4,
                state9: 4,
                original: Some(2),
                persist: Some(6),
                reverts_last_sec: 0,
                reverts_total: 1,
                id: Some(0x6001_FFFF_FFFF),
                team_no: Some(0),
                relation: Some(Relation::Local),
            }],
            local_basis: "only PlayerIns".to_string(),
            marks: 3,
            pvpve: PvpveState::Paused,
            pvpve_team: 29,
            pvpve_writes: 7,
            pvpve_scheme: "rivals".to_string(),
            team_size: 2,
            roster_warning: String::new(),
            downed_rival_team: 6,
            pos_selected: "PlayerIns+0x6D0 (2/6)".to_string(),
            pos_saved: true,
            combat_lines: vec!["LOCAL[0] 0x6001.. downs=1 hp=0 DOWNED".to_string()],
            revive_text: "Rival revive: redown, applied 0x".to_string(),
            probes_enabled: true,
            probe_lines: vec!["clock hint: 07:00:00 mult 1 [plausible]".to_string()],
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

    fn row(rel: Relation, team_no: Option<u8>) -> PlayerRow {
        PlayerRow {
            index: 0,
            addr: 0x1000,
            role: Role::Remote,
            basis: String::new(),
            team: Some(2),
            state8: 0,
            state9: 0,
            original: None,
            persist: None,
            reverts_last_sec: 0,
            id: None,
            team_no,
            relation: Some(rel),
            reverts_total: 0,
        }
    }

    #[test]
    fn badge_text_for_the_states() {
        let mut s = Snapshot::default();
        let waiting = (BADGE_TITLE.to_string(), BADGE_WAITING.to_string());
        assert_eq!(badge_lines(&s), waiting);
        // WorldChrMan live but no local player / no rows yet: still waiting.
        s.wcm_instance = 0x1234;
        assert_eq!(badge_lines(&s), waiting);
        s.local_player = Some(0x1000);
        assert_eq!(badge_lines(&s), waiting);
        // Solo in an expedition: team 1, no rivals.
        s.players = vec![row(Relation::Local, Some(0))];
        assert_eq!(badge_lines(&s).1, "Team 1 - 0 rivals");
        // One rival, then three.
        s.players.push(row(Relation::Rival, Some(1)));
        assert_eq!(badge_lines(&s).1, "Team 1 - 1 rival");
        s.players.push(row(Relation::Rival, Some(2)));
        s.players.push(row(Relation::Rival, Some(3)));
        s.players.push(row(Relation::Ally, Some(0)));
        assert_eq!(badge_lines(&s).1, "Team 1 - 3 rivals");
        assert_eq!(badge_lines(&s).0, "BATTLE ROYALE");
        // Rosters unavailable: team unknown.
        s.players = vec![row(Relation::Local, None)];
        assert_eq!(badge_lines(&s).1, "Team ? - 0 rivals");
        // Instance gone (leaving the expedition): back to waiting.
        s.wcm_instance = 0;
        assert_eq!(badge_lines(&s), waiting);
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
