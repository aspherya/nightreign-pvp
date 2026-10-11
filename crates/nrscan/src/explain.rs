//! `explain`: where does an address live, and what points at it.

#![allow(clippy::chunks_exact_to_as_chunks)]

use crate::logparse::LogInfo;
use crate::route::{self, Route, RouteCfg, RouteOut};
use crate::rtti::{fmt_in, Module, Rtti};
use crate::scan::{chunk_jobs, threads, Say};
use crate::winmem::{protect_str, Proc, Region};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Span we treat as "inside a PlayerIns". The size of NR's PlayerIns is UNKNOWN; verified fields reach +0xB78, so 0x2000 is a
/// generous assumption.
pub const PLAYERINS_SPAN: u64 = 0x2000;
/// Spec: "within 0x4000 of a singleton instance".
pub const SINGLETON_SPAN: u64 = 0x4000;
pub const BACK: u64 = 0x2000;
const HIT_CAP: usize = 64;
const L2_TARGETS: usize = 16;

/// (label, offset) if the address lies inside a known PlayerIns or singleton instance.
pub fn locate(info: &LogInfo, addr: u64) -> Option<(String, u64)> {
    for &p in &info.players {
        if addr >= p && addr - p < PLAYERINS_SPAN {
            return Some(("PlayerIns".to_string(), addr - p));
        }
    }
    for (n, a) in &info.singletons {
        if addr >= *a && addr - *a < SINGLETON_SPAN {
            return Some((n.clone(), addr - *a));
        }
    }
    None
}

/// Scan for 8-byte aligned values inside any of the inclusive ranges. Returns (address of the hit, value), capped.
pub fn pointer_scan(
    proc: &Proc,
    regions: &[Region],
    targets: &[(u64, u64)],
    cap: usize,
) -> Vec<(u64, u64)> {
    let jobs = chunk_jobs(regions);
    let lo = targets.iter().map(|t| t.0).min().unwrap_or(u64::MAX);
    let hi = targets.iter().map(|t| t.1).max().unwrap_or(0);
    let next = AtomicUsize::new(0);
    let hits: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
    let full = AtomicUsize::new(0);
    let scan = |base: u64, buf: &[u8], out: &mut Vec<(u64, u64)>| {
        for (i, w) in buf.chunks_exact(8).enumerate() {
            let v = u64::from_le_bytes([w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7]]);
            if v >= lo && v <= hi && targets.iter().any(|t| v >= t.0 && v <= t.1) {
                out.push((base + (i as u64) * 8, v));
            }
        }
    };
    std::thread::scope(|sc| {
        for _ in 0..threads() {
            sc.spawn(|| {
                let mut buf: Vec<u8> = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= jobs.len() || full.load(Ordering::Relaxed) != 0 {
                        break;
                    }
                    let (base, len) = jobs[i];
                    buf.resize(len, 0);
                    let mut local = Vec::new();
                    if proc.read(base, &mut buf[..len]) {
                        scan(base, &buf[..len], &mut local);
                    } else {
                        for (pi, page) in buf[..len].chunks_mut(4096).enumerate() {
                            let a = base + (pi as u64) * 4096;
                            if proc.read(a, page) {
                                scan(a, page, &mut local);
                            }
                        }
                    }
                    if !local.is_empty() {
                        if let Ok(mut h) = hits.lock() {
                            h.extend(local);
                            if h.len() >= cap {
                                full.store(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
            });
        }
    });
    let mut h = hits.into_inner().unwrap_or_default();
    h.sort_unstable();
    h.truncate(cap);
    h
}

#[derive(Clone, Debug, Default)]
pub struct ExplainOpts {
    pub route: bool,
    pub cfg: RouteCfg,
}

/// The main exe module only (RTTI of the game's own classes); empty when the module list is unavailable.
pub fn main_module(proc: &Proc) -> Vec<Module> {
    proc.modules().into_iter().take(1).collect()
}

fn short_class(rtti: &Rtti, obj: u64) -> String {
    match rtti.class_at(obj) {
        Some(n) => format!("<{n} @{obj:#X}>"),
        None => format!("<? @{obj:#X}>"),
    }
}

pub fn print_routes(rtti: &Rtti, out: &RouteOut, targets: &[u64], say: Say) {
    say(&format!(
        "route search: {} object(s) visited, depth {} done{}",
        out.visited,
        out.depth_done,
        out.stopped
            .as_ref()
            .map(|s| format!(", stopped: {s}"))
            .unwrap_or_default()
    ));
    for &t in targets {
        let mut rs: Vec<(&Route, usize)> = out
            .routes
            .iter()
            .filter(|r| r.target == t)
            .map(|r| {
                let named = r
                    .objects()
                    .iter()
                    .filter(|o| rtti.class_at(**o).is_some())
                    .count();
                (r, named)
            })
            .collect();
        rs.sort_by(|a, b| {
            a.0.hops
                .len()
                .cmp(&b.0.hops.len())
                .then(b.1.cmp(&a.1))
                .then(a.0.expr().cmp(&b.0.expr()))
        });
        if rs.is_empty() {
            say(&format!("  target {t:#X}: no route found"));
            continue;
        }
        say(&format!(
            "  target {t:#X}: {} route(s) with {} hop(s)",
            rs.len(),
            rs[0].0.hops.len()
        ));
        for (r, named) in rs.iter().take(10) {
            say(&format!(
                "    {}   ({} hop(s), {} of {} object(s) with a named class)",
                r.expr(),
                r.hops.len(),
                named,
                r.hops.len() + 1
            ));
            let mut line = format!("      {} {}", r.root, short_class(rtti, r.root_addr));
            for (off, o) in &r.hops {
                let named = rtti.class_at(*o).is_some();
                line.push_str(&format!(
                    "  -[+{off:#X}]->  {}{}",
                    short_class(rtti, *o),
                    if named { " *" } else { "" }
                ));
            }
            line.push_str(&format!("  => target = obj+{:#X}", r.delta));
            say(&line);
        }
    }
}

/// Roots for the route search: every PlayerIns in the log plus the WorldChrMan instance if the singleton map has it.
pub fn route_roots(info: &LogInfo) -> Vec<(String, u64)> {
    let mut roots: Vec<(String, u64)> = info
        .players
        .iter()
        .map(|p| ("PlayerIns".to_string(), *p))
        .collect();
    for (n, a) in &info.singletons {
        if n == "WorldChrMan" {
            roots.push((n.clone(), *a));
        }
    }
    roots
}

pub fn run_routes(
    proc: &Proc,
    info: &LogInfo,
    rtti: &Rtti,
    targets: &[u64],
    cfg: &RouteCfg,
    say: Say,
) {
    let roots = route_roots(info);
    if roots.is_empty() {
        say("route search: no PlayerIns / WorldChrMan root in the log");
        return;
    }
    let mut ranges: Vec<(u64, u64)> = proc
        .regions()
        .iter()
        .map(|r| (r.base, r.base + r.size))
        .collect();
    ranges.sort_unstable();
    let is_heap = move |v: u64| {
        let i = ranges.partition_point(|r| r.0 <= v);
        i > 0 && v < ranges[i - 1].1
    };
    say(&format!(
        "route search from {} root(s) ({}), depth <= {}, <= {} objects, <= {:?}",
        roots.len(),
        roots
            .iter()
            .map(|r| format!("{}@{:#X}", r.0, r.1))
            .collect::<Vec<_>>()
            .join(", "),
        cfg.depth,
        cfg.max_objects,
        cfg.time_cap
    ));
    let out = route::search(proc, &is_heap, &roots, targets, cfg);
    print_routes(rtti, &out, targets, say);
}

pub fn explain(proc: &Proc, info: &LogInfo, addrs: &[u64], opts: &ExplainOpts, say: Say) {
    let regions = proc.regions();
    let mods = main_module(proc);
    match mods.first() {
        Some(m) => say(&format!(
            "RTTI from the main module {} base {:#X} size {:#X}",
            m.name, m.base, m.size
        )),
        None => say("RTTI: could not enumerate modules, class names unavailable"),
    }
    let rtti = Rtti::new(proc, mods);
    for &addr in addrs {
        say(&format!("--- explain {addr:#X}"));
        match proc.query(addr) {
            Some((base, size, state, typ, prot)) => say(&format!(
                "  region: base {base:#X} size {size:#X} ({} MB) state {state:#X} type {typ:#X} protect {}",
                size >> 20,
                protect_str(prot)
            )),
            None => say("  region: query failed"),
        }
        match locate(info, addr) {
            Some((n, off)) => say(&format!("  inside {n} +{off:#X} (from the log)")),
            None => say("  not inside a PlayerIns or singleton instance listed in the log"),
        }
        let cont = rtti.containing(addr, 0x4000);
        if cont.is_empty() {
            say("  no object with a named vtable within 0x4000 below the address");
        } else {
            for (i, (o, n)) in cont.iter().take(8).enumerate() {
                say(&format!(
                    "  {} {}",
                    if i == 0 {
                        "nearest object:"
                    } else {
                        "      also before:"
                    },
                    fmt_in(addr, *o, n)
                ));
            }
            if cont.len() > 8 {
                say(&format!("  ... and {} more further back", cont.len() - 8));
            }
        }
        let l1 = pointer_scan(
            proc,
            &regions,
            &[(addr.saturating_sub(BACK).max(0x1_0000), addr)],
            HIT_CAP,
        );
        say(&format!(
            "  pointers into [addr-{BACK:#X}, addr]: {} hit(s){}",
            l1.len(),
            if l1.len() >= HIT_CAP { " (cap)" } else { "" }
        ));
        let holder = |h: u64| -> String {
            match rtti.containing(h, 0x4000).first() {
                Some((o, n)) => format!(" held by <{n} @{o:#X}>+{:#X}", h - o),
                None => String::new(),
            }
        };
        let mut unresolved: Vec<(u64, u64)> = Vec::new();
        for &(h, v) in &l1 {
            let delta = addr - v;
            let tgt = rtti
                .class_at(v)
                .map(|n| format!(" target is <{n}>"))
                .unwrap_or_default();
            match locate(info, h) {
                Some((n, off)) => say(&format!(
                    "    hit {h:#X} -> {v:#X}: [{n}+{off:#X}]+{delta:#X}{tgt}"
                )),
                None => {
                    say(&format!(
                        "    hit {h:#X} -> {v:#X}: not in a known struct (target +{delta:#X}){}{tgt}",
                        holder(h)
                    ));
                    unresolved.push((h, v));
                }
            }
        }
        if !unresolved.is_empty() {
            unresolved.truncate(L2_TARGETS);
            let targets: Vec<(u64, u64)> = unresolved
                .iter()
                .map(|(h, _)| (h.saturating_sub(BACK), *h))
                .collect();
            let l2 = pointer_scan(proc, &regions, &targets, HIT_CAP);
            say(&format!(
                "  level 2: pointers into the first {} unresolved hit(s): {} hit(s)",
                unresolved.len(),
                l2.len()
            ));
            for (h2, v2) in l2 {
                if let Some((h, v)) = unresolved
                    .iter()
                    .find(|(h, _)| v2 >= h.saturating_sub(BACK) && v2 <= *h)
                {
                    let d2 = h - v2;
                    let d1 = addr - v;
                    match locate(info, h2) {
                        Some((n, off)) => say(&format!("    hit2 {h2:#X} -> {v2:#X}: [[{n}+{off:#X}]+{d2:#X}]+{d1:#X}")),
                        None => say(&format!("    hit2 {h2:#X} -> {v2:#X}: not in a known struct (via {h:#X}, +{d2:#X}, +{d1:#X}){}", holder(h2))),
                    }
                }
            }
        }
    }
    if opts.route && !addrs.is_empty() {
        run_routes(proc, info, &rtti, addrs, &opts.cfg, say);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_structs() {
        let mut i = LogInfo::default();
        i.players.push(0x7FF3C12EE980);
        i.singletons.push(("WorldChrMan".into(), 0x7FF410810740));
        assert_eq!(
            locate(&i, 0x7FF3C12EE980 + 0xB78),
            Some(("PlayerIns".into(), 0xB78))
        );
        assert_eq!(
            locate(&i, 0x7FF410810740 + 0x3FFF),
            Some(("WorldChrMan".into(), 0x3FFF))
        );
        assert_eq!(locate(&i, 0x7FF410810740 + 0x4000), None);
    }

    #[test]
    fn pointer_scan_finds_own_pointer() {
        let target: &'static u64 = Box::leak(Box::new(0xDEAD_BEEFu64));
        let holder: &'static u64 = Box::leak(Box::new(target as *const u64 as u64));
        let proc = Proc::open(std::process::id()).unwrap();
        let regions = proc.regions();
        let t = target as *const u64 as u64;
        let hits = pointer_scan(&proc, &regions, &[(t, t)], 1_000_000);
        assert!(hits.iter().any(|h| h.0 == holder as *const u64 as u64));
    }
}
