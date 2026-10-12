//! MM-2 safety guard (S9 §2): the queue only runs next to Seamless Co-op and never next to EasyAntiCheat.
//! The decision is a pure function over a list of module names so it can be tested with fake lists.

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::ProcessStatus::{EnumProcessModules, GetModuleBaseNameW};
use windows::Win32::System::Threading::GetCurrentProcess;

/// Name of Seamless Co-op's DLL (TECHNICAL_NOTES / CLAUDE.md paths).
pub(crate) const SC_DLL: &str = "nrsc.dll";

/// `Ok(())` when queuing is allowed; `Err(reason)` (shown on screen and logged) when it is not.
pub(crate) fn decide<S: AsRef<str>>(modules: &[S]) -> Result<(), String> {
    if let Some(eac) = modules.iter().find(|m| m.as_ref().to_ascii_lowercase().contains("easyanticheat")) {
        return Err(format!(
            "an EasyAntiCheat module is loaded ({}): refusing to queue; start the game through the mod launcher (no EAC)",
            eac.as_ref()
        ));
    }
    if !modules.iter().any(|m| m.as_ref().eq_ignore_ascii_case(SC_DLL)) {
        return Err(format!("{SC_DLL} (Seamless Co-op) is not loaded in this process: refusing to queue"));
    }
    Ok(())
}

/// Base names of every module loaded in this process (empty if the enumeration fails).
pub(crate) fn loaded_modules() -> Vec<String> {
    // Room for 2048 modules; a process never has this many. If it does, the list is simply truncated.
    let mut handles = vec![HMODULE::default(); 2048];
    let mut needed = 0u32;
    // SAFETY: the buffer is a valid writable array of `cb` bytes; `needed` is a valid out pointer; the pseudo handle of the
    // current process needs no closing.
    let ok = unsafe {
        EnumProcessModules(
            GetCurrentProcess(),
            handles.as_mut_ptr(),
            (handles.len() * std::mem::size_of::<HMODULE>()) as u32,
            &mut needed,
        )
    };
    if ok.is_err() {
        return Vec::new();
    }
    let count = (needed as usize / std::mem::size_of::<HMODULE>()).min(handles.len());
    let mut out = Vec::with_capacity(count);
    for &h in &handles[..count] {
        let mut name = [0u16; 260];
        // SAFETY: `name` is a valid writable buffer; `h` came from EnumProcessModules for this process.
        let n = unsafe { GetModuleBaseNameW(GetCurrentProcess(), Some(h), &mut name) } as usize;
        if n > 0 && n <= name.len() {
            out.push(String::from_utf16_lossy(&name[..n]));
        }
    }
    out
}

/// The guard for this process right now.
pub(crate) fn check_process() -> Result<(), String> {
    let mods = loaded_modules();
    if mods.is_empty() {
        return Err("could not list the modules of this process: refusing to queue".to_string());
    }
    decide(&mods)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_seamless_coop_without_eac() {
        assert_eq!(decide(&["nightreign.exe", "steam_api64.dll", "NRSC.dll", "nrpvp.dll"]), Ok(()));
    }

    #[test]
    fn refuses_without_seamless_coop() {
        let e = decide(&["nightreign.exe", "steam_api64.dll", "nrpvp.dll"]).unwrap_err();
        assert!(e.contains("nrsc.dll") && e.contains("not loaded"), "{e}");
        assert!(decide::<&str>(&[]).is_err());
    }

    #[test]
    fn refuses_when_any_eac_module_is_loaded_in_any_case() {
        for eac in ["EasyAntiCheat.dll", "easyanticheat_x64.dll", "EASYANTICHEAT_EOS.DLL", "EasyAntiCheat_EOS_Bootstrapper.exe"] {
            let e = decide(&["nrsc.dll", eac]).unwrap_err();
            assert!(e.contains("EasyAntiCheat") && e.contains(eac), "{e}");
        }
        // EAC wins even when SC is missing too.
        assert!(decide(&["EasyAntiCheat.dll"]).unwrap_err().contains("EasyAntiCheat"));
    }

    #[test]
    fn the_real_enumeration_runs_in_the_test_process() {
        let mods = loaded_modules();
        assert!(!mods.is_empty());
        // A test binary has no SC and no EAC.
        assert!(check_process().is_err());
    }
}
