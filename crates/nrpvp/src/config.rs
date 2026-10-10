//! T-008: `nrpvp.toml` next to the DLL. Created with defaults if missing; the effective values are logged.
//! A file that fails to parse never stops the mod: the defaults are used and the error is logged.
//!
//! T-013 added the overlay and the configurable hotkeys. Key names are validated here (see
//! `hotkey::parse_key`); an unknown name falls back to that key's default with a warning, an empty string
//! unbinds the key.

use std::path::Path;

use serde::Deserialize;

use crate::hotkey::{self, Action, KeyBind};

pub(crate) const FILE_NAME: &str = "nrpvp.toml";

/// Hostile-to-player candidates from TECHNICAL_NOTES.md section 4 (Smithbox NR TEAM_TYPE enum; HYPOTHESIS
/// until TC-004): 6 Enemy, 29 Indiscriminate, 3 Black Ghost, 16 and 13 Invader, 2 White Ghost, 1 Player.
pub(crate) const DEFAULT_CANDIDATES: [u8; 7] = [6, 29, 3, 16, 13, 2, 1];
pub(crate) const DEFAULT_PERSIST_MS: u64 = 50;
pub(crate) const DEFAULT_WATCH_MS: u64 = 1000;
pub(crate) const DEFAULT_OVERLAY_ENABLED: bool = true;
/// Pause between the game window appearing and installing the D3D12 hooks (FS-0: Nucleus ProtoInput injects around then).
pub(crate) const DEFAULT_OVERLAY_DELAY_MS: u64 = 2000;
const OVERLAY_DELAY_MS_RANGE: (u64, u64) = (0, 600_000);
/// T-013 defaults. F4 is deliberately absent: Seamless Co-op binds it to its pause (TC-004 run 1).
pub(crate) const DEFAULT_OVERLAY_TOGGLE: &str = "Insert";
pub(crate) const DEFAULT_HOTKEY_WRITE: &str = "F5";
pub(crate) const DEFAULT_HOTKEY_RESTORE: &str = "F6";
pub(crate) const DEFAULT_HOTKEY_PERSIST: &str = "F7";
pub(crate) const DEFAULT_HOTKEY_HISTOGRAM: &str = "F8";
pub(crate) const DEFAULT_HOTKEY_DISCOVERY: &str = "F9";
/// Cycling the selection is done with the overlay buttons; this hotkey is unbound by default.
pub(crate) const DEFAULT_HOTKEY_CYCLE: &str = "";
/// T-015: numbered MARK line in the log.
pub(crate) const DEFAULT_HOTKEY_MARK: &str = "F10";
pub(crate) const DEFAULT_HOTKEY_WRITE_ALL: &str = "F11";
pub(crate) const DEFAULT_HOTKEY_RESTORE_ALL: &str = "F12";

/// Sanity bounds so a typo cannot spin a thread or silence the watcher.
const PERSIST_MS_RANGE: (u64, u64) = (10, 5000);
const WATCH_MS_RANGE: (u64, u64) = (100, 60_000);

/// Contents written when the file does not exist.
pub(crate) const DEFAULT_FILE_TEXT: &str = "\
# nrpvp configuration (T-008, T-013, T-015). Edit and restart the game; the effective values are logged at startup.

# team_type values (u8) offered by the overlay buttons and by the cycle hotkey.
team_candidates = [6, 29, 3, 16, 13, 2, 1]

# Persist mode: re-apply the selected value this often (milliseconds, 10..5000).
persist_interval_ms = 50

# How often the watcher logs PlayerIns changes (milliseconds, 100..60000).
watch_interval_ms = 1000

# In-game debug overlay (ImGui via hudhook, Direct3D 12). false = no overlay; the hotkeys below still work.
overlay_enabled = true

# Wait this long after the game window appears before hooking Direct3D 12 (0..600000 ms). Raise it (e.g. 60000) under
# Nucleus Co-op so ProtoInput injects first.
overlay_install_delay_ms = 2000

# T-018: absolute directory for the logs (default = next to the DLL). Logs go to <log_dir>\\<DLL folder name>\\nrpvp.log
# and ...\\logs\\. Use a TOML single-quoted string so backslashes stay literal. Nucleus deletes its Instance<N> folders
# on close; point this somewhere permanent. Falls back to the DLL folder if unusable. Read before logging starts.
# log_dir = 'C:\\some\\folder'

# Show/hide the overlay. The overlay starts visible. Key names: F1..F24, A..Z, 0..9, Numpad0..Numpad9,
# Insert, Delete, Home, End, PageUp, PageDown, Pause, ScrollLock. \"\" = unbound.
overlay_toggle_key = \"Insert\"

# Fallback hotkeys (only count while the game window is in the foreground). \"\" = unbound.
# F4 is not used by default because Seamless Co-op binds it to pause.
hotkey_write = \"F5\"       # write the selected candidate to the local player
hotkey_restore = \"F6\"     # restore the original value
hotkey_persist = \"F7\"     # persist mode on/off
hotkey_histogram = \"F8\"   # log a team_type histogram
hotkey_discovery = \"F9\"   # run the character discovery
hotkey_cycle = \"\"         # select the next candidate (unbound; use the overlay buttons)
hotkey_mark = \"F10\"       # log a numbered MARK line (T-015); the overlay has a text box for annotated marks
hotkey_write_all = \"F11\"   # FS-0: write the selected candidate to EVERY player (this client's copies)
hotkey_restore_all = \"F12\" # FS-0: restore every player
";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub(crate) struct Config {
    pub team_candidates: Vec<u8>,
    pub persist_interval_ms: u64,
    pub watch_interval_ms: u64,
    pub overlay_enabled: bool,
    pub overlay_install_delay_ms: u64,
    pub overlay_toggle_key: String,
    /// T-018: absolute log directory; empty = next to the DLL.
    pub log_dir: String,
    pub hotkey_write: String,
    pub hotkey_restore: String,
    pub hotkey_persist: String,
    pub hotkey_histogram: String,
    pub hotkey_discovery: String,
    pub hotkey_cycle: String,
    pub hotkey_mark: String,
    pub hotkey_write_all: String,
    pub hotkey_restore_all: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            team_candidates: DEFAULT_CANDIDATES.to_vec(),
            persist_interval_ms: DEFAULT_PERSIST_MS,
            watch_interval_ms: DEFAULT_WATCH_MS,
            overlay_enabled: DEFAULT_OVERLAY_ENABLED,
            overlay_install_delay_ms: DEFAULT_OVERLAY_DELAY_MS,
            overlay_toggle_key: DEFAULT_OVERLAY_TOGGLE.to_string(),
            log_dir: String::new(),
            hotkey_write: DEFAULT_HOTKEY_WRITE.to_string(),
            hotkey_restore: DEFAULT_HOTKEY_RESTORE.to_string(),
            hotkey_persist: DEFAULT_HOTKEY_PERSIST.to_string(),
            hotkey_histogram: DEFAULT_HOTKEY_HISTOGRAM.to_string(),
            hotkey_discovery: DEFAULT_HOTKEY_DISCOVERY.to_string(),
            hotkey_cycle: DEFAULT_HOTKEY_CYCLE.to_string(),
            hotkey_mark: DEFAULT_HOTKEY_MARK.to_string(),
            hotkey_write_all: DEFAULT_HOTKEY_WRITE_ALL.to_string(),
            hotkey_restore_all: DEFAULT_HOTKEY_RESTORE_ALL.to_string(),
        }
    }
}

impl Config {
    /// The configured key name for `a` (possibly empty = unbound).
    fn key_text(&self, a: Action) -> &str {
        match a {
            Action::Write => &self.hotkey_write,
            Action::Restore => &self.hotkey_restore,
            Action::Persist => &self.hotkey_persist,
            Action::Histogram => &self.hotkey_histogram,
            Action::Discovery => &self.hotkey_discovery,
            Action::Cycle => &self.hotkey_cycle,
            Action::ToggleOverlay => &self.overlay_toggle_key,
            Action::Mark => &self.hotkey_mark,
            Action::WriteAll => &self.hotkey_write_all,
            Action::RestoreAll => &self.hotkey_restore_all,
        }
    }

    /// The resolved key for `a`, or `None` if unbound (or, only possible on an unvalidated `Config`, if
    /// the name is unknown).
    pub(crate) fn binding(&self, a: Action) -> Option<KeyBind> {
        hotkey::parse_key(self.key_text(a))
    }

    /// Every bound action with its key, for the hotkey thread and for log labels.
    pub(crate) fn bindings(&self) -> Vec<(Action, KeyBind)> {
        hotkey::ALL_ACTIONS
            .iter()
            .filter_map(|&a| self.binding(a).map(|k| (a, k)))
            .collect()
    }

    /// `write=F5 restore=F6 ...` for the startup log; unbound actions show `-`.
    pub(crate) fn describe_bindings(&self) -> String {
        let one = |label: &str, a: Action| {
            format!(
                "{label}={}",
                self.binding(a).map_or("-".to_string(), |k| k.name)
            )
        };
        [
            one("write", Action::Write),
            one("restore", Action::Restore),
            one("persist", Action::Persist),
            one("histogram", Action::Histogram),
            one("discovery", Action::Discovery),
            one("cycle", Action::Cycle),
            one("overlay-toggle", Action::ToggleOverlay),
            one("mark", Action::Mark),
            one("write_all", Action::WriteAll),
            one("restore_all", Action::RestoreAll),
        ]
        .join(" ")
    }
}

/// Parse and sanitise. `Err` only for TOML/type errors; out-of-range values and unknown key names are
/// fixed and reported in the returned warnings.
pub(crate) fn parse(text: &str) -> Result<(Config, Vec<String>), String> {
    let mut cfg: Config = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut warnings = Vec::new();
    if cfg.team_candidates.is_empty() {
        warnings.push("team_candidates is empty; using the defaults".to_string());
        cfg.team_candidates = DEFAULT_CANDIDATES.to_vec();
    }
    let mut clamp = |name: &str, v: &mut u64, (lo, hi): (u64, u64)| {
        let c = (*v).clamp(lo, hi);
        if c != *v {
            warnings.push(format!("{name} = {v} is outside {lo}..={hi}; using {c}"));
            *v = c;
        }
    };
    clamp(
        "persist_interval_ms",
        &mut cfg.persist_interval_ms,
        PERSIST_MS_RANGE,
    );
    clamp(
        "watch_interval_ms",
        &mut cfg.watch_interval_ms,
        WATCH_MS_RANGE,
    );
    clamp(
        "overlay_install_delay_ms",
        &mut cfg.overlay_install_delay_ms,
        OVERLAY_DELAY_MS_RANGE,
    );

    if !cfg.log_dir.trim().is_empty() && !Path::new(cfg.log_dir.trim()).is_absolute() {
        warnings.push(format!(
            "log_dir = {:?} is not an absolute path; logging next to the DLL",
            cfg.log_dir
        ));
    }

    let mut key = |name: &str, v: &mut String, default: &str| {
        if v.trim().is_empty() {
            v.clear(); // unbound
        } else if hotkey::parse_key(v).is_none() {
            warnings.push(format!(
                "{name} = {v:?} is not a known key name; using {}",
                if default.is_empty() {
                    "unbound".to_string()
                } else {
                    format!("{default:?}")
                }
            ));
            *v = default.to_string();
        }
    };
    key(
        "overlay_toggle_key",
        &mut cfg.overlay_toggle_key,
        DEFAULT_OVERLAY_TOGGLE,
    );
    key("hotkey_write", &mut cfg.hotkey_write, DEFAULT_HOTKEY_WRITE);
    key(
        "hotkey_restore",
        &mut cfg.hotkey_restore,
        DEFAULT_HOTKEY_RESTORE,
    );
    key(
        "hotkey_persist",
        &mut cfg.hotkey_persist,
        DEFAULT_HOTKEY_PERSIST,
    );
    key(
        "hotkey_histogram",
        &mut cfg.hotkey_histogram,
        DEFAULT_HOTKEY_HISTOGRAM,
    );
    key(
        "hotkey_discovery",
        &mut cfg.hotkey_discovery,
        DEFAULT_HOTKEY_DISCOVERY,
    );
    key("hotkey_cycle", &mut cfg.hotkey_cycle, DEFAULT_HOTKEY_CYCLE);
    key("hotkey_mark", &mut cfg.hotkey_mark, DEFAULT_HOTKEY_MARK);
    key(
        "hotkey_write_all",
        &mut cfg.hotkey_write_all,
        DEFAULT_HOTKEY_WRITE_ALL,
    );
    key(
        "hotkey_restore_all",
        &mut cfg.hotkey_restore_all,
        DEFAULT_HOTKEY_RESTORE_ALL,
    );

    // Two actions on one key both fire; that is allowed but almost certainly a mistake.
    let bound = cfg.bindings();
    for (i, (a, ka)) in bound.iter().enumerate() {
        for (b, kb) in &bound[i + 1..] {
            if ka.vk == kb.vk {
                warnings.push(format!(
                    "{a:?} and {b:?} are both bound to {}; both will fire",
                    ka.name
                ));
            }
        }
    }
    Ok((cfg, warnings))
}

/// T-018: the raw `log_dir` from `<dir>/nrpvp.toml`, read before logging exists. Any problem (no file,
/// bad TOML, wrong type) gives `None` (= log next to the DLL); `load` reports parse errors later.
pub(crate) fn peek_log_dir(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(FILE_NAME)).ok()?;
    parse_log_dir(&text)
}

fn parse_log_dir(text: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Peek {
        log_dir: Option<String>,
    }
    let p: Peek = toml::from_str(text).ok()?;
    p.log_dir.filter(|s| !s.trim().is_empty())
}

/// Load `<dir>/nrpvp.toml`, creating it with defaults if it does not exist. Never fails.
pub(crate) fn load(dir: &Path) -> Config {
    let path = dir.join(FILE_NAME);
    let cfg = match std::fs::read_to_string(&path) {
        Ok(text) => match parse(&text) {
            Ok((cfg, warnings)) => {
                tracing::info!("T-008: read config {}", path.display());
                for w in warnings {
                    tracing::warn!("T-008: config: {w}");
                }
                cfg
            }
            Err(e) => {
                tracing::warn!(
                    "T-008: could not parse {}: {e}; using the defaults",
                    path.display()
                );
                Config::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::write(&path, DEFAULT_FILE_TEXT) {
                Ok(()) => tracing::info!("T-008: created default config {}", path.display()),
                Err(e) => tracing::warn!(
                    "T-008: could not create {}: {e}; using the defaults",
                    path.display()
                ),
            }
            Config::default()
        }
        Err(e) => {
            tracing::warn!(
                "T-008: could not read {}: {e}; using the defaults",
                path.display()
            );
            Config::default()
        }
    };
    tracing::info!(
        "T-008: effective config: team_candidates={:?} persist_interval_ms={} watch_interval_ms={} overlay_enabled={} log_dir={:?} keys: {}",
        cfg.team_candidates,
        cfg.persist_interval_ms,
        cfg.watch_interval_ms,
        cfg.overlay_enabled,
        cfg.log_dir,
        cfg.describe_bindings()
    );
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_text_parses_to_defaults() {
        let (cfg, warnings) = parse(DEFAULT_FILE_TEXT).expect("default text parses");
        assert_eq!(cfg, Config::default());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(cfg.team_candidates, vec![6, 29, 3, 16, 13, 2, 1]);
        assert_eq!((cfg.persist_interval_ms, cfg.watch_interval_ms), (50, 1000));
    }

    #[test]
    fn new_keys_have_the_documented_defaults() {
        let c = Config::default();
        assert!(c.overlay_enabled);
        assert_eq!(c.overlay_toggle_key, "Insert");
        assert_eq!(
            (
                c.hotkey_write.as_str(),
                c.hotkey_restore.as_str(),
                c.hotkey_persist.as_str(),
                c.hotkey_histogram.as_str(),
                c.hotkey_discovery.as_str(),
                c.hotkey_cycle.as_str(),
                c.hotkey_mark.as_str(),
            ),
            ("F5", "F6", "F7", "F8", "F9", "", "F10")
        );
        // F4 (Seamless Co-op pause) is bound to nothing by default.
        let f4 = hotkey::parse_key("F4").expect("F4 parses").vk;
        assert!(c.bindings().iter().all(|(_, k)| k.vk != f4));
        // Cycle is unbound; the other seven actions are bound.
        assert_eq!(c.bindings().len(), 9);
        assert!(c.binding(Action::Cycle).is_none());
        assert_eq!(
            c.binding(Action::ToggleOverlay).map(|k| (k.vk, k.name)),
            Some((0x2D, "Insert".to_string()))
        );
        assert_eq!(
            c.describe_bindings(),
            "write=F5 restore=F6 persist=F7 histogram=F8 discovery=F9 cycle=- overlay-toggle=Insert mark=F10 write_all=F11 restore_all=F12"
        );
    }

    #[test]
    fn new_keys_parse_and_normalise() {
        let (cfg, w) = parse(
            "overlay_enabled = false\noverlay_toggle_key = \"home\"\nhotkey_write = \"f11\"\nhotkey_write_all = \"f13\"\nhotkey_cycle = \"F4\"\nhotkey_restore = \"\"\nhotkey_persist = \"  \"\n",
        )
        .expect("ok");
        assert!(w.is_empty(), "{w:?}");
        assert!(!cfg.overlay_enabled);
        assert_eq!(
            cfg.binding(Action::ToggleOverlay).map(|k| k.name),
            Some("Home".into())
        );
        assert_eq!(cfg.binding(Action::Write).map(|k| k.vk), Some(0x7A));
        assert_eq!(
            cfg.binding(Action::Cycle).map(|k| k.name),
            Some("F4".into())
        );
        // Empty / blank strings unbind.
        assert_eq!(cfg.binding(Action::Restore), None);
        assert_eq!(cfg.binding(Action::Persist), None);
        assert_eq!(cfg.hotkey_persist, "");
        // Untouched keys keep their defaults.
        assert_eq!(cfg.hotkey_histogram, "F8");
        assert_eq!(cfg.hotkey_discovery, "F9");
    }

    #[test]
    fn unknown_key_names_fall_back_to_the_default_with_a_warning() {
        let (cfg, w) = parse(
            "overlay_toggle_key = \"Esc\"\nhotkey_write = \"banana\"\nhotkey_cycle = \"nope\"\n",
        )
        .expect("ok");
        assert_eq!(w.len(), 3, "{w:?}");
        assert_eq!(cfg.overlay_toggle_key, "Insert");
        assert_eq!(cfg.hotkey_write, "F5");
        assert_eq!(cfg.hotkey_cycle, ""); // its default is unbound
        assert!(w[0].contains("overlay_toggle_key") && w[0].contains("Esc"));
    }

    #[test]
    fn two_actions_on_one_key_warn() {
        let (_, w) = parse("hotkey_write = \"F6\"\n").expect("ok"); // restore is F6 too
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("Write") && w[0].contains("Restore") && w[0].contains("F6"));
    }

    #[test]
    fn non_string_key_values_are_errors() {
        assert!(parse("hotkey_write = 5").is_err());
        assert!(parse("overlay_enabled = \"yes\"").is_err());
    }

    #[test]
    fn log_dir_parses_and_peeks() {
        assert_eq!(Config::default().log_dir, "");
        let (cfg, w) = parse("log_dir = 'D:\\repo\\dist\\nucleus-logs'\n").expect("ok");
        assert_eq!(cfg.log_dir, "D:\\repo\\dist\\nucleus-logs");
        assert!(w.is_empty(), "{w:?}");
        // The peek ignores type errors in other keys (the full parse reports them later).
        assert_eq!(
            parse_log_dir("overlay_enabled = \"junk\"\nlog_dir = 'D:\\x'\n"),
            Some("D:\\x".to_string())
        );
        assert_eq!(
            parse_log_dir("team_candidates = [1]\nlog_dir = 'D:\\x'\n"),
            Some("D:\\x".to_string())
        );
        assert_eq!(parse_log_dir("log_dir = \"\"\n"), None);
        assert_eq!(parse_log_dir("log_dir = 5"), None);
        assert_eq!(parse_log_dir("not toml"), None);
        assert_eq!(parse_log_dir(""), None);
        let (_, w) = parse("log_dir = 'rel\\dir'\n").expect("ok");
        assert_eq!(w.len(), 1, "{w:?}");
    }

    #[test]
    fn empty_file_gives_defaults() {
        let (cfg, w) = parse("").expect("empty parses");
        assert_eq!(cfg, Config::default());
        assert!(w.is_empty());
    }

    #[test]
    fn partial_file_overrides_only_given_keys() {
        let (cfg, _) = parse("team_candidates = [13, 6]\nwatch_interval_ms = 250\n").expect("ok");
        assert_eq!(cfg.team_candidates, vec![13, 6]);
        assert_eq!(cfg.watch_interval_ms, 250);
        assert_eq!(cfg.persist_interval_ms, DEFAULT_PERSIST_MS);
        assert_eq!(cfg.overlay_toggle_key, "Insert");
    }

    #[test]
    fn out_of_range_values_are_fixed_with_warnings() {
        let (cfg, w) =
            parse("team_candidates = []\npersist_interval_ms = 1\nwatch_interval_ms = 999999\n")
                .expect("ok");
        assert_eq!(cfg.team_candidates, DEFAULT_CANDIDATES.to_vec());
        assert_eq!(cfg.persist_interval_ms, 10);
        assert_eq!(cfg.watch_interval_ms, 60_000);
        assert_eq!(w.len(), 3);
    }

    #[test]
    fn bad_types_and_values_are_errors() {
        assert!(parse("team_candidates = [300]").is_err()); // does not fit u8
        assert!(parse("team_candidates = \"six\"").is_err());
        assert!(parse("persist_interval_ms = -5").is_err());
        assert!(parse("this is not toml").is_err());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let (cfg, _) = parse("hostile_team_type = 6\n").expect("ok");
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn load_creates_the_default_file() {
        let dir = std::env::temp_dir().join(format!("nrpvp-cfg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let cfg = load(&dir);
        assert_eq!(cfg, Config::default());
        let written = std::fs::read_to_string(dir.join(FILE_NAME)).expect("file created");
        assert_eq!(written, DEFAULT_FILE_TEXT);
        // Second load reads it back.
        assert_eq!(load(&dir), Config::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
