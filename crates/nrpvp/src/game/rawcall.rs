//! NET-5 research (dev feature only): call one function of the exe with a single integer argument on the game thread, and report
//! the return value. Used to try the game's own `CSAutoControlAPI::SignIn` (`exe+0xF18D30`, NETWORKING.md §2b) in a game that runs
//! without Seamless Co-op. The caller names the RVA, so this is a research tool, never reachable in the player build.

use std::sync::atomic::{AtomicU64, Ordering};

static LAST: AtomicU64 = AtomicU64::new(u64::MAX);

/// Call `exe+rva(arg)` (x64 Windows ABI, one integer argument). Must run on the main loop.
pub(crate) fn call(base: usize, rva: u32, arg: u32) {
    let f: unsafe extern "system" fn(u32) -> u64 =
        // SAFETY: the address is inside the exe image by construction (base + a u32 offset); the operator chose it and takes
        // responsibility for its signature. Dev builds only.
        unsafe { std::mem::transmute::<usize, unsafe extern "system" fn(u32) -> u64>(base + rva as usize) };
    tracing::info!("rawcall: exe+{rva:#X}({arg}) ...");
    // SAFETY: see above.
    let rax = unsafe { f(arg) };
    tracing::info!("rawcall: exe+{rva:#X}({arg}) returned {rax:#X}");
    LAST.store(rax, Ordering::Relaxed);
}

pub(crate) fn reply() -> String {
    match LAST.load(Ordering::Relaxed) {
        u64::MAX => "rawcall: not run yet".into(),
        v => format!("rawcall: returned {v:#X} (rax; a bool is the low byte)"),
    }
}
