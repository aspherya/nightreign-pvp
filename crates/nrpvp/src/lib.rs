//! nrpvp: PvPvE mod for ELDEN RING NIGHTREIGN, loaded by me3 as a native DLL.
//!
//! T-001 skeleton: on `DLL_PROCESS_ATTACH` spawn one thread (nothing else happens under the loader
//! lock); that thread sets up file logging next to the DLL and writes the startup banner.

mod logging;
mod startup;
mod sysinfo;

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HINSTANCE, TRUE};
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

/// Handle of this DLL (set in `DllMain`, read by the mod thread to find the DLL's directory).
static DLL_HANDLE: AtomicIsize = AtomicIsize::new(0);

/// # Safety
/// Called by the Windows loader with the loader lock held. Does no work other than storing the module
/// handle and spawning a thread.
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> windows::core::BOOL {
    if reason == DLL_PROCESS_ATTACH {
        DLL_HANDLE.store(hinst.0 as isize, Ordering::SeqCst);
        // If spawning fails there is nowhere to log yet; the DLL just stays inert. Never fail the load.
        let _ = std::thread::Builder::new()
            .name("nrpvp-main".into())
            .spawn(startup::run);
    }
    TRUE
}

/// Module handle stored by `DllMain`, as a raw value (0 if `DllMain` has not run).
pub(crate) fn dll_handle() -> isize {
    DLL_HANDLE.load(Ordering::SeqCst)
}
