//! Every object offset the mod reads or writes, in one place (R2, 2026-10-20). Game 1.03.3 (`nightreign.exe` 1.3.3.0). The proof
//! for each one is in `.claude/re/MEMORY_MAP.md` (section noted per group); a game patch is re-checked here and nowhere else.
//! Code addresses (RVAs of functions the breakpoints use) stay with their feature (`menuflow`, `quietinvite`, `flags`).
//!
//! Naming: `WCM_*` WorldChrMan, `CHRSET_*` the ChrSet inside it, `CHR_*` any character (ChrIns), `PLAYER_*` PlayerIns only,
//! `GAMEDATA_*` the PlayerGameData object.

// ---- WorldChrMan and its ChrSet (MEMORY_MAP §2; T-007a) ----
/// WorldChrMan -> inline ChrSet.
pub(crate) const WCM_CHRSET: usize = 0x9A40;
/// ChrSet -> capacity (u32; 6 = Seamless Co-op's cap).
pub(crate) const CHRSET_CAPACITY: usize = 0x10;
/// ChrSet -> pointer to the entries.
pub(crate) const CHRSET_ENTRIES: usize = 0x18;
/// Size of one ChrSet entry; its first qword is the `ChrIns*`.
pub(crate) const CHRSET_ENTRY_SIZE: usize = 0x10;

// ---- Any character, ChrIns (MEMORY_MAP §3; T-007a, TC-004, FS-0/FS-1) ----
/// u64 handle: the character's id inside this game (other objects refer to it by this value). FS-0 run 7.
pub(crate) const CHR_HANDLE: usize = 0x08;
/// u32 character / NPC id (0 for real players).
pub(crate) const CHR_ID: usize = 0x68;
/// u32 character type (0 in the Roundtable, 13 in an expedition for players).
pub(crate) const CHR_TYPE: usize = 0x70;
/// u8 team type: who is hostile to whom; each game decides from its own copy (MEMORY_MAP §4).
pub(crate) const CHR_TEAM: usize = 0x74;
/// u64 session player id, the same on every game for one player (`0x6001FFFFFFFF` = p1); renumbered when the expedition starts.
pub(crate) const CHR_PLAYER_ID: usize = 0x78;
/// u32 ER `network_authority` shifted by NR's +8. Evidence only (log), not verified.
pub(crate) const CHR_NET_AUTHORITY: usize = 0x1EC;

// ---- Players only, PlayerIns (MEMORY_MAP §5-7; FS-0 runs 7, 9, 14) ----
/// u64 current attacker handle (all-ones when none).
pub(crate) const PLAYER_CUR_ATTACKER: usize = 0x198;
/// u64 handle of the last attacker; valid on the victim's own game only.
pub(crate) const PLAYER_LAST_ATTACKER: usize = 0x1A0;
/// Pointer to the player's `CS::PlayerGameData`. The same field the T-015 evidence log called ER's `player_session_holder`
/// (ER +0x598, shifted +8): that guess was the game-data pointer all along (FS-0 run 7).
pub(crate) const PLAYER_GAMEDATA: usize = 0x5A0;
/// ER `session_manager_player_entry` shifted by NR's +8. Evidence only (log), not verified.
pub(crate) const PLAYER_SESSION_ENTRY: usize = 0x6C0;
/// u8 near-death state: 0 normal, 1 downed, 2 getting up.
pub(crate) const PLAYER_DOWNED: usize = 0x710;
/// f32 revive bar, amount left (40/90/240 at down 1/2/3; each hit subtracts; 0 = revived).
pub(crate) const PLAYER_REVIVE_BAR: usize = 0x714;
/// f32 bleed-out clock, seconds left (death at 0; writable on the owner's game).
pub(crate) const PLAYER_BLEED_CLOCK: usize = 0x718;
/// f32 pause timer (about 3.5 s after a hit; the clock is frozen while it is above 0).
pub(crate) const PLAYER_PAUSE: usize = 0x71C;
/// u32 number of downs this expedition.
pub(crate) const PLAYER_DOWNS: usize = 0x730;
/// Pointer whose target holds the authoritative HP at [`HP_FIELD`] (the target has no vtable). FS-0 run 9.
pub(crate) const PLAYER_HP_PTR: usize = 0xB78;
/// u32 authoritative HP inside the [`PLAYER_HP_PTR`] target. Writing 0 on the owner's game downs the player (does not kill).
pub(crate) const HP_FIELD: usize = 0x70;
/// u32 HP mirror (read-only cross-check before a write).
pub(crate) const PLAYER_HP_MIRROR: usize = 0xBA0;

// ---- PlayerGameData (MEMORY_MAP §5) ----
/// u32 HP mirror inside PlayerGameData.
pub(crate) const GAMEDATA_HP: usize = 0x10;
/// Readable RTTI name of the PlayerGameData class (used to recognise the object before reading it).
pub(crate) const GAMEDATA_CLASS: &str = "CS::PlayerGameData";

// ---- Class tables (MEMORY_MAP §1) ----
/// RVA of the PlayerIns vtable seen by T-007a; only a sanity log against the RTTI result (RTTI is what the code trusts).
pub(crate) const PLAYER_VTABLE_RVA: usize = 0x2C307F8;
