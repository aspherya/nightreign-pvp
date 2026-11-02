//! P2-7: game modes as assemblies. A mode is the ordered list of features (rules and hook adapters) that make a match; the loop
//! (`game::watch`) ticks whatever list it is given and knows no mode. Today there is one ruleset, Battle Royale; a new mode is a new
//! file here that builds its own list from the same modules, plus one line in [`assemble`].
//!
//! Not to be confused with `features::mode::GameMode` (`br` / `offline`: how Commence behaves in the stock menu).

use crate::config::Config;
use crate::features::Features;

pub(crate) mod battle_royale;

/// The feature list for this run, in tick order. Called once at startup.
pub(crate) fn assemble(cfg: &Config) -> Features {
    Features::from_list(battle_royale::list(cfg))
}
