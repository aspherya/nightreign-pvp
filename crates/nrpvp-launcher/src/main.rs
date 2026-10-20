//! nrpvp_launcher.exe: the one file a player double-clicks.
//!
//! Install layout (same idea as Seamless Co-op: everything is dropped into the game folder):
//!
//! ```text
//! <Game>\nightreign.exe
//! <Game>\SeamlessCoop\nrsc.dll            (the player already has this)
//! <Game>\nrpvp_launcher.exe               (this program)
//! <Game>\NightreignPvP\nrpvp.dll, nrpvp.toml
//! <Game>\NightreignPvP\gamedata\msg\..., event\...   (our game-data package: texts + Roundtable events, UI-1/UI-2)
//! <Game>\NightreignPvP\me3\me3.exe, me3-launcher.exe, me3_mod_host.dll   (bundled me3, MIT/Apache-2.0)
//! ```
//!
//! It writes an me3 profile with absolute paths and runs the bundled me3 with `--exe`, so nothing has
//! to be installed and no Steam library lookup is needed. Profile shape verified in T-004 (TECHNICAL_NOTES §3):
//! nrsc.dll `load_early = true` with no initializer, nrpvp.dll `load_after` it. The game-data package is a `[[packages]]` entry,
//! the same shape as profiles/nrpvp-sc.me3.toml (UI-1/UI-2 solo runs).
#![windows_subsystem = "windows"]

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const TITLE: &str = "Nightreign PvP";
const MOD_DIR: &str = "NightreignPvP";
/// The game-data package inside [`MOD_DIR`] (built by scripts\build-gamedata.ps1, shipped by scripts\package-release.ps1).
const GAMEDATA_DIR: &str = "gamedata";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// me3 keeps running while the game runs; if it exits with an error inside this window, tell the player.
const EARLY_EXIT_WINDOW: Duration = Duration::from_secs(8);

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(
        hwnd: *mut core::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        kind: u32,
    ) -> i32;
}

fn message(text: &str) {
    let wide = |s: &str| {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (t, c) = (wide(text), wide(TITLE));
    // SAFETY: both buffers are valid NUL-terminated UTF-16 strings that outlive the call.
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), 0x10) };
}

/// A TOML string for a Windows path: a literal string unless the path contains a single quote.
fn toml_path(p: &Path) -> String {
    let s = p.display().to_string();
    if s.contains('\'') {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        format!("'{s}'")
    }
}

/// The me3 profile text. Kept pure for the unit test.
fn profile_text(nrsc: &Path, nrpvp: &Path, gamedata: &Path) -> String {
    format!(
        "# Written by nrpvp_launcher.exe on every start. Do not edit.\n\
         profileVersion = \"v1\"\n\n\
         [[supports]]\ngame = \"nightreign\"\n\n\
         [[natives]]\npath = {}\nload_early = true\n\n\
         [[natives]]\npath = {}\nload_after = [{{ id = \"nrsc.dll\", optional = false }}]\n\n\
         [[packages]]\nid = \"nrpvp-gamedata\"\npath = {}\n",
        toml_path(nrsc),
        toml_path(nrpvp),
        toml_path(gamedata)
    )
}

/// What is missing, in words a player can act on. `None` = ready.
fn check(game: &Path) -> Option<String> {
    let need = [
        (
            game.join("nightreign.exe"),
            "This launcher must be in the game folder, next to nightreign.exe and nrsc_launcher.exe.\n\nMove nrpvp_launcher.exe and the NightreignPvP folder there and try again.",
        ),
        (
            game.join("SeamlessCoop").join("nrsc.dll"),
            "Seamless Co-op was not found in this game folder.\n\nInstall Seamless Co-op first, then try again.",
        ),
        (
            game.join(MOD_DIR).join("nrpvp.dll"),
            "The NightreignPvP folder is missing or incomplete.\n\nCopy the whole NightreignPvP folder into the game folder, next to this launcher.",
        ),
        (
            game.join(MOD_DIR).join("me3").join("me3.exe"),
            "The NightreignPvP\\me3 folder is missing.\n\nCopy the whole NightreignPvP folder again from the zip.",
        ),
        (
            game.join(MOD_DIR).join(GAMEDATA_DIR).join("event").join("m10_00_00_00.emevd.dcx"),
            "The NightreignPvP\\gamedata folder is missing or incomplete.\n\nCopy the whole NightreignPvP folder again from the zip.",
        ),
    ];
    need.iter()
        .find(|(p, _)| !p.is_file())
        .map(|(_, why)| (*why).to_string())
}

fn run() -> Result<(), String> {
    let exe =
        std::env::current_exe().map_err(|e| format!("Could not find my own location: {e}"))?;
    let game: PathBuf = exe
        .parent()
        .ok_or("Could not find my own folder.")?
        .to_path_buf();
    if let Some(why) = check(&game) {
        return Err(why);
    }
    let mod_dir = game.join(MOD_DIR);
    let profile = mod_dir.join("nrpvp.me3.toml");
    fs::write(
        &profile,
        profile_text(
            &game.join("SeamlessCoop").join("nrsc.dll"),
            &mod_dir.join("nrpvp.dll"),
            &mod_dir.join(GAMEDATA_DIR),
        ),
    )
    .map_err(|e| {
        format!(
            "Could not write {}:\n{e}\n\nIs the game folder read-only?",
            profile.display()
        )
    })?;

    let log_path = mod_dir.join("launcher.log");
    let log = fs::File::create(&log_path)
        .map_err(|e| format!("Could not write {}:\n{e}", log_path.display()))?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    let mut child = Command::new(mod_dir.join("me3").join("me3.exe"))
        .arg("launch")
        .args(["-g", "nightreign"])
        .arg("-e")
        .arg(game.join("nightreign.exe"))
        .arg("-p")
        .arg(&profile)
        .current_dir(&game)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log2)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Could not start the mod loader (me3):\n{e}"))?;

    let step = Duration::from_millis(250);
    let mut waited = Duration::ZERO;
    while waited < EARLY_EXIT_WINDOW {
        if let Ok(Some(status)) = child.try_wait() {
            if !status.success() {
                return Err(format!(
                    "The game did not start (the mod loader stopped with {status}).\n\nMake sure Steam is running, then try again.\nDetails: {}",
                    log_path.display()
                ));
            }
            break;
        }
        std::thread::sleep(step);
        waited += step;
    }
    Ok(())
}

fn main() {
    if let Err(why) = run() {
        message(&why);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_uses_literal_paths_and_load_order() {
        let t = profile_text(
            Path::new(r"S:\Games\ELDEN RING NIGHTREIGN\Game\SeamlessCoop\nrsc.dll"),
            Path::new(r"S:\G\NightreignPvP\nrpvp.dll"),
            Path::new(r"S:\G\NightreignPvP\gamedata"),
        );
        assert!(t.contains(r"path = 'S:\Games\ELDEN RING NIGHTREIGN\Game\SeamlessCoop\nrsc.dll'"));
        assert!(t.contains("load_early = true"));
        assert!(t.contains(r#"load_after = [{ id = "nrsc.dll", optional = false }]"#));
        // SC must come first.
        assert!(t.find("nrsc.dll'").unwrap() < t.find("nrpvp.dll'").unwrap());
        assert!(t.contains(
            "[[packages]]\nid = \"nrpvp-gamedata\"\npath = 'S:\\G\\NightreignPvP\\gamedata'\n"
        ));
    }

    #[test]
    fn a_quote_in_the_path_falls_back_to_an_escaped_basic_string() {
        assert_eq!(
            toml_path(Path::new(r"C:\it's\x.dll")),
            r#""C:\\it's\\x.dll""#
        );
    }

    #[test]
    fn check_reports_the_first_missing_piece() {
        let dir = std::env::temp_dir().join(format!("nrpvp-launcher-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("SeamlessCoop")).unwrap();
        assert!(check(&dir).unwrap().contains("next to nightreign.exe"));
        fs::write(dir.join("nightreign.exe"), b"x").unwrap();
        assert!(check(&dir)
            .unwrap()
            .contains("Seamless Co-op was not found"));
        fs::write(dir.join("SeamlessCoop").join("nrsc.dll"), b"x").unwrap();
        assert!(check(&dir)
            .unwrap()
            .contains("NightreignPvP folder is missing"));
        fs::create_dir_all(dir.join(MOD_DIR).join("me3")).unwrap();
        fs::write(dir.join(MOD_DIR).join("nrpvp.dll"), b"x").unwrap();
        assert!(check(&dir).unwrap().contains("me3 folder is missing"));
        fs::write(dir.join(MOD_DIR).join("me3").join("me3.exe"), b"x").unwrap();
        assert!(check(&dir).unwrap().contains("gamedata folder is missing"));
        let event = dir.join(MOD_DIR).join(GAMEDATA_DIR).join("event");
        fs::create_dir_all(&event).unwrap();
        fs::write(event.join("m10_00_00_00.emevd.dcx"), b"x").unwrap();
        assert!(check(&dir).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
