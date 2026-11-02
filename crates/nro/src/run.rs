//! The scenario runner: stage → preflight → launch → steps → report.json → teardown. One normal game, or several emulated-Steam
//! instances (`[[instance]]`, MULTI-INSTANCE.md) launched one after another, each step acting on its `instance = N`.

use nro_core::logscan;
use nro_core::scenario::{Scenario, Since, Step};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::session::{self, Stage, State};

#[derive(Debug, Serialize)]
pub struct StepResult {
    pub step: String,
    pub ok: bool,
    pub detail: String,
    pub secs: f64,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub scenario: String,
    /// The run folder (several, joined by " | ", for a multi-instance run).
    pub run: String,
    /// PASS | FAIL | ABORTED | REFUSED
    pub verdict: String,
    pub steps: Vec<StepResult>,
    pub note: String,
}

/// The two buttons of the title's "Unable to restore current session due to lack of internet connectivity. Switch to online mode
/// to restore current session?" dialog (OK = online, CANCEL = offline; shown after a run was killed inside an expedition), as
/// fractions of the 16:9 game image: (x from, x to) of each button frame and the band of rows its top edge lies in (1.03.3,
/// measured on `title_slow.png` 2026-10-27, MULTI-INSTANCE §20).
const RESTORE_OK_X: (f32, f32) = (0.315, 0.455);
const RESTORE_CANCEL_X: (f32, f32) = (0.545, 0.685);
const RESTORE_TOP_Y: (f32, f32) = (0.555, 0.590);
/// The CANCEL button's centre (game-image fractions).
const RESTORE_CANCEL_AT: (f32, f32) = (0.613, 0.589);

/// The game image inside the client area: the game draws 16:9 and pads the rest black. Returns (x offset, width, y offset, height).
fn game_rect(w: usize, h: usize) -> (usize, usize, usize, usize) {
    let gw = (h * 16 / 9).min(w);
    let gh = (gw * 9 / 16).min(h);
    ((w - gw) / 2, gw, (h - gh) / 2, gh)
}

/// A button frame's top edge inside the row band `ys` over `xs` (game-image fractions): one row where at least 80 % of the pixels
/// are lit (luma >= 40: the unfocused CANCEL frame is dim, about 50-70) while the rows 4 px above and below are at least 80 % dark.
/// A bright background (a world, the sky) has no such thin line; text has no long unbroken run.
fn frame_line(w: usize, h: usize, rgb: &[u8], xs: (f32, f32), ys: (f32, f32)) -> bool {
    let (ox, gw, oy, gh) = game_rect(w, h);
    let (x0, x1) = (
        ox + (xs.0 * gw as f32) as usize,
        (ox + (xs.1 * gw as f32) as usize).min(w),
    );
    let (y0, y1) = (
        oy + (ys.0 * gh as f32) as usize,
        oy + (ys.1 * gh as f32) as usize,
    );
    let share = |y: usize, lit: bool| -> f32 {
        let n = (x0..x1)
            .filter(|&x| {
                let i = (y * w + x) * 3;
                let l = (rgb[i] as u32 + rgb[i + 1] as u32 + rgb[i + 2] as u32) / 3;
                (l >= 40) == lit
            })
            .count();
        n as f32 / (x1 - x0).max(1) as f32
    };
    (y0.max(4)..=y1.min(h.saturating_sub(5)))
        .any(|y| share(y, true) >= 0.8 && share(y - 4, false) >= 0.8 && share(y + 4, false) >= 0.8)
}

/// The restore-session dialog is on screen: both button frames' top edges are thin unbroken lines in their places.
pub fn is_restore_dialog(w: usize, h: usize, rgb: &[u8]) -> bool {
    frame_line(w, h, rgb, RESTORE_OK_X, RESTORE_TOP_Y)
        && frame_line(w, h, rgb, RESTORE_CANCEL_X, RESTORE_TOP_Y)
}

/// Read an RGB(A) 8-bit PNG as RGB.
fn read_rgb(p: &std::path::Path) -> Option<(usize, usize, Vec<u8>)> {
    let f = std::fs::File::open(p).ok()?;
    let mut r = png::Decoder::new(std::io::BufReader::new(f))
        .read_info()
        .ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgb = match info.color_type {
        png::ColorType::Rgb => buf[..w * h * 3].to_vec(),
        png::ColorType::Rgba => buf[..w * h * 4]
            .chunks(4)
            .flat_map(|c| [c[0], c[1], c[2]])
            .collect(),
        _ => return None,
    };
    Some((w, h, rgb))
}

/// Title screen → loaded world (the Roundtable): any key on the title, OK on the notice, Continue, then the load. Repeats until
/// the console says a world is loaded. The restore-session dialog (after a run killed in an expedition) asks to switch to ONLINE
/// mode with OK focused: it is detected on a screenshot before every key and answered with CANCEL (offline), never with ENTER.
pub fn title_to_world(st: &State, timeout: Duration) -> Result<String, String> {
    let t = Instant::now();
    let pid = st.live_pid().ok_or("the game is not running")?;
    let shot = st.dir().join("shots").join("title_check.png");
    let restore_dialog = || -> Result<Option<(usize, usize)>, String> {
        // A black picture (the game has not drawn its first frame yet) is no dialog; the key path below then reports the desk.
        if let Err(e) = crate::desk::request(
            serde_json::json!({"op": "shot", "pid": pid, "path": shot.display().to_string()}),
        ) {
            return if e.contains("black") {
                Ok(None)
            } else {
                Err(e)
            };
        }
        Ok(read_rgb(&shot).and_then(|(w, h, rgb)| is_restore_dialog(w, h, &rgb).then_some((w, h))))
    };
    let mut part = 0usize;
    while t.elapsed() < timeout {
        if let Ok(s) = session::cmd(st, "status") {
            if s.contains("world loaded") {
                return Ok(s);
            }
        }
        // The window of a game that was just launched does not exist for the first seconds: wait for it instead of failing.
        if crate::sys::main_window(pid).is_none() {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        if let Some((w, h)) = restore_dialog()? {
            let (ox, gw, oy, gh) = game_rect(w, h);
            let fx = (ox as f32 + RESTORE_CANCEL_AT.0 * gw as f32) / w as f32;
            let fy = (oy as f32 + RESTORE_CANCEL_AT.1 * gh as f32) / h as f32;
            st.log(
                "console.log",
                "title: restore-session dialog (switch to online?) -> CANCEL (offline)",
            );
            crate::desk::request(serde_json::json!({"op": "keys", "pid": pid,
                "seq": format!("click {fx:.3} {fy:.3}; wait 3s")}))?;
            continue;
        }
        // "Press any button" (any key), the "previous session did not end" notice's OK (the session before was killed, which is
        // how every orchestrated session ends), then Continue on the main menu. Mouse clicks: the menus ignore injected keys.
        // Positions are fractions of the client area (1.03.3 title layout at 16:9). One part per pass, so the dialog check above
        // runs before every key (the dialog appears only in reply to an input, and a part ends with a wait).
        const PARTS: [&str; 3] = [
            "ENTER; wait 3s",
            "click 0.5 0.66; wait 2s",
            "click 0.5 0.749; wait 3s",
        ];
        crate::desk::request(
            serde_json::json!({"op": "keys", "pid": pid, "seq": PARTS[part % PARTS.len()]}),
        )?;
        part += 1;
    }
    Err(format!("no world after {} s", timeout.as_secs()))
}

/// Roundtable → expedition (offline mode: Commence Solo starts at once), then off the start hawk. Position-independent like
/// [`invite_members`]: the mod's `tp` stands the player at [`TABLE_SPOT`] facing the table (the spawn spot and the Roundtable's look change
/// with the save and the Shifting Earth, so the old blind 5 s walk could run past the table), F opens the expedition menu (checked by
/// the menu word), then F2 / F1 / F (target) / F (Commence Solo Expedition); wait for the local team value 2 (expedition, MEMORY_MAP §12);
/// then Alt (disembark) and a few seconds to land.
///
/// With `god` (the default of a scenario step) the mod console `god 1` is sent last, so a wolf cannot kill the character while the
/// scenario runs; `hp0` turns it off again, and a scenario about a mob killing the character passes `god = false`.
pub fn to_expedition(st: &State, timeout: Duration, god: bool) -> Result<String, String> {
    let pid = st.live_pid().ok_or("the game is not running")?;
    let keys =
        |seq: &str| crate::desk::request(serde_json::json!({"op": "keys", "pid": pid, "seq": seq}));
    let s = session::cmd(st, "status")?;
    if !s.contains("team_type 2") {
        if !s.contains("team_type 1") {
            return Err(format!("not in the Roundtable: {s}"));
        }
        // The Number of Players setting is a ring of three values that the game keeps between runs (and a friend session or a
        // player can leave it on Duo or Trio): "Matchmaking failed" otherwise. Attempt 1 takes it as it is, each later attempt
        // dismisses the dialog and steps the ring once, so all three values are tried (Matchmaking Settings tab = F2, Target = F1).
        let t = Instant::now();
        let mut started = false;
        for attempt in 0..4 {
            // Back to a known state: dismiss the "Matchmaking failed" dialog of the previous attempt, close any menu, stand at the table.
            if attempt > 0 {
                // F is the game's "OK" ("F:OK Q:Close" in every menu); ENTER did not dismiss the dialog.
                keys("F; wait 1s")?;
            }
            for _ in 0..5 {
                if session::cmd(st, "pos")
                    .ok()
                    .and_then(|r| parse_pos(&r))
                    .and_then(|p| p.1)
                    == Some(0)
                {
                    break;
                }
                keys("Q; wait 1s")?;
            }
            // Stand at the table and stay there: for the first seconds after the world loads the game puts the player back at the
            // spawn spot, so `tp` is repeated until the position holds for 2 s (2026-11-01).
            let mut held = false;
            for _ in 0..15 {
                let r = session::cmd(
                    st,
                    &format!(
                        "tp {} {} {} {}",
                        TABLE_SPOT[0], TABLE_SPOT[1], TABLE_SPOT[2], TABLE_YAW
                    ),
                )?;
                if !r.starts_with("tp:") {
                    return Err(format!("tp refused: {r}"));
                }
                std::thread::sleep(Duration::from_millis(2000));
                let at = session::cmd(st, "pos")
                    .ok()
                    .and_then(|r| parse_pos(&r))
                    .map(|p| p.0);
                if at.is_some_and(|a| dist(a, TABLE_SPOT) <= SPOT_TOLERANCE) {
                    held = true;
                    break;
                }
            }
            if !held {
                return Err("the player does not stay at the table spot".into());
            }
            // F opens the expedition menu; the first key into a window that was in the back is sometimes lost, so retry.
            for _ in 0..4 {
                keys("F; wait 2500ms")?;
                let menu = session::cmd(st, "pos")
                    .ok()
                    .and_then(|r| parse_pos(&r))
                    .and_then(|p| p.1);
                if menu == Some(MENU_EXPEDITION) {
                    break;
                }
            }
            let step = if attempt == 0 {
                "F2; wait 1s; F1; wait 1s; F; wait 2s; F; wait 3s"
            } else {
                "F2; wait 1s; RIGHT; wait 1s; F1; wait 1s; F; wait 2s; F; wait 3s"
            };
            keys(step)?;
            // The expedition takes 30 s or more to load (seen 2026-11-01): wait long enough that a started load is not taken for a failure.
            let try_until = Instant::now() + Duration::from_secs(70);
            while Instant::now() < try_until {
                if session::cmd(st, "status").is_ok_and(|s| s.contains("team_type 2")) {
                    started = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            if started || t.elapsed() > timeout {
                break;
            }
        }
        if !started {
            return Err("the expedition did not start".into());
        }
        std::thread::sleep(Duration::from_secs(6));
        keys("ALT; wait 10s")?;
    }
    let status = session::cmd(st, "status")?;
    if god {
        session::cmd(st, "god 1")?;
    }
    Ok(status)
}

/// Where the host stands to get the table's "F: Commence expedition" prompt, and the heading that faces the table (Roundtable, world
/// metres and degrees as the mod's `pos` prints them; game 1.03.3, measured 2026-10-27 on instance 1, MULTI-INSTANCE §19). The prompt
/// needs both: at the spot but facing away, it does not show.
pub const TABLE_SPOT: [f32; 3] = [0.8, 10.66, 69.5];
pub const TABLE_YAW: f32 = -170.0;
/// The landing must be this close to the spot (metres).
const SPOT_TOLERANCE: f32 = 1.0;
/// The expedition menu is open: `CSMenuMan+0x48` (the `menu` word of `pos`) reads this with it open, 0 with no menu (2026-10-27).
const MENU_EXPEDITION: u32 = 0x20_0004;
/// In the open menu: F2 = the Matchmaking Settings tab (cursor on Number of Players), DOWN = the Team Session row's Invite Members
/// button, F = press it (screenshots `m2`/`m3`/`m4`, MULTI §19). Q closes the menu (in the plain world Q is the harmless lock-on).
const KEYS_INVITE: &str = "F2; wait 1500ms; DOWN; wait 800ms; F; wait 2s";
/// What the game logs (through the mod's lobby probe) when Invite Members has made the Seamless Co-op lobby.
const LOBBY_CREATED: &str = "LobbyCreated_t result=1";

/// `pos world (x, y, z) ... menu 0x..` → (world, menu word).
pub fn parse_pos(reply: &str) -> Option<([f32; 3], Option<u32>)> {
    let p = reply.split("world (").nth(1)?.split(')').next()?;
    let v: Vec<f32> = p.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    if v.len() != 3 {
        return None;
    }
    let menu = reply
        .split("menu 0x")
        .nth(1)
        .and_then(|m| u32::from_str_radix(m.split_whitespace().next()?, 16).ok());
    Some(([v[0], v[1], v[2]], menu))
}

/// The text after the last line that contains `tag` (`None` if the tag is not in the log tail).
pub fn after_tag<'a>(log: &'a str, tag: &str) -> Option<&'a str> {
    log.rfind(tag).map(|i| &log[i + tag.len()..])
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Host in the Roundtable → Invite Members pressed (Seamless Co-op makes its lobby; gbe sends the invite to every LAN instance, which
/// joins by itself). Position-independent: the mod's `tp` stands the player at [`TABLE_SPOT`] whatever spot the save remembers, then
/// the menu keys. Success = the mod's lobby probe logs [`LOBBY_CREATED`] after this attempt's marker; else close the menus and retry.
pub fn invite_members(st: &State, tries: u32) -> Result<String, String> {
    let pid = st.live_pid().ok_or("the game is not running")?;
    let keys =
        |seq: &str| crate::desk::request(serde_json::json!({"op": "keys", "pid": pid, "seq": seq}));
    let pos = || -> Result<([f32; 3], Option<u32>), String> {
        parse_pos(&session::cmd(st, "pos")?).ok_or_else(|| {
            "the mod's `pos` gave no position (dev build with `pos`/`tp`?)".to_string()
        })
    };
    // Close the game's menus (Q) until the menu word reads 0.
    let close_menus = || -> Result<(), String> {
        for _ in 0..5 {
            if pos()?.1 == Some(0) {
                return Ok(());
            }
            keys("Q; wait 1s")?;
        }
        Err("a game menu stays open after 5 x Q".into())
    };
    let s = session::cmd(st, "status")?;
    if !(s.contains("world loaded") && s.contains("team_type 1")) {
        return Err(format!("not in the Roundtable: {s}"));
    }
    let mut last = String::new();
    for attempt in 1..=tries {
        close_menus()?;
        let r = session::cmd(
            st,
            &format!(
                "tp {} {} {} {}",
                TABLE_SPOT[0], TABLE_SPOT[1], TABLE_SPOT[2], TABLE_YAW
            ),
        )?;
        if !r.starts_with("tp:") {
            return Err(format!("tp refused: {r}"));
        }
        std::thread::sleep(Duration::from_millis(1500));
        let (at, _) = pos()?;
        if dist(at, TABLE_SPOT) > SPOT_TOLERANCE {
            last = format!("attempt {attempt}: landed at {at:?}, not at the table spot");
            continue;
        }
        keys("F; wait 2500ms")?;
        let menu = pos()?.1;
        if menu != Some(MENU_EXPEDITION) {
            last = format!(
                "attempt {attempt}: F did not open the expedition menu (menu word {menu:?})"
            );
            continue;
        }
        let tag = format!(
            "nro invite_members attempt {attempt} {}",
            session::now_stamp()
        );
        session::cmd(st, &format!("echo {tag}"))?;
        keys(KEYS_INVITE)?;
        let until = Instant::now() + Duration::from_secs(12);
        while Instant::now() < until {
            let log = session::cmd(st, "log 300").unwrap_or_default();
            if let Some(line) =
                after_tag(&log, &tag).and_then(|t| t.lines().find(|l| l.contains(LOBBY_CREATED)))
            {
                let line = line.trim().to_string();
                close_menus()?;
                return Ok(format!("attempt {attempt}: {line}"));
            }
            std::thread::sleep(Duration::from_millis(700));
        }
        last = format!("attempt {attempt}: no `{LOBBY_CREATED}` in the log within 12 s");
    }
    let _ = close_menus();
    Err(format!(
        "Invite Members not reached after {tries} attempt(s); {last}"
    ))
}

/// A Seamless Co-op party member in the Roundtable → its "ready" for the expedition (MULTI-INSTANCE §20): at the table spot (`tp`), F
/// opens the expedition menu (the first key into a window that was in the back is sometimes lost: the menu word is checked and F
/// retried), then F until the menu closes (the host: target, then Commence; a joiner: Commence, the host's target is already set).
/// The party list then shows the player ready; the expedition loads when every member is. Success = the menu closed after F.
pub fn commence(st: &State, tries: u32) -> Result<String, String> {
    let pid = st.live_pid().ok_or("the game is not running")?;
    let keys =
        |seq: &str| crate::desk::request(serde_json::json!({"op": "keys", "pid": pid, "seq": seq}));
    let menu = || -> Result<Option<u32>, String> {
        Ok(parse_pos(&session::cmd(st, "pos")?)
            .ok_or("the mod's `pos` gave no position (dev build with `pos`/`tp`?)")?
            .1)
    };
    let s = session::cmd(st, "status")?;
    if !(s.contains("world loaded") && s.contains("team_type 1")) {
        return Err(format!("not in the Roundtable: {s}"));
    }
    let mut last = String::new();
    for attempt in 1..=tries {
        for _ in 0..5 {
            if menu()? == Some(0) {
                break;
            }
            keys("Q; wait 1s")?;
        }
        let r = session::cmd(
            st,
            &format!(
                "tp {} {} {} {}",
                TABLE_SPOT[0], TABLE_SPOT[1], TABLE_SPOT[2], TABLE_YAW
            ),
        )?;
        if !r.starts_with("tp:") {
            return Err(format!("tp refused: {r}"));
        }
        std::thread::sleep(Duration::from_millis(1500));
        let mut open = false;
        for _ in 0..3 {
            keys("wait 300ms; F; wait 2500ms")?;
            if menu()? == Some(MENU_EXPEDITION) {
                open = true;
                break;
            }
        }
        if !open {
            last = format!("attempt {attempt}: F did not open the expedition menu");
            continue;
        }
        for n in 1..=3 {
            keys("F; wait 2s")?;
            if menu()? == Some(0) {
                return Ok(format!("attempt {attempt}: ready after {n} F in the menu"));
            }
        }
        last = format!("attempt {attempt}: the expedition menu stayed open after 3 F");
    }
    Err(format!(
        "commence not reached after {tries} attempt(s); {last}"
    ))
}

fn step_name(s: &Step) -> String {
    match s {
        Step::Console(l) => format!("console {l}"),
        Step::Wait(w) => format!("wait {} ~ {:?}", w.console, w.contains.needles()),
        Step::Expect(e) => format!("expect {} ~ {:?}", e.console, e.contains.needles()),
        Step::Keys(k) => format!("keys {k}"),
        Step::Navigate(n) => format!("navigate {n}"),
        Step::SleepMs(m) => format!("sleep {m} ms"),
        Step::Screenshot(n) => format!("screenshot {n}"),
        Step::Puppets(n) => format!("puppets {n}"),
        Step::PuppetRing(r) => format!("puppet_ring {r}"),
        Step::PuppetFront(d) => format!("puppet_front {d}"),
        Step::Mark(n) => format!("mark {n}"),
        Step::WaitLog(w) => format!("wait_log ~ {:?}", w.contains.needles()),
        Step::ExpectLog(e) => format!("expect_log ~ {:?}", e.contains.needles()),
    }
}

/// What the log steps of one game remember: the marks and the log length when the previous step finished.
#[derive(Debug, Default, Clone)]
pub struct LogCtx {
    marks: BTreeMap<String, usize>,
    step_end: usize,
}

impl LogCtx {
    fn offset(&self, since: &Since) -> Result<usize, String> {
        match since {
            Since::Step => Ok(self.step_end),
            Since::Run => Ok(0),
            Since::Mark(n) => self
                .marks
                .get(n)
                .copied()
                .ok_or_else(|| format!("no mark {n:?} was set on this game")),
        }
    }
}

fn log_path(st: &State) -> PathBuf {
    st.dir().join("mod").join("nrpvp.log")
}

/// The game's mod log file as bytes (empty when it does not exist yet).
fn read_log(st: &State) -> Vec<u8> {
    std::fs::read(log_path(st)).unwrap_or_default()
}

/// Run one step with no log memory (`nro navigate`, `nro keys`). `Err` = the run cannot go on; `Ok((false, ..))` = an expectation failed.
pub fn step(st: &State, s: &Step) -> Result<(bool, String), String> {
    step_with(st, s, true, &mut LogCtx::default())
}

/// Run one step of a scenario. `god`: whether `navigate = "expedition"` ends with `god 1`; `ctx`: this game's log marks.
pub fn step_with(
    st: &State,
    s: &Step,
    god: bool,
    ctx: &mut LogCtx,
) -> Result<(bool, String), String> {
    match s {
        Step::Mark(n) => {
            let at = read_log(st).len();
            ctx.marks.insert(n.clone(), at);
            Ok((true, format!("mark {n} at byte {at}")))
        }
        Step::WaitLog(w) => {
            let off = ctx.offset(&w.since)?;
            let t = Instant::now();
            loop {
                let log = read_log(st);
                let sc = logscan::scan(&log, off, &w.contains, &w.absent);
                if let Some(f) = &sc.forbidden {
                    return Err(format!(
                        "forbidden text {f:?} in the log; tail: {}",
                        logscan::excerpt(&log, off, 300)
                    ));
                }
                if sc.found {
                    return Ok((true, logscan::excerpt(&log, off, 300)));
                }
                if st.live_pid().is_none() {
                    return Err("the game exited".into());
                }
                if t.elapsed() >= Duration::from_secs(w.timeout_s) {
                    return Err(format!(
                        "timed out after {} s waiting for {:?} in the log; tail: {}",
                        w.timeout_s,
                        w.contains.needles(),
                        logscan::excerpt(&log, off, 300)
                    ));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        Step::ExpectLog(e) => {
            let off = ctx.offset(&e.since)?;
            let log = read_log(st);
            let sc = logscan::scan(&log, off, &e.contains, &e.absent);
            let detail = match &sc.forbidden {
                Some(f) => format!("forbidden text {f:?} in the log"),
                None => logscan::excerpt(&log, off, 300),
            };
            Ok((sc.ok(), detail))
        }
        other => step_plain(st, other, god),
    }
}

fn step_plain(st: &State, s: &Step, god: bool) -> Result<(bool, String), String> {
    match s {
        Step::Mark(_) | Step::WaitLog(_) | Step::ExpectLog(_) => {
            Err("log steps need a scenario run".into())
        }
        Step::Console(l) => session::cmd(st, l).map(|r| (true, r)),
        Step::Wait(w) => {
            let t = Instant::now();
            let mut last = String::new();
            while t.elapsed() < Duration::from_secs(w.timeout_s) {
                if st.live_pid().is_none() {
                    return Err("the game exited".into());
                }
                if let Ok(r) = session::cmd(st, &w.console) {
                    if w.contains.all_in(&r) && w.absent.none_in(&r) {
                        return Ok((true, r));
                    }
                    last = r;
                }
                std::thread::sleep(Duration::from_millis(w.every_ms));
            }
            Err(format!(
                "timed out after {} s; last reply: {last}",
                w.timeout_s
            ))
        }
        Step::Expect(e) => {
            let r = session::cmd(st, &e.console)?;
            Ok((e.contains.all_in(&r) && e.absent.none_in(&r), r))
        }
        Step::Keys(k) => {
            let pid = st.live_pid().ok_or("the game is not running")?;
            crate::desk::request(serde_json::json!({"op": "keys", "pid": pid, "seq": k}))
                .map(|_| (true, String::new()))
        }
        Step::Navigate(n) => match n.as_str() {
            "title_to_world" => title_to_world(st, Duration::from_secs(300)).map(|r| (true, r)),
            "expedition" => to_expedition(st, Duration::from_secs(240), god).map(|r| (true, r)),
            "invite_members" => invite_members(st, 3).map(|r| (true, r)),
            "commence" => commence(st, 3).map(|r| (true, r)),
            other => Err(format!(
                "unknown navigate target {other:?} (known: title_to_world, expedition, invite_members, commence)"
            )),
        },
        Step::SleepMs(m) => {
            std::thread::sleep(Duration::from_millis(*m));
            Ok((true, String::new()))
        }
        Step::Puppets(n) => crate::ops::puppets(*n as usize)
            .map(|v| (true, v["list"].as_str().unwrap_or_default().to_string())),
        Step::PuppetRing(r) => crate::ops::puppet_ring(*r).map(|v| (true, v.to_string())),
        Step::PuppetFront(d) => crate::ops::puppet_front(*d).map(|v| (true, v.to_string())),
        Step::Screenshot(n) => {
            let pid = st.live_pid().ok_or("the game is not running")?;
            let p = st.dir().join("shots").join(format!("{n}.png"));
            crate::desk::request(
                serde_json::json!({"op": "shot", "pid": pid, "path": p.display().to_string()}),
            )
            .map(|v| (true, v.as_str().unwrap_or_default().to_string()))
        }
    }
}

/// The steps, in order, each on its instance's game. Returns (any expectation failed, the abort reason).
fn run_steps(
    sc: &Scenario,
    games: &BTreeMap<Option<u32>, State>,
    rep: &mut Report,
) -> (bool, Option<String>) {
    let mut failed = false;
    let mut ctxs: BTreeMap<Option<u32>, LogCtx> = BTreeMap::new();
    for s in &sc.steps {
        let n = sc.instance_of(s);
        let name = match n {
            Some(n) => format!("[i{n}] {}", step_name(&s.step)),
            None => step_name(&s.step),
        };
        let t = Instant::now();
        let r = match games.get(&n) {
            Some(st) => step_with(st, &s.step, s.god(), ctxs.entry(n).or_default()),
            None => Err(format!("no game for instance {n:?}")),
        };
        let secs = t.elapsed().as_secs_f64();
        // `since = "step"` of the next step starts here, in every game's log.
        for (k, st) in games {
            ctxs.entry(*k).or_default().step_end = read_log(st).len();
        }
        match r {
            Ok((ok, detail)) => {
                failed |= !ok;
                rep.steps.push(StepResult {
                    step: name,
                    ok,
                    detail,
                    secs,
                });
            }
            Err(e) => {
                rep.steps.push(StepResult {
                    step: name,
                    ok: false,
                    detail: e.clone(),
                    secs,
                });
                return (failed, Some(e));
            }
        }
    }
    (failed, None)
}

pub fn run(file: &std::path::Path, mod_dist: Option<PathBuf>) -> Result<Report, String> {
    let sc = Scenario::parse(
        &std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?,
    )?;
    let pins = session::pins()?;
    session::recover_registry();
    // The games, keyed by instance (`None` = the single normal game), in launch order.
    let plan: Vec<Option<u32>> = if sc.instances.is_empty() {
        vec![None]
    } else {
        sc.instances.iter().map(|i| Some(i.n)).collect()
    };
    for n in plan.iter().flatten() {
        if State::instance(*n).is_ok_and(|s| s.live_pid().is_some()) {
            return Err(format!(
                "instance {n} is already running; nro stop --all first"
            ));
        }
    }
    let mut rep = Report {
        scenario: sc.name.clone(),
        run: String::new(),
        verdict: String::new(),
        steps: Vec::new(),
        note: String::new(),
    };
    let mut games: BTreeMap<Option<u32>, State> = BTreeMap::new();
    let mut order: Vec<Option<u32>> = Vec::new();
    let mut aborted = None;
    let mut refused = None;
    for n in &plan {
        let mut st = session::stage(
            &pins,
            &Stage {
                name: match n {
                    Some(n) => format!("{}-i{n}", sc.name),
                    None => sc.name.clone(),
                },
                mod_dist: mod_dist.clone(),
                config: n.map_or_else(|| sc.config.clone(), |n| sc.config_for(n)),
                no_gamedata: false,
                emulated: *n,
            },
        )?;
        let t = Instant::now();
        let launched = session::launch(&pins, &mut st);
        let ok = match launched {
            Ok(pf) if !pf.ok => {
                refused = Some(st.note.clone());
                false
            }
            Ok(_) => match session::wait_console(&st, Duration::from_secs(180)) {
                Ok(()) => true,
                Err(e) => {
                    aborted = Some(format!("console: {e}"));
                    true // launched: it must be stopped
                }
            },
            Err(e) => {
                aborted = Some(format!("launch: {e}"));
                st.live_pid().is_some()
            }
        };
        rep.steps.push(StepResult {
            step: match n {
                Some(n) => format!("[i{n}] launch"),
                None => "launch".into(),
            },
            ok: refused.is_none() && aborted.is_none(),
            detail: st.dir.clone(),
            secs: t.elapsed().as_secs_f64(),
        });
        rep.run = if rep.run.is_empty() {
            st.dir.clone()
        } else {
            format!("{} | {}", rep.run, st.dir)
        };
        if ok || st.live_pid().is_some() {
            order.push(*n);
            games.insert(*n, st);
        } else {
            write(&st, &rep);
        }
        if refused.is_some() || aborted.is_some() {
            break;
        }
    }
    let mut failed = false;
    if refused.is_none() && aborted.is_none() {
        let (f, a) = run_steps(&sc, &games, &mut rep);
        failed = f;
        aborted = a;
    }
    // Evidence before teardown: each game's mod log tail.
    for st in games.values() {
        if st.live_pid().is_some() {
            if let Ok(t) = session::cmd(st, "log 120") {
                let _ = std::fs::write(st.dir().join("log-tail.txt"), t);
            }
        }
    }
    let mut notes = Vec::new();
    if sc.teardown.as_deref() != Some("keep") {
        // Last launched first.
        for n in order.iter().rev() {
            if let Some(st) = games.get_mut(n) {
                notes.push(session::stop(st));
            }
        }
    }
    let mut phase_note = None;
    for st in games.values() {
        let st2 = State::load(&st.dir()).unwrap_or(st.clone());
        if st2.phase == "aborted" {
            phase_note = Some(st2.note.clone());
        }
    }
    rep.note = notes.join(" | ");
    rep.verdict = if let Some(r) = refused {
        rep.note = format!("{r} | {}", rep.note);
        "REFUSED".into()
    } else if let Some(p) = phase_note {
        rep.note = format!("{p} | {}", rep.note);
        "ABORTED".into()
    } else if let Some(a) = aborted {
        rep.note = format!("{a} | {}", rep.note);
        "ABORTED".into()
    } else if failed {
        "FAIL".into()
    } else {
        "PASS".into()
    };
    for st in games.values() {
        write(st, &rep);
    }
    Ok(rep)
}

fn write(st: &State, rep: &Report) {
    let _ = std::fs::write(
        st.dir().join("report.json"),
        serde_json::to_string_pretty(rep).unwrap(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16:9 game image padded to 1720x720 with the two restore-dialog frame lines drawn (or only one).
    fn synthetic(both: bool) -> (usize, usize, Vec<u8>) {
        let (w, h) = (1720usize, 720usize);
        let mut rgb = vec![10u8; w * h * 3];
        let (ox, gw, oy, gh) = game_rect(w, h);
        let y = oy + (0.571 * gh as f32) as usize;
        let mut line = |xs: (f32, f32)| {
            for x in ox + (xs.0 * gw as f32) as usize..ox + (xs.1 * gw as f32) as usize {
                let i = (y * w + x) * 3;
                rgb[i..i + 3].copy_from_slice(&[160, 160, 170]);
            }
        };
        line(RESTORE_CANCEL_X);
        if both {
            line(RESTORE_OK_X);
        }
        (w, h, rgb)
    }

    #[test]
    fn restore_dialog_needs_both_button_frames() {
        assert_eq!(game_rect(1720, 720), (220, 1280, 0, 720));
        let (w, h, rgb) = synthetic(true);
        assert!(is_restore_dialog(w, h, &rgb));
        let (w, h, rgb) = synthetic(false);
        assert!(!is_restore_dialog(w, h, &rgb));
    }

    /// The real screenshots of 2026-10-27 (run folders on Leonardo's PC; skipped where they are missing): only the restore
    /// dialog matches, not the title, the "previous session" notice, the expedition menu or the Roundtable.
    #[test]
    fn restore_dialog_on_real_screenshots() {
        let runs = crate::session::runs_dir();
        for (f, want) in [
            (r"1793063296-mi-queue-i1\shots\title_slow.png", true),
            (r"1793062338-session\shots\title1.png", false),
            (r"1793062356-session\shots\title2.png", false),
            (r"1793062513-mi-queue-i1\shots\menu2.png", false),
            (r"1793062513-mi-queue-i1\shots\host.png", false),
            (r"1793062519-mi-queue-i2\shots\exp_joiner.png", false),
        ] {
            if let Some((w, h, rgb)) = read_rgb(&runs.join(f)) {
                if std::env::var_os("NRO_DEBUG_DIALOG").is_some() {
                    let (ox, gw, _, _) = game_rect(w, h);
                    for y in 395..445 {
                        let row: String = (0..w)
                            .step_by(20)
                            .map(|x| {
                                let i = (y * w + x) * 3;
                                let l = (rgb[i] as u32 + rgb[i + 1] as u32 + rgb[i + 2] as u32) / 3;
                                if l >= 80 {
                                    '#'
                                } else if l >= 40 {
                                    '+'
                                } else {
                                    '.'
                                }
                            })
                            .collect();
                        eprintln!("{w}x{h} ox{ox} gw{gw} y{y} {row}");
                    }
                }
                assert_eq!(is_restore_dialog(w, h, &rgb), want, "{f}");
            }
        }
    }

    #[test]
    fn reads_the_pos_line() {
        let r = "pos world (12.50, -3.00, 40.25) physics (1.0, 2.0, 3.0) origin (0.0, 0.0, 0.0) q+0x50 (0.000, 0.383, 0.000, 0.924) yaw 45 menu 0x11";
        let (w, m) = parse_pos(r).unwrap();
        assert_eq!(w, [12.5, -3.0, 40.25]);
        assert_eq!(m, Some(0x11));
        assert_eq!(parse_pos("pos world (1, 2, 3) menu ?").unwrap().1, None);
        assert!(parse_pos("no world").is_none());
    }

    #[test]
    fn only_log_lines_after_the_marker_count() {
        let log =
            "a LobbyCreated_t result=1 old\nconsole: echo: nro invite_members attempt 1 x\nb\n";
        let tail = after_tag(log, "nro invite_members attempt 1 x").unwrap();
        assert!(!tail.contains(LOBBY_CREATED));
        let log2 = format!("{log}c mm_probe: {LOBBY_CREATED} (1 = OK)\n");
        assert!(after_tag(&log2, "nro invite_members attempt 1 x")
            .unwrap()
            .contains(LOBBY_CREATED));
        assert!(after_tag(log, "attempt 2").is_none());
    }
}
