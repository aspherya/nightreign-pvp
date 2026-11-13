//! Every object offset the mod reads or writes, in one place (R2, 2026-10-20). Game 1.03.3 (`nightreign.exe` 1.3.3.0). The proof
//! for each one is in `.claude/re/MEMORY_MAP.md` (section noted per group); a game patch is re-checked here and nowhere else.
//! Code addresses (RVAs of functions the breakpoints use) stay with their feature (`menuflow`, `quietinvite`, `flags`).
//!
//! Naming: `WCM_*` WorldChrMan, `CHRSET_*` the ChrSet inside it, `CHR_*` any character (ChrIns), `PLAYER_*` PlayerIns only,
//! `GAMEDATA_*` the PlayerGameData object.

// ---- CSSessionManager (MEMORY_MAP §25; HM-0, HM-1) ----
/// Slot of the `CSSessionManager` pointer.
pub(crate) const RVA_SESSION: usize = 0x3C2_1EC8;
/// u32 role: 0 none, 3 host, 6 client.
pub(crate) const SESSION_ROLE: usize = 0xC;
pub(crate) const SESSION_ROLE_HOST: u32 = 3;
pub(crate) const SESSION_ROLE_CLIENT: u32 = 6;
/// `players` vector: first and last pointers.
pub(crate) const SESSION_PLAYERS_FIRST: usize = 0x78;
pub(crate) const SESSION_PLAYERS_LAST: usize = 0x80;
/// Size of one `players` entry.
pub(crate) const SESSION_PLAYER_ENTRY: usize = 0xF0;
/// u64 Steam id of the host.
pub(crate) const SESSION_HOST_STEAM: usize = 0xA0;

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
/// UTF-16 character name inside `PlayerGameData`, 16 units with the terminator (the name of the save) `[VERIFIED: peek, 2026-11-03, MEMORY_MAP §5]`.
pub(crate) const GAMEDATA_NAME: usize = 0x9C;
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
/// The Night's Tide rain on a character standing outside the circle (`PlayAreaCreateCommonParam` row 0 rain SpEffects, MEMORY_MAP §16).
/// `tide-lab` 2026-11-07: LOCAL held 99811 at 23:30 outside the final circle and not before.
pub(crate) const SPEFFECTS_TIDE_RAIN: [i32; 2] = [99_810, 99_811];

// ---- PlayerGameData (MEMORY_MAP §5) ----
/// u32 HP mirror inside PlayerGameData.
pub(crate) const GAMEDATA_HP: usize = 0x10;
/// u32 max HP inside PlayerGameData (T-040 run 1: 260 while HP was 43).
pub(crate) const GAMEDATA_MAX_HP: usize = 0x14;
/// Readable RTTI name of the PlayerGameData class (used to recognise the object before reading it).
pub(crate) const GAMEDATA_CLASS: &str = "CS::PlayerGameData";

// Saved loadouts: NR 1.03.3, static disassembly 2026-11-13 (MEMORY_MAP §29, Saved presets).
pub(crate) const LOADOUT_REMOVE_RVA: usize = 0x1F_3E90;
pub(crate) const LOADOUT_REMOVE_BYTES: [u8; 10] =
    [0x48, 0x89, 0x5C, 0x24, 0x18, 0x55, 0x56, 0x57, 0x41, 0x54];
pub(crate) const EQUIPPED_INDICES: usize = 0x1E0;

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
/// UI-3: the targets chosen in the expedition list, a `vector<i32>` of `NightBossMenuParam` rows inside the settings object (begin, end). The
/// game's own Commence reads the same list (`exe+0x9EEDB4`, and `exe+0xDF9E90` sends it) `[VERIFIED: exec_trace, list [9] on our card, 2026-11-05]`.
/// BOT-5: the "No. of Password Players" byte (0 = three players, the default; 1 = two), shown by the mod as **Bots: Off / On**. The row
/// builder `exe+0x9F2500` passes `[list+0x3E08]+0x48` as the row's value, the help code `exe+0x9FAD48` tests it, and `exe+0x9F98F0`
/// copies it with `+0x49` into the party settings (game 1.03.3, static 2026-11-06).
pub(crate) const MENU_SETTINGS_BOTS: usize = 0x48;
pub(crate) const MENU_SETTINGS_TARGETS_BEGIN: usize = 0x58;
pub(crate) const MENU_SETTINGS_TARGETS_END: usize = 0x60;
/// The row of our Battle Royale card (`gamedata/nightbossmenu-rows.json`).
pub(crate) const BR_CARD_ROW: i32 = 9;
/// ARENA-0: the Arena card, `NightBossMenuParam` row 17 (added by `gamedata/nightbossmenu-rows.json`, listed by `game::arenacard`).
pub(crate) const ARENA_CARD_ROW: i32 = 17;

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
/// WorldChrMan -> inline ChrSet of the bot manager's characters (set index 0x0E; capacity 120), same layout as [`WCM_CHRSET`]
/// (MEMORY_MAP §18: the creator `exe+0x4ABC40` is called with `WorldChrMan+0x9AF0`; OFF-5 found every bot in set 14, game 1.03.3).
pub(crate) const WCM_BOT_SET: usize = 0x9AF0;
/// A bot set larger than this is not plausible (sanity bound; the constructor makes 120).
pub(crate) const BOT_SET_MAX_CAPACITY: u32 = 0x400;
/// `NpcParam` id of the bot tables' Nightfarers (`CHR_ID`; OFF-5 run 1: every bot read 600040000).
pub(crate) const BOT_NPC_PARAM: u32 = 600_040_000;

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
/// f32 radius of the circle now (m). `tide-close-lab` 2026-11-07 (vanilla Day 2): 700 until 12:30, 527.6 at 14:20, 367.7 from
/// 16:09, 198.4 at 22:12, 80.1 at 23:30, 40.025 at 23:59:59 (the night clock holds there, one second short of the final radius).
pub(crate) const PLAYAREA_RADIUS: usize = 0x260;
/// Per-day stage records of 0x20 bytes (x, 230, z, radius as f32 at +0x0C..+0x18 of each): Day 1 at +0x3C (initial, first-circle
/// end, final), Day 2 at +0x9C. The radius fields: initial 700, final = `PlayAreaCreateCommonParam` row 0 `secondCircleRadius`
/// (40 in vanilla), first-circle end about halfway between them (367.724 with 40, 347.586 with 0). `tide-close-lab` /
/// `tide-close-max` 2026-11-07, unchanged from 07:00 to the night.
pub(crate) const PLAYAREA_DAY1_FINAL_RADIUS: usize = 0x88;
pub(crate) const PLAYAREA_DAY2_FIRST_RADIUS: usize = 0xC8;
pub(crate) const PLAYAREA_DAY2_FINAL_RADIUS: usize = 0xE8;
