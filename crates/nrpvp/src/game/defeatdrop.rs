//! RW-3 (both builds): a player defeated by a rival drops their gear where they fell; the host makes the drop, the game replicates it
//! to everyone and anyone (the killer included) picks it up. Game 1.03.3 / exe 1.3.3.0; the findings are in MEMORY_MAP §24.
//!
//! 1. The victim's game ([`drop_local`], asked by the match rules when LOCAL is judged defeated by a rival) reads what it carries
//!    ([`read_gear`]: the equipped weapons so far, see below) and the map id and block-local position the game's own upload filler
//!    uses ([`itemdrop::local_place`]).
//! 2. If the victim is the host the drop is queued at once. Otherwise the victim publishes the list on [`topic::ITEM_DROP`] (one fact
//!    per field, key = `drop id << 8 | field`) and the host assembles it ([`Book`]) and queues the drop ([`tick`]).
//! 3. [`itemdrop`] makes the call on the game's thread (`exe+0x576110`).
//!
//! What is read ([`read_gear`]): the six equipped weapon ids (straight from `PlayerGameData`), and from the inventory list every
//! talisman (accessory) and every picked-up consumable (goods) with its count, but not the starting flask (rows 1000..=1025) and
//! not the game's own system goods (rows from 10000 on: relics, murk, keys). Not read yet: armour, runes. The victim's inventory is
//! not emptied (it is out of the match). A weapon's upgrade level is part of its id, so the equipped id already carries it and
//! the dropped weapon keeps its level (verified live: a +3 axe drops as +3, MEMORY_MAP §24). Who is the host is read from the game's session object (`[exe+0x3C21EC8]+0xC == 3`).

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::itemdrop::{self, Job};
use super::{layout, mem};
use crate::net::bus::{self, topic};

/// The session object the drop code asks ("3 = host, 6 = client", static note) and the field.
const RVA_SESSION: usize = 0x3C2_1EC8;
const SESSION_STATE: usize = 0xC;
const STATE_HOST: u32 = 3;
/// Equipment ids inside `PlayerGameData`: `+0x1D8` block, then the six weapon ids (left 1, right 1, left 2, right 2, left 3, right 3)
/// `[VERIFIED: peek on two games, MEMORY_MAP §24]`.
pub(crate) const GEAR_BLOCK: usize = 0x1D8;
pub(crate) const WEAPON_IDS: usize = 0x134;
pub(crate) const WEAPON_SLOTS: usize = 6;
/// Empty slot placeholders: -1, 0 and the unarmed weapon (110000).
const UNARMED: u32 = 110_000;
/// The inventory list (`[VERIFIED live, MEMORY_MAP §24]`): a header at `PlayerGameData+0x3B8` (`u32` used entries, `u32` capacity,
/// the array pointer at `+0x18`); entries are 0x14 bytes `[handle, item id, quantity, sort id, flags]`, the first is the flask.
pub(crate) const INV_HEADER: usize = 0x3B8;
pub(crate) const INV_ARRAY: usize = 0x18;
pub(crate) const INV_ENTRY: usize = 0x14;
pub(crate) const INV_CAP: usize = 0x180;
/// Per kind: the game's own record holds up to 6 weapons, 5 talismans and 10 consumables.
const MAX_TALISMANS: usize = 5;
const MAX_CONSUMABLES: usize = 10;
/// The starting flask (Flask of Crimson Tears rows) is never dropped; goods rows from here on are the game's system items.
const FLASK_ROWS: std::ops::RangeInclusive<u32> = 1000..=1025;
const SYSTEM_GOODS_FROM: u32 = 10_000;
/// At most this many items per drop (the game's own record holds 6 + 5 + 10).
const MAX_ITEMS: usize = 21;

const F_N: u32 = 0;
const F_MAP: u32 = 1;
const F_X: u32 = 2;
const F_Y: u32 = 3;
const F_Z: u32 = 4;
const F_ID: u32 = 0x10;
const F_COUNT: u32 = 0x80;

/// Key of one field of the drop `id`.
pub(crate) fn key_of(id: u32, field: u32) -> u32 {
    (id << 8) | (field & 0xFF)
}

/// A 24-bit drop id from the victim's session player id (`ChrIns+0x78`, equal on every game).
pub(crate) fn id_of(player_id: u64) -> u32 {
    let f = (player_id ^ (player_id >> 32)) as u32;
    (f ^ (f >> 15)) & 0x00FF_FFFF
}

/// The weapon ids worth dropping out of the six slots (pure).
pub(crate) fn weapons_of(slots: &[u32]) -> Vec<(u32, u16)> {
    slots
        .iter()
        .copied()
        .filter(|&id| id != u32::MAX && id != 0 && id != UNARMED)
        .map(|id| (id, 1))
        .collect()
}

/// The talismans and consumables worth dropping out of `(item id, quantity)` inventory entries (pure, tested).
pub(crate) fn inventory_items(entries: &[(u32, u32)]) -> Vec<(u32, u16)> {
    let mut talismans = Vec::new();
    let mut goods = Vec::new();
    for &(id, qty) in entries {
        if id == u32::MAX || qty == 0 {
            continue;
        }
        let row = id & 0x0FFF_FFFF;
        match id >> 28 {
            2 if talismans.len() < MAX_TALISMANS => talismans.push((id, 1)),
            4 if goods.len() < MAX_CONSUMABLES
                && row < SYSTEM_GOODS_FROM
                && !FLASK_ROWS.contains(&row) =>
            {
                goods.push((id, u16::try_from(qty).unwrap_or(u16::MAX)));
            }
            _ => {}
        }
    }
    talismans.extend(goods);
    talismans
}

/// One drop being assembled from the facts of the victim.
#[derive(Clone, Debug, Default, PartialEq)]
struct Pending {
    n: Option<u32>,
    map: Option<u32>,
    pos: [Option<u32>; 3],
    ids: BTreeMap<u32, u32>,
    counts: BTreeMap<u32, u32>,
    done: bool,
}

impl Pending {
    fn complete(&self) -> Option<Job> {
        let n = self.n? as usize;
        if n == 0 || n > MAX_ITEMS {
            return None;
        }
        let items = (0..n as u32)
            .map(|i| {
                Some((
                    *self.ids.get(&i)?,
                    u16::try_from(*self.counts.get(&i)?).ok()?,
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Job {
            items,
            map: self.map?,
            pos: [
                f32::from_bits(self.pos[0]?),
                f32::from_bits(self.pos[1]?),
                f32::from_bits(self.pos[2]?),
            ],
            tag: String::new(),
        })
    }
}

/// The drops the host is assembling (pure, tested).
#[derive(Debug, Default)]
pub(crate) struct Book {
    drops: BTreeMap<u32, Pending>,
}

impl Book {
    pub(crate) fn clear(&mut self) {
        self.drops.clear();
    }

    /// One field of a drop arrived. `Some(drop id, job)` exactly once, when it completed the drop.
    pub(crate) fn on_fact(&mut self, key: u32, value: u32) -> Option<(u32, Job)> {
        let (id, field) = (key >> 8, key & 0xFF);
        let d = self.drops.entry(id).or_default();
        match field {
            F_N => d.n = Some(value),
            F_MAP => d.map = Some(value),
            F_X | F_Y | F_Z => d.pos[(field - F_X) as usize] = Some(value),
            f if (F_ID..F_ID + MAX_ITEMS as u32).contains(&f) => {
                d.ids.insert(f - F_ID, value);
            }
            f if (F_COUNT..F_COUNT + MAX_ITEMS as u32).contains(&f) => {
                d.counts.insert(f - F_COUNT, value);
            }
            _ => {}
        }
        if d.done {
            return None;
        }
        let job = d.complete()?;
        d.done = true;
        Some((id, job))
    }
}

/// The facts of one drop, in the order they are published (pure, tested).
pub(crate) fn facts_of(id: u32, job: &Job) -> Vec<(u32, u32)> {
    let mut f = vec![
        (key_of(id, F_N), job.items.len() as u32),
        (key_of(id, F_MAP), job.map),
        (key_of(id, F_X), job.pos[0].to_bits()),
        (key_of(id, F_Y), job.pos[1].to_bits()),
        (key_of(id, F_Z), job.pos[2].to_bits()),
    ];
    for (i, (item, n)) in job.items.iter().enumerate() {
        f.push((key_of(id, F_ID + i as u32), *item));
        f.push((key_of(id, F_COUNT + i as u32), u32::from(*n)));
    }
    f
}

struct State {
    book: Book,
    base: usize,
    in_match: bool,
    /// Counters for the log and the console.
    dropped: u32,
    published: u32,
}

static STATE: Mutex<State> = Mutex::new(State {
    book: Book {
        drops: BTreeMap::new(),
    },
    base: 0,
    in_match: false,
    dropped: 0,
    published: 0,
});

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// This game is the session host (`[exe+0x3C21EC8]+0xC == 3`).
pub(crate) fn is_host(base: usize) -> bool {
    mem::read::<usize>(base + RVA_SESSION)
        .filter(|s| mem::is_ptr(*s))
        .and_then(|s| mem::read::<u32>(s + SESSION_STATE))
        == Some(STATE_HOST)
}

/// What LOCAL carries that the drop covers now: `(item id, count)`.
pub(crate) fn read_gear(local: usize) -> Vec<(u32, u16)> {
    let Some(pgd) = mem::read::<usize>(local + layout::PLAYER_GAMEDATA).filter(|d| mem::is_ptr(*d))
    else {
        return Vec::new();
    };
    let slots: Vec<u32> = (0..WEAPON_SLOTS)
        .map(|i| mem::read::<u32>(pgd + GEAR_BLOCK + WEAPON_IDS + 4 * i).unwrap_or(u32::MAX))
        .collect();
    let mut items = weapons_of(&slots);
    items.extend(inventory_items(&read_inventory(pgd)));
    items
}

/// `(item id, quantity)` of the used inventory entries of a `PlayerGameData`.
fn read_inventory(pgd: usize) -> Vec<(u32, u32)> {
    let header = pgd + INV_HEADER;
    let used = mem::read::<u32>(header).unwrap_or(0) as usize;
    let cap = mem::read::<u32>(header + 4).unwrap_or(0) as usize;
    let Some(array) = mem::read::<usize>(header + INV_ARRAY).filter(|a| mem::is_ptr(*a)) else {
        return Vec::new();
    };
    // A sane header only: the count never exceeds the capacity, the capacity is the game's.
    if cap != INV_CAP || used > cap {
        return Vec::new();
    }
    (0..used)
        .filter_map(|i| {
            let e = array + i * INV_ENTRY;
            Some((mem::read::<u32>(e + 4)?, mem::read::<u32>(e + 8)?))
        })
        .collect()
}

/// LOCAL was judged defeated by a rival (or the test aid asked): drop its gear where it stands. Returns a log line.
pub(crate) fn drop_local(base: usize, local: usize) -> String {
    let gear = read_gear(local);
    if gear.is_empty() {
        return "defeatdrop: LOCAL carries nothing the drop covers (no weapon ids read)".into();
    }
    let (map, pos) = match itemdrop::local_place(base) {
        Ok(p) => p,
        Err(e) => return format!("defeatdrop: LOCAL's place is unreadable ({e}), nothing dropped"),
    };
    let job = Job {
        items: gear,
        map,
        pos,
        tag: "defeated LOCAL".into(),
    };
    let n = job.items.len();
    if is_host(base) {
        itemdrop::queue(base, job);
        lock().dropped += 1;
        return format!(
            "defeatdrop: this game is the host: {n} item(s) queued for the drop at LOCAL's spot"
        );
    }
    let Some(pid) = mem::read::<u64>(local + layout::CHR_PLAYER_ID) else {
        return "defeatdrop: LOCAL's player id is unreadable, nothing dropped".into();
    };
    let id = id_of(pid);
    let sent = facts_of(id, &job)
        .iter()
        .all(|&(k, v)| bus::publish(topic::ITEM_DROP, k, v));
    if !sent {
        return format!("defeatdrop: publishing drop {id:#08X} FAILED (queue full)");
    }
    lock().published += 1;
    format!("defeatdrop: published drop {id:#08X} ({n} item(s)) for the host to make")
}

/// Main loop, every pass (both builds): the host turns arrived drops into jobs; the breakpoint of [`itemdrop`] is kept in step.
pub(crate) fn tick(base: usize, in_match: bool) {
    let mut st = lock();
    st.base = base;
    if st.in_match != in_match {
        st.in_match = in_match;
        if !in_match {
            // A new match starts from nothing (the data channel forgets its keys when the lobby is left).
            st.book.clear();
        }
    }
    let host = is_host(base);
    for r in bus::take(topic::ITEM_DROP) {
        if let Some((id, mut job)) = st.book.on_fact(r.fact.key, r.fact.value) {
            if host {
                job.tag = format!("drop {id:#08X} (from {:?})", r.from);
                st.dropped += 1;
                itemdrop::queue(base, job);
            } else {
                tracing::info!("defeatdrop: drop {id:#08X} arrived on a client: the host makes it");
            }
        }
    }
    drop(st);
    itemdrop::pump();
}

static LAST: Mutex<String> = Mutex::new(String::new());

/// The last console aid reply.
#[cfg(feature = "dev")]
pub(crate) fn reply() -> String {
    LAST.lock().map(|g| g.clone()).unwrap_or_default()
}

/// Console aid (dev, main loop): `dry` lists what the drop would cover; otherwise it drops now, without a defeat.
#[cfg(feature = "dev")]
pub(crate) fn aid(base: usize, local: Option<usize>, dry: bool) {
    let text = match local {
        None => "drop_gear: no local player".to_string(),
        Some(l) if dry => {
            let st = lock();
            let items: Vec<String> = read_gear(l)
                .iter()
                .map(|(id, n)| format!("{id:#X} x{n}"))
                .collect();
            format!(
                "drop_gear: {} item(s): [{}]; host {}; {} drop(s) made here, {} published",
                items.len(),
                items.join(", "),
                if is_host(base) { "yes" } else { "no" },
                st.dropped,
                st.published
            )
        }
        Some(l) => drop_local(base, l),
    };
    tracing::info!("{text}");
    if let Ok(mut g) = LAST.lock() {
        *g = text;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> Job {
        Job {
            items: vec![(0x01D5_3530, 1), (0x0039_3870, 1)],
            map: 0x3C2D_2420,
            pos: [-121.5, 31.25, 121.75],
            tag: String::new(),
        }
    }

    #[test]
    fn placeholders_are_not_dropped() {
        let got = weapons_of(&[0x01D5_3530, 0x0039_3870, UNARMED, UNARMED, u32::MAX, 0]);
        assert_eq!(got, vec![(0x01D5_3530, 1), (0x0039_3870, 1)]);
        assert!(weapons_of(&[UNARMED; 6]).is_empty());
    }

    #[test]
    fn the_inventory_gives_talismans_and_consumables_but_not_the_flask_or_system_items() {
        let entries = [
            (0x4000_03E9, 3), // the starting flask (row 1001)
            (0x0001_ADB0, 1), // not goods or accessory
            (0x4000_012C, 2), // Fire Pot
            (0x2000_0410, 1), // Erdtree's Favor
            (0x4000_9E98, 1), // a system item (row 40600)
            (0x4000_02BC, 1), // Wending Grace
            (0x4000_0384, 0), // none left
            (u32::MAX, 1),    // empty slot
            (0x8080_020A, 1), // gem / art
        ];
        assert_eq!(
            inventory_items(&entries),
            vec![(0x2000_0410, 1), (0x4000_012C, 2), (0x4000_02BC, 1)]
        );
    }

    #[test]
    fn the_per_kind_caps_hold() {
        let many: Vec<(u32, u32)> = (0..30u32)
            .flat_map(|i| [(0x2000_0100 + i, 1), (0x4000_0100 + i, 1)])
            .collect();
        let got = inventory_items(&many);
        assert_eq!(got.iter().filter(|i| i.0 >> 28 == 2).count(), MAX_TALISMANS);
        assert_eq!(
            got.iter().filter(|i| i.0 >> 28 == 4).count(),
            MAX_CONSUMABLES
        );
    }

    #[test]
    fn a_drop_round_trips_through_the_facts_in_any_order() {
        let mut facts = facts_of(0xABCDEF, &job());
        assert_eq!(facts.len(), 5 + 2 * 2);
        // The channel may deliver in any order: reverse them.
        facts.reverse();
        let mut book = Book::default();
        let mut made = Vec::new();
        for (k, v) in facts {
            made.extend(book.on_fact(k, v));
        }
        assert_eq!(made.len(), 1, "completes exactly once");
        let (id, got) = &made[0];
        assert_eq!(*id, 0xABCDEF);
        assert_eq!(got.items, job().items);
        assert_eq!(got.map, 0x3C2D_2420);
        assert_eq!(got.pos, [-121.5, 31.25, 121.75]);
        // A resent fact does not make a second drop.
        assert!(book.on_fact(key_of(0xABCDEF, F_N), 2).is_none());
    }

    #[test]
    fn an_incomplete_drop_makes_nothing_and_two_drops_stay_apart() {
        let mut book = Book::default();
        let a = facts_of(1, &job());
        let b = facts_of(2, &job());
        // Everything of drop 1 except its last fact, all of drop 2.
        for &(k, v) in &a[..a.len() - 1] {
            assert!(book.on_fact(k, v).is_none());
        }
        let mut done = Vec::new();
        for (k, v) in b {
            done.extend(book.on_fact(k, v));
        }
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].0, 2);
        let (k, v) = a[a.len() - 1];
        assert_eq!(book.on_fact(k, v).map(|d| d.0), Some(1));
    }

    #[test]
    fn nonsense_is_ignored() {
        let mut book = Book::default();
        // Too many items, an unknown field, a count that does not fit u16.
        assert!(book.on_fact(key_of(5, F_N), 99).is_none());
        assert!(book.on_fact(key_of(5, 0x70), 1).is_none());
        assert!(book.on_fact(key_of(5, F_COUNT), 70_000).is_none());
    }

    #[test]
    fn ids_fit_the_key() {
        assert!(id_of(0x6001_FFFF_FFFF) <= 0x00FF_FFFF);
        assert_ne!(id_of(0x6001_FFFF_FFFF), id_of(0x6002_FFFF_FFFF));
        assert_eq!(key_of(0x00FF_FFFF, 0xFF), 0xFFFF_FFFF);
    }
}
