//! Process / module queries used by the startup banner. Win32 only; no game structures.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
};
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
use windows::Win32::System::Threading::GetCurrentProcess;

/// Seconds since the Unix epoch (0 if the clock is before it).
pub(crate) fn unix_time_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format the two DWORDs of a `VS_FIXEDFILEINFO` file version as `major.minor.build.revision`.
pub(crate) fn format_file_version(ms: u32, ls: u32) -> String {
    format!("{}.{}.{}.{}", ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF)
}

/// Full path of a module (lossy UTF-16 to `String`). `None` if the call fails.
fn module_path(module: HMODULE) -> Option<String> {
    let mut buf = vec![0u16; 32768];
    // SAFETY: `buf` is a valid writable buffer of the length passed; `module` is a handle obtained
    // from the loader (or the DLL handle stored by DllMain).
    let n = unsafe { GetModuleFileNameW(Some(module), &mut buf) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..n]))
}

/// Directory containing this DLL, from the module handle stored by `DllMain`.
pub(crate) fn dll_directory(handle: isize) -> Option<PathBuf> {
    if handle == 0 {
        return None;
    }
    let path = module_path(HMODULE(handle as *mut _))?;
    PathBuf::from(path).parent().map(|p| p.to_path_buf())
}

pub(crate) struct GameModule {
    pub name: &'static str,
    pub base: usize,
    pub size: usize,
    pub path: String,
    pub file_version: Option<String>,
    /// True when `nightreign.exe` was not found by name and the process main module was used.
    pub is_fallback: bool,
}

/// Locate `nightreign.exe` in the current process and read base, size, path, and FileVersion.
pub(crate) fn game_module() -> Option<GameModule> {
    // SAFETY: both calls only read loader state. A null name means "the process's main module".
    let (module, is_fallback) = unsafe {
        match GetModuleHandleW(w!("nightreign.exe")) {
            Ok(m) if !m.is_invalid() => (m, false),
            _ => (
                GetModuleHandleW(PCWSTR::null())
                    .ok()
                    .filter(|m| !m.is_invalid())?,
                true,
            ),
        }
    };

    let mut info = MODULEINFO::default();
    // SAFETY: `info` is a valid MODULEINFO and the size passed matches it; the process handle is the
    // current-process pseudo handle.
    unsafe {
        GetModuleInformation(
            GetCurrentProcess(),
            module,
            &mut info,
            std::mem::size_of::<MODULEINFO>() as u32,
        )
    }
    .ok()?;

    let path = module_path(module)?;
    let file_version = file_version_of(&path);
    Some(GameModule {
        name: if is_fallback {
            "main module"
        } else {
            "nightreign.exe"
        },
        base: info.lpBaseOfDll as usize,
        size: info.SizeOfImage as usize,
        path,
        file_version,
        is_fallback,
    })
}

/// Read the fixed FileVersion from the PE version resource of `path`.
fn file_version_of(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let pcwstr = PCWSTR(wide.as_ptr());

    // SAFETY: `wide` is NUL-terminated and outlives all calls. The data buffer is sized by the API's own
    // size query, and the pointer returned by VerQueryValueW points into that buffer; it is only read
    // while the buffer is alive and after checking for null and the reported length.
    unsafe {
        let size = GetFileVersionInfoSizeW(pcwstr, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(pcwstr, None, size, data.as_mut_ptr().cast()).ok()?;

        let mut ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut len: u32 = 0;
        if !VerQueryValueW(data.as_ptr().cast(), w!("\\"), &mut ptr, &mut len).as_bool() {
            return None;
        }
        if ptr.is_null() || (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>() {
            return None;
        }
        let ffi = std::ptr::read_unaligned(ptr as *const VS_FIXEDFILEINFO);
        Some(format_file_version(
            ffi.dwFileVersionMS,
            ffi.dwFileVersionLS,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_file_version() {
        // 1.3.3.0 as packed in VS_FIXEDFILEINFO.
        assert_eq!(format_file_version(0x0001_0003, 0x0003_0000), "1.3.3.0");
        assert_eq!(format_file_version(0xFFFF_FFFF, 0), "65535.65535.0.0");
    }

    #[test]
    fn dll_directory_rejects_null_handle() {
        assert!(dll_directory(0).is_none());
    }
}
