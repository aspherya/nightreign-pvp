//! Arena (dev only): the player's Ultimate Art gauge. Game 1.03.3 / exe 1.3.3.0, re/MEMORY_MAP.md §11 "Ultimate charge".
//!
//! Static reading (2026-11-08): the only caller of the charge getter `exe+0x48B810` is the HUD update `exe+0x7CBDF0`, which passes
//! `[[PlayerIns+0x1B8]+0x148]`. The getter returns f32 `+0x14` (the charge now), or `+0x18` (the charge needed) while bit 2 of byte
//! `+0x30` is set or a global debug byte is on. The same object holds the skill's charge (`+0x20` uses, `+0x24` now, `+0x28` needed,
//! `exe+0x48C03B`). `[HYPOTHESIS until a live check: writing +0x14 empties / fills the ultimate the player can use]`.

use super::mem;

/// `PlayerIns+0x1B8`: the character's module table (HP's data module is its `+0x0`, MEMORY_MAP §3).
const PLAYER_MODULES: usize = 0x1B8;
/// Module table `+0x148`: the art / skill gauge object (`exe+0x7CE3F7`).
const MODULE_GAUGES: usize = 0x148;
const ULT_NOW: usize = 0x14;
const ULT_NEEDED: usize = 0x18;
/// A charge needed above this is not one (sanity gate before any write).
const MAX_NEEDED: f32 = 1.0e6;

/// The gauge object of a player, with (now, needed) when both read as a plausible gauge.
pub(crate) fn read(player: usize) -> Option<(usize, f32, f32)> {
    let modules = mem::read::<usize>(player + PLAYER_MODULES).filter(|&m| m != 0)?;
    let g = mem::read::<usize>(modules + MODULE_GAUGES).filter(|&g| g != 0)?;
    let now = mem::read::<f32>(g + ULT_NOW)?;
    let needed = mem::read::<f32>(g + ULT_NEEDED)?;
    plausible(now, needed).then_some((g, now, needed))
}

/// Pure gate: a finite needed charge in (0, MAX_NEEDED] and a finite charge now in [0, needed].
pub(crate) fn plausible(now: f32, needed: f32) -> bool {
    needed.is_finite()
        && needed > 0.0
        && needed <= MAX_NEEDED
        && now.is_finite()
        && (0.0..=needed).contains(&now)
}

/// Set the charge now to `fraction` (0 = empty, 1 = full) of the charge needed. Returns (before, after).
pub(crate) fn set(player: usize, fraction: f32) -> Result<(f32, f32), String> {
    let (g, now, needed) = read(player).ok_or("ultimate gauge not found or not plausible")?;
    let v = needed * fraction.clamp(0.0, 1.0);
    if v != now {
        mem::write_bytes(g + ULT_NOW, &v.to_le_bytes()).map_err(|e| e.to_string())?;
    }
    Ok((now, v))
}

/// Console `ult [empty|full]`: LOCAL's ultimate charge, or set it.
pub(crate) fn cmd(args: &[String]) -> Result<String, String> {
    let chr = super::teleport::local_chr()?;
    let fraction = match args.first().map(String::as_str) {
        None => {
            let (g, now, needed) = read(chr).ok_or("ultimate gauge not found or not plausible")?;
            return Ok(format!(
                "ult: {now:.1} / {needed:.1} (gauge object 0x{g:X})"
            ));
        }
        Some("empty") => 0.0,
        Some("full") => 1.0,
        Some(other) => return Err(format!("usage: ult [empty|full] (got {other:?})")),
    };
    let (before, after) = set(chr, fraction)?;
    tracing::info!("ult: LOCAL's ultimate charge {before:.1} -> {after:.1}");
    Ok(format!("ult: {before:.1} -> {after:.1}"))
}

#[cfg(test)]
mod tests {
    use super::plausible;

    #[test]
    fn only_a_sane_gauge_is_written() {
        assert!(plausible(0.0, 100.0));
        assert!(plausible(100.0, 100.0));
        assert!(!plausible(101.0, 100.0));
        assert!(!plausible(-1.0, 100.0));
        assert!(!plausible(0.0, 0.0));
        assert!(!plausible(f32::NAN, 100.0));
        assert!(!plausible(0.0, 2.0e6));
    }
}
