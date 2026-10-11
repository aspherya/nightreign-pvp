//! Parsing of the nrpvp.log lines nrscan cares about, plus a tail reader.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    Down,
    Up,
    HpRestored,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateEvt {
    pub local: bool,
    pub who: String,
    pub kind: StateKind,
}

/// `... T-033 STATE who=LOCAL[0] id=0x6002FFFFFFFF DOWN (downs=1, hp=0, ...)`
pub fn parse_state_line(line: &str) -> Option<StateEvt> {
    let marker = "T-033 STATE who=";
    let rest = &line[line.find(marker)? + marker.len()..];
    let who_end = rest.find(' ')?;
    let who = &rest[..who_end];
    let rest = rest[who_end + 1..].strip_prefix("id=")?;
    let after_id = &rest[rest.find(' ')? + 1..];
    let kind_str = match after_id.find(" (") {
        Some(i) => &after_id[..i],
        None => after_id,
    }
    .trim();
    let kind = match kind_str {
        "DOWN" => StateKind::Down,
        "UP" => StateKind::Up,
        "HP restored" => StateKind::HpRestored,
        _ => StateKind::Other,
    };
    Some(StateEvt {
        local: who.starts_with("LOCAL"),
        who: who.to_string(),
        kind,
    })
}

#[derive(Debug, Default, Clone)]
pub struct LogInfo {
    pub players: Vec<u64>,
    /// (name, instance address)
    pub singletons: Vec<(String, u64)>,
}

fn parse_hex_after(s: &str, pat: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = s[from..].find(pat) {
        let start = from + i + pat.len();
        let digits: String = s[start..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if let Ok(v) = u64::from_str_radix(&digits, 16) {
            if (0x1_0000..0x8000_0000_0000).contains(&v) {
                out.push(v);
            }
        }
        from = start;
    }
    out
}

impl LogInfo {
    pub fn add_line(&mut self, line: &str) {
        // Player addresses: "PlayerIns 0x7FF3...", "local player = 0x...", "local player 0x...", "local = 0x...".
        for pat in [
            "PlayerIns 0x",
            "local player = 0x",
            "local player 0x",
            "local = 0x",
        ] {
            for v in parse_hex_after(line, pat) {
                if !self.players.contains(&v) {
                    self.players.push(v);
                }
            }
        }
        // Singleton map: "  <Name>   FD4|derived   slot=0x... (exe+0x...) instance=0x...|null"
        let toks: Vec<&str> = line.split_whitespace().collect();
        if let Some(i) = toks.iter().position(|t| t.starts_with("slot=0x")) {
            if i >= 2 && (toks[i - 1].starts_with("FD4") || toks[i - 1].starts_with("derived")) {
                if let Some(inst) = toks.iter().find_map(|t| t.strip_prefix("instance=0x")) {
                    if let Ok(v) = u64::from_str_radix(inst, 16) {
                        if v >= 0x1_0000 {
                            let name = toks[i - 2].to_string();
                            if !self.singletons.iter().any(|(n, a)| *n == name && *a == v) {
                                self.singletons.push((name, v));
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn load(path: &Path) -> LogInfo {
        let mut info = LogInfo::default();
        if let Ok(mut f) = File::open(path) {
            let mut bytes = Vec::new();
            let _ = f.read_to_end(&mut bytes);
            for l in String::from_utf8_lossy(&bytes).lines() {
                info.add_line(l);
            }
        }
        info
    }

    pub fn anchors(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self
            .players
            .iter()
            .copied()
            .chain(self.singletons.iter().map(|s| s.1))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// Reads new complete lines appended to a file, starting from its current end.
pub struct Tail {
    path: PathBuf,
    pos: u64,
    partial: String,
}

impl Tail {
    pub fn from_end(path: &Path) -> Tail {
        let pos = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Tail {
            path: path.to_path_buf(),
            pos,
            partial: String::new(),
        }
    }

    pub fn poll(&mut self) -> Vec<String> {
        let Ok(mut f) = File::open(&self.path) else {
            return Vec::new();
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.pos {
            // truncated by a relaunch: start over
            self.pos = 0;
            self.partial.clear();
        }
        if len == self.pos || f.seek(SeekFrom::Start(self.pos)).is_err() {
            return Vec::new();
        }
        let mut bytes = Vec::new();
        if f.read_to_end(&mut bytes).is_err() {
            return Vec::new();
        }
        self.pos += bytes.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&bytes));
        let mut out = Vec::new();
        while let Some(i) = self.partial.find('\n') {
            out.push(self.partial[..i].trim_end_matches('\r').to_string());
            self.partial.drain(..=i);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real lines from .claude/experiments/FS-0/run9_finisher2_instance0.log
    const DOWN: &str = "2026-10-11T04:56:41.008409Z  INFO      nrpvp-main T-033 STATE who=LOCAL[0] id=0x6002FFFFFFFF DOWN (downs=1, hp=0, last_attacker=RIVAL id=0x6001FFFFFFFF handle=0xFFFFFFFF10C00001)";
    const UP: &str = "2026-10-11T04:56:57.322396Z  INFO      nrpvp-main T-033 STATE who=LOCAL[0] id=0x6002FFFFFFFF UP (downs=1, hp=0, last_attacker=RIVAL id=0x6001FFFFFFFF handle=0xFFFFFFFF10C00001)";
    const HPR: &str = "2026-10-11T04:57:03.120312Z  INFO      nrpvp-main T-033 STATE who=LOCAL[0] id=0x6002FFFFFFFF HP restored (downs=1, hp=240, last_attacker=RIVAL id=0x6001FFFFFFFF handle=0xFFFFFFFF10C00001)";
    const INIT_R: &str = "2026-10-11T04:55:59.849210Z  INFO      nrpvp-main T-033 STATE who=REMOTE[1] id=0x6001FFFFFFFF INIT (downed=false, downs=0, hp=220)";
    const EVID: &str = "2026-10-11T04:54:50.864099Z  INFO    nrpvp-main T-008: team:   evidence PlayerIns 0x7FF3C12EE980 entry[0] raw=80 E9 2E C1 F3 7F 00 00 04 04 00 00";
    const LOCALP: &str = "2026-10-11T04:54:47.064507Z  INFO    nrpvp-main T-008: new WorldChrMan instance 0x7FF410810740: local player 0x7FF3C12EE980 team_type = 1 at start";
    const LOCALEQ: &str = "2026-10-11T04:54:50.864110Z  INFO    nrpvp-main T-008: team: local player = 1 valid PlayerIns in the ChrSet; local = 0x7FF3C12EE980 (first PlayerIns in the ChrSet (entry[0] rule))";
    const SING: &str = "2026-10-11T04:54:44.624524Z  INFO    nrpvp-main   AnibndRepository                             FD4     slot=0x7FF73A562BA8 (exe+0x3C22BA8) instance=0x7FF3B13406E0";
    const SING_NULL: &str = "2026-10-11T04:54:44.624569Z  INFO    nrpvp-main   CSActionButtonMan                            FD4     slot=0x7FF73A5577D8 (exe+0x3C177D8) instance=null";

    #[test]
    fn state_lines() {
        let d = parse_state_line(DOWN).unwrap();
        assert!(d.local && d.kind == StateKind::Down);
        assert_eq!(parse_state_line(UP).unwrap().kind, StateKind::Up);
        assert_eq!(parse_state_line(HPR).unwrap().kind, StateKind::HpRestored);
        let r = parse_state_line(INIT_R).unwrap();
        assert!(!r.local && r.kind == StateKind::Other);
        assert!(parse_state_line(EVID).is_none());
    }

    #[test]
    fn addresses() {
        let mut i = LogInfo::default();
        for l in [EVID, LOCALP, LOCALEQ, SING, SING_NULL] {
            i.add_line(l);
        }
        assert_eq!(i.players, vec![0x7FF3C12EE980]);
        assert_eq!(
            i.singletons,
            vec![("AnibndRepository".to_string(), 0x7FF3B13406E0)]
        );
    }
}
