//! Mod thread entry point: logging setup and the startup banner.

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::hotkey::{self, Action};
use crate::overlay::{self, shared::Shared};
use crate::{config, game, logging, sysinfo};

pub(crate) fn run() {
    // A panic here must never take the game down (and must never go unrecorded).
    let result = catch_unwind(AssertUnwindSafe(|| {
        let dll_dir = match sysinfo::dll_directory(crate::dll_handle()) {
            Some(d) => d,
            // Without our own directory there is nowhere sensible to put the log; bail quietly.
            None => return,
        };
        let setup = match logging::init(&dll_dir) {
            Ok(s) => s,
            Err(_) => return,
        };
        if let Some(m) = banner(&setup) {
            // T-008 config (created with defaults if missing).
            let cfg = config::load(&dll_dir);
            // T-013: hotkey polling thread (also delivers the overlay toggle), then the overlay install
            // thread. Both fail safe: the mod carries on without them.
            hotkey::spawn(cfg.bindings());
            tracing::info!(
                "hotkeys: {} (polled every 50 ms; count only while the game window is foreground)",
                cfg.describe_bindings()
            );
            let ui = if cfg.overlay_enabled {
                let shared = Shared::new();
                overlay::spawn(
                    std::sync::Arc::clone(&shared),
                    cfg.binding(Action::ToggleOverlay).map(|k| k.name),
                );
                Some(shared)
            } else {
                tracing::info!("overlay: disabled by overlay_enabled = false in nrpvp.toml");
                None
            };
            // T-006 singleton discovery, T-007a RTTI + character discovery and the T-008 controls. Never
            // returns.
            game::run(&m, &cfg, ui);
        }
    }));
    if result.is_err() {
        tracing::error!("nrpvp startup thread panicked");
    }
}

fn banner(setup: &logging::LogSetup) -> Option<sysinfo::GameModule> {
    tracing::info!(
        "nrpvp {} starting (T-006 singletons, T-007a discovery, T-008 team controls, T-013 overlay)",
        env!("CARGO_PKG_VERSION")
    );
    tracing::info!("unix time {}", sysinfo::unix_time_secs());
    match &setup.session_path {
        Some(p) => tracing::info!("session log: {}", p.display()),
        None => tracing::info!(
            "session log: none (logging to {} only)",
            logging::LOG_FILE_NAME
        ),
    }

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
            Some(m)
        }
        None => {
            tracing::warn!("could not resolve the game module (GetModuleHandle failed)");
            None
        }
    }
}
