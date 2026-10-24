//! The pins file (`pins/nightreign.toml`): where things are and which hashes a launch requires.

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Pins {
    pub game_dir: String,
    pub me3_dir: String,
    /// Game-folder-relative path (forward slashes) → SHA-256, upper-case hex.
    pub sha256: BTreeMap<String, String>,
    pub me3: Me3Pins,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Me3Pins {
    pub version: String,
    /// me3_dir-relative file name → SHA-256 (the `version` key is not a file).
    #[serde(flatten)]
    pub files: BTreeMap<String, String>,
}

impl Pins {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| format!("pins: {e}"))
    }
}

/// The pins built into the binary (the repo's `pins/nightreign.toml`).
pub const EMBEDDED: &str = include_str!("../../../pins/nightreign.toml");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_pins_parse() {
        let p = Pins::parse(EMBEDDED).unwrap();
        assert!(p.sha256.contains_key("nightreign.exe"));
        assert_eq!(p.me3.version, "0.11.0");
        assert!(p.me3.files.contains_key("me3.exe"));
        assert!(!p.me3.files.contains_key("version"));
    }
}
