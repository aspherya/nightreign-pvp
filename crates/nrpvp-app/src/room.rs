//! RW-6: the private room code in the player's `nrpvp.toml` (`private_code`). Players who set the same code meet only each other;
//! empty = the public queue. The launcher reads and writes the one line and keeps it across updates (an update replaces the file).
//! The rules mirror the mod's `config::normalize_private_code`: trimmed, lower case, letters, digits, `-` and `_`, 32 at most.

/// Longest code (the mod's `PRIVATE_CODE_MAX`).
pub const MAX: usize = 32;
const KEY: &str = "private_code";
const COMMENT: &str =
    "# Private room (set in the launcher): players with the same code meet only each other. Empty = public matchmaking.";

/// The code as the mod compares it, or why it is refused.
pub fn normalize(raw: &str) -> Result<String, String> {
    let t = raw.trim().to_ascii_lowercase();
    if t.len() > MAX {
        return Err(format!("The code can be {MAX} characters at most."));
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("The code may only hold letters, digits, - and _.".into());
    }
    Ok(t)
}

/// True for a line that opens a TOML table (`[name]`, `[[name]]`): a top-level key must come before the first one.
fn is_table(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

/// The top-level `private_code = "..."` line's key part matches.
fn is_code_line(line: &str) -> bool {
    let t = line.trim_start();
    t.strip_prefix(KEY)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// The code set in a `nrpvp.toml` text ("" when none or not readable).
pub fn read(toml: &str) -> String {
    for line in toml.lines() {
        if is_table(line) {
            break;
        }
        if is_code_line(line) {
            let value = line.split_once('=').map_or("", |(_, v)| v);
            let value = value.split('#').next().unwrap_or("").trim();
            let inner = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or("");
            return normalize(inner).unwrap_or_default();
        }
    }
    String::new()
}

/// `toml` with its top-level code set to `code` (already normalised; "" = public). The old pair (our comment, the blank line before it
/// and the key line) is removed; the new one goes right after the last top-level key, so it stays before the first table (a key after
/// `[hawk]` would belong to that table) and a table's own comment stays on top of it. Writing twice gives the same text.
pub fn write(toml: &str, code: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut in_tables = false;
    for line in toml.lines() {
        in_tables |= is_table(line);
        if !in_tables && line.trim() == COMMENT {
            if out.last().is_some_and(|l| l.trim().is_empty()) {
                out.pop();
            }
            continue;
        }
        if !in_tables && is_code_line(line) {
            continue;
        }
        out.push(line);
    }
    let first_table = out.iter().position(|l| is_table(l)).unwrap_or(out.len());
    let is_key = |l: &&str| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
    };
    let pair = format!("{KEY} = \"{code}\"");
    let mut lines: Vec<&str> = Vec::new();
    match out[..first_table].iter().rposition(is_key) {
        Some(last_key) => {
            lines.extend(&out[..=last_key]);
            lines.extend(["", COMMENT, pair.as_str()]);
            lines.extend(&out[last_key + 1..]);
        }
        None => {
            lines.extend([COMMENT, pair.as_str()]);
            let rest: Vec<&str> = out
                .iter()
                .copied()
                .skip_while(|l| l.trim().is_empty())
                .collect();
            if !rest.is_empty() {
                lines.push("");
            }
            lines.extend(rest);
        }
    }
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &str = "# Nightreign PvP settings.\nteam_size = 1\nmatchmaking = true\nmm_max_players = 2\n\n# Hawk crossing. Keep this table last.\n[hawk]\nenabled = true\n";

    #[test]
    fn a_code_is_normalised_like_the_mod_does() {
        assert_eq!(normalize("  Night-Owls_2 ").unwrap(), "night-owls_2");
        assert_eq!(normalize("").unwrap(), "");
        assert!(normalize("two words").is_err());
        assert!(normalize("é").is_err());
        assert!(normalize(&"a".repeat(33)).is_err());
        assert_eq!(normalize(&"a".repeat(32)).unwrap().len(), 32);
    }

    #[test]
    fn the_code_goes_before_the_first_table_and_reads_back() {
        let t = write(RELEASE, "night-owls");
        assert_eq!(read(&t), "night-owls");
        let code_at = t.find("private_code = \"night-owls\"").unwrap();
        assert!(code_at < t.find("[hawk]").unwrap(), "{t}");
        assert!(
            t.contains("\n\n# Hawk crossing. Keep this table last.\n[hawk]\nenabled = true\n"),
            "the table keeps its own comment: {t}"
        );
        assert!(t.contains(&format!(
            "mm_max_players = 2\n\n{COMMENT}\nprivate_code = \"night-owls\"\n\n# Hawk"
        )));
        assert_eq!(write(&t, "night-owls"), t, "stable");
    }

    #[test]
    fn writing_again_replaces_the_line_and_empty_means_public() {
        let once = write(RELEASE, "alpha");
        let twice = write(&once, "beta");
        assert_eq!(twice.matches("private_code").count(), 1, "{twice}");
        assert_eq!(twice.matches(COMMENT).count(), 1);
        assert_eq!(read(&twice), "beta");
        let public = write(&twice, "");
        assert_eq!(read(&public), "");
        assert_eq!(write(&public, ""), public, "stable");
    }

    #[test]
    fn the_mods_own_template_line_is_read_and_replaced() {
        let t = "team_size = 1\n# RW-6 private match ...\nprivate_code = \"Friends\"   # a note\n\n[hawk]\nenabled = true\n";
        assert_eq!(read(t), "friends");
        let w = write(t, "other");
        assert_eq!(w.matches("private_code =").count(), 1, "{w}");
        assert_eq!(read(&w), "other");
    }

    #[test]
    fn a_key_inside_a_table_is_not_ours_and_no_table_is_fine() {
        assert_eq!(read("[hawk]\nprivate_code = \"x\"\n"), "");
        let w = write("team_size = 1\n", "abc");
        assert_eq!(
            w,
            format!("team_size = 1\n\n{COMMENT}\nprivate_code = \"abc\"\n")
        );
        let only_table = write("[hawk]\nenabled = true\n", "abc");
        assert_eq!(
            only_table,
            format!("{COMMENT}\nprivate_code = \"abc\"\n\n[hawk]\nenabled = true\n")
        );
        assert_eq!(write(&only_table, "abc"), only_table, "stable");
        assert_eq!(read("private_code = 'single'\n"), "single");
        assert_eq!(
            read("private_code = \"bad code\"\n"),
            "",
            "an invalid code reads as public, as in the mod"
        );
    }
}
