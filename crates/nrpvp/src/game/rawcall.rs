//! NET-5 research (dev feature only): call one function of the exe with up to four integer arguments on the game thread, and report
//! the return value. Used to try the game's own `CSAutoControlAPI::SignIn` (`exe+0xF18D30`, NETWORKING.md §2b) in a game that runs
//! without Seamless Co-op. The caller names the RVA, so this is a research tool, never reachable in the player build.

use std::sync::atomic::{AtomicU64, Ordering};

static LAST: AtomicU64 = AtomicU64::new(u64::MAX);

type Fn4 = unsafe extern "system" fn(u64, u64, u64, u64) -> u64;

/// Call `exe+rva(a0, a1, a2, a3)` (x64 Windows ABI: rcx, rdx, r8, r9; a function taking fewer ignores the rest). Main loop only.
pub(crate) fn call(base: usize, rva: u32, a: [u64; 4]) {
    // SAFETY: the address is inside the exe image by construction (base + a u32 offset); the operator chose it and takes
    // responsibility for its signature. Dev builds only.
    let f: Fn4 = unsafe { std::mem::transmute::<usize, Fn4>(base + rva as usize) };
    tracing::info!(
        "rawcall: exe+{rva:#X}({:#X}, {:#X}, {:#X}, {:#X}) ...",
        a[0],
        a[1],
        a[2],
        a[3]
    );
    // SAFETY: see above.
    let rax = unsafe { f(a[0], a[1], a[2], a[3]) };
    tracing::info!("rawcall: exe+{rva:#X} returned {rax:#X}");
    LAST.store(rax, Ordering::Relaxed);
}

pub(crate) fn reply() -> String {
    match LAST.load(Ordering::Relaxed) {
        u64::MAX => "rawcall: not run yet".into(),
        v => format!("rawcall: returned {v:#X} (rax; a bool is the low byte)"),
    }
}
