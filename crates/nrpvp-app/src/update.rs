//! Verified launcher updates, applied by a temporary copy after the main window closes.
use crate::release;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub const VERSION: &str = match option_env!("NRPVP_BUILD_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

pub fn newer(remote: &str, local: &str) -> bool {
    match (
        semver::Version::parse(remote),
        semver::Version::parse(local),
    ) {
        (Ok(r), Ok(l)) => r > l,
        _ => false,
    }
}

pub fn prepare(progress: impl FnMut(u64, u64)) -> Result<bool, String> {
    let manifest = release::latest()?;
    if !newer(&manifest.version, VERSION) {
        return Ok(false);
    }
    let asset = manifest
        .launcher
        .as_ref()
        .ok_or("This release does not contain a launcher update.")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let work = std::env::temp_dir().join(format!("nrpvp-update-{}-{nonce}", std::process::id()));
    fs::create_dir(&work).map_err(|e| e.to_string())?;
    let payload = work.join("payload.exe");
    release::fetch_asset(&manifest, asset, &payload, progress)?;
    let target = std::env::current_exe().map_err(|e| e.to_string())?;
    let helper = work.join("apply.exe");
    fs::copy(&target, &helper).map_err(|e| e.to_string())?;
    Command::new(helper)
        .arg("--apply-launcher-update")
        .arg(&target)
        .arg(std::process::id().to_string())
        .spawn()
        .map_err(|e| format!("Could not start the update helper: {e}"))?;
    Ok(true)
}

fn wait_for_parent(pid: u32) -> Result<(), String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut core::ffi::c_void;
        fn WaitForSingleObject(handle: *mut core::ffi::c_void, timeout: u32) -> u32;
        fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
    }
    // SAFETY: handle is checked, used only while open, and always closed. SYNCHRONIZE access only.
    unsafe {
        let handle = OpenProcess(0x00100000, 0, pid);
        if handle.is_null() {
            // The main process can exit before the helper finishes loading.
            if std::io::Error::last_os_error().raw_os_error() == Some(87) {
                return Ok(());
            }
            return Err("Could not wait for the launcher to close.".into());
        }
        let result = WaitForSingleObject(handle, 60_000);
        CloseHandle(handle);
        if result != 0 {
            return Err("The launcher did not close in time.".into());
        }
    }
    Ok(())
}

fn apply(target: &Path, pid: u32) -> Result<(), String> {
    let helper = std::env::current_exe().map_err(|e| e.to_string())?;
    let payload = helper
        .parent()
        .ok_or("Missing update folder.")?
        .join("payload.exe");
    wait_for_parent(pid)?;
    install_payload(target, &payload, pid, || {
        Command::new(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}

fn install_payload(
    target: &Path,
    payload: &Path,
    pid: u32,
    restart: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let staged = target.with_extension(format!("{}.update", pid));
    let backup = target.with_extension(format!("{}.previous", pid));
    // Copy beside the target first; renames then stay on the same volume.
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)
        .map_err(|e| e.to_string())?;
    fs::copy(payload, &staged).map_err(|e| e.to_string())?;
    if backup.exists() {
        return Err("A previous update backup already exists.".into());
    }
    let mut renamed = false;
    for _ in 0..40 {
        if fs::rename(target, &backup).is_ok() {
            renamed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    if !renamed {
        return Err(
            "Could not replace the launcher; its original executable was preserved.".into(),
        );
    }
    if let Err(error) = fs::rename(&staged, target) {
        let _ = fs::rename(&backup, target);
        return Err(error.to_string());
    }
    if let Err(error) = restart() {
        let _ = fs::rename(target, &staged);
        let _ = fs::rename(&backup, target);
        return Err(error);
    }
    Ok(())
}

/// Helper mode must run before Tauri starts or any UI is created.
pub fn run_helper() -> bool {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--apply-launcher-update")) {
        return false;
    }
    if let (Some(target), Some(pid)) = (args.next(), args.next()) {
        let target = PathBuf::from(target);
        if let Some(pid) = pid.to_str().and_then(|p| p.parse::<u32>().ok()) {
            if let Err(error) = apply(&target, pid) {
                let _ = fs::write(target.with_extension("update-error.log"), error);
                let _ = Command::new(target).arg("--skip-auto-update").spawn();
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_never_downgrade_or_loop() {
        assert!(newer("0.10.0", "0.2.0"));
        assert!(!newer("0.2.0", "0.10.0"));
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("broken", "0.2.0"));
    }
    #[test]
    fn update_keeps_previous_binary_and_rolls_back_failed_restart() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("nrpvp-update-test-{nonce}"));
        fs::create_dir(&root).unwrap();
        let target = root.join("Launcher with spaces.exe");
        let payload = root.join("payload.exe");
        fs::write(&target, b"old launcher").unwrap();
        fs::write(&payload, b"new launcher").unwrap();
        install_payload(&target, &payload, 1, || Ok(())).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new launcher");
        assert_eq!(
            fs::read(target.with_extension("1.previous")).unwrap(),
            b"old launcher"
        );
        fs::write(&payload, b"failed update").unwrap();
        assert!(install_payload(&target, &payload, 2, || Err("Restart failed".into())).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"new launcher");
        assert_eq!(
            fs::read(target.with_extension("2.update")).unwrap(),
            b"failed update"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
