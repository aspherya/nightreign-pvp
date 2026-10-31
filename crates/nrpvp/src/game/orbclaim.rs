//! RW-5 (both builds): a boss's Dormant Power orb belongs to the first team that opens it. Game 1.03.3 / exe 1.3.3.0; the reading is in
//! MEMORY_MAP §22.
//!
//! After a field boss dies every game holds its own orb: a lot record in one of the pointer slots of `MapItemMan` (`[exe+0x3C10B68]`,
//! slots from `+0x40`; `+0x58` was the one seen). Writing 0 into that slot takes `F: Examine` and the compass marker away, writing the
//! pointer back restores both. This module
//!
//! 1. finds the records by content (never by a fixed slot) and gives each a key every game computes alike: the boss handle's high 5
//!    bytes folded to a `u32` ([`key_of`]),
//! 2. detects that LOCAL opened the Dormant Power screen next to an orb (a *claim*), publishes it on the data channel
//!    ([`crate::net::bus::topic::ORB_CLAIM`], value = team number + 1) and remembers it,
//! 3. switches the orb off on this game when another team's claim is known, and back on if a claim supersedes (the lowest Steam id
//!    wins a tie, `net::facts`) so that our own team owns it.
//!
//! [`Book`] is the pure part (tested); the rest reads and writes the game. The only write is the pointer slot, through
//! [`mem::swap_ptr`] (one atomic compare-and-swap, so a slot the game changed meanwhile is never overwritten). Log lines start with `orb:`.

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::mem;

/// `MapItemMan` singleton slot. `[VERIFIED: peek before/after a boss kill on two games, MEMORY_MAP §22]`
const RVA_MAP_ITEM_MAN: usize = 0x3C1_0B68;
/// The pointer slots scanned (`MapItemMan+0x40 .. +0x100`, 8 bytes each). Records are found by content, not by slot.
const SLOT_FIRST: usize = 0x40;
const SLOT_END: usize = 0x100;
/// Record fields: item id (`0x40xxxxxx` for a lot record), a flag word whose high byte is 0x48 for boss rewards (0x68 for map items),
/// the boss handle, the item lot id, and the pointer chain to the position node (floats at `+0xA0`, physics coordinates).
const REC_ID: usize = 0x28;
const REC_FLAGS: usize = 0x78;
const REC_HANDLE: usize = 0x80;
const REC_LOT: usize = 0xD8;
const REC_NODE: [usize; 2] = [0xE8, 0x88];
const NODE_POS: usize = 0xA0;
/// Lot ids of boss rewards seen so far: 6110000 (Dormant Power) is in this range.
const LOT_RANGE: std::ops::Range<u32> = 6_000_000..7_000_000;
/// LOCAL counts as standing at the orb inside this distance (metres, physics coordinates).
const CLAIM_RADIUS: f32 = 3.0;

/// One boss-reward lot record found in a `MapItemMan` slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rec {
    /// Offset of the slot inside `MapItemMan`.
    pub slot: usize,
    pub ptr: usize,
    pub id: u32,
    pub lot: u32,
    pub handle: u64,
}

/// Is this record a boss reward lot record? (pure)
pub(crate) fn is_boss_orb(flags: u64, id: u32, lot: u32) -> bool {
    flags >> 56 == 0x48 && id & 0xFF00_0000 == 0x4000_0000 && LOT_RANGE.contains(&lot)
}

/// The key of the orb of the boss with this handle: the handle's high 5 bytes (the low 3 differ per game, MEMORY_MAP §22) folded to 32
/// bits. Equal on every game for the same boss.
pub(crate) fn key_of(handle: u64) -> u32 {
    let h = handle >> 24;
    (h as u32) ^ ((h >> 32) as u32)
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

struct Orb {
    rec: Rec,
    pos: Option<[f32; 3]>,
    /// The slot was nulled by us.
    off: bool,
    /// Switched by the console: the automatic rules leave it alone.
    manual: bool,
    /// A switch failed (the slot or record had changed): not tried again for this record.
    blocked: bool,
}

/// What [`Book::plan`] asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Off(u32),
    On(u32),
}

/// The orbs seen on this game and who owns each (team number + 1, as published). Pure state, no game access.
#[derive(Default)]
pub(crate) struct Book {
    orbs: BTreeMap<u32, Orb>,
    owner: BTreeMap<u32, u32>,
}

impl Book {
    /// The records the scan found now. Returns log lines (appeared / replaced / disappeared). An orb we switched off is not in the
    /// scan (its slot is 0); it stays until a new record shows up under its key.
    pub(crate) fn observe(&mut self, found: &[(Rec, Option<[f32; 3]>)]) -> Vec<String> {
        let mut log = Vec::new();
        for (rec, pos) in found {
            let key = key_of(rec.handle);
            match self.orbs.get_mut(&key) {
                None => {
                    log.push(format!(
                        "orb {key:#010X} appeared (slot +{:#X}, lot {}, handle {:#X})",
                        rec.slot, rec.lot, rec.handle
                    ));
                    self.orbs.insert(
                        key,
                        Orb {
                            rec: *rec,
                            pos: *pos,
                            off: false,
                            manual: false,
                            blocked: false,
                        },
                    );
                }
                Some(o) if o.rec.ptr == rec.ptr && !o.off => {
                    o.rec = *rec;
                    o.pos = pos.or(o.pos);
                }
                Some(o) => {
                    log.push(format!(
                        "orb {key:#010X}: a new record (slot +{:#X}, handle {:#X}) replaced {}",
                        rec.slot,
                        rec.handle,
                        if o.off {
                            "the one we switched off (it is lost)"
                        } else {
                            "the old one"
                        }
                    ));
                    *o = Orb {
                        rec: *rec,
                        pos: *pos,
                        off: false,
                        manual: false,
                        blocked: false,
                    };
                }
            }
        }
        let gone: Vec<u32> = self
            .orbs
            .iter()
            .filter(|(k, o)| !o.off && !found.iter().any(|(r, _)| key_of(r.handle) == **k && r.ptr == o.rec.ptr))
            .map(|(k, _)| *k)
            .collect();
        for k in gone {
            self.orbs.remove(&k);
            log.push(format!("orb {k:#010X} disappeared"));
        }
        log
    }

    /// LOCAL opened the Dormant Power screen next to an unowned orb: the key of the nearest one within [`CLAIM_RADIUS`].
    pub(crate) fn claimable(&self, local: [f32; 3]) -> Option<u32> {
        self.orbs
            .iter()
            .filter(|(k, o)| !o.off && !self.owner.contains_key(k))
            .filter_map(|(k, o)| Some((*k, dist(local, o.pos?))))
            .filter(|(_, d)| *d <= CLAIM_RADIUS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    }

    /// LOCAL's team claims `key`. Returns `true` when this is news (the caller publishes it); an orb that already has an owner keeps it.
    pub(crate) fn claim_local(&mut self, key: u32, my: u32) -> bool {
        if self.owner.contains_key(&key) {
            return false;
        }
        self.owner.insert(key, my);
        true
    }

    /// A claim arrived from another game; it supersedes what we knew (the data channel already applied the lowest-id rule).
    pub(crate) fn claim_remote(&mut self, key: u32, team: u32) -> Option<u32> {
        self.owner.insert(key, team)
    }

    pub(crate) fn owner(&self, key: u32) -> Option<u32> {
        self.owner.get(&key).copied()
    }

    /// What to switch now, given our own team (`None` = unknown: nothing is switched).
    pub(crate) fn plan(&self, my: Option<u32>) -> Vec<Step> {
        let Some(my) = my else { return Vec::new() };
        self.orbs
            .iter()
            .filter(|(_, o)| !o.manual && !o.blocked)
            .filter_map(|(k, o)| match self.owner.get(k) {
                Some(t) if *t != my && !o.off => Some(Step::Off(*k)),
                Some(t) if *t == my && o.off => Some(Step::On(*k)),
                _ => None,
            })
            .collect()
    }

    fn rec(&self, key: u32) -> Option<Rec> {
        self.orbs.get(&key).map(|o| o.rec)
    }

    fn set_off(&mut self, key: u32, off: bool, manual: bool) {
        if let Some(o) = self.orbs.get_mut(&key) {
            o.off = off;
            o.manual = manual && off;
        }
    }

    fn block(&mut self, key: u32) {
        if let Some(o) = self.orbs.get_mut(&key) {
            o.blocked = true;
        }
    }

    fn clear(&mut self) {
        self.orbs.clear();
        self.owner.clear();
    }

    /// One line per orb for the console.
    fn describe(&self) -> Vec<String> {
        self.orbs
            .iter()
            .map(|(k, o)| {
                format!(
                    "orb {k:#010X}: {} lot {} handle {:#X} slot +{:#X} pos {} owner {}{}{}",
                    if o.off { "OFF" } else { "on" },
                    o.rec.lot,
                    o.rec.handle,
                    o.rec.slot,
                    o.pos
                        .map_or("?".into(), |p| format!("({:.1}, {:.1}, {:.1})", p[0], p[1], p[2])),
                    self.owner
                        .get(k)
                        .map_or("-".into(), |t| format!("team {}", t - 1)),
                    if o.manual { " (manual)" } else { "" },
                    if o.blocked { " (blocked)" } else { "" },
                )
            })
            .collect()
    }
}

// ---- the game side ----

struct State {
    book: Book,
    base: usize,
    my: Option<u32>,
    in_match: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    book: Book {
        orbs: BTreeMap::new(),
        owner: BTreeMap::new(),
    },
    base: 0,
    my: None,
    in_match: false,
});

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn read_node_pos(rec: usize) -> Option<[f32; 3]> {
    let n1 = mem::read::<usize>(rec + REC_NODE[0]).filter(|p| mem::is_ptr(*p))?;
    let n2 = mem::read::<usize>(n1 + REC_NODE[1]).filter(|p| mem::is_ptr(*p))?;
    let r = |o: usize| mem::read::<f32>(n2 + NODE_POS + o);
    let p = [r(0)?, r(4)?, r(8)?];
    p.iter().all(|v| v.is_finite()).then_some(p)
}

/// Every boss-reward lot record in the `MapItemMan` slots now (read only).
fn scan(base: usize) -> Vec<(Rec, Option<[f32; 3]>)> {
    let Some(man) = mem::read::<usize>(base + RVA_MAP_ITEM_MAN).filter(|m| mem::is_ptr(*m)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for slot in (SLOT_FIRST..SLOT_END).step_by(8) {
        let Some(ptr) = mem::read::<usize>(man + slot).filter(|p| mem::is_ptr(*p)) else {
            continue;
        };
        let (Some(flags), Some(id), Some(lot), Some(handle)) = (
            mem::read::<u64>(ptr + REC_FLAGS),
            mem::read::<u32>(ptr + REC_ID),
            mem::read::<u32>(ptr + REC_LOT),
            mem::read::<u64>(ptr + REC_HANDLE),
        ) else {
            continue;
        };
        if is_boss_orb(flags, id, lot) {
            let rec = Rec {
                slot,
                ptr,
                id,
                lot,
                handle,
            };
            out.push((rec, read_node_pos(ptr)));
        }
    }
    out
}

fn slot_addr(st: &State, rec: &Rec) -> Option<usize> {
    mem::read::<usize>(st.base + RVA_MAP_ITEM_MAN)
        .filter(|m| mem::is_ptr(*m))
        .map(|m| m + rec.slot)
}

/// The record still is what we saved (nothing else was allocated over it).
fn record_intact(rec: &Rec) -> bool {
    mem::read::<u64>(rec.ptr + REC_HANDLE) == Some(rec.handle)
        && mem::read::<u32>(rec.ptr + REC_LOT) == Some(rec.lot)
        && mem::read::<u32>(rec.ptr + REC_ID) == Some(rec.id)
}

/// Null the orb's slot (only if it still holds the record) and remember the pointer in the book.
fn switch_off(st: &mut State, key: u32, manual: bool) -> Result<(), String> {
    let rec = st.book.rec(key).ok_or("unknown orb")?;
    let addr = slot_addr(st, &rec).ok_or("MapItemMan is not there")?;
    if !record_intact(&rec) {
        st.book.block(key);
        return Err("the record changed".into());
    }
    mem::swap_ptr(addr, rec.ptr, 0).inspect_err(|_| st.book.block(key))?;
    st.book.set_off(key, true, manual);
    Ok(())
}

/// Write the saved pointer back, only if the slot is still 0 and the record still is what we saved.
fn switch_on(st: &mut State, key: u32) -> Result<(), String> {
    let rec = st.book.rec(key).ok_or("unknown orb")?;
    let addr = slot_addr(st, &rec).ok_or("MapItemMan is not there")?;
    if !record_intact(&rec) {
        st.book.block(key);
        return Err("the record changed, left as it is".into());
    }
    mem::swap_ptr(addr, 0, rec.ptr).inspect_err(|_| st.book.block(key))?;
    st.book.set_off(key, false, false);
    Ok(())
}

fn apply(st: &mut State) {
    for step in st.book.plan(st.my) {
        let (key, on) = match step {
            Step::Off(k) => (k, false),
            Step::On(k) => (k, true),
        };
        let owner = st.book.owner(key).map_or(0, |t| t.saturating_sub(1));
        let res = if on {
            switch_on(st, key)
        } else {
            switch_off(st, key, false)
        };
        match res {
            Ok(()) if on => tracing::info!("orb: {key:#010X} is ours (team {owner}) -> on"),
            Ok(()) => tracing::info!("orb: {key:#010X} claimed by team {owner} (rival) -> off"),
            Err(e) => tracing::warn!(
                "orb: {key:#010X} could not be switched {}: {e}",
                if on { "on" } else { "off" }
            ),
        }
    }
}

/// The menu word (`CSMenuMan+0x48`) that says the Dormant Power screen is open. UNKNOWN until measured (MEMORY_MAP §22).
fn dormant_screen_open(base: usize) -> bool {
    super::menugate::menu_open(base)
}

fn local_phys(local: usize) -> Option<[f32; 3]> {
    let mods = mem::read::<usize>(local + super::layout::PLAYER_MODULES)?;
    let phys = mem::read::<usize>(mods + 0x68).filter(|p| mem::is_ptr(*p))?;
    let r = |o: usize| mem::read::<f32>(phys + super::layout::PHYSICS_POS + o);
    Some([r(0)?, r(4)?, r(8)?])
}

/// Main loop, every pass (both builds). `my_team` is LOCAL's team number (`None` until the rosters are known), `local` its `PlayerIns`.
pub(crate) fn tick(base: usize, in_match: bool, my_team: Option<u8>, local: Option<usize>) {
    use crate::net::bus::{self, topic};
    let mut st = lock();
    st.base = base;
    st.my = my_team.map(|t| u32::from(t) + 1);
    if st.in_match != in_match {
        st.in_match = in_match;
        if !in_match {
            // A new match starts from nothing (the data channel forgets its keys when the lobby is left).
            st.book.clear();
        }
    }
    let found = scan(base);
    for line in st.book.observe(&found) {
        tracing::info!("{line}");
    }
    // Facts first, so a claim that arrived with the record is applied in the same pass.
    for r in bus::take(topic::ORB_CLAIM) {
        let team = r.fact.value;
        if team == 0 {
            continue;
        }
        let key = r.fact.key;
        let before = st.book.claim_remote(key, team);
        if before != Some(team) {
            tracing::info!(
                "orb: {key:#010X} claim by team {} arrived{}",
                team - 1,
                before.map_or(String::new(), |b| format!(" (was team {})", b - 1))
            );
        }
    }
    if !in_match {
        return;
    }
    if let (Some(my), Some(pos)) = (st.my, local.and_then(local_phys)) {
        if dormant_screen_open(base) {
            if let Some(key) = st.book.claimable(pos) {
                claim_local(&mut st, key, my);
            }
        }
    }
    apply(&mut st);
}

fn claim_local(st: &mut State, key: u32, my: u32) {
    if st.book.claim_local(key, my) {
        let sent = crate::net::bus::publish(crate::net::bus::topic::ORB_CLAIM, key, my);
        tracing::info!(
            "orb: {key:#010X} claimed by me (team {}) -> published {}",
            my - 1,
            if sent { "ok" } else { "FAILED (queue full)" }
        );
    }
}

// ---- console (dev builds) ----

fn parse_key(s: &str) -> Result<u32, String> {
    let t = s.strip_prefix("0x").unwrap_or(s);
    u32::from_str_radix(t, 16).map_err(|_| format!("'{s}' is not a hex orb key (see orb_list)"))
}

/// `orb_list`: the records found now and the book.
#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn list_cmd() -> Result<String, String> {
    let mut st = lock();
    if st.base == 0 {
        return Err("the main loop has not started".into());
    }
    let found = scan(st.base);
    for line in st.book.observe(&found) {
        tracing::info!("{line}");
    }
    let mut lines = vec![format!(
        "orb_list: {} record(s) in MapItemMan, my team {}, match {}",
        found.len(),
        st.my.map_or("?".into(), |t| (t - 1).to_string()),
        if st.in_match { "yes" } else { "no" }
    )];
    lines.extend(st.book.describe());
    Ok(lines.join("\n"))
}

/// `orb_off <key>` / `orb_on <key>`: switch by hand (the automatic rules then leave that orb alone).
#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn switch_cmd(args: &[String], on: bool) -> Result<String, String> {
    let key = parse_key(args.first().ok_or("an orb key is needed")?)?;
    let mut st = lock();
    let res = if on {
        switch_on(&mut st, key)
    } else {
        switch_off(&mut st, key, true)
    };
    res?;
    tracing::info!("orb: {key:#010X} switched {} by the console", if on { "on" } else { "off" });
    Ok(format!("orb {key:#010X} {}", if on { "on" } else { "OFF" }))
}

/// `orb_claim <key>`: LOCAL's team claims the orb as if the screen had been opened (publishes the claim).
#[cfg_attr(not(feature = "dev"), allow(dead_code))]
pub(crate) fn claim_cmd(args: &[String]) -> Result<String, String> {
    let key = parse_key(args.first().ok_or("an orb key is needed")?)?;
    let mut st = lock();
    let my = st.my.ok_or("my team is not known yet")?;
    if !st.book.orbs.contains_key(&key) {
        return Err("unknown orb (orb_list)".into());
    }
    if let Some(t) = st.book.owner(key) {
        return Err(format!("already claimed by team {}", t - 1));
    }
    claim_local(&mut st, key, my);
    Ok(format!("orb {key:#010X} claimed by team {}", my - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(handle: u64, ptr: usize) -> (Rec, Option<[f32; 3]>) {
        (
            Rec {
                slot: 0x58,
                ptr,
                id: 0x4000_1234,
                lot: 6_110_000,
                handle,
            },
            Some([10.0, 0.0, 10.0]),
        )
    }
    const H: u64 = 0x2E5B_0739_1100_0259;
    const K: u32 = 0x5B07_3911 ^ 0x2E;

    #[test]
    fn records_are_told_by_content() {
        assert!(is_boss_orb(0x4800_0000_0000_0000, 0x4012_3456, 6_110_000));
        assert!(!is_boss_orb(0x6800_0000_0000_0000, 0x4012_3456, 6_110_000), "a map item");
        assert!(!is_boss_orb(0x4800_0000_0000_0000, 0x0012_3456, 6_110_000));
        assert!(!is_boss_orb(0x4800_0000_0000_0000, 0x4012_3456, 5_000_000));
    }

    #[test]
    fn key_ignores_the_low_three_bytes_of_the_handle() {
        assert_eq!(key_of(H), key_of(0x2E5B_0739_1100_0001));
        assert_ne!(key_of(H), key_of(0x2E5B_0739_1200_0259));
        assert_eq!(key_of(H), K);
    }

    #[test]
    fn a_rival_claim_turns_the_orb_off_and_a_teammate_claim_keeps_it() {
        let mut b = Book::default();
        b.observe(&[rec(H, 0x1000)]);
        assert!(b.plan(Some(1)).is_empty(), "unowned: nothing happens");
        b.claim_remote(K, 2);
        assert_eq!(b.plan(Some(1)), [Step::Off(K)]);
        assert!(b.plan(None).is_empty(), "unknown own team: fail open");
        b.set_off(K, true, false);
        assert!(b.plan(Some(1)).is_empty(), "already off");
        // The same claim seen from the owner's team.
        let mut c = Book::default();
        c.observe(&[rec(H, 0x1000)]);
        c.claim_remote(K, 1);
        assert!(c.plan(Some(1)).is_empty(), "a teammate's claim keeps the orb");
    }

    #[test]
    fn a_claim_that_arrives_before_the_record_is_applied_when_it_appears() {
        let mut b = Book::default();
        b.claim_remote(K, 2);
        assert!(b.plan(Some(1)).is_empty());
        b.observe(&[rec(H, 0x1000)]);
        assert_eq!(b.plan(Some(1)), [Step::Off(K)]);
    }

    #[test]
    fn a_superseded_claim_restores_the_orb() {
        let mut b = Book::default();
        b.observe(&[rec(H, 0x1000)]);
        b.claim_remote(K, 2);
        b.set_off(K, true, false);
        // The lower Steam id's claim (ours) arrives later and wins.
        assert_eq!(b.claim_remote(K, 1), Some(2));
        assert_eq!(b.plan(Some(1)), [Step::On(K)]);
        b.set_off(K, false, false);
        assert!(b.plan(Some(1)).is_empty());
    }

    #[test]
    fn a_local_claim_needs_an_unowned_orb_within_reach_and_is_made_once() {
        let mut b = Book::default();
        b.observe(&[rec(H, 0x1000)]);
        assert_eq!(b.claimable([10.0, 0.0, 12.5]), Some(K));
        assert_eq!(b.claimable([10.0, 0.0, 14.0]), None, "too far");
        assert!(b.claim_local(K, 1));
        assert!(!b.claim_local(K, 1), "once");
        assert_eq!(b.claimable([10.0, 0.0, 10.0]), None, "owned");
        let mut c = Book::default();
        c.observe(&[rec(H, 0x1000)]);
        c.claim_remote(K, 2);
        assert!(!c.claim_local(K, 1), "a rival got there first");
        assert_eq!(c.owner(K), Some(2));
    }

    #[test]
    fn appear_replace_and_disappear_are_reported() {
        let mut b = Book::default();
        assert_eq!(b.observe(&[rec(H, 0x1000)]).len(), 1);
        assert!(b.observe(&[rec(H, 0x1000)]).is_empty(), "unchanged");
        assert_eq!(b.observe(&[rec(H, 0x2000)]).len(), 1, "a new record under the same key");
        assert_eq!(b.observe(&[]).len(), 1, "gone");
        assert!(b.describe().is_empty());
        // An orb we switched off is not in the scan and stays.
        b.observe(&[rec(H, 0x1000)]);
        b.set_off(K, true, false);
        assert!(b.observe(&[]).is_empty());
        assert_eq!(b.describe().len(), 1);
    }

    #[test]
    fn manual_and_blocked_orbs_are_left_alone() {
        let mut b = Book::default();
        b.observe(&[rec(H, 0x1000)]);
        b.claim_remote(K, 1);
        b.set_off(K, true, true);
        assert!(b.plan(Some(1)).is_empty(), "manual: no automatic restore");
        b.set_off(K, false, false);
        b.claim_remote(K, 2);
        b.block(K);
        assert!(b.plan(Some(1)).is_empty());
    }
}
