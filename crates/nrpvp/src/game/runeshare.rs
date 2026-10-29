//! RW-8 (both builds): kill runes stay inside the killer's team. Game 1.03.3 / exe 1.3.3.0; the reading is in MEMORY_MAP §21.
//!
//! The game credits kill runes on **every** game separately, always to that game's own player (LOCAL), without asking who killed:
//! when a character dies, `exe+0x3DE710(dying)` reads its rune value (`vf 0x1A0` param `+0x2C` × SpEffect rate) and calls
//! `exe+0x668380(LOCAL, amount, dying, flag)`, which calls `PlayerIns vf 0x5D0` → `exe+0x2098C0` (`PlayerGameData+0x6C` += n). A
//! second caller, `exe+0x51DA30` (a loop over received messages), credits LOCAL for an enemy that is not loaded on this game.
//!
//! | Site | Call | Our answer |
//! |---|---|---|
//! | `exe+0x3DE800` | `exe+0x668380(LOCAL, amount, dying, flag)` in the death credit `exe+0x3DE710` | amount 0 when the dying character's last attacker (`ChrIns+0x1A0`) is a rival, or in Solo when none is recorded (a game records only hits it applied itself: the enemy's owner) |
//! | `exe+0x51DC0B` | same function, from the message loop `exe+0x51DA30` (no dying character) | passed on unchanged, counted (who killed is not in reach) |
//!
//! Both `call rel32` are pointed at a trampoline like the RW-9 sites (`hudfilter::patch`: the bytes are checked first, only the
//! displacement changes). With amount 0 the game's function still runs its tail (`exe+0x50A150`), as for an enemy worth no runes.
//! Teammates are not rivals, so their kills still credit (the game's vanilla share); LOCAL's own kills, items and pickups do not
//! pass here or are not touched. Log lines start with `runeshare:`.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use super::hudfilter::{alloc_near, patch, trampoline};

/// Site RVA, the callee RVA its `call rel32` must hold. `[VERIFIED: static disassembly of exe 1.3.3.0, MEMORY_MAP §21]`
const SITE_DEATH: (usize, usize) = (0x3D_E800, 0x66_8380);
const SITE_MESSAGE: (usize, usize) = (0x51_DC0B, 0x66_8380);
/// `ChrIns` -> handle of the character that hit it last (MEMORY_MAP §7 for players; on enemies see §21).
const CHR_LAST_ATTACKER: usize = super::layout::PLAYER_LAST_ATTACKER;
const CHR_CUR_ATTACKER: usize = super::layout::PLAYER_CUR_ATTACKER;
const CHR_HANDLE: usize = super::layout::CHR_HANDLE;
const CHR_ID: usize = super::layout::CHR_ID;
/// `PlayerGameData` -> rune count (`exe+0x2098C0` writes it).
pub(crate) const GAMEDATA_RUNES: usize = 0x6C;

const MAX_RIVALS: usize = 8;
static RIVALS: [AtomicU64; MAX_RIVALS] = [const { AtomicU64::new(0) }; MAX_RIVALS];
static RIVAL_COUNT: AtomicUsize = AtomicUsize::new(0);
static LOCAL_HANDLE: AtomicU64 = AtomicU64::new(u64::MAX);
/// The filter acts (a match runs, or the console forces it).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Console override: 0 = automatic, 1 = forced on, 2 = forced off.
static OVERRIDE: AtomicU8 = AtomicU8::new(0);
/// LOCAL has no teammate (team size 1).
static SOLO: AtomicBool = AtomicBool::new(true);
static ORIG: AtomicUsize = AtomicUsize::new(0);
static INSTALL_TRIED: AtomicBool = AtomicBool::new(false);
static DEATH_HOOKED: AtomicBool = AtomicBool::new(false);
static MESSAGE_HOOKED: AtomicBool = AtomicBool::new(false);
/// Credits seen / blocked at the death site, credits seen at the message site.
static KEPT: AtomicU32 = AtomicU32::new(0);
static BLOCKED: AtomicU32 = AtomicU32::new(0);
static BLOCKED_RUNES: AtomicU64 = AtomicU64::new(0);
static MESSAGES: AtomicU32 = AtomicU32::new(0);
/// LOCAL's rune count when `runes_mark` was last given (u64::MAX = never).
static MARK: AtomicU64 = AtomicU64::new(u64::MAX);
static RUNES_NOW: AtomicU64 = AtomicU64::new(u64::MAX);

/// What the hook saw (logged by [`tick`] on the main loop, not on the game thread).
#[derive(Clone, Copy, Debug)]
struct Event {
    site: &'static str,
    amount: f32,
    dying: u64,
    npc: u32,
    last: u64,
    cur: u64,
    who: Killer,
    blocked: bool,
}
static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());

/// Who hit the dying character last, from this game's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Killer {
    Local,
    Rival,
    /// A known character that is neither: a teammate (or an enemy).
    Other,
    /// Nobody recorded on this game: what a game sees when the enemy's owner game applied the hits (seen live 2026-10-27:
    /// `+0x198` / `+0x1A0` = -1 on the client for every kill by the host, MEMORY_MAP §21).
    Unknown,
}

/// Classify the last attacker's handle (pure, tested).
pub(crate) fn killer(last: u64, local: u64, rivals: &[u64]) -> Killer {
    if last == local && local != u64::MAX {
        Killer::Local
    } else if last != 0 && last != u64::MAX && rivals.contains(&last) {
        Killer::Rival
    } else if last == 0 || last == u64::MAX {
        Killer::Unknown
    } else {
        Killer::Other
    }
}

/// Block the credit while the filter is on: a rival's kill always; a kill nobody is recorded for only when LOCAL has no teammate
/// (Solo: the only kills that are LOCAL's own record LOCAL as the attacker). With teammates an unknown killer stays vanilla (it may
/// be a teammate's kill; the limit is in MEMORY_MAP §21). Pure, tested.
pub(crate) fn block(active: bool, who: Killer, solo: bool) -> bool {
    active && (who == Killer::Rival || (who == Killer::Unknown && solo))
}

fn wanted(in_match: bool) -> bool {
    match OVERRIDE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => in_match,
    }
}

/// Console `runeshare [0|1|auto]`.
pub(crate) fn set_override(v: Option<bool>) {
    OVERRIDE.store(
        match v {
            None => 0,
            Some(true) => 1,
            Some(false) => 2,
        },
        Ordering::Relaxed,
    );
}

fn push(e: Event) {
    if let Ok(mut v) = EVENTS.try_lock() {
        if v.len() < 64 {
            v.push(e);
        }
    }
}

type Award = unsafe extern "system" fn(usize, f32, usize, u8) -> i32;

fn orig() -> Award {
    // SAFETY: ORIG holds the game's function `exe+0x668380` (set before any site was patched).
    unsafe { std::mem::transmute::<usize, Award>(ORIG.load(Ordering::Relaxed)) }
}

/// Death credit `exe+0x3DE710` (game thread): `dying` is the character that died on this game.
unsafe extern "system" fn hook_death(local: usize, amount: f32, dying: usize, flag: u8) -> i32 {
    let read = |a: usize| super::mem::read::<u64>(a).unwrap_or(u64::MAX);
    let (handle, last, cur) = if dying != 0 {
        (
            read(dying + CHR_HANDLE),
            read(dying + CHR_LAST_ATTACKER),
            read(dying + CHR_CUR_ATTACKER),
        )
    } else {
        (u64::MAX, u64::MAX, u64::MAX)
    };
    let npc = if dying != 0 {
        super::mem::read::<u32>(dying + CHR_ID).unwrap_or(0)
    } else {
        0
    };
    let n = RIVAL_COUNT.load(Ordering::Acquire).min(MAX_RIVALS);
    let mut list = [0u64; MAX_RIVALS];
    for (i, slot) in list.iter_mut().enumerate().take(n) {
        *slot = RIVALS[i].load(Ordering::Relaxed);
    }
    let who = killer(last, LOCAL_HANDLE.load(Ordering::Relaxed), &list[..n]);
    let blocked = block(
        ACTIVE.load(Ordering::Relaxed),
        who,
        SOLO.load(Ordering::Relaxed),
    );
    if blocked {
        BLOCKED.fetch_add(1, Ordering::Relaxed);
        BLOCKED_RUNES.fetch_add(amount.max(0.0) as u64, Ordering::Relaxed);
    } else {
        KEPT.fetch_add(1, Ordering::Relaxed);
    }
    push(Event {
        site: "death",
        amount,
        dying: handle,
        npc,
        last,
        cur,
        who,
        blocked,
    });
    // SAFETY: the game's own function with the game's own arguments; amount 0 = "this kill is worth nothing" (its own early-out).
    unsafe { orig()(local, if blocked { 0.0 } else { amount }, dying, flag) }
}

/// Message credit `exe+0x51DA30` (game thread): counted and passed on.
unsafe extern "system" fn hook_message(local: usize, amount: f32, src: usize, flag: u8) -> i32 {
    MESSAGES.fetch_add(1, Ordering::Relaxed);
    push(Event {
        site: "message",
        amount,
        dying: u64::MAX,
        npc: 0,
        last: u64::MAX,
        cur: u64::MAX,
        who: Killer::Unknown,
        blocked: false,
    });
    // SAFETY: as `hook_death`, arguments unchanged.
    unsafe { orig()(local, amount, src, flag) }
}

fn install(base: usize) {
    if INSTALL_TRIED.swap(true, Ordering::Relaxed) {
        return;
    }
    ORIG.store(base + SITE_DEATH.1, Ordering::Relaxed);
    let Some(page) = alloc_near(base) else {
        tracing::warn!(
            "runeshare: no page near the exe for the trampolines; runes stay shared with rivals"
        );
        return;
    };
    let hooks: [(&AtomicBool, &str, (usize, usize), usize); 2] = [
        (
            &DEATH_HOOKED,
            "death credit",
            SITE_DEATH,
            hook_death as Award as usize,
        ),
        (
            &MESSAGE_HOOKED,
            "message credit",
            SITE_MESSAGE,
            hook_message as Award as usize,
        ),
    ];
    for (i, (flag, name, (site, callee), hook)) in hooks.into_iter().enumerate() {
        let tramp = page + i * 16;
        let code = trampoline(hook);
        // SAFETY: `page` is our own fresh RWX page of 0x1000 bytes; 2 × 16 bytes fit.
        unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), tramp as *mut u8, code.len()) };
        match patch(base + site, base + callee, tramp) {
            Ok(()) => {
                flag.store(true, Ordering::Relaxed);
                tracing::info!(
                    "runeshare: {name} hooked at exe+0x{site:X} (trampoline 0x{tramp:X})"
                );
            }
            Err(e) => tracing::warn!("runeshare: {name} at exe+0x{site:X} NOT hooked: {e}"),
        }
    }
}

/// Main loop, every tick (both builds). `in_match`: the mod's PvP mode runs in an expedition; `rivals`: rival players' handles;
/// `local`: LOCAL's PlayerIns.
pub(crate) fn tick(base: usize, in_match: bool, solo: bool, rivals: &[u64], local: Option<usize>) {
    SOLO.store(solo, Ordering::Relaxed);
    install(base);
    let n = rivals.len().min(MAX_RIVALS);
    for (slot, &h) in RIVALS.iter().zip(rivals) {
        slot.store(h, Ordering::Relaxed);
    }
    RIVAL_COUNT.store(n, Ordering::Release);
    LOCAL_HANDLE.store(
        local
            .and_then(|l| super::mem::read::<u64>(l + CHR_HANDLE))
            .unwrap_or(u64::MAX),
        Ordering::Relaxed,
    );
    RUNES_NOW.store(
        local.and_then(local_runes).map_or(u64::MAX, u64::from),
        Ordering::Relaxed,
    );
    let want = wanted(in_match);
    if ACTIVE.swap(want, Ordering::Relaxed) != want {
        tracing::info!(
            "runeshare: kill runes {} ({n} rival(s))",
            if want {
                "from a rival's kill are not credited"
            } else {
                "are credited as in the game"
            }
        );
    }
    let events: Vec<Event> =
        std::mem::take(&mut *EVENTS.lock().unwrap_or_else(PoisonError::into_inner));
    for e in events {
        tracing::info!(
            "runeshare: {} credit of {:.0} rune(s) for chr {:#X} (npc {}), last attacker {:#X}, current {:#X} = {:?} -> {}",
            e.site,
            e.amount,
            e.dying,
            e.npc,
            e.last,
            e.cur,
            e.who,
            if e.blocked {
                "BLOCKED (not this team's kill)"
            } else {
                "credited"
            }
        );
    }
}

/// LOCAL's rune count (`[PlayerIns+0x5A0]+0x6C`).
fn local_runes(local: usize) -> Option<u32> {
    let data =
        super::mem::read::<usize>(local + super::layout::PLAYER_GAMEDATA).filter(|d| *d != 0)?;
    super::mem::read::<u32>(data + GAMEDATA_RUNES)
}

/// Console `runes_mark`: remember LOCAL's rune count now (the next `runeshare` shows the change since).
pub(crate) fn mark() -> String {
    let now = RUNES_NOW.load(Ordering::Relaxed);
    MARK.store(now, Ordering::Relaxed);
    if now == u64::MAX {
        "runes_mark: LOCAL's rune count is not readable now".into()
    } else {
        format!("runes_mark: {now} rune(s)")
    }
}

/// `+d since mark` text (pure, tested).
fn since(now: u64, mark: u64) -> String {
    if now == u64::MAX {
        "runes ?".into()
    } else if mark == u64::MAX {
        format!("runes {now} (no mark)")
    } else {
        format!("runes {now} ({:+} since mark)", now as i64 - mark as i64)
    }
}

/// Console `runeshare` with no argument.
pub(crate) fn describe() -> String {
    let hooked = |b: &AtomicBool| {
        if b.load(Ordering::Relaxed) {
            "hooked"
        } else {
            "NOT hooked"
        }
    };
    format!(
        "runeshare: {} ({}); {}; death credit {} ({} credited, {} blocked = {} rune(s)); message credit {} ({} seen); rivals {}",
        if ACTIVE.load(Ordering::Relaxed) {
            "rivals' kills give no runes"
        } else {
            "runes as in the game"
        },
        ["automatic (in a match)", "forced on", "forced off"]
            [usize::from(OVERRIDE.load(Ordering::Relaxed)).min(2)],
        since(RUNES_NOW.load(Ordering::Relaxed), MARK.load(Ordering::Relaxed)),
        hooked(&DEATH_HOOKED),
        KEPT.load(Ordering::Relaxed),
        BLOCKED.load(Ordering::Relaxed),
        BLOCKED_RUNES.load(Ordering::Relaxed),
        hooked(&MESSAGE_HOOKED),
        MESSAGES.load(Ordering::Relaxed),
        RIVAL_COUNT.load(Ordering::Relaxed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::hudfilter::rel32;

    #[test]
    fn the_documented_sites() {
        // E8 7B 9B 28 00 and E8 70 A7 14 00 in the file.
        assert_eq!(rel32(SITE_DEATH.0, SITE_DEATH.1), Some(0x0028_9B7B));
        assert_eq!(rel32(SITE_MESSAGE.0, SITE_MESSAGE.1), Some(0x0014_A770));
        for (s, _) in [SITE_DEATH, SITE_MESSAGE] {
            assert_eq!((s + 1) / 64, (s + 4) / 64, "one cache line");
        }
        assert_eq!(
            (CHR_LAST_ATTACKER, CHR_CUR_ATTACKER, GAMEDATA_RUNES),
            (0x1A0, 0x198, 0x6C)
        );
    }

    #[test]
    fn only_a_rivals_kill_is_blocked() {
        let rivals = [0x10, 0x20];
        assert_eq!(killer(0x10, 0x1, &rivals), Killer::Rival);
        assert_eq!(killer(0x1, 0x1, &rivals), Killer::Local);
        assert_eq!(killer(0x30, 0x1, &rivals), Killer::Other, "a teammate");
        assert_eq!(
            killer(u64::MAX, 0x1, &rivals),
            Killer::Unknown,
            "nobody recorded (the host applied the hits)"
        );
        assert_eq!(killer(0, 0x1, &rivals), Killer::Unknown);
        for solo in [true, false] {
            assert!(block(true, Killer::Rival, solo));
            assert!(!block(false, Killer::Rival, solo), "off outside a match");
            assert!(!block(true, Killer::Local, solo) && !block(true, Killer::Other, solo));
        }
        assert!(block(true, Killer::Unknown, true), "Solo: not LOCAL's kill");
        assert!(
            !block(true, Killer::Unknown, false),
            "with teammates: may be a teammate's"
        );
    }

    #[test]
    fn change_since_the_mark() {
        assert_eq!(since(150, 100), "runes 150 (+50 since mark)");
        assert_eq!(since(100, 100), "runes 100 (+0 since mark)");
        assert_eq!(since(90, 100), "runes 90 (-10 since mark)");
        assert_eq!(since(5, u64::MAX), "runes 5 (no mark)");
        assert_eq!(since(u64::MAX, 3), "runes ?");
    }

    #[test]
    fn the_override_beats_the_match() {
        set_override(Some(true));
        assert!(wanted(false));
        set_override(Some(false));
        assert!(!wanted(true));
        set_override(None);
        assert!(wanted(true) && !wanted(false));
    }
}
