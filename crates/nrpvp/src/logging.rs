//! File logger: `<dll dir>/nrpvp.log`, appended across sessions, written per event (no extra buffering at
//! our layer, so the last lines survive a crash).

use std::path::Path;

use tracing_subscriber::fmt;

pub(crate) const LOG_FILE_NAME: &str = "nrpvp.log";

pub(crate) fn init(dir: &Path) -> Result<(), String> {
    // `rolling::never` appends to a single file.
    let appender = tracing_appender::rolling::never(dir, LOG_FILE_NAME);
    fmt()
        .with_writer(appender)
        .with_ansi(false)
        .with_target(false)
        .with_thread_names(true)
        .try_init()
        .map_err(|e| e.to_string())?;

    // Panics on any thread get recorded in the log, then the default hook still runs.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        default_hook(info);
    }));
    Ok(())
}
