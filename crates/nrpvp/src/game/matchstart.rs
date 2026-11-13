//! MM-3g + MM-7: the two game actions behind "no click after Commence", called from the mod with no menu open. Both builds.
//!
//! **The lobby (MM-3g).** The Invite Members item queues three steps on its menu (`exe+0x9F9DB0`, MEMORY_MAP §12): `exe+0x9F4AE0` (a sound,
//! then `exe+0xDFD0C0(session)` starts the job that creates the game's lobby), a wait while `session+0x70` is 1, and the call that ends in
//! the Steam invite window. Only the job is called here, so the window has no caller at all. `session` is the object at `exe+0x3C21FC8`.
//!
//! **The start (MM-7).** In a party the Commence button's last step (`exe+0x9EA5F0`, reached through `exe+0x9ED090` while the menu's state
//! `+0x19C8` is 2) does two things with the team object at `exe+0x3C21F48`: the party's leader sends the settings to everyone
//! (`exe+0xDF9E90(team, settings)`, packet 0x95: the byte at `+0`, the target list and the Number of Players byte), and every player marks
//! itself ready (`exe+0xDFA0A0(team, 1)`, packet 0x96). When all are ready the game itself runs the departure. Neither call needs the
//! menu: the first copies what it needs out of `settings`, the second only changes anything when the value changes.
//!
//! Both run on the game's own thread (they allocate, TRAPS "Game functions that allocate"), from the frame hook of `game::gametoast`: the
//! same place the menu's own steps run from. No hardware slot is used. Log lines start with `matchstart:`.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use super::mem;

/// `session.start_lobby_job()`: `mov [rsp+10h],rbx; mov [rsp+18h],rsi` (game 1.03.3, exe 1.3.3.0).
const RVA_LOBBY_JOB: usize = 0xDF_D0C0;
const LOBBY_JOB_BYTES: [u8; 8] = [0x48, 0x89, 0x5C, 0x24, 0x10, 0x48, 0x89, 0x74];
/// `team.send_settings(settings)`: `push rbx; sub rsp,50h; mov rax,[rip+...]` (game 1.03.3, exe 1.3.3.0).
const RVA_SEND_SETTINGS: usize = 0xDF_9E90;
const SEND_SETTINGS_BYTES: [u8; 8] = [0x40, 0x53, 0x48, 0x83, 0xEC, 0x50, 0x48, 0x8B];
/// `team.set_ready(bool)`: `mov [rsp+20h],rsi; push rdi; sub rsp,20h` (game 1.03.3, exe 1.3.3.0).
const RVA_SET_READY: usize = 0xDF_A0A0;
const SET_READY_BYTES: [u8; 8] = [0x48, 0x89, 0x74, 0x24, 0x20, 0x57, 0x48, 0x83];
/// The team object.
const SLOT_TEAM: usize = 0x3C2_1F48;
/// `set_targets(_, list, 0)` keeps the leader's own chosen targets: `mov [rsp+8],rbx; push rdi; sub rsp,20h` (game 1.03.3, exe 1.3.3.0). The
/// list object is `settings+0x50` (begin at `+8`, end at `+0x10`).
const RVA_SET_TARGETS: usize = 0x8D_EE80;
const SET_TARGETS_BYTES: [u8; 8] = [0x48, 0x89, 0x5C, 0x24, 0x08, 0x57, 0x48, 0x83];
const SET_TARGET_LIST: usize = 0x50;
/// Our Battle Royale card is what the game's own Commence sends as the target `[VERIFIED: exec_trace on exe+0xDF9E90, 2026-11-05]`.
pub(crate) const TARGET_BR_CARD: i32 = super::layout::BR_CARD_ROW;
/// The Number of Players byte the game's own Commence sent in the same trace.
pub(crate) const PLAYERS_ANY: u8 = 0;
/// The session object (`[exe+0x3C21FC8]`) and its state byte: 1, 3, 5, 7 = a job is running, 4 = busy (`exe+0x9F4B1E`).
const SLOT_SESSION: usize = 0x3C2_1FC8;
const SESSION_STATE: usize = 0x70;
/// The session object's first dword (non-zero while a lobby is held; the all-ready check `exe+0xDF9420` needs it) and its members
/// (`+0x30` first, `+0x38` end, 0xF0 each).
const SESSION_ACTIVE: usize = 0x0;
const SESSION_MEMBERS: usize = 0x30;
const SESSION_MEMBER_SIZE: usize = 0xF0;
/// `session.leave_lobby(reason)`: state 8, then the platform's leave of the lobby id (`exe+0xDFD940`; the game's own callers pass 0,
/// or 7 when it moves to another lobby). `mov [rsp+8],rbx; push rdi; sub rsp,..` (game 1.03.3, exe 1.3.3.0).
const RVA_LEAVE_LOBBY: usize = 0xDF_D940;
const LEAVE_LOBBY_BYTES: [u8; 8] = [0x48, 0x89, 0x5C, 0x24, 0x08, 0x57, 0x48, 0x83];
/// The online switch the Invite Members item reads (`exe+0x6AEAE0`): `[[exe+0x3C13258]+0xB40]`.
const SLOT_ONLINE: usize = 0x3C1_3258;
const ONLINE_BYTE: usize = 0xB40;

/// Settings object, as far as `exe+0xDF9E90` reads it.
const SET_KIND: usize = 0x00;
const SET_PLAYERS: usize = 0x49;
const SET_TARGETS_BEGIN: usize = 0x58;
const SET_TARGETS_END: usize = 0x60;
const SETTINGS_SIZE: usize = 0x80;

static WANT_LOBBY: AtomicBool = AtomicBool::new(false);
static WANT_LEAVE_ALONE: AtomicBool = AtomicBool::new(false);
static WANT_START: AtomicBool = AtomicBool::new(false);
static PLAYERS: AtomicU32 = AtomicU32::new(0);
static TARGET: AtomicI32 = AtomicI32::new(-1);
static LEADER: AtomicBool = AtomicBool::new(false);

type LobbyJob = unsafe extern "system" fn(usize) -> u8;
type SendSettings = unsafe extern "system" fn(usize, *const u8);
type SetReady = unsafe extern "system" fn(usize, u8);
type SetTargets = unsafe extern "system" fn(usize, *const u8, u8);

/// Whether the session state allows a new lobby job (pure; the check of `exe+0x9F4B1E`: `state & 0xF9 == 0 && state != 4`).
pub(crate) fn lobby_job_allowed(state: u8) -> bool {
    state & 0xF9 == 0 && state != 4
}

/// The settings bytes the leader sends (pure): only the fields the game reads, the targets as a `vector<int>` over `targets`.
pub(crate) fn settings_bytes(
    kind: u8,
    players: u8,
    targets: (usize, usize),
) -> [u8; SETTINGS_SIZE] {
    let mut s = [0u8; SETTINGS_SIZE];
    s[SET_KIND] = kind;
    s[SET_PLAYERS] = players;
    s[SET_TARGETS_BEGIN..SET_TARGETS_BEGIN + 8].copy_from_slice(&(targets.0 as u64).to_le_bytes());
    s[SET_TARGETS_END..SET_TARGETS_END + 8].copy_from_slice(&(targets.1 as u64).to_le_bytes());
    s
}

/// Ask the game for its lobby, on the next frame (any thread).
pub(crate) fn request_lobby() {
    WANT_LOBBY.store(true, Ordering::Release);
}

/// MM-REQ (any thread): on the next frame, leave the game's lobby when this player is its only member. After a match whose other
/// player went back to the Roundtable first, the host's game keeps that match's lobby with itself alone; a card pick then takes the
/// party path, where "everyone" (the host) is ready at once, and a vanilla run starts without our Commence (friend sessions
/// 2026-11-11 and 2026-11-12, `mi-arena-requeue`). The game's own leave clears it; the next Commence is ours again (live 2026-11-13).
pub(crate) fn request_leave_if_alone() {
    WANT_LEAVE_ALONE.store(true, Ordering::Release);
}

/// The state a held lobby sits in (the host's leftover read `active` 1, state 3, one member; the client's game read 0 / 0 / 0).
const STATE_HELD: u8 = 3;

/// Whether the lobby is a leftover to leave (pure): held (`active` non-zero, state 3, not busy or already leaving) with nobody but us.
pub(crate) fn alone_in_lobby(active: u32, state: u8, members: usize) -> bool {
    active != 0 && state == STATE_HELD && members <= 1
}

/// Mark this player ready, on the next frame (any thread); the party's `leader` sends the settings first: the Number of Players byte and
/// one target (negative: none).
pub(crate) fn request_start(leader: bool, players: u8, target: i32) {
    PLAYERS.store(u32::from(players), Ordering::Relaxed);
    TARGET.store(target, Ordering::Relaxed);
    LEADER.store(leader, Ordering::Relaxed);
    WANT_START.store(true, Ordering::Release);
}

/// Game thread, every frame (`game::gametoast`).
pub(crate) fn pump(base: usize) {
    if WANT_LOBBY.swap(false, Ordering::AcqRel) {
        lobby_now(base);
    }
    if WANT_START.swap(false, Ordering::AcqRel) {
        start_now(base);
    }
    if WANT_LEAVE_ALONE.swap(false, Ordering::AcqRel) {
        leave_if_alone(base);
    }
    #[cfg(feature = "dev")]
    trace::log_hit();
}

fn lobby_now(base: usize) {
    if mem::read::<[u8; 8]>(base + RVA_LOBBY_JOB) != Some(LOBBY_JOB_BYTES) {
        tracing::warn!("matchstart: the lobby job differs from game 1.03.3; not called");
        return;
    }
    let Some(online) =
        mem::read::<usize>(base + SLOT_ONLINE).and_then(|o| mem::read::<u8>(o + ONLINE_BYTE))
    else {
        tracing::warn!("matchstart: the online switch cannot be read; no lobby");
        return;
    };
    let Some(session) = mem::read::<usize>(base + SLOT_SESSION).filter(|s| *s != 0) else {
        tracing::warn!("matchstart: no session object; no lobby");
        return;
    };
    let Some(state) = mem::read::<u8>(session + SESSION_STATE) else {
        tracing::warn!("matchstart: the session state cannot be read; no lobby");
        return;
    };
    if !lobby_job_allowed(state) {
        tracing::info!("matchstart: a session job is running (state {state}); no second lobby");
        return;
    }
    // SAFETY: the address is the game's own function, its first bytes were checked above; it takes the session object, read from the
    // game's own slot and checked non-null; this is the game's thread, where the menu's own step calls it.
    let ok = unsafe { std::mem::transmute::<usize, LobbyJob>(base + RVA_LOBBY_JOB)(session) };
    tracing::info!(
        "matchstart: lobby job started from state {state} (online switch {online}), returned {ok}"
    );
}

fn leave_if_alone(base: usize) {
    // A queue that found a match makes a new lobby with us alone in it at first: never touch that one.
    if crate::mm::phase() != crate::mm::PHASE_IDLE {
        return;
    }
    let Some(session) = mem::read::<usize>(base + SLOT_SESSION).filter(|s| *s != 0) else {
        return;
    };
    let (Some(active), Some(state), Some(first), Some(end)) = (
        mem::read::<u32>(session + SESSION_ACTIVE),
        mem::read::<u8>(session + SESSION_STATE),
        mem::read::<usize>(session + SESSION_MEMBERS),
        mem::read::<usize>(session + SESSION_MEMBERS + 8),
    ) else {
        return;
    };
    let members = end.saturating_sub(first) / SESSION_MEMBER_SIZE;
    if !alone_in_lobby(active, state, members) {
        return;
    }
    if mem::read::<[u8; 8]>(base + RVA_LEAVE_LOBBY) != Some(LEAVE_LOBBY_BYTES) {
        tracing::warn!(
            "matchstart: the lobby leave differs from game 1.03.3; the leftover lobby stays"
        );
        return;
    }
    type LeaveLobby = unsafe extern "system" fn(usize, u8) -> u64;
    // SAFETY: the game's own function, first bytes checked above; the session object from the game's own slot, checked non-null; reason
    // 0 as its own callers pass; the game's thread (frame hook), where its menu's steps call it. Verified live with `rawcall DFD940`.
    let r = unsafe { std::mem::transmute::<usize, LeaveLobby>(base + RVA_LEAVE_LOBBY)(session, 0) };
    let after = mem::read::<u32>(session + SESSION_ACTIVE).unwrap_or(u32::MAX);
    tracing::info!(
        "matchstart: back from a match alone in the game's lobby (state {state}, {members} member(s)): left it (returned {r:#x}, active now {after}); the next Commence is ours"
    );
}

fn start_now(base: usize) {
    if mem::read::<[u8; 8]>(base + RVA_SEND_SETTINGS) != Some(SEND_SETTINGS_BYTES)
        || mem::read::<[u8; 8]>(base + RVA_SET_READY) != Some(SET_READY_BYTES)
        || mem::read::<[u8; 8]>(base + RVA_SET_TARGETS) != Some(SET_TARGETS_BYTES)
    {
        tracing::warn!("matchstart: the start functions differ from game 1.03.3; not called");
        return;
    }
    let Some(team) = mem::read::<usize>(base + SLOT_TEAM).filter(|t| *t != 0) else {
        tracing::warn!("matchstart: no team object; no start");
        return;
    };
    let leader = LEADER.load(Ordering::Relaxed);
    let players = PLAYERS.load(Ordering::Relaxed) as u8;
    let target = TARGET.load(Ordering::Relaxed);
    if leader {
        let list = [target];
        let begin = list.as_ptr() as usize;
        let range = (begin, begin + if target < 0 { 0 } else { 4 });
        let settings = settings_bytes(0, players, range);
        // SAFETY: the game's own functions, first bytes checked above; `team` comes from the game's own slot; `settings` holds every field
        // they read and, like `list`, outlives the calls; this is the game's thread, where the menu's own step calls them. The first
        // argument of the target function is not read.
        unsafe {
            std::mem::transmute::<usize, SetTargets>(base + RVA_SET_TARGETS)(
                0,
                settings.as_ptr().add(SET_TARGET_LIST),
                0,
            );
            std::mem::transmute::<usize, SendSettings>(base + RVA_SEND_SETTINGS)(
                team,
                settings.as_ptr(),
            );
        }
    }
    // SAFETY: as above; the second argument is a bool.
    unsafe { std::mem::transmute::<usize, SetReady>(base + RVA_SET_READY)(team, 1) };
    tracing::info!(
        "matchstart: ready sent (leader {leader}, players byte {players}, target {target})"
    );
}

/// Research: which game functions run when a player does something (console `exec_trace`). Up to three hardware execute breakpoints; each
/// hit is logged with its registers, the code addresses on the stack and the memory `rdx` points at.
#[cfg(feature = "dev")]
pub(crate) mod trace {
    use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
    use std::sync::Once;

    use windows::Win32::Foundation::STATUS_SINGLE_STEP;
    use windows::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_POINTERS,
    };

    use crate::dr::{EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH};
    use crate::game::mem;

    const IMAGE_SIZE: usize = 0x7C6_0000;
    const RING: usize = 16;
    const REGS: usize = 7;
    const RETS: usize = 10;
    const DUMP: usize = 16;
    const WORDS: usize = 1 + REGS + RETS + DUMP;

    static BASE: AtomicUsize = AtomicUsize::new(0);
    static ADDR: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
    static WRITTEN: AtomicU32 = AtomicU32::new(0);
    static READ: AtomicU32 = AtomicU32::new(0);
    static CAP: [AtomicU64; RING * WORDS] = [const { AtomicU64::new(0) }; RING * WORDS];
    static HOOK: Once = Once::new();

    unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
        // SAFETY: Windows passes a valid EXCEPTION_POINTERS; pointers are null-checked; every game read goes through `mem::read`.
        unsafe {
            if info.is_null() {
                return EXCEPTION_CONTINUE_SEARCH;
            }
            let (rec, ctx) = ((*info).ExceptionRecord, (*info).ContextRecord);
            if rec.is_null() || ctx.is_null() || (*rec).ExceptionCode != STATUS_SINGLE_STEP {
                return EXCEPTION_CONTINUE_SEARCH;
            }
            let rip = (*ctx).Rip as usize;
            if rip == 0 || !ADDR.iter().any(|a| a.load(Ordering::Relaxed) == rip) {
                return EXCEPTION_CONTINUE_SEARCH;
            }
            let c = &*ctx;
            let at = (WRITTEN.fetch_add(1, Ordering::Relaxed) as usize % RING) * WORDS;
            let base = BASE.load(Ordering::Relaxed) as u64;
            CAP[at].store(c.Rip.wrapping_sub(base), Ordering::Relaxed);
            for (i, v) in [c.Rcx, c.Rdx, c.R8, c.R9, c.Rbx, c.R14, c.Rsi]
                .into_iter()
                .enumerate()
            {
                CAP[at + 1 + i].store(v, Ordering::Relaxed);
            }
            let mut n = 0;
            if let Some(stack) = mem::read::<[u64; 96]>(c.Rsp as usize) {
                for v in stack {
                    if n < RETS && v >= base && v < base + IMAGE_SIZE as u64 {
                        CAP[at + 1 + REGS + n].store(v - base, Ordering::Relaxed);
                        n += 1;
                    }
                }
            }
            for i in n..RETS {
                CAP[at + 1 + REGS + i].store(0, Ordering::Relaxed);
            }
            let mut dump = mem::read::<[u64; DUMP]>(c.Rdx as usize).unwrap_or([0; DUMP]);
            // The settings' target list when rdx is a settings object (MM-7); else whatever is there.
            dump[DUMP - 2] = mem::read::<u64>(dump[11] as usize).unwrap_or(0);
            dump[DUMP - 1] = mem::read::<u64>(dump[11] as usize + 8).unwrap_or(0);
            for (i, v) in dump.into_iter().enumerate() {
                CAP[at + 1 + REGS + RETS + i].store(v, Ordering::Relaxed);
            }
            (*ctx).EFlags |= EFLAGS_RF;
        }
        EXCEPTION_CONTINUE_EXECUTION
    }

    /// Put an execute breakpoint on `exe+rva` in `slot`, or free the slot (`rva` 0).
    pub(crate) fn set(base: usize, slot: usize, rva: usize) -> String {
        BASE.store(base, Ordering::Relaxed);
        if rva == 0 {
            ADDR[slot].store(0, Ordering::Relaxed);
            crate::hwbp::clear(slot);
            return format!("exec_trace: slot {slot} free");
        }
        // SAFETY: `veh` has the PVECTORED_EXCEPTION_HANDLER signature and returns CONTINUE_SEARCH for anything not on its addresses.
        HOOK.call_once(|| unsafe {
            let _ = AddVectoredExceptionHandler(1, Some(veh));
        });
        ADDR[slot].store(base + rva, Ordering::Relaxed);
        crate::hwbp::set(slot, base + rva);
        format!("exec_trace: slot {slot} on exe+0x{rva:X}")
    }

    /// Game thread: log the hits.
    pub(crate) fn log_hit() {
        let w = WRITTEN.load(Ordering::Relaxed);
        let mut r = READ.load(Ordering::Relaxed);
        if r == w {
            return;
        }
        if w.wrapping_sub(r) as usize > RING {
            r = w - RING as u32;
        }
        while r != w {
            let at = (r as usize % RING) * WORDS;
            let word = |i: usize| CAP[at + i].load(Ordering::Relaxed);
            let regs: Vec<String> = (1..=REGS).map(|i| format!("{:X}", word(i))).collect();
            let rets: Vec<String> = (0..RETS)
                .map(|i| word(1 + REGS + i))
                .filter(|v| *v != 0)
                .map(|v| format!("{v:X}"))
                .collect();
            let dump: Vec<String> = (0..DUMP)
                .map(|i| format!("{:X}", word(1 + REGS + RETS + i)))
                .collect();
            tracing::info!(
                "exec_trace: exe+0x{:X} rcx,rdx,r8,r9,rbx,r14,rsi={} stack={} [rdx]={}",
                word(0),
                regs.join(","),
                rets.join(","),
                dump.join(" ")
            );
            r = r.wrapping_add(1);
        }
        READ.store(r, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_held_lobby_with_nobody_else_is_a_leftover() {
        // Live 2026-11-13: the host's leftover read active 1, state 3, one member; the client's game 0 / 0 / 0.
        assert!(alone_in_lobby(1, 3, 1));
        assert!(!alone_in_lobby(0, 0, 0), "no lobby");
        assert!(!alone_in_lobby(1, 3, 2), "the other player is still in it");
        assert!(
            !alone_in_lobby(1, 1, 1),
            "a lobby job is running (a new match's lobby)"
        );
        assert!(!alone_in_lobby(1, 8, 1), "already leaving");
        assert!(!alone_in_lobby(1, 4, 1), "busy");
    }

    #[test]
    fn a_new_lobby_job_only_from_the_states_the_game_allows() {
        for s in [0u8, 2, 6] {
            assert!(lobby_job_allowed(s), "{s}");
        }
        for s in [1u8, 3, 4, 5, 7, 8, 9] {
            assert!(!lobby_job_allowed(s), "{s}");
        }
    }

    #[test]
    fn the_target_is_the_row_of_our_card() {
        let rows = include_str!("../../../../gamedata/nightbossmenu-rows.json");
        assert!(rows.contains(&format!("\"id\": {TARGET_BR_CARD},")));
    }

    #[test]
    fn the_settings_hold_the_fields_the_game_reads() {
        let s = settings_bytes(3, 1, (0x1000, 0x1004));
        assert_eq!(s[SET_KIND], 3);
        assert_eq!(s[SET_PLAYERS], 1);
        let q = |o: usize| u64::from_le_bytes(s[o..o + 8].try_into().unwrap());
        assert_eq!(q(SET_TARGETS_BEGIN), 0x1000);
        assert_eq!(q(SET_TARGETS_END), 0x1004);
    }
}
