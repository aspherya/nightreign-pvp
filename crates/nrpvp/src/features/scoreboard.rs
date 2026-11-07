//! SB: the K/D scoreboard (`.claude/SCOREBOARD.md`). [`Tally`] counts every participant's kills and deaths from the killfeed's events
//! (the killfeed is the one reader of the bus topic and hands every event here, so the feed and the board never disagree). During a
//! match a player sees only their own numbers: opening the pause menu (Esc / pad Start) shows them as the game's own centre message (the
//! blinking message of event 99100807, the line the spectator hint uses). When the match is over, the first moment the player stands in
//! the Roundtable a dialog (event 99100310) lists every participant ([`summary`]). Both texts are our placeholders in
//! EventTextForMap_dlc01 ([`TEXT_ID`], [`SUMMARY_ID`]), rewritten in memory just before the flag is raised. Log lines start with
//! `scoreboard:`.
//!
//! Rules (SCOREBOARD §3): a kill is the killer of a `Killed` event (the last rival who hit); a death is `Killed`, `Tide` or `Died` (a mob
//! or a fall; the game respawns the player); a down is neither; a defeat reported twice counts once.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use super::killfeed::{KillEvent, KillKind};
use crate::game::{fmg, gametoast, mem};

/// Our flag that shows the score message (event 99100807 in `gamedata/emevd-nights.json`).
pub(crate) const FLAG_SCORE: u32 = 10_007_414;
/// The score message's text (EventTextForMap_dlc01, `gamedata/fmg-entries.json`): a placeholder rewritten at run time.
pub(crate) const TEXT_ID: u32 = 99_100_601;
/// UTF-16 units of the placeholder, terminator included.
pub(crate) const TEXT_UNITS: usize = 64;
/// Our flag that shows the end-of-match summary (event 99100310 in `gamedata/emevd-events.json`, the Roundtable's event file).
pub(crate) const FLAG_SUMMARY: u32 = 10_007_110;
/// The summary's text (EventTextForMap_dlc01): a long placeholder rewritten at run time.
pub(crate) const SUMMARY_ID: u32 = 99_100_602;
/// UTF-16 units of the summary placeholder, terminator included.
pub(crate) const SUMMARY_UNITS: usize = 1024;
const PLACEHOLDER: u16 = b'_' as u16;

/// Dev console `score_show`: show the message as if the key was pressed.
static SHOW_ASKED: AtomicBool = AtomicBool::new(false);
/// The console's table (written by the main loop).
static TABLE: Mutex<String> = Mutex::new(String::new());
/// The placeholders' addresses by text id (the text table never moves).
static TEXT_AT: Mutex<BTreeMap<u32, usize>> = Mutex::new(BTreeMap::new());

/// One participant's numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) kills: u32,
    pub(crate) deaths: u32,
    /// Out of the match for good (a rival or the tide).
    pub(crate) out: bool,
}

/// Every participant's numbers this match, by short id (pure).
#[derive(Debug, Default)]
pub(crate) struct Tally {
    lines: BTreeMap<u16, Line>,
    /// Who went out, in order (the place: out later = better).
    order: Vec<u16>,
    /// Players who left the match before they were out.
    left: BTreeSet<u16>,
}

impl Tally {
    /// Count one event. A player already out gets nothing more (a report that came twice).
    pub(crate) fn record(&mut self, ev: KillEvent) {
        if ev.kind == KillKind::Downed || self.get(ev.victim).out {
            return;
        }
        let victim = self.lines.entry(ev.victim).or_default();
        victim.deaths += 1;
        if ev.kind != KillKind::Died {
            victim.out = true;
            self.order.push(ev.victim);
        }
        if let (KillKind::Killed, Some(k)) = (ev.kind, ev.killer) {
            if k != ev.victim {
                self.lines.entry(k).or_default().kills += 1;
            }
        }
    }

    /// A participant gone from the match while not out: a player who left (no death), or a bot that a mob killed (a death; the game
    /// replaces it with a new bot under a new name).
    pub(crate) fn gone(&mut self, id: u16, died: bool) {
        if self.get(id).out {
            return;
        }
        let line = self.lines.entry(id).or_default();
        line.out = true;
        if died {
            line.deaths += 1;
        } else {
            self.left.insert(id);
        }
        self.order.push(id);
    }

    pub(crate) fn get(&self, id: u16) -> Line {
        self.lines.get(&id).copied().unwrap_or_default()
    }

    /// One line per participant for the console: `name K n D n [out]`.
    pub(crate) fn table(&self, name: impl Fn(u16) -> String) -> String {
        if self.lines.is_empty() {
            return "score: nothing counted yet".into();
        }
        self.lines
            .iter()
            .map(|(id, l)| {
                format!(
                    "{} K {} D {}{}",
                    name(*id),
                    l.kills,
                    l.deaths,
                    if l.out { " out" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// One participant for the summary: short id, name, team.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) id: u16,
    pub(crate) name: String,
    pub(crate) team: u64,
}

fn count(n: u32, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// The end-of-match summary (pure): teams by place (still standing first, then the one out last), each team's players by kills; `me`
/// is marked. "Winner" only when exactly one team is still standing.
pub(crate) fn summary(t: &Tally, rows: &[Row], me: Option<u16>) -> String {
    let rank = |id: u16| -> usize {
        if t.get(id).out {
            t.order.iter().position(|o| *o == id).unwrap_or(0)
        } else {
            usize::MAX
        }
    };
    let mut teams: Vec<(u64, usize)> = Vec::new();
    for r in rows {
        let k = rank(r.id);
        match teams.iter_mut().find(|(team, _)| *team == r.team) {
            Some((_, best)) => *best = (*best).max(k),
            None => teams.push((r.team, k)),
        }
    }
    teams.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let standing = teams.iter().filter(|(_, k)| *k == usize::MAX).count();
    let mut out = String::from(SUMMARY_TITLE);
    for (place, (team, k)) in teams.iter().enumerate() {
        let mut members: Vec<&Row> = rows.iter().filter(|r| r.team == *team).collect();
        members.sort_by(|a, b| {
            t.get(b.id)
                .kills
                .cmp(&t.get(a.id).kills)
                .then(a.name.cmp(&b.name))
        });
        for r in members {
            let l = t.get(r.id);
            out.push_str(&format!(
                "\n{}.  {}:  {}, {}{}{}{}",
                place + 1,
                r.name,
                count(l.kills, "kill"),
                count(l.deaths, "death"),
                if *k == usize::MAX && standing == 1 {
                    "  - Winner"
                } else {
                    ""
                },
                if t.left.contains(&r.id) {
                    "  (left)"
                } else {
                    ""
                },
                if Some(r.id) == me { "  (you)" } else { "" },
            ));
        }
    }
    out
}

/// The summary's first line.
const SUMMARY_TITLE: &str = "Battle Royale results";
/// Lines the game's generic dialog shows (more are cut, and a long line is cut too, not wrapped) `[VERIFIED: score-dialog-lab run
/// 1794071916, 2026-11-07]`.
pub(crate) const DIALOG_LINES: usize = 3;

/// The summary as dialog pages of [`DIALOG_LINES`] lines (pure): the title and the first rows, then the next rows; the title says how
/// many pages there are when there is more than one.
pub(crate) fn pages(text: &str) -> Vec<String> {
    let mut lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let title = lines.remove(0);
    let first = lines.len().min(DIALOG_LINES - 1);
    let mut chunks: Vec<Vec<&str>> = vec![lines[..first].to_vec()];
    chunks.extend(lines[first..].chunks(DIALOG_LINES).map(<[&str]>::to_vec));
    let n = chunks.len();
    chunks
        .into_iter()
        .enumerate()
        .map(|(i, rows)| {
            let mut page: Vec<String> = Vec::new();
            if i == 0 {
                page.push(if n > 1 {
                    format!("{title}  (1/{n})")
                } else {
                    title.to_string()
                });
            }
            page.extend(rows.iter().map(|r| r.to_string()));
            page.join("\n")
        })
        .collect()
}

/// The message a player reads (pure).
pub(crate) fn message(l: Line) -> String {
    format!("Kills: {}   |   Deaths: {}", l.kills, l.deaths)
}

/// Console `score_show`.
#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn ask_show() -> String {
    SHOW_ASKED.store(true, Ordering::Relaxed);
    "score_show: the score message shows on the next tick (in an expedition)".into()
}

/// The console `score` command, or the key: show was asked since the last call.
pub(crate) fn take_show_asked() -> bool {
    SHOW_ASKED.swap(false, Ordering::Relaxed)
}

/// Console `score`.
#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn describe() -> String {
    let t = TABLE.lock().unwrap_or_else(PoisonError::into_inner);
    if t.is_empty() {
        "score: no expedition yet".into()
    } else {
        t.clone()
    }
}

pub(crate) fn set_table(table: String) {
    *TABLE.lock().unwrap_or_else(PoisonError::into_inner) = table;
}

/// A placeholder's address in the game's text table (searched once per id).
fn text_addr(base: usize, id: u32) -> Result<usize, String> {
    let mut map = TEXT_AT.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(a) = map.get(&id) {
        return Ok(*a);
    }
    let a = fmg::find_text(base, id)
        .ok_or_else(|| format!("text {id} is not in the game's text table"))?;
    if mem::read::<u16>(a) != Some(PLACEHOLDER) {
        return Err(format!("text {id} is not our placeholder"));
    }
    map.insert(id, a);
    Ok(a)
}

/// Write `text` over placeholder `id` of `units` UTF-16 units (main loop, before the flag is raised). `Err` says why it was not written.
fn write_text(base: usize, id: u32, units: usize, text: &str) -> Result<(), String> {
    if base == 0 {
        return Err("the exe base is not known yet".into());
    }
    let at = text_addr(base, id)?;
    let bytes = encode(text, units);
    for (k, chunk) in bytes.chunks(16).enumerate() {
        mem::write_bytes(at + k * 16, chunk).map_err(|e| format!("text write: {e}"))?;
    }
    Ok(())
}

/// The own-score message over its placeholder.
pub(crate) fn write_message(base: usize, text: &str) -> Result<(), String> {
    write_text(base, TEXT_ID, TEXT_UNITS, text)
}

/// Console `score_dialog`: any text over the summary's placeholder ('|' = a new line), to measure the dialog.
#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn dialog_test(text: &str) -> Result<String, String> {
    let text = text.replace(
        '|', "
",
    );
    write_summary(gametoast::exe_base(), &text)?;
    Ok(format!(
        "score_dialog: {} unit(s) written; `flag {FLAG_SUMMARY} 1` shows them",
        text.encode_utf16().count()
    ))
}

/// The end-of-match summary over its placeholder.
pub(crate) fn write_summary(base: usize, text: &str) -> Result<(), String> {
    write_text(base, SUMMARY_ID, SUMMARY_UNITS, text)
}

/// A text as a placeholder's bytes: UTF-16, cut to `units` with its terminator (pure). Names in it went through `gametoast::clean`.
fn encode(text: &str, units: usize) -> Vec<u8> {
    let mut u: Vec<u16> = text.encode_utf16().collect();
    u.truncate(units - 1);
    u.push(0);
    u.iter().flat_map(|u| u.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u16 = 0x6001;
    const B: u16 = 0x6002;
    const C: u16 = 0x6003;

    fn ev(kind: KillKind, victim: u16, killer: Option<u16>) -> KillEvent {
        KillEvent {
            kind,
            victim,
            killer,
        }
    }

    #[test]
    fn a_kill_goes_to_the_killer_and_a_death_to_the_victim() {
        let mut t = Tally::default();
        t.record(ev(KillKind::Downed, B, Some(A)));
        assert_eq!(t.get(B), Line::default(), "a down is neither");
        t.record(ev(KillKind::Killed, B, Some(C)));
        assert_eq!(t.get(C).kills, 1, "the last rival who hit");
        assert_eq!(t.get(A).kills, 0);
        assert_eq!(
            t.get(B),
            Line {
                kills: 0,
                deaths: 1,
                out: true
            }
        );
    }

    #[test]
    fn the_tide_and_a_mob_are_deaths_with_no_kill_and_only_a_mob_death_lets_you_go_on() {
        let mut t = Tally::default();
        t.record(ev(KillKind::Died, A, None));
        t.record(ev(KillKind::Died, A, None));
        assert_eq!(
            t.get(A),
            Line {
                kills: 0,
                deaths: 2,
                out: false
            }
        );
        t.record(ev(KillKind::Tide, A, None));
        assert_eq!(t.get(A).deaths, 3);
        assert!(t.get(A).out);
        assert_eq!(t.table(|_| "x".into()), "x K 0 D 3 out");
    }

    #[test]
    fn a_defeat_reported_twice_counts_once() {
        let mut t = Tally::default();
        t.record(ev(KillKind::Killed, B, Some(A)));
        t.record(ev(KillKind::Killed, B, Some(A)));
        t.record(ev(KillKind::Died, B, None));
        assert_eq!(t.get(A).kills, 1);
        assert_eq!(t.get(B).deaths, 1);
    }

    #[test]
    fn the_message_fits_the_placeholder() {
        let m = message(Line {
            kills: 99_999,
            deaths: 99_999,
            out: true,
        });
        assert_eq!(
            message(Line {
                kills: 3,
                deaths: 1,
                out: false
            }),
            "Kills: 3   |   Deaths: 1"
        );
        assert!(m.encode_utf16().count() < TEXT_UNITS, "{m}");
    }

    #[test]
    fn the_flag_text_and_event_are_in_the_game_data_files() {
        let events = include_str!("../../../../gamedata/emevd-nights.json");
        assert!(
            events.contains(&format!(
                "\"id\": 99100807, \"message\": {TEXT_ID}, \"display\": \"blinking\", \"flag\": {FLAG_SCORE}"
            )),
            "emevd-nights.json"
        );
        let texts = include_str!("../../../../gamedata/fmg-entries.json");
        for (id, units) in [(TEXT_ID, TEXT_UNITS), (SUMMARY_ID, SUMMARY_UNITS)] {
            let placeholder = "_".repeat(units - 1);
            assert!(
                texts.contains(&format!("\"{id}\": \"{placeholder}\"")),
                "fmg-entries.json {id}"
            );
        }
        let roundtable = include_str!("../../../../gamedata/emevd-events.json");
        assert!(
            roundtable.contains(&format!(
                "\"id\": 99100310, \"flag\": {FLAG_SUMMARY}, \"message\": {SUMMARY_ID}, \"display\": \"dialog\""
            )),
            "emevd-events.json"
        );
    }

    fn row(id: u16, name: &str, team: u64) -> Row {
        Row {
            id,
            name: name.into(),
            team,
        }
    }

    #[test]
    fn the_summary_lists_the_winner_first_then_who_went_out_last() {
        let mut t = Tally::default();
        t.record(ev(KillKind::Killed, B, Some(A)));
        t.record(ev(KillKind::Died, A, None));
        t.record(ev(KillKind::Tide, C, None));
        let rows = [row(A, "Leo", 1), row(B, "Bob", 2), row(C, "Ana", 3)];
        assert_eq!(
            summary(&t, &rows, Some(A)),
            "Battle Royale results\
             \n1.  Leo:  1 kill, 1 death  - Winner  (you)\
             \n2.  Ana:  0 kills, 1 death\
             \n3.  Bob:  0 kills, 1 death"
        );
    }

    #[test]
    fn a_team_shares_its_place_and_a_leaver_is_marked() {
        let mut t = Tally::default();
        t.record(ev(KillKind::Killed, C, Some(B)));
        t.gone(A, false);
        t.record(ev(KillKind::Killed, A, Some(C)));
        assert_eq!(
            t.get(A).deaths,
            0,
            "out already: the late report counts nothing"
        );
        let rows = [row(A, "Leo", 1), row(B, "Bob", 1), row(C, "Ana", 2)];
        assert_eq!(
            summary(&t, &rows, Some(A)),
            "Battle Royale results\
             \n1.  Bob:  1 kill, 0 deaths  - Winner\
             \n1.  Leo:  0 kills, 0 deaths  - Winner  (left)  (you)\
             \n2.  Ana:  0 kills, 1 death"
        );
    }

    #[test]
    fn the_summary_is_cut_into_pages_of_three_lines() {
        let text = "T\n1\n2\n3\n4\n5\n6\n7";
        assert_eq!(pages(text), ["T  (1/3)\n1\n2", "3\n4\n5", "6\n7"]);
        assert_eq!(pages("T\n1\n2"), ["T\n1\n2"]);
        assert_eq!(pages("T\n1\n2\n3"), ["T  (1/2)\n1\n2", "3"]);
        assert!(pages("").is_empty());
    }

    #[test]
    fn a_bot_replaced_after_a_mob_death_is_out_with_a_death_and_twelve_rows_fit() {
        let mut t = Tally::default();
        t.gone(0xB001, true);
        assert_eq!(
            t.get(0xB001),
            Line {
                kills: 0,
                deaths: 1,
                out: true
            }
        );
        let rows: Vec<Row> = (0..12u16)
            .map(|i| row(0x6000 + i, "WWWWWWWWWWWWWWW", u64::from(i)))
            .collect();
        for i in 0..11u16 {
            t.record(ev(KillKind::Killed, 0x6000 + i, Some(0x600B)));
        }
        let s = summary(&t, &rows, Some(0x6000));
        assert!(s.encode_utf16().count() < SUMMARY_UNITS, "{}", s.len());
    }
}
