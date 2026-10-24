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
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
    /// How to end: "kill" (default; the game's quit crashes on every run, mod STATUS §1) or "keep" (leave the game running).
    #[serde(default)]
    pub teardown: Option<String>,
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
        toml::from_str(text).map_err(|e| format!("scenario: {e}"))
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
        assert_eq!(s.steps[5], Step::Puppets(4));
        assert!(matches!(&s.steps[1], Step::Console(l) if l == "puppet_add 3"));
        if let Step::Expect(e) = &s.steps[2] {
            assert!(e.contains.all_in("P2 x p3"));
            assert!(!e.contains.all_in("p2"));
            assert!(e.absent.none_in("fine"));
            assert!(!e.absent.none_in("an ERROR"));
        } else {
            panic!()
        }
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
