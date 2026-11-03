# Host migration (and player count): findings and plan

**Owner doc for this topic.** Leonardo's goal (2026-11-02, DECISIONS): a match never ends because the host left. No pause; a Call of Duty style reload only as a last resort. Raising the player count (up to 12) comes later, only on good results.
Tags: `[VERIFIED: how]`, `[SOURCE: link]`, `[HYPOTHESIS]`, UNKNOWN. **ER** = Elden Ring fact, a hint only for NR. Pins: game 1.03.3 (exe 1.3.3.0), SC 1.1.3, me3 0.11.0. Raw results: `testing-log/2026-10-20_on.md`, entry "HM-0". Memory layout: `re/MEMORY_MAP.md` §25.

**Working rule (Leonardo, 2026-11-02):** if a step runs past its budget in §6, stop and ask him. Do not keep digging.

## 0. Bottom line

1. **The game already has host migration.** When the Steam lobby owner changes, its own code promotes the new owner: `exe+0xDF6890` writes `lobby_state` 6 -> 3 at `exe+0xDF6980`. We never write it. Seen working on one PC, twice in a row in one match (§1a).
2. **Under the new host:** players stay together, enemy deaths reach both games, the clock follows the host, and the host's night end gives Day 2 on the other game.
3. **Why it looked impossible:** our last-team rule ended the match (with 2 players, the only size played with friends so far, that is even correct), and the emulator keeps a killed host as lobby owner, so a kill alone never starts the migration.
4. **A defeated host who leaves with F10 works with nothing simulated:** the game leaves the lobby itself and a client is promoted about 11 s later (§1a). Emulated Steam still.
5. **Not proven: real Steam after a crash.** In the kill tests the trigger was simulated: the host calls `LeaveLobby` just before it dies. Steam's documentation says ownership moves when the owner leaves `[SOURCE]`. Whether it moves within the game's grace time after a crash is UNKNOWN. **This is the decisive open test: runbook FS-HM1, TESTING §6.**

## 1. What happens when the host dies

### 1a. The lobby owner moves: the game migrates by itself

Three emulated instances, one expedition. The host calls `hm_leave_lobbies`, then is killed 1.5 s later (`nro stop --i 1`). `god 1` on the clients. Runs `1793673068` (rule off) and `1793673711` (rule on, as players run it). No write to game memory by us.

| Time after the host left | What the survivors show |
|---|---|
| 0 to +20 s | 3 players, `lobby_state` 6; Steam already names the new owner |
| +22 to +27 s | **One client is `lobby_state` 3, the other stays 6 and points `+0xA0` at it.** Host gone from the list |
| over 150 s later | Same session, no reset at +55 s, no Result screen. With the rule on: no `match decided` |

| Checked under the new host (run 1793673068) | Result |
|---|---|
| Positions | The other client's copy matches exactly on both games |
| Clocks | Within seconds on both |
| `mob_kill` on the client | Enemy count 536 -> 535 on both |
| `mob_kill` on the new host | 535 -> 534 on both |
| `clock end` on the new host | The other client's clock jumped too (23:59:59) |
| `night_end` on the new host | Flags 7502, 7503 ON on both; later `night_match: day 2`, flag 7505 ON, clock 08:57 |
| `drop_gear dry` | New host `host yes`, other client `host no` |
| **Second migration:** the new host leaves and dies | The last survivor went to `lobby_state` 3 at about +25 s and kept playing |

| Checked in scenario `mi-host-migration` (PASS 2026-11-03, rule on) | Result |
|---|---|
| Past the old +55 s | Still together, no `match decided` |
| Data channel | Each survivor has the other as peer (fixed: the roster now also reads the game's expedition lobby) |
| Enemies compared by handle (`mobs_dump`) | 588 on both, **all identical** in npc, HP and position. 102 more only on the game whose player was near them, all in 3 map blocks: **a count difference is what is loaded around a player, not a divergence** |
| A client is defeated by a rival | It publishes, **the new host makes the drop** (`itemdrop ... rax 0x1`) |
| Match end | `match decided` with `this game's player won, host` on the new host (fixed: the rules' host flag followed the session id, so a promoted client would never have ended Day 1) |

| **Defeated host leaves with F10, nothing simulated** (run `1793677628`, rule on) | Result |
|---|---|
| Host | `LOCAL defeated by a rival`, F10, then the game's own Result screen |
| Survivors | At +11 s one client `lobby_state` 3, the other follows; both playing together at +100 s and after; no `match decided` |

Not checked: a real player's hits, a boss fight across the change, 4+ players, what a player sees of a drop. One moving enemy can differ by metres between games at a given moment.

### 1b. The lobby owner does not move: the session ends

Same setup, host only killed, `night_count 0`. The emulator keeps the dead host as owner, so this is what happens when no new owner ever appears. Same timeline in both runs.

| After the kill | What the clients show |
|---|---|
| up to +20 s | Nothing changes: 3 players, `lobby_state` 6 |
| **+20 s** | Host leaves the player list and both Steam lobbies (`LobbyChatUpdate_t`, state 0x2). `players` vector 0x2D0 -> 0x1E0 bytes. `lobby_state` stays 6, `+0xA0` still holds the dead host. Enemy HP falls (336 -> 298) and the enemy count rises (523 -> 536) on both |
| **+20 to +55 s** | **Hostless and together:** client 2's copy of client 3 matches client 3's own position; one moving enemy matches within about 0.5 m |
| **+55 s** | The other client leaves the player list |
| **+56 s** | `lobby_state` 6 -> 0, protocol 7 -> 0, `players` empty; local player id becomes `0x6001...` |
| after that | Each client plays alone. The same enemy is about 3 m off between them: the worlds run apart |
| over 16 min later (run 1) | Both alive: enemies moved and fought, `mob_kill` removed an enemy from the count, the clock ran, a jump to `clock circle` showed the tide |

Not checked: Day 2, drops, a real player's hits, a menu quit by the host. `[VERIFIED: runs 1793668424 and 1793670659, testing-log]`

**Correction:** the log entry MM-9 step 2 concluded "the match ends for everybody about a minute later". Its log shows our rule did that (`match decided: ... last one standing`, flag 10007411, banner, Result). With the rule off there is no Result screen, no title screen, no crash.

## 2. Evidence in the game's code

| Finding | Why it matters | Tag |
|---|---|---|
| **Native migration:** `exe+0xDF6890(session, &new_owner)` updates `is_host` on every entry, copies the new host into `+0x90`, and if the local player is the new host writes `lobby_state` 3 at `exe+0xDF6980` | The game does the promotion | `[VERIFIED: static, and the role changed live with no write by us]` |
| **Trigger:** the platform session asks `GetLobbyOwner` every 5 s (`exe+0x25929F2`, slot 0x118); on a change `exe+0x2591340` marks the owner peer (`+0x150`), stores it at `+0x8A0`, refreshes `+0x828` and queues event 3; the event loop at `exe+0xDF2EDB` calls `exe+0xDF6890` | Why the emulator's dead owner never migrates | `[VERIFIED: static]`; the chain as a whole live |
| **Grace time:** while `+0x69C` == 2 and bit 1 of `+0x60` is set: `now > +0x828 + +0x49C` raises `0x81040006` (`exe+0x2592A9D`). Measured: session end about 55 s after the host stopped. With bit 1 off, losing the owner peer raises it at once (`exe+0x258E1B8`) | The window a new owner must appear in. Value of `+0x49C` and the state of bit 1 on real Steam: UNKNOWN | `[VERIFIED: static, timing live]` |
| `session+0x1E` = 1 in an expedition; if 0, a host change leaves the session (`exe+0xDF6958`) | A switch to keep an eye on | `[VERIFIED: static, live read]` |
| **The role is one field:** `CSSessionManager+0xC`, 3 host, 6 client, 0 none. 313 sites read it first; 131 compare with 6, 107 with 3 | The whole game follows when the native code flips it | `[VERIFIED: capstone scan; values live]` |
| **Session reset:** `exe+0xDF61E0` writes `+0xC` = 0 at `exe+0xDF62D0`. Caught live, called from `exe+0xDF2F27`, on the game's main loop, no `nrsc.dll` address in the stack candidates | The place to hold the session, or just above it | `[VERIFIED: write watch, run 2]` |
| **The trigger is an event:** the loop at `exe+0xDF2EDB` polls `[impl]+0x68` (`impl` from `exe+0x257DB50`). Event type 4 carries a code, stored at `session+0x6B8`, then resets. Code seen: `0x81040006`, on both clients | The decision is below, in the platform session layer. Where it is raised and what the 35 s are: UNKNOWN | `[VERIFIED: static, live read]` |
| Other callers of the reset: `0xDF2BB3`, `0xDF2E30`, `0xDF2E9F`, `0xDF3379`, `0xDF582A` | A hold has to consider them too | `[VERIFIED: static]` |
| Native re-entry: `RejoinMapEnterStep` (`Setup_PreApplyInitEventFlagInfo`, `Setup_PostApplyInitEventFlagInfo`, `STEP_Wait_PlayEnterAnim`; vtable `exe+0x2C893E0`), `CSInitSyncStep`, `CSMapActivateSynchronizer` | Base of the last-resort reload. How it is entered and whether it works without Bandai's server: UNKNOWN | `[VERIFIED: strings, RTTI]` |
| ER's `ProtocolState` has `WaitReentryToMap` | A reload that keeps the session may exist | `[SOURCE: fromsoftware-rs session_manager.rs]` **ER** |
| Telemetry keys describe a run in few values: `map_pat_id`, `random_seed_id`, `hotspot_info`, `circle_position_day1`, `circle_position_day2`, `boss_day3`; also `disconnect_session_role_id`, `reconnect_session_role_id`, `leave_error_code` | What a reload would need. The game models a role disconnecting and reconnecting | `[VERIFIED: strings]`; meaning `[HYPOTHESIS]` |
| ER's `ChrIns` has `network_authority` (ER +0x1E4); RTTI has `CSAiThinkOwner`, `NetChrSetSync`, `NetOpenFieldChrSetSync`, `EcTestIsMyMapOwner` | Candidates for who runs an enemy. NR behaviour UNKNOWN | `[SOURCE: fromsoftware-rs chr_ins.rs]` **ER**; `[VERIFIED: RTTI]` |
| The same enemy has the same handle (`ChrIns+0x08`) on every game | A stable cross-game id for enemies | `[VERIFIED: same handle, same world position]` |
| `HostDead`, `HostDead_1`, `OnGuestDead` sit next to UI text effects | Not a host-loss path, as far as this shows | `[VERIFIED: strings]`; meaning `[HYPOTHESIS]` |

**Who changes what (census, `archive/experiments/S10/hook_census.py`, client in an expedition):** 51 inline jumps in the exe's code; 586 redirects in all, most of them import slots. 5 sit in the member-table code (`0x1FDD80`, `0x1FE7B0`, `0x1FECB0`, `0x200A50`, `0x200AA0`), 2 at the player set (`0x4AE340`, `0x4AE430`), 2 in session code (`0xDF9420`, `0xDFE4F0`), about a dozen mid-function ones in `0x576A7E..0x5970A2` and `0x7296EF..0x72AE9E` go to `nrsc.dll`. Not yet sorted into SC's, me3's and ours. The reset above is not hooked. `[VERIFIED: read-only census; addresses only]`

## 3. Design

**Use the game's own migration.** Do not build one. The work left is around it:

1. **Prove the trigger on real Steam** (HM-1e). If the owner moves within the grace time, a crash is covered with no hook at all.
2. **Keep the session when Steam is slow.** If the owner moves after the grace time, extend it (refresh `+0x828`, or hold the raise). Only if HM-1e shows it is needed.
3. **Our mod around it (done 2026-11-03):** the data channel roster survives the change (`net/feed.rs`); the rules' host flag follows the session role (`br_match::is_host`), so a promoted client ends Day 1 and wins as host; every build logs `session: role ...` (`game/hostwatch.rs`).
4. **Known small gap:** a defeat in the seconds when nobody is host yet: the fact arrives on clients only, and its drop is not made. The victim's items are lost, the match goes on. Not worth more code until a session shows it.
5. **Open, Leonardo decides:** tell the players when the host changes, or stay silent. Silent today.

**Last resort, a reload.** Not needed so far. Would use §2 re-entry and the telemetry values.

**Safety net, independent.** A host that quits by menu or Alt-F4 keeps its process running hidden until the match ends. Probably not needed if HM-1e passes. Not started; Leonardo decides.

**Rejected for now:** a dedicated host PC (DECISIONS 2026-10-21).

## 4. Where the code goes

In `nrpvp`, not a separate project. Dev only: `session`, `mobs_raw`, `mobs_dump`, `players_pos`, `hm_leave_lobbies`. In every build: the roster fix, the host flag, the role log. No hook on the game for any of this. A second DLL would have to rebuild the loader, console, `nro` hooks, `net::bus` and memory views, and would compete for the four hardware breakpoints (TRAPS). Migration also has to be tested against the mod's own host-dependent rules (33 source files mention the host).

## 5. Reproduce

Steam closed, `nro desk` in a terminal, the mouse nudged so the screensaver stays off (TRAPS), about 21 GB of free RAM.

```text
target\release\nro.exe stop --all
target\release\nro.exe run scenarios\mi-host-kill.toml      # three instances, one expedition, stay open
target\release\nro.exe cmd hawk_drop --i 2                  # same for 1 and 3
target\release\nro.exe cmd "night_count 0" --i 2            # same for 3: our match rule cannot end it
target\release\nro.exe cmd hm_leave_lobbies --i 1             # the owner moves, as on real Steam (leave this out for 1b)
target\release\nro.exe stop --i 1                           # kill the host
target\release\nro.exe cmd session --i 2                    # role, protocol, players; also players_pos, mobs_raw
```

Or the whole thing as one test: `target\release\nro.exe run scenarios\mi-host-migration.toml`. Every build logs `session: role ...`. Watch the role field by hand: `trace_write <session address + 0xC>`; hits appear as `T-041 FLAG WRITE`.

## 6. Plan and budgets

Estimates are `[HYPOTHESIS]`. **Past a budget: stop and ask Leonardo.**

| Step | Content | Done when | Budget |
|---|---|---|---|
| HM-0 | Live study | **Done 2026-11-02** (menu quit not run) | |
| HM-1a | Find where the session end comes from | **Done 2026-11-02** (§2) | |
| HM-1b | Hold the session | **Not needed so far:** the owner change prevents the end (§1a) | |
| HM-1c | 10 minutes together | **Session held 10 min, rule on:** positions exact, clocks agree, no reset. The enemy count differs; explained by what is loaded around a player (§1a) | |
| HM-1d | The new host's authority | **Done on one PC** (§1a) | |
| **HM-1e** | **Real Steam: FS-HM1, TESTING §6** | Same as §1a with a real crash | 1 Friend Session |
| HM-1f | Mod fixes of §3, a real item drop after a migration, a defeated host who leaves | **Done 2026-11-03** (`mi-host-migration` PASS) | |
| HM-5 | What FS-HM1 leaves open; release | Host killed mid-match, the match goes on, on real PCs | Depends on HM-1e |
| HM-2, HM-3 | Own sign-in and session, parity with SC at 6. **Only if SC blocks the migration on real Steam** | `mi-nosc-join` PASS, regression set without `nrsc.dll` | 1-3 months |

**Stop and decide with Leonardo if:** SC turns out to force the end; a write to the role crashes or returns to the title and no way around shows up within the budget; real Steam behaves differently enough to invalidate the hold.

## 7. Open unknowns

| # | Unknown | How to find out |
|---|---|---|
| 1 | **Does the owner move on real Steam after a crash, and within the grace time?** | HM-1e |
| 2 | Value of `+0x49C`, state of bit 1 of `+0x60` on real Steam | HM-1e |
| 3 | Does SC interfere with the owner change on real Steam? Under the emulator it did not | HM-1e |
| 4 | Who runs an enemy | When a real fight shows a problem |
| 5 | Why enemy HP and count change when the host goes (session size is a suspect) | When a match shows it matters |
| 6 | 4 or more players, real hits between players after the change | FS-HM1 |

## 8. Player count (parked)

Target up to 12, later, on good results. **Migration does not make it much easier:** migration is about who runs the world; player count is about slots, per-slot data, network load and leaving SC. They share only the session knowledge.

| Finding | Tag |
|---|---|
| **12 cannot run on top of SC without patching it**, which hard rule 3 forbids: SC's member table has 5 entries and its sweep covers slots 1..5 (MEMORY_MAP §18). So it needs the own session layer (HM-2, HM-3), then widening | `[VERIFIED: live code read]`; the conclusion is our reading |
| Engine bounds read so far allow it: remote id -1..0x17 (`exe+0x20BB20`), generic player-set creator scans 20 entries (`exe+0x4ABC40`), `CSFixedList<MEMBER_LEAVE_INFO_FOR_SERVER_LOG, 30>`. Bound at 2 remotes: the member getter and lookup (`exe+0x1FE530`, `0x1FE560`) | `[VERIFIED: disassembly, RTTI]` |
| Not counted yet: per-slot data, sync buffers, the mod's own 6-slot assumptions | UNKNOWN |
| SC's author reports bugs above 5 players | `[SOURCE: PC Gamer 2025]` |
| A home connection may not carry 12 players; one PC cannot test it beyond 3 instances. A replay tool from NET-3 captures, then growing Friend Sessions | `[HYPOTHESIS]` |

If the sign-in needs real server answers at many steps: try a local stand-in, as Waygate does for ER `[SOURCE: github.com/vswarte/waygate-server]`, or stay at 6 with SC. Risk and guards of the sign-in work: NETWORKING §7a, DECISIONS 2026-11-02.
