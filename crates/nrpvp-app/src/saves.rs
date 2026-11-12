//! First setup preserves existing Seamless saves. Edits are backed up and replaced atomically.
use crate::{
    game,
    save_format::{self, Character, Save},
};
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

static SAVE_LOCK: Mutex<()> = Mutex::new(());

pub fn detect_account() -> Option<String> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Valve\Steam\ActiveProcess")
        .ok()?;
    let active: u32 = key.get_value("ActiveUser").ok()?;
    let pid: u32 = key.get_value("pid").ok()?;
    (active != 0 && is_steam_process(pid))
        .then(|| (0x0110000100000000u64 + active as u64).to_string())
}

fn is_steam_process(pid: u32) -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut core::ffi::c_void;
        fn QueryFullProcessImageNameW(
            handle: *mut core::ffi::c_void,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
    }
    // SAFETY: the handle is checked and closed; the writable buffer length matches size.
    unsafe {
        let handle = OpenProcess(0x1000, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut buffer = vec![0u16; 32768];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        result != 0
            && Path::new(&String::from_utf16_lossy(&buffer[..size as usize]))
                .file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("steam.exe"))
    }
}

pub fn current_account() -> Result<String, String> {
    detect_account().ok_or_else(|| {
        "Start Steam and sign in, then try again. Your Steam account is detected automatically."
            .into()
    })
}

pub fn require_account(expected: &str) -> Result<String, String> {
    let current = current_account()?;
    if current != expected {
        return Err("The Steam account changed. Refresh Player settings and confirm again.".into());
    }
    Ok(current)
}

pub fn account(id: &str) -> Result<u64, String> {
    let id: u64 = id
        .trim()
        .parse()
        .map_err(|_| "Enter your 17-digit SteamID64.")?;
    if !save_format::valid_id(id) {
        return Err("Invalid SteamID64.".into());
    }
    Ok(id)
}

fn save_path(game: &Path, id: u64) -> Result<PathBuf, String> {
    let ini = fs::read_to_string(game.join("SeamlessCoop/nrsc_settings.ini"))
        .map_err(|e| format!("Could not read Seamless Co-op save settings: {e}"))?;
    let extension = ini
        .lines()
        .filter_map(|line| line.split_once('='))
        .find(|(key, _)| key.trim().eq_ignore_ascii_case("save_file_extension"))
        .map(|(_, value)| value.split(';').next().unwrap_or("").trim())
        .unwrap_or("co2");
    if extension.eq_ignore_ascii_case("sl2")
        || extension.is_empty()
        || extension.len() > 120
        || !extension.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err(
            "Set Seamless Co-op's save_file_extension to co2. Vanilla .sl2 saves are never edited."
                .into(),
        );
    }
    let root = std::env::var_os("APPDATA").ok_or("APPDATA is not set.")?;
    Ok(PathBuf::from(root)
        .join("Nightreign")
        .join(id.to_string())
        .join(format!("NR0000.{extension}")))
}

#[derive(Default, Serialize)]
pub struct Status {
    pub steam_id: Option<String>,
    pub path: Option<String>,
    pub present: bool,
    pub characters: Vec<Character>,
    pub problem: Option<String>,
}

pub fn status(game: Option<&Path>, steam_id: Option<String>) -> Status {
    let mut status = Status {
        steam_id,
        ..Status::default()
    };
    let result = (|| -> Result<(), String> {
        let game = game.ok_or("Choose the game folder first.")?;
        let id =
            account(status.steam_id.as_deref().ok_or(
                "Start Steam and sign in. Your Steam account is detected automatically.",
            )?)?;
        let path = save_path(game, id)?;
        status.path = Some(path.display().to_string());
        status.present = path.is_file();
        if status.present {
            let save = Save::parse(&fs::read(&path).map_err(|e| e.to_string())?)?;
            if save.owner() != id {
                return Err(
                    "This save belongs to another Steam account; it was left unchanged.".into(),
                );
            }
            status.characters = save.characters();
        }
        Ok(())
    })();
    status.problem = result.err();
    status
}

fn stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn commit(path: &Path, bytes: &[u8], previous: Option<&[u8]>) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid save path.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let nonce = stamp();
    let temp = parent.join(format!(".nrpvp-{}-{nonce}.tmp", std::process::id()));
    let result = (|| -> Result<(), String> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        if game::is_running() {
            return Err("Close the game before changing a save.".into());
        }
        if let Some(old) = previous {
            if fs::read(path).map_err(|e| e.to_string())? != old {
                return Err(
                    "The save changed while editing. Try again with the game closed.".into(),
                );
            }
            let backup = parent.join(format!(
                "{}.nrpvp-{nonce}.bak",
                path.file_name().unwrap().to_string_lossy()
            ));
            let mut backup_file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(backup)
                .map_err(|e| e.to_string())?;
            backup_file
                .write_all(old)
                .and_then(|_| backup_file.sync_all())
                .map_err(|e| e.to_string())?;
            replace(&temp, path)?;
        } else {
            // Atomic create without overwrite, including if another process created a save meanwhile.
            fs::hard_link(&temp, path).map_err(|e| {
                format!("Could not install the starter save (existing files are preserved): {e}")
            })?;
        }
        Ok(())
    })();
    let _ = fs::remove_file(&temp);
    result
}

pub(crate) fn replace(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both pointers address live, NUL-terminated strings. Replace existing + write through.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1 | 0x8) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

pub fn setup(game: &Path, id: &str) -> Result<(), String> {
    let _guard = SAVE_LOCK
        .lock()
        .map_err(|_| "Save operation failed; restart the launcher.")?;
    if game::is_running() {
        return Err("Close the game before setting up a save.".into());
    }
    let id = account(id)?;
    let path = save_path(game, id)?;
    if path.exists() {
        let save = Save::parse(&fs::read(&path).map_err(|e| e.to_string())?)?;
        if save.owner() != id {
            return Err(
                "This save belongs to another Steam account; it was left unchanged.".into(),
            );
        }
        return Ok(());
    }
    install_starter_at(&path, id, None)
}

fn install_starter_at(path: &Path, id: u64, previous: Option<&[u8]>) -> Result<(), String> {
    let mut save = Save::parse(save_format::STARTER)?;
    save.rebind(id)?;
    // Keep the player's name when replacing a supported save from this account.
    if let Some(old) = previous
        .and_then(|bytes| Save::parse(bytes).ok())
        .filter(|s| s.owner() == id)
    {
        if let Some(character) = old.characters().first() {
            save.rename(0, &character.name)?;
        }
    }
    commit(path, &save.encode()?, previous)
}

/// Explicit replacement is available after setup too, including recovery of a damaged save.
pub fn reset(game: &Path, expected_id: &str) -> Result<(), String> {
    let _guard = SAVE_LOCK
        .lock()
        .map_err(|_| "Save operation failed; restart the launcher.")?;
    if game::is_running() {
        return Err("Close the game before replacing your save.".into());
    }
    let current = require_account(expected_id)?;
    let id = account(&current)?;
    let path = save_path(game, id)?;
    let previous = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Could not back up your save: {error}")),
    };
    require_account(&current)?;
    install_starter_at(&path, id, previous.as_deref())
}

pub fn rename(game: &Path, id: &str, slot: usize, name: &str) -> Result<(), String> {
    let _guard = SAVE_LOCK
        .lock()
        .map_err(|_| "Save operation failed; restart the launcher.")?;
    if game::is_running() {
        return Err("Close the game before changing your name.".into());
    }
    let id = account(id)?;
    let path = save_path(game, id)?;
    let old = fs::read(&path).map_err(|e| format!("Could not read your save: {e}"))?;
    let mut save = Save::parse(&old)?;
    if save.owner() != id {
        return Err("This save belongs to another Steam account; it was left unchanged.".into());
    }
    save.rename(slot, name)?;
    commit(&path, &save.encode()?, Some(&old))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn starter_replacement_rebinds_keeps_name_and_backs_up_exact_old_save() {
        let root = std::env::temp_dir().join(format!("nrpvp-reset-test-{}", stamp()));
        let path = root.join("NR0000.co2");
        let id = 0x0110000100001234;
        install_starter_at(&path, id, None).unwrap();
        let mut old = Save::parse(&fs::read(&path).unwrap()).unwrap();
        old.rename(0, "My name").unwrap();
        let previous = old.encode().unwrap();
        fs::write(&path, &previous).unwrap();
        install_starter_at(&path, id, Some(&previous)).unwrap();
        let replacement = Save::parse(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(replacement.owner(), id);
        assert_eq!(replacement.characters()[0].name, "My name");
        let backup = fs::read_dir(&root)
            .unwrap()
            .map(Result::unwrap)
            .find(|entry| entry.path().extension().is_some_and(|e| e == "bak"))
            .unwrap();
        assert_eq!(fs::read(backup.path()).unwrap(), previous);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn steam_detection_rejects_other_processes_and_stale_ids() {
        assert!(!is_steam_process(std::process::id()));
        assert!(!is_steam_process(0));
    }
    #[test]
    fn atomic_install_preserves_existing_and_rename_keeps_backup() {
        let root = std::env::temp_dir().join(format!("nrpvp-save-test-{}", stamp()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("NR0000.co2");
        commit(&path, b"original", None).unwrap();
        assert!(commit(&path, b"replacement", None).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert!(commit(&path, b"new", Some(b"stale")).is_err());
        commit(&path, b"new", Some(b"original")).unwrap();
        let backup = fs::read_dir(&root)
            .unwrap()
            .map(Result::unwrap)
            .find(|e| e.path().extension().is_some_and(|x| x == "bak"))
            .unwrap();
        assert_eq!(fs::read(backup.path()).unwrap(), b"original");
        assert_eq!(fs::read(&path).unwrap(), b"new");
        fs::remove_dir_all(root).unwrap();
    }
}
