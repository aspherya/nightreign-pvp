//! Preflight policy (PLAN §4.3): the launch happens only when every check passes. A check that could not be made counts as failed.
//! The Windows layer fills [`Facts`]; [`evaluate`] decides. Nothing here can be overridden by a caller: there is no "force".

use crate::pins::Pins;
use crate::profile;
use serde::Serialize;

#[derive(Debug, Clone, Default)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    /// Full image path when it could be read.
    pub path: Option<String>,
}

/// Everything the policy looks at, gathered by the caller just before launch.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// Pinned game file (pins key) → its SHA-256 now, `None` = could not be read.
    pub game_hashes: Vec<(String, Option<String>)>,
    /// Pinned me3 file → its SHA-256 now.
    pub me3_hashes: Vec<(String, Option<String>)>,
    /// `me3 --version` output, `None` = could not run it.
    pub me3_version: Option<String>,
    pub processes: Vec<Process>,
    /// The profile text the launch will pass to me3.
    pub profile: String,
    /// The extra arguments the launch will pass to me3 after `launch -g nightreign -e <exe> -p <profile>`.
    pub me3_args: Vec<String>,
    /// The staged `nrpvp.toml` text (`None` = missing).
    pub mod_toml: Option<String>,
    /// Absolute paths of the files the profile references, with whether they exist.
    pub profile_files: Vec<(String, bool)>,
    /// The real vanilla saves (`NR0000.sl2*`) were hashed for the before/after check.
    pub vanilla_saves_hashed: bool,
    /// Set for an emulated-Steam instance (docs: orchestrator/MULTI-INSTANCE.md §3.4); `None` = the normal real-Steam launch.
    pub emulated: Option<Emulated>,
}

/// What the policy needs to know about an instance that runs on the Steam emulator (gbe_fork) instead of the real Steam.
#[derive(Debug, Clone, Default)]
pub struct Emulated {
    /// The staged `steam_settings\configs.main.ini` text (`None` = missing).
    pub main_ini: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Check {
    pub id: &'static str,
    pub ok: bool,
    /// What was seen.
    pub detail: String,
    /// What to do about a failure (empty when ok).
    pub fix: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub ok: bool,
    pub checks: Vec<Check>,
    /// Observations that do not block (for example me3 processes left over from another tool).
    pub warnings: Vec<String>,
}

fn check(id: &'static str, ok: bool, detail: impl Into<String>, fix: impl Into<String>) -> Check {
    Check {
        id,
        ok,
        detail: detail.into(),
        fix: if ok { String::new() } else { fix.into() },
    }
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

pub fn evaluate(pins: &Pins, f: &Facts) -> Report {
    let mut checks = Vec::new();
    let mut warnings = Vec::new();

    // P1 is structural: the only launch code builds its own command line (nro::launch); recorded for the report.
    checks.push(check(
        "P1 launch route",
        true,
        "me3 launch -g nightreign -e <game>\\nightreign.exe -p <run profile>, built by nro",
        "",
    ));

    // P2 profile.
    let prof = profile::audit(&f.profile);
    let missing: Vec<&str> = f
        .profile_files
        .iter()
        .filter(|(_, e)| !e)
        .map(|(p, _)| p.as_str())
        .collect();
    checks.push(check(
        "P2 profile",
        prof.is_empty() && missing.is_empty(),
        if prof.is_empty() && missing.is_empty() {
            "no start_online, no savefile, every referenced file present".to_string()
        } else {
            format!("{} {}", prof.join("; "), missing.iter().map(|m| format!("missing {m}")).collect::<Vec<_>>().join("; "))
        },
        "the run profile must only load SC early, the staged nrpvp.dll and the staged game data; rebuild the run folder",
    ));

    // P3 me3 arguments.
    // An emulated instance needs `--skip-steam-init true` (no Steam client to initialise) and `--no-boot-boost true` (several games must not share
    // me3's decrypted-archive cache); nothing else changes.
    let emu = f.emulated.is_some();
    let mut bad: Vec<&String> = Vec::new();
    let mut i = 0;
    while i < f.me3_args.len() {
        let a = lower(&f.me3_args[i]);
        if emu
            && (a == "--skip-steam-init" || a == "--no-boot-boost")
            && f.me3_args.get(i + 1).map(|v| lower(v)) == Some("true".into())
        {
            i += 2;
            continue;
        }
        if a.starts_with("--online") || a.starts_with("--skip-steam-init") || a == "--suspend" {
            bad.push(&f.me3_args[i]);
        }
        i += 1;
    }
    checks.push(check(
        "P3 me3 arguments",
        bad.is_empty(),
        if bad.is_empty() {
            format!("{:?}", f.me3_args)
        } else {
            format!("refused: {bad:?}")
        },
        "never pass --online (re-enables matchmaking) or --skip-steam-init (Nucleus only)",
    ));

    // P4 pins.
    let mut wrong = Vec::new();
    for (k, want) in &pins.sha256 {
        match f.game_hashes.iter().find(|(n, _)| n == k) {
            Some((_, Some(h))) if h.eq_ignore_ascii_case(want) => {}
            Some((_, Some(h))) => wrong.push(format!(
                "{k} is {}…, pinned {}…",
                &h[..12.min(h.len())],
                &want[..12]
            )),
            _ => wrong.push(format!("{k} could not be hashed")),
        }
    }
    for (k, want) in &pins.me3.files {
        match f.me3_hashes.iter().find(|(n, _)| n == k) {
            Some((_, Some(h))) if h.eq_ignore_ascii_case(want) => {}
            Some((_, Some(_))) => wrong.push(format!("me3 {k} differs from the pin")),
            _ => wrong.push(format!("me3 {k} could not be hashed")),
        }
    }
    match &f.me3_version {
        Some(v) if v.contains(&pins.me3.version) => {}
        Some(v) => wrong.push(format!("me3 says {v:?}, pinned {}", pins.me3.version)),
        None => wrong.push("me3 --version did not run".into()),
    }
    checks.push(check(
        "P4 pins",
        wrong.is_empty(),
        if wrong.is_empty() { "game, Seamless Co-op, regulation and me3 match the pins".into() } else { wrong.join("; ") },
        "a game, SC or me3 update needs re-pinning (pins/nightreign.toml and the mod's TECHNICAL_NOTES §1) before tests run",
    ));

    // P5 processes.
    let mut blocking = Vec::new();
    for p in &f.processes {
        let n = lower(&p.name);
        // Another emulated instance of ours (its exe lives under nro-instances) does not block the next one.
        let ours = f.emulated.is_some()
            && n == "nightreign.exe"
            && p.path
                .as_deref()
                .is_some_and(|x| lower(x).contains(r"\nro-instances\"));
        if ours {
            continue;
        }
        if n == "nightreign.exe" || n == "start_protected_game.exe" || n.contains("easyanticheat") {
            blocking.push(format!("{} (pid {})", p.name, p.pid));
        } else if n == "me3.exe" || n == "me3-launcher.exe" {
            warnings.push(format!(
                "{} (pid {}) is running from {} (not started by nro)",
                p.name,
                p.pid,
                p.path.as_deref().unwrap_or("?")
            ));
        }
    }
    checks.push(check(
        "P5 processes",
        blocking.is_empty(),
        if blocking.is_empty() { "no game, no anti-cheat launcher, no EasyAntiCheat".into() } else { blocking.join(", ") },
        "close the game first; if EasyAntiCheat or start_protected_game.exe is running, the normal game was started: quit it",
    ));

    // P6 Steam. Real launch: Steam must run. Emulated launch: flipped, a real Steam next to a failed emulator load would put the
    // real account in the game.
    let steam = f.processes.iter().any(|p| lower(&p.name) == "steam.exe");
    if emu {
        checks.push(check(
            "P6 Steam",
            !steam,
            if steam {
                "steam.exe is running"
            } else {
                "steam.exe is not running (emulated instance)"
            },
            "close Steam completely: an emulated instance must never run next to the real client",
        ));
        let lan = f
            .emulated
            .as_ref()
            .and_then(|e| e.main_ini.as_deref())
            .map(|t| {
                t.lines()
                    .filter_map(|l| l.split_once('='))
                    .any(|(k, v)| k.trim() == "disable_lan_only" && v.trim() == "0")
            })
            .unwrap_or(false);
        checks.push(check(
            "P6b emulator LAN-only",
            lan,
            if lan { "disable_lan_only = 0" } else { "disable_lan_only is not 0 (or the ini is missing)" },
            "stage steam_settings/configs.main.ini with disable_lan_only=0 (the emulator then refuses non-LAN peers)",
        ));
    } else {
        checks.push(check(
            "P6 Steam",
            steam,
            if steam {
                "steam.exe is running"
            } else {
                "steam.exe is not running"
            },
            "start Steam and sign in (the game needs it; nro never starts the game through Steam)",
        ));
    }

    // P7 mod config.
    match &f.mod_toml {
        None => checks.push(check(
            "P7 nrpvp.toml",
            false,
            "missing",
            "the run folder needs a staged nrpvp.toml",
        )),
        Some(t) => {
            let v: Result<toml::Table, _> = toml::from_str(t);
            let (ok, detail) = match v {
                Err(e) => (false, format!("does not parse: {e}")),
                Ok(t) => {
                    let dev = t.get("dev_mode").and_then(|v| v.as_bool()) == Some(true);
                    // `matchmaking` is the player-build switch; the developer build ignores it (mod config.rs).
                    let queue = t.get("mm_queue").and_then(|v| v.as_bool()) == Some(true);
                    let br = t.get("br_menu").and_then(|v| v.as_bool()) == Some(true);
                    // The joiner's `steam://joinlobby` link (mm_join_auto "link" / "invite+link") goes to the shell, which starts the
                    // REAL Steam client when it is closed: never in an emulated run.
                    let link = t
                        .get("mm_join_auto")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| lower(s).contains("link"));
                    if emu {
                        // Emulated (MULTI-INSTANCE §20): the queue's lobby calls go to gbe_fork, LAN-only (P6b), Steam closed (P6),
                        // the emulator module proven in the game (P-module) and the emulated id checked in the mod log before
                        // queuing (nro, `queue_identity`): they cannot reach Steam's servers.
                        (
                            dev && !link,
                            format!("dev_mode = {dev}, queue = {queue}, br_menu = {br} (emulated: allowed), joiner link = {link}"),
                        )
                    } else {
                        (
                            dev && !queue && !br,
                            format!("dev_mode = {dev}, public queue = {queue}, br_menu = {br}"),
                        )
                    }
                }
            };
            checks.push(check(
                "P7 nrpvp.toml",
                ok,
                detail,
                if emu {
                    "an emulated run needs dev_mode = true and mm_join_auto without \"link\" (the shell would start the real Steam)"
                } else {
                    "the run config needs dev_mode = true (console) and the public Steam queue off (mm_queue, br_menu)"
                },
            ));
        }
    }

    // P8 saves.
    checks.push(check(
        "P8 saves",
        f.vanilla_saves_hashed,
        if f.vanilla_saves_hashed {
            "vanilla NR0000.sl2* hashed for the after-run check"
        } else {
            "could not hash the vanilla saves"
        },
        "check %APPDATA%\\Nightreign is readable",
    ));

    Report {
        ok: checks.iter().all(|c| c.ok),
        checks,
        warnings,
    }
}

/// The mod's own Steam id as its queue worker (or lobby probe) logged it at Steam init: the LAST
/// `mm_queue: worker started: own Steam id <N>` / `mm_probe: own Steam ID = <N>` line.
pub fn own_steam_id(log: &str) -> Option<u64> {
    log.lines()
        .rev()
        .find_map(|l| {
            let rest = l
                .split_once("mm_queue: worker started: own Steam id ")
                .or_else(|| l.split_once("mm_probe: own Steam ID = "))?
                .1;
            Some(
                rest.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>(),
            )
        })
        .and_then(|d| d.parse().ok())
}

/// Runtime check before an emulated instance queues (MULTI-INSTANCE §20): the game must run as the emulated account `want`. Any
/// other id (the real account through the real Steam client) or no id at all refuses.
pub fn queue_identity(log: &str, want: u64) -> Result<u64, String> {
    match own_steam_id(log) {
        Some(id) if id == want && want != 0 => Ok(id),
        Some(id) => Err(format!(
            "the mod logged own Steam id {id}, not the emulated {want}: the game is not on the emulator"
        )),
        None => Err("the mod has not logged its own Steam id yet (mm_queue worker / mm_probe)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pins::{Pins, EMBEDDED};

    fn good() -> (Pins, Facts) {
        let pins = Pins::parse(EMBEDDED).unwrap();
        let f = Facts {
            game_hashes: pins
                .sha256
                .iter()
                .map(|(k, v)| (k.clone(), Some(v.clone())))
                .collect(),
            me3_hashes: pins
                .me3
                .files
                .iter()
                .map(|(k, v)| (k.clone(), Some(v.clone())))
                .collect(),
            me3_version: Some(format!("me3 {}", pins.me3.version)),
            processes: vec![Process {
                pid: 1,
                name: "steam.exe".into(),
                path: None,
            }],
            profile: profile::text(
                r"C:\G\SeamlessCoop\nrsc.dll",
                r"C:\run\nrpvp.dll",
                Some(r"C:\run\gamedata"),
            ),
            me3_args: vec![],
            mod_toml: Some("dev_mode = true\nmm_queue = false\nbr_menu = false\n".into()),
            profile_files: vec![("a".into(), true)],
            vanilla_saves_hashed: true,
            emulated: None,
        };
        (pins, f)
    }

    fn failed(r: &Report) -> Vec<&'static str> {
        r.checks.iter().filter(|c| !c.ok).map(|c| c.id).collect()
    }

    #[test]
    fn all_good_passes() {
        let (p, f) = good();
        let r = evaluate(&p, &f);
        assert!(r.ok, "{:?}", r.checks);
    }

    #[test]
    fn online_argument_refuses() {
        let (p, mut f) = good();
        f.me3_args = vec!["--online".into(), "true".into()];
        assert_eq!(failed(&evaluate(&p, &f)), ["P3 me3 arguments"]);
    }

    #[test]
    fn start_online_in_profile_refuses() {
        let (p, mut f) = good();
        f.profile.push_str("start_online = true\n");
        assert_eq!(failed(&evaluate(&p, &f)), ["P2 profile"]);
    }

    #[test]
    fn wrong_exe_hash_refuses() {
        let (p, mut f) = good();
        f.game_hashes[0].1 = Some("00".repeat(32));
        assert_eq!(failed(&evaluate(&p, &f)), ["P4 pins"]);
        f.game_hashes[0].1 = None;
        assert_eq!(failed(&evaluate(&p, &f)), ["P4 pins"]);
    }

    #[test]
    fn me3_version_drift_refuses() {
        let (p, mut f) = good();
        f.me3_version = Some("me3 0.13.0".into());
        assert_eq!(failed(&evaluate(&p, &f)), ["P4 pins"]);
    }

    #[test]
    fn eac_or_running_game_refuses_and_leftover_me3_warns() {
        let (p, mut f) = good();
        f.processes.push(Process {
            pid: 7,
            name: "EasyAntiCheat_EOS.exe".into(),
            path: None,
        });
        f.processes.push(Process {
            pid: 8,
            name: "me3.exe".into(),
            path: Some(r"D:\x\me3.exe".into()),
        });
        let r = evaluate(&p, &f);
        assert_eq!(failed(&r), ["P5 processes"]);
        assert_eq!(r.warnings.len(), 1);
        f.processes.retain(|p| p.pid != 7);
        f.processes.push(Process {
            pid: 9,
            name: "nightreign.exe".into(),
            path: None,
        });
        assert_eq!(failed(&evaluate(&p, &f)), ["P5 processes"]);
    }

    #[test]
    fn no_steam_refuses() {
        let (p, mut f) = good();
        f.processes.clear();
        assert_eq!(failed(&evaluate(&p, &f)), ["P6 Steam"]);
    }

    #[test]
    fn queue_on_or_no_console_refuses() {
        let (p, mut f) = good();
        f.mod_toml = Some("dev_mode = true\nmm_queue = true\n".into());
        assert_eq!(failed(&evaluate(&p, &f)), ["P7 nrpvp.toml"]);
        f.mod_toml = Some("dev_mode = false\n".into());
        assert_eq!(failed(&evaluate(&p, &f)), ["P7 nrpvp.toml"]);
        f.mod_toml = None;
        assert_eq!(failed(&evaluate(&p, &f)), ["P7 nrpvp.toml"]);
    }

    fn emulated() -> (Pins, Facts) {
        let (p, mut f) = good();
        f.processes.clear();
        f.me3_args = vec!["--skip-steam-init".into(), "true".into()];
        f.emulated = Some(Emulated {
            main_ini: Some(
                "[main::connectivity]
disable_lan_only=0
"
                .into(),
            ),
        });
        (p, f)
    }

    #[test]
    fn emulated_instance_passes_without_steam() {
        let (p, f) = emulated();
        let r = evaluate(&p, &f);
        assert!(r.ok, "{:?}", r.checks);
    }

    #[test]
    fn emulated_refuses_steam_running_or_lan_unlocked_or_other_args() {
        let (p, mut f) = emulated();
        f.processes.push(Process {
            pid: 1,
            name: "steam.exe".into(),
            path: None,
        });
        assert_eq!(failed(&evaluate(&p, &f)), ["P6 Steam"]);
        f.processes.clear();
        f.emulated.as_mut().unwrap().main_ini = Some(
            "disable_lan_only=1
"
            .into(),
        );
        assert_eq!(failed(&evaluate(&p, &f)), ["P6b emulator LAN-only"]);
        f.emulated.as_mut().unwrap().main_ini = None;
        assert_eq!(failed(&evaluate(&p, &f)), ["P6b emulator LAN-only"]);
        let (p, mut f) = emulated();
        f.me3_args.push("--online".into());
        assert_eq!(failed(&evaluate(&p, &f)), ["P3 me3 arguments"]);
        f.me3_args = vec!["--skip-steam-init".into(), "false".into()];
        assert_eq!(failed(&evaluate(&p, &f)), ["P3 me3 arguments"]);
    }

    #[test]
    fn emulated_allows_a_running_instance_of_ours_but_not_the_real_game() {
        let (p, mut f) = emulated();
        f.processes.push(Process {
            pid: 5,
            name: "nightreign.exe".into(),
            path: Some(r"Z:\nro-instances\r\i1\game\nightreign.exe".into()),
        });
        assert!(evaluate(&p, &f).ok);
        f.processes.push(Process {
            pid: 6,
            name: "nightreign.exe".into(),
            path: Some(r"Z:\SteamLibrary\steamapps\common\X\Game\nightreign.exe".into()),
        });
        assert_eq!(failed(&evaluate(&p, &f)), ["P5 processes"]);
    }

    #[test]
    fn emulated_allows_the_queue_but_never_the_joiner_link() {
        let (p, mut f) = emulated();
        f.mod_toml = Some(
            "dev_mode = true\nmm_queue = true\nbr_menu = true\nmm_join = true\nmm_join_auto = \"invite\"\n".into(),
        );
        assert!(evaluate(&p, &f).ok);
        for a in ["invite+link", "link", "Invite+Link"] {
            f.mod_toml = Some(format!(
                "dev_mode = true\nmm_queue = true\nmm_join_auto = \"{a}\"\n"
            ));
            assert_eq!(failed(&evaluate(&p, &f)), ["P7 nrpvp.toml"], "{a}");
        }
        f.mod_toml = Some("dev_mode = false\nmm_queue = true\n".into());
        assert_eq!(failed(&evaluate(&p, &f)), ["P7 nrpvp.toml"]);
        // The same queue config on a real-Steam launch is refused.
        let (p, mut f) = good();
        f.mod_toml = Some("dev_mode = true\nmm_queue = true\nmm_join_auto = \"invite\"\n".into());
        assert_eq!(failed(&evaluate(&p, &f)), ["P7 nrpvp.toml"]);
        f.mod_toml = Some("dev_mode = true\nbr_menu = true\n".into());
        assert_eq!(failed(&evaluate(&p, &f)), ["P7 nrpvp.toml"]);
    }

    #[test]
    fn queue_identity_needs_the_emulated_id() {
        let log = "x\n[INFO] mm_queue: worker started: own Steam id 76561199023125438 (0x1100001...), lobby size (mm_max_players)=2\n";
        assert_eq!(own_steam_id(log), Some(76561199023125438));
        assert!(queue_identity(log, 76561199023125438).is_ok());
        assert!(queue_identity(log, 76561199023125439).is_err());
        assert!(queue_identity("nothing yet\n", 76561199023125438).is_err());
        let probe = "mm_probe: own Steam ID = 76561199023125439 (0x1)\n";
        assert_eq!(own_steam_id(probe), Some(76561199023125439));
        // The last line wins (a relaunch appends to nrpvp.log).
        let two = format!("{log}{probe}");
        assert!(queue_identity(&two, 76561199023125438).is_err());
        assert!(queue_identity("mm_queue: worker started: own Steam id 0 (0x0)\n", 0).is_err());
    }

    #[test]
    fn real_launch_still_refuses_skip_steam_init() {
        let (p, mut f) = good();
        f.me3_args = vec!["--skip-steam-init".into(), "true".into()];
        assert_eq!(failed(&evaluate(&p, &f)), ["P3 me3 arguments"]);
    }

    #[test]
    fn unhashed_saves_refuse() {
        let (p, mut f) = good();
        f.vanilla_saves_hashed = false;
        assert_eq!(failed(&evaluate(&p, &f)), ["P8 saves"]);
    }
}
