# T-003: Nightreign Seamless Co-op (NR SC): features, settings, terms

Researched 2026-10-09. Tags: `[SOURCE: ...]` external, `[VERIFIED: how]` checked on this machine (read-only),
`[HYPOTHESIS]`, `UNKNOWN`. Everything about Elden Ring (ER) is marked **ER** and is context only.

Abbreviations: NR = Nightreign, SC = Seamless Co-op. Primary page = Nexus "Seamless Co-op (Nightreign)",
mod ID **3** (https://www.nexusmods.com/eldenringnightreign/mods/3). Plain `curl` and WebFetch get HTTP 403 from
Nexus. The pages were read in the built-in browser pane (public pages, no login, cookie banner declined).

---

## 0. Bottom line for the planner

1. **NR SC has no PvP, friendly-fire, team, or invasion feature and no such item.** The feature list says so
   by omission, the author has said he will not add PvP, and the only gameplay-ish ini section is empty.
   Our PvP has to come entirely from our own code (H1). See section 1.
2. **The full ini for the current version (1.13) is exactly what is installed here.** There are no hidden keys.
   Nexus's file preview lists `nrsc_settings.ini` at 246 B, and the local file is 246 B. See section 2.
3. **Current version is 1.13** (Nexus file dated 2026-07-02, compatible with game 1.03.3). The local `nrsc.dll`
   is very likely 1.13. Player cap is **6**, with 4-6 player bugs still listed as known. See section 3.
4. **Loading SC through me3 works in the field**, on SC 1.1.2 with a profile entry that has `load_early = true`
   and no `initializer`. me3's source shows why. The Nexus "me3 can't load SC on NR" story (discussion #118,
   June 2025) is outdated. See section 4.
5. **Terms are restrictive.** No re-upload, no modification, no bundling without the author's permission.
   Use SC strictly as a user-installed runtime dependency and never ship or patch `nrsc.dll`. See section 5.

---

## 1. PvP, friendly fire, teams, invasions (priority 1)

| Finding | Tag |
|---|---|
| The author's own feature list for NR SC is: 1-6 player lobbies, party member visibility in the Round Table Hold, customisable enemy scaling, mod compatibility (EAC not running). It says "The mod is feature complete". No PvP, team, friendly-fire, or invasion entry. | [SOURCE: https://www.nexusmods.com/eldenringnightreign/mods/3 (Description tab), read 2026-10-09] |
| Yui said he has no plans to add PvP or random matchmaking (quoted in PC Gamer, 2025-08-05). He also said the project goes into maintenance mode after the remaining bug fixes. | [SOURCE: https://tech.yahoo.com/gaming/articles/elden-ring-nightreigns-seamless-co-153029980.html (PC Gamer syndication)] |
| A search-result summary of Yui's Patreon post quotes "I have no plans on adding PvP and/or random matchmaking" and says he had earlier looked at duels plus friendly fire, or invasions. I could not open Patreon (blocked in the browser pane, 403 in WebFetch), so this is second-hand. | [SOURCE: https://www.patreon.com/posts/update-elden-2-130430915 via WebSearch snippet, unverified] |
| The FAQ on the NR page has only 3 questions (progress sync, modification permission, random matchmaking). Nothing about PvP. | [SOURCE: Nexus NR mod 3, Description] |
| The shipped English locale has only lobby, version-mismatch, and pause strings: "Solo Mission", "Up to 6 players", "host is not using seamless co-op", "host version ... does not match", "Paused", "Resumed". It has no item, PvP, or rule strings. | [VERIFIED: read `...\Game\SeamlessCoop\locale\english.json`] |
| The only in-game controls named anywhere are a **host pause key, F4**, which halts night progress (added 1.07, usable solo from 1.09). | [SOURCE: Nexus NR mod 3, Changelogs 1.07 and 1.09] |
| `[GAMEPLAY]` is an empty section in the shipped ini. NR SC does not use it for anything today. Whether it is reserved for future options is UNKNOWN. A "feature complete" mod in maintenance mode makes new options unlikely. | [VERIFIED: local ini] [HYPOTHESIS: unlikely to gain keys] |
| I found no third-party NR PvP, friendly-fire, or team mod built on SC. Searches covered Nexus, Google-style queries, and GitHub (`yuiamoroll` repos). I did **not** browse the Nexus list "Mods using this mod (47)", because its table loads lazily and never rendered. | [SOURCE: WebSearch, no hits] UNKNOWN: contents of the 47-mod list |
| "Nightreign 6 Player Fixes" (Sayu) is a companion mod for >3-player groups: map, script, and event tweaks, with rune scaling down at 4/5/6 players. It is not PvP. It shows that event and param edits layered on SC are an accepted pattern. | [SOURCE: https://www.nexusmods.com/eldenringnightreign/mods/596 (search snippet)] |

### ER comparison (context only, **ER**)

- **ER:** the host uses the "Judicator's Rulebook" item to toggle friendly fire and PvP modes. The hairpin item
  lets a player invade; `allow_invaders = 1` in `ersc_settings.ini` allows invaders.
  [SOURCE: https://www.nexusmods.com/eldenring/mods/510 (Description), read 2026-10-09]
- **ER:** the invasion and PvP code exists in the game engine, and SC only toggles and routes it. NR has none of
  those items, and its SC has no equivalent toggle. Whether the NR engine still has dormant invasion paths is
  what our experiments test. [HYPOTHESIS]
- **ER:** the SC author says certain runtime regulation edits and debug flags (his example: player speed
  multiplier) are disabled by SC's "minimalist anticheat", and he will not remove this.
  [SOURCE: https://www.nexusmods.com/eldenring/mods/510 FAQ]
  [HYPOTHESIS: NR SC has the same behaviour, so test that our `team_type` writes and any param writes survive
  under SC. This is a risk for T-007, T-008, and T-012.]

---

## 2. `nrsc_settings.ini`: complete key list (priority 2)

### Current version (1.13)

```ini
[GAMEPLAY]            ; empty, no keys

[SCALING]
health_scaling  = 100
damage_scaling  = 100
posture_scaling = 100

[SAVE]
save_file_extension = co2   ; alphanumeric, limit 120 (per the shipped comment)
```
[VERIFIED: local `SeamlessCoop\nrsc_settings.ini`, 246 bytes]

Why this is the complete 1.13 key set: the Nexus file preview of the 1.13 package (file "Nightreign Seamless
Co-op v1.1.3") lists `nrsc_settings.ini (246 B)`, `nrsc.dll (8.8 MB)`, `english.json (536 B)`,
`nrsc_launcher.exe (204.5 KB)`, and `crashpad_handler.exe (805.5 KB)`. The local sizes match to the byte or to
the KB shown (246, 536, 209408 B = 204.5 KB, 824832 B = 805.5 KB, 9200656 B = 8.8 MB).
[SOURCE: https://www.nexusmods.com/eldenringnightreign/mods/3?tab=files, "Preview file contents"]
[VERIFIED: `ls -la` of the install] So the installed package is the 1.13 package, and no other keys exist in it.
`nrsc.dll` also contains the plain string "1.13" ([VERIFIED: `grep -a` plain strings]), a weak but consistent
hint.

### Semantics

| Key | Meaning | Tag |
|---|---|---|
| `health_scaling` | The percentage of the **vanilla** per-extra-player scaling formula that is applied. 100 = vanilla. Example from the author: if vanilla gives +25% health per player, then 50 gives +12.5% and 1000 gives +250%. It is not "+100% HP per player". Camp enemies have no vanilla scaling, so it mostly affects mini-bosses and bosses. | [SOURCE: Nexus NR mod 3, Description "Customisable scaling"] |
| `damage_scaling`, `posture_scaling` | Not described separately anywhere I could reach. The description only shows `health_scaling` and says the scaling options are "set up like" it. Presumably the same percent-of-vanilla semantics for enemy damage and posture. | [HYPOTHESIS] UNKNOWN: exact semantics |
| `save_file_extension` | Replaces `.sl2` with the given extension (shipped value `co2`). Save path is `%AppData%\Roaming\Nightreign\<SteamID>\NR0000.sl2` or `.co2`. | [VERIFIED: ini comment] [SOURCE: Nexus NR mod 3, sticky post "Save files are located in..."] |
| (none) player-count key | **Removed.** Old versions had an ini player count. Since 0.03 the party just starts the expedition when ready. | [SOURCE: Nexus NR mod 3, Changelog 0.03] |

### Per-version history (partial, as far as sources allow)

| Version | Ini-relevant change | Tag |
|---|---|---|
| 0.00-0.02 | Ini held a required-player-count setting (key name UNKNOWN; secondary guides say "required number of players") | [SOURCE: https://vgtimes.com/games/elden-ring-nightreign/files/83987-.html] UNKNOWN: key name |
| 0.03 | Player count no longer controlled by the ini | [SOURCE: Nexus NR changelog] |
| 1.00 | "Add custom scaling options" (the `[SCALING]` section). Note: 1.04 says "Fixed some issues with duo scaling which caused it to be non-customisable" | [SOURCE: Nexus NR changelog 1.00, 1.04] |
| 1.01-1.13 | No ini changes mentioned in the changelog. When `save_file_extension` or `[GAMEPLAY]` first appeared is UNKNOWN | [SOURCE: Nexus NR changelog] |

Other things in the DLL: plain strings "Settings version is not" and "Settings magic is not" suggest the ini
carries a version or magic check (the shipped ini shows none) and that a stale ini may be rejected.
[VERIFIED: `grep -a` plain strings] [HYPOTHESIS: meaning]. I found no source on whether an update needs the ini
re-created. Config key names are not stored as plain strings in the DLL (previous finding, see
TECHNICAL_NOTES §3). The DLL carries WinLicense protector markers ("WinLicenseVersion", "WLSoftwareVersion"), so
static string search will not reveal more. [VERIFIED: `grep -a` plain strings]

---

## 3. Version, date, player cap, stability (priority 3)

### Current version

| Fact | Tag |
|---|---|
| **Current release: 1.13.** Changelog: "Compatibility with game version 1.03.3". File "Nightreign Seamless Co-op v1.1.3", version field 1.13, uploaded **2026-07-02 13:30**, 5.3 MB. The 21 older files are archived. | [SOURCE: https://www.nexusmods.com/eldenringnightreign/mods/3?tab=files, and the Changelogs section on the Description tab, read 2026-10-09] |
| The Nexus header still shows "Version 1.12 / last updated 02 July 2026, 1:30PM" while the file and changelog say 1.13. The header looks stale. The file timestamp is the same. | [SOURCE: same page] |
| The author's release repo VERSION file marks `>1.13` as the current entry, and its last commit (2026-07-07) is "Update VERSION file for version 1.12 and 1.13". | [SOURCE: https://github.com/yuiamoroll/NightreignSeamlessCoopRelease (VERSION, commit list); the owner "Yui", blog link to Patreon `yuiamoroll`] |
| The VERSION file lists every version from 0.00 to 1.13 with a trailing 0/1 flag. Flag 1 appears on 1.06 and 1.13 only. What the flag means is UNKNOWN. | [SOURCE: same VERSION file] [HYPOTHESIS: block/force-update list] |
| Same repo has data files `BLOCKSYNC`, `ENDLESSNIGHT`, `SUPERNIGHTLORD`, `SUPERNIGHTLORD2`, `SUPERNIGHTLORD3` (tiny numeric lists). The author says SC contacts GitHub for the Everdark/Endless Night "temporary solution", and that the feature fails if GitHub is blocked. So SC makes outgoing HTTPS requests to GitHub. | [SOURCE: repo files; Nexus NR mod 3 Description] |
| The installed `nrsc.dll` (SHA256 prefix 243EEC929A97B71E) has a plain string "1.13", and its size matches the 1.13 package. I consider the installed SC to be **1.13**. TECHNICAL_NOTES §1 can change "UNKNOWN" to 1.13. | [VERIFIED: size match + `grep -a`] [HYPOTHESIS: certainty ~high] |
| The local game exe FileVersion is 1.3.3.0, matching SC's stated compatibility "1.03.3". Comments mention "calibrations" 1.03.4 and 1.03.5, meaning regulation-only updates that do not change the exe. Our regulation.bin dated 2026-04-01 may be one. | [SOURCE: Nexus comments on https://www.nexusmods.com/eldenringnightreign/mods/78?tab=posts] [HYPOTHESIS: naming map] |
| Release timeline: first upload 2025-05-30; 0.0.2 on 2025-06-10 (secondary mirror); 1.00 (scaling, 1-6 players) around June-August 2025; 1.07 reverted to 3 players "undergoing maintenance"; 1.10 re-enabled 1-6; 1.12/1.13 are the latest. | [SOURCE: Nexus NR changelog; https://vgtimes.com/games/elden-ring-nightreign/files/83987-.html] |
| Popularity: 373,635 unique DLs, 880,809 total DLs, 5,956 endorsements (as read). | [SOURCE: Nexus NR mod 3 header] |

### Player cap and stability

| Fact | Tag |
|---|---|
| Lobbies of **1-6** players. Yui says the cap could go higher but more than 6 is overkill, because the game is "comically easy" beyond that. The in-game lobby label reads "Up to 6 players". | [SOURCE: PC Gamer via Yahoo, 2025-08-05] [VERIFIED: english.json] |
| Yui said bugs appear once more than 5 players join a run, so 6 is considered shaky as of Aug 2025. | [SOURCE: same PC Gamer article] |
| Fixes for slots 4-6: 1.02 (dormant power drops), 1.04 (more dormant-power and desync fixes), 1.10 (1-6 restored), 1.11 (relics unusable for 4-6), 1.12 ("Patched some events that could cause issues for players 4, 5, and 6"). | [SOURCE: Nexus NR changelog] |
| **Known issues** from the author's rolling-changelog sticky post (date shown 31 May 2025, may have been edited since): "Some global events may be incorrect in 4-6 player sessions", and "Revenant players in player slots 4, 5, and 6 have issues using their primary power". The "1.1.4 (so far)" list is empty. | [SOURCE: Nexus NR mod 3, Posts tab, sticky] |
| All players must run the **same SC version and same game version**. The locale has an explicit mismatch message. | [SOURCE: Nexus NR description] [VERIFIED: english.json "host version ... does not match"] |
| Nexus comments on the mod page are **locked** (since 2025-06-20). The author directs support to the unaffiliated ER SC Discord. | [SOURCE: Nexus NR mod 3 Posts, sticky] |
| The author does not support pirated copies. | [SOURCE: Nexus NR mod 3 Posts, sticky] |
| Networking is Steam P2P (previously found, strings `SteamNetworkingMessages002`, `SteamMatchMaking009`). Requires Steam signed in and not offline. | [VERIFIED: strings] [SOURCE: Nexus description] |
| No public quantitative crash or stability data found (only the author's statements above and Nexus comments, which are locked). | UNKNOWN |

Version-gating hazard for our project: SC finds game code through byte-pattern signatures. The DLL contains
"incompatible game version: inconsistent pattern result" and the source-path string
`nrsc\signatures\signatures.cpp`. So **a game patch can break SC until the author ships an update**. The
changelog shows Yui doing quick compatibility releases (1.01, 1.05, 1.06, 1.09, 1.13).
[VERIFIED: `grep -a` plain strings] [SOURCE: Nexus NR changelog] Our pins in TECHNICAL_NOTES §1 should treat a
game update as "retest everything".

---

## 4. Loading SC next to other DLL mods and me3 (priority 4)

### What the author says

| Fact | Tag |
|---|---|
| The official path is `nrsc_launcher.exe`. Nothing else is documented on the NR page. There is no me3 section and no mention of load order. | [SOURCE: Nexus NR mod 3, Installation] |
| The author's own message in the DLL says: `Method "%s" is not available. If you're using ModEngine, please make sure seamless co-op is loaded early. The game will now exit.` So SC hooks a game method that must not have been initialised yet. If SC loads late it **exits the game**. | [VERIFIED: `grep -a` plain string in nrsc.dll] |
| `modengine_ext_init` is present in the DLL (two string hits). 0.01 changelog: "Add mod engine extention support". | [VERIFIED: strings] [SOURCE: Nexus NR changelog 0.01] |
| Mod compatibility statement: "Compatibility with other mods has not been tested", with the condition that mods must not interfere with the multiplayer aspect "or edit any of the items the mod uses to function". | [SOURCE: Nexus NR mod 3, Installation] |
| The launcher binary references `SeamlessCoop//nrsc.dll` and `nightreign.exe` and has no obvious DLL-list or other-DLL strings. It looks like a fixed launcher that starts the exe and loads `nrsc.dll`. Whether it can load additional DLLs is UNKNOWN (no decompilation done, per rules). | [VERIFIED: `grep -a` plain strings of nrsc_launcher.exe] |

### What me3 does (source-verified)

| Fact | Tag |
|---|---|
| me3 added "Support loading mods that rely on modengine_ext_init" and "Support Seamless Coop" in **v0.5.0 (2025-06-14)**. Latest release is **v0.13.0 (2026-08-11)**; local install is 0.11.0 (2026-02-28). `load_early` was fixed for multiple natives in 0.11.0 (#634) and documented (#652). | [SOURCE: https://raw.githubusercontent.com/garyttierney/me3/main/CHANGELOG.md, lines 7, 227, 372, 397, 1134, 1181, 1304] |
| In me3's host, a native with **no `initializer` key** is loaded with `libloading`, then me3 looks up `modengine_ext_init` and, if found, calls it with a ModEngine2-compat shim (`ModEngineConnectorShim`, plus an out-pointer for the extension). It then logs "loaded native with me2 compatibility shim". This is identical in v0.5.0, v0.11.0, v0.13.0. | [SOURCE: https://raw.githubusercontent.com/garyttierney/me3/v0.11.0/crates/mod-host/src/host.rs#L69-L96 (and main, L90-L115)] |
| If a native **has** `initializer = { function = "..." }`, me3 instead calls that symbol as `fn() -> bool` with **no arguments** and **skips the me2 shim**. So giving `nrsc.dll` an `initializer = {function="modengine_ext_init"}` (the June 2025 suggestion in #118) would call it with the wrong signature. Do not do that. | [SOURCE: same host.rs] |
| `load_early = true` natives are loaded in `before_game_main`, before the game's own main runs. Natives with an initializer set are loaded later, on a **separate thread** (delayed). Natives without one are loaded immediately, in profile/dependency order. | [SOURCE: https://raw.githubusercontent.com/garyttierney/me3/v0.11.0/crates/mod-host/src/lib.rs#L160-L216 (early_natives loop L160; delayed split L193)] |
| Native fields in the profile schema: `path`, `optional` (default false), `enabled` (default true), `load_before`, `load_after`, `initializer`, `finalizer`, `load_early` (default false). A native's id for `load_before`/`load_after` is its **file name** (e.g. `nrsc.dll`). Each dependency entry is `{ id = "...", optional = bool }` (both required). | [SOURCE: https://raw.githubusercontent.com/garyttierney/me3/main/schemas/mod-profile.md#L32-L53; crates/mod-protocol/src/native.rs (`Dependency for Native::id`)] |
| me3 docs: `start_online` is "not needed for Seamless Co-op"; me3 blocks official matchmaking by default. The profile `savefile` key can compartmentalise saves (it copies a base save if the file does not exist). | [SOURCE: https://raw.githubusercontent.com/garyttierney/me3/main/docs/configuration-reference.md] |
| Discussion #118 (2025-06-09): SC did not load via `--native` because me3 then did not call `modengine_ext_init`. Maintainer: WIP shim fix (this became 0.5.0). A user tested the shim branch on NR SC and it did **not** work **at that time** (June 2025, SC 0.0x). No later reply in the thread. | [SOURCE: https://github.com/garyttierney/me3/discussions/118, read in full 2026-10-09] |
| Discussion #213 (June 2025): attach log shows `nrsc.dll` as a native with initializer `modengine_ext_init` (a different config than the shim path). Failures there came from `require_steam` on a pirated copy, not SC. Fix: `--skip-steam-init` (v0.6.0). | [SOURCE: https://github.com/garyttierney/me3/discussions/213] |

### Field evidence that it works (2025-2026)

| Fact | Tag |
|---|---|
| A user (2026-04-06) with SC 1.1.2, game 1.03.2, "ME3 Manager 1.4.4" (third-party GUI) posted a profile that loads `nrsc.dll` as a native with `load_early = true` and the comment `# Due to: Method "SignIn" is not available.`, then other DLLs and packages after it. He reports "Seamless works". Profile keys used: `profileVersion="v1"`, `savefile="ME3NR.sl2"`, `start_online=false`, `disable_arxan=false`, `[[supports]] game="nightrein"` (a valid me3 alias for nightreign), natives with `path` and `load_early`, packages with `id` and `path`. (`config = ...` lines are ME3 Manager extras, not in me3's schema.) | [SOURCE: https://www.nexusmods.com/eldenringnightreign/mods/78?tab=posts, comment "demondobf"] |
| An earlier user (2025-08-11) had SC as a plain `[[natives]]` entry (no `load_early`) next to other DLLs, and hit failures only when SC was missing from the profile after an update. After adding it again, all worked. | [SOURCE: same page, comment "aesthesis"] |
| me3's official "creating mod profiles" guide lists NR DLL mods but not SC. It recommends `[[supports]] game = "nightreign"`, `[[natives]] path = '...'`, with paths relative to the `.me3` file. | [SOURCE: https://raw.githubusercontent.com/garyttierney/me3/main/docs/user-guide/creating-mod-profiles.md] |
| **ER** parallel: Nightreign Revive (an **ER** mod using ER SC) tells users to add its early bootstrap DLL with `Load Early` and a named initializer function, and its main DLL with `Load After` the SC DLL (`ersc.dll`) and others. Pattern only; this is ER. | [SOURCE: https://www.nexusmods.com/eldenring/mods/11175 (Description)] |

### Recommended profile shape for T-004 (derived, not yet tested)

```toml
profileVersion = "v1"
# savefile: leave unset; SC already uses its own .co2 via nrsc_settings.ini

[[supports]]
game = "nightreign"

[[natives]]
path = 'Z:\...\Game\SeamlessCoop\nrsc.dll'     # real path to the installed DLL, not a copy
load_early = true                              # SC exits the game if loaded late
# NO initializer key: me3 then calls modengine_ext_init via its me2 shim

[[natives]]
path = 'dist\nrpvp.dll'
load_after = [ { id = "nrsc.dll", optional = false } ]
# NO load_early. Our DLL needs nothing special; DllMain just spawns a thread.
```
[HYPOTHESIS: synthesised from the sources above. Syntax of `load_after` entries is from the schema: `id` and
`optional` both required. Still needs a HUMAN launch to confirm. The profile does not need `start_online`.]

Notes:
- Keep SC's DLL in its install folder (`SeamlessCoop\`, next to `nrsc_settings.ini` and `locale\`). That is the
  shipped layout [SOURCE: Nexus file preview]; whether SC accepts a relocated DLL is UNKNOWN.
- `load_early` ordering: early natives are loaded before the main game code, so SC loads before `nrpvp.dll`
  even without `load_after`. Keep `load_after` anyway for determinism.
- Do not set an `initializer` on `nrsc.dll` (see above). Do not set `start_online`.
- Everyone in a session must launch the same way or at least with matching SC and game versions.
- Evidence gap: no source shows `load_early` + me3 0.11.0 + NR 1.3.3 + SC 1.13 together. The nearest is SC
  1.1.2 + game 1.03.2 + an unknown me3 version (ME3 Manager 1.4.4 bundles its own). me3 0.13.0 exists. Consider
  checking whether to upgrade me3 if 0.11.0 misbehaves. UNKNOWN
- Fallback (if me3 loading fails): the stock launcher is the only author-documented path, and no source describes
  proxy-DLL loading for NR SC. Sources describing Online-Fix `dlllist.txt` (pastebin) are for cracked copies and
  out of scope.

---

## 5. License, permission, redistribution (priority 5)

| Item | Detail | Tag |
|---|---|---|
| Distribution channels | Nexus (mod 3) as the main channel; Yui's GitHub `yuiamoroll/NightreignSeamlessCoopRelease` holds only data/flag files and **no license file** (`license: null`, no releases). Source is closed. | [SOURCE: https://api.github.com/repos/yuiamoroll/NightreignSeamlessCoopRelease] |
| Upload permission | "You are not allowed to upload this file to other sites under any circumstances" | [SOURCE: Nexus NR mod 3, Permissions and credits] |
| Modification permission | "You must get permission from me before you are allowed to modify my files to improve it" | [SOURCE: same] |
| Conversion permission | Not allowed to convert to other games | [SOURCE: same] |
| Asset use | Permission required; not allowed in sold mods or for donation points | [SOURCE: same] |
| Author FAQ #2 | "Please do not modify, redistribute, or bundle this mod without my permission. Please send me a message if you'd like to re-upload it elsewhere." | [SOURCE: Nexus NR mod 3, Description FAQ] |
| Use as a dependency | Not addressed in the permissions. Precedent: Nexus lists **47 mods "using this mod"** and says "The Nexus Requirements tab is mods that require Seamless Coop"; other NR mods tell users to install SC separately and run beside it. The author states you may use other mods with SC. | [SOURCE: Nexus NR mod 3 (Description; "Mods using this mod (47)")] |
| Ban safety | "Can this ban me? No... There's no way to get banned using this mod unless you modify it with the intent of connecting to vanilla players." Separate save files. | [SOURCE: Nexus NR mod 3] |
| Takedown sensitivity | Yui refused to unlock Everdark bosses ahead of vanilla players, because giving seamless players content early is "a good way of getting the mod taken down". He cares about not drawing FromSoftware attention. | [SOURCE: Nexus NR mod 3 Posts, comment thread 2025-06-19, Yui reply] |
| Local file note | The SC readme text does not ship in the package (files are only the five listed in section 2). No license text locally. | [VERIFIED: install listing] |

Implications for us (planner to confirm):
- **Never bundle, mirror, patch, or redistribute `nrsc.dll`/`nrsc_launcher.exe`/the ini.** Document "install SC from
  Nexus yourself". Same rule as CLAUDE.md hard rule 3.
- Running a **separate** DLL beside SC that does not alter SC's files looks like normal dependent-mod use
  [HYPOTHESIS]. Hooking or patching code **inside** `nrsc.dll` could count as modifying it, so avoid it.
- If PvP is ever released publicly, asking the author is cheap (Nexus message or Patreon, per his own FAQ)
  and he has publicly rejected PvP himself. A PvP mod that flips game team types could be seen as undesirable
  by him. Treat the permission and tone question as an open question, not an assumption.
- A public PvP mod also touches the author's stated safety claim ("no way to get banned... unless you modify
  it with the intent of connecting to vanilla players"). Our mod must stay offline-from-matchmaking, as in CLAUDE.md
  hard rule 2.

---

## 6. Other facts useful for the build (from plain strings and the page)

- SC contains source-path strings `nrsc\cs\world_chr_man\world_chr_man.cpp`, `...\bloodstain_man\...`,
  `...\auto_control_api\...`, `nrsc\mod\param_repository\param_repository.cpp`,
  `nrsc\mod\server_manager\server_manager.cpp`, `nrsc\mod\preferences\preferences.cpp`,
  `nrsc\hooks\hooks.cpp`, `nrsc\signatures\signatures.cpp`, plus RTTI for `CS::WorldChrManImp` (the
  `_UpdateMultiDoping` lambda), `CSQuickMatchLobbyContext`, and the `YuiNexusNetRankingDB` string. So **SC
  hooks `WorldChrManImp` and edits params at runtime** (`param_repository`). This overlaps the areas we plan to
  touch (team type, WorldChrMan), so expect interactions. Hook-conflict risk is HYPOTHESIS.
  [VERIFIED: `grep -a` plain strings, nrsc.dll]
- The debug PDB paths embedded in both binaries are `C:\Users\Yui\source\repos\nrsc\...\nrsc.pdb` and
  `C:\Users\Luke\...\nrsc_launcher.pdb`. Informational only. [VERIFIED: plain strings]
- Scaling is applied per additional player from the vanilla formula, so it should apply for any extra
  players, including those we later place on other teams. How SC scaling relates to teams is UNKNOWN.
- The 6-player lobby was added to the vanilla UI strings by SC itself ("Up to 6 players" for both the 2- and
  3-player options), so the vanilla expedition-type menu is reused. [VERIFIED: english.json]
  [SOURCE: Nexus 1.04 changelog: "Two player and Three player options ... are no longer relevant"]

---

## 7. Conflicts with, or additions to, TECHNICAL_NOTES.md §1 and §3

| Notes entry | Finding |
|---|---|
| §1 "version number UNKNOWN" | Version is almost certainly **1.13** (size-matched package, plain string "1.13"). |
| §3 "has mentioned PvPvE-style modes as a *possible* future" | The PC Gamer article, which says "no plans to add PvP and/or random matchmaking", is dated 2025-08-05; the Patreon snippet says he once explored duels/friendly fire/invasions and dropped it. The PvPvE-possible wording is out of date (rutab article was from the same day, 2025-08-05; the newer wording is the stronger one). |
| §3 "ER SC had PvP toggles... NR equivalent UNKNOWN" | Now answered: **none in NR SC** (section 1). |
| §3 "empty `[GAMEPLAY]`... NR keys UNKNOWN" | Now answered: the 1.13 ini has **no** other keys (section 2). |
| §3 "me3 + SC: ... Current behavior on 0.11 is UNKNOWN" | me3 source (0.11.0) supports the shim path, and field evidence says SC 1.1.2 works through me3 with `load_early = true` (section 4). Still needs our own T-004 launch. |
| §3 "Exports or expects `modengine_ext_init`" | Confirmed, plus the author's message that SC must be loaded early, or the game exits. |
| §1 me3 0.11.0 | me3 0.13.0 (2026-08-11) is the latest, but 0.11.0 contains everything needed for SC. |
| New | SC talks to GitHub for version/Everdark/Endless Night data. SC needs matching SC + game versions on every client, and a game patch can break it via signature scanning. |

---

## 8. Open questions

1. Does `load_early = true` + me3 0.11.0 + NR 1.3.3 + SC 1.13 actually work here? (T-004, HUMAN.) Does SC survive
   `nrpvp.dll` being loaded after it?
2. What do `damage_scaling` and `posture_scaling` do exactly? (Low priority; only affects balance.)
3. What does the 0/1 flag in the author's `VERSION` file mean, and does SC refuse to run when its own entry
   is flagged? Does SC block play when GitHub is unreachable? (Matters if a test PC is offline or behind a
   block.)
4. Does SC NR block runtime param or memory edits (ER SC's "minimalist anticheat")? Must be checked by the first
   `team_type` write experiment (T-008/T-012), not assumed either way.
5. Does SC overwrite `team_type` or `ChrTemporaryTeamTypeSlot` on spawn or on 4-6 player events? Its RTTI
   includes `EcTestTeamType`, `WorldChrManImp`, and `ChrFinderImp::_UpdateMultiDoping`.
6. Which of the 47 mods "using this mod" modify gameplay with SC (e.g. any that touch teams)? The list
   did not load, so a human can read it in a normal browser.
7. Would the author object to a PvP mod built on SC, and does using SC purely as a loaded dependency need
   permission? Not stated anywhere; ask only when the project nears a public release.
8. Are there unlisted SC options or a settings-version mechanism ("Settings version is not...") that rewrites
   the ini on updates? Check by diffing the ini after the next SC update.
9. Is the Nexus header (1.12) or the file (1.13) the truth for the exact build shipped? Hash comparison against the
   Nexus download would settle it (requires a manual download, which needs the human's Nexus login).
