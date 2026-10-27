# What nro uses from nightreign-pvp

Change these on the mod side and nro must change with them.

| Item | Where in nightreign-pvp | Used by |
|---|---|---|
| Dev DLL, `nrpvp.toml`, `gamedata\` in a `dist\` folder | `scripts\build.ps1`, `scripts\build-gamedata.ps1` | `session::stage` copies them into `runs\<id>\mod\` |
| me3 profile shape (SC `load_early`, nrpvp `load_after`, `nrpvp-gamedata` package) | `crates\nrpvp-launcher\src\main.rs` `profile_text` | `nro_core::profile` |
| `nrpvp.toml` keys `dev_mode`, `mm_queue`, `br_menu`, `mode`, `offline_bots`; top-level keys before the first `[table]` | `crates\nrpvp\src\config.rs` | preflight P7, `scenario::apply_config` |
| Console pipe `\.\pipe\nrpvp-<pid>`: one line in, reply lines, a line with only EOT (U+0004) | `crates\nrpvp\src\console\pipe.rs` | `nro::console` |
| `status` reply: `world loaded, N player(s)`, `no world`, `team_type 1` (Roundtable) / `2` (expedition) | `console\mod.rs` `status_text` | navigation, waits |
| Puppet commands (dev): `puppet_arm`, `puppet_max`, `puppet_list` (lines `slot N: … PUPPET …`, `pos (x, y, z)`, `N filled`), `puppet_move`, `puppet_hp`, `bots 10`, `bots_clear` | `crates\nrpvp\src\game\puppet.rs`, `features\bots.rs` | `ops::puppets`, `puppet_front`, `puppet_ring` |
| Log lines `T-033 STATE … DOWN/UP`, `T-039 BLOCK` | `game\combat_state` | `scenarios\puppets-smoke.toml` |
| `nrpvp.toml` key `mm_join_auto` (`invite` / `…link…`); log line `mm_queue: worker started: own Steam id <N>` (or `mm_probe: own Steam ID = <N>`) in `nrpvp.log` | `config.rs`, `mm\worker.rs`, `steam\probe.rs` | preflight P7 (emulated), `preflight::queue_identity` (the identity gate before any queue, MULTI-INSTANCE §20) |
| `pos` reply (`world (x, y, z)`, `menu 0x…`; `0x200004` = expedition menu), `tp x y z yaw`; `mm_status` texts `searching`, `MATCH READY`, `ALL PRESENT`, `IN EXPEDITION` | `game\teleport.rs`, `mm\machine\gate.rs` | `navigate = invite_members / commence`, `scenarios\mi-queue.toml` |
| Version pins | `.claude\TECHNICAL_NOTES.md` §1 | `pins\nightreign.toml` |
