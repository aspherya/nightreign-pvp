//! R7-3: keyboard, mouse and pad edges the main loop turns into [`crate::game::world::World`] fields (F10 and the spectator's switch
//! buttons). Moved out of `game/mod.rs` unchanged.

use super::menugate;

/// RW-1: the spectator's switch inputs went down since the last call: next = left mouse button or the pad's d-pad right, previous = right
/// mouse button or the d-pad left (edges, so holding a button switches once). The pad is read from the game's own input
/// (`menugate::pad_spectate_edges`): XInput shows no pad inside the game.
pub(crate) fn spectate_switch_edges() -> (bool, bool) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    static WAS: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
    // SAFETY: GetAsyncKeyState only reads the keyboard / mouse state.
    let (next, prev) = unsafe {
        (
            GetAsyncKeyState(i32::from(VK_LBUTTON.0)) < 0,
            GetAsyncKeyState(i32::from(VK_RBUTTON.0)) < 0,
        )
    };
    let edge = |i: usize, down: bool| down && !WAS[i].swap(down, Ordering::Relaxed);
    let n = edge(0, next);
    let p = edge(1, prev);
    if !next {
        WAS[0].store(false, Ordering::Relaxed);
    }
    if !prev {
        WAS[1].store(false, Ordering::Relaxed);
    }
    let (pn, pp) = menugate::pad_spectate_edges();
    (n || pn, p || pp)
}

/// RW-1: the leave key (F10) went down since the last call (an edge, so holding it asks once).
pub(crate) fn leave_key_edge() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F10};
    static WAS_DOWN: AtomicBool = AtomicBool::new(false);
    // SAFETY: GetAsyncKeyState only reads the keyboard state.
    let down = unsafe { GetAsyncKeyState(i32::from(VK_F10.0)) } < 0;
    let edge = down && !WAS_DOWN.load(Ordering::Relaxed);
    WAS_DOWN.store(down, Ordering::Relaxed);
    edge
}
