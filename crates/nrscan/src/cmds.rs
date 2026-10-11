//! Command implementations added in T-038: `peek`, `poke`, `bar --changed`.

use crate::changed::{self, HitFeed, Role};
use crate::logparse::LogInfo;
use crate::report::say;
use crate::scan::Say;
use crate::{
    explain, flag, near_regions, opt, out_path, parse_addr, positionals, region_summary, save,
    say_fn, target, target_with, wait_for_down, Watch, VALUED,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Snapshot memory budget for `bar --changed`.
pub const SNAPSHOT_CAP: u64 = 1536 << 20;

fn one_addr(rest: &[String], extra_valued: &[&str]) -> Result<u64, String> {
    let mut valued: Vec<&str> = VALUED.to_vec();
    valued.extend_from_slice(extra_valued);
    let pos = positionals(rest, &valued);
    match pos.as_slice() {
        [a] => parse_addr(a).ok_or_else(|| format!("bad address '{a}'")),
        [] => Err("need an address".into()),
        _ => Err("need exactly one address".into()),
    }
}

// ------------------------------------------------------------ peek

pub fn cmd_peek(rest: &[String]) -> i32 {
    match peek_inner(rest) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn peek_inner(rest: &[String]) -> Result<i32, String> {
    let addr = one_addr(rest, &[])?;
    let tg = target(rest)?;
    let interval: u64 = opt(rest, "--interval-ms")?.unwrap_or(500);
    let count: u64 = opt(rest, "--count")?.unwrap_or(0);
    let max_secs: f64 = opt(rest, "--max-secs")?.unwrap_or(0.0);
    let hex: Option<usize> = opt(rest, "--hex")?;
    let (f, u) = (flag(rest, "--f32"), flag(rest, "--u32"));
    let t0 = Instant::now();
    let mut n = 0u64;
    println!(
        "peek {} pid {} {addr:#X} every {interval} ms (Ctrl+C to stop)",
        tg.name, tg.proc.pid
    );
    loop {
        let t = t0.elapsed().as_secs_f64();
        match hex {
            Some(len) => {
                let mut buf = vec![0u8; len.clamp(1, 4096)];
                if tg.proc.read(addr, &mut buf) {
                    println!("[{t:7.2}s]");
                    for (i, row) in buf.chunks(16).enumerate() {
                        let h: Vec<String> = row.iter().map(|b| format!("{b:02X}")).collect();
                        println!("  {:#X}: {}", addr + (i * 16) as u64, h.join(" "));
                    }
                } else {
                    println!("[{t:7.2}s] {addr:#X}: unreadable");
                }
            }
            None => {
                let mut b = [0u8; 4];
                if tg.proc.read(addr, &mut b) {
                    let bits = u32::from_le_bytes(b);
                    let fs = format!("f32 {}", f32::from_bits(bits));
                    let us = format!("u32 {bits} (0x{bits:08X})");
                    let line = match (f, u) {
                        (true, false) => fs,
                        (false, true) => us,
                        _ => format!("{fs}   {us}"),
                    };
                    println!("[{t:7.2}s] {addr:#X}: {line}");
                } else {
                    println!("[{t:7.2}s] {addr:#X}: unreadable");
                }
            }
        }
        n += 1;
        if (count > 0 && n >= count) || (max_secs > 0.0 && t0.elapsed().as_secs_f64() >= max_secs) {
            break;
        }
        std::thread::sleep(Duration::from_millis(interval));
    }
    Ok(0)
}

// ------------------------------------------------------------ poke

pub fn cmd_poke(rest: &[String]) -> i32 {
    match poke_inner(rest) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn poke_inner(rest: &[String]) -> Result<i32, String> {
    let addr = one_addr(rest, &["--f32", "--u32"])?;
    let f: Option<f32> = opt(rest, "--f32")?;
    let u: Option<u32> = opt(rest, "--u32")?;
    let tg = target_with(rest, true)?;
    let bytes = match (f, u) {
        (Some(v), None) => v.to_bits(),
        (None, Some(v)) => v,
        _ => return Err("give exactly one of --f32 <v> or --u32 <v>".into()),
    };
    let mut b = [0u8; 4];
    if !tg.proc.read(addr, &mut b) {
        return Err(format!("cannot read {addr:#X}"));
    }
    let old = u32::from_le_bytes(b);
    tg.proc.write(addr, &bytes.to_le_bytes())?;
    let mut r = [0u8; 4];
    if !tg.proc.read(addr, &mut r) {
        return Err(format!("cannot read back {addr:#X}"));
    }
    let rb = u32::from_le_bytes(r);
    println!(
        "poke {} {addr:#X}: {old:#010X} (f32 {}) -> wrote {bytes:#010X} (f32 {}) -> readback {rb:#010X} (f32 {}){}",
        tg.name,
        f32::from_bits(old),
        f32::from_bits(bytes),
        f32::from_bits(rb),
        if rb == bytes { "" } else { "  [READBACK DIFFERS: the game rewrote it]" }
    );
    Ok(0)
}

// ------------------------------------------------------------ bar --changed

pub fn cmd_bar_changed(rest: &[String]) -> i32 {
    match bar_changed_inner(rest) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn fmt_series(vals: &[u32], times: &[f32], f32mode: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut last: Option<u32> = None;
    for (i, &b) in vals.iter().enumerate() {
        if last != Some(b) {
            let t = times.get(i).copied().unwrap_or(0.0);
            parts.push(if f32mode {
                format!("{:.4}@{t:.1}", f32::from_bits(b))
            } else {
                format!("{b}@{t:.1}")
            });
            last = Some(b);
        }
    }
    parts.join(" ")
}

fn bar_changed_inner(rest: &[String]) -> Result<i32, String> {
    let tg = target(rest)?;
    let interval: u64 = opt(rest, "--interval-ms")?.unwrap_or(300);
    let max_secs: f64 = opt(rest, "--max-secs")?.unwrap_or(40.0);
    let near_mb: u64 = opt(rest, "--near-mb")?.unwrap_or(256);
    let everywhere = flag(rest, "--everywhere");
    let keyboard = flag(rest, "--hits-from-keyboard");
    let info = LogInfo::load(&tg.log);
    say(format!(
        "nrscan bar --changed: {} pid {} (interval {} ms, hits from {})",
        tg.name,
        tg.proc.pid,
        interval,
        if keyboard {
            "the keyboard (press Enter at each hit)"
        } else {
            "memory (PlayerIns+0x198 / +0x1A0)"
        }
    ));
    say(format!(
        "log: {} PlayerIns address(es), {} singleton instance(s)",
        info.players.len(),
        info.singletons.len()
    ));
    let mut w = Watch::new(&tg.log);
    if flag(rest, "--now") {
        say("--now: starting immediately");
    } else {
        wait_for_down(&mut w, &tg.log);
        say("LOCAL DOWN seen: snapshotting");
    }
    let started = Instant::now();
    let all = tg.proc.regions();
    region_summary(&all);
    let total = |r: &[crate::winmem::Region]| r.iter().map(|x| x.size).sum::<u64>();
    let mut regions = if everywhere {
        all.clone()
    } else {
        let (r, anchors) = near_regions(&all, &info, near_mb, flag(rest, "--near-all"));
        if anchors.is_empty() {
            return Err("no PlayerIns address in the log to anchor on (use --everywhere only if memory allows)".into());
        }
        r
    };
    let mut mb = near_mb;
    while total(&regions) > SNAPSHOT_CAP {
        if everywhere || mb < 16 {
            return Err(format!(
                "regions cover {} MB, over the {} MB snapshot cap",
                total(&regions) >> 20,
                SNAPSHOT_CAP >> 20
            ));
        }
        mb /= 2;
        say(format!(
            "snapshot cap {} MB exceeded: radius halved to {mb} MB",
            SNAPSHOT_CAP >> 20
        ));
        regions = near_regions(&all, &info, mb, flag(rest, "--near-all")).0;
    }
    say(format!(
        "snapshot set: {} piece(s), {} MB",
        regions.len(),
        total(&regions) >> 20
    ));
    // hit sources
    let feed = Arc::new(HitFeed::default());
    let stop = Arc::new(AtomicBool::new(false));
    let mut handle = None;
    if keyboard {
        changed::spawn_keyboard_hits(started, feed.clone());
        say("press Enter right after each hit lands");
    } else {
        let players: Vec<u64> = match opt::<String>(rest, "--hit-player")? {
            Some(s) => vec![parse_addr(&s).ok_or("bad --hit-player address")?],
            None => info.players.clone(),
        };
        if players.is_empty() {
            return Err("no PlayerIns in the log to watch for hits; use --hits-from-keyboard or --hit-player".into());
        }
        say(format!(
            "watching {} PlayerIns for attacker-handle changes",
            players.len()
        ));
        handle = Some(changed::spawn_mem_hits(
            tg.proc.pid,
            players,
            flag(rest, "--hit-hp"),
            started,
            feed.clone(),
            stop.clone(),
        ));
    }
    let qs = say_fn();
    let sref: Say = &qs;
    let out = changed::changed_scan(
        &tg.proc,
        &regions,
        interval,
        started,
        &feed,
        &mut || {
            w.poll();
            w.up || started.elapsed().as_secs_f64() > max_secs
        },
        sref,
    );
    stop.store(true, Ordering::Relaxed);
    if let Some(h) = handle {
        let _ = h.join();
    }
    say(format!(
        "scan ended {:.1}s after start{}",
        started.elapsed().as_secs_f64(),
        if w.up {
            " (LOCAL left the downed state)"
        } else {
            ""
        }
    ));
    if let Some(n) = &out.note {
        say(format!("NOTE: {n}"));
    }
    if let Ok(notes) = feed.notes.lock() {
        say(format!("{} hit event(s) recorded:", notes.len()));
        for n in notes.iter().take(40) {
            say(format!("  {n}"));
        }
    }
    say(format!(
        "snapshot {} MB; {} seed word(s); {} hit group(s), {} non-hit sample(s)",
        out.snapshot_bytes >> 20,
        out.seeded,
        out.totals.groups,
        out.totals.nonhit
    ));
    say(format!(
        "sample roles: {}",
        out.roles
            .iter()
            .map(|(t, r)| match r {
                Role::NonHit => format!("{t:.1}:."),
                Role::Group(g) => format!("{t:.1}:H{g}"),
            })
            .collect::<Vec<_>>()
            .join(" ")
    ));
    if out.totals.groups < 3 {
        say("WARNING: fewer than 3 hit groups; the 60% / 10% criteria are weak");
    }
    say(format!(
        "{} word(s) changed on >= 60% of the hit groups and <= 10% of the non-hit samples",
        out.rows.len()
    ));
    for r in out.rows.iter().take(100) {
        say(format!(
            "  {:#X} hit groups {}/{} non-hit {}/{}",
            r.addr,
            r.corr.groups_changed,
            out.totals.groups,
            r.corr.nonhit_changed,
            out.totals.nonhit
        ));
        say(format!(
            "      f32: {}",
            fmt_series(&r.hist, &out.times, true)
        ));
        say(format!(
            "      u32: {}",
            fmt_series(&r.hist, &out.times, false)
        ));
    }
    let top: Vec<u64> = out.rows.iter().take(10).map(|r| r.addr).collect();
    if !top.is_empty() {
        say(format!("explaining the top {} word(s)", top.len()));
        let opts = explain::ExplainOpts {
            route: true,
            ..Default::default()
        };
        explain::explain(&tg.proc, &info, &top, &opts, sref);
    }
    save(&out_path("barchanged"));
    Ok(if out.rows.is_empty() { 3 } else { 0 })
}
