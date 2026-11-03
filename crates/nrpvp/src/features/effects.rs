//! R7-4: the one place that carries out what the features asked for. The match rules' requests ([`Act`]: move LOCAL, drop its gear,
//! hold the comeback, open the Esc menu, the clock's speed, the game's flags) used to be executed by `game::watch`; they run here now,
//! as the first step of [`super::Ctx::flush`] (before the matchmaking commands, our flags and the clock time, as before).

use super::Targets;
use crate::game::gamemem::GameMem;
use crate::game::{deathhold, defeatdrop, flags, menugate, spawn, views};

/// What the match rules ask the main loop to do (besides our flags, which go through [`super::Ctx::flags`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Act {
    /// Move LOCAL to this world position.
    Teleport([f32; 3]),
    /// RW-1: move the defeated, invisible body next to the watched player (10 times a second, so the move is not logged).
    Follow([f32; 3]),
    /// Write one of the game's flags (`game::flags::MATCH_WRITABLE`).
    GameFlag(u32, bool),
    /// Arm or disarm the hold of a dead LOCAL's comeback (`game::deathhold::set_armed`).
    DeathHold(bool),
    /// RW-4: the play-area clock's speed (game seconds per real second).
    ClockSpeed(f32),
    /// RW-1: let a defeated, held player's Esc menu open (`game::menugate`).
    MenuGate(bool),
    /// RW-3: LOCAL was defeated by a rival: its gear drops where it stands (`game::defeatdrop`).
    DropItems,
    /// KF-1: a line of the killfeed, from the victim's game (`features::killfeed::report`).
    KillFeed(super::killfeed::KillEvent),
}

/// Carry out the match rules' requests (the hold of a defeated player, a move, the game's flags, the clock's speed).
pub(super) fn run_acts(acts: Vec<Act>, t: Targets, mem: &dyn GameMem) {
    let (local, base, play_area) = (t.local, t.base, t.play_area);
    for a in acts {
        let r: Result<(), &'static str> = match a {
            Act::Teleport(to) => match local.map(|p| spawn::teleport(p, base, to)) {
                Some(Ok(from)) => {
                    tracing::info!(
                        "night_match: moved LOCAL from world ({:.1}, {:.1}, {:.1}) to ({:.1}, {:.1}, {:.1})",
                        from[0], from[1], from[2], to[0], to[1], to[2]
                    );
                    Ok(())
                }
                Some(Err(e)) => Err(e),
                None => Err("no local player"),
            },
            Act::Follow(to) => match local.map(|p| spawn::teleport(p, base, to)) {
                Some(Ok(_)) => Ok(()),
                Some(Err(e)) => Err(e),
                None => Err("no local player"),
            },
            Act::DropItems => match local {
                Some(p) => {
                    tracing::info!("{}", defeatdrop::drop_local(base, p));
                    Ok(())
                }
                None => Err("no local player"),
            },
            Act::KillFeed(ev) => {
                super::killfeed::report(ev);
                Ok(())
            }
            Act::MenuGate(on) => {
                menugate::set_active(on);
                Ok(())
            }
            Act::DeathHold(on) => {
                deathhold::set_armed(on);
                Ok(())
            }
            Act::ClockSpeed(speed) => {
                if play_area == 0 {
                    Err("no play-area manager")
                } else {
                    views::PlayArea::new(mem, play_area).set_speed(speed)
                }
            }
            Act::GameFlag(f, on) => flags::set_match_flag(base, f, on).map(|prev| {
                tracing::info!(
                    "night_match: game flag {f} set {} (was {prev})",
                    if on { "ON" } else { "off" }
                );
            }),
        };
        if let Err(e) = r {
            // A failing spectator move repeats 10 times a second: say it once.
            static FOLLOW_WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !matches!(a, Act::Follow(_))
                || !FOLLOW_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                tracing::warn!("night_match: {a:?} failed: {e}");
            }
        }
    }
}
