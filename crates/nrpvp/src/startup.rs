//! Mod thread entry point: logging setup and the startup banner.

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::{logging, sysinfo};

pub(crate) fn run() {
    // A panic here must never take the game down (and must never go unrecorded).
    let result = catch_unwind(AssertUnwindSafe(|| {
        let dll_dir = match sysinfo::dll_directory(crate::dll_handle()) {
            Some(d) => d,
            // Without our own directory there is nowhere sensible to put the log; bail quietly.
            None => return,
        };
        if logging::init(&dll_dir).is_err() {
            return;
        }
        banner();
    }));
    if result.is_err() {
        tracing::error!("nrpvp startup thread panicked");
    }
}

fn banner() {
    tracing::info!(
        "nrpvp {} starting (T-001 skeleton)",
        env!("CARGO_PKG_VERSION")
    );
    tracing::info!("unix time {}", sysinfo::unix_time_secs());

    match sysinfo::game_module() {
        Some(m) => {
            tracing::info!(
                "{}: base=0x{:X} size=0x{:X}{}",
                m.name,
                m.base,
                m.size,
                if m.is_fallback {
                    " (nightreign.exe not found by name, using the process main module)"
                } else {
                    ""
                }
            );
            tracing::info!("exe path: {}", m.path);
            match &m.file_version {
                Some(v) => tracing::info!("exe FileVersion: {v}"),
                None => {
                    tracing::warn!("exe FileVersion: unavailable (version resource not readable)")
                }
            }
        }
        None => tracing::warn!("could not resolve the game module (GetModuleHandle failed)"),
    }
}
