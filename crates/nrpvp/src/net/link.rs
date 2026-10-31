//! NET-1: the port between the data-channel logic and whatever carries the bytes (NETWORKING.md §4). Features and the worker only
//! know [`PeerLink`]; the Steam implementation lives in `steam_link.rs`, tests use [`FakeLink`].

use super::msg::Msg;

pub(crate) type SteamId = u64;

pub(crate) trait PeerLink {
    /// Our own Steam id (0 = not known yet).
    fn own_id(&self) -> SteamId;
    /// Send one message to one peer (reliable). `Err` carries a short reason for the log.
    fn send(&mut self, to: SteamId, msg: &Msg) -> Result<(), String>;
    /// Move every message that arrived since the last call into `out` as `(sender, message)`. Undecodable input is dropped inside
    /// the link and counted in [`PeerLink::dropped`].
    fn poll(&mut self, out: &mut Vec<(SteamId, Msg)>);
    /// Messages dropped so far because they did not decode.
    fn dropped(&self) -> u64;
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    type Queue = Rc<RefCell<VecDeque<(SteamId, Msg)>>>;

    /// An in-memory link: two ends share crossed queues.
    pub(crate) struct FakeLink {
        id: SteamId,
        inbox: Queue,
        peer_inbox: Queue,
    }

    impl FakeLink {
        pub(crate) fn pair(a: SteamId, b: SteamId) -> (FakeLink, FakeLink) {
            let qa: Queue = Rc::default();
            let qb: Queue = Rc::default();
            (
                FakeLink {
                    id: a,
                    inbox: qa.clone(),
                    peer_inbox: qb.clone(),
                },
                FakeLink {
                    id: b,
                    inbox: qb,
                    peer_inbox: qa,
                },
            )
        }
    }

    impl PeerLink for FakeLink {
        fn own_id(&self) -> SteamId {
            self.id
        }
        fn send(&mut self, _to: SteamId, msg: &Msg) -> Result<(), String> {
            self.peer_inbox.borrow_mut().push_back((self.id, *msg));
            Ok(())
        }
        fn poll(&mut self, out: &mut Vec<(SteamId, Msg)>) {
            out.extend(self.inbox.borrow_mut().drain(..));
        }
        fn dropped(&self) -> u64 {
            0
        }
    }
}
