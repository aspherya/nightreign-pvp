//! NET-1: the mod-to-mod data channel (NETWORKING.md). A private `ISteamNetworkingMessages` channel between our DLLs, independent of
//! Seamless Co-op's session. Developer builds only for now (`net = true`, dev mode); the player build forces it off.
//!
//! Threads: one worker (`nrpvp-net`) owns the Steam link and the [`chan::Channel`]; the console talks to it through [`send_cmd`] and
//! reads the status line from [`status`]. Nothing on the game loop waits for Steam.

pub(crate) mod chan;
pub(crate) mod link;
pub(crate) mod msg;
pub(crate) mod steam_link;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::steam::{steam_dll_loaded, Flat};
use link::PeerLink;

/// What the console can ask the worker.
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

pub(crate) fn send_cmd(c: Cmd) -> Result<(), String> {
    match CTL.lock() {
        Ok(g) => match g.as_ref() {
            Some(tx) => tx.send(c).map_err(|_| "net worker stopped".to_string()),
            None => Err("net is off (config `net = true`, dev mode) or not started yet".into()),
        },
        Err(_) => Err("net lock poisoned".into()),
    }
}

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
    loop {
        while let Ok(c) = rx.try_recv() {
            match c {
                Cmd::Peer(id) => {
                    let added = ch.add_peer(id);
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
        ch.tick(Instant::now());
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
