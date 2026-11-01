# S1: Player cap (what enforces 6, and is there a route beyond it)

Researcher: Sonnet, 2026-10-10. Read-only. Tags: `[SOURCE]`, `[VERIFIED]`, `[HYPOTHESIS]`, `UNKNOWN`.
ER = Elden Ring, NR = Nightreign. ER facts are hypotheses for NR.

## Bottom line

- The only hard evidence for the 6 limit in NR is **SC-side and soft**: the lobby text, the author's statements, the Nucleus handler value, and a player ChrSet whose capacity reads 6 under SC.
- **No source and no experiment says the engine itself caps at 6.** The ChrSet capacity of 6 may be set by SC, by the game, or both. We cannot tell which. UNKNOWN.
- Beyond 6 there is no known route. All realistic routes pass through SC's author (Yui) or through deep RE of a closed, WinLicense-protected DLL. Plan for 6.

## 1. Vanilla NR (limit 3)

| Fact | Tag |
|---|---|
| Vanilla NR is 3 players. Title Update 1.02 added an official two-player mode (live 2025-07-30). Nothing official for 4-6. | [SOURCE: https://www.dsogaming.com/news/elden-ring-nightreign-gets-an-official-two-player-mode-on-july-30th/ (search snippet, secondary)] |
| Where the 3 comes from (code, param or session limit): UNKNOWN. Looked in: fromsoftware-rs `crates/nightreign/src` contains only `lib.rs` and `param.rs`, so there are no NR session or WorldChrMan structs to read. | [SOURCE: https://api.github.com/repos/vswarte/fromsoftware-rs/contents/crates/nightreign/src, listed 2026-10-10] |
| SC's 1.04 changelog says the "Two player / Three player" options are no longer relevant. SC reuses the vanilla expedition-type menu and relabels it "Up to 6 players". | [SOURCE: Nexus NR mod 3 changelog, via research/T-003-seamless-coop.md#L266-L268] [VERIFIED: locale english.json "Up to 6 players"] |

## 2. What SC's author has said publicly (NR)

All taken from `research/T-003-seamless-coop.md` (which read Nexus mod 3 on 2026-10-09). I did not re-read Nexus; fetch tools get 403 on Nexus (checked mod 596).

| Statement | Tag |
|---|---|
| Lobbies are 1-6. Yui says the cap could go higher, but more than 6 is overkill because the game is "comically easy" beyond that. | [SOURCE: PC Gamer via Yahoo, 2025-08-05; T-003#L132] |
| Yui said bugs appear once more than 5 players join, so 6 is shaky. | [SOURCE: same article; T-003#L133] |
| The mod is "feature complete" and in maintenance. No plan for PvP. | [SOURCE: Nexus NR mod 3; T-003#L33] |
| Changelog history: 1.00 added 1-6. 1.02 and 1.04 fixed dormant-power and desync bugs for later slots. 1.07 reverted to 3 players "for maintenance". 1.10 restored 1-6. 1.11 fixed relics unusable for slots 4-6. 1.12 patched events that "could cause issues for players 4, 5, and 6". | [SOURCE: Nexus NR changelog; T-003#L125,#L134] |
| Known issues (sticky post): global events may be wrong in 4-6 player sessions; Revenant in slots 4-6 has problems with its primary power. | [SOURCE: Nexus NR mod 3 Posts; T-003#L135] |
| Companion mod "Nightreign 6 Player Fixes" (Sayu) tweaks map, script and event data for >3 players. | [SOURCE: https://www.nexusmods.com/eldenringnightreign/mods/596 via T-003#L41, search snippet only] |

What this implies for us:
- Slots 4-6 required per-slot fixes from the author: per-slot content exists (events, power effects) and it was extended by hand from 3 to 6. The game's data was designed for 3 slots.
- [HYPOTHESIS] Going to 7+ would hit the same class of bugs in more places, and SC would need a change for the lobby too.
- No public statement about a hidden ini or setting for more than 6. The 246-byte ini has only scaling keys. [SOURCE: T-003#L88-L92]
- GitHub `yuiamoroll/NightreignSeamlessCoopRelease` holds only data files: `BLOCKSYNC`, `ENDLESSNIGHT`, `SUPERNIGHTLORD`, `SUPERNIGHTLORD2`, `SUPERNIGHTLORD3`, `VERSION`. No README, no source, no issues, no player-limit information. [VERIFIED: GitHub contents API listing, 2026-10-10] `VERSION` ends with `>1.13 1`. [SOURCE: https://raw.githubusercontent.com/yuiamoroll/NightreignSeamlessCoopRelease/main/VERSION]

## 3. Evidence inside nrsc.dll (plain strings only, no disassembly)

- Plain strings matching "player": `MODMESSAGE_YUINEXUSBEGINONEPLAYER`, `...BEGINTWOPLAYER`, `...BEGINTHREEPLAYER`, `...PLAYERCOUNTREMAINING`, and the RTTI `EcTestPlayerNo`. There is no string for "max players" or "player limit". [VERIFIED: byte-pattern grep of `Game\SeamlessCoop\nrsc.dll`, 2026-10-10]
- The three BEGIN messages are the vanilla expedition-type menu choices (1/2/3). [HYPOTHESIS: they map to those menu entries]. They show the engine/UI side is still keyed to 1-3, with SC layering 4-6 on top. The mechanism is UNKNOWN.
- Config keys are not stored as plain text (T-003), so a hidden limit key can't be ruled out statically.

## 4. Engine-side evidence (NR)

| Fact | Tag |
|---|---|
| Player ChrSet inline at WorldChrMan+0x9A40, capacity (+0x10) = 6, entries ptr at +0x18, 0x10-byte entries, ER shape. | [VERIFIED: TECHNICAL_NOTES.md §4, T-007a run 2] |
| Local player is entry[0]. `ChrIns+0x78` is a per-player id identical on all clients: host `0x6001FFFFFFFF`, joiner `0x6002FFFFFFFF`. Only 2 players have been observed. The pattern `0x600N...` for slot N is [HYPOTHESIS]. | [VERIFIED: FS-0/FS-1 per TECHNICAL_NOTES.md §4] |
| An observation we have not explained: 34 PlayerIns objects exist during an expedition. Four are pre-allocated, ten more sit in pairs at +0x9C20..+0xA170. These are team 30 (Object), not players. Is the ChrSet of 6 sized to a vanilla value (3) times 2, or SC-patched? UNKNOWN. | [VERIFIED: TECHNICAL_NOTES.md §4] [HYPOTHESIS: purpose] |
| The capacity is a runtime field, not a compile-time array. A way to learn who set it: read WorldChrMan in a **vanilla (SC-free) Nightreign** session and read the same field. Cost is low: one launch, a read-only log line, offline. That reads capacity 3 or 6 (or something else) and tells us whether SC changes it. | [HYPOTHESIS: experiment proposal] |

## 5. ER evidence (hypotheses for NR)

| Fact (ER) | Tag |
|---|---|
| `WorldChrMan.player_chr_set: ChrSet<PlayerIns>` at +0x10EE0. ChrSet has a runtime `capacity` field; its vftable has `get_capacity`. | [SOURCE: https://raw.githubusercontent.com/vswarte/fromsoftware-rs/main/crates/eldenring/src/cs/world_chr_man.rs#L40-L41,#L277-L278,#L334-L335] |
| `CSSessionManager` has `players: DLVector<SessionManagerPlayerEntry>`. `session_player_limit: u32` is documented "Set to 4 in open world and 6 in quickmatch arena". | [SOURCE: https://raw.githubusercontent.com/vswarte/fromsoftware-rs/main/crates/eldenring/src/cs/session_manager.rs#L100-L106] |
| `session_player_limit_override: u32` is "set to 1 on init and never changed. If it's not 1, all sessions will use this value instead of the default session player limit." This is the nearest thing to a built-in engine knob for the limit, and it suggests the limit is a runtime value, not a hard array bound. | [SOURCE: same file#L145-L148] [HYPOTHESIS for NR: same field exists] |
| ER's own limits: 4 in open world (host + 3), 6 in the arena. So ER vanilla already has a 6-player mode (quickmatch), and ER's ChrSet must hold at least 6. | [SOURCE: same file#L104-L106] |
| SC for ER (Yui) is described in secondary sources as 6 players, and a 2.0.0 rewrite mentions "player count" changes. One low-quality article claims 127. None are primary and no setting name was found. | [SOURCE: https://dotesports.com/elden-ring/news/beloved-elden-ring-coop-mod-overhauled-pretty-comprehensively (snippet)] UNKNOWN: ER SC max player key, if any |
| I found no ER or NR mod that exceeds the standard player count by engine change, other than SC itself (3 -> 6 in NR). Searches run: "Elden Ring Seamless Co-op max players more than 6", "Nightreign Seamless Co-op more than 3 players". | UNKNOWN (searched, nothing primary) |

No NR structs for `CSSessionManager` or `WorldChrMan` exist in fromsoftware-rs (NR crate has only params), so every ER field above is a pointer for CE, not a fact about NR.

## 6. Nucleus Co-op (test rig)

- The local handler `D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN.js` sets `Game.MaxPlayers = 6` and `Game.MaxPlayersOneMonitor = 6`. [VERIFIED: file read, lines 61-62]. So Nucleus allows up to 6 instances per that handler.
- We have only run **2** instances. Whether the PC can run 3+ instances at once (RAM, VRAM, CPU) is UNKNOWN, and whether SC's lobby forms with 3-6 Goldberg instances is UNKNOWN. This is the first practical test: T-021 already plans 3-4 instances.
- Nucleus uses Goldberg Steam emulation, not real Steam. [VERIFIED: TESTING.md FS-0]

## 7. Options for more than 6

| # | Route | Evidence | Verdict |
|---|---|---|---|
| A | Plan for 6 (2x3 or 3x2). | Everything above | Recommended for the prototype. Known to work; slots 4-6 patched upstream. |
| B | Ask Yui for a higher cap or a hidden setting. | He has said the cap "could go higher" but he considers it overkill; he is in maintenance mode; slot 4-6 bugs exist at 6 already; he guards against drawing attention (T-003#L235) | Low chance. Cheap to ask. We need an answer to: is the cap an ini/compile constant in SC, is 7+ only a UI/lobby limit, and would he accept our mod as a dependency. |
| C | Raise the engine ChrSet capacity ourselves (or `session_player_limit_override` equivalent). | ER has runtime fields. For NR: capacity 6 is observed, who sets it is unknown; a resized ChrSet means reallocating entries and dependent sync buffers (NetChrSetSync) | Deep RE, high risk, and SC's Steam lobby and message routing would also need to accept >6. We would also be altering behaviour inside SC's hooks (WorldChrMan hook, PlayerNetworkSession). Not recommended before B and the vanilla-capacity probe. |
| D | Our own player layer: run a second, independent SC session group (two 6-lobbies) bridged by our own messages. | Nothing supports it. SC session = one Steam lobby; a client can be in one lobby; the engine only renders players in its own ChrSet | Not a route to 18 in one world. [HYPOTHESIS] |
| E | Spectators or ghosts: extra participants that are not PlayerIns (own sync, no avatars). | No evidence; overlaps S4 | Out of scope here; note for S4. |

## 8. What we'd need from SC's author

1. Is "6" an SC constant, a game constant, or both? Which part of SC's code enforces it (lobby size, ChrSet resize, UI)?
2. Would a 7-18 player lobby be technically possible in SC, and is he willing, or willing to say what blocks it?
3. Does SC edit the player ChrSet capacity or `session_player_limit` itself?
4. Permission to ship a mod that depends on SC (project-wide question, already open in T-003).

## Open questions

1. Who sets ChrSet capacity 6: game or SC? Cheapest test: log WorldChrMan+0x9A40 capacity in a vanilla NR session (offline, no SC, our DLL loaded through me3). HUMAN step.
2. Does the `ChrIns+0x78` pattern continue as `0x6003...` to `0x6006...` with 3-6 Nucleus instances? Test with T-021.
3. Can the user's PC run 3, 4, 6 Nucleus instances, and does SC form a lobby with them?
4. Where does vanilla's 3-player limit live (session limit, matchmaking, UI)? Needs RE on NR; NR session struct layout is not in fromsoftware-rs.
5. Does ER SC (2.x) have a configurable player maximum? Primary source (ER SC Nexus page, `ersc_settings.ini` docs) not fetched (Nexus returns 403 to fetch tools; Browser pane not used).
6. Is the Nexus mod 3 Posts section holding newer author statements (after 2025-08-05) about the cap? Not re-read in this spike.
