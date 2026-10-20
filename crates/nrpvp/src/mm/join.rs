//! MM-3: pure helpers of the join step (no Steam calls): the `steam://joinlobby` URL, the guard of method (a), the automatic-action
//! setting, and the game-phase signal used to end a match.

/// ELDEN RING NIGHTREIGN's Steam app id. `[VERIFIED: appmanifest_2622380.acf, TECHNICAL_NOTES.md §1]`; the Steam store page
/// used in the repo docs (S2) is `store/app/2622380` as well.
pub(crate) const NIGHTREIGN_APP_ID: u32 = 2_622_380;

/// What the mod does by itself once the host's Seamless Co-op lobby is known (`mm_join_auto`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JoinAuto {
    /// Method (b) only: the host's mod invites every absent roster member. Default.
    Invite,
    /// (b) on the host, then the joiner's mod runs (a) by itself when it sees `sc_lobby`.
    InviteLink,
    /// (a) only, by the joiners.
    Link,
    /// Nothing automatic; only the dev buttons.
    None,
}

impl JoinAuto {
    pub(crate) fn parse(s: &str) -> Option<JoinAuto> {
        match s.trim().to_ascii_lowercase().as_str() {
            "invite" => Some(JoinAuto::Invite),
            "invite+link" => Some(JoinAuto::InviteLink),
            "link" => Some(JoinAuto::Link),
            "none" => Some(JoinAuto::None),
            _ => Option::None,
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            JoinAuto::Invite => "invite",
            JoinAuto::InviteLink => "invite+link",
            JoinAuto::Link => "link",
            JoinAuto::None => "none",
        }
    }
    /// The host's mod sends invites by itself.
    pub(crate) fn invites(self) -> bool {
        matches!(self, JoinAuto::Invite | JoinAuto::InviteLink)
    }
    /// The joiner's mod opens the join link by itself.
    pub(crate) fn links(self) -> bool {
        matches!(self, JoinAuto::InviteLink | JoinAuto::Link)
    }
}

/// The dev-button join methods (one press = one attempt, each logged as `mm_join: method (x) ...`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JoinCmd {
    /// (a) open `steam://joinlobby/<appid>/<sc_lobby>/<host>` (joiner).
    Link,
    /// (b) `InviteUserToLobby` for every roster member who is not in the host's lobby; also the "re-invite" (host).
    Invite,
    /// (c) `JoinLobby(sc_lobby)` from our DLL (joiner).
    Direct,
    /// (d) `SetLobbyType(sc_lobby, Public)` (owner of the SC lobby).
    UnlockPublic,
    /// (d) `SetLobbyType(sc_lobby, Invisible)` (owner of the SC lobby).
    UnlockInvisible,
    /// `SetLobbyType(sc_lobby, Private)`: the original type of SC's lobby is UNKNOWN, so this is "locked again" only in spirit.
    RelockPrivate,
}

/// `steam://joinlobby/<appid>/<lobby id>/<host steam id>`: the form Steam's friends UI uses for a "join game" link.
pub(crate) fn link_url(app_id: u32, lobby: u64, host: u64) -> String {
    format!("steam://joinlobby/{app_id}/{lobby}/{host}")
}

/// Method (a) may only run when Steam will deliver the join to THIS running game: the process app id is Nightreign's and the
/// overlay is enabled for it, and the MM-2 safety guard passes. Otherwise Steam could start a second, vanilla copy of the game.
pub(crate) fn link_guard(
    app_id: u32,
    overlay_enabled: bool,
    safety: &Result<(), String>,
) -> Result<(), String> {
    if let Err(e) = safety {
        return Err(format!("safety guard: {e}"));
    }
    if app_id != NIGHTREIGN_APP_ID {
        return Err(format!("ISteamUtils::GetAppID() = {app_id}, not {NIGHTREIGN_APP_ID}: Steam could start another game"));
    }
    if !overlay_enabled {
        return Err("ISteamUtils::IsOverlayEnabled() is false: Steam might start a new copy of the game instead of using this one".into());
    }
    Ok(())
}

/// Where the local player is, from the local `team_type` (TECHNICAL_NOTES.md §4, `[VERIFIED: TC-004 run 1]`: 1 in the Roundtable, 2 during
/// an expedition). Any other value (a manual experiment, our own writes) tells nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Roundtable,
    Expedition,
}

impl Phase {
    pub(crate) fn from_team(team: u8) -> Option<Phase> {
        match team {
            1 => Some(Phase::Roundtable),
            2 => Some(Phase::Expedition),
            _ => Option::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_has_the_documented_shape() {
        assert_eq!(
            link_url(2622380, 109775241234567890, 76561198000000000),
            "steam://joinlobby/2622380/109775241234567890/76561198000000000"
        );
    }

    #[test]
    fn link_guard_needs_app_id_overlay_and_the_safety_guard() {
        let ok: Result<(), String> = Ok(());
        assert_eq!(link_guard(NIGHTREIGN_APP_ID, true, &ok), Ok(()));
        assert!(link_guard(480, true, &ok).unwrap_err().contains("GetAppID"));
        assert!(link_guard(0, true, &ok).unwrap_err().contains("GetAppID"));
        assert!(link_guard(NIGHTREIGN_APP_ID, false, &ok)
            .unwrap_err()
            .contains("IsOverlayEnabled"));
        let bad: Result<(), String> = Err("no nrsc".into());
        assert!(link_guard(NIGHTREIGN_APP_ID, true, &bad)
            .unwrap_err()
            .contains("safety guard"));
    }

    #[test]
    fn auto_modes_parse_and_say_who_acts() {
        assert_eq!(JoinAuto::parse(" Invite+Link "), Some(JoinAuto::InviteLink));
        assert_eq!(JoinAuto::parse("banana"), None);
        for (m, inv, lnk) in [
            (JoinAuto::Invite, true, false),
            (JoinAuto::InviteLink, true, true),
            (JoinAuto::Link, false, true),
            (JoinAuto::None, false, false),
        ] {
            assert_eq!((m.invites(), m.links()), (inv, lnk), "{m:?}");
            assert_eq!(JoinAuto::parse(m.label()), Some(m));
        }
    }

    #[test]
    fn phase_comes_from_the_local_team_type() {
        assert_eq!(Phase::from_team(1), Some(Phase::Roundtable));
        assert_eq!(Phase::from_team(2), Some(Phase::Expedition));
        assert_eq!(Phase::from_team(29), None);
        assert_eq!(Phase::from_team(0), None);
    }
}
