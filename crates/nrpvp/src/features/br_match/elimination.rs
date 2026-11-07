//! RW-1: who is out, and the match decision (the last team standing).

use std::sync::atomic::{AtomicBool, Ordering};

use super::{MatchBook, MatchOut, MatchRule, Tick, FLAG_LOSE, FLAG_WIN};
use crate::game::combat_state::Fighter;

/// Dead this long (HP 0, not downed) = out of the match. A mob's death respawns in about 14 s (defeat-mob run 2026-10-24: HP 0 to standing
/// 14.2 s), so this must stay well above that or another game would call a respawning player out; a defeated player is held for good.
pub(crate) const OUT_DEAD_MS: u64 = 30_000;
/// Missing from the player list this long after being seen = out (left the match).
pub(crate) const GONE_MS: u64 = 6_000;
/// One team must be the only one standing this long before the match is decided (one stray reading never ends it).
pub(crate) const DECIDE_MS: u64 = 1_000;

/// Dev console `night_count 0|1` (on by default; resets at launch).
pub(super) static COUNT: AtomicBool = AtomicBool::new(true);

/// How the match ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Decision {
    /// The last team standing (`None`: everyone was out at once).
    pub winner: Option<u64>,
    pub at_ms: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Seen {
    team: u64,
    dead_since: Option<u64>,
    gone_since: Option<u64>,
}

impl Seen {
    pub(super) fn out(&self, now_ms: u64) -> bool {
        self.dead_since
            .is_some_and(|t| now_ms.saturating_sub(t) >= OUT_DEAD_MS)
            || self
                .gone_since
                .is_some_and(|t| now_ms.saturating_sub(t) >= GONE_MS)
    }
}

/// The rule: count the players still in and decide the match.
#[derive(Debug, Default)]
pub(crate) struct Elimination;

impl MatchRule for Elimination {
    fn step(&mut self, t: &Tick, book: &mut MatchBook, out: &mut MatchOut) {
        let (now_ms, fighters, local, host) = (t.now_ms, t.fighters, t.local, t.host);
        // BOT-2: the rival bots count like players (each its own team).
        let all: Vec<&Fighter> = fighters.iter().map(|(_, f)| f).chain(t.bots).collect();
        for f in &all {
            let s = book.seen.entry(f.id).or_default();
            s.team = f.team;
            s.gone_since = None;
            if f.hp == 0 && f.state != 1 {
                s.dead_since.get_or_insert(now_ms);
            } else {
                s.dead_since = None;
            }
        }
        for (id, s) in book.seen.iter_mut() {
            if !all.iter().any(|f| f.id == *id) {
                s.gone_since.get_or_insert(now_ms);
            }
        }
        let mut teams: Vec<u64> = book.seen.values().map(|s| s.team).collect();
        teams.sort_unstable();
        teams.dedup();
        book.max_teams = book.max_teams.max(teams.len());
        if book.match_over || book.count_off || book.max_teams < 2 {
            return;
        }
        let local_out = book.defeat.is_some();
        // BOT-2: a defeated LOCAL with no other player left to watch (only bots stand) has lost: the match ends for it now, it does not
        // wait for the bots to finish each other (Leonardo's match of 2026-11-07 never ended).
        let humans_left = fighters
            .iter()
            .any(|(_, f)| !f.local && book.seen.get(&f.id).is_some_and(|s| !s.out(now_ms)));
        if local_out && !humans_left && !t.bots.is_empty() {
            book.decided = Some(Decision {
                winner: None,
                at_ms: now_ms,
            });
            book.match_over = true;
            out.lines.push(
                "night_match: match decided: LOCAL is out and only bots are left (this game's player lost)".into(),
            );
            if !book.defeat.is_some_and(|d| d.exit_sent) {
                out.flags.push((FLAG_LOSE, true));
            }
            return;
        }
        let standing: Vec<u64> = teams
            .iter()
            .copied()
            .filter(|t| {
                book.seen.iter().any(|(id, s)| {
                    s.team == *t && !s.out(now_ms) && !(local_out && *id == local.id)
                })
            })
            .collect();
        // BOT-2: a bot killed by a mob is still owed a replacement: it is a rival standing until it spawns (or is given up).
        if standing.len() > 1 || t.world.bots_pending > 0 {
            book.lone_since = None;
            return;
        }
        let since = *book.lone_since.get_or_insert(now_ms);
        if now_ms.saturating_sub(since) < DECIDE_MS {
            return;
        }
        let winner = standing.first().copied();
        book.decided = Some(Decision {
            winner,
            at_ms: now_ms,
        });
        book.match_over = true;
        let won = winner.is_some() && winner == Some(local.team);
        out.lines.push(format!(
            "night_match: match decided: {} (this game's player {}{})",
            winner.map_or("nobody left standing".into(), |w| format!(
                "team {w:#X} is the last one standing"
            )),
            if won { "won" } else { "lost" },
            if host { ", host" } else { "" }
        ));
        let already_lost = book.defeat.is_some_and(|d| d.exit_sent);
        if won {
            out.flags.push((FLAG_WIN, true));
        } else if !already_lost {
            out.flags.push((FLAG_LOSE, true));
        }
    }
}

/// Console `night_count [0|1]`: switch the end-by-count (`Some`) and report it.
pub(crate) fn count_cmd(on: Option<bool>) -> String {
    if let Some(on) = on {
        COUNT.store(on, Ordering::Relaxed);
        tracing::info!("night_match: count {}", if on { "ON" } else { "OFF" });
    }
    if COUNT.load(Ordering::Relaxed) {
        "night_count 1: the match ends when one team is left standing".into()
    } else {
        "night_count 0: the match never ends by count".into()
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;
    use crate::game::world::NightFlags;

    #[test]
    fn in_a_two_player_match_the_defeat_ends_the_match_at_once_for_both() {
        let nf = NightFlags::default();
        // The loser's game.
        let mut c = core();
        run(&mut c, 0, &world(&[up(ME, true), up(P2, false)], nf));
        let mut w = world(&[dead(ME, true), up(P2, false)], nf);
        rival_hit(&mut w, 100);
        run_dead(&mut c, 100, &w);
        let (_, flags, lines) = run(&mut c, 2_100, &w);
        assert_eq!(flags, vec![(FLAG_LOSE, true)], "{lines:?}");
        let (_, flags, _) = run(&mut c, 30_000, &w);
        assert!(flags.is_empty());
        // The winner's game sees the loser dead and out only after OUT_DEAD_MS.
        let mut c = core();
        run(&mut c, 0, &world(&[up(P2, true), up(ME, false)], nf));
        let w = world(&[up(P2, true), dead(ME, false)], nf);
        run(&mut c, 100, &w);
        let (_, flags, _) = run(&mut c, 100 + OUT_DEAD_MS, &w);
        assert!(flags.is_empty());
        let (_, flags, _) = run(&mut c, 1_200 + OUT_DEAD_MS, &w);
        assert_eq!(flags, vec![(FLAG_WIN, true)]);
    }

    /// BOT-2: rival bots count like rivals: alive they keep the match going; dead or gone from the bot set they are out.
    #[test]
    fn rival_bots_are_rivals_and_the_last_one_out_wins_the_match() {
        use crate::game::world::BotChr;
        let bot = |handle: u64, hp: u32, team: u8| BotChr {
            handle,
            chr: 0x7000 + handle as usize,
            team,
            npc_id: crate::game::layout::BOT_NPC_PARAM,
            hp,
            max_hp: 240,
            pos: None,
            attacker: 0,
        };
        let mut c = core();
        let mut w = world(&[up(ME, true)], NightFlags::default());
        // A bot left on the ally team 2 (not kept by the fill) is not a rival.
        w.bot_chrs = vec![bot(0xA0, 700, 29), bot(0xA1, 700, 29), bot(0xA2, 700, 2)];
        run(&mut c, 0, &w);
        let (_, flags, _) = run(&mut c, 60_000, &w);
        assert!(flags.is_empty(), "two bots alive");
        // One bot dies (HP 0), the other leaves the set (its body removed).
        w.bot_chrs = vec![bot(0xA0, 0, 29)];
        run(&mut c, 61_000, &w);
        let (_, flags, _) = run(&mut c, 61_000 + GONE_MS, &w);
        assert!(flags.is_empty(), "the dead bot is not out yet");
        run(&mut c, 61_000 + OUT_DEAD_MS, &w);
        let (_, flags, lines) = run(&mut c, 62_000 + OUT_DEAD_MS, &w);
        assert_eq!(flags, vec![(FLAG_WIN, true)], "{lines:?}");
    }

    /// BOT-2: the last bot killed by a mob does not hand LOCAL the win while its replacement is coming.
    #[test]
    fn a_bot_owed_a_replacement_keeps_the_match_open() {
        use crate::game::world::BotChr;
        let bot = |hp| BotChr {
            handle: 0xA0,
            chr: 0x7000,
            team: 29,
            npc_id: crate::game::layout::BOT_NPC_PARAM,
            hp,
            max_hp: 240,
            pos: None,
            attacker: 0,
        };
        let mut c = core();
        let mut w = world(&[up(ME, true)], NightFlags::default());
        w.bot_chrs = vec![bot(700)];
        run(&mut c, 0, &w);
        w.bot_chrs = vec![bot(0)];
        w.bots_pending = 1;
        run(&mut c, 1_000, &w);
        let (_, flags, _) = run(&mut c, 2_000 + OUT_DEAD_MS * 2, &w);
        assert!(flags.is_empty(), "the replacement is owed");
        w.bots_pending = 0;
        run(&mut c, 3_000 + OUT_DEAD_MS * 2, &w);
        let (_, flags, _) = run(&mut c, 4_000 + OUT_DEAD_MS * 2, &w);
        assert_eq!(flags, vec![(FLAG_WIN, true)]);
    }

    /// BOT-2: LOCAL defeated with only bots standing: lost at once.
    #[test]
    fn a_defeated_player_with_only_bots_left_loses_at_once() {
        use crate::game::world::BotChr;
        let bot = |handle: u64| BotChr {
            handle,
            chr: 0x7000 + handle as usize,
            team: 29,
            npc_id: crate::game::layout::BOT_NPC_PARAM,
            hp: 700,
            max_hp: 240,
            pos: None,
            attacker: 0,
        };
        let mut c = core();
        let mut w = world(&[up(ME, true)], NightFlags::default());
        w.bot_chrs = vec![bot(0xA0), bot(0xA1)];
        run(&mut c, 0, &w);
        // LOCAL killed by a bot (a rival hit), dead.
        let mut d = world(&[dead(ME, true)], NightFlags::default());
        d.bot_chrs = w.bot_chrs.clone();
        crate::features::br_match::testutil::rival_hit(&mut d, 500);
        let (_, flags, lines) = crate::features::br_match::testutil::run_dead(&mut c, 1_000, &d);
        let (_, flags2, lines2) = run(&mut c, 2_000, &d);
        let all: Vec<_> = flags.iter().chain(&flags2).copied().collect();
        assert!(all.contains(&(FLAG_LOSE, true)), "{lines:?} {lines2:?}");
        assert!(lines
            .iter()
            .chain(&lines2)
            .any(|l| l.contains("only bots are left")));
    }

    #[test]
    fn the_match_ends_at_any_time_when_one_team_is_left_and_the_winner_gets_the_win_flag() {
        let mut c = core();
        let all = world(
            &[up(ME, true), up(P2, false), up(P3, false)],
            NightFlags::default(),
        );
        run(&mut c, 0, &all);
        // P2 and P3 die for good (dead for OUT_DEAD_MS).
        let w = world(
            &[up(ME, true), dead(P2, false), dead(P3, false)],
            NightFlags::default(),
        );
        run(&mut c, 1_000, &w);
        let (_, flags, _) = run(&mut c, 1_000 + OUT_DEAD_MS - 1, &w);
        assert!(flags.is_empty(), "not out yet");
        run(&mut c, 1_000 + OUT_DEAD_MS, &w);
        let (_, flags, lines) = run(&mut c, 2_000 + OUT_DEAD_MS, &w);
        assert_eq!(flags, vec![(FLAG_WIN, true)], "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("match decided")));
        // Decided once.
        let (_, flags, _) = run(&mut c, 3_000 + OUT_DEAD_MS, &w);
        assert!(flags.is_empty());
    }

    #[test]
    fn a_downed_player_is_still_in_and_one_team_alone_is_not_enough_before_a_rival_was_seen() {
        let mut c = core();
        let solo = world(&[up(ME, true)], NightFlags::default());
        for t in [0, 2_000, 60_000] {
            let (_, flags, _) = run(&mut c, t, &solo);
            assert!(flags.is_empty(), "never a match with one team seen");
        }
        let two = world(
            &[up(ME, true), fighter(P2, false, 1, 0)],
            NightFlags::default(),
        );
        for t in [61_000, 70_000, 90_000] {
            let (_, flags, _) = run(&mut c, t, &two);
            assert!(flags.is_empty(), "a downed player can still be revived");
        }
    }

    #[test]
    fn a_player_who_leaves_the_match_is_out_after_a_few_seconds() {
        let mut c = core();
        run(
            &mut c,
            0,
            &world(&[up(ME, true), up(P2, false)], NightFlags::default()),
        );
        let alone = world(&[up(ME, true)], NightFlags::default());
        let (_, flags, _) = run(&mut c, 1_000, &alone);
        assert!(flags.is_empty());
        let (_, flags, _) = run(&mut c, 1_000 + GONE_MS, &alone);
        assert!(flags.is_empty(), "the lone team must stand DECIDE_MS");
        let (_, flags, _) = run(&mut c, 2_100 + GONE_MS, &alone);
        assert_eq!(flags, vec![(FLAG_WIN, true)]);
    }

    #[test]
    fn the_count_can_be_switched_off() {
        let mut c = core();
        c.book.count_off = true;
        let w = world(&[up(P2, true), dead(ME, false)], NightFlags::default());
        run(&mut c, 0, &w);
        let (_, flags, _) = run(&mut c, 40_000, &w);
        assert!(flags.is_empty());
        c.book.count_off = false;
        run(&mut c, 41_000, &w);
        let (_, flags, _) = run(&mut c, 42_100, &w);
        assert_eq!(flags, vec![(FLAG_WIN, true)]);
    }
}
