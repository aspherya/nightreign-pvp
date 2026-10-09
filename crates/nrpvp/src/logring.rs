//! T-013: in-memory tail of the log for the overlay. A second `tracing` layer (see `logging.rs`) formats
//! every event like the file layer does and pushes the lines here; the overlay copies them out on the
//! render thread. Nothing in this module touches game memory.

use std::collections::VecDeque;
use std::io;
use std::sync::{Mutex, MutexGuard, PoisonError};

use tracing_subscriber::fmt::MakeWriter;

/// How many formatted lines the overlay can scroll back through.
pub(crate) const RING_CAPACITY: usize = 300;

/// Fixed-capacity line buffer. `seq` counts every line ever pushed, so a reader can tell cheaply whether
/// anything changed since its last copy.
pub(crate) struct LogRing {
    lines: VecDeque<String>,
    cap: usize,
    seq: u64,
}

impl LogRing {
    pub(crate) const fn new(cap: usize) -> Self {
        LogRing {
            lines: VecDeque::new(),
            cap,
            seq: 0,
        }
    }

    /// Append one line (no trailing newline), dropping the oldest when full.
    pub(crate) fn push(&mut self, line: String) {
        if self.cap == 0 {
            return;
        }
        while self.lines.len() >= self.cap {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
        self.seq += 1;
    }

    /// Append possibly multi-line text as separate lines. Trailing `\r`/`\n` are dropped and blank lines
    /// are skipped.
    pub(crate) fn push_text(&mut self, text: &str) {
        for l in text.lines() {
            let l = l.trim_end_matches(['\r', '\n']);
            if !l.is_empty() {
                self.push(l.to_string());
            }
        }
    }

    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lines.len()
    }

    /// Oldest-first copy of the retained lines.
    pub(crate) fn lines(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }
}

static GLOBAL: Mutex<LogRing> = Mutex::new(LogRing::new(RING_CAPACITY));

fn lock() -> MutexGuard<'static, LogRing> {
    // A panic elsewhere must never make logging (or the overlay) panic in turn.
    GLOBAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `(seq, lines)` copy of the global ring. The lock is held only for the copy; never log while holding it.
pub(crate) fn snapshot() -> (u64, Vec<String>) {
    let g = lock();
    (g.seq(), g.lines())
}

/// Sequence number of the global ring (cheap "did anything change" check).
pub(crate) fn seq() -> u64 {
    lock().seq()
}

fn push_global(text: &str) {
    lock().push_text(text);
}

/// `MakeWriter` for the fmt layer that feeds the global ring.
#[derive(Clone, Copy, Default)]
pub(crate) struct RingMakeWriter;

/// Collects what the fmt layer writes for one event and pushes it to the ring when dropped.
pub(crate) struct RingWriter {
    buf: Vec<u8>,
}

impl io::Write for RingWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for RingWriter {
    fn drop(&mut self) {
        if !self.buf.is_empty() {
            push_global(&String::from_utf8_lossy(&self.buf));
        }
    }
}

impl<'a> MakeWriter<'a> for RingMakeWriter {
    type Writer = RingWriter;

    fn make_writer(&'a self) -> RingWriter {
        RingWriter { buf: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn keeps_only_the_newest_lines_in_order() {
        let mut r = LogRing::new(3);
        assert_eq!((r.len(), r.seq()), (0, 0));
        for i in 1..=5 {
            r.push(format!("line {i}"));
        }
        assert_eq!(r.lines(), vec!["line 3", "line 4", "line 5"]);
        assert_eq!(r.len(), 3);
        // seq counts every push, not what is retained.
        assert_eq!(r.seq(), 5);
    }

    #[test]
    fn zero_capacity_ring_drops_everything() {
        let mut r = LogRing::new(0);
        r.push("x".into());
        assert_eq!((r.len(), r.seq()), (0, 0));
    }

    #[test]
    fn push_text_splits_lines_and_trims() {
        let mut r = LogRing::new(10);
        r.push_text("a\nb\r\n\n\nc\n");
        assert_eq!(r.lines(), vec!["a", "b", "c"]);
        assert_eq!(r.seq(), 3);
        r.push_text("");
        r.push_text("\n");
        assert_eq!(r.seq(), 3);
    }

    #[test]
    fn default_capacity_is_about_300() {
        let mut r = LogRing::new(RING_CAPACITY);
        for i in 0..1000 {
            r.push(format!("{i}"));
        }
        assert_eq!(r.len(), RING_CAPACITY);
        assert_eq!(r.lines().first().map(String::as_str), Some("700"));
        assert_eq!(r.lines().last().map(String::as_str), Some("999"));
    }

    #[test]
    fn writer_pushes_one_event_to_the_global_ring_on_drop() {
        let before = seq();
        {
            let mut w = RingMakeWriter.make_writer();
            w.write_all(b"  0.001s  INFO ring-writer-test-marker\n")
                .expect("write");
            // Not visible until the writer is dropped (the layer writes the whole event, then drops it).
        }
        let (after, lines) = snapshot();
        assert!(after > before);
        assert!(lines.iter().any(|l| l.contains("ring-writer-test-marker")));
    }
}
