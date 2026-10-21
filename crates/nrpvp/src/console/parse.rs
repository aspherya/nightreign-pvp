//! CON-1: console line parsing, CS style (CONSOLE.md §3). Pure, unit-tested.
//!
//! - `;` separates commands (outside quotes): `team_set 29 all; players`.
//! - Whitespace separates tokens; `"..."` keeps spaces inside one token (`echo "two words"`).
//! - A leading `/` on a command name is accepted and ignored (`/kill` = `kill`).
//! - Empty commands are dropped.

/// Split one input line into commands, each a list of tokens (name first).
pub(crate) fn split_commands(line: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut cmd: Vec<String> = Vec::new();
    let mut tok = String::new();
    // A token is "open" once it started, even if empty (`""` is an empty argument).
    let mut open = false;
    let mut quoted = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                open = true;
            }
            ';' if !quoted => {
                flush(&mut tok, &mut open, &mut cmd);
                push_cmd(&mut cmd, &mut out);
            }
            c if c.is_whitespace() && !quoted => flush(&mut tok, &mut open, &mut cmd),
            c => {
                tok.push(c);
                open = true;
            }
        }
    }
    flush(&mut tok, &mut open, &mut cmd);
    push_cmd(&mut cmd, &mut out);
    out
}

fn flush(tok: &mut String, open: &mut bool, cmd: &mut Vec<String>) {
    if *open {
        cmd.push(std::mem::take(tok));
        *open = false;
    }
}

fn push_cmd(cmd: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    if cmd.is_empty() {
        return;
    }
    let mut c = std::mem::take(cmd);
    if let Some(stripped) = c[0].strip_prefix('/') {
        c[0] = stripped.to_string();
    }
    if !c[0].is_empty() {
        c[0] = c[0].to_ascii_lowercase();
        out.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&[&str]]) -> Vec<Vec<String>> {
        v.iter()
            .map(|c| c.iter().map(|t| t.to_string()).collect())
            .collect()
    }

    #[test]
    fn tokens_commands_quotes_and_slash() {
        assert_eq!(split_commands("status"), s(&[&["status"]]));
        assert_eq!(
            split_commands("  team_set 29   all ;players;"),
            s(&[&["team_set", "29", "all"], &["players"]])
        );
        assert_eq!(
            split_commands(r#"echo "a; b c" d"#),
            s(&[&["echo", "a; b c", "d"]])
        );
        assert_eq!(split_commands("/KILL p2"), s(&[&["kill", "p2"]]));
        assert_eq!(split_commands(r#"echo """#), s(&[&["echo", ""]]));
        assert!(split_commands(" ; ;  ").is_empty());
        assert!(split_commands("/").is_empty());
    }
}
