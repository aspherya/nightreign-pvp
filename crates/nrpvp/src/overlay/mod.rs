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
use hudhook::imgui::{Condition, Context, Io, StyleColor, Ui, WindowFlags};
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
use shared::{
    badge_lines, short_id, team_name, Command, PlayerRow, PvpveState, Relation, Role, Shared,
    Snapshot,
};

/// How long to wait for the game window before giving up on the overlay.
const WINDOW_WAIT: Duration = Duration::from_secs(600);
const WINDOW_POLL: Duration = Duration::from_millis(500);
/// A top-level window smaller than this is not the game's main window.
const MIN_WINDOW: (i32, i32) = (320, 240);
/// Candidate buttons per row.
const BUTTONS_PER_ROW: usize = 4;

/// Gap between the badge and the screen corner, in pixels.
const BADGE_MARGIN: f32 = 16.0;

const GREEN: [f32; 4] = [0.20, 0.55, 0.25, 1.0];
const GREEN_TEXT: [f32; 4] = [0.45, 0.90, 0.50, 1.0];
const RED: [f32; 4] = [0.95, 0.35, 0.30, 1.0];
const AMBER: [f32; 4] = [0.95, 0.75, 0.25, 1.0];
const GREY: [f32; 4] = [0.60, 0.60, 0.60, 1.0];

/// Start the install thread. Returns immediately; never fails the caller.
pub(crate) fn spawn(
    shared: Arc<Shared>,
    toggle_name: Option<String>,
    settle: Duration,
    player: bool,
) {
    let r = std::thread::Builder::new()
        .name("nrpvp-overlay".into())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| run(shared, toggle_name, settle, player)));
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

fn run(shared: Arc<Shared>, toggle_name: Option<String>, settle: Duration, player: bool) {
    let t0 = Instant::now();
    if !wait_for_game_window() {
        tracing::warn!(
            "overlay: no game window after {} s; continuing without an overlay (hotkeys still work)",
            WINDOW_WAIT.as_secs()
        );
        return;
    }
    tracing::info!(
        "overlay: game window found after {} ms; installing the Direct3D 12 hooks in {} ms (overlay_install_delay_ms)",
        t0.elapsed().as_millis(),
        settle.as_millis()
    );
    std::thread::sleep(settle);
    match install(shared, toggle_name, player) {
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
fn install(shared: Arc<Shared>, toggle_name: Option<String>, player: bool) -> Result<(), String> {
    let app = OverlayApp::new(shared, toggle_name, player);
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
    /// T-030: player build. Draws only the read-only badge; never captures mouse or keyboard; cannot be hidden.
    player: bool,
    visible: bool,
    /// Set after a panic while drawing: the overlay then stops drawing instead of risking the game.
    broken: bool,
    snap: Snapshot,
    log_seq: u64,
    log_lines: Vec<String>,
    /// T-015: the MARK text box content.
    mark_text: String,
}

impl OverlayApp {
    fn new(shared: Arc<Shared>, toggle_name: Option<String>, player: bool) -> Self {
        OverlayApp {
            shared,
            toggle_name,
            player,
            visible: true,
            broken: false,
            snap: Snapshot::default(),
            log_seq: u64::MAX,
            log_lines: Vec::new(),
            mark_text: String::new(),
        }
    }

    /// T-030: the player badge, a small non-interactive box in the top-right corner (no title bar, no input, no
    /// saved settings, semi-transparent).
    fn draw_badge(&mut self, ui: &Ui) {
        self.snap = self.shared.snapshot();
        let (l1, l2) = badge_lines(&self.snap);
        let screen = ui.io().display_size;
        ui.window("##nrpvp_badge")
            .flags(
                WindowFlags::NO_DECORATION
                    | WindowFlags::NO_INPUTS
                    | WindowFlags::NO_NAV
                    | WindowFlags::NO_MOVE
                    | WindowFlags::NO_SAVED_SETTINGS
                    | WindowFlags::NO_FOCUS_ON_APPEARING
                    | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS
                    | WindowFlags::ALWAYS_AUTO_RESIZE,
            )
            .position([screen[0] - BADGE_MARGIN, BADGE_MARGIN], Condition::Always)
            .position_pivot([1.0, 0.0])
            .bg_alpha(0.45)
            .build(|| {
                ui.text_colored(AMBER, l1);
                ui.text(l2);
            });
    }

    fn send(&self, c: Command) {
        let shown = format!("{c:?}");
        if !self.shared.push(c) {
            tracing::warn!("overlay: command queue full; dropped {shown}");
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
            .size([640.0, 780.0], Condition::FirstUseEver)
            .position([16.0, 16.0], Condition::FirstUseEver)
            .bg_alpha(0.88)
            .build(|| {
                self.status_block(ui);
                self.controls(ui);
                self.mm_block(ui);
                self.players_block(ui);
                self.probes_block(ui);
                self.mark_block(ui);
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
            Some(p) => {
                ui.text(format!("Local player: PlayerIns 0x{p:X}"));
                if !s.local_basis.is_empty() {
                    ui.same_line();
                    ui.text_colored(GREY, format!("({})", s.local_basis));
                }
            }
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
        match s.pvpve {
            PvpveState::On => ui.text_colored(GREEN_TEXT, "PvPvE: ON"),
            PvpveState::Paused => ui.text_colored(AMBER, "PvPvE: PAUSED"),
            PvpveState::Off => ui.text_colored(GREY, "PvPvE: OFF"),
        }
        ui.same_line();
        ui.text(format!(
            "   team value {} ({})   automatic writes this session: {}",
            s.pvpve_team,
            team_name(s.pvpve_team),
            s.pvpve_writes
        ));
        ui.text(format!(
            "Scheme: {}   team_size: {}",
            if s.pvpve_scheme.is_empty() {
                "?"
            } else {
                &s.pvpve_scheme
            },
            s.team_size
        ));
        if !s.roster_warning.is_empty() {
            ui.text_colored(RED, format!("Rosters: {}", s.roster_warning));
        }
        ui.text_colored(
            GREY,
            format!(
                "Downed-rival value (F8 cycles): {} - INERT, the downed marker is unknown",
                if s.downed_rival_team == 0 {
                    "off".to_string()
                } else {
                    format!(
                        "team {} ({})",
                        s.downed_rival_team,
                        team_name(s.downed_rival_team)
                    )
                }
            ),
        );
        ui.text_colored(
            AMBER,
            format!(
                "Snap-back test (F9 select, F7 save, F6 write): {}, saved: {}",
                s.pos_selected,
                if s.pos_saved { "yes" } else { "no" }
            ),
        );
        if !s.revive_text.is_empty() {
            ui.text_colored(AMBER, &s.revive_text);
            for l in &s.combat_lines {
                ui.text(l);
            }
        }
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
        {
            let _off = ui.begin_disabled(s.pvpve == PvpveState::Off);
            let mut on = s.pvpve == PvpveState::On;
            if ui.checkbox("PvPvE mode (rewrite team 2 to the pvpve team)", &mut on) {
                self.send(Command::SetPvpve(on));
            }
        }
        if s.pvpve == PvpveState::Off {
            ui.same_line();
            ui.text_colored(GREY, "off: set pvpve = true in nrpvp.toml");
        }
        ui.text("Team candidates (click = select and write to the local player; per-player buttons are below)");
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

    /// MM-2: the matchmaking queue line and buttons (only when `mm_queue = true`). The buttons send `Command`s; the status is the
    /// plain value the Steam worker thread published.
    fn mm_block(&self, ui: &Ui) {
        if !crate::mm::enabled() {
            return;
        }
        let st = crate::mm::status();
        ui.separator();
        let line = if st.line.is_empty() { "Queue: starting (waiting for Steam)".to_string() } else { st.line.clone() };
        let colour = if line.starts_with("MATCH READY") || line.starts_with("ALL PRESENT") {
            GREEN_TEXT
        } else if line.contains("REFUSED") || line.contains("failed") {
            RED
        } else if st.active {
            AMBER
        } else {
            GREY
        };
        ui.text_colored(colour, line);
        {
            let _off = ui.begin_disabled(st.active);
            if ui.button("Queue Solo") {
                self.send(Command::MmQueue(crate::mm::Mode::Solo));
            }
            ui.same_line();
            if ui.button("Queue Duo") {
                self.send(Command::MmQueue(crate::mm::Mode::Duo));
            }
            ui.same_line();
            if ui.button("Queue Squad") {
                self.send(Command::MmQueue(crate::mm::Mode::Squad));
            }
        }
        ui.same_line();
        {
            let _off = ui.begin_disabled(!st.active);
            if ui.button("Leave queue") {
                self.send(Command::MmLeave);
            }
        }
        // MM-3d (dev only, `mm_sc_create`): create a lobby the way the game does and see whether Seamless Co-op adopts it.
        if st.sc_create_ui {
            if ui.button("create SC lobby (test)") {
                self.send(Command::MmScCreate);
            }
            if !st.sc_create_line.is_empty() {
                let c = if st.sc_create_line.contains("ADOPTED") {
                    GREEN_TEXT
                } else if st.sc_create_line.contains("REFUSED") || st.sc_create_line.contains("failed") {
                    RED
                } else {
                    AMBER
                };
                ui.text_colored(c, &st.sc_create_line);
            }
        }
        // MM-3f (dev only, `mm_quiet_invite`): suppress the Steam invite window for 60 s, then press Matchmaking Settings > Invite Members.
        if st.quiet_ui {
            if ui.button("test quiet invite") {
                self.send(Command::MmQuietTest);
            }
            if !st.quiet_line.is_empty() {
                let c = if st.quiet_line.contains("SUPPRESSED") { GREEN_TEXT } else { AMBER };
                ui.text_colored(c, &st.quiet_line);
            }
        }
        // MM-3 (dev only, `mm_join`): the join methods, one attempt per press, usable once the host's lobby id is known.
        if st.join_ui {
            use crate::mm::join::JoinCmd;
            ui.text_colored(GREY, "Join methods (each press is logged as mm_join: method ...):");
            {
                let _off = ui.begin_disabled(!st.sc_known);
                if ui.button("(a) link") {
                    self.send(Command::MmJoin(JoinCmd::Link));
                }
                ui.same_line();
                if ui.button("(b) invite / re-invite") {
                    self.send(Command::MmJoin(JoinCmd::Invite));
                }
                // MM-3f: the direct JoinLobby gives Steam membership without the Seamless Co-op session (S9 5f): not offered with mm_quiet_invite.
                if !st.quiet_ui {
                    ui.same_line();
                    if ui.button("(c) direct JoinLobby") {
                        self.send(Command::MmJoin(JoinCmd::Direct));
                    }
                }
            }
            {
                let _off = ui.begin_disabled(!st.sc_known || !st.sc_owner);
                if ui.button("(d) unlock: Public") {
                    self.send(Command::MmJoin(JoinCmd::UnlockPublic));
                }
                ui.same_line();
                if ui.button("(d) unlock: Invisible") {
                    self.send(Command::MmJoin(JoinCmd::UnlockInvisible));
                }
                ui.same_line();
                if ui.button("relock: Private") {
                    self.send(Command::MmJoin(JoinCmd::RelockPrivate));
                }
            }
        }
    }

    /// T-015: every PlayerIns of the ChrSet with its own buttons. Any row can be written, local or remote.
    fn players_block(&self, ui: &Ui) {
        let s = &self.snap;
        ui.separator();
        ui.text(format!("Players in the ChrSet: {}", s.players.len()));
        if s.players.is_empty() {
            ui.text_colored(
                GREY,
                "  none (WorldChrMan not live, or the ChrSet is empty while loading)",
            );
            return;
        }
        for row in &s.players {
            self.player_row(ui, row, &s.candidates);
        }
    }

    fn player_row(&self, ui: &Ui, row: &PlayerRow, candidates: &[u8]) {
        let id = ui.push_id_usize(row.addr);
        ui.separator();
        let (who, colour) = match row.role {
            Role::Local => ("LOCAL".to_string(), GREEN_TEXT),
            Role::Remote => (format!("REMOTE[{}]", row.index), AMBER),
        };
        ui.text_colored(colour, &who);
        ui.same_line();
        let rel_text = row.relation.map_or("?", Relation::label);
        let rel_colour = match row.relation {
            Some(Relation::Rival) => RED,
            Some(_) => GREEN_TEXT,
            None => GREY,
        };
        ui.text_colored(
            rel_colour,
            format!(
                "id {} team {} {rel_text}",
                short_id(row.id),
                row.team_no.map_or("?".to_string(), |t| t.to_string())
            ),
        );
        ui.same_line();
        let team = row
            .team
            .map_or("?".to_string(), |t| format!("{t} ({})", team_name(t)));
        ui.text(format!(
            "entry[{}] 0x{:X}  team_type {team}  entry bytes +8/+9: {:02X} {:02X}",
            row.index, row.addr, row.state8, row.state9
        ));
        if row.role == Role::Local {
            ui.text_colored(GREY, format!("  decided by: {}", row.basis));
        }
        if row.original.is_some() || row.persist.is_some() {
            ui.text_colored(
                GREY,
                format!(
                    "  original {}   persist {}   reverts last 1 s {} total {}",
                    row.original.map_or("-".to_string(), |o| o.to_string()),
                    row.persist.map_or("off".to_string(), |p| p.to_string()),
                    row.reverts_last_sec,
                    row.reverts_total
                ),
            );
        }
        for (k, &v) in candidates.iter().enumerate() {
            if k > 0 {
                ui.same_line();
            }
            if ui.button_with_size(format!("{v}##c{k}"), [38.0, 0.0]) {
                self.send(Command::SetFor {
                    addr: row.addr,
                    value: v,
                });
            }
            if ui.is_item_hovered() {
                ui.tooltip_text(format!("write {v} ({}) to this player", team_name(v)));
            }
        }
        {
            let _off = ui.begin_disabled(row.original.is_none());
            if ui.button("Restore") {
                self.send(Command::RestoreFor { addr: row.addr });
            }
        }
        ui.same_line();
        let mut persist = row.persist.is_some();
        if ui.checkbox("Persist", &mut persist) {
            self.send(Command::PersistFor {
                addr: row.addr,
                on: persist,
            });
        }
        id.pop();
    }

    /// T-027: the read-only probe candidates (UNVERIFIED), to compare with the HUD.
    fn probes_block(&self, ui: &Ui) {
        ui.separator();
        ui.text_colored(AMBER, "Probes (UNVERIFIED)");
        if !self.snap.probes_enabled {
            ui.text_colored(GREY, "off: set probes = true in nrpvp.toml");
            return;
        }
        for l in &self.snap.probe_lines {
            ui.text(l);
        }
    }

    /// T-015: the MARK text box (Enter submits) and button. While the box is focused ImGui wants text input
    /// and `message_filter` keeps the keyboard away from the game.
    fn mark_block(&mut self, ui: &Ui) {
        ui.separator();
        ui.text(format!(
            "MARK note ({} logged; hotkey logs one without text)",
            self.snap.marks
        ));
        let entered = ui
            .input_text("##mark_text", &mut self.mark_text)
            .hint("what just happened? Enter = MARK")
            .enter_returns_true(true)
            .build();
        ui.same_line();
        let clicked = ui.button("MARK");
        if entered || clicked {
            let text = std::mem::take(&mut self.mark_text);
            self.send(Command::Mark(Some(text)));
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
        if self.player {
            // The badge cannot be hidden and ImGui never draws a cursor of its own.
            ctx.io_mut().mouse_draw_cursor = false;
            return;
        }
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
        let drawn = catch_unwind(AssertUnwindSafe(|| {
            if self.player {
                self.draw_badge(ui)
            } else {
                self.draw(ui)
            }
        }));
        if drawn.is_err() {
            self.broken = true;
            tracing::error!("overlay: panic while drawing; the overlay is disabled until restart");
        }
    }

    fn message_filter(&self, io: &Io) -> MessageFilter {
        // T-030: the player badge never blocks game input.
        if self.player {
            return MessageFilter::empty();
        }
        input_filter(
            self.visible,
            self.broken,
            io.want_capture_mouse,
            io.want_text_input,
        )
    }
}

/// Which input the game must not see. Mouse and raw input are blocked while the panel is up and ImGui
/// wants the mouse; the keyboard is blocked only while a text box is focused (`want_text_input`), so the
/// game keeps its keys the rest of the time (T-015).
fn input_filter(visible: bool, broken: bool, want_mouse: bool, want_text: bool) -> MessageFilter {
    let mut f = MessageFilter::empty();
    if visible && !broken {
        if want_mouse {
            f |= MessageFilter::InputMouse | MessageFilter::InputRaw;
        }
        if want_text {
            f |= MessageFilter::InputKeyboard;
        }
    }
    f
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
            players: vec![
                PlayerRow {
                    index: 1,
                    addr: 0x2_0000,
                    role: Role::Local,
                    basis: "only PlayerIns in the ChrSet".to_string(),
                    team: Some(2),
                    state8: 4,
                    state9: 4,
                    original: Some(2),
                    persist: Some(29),
                    reverts_last_sec: 0,
                    reverts_total: 2,
                    id: Some(0x6001_FFFF_FFFF),
                    team_no: Some(0),
                    relation: Some(Relation::Local),
                },
                PlayerRow {
                    index: 3,
                    addr: 0x3_0000,
                    role: Role::Remote,
                    basis: "not the local player".to_string(),
                    team: None,
                    state8: 4,
                    state9: 2,
                    original: None,
                    persist: None,
                    reverts_last_sec: 0,
                    reverts_total: 0,
                    id: Some(0x6002_FFFF_FFFF),
                    team_no: Some(1),
                    relation: Some(Relation::Rival),
                },
                PlayerRow {
                    index: 4,
                    addr: 0x4_0000,
                    role: Role::Remote,
                    basis: "UNKNOWN".to_string(),
                    team: Some(30),
                    state8: 0,
                    state9: 0,
                    original: Some(30),
                    persist: None,
                    reverts_last_sec: 0,
                    reverts_total: 0,
                    id: None,
                    team_no: None,
                    relation: None,
                },
            ],
            local_basis: "only PlayerIns in the ChrSet".to_string(),
            marks: 2,
            pvpve: PvpveState::On,
            pvpve_team: 29,
            pvpve_writes: 12,
            pvpve_scheme: "rivals".to_string(),
            team_size: 1,
            roster_warning: String::new(),
            downed_rival_team: 6,
            pos_selected: "PlayerIns+0x6D0 (2/6)".to_string(),
            pos_saved: true,
            combat_lines: vec!["LOCAL[0] 0x6001.. downs=1 hp=0 DOWNED".to_string()],
            revive_text: "Rival revive: redown, applied 0x".to_string(),
            probes_enabled: true,
            probe_lines: vec![
                "clock hint: 07:00:00 mult 1 [plausible]".to_string(),
                "flags: layout ok; ON: 7500 7501; 0/28 not readable".to_string(),
            ],
        }
    }

    /// Runs the real UI code for a few headless frames (no D3D, no window): a populated snapshot, a busy
    /// worker, no local player, an empty candidate list, a long log. Also checks the input filter. ImGui
    /// allows one live context per process, so everything that needs one lives in this single test.
    #[test]
    fn panel_draws_headless_and_the_filter_follows_imgui() {
        let shared = Shared::new();
        let mut app = OverlayApp::new(Arc::clone(&shared), Some("Insert".to_string()), false);
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
        // Keyboard: blocked from the game only while a text box is focused.
        ctx.io_mut().want_capture_mouse = false;
        ctx.io_mut().want_text_input = true;
        app.visible = true;
        app.broken = false;
        assert_eq!(
            app.message_filter(ctx.io()).bits(),
            MessageFilter::InputKeyboard.bits()
        );
        ctx.io_mut().want_capture_mouse = true;
        assert_eq!(
            app.message_filter(ctx.io()).bits(),
            (MessageFilter::InputMouse | MessageFilter::InputRaw | MessageFilter::InputKeyboard)
                .bits()
        );
        ctx.io_mut().want_text_input = false;
        ctx.io_mut().want_capture_mouse = true;
        app.visible = false;
        assert_eq!(app.message_filter(ctx.io()).bits(), 0);
        app.visible = true;
        app.broken = true;
        assert_eq!(app.message_filter(ctx.io()).bits(), 0);

        // T-030 player badge: draws, and never blocks input even if ImGui claims mouse and keyboard.
        let mut badge = OverlayApp::new(Arc::clone(&shared), None, true);
        shared.publish(snap());
        ctx.io_mut().want_capture_mouse = true;
        ctx.io_mut().want_text_input = true;
        assert_eq!(badge.message_filter(ctx.io()).bits(), 0);
        // An auto-resizing window only shows from its second frame.
        for _ in 0..3 {
            ctx.io_mut().update_delta_time(Duration::from_millis(16));
            let ui = ctx.frame();
            badge.draw_badge(ui);
            let _ = ctx.render();
        }
        ctx.io_mut().update_delta_time(Duration::from_millis(16));
        let ui = ctx.frame();
        badge.draw_badge(ui);
        let draw = ctx.render();
        assert!(draw.total_vtx_count > 0, "badge drew nothing");
        assert_eq!(badge.snap, snap());
    }

    #[test]
    fn keyboard_reaches_the_game_unless_a_text_box_is_focused() {
        let kb = MessageFilter::InputKeyboard.bits();
        let mouse = (MessageFilter::InputMouse | MessageFilter::InputRaw).bits();
        // (visible, broken, want_mouse, want_text) -> expected bits
        let cases = [
            (true, false, false, false, 0),
            (true, false, true, false, mouse),
            (true, false, false, true, kb),
            (true, false, true, true, mouse | kb),
            (false, false, true, true, 0),
            (true, true, true, true, 0),
        ];
        for (v, b, m, t, want) in cases {
            assert_eq!(input_filter(v, b, m, t).bits(), want, "{v} {b} {m} {t}");
        }
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
        install(Shared::new(), None, false).expect("hooks apply");
    }
}
