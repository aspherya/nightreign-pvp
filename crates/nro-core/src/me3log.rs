//! After-launch check L1: me3 prints the attach config it hands to the game (`wrote attach config … attach_config=AttachConfig {
//! … start_online: false, … }`, seen in the mod's `launcher.log`, me3 0.11/0.13). nro reads its own me3 stdout and requires
//! `start_online: false` and exactly the expected modules.

/// Remove ANSI colour escapes (me3 logs with colour even into a file).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else if it.peek() == Some(&']') {
                // OSC: up to BEL.
                for d in it.by_ref() {
                    if d == '\u{7}' {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Attach {
    pub start_online: Option<bool>,
    /// Every `ModFile("…")` path (natives and packages), in order.
    pub files: Vec<String>,
}

/// The attach config in me3's output, if it has been printed yet.
pub fn attach(log: &str) -> Option<Attach> {
    let log = strip_ansi(log);
    let start = log.find("AttachConfig {")?;
    let line = &log[start
        ..log[start..]
            .find('\n')
            .map(|n| start + n)
            .unwrap_or(log.len())];
    let start_online = if line.contains("start_online: false") {
        Some(false)
    } else if line.contains("start_online: true") {
        Some(true)
    } else {
        None
    };
    let mut files = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find("ModFile(\"") {
        rest = &rest[i + 9..];
        if let Some(j) = rest.find("\")") {
            files.push(rest[..j].replace("\\\\", "\\"));
            rest = &rest[j..];
        }
    }
    Some(Attach {
        start_online,
        files,
    })
}

/// L1 verdict: `Ok` when me3 kept the matchmaking block and loaded only what we asked for.
pub fn verify(a: &Attach, expected: &[String]) -> Result<(), String> {
    if a.start_online != Some(false) {
        return Err(format!(
            "me3 attach config start_online = {:?}",
            a.start_online
        ));
    }
    let norm = |s: &str| s.to_ascii_lowercase().replace('/', "\\");
    let mut got: Vec<String> = a.files.iter().map(|s| norm(s)).collect();
    let mut want: Vec<String> = expected.iter().map(|s| norm(s)).collect();
    got.sort();
    want.sort();
    if got != want {
        return Err(format!("me3 loads {:?}, expected {:?}", a.files, expected));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{1b}[32m INFO\u{1b}[0m launch: wrote attach config attach_config=AttachConfig { game: Nightreign, natives: [Native { path: ModFile(\"D:\\\\x\\\\nrpvp.dll\"), optional: false }], early_natives: [Native { path: ModFile(\"D:\\\\x\\\\SeamlessCoop\\\\nrsc.dll\"), load_early: true }], packages: [Package { path: ModFile(\"D:\\\\x\\\\gamedata\") }], savefile: None, start_online: false, disable_arxan: false }\nnext line\n";

    #[test]
    fn reads_the_attach_line() {
        let a = attach(SAMPLE).unwrap();
        assert_eq!(a.start_online, Some(false));
        assert_eq!(
            a.files,
            [
                r"D:\x\nrpvp.dll",
                r"D:\x\SeamlessCoop\nrsc.dll",
                r"D:\x\gamedata"
            ]
        );
        let want = vec![
            r"d:\x\gamedata".to_string(),
            r"D:\x\nrpvp.dll".into(),
            r"D:\x\SeamlessCoop\nrsc.dll".into(),
        ];
        assert!(verify(&a, &want).is_ok());
        assert!(verify(&a, &want[..2]).is_err());
    }

    #[test]
    fn online_or_missing_fails() {
        let a = attach(&SAMPLE.replace("start_online: false", "start_online: true")).unwrap();
        assert!(verify(&a, &[]).is_err());
        assert!(attach("nothing yet").is_none());
    }
}
