//! Scenario files (`scenarios/*.toml`): what to set up, what to do, what must be seen. The runner is in `nro::run`; this is the
//! model and the matching rules, so a scenario's meaning is testable without the game.
//!
//! ```toml
//! name = "puppets-recognized"
//! about = "three AFK puppet players appear in the player list"
//! [config]            # keys written over the staged nrpvp.toml (top level only)
//! mode = "offline"
//! [[step]]
//! wait = { console = "status", contains = "world: loaded", timeout_s = 240 }
//! [[step]]
//! console = "puppet_add 3"
//! [[step]]
//! expect = { console = "players", contains = ["p2", "p3", "p4"] }
//! ```

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub about: String,
    /// Top-level keys to set in the staged `nrpvp.toml`.
    #[serde(default)]
    pub config: BTreeMap<String, toml::Value>,
    /// Several emulated-Steam game instances (MULTI-INSTANCE.md); none = one normal real-Steam game.
    #[serde(default, rename = "instance")]
    pub instances: Vec<Instance>,
    #[serde(default, rename = "step")]
    pub steps: Vec<StepAt>,
    /// How to end: "kill" (default; the game's quit crashes on every run, mod STATUS §1) or "keep" (leave the game running).
    #[serde(default)]
    pub teardown: Option<String>,
}

/// One `[[instance]]`: emulated instance `n` (1..=3), launched in file order.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub n: u32,
    /// "host" or "client" (documentation and the report; the join itself is a step, e.g. `navigate = "invite_members"`).
    #[serde(default)]
    pub role: String,
    /// Keys for this instance's `nrpvp.toml`, over the scenario's `[config]`.
    #[serde(default)]
    pub config: BTreeMap<String, toml::Value>,
}

/// A step and the instance it acts on (`instance = N`; default the first instance, or the only game).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct StepAt {
    #[serde(default)]
    pub instance: Option<u32>,
    #[serde(flatten)]
    pub step: Step,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// One console line; its reply is recorded.
    Console(String),
    /// Repeat a console line until its reply matches, or fail after the timeout.
    Wait(Wait),
    /// One console line whose reply must match (a failed expectation makes the run FAIL, not abort).
    Expect(Expect),
    /// Keys for the game window, e.g. "ENTER", "E*3", "DOWN 300ms", separated by ';'.
    Keys(String),
    /// A named macro the runner knows (`title_to_world`).
    Navigate(String),
    SleepMs(u64),
    /// A screenshot of the game window, saved as `<run>\shots\<name>.png`.
    Screenshot(String),
    /// Make this many AFK puppet players (1..=5; mod ORCH-2).
    Puppets(u32),
    /// Put the puppets on a circle of this radius (metres) around the player.
    PuppetRing(f32),
    /// Put the puppets this many metres in front of the player (facing from a short step forward).
    PuppetFront(f32),
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Wait {
    pub console: String,
    #[serde(default)]
    pub contains: Contains,
    /// None of these may be in the reply that ends the wait (it keeps waiting while one is).
    #[serde(default)]
    pub absent: Contains,
    #[serde(default = "default_timeout")]
    pub timeout_s: u64,
    #[serde(default = "default_every")]
    pub every_ms: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Expect {
    pub console: String,
    #[serde(default)]
    pub contains: Contains,
    #[serde(default)]
    pub absent: Contains,
}

fn default_timeout() -> u64 {
    60
}
fn default_every() -> u64 {
    1000
}

/// One needle or several (all must match).
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(untagged)]
pub enum Contains {
    #[default]
    None,
    One(String),
    All(Vec<String>),
}

impl Contains {
    pub fn needles(&self) -> Vec<&str> {
        match self {
            Contains::None => Vec::new(),
            Contains::One(s) => vec![s.as_str()],
            Contains::All(v) => v.iter().map(|s| s.as_str()).collect(),
        }
    }
    /// Every needle is in `text` (case-insensitive). No needles = true.
    pub fn all_in(&self, text: &str) -> bool {
        let t = text.to_ascii_lowercase();
        self.needles()
            .iter()
            .all(|n| t.contains(&n.to_ascii_lowercase()))
    }
    /// No needle is in `text`.
    pub fn none_in(&self, text: &str) -> bool {
        let t = text.to_ascii_lowercase();
        !self
            .needles()
            .iter()
            .any(|n| t.contains(&n.to_ascii_lowercase()))
    }
}

impl Scenario {
    pub fn parse(text: &str) -> Result<Self, String> {
        let s: Self = toml::from_str(text).map_err(|e| format!("scenario: {e}"))?;
        s.check()?;
        Ok(s)
    }

    /// Instance numbers are 1..=3 and unique; roles are host/client/empty; a step's `instance` names a declared instance (and is
    /// only allowed when there are instances).
    fn check(&self) -> Result<(), String> {
        let mut seen = Vec::new();
        for i in &self.instances {
            if !(1..=3).contains(&i.n) {
                return Err(format!("scenario: instance n = {} (1..=3)", i.n));
            }
            if seen.contains(&i.n) {
                return Err(format!("scenario: instance {} declared twice", i.n));
            }
            if !["", "host", "client"].contains(&i.role.as_str()) {
                return Err(format!(
                    "scenario: instance {} role {:?} (host or client)",
                    i.n, i.role
                ));
            }
            seen.push(i.n);
        }
        for (k, s) in self.steps.iter().enumerate() {
            if let Some(n) = s.instance {
                if !seen.contains(&n) {
                    return Err(format!(
                        "scenario: step {} names instance {n}, which is not declared",
                        k + 1
                    ));
                }
            }
        }
        Ok(())
    }

    /// The instance a step acts on: its own `instance`, else the first declared one; `None` = the single normal game.
    pub fn instance_of(&self, s: &StepAt) -> Option<u32> {
        s.instance.or(self.instances.first().map(|i| i.n))
    }

    /// The config for instance `n`: `[config]` with the instance's own keys over it.
    pub fn config_for(&self, n: u32) -> BTreeMap<String, toml::Value> {
        let mut c = self.config.clone();
        if let Some(i) = self.instances.iter().find(|i| i.n == n) {
            c.extend(i.config.clone());
        }
        c
    }
}

/// Apply scenario config over a `nrpvp.toml` text. Top-level keys must stay before the first `[table]` (the mod's HAWK-2 rule in
/// `deploy-nucleus.ps1`), so replaced keys are edited in place and new ones go just before the first table.
pub fn apply_config(base: &str, cfg: &BTreeMap<String, toml::Value>) -> String {
    let mut lines: Vec<String> = base.lines().map(String::from).collect();
    let first_table = lines
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .unwrap_or(lines.len());
    let mut add = Vec::new();
    for (k, v) in cfg {
        let new = format!("{k} = {v}");
        let hit = lines[..first_table].iter().position(|l| {
            let t = l.trim_start();
            t.strip_prefix(k.as_str())
                .is_some_and(|r| r.trim_start().starts_with('='))
        });
        match hit {
            Some(i) => lines[i] = new,
            None => add.push(new),
        }
    }
    let tail = lines.split_off(first_table);
    lines.extend(add);
    lines.extend(tail);
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_scenario() {
        let s = Scenario::parse(
            r#"
name = "x"
[config]
mode = "offline"
br_menu = false
[[step]]
wait = { console = "status", contains = "world: loaded", timeout_s = 5 }
[[step]]
console = "puppet_add 3"
[[step]]
expect = { console = "players", contains = ["p2", "p3"], absent = "error" }
[[step]]
keys = "ENTER; E*3"
[[step]]
sleep_ms = 200
[[step]]
puppets = 4
[[step]]
puppet_ring = 1.5
"#,
        )
        .unwrap();
        assert_eq!(s.steps.len(), 7);
        assert!(s.instances.is_empty());
        assert!(s.steps.iter().all(|x| x.instance.is_none()));
        assert_eq!(s.instance_of(&s.steps[0]), None);
        assert_eq!(s.steps[5].step, Step::Puppets(4));
        assert!(matches!(&s.steps[1].step, Step::Console(l) if l == "puppet_add 3"));
        if let Step::Expect(e) = &s.steps[2].step {
            assert!(e.contains.all_in("P2 x p3"));
            assert!(!e.contains.all_in("p2"));
            assert!(e.absent.none_in("fine"));
            assert!(!e.absent.none_in("an ERROR"));
        } else {
            panic!()
        }
    }

    const MULTI: &str = r#"
name = "mi"
[config]
team_size = 1
ui_flag_watch = false
[[instance]]
n = 1
role = "host"
[instance.config]
team_size = 2
[[instance]]
n = 2
role = "client"
[[step]]
navigate = "title_to_world"
[[step]]
instance = 2
wait = { console = "status", contains = ["world loaded, 2 player(s)", "team_type 1"], timeout_s = 90 }
[[step]]
instance = 1
navigate = "invite_members"
"#;

    #[test]
    fn parses_instances_and_step_targets() {
        let s = Scenario::parse(MULTI).unwrap();
        assert_eq!(s.instances.len(), 2);
        assert_eq!(s.instances[0].role, "host");
        assert_eq!(s.instances[1].n, 2);
        assert_eq!(s.instance_of(&s.steps[0]), Some(1)); // default: the first instance
        assert_eq!(s.instance_of(&s.steps[1]), Some(2));
        assert_eq!(s.instance_of(&s.steps[2]), Some(1));
        assert_eq!(s.steps[2].step, Step::Navigate("invite_members".into()));
        if let Step::Wait(w) = &s.steps[1].step {
            assert_eq!(w.timeout_s, 90);
            assert!(w.contains.all_in("World loaded, 2 player(s), team_type 1"));
            assert!(
                w.absent.none_in("anything"),
                "no absent list = nothing forbidden"
            );
        } else {
            panic!()
        }
        assert_eq!(s.config_for(1)["team_size"].as_integer(), Some(2));
        assert_eq!(s.config_for(2)["team_size"].as_integer(), Some(1));
        assert_eq!(s.config_for(2)["ui_flag_watch"].as_bool(), Some(false));
    }

    #[test]
    fn refuses_bad_instances() {
        let bad = |from: &str, to: &str| Scenario::parse(&MULTI.replacen(from, to, 1)).unwrap_err();
        assert!(bad("n = 2", "n = 1").contains("twice"));
        assert!(bad("n = 2", "n = 4").contains("1..=3"));
        assert!(bad("role = \"client\"", "role = \"boss\"").contains("role"));
        assert!(bad("instance = 2", "instance = 3").contains("not declared"));
        // `instance = N` without any [[instance]] table.
        let e =
            Scenario::parse("name = \"x\"\n[[step]]\ninstance = 1\nsleep_ms = 1\n").unwrap_err();
        assert!(e.contains("not declared"), "{e}");
        // A typo in an instance table is an error, not silently ignored.
        assert!(bad("role = \"host\"", "rol = \"host\"").contains("rol"));
    }

    #[test]
    fn every_scenario_file_parses() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios");
        let mut n = 0;
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            if e.path().extension().is_some_and(|x| x == "toml") {
                let t = std::fs::read_to_string(e.path()).unwrap();
                Scenario::parse(&t).unwrap_or_else(|er| panic!("{}: {er}", e.path().display()));
                n += 1;
            }
        }
        assert!(n >= 9, "only {n} scenario files");
    }

    #[test]
    fn config_goes_before_the_first_table() {
        let base = "dev_mode = true\nmode = \"br\"\n\n[hawk]\nenabled = true\n";
        let mut c = BTreeMap::new();
        c.insert("mode".to_string(), toml::Value::String("offline".into()));
        c.insert("br_menu".to_string(), toml::Value::Boolean(false));
        let out = apply_config(base, &c);
        let t: toml::Table = toml::from_str(&out).unwrap();
        assert_eq!(t["mode"].as_str(), Some("offline"));
        assert_eq!(t["br_menu"].as_bool(), Some(false));
        assert_eq!(t["hawk"]["enabled"].as_bool(), Some(true));
        assert!(t["hawk"].get("br_menu").is_none());
    }
}
