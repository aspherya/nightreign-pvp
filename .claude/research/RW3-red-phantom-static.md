# RW-3: the red phantom (and white phantoms), static reading of nightreign.exe 1.3.3.0

Date 2026-10-31. Read-only static work. **Method:** Python 3.12 + capstone on the file, using a copy of
`Z:\Tools\rea\elden-ring-nightreign\tools\nr.py` (RTTI walk, RIP-relative xrefs, call xrefs). Ghidra/REA was **not** used: no
snapshot of `nightreign.exe` exists (`Z:\Tools\ghidra-projects\nightreign` is an empty project, and STATUS has no snapshot path), and
a first analysis of the 118 MB exe takes hours. All addresses are RVAs (`exe+0x...`). Tags: `[VERIFIED: static disasm]` means
"these instructions are in the file" (the call, the offset, the constant). What the code *means* is `[HYPOTHESIS]` until a live run
confirms it. Part of the code is Arxan-obfuscated (jump chains); where it was, I followed the jumps by hand.

## 0. The main finding (it changes the plan)

**The red phantom is not made from a death in your own session.** It is the game's **asynchronous online "DeathBox" feature**.
When a player dies, the game uploads a record of that player's gear to FromSoftware's server. Other players' games download a list
of these records and show one of them as a kneeling red phantom. Opening it drops that gear.

- Strings and RTTI: `CSDeathBoxInsManImpl` (FD4 singleton `CSDeathBoxInsMan`), `CSDeathBoxUploadJob`, `CSDeathBoxDownloadJob`,
  `CSDeathBoxDownloadAndSelect`, `CSDeathBoxNamedWeapon{Download,Update,Delete}Job`, `CSDeathBoxUserInfoDownloadJob`,
  `CSDeathBoxManager::Update` / `_CreateAddSaveDataJob(CS_DEATHBOX_DATA const&)`, `FromNet::FNDeathBox(Impl)`, server requests
  `RequestCreateDeathBoxDataParams`, `RequestGetDeathBoxDataListParams`, `RequestGetDeathBoxUserInfoListParams`,
  `RequestUseDeathBoxLogParams`, `Request{Get,Update,Delete}NamedWeaponParams`, `{"deathbox_all":` / `,"deathbox_opened":`
  `[VERIFIED: static string/RTTI scan]`.
- The official starter guide says players meet these "echoes from other worlds" only when playing online. GameWith says offline play
  is solo only. The Grief bonus on dropped weapons comes from these looted phantoms.
  `[SOURCE: https://media-center.bandainamcoent.eu/games/elden-ring-nightreign/ERN_FuturePress_StarterGuide.pdf ; https://gamewith.net/eldenring-nightreign/57337 ; https://gaggula.pages.dev/posts/all-character-grief-passives-stat-bonuses-in-elden-ring-nightreign]`
- **So the hold is not why we see nothing.** Vanilla never makes a red phantom for a player in the same session. The upload starts
  inside the local death handler, *before* the step we hold (§2). It returns at once unless the game is in online mode, and we
  never go online (CLAUDE.md rule 2). The download side needs the server. **We cannot get the vanilla path to run.** What we can
  reuse is the local half: the phantom display, the `F` prompt and the drop into `MapItemMan`. §5 ranks the options.

## 1. What the object is

| Piece | What it is | Evidence |
|---|---|---|
| `CSNetManImp` (slot `exe+0x3C046A8`, ctor `exe+0x1867B0`, vtable `exe+0x2BDA5D8`) | Owns the online databases. Ctor of its children `exe+0x1883C0`: `+0x38` `CSNetWanderingGhostDb` (white phantoms), `+0x40` `CSNetBonfireDb`, **`+0x48` the DeathBox manager** (0x128 bytes, ctor `exe+0xB10810`, no RTTI; the lambda names say `CSDeathBoxManager`), `+0x50`, `+0x58` (used by `CSShareClearDataInsMan`), `+0x60`, `+0x68`, `+0x70` | `[VERIFIED: static disasm]` |
| DeathBox manager (`[CSNetMan+0x48]`) | Holds the downloaded records: `exe+0xB12E30` returns `mgr+0x48`, an MSVC `std::list` (record at node `+0x10`). Named-weapon (Grief) data is in a `std::map` at `mgr+0x68`, keyed by u64 (lookup `exe+0xB12C80`). The job list is at `mgr+8`. `exe+0xB12E80(mgr, id)` sets list node `+0xBD = 1` (opened) | `[VERIFIED: static disasm]`; the roles are `[HYPOTHESIS]` |
| `CSDeathBoxInsManImpl` (vtable `exe+0x2C1C760`, ctor `exe+0x53DA90`, dtor `exe+0x53E0D0`, size 0x90; name string `exe+0x2C1C718`; also a `CSWorldShiftFragmentProxy<CSDeathBoxInsManImpl>`) | The in-world manager. `+0x08` = `std::map<u64 id, Instance*>` (head at `+0x10`, value at node `+0x28`). `+0x60` = **one `CSDisplayGhost`** (0x2C0 bytes, ctor `exe+0x4E1400`). `+0x68` / `+0x78` = SFX handles. `+0x70` = an SFX/dummy id. `+0x80` = the instance shown now. `+0x88` = **50.0 f** (show radius). `+0x8C` = state 0/1/2 | `[VERIFIED: static disasm]` |
| Instance (0xC8 bytes, built by `exe+0x53C410` → base copy `exe+0x53C590` from a downloaded record) | One red phantom: the `CS_DEATHBOX_DATA` record plus 3 fields | Layout below |

Instance / record layout (offsets from the copy in `exe+0x53C590` and the readers) `[VERIFIED: static disasm]`, meanings `[HYPOTHESIS]`:

| Off | Type | Meaning (how used) |
|---|---|---|
| +0x00 | u64 | **DeathBox id** (map key, network message payload, DB lookups) |
| +0x08 | u32 | Character (Nightfarer) id: passed to the display ghost. The upload fills it from `PlayerGameData+0x868` (or `+0xE8` when that is -1) |
| +0x0C | u32 | Map/block id (decoded like `mAA_BB_CC_DD`, `exe+0x6772B0`). With `+0x10` it is turned into world coordinates (`exe+0x631530`) |
| +0x10 | f32×3 | Position inside that block |
| +0x1C | wchar[16] | **Dead player's name.** Put into text tag 0x147 before the prompt (`exe+0xE61A20`) |
| +0x3C / +0x3E | u16 / u8 | UNKNOWN |
| +0x40..+0x58 | vector<u64> | **Named-weapon ids** (Grief weapons, resolved in the DB map `+0x68`) |
| +0x60..+0x78 | vector<{u32 item id, u16/u32 count}> | **Plain items** (ids carry category bits, e.g. `0x20000000` accessory) |
| +0x80 | 0x20-byte sub-object (copy `exe+0x131820`) | Input to `exe+0xAF3400` → `+0xBC` and `exe+0xAF27A0` → `+0xC0` |
| +0xA8 | f32 | Yaw (display and drop rotation) |
| +0xAD | u8 | **Opened** (set once; blocks a second open) |
| +0xB8 | u32 | From `exe+0xE78310`; passed to the display (appearance/animation, `[HYPOTHESIS]`) |
| +0xBC | u8 | Variant flag: uses action button 1031 instead of 1030 and a second SFX |
| +0xC0 | u32 | Variant value (sent with the action-button request) |

## 2. Creation

**Upload: the dying player's own game, inside the death handler.**
- The local death handler `exe+0x5C2770` (MEMORY_MAP §17) calls **`exe+0xB14370([CSNetMan+0x48])` at `exe+0x5C296B`**. This is
  early in the handler: it comes right after it ORs `+0x58 |= 2` and calls `exe+0x611450` / `exe+0xF7EEB0` / `exe+0xF7EF00`,
  and well before the comeback steps (`exe+0x5C4810` start, which we hold) `[VERIFIED: static disasm]`. So **holding the comeback
  does not stop it**.
- `exe+0xB14370` returns at once unless `byte [[exe+0x3C078D0]+0xD8] == 2` (`exe+0x1FE1C0`) and `byte [[exe+0x3C078D0]+0xDE] <= 3`
  (`exe+0x1FE750`). It also skips the upload when the map id is `0x0C000000`. Otherwise it fills a record (`exe+0xB0DF60`) and
  queues a `CSDeathBoxUploadJob` (`exe+0xAFA520`) on the job manager at slot `exe+0x3C17248`.
  `[VERIFIED: static disasm]`; "`+0xD8 == 2` means online mode" is `[HYPOTHESIS]` (singleton `exe+0x3C078D0` is not named yet).
- **What it snapshots** (`exe+0xB0DF60`), all from LOCAL (`[WorldChrMan]+0x174E8`) → vf `0x190` = `PlayerGameData`:
  `+0x00` u8 = PGD`+0xC7`; `+0x04` = PGD`+0xEC`; `+0x08` = PGD`+0x868` (or `+0xE8`); `+0x0C` map id (`exe+0x66B7C0`);
  `+0x10` position (`exe+0x66B800`); `+0x20` name (`exe+0x66F8F0`); `+0x50` = (`exe+0x1FE740() == 2`);
  **6 weapon slots** from the equipment block at PGD`+0x1D8`+0x98 (`exe+0x381890`, 0x20-byte entries, some skipped by
  `exe+0x6A50C0` and by bit 0 of `+0x17C`) → vector `+0x58`; **5 accessory slots** (`exe+0x381350`, id | `0x20000000`, count 1)
  and **10 quick-item slots** (`exe+0x1F2610(PGD+0x1D8, 0x20+i)` + inventory count) → vector `+0x78`. Valid only with 1..6 weapons
  and ≤ 15 items `[VERIFIED: static disasm]`. Runes are not in it.
- **Downed vs dead:** the upload hangs off the death handler (the Lua event "death" callback), which runs on a real death only
  (MEMORY_MAP §17: the handler runs on death, then the comeback). `[HYPOTHESIS]`: not uploaded when merely downed.

**Download and appearance: every game, from the server.** `CSDeathBoxDownloadAndSelect` (vtable `exe+0x2C74060`, built in
`exe+0xB05CD0`, called from the manager at `exe+0xB12876`) fills the manager's list. On world/fragment load, the InsMan vtable thunk
`exe+0x53F430` → **`exe+0x53DE40` rebuilds the instances**: for each list node it calls `alloc(0xC8)` + `exe+0x53C410(node+0x10)`
and inserts the result into the map at InsMan`+8` keyed by the id `[VERIFIED: static disasm]`. Nothing in this path is a
same-session message. A teammate's death adds nothing to the list `[HYPOTHESIS, strong: the only writers found are the server jobs]`.

**Per-frame logic** (InsMan update `exe+0x53E600`, called at `exe+0x637EEE`):
1. Among all instances, find the nearest to LOCAL within InsMan`+0x88` (50 m) (`exe+0x53CC30`).
2. If it changed: `exe+0x53CDC0` builds the display data (world transform from `+0x0C`/`+0x10`, yaw `+0xA8`, `+0x08`, `+0xB8`,
   `+0xBC`). Then `CSDisplayGhost::show` `exe+0x4E2700` → state 1. Hide is `exe+0x4E2810`.
3. State 1, when the ghost is ready (`exe+0x4E2640`): attach SFX through `CSSfxImp` (slot `exe+0x3C2A098`, `exe+0xEDCE20`). The
   SFX ids are fields `+0x194` / `+0x19C` (and `+0x198`) of **row 0 of param #0x29** (`exe+0xE94490`, repository slot
   `exe+0x3C28880`). Then state 2.
4. State 2, not opened: `exe+0x53D510` (the interaction, §3). If it returns true: **`exe+0x53D070` (open + drop)**.
5. Message loops: `exe+0xE0C740` (requests). When `[CSSessionManager]+0xC == 3`, it looks up the id and calls `exe+0x53D070`.
   `exe+0xE0C4B0` ("opened" notices) calls `exe+0x53CD60` (sets `+0xAD = 1` and the DB node flag).
   `[VERIFIED: static disasm]`; "state 3 = host, 6 = client" is `[HYPOTHESIS]`.

## 3. Interaction and loot

- **Prompt** (`exe+0x53D510`, Arxan-wrapped, entry → `exe+0xC2F4C8` → `exe+0x6C15286` → `exe+0x53D546`): it skips when `+0xAD` is
  set, needs LOCAL, and uses **ActionButtonParam id 1030 (`0x406`), or 1031 (`0x407`) when `+0xBC` is set**. It goes through
  `CSActionButtonRegionSystemImp` (slot `exe+0x3C17858`): `exe+0xB8FA20` checks the region, then `exe+0xE61A20(text, 0x147, name)`
  puts the dead player's name into text tag 0x147, then `exe+0xB8FC70(req)` registers the prompt and reports a press
  `[VERIFIED: static disasm]`.
- **Who may interact:** there is no team or hostility check in that path. Any LOCAL player whose game holds the instance can press
  it, but only the one nearest phantom within 50 m is shown and active `[VERIFIED: static disasm]`.
- **Open + drop** `exe+0x53D070(instance)`:
  - If `[CSSessionManager]+0xC == 6`: it sends session message **type 0x6E** (8-byte id, `exe+0xE0D2C0` → `exe+0xDF2550`) and does
    nothing else. The client asks the host.
  - Otherwise: it sets `+0xAD = 1` and sends **type 0x6F** (id, `exe+0xE0D170` → `exe+0xDF24D0`), which tells the others. Then it
    builds a vector of 0x50-byte item entries:
    - named weapons: `exe+0x56DED0` init, `exe+0xB12C80` DB lookup, `exe+0xB0E390` fill;
    - plain items: `exe+0x56DED0` + `exe+0x578A60(entry, &id, count)`; entry `+0x00` id, `+0x48` u16 count.
  - Then it calls **`exe+0x576110(MapItemMan, &vector, &pos (+0x10), &mapId (+0x0C), yaw (+0xA8))`**, with `MapItemMan` =
    `[exe+0x3C10B68]`. That is its only caller.

  `[VERIFIED: static disasm]`. So the gear lands as ordinary **`MapItemMan` item records**, the same container as the boss orb
  (MEMORY_MAP §22), but through a different creator (not the item-lot award). In a session the **host** makes the drop
  `[HYPOTHESIS]`. This matches RW-5's finding that a client's pick needs the host's copy.
- **Pickup / sharing:** ordinary map-item pickup. Whether the host's drop shows on clients is UNKNOWN; it is likely, given the
  0x6E/0x6F design.

## 4. Persistence / cleanup

- Instances live in the InsMan map until the next rebuild (`exe+0x53DE40` on a world/fragment change) or InsMan destruction.
  Clean-up helpers: `exe+0x53F7C0`, dtor `exe+0x53E0D0` (frees the display ghost through allocator `exe+0x3C2D9C0`)
  `[VERIFIED: static disasm]`.
- An opened phantom stays flagged (`+0xAD`, DB node `+0xBD`); it is not deleted on open. `exe+0x53CB70` reports to the DB
  (`exe+0xB123A0` / `exe+0xB12190`, probably the use log `RequestUseDeathBoxLog`) `[HYPOTHESIS]`.
- Lifetime of the dropped items: whatever `MapItemMan` does with drops (RW-5 saw 36 s to minutes, cause UNKNOWN).
- Link to the expedition: the whole list comes from the server for the current world. UNKNOWN whether it is cleared per expedition.

## 5. How we could make it appear for a defeated rival (ranked by cost)

1. **Drop only, no phantom (cheapest).** The victim's game sends the gear list (ids + counts, from the same PGD sources as
   `exe+0xB0DF60`) and the death position over `net/bus.rs`. **The host** builds 0x50-byte entries (`exe+0x56DED0` +
   `exe+0x578A60`). Then it calls `exe+0x576110(MapItemMan, &vec, &localPos, &mapId, yaw)`. The victim's game removes the items
   from its own inventory (a separate problem, S2 E3).
   - Needs: the vector type (MSVC `std::vector` with the allocator the callee frees with). The pos/mapId convention can be read from
     a live instance, or by using block-local coordinates the way `exe+0x53CC30` does. Plus an inventory-remove call.
   - Risk: low/medium, one game function. Open question: whether clients see the host's drop.
2. **Full red phantom by record injection (medium/high).** Build a `CS_DEATHBOX_DATA` record (layout §1) with the same new id on
   **every** game (from the bus), using only the plain-item vector (`+0x60`) and leaving named weapons empty. Then either insert a
   node into `[CSNetMan+0x48]+0x48` and trigger the rebuild (`exe+0x53DE40` via the InsMan vtable), or call `alloc(0xC8)` +
   `exe+0x53C410` and insert into the InsMan map at `+8`.
   - The vanilla update then shows the phantom, the `F` prompt (button 1030, the name in tag 0x147) and the host-side drop, with the
     0x6E/0x6F messages. The ids must match on all games so the host can find the record a client asks for.
   - Risks: MSVC list/map insertion with the game's allocator, and the DB's own bookkeeping. Whether the instance code reads the DB
     later and finds no record is UNKNOWN (`exe+0x53CB70`). Whether the phantom model/resources load offline is UNKNOWN. Only one
     phantom shows at a time (the nearest within 50 m).
3. **Let the vanilla path run (not possible).** The upload needs online mode and a server; the download needs the server. Rejected
   by CLAUDE.md rule 2. Even online, it would not target players in your session.
4. **Mod-made marker + option 1 (alternative to 2).** Spawn our own visual (SFX through `CSSfxImp` at the spot, using the SFX ids
   from param #0x29 row 0) plus our own key prompt, and call option 1's drop when the prompt is used. This avoids touching the DB
   containers.

## 6. White phantoms, and what they tell us about red ones

- **Static white phantoms at the Roundtable (Nightlord-clear gear):** `CSShareClearDataInsManImp` (vtable `exe+0x2C82450`, ctor
  around `exe+0xBF5A80`) works the same way as the red one.
  - It allocates its own **`CSDisplayGhost`** (0x2C0 bytes, `exe+0x4E1400`, call at `exe+0xBF5B54`) and registers it with a CSNetMan
    database (`[CSNetMan+0x58]`, then `exe+0xB71200`, then `exe+0x4E1C10`), just as the DeathBox InsMan does with `[CSNetMan+0x48]`
    (`exe+0xB12C60` + `exe+0x4E1C10` at `exe+0x53DBB7`) `[VERIFIED: static disasm]`.
  - So the "share clear data" (gear used to beat a Nightlord) is also a **server-fed record shown by the same display-ghost class**.
    `SosSignMan` (ctor `exe+0x73DC50`) is the third user of `CSDisplayGhost`.
- **Live white phantoms in Limveld:** a different system.
  - `CSNetWanderingGhostDb` (CSNetMan `+0x38`, ctor `exe+0x189F70`, vtable `exe+0x2BDAAF0`, items `CSNetWanderingGhostDbItem`
    vtable `exe+0x2BDABE0`).
  - Fed by `GhostDownloadJob` (vtable `exe+0x2C71758`, made in `exe+0xAFA8E0` from `exe+0x18BF34`) and uploaded by `GhostUploadJob`
    (vtable `exe+0x2C74868`, made in `exe+0xB195A0` from `exe+0xAFAAAB` ← `exe+0x18D73E`). Server API `FNGhost`
    (`RequestCreateGhostData`, `RequestGetGhostDataList`, `RequestGhostResultLog`).
  - Shown as **`ReplayGhostIns`**, a real character (vtable `exe+0x2C1A450`, ctor `exe+0x4FD7B0`, created via `exe+0x3E5B20` ←
    `exe+0x4AC410`), managed by `WanderGhostMan(Imp)`, with `WanderGhostDrawSwitcher` (vtable `exe+0x2C35AB0`).
  - `[VERIFIED: static RTTI/disasm]`; the Arts-gauge refill was not traced.
- **In common with red:** server-stored records about another player, downloaded into a CSNetMan database, never made from a death in
  your own session. The static ones (Roundtable, red) use **`CSDisplayGhost`** (a posed figure built from character id + appearance
  fields). The gear itself is in the record's item lists, not in a live character.
- **Different from red:** live white phantoms are a replayed character (`ReplayGhostIns`) with recorded motion and no loot or prompt.
  Static white phantoms only display. Only the red one has the action button and the `MapItemMan` drop.
- **For us:** we need no recorded data. The red phantom is "a record + a display ghost + an action-button prompt + a MapItemMan drop".
  The display and prompt are generic engine parts (`CSDisplayGhost`, `CSActionButtonRegionSystem`), so rank 2 is technically possible.
  But the drop (`exe+0x576110`) is the part that matters for gameplay.

## Address table

| Address | What | How found | Confidence |
|---|---|---|---|
| `exe+0x3C046A8` | `CSNetManImp` slot (ctor `0x1867B0`, children `0x1883C0`) | writer `0xF34E70`, vtable RTTI | VERIFIED static |
| `[CSNetMan+0x48]` | DeathBox manager (ctor `0xB10810`, 0x128 B); list `+0x48`, named-weapon map `+0x68`, jobs `+8` | ctor + `0xB12E30`/`0xB12C80` | VERIFIED static; roles HYPOTHESIS |
| `exe+0x2C1C760` | `CSDeathBoxInsManImpl` vtable (ctor `0x53DA90`, dtor `0x53E0D0`) | RTTI | VERIFIED static |
| `exe+0x53E600` | InsMan per-frame update (from `0x637EEE`) | callers | VERIFIED static |
| `exe+0x53DE40` | Rebuild instances from the DB list (via thunk `0x53F430`) | disasm | VERIFIED static |
| `exe+0x53C410` / `0x53C590` | Instance ctor (0xC8) / record copy | disasm | VERIFIED static |
| `exe+0x53CC30` / `0x53CDC0` | Range test / display data | disasm | VERIFIED static; meaning HYPOTHESIS |
| `exe+0x53D510` | Interaction check, ActionButton 1030/1031, name to tag 0x147 | followed Arxan jumps | VERIFIED static |
| `exe+0x53D070` | **Open + drop** (0x6E/0x6F messages, `MapItemMan` drop) | disasm | VERIFIED static |
| `exe+0x576110` | `MapItemMan` drop from a vector of 0x50-byte entries at pos/map/yaw | only caller `0x53D4B1` | VERIFIED static; signature HYPOTHESIS |
| `exe+0x56DED0` / `0x578A60` | Item entry init / entry from (id, count) | disasm | VERIFIED static |
| `exe+0x53CD60` | Mark opened (`+0xAD`, DB `+0xBD`) on a 0x6F notice | disasm | VERIFIED static |
| `exe+0xE0D170` / `0xE0D2C0` | Send msg 0x6F (opened) / 0x6E (request) via `CSSessionManager` | disasm | VERIFIED static; direction HYPOTHESIS |
| `exe+0xE0C740` / `0xE0C4B0` | Receive loops (request → open on `+0xC == 3`; notice → mark) | disasm | VERIFIED static |
| `exe+0x5C296B` | Death handler → `exe+0xB14370` upload | callers | VERIFIED static |
| `exe+0xB14370` | Upload starter (guards `[0x3C078D0]+0xD8 == 2`, `+0xDE <= 3`, map != `0x0C000000`) | disasm | VERIFIED static |
| `exe+0xB0DF60` | Fill the upload record from LOCAL PGD (6 weapons, 5 accessories, 10 quick items, name, pos) | disasm | VERIFIED static |
| `exe+0x3C17858` | `CSActionButtonRegionSystemImp` slot (`0xB8FA20`, `0xB8FC70`) | writer `0x6321DF` ctor RTTI | VERIFIED static |
| `exe+0x3C2A098` | `CSSfxImp` slot | writer `0xC5BFE4` ctor RTTI | VERIFIED static |
| `exe+0x3C246B0` | Text tag table (`0xE61A20(tbl, tag, wstr)`, 0x1EA tags) | disasm | VERIFIED static; name UNKNOWN |
| `exe+0x4E1400` | `CSDisplayGhost` ctor (vtable `0x2C19638`); show `0x4E2700`, hide `0x4E2810`, ready `0x4E2640` | RTTI + calls | VERIFIED static; method roles HYPOTHESIS |
| `exe+0x2C82450` | `CSShareClearDataInsManImp` vtable (display ghost at `0xBF5B54`, DB `[CSNetMan+0x58]`) | RTTI | VERIFIED static |
| `exe+0x2BDAAF0` | `CSNetWanderingGhostDb` vtable (CSNetMan `+0x38`) | RTTI + `0x1883C0` | VERIFIED static |
| `exe+0x2C1A450` | `ReplayGhostIns` vtable (ctor `0x4FD7B0`) | RTTI | VERIFIED static |

## UNKNOWN / next experiments (solo or `nro` puppets, no online play)

1. **Does the `exe+0x576110` drop work offline and show on clients?** Add a dev console aid (card needed) that calls
   `exe+0x576110` on the host with one plain item (e.g. a known weapon id from S2) at LOCAL's spot. Check that it appears (`peek`
   `MapItemMan` slots +0x40..+0x100, §22) on both games of `nro` with two instances, and that a client can pick it. This settles
   rank 1.
2. **Coordinate convention for `exe+0x576110`:** read `+0x0C`/`+0x10` the game builds for LOCAL (the `exe+0x66B7C0` /
   `exe+0x66B800` pair used by the upload filler) with a one-shot call or a `trace_write`-style read of the filler's stack. Compare
   with `pos`.
3. **Online-mode byte:** `peek [exe+0x3C078D0]+0xD8` and `+0xDE` in our offline game (expected ≠ 2, so no upload). Name the
   singleton at `exe+0x3C078D0` (RTTI of its ctor).
4. **Does the DeathBox InsMan exist in an expedition offline?** `peek` `CSDeathBoxInsMan` (find its FD4 singleton slot: the writer of
   the object built by `exe+0x632772`), its map size at `+0x18`, and `[CSNetMan+0x48]+0x48` list size. Expected: both empty
   offline.
5. **Param #0x29 identity** (row 0, fields `+0x194/+0x198/+0x19C`): look up which param is index 0x29 in the regulation, to name the
   SFX ids.
6. **`[CSSessionManager]+0xC` values:** read on host and client in `nro` (expected 3 / 6) to confirm who drops.
7. Before any rank-2 attempt: check whether the phantom resources load offline by injecting a record in a throwaway build. Not
   before 1–4 pass.

## Addendum 2026-10-31 (step 0): the drop function read in detail

Python + capstone on the file (`Z:\Tools\rea\...\nr.py`). `[VERIFIED: static disasm]` = the instructions are in the file; meanings `[HYPOTHESIS]` until a live call.

**`exe+0x576110`: drop a list of items on the ground** (only caller `exe+0x53D4B1`). Signature, from the register use:
`bool drop(MapItemMan* mim /*rcx*/, ItemVec* items /*rdx*/, float pos[3] /*r8*/, u32* mapId /*r9*/, float yaw /*5th arg, stack [rsp+0x28] at entry*/)`.
- `items`: MSVC-style vector, `+8` = begin, `+0x10` = end; **entry size 0x50**; count = (end-begin)/0x50. Returns `al = 1` when the vector is not empty, `0` for an empty one. The function does not take ownership of the vector.
- `pos`: 12 bytes read as a `movsd` + a dword (x, y, z floats), copied into each spawn request. `mapId`: a u32 read through the pointer; `-1` is passed through, a value whose top byte is in 0x3C..0x58 is passed through, anything else is converted by `exe+0x6772B0` (the `mAA_BB_CC_DD` decode, as in the DeathBox instance, static §1).
- Per entry: copies the entry (0x50) into a spawn request, calls `exe+0x56E440(req)` to initialise it, builds an **instance id**: `0xC0000000 | ((min(1,[X+8]->+8)... & 3) << 27) | (counter & 0x7FFFFFF)` where X = the singleton at **`exe+0x3C078D0`** (`[X+8]` = pointer to a small object whose `+8` int is a player / slot number; `mim+0x2C` = a running counter, wrapped at 0xFFFFFF). So ids carry a per-player tag in bits 27-28 and do not collide across games. Then it sets position / map / yaw fields and calls `exe+0x5710C0(req, 3 float constants)` (scatter / spread, UNKNOWN), and finally **`exe+0x5759E0(mim, req)`**, which actually creates the world item.
- `exe+0x5759E0` (create): checks `[Y]+0xC` (the session-state object Y at `exe+0x3C0xxxx`, value read at `+0xC`): **6 (client) → `exe+0x57FDE0`** (asks the host?), otherwise **`exe+0x57D0A0`** creates the item locally and, when `+0xC == 3` (host), also calls `exe+0x57FA30` (tells the clients?). So a drop made on the **host** is broadcast, and on a **client** is forwarded; this answers part of E1's question statically `[HYPOTHESIS]`, the live call confirms it.
- **Item entry (0x50 bytes)**: `+0x00` u32 item id (`-1` = empty), `+0x48` u16 count, `+0x44` and `+0x4C` = 0 / `0xFFFF0000` by the init `exe+0x56DED0(entry)` (which calls `exe+0x6C0C10` to blank the 0x50 bytes). `exe+0x578A60(entry*, u32* id, u16 count)` builds a ready entry: blanks, then if `*id != -1` stores `id` at `+0x00` and `count` at `+0x48`. Item ids carry the category in the top byte (accessories `0x20000000 | id`; weapons / goods per `S2-inventory-drops.md` §3).
- **Spawn request (0xC0 bytes, init `exe+0x56E440`)**: `+0x00` byte flag, `+0x08..+0x58` the entry copy, `+0x58` (state checked by `exe+0x6A1BA0`), `+0x60 / +0x68` = -1 handles, `+0x78..` floats, `+0x94` = 0x5A, `+0xA8` = 0xFF00, `+0xAC` = -1, `+0xB8` = -1. Meanings UNKNOWN.

**Consequence for E1 (the first live experiment):** a dev console aid needs (a) a 0x50-byte entry built by `exe+0x578A60`, (b) a vector header in memory (`begin`, `end`, optional `cap`) pointing at it, (c) `pos` (world or block-local? the DeathBox instance path passes block-local `+0x10` with the block id from `+0x0C`: the aid should first try LOCAL's block-local position and map id read from the same sources the upload filler uses, `exe+0x66B7C0` / `exe+0x66B800`), (d) a call on the game thread. All four are small; the unknown is (c).

**The singleton at `exe+0x3C078D0` is used by both the online-mode guard (`+0xD8 == 2`, static §2) and the drop-id builder (`[X+8]->+8`): it looks like the session / network-player state object.** Name still UNKNOWN (find via the RTTI of its constructor).
