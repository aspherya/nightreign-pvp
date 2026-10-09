//! File logger. T-014: every line goes to two files, `<dll dir>/nrpvp.log` (truncated at startup, so it
//! always mirrors the current session and `Get-Content nrpvp.log -Wait` keeps working) and a per-session
//! `<dll dir>/logs/nrpvp_<UTC yyyy-mm-dd_HH-MM-SS>.log` (kept; only the newest 30 survive, pruned at
//! startup). If `logs/` cannot be created the session file is skipped with a warning. No symlinks or
//! hardlinks. Lines are written per event with no buffering at our layer, so the tail survives a crash.
//!
//! T-013: a second layer keeps the last ~300 lines in memory (`logring`) for the overlay's log tail. Both
//! layers share one `INFO` level filter.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::logring::RingMakeWriter;

pub(crate) const LOG_FILE_NAME: &str = "nrpvp.log";
pub(crate) const SESSION_DIR_NAME: &str = "logs";
/// Number of session logs kept in `logs/` (including the current one).
pub(crate) const KEEP_SESSION_LOGS: usize = 30;
const SESSION_PREFIX: &str = "nrpvp_";
const SESSION_SUFFIX: &str = ".log";
/// `yyyy-mm-dd_HH-MM-SS`
const STAMP_LEN: usize = 19;

/// What `init` set up, for the startup banner.
pub(crate) struct LogSetup {
    /// The per-session file, or `None` when only `nrpvp.log` is being written.
    pub(crate) session_path: Option<PathBuf>,
}

/// Writes each event to the mirror file and, when present, the session file.
struct Sinks {
    main: Mutex<File>,
    session: Option<Mutex<File>>,
}

#[derive(Clone)]
struct TeeMakeWriter(Arc<Sinks>);

struct TeeWriter(Arc<Sinks>);

fn lock(m: &Mutex<File>) -> MutexGuard<'_, File> {
    // A poisoned lock only means another thread panicked mid-write; keep logging.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl<'a> MakeWriter<'a> for TeeMakeWriter {
    type Writer = TeeWriter;
    fn make_writer(&'a self) -> TeeWriter {
        TeeWriter(Arc::clone(&self.0))
    }
}

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Failure of one file must not lose the write to the other.
        let main = lock(&self.0.main).write_all(buf);
        let session = match &self.0.session {
            Some(s) => lock(s).write_all(buf),
            None => Ok(()),
        };
        main.and(session).map(|()| buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        let _ = lock(&self.0.main).flush();
        if let Some(s) = &self.0.session {
            let _ = lock(s).flush();
        }
        Ok(())
    }
}

/// `yyyy-mm-dd_HH-MM-SS` for a Unix time (UTC), via the days-to-civil algorithm (no time crate).
pub(crate) fn utc_stamp(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

pub(crate) fn session_file_name(unix_secs: u64) -> String {
    format!("{SESSION_PREFIX}{}{SESSION_SUFFIX}", utc_stamp(unix_secs))
}

fn is_session_file_name(name: &str) -> bool {
    let Some(stamp) = name
        .strip_prefix(SESSION_PREFIX)
        .and_then(|n| n.strip_suffix(SESSION_SUFFIX))
    else {
        return false;
    };
    stamp.len() == STAMP_LEN
        && stamp.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 | 13 | 16 => b == b'-',
            10 => b == b'_',
            _ => b.is_ascii_digit(),
        })
}

/// Delete all but the newest `keep` session logs in `dir` (names sort chronologically). Only files that
/// match the session naming are considered. Returns how many were removed.
pub(crate) fn prune_sessions(dir: &Path, keep: usize) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_session_file_name(n))
        .collect();
    names.sort();
    let excess = names.len().saturating_sub(keep);
    names[..excess]
        .iter()
        .filter(|n| std::fs::remove_file(dir.join(n)).is_ok())
        .count()
}

fn open_session_file(dir: &Path, unix_secs: u64) -> io::Result<(PathBuf, File)> {
    let logs = dir.join(SESSION_DIR_NAME);
    std::fs::create_dir_all(&logs)?;
    let path = logs.join(session_file_name(unix_secs));
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    Ok((path, file))
}

pub(crate) fn init(dir: &Path) -> Result<LogSetup, String> {
    // Mirror file: truncated, always the current session.
    let main = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dir.join(LOG_FILE_NAME))
        .map_err(|e| e.to_string())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (session_path, session, session_err) = match open_session_file(dir, now) {
        Ok((p, f)) => (Some(p), Some(Mutex::new(f)), None),
        Err(e) => (None, None, Some(e)),
    };
    let sinks = Arc::new(Sinks {
        main: Mutex::new(main),
        session,
    });

    let file_layer = fmt::layer()
        .with_writer(TeeMakeWriter(sinks))
        .with_ansi(false)
        .with_target(false)
        .with_thread_names(true);
    // Compact lines for the overlay: seconds since start instead of a full timestamp, no thread name.
    let ring_layer = fmt::layer()
        .with_writer(RingMakeWriter)
        .with_ansi(false)
        .with_target(false)
        .with_timer(fmt::time::uptime());
    tracing_subscriber::registry()
        .with(LevelFilter::INFO)
        .with(file_layer)
        .with(ring_layer)
        .try_init()
        .map_err(|e| e.to_string())?;

    if let Some(e) = session_err {
        tracing::warn!(
            "could not create the per-session log in {}: {e}; logging to {LOG_FILE_NAME} only",
            dir.join(SESSION_DIR_NAME).display()
        );
    } else {
        let removed = prune_sessions(&dir.join(SESSION_DIR_NAME), KEEP_SESSION_LOGS);
        if removed > 0 {
            tracing::info!("pruned {removed} old session log(s), keeping {KEEP_SESSION_LOGS}");
        }
    }

    // Panics on any thread get recorded in the log, then the default hook still runs.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        default_hook(info);
    }));
    Ok(LogSetup { session_path })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `init` installs the global subscriber, so this is the only test that calls it. One event must land
    /// in both files (full timestamp, thread name) and the overlay ring (compact uptime stamp).
    #[test]
    fn init_feeds_the_files_and_the_ring() {
        let dir = std::env::temp_dir().join(format!("nrpvp-log-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(
            dir.join(LOG_FILE_NAME),
            "stale line from a previous session\n",
        )
        .expect("seed");
        let setup = init(&dir).expect("global subscriber installs once");
        assert!(setup.session_path.is_some());
        tracing::info!("logging-test-marker-info");
        tracing::debug!("logging-test-marker-debug-must-be-filtered");

        let file = std::fs::read_to_string(dir.join(LOG_FILE_NAME)).expect("log file");
        assert!(file.contains("logging-test-marker-info"), "{file}");
        assert!(file.contains(" INFO "), "{file}");
        assert!(!file.contains("must-be-filtered"));
        assert!(!file.contains("stale line"), "nrpvp.log must be truncated");

        // The same bytes are in this session's file under logs/.
        let session = std::fs::read_to_string(setup.session_path.expect("session path"))
            .expect("session log");
        assert_eq!(session, file);

        let (_, lines) = crate::logring::snapshot();
        let line = lines
            .iter()
            .find(|l| l.contains("logging-test-marker-info"))
            .expect("ring has the event");
        assert!(line.contains("INFO") && !line.contains('\n'), "{line}");
        assert!(!lines.iter().any(|l| l.contains("must-be-filtered")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stamp_and_file_name() {
        assert_eq!(utc_stamp(0), "1970-01-01_00-00-00");
        assert_eq!(utc_stamp(951_782_400 + 3_661), "2000-02-29_01-01-01");
        assert_eq!(utc_stamp(1_791_503_999), "2026-10-08_23-59-59");
        assert_eq!(session_file_name(0), "nrpvp_1970-01-01_00-00-00.log");
        assert!(is_session_file_name(&session_file_name(1_791_503_999)));
        assert!(!is_session_file_name("nrpvp.log"));
        assert!(!is_session_file_name("nrpvp_1970-01-01_00-00-00.log.bak"));
        assert!(!is_session_file_name("notes.log"));
    }

    #[test]
    fn prune_keeps_newest_and_ignores_other_files() {
        let dir = std::env::temp_dir().join(format!("nrpvp-prune-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        for i in 0..35u64 {
            std::fs::write(dir.join(session_file_name(i * 100)), "x").expect("write");
        }
        std::fs::write(dir.join("keep-me.txt"), "x").expect("write");
        assert_eq!(prune_sessions(&dir, KEEP_SESSION_LOGS), 5);
        assert!(!dir.join(session_file_name(0)).exists());
        assert!(!dir.join(session_file_name(400)).exists());
        assert!(dir.join(session_file_name(500)).exists());
        assert!(dir.join(session_file_name(3_400)).exists());
        assert!(dir.join("keep-me.txt").exists());
        assert_eq!(prune_sessions(&dir, KEEP_SESSION_LOGS), 0);
        assert_eq!(prune_sessions(&dir.join("missing"), 30), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
