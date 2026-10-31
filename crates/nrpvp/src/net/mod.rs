//! NET-1/2: the mod-to-mod data channel (NETWORKING.md). A private `ISteamNetworkingMessages` channel between our DLLs, independent of
//! Seamless Co-op's session. On with config `net = true` (developer builds) or with matchmaking (player build).
//!
//! Features use only [`bus`] (`publish` / `take`). Threads: one worker (`nrpvp-net`) owns the Steam link and the [`chan::Channel`]; the
//! roster comes from the Seamless Co-op lobby ([`feed`], filled by the Steam thread) plus peers added by console; the console talks to the
//! worker through [`send_cmd`] and reads the status line from [`status`]. Nothing on the game loop waits for Steam.

pub(crate) mod bus;
pub(crate) mod chan;
pub(crate) mod facts;
pub(crate) mod feed;
pub(crate) mod link;
pub(crate) mod msg;
pub(crate) mod steam_link;

use std::collections::BTreeSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::steam::{steam_dll_loaded, Flat};
use link::PeerLink;

/// What the console can ask the worker.
#[cfg_attr(not(feature = "dev"), allow(dead_code))]
#[derive(Debug, Clone, Copy)]
pub(crate) enum Cmd {
    Peer(u64),
    Ping(u32),
}

static CTL: Mutex<Option<Sender<Cmd>>> = Mutex::new(None);
static STATUS: Mutex<String> = Mutex::new(String::new());

const TICK: Duration = Duration::from_millis(50);
const WAIT_STEAM: Duration = Duration::from_secs(300);
/// Build tag sent in `Hello`.
const BUILD: u16 = 1;

#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn send_cmd(c: Cmd) -> Result<(), String> {
    match CTL.lock() {
        Ok(g) => match g.as_ref() {
            Some(tx) => tx.send(c).map_err(|_| "net worker stopped".to_string()),
            None => Err("net is off (config `net = true`, dev mode) or not started yet".into()),
        },
        Err(_) => Err("net lock poisoned".into()),
    }
}

#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn status() -> String {
    STATUS
        .lock()
        .map(|s| {
            if s.is_empty() {
                "net: not started".to_string()
            } else {
                s.clone()
            }
        })
        .unwrap_or_else(|_| "net: status unavailable".to_string())
}

pub(crate) fn spawn() {
    let (tx, rx) = channel();
    if let Ok(mut g) = CTL.lock() {
        *g = Some(tx);
    }
    let r = std::thread::Builder::new()
        .name("nrpvp-net".into())
        .spawn(move || {
            if catch_unwind(AssertUnwindSafe(|| run(rx))).is_err() {
                tracing::error!("net: worker panicked");
            }
        });
    if r.is_err() {
        tracing::warn!("net: could not spawn the worker thread");
    }
}

fn set_status(s: String) {
    if let Ok(mut g) = STATUS.lock() {
        *g = s;
    }
}

fn run(rx: Receiver<Cmd>) {
    let start = Instant::now();
    set_status("net: waiting for Steam".into());
    let link = loop {
        if steam_dll_loaded() {
            let (flat, _) = Flat::resolve();
            if flat.hsteam_user() != 0 {
                match steam_link::SteamLink::open() {
                    Ok(l) => break l,
                    Err(e) => {
                        tracing::warn!("net: cannot open the Messages link: {e}");
                        set_status(format!("net: unavailable ({e})"));
                        return;
                    }
                }
            }
        }
        if start.elapsed() > WAIT_STEAM {
            tracing::warn!("net: gave up waiting for Steam");
            set_status("net: Steam never came up".into());
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    tracing::info!(
        "net: Messages link open after {:.1} s, own Steam id {}, channel {}",
        start.elapsed().as_secs_f32(),
        link.own_id(),
        steam_link::CHANNEL
    );
    let mut ch = chan::Channel::new(link, BUILD);
    let mut last_log = Instant::now();
    let mut last_line = String::new();
    // Roster = peers added by console + members of our Seamless Co-op lobby (feed). Recomputed when either changes.
    let mut manual: BTreeSet<u64> = BTreeSet::new();
    let mut lobby: BTreeSet<u64> = BTreeSet::new();
    loop {
        let now_lobby = feed::lobby_members();
        let mut roster_changed = now_lobby != lobby;
        lobby = now_lobby;
        while let Ok(c) = rx.try_recv() {
            match c {
                Cmd::Peer(id) => {
                    let added = id != 0 && id != ch.own_id() && manual.insert(id);
                    roster_changed |= added;
                    tracing::info!(
                        "net: peer {id} {}",
                        if added {
                            "added"
                        } else {
                            "not added (own id, zero or known)"
                        }
                    );
                }
                Cmd::Ping(n) => {
                    tracing::info!("net: pinging every peer {n} times");
                    ch.ping_all(n, Duration::from_millis(100), Instant::now());
                }
            }
        }
        if roster_changed {
            let all: BTreeSet<u64> = manual.union(&lobby).copied().collect();
            ch.set_roster(&all);
            tracing::info!("net: roster is now {:?}", ch.roster());
        }
        let now = Instant::now();
        for f in bus::drain_out() {
            let ok = ch.publish(f, now);
            tracing::info!(
                "net: publish topic={} key={} value={} -> {}",
                f.topic,
                f.key,
                f.value,
                if ok {
                    "queued"
                } else {
                    "refused (key already settled or a limit)"
                }
            );
        }
        ch.tick(now);
        let got = ch.take_received();
        for r in &got {
            tracing::info!(
                "net: fact topic={} key={} value={} from={}",
                r.fact.topic,
                r.fact.key,
                r.fact.value,
                r.from
            );
            if r.fact.topic == bus::topic::BOSS_VERDICT {
                tracing::info!("net: verdict boss={} team={}", r.fact.key, r.fact.value);
            }
        }
        bus::deliver(got);
        let line = ch.status();
        if line != last_line && last_log.elapsed() >= Duration::from_secs(2) {
            tracing::info!("{line}");
            last_log = Instant::now();
        }
        set_status(line.clone());
        last_line = line;
        std::thread::sleep(TICK);
    }
}
