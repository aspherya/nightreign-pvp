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
/// u32 HP mirror (read-only cross-check before a write). T-040 run 1: held part of a pointer on both Nucleus games, so the gates
/// never rely on it alone.
pub(crate) const PLAYER_HP_MIRROR: usize = 0xBA0;
/// u32 current HP inside the character data module (entry 0 of [`PLAYER_MODULES`], vtable exe+0x2C10B88). Writing 0 on the
/// owner's game downs the player (T-040 run 1, Nucleus 2026-10-21; MEMORY_MAP §5). The game's own HP write is exe+0x428CDD.
pub(crate) const DATA_MODULE_HP: usize = 0x140;
/// u32 max HP inside the character data module (MEMORY_MAP §5 "Character data" row).
pub(crate) const DATA_MODULE_MAX_HP: usize = 0x14C;

// ---- Position (MEMORY_MAP §8; T-028 runs 1-4, NR 1.3.3.0) ----
/// PlayerIns -> the character's module list (a block of pointers, no vtable). Seen in the FS-0 run 7 dump (57 pointers); that it
/// is the module list is a HYPOTHESIS (ER has it at +0x190, which is `CS::SpecialEffect` in NR). The physics module was found in it.
pub(crate) const PLAYER_MODULES: usize = 0x1B8;
/// Bytes of the module list searched for the physics module (it sits at +0x68, as in ER; T-028 run 1).
pub(crate) const MODULES_SPAN: usize = 0x1C8;
/// PlayerIns -> origin of the local frame the physics position is measured in (x, y, z f32; y stays 0). The game moves it in
/// 8 m steps as the player walks, so world position = physics position - origin. T-028 run 3: a smooth path across every jump.
pub(crate) const PLAYER_ORIGIN: usize = 0x80;
/// x, y, z f32 position inside `CS::CSChrPhysicsModule`, relative to [`PLAYER_ORIGIN`]. Writing it moves the player and the
/// other games see the move (T-028 run 1, run 4, Nucleus 2026-10-14).
pub(crate) const PHYSICS_POS: usize = 0x70;
/// u8 inside the physics module: 1 standing (and riding the hawk), 0 in the air. T-028 runs 1-4.
pub(crate) const PHYSICS_GROUND: usize = 0x92;

// ---- SpEffects on a character (MEMORY_MAP §17; M5-8 solo run 2026-10-23, the game's own list walk exe+0x509150) ----
/// PlayerIns -> the SpEffect holder; its first qword (+8) is the first list node.
pub(crate) const PLAYER_SPEFFECTS: usize = 0x190;
/// Holder -> first node pointer.
pub(crate) const SPEFFECT_FIRST: usize = 0x8;
/// Node -> i32 SpEffect id.
pub(crate) const SPEFFECT_ID: usize = 0x8;
/// Node -> next node pointer.
pub(crate) const SPEFFECT_NEXT: usize = 0x38;
/// The list walk stops after this many nodes.
pub(crate) const SPEFFECT_MAX: usize = 256;
/// SpEffects that make the player script's direct-death check pass: a revive item at work (540155 / 6999100 are the revival flags
/// 7010-7012 of `common` 1183; MEMORY_MAP §17, archive/research/M5-8-death.md §R1). The die-at-once SpEffect 102130 must not be given
/// to a player holding any of them.
pub(crate) const SPEFFECTS_REVIVE_ITEM: [i32; 4] = [540_155, 6_999_100, 6_999_500, 8_970_061];

// ---- PlayerGameData (MEMORY_MAP §5) ----
/// u32 HP mirror inside PlayerGameData.
pub(crate) const GAMEDATA_HP: usize = 0x10;
/// u32 max HP inside PlayerGameData (T-040 run 1: 260 while HP was 43).
pub(crate) const GAMEDATA_MAX_HP: usize = 0x14;
/// Readable RTTI name of the PlayerGameData class (used to recognise the object before reading it).
pub(crate) const GAMEDATA_CLASS: &str = "CS::PlayerGameData";

// ---- Class tables (MEMORY_MAP §1) ----
/// RVA of the PlayerIns vtable seen by T-007a; only a sanity log against the RTTI result (RTTI is what the code trusts).
pub(crate) const PLAYER_VTABLE_RVA: usize = 0x2C307F8;
/// RVA of the `CS::CSChrPhysicsModule` vtable, read from the RTTI of `nightreign.exe` 1.3.3.0 on disk (2026-10-13). The game reads
/// the position at +0x70 of this object (code at exe+0x46E5C1).
pub(crate) const PHYSICS_VTABLE_RVA: usize = 0x2C1_3720;

// ---- Character type values (MEMORY_MAP §3; TC-004) ----
/// [`CHR_TYPE`] of a player in the Roundtable.
pub(crate) const CHR_TYPE_ROUNDTABLE: u32 = 0;
/// [`CHR_TYPE`] of a player in an expedition.
pub(crate) const CHR_TYPE_EXPEDITION: u32 = 13;

// ---- The stock expedition menu (MEMORY_MAP §13; UI-1 trace, game 1.03.3) ----
/// Expedition menu (`r14` in the Commence checks `exe+0x9EEBE0`) -> pointer to its settings object.
pub(crate) const MENU_SETTINGS: usize = 0xFC8;
/// u8 Number of Players inside the settings object: 2 one player, 1 two, 0 three.
pub(crate) const MENU_SETTINGS_PLAYERS: usize = 0x49;
/// UI-3: which card of the expedition list is selected, read from the menu object (`i32` fields; game 1.03.3, 18 Commence presses on
/// 9 cards, 2026-11-02, log `br_menu dump`). `+0x1338` was 10 on our Battle Royale card only (4 on the Everdark Tricephalos, 0 on the
/// seven other cards sampled) and `+0x10C4` was 18 on it in all four presses. `[VERIFIED]` against The Deep of Night
/// too (our row is a copy of it, yet Commence on the real one was left to the game, 2026-11-02).
pub(crate) const MENU_CARD_KIND: usize = 0x1338;
pub(crate) const MENU_CARD_SLOT: usize = 0x10C4;
/// The values our card reads (`MENU_CARD_KIND`, `MENU_CARD_SLOT`).
pub(crate) const BR_CARD_KIND: u32 = 10;
pub(crate) const BR_CARD_SLOT: u32 = 18;

// ---- The NPC bot manager, CSNpcBotMan (MEMORY_MAP §15; OFF-3 static reading, game 1.03.3) ----
/// Singleton name in the FD4 map (slot `exe+0x3C0B518` in 1.03.3; found by name, never by address).
pub(crate) const BOTMAN_NAME: &str = "CSNpcBotMan";
/// i32 bot table id (`NPCBotTableParam` row id), -1 = none.
pub(crate) const BOTMAN_TABLE: usize = 0x0;
/// u8 armed (1 after event `2003[97]` loaded a table).
pub(crate) const BOTMAN_ARMED: usize = 0x4;
/// Spawned-bot vector: begin and end pointers; 16-byte entries (byte +9 = "remove").
pub(crate) const BOTMAN_BOTS_BEGIN: usize = 0x28;
pub(crate) const BOTMAN_BOTS_END: usize = 0x30;
pub(crate) const BOTMAN_BOT_ENTRY: usize = 0x10;

// ---- The play-area clock, WorldPlayAreaManager (MEMORY_MAP §16; M5-1 static reading, game 1.03.3) ----
/// Singleton name in the FD4 map (slot `exe+0x3C13158` in 1.03.3; found by name, never by address).
pub(crate) const PLAYAREA_NAME: &str = "WorldPlayAreaManager";
/// `DLDateTime` of the play-area clock: u64 FILETIME-like value (0 = no clock running), then the packed fields at
/// [`PLAYAREA_CLOCK_PACKED`] (hour bits 34..39, minute 39..45, second 45..51). Read by the event conditions `1[6]`/`1[7]`.
pub(crate) const PLAYAREA_CLOCK: usize = 0xF8;
pub(crate) const PLAYAREA_CLOCK_PACKED: usize = 0x100;
/// f32 clock speed the per-frame update multiplies the frame time by.
pub(crate) const PLAYAREA_SPEED: usize = 0x118;
/// i32 set-time mailbox (hour, minute, second; -1 = empty). The game's update (`exe+0x698430`) applies a valid triple with its
/// own setter (`exe+0x68FF30`), empties it, and in a session sends the time to the other players.
pub(crate) const PLAYAREA_SET_HOUR: usize = 0x124;
pub(crate) const PLAYAREA_SET_MINUTE: usize = 0x128;
pub(crate) const PLAYAREA_SET_SECOND: usize = 0x12C;
/// u8 1 while the clock advanced in the last update (0 = held: paused, a cutscene, a reset in progress).
pub(crate) const PLAYAREA_RUNNING: usize = 0x130;
