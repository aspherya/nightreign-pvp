//! nrpvp_launcher.exe: the one file a player double-clicks in the game folder. The work is in the library (lib.rs).
#![windows_subsystem = "windows"]

const TITLE: &str = "Nightreign PvP";

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(
        hwnd: *mut core::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        kind: u32,
    ) -> i32;
}

fn message(text: &str) {
    let wide = |s: &str| {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (t, c) = (wide(text), wide(TITLE));
    // SAFETY: both buffers are valid NUL-terminated UTF-16 strings that outlive the call.
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), 0x10) };
}

fn main() {
    let game = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
    let result = match game {
        Some(game) => nrpvp_launch::launch(&game),
        None => Err("Could not find my own folder.".to_string()),
    };
    if let Err(why) = result {
        message(&why);
    }
}
