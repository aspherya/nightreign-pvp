//! Game-memory access (CLAUDE.md convention: all of it lives under `game/`).
//!
//! T-006 orchestration: locate engine singletons without hardcoded addresses, wait for the game to
//! construct them, log the full map.
//! T-007a orchestration: recover MSVC RTTI once from the static image, then (10 s after WorldChrMan becomes
//! live or is re-created, and on every F9) scan the WorldChrMan instance for ChrIns-derived objects and
//! hex-dump them. Read-only: no hooks.
//! T-008 orchestration: `team` resolves the local player each tick, watches it, and on a write command
//! selects/writes `team_type` (the only memory write in the project, see `mem::write_u8`); the histogram runs
//! on the discovery worker.
//! T-013: commands come from two sources, the (configurable) hotkeys and the overlay's command queue. Both
//! are executed here, on the main loop thread, by [`run_command`]; the overlay's render thread never touches
//! game memory. After each tick the loop publishes a [`Snapshot`] for the overlay.

pub(crate) mod discover;
pub(crate) mod image;
pub(crate) mod mem;
pub(crate) mod rtti;
pub(crate) mod singletons;
pub(crate) mod team;

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::hotkey::{self, Action};
use crate::overlay::shared::{Command, Shared, Snapshot};
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
/// Loop period once WorldChrMan is live (also the F9 reaction time together with the 50 ms key poll).
const LIVE_POLL: Duration = Duration::from_millis(50);
/// T-007a: automatic discovery this long after WorldChrMan becomes live or is re-created.
const AUTO_DELAY: Duration = Duration::from_secs(10);
/// RTTI recovery attempts in total (the first happens at startup; retries only if it found no ChrIns).
const RTTI_MAX_ATTEMPTS: u32 = 4;
/// Do not read data sections larger than this (sanity bound on a corrupt header).
const MAX_SECTION_COPY: usize = 0x4000_0000;
/// After this long without `WorldChrMan` we log a diagnostic dump (polling continues regardless).
const WAIT_TIMEOUT: Duration = Duration::from_secs(180);
const PROGRESS_EVERY: Duration = Duration::from_secs(10);
/// Scan attempts when the first scans find nothing (e.g. code not yet decrypted).
const SCAN_ATTEMPTS: usize = 12;
const SCAN_RETRY: Duration = Duration::from_secs(5);

/// Entry point, called from the mod thread after the startup banner.
pub(crate) fn run(m: &GameModule, cfg: &Config, ui: Option<Arc<Shared>>) {
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

    watch(m, cfg, &scan, derived, &sections, rtti, ui);
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
    discover::log_rtti(&rtti);
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

/// A unit of work for the discovery worker: gets the RTTI and the run sequence number.
type Job = Box<dyn FnOnce(&Rtti, u32) + Send>;

/// RTTI recovery state shared with the discovery worker (only one worker runs at a time).
struct DiscState {
    rtti: Rtti,
    attempts: u32,
}

/// Everything the T-007a discovery needs between runs. Each run executes on its own short-lived worker
/// thread (`nrpvp-discovery`), one at a time, so neither the main loop nor the F9 handler ever blocks on it.
struct Discovery {
    state: Arc<Mutex<DiscState>>,
    gate: Arc<discover::Gate>,
    sections: Arc<Vec<image::Section>>,
    base: usize,
    size: usize,
    seq: u32,
}

impl Discovery {
    /// Start one discovery run on `inst` in the background. `Err(running_ms)` if a run is already in flight
    /// (nothing is started). The worker re-attempts RTTI recovery first if it earlier found no ChrIns.
    fn start(&mut self, inst: usize, reason: &str) -> Result<(), u64> {
        let (base, size) = (self.base, self.size);
        let reason = reason.to_string();
        self.start_job(
            "T-007a discovery",
            Box::new(move |rtti, seq| discover::run(rtti, inst, &reason, seq, base, size)),
        )
    }

    /// T-008 histogram of team_type over everything the scan reaches, on the same worker/gate.
    fn start_team_histogram(&mut self, inst: usize) -> Result<(), u64> {
        let base = self.base;
        self.start_job(
            "T-008 histogram",
            Box::new(move |rtti, _seq| team::histogram_report(rtti, inst, base)),
        )
    }

    /// Run `job(rtti, seq)` on a worker thread (one at a time via the gate).
    fn start_job(&mut self, what: &'static str, job: Job) -> Result<(), u64> {
        let guard = self.gate.try_acquire()?;
        self.seq += 1;
        let seq = self.seq;
        let state = Arc::clone(&self.state);
        let sections = Arc::clone(&self.sections);
        let (base, size) = (self.base, self.size);
        let spawned = std::thread::Builder::new()
            .name("nrpvp-discovery".into())
            .spawn(move || {
                // Released on every exit path (return, panic) when the guard drops.
                let _guard = guard;
                let mut st = state.lock().unwrap_or_else(PoisonError::into_inner);
                if st.rtti.chr_class_count() == 0 && st.attempts < RTTI_MAX_ATTEMPTS {
                    st.attempts += 1;
                    tracing::info!(
                        "T-007a: no ChrIns classes yet; re-running RTTI recovery (attempt {}/{RTTI_MAX_ATTEMPTS})",
                        st.attempts
                    );
                    st.rtti = build_rtti(base, size, &sections);
                }
                let rtti = &st.rtti;
                let r = catch_unwind(AssertUnwindSafe(|| job(rtti, seq)));
                if r.is_err() {
                    tracing::error!("{what} #{seq} panicked; continuing");
                }
            });
        if let Err(e) = spawned {
            // The closure (and with it the guard) was dropped, so the gate is free again.
            tracing::error!("T-007a: could not spawn the discovery thread: {e}");
        }
        Ok(())
    }
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

/// Poll until `WorldChrMan` is non-null (dump the map once), then serve discovery requests forever: the
/// automatic run 10 s after the instance appears or changes, and every F9 press.
fn watch(
    m: &GameModule,
    cfg: &Config,
    scan: &Scan,
    derived: Vec<Found>,
    sections: &[image::Section],
    rtti: Rtti,
    ui: Option<Arc<Shared>>,
) -> ! {
    let (base, size) = (m.base, m.size);
    let mut team = team::TeamCtl::new(cfg, base, size);
    // Bound hotkeys with their key names (the names label the log lines, e.g. "F5 team write").
    let hotkeys = cfg.bindings();
    let mut last_published: Option<Snapshot> = None;
    let mut disc = Discovery {
        state: Arc::new(Mutex::new(DiscState { rtti, attempts: 1 })),
        gate: discover::Gate::new(),
        sections: Arc::new(sections.to_vec()),
        base,
        size,
        seq: 0,
    };
    let mut auto_deferred_logged = false;
    // WorldChrMan instance we last scheduled/ran an automatic discovery for (0 = none/null).
    let mut last_wcm_inst: usize = 0;
    let mut auto_due: Option<Instant> = None;
    let start = Instant::now();
    let mut last_progress = Instant::now();
    let mut timeout_logged = false;
    let mut fd4: Option<Vec<Found>> = None;
    let mut fd4_resolve_tried = false;
    let mut nonnull_ticks = 0u32;
    let mut dumped = false;
    let mut dumped_with_fd4 = false;
    let mut wcm_slot: Option<usize> = None;
    let mut last_change_poll = Instant::now() - CHANGE_POLL;
    let mut last_inst: BTreeMap<String, usize> = BTreeMap::new();

    loop {
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
            dump(m, &all, "WorldChrMan live");
        }

        if !wcm_live && !timeout_logged && start.elapsed() >= WAIT_TIMEOUT {
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
            dump(m, &all, "timeout");
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
        if !wcm_live && last_progress.elapsed() >= PROGRESS_EVERY {
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

        // 4. T-007a: schedule the automatic discovery when WorldChrMan appears or is re-created (area
        //    change), then serve F9. The key flag is always consumed so presses are never queued.
        if wcm_inst != last_wcm_inst {
            if wcm_inst != 0 {
                tracing::info!(
                    "T-007a: WorldChrMan instance is now {}; automatic discovery in {} s",
                    va_rva(wcm_inst, base, size),
                    AUTO_DELAY.as_secs()
                );
                auto_due = Some(Instant::now() + AUTO_DELAY);
            } else {
                auto_due = None;
            }
            last_wcm_inst = wcm_inst;
        }

        // 4b. T-008: team-type controls. PlayerIns vtables come from the RTTI the discovery worker may be
        //     holding the lock on; never block on it (try_lock) so hotkeys and persist stay responsive.
        if team.needs_vts() {
            match disc.state.try_lock() {
                Ok(st) => team.set_vts(&st.rtti),
                Err(TryLockError::Poisoned(p)) => team.set_vts(&p.into_inner().rtti),
                Err(TryLockError::WouldBlock) => {}
            }
        }
        team.set_instance(wcm_inst);
        // Hotkey presses and overlay commands, executed in order on this thread. The hotkey flags are
        // always consumed so presses never queue.
        for (action, key) in &hotkeys {
            // The overlay consumes its own toggle flag on the render thread.
            if *action == Action::ToggleOverlay || !hotkey::take(*action) {
                continue;
            }
            match hotkey_command(*action, &team) {
                Some(cmd) => run_command(cmd, &key.name, &mut team, &mut disc, wcm_inst),
                None if *action == Action::Cycle => team.cycle(),
                None => {}
            }
        }
        if let Some(ui) = &ui {
            for cmd in ui.drain() {
                run_command(cmd, "overlay", &mut team, &mut disc, wcm_inst);
            }
        }
        team.tick();

        if wcm_live && auto_due.is_some_and(|d| Instant::now() >= d) {
            match disc.start(wcm_inst, "auto, 10 s after WorldChrMan live/changed") {
                Ok(()) => {
                    auto_due = None;
                    auto_deferred_logged = false;
                }
                // Stays due: retried every tick until the running discovery finishes.
                Err(ms) => {
                    if !auto_deferred_logged {
                        auto_deferred_logged = true;
                        tracing::info!(
                            "T-007a: automatic discovery deferred: discovery busy (running {ms} ms)"
                        );
                    }
                }
            }
        }

        // 5. Singleton instance-change watch (every 2 s) once the target is live.
        if wcm_live && last_change_poll.elapsed() >= CHANGE_POLL {
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

        // T-013: hand the overlay a fresh snapshot (only when something changed).
        if let Some(ui) = &ui {
            let mut snap = Snapshot {
                wcm_instance: wcm_inst,
                busy: disc.gate.is_busy(),
                ..Snapshot::default()
            };
            team.fill_snapshot(&mut snap);
            if last_published.as_ref() != Some(&snap) {
                ui.publish(snap.clone());
                last_published = Some(snap);
            }
        }

        std::thread::sleep(if wcm_live {
            team.poll_interval(LIVE_POLL)
        } else {
            WAIT_POLL
        });
    }
}

/// The command a hotkey stands for. `Cycle` has none (it only changes the selection) and so is handled by the
/// caller.
fn hotkey_command(a: Action, team: &team::TeamCtl) -> Option<Command> {
    match a {
        Action::Write => Some(Command::Set(team.selected())),
        Action::Restore => Some(Command::Restore),
        Action::Persist => Some(Command::TogglePersist),
        Action::Histogram => Some(Command::RunHistogram),
        Action::Discovery => Some(Command::RunDiscovery),
        Action::Cycle | Action::ToggleOverlay => None,
    }
}

/// Execute one command on the main loop thread. `src` is the trigger for the log ("F5", "overlay", ...).
fn run_command(
    cmd: Command,
    src: &str,
    team: &mut team::TeamCtl,
    disc: &mut Discovery,
    wcm_inst: usize,
) {
    let wcm_live = wcm_inst != 0;
    match cmd {
        Command::Set(v) => team.set_and_write(v, src),
        Command::Restore => team.restore(src),
        Command::TogglePersist => team.toggle_persist(src),
        Command::RunDiscovery => {
            if wcm_live {
                if let Err(ms) = disc.start(wcm_inst, src) {
                    tracing::info!("T-007a: {src} ignored: discovery busy (running {ms} ms)");
                }
            } else {
                tracing::info!("T-007a: {src} ignored, WorldChrMan is not live");
            }
        }
        Command::RunHistogram => {
            if wcm_live {
                if let Err(ms) = disc.start_team_histogram(wcm_inst) {
                    tracing::info!("T-008: {src} ignored: discovery/scan busy (running {ms} ms)");
                }
            } else {
                tracing::info!("T-008: {src} ignored, WorldChrMan is not live");
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

#[cfg(test)]
mod command_tests {
    use super::*;
    use crate::config::Config;

    fn fake_disc() -> Discovery {
        Discovery {
            state: Arc::new(Mutex::new(DiscState {
                rtti: Rtti::from_classes(0, Vec::new(), rtti::Stats::default()),
                attempts: RTTI_MAX_ATTEMPTS,
            })),
            gate: discover::Gate::new(),
            sections: Arc::new(Vec::new()),
            base: 0,
            size: 0,
            seq: 0,
        }
    }

    #[test]
    fn hotkeys_map_to_the_same_commands_as_the_overlay() {
        let team = team::TeamCtl::new(&Config::default(), 0, 0);
        assert_eq!(
            hotkey_command(Action::Write, &team),
            Some(Command::Set(6)),
            "the write hotkey applies the selected candidate"
        );
        assert_eq!(
            hotkey_command(Action::Restore, &team),
            Some(Command::Restore)
        );
        assert_eq!(
            hotkey_command(Action::Persist, &team),
            Some(Command::TogglePersist)
        );
        assert_eq!(
            hotkey_command(Action::Histogram, &team),
            Some(Command::RunHistogram)
        );
        assert_eq!(
            hotkey_command(Action::Discovery, &team),
            Some(Command::RunDiscovery)
        );
        // Cycle only changes the selection; the overlay toggle belongs to the render thread.
        assert_eq!(hotkey_command(Action::Cycle, &team), None);
        assert_eq!(hotkey_command(Action::ToggleOverlay, &team), None);
    }

    #[test]
    fn commands_with_no_live_worldchrman_do_nothing_and_do_not_panic() {
        let mut team = team::TeamCtl::new(&Config::default(), 0, 0);
        let mut disc = fake_disc();
        for cmd in [
            Command::Set(6),
            Command::Set(200),
            Command::Restore,
            Command::TogglePersist,
            Command::TogglePersist,
            Command::RunDiscovery,
            Command::RunHistogram,
        ] {
            run_command(cmd, "test", &mut team, &mut disc, 0);
        }
        assert!(
            !disc.gate.is_busy(),
            "no worker may start without a live WorldChrMan"
        );
    }

    #[test]
    fn the_draining_main_loop_sees_overlay_commands_in_order() {
        let shared = Shared::new();
        shared.push(Command::Set(29));
        shared.push(Command::Restore);
        let mut team = team::TeamCtl::new(&Config::default(), 0, 0);
        let mut disc = fake_disc();
        let drained = shared.drain();
        assert_eq!(drained, vec![Command::Set(29), Command::Restore]);
        for cmd in drained {
            run_command(cmd, "overlay", &mut team, &mut disc, 0);
        }
        // Set(29) selected the candidate even though there was no local player to write to.
        assert!(shared.drain().is_empty());
    }
}
