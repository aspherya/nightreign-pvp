//! T-013: in-game debug overlay (Dear ImGui through `hudhook`, Direct3D 12 renderer).
//!
//! How it is wired (versions: hudhook 0.9.3 / imgui 0.12, read from the crate source):
//! - We keep our own `DllMain`. hudhook's `hudhook!` macro (which would define a second one) is not used;
//!   instead [`spawn`] starts a thread that waits for the game window and then calls
//!   `Hudhook::builder().with::<ImguiDx12Hooks>(app).build().apply()`. That MinHook-hooks
//!   `IDXGISwapChain::Present` (+ `Present1`, `ResizeBuffers`, `ResizeBuffers1`, `SetSourceSize`,
//!   `IDXGIFactory::CreateSwapChain[ForHwnd]`, `ID3D12CommandQueue::ExecuteCommandLists`) through the vtable
//!   addresses of a throw-away dummy device and swap chain, so it works whenever it is installed.
//!   `with_hmodule` is deliberately NOT called: without it hudhook can never `FreeLibraryAndExitThread` our
//!   DLL (which would crash the game while our other threads run).
//! - Fail safe: any error or panic during install is logged and the mod continues without an overlay; the
//!   hotkeys do not depend on it.
//! - **The render thread never touches game memory.** It reads a [`shared::Snapshot`] published by the main
//!   loop and pushes [`shared::Command`]s that the main loop executes.
//! - Input: hudhook forwards every window message to ImGui and, by default, ALSO to the game (its default
//!   message filter blocks nothing). We return a filter from `message_filter` that blocks mouse (and raw
//!   input) messages from the game while the overlay is visible and ImGui wants the mouse (cursor over the
//!   panel, or a drag in progress), so clicks on the panel do not also hit the game.
//! - ImGui's `imgui.ini`/log files are disabled (nothing is ever written into the game directory).

pub(crate) mod shared;

use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hudhook::hooks::dx12::ImguiDx12Hooks;
use hudhook::imgui::{Condition, Context, Io, StyleColor, Ui};
use hudhook::{Hudhook, ImguiRenderLoop, MessageFilter, RenderContext};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetCursorInfo, GetWindowThreadProcessId, IsWindowVisible,
    CURSORINFO, CURSOR_SHOWING,
};

use crate::hotkey::{self, Action};
use crate::logring;
use shared::{team_name, Command, Shared, Snapshot};

/// How long to wait for the game window before giving up on the overlay.
const WINDOW_WAIT: Duration = Duration::from_secs(600);
const WINDOW_POLL: Duration = Duration::from_millis(500);
/// Extra pause after the window appears, so the game has started creating its swap chain.
const SETTLE: Duration = Duration::from_secs(2);
/// A top-level window smaller than this is not the game's main window.
const MIN_WINDOW: (i32, i32) = (320, 240);
/// Candidate buttons per row.
const BUTTONS_PER_ROW: usize = 4;

const GREEN: [f32; 4] = [0.20, 0.55, 0.25, 1.0];
const RED: [f32; 4] = [0.95, 0.35, 0.30, 1.0];
const AMBER: [f32; 4] = [0.95, 0.75, 0.25, 1.0];
const GREY: [f32; 4] = [0.60, 0.60, 0.60, 1.0];

/// Start the install thread. Returns immediately; never fails the caller.
pub(crate) fn spawn(shared: Arc<Shared>, toggle_name: Option<String>) {
    let r = std::thread::Builder::new()
        .name("nrpvp-overlay".into())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| run(shared, toggle_name)));
            if result.is_err() {
                tracing::error!(
                    "overlay: install thread panicked; continuing without an overlay (hotkeys still work)"
                );
            }
        });
    if let Err(e) = r {
        tracing::error!(
            "overlay: could not spawn the install thread: {e}; continuing without an overlay"
        );
    }
}

fn run(shared: Arc<Shared>, toggle_name: Option<String>) {
    let t0 = Instant::now();
    if !wait_for_game_window() {
        tracing::warn!(
            "overlay: no game window after {} s; continuing without an overlay (hotkeys still work)",
            WINDOW_WAIT.as_secs()
        );
        return;
    }
    tracing::info!(
        "overlay: game window found after {} ms; installing the Direct3D 12 hooks in {} s",
        t0.elapsed().as_millis(),
        SETTLE.as_secs()
    );
    std::thread::sleep(SETTLE);
    match install(shared, toggle_name) {
        Ok(()) => tracing::info!(
            "overlay: hudhook DX12 hooks applied (the panel appears with the first presented frame; visible at start)"
        ),
        Err(e) => tracing::error!(
            "overlay: hook install FAILED: {e}; continuing without an overlay (hotkeys still work)"
        ),
    }
}

/// Apply the hudhook DX12 hooks. Panics inside hudhook (it uses `expect` while creating hooks) become an
/// `Err` here.
fn install(shared: Arc<Shared>, toggle_name: Option<String>) -> Result<(), String> {
    let app = OverlayApp::new(shared, toggle_name);
    let applied = catch_unwind(AssertUnwindSafe(|| {
        Hudhook::builder()
            .with::<ImguiDx12Hooks>(app)
            .build()
            .apply()
            .map_err(|e| format!("{e:?}"))
    }));
    match applied {
        Ok(r) => r,
        Err(_) => Err("hudhook panicked while creating the hooks".to_string()),
    }
}

fn wait_for_game_window() -> bool {
    let start = Instant::now();
    while start.elapsed() < WINDOW_WAIT {
        if game_window_exists() {
            return true;
        }
        std::thread::sleep(WINDOW_POLL);
    }
    false
}

/// True if this process has a visible top-level window that is big enough to be the game window.
fn game_window_exists() -> bool {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the address of the `bool` local in `game_window_exists`, alive for the whole
        // (synchronous) EnumWindows call and used from this thread only.
        let found = unsafe { &mut *(lparam.0 as *mut bool) };
        let mut pid = 0u32;
        // SAFETY: `pid` is a valid out-pointer; `hwnd` comes from EnumWindows.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        // SAFETY: plain Win32 queries on a window handle supplied by EnumWindows.
        if pid == unsafe { GetCurrentProcessId() } && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            let mut rc = RECT::default();
            // SAFETY: `rc` is a valid out-pointer.
            if unsafe { GetClientRect(hwnd, &mut rc) }.is_ok()
                && rc.right - rc.left >= MIN_WINDOW.0
                && rc.bottom - rc.top >= MIN_WINDOW.1
            {
                *found = true;
                return BOOL(0); // stop enumerating
            }
        }
        BOOL(1)
    }
    let mut found = false;
    // SAFETY: the callback only dereferences the pointer passed here, which outlives the call. EnumWindows
    // reports an error when the callback stops it early; that is expected and ignored.
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut found as *mut bool as isize)) };
    found
}

/// True if the OS cursor is currently showing (so ImGui need not draw its own).
fn os_cursor_showing() -> bool {
    let mut ci = CURSORINFO {
        cbSize: size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `ci` is a valid local with `cbSize` set, as GetCursorInfo requires.
    if unsafe { GetCursorInfo(&mut ci) }.is_err() {
        return false;
    }
    ci.flags.0 & CURSOR_SHOWING.0 != 0
}

/// The ImGui render loop. Lives inside hudhook's pipeline and runs on the game's render thread.
struct OverlayApp {
    shared: Arc<Shared>,
    toggle_name: Option<String>,
    visible: bool,
    /// Set after a panic while drawing: the overlay then stops drawing instead of risking the game.
    broken: bool,
    snap: Snapshot,
    log_seq: u64,
    log_lines: Vec<String>,
}

impl OverlayApp {
    fn new(shared: Arc<Shared>, toggle_name: Option<String>) -> Self {
        OverlayApp {
            shared,
            toggle_name,
            visible: true,
            broken: false,
            snap: Snapshot::default(),
            log_seq: u64::MAX,
            log_lines: Vec::new(),
        }
    }

    fn send(&self, c: Command) {
        if !self.shared.push(c) {
            tracing::warn!("overlay: command queue full; dropped {c:?}");
        }
    }

    fn draw(&mut self, ui: &Ui) {
        self.snap = self.shared.snapshot();
        let seq = logring::seq();
        if seq != self.log_seq {
            let (s, lines) = logring::snapshot();
            self.log_seq = s;
            self.log_lines = lines;
        }
        // "###" keeps the window identity stable while the visible title carries the version.
        let title = format!(
            "nrpvp {} - debug overlay###nrpvp_overlay",
            env!("CARGO_PKG_VERSION")
        );
        ui.window(title)
            .size([560.0, 660.0], Condition::FirstUseEver)
            .position([16.0, 16.0], Condition::FirstUseEver)
            .bg_alpha(0.88)
            .build(|| {
                self.status_block(ui);
                self.controls(ui);
                self.log_tail(ui);
            });
    }

    fn status_block(&self, ui: &Ui) {
        let s = &self.snap;
        ui.text_colored(
            GREY,
            match &self.toggle_name {
                Some(k) => format!("{k}: hide/show overlay"),
                None => "overlay toggle key is unbound".to_string(),
            },
        );
        ui.separator();
        if s.wcm_instance == 0 {
            ui.text_colored(AMBER, "WorldChrMan: not live");
        } else {
            ui.text(format!(
                "WorldChrMan: live, instance 0x{:X}",
                s.wcm_instance
            ));
        }
        match s.local_player {
            Some(p) => ui.text(format!("Local player: PlayerIns 0x{p:X}")),
            None => {
                ui.text_colored(AMBER, "Local player: not resolved");
                if !s.vtables_known {
                    ui.text_colored(GREY, "  PlayerIns vtables unknown (RTTI not recovered yet)");
                } else if !s.note.is_empty() {
                    ui.text_wrapped(format!("  {}", s.note));
                }
            }
        }
        match s.team_type {
            Some(t) => ui.text(format!("Local team_type (live): {t} ({})", team_name(t))),
            None => ui.text("Local team_type (live): ?"),
        }
        match s.original {
            Some(o) => ui.text(format!("Original (captured): {o} ({})", team_name(o))),
            None => ui.text("Original (captured): none yet (write a value first)"),
        }
        ui.text(format!(
            "Selected candidate: {} ({})",
            s.selected,
            team_name(s.selected)
        ));
        if s.persist {
            ui.text_colored(GREEN, "Persist: ON");
        } else {
            ui.text("Persist: off");
        }
        ui.same_line();
        ui.text(format!(
            "   reverts in last 1 s: {}   total: {}",
            s.reverts_last_sec, s.reverts_total
        ));
        if s.busy {
            ui.text_colored(AMBER, "Discovery/histogram: BUSY");
        } else {
            ui.text("Discovery/histogram: idle");
        }
    }

    fn controls(&self, ui: &Ui) {
        let s = &self.snap;
        ui.separator();
        ui.text("Team candidates (click = select and write to the local player)");
        let can_write = s.local_player.is_some();
        {
            let _off = ui.begin_disabled(!can_write);
            for (i, &v) in s.candidates.iter().enumerate() {
                if i % BUTTONS_PER_ROW != 0 {
                    ui.same_line();
                }
                let label = format!("{v} {}##cand{i}", team_name(v));
                let tint =
                    (v == s.selected).then(|| ui.push_style_color(StyleColor::Button, GREEN));
                if ui.button_with_size(label, [128.0, 0.0]) {
                    self.send(Command::Set(v));
                }
                drop(tint);
            }
        }
        ui.spacing();
        {
            let _off = ui.begin_disabled(!can_write || s.original.is_none());
            if ui.button("Restore original") {
                self.send(Command::Restore);
            }
        }
        ui.same_line();
        let mut persist = s.persist;
        if ui.checkbox("Persist (re-apply selected value every tick)", &mut persist) {
            self.send(Command::TogglePersist);
        }
        {
            let _off = ui.begin_disabled(s.busy || s.wcm_instance == 0);
            if ui.button("Discovery") {
                self.send(Command::RunDiscovery);
            }
            ui.same_line();
            if ui.button("Histogram") {
                self.send(Command::RunHistogram);
            }
        }
        if s.local_player.is_none() {
            ui.text_colored(
                GREY,
                "Candidate and Restore buttons need a resolved local player.",
            );
        }
    }

    fn log_tail(&self, ui: &Ui) {
        ui.separator();
        ui.text(format!(
            "Log tail ({} lines kept; scroll up to pause auto-scroll)",
            logring::RING_CAPACITY
        ));
        ui.child_window("##nrpvp_log")
            .size([0.0, 0.0])
            .border(true)
            .horizontal_scrollbar(true)
            .build(|| {
                // Stick to the newest line unless the user scrolled up.
                let at_bottom = ui.scroll_y() >= ui.scroll_max_y() - 2.0;
                for l in &self.log_lines {
                    if l.contains(" WARN ") {
                        ui.text_colored(AMBER, l);
                    } else if l.contains("ERROR") {
                        ui.text_colored(RED, l);
                    } else {
                        ui.text(l);
                    }
                }
                if at_bottom {
                    ui.set_scroll_here_y_with_ratio(1.0);
                }
            });
    }
}

impl ImguiRenderLoop for OverlayApp {
    fn initialize<'a>(&'a mut self, ctx: &mut Context, _render_context: &'a mut dyn RenderContext) {
        // Never write imgui.ini / imgui_log.txt into the game directory.
        ctx.set_ini_filename(None::<std::path::PathBuf>);
        ctx.set_log_filename(None::<std::path::PathBuf>);
        // A toggle press from before the first frame must not hide the overlay.
        let _ = hotkey::take(Action::ToggleOverlay);
        tracing::info!("overlay: ImGui context initialised (first usable Present)");
    }

    fn before_render<'a>(
        &'a mut self,
        ctx: &mut Context,
        _render_context: &'a mut dyn RenderContext,
    ) {
        if hotkey::take(Action::ToggleOverlay) {
            self.visible = !self.visible;
            tracing::info!("overlay: {}", if self.visible { "shown" } else { "hidden" });
        }
        // Draw ImGui's own cursor only while the panel is up and the game shows no OS cursor.
        ctx.io_mut().mouse_draw_cursor = self.visible && !os_cursor_showing();
    }

    fn render(&mut self, ui: &mut Ui) {
        if !self.visible || self.broken {
            return;
        }
        // A panic must not unwind into the game's Present call (that would abort the process).
        if catch_unwind(AssertUnwindSafe(|| self.draw(ui))).is_err() {
            self.broken = true;
            tracing::error!("overlay: panic while drawing; the overlay is disabled until restart");
        }
    }

    fn message_filter(&self, io: &Io) -> MessageFilter {
        let mut f = MessageFilter::empty();
        if self.visible && !self.broken {
            if io.want_capture_mouse {
                f |= MessageFilter::InputMouse | MessageFilter::InputRaw;
            }
            if io.want_text_input {
                f |= MessageFilter::InputKeyboard;
            }
        }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logring::RingMakeWriter;
    use std::io::Write as _;
    use tracing_subscriber::fmt::MakeWriter as _;

    fn snap() -> Snapshot {
        Snapshot {
            wcm_instance: 0x1_0000,
            vtables_known: true,
            local_player: Some(0x2_0000),
            team_type: Some(2),
            original: Some(2),
            selected: 29,
            candidates: vec![6, 29, 3, 16, 13, 2, 1],
            persist: true,
            reverts_last_sec: 1,
            reverts_total: 4,
            busy: false,
            note: String::new(),
        }
    }

    /// Runs the real UI code for a few headless frames (no D3D, no window): a populated snapshot, a busy
    /// worker, no local player, an empty candidate list, a long log. Also checks the input filter. ImGui
    /// allows one live context per process, so everything that needs one lives in this single test.
    #[test]
    fn panel_draws_headless_and_the_filter_follows_imgui() {
        let shared = Shared::new();
        let mut app = OverlayApp::new(Arc::clone(&shared), Some("Insert".to_string()));
        let mut ctx = Context::create();
        ctx.set_ini_filename(None::<std::path::PathBuf>);
        ctx.io_mut().display_size = [1280.0, 720.0];
        let _ = ctx.fonts().build_rgba32_texture();

        let long = "x".repeat(400);
        let cases = [
            snap(),
            Snapshot {
                busy: true,
                persist: false,
                ..snap()
            },
            Snapshot {
                local_player: None,
                team_type: None,
                original: None,
                vtables_known: false,
                note: "ambiguous".to_string(),
                ..snap()
            },
            Snapshot {
                wcm_instance: 0,
                candidates: Vec::new(),
                ..snap()
            },
            Snapshot::default(),
        ];
        for (i, s) in cases.iter().enumerate() {
            shared.publish(s.clone());
            for k in 0..20 {
                let mut w = RingMakeWriter.make_writer();
                let level = ["INFO", "WARN", "ERROR"][k % 3];
                let _ = writeln!(w, "  0.{k:03}s {level} headless frame {i} {long}");
            }
            ctx.io_mut().update_delta_time(Duration::from_millis(16));
            let ui = ctx.frame();
            app.draw(ui);
            let draw = ctx.render();
            assert!(draw.draw_lists_count() > 0, "frame {i} drew nothing");
            assert_eq!(app.snap, *s, "the panel shows the published snapshot");
            assert!(!app.log_lines.is_empty(), "the log tail is populated");
        }

        // The mouse filter: only while visible and only when ImGui wants the mouse.
        ctx.io_mut().want_capture_mouse = false;
        ctx.io_mut().want_text_input = false;
        assert_eq!(app.message_filter(ctx.io()).bits(), 0);
        ctx.io_mut().want_capture_mouse = true;
        assert_eq!(
            app.message_filter(ctx.io()).bits(),
            (MessageFilter::InputMouse | MessageFilter::InputRaw).bits()
        );
        app.visible = false;
        assert_eq!(app.message_filter(ctx.io()).bits(), 0);
        app.visible = true;
        app.broken = true;
        assert_eq!(app.message_filter(ctx.io()).bits(), 0);
    }

    #[test]
    fn game_window_search_does_not_panic() {
        // No game window in the test process; just exercise the EnumWindows callback.
        let _ = game_window_exists();
        let _ = os_cursor_showing();
    }

    /// Installs the real hudhook DX12 hooks into the TEST process (creates a dummy D3D12 device and swap
    /// chain, MinHook-hooks the DXGI/D3D12 vtable functions). Manual check that the programmatic API
    /// works on this machine: `cargo test --release -- --ignored hudhook_install_smoke`.
    #[test]
    #[ignore]
    fn hudhook_install_smoke() {
        install(Shared::new(), None).expect("hooks apply");
    }
}
