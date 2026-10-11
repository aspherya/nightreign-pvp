//! Breadth-first route search: from root objects (PlayerIns, WorldChrMan) following 8-aligned pointers found in the first
//! `span` bytes of each visited object, until an object's range `[target - span, target]` contains the target address.
#![allow(clippy::chunks_exact_to_as_chunks)]

use crate::mem::{read_lenient, MemSrc};
use crate::scan::threads;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct RouteCfg {
    pub depth: usize,
    pub max_objects: usize,
    pub time_cap: Duration,
    pub span: u64,
    pub keep: usize,
}

impl Default for RouteCfg {
    fn default() -> Self {
        RouteCfg {
            depth: 4,
            max_objects: 200_000,
            time_cap: Duration::from_secs(20),
            span: 0x1000,
            keep: 10,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub target: u64,
    pub root: String,
    pub root_addr: u64,
    /// (offset read inside the previous object, pointer value = next object address)
    pub hops: Vec<(u64, u64)>,
    /// target - last object address
    pub delta: u64,
}

impl Route {
    /// `[[PlayerIns+0xA]+0xB]+0xC`
    pub fn expr(&self) -> String {
        let mut s = self.root.clone();
        for (off, _) in &self.hops {
            s = format!("[{s}+{off:#X}]");
        }
        format!("{s}+{:#X}", self.delta)
    }

    /// Object addresses along the route, root first.
    pub fn objects(&self) -> Vec<u64> {
        std::iter::once(self.root_addr)
            .chain(self.hops.iter().map(|h| h.1))
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct RouteOut {
    pub routes: Vec<Route>,
    pub visited: usize,
    pub depth_done: usize,
    pub stopped: Option<String>,
}

struct Node {
    addr: u64,
    parent: u32,
    off: u32,
    root: u32,
}

struct Scan {
    node: usize,
    ptrs: Vec<(u32, u64)>,
    hits: Vec<(usize, u32, u64)>,
}

pub fn search(
    mem: &dyn MemSrc,
    is_heap: &(dyn Fn(u64) -> bool + Sync),
    roots: &[(String, u64)],
    targets: &[u64],
    cfg: &RouteCfg,
) -> RouteOut {
    let t0 = Instant::now();
    let mut out = RouteOut::default();
    let mut nodes: Vec<Node> = Vec::new();
    let mut seen: HashSet<u64> = HashSet::new();
    let mut active: Vec<bool> = vec![true; targets.len()];
    let in_range = |obj: u64, t: u64| obj <= t && t - obj <= cfg.span;
    let mut per_target = vec![0usize; targets.len()];
    for (ri, (_, a)) in roots.iter().enumerate() {
        if seen.insert(*a) {
            nodes.push(Node {
                addr: *a,
                parent: u32::MAX,
                off: 0,
                root: ri as u32,
            });
        }
    }
    // zero-hop routes: the target lies inside the root object itself
    for (ti, &t) in targets.iter().enumerate() {
        for (ri, (name, a)) in roots.iter().enumerate() {
            let _ = ri;
            if in_range(*a, t) {
                out.routes.push(Route {
                    target: t,
                    root: name.clone(),
                    root_addr: *a,
                    hops: Vec::new(),
                    delta: t - a,
                });
                per_target[ti] += 1;
                active[ti] = false;
            }
        }
    }
    let mut frontier: Vec<usize> = (0..nodes.len()).collect();
    let nt = threads();
    'levels: for depth in 1..=cfg.depth {
        if !active.iter().any(|a| *a) {
            break;
        }
        let mut next: Vec<usize> = Vec::new();
        for batch in frontier.chunks(nt * 64) {
            if t0.elapsed() > cfg.time_cap {
                out.stopped = Some(format!("time cap {:?} at depth {depth}", cfg.time_cap));
                break 'levels;
            }
            if nodes.len() >= cfg.max_objects {
                out.stopped = Some(format!(
                    "object cap {} reached at depth {depth}",
                    cfg.max_objects
                ));
                break 'levels;
            }
            let idx = AtomicUsize::new(0);
            let res: Mutex<Vec<Scan>> = Mutex::new(Vec::new());
            let (nodes_ref, active_ref) = (&nodes, &active);
            std::thread::scope(|sc| {
                for _ in 0..nt.min(batch.len()) {
                    sc.spawn(|| loop {
                        let i = idx.fetch_add(1, Ordering::Relaxed);
                        if i >= batch.len() {
                            break;
                        }
                        let ni = batch[i];
                        let a = nodes_ref[ni].addr;
                        let buf = read_lenient(mem, a, cfg.span as usize);
                        let mut sc_out = Scan {
                            node: ni,
                            ptrs: Vec::new(),
                            hits: Vec::new(),
                        };
                        let mut local: HashSet<u64> = HashSet::new();
                        for (k, w) in buf.chunks_exact(8).enumerate() {
                            let v = u64::from_le_bytes(w.try_into().unwrap_or([0; 8]));
                            if v < 0x1_0000 || v == a {
                                continue;
                            }
                            for (ti, &t) in targets.iter().enumerate() {
                                if active_ref[ti] && in_range(v, t) {
                                    sc_out.hits.push((ti, (k * 8) as u32, v));
                                }
                            }
                            if is_heap(v) && local.insert(v) {
                                sc_out.ptrs.push(((k * 8) as u32, v));
                            }
                        }
                        if let Ok(mut r) = res.lock() {
                            r.push(sc_out);
                        }
                    });
                }
            });
            let mut results = res.into_inner().unwrap_or_default();
            results.sort_by_key(|r| r.node);
            for r in results {
                for (ti, off, v) in r.hits {
                    if per_target[ti] >= cfg.keep {
                        continue;
                    }
                    let mut hops = vec![(off as u64, v)];
                    let mut cur = r.node;
                    while nodes[cur].parent != u32::MAX {
                        hops.push((nodes[cur].off as u64, nodes[cur].addr));
                        cur = nodes[cur].parent as usize;
                    }
                    hops.reverse();
                    let root = &roots[nodes[cur].root as usize];
                    out.routes.push(Route {
                        target: targets[ti],
                        root: root.0.clone(),
                        root_addr: root.1,
                        hops,
                        delta: targets[ti] - v,
                    });
                    per_target[ti] += 1;
                }
                for (off, v) in r.ptrs {
                    if nodes.len() < cfg.max_objects && seen.insert(v) {
                        nodes.push(Node {
                            addr: v,
                            parent: r.node as u32,
                            off,
                            root: nodes[r.node].root,
                        });
                        next.push(nodes.len() - 1);
                    }
                }
            }
        }
        // targets with a route at this depth are finished (shortest routes only)
        for ti in 0..targets.len() {
            if per_target[ti] > 0 {
                active[ti] = false;
            }
        }
        out.depth_done = depth;
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    out.visited = nodes.len();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::FakeMem;

    // root(0x10000000) -+0x18-> A(0x20000000) -+0x40-> B(0x30000000) -+0x8-> C(0x40000000); target inside C at +0x2B4
    // plus a decoy: root +0x20 -> D(0x50000000) -+0x0-> A (longer route to the same objects)
    fn graph() -> FakeMem {
        let mut m = FakeMem::default();
        for base in [
            0x1000_0000u64,
            0x2000_0000,
            0x3000_0000,
            0x4000_0000,
            0x5000_0000,
        ] {
            m.zeros(base, 0x1000);
        }
        m.put_u64(0x1000_0000 + 0x18, 0x2000_0000);
        m.put_u64(0x1000_0000 + 0x20, 0x5000_0000);
        m.put_u64(0x5000_0000, 0x2000_0000);
        m.put_u64(0x2000_0000 + 0x40, 0x3000_0000);
        m.put_u64(0x3000_0000 + 0x8, 0x4000_0000);
        m
    }

    fn heap(v: u64) -> bool {
        (0x1000_0000..0x6000_0000).contains(&v)
    }

    #[test]
    fn finds_shortest_route() {
        let m = graph();
        let out = search(
            &m,
            &heap,
            &[("Root".into(), 0x1000_0000)],
            &[0x4000_0000 + 0x2B4],
            &RouteCfg::default(),
        );
        assert_eq!(out.routes.len(), 1, "{out:?}");
        assert_eq!(out.routes[0].expr(), "[[[Root+0x18]+0x40]+0x8]+0x2B4");
        assert_eq!(
            out.routes[0].objects(),
            vec![0x1000_0000, 0x2000_0000, 0x3000_0000, 0x4000_0000]
        );
    }

    #[test]
    fn depth_cap_and_zero_hop() {
        let m = graph();
        let cfg = RouteCfg {
            depth: 2,
            ..Default::default()
        };
        let out = search(
            &m,
            &heap,
            &[("Root".into(), 0x1000_0000)],
            &[0x4000_0000 + 0x2B4],
            &cfg,
        );
        assert!(out.routes.is_empty());
        let out = search(
            &m,
            &heap,
            &[("Root".into(), 0x1000_0000)],
            &[0x1000_0000 + 0x300],
            &RouteCfg::default(),
        );
        assert_eq!(out.routes[0].expr(), "Root+0x300");
    }
}
