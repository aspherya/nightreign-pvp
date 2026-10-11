//! T-031 (b): the rival-revive experiment switch, now INERT.
//!
//! FS-0 run 6 (NR 1.3.3.0) disproved the candidate "downed" marker (ChrSet entry byte +9 never changed during downs), so the
//! planner that wrote `downed_rival_team` to downed rivals was removed in T-032: nothing writes based on a marker we do not
//! know. Only the value list remains, so the config key and the F8 cycling keep working (they select a value that nothing
//! uses yet) until T-032's MARK dumps find the real marker.

/// `downed_rival_team` and the list the F8 key steps through. `current() == 0` means the experiment is off.
#[derive(Debug, Clone)]
pub(crate) struct DownedCycle {
    list: Vec<u8>,
    cur: u8,
}

impl DownedCycle {
    /// `start` is the configured `downed_rival_team`; it need not be in `list`. An empty list uses the defaults.
    pub(crate) fn new(start: u8, list: &[u8]) -> Self {
        let list = if list.is_empty() {
            crate::config::DEFAULT_DOWNED_CANDIDATES.to_vec()
        } else {
            list.to_vec()
        };
        DownedCycle { list, cur: start }
    }

    pub(crate) fn current(&self) -> u8 {
        self.cur
    }

    /// Step to the next list entry (the first one when the current value is not in the list).
    pub(crate) fn next(&mut self) -> u8 {
        let i = self.list.iter().position(|&v| v == self.cur);
        self.cur = match i {
            Some(i) => self.list[(i + 1) % self.list.len()],
            None => self.list[0],
        };
        self.cur
    }

    /// 1-based position in the list and the list length, if the current value is in the list.
    pub(crate) fn position(&self) -> Option<(usize, usize)> {
        self.list
            .iter()
            .position(|&v| v == self.cur)
            .map(|i| (i + 1, self.list.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_steps_through_the_list_and_wraps() {
        let mut c = DownedCycle::new(0, &[0, 6, 14, 30, 2]);
        assert_eq!(c.current(), 0);
        assert_eq!(c.position(), Some((1, 5)));
        let seq: Vec<u8> = (0..6).map(|_| c.next()).collect();
        assert_eq!(seq, [6, 14, 30, 2, 0, 6]);
        // A start value outside the list jumps to the first entry on the first step.
        let mut c = DownedCycle::new(9, &[0, 6]);
        assert_eq!(c.position(), None);
        assert_eq!(c.next(), 0);
        // An empty list falls back to the defaults.
        let c = DownedCycle::new(0, &[]);
        assert_eq!(c.list, crate::config::DEFAULT_DOWNED_CANDIDATES.to_vec());
    }
}
