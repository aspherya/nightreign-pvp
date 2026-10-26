//! RW-1 (both builds): let a defeated, held player open the Esc menu, and give the spectator pad buttons. Game 1.03.3 / exe 1.3.3.0; the
//! reading is in MEMORY_MAP §19.
//!
//! The game turns the menu key (Esc or the pad's Start) into a request byte: the per-frame input block calls the predicate `exe+0xF704E0`,
//! and when it says yes sets `ctrl+0x179 = 1` (`ctrl = [CSMenuMan+0xA0]`); the menu update's consumer (`exe+0x84E790`, runs every frame,
//! dead or alive) then builds the pause menu (`CS::MainDialog`). The predicate refuses while LOCAL's HP is 0 or less (`exe+0xF70645`), so a
//! held dead player never gets the request. [`tick`] sets the byte itself on a menu-key edge while [`set_active`] is on, after the
//! predicate's own cheap checks (`CSMenuMan+0x48 & !0x100 == 0`, `CSMenuMan+0x7A8 == 0`, so the key that closes the menu does not reopen
//! it). Nothing is patched.
//!
//! The pad: inside the game process XInput reports no pad (Steam Input hands the pad to the game another way; log 2026-10-25), so the
//! game's own input is read: the `FD4PadManager` (slot `exe+0x4707D48`) holds the object behind the in-game `CSPad`,
//! whose frame bit array (`[[[pad+0x10]+8]+0x50]`, MEMORY_MAP §19 "The game's own input") says which virtual buttons are down. No
//! breakpoint (the four hardware slots are shared). The main loop reads it every tick (50 ms). Console `padpoll` lists the set bits, which
//! is how the button bits below are measured. Log lines start with `menugate`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::mem;

/// `CSMenuMan` singleton slot, the menu control pointer inside it, the request byte inside that, and the two predicate checks.
const RVA_MENU_MAN: usize = 0x3C1_49D8;
const MAN_CTRL: usize = 0xA0;
const CTRL_OPEN_REQUEST: usize = 0x179;
const MAN_BUSY: usize = 0x48;
const MAN_BUSY2: usize = 0x7A8;

/// `FD4PadManager` singleton slot.
const RVA_PAD_MANAGER: usize = 0x470_7D48;

/// Virtual input bits of the pad's Start, d-pad right (next watched player) and d-pad left (previous). Each button shows up as a group
/// of bits (one per device mapping): it is down when any bit of its group is set. `[VERIFIED: padpoll while holding each button,
/// Nucleus-free one-PC run 2026-10-25, a Steam Input virtual Xbox pad]`; the numbers depend on the pad layout, MEMORY_MAP §19.
const BITS: [&[u16]; 3] = [
    &[273, 417],
    &[353, 396, 209, 278, 252],
    &[393, 206, 279, 249, 350],
];

static BASE: AtomicUsize = AtomicUsize::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static WAS_DOWN: AtomicBool = AtomicBool::new(false);
static WAS_PAD: [AtomicBool; 3] = [const { AtomicBool::new(false) }; 3];
static SPEC_NEXT: AtomicBool = AtomicBool::new(false);
static SPEC_PREV: AtomicBool = AtomicBool::new(false);

/// The night match turns the menu key on (a defeated player whose body is held) or off.
pub(crate) fn set_active(on: bool) {
    if ACTIVE.swap(on, Ordering::Relaxed) != on {
        tracing::info!(
            "menugate: the menu key {}",
            if on {
                "opens the pause menu for LOCAL"
            } else {
                "left to the game"
            }
        );
    }
}

/// The d-pad's next / previous spectated player went down since the last call (edges seen by [`tick`]).
pub(crate) fn pad_spectate_edges() -> (bool, bool) {
    (
        SPEC_NEXT.swap(false, Ordering::Relaxed),
        SPEC_PREV.swap(false, Ordering::Relaxed),
    )
}

/// The keyboard's Esc went down since the last call (an edge; the "pressed since last call" bit catches taps shorter than a tick).
fn esc_edge() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
    // SAFETY: GetAsyncKeyState only reads the keyboard state.
    let st = unsafe { GetAsyncKeyState(i32::from(VK_ESCAPE.0)) };
    let down = st < 0;
    let was = WAS_DOWN.swap(down, Ordering::Relaxed);
    (down && !was) || (st & 1 != 0 && !down)
}

/// The game's own menu (pause menu, a dialog, the map...) is open: `CSMenuMan+0x48 != 0` (bit 0x100, which the menu predicate ignores,
/// is most likely the map tab). The spectator's switching is off meanwhile, so clicking in the menu does not change the watched player.
pub(crate) fn menu_open(base: usize) -> bool {
    mem::read::<usize>(base + RVA_MENU_MAN)
        .filter(|m| *m != 0)
        .and_then(|m| mem::read::<u32>(m + MAN_BUSY))
        .is_some_and(|b| b != 0)
}

// ---- reading the game's input (MEMORY_MAP §19, "The game's own input") ----

/// The frame's virtual-input words: `[[[mgr+0x18]+8]+0x50]`, count at `+0x48`. `[mgr+0x18]` is the same object as `[CSPad+0x10]`, which a
/// heap `CSPad` (vtable `CSInGamePad_UserInput1`) also points to (peeked 2026-10-25: the pad object itself is not linked from the manager's
/// maps in the Elden Ring layout, this field is).
fn input_words(base: usize) -> Option<(usize, usize)> {
    let mgr = mem::read::<usize>(base + RVA_PAD_MANAGER).filter(|m| *m != 0)?;
    let a = mem::read::<usize>(mgr + 0x18).filter(|p| *p != 0)?;
    let b = mem::read::<usize>(a + 8).filter(|p| *p != 0)?;
    let words = mem::read::<usize>(b + 0x50).filter(|p| *p != 0)?;
    let n = mem::read::<u32>(b + 0x48)? as usize;
    Some((words, n.min(64)))
}

fn bit(words: (usize, usize), v: u16) -> bool {
    words.1 > usize::from(v / 32)
        && mem::read::<u32>(words.0 + 4 * usize::from(v / 32))
            .is_some_and(|x| x & (1 << (v % 32)) != 0)
}

/// A button's group has a set bit.
fn group_down(words: (usize, usize), button: usize) -> bool {
    BITS[button].iter().any(|v| bit(words, *v))
}

/// Console `padpoll`: every virtual input bit that is set right now (hold a button, call it, read which bit appeared).
pub(crate) fn describe() -> String {
    let base = BASE.load(Ordering::Relaxed);
    let Some(words) = input_words(base) else {
        return "padpoll: the pad manager's input words are unreadable".into();
    };
    let set: Vec<String> = (0..words.1 as u16 * 32)
        .filter(|v| bit(words, *v))
        .map(|v| v.to_string())
        .collect();
    format!(
        "padpoll: {} words, set virtual bits [{}]; start down {}, next down {}, previous down {}",
        words.1,
        set.join(" "),
        group_down(words, 0),
        group_down(words, 1),
        group_down(words, 2)
    )
}

/// Main loop, every tick (both builds): read the pad's bits, act on a menu-key edge.
pub(crate) fn tick(local: Option<usize>, base: usize) {
    BASE.store(base, Ordering::Relaxed);
    if !ACTIVE.load(Ordering::Relaxed) || local.is_none() {
        return;
    }
    let mut pad_edge = false;
    if let Some(words) = input_words(base) {
        for (i, was) in WAS_PAD.iter().enumerate() {
            let down = group_down(words, i);
            let edge = down && !was.swap(true, Ordering::Relaxed);
            if !down {
                was.store(false, Ordering::Relaxed);
            }
            match (i, edge) {
                (0, true) => pad_edge = true,
                (1, true) => SPEC_NEXT.store(true, Ordering::Relaxed),
                (2, true) => SPEC_PREV.store(true, Ordering::Relaxed),
                _ => {}
            }
        }
    }
    let key_edge = esc_edge();
    if !pad_edge && !key_edge {
        return;
    }
    let src = if key_edge { "Esc" } else { "pad Start" };
    let Some(man) = mem::read::<usize>(base + RVA_MENU_MAN).filter(|m| *m != 0) else {
        return;
    };
    let busy = mem::read::<u32>(man + MAN_BUSY).unwrap_or(1) & !0x100;
    let busy2 = mem::read::<u32>(man + MAN_BUSY2).unwrap_or(1);
    if busy != 0 || busy2 != 0 {
        tracing::info!(
            "menugate: {src} pressed, menu busy (+0x48 = 0x{busy:X}, +0x7A8 = 0x{busy2:X}): left to the game"
        );
        return;
    }
    let Some(ctrl) = mem::read::<usize>(man + MAN_CTRL).filter(|c| *c != 0) else {
        return;
    };
    match mem::write_u8(ctrl + CTRL_OPEN_REQUEST, 1) {
        Ok(()) => tracing::info!(
            "menugate: {src} pressed while defeated: asked the game to open the pause menu"
        ),
        Err(e) => tracing::warn!("menugate: could not write the open request: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_offsets() {
        assert_eq!(RVA_MENU_MAN, 0x3C149D8);
        assert_eq!(CTRL_OPEN_REQUEST, 0x179);
        assert_eq!(RVA_PAD_MANAGER, 0x4707D48);
    }

    #[test]
    fn activation_is_a_plain_switch() {
        set_active(true);
        assert!(ACTIVE.load(Ordering::Relaxed));
        set_active(false);
        assert!(!ACTIVE.load(Ordering::Relaxed));
    }

    #[test]
    fn a_button_is_never_down_without_input_words() {
        assert!(!group_down((0, 0), 0));
    }
}
