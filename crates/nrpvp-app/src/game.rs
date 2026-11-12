//! Finding the game folder (`...\ELDEN RING NIGHTREIGN\Game`) through Steam, and small checks on it.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;

/// Steam's folder name for the game inside `steamapps\common` (Steam app 2622380).
const COMMON_DIR: &str = "ELDEN RING NIGHTREIGN";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// True when `dir` is the game's `Game` folder (holds nightreign.exe).
pub fn is_game_dir(dir: &Path) -> bool {
    dir.join("nightreign.exe").is_file()
}

/// A folder the player picked may be the install root (one level above `Game`); accept both.
pub fn normalize(dir: &Path) -> Option<PathBuf> {
    if is_game_dir(dir) {
        return Some(dir.to_path_buf());
    }
    let inner = dir.join("Game");
    is_game_dir(&inner).then_some(inner)
}

/// The game folder from Steam's registry entry and its library list, if the game is installed.
pub fn detect() -> Option<PathBuf> {
    let mut steam_roots = Vec::new();
    if let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam") {
        if let Ok(p) = k.get_value::<String, _>("SteamPath") {
            steam_roots.push(PathBuf::from(p.replace('/', "\\")));
        }
    }
    if let Ok(k) =
        RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\WOW6432Node\Valve\Steam")
    {
        if let Ok(p) = k.get_value::<String, _>("InstallPath") {
            steam_roots.push(PathBuf::from(p));
        }
    }
    let mut libraries = Vec::new();
    for root in &steam_roots {
        libraries.push(root.clone());
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(vdf) {
            libraries.extend(library_paths(&text));
        }
    }
    libraries
        .into_iter()
        .map(|lib| {
            lib.join("steamapps")
                .join("common")
                .join(COMMON_DIR)
                .join("Game")
        })
        .find(|dir| is_game_dir(dir))
}

/// The `"path"` values of Steam's `libraryfolders.vdf`. Kept pure for the unit test.
pub fn library_paths(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("\"path\""))
        .filter_map(|rest| {
            let v = rest.trim();
            let v = v.strip_prefix('"')?.strip_suffix('"')?;
            Some(PathBuf::from(v.replace("\\\\", "\\")))
        })
        .collect()
}

/// Files other tools put next to the game that make it load their code (a proxy DLL and its list) or replace Steam (an "online fix"
/// or emulator). Seen 2026-11-08: an OnlineFix package (`winmm.dll` + `dlllist.txt`) in a tester's game folder broke every match.
const FOREIGN_FILES: [&str; 8] = [
    "winmm.dll",
    "dlllist.txt",
    "OnlineFix.ini",
    "OnlineFix64.dll",
    "OnlineFix.url",
    "steam_emu.ini",
    "ColdClientLoader.ini",
    "steam_settings",
];

/// The [`FOREIGN_FILES`] present in the game folder (names as found, case-insensitive match). Nothing is removed: the launcher only
/// warns.
pub fn foreign_files(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| FOREIGN_FILES.iter().any(|f| f.eq_ignore_ascii_case(n)))
        .collect();
    found.sort();
    found
}

/// True while nightreign.exe runs (files in the game folder are then locked).
pub fn is_running() -> bool {
    Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq nightreign.exe", "/NH", "/FO", "CSV"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_ascii_lowercase()
                .contains("\"nightreign.exe\"")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_paths_reads_every_path_line() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"label\"\t\t\"\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"Z:\\\\SteamLibrary\"\n\t}\n}\n";
        assert_eq!(
            library_paths(vdf),
            [
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"Z:\SteamLibrary")
            ]
        );
    }

    #[test]
    fn foreign_files_are_listed_and_ours_are_not() {
        let dir = std::env::temp_dir().join(format!("nrpvp-app-foreign-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("SeamlessCoop")).unwrap();
        for f in [
            "nightreign.exe",
            "steam_api64.dll",
            "nrsc_launcher.exe",
            "WINMM.dll",
            "OnlineFix.ini",
        ] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        assert_eq!(foreign_files(&dir), ["OnlineFix.ini", "WINMM.dll"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalize_accepts_the_install_root() {
        let dir = std::env::temp_dir().join(format!("nrpvp-app-game-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Game")).unwrap();
        assert_eq!(normalize(&dir), None);
        std::fs::write(dir.join("Game").join("nightreign.exe"), b"x").unwrap();
        assert_eq!(normalize(&dir), Some(dir.join("Game")));
        assert_eq!(normalize(&dir.join("Game")), Some(dir.join("Game")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
