//! Matching rules for the log-file steps (`wait_log`, `expect_log`): a pure function over the log's bytes and a start offset. The
//! runner (`nro::run`) reads `runs\<id>\mod\nrpvp.log` and the offsets; this decides what the text after the offset says.
//!
//! Rules:
//! - Only text from `offset` on counts. An offset past the end (the mod truncated the log) means "from the start".
//! - CRLF counts as LF; matching is case-insensitive like [`Contains`].
//! - `contains` is looked for in COMPLETE lines only: the last line may still be half-written, so a needle cut by the end of the
//!   file does not match yet. `absent` looks at everything, partial line included: a forbidden text that is visible is a failure.

use crate::scenario::Contains;

/// What the text after the offset says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    /// Every `contains` needle is in a complete line after the offset (no needles = true).
    pub found: bool,
    /// The first `absent` needle that appears after the offset.
    pub forbidden: Option<String>,
}

impl Scan {
    /// `found` and nothing forbidden.
    pub fn ok(&self) -> bool {
        self.found && self.forbidden.is_none()
    }
}

/// The text of `bytes` from `offset` (clamped to a truncated log), CRLF turned into LF, invalid UTF-8 replaced.
fn tail(bytes: &[u8], offset: usize) -> String {
    let from = if offset > bytes.len() { 0 } else { offset };
    String::from_utf8_lossy(&bytes[from..]).replace("\r\n", "\n")
}

/// Scan the log `bytes` from `offset`.
pub fn scan(bytes: &[u8], offset: usize, contains: &Contains, absent: &Contains) -> Scan {
    let text = tail(bytes, offset);
    let complete = match text.rfind('\n') {
        Some(i) => &text[..=i],
        None => "",
    };
    let forbidden = absent
        .needles()
        .into_iter()
        .find(|n| text.to_ascii_lowercase().contains(&n.to_ascii_lowercase()))
        .map(String::from);
    Scan {
        found: contains.all_in(complete),
        forbidden,
    }
}

/// The last `max` characters of the text after `offset`, for a step's detail line.
pub fn excerpt(bytes: &[u8], offset: usize, max: usize) -> String {
    let t = tail(bytes, offset);
    let n = t.chars().count();
    t.chars().skip(n.saturating_sub(max)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(v: &[&str]) -> Contains {
        Contains::All(v.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn finds_needles_case_insensitively_across_lines() {
        let log = b"a: start\nb: Match Decided\nc: won\n";
        assert!(scan(log, 0, &c(&["match decided", "WON"]), &Contains::None).ok());
        assert!(!scan(log, 0, &c(&["match decided", "lost"]), &Contains::None).found);
        assert!(scan(log, 0, &Contains::None, &Contains::None).ok());
    }

    #[test]
    fn crlf_is_a_line_break() {
        let log = b"one\r\ntwo needle\r\n";
        assert!(scan(log, 0, &c(&["two needle"]), &Contains::None).ok());
        // The text before the offset is ignored; the offset is a byte offset.
        assert!(!scan(log, 5, &c(&["one"]), &Contains::None).found);
        assert!(scan(log, 5, &c(&["two"]), &Contains::None).found);
    }

    #[test]
    fn the_partial_last_line_does_not_satisfy_contains() {
        let half = b"done\nmatch deci";
        assert!(!scan(half, 0, &c(&["match decided"]), &Contains::None).found);
        let whole = b"done\nmatch decided";
        assert!(
            !scan(whole, 0, &c(&["match decided"]), &Contains::None).found,
            "no newline yet: the line may still grow"
        );
        let done = b"done\nmatch decided\n";
        assert!(scan(done, 0, &c(&["match decided"]), &Contains::None).found);
        // A needle in the complete part still counts while a partial line follows.
        assert!(scan(b"done\nmatch deci", 0, &c(&["done"]), &Contains::None).found);
    }

    #[test]
    fn absent_sees_the_partial_line_too() {
        let log = b"fine\npan";
        assert!(scan(log, 0, &Contains::None, &c(&["panic"])).ok());
        let log = b"fine\npanic: boom";
        let s = scan(log, 0, &Contains::None, &c(&["panic"]));
        assert_eq!(s.forbidden.as_deref(), Some("panic"));
        assert!(!s.ok());
    }

    #[test]
    fn absent_only_counts_after_the_offset() {
        // The offset is taken BEFORE the triggering command: the line it logs is after it and counts; older text does not.
        let before = b"old panic line\n";
        let mut log = before.to_vec();
        assert!(
            scan(&log, before.len(), &c(&["RELEASED"]), &c(&["panic"]))
                .forbidden
                .is_none(),
            "old panic is before the offset"
        );
        log.extend_from_slice(b"RELEASED\n");
        assert!(scan(&log, before.len(), &c(&["RELEASED"]), &c(&["panic"])).ok());
        log.extend_from_slice(b"panic at x\n");
        let s = scan(&log, before.len(), &c(&["RELEASED"]), &c(&["panic"]));
        assert!(s.found);
        assert_eq!(s.forbidden.as_deref(), Some("panic"));
        // From the start of the run the old line is seen too.
        assert!(scan(&log, 0, &Contains::None, &c(&["panic"]))
            .forbidden
            .is_some());
    }

    #[test]
    fn a_truncated_log_is_read_from_the_start() {
        let log = b"fresh line\n";
        assert!(scan(log, 5_000, &c(&["fresh line"]), &Contains::None).ok());
    }

    #[test]
    fn an_offset_inside_a_utf8_character_does_not_panic() {
        let log = "caf\u{e9} ok\n".as_bytes();
        assert!(scan(log, 4, &c(&["ok"]), &Contains::None).found);
    }

    #[test]
    fn excerpt_is_the_end_of_the_text_after_the_offset() {
        assert_eq!(excerpt(b"abcdef\n", 2, 4), "def\n".to_string());
        assert_eq!(excerpt(b"ab", 9, 10), "ab");
    }
}
