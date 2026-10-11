//! Configurable hotkeys (T-007a F9, T-008 F4-F8, T-013 bindings from `nrpvp.toml`): `GetAsyncKeyState`
//! polled every ~50 ms on our own thread, edge-triggered, and only counted while a window of THIS process
//! is in the foreground. Read-only input polling: no hooks. The thread only raises flags; the consumers
//! (`game::watch`, and the overlay for [`Action::ToggleOverlay`]) take them.
//!
//! Key names accepted in the config: `F1`..`F24`, `A`..`Z`, `0`..`9`, `Numpad0`..`Numpad9`, `Insert`,
//! `Delete`, `Home`, `End`, `PageUp`, `PageDown`, `Pause`, `ScrollLock` (case-insensitive). An empty
//! string means "unbound".

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_DELETE, VK_END, VK_F1, VK_HOME, VK_INSERT, VK_NEXT, VK_NUMPAD0, VK_PAUSE,
    VK_PRIOR, VK_SCROLL,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

const POLL: Duration = Duration::from_millis(50);

/// What a key press asks for. The discriminant is the pending-flag bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Write the selected candidate to the local player.
    Write,
    /// Restore the captured original value.
    Restore,
    /// Toggle persist mode.
    Persist,
    /// Run the team_type histogram.
    Histogram,
    /// Run the character discovery.
    Discovery,
    /// Select the next team candidate (unbound by default; the overlay buttons select).
    Cycle,
    /// Show/hide the overlay.
    ToggleOverlay,
    /// T-015: log a numbered `MARK #n: (hotkey)` line.
    Mark,
    /// FS-0: write the selected candidate to every player.
    WriteAll,
    /// FS-0: restore every player.
    RestoreAll,
    /// T-031: select the next `downed_rival_team` candidate (dev mode).
    DownedCycle,
    /// T-032: select the next snap-back position candidate (dev mode).
    PosCycle,
    /// T-032: save the local player's current values of every position candidate (dev mode).
    PosSave,
    /// T-032: write the saved values into the selected position candidate, local player only (dev mode).
    PosWrite,
}

pub(crate) const ALL_ACTIONS: [Action; 14] = [
    Action::Write,
    Action::Restore,
    Action::Persist,
    Action::Histogram,
    Action::Discovery,
    Action::Cycle,
    Action::ToggleOverlay,
    Action::Mark,
    Action::WriteAll,
    Action::RestoreAll,
    Action::DownedCycle,
    Action::PosCycle,
    Action::PosSave,
    Action::PosWrite,
];

impl Action {
    fn bit(self) -> u32 {
        1 << (self as u32)
    }
}

/// A resolved key: virtual-key code plus the canonical spelling used in logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeyBind {
    pub vk: u16,
    pub name: String,
}

/// Parse a config key name. `None` for unknown names (the empty string is handled by the caller as
/// "unbound").
pub(crate) fn parse_key(s: &str) -> Option<KeyBind> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    let named: &[(&str, u16, &str)] = &[
        ("insert", VK_INSERT.0, "Insert"),
        ("delete", VK_DELETE.0, "Delete"),
        ("home", VK_HOME.0, "Home"),
        ("end", VK_END.0, "End"),
        ("pageup", VK_PRIOR.0, "PageUp"),
        ("pagedown", VK_NEXT.0, "PageDown"),
        ("pause", VK_PAUSE.0, "Pause"),
        ("scrolllock", VK_SCROLL.0, "ScrollLock"),
    ];
    if let Some(&(_, vk, name)) = named.iter().find(|(n, _, _)| *n == lower) {
        return Some(KeyBind {
            vk,
            name: name.to_string(),
        });
    }
    // F1..F24
    if let Some(n) = lower.strip_prefix('f').and_then(|d| d.parse::<u16>().ok()) {
        if (1..=24).contains(&n) && !lower[1..].starts_with('+') {
            return Some(KeyBind {
                vk: VK_F1.0 + (n - 1),
                name: format!("F{n}"),
            });
        }
    }
    // Numpad0..Numpad9
    if let Some(d) = lower.strip_prefix("numpad") {
        if let Ok(n) = d.parse::<u16>() {
            if n <= 9 && d.len() == 1 {
                return Some(KeyBind {
                    vk: VK_NUMPAD0.0 + n,
                    name: format!("Numpad{n}"),
                });
            }
        }
    }
    // A single letter or digit: its virtual-key code is the upper-case ASCII value.
    let mut chars = t.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphanumeric() {
            let up = c.to_ascii_uppercase();
            return Some(KeyBind {
                vk: up as u16,
                name: up.to_string(),
            });
        }
    }
    None
}

/// One pending-press bit per action. Set by the hotkey thread on each press edge, consumed by the loop.
pub(crate) struct Pending(AtomicU32);

impl Pending {
    pub(crate) const fn new() -> Self {
        Pending(AtomicU32::new(0))
    }

    pub(crate) fn raise(&self, a: Action) {
        self.0.fetch_or(a.bit(), Ordering::SeqCst);
    }

    /// True once per press since the last call for that action.
    pub(crate) fn take(&self, a: Action) -> bool {
        self.0.fetch_and(!a.bit(), Ordering::SeqCst) & a.bit() != 0
    }
}

static PENDING: Pending = Pending::new();

/// Returns true once per press of `a` since the last call.
pub(crate) fn take(a: Action) -> bool {
    PENDING.take(a)
}

/// Rising-edge detector: `update(true)` returns true only on a false -> true transition.
#[derive(Default)]
pub(crate) struct Edge {
    prev: bool,
}

impl Edge {
    pub(crate) fn update(&mut self, down: bool) -> bool {
        let edge = down && !self.prev;
        self.prev = down;
        edge
    }
}

/// True if the foreground window belongs to this process (the game window, in practice).
fn own_window_is_foreground() -> bool {
    // SAFETY: plain Win32 queries with no pointers into our memory except the pid out-parameter, which is
    // a valid local `u32`. A null/invalid HWND (no foreground window) is handled.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid != 0 && pid == GetCurrentProcessId()
    }
}

fn vk_down(vk: u16) -> bool {
    // SAFETY: GetAsyncKeyState takes a virtual-key code and has no memory-safety preconditions. The high
    // bit (sign of the i16) means "currently down".
    let state = unsafe { GetAsyncKeyState(i32::from(vk)) };
    state < 0
}

/// Spawn the polling thread for the given bindings. Failure to spawn is logged and is not fatal.
pub(crate) fn spawn(bindings: Vec<(Action, KeyBind)>) {
    if bindings.is_empty() {
        tracing::info!("hotkeys: no keys bound; the polling thread is not started");
        return;
    }
    let r = std::thread::Builder::new()
        .name("nrpvp-hotkey".into())
        .spawn(move || {
            let mut edges: Vec<Edge> = bindings.iter().map(|_| Edge::default()).collect();
            loop {
                // Only count keys while the game is foreground; otherwise treat them as released so a
                // key held while alt-tabbing does not fire on return.
                let fg = own_window_is_foreground();
                for ((action, key), edge) in bindings.iter().zip(edges.iter_mut()) {
                    if edge.update(fg && vk_down(key.vk)) {
                        PENDING.raise(*action);
                    }
                }
                std::thread::sleep(POLL);
            }
        });
    if let Err(e) = r {
        tracing::error!("hotkeys: could not spawn the hotkey thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_fires_once_per_press() {
        let mut e = Edge::default();
        let seq = [false, true, true, true, false, true, false, false, true];
        let fired: Vec<bool> = seq.iter().map(|&d| e.update(d)).collect();
        assert_eq!(
            fired,
            vec![false, true, false, false, false, true, false, false, true]
        );
    }

    #[test]
    fn pending_flags_are_per_action_and_consumed() {
        let p = Pending::new();
        p.raise(Action::Write);
        p.raise(Action::Discovery);
        assert!(!p.take(Action::Cycle));
        assert!(p.take(Action::Write));
        assert!(!p.take(Action::Write));
        // Taking one action leaves the others pending.
        assert!(p.take(Action::Discovery));
        assert!(!p.take(Action::Discovery));
        // Raised twice before being taken is still one press.
        p.raise(Action::Histogram);
        p.raise(Action::Histogram);
        assert!(p.take(Action::Histogram));
        assert!(!p.take(Action::Histogram));
    }

    #[test]
    fn every_action_has_a_distinct_bit() {
        let mut bits = 0u32;
        for a in ALL_ACTIONS {
            assert_eq!(bits & a.bit(), 0);
            bits |= a.bit();
        }
    }

    #[test]
    fn key_names_parse_to_virtual_keys() {
        let k = |s: &str| parse_key(s).map(|b| (b.vk, b.name));
        assert_eq!(k("F4"), Some((0x73, "F4".to_string())));
        assert_eq!(k("f9"), Some((0x78, "F9".to_string())));
        assert_eq!(k(" F24 "), Some((0x87, "F24".to_string())));
        assert_eq!(k("Insert"), Some((0x2D, "Insert".to_string())));
        assert_eq!(k("INSERT"), Some((0x2D, "Insert".to_string())));
        assert_eq!(k("delete"), Some((0x2E, "Delete".to_string())));
        assert_eq!(k("PageUp"), Some((0x21, "PageUp".to_string())));
        assert_eq!(k("pagedown"), Some((0x22, "PageDown".to_string())));
        assert_eq!(k("Home"), Some((0x24, "Home".to_string())));
        assert_eq!(k("End"), Some((0x23, "End".to_string())));
        assert_eq!(k("Pause"), Some((0x13, "Pause".to_string())));
        assert_eq!(k("ScrollLock"), Some((0x91, "ScrollLock".to_string())));
        assert_eq!(k("numpad5"), Some((0x65, "Numpad5".to_string())));
        assert_eq!(k("g"), Some((0x47, "G".to_string())));
        assert_eq!(k("7"), Some((0x37, "7".to_string())));
    }

    #[test]
    fn bad_key_names_are_rejected() {
        for s in [
            "", "  ", "F0", "F25", "FF", "F+1", "Numpad10", "Numpad", "Esc", "ab", "-", "Ins", "ü",
        ] {
            assert_eq!(parse_key(s), None, "{s:?}");
        }
    }

    #[test]
    fn win32_queries_do_not_panic() {
        // In a test harness the keys are up; the foreground window is whatever it is.
        for s in ["F5", "Insert", "A"] {
            if let Some(k) = parse_key(s) {
                let _ = vk_down(k.vk);
            }
        }
        let _ = own_window_is_foreground();
    }
}
