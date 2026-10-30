//! Game-memory access (CLAUDE.md convention: all of it lives under `game/`).
//!
//! The main loop ([`run`] -> `watch`): locate the engine singletons (T-006) without hardcoded addresses, recover the RTTI once
//! from the static image (T-007a) for the PlayerIns vtable check, then every tick resolve the players and run the rules: team
//! values (`team`, T-008/T-020/T-026) and the downed rule (`combat_state`, T-035/T-039). Commands (from the developer console)
//! are executed here, on this thread, by [`run_command`]; nothing else writes game memory from another thread.
//! R5 (2026-10-20) removed the overlay, the hotkeys, the discovery worker, the probes and the MARK dumps.
//! Size: just over 800 lines (ARCHITECTURE §6 budget) because `watch` still ticks the older controllers itself; they move into
//! `crate::features` (R3 rest), which shrinks this file.

#[cfg(feature = "dev")]
pub(crate) mod bartrace;
pub(crate) mod combat_state;
pub(crate) mod deathhold;
#[cfg(feature = "dev")]
pub(crate) mod deathtrace;
pub(crate) mod event_flags;
pub(crate) mod flags;
#[cfg(feature = "dev")]
pub(crate) mod flagwatch;
pub(crate) mod fmg;
pub(crate) mod gamemem;
pub(crate) mod hawk;
pub(crate) mod hudfilter;
pub(crate) mod image;
pub(crate) mod layout;
pub(crate) mod maphide;
pub(crate) mod mem;
pub(crate) mod memscan;
pub(crate) mod menuflow;
pub(crate) mod menugate;
#[cfg(feature = "dev")]
pub(crate) mod menutrace;
pub(crate) mod multi_instance;
pub(crate) mod multiplay;
pub(crate) mod poswrite;
pub(crate) mod puppet;
#[cfg(feature = "dev")]
#[cfg(feature = "dev")]
pub(crate) mod rawcall;
pub(crate) mod reviveblock;
pub(crate) mod rtti;
pub(crate) mod runeshare;
pub(crate) mod singletons;
pub(crate) mod spawn;
pub(crate) mod team;
#[cfg(feature = "dev")]
pub(crate) mod teleport;
#[cfg(feature = "dev")]
pub(crate) mod texttrace;
pub(crate) mod views;
#[cfg(feature = "dev")]
pub(crate) mod vpad;
pub(crate) mod world;

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::features::{Ctx, Features};
use crate::overlay::shared::Command;
#[cfg(feature = "dev")]
use crate::overlay::shared::Snapshot;
use crate::sysinfo::GameModule;
use mem::va_rva;
use rtti::Rtti;
use singletons::{Found, Scan};

/// DLRF name of the singleton we are waiting for. [SOURCE: er-reference.md section 1.3 "Name caveat";
/// fsrs cs/world_chr_man.rs#L15]. Whether NR uses the same name is exactly what this task finds out.
const TARGET: &str = "WorldChrMan";

const WAIT_POLL: Duration = Duration::from_millis(500);
/// Cadence of the "did any singleton instance change" log.
const CHANGE_POLL: Duration = Duration::from_secs(2);
/// Loop period once WorldChrMan is live (also the console's reaction time).
const LIVE_POLL: Duration = Duration::from_millis(50);
/// Do not read data sections larger than this (sanity bound on a corrupt header).
const MAX_SECTION_COPY: usize = 0x4000_0000;
/// After this long without `WorldChrMan` we log a diagnostic dump (polling continues regardless).
const WAIT_TIMEOUT: Duration = Duration::from_secs(180);
/// MM-3a: a WorldChrMan that was live before and is null again this long is logged once (info) as a load.
const NULL_LOADING_LOG: Duration = Duration::from_secs(10);
const PROGRESS_EVERY: Duration = Duration::from_secs(10);
/// T-019: a main-loop heartbeat line this often, so a stalled loop can be told apart from a frozen game.
const HEARTBEAT_EVERY: Duration = Duration::from_secs(10);
/// T-030: heartbeat period in the player build.
const HEARTBEAT_EVERY_PLAYER: Duration = Duration::from_secs(60);
/// Scan attempts when the first scans find nothing (e.g. code not yet decrypted).
const SCAN_ATTEMPTS: usize = 12;
const SCAN_RETRY: Duration = Duration::from_secs(5);

/// Entry point, called from the mod thread after the startup banner.
pub(crate) fn run(m: &GameModule, cfg: &Config, features: Features) {
    let (base, size) = (m.base, m.size);

    let Some(sections) = load_sections(base, size) else {
        tracing::error!(
            "T-006: could not read/parse the PE section table; singleton discovery aborted"
        );
        return;
    };
    for s in &sections {
        tracing::info!(
            "section {:<8} rva=0x{:X} vsize=0x{:X} flags=0x{:08X}{}",
            s.name,
            s.rva,
            s.vsize,
            s.characteristics,
            if s.is_executable() { " [scanned]" } else { "" }
        );
    }
    let exec = image::scan_sections(&sections);
    let data = image::data_ranges(&sections);
    let exec_ranges: Vec<_> = exec.iter().map(|s| s.range()).collect();
    let n_text = sections.iter().filter(|s| s.name == ".text").count();
    tracing::info!(
        "T-006: scanning {} executable section(s) ({} named .text); stock from-singleton scans only the first .text; .data ranges: {:X?}",
        exec.len(),
        n_text,
        data
    );

    let Some(scan) = acquire_scan(base, size, &exec, &data, &exec_ranges) else {
        tracing::error!(
            "T-006: scan found no singleton call sites in any executable section after {SCAN_ATTEMPTS} attempts; giving up"
        );
        return;
    };

    // Derived singleton names are plain strings: resolve right away.
    let (derived, derived_failed) = singletons::resolve_derived(&scan, base);
    tracing::info!(
        "T-006: {} FD4DerivedSingleton resolved ({} name reads failed); {} FD4Singleton candidates (names need DLRF init)",
        derived.len(),
        derived_failed,
        scan.fd4.len()
    );

    // T-007a step 1: RTTI from the static image, once. `.rdata` is plain even if code is Arxan-encrypted.
    let rtti = build_rtti(base, size, &sections);

    watch(m, cfg, &scan, derived, rtti, features);
}

/// Snapshot the RTTI-bearing data sections and recover classes/vtables. Never panics out of here.
fn build_rtti(base: usize, size: usize, sections: &[image::Section]) -> Rtti {
    let t0 = Instant::now();
    let mut regions = Vec::new();
    let mut missing_total = 0usize;
    for s in image::rtti_sections(sections) {
        let rva = s.rva as usize;
        let len = (s.vsize as usize).min(size.saturating_sub(rva));
        if len == 0 || len > MAX_SECTION_COPY {
            tracing::warn!(
                "T-007a: skipping section {} (rva 0x{:X}, vsize 0x{:X}, image size 0x{:X})",
                s.name,
                s.rva,
                s.vsize,
                size
            );
            continue;
        }
        // Reads are VirtualQuery-checked page by page; unreadable pages stay zero. Section list comes from
        // the PE header of NR 1.3.3.0 (TECHNICAL_NOTES.md section 1).
        let (bytes, missing) = mem::copy_guarded(base + rva, len);
        missing_total += missing;
        tracing::info!(
            "T-007a: RTTI snapshot of {} rva=0x{:X} len=0x{:X} unreadable=0x{:X}",
            s.name,
            s.rva,
            len,
            missing
        );
        regions.push(rtti::Region { rva: s.rva, bytes });
    }
    let result = catch_unwind(AssertUnwindSafe(|| rtti::analyze(&regions, base)));
    let rtti = match result {
        Ok(r) => r,
        Err(_) => {
            tracing::error!("T-007a: RTTI analysis panicked; continuing without RTTI");
            Rtti::from_classes(base, Vec::new(), rtti::Stats::default())
        }
    };
    tracing::info!(
        "T-007a: RTTI done in {} ms (unreadable bytes total 0x{:X})",
        t0.elapsed().as_millis(),
        missing_total
    );
    if rtti.stats.cols_primary == 0 || rtti.chr_class_count() == 0 {
        tracing::warn!(
            "T-007a: RTTI recovery found {} primary COLs and {} ChrIns-derived classes; discovery will use the fallback vtable histogram. Sample TypeDescriptors:",
            rtti.stats.cols_primary,
            rtti.chr_class_count()
        );
        for l in rtti::sample_type_descriptors(&regions, 12) {
            tracing::warn!("  {l}");
        }
    }
    rtti
}

fn load_sections(base: usize, size: usize) -> Option<Vec<image::Section>> {
    let len = size.min(0x1000);
    // SAFETY: the game image header page stays mapped for the process lifetime; length is checked
    // readable by `readable_slice` (PE headers, TECHNICAL_NOTES.md section 1: nightreign.exe 1.3.3.0).
    let hdr = unsafe { mem::readable_slice(base, len) }?;
    image::parse_sections(hdr)
}

/// Scan all executable sections, retrying if nothing is found (code could still be being unpacked).
fn acquire_scan(
    base: usize,
    size: usize,
    exec: &[&image::Section],
    data: &[std::ops::Range<u32>],
    exec_ranges: &[std::ops::Range<u32>],
) -> Option<Scan> {
    for attempt in 1..=SCAN_ATTEMPTS {
        let t0 = Instant::now();
        // SAFETY: `base`/`size` come from GetModuleInformation of the game image and `exec` from its
        // parsed section table; scan_image only reads, through VirtualQuery-checked ranges.
        let (raw, stats) = unsafe { singletons::scan_image(base, size, exec, data) };
        for st in &stats {
            tracing::info!(
                "scan attempt {attempt}: section {} (rva 0x{:X}): scanned 0x{:X} bytes; derived hits {} accepted {}; fd4 hits {} accepted {}",
                st.name,
                st.rva,
                st.scanned_len,
                st.derived_hits,
                st.derived_accepted,
                st.fd4_hits,
                st.fd4_accepted
            );
        }
        let scan = singletons::finalize(raw, exec_ranges);
        tracing::info!(
            "scan attempt {attempt}: {} derived, {} fd4 candidates using the chosen name-getter in {} ms; getter={}",
            scan.derived.len(),
            scan.fd4.len(),
            t0.elapsed().as_millis(),
            match scan.getter {
                Some((g, votes, total)) => format!("exe+0x{g:X} ({votes}/{total} votes)"),
                None => "none".to_string(),
            }
        );
        if !scan.derived.is_empty() || !scan.fd4.is_empty() {
            return Some(scan);
        }
        if attempt < SCAN_ATTEMPTS {
            std::thread::sleep(SCAN_RETRY);
        }
    }
    None
}

fn instance_of(slot: usize) -> Option<usize> {
    mem::read::<usize>(slot)
}

/// Poll until `WorldChrMan` is non-null (dump the map once in dev mode), then run the rules every tick, forever.
fn watch(
    m: &GameModule,
    cfg: &Config,
    scan: &Scan,
    derived: Vec<Found>,
    rtti: Rtti,
    mut features: Features,
) -> ! {
    let (base, size) = (m.base, m.size);
    tracing::info!("features: {:?}", features.names());
    let mem = gamemem::RealMem;
    let mut team = team::TeamCtl::new(cfg, base, size);
    // T-035: combat-state tracker and the rival revive rule (both modes).
    let mut combat = Some(combat_state::CombatCtl::new(cfg, base));
    // HAWK-2: the hawk crossing's settings (`[hawk]`); the swap goes in on the first tick that finds the env object.
    hawk::configure(&cfg.hawk);
    // WorldChrMan instance last seen (0 = none/null), for the "instance is now" log line.
    let mut last_wcm_inst: usize = 0;
    // The PlayerIns vtables and the combat tracker's RTTI are taken from `rtti` once (it never changes after startup).
    let mut rtti_applied = false;
    let start = Instant::now();
    let mut last_progress = Instant::now();
    let mut timeout_logged = false;
    // MM-3a: WorldChrMan was live before and is null again (a load screen): when it started, and whether we logged it.
    let mut null_since: Option<Instant> = None;
    let mut null_logged = false;
    let mut fd4: Option<Vec<Found>> = None;
    let mut fd4_resolve_tried = false;
    let mut nonnull_ticks = 0u32;
    let mut dumped = false;
    let mut dumped_with_fd4 = false;
    let mut wcm_slot: Option<usize> = None;
    // OFF-4: the NPC bot manager's slot (FD4 singleton, found by name once the FD4 names resolve).
    let mut botman_slot: Option<usize> = None;
    // M5: the play-area manager's slot (the day clock), found by name like the bot manager.
    let mut playarea_slot: Option<usize> = None;
    let mut last_change_poll = Instant::now() - CHANGE_POLL;
    let mut last_inst: BTreeMap<String, usize> = BTreeMap::new();
    // T-019 heartbeat: loop iterations, and the slowest iteration (work only, not the sleep) since the last line.
    let mut ticks: u64 = 0;
    let mut last_heartbeat = Instant::now();
    let mut slowest_tick = Duration::ZERO;
    // Where the slowest iteration spent its time: team controls, combat tracker + downed rule.
    let mut slowest_parts = [Duration::ZERO; 2];
    let heartbeat_every = if cfg.dev_mode {
        HEARTBEAT_EVERY
    } else {
        HEARTBEAT_EVERY_PLAYER
    };

    loop {
        let tick_started = Instant::now();
        ticks += 1;
        // 1. Resolve FD4 names once reflection looks initialised (>= 1 FD4 slot non-null on 2
        //    consecutive polls).
        if fd4.is_none() && !fd4_resolve_tried {
            if singletons::count_nonnull_fd4(scan, base) > 0 {
                nonnull_ticks += 1;
            } else {
                nonnull_ticks = 0;
            }
            if nonnull_ticks >= 2 {
                fd4_resolve_tried = true;
                tracing::info!(
                    "T-006: FD4 singletons are being constructed (+{} s); resolving names via the name-getter",
                    start.elapsed().as_secs()
                );
                // SAFETY: gated on FD4 slots being non-null (DLRF reflection exists); getter chosen by
                // majority vote over call sites (singletons.rs). Same call from-singleton's `finish`
                // makes. UNVERIFIED on NR 1.3.3.0 until the HUMAN run.
                let r = catch_unwind(AssertUnwindSafe(|| unsafe {
                    singletons::resolve_fd4(scan, base)
                }));
                match r {
                    Ok((list, failed)) => {
                        tracing::info!(
                            "T-006: {} FD4Singleton names resolved, {} failed",
                            list.len(),
                            failed
                        );
                        fd4 = Some(list);
                    }
                    Err(_) => tracing::error!("T-006: resolving FD4 names panicked"),
                }
            }
        }

        let all: Vec<&Found> = derived.iter().chain(fd4.iter().flatten()).collect();

        // 2. Find the target.
        if wcm_slot.is_none() {
            wcm_slot = all.iter().find(|f| f.name == TARGET).map(|f| f.slot);
        }
        if playarea_slot.is_none() {
            playarea_slot = all
                .iter()
                .find(|f| f.name == layout::PLAYAREA_NAME)
                .map(|f| f.slot);
        }
        if botman_slot.is_none() {
            botman_slot = all
                .iter()
                .find(|f| f.name == layout::BOTMAN_NAME)
                .map(|f| f.slot);
        }
        let wcm_inst = wcm_slot
            .and_then(instance_of)
            .filter(|&p| p != 0)
            .unwrap_or(0);
        let wcm_live = wcm_inst != 0;

        if wcm_live && (!dumped || (!dumped_with_fd4 && fd4.is_some())) {
            dumped = true;
            dumped_with_fd4 = fd4.is_some();
            tracing::info!(
                "T-006: {TARGET} instance is non-null after {} s",
                start.elapsed().as_secs()
            );
            if cfg.dev_mode {
                dump(m, &all, "WorldChrMan live");
            }
        }

        if wcm_live {
            null_since = None;
            null_logged = false;
        } else if dumped {
            // Found at least once already (`dumped`): a long null stretch is a load, not a failed discovery; info only.
            let since = *null_since.get_or_insert_with(Instant::now);
            if !null_logged && since.elapsed() >= NULL_LOADING_LOG {
                null_logged = true;
                tracing::info!(
                    "{TARGET} null for {} s (loading)",
                    since.elapsed().as_secs()
                );
            }
        }
        if !wcm_live && !dumped && !timeout_logged && start.elapsed() >= WAIT_TIMEOUT {
            timeout_logged = true;
            tracing::warn!(
                "T-006: timeout after {} s waiting for {TARGET} to be non-null (still polling every 500 ms). {}",
                WAIT_TIMEOUT.as_secs(),
                if wcm_slot.is_some() {
                    format!("{TARGET} IS in the map but its instance is still null (not constructed yet?)")
                } else {
                    format!("{TARGET} is NOT in the discovered map under that name")
                }
            );
            if cfg.dev_mode {
                dump(m, &all, "timeout");
            }
            if wcm_slot.is_none() {
                for f in all
                    .iter()
                    .filter(|f| f.name.contains("Chr") || f.name.contains("World"))
                {
                    tracing::info!(
                        "  name hint: {} slot={}",
                        f.name,
                        va_rva(f.slot, base, size)
                    );
                }
            }
        }

        // 3. Progress line while waiting.
        if cfg.dev_mode && !wcm_live && last_progress.elapsed() >= PROGRESS_EVERY {
            last_progress = Instant::now();
            let nn = all
                .iter()
                .filter(|f| matches!(instance_of(f.slot), Some(p) if p != 0))
                .count();
            tracing::info!(
                "T-006: waiting for {TARGET} (+{} s): {} named singletons, {} non-null, {} FD4 slots non-null (fd4 names resolved: {})",
                start.elapsed().as_secs(),
                all.len(),
                nn,
                singletons::count_nonnull_fd4(scan, base),
                fd4.is_some()
            );
        }

        // 4. Log WorldChrMan (re)creation (Roundtable <-> expedition loads re-create it).
        if wcm_inst != last_wcm_inst {
            if wcm_inst != 0 {
                tracing::info!(
                    "T-007a: WorldChrMan instance is now {}",
                    va_rva(wcm_inst, base, size)
                );
            }
            last_wcm_inst = wcm_inst;
        }

        // 4b. T-008: the PlayerIns vtables (for the write gate) and the combat tracker's class table, once.
        if !rtti_applied {
            rtti_applied = true;
            team.set_vts(&rtti);
            if let Some(c) = combat.as_mut() {
                c.set_rtti(&rtti);
            }
        }
        let team_started = Instant::now();
        team.set_instance(wcm_inst);
        // R3 / OFF-1: this tick's read model (the local player as the downed rule last resolved it) and the features' outbox.
        let bot_man = botman_slot.and_then(instance_of).unwrap_or(0);
        let play_area = playarea_slot.and_then(instance_of).unwrap_or(0);
        let mut world = world::World::read(
            &mem,
            &world::Inputs {
                wcm_instance: wcm_inst,
                local: combat
                    .as_ref()
                    .and_then(|c| c.local_state())
                    .map(|(a, _)| a),
                mm_phase: crate::mm::phase(),
                bot_man,
                play_area,
            },
        );
        // M5-5: what the night match reads besides the read model (flags, every player's downed state, LOCAL's position).
        if world.in_expedition() {
            world.night = night_flags(base);
            if let Some(c) = combat.as_ref() {
                for (o, f) in world.fighters.iter_mut().zip(c.fighters()) {
                    *o = Some(f);
                }
            }
            world.local_pos = world.local.and_then(|p| spawn::world_position(p, base));
            if let Some(c) = combat.as_ref() {
                world.local_hits = c.local_hits();
            }
            world.leave_pressed = leave_key_edge();
            // The edges are read every tick (their state stays current) but dropped while a menu is open.
            let (sn, sp) = spectate_switch_edges();
            let in_menu = menugate::menu_open(base);
            (world.spec_next, world.spec_prev) = (sn && !in_menu, sp && !in_menu);
            world.death_held = deathhold::held();
            if let Some(local) = world.local {
                for (o, f) in world.positions.iter_mut().zip(world.fighters.iter()) {
                    *o = f.and_then(|f| {
                        if f.local {
                            spawn::world_position(f.addr, base).map(|p| p.0)
                        } else {
                            spawn::world_position_of(f.addr, local, base)
                        }
                    });
                }
            }
            world.team_size = crate::mm::effective_team_size(cfg.team_size_usize());
        }
        let mut fctx = Ctx::default();
        // CON-1: developer console requests (dev build + dev mode), on this thread like the hotkeys; replies go back to the pipe.
        #[cfg(feature = "dev")]
        for (req, reply) in crate::console::take() {
            let mut snap = Snapshot {
                wcm_instance: wcm_inst,
                ..Snapshot::default()
            };
            team.refresh_live_next();
            team.fill_snapshot(&mut snap);
            snap.revive_fail_left = combat.as_ref().map_or(0, |c| c.forced_fails());
            snap.early_redown = combat.as_ref().is_some_and(|c| c.early_redown());
            snap.bar_trace = bartrace::enabled();
            snap.bar_trace_hits = bartrace::hits();
            snap.revive_block = reviveblock::enabled();
            snap.revive_blocks = reviveblock::blocks();
            let text = crate::console::answer(req, &snap, &mut |c| match c {
                // T-040: the combat controller lives next to the team controller, not inside it.
                Command::ReviveFail(n) => {
                    if let Some(cc) = combat.as_mut() {
                        cc.set_forced_fails(n);
                    }
                }
                Command::EarlyRedown(on) => {
                    if let Some(cc) = combat.as_mut() {
                        cc.set_early_redown(on);
                    }
                }
                // T-041: the trace is a module-level switch.
                Command::BarTrace(on) => bartrace::set_enabled(on),
                Command::ReviveBlock(on) => reviveblock::set_enabled(on),
                // HAWK-3: module-level state like the trace.
                Command::Hawk(h) => hawk::command(h, base, size),
                // OFF-1: features get their commands with this tick's world.
                Command::Mode(m) => features.mode.request(m, "console", &world, &mut fctx),
                Command::Clock(c) => features.clock.request(c, "console", &world, &mut fctx),
                Command::NightEnd => features.nights.request_end("console", &world, &mut fctx),
                Command::Flag(f, v) => crate::game::flags::console_cmd(base, f, v),
                #[cfg(feature = "dev")]
                Command::RawCall(rva, arg) => rawcall::call(base, rva, arg),
                Command::Bots(b) => features.bots.request(
                    b,
                    "console",
                    crate::features::mode::current(),
                    &world,
                    &mut fctx,
                ),
                c => run_command(c, "console", &mut team),
            });
            let _ = reply.send(text);
        }
        // RW-10: enemy scaling follows the team size (the scan starts on the first pass).
        multiplay::tick(crate::mm::effective_team_size(cfg.team_size_usize()));
        features.tick(&world, &mut fctx);
        night_acts(
            std::mem::take(&mut fctx.night),
            world.local,
            base,
            play_area,
        );
        fctx.flush(crate::features::Targets { base, play_area }, &mem);
        team.tick();
        let team_spent = team_started.elapsed();

        // 5. Singleton instance-change watch (every 2 s) once the target is live.
        if cfg.dev_mode && wcm_live && last_change_poll.elapsed() >= CHANGE_POLL {
            last_change_poll = Instant::now();
            for f in &all {
                let cur = instance_of(f.slot).unwrap_or(0);
                let prev = last_inst.insert(f.name.clone(), cur);
                // First pass only seeds the table (the dump already printed it).
                if let Some(prev) = prev {
                    if prev != cur {
                        tracing::info!(
                            "singleton instance change: {} {} -> {}",
                            f.name,
                            va_rva(prev, base, size),
                            va_rva(cur, base, size)
                        );
                    }
                }
            }
        }

        // T-035 (both modes): tracker and the downed rule.
        let combat_started = Instant::now();
        if let Some(c) = combat.as_mut() {
            let subjects = if wcm_live {
                team.combat_subjects()
            } else {
                Vec::new()
            };
            c.tick(subjects);
        }
        // T-041 step 1 (dev only): arm / drain the revive-bar write watch.
        #[cfg(feature = "dev")]
        if let Some(c) = combat.as_ref() {
            bartrace::tick(c, base, size);
        }
        // M5-8 (dev only): the comeback trace / hold and LOCAL's SpEffects.
        #[cfg(feature = "dev")]
        deathtrace::tick(
            combat
                .as_ref()
                .and_then(|c| c.local_state())
                .map(|(a, _)| a),
            base,
            size,
        );
        // M5-9 (both builds): hold a dead LOCAL's comeback at night in Solo, release it at the morning.
        deathhold::tick(
            combat
                .as_ref()
                .and_then(|c| c.local_state())
                .map(|(a, _)| a),
            base,
        );
        // RW-1 (both builds): the Esc menu of a defeated player (the breakpoint is armed only while the night match asks for it).
        menugate::tick(
            combat
                .as_ref()
                .and_then(|c| c.local_state())
                .map(|(a, _)| a),
            base,
        );
        // RW-9 (both builds): rivals are not on the map, the compass, the party bars or (unless locked on) the name tags while a
        // match runs (the mod's PvP mode on, in an expedition); teammates stay. `maphide` (Solo: the whole map / compass list
        // emptied) is the fallback for when the map site could not be hooked. Map markers (pins) a rival places are kept off the
        // map, the compass and the 3D world too.
        let in_match =
            world.in_expedition() && team.pvpve_state() == crate::overlay::shared::PvpveState::On;
        hudfilter::tick(
            base,
            in_match,
            &combat
                .as_ref()
                .map(|c| c.rival_handles())
                .unwrap_or_default(),
            &combat.as_ref().map(|c| c.rival_addrs()).unwrap_or_default(),
            combat
                .as_ref()
                .and_then(|c| c.local_state())
                .map(|(a, _)| a),
        );
        // RW-8 (both builds): a rival's kill credits no runes to LOCAL; teammates' and LOCAL's own kills stay vanilla.
        runeshare::tick(
            base,
            in_match,
            crate::mm::effective_team_size(cfg.team_size_usize()) == 1,
            &combat
                .as_ref()
                .map(|c| c.rival_handles())
                .unwrap_or_default(),
            combat
                .as_ref()
                .and_then(|c| c.local_state())
                .map(|(a, _)| a),
        );
        maphide::tick(
            base,
            in_match
                && !hudfilter::map_hooked()
                && crate::mm::effective_team_size(cfg.team_size_usize()) == 1,
        );
        // HAWK-2 (both builds): keep the env swap in place, run the hawk crossing's ride phases, log the hawk answers.
        hawk::tick(
            base,
            size,
            combat
                .as_ref()
                .and_then(|c| c.local_state())
                .map(|(a, _)| a),
        );
        // T-041: drop a rival's revive event before the game starts the get-up (both builds, always on).
        if let Some(c) = combat.as_mut() {
            reviveblock::tick(c, base);
        }
        let combat_spent = combat_started.elapsed();
        let spent = tick_started.elapsed();
        if spent >= slowest_tick {
            slowest_tick = spent;
            slowest_parts = [team_spent, combat_spent];
        }
        if last_heartbeat.elapsed() >= heartbeat_every {
            last_heartbeat = Instant::now();
            tracing::info!(
                "{}",
                heartbeat_line(ticks, wcm_inst, &team, slowest_tick, slowest_parts)
            );
            slowest_tick = Duration::ZERO;
            slowest_parts = [Duration::ZERO; 2];
        }

        std::thread::sleep(if wcm_live {
            team.poll_interval(LIVE_POLL)
        } else {
            WAIT_POLL
        });
    }
}

/// T-019: `heartbeat: tick N, wcm=0x..., players=N, pvpve=ON (team 29, 3 writes), slowest tick ... ms`. If these
/// lines stop while the game is frozen, the main loop is what is stuck; if they continue, it is not.
/// M5-5: the night flags, all or nothing (`None` while the flag manager is not live).
fn night_flags(base: usize) -> Option<world::NightFlags> {
    let mut v = [false; 13];
    for (o, id) in v.iter_mut().zip(world::NightFlags::IDS) {
        *o = flags::get(base, id).ok()?;
    }
    Some(world::NightFlags::from_values(v))
}

/// RW-1: the spectator's switch inputs went down since the last call: next = left mouse button or the pad's d-pad right, previous = right
/// mouse button or the d-pad left (edges, so holding a button switches once). The pad is read from the game's own input
/// (`menugate::pad_spectate_edges`): XInput shows no pad inside the game.
fn spectate_switch_edges() -> (bool, bool) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    static WAS: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
    // SAFETY: GetAsyncKeyState only reads the keyboard / mouse state.
    let (next, prev) = unsafe {
        (
            GetAsyncKeyState(i32::from(VK_LBUTTON.0)) < 0,
            GetAsyncKeyState(i32::from(VK_RBUTTON.0)) < 0,
        )
    };
    let edge = |i: usize, down: bool| down && !WAS[i].swap(down, Ordering::Relaxed);
    let n = edge(0, next);
    let p = edge(1, prev);
    if !next {
        WAS[0].store(false, Ordering::Relaxed);
    }
    if !prev {
        WAS[1].store(false, Ordering::Relaxed);
    }
    let (pn, pp) = menugate::pad_spectate_edges();
    (n || pn, p || pp)
}

/// RW-1: the leave key (F10) went down since the last call (an edge, so holding it asks once).
fn leave_key_edge() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F10};
    static WAS_DOWN: AtomicBool = AtomicBool::new(false);
    // SAFETY: GetAsyncKeyState only reads the keyboard state.
    let down = unsafe { GetAsyncKeyState(i32::from(VK_F10.0)) } < 0;
    let edge = down && !WAS_DOWN.load(Ordering::Relaxed);
    WAS_DOWN.store(down, Ordering::Relaxed);
    edge
}

/// Carry out the match rules' requests (the hold of a defeated player, a move, the game's flags, the clock's speed).
fn night_acts(
    acts: Vec<crate::features::night_match::NightAct>,
    local: Option<usize>,
    base: usize,
    play_area: usize,
) {
    use crate::features::night_match::NightAct;
    for a in acts {
        let r: Result<(), &'static str> = match a {
            NightAct::Teleport(to) => match local.map(|p| spawn::teleport(p, base, to)) {
                Some(Ok(from)) => {
                    tracing::info!(
                        "night_match: moved LOCAL from world ({:.1}, {:.1}, {:.1}) to ({:.1}, {:.1}, {:.1})",
                        from[0], from[1], from[2], to[0], to[1], to[2]
                    );
                    Ok(())
                }
                Some(Err(e)) => Err(e),
                None => Err("no local player"),
            },
            NightAct::Follow(to) => match local.map(|p| spawn::teleport(p, base, to)) {
                Some(Ok(_)) => Ok(()),
                Some(Err(e)) => Err(e),
                None => Err("no local player"),
            },
            NightAct::MenuGate(on) => {
                menugate::set_active(on);
                Ok(())
            }
            NightAct::DeathHold(on) => {
                deathhold::set_armed(on);
                Ok(())
            }
            NightAct::ClockSpeed(speed) => {
                if play_area == 0 {
                    Err("no play-area manager")
                } else {
                    views::PlayArea::new(&gamemem::RealMem, play_area).set_speed(speed)
                }
            }
            NightAct::GameFlag(f, on) => flags::set_match_flag(base, f, on).map(|prev| {
                tracing::info!(
                    "night_match: game flag {f} set {} (was {prev})",
                    if on { "ON" } else { "off" }
                );
            }),
        };
        if let Err(e) = r {
            // A failing spectator move repeats 10 times a second: say it once.
            static FOLLOW_WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !matches!(a, NightAct::Follow(_))
                || !FOLLOW_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                tracing::warn!("night_match: {a:?} failed: {e}");
            }
        }
    }
}

fn heartbeat_line(
    ticks: u64,
    wcm_inst: usize,
    team: &team::TeamCtl,
    slowest: Duration,
    parts: [Duration; 2],
) -> String {
    format!(
        "heartbeat: tick {ticks}, wcm=0x{wcm_inst:X}, players={}, pvpve={}, slowest tick since last heartbeat {} ms (team {} ms, combat {} ms)",
        team.player_count_text(),
        team.pvpve_summary(),
        slowest.as_millis(),
        parts[0].as_millis(),
        parts[1].as_millis()
    )
}

/// Execute one command on the main loop thread. `src` is the trigger for the log ("F5", "overlay", ...).
fn run_command(cmd: Command, src: &str, team: &mut team::TeamCtl) {
    match cmd {
        Command::Set(v) => team.set_and_write(v, src),
        Command::Restore => team.restore(src),
        Command::SetFor { addr, value } => team.set_for(addr, value, src),
        Command::RestoreFor { addr } => team.restore_for(addr, src),
        Command::PersistFor { addr, on } => team.set_persist_for(addr, on, src),
        Command::SetPvpve(on) => team.set_pvpve(on, src),
        Command::Mark(text) => team.mark(text.as_deref()),
        Command::PosCycle => team.pos_cycle(),
        Command::PosSave => team.pos_save(src),
        Command::PosWrite => team.pos_write(src),
        // T-040: intercepted in the loop (it needs the combat controller); never reaches here.
        Command::ReviveFail(_)
        | Command::EarlyRedown(_)
        | Command::BarTrace(_)
        | Command::ReviveBlock(_)
        | Command::Hawk(_)
        | Command::Mode(_)
        | Command::Bots(_)
        | Command::Clock(_)
        | Command::NightEnd
        | Command::Flag(..) => {}
        #[cfg(feature = "dev")]
        Command::RawCall(..) => {}
        // MM-2: only hands the request to the Steam worker thread (a bounded queue push); never calls Steam here.
        Command::MmQueue(mode) => {
            if !crate::mm::send(crate::mm::Cmd::Queue(mode)) {
                tracing::info!(
                    "mm_queue: {src}: queue {} not sent (queue feature off or command queue full)",
                    mode.label()
                );
            }
        }
        Command::MmJoin(j) => {
            if !crate::mm::send(crate::mm::Cmd::Join(j)) {
                tracing::info!(
                    "mm_join: {src}: {j:?} not sent (queue feature off or command queue full)"
                );
            }
        }
        Command::MmScCreate => {
            if !crate::mm::send(crate::mm::Cmd::ScCreate) {
                tracing::info!("mm_sc_create: {src}: create test not sent (queue feature off or command queue full)");
            }
        }
        Command::MmQuietTest => {
            if !crate::mm::send(crate::mm::Cmd::QuietTest) {
                tracing::info!("mm_quiet_invite: {src}: test not sent (queue feature off or command queue full)");
            }
        }
        Command::MmLeave => {
            if !crate::mm::send(crate::mm::Cmd::Leave) {
                tracing::info!(
                    "mm_queue: {src}: leave not sent (queue feature off or command queue full)"
                );
            }
        }
    }
}

/// Log the full map: name, kind, static slot address (and exe RVA), current instance pointer, plus the
/// comparison against the stock `from_singleton::map()`.
fn dump(m: &GameModule, all: &[&Found], why: &str) {
    let (base, size) = (m.base, m.size);
    let mut sorted: Vec<&&Found> = all.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name).then(a.slot.cmp(&b.slot)));
    tracing::info!("=== singleton map ({why}): {} entries ===", sorted.len());
    for f in &sorted {
        let inst = instance_of(f.slot);
        tracing::info!(
            "  {:<44} {:<7} slot={} instance={}",
            f.name,
            match f.kind {
                singletons::Kind::Fd4 => "FD4",
                singletons::Kind::Derived => "derived",
            },
            va_rva(f.slot, base, size),
            match inst {
                None => "<unreadable>".to_string(),
                Some(0) => "null".to_string(),
                Some(p) => va_rva(p, base, size),
            }
        );
    }
    tracing::info!("=== end singleton map ===");

    // Step 1 of the card: does the stock finder apply to NR? Called AFTER our own dump so a crash inside
    // it cannot hide our results.
    let stock = catch_unwind(AssertUnwindSafe(|| {
        from_singleton::map()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_ptr() as usize))
            .collect::<BTreeMap<String, usize>>()
    }));
    match stock {
        Err(_) => tracing::warn!(
            "STOCK from_singleton::map() PANICKED (stock finder does not apply to this exe as is)"
        ),
        Ok(stock) => {
            let ours: BTreeMap<&str, usize> =
                all.iter().map(|f| (f.name.as_str(), f.slot)).collect();
            tracing::info!(
                "STOCK from_singleton::map(): {} entries; ours (all executable sections): {} entries; {TARGET} in stock map: {}",
                stock.len(),
                ours.len(),
                stock.contains_key(TARGET)
            );
            let only_ours: Vec<&str> = ours
                .keys()
                .filter(|k| !stock.contains_key(**k))
                .copied()
                .collect();
            let only_stock: Vec<&str> = stock
                .keys()
                .filter(|k| !ours.contains_key(k.as_str()))
                .map(String::as_str)
                .collect();
            let slot_mismatch: Vec<&str> = ours
                .iter()
                .filter(|(k, v)| stock.get(**k).is_some_and(|s| s != *v))
                .map(|(k, _)| *k)
                .collect();
            tracing::info!("  only in ours ({}): {:?}", only_ours.len(), only_ours);
            tracing::info!("  only in stock ({}): {:?}", only_stock.len(), only_stock);
            tracing::info!(
                "  in both with different slot ({}): {:?}",
                slot_mismatch.len(),
                slot_mismatch
            );
        }
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::System::LibraryLoader::LoadLibraryW;
    use windows::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
    use windows::Win32::System::Threading::GetCurrentProcess;

    /// Validates the RTTI layout assumptions against a REAL MSVC x64 binary (a Windows system DLL built
    /// from C++): the analyzer must find classes with vtables and COL-derived base lists.
    #[test]
    fn rtti_on_real_msvc_binary() {
        for name in [
            w!("msvcp140.dll"),
            w!("windows.storage.dll"),
            w!("shell32.dll"),
            w!("combase.dll"),
            w!("mshtml.dll"),
            w!("d3d11.dll"),
            w!("explorerframe.dll"),
            w!("twinui.dll"),
            w!("Windows.UI.Xaml.dll"),
            w!("wininet.dll"),
            w!("urlmon.dll"),
            w!("propsys.dll"),
            w!("windowscodecs.dll"),
            w!("dwrite.dll"),
        ] {
            // SAFETY: loading a Windows system DLL into the test process; it is never unloaded.
            let Ok(h) = (unsafe { LoadLibraryW(name) }) else {
                continue;
            };
            let mut info = MODULEINFO::default();
            // SAFETY: valid out-buffer and size; current-process pseudo handle.
            let ok = unsafe {
                GetModuleInformation(
                    GetCurrentProcess(),
                    h,
                    &mut info,
                    std::mem::size_of::<MODULEINFO>() as u32,
                )
            };
            if ok.is_err() {
                continue;
            }
            let (base, size) = (info.lpBaseOfDll as usize, info.SizeOfImage as usize);
            let hdr = mem::read_prefix(base, 0x1000);
            let secs = image::parse_sections(&hdr).expect("PE sections");
            let mut regions = Vec::new();
            for s in image::rtti_sections(&secs) {
                let len = (s.vsize as usize).min(size.saturating_sub(s.rva as usize));
                let (bytes, _) = mem::copy_guarded(base + s.rva as usize, len);
                regions.push(rtti::Region { rva: s.rva, bytes });
            }
            let r = rtti::analyze(&regions, base);
            println!("{name:?}: {:?}", r.stats);
            let with_bases = r.classes.iter().filter(|c| c.bases.len() > 1).count();
            println!("  classes with >1 base: {with_bases}");
            assert!(r.stats.type_descriptors > 50, "TDs");
            assert!(r.stats.cols_primary > 50, "primary COLs");
            assert!(r.stats.classes_with_vtable > 50, "vtables located");
            assert!(with_bases > 5, "base class arrays parsed");
            // Every located vtable must point into the image and hold a pointer into the image's code.
            let mut checked = 0;
            for c in r.classes.iter().filter(|c| !c.vtables.is_empty()).take(200) {
                let slot0 = mem::read::<usize>(base + c.vtables[0] as usize);
                if let Some(f) = slot0 {
                    if f >= base && f < base + size {
                        checked += 1;
                    }
                }
            }
            assert!(checked > 20, "vtable[0] points into the module: {checked}");
            return;
        }
        panic!("no system DLL could be analysed");
    }
}
