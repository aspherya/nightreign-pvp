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
/// T-020: team_type the PvPvE mode writes over the expedition default (2).
pub(crate) const DEFAULT_PVPVE_TEAM: u8 = 29;
pub(crate) const DEFAULT_PVPVE_INTERVAL_MS: u64 = 250;
const PVPVE_INTERVAL_MS_RANGE: (u64, u64) = (50, 5000);
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
/// T-027: read-only probes (timeline flags, world clock, PlayerGameData candidates); off by default.
pub(crate) const DEFAULT_PROBE_INTERVAL_MS: u64 = 1000;
const PROBE_INTERVAL_MS_RANGE: (u64, u64) = (100, 60_000);

/// Contents written when the file does not exist.
pub(crate) const DEFAULT_FILE_TEXT: &str = "\
# nrpvp configuration (T-008, T-013, T-015). Edit and restart the game; the effective values are logged at startup.

# team_type values (u8) offered by the overlay buttons and by the cycle hotkey.
team_candidates = [6, 29, 3, 16, 13, 2, 1]

# Persist mode: re-apply the selected value this often (milliseconds, 10..5000).
persist_interval_ms = 50

# How often the watcher logs PlayerIns changes (milliseconds, 100..60000).
watch_interval_ms = 1000

# T-030: false (default) = player build: PvP always on, no hotkeys, no debug panel (only a small BATTLE ROYALE badge).
# true = developer mode (all hotkeys, the debug panel, discovery, pause/resume).
dev_mode = false

# In-game debug overlay (ImGui via hudhook, Direct3D 12). false = no overlay; the hotkeys below still work.
overlay_enabled = true

# Wait this long after the game window appears before hooking Direct3D 12 (0..600000 ms). Raise it (e.g. 60000) under
# Nucleus Co-op so ProtoInput injects first.
overlay_install_delay_ms = 2000

# T-020 PvPvE mode: while on, this client rewrites its own copy of team_type for EVERY player whose value is 2 (the
# expedition default) to pvpve_team, every pvpve_interval_ms (50..5000). Other values (e.g. 1 in the Roundtable) are
# never touched. F12 pauses the mode (and restores captured originals), F11 resumes it; the overlay has a checkbox.
# Every client in the session must run it: hostility is decided from each client's own copies.
# pvpve = true   (dev mode only; the player build always has PvP on)
pvpve_team = 29
pvpve_interval_ms = 250
# T-026 scheme. \"rivals\" (default): this client writes pvpve_team only to players that are RIVALS of the local player
# (per the rosters below); itself and its allies stay on the vanilla value, so graces, pickups and revives keep working.
# \"all\": every player with value 2 is rewritten (the T-020 behaviour). Anything else: warning + \"rivals\".
pvpve_scheme = \"rivals\"
# Players per team (1..=3). 1 = free-for-all. Rosters come from the per-player id (ChrIns+0x78), sorted, chunked by team_size.
team_size = 1
# Deprecated alias (FS-0): auto_team_all = N means pvpve = true, pvpve_team = N when pvpve is not set. 0 = off.
auto_team_all = 0

# T-027 read-only probes (UNVERIFIED ER-layout hypotheses): while on, every probe_interval_ms (100..60000) the mod reads the
# timeline event flags, the world clock and PlayerGameData candidates, logs changes, and shows them in the overlay.
# Never writes game memory.
probes = false
probe_interval_ms = 1000

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
    /// T-030: developer mode. false (default) = player build: no hotkeys, PvPvE forced on with the rivals scheme, no
    /// discovery or probes, a read-only badge instead of the debug panel.
    pub dev_mode: bool,
    pub team_candidates: Vec<u8>,
    pub persist_interval_ms: u64,
    pub watch_interval_ms: u64,
    pub overlay_enabled: bool,
    pub overlay_install_delay_ms: u64,
    /// T-020: PvPvE mode on (after the alias is resolved in `parse`).
    pub pvpve: bool,
    pub pvpve_team: u8,
    pub pvpve_interval_ms: u64,
    /// T-026: `"rivals"` or `"all"` (normalised in `parse`; read it through [`Config::scheme`]).
    pub pvpve_scheme: String,
    /// T-026: players per team, clamped to 1..=3 in `parse`.
    pub team_size: u64,
    /// Deprecated FS-0 alias of `pvpve` + `pvpve_team`.
    pub auto_team_all: u32,
    /// T-027: read-only probes on.
    pub probes: bool,
    /// T-027: probe period in milliseconds (100..=60000).
    pub probe_interval_ms: u64,
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
            dev_mode: false,
            team_candidates: DEFAULT_CANDIDATES.to_vec(),
            persist_interval_ms: DEFAULT_PERSIST_MS,
            watch_interval_ms: DEFAULT_WATCH_MS,
            overlay_enabled: DEFAULT_OVERLAY_ENABLED,
            overlay_install_delay_ms: DEFAULT_OVERLAY_DELAY_MS,
            pvpve: false,
            pvpve_team: DEFAULT_PVPVE_TEAM,
            pvpve_interval_ms: DEFAULT_PVPVE_INTERVAL_MS,
            pvpve_scheme: SCHEME_RIVALS.to_string(),
            team_size: DEFAULT_TEAM_SIZE,
            auto_team_all: 0,
            probes: false,
            probe_interval_ms: DEFAULT_PROBE_INTERVAL_MS,
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

/// T-026: which players the PvPvE mode rewrites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scheme {
    /// Only rivals of the local player (default).
    Rivals,
    /// Every player with value 2 (T-020 behaviour).
    All,
}

impl Scheme {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Scheme::Rivals => SCHEME_RIVALS,
            Scheme::All => SCHEME_ALL,
        }
    }
}

const SCHEME_RIVALS: &str = "rivals";
const SCHEME_ALL: &str = "all";
const DEFAULT_TEAM_SIZE: u64 = 1;
const TEAM_SIZE_RANGE: (u64, u64) = (1, 3);

impl Config {
    /// T-026: the scheme (an unknown text, only possible on an unvalidated `Config`, means rivals).
    pub(crate) fn scheme(&self) -> Scheme {
        if self.pvpve_scheme == SCHEME_ALL {
            Scheme::All
        } else {
            Scheme::Rivals
        }
    }

    /// T-026: players per team, within 1..=3.
    pub(crate) fn team_size_usize(&self) -> usize {
        usize::try_from(self.team_size.clamp(TEAM_SIZE_RANGE.0, TEAM_SIZE_RANGE.1)).unwrap_or(1)
    }

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
    ///
    /// T-030: always `None` outside dev mode, so no key is ever bound in the player build.
    pub(crate) fn binding(&self, a: Action) -> Option<KeyBind> {
        if !self.dev_mode {
            return None;
        }
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
        if !self.dev_mode {
            return "none (player mode)".to_string();
        }
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

/// T-030: the player build fixes the mode: PvPvE on, rivals scheme, no probes. `explicit_off` / `explicit_all` say
/// whether the file itself asked for `pvpve = false` / `pvpve_scheme = "all"` (only then is a log line worth writing).
fn enforce_player_mode(
    cfg: &mut Config,
    explicit_off: bool,
    explicit_all: bool,
    warnings: &mut Vec<String>,
) {
    if cfg.dev_mode {
        return;
    }
    if explicit_off {
        warnings.push("pvpve = false is ignored: PvP is always on in the player build".to_string());
    }
    if explicit_all {
        warnings.push(
            "pvpve_scheme = \"all\" is ignored: the player build always uses \"rivals\""
                .to_string(),
        );
    }
    cfg.pvpve = true;
    cfg.pvpve_scheme = SCHEME_RIVALS.to_string();
    cfg.probes = false;
}

/// What the file set explicitly (before defaults and the alias): `pvpve = false`, `pvpve_scheme = "all"`.
fn peek_pvpve_overrides(text: &str) -> (bool, bool) {
    #[derive(Deserialize)]
    struct Peek {
        pvpve: Option<bool>,
        pvpve_scheme: Option<String>,
    }
    match toml::from_str::<Peek>(text) {
        Ok(p) => (
            p.pvpve == Some(false),
            p.pvpve_scheme
                .is_some_and(|s| s.trim().eq_ignore_ascii_case(SCHEME_ALL)),
        ),
        Err(_) => (false, false),
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
    clamp(
        "pvpve_interval_ms",
        &mut cfg.pvpve_interval_ms,
        PVPVE_INTERVAL_MS_RANGE,
    );
    clamp("team_size", &mut cfg.team_size, TEAM_SIZE_RANGE);
    clamp(
        "probe_interval_ms",
        &mut cfg.probe_interval_ms,
        PROBE_INTERVAL_MS_RANGE,
    );
    let scheme = cfg.pvpve_scheme.trim().to_ascii_lowercase();
    if scheme == SCHEME_RIVALS || scheme == SCHEME_ALL {
        cfg.pvpve_scheme = scheme;
    } else {
        warnings.push(format!(
            "pvpve_scheme = {:?} is not \"rivals\" or \"all\"; using \"{SCHEME_RIVALS}\"",
            cfg.pvpve_scheme
        ));
        cfg.pvpve_scheme = SCHEME_RIVALS.to_string();
    }
    // The mode only replaces the expedition default (2), so 2 would be a no-op and 0 is "None".
    if cfg.pvpve_team == 0 || cfg.pvpve_team == 2 {
        warnings.push(format!(
            "pvpve_team = {} is not usable (0 = None, 2 = the value it replaces); using {DEFAULT_PVPVE_TEAM}",
            cfg.pvpve_team
        ));
        cfg.pvpve_team = DEFAULT_PVPVE_TEAM;
    }
    // Deprecated alias: auto_team_all = N means pvpve = true, pvpve_team = N, unless pvpve is set explicitly.
    if cfg.auto_team_all != 0 {
        let pvpve_set = pvpve_is_set(text);
        match u8::try_from(cfg.auto_team_all) {
            _ if pvpve_set => warnings.push(format!(
                "auto_team_all = {} is deprecated and ignored because pvpve is set",
                cfg.auto_team_all
            )),
            Ok(n) if n != 2 => {
                warnings.push(format!(
                    "auto_team_all = {n} is deprecated; treating it as pvpve = true, pvpve_team = {n} (set pvpve explicitly)"
                ));
                cfg.pvpve = true;
                cfg.pvpve_team = n;
            }
            _ => warnings.push(format!(
                "auto_team_all = {} is deprecated and not a usable team value (1..=255 except 2); ignored",
                cfg.auto_team_all
            )),
        }
    }

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

    let (explicit_off, explicit_all) = peek_pvpve_overrides(text);
    enforce_player_mode(&mut cfg, explicit_off, explicit_all, &mut warnings);

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

/// True if the file sets `pvpve` explicitly (so the deprecated `auto_team_all` alias must not override it).
fn pvpve_is_set(text: &str) -> bool {
    #[derive(Deserialize)]
    struct Peek {
        pvpve: Option<bool>,
    }
    toml::from_str::<Peek>(text).is_ok_and(|p| p.pvpve.is_some())
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

/// The config used when the file is missing or unreadable: the defaults, with the player-mode rules applied.
fn fallback() -> Config {
    let mut cfg = Config::default();
    enforce_player_mode(&mut cfg, false, false, &mut Vec::new());
    cfg
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
                fallback()
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
            fallback()
        }
        Err(e) => {
            tracing::warn!(
                "T-008: could not read {}: {e}; using the defaults",
                path.display()
            );
            fallback()
        }
    };
    tracing::info!("mode: {}", if cfg.dev_mode { "DEV" } else { "PLAYER" });
    tracing::info!(
        "T-008: effective config: team_candidates={:?} persist_interval_ms={} watch_interval_ms={} overlay_enabled={} pvpve={} pvpve_team={} pvpve_interval_ms={} pvpve_scheme={} team_size={} log_dir={:?} keys: {}",
        cfg.team_candidates,
        cfg.persist_interval_ms,
        cfg.watch_interval_ms,
        cfg.overlay_enabled,
        cfg.pvpve,
        cfg.pvpve_team,
        cfg.pvpve_interval_ms,
        cfg.pvpve_scheme,
        cfg.team_size,
        cfg.log_dir,
        cfg.describe_bindings()
    );
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a player-mode parse of an empty file gives.
    fn player_default() -> Config {
        Config {
            pvpve: true,
            ..Config::default()
        }
    }

    fn dev_default() -> Config {
        Config {
            dev_mode: true,
            ..Config::default()
        }
    }

    fn parse_dev(text: &str) -> Result<(Config, Vec<String>), String> {
        parse(&format!(
            "dev_mode = true
{text}"
        ))
    }

    #[test]
    fn default_file_text_parses_to_defaults() {
        let (cfg, warnings) = parse(DEFAULT_FILE_TEXT).expect("default text parses");
        assert_eq!(cfg, player_default());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(cfg.team_candidates, vec![6, 29, 3, 16, 13, 2, 1]);
        assert_eq!((cfg.persist_interval_ms, cfg.watch_interval_ms), (50, 1000));
    }

    #[test]
    fn new_keys_have_the_documented_defaults() {
        let c = dev_default();
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
        let (cfg, w) = parse_dev(
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
        let (cfg, w) = parse_dev(
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
        let (_, w) = parse_dev("hotkey_write = \"F6\"\n").expect("ok"); // restore is F6 too
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("Write") && w[0].contains("Restore") && w[0].contains("F6"));
    }

    #[test]
    fn non_string_key_values_are_errors() {
        assert!(parse_dev("hotkey_write = 5").is_err());
        assert!(parse_dev("overlay_enabled = \"yes\"").is_err());
    }

    #[test]
    fn log_dir_parses_and_peeks() {
        assert_eq!(dev_default().log_dir, "");
        let (cfg, w) = parse_dev("log_dir = 'D:\\repo\\dist\\nucleus-logs'\n").expect("ok");
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
        let (_, w) = parse_dev("log_dir = 'rel\\dir'\n").expect("ok");
        assert_eq!(w.len(), 1, "{w:?}");
    }

    #[test]
    fn pvpve_defaults_and_parsing() {
        let c = dev_default();
        assert_eq!(
            (c.pvpve, c.pvpve_team, c.pvpve_interval_ms),
            (false, 29, 250)
        );
        let (cfg, w) =
            parse_dev("pvpve = true\npvpve_team = 13\npvpve_interval_ms = 100\n").expect("ok");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(
            (cfg.pvpve, cfg.pvpve_team, cfg.pvpve_interval_ms),
            (true, 13, 100)
        );
        // Out-of-range interval and unusable teams are fixed with warnings.
        let (cfg, w) = parse_dev("pvpve_interval_ms = 1\npvpve_team = 2\n").expect("ok");
        assert_eq!((cfg.pvpve_interval_ms, cfg.pvpve_team), (50, 29));
        assert_eq!(w.len(), 2, "{w:?}");
        let (cfg, w) = parse_dev("pvpve_team = 0\n").expect("ok");
        assert_eq!(cfg.pvpve_team, 29);
        assert_eq!(w.len(), 1);
        assert!(parse_dev("pvpve = \"yes\"").is_err());
        assert!(parse_dev("pvpve_team = 300").is_err());
    }

    #[test]
    fn probe_keys_default_off_and_are_clamped() {
        let c = dev_default();
        assert_eq!((c.probes, c.probe_interval_ms), (false, 1000));
        let (cfg, w) = parse_dev("probes = true\nprobe_interval_ms = 250\n").expect("ok");
        assert_eq!((cfg.probes, cfg.probe_interval_ms), (true, 250));
        assert!(w.is_empty(), "{w:?}");
        let (cfg, w) = parse_dev("probe_interval_ms = 1\n").expect("ok");
        assert_eq!(cfg.probe_interval_ms, 100);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(parse_dev("probes = \"yes\"").is_err());
    }

    #[test]
    fn scheme_and_team_size_parse_with_defaults_and_warnings() {
        let (c, w) = parse_dev("").expect("ok");
        assert_eq!((c.scheme(), c.team_size_usize()), (Scheme::Rivals, 1));
        assert!(w.is_empty());
        let (c, w) = parse_dev("pvpve_scheme = \"all\"\nteam_size = 3\n").expect("ok");
        assert_eq!((c.scheme(), c.team_size_usize()), (Scheme::All, 3));
        assert!(w.is_empty());
        let (c, _) = parse_dev("pvpve_scheme = \" ALL \"\n").expect("ok");
        assert_eq!(c.scheme(), Scheme::All);
        let (c, w) = parse_dev("pvpve_scheme = \"bogus\"\nteam_size = 9\n").expect("ok");
        assert_eq!((c.scheme(), c.team_size_usize()), (Scheme::Rivals, 3));
        assert_eq!(w.len(), 2, "{w:?}");
        let (c, w) = parse_dev("team_size = 0\n").expect("ok");
        assert_eq!(c.team_size_usize(), 1);
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn auto_team_all_is_a_deprecated_alias() {
        // Unset pvpve + non-zero alias: mode on with that team, one warning.
        let (cfg, w) = parse_dev("auto_team_all = 29\n").expect("ok");
        assert_eq!((cfg.pvpve, cfg.pvpve_team), (true, 29));
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("deprecated"));
        let (cfg, _) = parse_dev("auto_team_all = 6\n").expect("ok");
        assert_eq!((cfg.pvpve, cfg.pvpve_team), (true, 6));
        // Alias 0 does nothing and says nothing.
        let (cfg, w) = parse_dev("auto_team_all = 0\n").expect("ok");
        assert!(!cfg.pvpve && w.is_empty());
        // An explicit pvpve (even false) wins; the alias is ignored with a warning.
        let (cfg, w) = parse_dev("pvpve = false\nauto_team_all = 29\n").expect("ok");
        assert!(!cfg.pvpve);
        assert_eq!(w.len(), 1, "{w:?}");
        let (cfg, _) =
            parse_dev("pvpve = true\npvpve_team = 13\nauto_team_all = 29\n").expect("ok");
        assert_eq!((cfg.pvpve, cfg.pvpve_team), (true, 13));
        // A value that is not a usable team is ignored.
        let (cfg, w) = parse_dev("auto_team_all = 300\n").expect("ok");
        assert!(!cfg.pvpve);
        assert_eq!(w.len(), 1, "{w:?}");
        let (cfg, _) = parse_dev("auto_team_all = 2\n").expect("ok");
        assert!(!cfg.pvpve);
    }

    #[test]
    fn empty_file_gives_defaults() {
        let (cfg, w) = parse("").expect("empty parses");
        assert_eq!(cfg, player_default());
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
        assert_eq!(cfg, player_default());
    }

    #[test]
    fn dev_mode_defaults_to_false_and_parses() {
        assert!(!Config::default().dev_mode);
        let (cfg, _) = parse("").expect("ok");
        assert!(!cfg.dev_mode);
        let (cfg, _) = parse(
            "dev_mode = true
",
        )
        .expect("ok");
        assert!(cfg.dev_mode);
        assert!(parse("dev_mode = \"yes\"").is_err());
    }

    #[test]
    fn player_mode_forces_pvpve_on_with_rivals_and_logs_the_overrides() {
        let (cfg, w) = parse(
            "pvpve = false
pvpve_scheme = \"all\"
pvpve_team = 13
team_size = 2
probes = true
",
        )
        .expect("ok");
        assert!(cfg.pvpve);
        assert_eq!(cfg.scheme(), Scheme::Rivals);
        assert_eq!((cfg.pvpve_team, cfg.team_size_usize()), (13, 2));
        assert!(!cfg.probes);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].contains("pvpve = false") && w[1].contains("pvpve_scheme"));
        // Silent when the file does not ask for either.
        let (cfg, w) = parse(
            "pvpve_team = 29
",
        )
        .expect("ok");
        assert!(
            cfg.pvpve && cfg.scheme() == Scheme::Rivals && w.is_empty(),
            "{w:?}"
        );
        // The fallback used for a missing or broken file obeys the same rules.
        let f = fallback();
        assert!(f.pvpve && !f.dev_mode && f.pvpve_team == DEFAULT_PVPVE_TEAM);
    }

    #[test]
    fn player_mode_has_no_key_bindings() {
        let (cfg, _) = parse(
            "hotkey_write = \"F5\"
overlay_toggle_key = \"Insert\"
",
        )
        .expect("ok");
        assert!(cfg.bindings().is_empty());
        for a in hotkey::ALL_ACTIONS {
            assert!(cfg.binding(a).is_none(), "{a:?}");
        }
        assert_eq!(cfg.describe_bindings(), "none (player mode)");
    }

    #[test]
    fn dev_mode_keeps_the_config_as_written() {
        let (cfg, w) = parse(
            "dev_mode = true
pvpve = false
pvpve_scheme = \"all\"
probes = true
",
        )
        .expect("ok");
        assert!(!cfg.pvpve && cfg.scheme() == Scheme::All && cfg.probes);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(cfg.bindings().len(), 9);
    }

    #[test]
    fn load_creates_the_default_file() {
        let dir = std::env::temp_dir().join(format!("nrpvp-cfg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let cfg = load(&dir);
        assert_eq!(cfg, player_default());
        let written = std::fs::read_to_string(dir.join(FILE_NAME)).expect("file created");
        assert_eq!(written, DEFAULT_FILE_TEXT);
        // Second load reads it back.
        assert_eq!(load(&dir), player_default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
