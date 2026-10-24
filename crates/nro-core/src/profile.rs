//! The me3 profile nro writes for a run. Same shape as `nrpvp_launcher.exe`'s (nightreign-pvp `crates/nrpvp-launcher/src/main.rs`
//! `profile_text`, verified in the mod's T-004): Seamless Co-op `load_early` with no initializer, `nrpvp.dll` `load_after` it, and
//! the game-data package. No `start_online` (me3 then blocks the official matchmaking servers), no `savefile` (SC uses its `.co2`).

/// A TOML string for a Windows path: a literal string unless the path contains a single quote.
pub fn toml_path(p: &str) -> String {
    if p.contains('\'') {
        format!("\"{}\"", p.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        format!("'{p}'")
    }
}

pub fn text(nrsc: &str, nrpvp: &str, gamedata: Option<&str>) -> String {
    let mut s = format!(
        "# Written by nro for one run. Do not edit.\n\
         profileVersion = \"v1\"\n\n\
         [[supports]]\ngame = \"nightreign\"\n\n\
         [[natives]]\npath = {}\nload_early = true\n\n\
         [[natives]]\npath = {}\nload_after = [{{ id = \"nrsc.dll\", optional = false }}]\n",
        toml_path(nrsc),
        toml_path(nrpvp)
    );
    if let Some(g) = gamedata {
        s.push_str(&format!(
            "\n[[packages]]\nid = \"nrpvp-gamedata\"\npath = {}\n",
            toml_path(g)
        ));
    }
    s
}

/// Problems with a profile text (empty = acceptable). Used by preflight P2 on the exact text that is passed to me3.
pub fn audit(profile: &str) -> Vec<String> {
    let mut out = Vec::new();
    match toml::from_str::<toml::Table>(profile) {
        Err(e) => out.push(format!("profile does not parse: {e}")),
        Ok(t) => {
            // Anywhere, not only at the top level: a key after a table header belongs to that table, and me3's reading of a
            // misplaced key is not something to rely on.
            for key in ["start_online", "savefile", "disable_arxan"] {
                if profile.lines().any(|l| l.trim_start().starts_with(key)) {
                    out.push(format!("profile sets {key}"));
                }
            }
            let natives = t
                .get("natives")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if natives.len() != 2 {
                out.push(format!(
                    "profile has {} natives, expected 2 (nrsc.dll, nrpvp.dll)",
                    natives.len()
                ));
            }
            for n in &natives {
                let p = n.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let name = p
                    .rsplit(['\\', '/'])
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if name != "nrsc.dll" && name != "nrpvp.dll" {
                    out.push(format!("unexpected native {p}"));
                }
                if n.get("initializer").is_some() {
                    out.push(format!("native {p} has an initializer"));
                }
            }
        }
    }
    out
}

/// Every file or folder path a profile text references (for the existence check).
pub fn paths(profile: &str) -> Vec<String> {
    let Ok(t) = toml::from_str::<toml::Table>(profile) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for key in ["natives", "packages"] {
        for n in t
            .get(key)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
        {
            if let Some(p) = n.get("path").and_then(|v| v.as_str()) {
                out.push(p.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_shape_as_the_launcher() {
        let t = text(
            r"S:\G\SeamlessCoop\nrsc.dll",
            r"S:\run\nrpvp.dll",
            Some(r"S:\run\gamedata"),
        );
        assert!(t.contains(r"path = 'S:\G\SeamlessCoop\nrsc.dll'"));
        assert!(t.contains("load_early = true"));
        assert!(t.contains(r#"load_after = [{ id = "nrsc.dll", optional = false }]"#));
        assert!(t.find("nrsc.dll'").unwrap() < t.find("nrpvp.dll'").unwrap());
        assert!(t.contains("[[packages]]\nid = \"nrpvp-gamedata\"\npath = 'S:\\run\\gamedata'\n"));
        assert!(audit(&t).is_empty(), "{:?}", audit(&t));
        assert_eq!(paths(&t).len(), 3);
    }

    #[test]
    fn audit_catches_online_and_extra_natives() {
        let mut t = text("a\\nrsc.dll", "b\\nrpvp.dll", None);
        t = t.replace("profileVersion", "start_online = true\nprofileVersion");
        t.push_str("\n[[natives]]\npath = 'x\\evil.dll'\n");
        let a = audit(&t);
        assert!(a.iter().any(|s| s.contains("start_online")));
        assert!(a.iter().any(|s| s.contains("evil.dll")));
    }
}
