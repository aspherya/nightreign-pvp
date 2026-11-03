//! KF-1: the killfeed. `Leo downed Bob`, `Ana killed Bob`, on every player's screen as the game's own toasts (`game::gametoast`),
//! with the names the players gave their characters (`PlayerGameData+0x9C`, which every game holds for every player it has loaded).
//!
//! Only the victim's game knows who hit it (`PlayerIns+0x1A0` is valid there only, MEMORY_MAP §7), so that game reports: the rule
//! [`super::br_match::kill_report`] raises a [`KillEvent`], [`report`] publishes it on `net::bus` and hands it to this game's feed; every other
//! game receives the fact. [`Feed`] turns events into lines; it keeps the names, so a player whose character is gone still has one.
//! No line for a mob's or a fall's death: the game's own respawn follows, so nobody is out. Log lines start with `killfeed:`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::{Ctx, Feature, Phase};
use crate::game::world::World;
use crate::game::{gametoast, layout, mem};
use crate::net::bus::{self, topic};

/// Names are read again this often (a player who just loaded in).
const NAME_EVERY: Duration = Duration::from_secs(1);
/// Characters of a name in `PlayerGameData` (the game's own limit, 16 with the terminator).
const NAME_UNITS: usize = 16;
/// A player with no name known yet.
const NO_NAME: &str = "A player";
/// Names on the feed: this game's own team and every other one, as battle royales colour them (the toast's text takes the game's markup,
/// seen in the game 2026-11-03).
const COLOR_MINE: &str = "#7FC8FF";
const COLOR_RIVAL: &str = "#FF6A5A";

/// What happened to a player. Who it is: the session player id's upper half (`0x6002` of `0x6002FFFFFFFF`), the same on every game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KillKind {
    /// A rival's hit put the player in the downed state.
    Downed,
    /// Out of the match by a rival (the last one who hit).
    Killed,
    /// Out of the match by the Night's Tide.
    Tide,
}

/// One line of the feed, as the victim's game saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KillEvent {
    pub(crate) kind: KillKind,
    pub(crate) victim: u16,
    /// `None`: no rival is known (the tide, or a hit whose attacker could not be read).
    pub(crate) killer: Option<u16>,
}

/// The short id of a session player id.
pub(crate) fn short_id(id: u64) -> u16 {
    (id >> 32) as u16
}

/// The bus fact of an event (pure). `seq` makes the key unique: a player can be downed many times.
pub(crate) fn encode(ev: KillEvent, seq: u32) -> (u32, u32) {
    let kind = match ev.kind {
        KillKind::Downed => 0,
        KillKind::Killed => 1,
        KillKind::Tide => 2,
    };
    (
        (u32::from(ev.victim) << 16) | ((seq & 0x3FFF) << 2) | kind,
        ev.killer.map_or(0, u32::from),
    )
}

/// The event of a bus fact (pure); `None` for a kind this build does not know.
pub(crate) fn decode(key: u32, value: u32) -> Option<KillEvent> {
    let kind = match key & 3 {
        0 => KillKind::Downed,
        1 => KillKind::Killed,
        2 => KillKind::Tide,
        _ => return None,
    };
    Some(KillEvent {
        kind,
        victim: (key >> 16) as u16,
        killer: u16::try_from(value).ok().filter(|k| *k != 0),
    })
}

static LOCAL_EVENTS: Mutex<Vec<KillEvent>> = Mutex::new(Vec::new());
/// Never reset: the facts of a match are forgotten when its lobby is left, but one more number costs nothing.
static SEQ: AtomicU32 = AtomicU32::new(0);
static SHOWN: AtomicU32 = AtomicU32::new(0);
static LAST: Mutex<String> = Mutex::new(String::new());

/// The victim's game: tell everyone (main loop).
pub(crate) fn report(ev: KillEvent) {
    let (key, value) = encode(ev, SEQ.fetch_add(1, Ordering::Relaxed));
    let sent = bus::publish(topic::KILL_FEED, key, value);
    tracing::info!(
        "killfeed: reporting {ev:?}{}",
        if sent { "" } else { " (the bus refused it)" }
    );
    LOCAL_EVENTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(ev);
}

/// The console's status line.
#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn describe() -> String {
    format!(
        "killfeed: {} line(s) shown, last: {}",
        SHOWN.load(Ordering::Relaxed),
        LAST.lock().unwrap_or_else(PoisonError::into_inner)
    )
}

/// The names, teams and who is out; turns events into lines (pure).
#[derive(Debug, Default)]
pub(crate) struct Feed {
    names: BTreeMap<u16, String>,
    teams: BTreeMap<u16, u64>,
    out: BTreeSet<u16>,
    /// LOCAL's team.
    mine: Option<u64>,
}

impl Feed {
    /// A player of the list this tick.
    pub(crate) fn see(&mut self, id: u16, team: u64, local: bool) {
        self.teams.insert(id, team);
        if local {
            self.mine = Some(team);
        }
    }

    /// A player's name as the game holds it (a blank one is no name). The game's texts are markup, a name must not be.
    pub(crate) fn name(&mut self, id: u16, name: &str) {
        let name = gametoast::clean(name.trim());
        if !name.is_empty() {
            self.names.insert(id, name);
        }
    }

    /// A player's name in the colour of their side.
    fn who(&self, id: u16) -> String {
        let name = self.names.get(&id).map_or(NO_NAME, String::as_str);
        let mine = self.mine.is_some() && self.teams.get(&id).copied() == self.mine;
        let color = if mine { COLOR_MINE } else { COLOR_RIVAL };
        format!("<font color='{color}'>{name}</font>")
    }

    /// The line of an event, and the book kept. A player already out gets no second line (a report that came twice).
    pub(crate) fn line(&mut self, ev: KillEvent) -> Option<String> {
        if self.out.contains(&ev.victim) {
            return None;
        }
        let victim = self.who(ev.victim);
        let killer = ev.killer.map(|k| self.who(k));
        if ev.kind != KillKind::Downed {
            self.out.insert(ev.victim);
        }
        Some(match (ev.kind, killer) {
            (KillKind::Downed, Some(k)) => format!("{k} downed {victim}"),
            (KillKind::Downed, None) => format!("{victim} is down"),
            (KillKind::Killed, Some(k)) => format!("{k} killed {victim}"),
            (KillKind::Killed, None) => format!("{victim} was killed"),
            (KillKind::Tide, _) => format!("{victim} was consumed by the tide"),
        })
    }
}

/// The name in a `PlayerGameData`, as far as it reads (game memory).
fn read_name(player: usize) -> Option<String> {
    let data = mem::read::<usize>(player + layout::PLAYER_GAMEDATA).filter(|p| mem::is_ptr(*p))?;
    let units = mem::read::<[u16; NAME_UNITS]>(data + layout::GAMEDATA_NAME)?;
    let n = units.iter().position(|u| *u == 0).unwrap_or(NAME_UNITS);
    Some(String::from_utf16_lossy(&units[..n]))
}

/// The feature: names from the world, events from the bus and from this game's rules, lines to the screen.
pub(crate) struct KillFeed {
    feed: Feed,
    active: bool,
    names_at: Option<Instant>,
}

impl KillFeed {
    pub(crate) fn new() -> Self {
        KillFeed {
            feed: Feed::default(),
            active: false,
            names_at: None,
        }
    }
}

impl Feature for KillFeed {
    fn name(&self) -> &'static str {
        "killfeed"
    }
    fn phase(&self) -> Phase {
        Phase::AfterCombat
    }
    fn tick(&mut self, w: &World, _ctx: &mut Ctx) {
        let received = bus::take(topic::KILL_FEED);
        let local =
            std::mem::take(&mut *LOCAL_EVENTS.lock().unwrap_or_else(PoisonError::into_inner));
        if !w.in_expedition() {
            if self.active {
                self.active = false;
                self.feed = Feed::default();
                self.names_at = None;
            }
            return;
        }
        self.active = true;
        let refresh = self.names_at.is_none_or(|t| t.elapsed() >= NAME_EVERY);
        if refresh {
            self.names_at = Some(Instant::now());
        }
        for f in w.fighters.iter().flatten() {
            let id = short_id(f.id);
            self.feed.see(id, f.team, f.local);
            if refresh {
                if let Some(name) = read_name(f.addr) {
                    self.feed.name(id, &name);
                }
            }
        }
        let events = local.into_iter().chain(
            received
                .iter()
                .filter_map(|r| decode(r.fact.key, r.fact.value)),
        );
        for ev in events {
            if let Some(line) = self.feed.line(ev) {
                tracing::info!("killfeed: {line}");
                gametoast::show(&line);
                SHOWN.fetch_add(1, Ordering::Relaxed);
                *LAST.lock().unwrap_or_else(PoisonError::into_inner) = line;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u16 = 0x6001;
    const B: u16 = 0x6002;
    const C: u16 = 0x6003;

    /// A is LOCAL, three teams of one.
    fn feed() -> Feed {
        let mut f = Feed::default();
        for (id, name) in [(A, "Leo"), (B, "Bob"), (C, "Ana")] {
            f.see(id, u64::from(id), id == A);
            f.name(id, name);
        }
        f
    }

    fn ev(kind: KillKind, victim: u16, killer: Option<u16>) -> KillEvent {
        KillEvent {
            kind,
            victim,
            killer,
        }
    }

    /// The line as the player reads it.
    fn plain(line: Option<String>) -> String {
        let line = line.expect("a line");
        line.replace(&format!("<font color='{COLOR_MINE}'>"), "")
            .replace(&format!("<font color='{COLOR_RIVAL}'>"), "")
            .replace("</font>", "")
    }

    #[test]
    fn a_fact_carries_the_event_and_every_report_has_its_own_key() {
        for e in [
            ev(KillKind::Downed, B, Some(A)),
            ev(KillKind::Killed, B, Some(C)),
            ev(KillKind::Killed, B, None),
            ev(KillKind::Tide, C, None),
        ] {
            let (key, value) = encode(e, 7);
            assert_eq!(decode(key, value), Some(e));
            assert_ne!(encode(e, 8).0, key);
        }
        assert_eq!(decode(3, 0), None, "a kind of a later build");
    }

    #[test]
    fn the_short_id_is_the_upper_half() {
        assert_eq!(short_id(0x6002_FFFF_FFFF), 0x6002);
    }

    #[test]
    fn downed_then_killed_reads_as_in_a_battle_royale() {
        let mut f = feed();
        assert_eq!(
            plain(f.line(ev(KillKind::Downed, B, Some(A)))),
            "Leo downed Bob"
        );
        // A third player finishes: the kill is theirs.
        assert_eq!(
            plain(f.line(ev(KillKind::Killed, B, Some(C)))),
            "Ana killed Bob"
        );
    }

    #[test]
    fn my_team_is_blue_and_everyone_else_is_red() {
        let mut f = feed();
        assert_eq!(
            f.line(ev(KillKind::Downed, B, Some(A))).unwrap(),
            format!(
                "<font color='{COLOR_MINE}'>Leo</font> downed <font color='{COLOR_RIVAL}'>Bob</font>"
            )
        );
    }

    #[test]
    fn the_longest_line_fits_a_placeholder() {
        let mut f = Feed::default();
        for (id, team) in [(A, 1), (B, 2), (C, 3)] {
            f.see(id, team, id == A);
            f.name(id, "WWWWWWWWWWWWWWW");
        }
        let line = f.line(ev(KillKind::Killed, B, Some(A))).unwrap();
        assert!(
            line.encode_utf16().count() < gametoast::TEXT_UNITS,
            "{line}"
        );
    }

    #[test]
    fn the_tide_has_its_own_line() {
        let mut f = feed();
        assert_eq!(
            plain(f.line(ev(KillKind::Tide, C, None))),
            "Ana was consumed by the tide"
        );
    }

    #[test]
    fn a_teammate_is_blue_too() {
        let mut f = Feed::default();
        for (id, team) in [(A, 1), (B, 1), (C, 2)] {
            f.see(id, team, id == A);
            f.name(id, "x");
        }
        assert_eq!(
            f.line(ev(KillKind::Killed, B, Some(C))).unwrap(),
            format!(
                "<font color='{COLOR_RIVAL}'>x</font> killed <font color='{COLOR_MINE}'>x</font>"
            )
        );
    }

    #[test]
    fn a_player_with_no_name_or_no_killer_still_has_a_line() {
        let mut f = Feed::default();
        f.name(A, "   ");
        assert_eq!(
            plain(f.line(ev(KillKind::Downed, A, Some(B)))),
            "A player downed A player"
        );
        assert_eq!(
            plain(f.line(ev(KillKind::Killed, A, None))),
            "A player was killed"
        );
    }

    #[test]
    fn a_player_who_is_out_gets_no_second_line_and_a_revive_lets_them_go_down_again() {
        let mut f = feed();
        assert!(f.line(ev(KillKind::Downed, B, Some(A))).is_some());
        assert!(f.line(ev(KillKind::Downed, B, Some(C))).is_some());
        assert!(f.line(ev(KillKind::Killed, B, Some(C))).is_some());
        assert!(f.line(ev(KillKind::Killed, B, Some(C))).is_none());
    }

    #[test]
    fn a_name_cannot_carry_markup() {
        let mut f = feed();
        f.name(A, "<font>x");
        assert_eq!(
            plain(f.line(ev(KillKind::Downed, B, Some(A)))),
            " font x downed Bob"
        );
    }
}
