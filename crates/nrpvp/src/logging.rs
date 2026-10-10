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
    /// Directory holding `nrpvp.log` and `logs/` (the DLL dir, or `<log_dir>/<tag>`).
    pub(crate) log_dir: PathBuf,
    /// Why a configured `log_dir` was not used (logged by the banner), if so.
    pub(crate) fallback_note: Option<String>,
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

/// T-018: instance tag = the DLL's parent folder name (e.g. `Instance0`) reduced to `[A-Za-z0-9_-]`;
/// every other character becomes `_`. An empty name gives `instance`.
pub(crate) fn instance_tag(dll_dir: &Path) -> String {
    let raw = dll_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tag: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if tag.is_empty() {
        "instance".to_string()
    } else {
        tag
    }
}

/// T-018: where the logs go. `log_dir` empty/None, not absolute, or not creatable: the DLL dir (with a
/// note for the latter two). Otherwise `<log_dir>/<tag>`.
pub(crate) fn resolve_dir(dll_dir: &Path, log_dir: Option<&str>) -> (PathBuf, Option<String>) {
    let Some(configured) = log_dir.map(str::trim).filter(|s| !s.is_empty()) else {
        return (dll_dir.to_path_buf(), None);
    };
    let base = Path::new(configured);
    if !base.is_absolute() {
        return (
            dll_dir.to_path_buf(),
            Some(format!("log_dir {configured:?} is not an absolute path")),
        );
    }
    let target = base.join(instance_tag(dll_dir));
    match std::fs::create_dir_all(&target) {
        Ok(()) => (target, None),
        Err(e) => (
            dll_dir.to_path_buf(),
            Some(format!("could not create {}: {e}", target.display())),
        ),
    }
}

fn open_session_file(dir: &Path, unix_secs: u64) -> io::Result<(PathBuf, File)> {
    let logs = dir.join(SESSION_DIR_NAME);
    std::fs::create_dir_all(&logs)?;
    let path = logs.join(session_file_name(unix_secs));
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    Ok((path, file))
}

/// `log_dir` is the raw `log_dir` setting (see `config::peek_log_dir`); `None` = next to the DLL.
pub(crate) fn init(dll_dir: &Path, log_dir: Option<&str>) -> Result<LogSetup, String> {
    let (mut dir, mut fallback_note) = resolve_dir(dll_dir, log_dir);
    // Mirror file: truncated, always the current session.
    let open_main = |d: &Path| {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(d.join(LOG_FILE_NAME))
    };
    let main = match open_main(&dir) {
        Ok(f) => f,
        Err(e) if dir != dll_dir => {
            fallback_note = Some(format!("could not open {}: {e}", dir.display()));
            dir = dll_dir.to_path_buf();
            open_main(&dir).map_err(|e| e.to_string())?
        }
        Err(e) => return Err(e.to_string()),
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (session_path, session, session_err) = match open_session_file(&dir, now) {
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
    Ok(LogSetup {
        log_dir: dir,
        fallback_note,
        session_path,
    })
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
        let setup = init(&dir, None).expect("global subscriber installs once");
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
    fn instance_tag_is_sanitised() {
        assert_eq!(instance_tag(Path::new("x/Instance0")), "Instance0");
        assert_eq!(instance_tag(Path::new("x/My Inst.1")), "My_Inst_1");
        assert_eq!(instance_tag(Path::new("a-b_C9")), "a-b_C9");
        assert_eq!(instance_tag(Path::new("")), "instance");
        assert_eq!(instance_tag(Path::new("/")), "instance");
    }

    #[test]
    fn resolve_dir_uses_tag_and_falls_back() {
        let root = std::env::temp_dir().join(format!("nrpvp-resolve-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dll = root.join("Instance3");
        let out = root.join("out");
        assert_eq!(resolve_dir(&dll, None), (dll.clone(), None));
        assert_eq!(resolve_dir(&dll, Some("  ")), (dll.clone(), None));
        let (d, n) = resolve_dir(&dll, out.to_str());
        assert_eq!(d, out.join("Instance3"));
        assert!(n.is_none() && d.is_dir());
        let (d, n) = resolve_dir(&dll, Some("relative/dir"));
        assert_eq!(d, dll);
        assert!(n.expect("note").contains("absolute"));
        // A file in the way makes create_dir_all fail.
        std::fs::write(root.join("blocker"), "x").expect("file");
        let blocked = root.join("blocker");
        let (d, n) = resolve_dir(&dll, blocked.to_str());
        assert_eq!(d, dll);
        assert!(n.is_some());
        let _ = std::fs::remove_dir_all(&root);
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
