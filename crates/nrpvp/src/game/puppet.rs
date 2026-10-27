//! ORCH-2 (dev only): puppet players. Extra AFK characters that the game itself keeps in its **player list** (`WorldChrMan+0x9A40`,
//! `CSChrSet<PlayerIns>`), so everything that asks "who are the players" (the game's per-player code, our ChrSet readers, the team
//! and downed rules, the night match) sees them as players, in one game, with no second client.
//!
//! How (game 1.03.3, exe 1.3.3.0; static reading in .claude/research/ORCH-R2-net-player-creation.md): the game's
//! NPC bot manager (`CSNpcBotMan`, OFF-3/OFF-4) creates its Nightfarers with the generic ChrSet creator `exe+0x4ABC40(set, request)`,
//! called at `exe+0x38F2B3` with `set = WorldChrMan+0x9AF0` (the bot set). That creator builds a full `CS::PlayerIns` (constructor
//! `exe+0x6667F0`, the same class as every real player). While puppets are armed, an execute breakpoint on that call site swaps
//! `rcx` for `WorldChrMan+0x9A40`, so the game puts the new PlayerIns into the next free player slot instead. Optionally the
//! request's NpcThink id (`request+0x44`) is replaced, to leave the puppet without the bots' follow-and-fight AI (AFK). No game code
//! is patched; the breakpoint takes a free `hwbp` slot only while armed.
//!
//! Use (offline mode, in an expedition, like `bots`): `puppet_arm 1`, `bots 10` (2 puppets per request; the manager holds 2), then
//! `puppet_forget` and `bots 10` again for more (up to the 5 free player slots), `puppet_arm 0`. `puppet_list` shows the player
//! list. Log lines start with `puppet:`.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, Once, PoisonError};

use windows::Win32::Foundation::STATUS_SINGLE_STEP;
use windows::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use super::gamemem::RealMem;
use super::views::{BotMan, ChrIns};
use crate::dr::{EFLAGS_RF, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH, TARGETS};
use crate::hwbp;

/// `call exe+0x4ABC40` in the bot manager's spawn step (exe 1.3.3.0).
const RVA_CALL: usize = 0x38_F2B3;
const RVA_CREATOR: usize = 0x4A_BC40;
/// `WorldChrMan` "destroy character" (`rcx` = WorldChrMan, `rdx` = the character): unlinks it from every ChrSet and frees it.
const RVA_DESTROY: usize = 0x51_B2B0;
/// Add a SpEffect to one character: `exe+0x609F40(chr, id)` (the body of event `2004[8]`, MEMORY_MAP §17), entry bytes checked.
const RVA_ADD_SPEFFECT: usize = 0x60_9F40;
const ADD_SPEFFECT_HEAD: [u8; 6] = [0x48, 0x89, 0x74, 0x24, 0x18, 0x57];
/// Remove a SpEffect from one character: `exe+0x601210(chr, id)` (event `2004[21]`, MEMORY_MAP §17).
const RVA_DEL_SPEFFECT: usize = 0x60_1210;
const DEL_SPEFFECT_HEAD: [u8; 6] = [0x48, 0x89, 0x5C, 0x24, 0x18, 0x56];
/// `PlayerIns+0x710`: 0 up, 1 downed, 2 getting up (MEMORY_MAP §6).
const CHR_DOWNED: usize = 0x710;
/// SpEffects every new puppet gets. 8970061: with it, the player script's solo "direct death" check is false, so a puppet at 0 HP
/// goes into the downed state (purple bar, bleed-out) like a player in a session (research/M5-8-death.md, `IsDirectDeath`).
static SPEFFECTS: [AtomicI32; 4] = [
    AtomicI32::new(8_970_061),
    AtomicI32::new(0),
    AtomicI32::new(0),
    AtomicI32::new(0),
];
/// The SpEffect call chain at the creation site: index of the next id to add (0 = idle), the saved stack pointer, the character.
static CALL_NEXT: AtomicUsize = AtomicUsize::new(0);
static CALL_RSP: AtomicU64 = AtomicU64::new(0);
static CALL_CHR: AtomicU64 = AtomicU64::new(0);
static SPEFFECT_CALLS: AtomicU64 = AtomicU64::new(0);
/// Re-add the near-death SpEffect whenever a puppet has lost it (default on; `puppet_near_death 0` = one down, then real deaths).
static KEEP_NEAR_DEATH: AtomicBool = AtomicBool::new(true);
static REARMED: AtomicU64 = AtomicU64::new(0);

/// The character holds SpEffect `id` (the game's list: `[[chr+0x190]+8]`, id at node +8, next at node +0x38; MEMORY_MAP §17).
/// Bounded walk, plain reads only (runs inside the exception handler).
fn has_speffect(chr: usize, id: i32) -> bool {
    let Some(mut node) = crate::game::mem::read::<usize>(chr + 0x190)
        .and_then(|l| crate::game::mem::read::<usize>(l + 8))
    else {
        return false;
    };
    for _ in 0..256 {
        if node == 0 {
            return false;
        }
        if crate::game::mem::read::<i32>(node + 8) == Some(id) {
            return true;
        }
        node = crate::game::mem::read::<usize>(node + 0x38).unwrap_or(0);
    }
    false
}

/// `puppet_near_death [0|1]`.
pub(crate) fn near_death_cmd(on: Option<bool>) -> String {
    if let Some(on) = on {
        KEEP_NEAR_DEATH.store(on, Ordering::Relaxed);
    }
    format!(
        "puppet near-death: {} (SpEffect {} added/removed {} time(s))",
        if KEEP_NEAR_DEATH.load(Ordering::Relaxed) {
            "on (held while up, dropped while downed: every 0 HP is a down, then bleed-out)"
        } else {
            "first down only"
        },
        SPEFFECTS[0].load(Ordering::Relaxed),
        REARMED.load(Ordering::Relaxed)
    )
}
/// FD4 singleton slots (exe 1.3.3.0; MEMORY_MAP §1 and §15).
const RVA_WCM_SLOT: usize = 0x3C0_F0A8;
const RVA_BOTMAN_SLOT: usize = 0x3C0_B518;
const WCM_PLAYER_SET: usize = super::layout::WCM_CHRSET;
const WCM_BOT_SET: usize = 0x9AF0;
/// Request field the creator passes on as the NpcThink param id (the bot manager writes its table row's think id here).
const REQ_THINK: usize = 0x44;
const REQ_NPC: usize = 0x40;
const REQ_CHARA: usize = 0x48;
/// The per-player id the mod's team rules key on (`ChrIns+0x78`, MEMORY_MAP §3).
const CHR_PLAYER_ID: usize = 0x78;
/// Player list capacity under Seamless Co-op (slot 0 = LOCAL).
const PLAYER_SLOTS: usize = 6;

/// A free player slot (1..6) in the live player list. The game's creator scans 20 entries whatever the capacity, so a redirect
/// is only done when this finds one (else the bot stays a bot).
fn free_player_slot(wcm: usize) -> Option<usize> {
    let set = wcm + WCM_PLAYER_SET;
    let cap = crate::game::mem::read::<u32>(set + 0x10)? as usize;
    let ents = crate::game::mem::read::<usize>(set + 0x18)?;
    (1..cap.min(PLAYER_SLOTS)).find(|i| crate::game::mem::read::<usize>(ents + i * 16) == Some(0))
}

/// World position of a character: physics module position minus the character's moving origin (`PlayerIns+0x80`, MEMORY_MAP §8).
fn physics(chr: usize) -> Option<usize> {
    let mods = crate::game::mem::read::<usize>(chr + 0x1B8)?;
    crate::game::mem::read::<usize>(mods + 0x68).filter(|&p| p != 0)
}

/// Physics position (the frame every character in the world shares: LOCAL's moving origin, MEMORY_MAP §8).
fn phys_pos(chr: usize) -> Option<[f32; 3]> {
    let p = physics(chr)?;
    let r = |a: usize| crate::game::mem::read::<f32>(a);
    Some([r(p + 0x70)?, r(p + 0x74)?, r(p + 0x78)?])
}

/// World position = physics position minus LOCAL's origin (`LOCAL+0x80`; a puppet's own `+0x80` is not an origin: seen live).
fn world_pos(chr: usize, local: usize) -> Option<[f32; 3]> {
    let p = phys_pos(chr)?;
    let r = |a: usize| crate::game::mem::read::<f32>(a);
    Some([
        p[0] - r(local + 0x80)?,
        p[1] - r(local + 0x84)?,
        p[2] - r(local + 0x88)?,
    ])
}

/// `puppet_hp <slot> <hp>`: write a puppet's current HP (character data module, MEMORY_MAP §5; 0 = what a killing blow leaves).
pub(crate) fn hp_cmd(args: &[String]) -> Result<String, String> {
    let usage = "usage: puppet_hp <slot 1..5> <hp>";
    let slot: usize = args
        .first()
        .and_then(|v| v.parse().ok())
        .filter(|&s| (1..PLAYER_SLOTS).contains(&s))
        .ok_or(usage)?;
    let hp: u32 = args.get(1).and_then(|v| v.parse().ok()).ok_or(usage)?;
    let chr = puppet_in(slot)?;
    let data = crate::game::mem::read::<usize>(chr + super::layout::PLAYER_MODULES)
        .and_then(crate::game::mem::read::<usize>)
        .filter(|&d| d != 0)
        .ok_or("no character data module")?;
    let max = crate::game::mem::read::<u32>(data + super::layout::DATA_MODULE_MAX_HP).unwrap_or(0);
    crate::game::mem::write_bytes(
        data + super::layout::DATA_MODULE_HP,
        &hp.min(max.max(1)).to_le_bytes(),
    )
    .map_err(|e| e.to_string())?;
    tracing::info!("puppet: slot {slot} hp -> {hp} (max {max})");
    Ok(format!(
        "puppet slot {slot}: hp {} (max {max})",
        hp.min(max.max(1))
    ))
}

/// The puppet in a player slot (1..5), or why not.
fn puppet_in(slot: usize) -> Result<usize, String> {
    let b = base()?;
    let wcm = crate::game::mem::read::<usize>(b + RVA_WCM_SLOT)
        .filter(|&w| w != 0)
        .ok_or("no world")?;
    let ents = crate::game::mem::read::<usize>(wcm + WCM_PLAYER_SET + 0x18).ok_or("no list")?;
    let chr = crate::game::mem::read::<usize>(ents + slot * 16)
        .filter(|&c| c != 0)
        .ok_or("that slot is empty")?;
    if !is_puppet(chr as u64) {
        return Err("that slot is not one of our puppets".into());
    }
    Ok(chr)
}

/// `puppet_move <slot> <dx> <dz>`: put a puppet at LOCAL's world position + (dx, 0, dz) metres.
pub(crate) fn move_cmd(args: &[String]) -> Result<String, String> {
    let n = |i: usize| -> Result<f32, String> {
        args.get(i)
            .ok_or("usage: puppet_move <slot> <dx> <dz>")?
            .parse::<f32>()
            .map_err(|_| "numbers".into())
    };
    let slot = n(0)? as usize;
    let (dx, dz) = (n(1)?, n(2)?);
    let b = base()?;
    let wcm = crate::game::mem::read::<usize>(b + RVA_WCM_SLOT)
        .filter(|&w| w != 0)
        .ok_or("no world")?;
    let ents = crate::game::mem::read::<usize>(wcm + WCM_PLAYER_SET + 0x18).ok_or("no list")?;
    let local = crate::game::mem::read::<usize>(ents)
        .filter(|&c| c != 0)
        .ok_or("no LOCAL")?;
    let chr = crate::game::mem::read::<usize>(ents + slot * 16)
        .filter(|&c| c != 0 && slot > 0)
        .ok_or("no puppet in that slot")?;
    if !is_puppet(chr as u64) {
        return Err("that slot is not one of our puppets".into());
    }
    let lp = phys_pos(local).ok_or("LOCAL position unreadable")?;
    let p = physics(chr).ok_or("puppet physics unreadable")?;
    if dx.abs() > 50.0 || dz.abs() > 50.0 {
        return Err("offsets are limited to 50 m".into());
    }
    let target = [lp[0] + dx, lp[1], lp[2] + dz];
    let mut bytes = Vec::with_capacity(12);
    for v in target {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    crate::game::mem::write_bytes(p + 0x70, &bytes).map_err(|e| e.to_string())?;
    let lw = world_pos(local, local).unwrap_or_default();
    Ok(format!(
        "puppet slot {slot} moved to LOCAL + ({dx}, 0, {dz}) = world {:?}",
        [lw[0] + dx, lw[1], lw[2] + dz]
    ))
}
/// `think` value meaning "leave the request alone".
const THINK_KEEP: i32 = i32::MIN;

static ARMED: AtomicBool = AtomicBool::new(false);
static ADDR: AtomicUsize = AtomicUsize::new(0);
/// `or byte [rbx+0x1F8], 0x40` at exe+0x38F2EE, just after the bot manager stored the new character's handle in its list
/// (`[r15 + [rdi+0x28]]`): there the puppet is `rbx`, and the handle is set back to -1 so the manager forgets it at once (left
/// in its list, the manager destroys a character it cannot find in the bot set on its next update: seen live 2026-10-23).
static ADDR_RET: AtomicUsize = AtomicUsize::new(0);
const RET_OFFSET: usize = 0x3B;
/// The PlayerIns field Seamless Co-op checks (live reading of nrsc.dll 2026-10-23, read-only): for each player slot 1..5 it takes
/// `[chr+0x5D0]`, looks for its `+8` key among SC's session members, and destroys the character when there is no match; a null
/// field is skipped. 0 = leave it, 1 = null it (SC skips the puppet), 2 = point it at LOCAL's session object.
const CHR_SESSION: usize = 0x5D0;
static SESSION_MODE: AtomicU32 = AtomicU32::new(0);
static LAST_SESSION: AtomicU64 = AtomicU64::new(0);
static LAST_SESSION_VT: AtomicU64 = AtomicU64::new(0);
static LAST_SESSION_KEY: AtomicU64 = AtomicU64::new(0);
/// Seamless Co-op's per-frame sweep destroys every player-slot character it cannot match to one of its session members, through
/// the game's destroy function. While puppets are armed, an execute breakpoint on that function returns at once when the caller is
/// inside nrsc.dll and the character is one of ours; every other destroy (the game's own, area loads) runs normally. SC's code
/// and data are not touched.
static GUARD_SLOT: Mutex<Option<usize>> = Mutex::new(None);
static ADDR_DESTROY: AtomicUsize = AtomicUsize::new(0);
static SC_LO: AtomicUsize = AtomicUsize::new(0);
static SC_HI: AtomicUsize = AtomicUsize::new(0);
static GUARDED: AtomicU64 = AtomicU64::new(0);
static PASSED: AtomicU64 = AtomicU64::new(0);
/// Our puppets (character addresses; 0 = free).
static PUPPETS: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

fn is_puppet(chr: u64) -> bool {
    chr != 0 && PUPPETS.iter().any(|p| p.load(Ordering::Relaxed) == chr)
}

/// The current call was redirected (set at the call site, used at [`ADDR_RET`]).
static PENDING: AtomicBool = AtomicBool::new(false);
static RET_SLOT: Mutex<Option<usize>> = Mutex::new(None);
/// The last characters the creator returned while armed (ring of 8).
static CREATED: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
static CREATED_N: AtomicU64 = AtomicU64::new(0);
/// Research: a 4-byte write watch (dev `puppet_watch`) and what wrote there: (rip, the watched value after, up to 6 exe return
/// addresses found on the stack).
static WATCH_SLOT: AtomicUsize = AtomicUsize::new(usize::MAX);
static WATCH_ADDR: AtomicUsize = AtomicUsize::new(0);
static WATCH_N: AtomicU64 = AtomicU64::new(0);
static WATCH_LOG: [[AtomicU64; 8]; 16] = [const { [const { AtomicU64::new(0) }; 8] }; 16];
/// Execute-breakpoint mode of the research watch: log rcx, rdx and the return address at `WATCH_ADDR`.
static WATCH_EXEC: AtomicBool = AtomicBool::new(false);
static BASE: AtomicUsize = AtomicUsize::new(0);
static SLOT: Mutex<Option<usize>> = Mutex::new(None);
/// NpcThink id for puppets; default -1 (no think param: no AI) [HYPOTHESIS until the first live run].
static THINK: AtomicI32 = AtomicI32::new(-1);
static HITS: AtomicU64 = AtomicU64::new(0);
/// At most this many puppets (`puppet_max`); a creation beyond it is skipped (the call returns null, which the bot manager handles
/// as a failed spawn), so no stray bot appears either.
static MAX: AtomicUsize = AtomicUsize::new(5);
static SKIPPED: AtomicU64 = AtomicU64::new(0);
static REDIRECTED: AtomicU64 = AtomicU64::new(0);
static LAST_NPC: AtomicI32 = AtomicI32::new(0);
static LAST_THINK: AtomicI32 = AtomicI32::new(0);
static LAST_CHARA: AtomicI32 = AtomicI32::new(0);
static LAST_RCX: AtomicU64 = AtomicU64::new(0);
static LAST_WCM: AtomicU64 = AtomicU64::new(0);
static TID: AtomicU32 = AtomicU32::new(0);
static VEH: Once = Once::new();

/// nrsc.dll's image range.
fn sc_range() -> Option<(usize, usize)> {
    use windows::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
    use windows::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: looking up a loaded module by name and reading its size; no preconditions.
    unsafe {
        let h = GetModuleHandleW(windows::core::w!("nrsc.dll")).ok()?;
        let mut mi = MODULEINFO::default();
        GetModuleInformation(
            GetCurrentProcess(),
            h,
            &mut mi,
            std::mem::size_of::<MODULEINFO>() as u32,
        )
        .ok()?;
        Some((
            mi.lpBaseOfDll as usize,
            mi.lpBaseOfDll as usize + mi.SizeOfImage as usize,
        ))
    }
}

/// The exe base (also for `teleport.rs`).
pub(crate) fn exe_base() -> Result<usize, String> {
    base()
}

fn base() -> Result<usize, String> {
    let b = BASE.load(Ordering::Relaxed);
    if b != 0 {
        return Ok(b);
    }
    // SAFETY: the exe's own module handle; no preconditions.
    let b = unsafe { GetModuleHandleW(None) }
        .map_err(|e| e.to_string())?
        .0 as usize;
    BASE.store(b, Ordering::Relaxed);
    Ok(b)
}

/// The call site, if the bytes are the ones this module was written for (a different build stays passive).
fn call_site() -> Result<usize, String> {
    let b = base()?;
    let a = b + RVA_CALL;
    let op = crate::game::mem::read::<u8>(a).ok_or("call site unreadable")?;
    let rel = crate::game::mem::read::<i32>(a + 1).ok_or("call site unreadable")?;
    let target = (a as i64 + 5 + rel as i64) as usize;
    if op != 0xE8 || target != b + RVA_CREATOR {
        return Err(format!(
            "exe+0x{RVA_CALL:X} is not `call exe+0x{RVA_CREATOR:X}` (op {op:#04X}, target exe+0x{:X}): another game build",
            target.wrapping_sub(b)
        ));
    }
    Ok(a)
}

static AV_N: AtomicU64 = AtomicU64::new(0);

/// Research (armed with the puppets): the first 12 access violations are appended to `puppet-av.log` in the working folder (the
/// game folder), so a crash after a puppet change names its instruction. First chance only; returns CONTINUE_SEARCH always.
unsafe extern "system" fn av_veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes valid pointers (null-checked). Writing a file here allocates; acceptable for a research logger that
    // only runs on access violations, at most 12 times.
    unsafe {
        let rec = (*info).ExceptionRecord;
        let ctx = (*info).ContextRecord;
        if rec.is_null() || ctx.is_null() || (*rec).ExceptionCode.0 as u32 != 0xC000_0005 {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        if AV_N.fetch_add(1, Ordering::Relaxed) >= 12 {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let b = BASE.load(Ordering::Relaxed) as u64;
        let c = &*ctx;
        let rel = |v: u64| {
            if v >= b && v < b + 0x0800_0000 {
                format!("exe+0x{:X}", v - b)
            } else {
                format!("0x{v:X}")
            }
        };
        let line = format!(
            "AV rip {} addr 0x{:X} (kind {}) rax 0x{:X} rbx 0x{:X} rcx 0x{:X} rdx 0x{:X} rsi 0x{:X} rdi 0x{:X} [rsp] {}
",
            rel(c.Rip),
            (*rec).ExceptionInformation[1],
            (*rec).ExceptionInformation[0],
            c.Rax, c.Rbx, c.Rcx, c.Rdx, c.Rsi, c.Rdi,
            rel(crate::game::mem::read::<u64>(c.Rsp as usize).unwrap_or(0))
        );
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("puppet-av.log")
        {
            let _ = std::io::Write::write_all(&mut f, line.as_bytes());
        }
    }
    EXCEPTION_CONTINUE_SEARCH
}

/// Make the thread call `exe+0x609F40(chr, SPEFFECTS[k-1])` and come back to the creation-site instruction (where only
/// non-volatile registers are live, so nothing else must be saved): RSP = saved − 0x28 (fresh shadow space, entry alignment),
/// [RSP] = the instruction, RIP = the function. Returns false when there is nothing (left) to add.
///
/// # Safety
/// `ctx` is the CONTEXT of the thread stopped at the creation site with the saved RSP in [`CALL_RSP`].
unsafe fn start_speffect_call(
    ctx: *mut windows::Win32::System::Diagnostics::Debug::CONTEXT,
    k: usize,
) -> bool {
    let b = BASE.load(Ordering::Relaxed);
    let f = b + RVA_ADD_SPEFFECT;
    if k > SPEFFECTS.len() || crate::game::mem::read::<[u8; 6]>(f) != Some(ADD_SPEFFECT_HEAD) {
        return false;
    }
    let id = SPEFFECTS[k - 1].load(Ordering::Relaxed);
    if id <= 0 {
        return false;
    }
    // SAFETY: (caller) a live thread context; the new stack slot is below the frame in use.
    unsafe {
        let rsp = CALL_RSP.load(Ordering::Relaxed) - 0x28;
        std::ptr::write_volatile(rsp as *mut u64, (*ctx).Rip);
        (*ctx).Rsp = rsp;
        (*ctx).Rcx = CALL_CHR.load(Ordering::Relaxed);
        (*ctx).Rdx = id as u32 as u64;
        (*ctx).Rip = f as u64;
    }
    CALL_NEXT.store(k + 1, Ordering::Relaxed);
    SPEFFECT_CALLS.fetch_add(1, Ordering::Relaxed);
    true
}

unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS (null-checked). At the call site `rcx` is the ChrSet the bot manager is
    // about to create into and `rdx` its request (a stack object of the bot manager's frame); only `rcx` and the request's think id
    // are changed, and only when `rcx` is exactly the bot set of the live WorldChrMan. Nothing allocates, logs or locks here.
    unsafe {
        let rec = (*info).ExceptionRecord;
        let ctx = (*info).ContextRecord;
        if rec.is_null() || ctx.is_null() || (*rec).ExceptionCode != STATUS_SINGLE_STEP {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let addr = ADDR.load(Ordering::Relaxed);
        let rip = (*ctx).Rip as usize;
        if rip != 0 && rip == ADDR_DESTROY.load(Ordering::Relaxed) {
            let ret = crate::game::mem::read::<u64>((*ctx).Rsp as usize).unwrap_or(0) as usize;
            let from_sc =
                ret >= SC_LO.load(Ordering::Relaxed) && ret < SC_HI.load(Ordering::Relaxed);
            if from_sc && ARMED.load(Ordering::Relaxed) && is_puppet((*ctx).Rdx) {
                GUARDED.fetch_add(1, Ordering::Relaxed);
                // SC's sweep runs about once per frame per puppet on the game thread: also the place to keep the near-death
                // SpEffect on it (the first down consumes it). Missing → this call becomes add_speffect(chr, id), a tail call
                // with SC's own return address; present → return at once (pop the return address), as if destroyed.
                // While up the puppet holds the effect (a 0 HP hit downs it instead of killing it); while downed it must not
                // (the effect is also the solo auto-revive, which would stand it up after ~5 s; a session player bleeds out).
                let chr = (*ctx).Rdx as usize;
                let id = SPEFFECTS[0].load(Ordering::Relaxed);
                let b = BASE.load(Ordering::Relaxed);
                let state = crate::game::mem::read::<u8>(chr + CHR_DOWNED).unwrap_or(0);
                let has = has_speffect(chr, id);
                let call = match (state, has) {
                    (0, false) => Some((b + RVA_ADD_SPEFFECT, ADD_SPEFFECT_HEAD)),
                    (1, true) => Some((b + RVA_DEL_SPEFFECT, DEL_SPEFFECT_HEAD)),
                    _ => None,
                };
                if let Some((f, head)) =
                    call.filter(|_| KEEP_NEAR_DEATH.load(Ordering::Relaxed) && id > 0)
                {
                    if crate::game::mem::read::<[u8; 6]>(f) == Some(head) {
                        (*ctx).Rcx = chr as u64;
                        (*ctx).Rdx = id as u32 as u64;
                        (*ctx).Rip = f as u64;
                        REARMED.fetch_add(1, Ordering::Relaxed);
                        return EXCEPTION_CONTINUE_EXECUTION;
                    }
                }
                (*ctx).Rip = ret as u64;
                (*ctx).Rsp += 8;
                return EXCEPTION_CONTINUE_EXECUTION;
            }
            if is_puppet((*ctx).Rdx) {
                // Destroyed for real (the game's own reason): forget it.
                for p in &PUPPETS {
                    let _ = p.compare_exchange((*ctx).Rdx, 0, Ordering::Relaxed, Ordering::Relaxed);
                }
            }
            PASSED.fetch_add(1, Ordering::Relaxed);
            (*ctx).EFlags |= EFLAGS_RF;
            return EXCEPTION_CONTINUE_EXECUTION;
        }
        let ws = WATCH_SLOT.load(Ordering::Relaxed);
        if WATCH_EXEC.load(Ordering::Relaxed)
            && rip != 0
            && rip == WATCH_ADDR.load(Ordering::Relaxed)
        {
            let n = WATCH_N.fetch_add(1, Ordering::Relaxed);
            let row = &WATCH_LOG[(n % 16) as usize];
            row[0].store(rip as u64, Ordering::Relaxed);
            row[1].store((*ctx).Rdx, Ordering::Relaxed);
            row[2].store((*ctx).Rcx, Ordering::Relaxed);
            row[3].store((*ctx).R15, Ordering::Relaxed);
            let rsp = (*ctx).Rsp as usize;
            let b = BASE.load(Ordering::Relaxed) as u64;
            row[4].store(
                crate::game::mem::read::<u64>(rsp).unwrap_or(0),
                Ordering::Relaxed,
            );
            // SC's member list, when this is its slot check: o = [r15+0x18], m = [o+0x28], vector [m]..[m+8].
            let o = crate::game::mem::read::<u64>((*ctx).R15 as usize + 0x18).unwrap_or(0);
            let m = crate::game::mem::read::<u64>(o as usize + 0x28).unwrap_or(0);
            row[5].store(o, Ordering::Relaxed);
            row[6].store(
                crate::game::mem::read::<u64>(m as usize).unwrap_or(0),
                Ordering::Relaxed,
            );
            row[7].store(
                crate::game::mem::read::<u64>(m as usize + 8).unwrap_or(0),
                Ordering::Relaxed,
            );
            let mut k = 8;
            for i in 1..200 {
                if k >= 8 {
                    break;
                }
                let v = crate::game::mem::read::<u64>(rsp + i * 8).unwrap_or(0);
                if v > b + 0x1000 && v < b + 0x0500_0000 {
                    row[k].store(v, Ordering::Relaxed);
                    k += 1;
                }
            }
            (*ctx).EFlags |= EFLAGS_RF;
            return EXCEPTION_CONTINUE_EXECUTION;
        }
        if !WATCH_EXEC.load(Ordering::Relaxed)
            && ws < TARGETS
            && (*ctx).Dr6 & (1 << ws) != 0
            && WATCH_ADDR.load(Ordering::Relaxed) != 0
        {
            let n = WATCH_N.fetch_add(1, Ordering::Relaxed);
            let row = &WATCH_LOG[(n % 16) as usize];
            row[0].store(rip as u64, Ordering::Relaxed);
            row[1].store(
                crate::game::mem::read::<u64>(WATCH_ADDR.load(Ordering::Relaxed) & !7).unwrap_or(0),
                Ordering::Relaxed,
            );
            let b = BASE.load(Ordering::Relaxed) as u64;
            let mut k = 2;
            let rsp = (*ctx).Rsp as usize;
            for i in 0..256 {
                if k >= 8 {
                    break;
                }
                let v = crate::game::mem::read::<u64>(rsp + i * 8).unwrap_or(0);
                if v > b + 0x1000 && v < b + 0x0500_0000 {
                    row[k].store(v, Ordering::Relaxed);
                    k += 1;
                }
            }
            (*ctx).Dr6 = 0;
            (*ctx).EFlags |= EFLAGS_RF;
            return EXCEPTION_CONTINUE_EXECUTION;
        }
        if rip != 0 && rip == ADDR_RET.load(Ordering::Relaxed) {
            // Back from an injected SpEffect call (it returns to this same instruction): put the stack back, then the next id.
            let next = CALL_NEXT.load(Ordering::Relaxed);
            if next != 0 {
                (*ctx).Rsp = CALL_RSP.load(Ordering::Relaxed);
                if !start_speffect_call(ctx, next) {
                    CALL_NEXT.store(0, Ordering::Relaxed);
                    (*ctx).EFlags |= EFLAGS_RF;
                }
                return EXCEPTION_CONTINUE_EXECUTION;
            }
            if PENDING.swap(false, Ordering::Relaxed) {
                let n = CREATED_N.fetch_add(1, Ordering::Relaxed);
                CREATED[(n % 8) as usize].store((*ctx).Rbx, Ordering::Relaxed);
                if let Some(off) = crate::game::mem::read::<usize>((*ctx).Rdi as usize + 0x28) {
                    std::ptr::write_volatile(((*ctx).R15 as usize + off) as *mut u64, u64::MAX);
                }
                let chr = (*ctx).Rbx as usize;
                if let Some(free) = PUPPETS.iter().find(|p| p.load(Ordering::Relaxed) == 0) {
                    free.store(chr as u64, Ordering::Relaxed);
                }
                // A player id like a joiner's: 0x600N_FFFF_FFFF for slot N-1 (LOCAL in slot 0 is 0x6001...).
                let wcm =
                    crate::game::mem::read::<usize>(BASE.load(Ordering::Relaxed) + RVA_WCM_SLOT)
                        .unwrap_or(0);
                let ents =
                    crate::game::mem::read::<usize>(wcm + WCM_PLAYER_SET + 0x18).unwrap_or(0);
                if let Some(slot) =
                    (1..6).find(|i| crate::game::mem::read::<usize>(ents + i * 16) == Some(chr))
                {
                    std::ptr::write_volatile(
                        (chr + CHR_PLAYER_ID) as *mut u64,
                        ((0x6001 + slot as u64) << 32) | 0xFFFF_FFFF,
                    );
                }
                let sess = crate::game::mem::read::<u64>(chr + CHR_SESSION).unwrap_or(0);
                LAST_SESSION.store(sess, Ordering::Relaxed);
                LAST_SESSION_VT.store(
                    crate::game::mem::read::<u64>(sess as usize).unwrap_or(0),
                    Ordering::Relaxed,
                );
                LAST_SESSION_KEY.store(
                    crate::game::mem::read::<u64>(sess as usize + 8).unwrap_or(0),
                    Ordering::Relaxed,
                );
                CALL_CHR.store(chr as u64, Ordering::Relaxed);
                CALL_RSP.store((*ctx).Rsp, Ordering::Relaxed);
                if start_speffect_call(ctx, 1) {
                    // The rest of this handler (session mode) is not needed for the default mode; the call runs first.
                    return EXCEPTION_CONTINUE_EXECUTION;
                }
                match SESSION_MODE.load(Ordering::Relaxed) {
                    1 => std::ptr::write_volatile((chr + CHR_SESSION) as *mut u64, 0),
                    2 => {
                        let wcm = crate::game::mem::read::<usize>(
                            BASE.load(Ordering::Relaxed) + RVA_WCM_SLOT,
                        )
                        .unwrap_or(0);
                        let ents = crate::game::mem::read::<usize>(wcm + WCM_PLAYER_SET + 0x18)
                            .unwrap_or(0);
                        let local = crate::game::mem::read::<usize>(ents).unwrap_or(0);
                        if let Some(ls) =
                            crate::game::mem::read::<u64>(local + CHR_SESSION).filter(|&v| v != 0)
                        {
                            std::ptr::write_volatile((chr + CHR_SESSION) as *mut u64, ls);
                        }
                    }
                    _ => {}
                }
            }
            (*ctx).EFlags |= EFLAGS_RF;
            return EXCEPTION_CONTINUE_EXECUTION;
        }
        if addr == 0 || rip != addr {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        HITS.fetch_add(1, Ordering::Relaxed);
        TID.store(
            windows::Win32::System::Threading::GetCurrentThreadId(),
            Ordering::Relaxed,
        );
        let req = (*ctx).Rdx as usize;
        LAST_RCX.store((*ctx).Rcx, Ordering::Relaxed);
        let wcm = crate::game::mem::read::<usize>(BASE.load(Ordering::Relaxed) + RVA_WCM_SLOT)
            .unwrap_or(0);
        LAST_WCM.store(wcm as u64, Ordering::Relaxed);
        if let (Some(n), Some(t), Some(c)) = (
            crate::game::mem::read::<i32>(req + REQ_NPC),
            crate::game::mem::read::<i32>(req + REQ_THINK),
            crate::game::mem::read::<i32>(req + REQ_CHARA),
        ) {
            LAST_NPC.store(n, Ordering::Relaxed);
            LAST_THINK.store(t, Ordering::Relaxed);
            LAST_CHARA.store(c, Ordering::Relaxed);
        }
        let live = PUPPETS
            .iter()
            .filter(|p| p.load(Ordering::Relaxed) != 0)
            .count();
        if ARMED.load(Ordering::Relaxed)
            && wcm != 0
            && (*ctx).Rcx as usize == wcm + WCM_BOT_SET
            && live >= MAX.load(Ordering::Relaxed)
        {
            // Skip the call: RAX = 0 (no character), continue after it.
            (*ctx).Rax = 0;
            (*ctx).Rip = addr as u64 + 5;
            SKIPPED.fetch_add(1, Ordering::Relaxed);
            return EXCEPTION_CONTINUE_EXECUTION;
        }
        if ARMED.load(Ordering::Relaxed)
            && wcm != 0
            && (*ctx).Rcx as usize == wcm + WCM_BOT_SET
            && free_player_slot(wcm).is_some()
        {
            (*ctx).Rcx = (wcm + WCM_PLAYER_SET) as u64;
            let think = THINK.load(Ordering::Relaxed);
            if think != THINK_KEEP {
                std::ptr::write_volatile((req + REQ_THINK) as *mut i32, think);
            }
            REDIRECTED.fetch_add(1, Ordering::Relaxed);
            PENDING.store(true, Ordering::Relaxed);
        }
        (*ctx).EFlags |= EFLAGS_RF;
    }
    EXCEPTION_CONTINUE_EXECUTION
}

/// `puppet_arm [0|1]`.
pub(crate) fn arm_cmd(on: Option<bool>) -> String {
    if let Some(on) = on {
        if let Err(e) = arm(on) {
            return format!("puppet: {e}");
        }
    }
    status()
}

fn arm(on: bool) -> Result<(), String> {
    let mut slot = SLOT.lock().unwrap_or_else(PoisonError::into_inner);
    let mut ret_slot = RET_SLOT.lock().unwrap_or_else(PoisonError::into_inner);
    if !on {
        ARMED.store(false, Ordering::Relaxed);
        let g = GUARD_SLOT
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        for s in [slot.take(), ret_slot.take(), g].into_iter().flatten() {
            hwbp::clear(s);
        }
        tracing::info!("puppet: disarmed");
        return Ok(());
    }
    let a = call_site()?;
    ADDR.store(a, Ordering::Relaxed);
    VEH.call_once(|| {
        // SAFETY: registering vectored handlers with valid function pointers.
        if unsafe { AddVectoredExceptionHandler(1, Some(veh)) }.is_null() {
            tracing::warn!("puppet: AddVectoredExceptionHandler failed");
        }
        let _ = unsafe { AddVectoredExceptionHandler(1, Some(av_veh)) };
    });
    if slot.is_none() {
        let s = (0..TARGETS)
            .rev()
            .find(|&s| hwbp::is_free(s))
            .ok_or("no free hardware breakpoint slot")?;
        hwbp::set(s, a);
        *slot = Some(s);
    }
    let mut guard = GUARD_SLOT.lock().unwrap_or_else(PoisonError::into_inner);
    if guard.is_none() {
        let sc = sc_range().ok_or("nrsc.dll (Seamless Co-op) is not loaded")?;
        SC_LO.store(sc.0, Ordering::Relaxed);
        SC_HI.store(sc.1, Ordering::Relaxed);
        let s = (0..TARGETS)
            .rev()
            .find(|&s| hwbp::is_free(s))
            .ok_or("puppets need three free hardware breakpoint slots")?;
        ADDR_DESTROY.store(base()? + RVA_DESTROY, Ordering::Relaxed);
        hwbp::set(s, base()? + RVA_DESTROY);
        *guard = Some(s);
    }
    if ret_slot.is_none() {
        let s = (0..TARGETS)
            .rev()
            .find(|&s| hwbp::is_free(s))
            .ok_or("puppets need three free hardware breakpoint slots")?;
        ADDR_RET.store(a + RET_OFFSET, Ordering::Relaxed);
        hwbp::set(s, a + RET_OFFSET);
        *ret_slot = Some(s);
    }
    ARMED.store(true, Ordering::Relaxed);
    tracing::info!(
        "puppet: armed (breakpoint slot {:?} on exe+0x{RVA_CALL:X}, think {})",
        *slot,
        THINK.load(Ordering::Relaxed)
    );
    Ok(())
}

/// `puppet_think <id|keep>`.
pub(crate) fn think_cmd(arg: Option<&str>) -> Result<String, String> {
    if let Some(a) = arg {
        let v = if a.eq_ignore_ascii_case("keep") {
            THINK_KEEP
        } else {
            a.parse::<i32>().map_err(|_| "id or keep")?
        };
        THINK.store(v, Ordering::Relaxed);
        tracing::info!("puppet: think set to {a}");
    }
    Ok(status())
}

/// `puppet_max [n]` (0..=5).
pub(crate) fn max_cmd(arg: Option<&str>) -> Result<String, String> {
    if let Some(a) = arg {
        let n: usize = a.parse().ok().filter(|&n| n < PLAYER_SLOTS).ok_or("0..5")?;
        MAX.store(n, Ordering::Relaxed);
    }
    Ok(format!(
        "puppet max {}; creations skipped at the cap {}",
        MAX.load(Ordering::Relaxed),
        SKIPPED.load(Ordering::Relaxed)
    ))
}

/// `puppet_session [keep|null|local]`.
pub(crate) fn session_cmd(arg: Option<&str>) -> Result<String, String> {
    if let Some(a) = arg {
        SESSION_MODE.store(
            match a {
                "keep" => 0,
                "null" => 1,
                "local" => 2,
                _ => return Err("keep, null or local".into()),
            },
            Ordering::Relaxed,
        );
    }
    let b = BASE.load(Ordering::Relaxed) as u64;
    Ok(format!(
        "puppet session mode {}; last puppet's original +0x5D0 0x{:X} (vtable exe+0x{:X}, key 0x{:X})",
        ["keep", "null", "local"][SESSION_MODE.load(Ordering::Relaxed).min(2) as usize],
        LAST_SESSION.load(Ordering::Relaxed),
        LAST_SESSION_VT.load(Ordering::Relaxed).wrapping_sub(b),
        LAST_SESSION_KEY.load(Ordering::Relaxed)
    ))
}

/// `puppet_forget`: the bot manager forgets the characters it holds (they stay in the world), so the next `bots` request spawns
/// two more (the OFF-5 detach, MEMORY_MAP §15).
pub(crate) fn forget_cmd() -> String {
    let Ok(b) = base() else {
        return "puppet: no exe base".into();
    };
    let Some(man) = crate::game::mem::read::<usize>(b + RVA_BOTMAN_SLOT).filter(|&m| m != 0) else {
        return "puppet: no bot manager (no world loaded?)".into();
    };
    let mem = RealMem;
    let bm = BotMan::new(&mem, man);
    let r: Vec<String> = (0..2)
        .map(|s| format!("slot {s}: {:?}", bm.forget_slot(s)))
        .collect();
    tracing::info!("puppet: forget {}", r.join(", "));
    format!("puppet: bot manager forgot its bots ({})", r.join(", "))
}

/// One line per player-list entry.
pub(crate) fn list() -> String {
    let Ok(b) = base() else {
        return "puppet: no exe base".into();
    };
    let Some(wcm) = crate::game::mem::read::<usize>(b + RVA_WCM_SLOT).filter(|&w| w != 0) else {
        return "no world (WorldChrMan is not live)".into();
    };
    let set = wcm + WCM_PLAYER_SET;
    let cap = crate::game::mem::read::<u32>(set + 0x10).unwrap_or(0) as usize;
    let entries = crate::game::mem::read::<usize>(set + 0x18).unwrap_or(0);
    let mem = RealMem;
    let mut out = vec![format!(
        "player list WorldChrMan+0x{WCM_PLAYER_SET:X}: {cap} slot(s)"
    )];
    let mut n = 0;
    let local0 = crate::game::mem::read::<usize>(entries).unwrap_or(0);
    for i in 0..cap.min(16) {
        let chr = crate::game::mem::read::<usize>(entries + i * 16).unwrap_or(0);
        let tail = crate::game::mem::read::<u64>(entries + i * 16 + 8).unwrap_or(0);
        if chr == 0 {
            if tail != 0 {
                out.push(format!("  slot {i}: empty, entry tail {tail:#018X}"));
            }
            continue;
        }
        n += 1;
        let c = ChrIns::new(&mem, chr);
        let vt = crate::game::mem::read::<usize>(chr)
            .unwrap_or(0)
            .wrapping_sub(b);
        let kind = if i == 0 {
            "LOCAL"
        } else if is_puppet(chr as u64) {
            "PUPPET"
        } else {
            "player"
        };
        out.push(format!(
            "  slot {i}: 0x{chr:X} {kind} vtable exe+0x{vt:X} handle {:#X} id {:#X} npc {} team {} hp {} downed {} pos {} near-death {} map id {} (entry tail {tail:#018X})",
            c.handle().unwrap_or(0),
            crate::game::mem::read::<u64>(chr + CHR_PLAYER_ID).unwrap_or(0),
            c.npc_id().map_or("?".into(), |v| v.to_string()),
            c.team().map_or("?".into(), |v| v.to_string()),
            c.hp().map_or("?".into(), |v| v.to_string()),
            c.downed_state().map_or("?".into(), |v| v.to_string()),
            world_pos(chr, local0).map_or("?".into(), |p| format!("({:.1}, {:.1}, {:.1})", p[0], p[1], p[2])),
            if c.holds_any_speffect(&[8_970_061]) { "on" } else { "off" },
            // RW-9: the id the map code matches against the remote slots of GameDataMan ([[chr+0x5A0]+8]).
            crate::game::mem::read::<usize>(chr + 0x5A0)
                .filter(|p| *p != 0)
                .and_then(|p| crate::game::mem::read::<u64>(p + 8))
                .map_or("?".into(), |v| format!("{v:#X}")),
        ));
    }
    out.push(format!("{n} filled"));
    out.join("\n")
}

pub(crate) fn status() -> String {
    let slot = *SLOT.lock().unwrap_or_else(PoisonError::into_inner);
    let think = THINK.load(Ordering::Relaxed);
    format!(
        "puppet: {} (slot {slot:?}), think {}; speffects {:?} ({} added); puppets [{}]; SC destroys skipped {}, other destroys {}; call-site hits {} (thread {}), redirected {}; last request npc {} think {} chara {}; last rcx 0x{:X} (wcm 0x{:X}); created {}",
        if ARMED.load(Ordering::Relaxed) { "ARMED" } else { "off" },
        if think == THINK_KEEP { "keep".to_string() } else { think.to_string() },
        SPEFFECTS.iter().map(|x| x.load(Ordering::Relaxed)).filter(|&x| x > 0).collect::<Vec<_>>(),
        SPEFFECT_CALLS.load(Ordering::Relaxed),
        PUPPETS.iter().map(|p| p.load(Ordering::Relaxed)).filter(|&p| p != 0).map(|p| format!("0x{p:X}")).collect::<Vec<_>>().join(" "),
        GUARDED.load(Ordering::Relaxed),
        PASSED.load(Ordering::Relaxed),
        HITS.load(Ordering::Relaxed),
        TID.load(Ordering::Relaxed),
        REDIRECTED.load(Ordering::Relaxed),
        LAST_NPC.load(Ordering::Relaxed),
        LAST_THINK.load(Ordering::Relaxed),
        LAST_CHARA.load(Ordering::Relaxed),
        LAST_RCX.load(Ordering::Relaxed),
        LAST_WCM.load(Ordering::Relaxed),
        {
            let n = CREATED_N.load(Ordering::Relaxed);
            (n.saturating_sub(8)..n).map(|i| format!("0x{:X}", CREATED[(i % 8) as usize].load(Ordering::Relaxed))).collect::<Vec<_>>().join(" ")
        },
    )
}

/// `peek <addr|wcm+off|exe+off> [len]`: hex dump (read-only, dev). `wcm` = the live WorldChrMan instance, `exe` = the exe base.
/// `wcm+0x..`, `exe+0x..`, `me+0x..` (LOCAL's PlayerIns), `[0xoff]` (dereference at the running sum + offset) or a number; parts joined by `+`.
fn parse_addr(spec: &str) -> Result<usize, String> {
    let b = base()?;
    let num = |t: &str| -> Result<usize, String> {
        let t = t.trim();
        let (t, r) = t
            .strip_prefix("0x")
            .or(t.strip_prefix("0X"))
            .map_or((t, 10), |x| (x, 16));
        usize::from_str_radix(t, r).map_err(|_| format!("bad number {t:?}"))
    };
    let mut addr = 0usize;
    for part in spec.split('+') {
        addr = addr.wrapping_add(match part.to_ascii_lowercase().as_str() {
            "wcm" => crate::game::mem::read::<usize>(b + RVA_WCM_SLOT).ok_or("no WorldChrMan")?,
            "me" => {
                let wcm =
                    crate::game::mem::read::<usize>(b + RVA_WCM_SLOT).ok_or("no WorldChrMan")?;
                let entries = crate::game::mem::read::<usize>(wcm + WCM_PLAYER_SET + 0x18)
                    .ok_or("no player set")?;
                crate::game::mem::read::<usize>(entries)
                    .filter(|&p| p != 0)
                    .ok_or("no local player")?
            }
            "exe" => b,
            p if p.starts_with('[') && p.ends_with(']') => {
                let inner = num(&p[1..p.len() - 1])?;
                crate::game::mem::read::<usize>(addr.wrapping_add(inner))
                    .ok_or("unreadable pointer")?
                    .wrapping_sub(addr)
            }
            _ => num(part)?,
        });
    }
    Ok(addr)
}

/// `poke <addr|me+0x..|wcm+0x..|exe+0x..> <hex bytes, at most 16>` (research, dev): write bytes into game memory (guarded by `mem::write_bytes`).
pub(crate) fn poke_cmd(args: &[String]) -> Result<String, String> {
    let usage = "usage: poke <addr|me+0x..|wcm+0x..|exe+0x..> <hex bytes, at most 16>";
    let addr = parse_addr(args.first().ok_or(usage)?)?;
    let hex: String = args[1..].join("");
    let bytes: Vec<u8> = hex
        .as_bytes()
        .chunks(2)
        .map(|c| {
            std::str::from_utf8(c)
                .ok()
                .and_then(|t| u8::from_str_radix(t, 16).ok())
                .ok_or(usage)
        })
        .collect::<Result<_, _>>()?;
    let mut before = vec![0u8; bytes.len()];
    if !crate::game::mem::copy_out(addr, &mut before) {
        return Err(format!("{addr:#X} is not readable"));
    }
    crate::game::mem::write_bytes(addr, &bytes).map_err(|e| e.to_string())?;
    let fmt = |b: &[u8]| {
        b.iter()
            .map(|x| format!("{x:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let line = format!("poke {addr:#X}: {} -> {}", fmt(&before), fmt(&bytes));
    tracing::info!("research {line}");
    Ok(line)
}

pub(crate) fn peek_cmd(args: &[String]) -> Result<String, String> {
    let spec = args
        .first()
        .ok_or("usage: peek <addr|wcm+0x..|exe+0x..> [len]")?;
    let addr = parse_addr(spec)?;
    let num = |t: &str| -> Result<usize, String> {
        let t = t.trim();
        let (t, r) = t
            .strip_prefix("0x")
            .or(t.strip_prefix("0X"))
            .map_or((t, 10), |x| (x, 16));
        usize::from_str_radix(t, r).map_err(|_| format!("bad number {t:?}"))
    };
    let len = args
        .get(1)
        .map(|l| num(l))
        .transpose()?
        .unwrap_or(64)
        .min(1024);
    let bytes = crate::game::mem::copy_guarded(addr, len).0;
    let mut out = vec![format!(
        "0x{addr:X} ({} of {len} bytes readable)",
        bytes.len()
    )];
    for (i, row) in bytes.chunks(16).enumerate() {
        let q: Vec<String> = row
            .chunks(8)
            .map(|c| {
                let mut v = [0u8; 8];
                v[..c.len()].copy_from_slice(c);
                format!("{:016X}", u64::from_le_bytes(v))
            })
            .collect();
        out.push(format!("+{:03X}: {}", i * 16, q.join(" ")));
    }
    Ok(out.join(
        "
",
    ))
}

/// `puppet_watch <addr|off>`: research write watch (4 bytes) on an address, `slot1` = the player list's entry 1, `off` = remove.
/// `puppet_watch` alone prints what wrote there (rip and exe return addresses on the stack, as exe offsets).
pub(crate) fn watch_cmd(args: &[String]) -> Result<String, String> {
    let b = base()?;
    if let Some(a) = args.first() {
        let cur = WATCH_SLOT.load(Ordering::Relaxed);
        if a == "off" {
            if cur < TARGETS {
                hwbp::clear(cur);
            }
            WATCH_SLOT.store(usize::MAX, Ordering::Relaxed);
            WATCH_ADDR.store(0, Ordering::Relaxed);
        } else {
            let exec = a == "exec";
            let a = if exec {
                args.get(1).ok_or("puppet_watch exec <exe offset hex>")?
            } else {
                a
            };
            let addr = if exec && a.starts_with("abs:") {
                usize::from_str_radix(a.trim_start_matches("abs:").trim_start_matches("0x"), 16)
                    .map_err(|_| "hex address")?
            } else if exec {
                b + usize::from_str_radix(a.trim_start_matches("exe+").trim_start_matches("0x"), 16)
                    .map_err(|_| "hex offset")?
            } else if a == "slot1" || a == "slot1s" {
                let wcm = crate::game::mem::read::<usize>(b + RVA_WCM_SLOT)
                    .filter(|&w| w != 0)
                    .ok_or("no world")?;
                crate::game::mem::read::<usize>(wcm + WCM_PLAYER_SET + 0x18).ok_or("no entries")?
                    + if a == "slot1s" { 24 } else { 16 }
            } else {
                usize::from_str_radix(a.trim_start_matches("0x"), 16).map_err(|_| "hex address")?
            };
            VEH.call_once(|| {
                // SAFETY: registering a vectored handler with a valid function pointer.
                if unsafe { AddVectoredExceptionHandler(1, Some(veh)) }.is_null() {
                    tracing::warn!("puppet: AddVectoredExceptionHandler failed");
                }
            });
            let s = if cur < TARGETS {
                cur
            } else {
                (0..TARGETS)
                    .find(|&s| hwbp::is_free(s))
                    .ok_or("no free slot")?
            };
            WATCH_ADDR.store(addr, Ordering::Relaxed);
            WATCH_SLOT.store(s, Ordering::Relaxed);
            WATCH_EXEC.store(exec, Ordering::Relaxed);
            if exec {
                hwbp::set(s, addr);
            } else {
                hwbp::set_write(s, addr & !3);
            }
            tracing::info!("puppet: write watch slot {s} on 0x{addr:X}");
        }
    }
    let n = WATCH_N.load(Ordering::Relaxed);
    let mut out = vec![format!(
        "watch slot {} on 0x{:X}: {n} write(s)",
        WATCH_SLOT.load(Ordering::Relaxed) as isize,
        WATCH_ADDR.load(Ordering::Relaxed)
    )];
    for i in n.saturating_sub(16)..n {
        let row = &WATCH_LOG[(i % 16) as usize];
        let v: Vec<u64> = row.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        let off = |x: u64| {
            if x >= b as u64 {
                format!("exe+0x{:X}", x - b as u64)
            } else {
                format!("0x{x:X}")
            }
        };
        out.push(format!(
            "  #{i}: rip {} value/rdx 0x{:X} stack/rcx,r15,ret {}",
            off(v[0]),
            v[1],
            v[2..]
                .iter()
                .filter(|&&x| x != 0)
                .map(|&x| off(x))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    Ok(out.join(
        "
",
    ))
}
