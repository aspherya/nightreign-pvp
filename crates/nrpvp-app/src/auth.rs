//! Private-release token storage, protected for the current Windows user with DPAPI.
use std::{fs, path::PathBuf, sync::Mutex};

static LOCK: Mutex<()> = Mutex::new(());

fn path() -> Result<PathBuf, String> {
    let root = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set.")?;
    Ok(PathBuf::from(root).join("NightreignPvP/github-token.dpapi"))
}

#[repr(C)]
struct Blob {
    size: u32,
    data: *mut u8,
}

fn protect(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            input: *const Blob,
            description: *const u16,
            entropy: *const Blob,
            reserved: *mut core::ffi::c_void,
            prompt: *mut core::ffi::c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *mut core::ffi::c_void,
            prompt: *mut core::ffi::c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    }
    let size = u32::try_from(bytes.len()).map_err(|_| "Token data is too large.")?;
    let input = Blob {
        size,
        data: bytes.as_ptr().cast_mut(),
    };
    let mut output = Blob {
        size: 0,
        data: std::ptr::null_mut(),
    };
    // SAFETY: input stays live; DPAPI allocates output, which is copied and freed with LocalFree.
    // CRYPTPROTECT_UI_FORBIDDEN prevents authentication/permission dialogs in background work.
    unsafe {
        let success = if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut output,
            )
        };
        if success == 0 {
            return Err(
                "Windows could not access the saved GitHub token. Enter it again in Settings."
                    .into(),
            );
        }
        let result = std::slice::from_raw_parts(output.data, output.size as usize).to_vec();
        LocalFree(output.data.cast());
        Ok(result)
    }
}

pub fn token() -> Result<Option<String>, String> {
    let _guard = LOCK
        .lock()
        .map_err(|_| "Restart the launcher to read the GitHub token.")?;
    let encrypted = match fs::read(path()?) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err("Could not read the saved GitHub token. Enter it again in Settings.".into())
        }
    };
    String::from_utf8(protect(&encrypted, false)?)
        .map(Some)
        .map_err(|_| "The saved GitHub token is invalid. Enter it again in Settings.".into())
}

pub fn store(token: &str) -> Result<(), String> {
    let _guard = LOCK
        .lock()
        .map_err(|_| "Restart the launcher to save the GitHub token.")?;
    let path = path()?;
    if token.is_empty() {
        return match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("Could not remove the GitHub token.".into()),
        };
    }
    let encrypted = protect(token.as_bytes(), true)?;
    fs::create_dir_all(path.parent().unwrap())
        .map_err(|_| "Could not create the token settings folder.")?;
    let temporary = path.with_extension("dpapi.tmp");
    fs::write(&temporary, encrypted).map_err(|_| "Could not save the encrypted GitHub token.")?;
    let result = crate::saves::replace(&temporary, &path);
    let _ = fs::remove_file(temporary);
    result.map_err(|_| "Could not replace the encrypted GitHub token.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_protection_roundtrips_without_storing_plaintext() {
        let secret = b"test-token-only-not-a-real-credential";
        let encrypted = protect(secret, true).unwrap();
        assert!(!encrypted.windows(secret.len()).any(|bytes| bytes == secret));
        assert_eq!(protect(&encrypted, false).unwrap(), secret);
        assert!(protect(b"not DPAPI data", false).is_err());
    }
}
