//! CON-1: who a player command acts on (CONSOLE.md §3.1). Pure: resolved against the main loop's [`Snapshot`] rows.
//!
//! `me` (default), `all`, `allies`, `rivals`, `p1`..`p6` (session player number from the per-player id `ChrIns+0x78`, MEMORY_MAP §3:
//! `0x6001FFFFFFFF` = p1, the same on every game once the expedition has loaded), `s0`..`s5` (ChrSet slot on this game).

use crate::overlay::shared::{PlayerRow, Relation, Role};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Me,
    All,
    Allies,
    Rivals,
    /// Session player number (1-based).
    Session(u16),
    /// ChrSet slot on this game.
    Slot(usize),
}

impl Target {
    /// How the user typed it (`p2`, `s0`, `rivals`).
    pub(crate) fn label(self) -> String {
        match self {
            Target::Me => "me".into(),
            Target::All => "all".into(),
            Target::Allies => "allies".into(),
            Target::Rivals => "rivals".into(),
            Target::Session(n) => format!("p{n}"),
            Target::Slot(i) => format!("s{i}"),
        }
    }
}

pub(crate) const TARGET_HELP: &str = "me | all | allies | rivals | p1..p6 | s0..s5";

pub(crate) fn parse_target(t: Option<&str>) -> Result<Target, String> {
    let Some(t) = t else {
        return Ok(Target::Me);
    };
    let t = t.to_ascii_lowercase();
    match t.as_str() {
        "me" => return Ok(Target::Me),
        "all" => return Ok(Target::All),
        "allies" => return Ok(Target::Allies),
        "rivals" => return Ok(Target::Rivals),
        _ => {}
    }
    let num = |rest: &str| rest.parse::<u16>().ok();
    if let Some(n) = t.strip_prefix('p').and_then(num).filter(|n| *n >= 1) {
        return Ok(Target::Session(n));
    }
    if let Some(n) = t.strip_prefix('s').and_then(num) {
        return Ok(Target::Slot(usize::from(n)));
    }
    Err(format!("unknown target '{t}' (use {TARGET_HELP})"))
}

/// Session player number of a per-player id: `0x600N_FFFF_FFFF` -> N. `None` for anything else.
pub(crate) fn session_no(id: Option<u64>) -> Option<u16> {
    let hi = (id? >> 32) as u16;
    (hi & 0xF000 == 0x6000 && hi & 0x0FFF != 0).then_some(hi & 0x0FFF)
}

fn is_me(r: &PlayerRow) -> bool {
    r.role == Role::Local || r.relation == Some(Relation::Local)
}

/// The rows a target selects, in ChrSet order. An error says why nothing matched.
pub(crate) fn pick(t: Target, rows: &[PlayerRow]) -> Result<Vec<&PlayerRow>, String> {
    let sel: Vec<&PlayerRow> = rows
        .iter()
        .filter(|r| match t {
            Target::Me => is_me(r),
            Target::All => true,
            Target::Allies => r.relation == Some(Relation::Ally),
            Target::Rivals => r.relation == Some(Relation::Rival),
            Target::Session(n) => session_no(r.id) == Some(n),
            Target::Slot(i) => r.index == i,
        })
        .collect();
    if sel.is_empty() {
        let why = if rows.is_empty() {
            "no players loaded (no world, or still loading)"
        } else {
            "no player matches"
        };
        return Err(format!("{}: {why}", t.label()));
    }
    Ok(sel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(index: usize, local: bool, id: u64, rel: Option<Relation>) -> PlayerRow {
        PlayerRow {
            index,
            addr: 0x1000 * (index + 1),
            role: if local { Role::Local } else { Role::Remote },
            basis: String::new(),
            team: Some(2),
            state8: 0,
            state9: 0,
            original: None,
            persist: None,
            reverts_last_sec: 0,
            id: Some(id),
            team_no: None,
            relation: rel,
            reverts_total: 0,
        }
    }

    #[test]
    fn targets_parse() {
        assert_eq!(parse_target(None), Ok(Target::Me));
        assert_eq!(parse_target(Some("ALL")), Ok(Target::All));
        assert_eq!(parse_target(Some("p2")), Ok(Target::Session(2)));
        assert_eq!(parse_target(Some("s0")), Ok(Target::Slot(0)));
        assert!(parse_target(Some("p0")).is_err());
        assert!(parse_target(Some("bob")).is_err());
    }

    #[test]
    fn session_numbers_come_from_the_player_id() {
        assert_eq!(session_no(Some(0x6001_FFFF_FFFF)), Some(1));
        assert_eq!(session_no(Some(0x6003_FFFF_FFFF)), Some(3));
        assert_eq!(session_no(Some(0x6000_FFFF_FFFF)), None);
        assert_eq!(session_no(Some(0x1234)), None);
        assert_eq!(session_no(None), None);
    }

    #[test]
    fn pick_selects_by_role_relation_id_and_slot() {
        let rows = vec![
            row(0, true, 0x6002_FFFF_FFFF, Some(Relation::Local)),
            row(1, false, 0x6001_FFFF_FFFF, Some(Relation::Rival)),
            row(2, false, 0x6003_FFFF_FFFF, Some(Relation::Ally)),
        ];
        let idx = |t| {
            pick(t, &rows)
                .map(|v| v.iter().map(|r| r.index).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        assert_eq!(idx(Target::Me), vec![0]);
        assert_eq!(idx(Target::All), vec![0, 1, 2]);
        assert_eq!(idx(Target::Rivals), vec![1]);
        assert_eq!(idx(Target::Allies), vec![2]);
        assert_eq!(idx(Target::Session(1)), vec![1]);
        assert_eq!(idx(Target::Slot(2)), vec![2]);
        assert!(pick(Target::Session(5), &rows).is_err());
        assert!(pick(Target::Me, &[]).unwrap_err().contains("no players"));
    }
}
