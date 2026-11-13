//! Battle Royale (MODE-01, `re/PROJECT_SCOPE.md`): the one mode shipped today. Everything that makes it Battle Royale is named here;
//! the order is behaviour (the loop reads the world before `combat.tick`, the hook modules run after it, `maphide` after `hudfilter`).

use crate::config::Config;
use crate::features::{bots, br_match, clock, hooks, mode, nights, Feature};

/// The five rules features in tick order (mode, bots, clock, nights, br_match's `night_match`), then the hook modules' adapters (R7-6, see
/// `hooks`): `multiplay` (Rules) and the `AfterCombat` ones in the order the loop always ticked them.
pub(crate) fn list(cfg: &Config) -> Vec<Box<dyn Feature>> {
    let team = cfg.team_size_usize();
    vec![
        Box::new(mode::ModeFeature::new(cfg.game_mode(), "nrpvp.toml")),
        Box::new(bots::BotsFeature::new(
            usize::try_from(cfg.bots_fill).unwrap_or(0),
            cfg.pvpve_team,
        )),
        Box::new(clock::ClockFeature::default()),
        Box::new(nights::NightsFeature),
        Box::new(br_match::MatchFeature::new(cfg)),
        Box::new(hooks::Multiplay::new(team)),
        Box::new(hooks::DeathHold),
        Box::new(hooks::MenuGate),
        Box::new(hooks::HudFilter),
        Box::new(hooks::RuneShare::new(team)),
        Box::new(hooks::DefeatDrop),
        Box::new(hooks::OrbClaim),
        // After HudFilter: it reads `hudfilter::map_hooked()`.
        Box::new(hooks::MapHide::new(team)),
        Box::new(hooks::ToastFilter),
        Box::new(hooks::Hawk),
        Box::new(crate::features::killfeed::KillFeed::new()),
        #[cfg(feature = "arena")]
        Box::new(crate::game::saved_loadouts::SavedLoadouts::default()),
    ]
}
