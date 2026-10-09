//! T-015: numbered MARK notes. A tester annotates a moment ("A hits B", "B died") and the line lands in
//! both PCs' logs next to the write lines, so they can be lined up by timestamp. Pure logic; the main loop
//! owns the counter and does the logging.

/// Longest note kept (characters); the rest is cut so one stray paste cannot flood the log.
pub(crate) const MAX_TEXT_CHARS: usize = 200;

/// Running MARK counter. The numbering is per session and is not reset by WorldChrMan changes.
#[derive(Default)]
pub(crate) struct Marker {
    n: u32,
}

impl Marker {
    /// How many marks were logged so far.
    pub(crate) fn count(&self) -> u32 {
        self.n
    }

    /// Next mark: `MARK #<n>: <text>`. `None` (hotkey) gives `(hotkey)`, blank text gives `(no text)`.
    pub(crate) fn next_line(&mut self, text: Option<&str>) -> String {
        self.n = self.n.saturating_add(1);
        let body = match text {
            None => "(hotkey)".to_string(),
            Some(t) => {
                let clean = clean_text(t);
                if clean.is_empty() {
                    "(no text)".to_string()
                } else {
                    clean
                }
            }
        };
        format!("MARK #{}: {}", self.n, body)
    }
}

/// One line, trimmed, control characters (newlines, tabs) turned into spaces, capped at
/// [`MAX_TEXT_CHARS`].
pub(crate) fn clean_text(t: &str) -> String {
    let one_line: String = t
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    one_line.trim().chars().take(MAX_TEXT_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_is_sequential_across_sources() {
        let mut m = Marker::default();
        assert_eq!(m.count(), 0);
        assert_eq!(m.next_line(Some("A hits B")), "MARK #1: A hits B");
        assert_eq!(m.next_line(None), "MARK #2: (hotkey)");
        assert_eq!(m.next_line(Some("B died")), "MARK #3: B died");
        assert_eq!(m.count(), 3);
    }

    #[test]
    fn blank_text_still_counts() {
        let mut m = Marker::default();
        assert_eq!(m.next_line(Some("   ")), "MARK #1: (no text)");
        assert_eq!(m.next_line(Some("")), "MARK #2: (no text)");
    }

    #[test]
    fn text_is_one_line_and_bounded() {
        assert_eq!(clean_text("  a\nb\tc\r\n "), "a b c");
        let long = "x".repeat(MAX_TEXT_CHARS + 50);
        assert_eq!(clean_text(&long).chars().count(), MAX_TEXT_CHARS);
        // Multi-byte characters are cut on character boundaries, not bytes.
        let wide = "é".repeat(MAX_TEXT_CHARS + 5);
        assert_eq!(clean_text(&wide).chars().count(), MAX_TEXT_CHARS);
    }
}
