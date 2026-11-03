//! HM-1 (both builds): logs every change of this game's session role and of who the host is, so a test with real players can be read
//! from `nrpvp.log`. The game migrates the host by itself when the Steam lobby owner changes (HOST_MIGRATION.md); this only watches.
//! Read-only. Log lines start with `session:`.

use std::sync::Mutex;

use super::{layout, mem};

static LAST: Mutex<(u32, u64)> = Mutex::new((u32::MAX, 0));

fn role_name(role: u32) -> &'static str {
    match role {
        0 => "none",
        3 => "host",
        6 => "client",
        _ => "other",
    }
}

/// The line for a change from `old` to `new` (`(role, host steam id)`), `None` when nothing changed.
fn change_line(old: (u32, u64), new: (u32, u64), players: usize) -> Option<String> {
    (old != new).then(|| {
        format!(
            "session: role {} -> {} ({}), host {} -> {}, {players} player(s)",
            if old.0 == u32::MAX {
                "?"
            } else {
                role_name(old.0)
            },
            role_name(new.0),
            new.0,
            old.1,
            new.1,
        )
    })
}

/// Main loop, every pass.
pub(crate) fn tick(base: usize) {
    let Some(s) = mem::read::<usize>(base + layout::RVA_SESSION).filter(|s| mem::is_ptr(*s)) else {
        return;
    };
    let (Some(role), Some(host)) = (
        mem::read::<u32>(s + layout::SESSION_ROLE),
        mem::read::<u64>(s + layout::SESSION_HOST_STEAM),
    ) else {
        return;
    };
    let Ok(mut last) = LAST.lock() else {
        return;
    };
    if *last == (role, host) {
        return;
    }
    let first = mem::read::<usize>(s + layout::SESSION_PLAYERS_FIRST).unwrap_or(0);
    let end = mem::read::<usize>(s + layout::SESSION_PLAYERS_LAST).unwrap_or(0);
    let players = end.saturating_sub(first) / layout::SESSION_PLAYER_ENTRY;
    if let Some(line) = change_line(*last, (role, host), players) {
        tracing::info!("{line}");
    }
    *last = (role, host);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_gives_one_readable_line() {
        assert_eq!(change_line((6, 1), (6, 1), 3), None);
        assert_eq!(
            change_line((6, 1), (3, 2), 2).as_deref(),
            Some("session: role client -> host (3), host 1 -> 2, 2 player(s)")
        );
        assert_eq!(
            change_line((u32::MAX, 0), (0, 0), 0).as_deref(),
            Some("session: role ? -> none (0), host 0 -> 0, 0 player(s)")
        );
    }
}
