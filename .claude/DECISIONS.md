# Decisions made (so they are not re-argued)

One line per decision with date and who decided. Product intent: `re/PROJECT_SCOPE.md`; engineering decision records with evidence (ADRs): `TECHNICAL_NOTES.md`.

- Stack: Rust DLL loaded through me3 next to Seamless Co-op; nothing in Seamless Co-op is modified.
- **The table offers only Solo / Duo / Trio Battle Royale** (Leonardo, 2026-10-19): no normal expedition while the mod is installed. The
  choice opens the stock boss menu; pressing start begins our matchmaking; the game's own "Cancel matchmaking" stops it (no Leave entry). UI-1.
- **No ImGui for players in the final product** (Leonardo, 2026-10-19): menus and status messages must be the game's own UI. The
  Insert panel was a stop-gap; **removed from the player build on 2026-10-20** (the overlay is a developer tool only).
- **Every match has 6 players** (Seamless Co-op's limit; Leonardo, 2026-10-19): Solo 6 × 1, Duo 3 × 2, Squad 2 × 3. Pre-release zips use 2 for testing.
- PvP is always on for rivals; players cannot switch it off. Launching with `nrpvp_launcher.exe` is the mode, until a menu entry exists.
- Hostility: each game keeps itself and teammates on the normal value and marks only rivals (value 29).
- Downed players: teammates fill the purple bar; rivals shorten the bleed-out and never move the bar. Instant death on 0 health was
  rejected on 2026-10-12 (auto-revive items would defeat it); **superseded for Solo nights on 2026-10-23** (a kill is a real death, die-at-once only without a revive item).
- Investigation first, then building: confirm memory locations with the scanner before adding mod code.
- Matchmaking joins through the game's own lobby (Invite Members path), never a lobby our mod creates; never EAC, never Bandai servers.
- Leonardo only does smoke tests; Claude decides design and order, and does not build tools that are not needed for the goal.
- **Developer control is a text console** (2026-10-20): commands over a named pipe with `nrcon`, so agents can drive a running game;
  the ImGui panel and F-keys were deleted (R5, 2026-10-20). `CONSOLE.md`.
- **Code structure** (2026-10-20): ports and adapters, typed memory views, feature modules, dev code behind a cargo feature; refactor
  before the next big features. `ARCHITECTURE.md`.
- **The crossing line** (Leonardo, 2026-10-22): one line for every start route, diagonal "B" of the route atlas (the longer and higher;
  the "bottom to top" rule is dropped). TASKS HAWK-1b.
- **Solo: a kill is a real death** (Leonardo, 2026-10-23; PROJECT_SCOPE §2): at night 0 HP without a revive item = dead at once;
  the dead spectate; Day 2 respawns everyone at random places. Research first (TASKS M5-8), then M5-9. Kill feed: Leonardo's own
  later system (bottom-right notifications), not now.
- **Eliminated = held down, not dead** (Claude, 2026-10-23; implements M5-D; stays for Duo/Trio and as the fallback): the game's death and ~7 s comeback are engine code in
  an obfuscated region, so an eliminated player is kept in the downed state (clock held) until the morning and then stood up with
  the game's own get-up. Spectating someone else is a separate camera card (M5-7). **Solo uses the real death instead** (M5-9,
  from M5-8: SpEffect 102130 + the held comeback). Esc while dead: not now; M5-7 must at least allow quitting (Leonardo, 2026-10-23).
- **Match start = hawk crossing** (Leonardo, 2026-10-21): the start hawk flies one straight line across the whole map and each player
  jumps off where they want (Fortnite style). Replaces separate team spawns. `PROJECT_PLAN.md` §13.
- **Matchmaking direction** (Leonardo, 2026-10-21): stay on Seamless Co-op as long as it carries us, with three must-haves: no
  Steam window or invite ever on a player's screen, everyone arrives ready and the match starts by itself, and the "host" stops being a
  player-facing role. Asking SC's author is out of scope. Studying how SC works (read-only, for interop; never patched, copied or
  redistributed, hard rule 3) is approved, to prepare our own matchmaking/session layer if SC becomes the roadblock.
- **Nights are elimination rounds** (Leonardo, 2026-10-22; `re/PROJECT_SCOPE.md` §2): no boss, no minions; no respawn at night,
  the dead spectate; a night ends only when one player (team) is left, no timer; only that winner is healed; the others respawn
  spread out at the morning; night 2's winner wins the match, the rest lose; no Day 3 / Nightlord; the 40 m final circle stays.
  At night 0 HP = knocked for the normal 15 s (Solo too; rival hits −3 s, teammates revive); bleed-out over = eliminated until
  the morning, spectating (M5-D, 2026-10-22).
  Research and tests come before code: Leonardo runs Nucleus multi-instance checks for it before the first UAT with his friend.
- **Host leaving mid-match** (Leonardo, 2026-10-21): for now accept that the match ends, and end it cleanly; the real goal is **host
  migration** (Destiny / old CoD style), worked on as far as it goes. The host is picked by the matchmaker (best connection), never by
  luck, and has no player-facing role. **A server host (a PC running the game with no player) is rejected** for now.
- **One owner doc per topic** (2026-10-20); the map is in `CLAUDE.md`. Finished task cards and runbooks leave the docs (git keeps them).
- **Bots are not a game feature; Nucleus is a secondary tool; the orchestrator is in this repo** (Leonardo, 2026-10-24): the automatic offline bots (OFF-5) are gone; the game's bot manager stays as the engine of the orchestrator's puppet players. `scripts\deploy-nucleus.ps1` stays for quick two-window checks of plain rules (no event files, no `-Me3`); anything that needs our event files runs through `nro` (offline, puppets) or a friend session. `nro`, its scenarios and tools live in `crates/nro*`, `scenarios/`, `tools/`, `pins/`.
- **Docs are small files with one owner each** (Leonardo + Claude, 2026-10-24): STATUS holds only "now" (≤ 8 KB), results go to `testing-log/`, traps to `TRAPS.md`, decisions to `DECISIONS.md`, tools to `TOOLS.md`; a card's acceptance is a scenario where it can be.

- **`nrscan` deleted** (Leonardo, 2026-10-24): the external scanner/poker of Nucleus games was replaced by the in-DLL console and the orchestrator; it stays in git history (`git show cbbffa2:crates/nrscan`). `nrcon` stays (console client; reaches several games with `--all` / `-i N`).
- **No winner's choice; the final fight is the whole of night 2; everyone sees the report** (Leonardo, 2026-10-24): winning ends the match with nothing offered (PROJECT_PLAN F13 dropped); the final arena is the fight between all players on night 2; winner and losers both get the game's report / Result screen and return to the Roundtable. Contraction and sudden death stay as designed in PROJECT_SCOPE §4 until Leonardo changes them. Verified on one PC 2026-10-24: after the ending, win and loss both go Result → Reward → Roundtable (testing-log).
- **After the first real match: the rule set changes** (Leonardo, 2026-10-24; cards RW-*, TASKS): (1) a player killed by another player/team or by the night-tide is **defeated for good** and becomes a spectator, who may keep watching or quit to the Roundtable and search again; (2) a defeated player drops their items (red phantom) and runes on the ground for others to loot; (3) death by a mob or boss is the vanilla death: respawn with items, fetch the runes; (4) Dormant Powers and weapons from bosses drop only for the team/player that landed the last hit; (5) Day 1 and Night 1 run their normal length with **no night-tide**; the tide's vanilla behaviour starts on Day 2; (6) the match ends the moment one team/player is left alive, even in the first minutes; (7) a private match joined by password, outside matchmaking. Fixes: respawn far from all players; runes shared only inside the own team; no other player's map/compass position and nametag/lifebar only for the player you target; enemy count and strength use the vanilla Solo/Duo/Trio values. **Supersedes** "nights are elimination rounds / losers get up at the morning" (2026-10-22) and the M5-9 hold-until-morning for players killed by players; night 1 is no longer a round of its own.
- **Downed always, in Solo too** (Leonardo, 2026-10-24, after talking to a player; reverses "Solo: a kill is a real death" of 2026-10-23): no insta-death. A player at 0 HP is downed for the normal bleed-out (rival hits shorten it, rivals cannot revive); when it runs out the death is judged: a rival or the tide = defeated for good, anything else = the game's own respawn. Solo, Duo and Trio behave the same. Die-at-once (SpEffect 102130, flag 10007420) stays only as a one-PC test aid (offline solo has a revive effect).
- **Spectating = the mod's moved body, on by default** (Leonardo, 2026-10-25, Nucleus 3-player test): with `spectate 0` the game does nothing for a defeated player (no native spectator in our rivals-only match), so the held, invisible body follows a living player and the game's follow camera shows them. Works ("exactly like I wanted"), camera movement still to be smoothed (find the follow camera's target: `ChrExFollowCam` +0xA0 holds the followed position history; TASKS).
- **Map and compass show only your own team** (Leonardo, 2026-10-26; card RW-9): rival players and teams must never see each other's position on the world map or the compass bar, so the UI does not hand out where the enemies are and hunting stays a player's work. **Solo** = no other player on the map or compass. **Duo / Trio** (same rule) = a player sees only their own team's members there. Built so far: Solo only (`game/maphide.rs`); Duo / Trio needs a per-player decision (hide rivals, keep teammates) and is the next step of RW-9. Teams otherwise behave as in vanilla (teammates keep their map / compass marks, name and bars). **Life bars stay vanilla** (Leonardo, 2026-10-26, withdrawing his stricter wording): the enemy bar a player gets by lock-on (R3), by hitting a player, or when that player's HP drops is kept as the game does it; nothing to hook. What is still open is only the always-on overhead name tag and the party bar of other players (TASKS RW-9: look at them in the friend session first; levers in MEMORY_MAP §20: SpEffect 446, party bar producer `exe+0x7DA8B0`, tag producer `exe+0x7D9F80`).
- **Seamless Co-op will be replaced by our own networking** (Leonardo, 2026-10-30, while designing RW-5): do not build new cross-game features on SC's lobby, chat or flags. Features that need the other game's knowledge (RW-5: who landed the last hit) go behind an interface (`BossVerdict`) with the game's own knowledge as today's source; our transport plugs in later. Fail-open (vanilla behaviour) while a verdict is unknown.
- **RW-5 = first to loot, no last-hitter logic** (Leonardo, 2026-10-31): only the first team to OPEN a boss's Dormant Power screen may choose from it; the claim is made at open time and sent on the data channel (`ORB_CLAIM`); other teams' games lose the orb's interaction by nulling its `MapItemMan` slot. Supersedes the old "blank boss rows + verdict" plan (`rw5-boss-drops`). Skeleton built 2026-10-31 on `claude/happy-goldstine-746484`, not merged.
