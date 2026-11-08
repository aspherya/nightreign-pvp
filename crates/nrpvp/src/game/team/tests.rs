use super::*;
use crate::overlay::shared::{PvpveState, Relation, Role, Snapshot};

fn entry_bytes(ptrs: &[u64]) -> Vec<u8> {
    let mut b = Vec::new();
    for (i, p) in ptrs.iter().enumerate() {
        b.extend_from_slice(&p.to_le_bytes());
        b.extend_from_slice(&(0xAA00 + i as u64).to_le_bytes());
    }
    b
}

#[test]
fn capacity_bounds() {
    assert!(!capacity_ok(0));
    assert!(capacity_ok(1));
    assert!(capacity_ok(6));
    assert!(capacity_ok(16));
    assert!(!capacity_ok(17));
    assert!(!capacity_ok(0x1000));
}

#[test]
fn parses_chrset_entries_from_a_synthetic_buffer() {
    let buf = entry_bytes(&[0x7FF0_0000_1000, 0, 0x7FF0_0000_2000]);
    let e = parse_entries(&buf, 3);
    assert_eq!(e.len(), 3);
    assert_eq!((e[0].index, e[0].ptr), (0, 0x7FF0_0000_1000));
    assert_eq!(e[1].ptr, 0);
    assert_eq!(e[2].ptr, 0x7FF0_0000_2000);
    // The second qword is kept verbatim in `raw`.
    assert_eq!(&e[2].raw[8..], &0xAA02u64.to_le_bytes());
    assert_eq!(
        hex16(&e[0].raw),
        "00 10 00 00 F0 7F 00 00 00 AA 00 00 00 00 00 00"
    );
    // A buffer shorter than capacity yields only the complete entries; a partial entry is dropped.
    assert_eq!(parse_entries(&buf[..40], 6).len(), 2);
    assert!(parse_entries(&[], 6).is_empty());
    assert_eq!(parse_entries(&buf, 0).len(), 0);
    // Capacity smaller than the buffer ignores the rest.
    assert_eq!(parse_entries(&buf, 2).len(), 2);
}

#[test]
fn persist_counts_reverts_not_the_initial_application() {
    let mut t = PersistTracker::new();
    t.reset(0);
    let me = 0x1000;
    // First tick: the value is still the original (30). We rewrite it; that is not a revert.
    assert!(t.observe(me, 6, 30));
    assert_eq!(t.total_reverts(), 0);
    // Stays at 6: nothing to do.
    assert!(!t.observe(me, 6, 6));
    assert!(!t.observe(me, 6, 6));
    // The game flips it back: counted, rewritten.
    assert!(t.observe(me, 6, 30));
    assert_eq!(t.total_reverts(), 1);
    assert!(!t.observe(me, 6, 6));
    assert!(t.observe(me, 6, 26));
    assert_eq!(t.total_reverts(), 2);
    // Summary only after a full second and only if something was reverted.
    assert_eq!(t.summary(500), None);
    assert_eq!(
        t.summary(1000).as_deref(),
        Some("persist: 2 reverts in last 1 s, last seen value 26")
    );
    // Window restarts: quiet second -> no line.
    assert_eq!(t.summary(1500), None);
    assert_eq!(t.summary(2000), None);
    assert!(!t.observe(me, 6, 6));
    assert!(t.observe(me, 6, 0));
    assert_eq!(t.summary(2500), None); // < 1 s since the last window start (2000)
    assert_eq!(
        t.summary(3000).as_deref(),
        Some("persist: 1 reverts in last 1 s, last seen value 0")
    );
    assert_eq!(t.total_reverts(), 3);
}

#[test]
fn persist_rearms_on_new_target_or_selection_and_after_write_failure() {
    let mut t = PersistTracker::new();
    t.reset(0);
    assert!(!t.observe(0x1000, 6, 6)); // already matches: armed
                                       // Selection changes (F4): the mismatch is our own re-application, not a revert.
    assert!(t.observe(0x1000, 29, 6));
    assert_eq!(t.total_reverts(), 0);
    // New PlayerIns (area change): same.
    assert!(!t.observe(0x1000, 29, 29));
    assert!(t.observe(0x2000, 29, 30));
    assert_eq!(t.total_reverts(), 0);
    // A failed write must not make the next tick look like a game revert.
    assert!(!t.observe(0x2000, 29, 29));
    assert!(t.observe(0x2000, 29, 1)); // counted: 1
    t.write_failed();
    assert!(t.observe(0x2000, 29, 1)); // not counted again
    assert_eq!(t.total_reverts(), 1);
}

#[test]
fn persist_failure_log_is_rate_limited() {
    let mut t = PersistTracker::new();
    assert!(t.should_log_failure(10));
    assert!(!t.should_log_failure(500));
    assert!(t.should_log_failure(1010));
}

#[test]
fn watcher_reports_only_changes() {
    let mut w = WatchState::default();
    let a = Fields {
        id: 1,
        v70: 5,
        team: 30,
    };
    let b = Fields {
        id: 1,
        v70: 5,
        team: 6,
    };
    assert_eq!(w.update(0x10, a), Some(None)); // new
    assert_eq!(w.update(0x10, a), None);
    assert_eq!(w.update(0x10, b), Some(Some(a)));
    assert_eq!(w.update(0x20, a), Some(None)); // other address is independent
    w.clear();
    assert_eq!(w.update(0x10, b), Some(None));
    let line = watch_line(0x10, "REMOTE[2]", b, Some(a));
    assert!(
        line.contains("who=REMOTE[2]") && line.contains("team_type=6") && line.contains("was id=1")
    );
    assert!(!watch_line(0x10, "LOCAL", b, None).contains("was"));
}

// ---- T-020: the entry[0] local-player rule on synthetic multi-player data ----

/// A probe at the given ChrSet entry index with arbitrary evidence values.
fn probe(addr: usize, index: usize, vals: &[u64]) -> PlayerProbe {
    const NAMES: [&str; 3] = ["sigA", "sigB", "sigC"];
    PlayerProbe {
        addr,
        index,
        signals: vals
            .iter()
            .enumerate()
            .map(|(i, &v)| Signal {
                name: NAMES[i],
                value: Some(v),
                hex: false,
            })
            .collect(),
    }
}

#[test]
fn local_is_the_first_valid_playerins_whatever_the_evidence_says() {
    let d = decide_local(&[]);
    assert_eq!((d.local, &d.basis), (None, &Basis::NoPlayers));

    // One player: local.
    let d = decide_local(&[probe(0x100, 3, &[1, 2])]);
    assert_eq!((d.local, &d.basis), (Some(0x100), &Basis::FirstEntry));

    // Two, three and four players: always the first in entry order; the signals are irrelevant (here the
    // third player is the "odd one out" on sigA, which the old heuristic would have picked).
    let ps = [
        probe(0xA, 0, &[5, 7]),
        probe(0xB, 1, &[5, 7]),
        probe(0xC, 2, &[9, 7]),
        probe(0xD, 3, &[5, 7]),
    ];
    for n in 2..=4 {
        let d = decide_local(&ps[..n]);
        assert_eq!((d.local, &d.basis), (Some(0xA), &Basis::FirstEntry), "{n}");
    }
    // Index gaps do not matter: the first valid player is local even if it is not at entry 0.
    let d = decide_local(&[probe(0xB, 2, &[1]), probe(0xA, 4, &[1])]);
    assert_eq!(d.local, Some(0xB));
    assert_eq!(
        Basis::FirstEntry.label(),
        "first PlayerIns in the ChrSet (entry[0] rule)"
    );
}

#[test]
fn who_labels() {
    assert_eq!(who_label(Role::Local, 2), "LOCAL");
    assert_eq!(who_label(Role::Remote, 3), "REMOTE[3]");
}

// ---- resolve() and the gated write on fake memory ----

const VT_PLAYER: usize = 0x7FF6_0000_0000;
const VT_ENEMY: usize = 0x7FF6_0000_1000;

/// A fake object: 0x800 bytes (so every candidate offset is inside it), first qword = vtable, byte
/// 0x74 = team.
fn fake_obj(vt: usize, team: u8) -> Vec<u8> {
    let mut v = vec![0u8; 0x800];
    v[..8].copy_from_slice(&(vt as u64).to_le_bytes());
    v[CHR_TEAM] = team;
    v[CHR_ID..CHR_ID + 4].copy_from_slice(&1234u32.to_le_bytes());
    v
}

fn vts() -> PlayerVtables {
    PlayerVtables::from_set([VT_PLAYER].into_iter().collect())
}

/// Entries buffer with explicit state bytes (+8, +9), padded so reads stay inside the allocation.
fn entries_with_state(ptrs: &[u64], state: &[(u8, u8)]) -> Vec<u8> {
    let mut b = Vec::new();
    for (i, p) in ptrs.iter().enumerate() {
        let (s8, s9) = state.get(i).copied().unwrap_or((4, 4));
        b.extend_from_slice(&p.to_le_bytes());
        b.extend_from_slice(&[s8, s9, 0, 0, 0, 0, 0, 0]);
    }
    b.resize(0x100, 0);
    b
}

/// A fake WorldChrMan big enough for +0x9A40 with a ChrSet of the given capacity. The entries buffer
/// is returned too and must stay alive.
fn fake_wcm(capacity: u32, entries: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut w = vec![0u8; 0x1D000];
    w[WCM_CHRSET + CHRSET_CAPACITY..WCM_CHRSET + CHRSET_CAPACITY + 4]
        .copy_from_slice(&capacity.to_le_bytes());
    let e = entries.to_vec();
    w[WCM_CHRSET + CHRSET_ENTRIES..WCM_CHRSET + CHRSET_ENTRIES + 8]
        .copy_from_slice(&(e.as_ptr() as u64).to_le_bytes());
    (w, e)
}

#[test]
fn resolve_finds_the_players_among_entries() {
    let player = fake_obj(VT_PLAYER, 30);
    let enemy = fake_obj(VT_ENEMY, 6);
    let pa = player.as_ptr() as u64;
    let entries = entries_with_state(&[0, pa, enemy.as_ptr() as u64, 0, 0, 0], &[]);
    let (wcm, _e) = fake_wcm(6, &entries);
    let r = resolve(wcm.as_ptr() as usize, &vts()).expect("resolves");
    assert_eq!(r.capacity, 6);
    assert_eq!(r.entries.len(), 6);
    let kinds: Vec<EntryKind> = r.entries.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![
            EntryKind::Empty,
            EntryKind::Player,
            EntryKind::Other,
            EntryKind::Empty,
            EntryKind::Empty,
            EntryKind::Empty
        ]
    );
    assert_eq!(r.players, vec![pa as usize]);
    // The only PlayerIns is LOCAL; its ChrSet entry index is 1.
    assert_eq!(r.local, Some(pa as usize));
    assert_eq!(r.basis, Basis::FirstEntry);
    assert_eq!(r.who(pa as usize).as_deref(), Some("LOCAL"));
    assert_eq!(r.who(enemy.as_ptr() as usize), None);

    // The log prints every raw entry and the evidence for the player the first time, nothing when
    // unchanged.
    let mut lg = ResolveLog::default();
    let lines = lg.lines(&r, 0, (0, 0));
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.starts_with("team:   entry["))
            .count(),
        6
    );
    let ev: Vec<&String> = lines.iter().filter(|l| l.contains("evidence")).collect();
    assert_eq!(ev.len(), 1);
    assert!(ev[0].contains("evidence only") && ev[0].contains("net_authority@+0x1EC"));
    assert!(
        ev[0].contains("04 04"),
        "the raw entry bytes are in the evidence"
    );
    assert!(lines.iter().any(|l| l.contains("local player = ")));
    assert!(
        !lines.iter().any(|l| l.contains("1C820")),
        "the old +0x1C820 rule and its log lines are gone"
    );
    assert!(lg.lines(&r, 10, (0, 0)).is_empty());
}

#[test]
fn resolve_with_several_players_uses_entry_order_and_labels_rows() {
    // Three PlayerIns in entries 1..=3 (entry 0 empty); the third has a different chr_type (+0x70), which
    // the old heuristic would have picked. The rule is the first valid PlayerIns, whatever the evidence says.
    let mut a = fake_obj(VT_PLAYER, 2);
    let mut b = fake_obj(VT_PLAYER, 2);
    let mut c = fake_obj(VT_PLAYER, 2);
    for o in [&mut a, &mut b] {
        o[CHR_TYPE70..CHR_TYPE70 + 4].copy_from_slice(&5u32.to_le_bytes());
    }
    c[CHR_TYPE70..CHR_TYPE70 + 4].copy_from_slice(&13u32.to_le_bytes());
    let (pa, pb, pc) = (
        a.as_ptr() as usize,
        b.as_ptr() as usize,
        c.as_ptr() as usize,
    );
    let entries = entries_with_state(&[0, pa as u64, pb as u64, pc as u64], &[]);
    let (wcm, _e) = fake_wcm(4, &entries);
    let w = wcm.as_ptr() as usize;

    let r = resolve(w, &vts()).expect("resolves");
    assert_eq!(r.players, vec![pa, pb, pc]);
    assert_eq!(r.local, Some(pa), "the first valid PlayerIns is local");
    assert_eq!(r.basis, Basis::FirstEntry);
    assert_eq!(r.who(pa).as_deref(), Some("LOCAL"));
    assert_eq!(r.who(pb).as_deref(), Some("REMOTE[2]"));
    assert_eq!(r.who(pc).as_deref(), Some("REMOTE[3]"));
    assert_eq!(r.role_of(pb), Role::Remote);
    let rows = r.rows(&mut |addr| Some(unsafe { *((addr + CHR_TEAM) as *const u8) }));
    assert_eq!(
        rows.iter().map(|x| (x.index, x.role)).collect::<Vec<_>>(),
        vec![(1, Role::Local), (2, Role::Remote), (3, Role::Remote)]
    );
    assert_eq!(
        (rows[0].state8, rows[0].state9, rows[0].team),
        (4, 4, Some(2))
    );
    assert_eq!(rows[0].basis, Basis::FirstEntry.label());
    assert_eq!(rows[1].basis, "not the local player");

    // With the local player in entry[0] (as in the real game) there is no warning; with an empty entry[0]
    // (above) there is one.
    let entries0 = entries_with_state(&[pa as u64, pb as u64, pc as u64], &[]);
    let (wcm0, _e0) = fake_wcm(3, &entries0);
    let r0 = resolve(wcm0.as_ptr() as usize, &vts()).expect("resolves");
    assert_eq!(r0.local, Some(pa));
    assert_eq!(r0.who(pa).as_deref(), Some("LOCAL"));
    assert_eq!(r0.who(pc).as_deref(), Some("REMOTE[2]"));
    let mut lg = ResolveLog::default();
    assert!(!lg
        .lines(&r0, 0, (0, 0))
        .iter()
        .any(|l| l.contains("WARNING")));
    let mut lg = ResolveLog::default();
    assert!(lg
        .lines(&r, 0, (0, 0))
        .iter()
        .any(|l| l.contains("WARNING") && l.contains("entry[1]")));

    // Permuting the entries moves the LOCAL role with them.
    let swapped = entries_with_state(&[pb as u64, pa as u64], &[]);
    let (wcm2, _e2) = fake_wcm(2, &swapped);
    let r2 = resolve(wcm2.as_ptr() as usize, &vts()).expect("resolves");
    assert_eq!(r2.local, Some(pb));
    assert_eq!(r2.who(pa).as_deref(), Some("REMOTE[1]"));
}

#[test]
fn resolve_refuses_bad_capacity_and_pointer() {
    let (wcm, _e) = fake_wcm(0, &[0u8; 0x100]);
    assert_eq!(
        resolve(wcm.as_ptr() as usize, &vts()).err(),
        Some(ResolveError::BadCapacity(0))
    );
    let (wcm, _e) = fake_wcm(17, &[0u8; 0x100]);
    assert_eq!(
        resolve(wcm.as_ptr() as usize, &vts()).err(),
        Some(ResolveError::BadCapacity(17))
    );
    let mut wcm = vec![0u8; 0x1D000];
    wcm[WCM_CHRSET + CHRSET_CAPACITY] = 6; // entries pointer left null
    assert_eq!(
        resolve(wcm.as_ptr() as usize, &vts()).err(),
        Some(ResolveError::BadEntriesPtr(0))
    );
    assert_eq!(
        resolve(0, &vts()).err(),
        Some(ResolveError::HeaderUnreadable)
    );
}

#[test]
fn resolve_log_reports_changes_and_coalesces_bursts() {
    let player = fake_obj(VT_PLAYER, 30);
    let pa = player.as_ptr() as u64;
    let entries = entries_with_state(&[pa, 0], &[]);
    let (wcm, _e) = fake_wcm(2, &entries);
    let mut lg = ResolveLog::default();
    let r1 = resolve(wcm.as_ptr() as usize, &vts()).expect("ok");
    assert!(!lg.lines(&r1, 0, (0, 0)).is_empty());
    assert!(lg.lines(&r1, 1000, (0, 0)).is_empty());
    // The entry state byte +9 blinks 04 -> 02 (seen in TC-004): within the gap it is coalesced...
    let mut r2 = resolve(wcm.as_ptr() as usize, &vts()).expect("ok");
    r2.entries[0].raw.raw[9] = 0x02;
    assert!(lg.lines(&r2, 2000, (0, 0)).is_empty());
    // ...and reported (with the count) once the gap has passed.
    let lines = lg.lines(&r2, 6000, (0, 0));
    assert!(lines[0].contains("CHANGED") && lines[0].contains("1 earlier change(s) coalesced"));
    assert!(lg.lines(&r2, 7000, (0, 0)).is_empty());
}

#[test]
fn write_is_gated_on_the_playerins_vtable() {
    let mut player = fake_obj(VT_PLAYER, 30);
    let a = player.as_mut_ptr() as usize;
    let w = write_team_type(a, 6, &vts()).expect("writes");
    assert_eq!((w.old, w.readback), (30, 6));
    assert_eq!(player[CHR_TEAM], 6);
    // Only that one byte changed.
    let mut expect = fake_obj(VT_PLAYER, 6);
    assert_eq!(player, expect);
    expect[CHR_TEAM] = 30;

    // Wrong class (EnemyIns): refused, memory untouched.
    let mut enemy = fake_obj(VT_ENEMY, 6);
    let e = write_team_type(enemy.as_mut_ptr() as usize, 29, &vts());
    assert!(e.is_err());
    assert_eq!(enemy[CHR_TEAM], 6);
    // Empty vtable set (RTTI not recovered): everything refused.
    let none = PlayerVtables::default();
    assert!(write_team_type(a, 1, &none).is_err());
    assert_eq!(player[CHR_TEAM], 6);
    // Null / garbage / unmapped pointers.
    assert!(write_team_type(0, 1, &vts()).is_err());
    assert!(write_team_type(0x10, 1, &vts()).is_err());
    assert!(write_team_type(0x7FFF_FFFF_0000, 1, &vts()).is_err());
}

// ---- T-013: selection by value, revert window, and the overlay snapshot ----

#[test]
fn select_by_value_only_accepts_candidates() {
    let mut c = Cycler::new(&[6, 29, 3]);
    assert!(c.select(3));
    assert_eq!((c.current(), c.position()), (3, (3, 3)));
    assert!(!c.select(99));
    assert_eq!(c.current(), 3, "a refused select changes nothing");
    assert!(c.select(6));
    assert_eq!(c.position(), (1, 3));
    assert_eq!(c.list(), &[6, 29, 3]);
}

#[test]
fn reverts_last_window_reports_the_last_completed_second() {
    let mut t = PersistTracker::new();
    t.reset(0);
    assert_eq!(t.reverts_last_window(), 0);
    assert!(!t.observe(0x1000, 6, 6)); // armed
    assert!(t.observe(0x1000, 6, 30)); // revert 1
    assert!(!t.observe(0x1000, 6, 6));
    assert!(t.observe(0x1000, 6, 30)); // revert 2
                                       // Nothing is reported until a full second has passed.
    assert_eq!(t.reverts_last_window(), 0);
    assert!(t.summary(1000).is_some());
    assert_eq!(t.reverts_last_window(), 2);
    // A quiet second resets it to 0.
    assert!(t.summary(2000).is_none());
    assert_eq!(t.reverts_last_window(), 0);
    assert_eq!(t.total_reverts(), 2);
}

// ---- T-015: per-player writes, per-player persist, MARK, "this is me" on fake memory ----

/// Three fake PlayerIns (team 2) in entries 1..=3 of a 4-slot ChrSet, in the order objs[2], objs[0], objs[1].
/// The first valid entry (objs[2]) is LOCAL.
struct Fake {
    objs: Vec<Vec<u8>>,
    wcm: Vec<u8>,
    _entries: Vec<u8>,
}

impl Fake {
    fn new() -> Fake {
        let mut objs: Vec<Vec<u8>> = (0..3).map(|_| fake_obj(VT_PLAYER, 2)).collect();
        for (i, o) in objs.iter_mut().enumerate() {
            let t: u32 = if i == 2 { 13 } else { 5 };
            o[CHR_TYPE70..CHR_TYPE70 + 4].copy_from_slice(&t.to_le_bytes());
        }
        // Entry order: empty, objs[2], objs[0], objs[1]. The first valid entry (objs[2]) is LOCAL.
        let ptrs: Vec<u64> = [None, Some(2), Some(0), Some(1)]
            .iter()
            .map(|i| i.map_or(0, |i: usize| objs[i].as_ptr() as u64))
            .collect();
        let entries = entries_with_state(&ptrs, &[]);
        let (wcm, e) = fake_wcm(4, &entries);
        Fake {
            objs,
            wcm,
            _entries: e,
        }
    }

    fn addr(&self, i: usize) -> usize {
        self.objs[i].as_ptr() as usize
    }

    fn team(&self, i: usize) -> u8 {
        self.objs[i][CHR_TEAM]
    }

    fn ctl(&self) -> TeamCtl {
        self.ctl_with(&Config::default())
    }

    fn ctl_with(&self, cfg: &Config) -> TeamCtl {
        let mut ctl = TeamCtl::new(cfg, 0, 0);
        ctl.set_instance(self.wcm.as_ptr() as usize);
        ctl.vts = vts();
        ctl
    }
}

#[test]
fn writes_go_to_any_valid_playerins_and_are_gated() {
    let f = Fake::new();
    let mut ctl = f.ctl();
    let (a, b, c) = (f.addr(0), f.addr(1), f.addr(2));

    // A remote row button writes only that player's byte; the selected candidate is untouched.
    ctl.set_for(a, 29, "overlay");
    assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 2, 2));
    assert_eq!(ctl.cycler.current(), 6);
    assert_eq!(ctl.originals.get(&a), Some(&2));
    // The global candidate button targets the local player (the suggested third one).
    ctl.set_and_write(3, "overlay");
    assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 2, 3));
    assert_eq!(ctl.originals.get(&c), Some(&2));
    // Restore per player.
    ctl.restore_for(a, "overlay");
    assert_eq!(f.team(0), 2);
    ctl.restore("overlay");
    assert_eq!(f.team(2), 2);
    // Restoring a player that was never written changes nothing.
    ctl.restore_for(b, "overlay");
    assert_eq!(f.team(1), 2);

    // A stale / foreign address is refused and nothing is written.
    let mut other = fake_obj(VT_PLAYER, 7);
    ctl.set_for(other.as_mut_ptr() as usize, 29, "overlay");
    assert_eq!(other[CHR_TEAM], 7);
    ctl.set_for(0, 29, "overlay");
    ctl.set_for(0x10, 29, "overlay");
    // Without vtables nothing is written either.
    ctl.vts = PlayerVtables::default();
    ctl.set_for(b, 29, "overlay");
    assert_eq!(f.team(1), 2);
}

#[test]
fn snapshot_marks_the_first_entry_local_and_a_new_instance_forgets_the_rest() {
    let f = Fake::new();
    let mut ctl = f.ctl();
    let c = f.addr(2);
    let mut snap = Snapshot::default();
    ctl.fill_snapshot(&mut snap);
    // `c` sits in the first valid entry, so it is LOCAL (the entry[0] rule), whatever its evidence says.
    assert_eq!(snap.local_player, Some(c));
    assert_eq!(snap.local_basis, Basis::FirstEntry.label());
    assert_eq!(snap.players.len(), 3);
    assert_eq!(
        snap.players.iter().map(|p| p.role).collect::<Vec<_>>(),
        vec![Role::Local, Role::Remote, Role::Remote]
    );
    assert_eq!(snap.players[0].addr, c);
    // The global buttons write to the local player only.
    ctl.set_and_write(29, "overlay");
    assert_eq!((f.team(0), f.team(1), f.team(2)), (2, 2, 29));
    // A new WorldChrMan instance forgets everything keyed by address.
    ctl.set_instance(f.wcm.as_ptr() as usize + 8);
    assert!(ctl.originals.is_empty());
}

// ---- T-020: the PvPvE mode ----

fn pvpve_cfg(team: u8) -> Config {
    Config {
        dev_mode: true,
        pvpve: true,
        pvpve_team: team,
        pvpve_scheme: "all".to_string(),
        ..Config::default()
    }
}

#[test]
fn pvpve_rewrites_only_team_2_and_counts_the_writes() {
    assert!(pvpve_wants_write(2));
    for v in [0u8, 1, 3, 6, 13, 29, 30, 255] {
        assert!(!pvpve_wants_write(v), "{v}");
    }
    let f = Fake::new();
    let mut ctl = f.ctl_with(&pvpve_cfg(29));
    assert_eq!(ctl.pvpve_state(), PvpveState::On);
    // Players: 0 = Roundtable (1), 1 = expedition default (2), 2 = already 29 (e.g. set by hand).
    f_set(&f, 0, 1);
    f_set(&f, 2, 29);
    ctl.pvpve_tick();
    assert_eq!((f.team(0), f.team(1), f.team(2)), (1, 29, 29));
    assert_eq!(ctl.pvpve_writes(), 1);
    // A second pass has nothing to do.
    ctl.pvpve_tick();
    assert_eq!(ctl.pvpve_writes(), 1);
    // The expedition starts for the Roundtable player too: 1 -> 2 (the game) -> 29 (us).
    f_set(&f, 0, 2);
    ctl.pvpve_tick();
    assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 29, 29));
    assert_eq!(ctl.pvpve_writes(), 2);
    // The original (2) was captured, so F12 can restore it.
    assert_eq!(ctl.originals.get(&f.addr(0)), Some(&2));
    // A manual experiment value (6) is never fought.
    f_set(&f, 1, 6);
    ctl.pvpve_tick();
    assert_eq!(f.team(1), 6);
    assert_eq!(ctl.pvpve_writes(), 2);
    // Other team values work too.
    let g = Fake::new();
    let mut ctl13 = g.ctl_with(&pvpve_cfg(13));
    ctl13.pvpve_tick();
    assert_eq!((g.team(0), g.team(1), g.team(2)), (13, 13, 13));
    assert_eq!(ctl13.pvpve_writes(), 3);
    assert_eq!(ctl13.player_count_text(), "3");
}

#[test]
fn pvpve_runs_from_tick_on_its_own_interval_and_never_when_off() {
    let f = Fake::new();
    let mut cfg = pvpve_cfg(29);
    cfg.pvpve_interval_ms = 5000;
    let mut ctl = f.ctl_with(&cfg);
    ctl.tick();
    assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 29, 29));
    // Not due again for 5 s: a value reset to 2 is left alone by an immediate second tick.
    f_set(&f, 1, 2);
    ctl.tick();
    assert_eq!(f.team(1), 2);
    ctl.next_pvpve = Instant::now();
    ctl.tick();
    assert_eq!(f.team(1), 29);

    // Mode off (the default config): nothing is ever written; the checkbox and F11 cannot turn it on.
    let g = Fake::new();
    let mut off = g.ctl();
    assert_eq!(off.pvpve_state(), PvpveState::Off);
    off.tick();
    off.set_pvpve(true, "console");
    assert_eq!(off.pvpve_state(), PvpveState::Off);
    off.next_pvpve = Instant::now();
    off.tick();
    assert_eq!(off.pvpve_writes(), 0);
    assert_eq!((g.team(0), g.team(1), g.team(2)), (2, 2, 2));
}

#[test]
fn pause_and_resume_from_the_console() {
    let f = Fake::new();
    let mut ctl = f.ctl_with(&pvpve_cfg(29));
    ctl.pvpve_tick();
    assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 29, 29));

    // `pvp 0`: pauses; the console then restores every player with a captured original (2), like the old F12. The mode must
    // not write the value again while paused.
    ctl.set_pvpve(false, "console");
    for i in 0..3 {
        ctl.restore_for(f.addr(i), "console");
    }
    assert_eq!(ctl.pvpve_state(), PvpveState::Paused);
    assert_eq!((f.team(0), f.team(1), f.team(2)), (2, 2, 2));
    ctl.next_pvpve = Instant::now();
    ctl.tick();
    assert_eq!((f.team(0), f.team(1), f.team(2)), (2, 2, 2));
    let writes_when_paused = ctl.pvpve_writes();

    // `pvp 1` resumes it, and the next tick applies again.
    ctl.set_pvpve(true, "console");
    assert_eq!(ctl.pvpve_state(), PvpveState::On);
    ctl.tick();
    assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 29, 29));
    assert_eq!(ctl.pvpve_writes(), writes_when_paused + 3);

    // `pvp 0` pauses it again.
    ctl.set_pvpve(false, "console");
    f_set(&f, 0, 2);
    ctl.next_pvpve = Instant::now();
    ctl.tick();
    assert_eq!(f.team(0), 2);

    // Resumed, the next tick writes its own value to everyone again.
    ctl.set_pvpve(true, "console");
    assert_eq!(ctl.pvpve_state(), PvpveState::On);
    ctl.tick();
    assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 29, 29));

    // The state survives a WorldChrMan change; a paused mode stays paused.
    ctl.set_pvpve(false, "console");
    ctl.set_instance(0);
    assert_eq!(ctl.pvpve_state(), PvpveState::Paused);
    let mut snap = Snapshot::default();
    ctl.fill_snapshot(&mut snap);
    assert_eq!(
        (snap.pvpve, snap.pvpve_team, snap.pvpve_writes),
        (PvpveState::Paused, 29, ctl.pvpve_writes())
    );
    assert!(ctl.pvpve_summary().starts_with("PAUSED (team 29,"));
}

#[test]
fn the_deprecated_alias_drives_the_same_mode() {
    let (cfg, _w) =
        crate::config::parse("dev_mode = true\nauto_team_all = 29\npvpve_scheme = \"all\"\n")
            .expect("parses");
    let f = Fake::new();
    let mut ctl = f.ctl_with(&cfg);
    assert_eq!(ctl.pvpve_state(), PvpveState::On);
    ctl.pvpve_tick();
    assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 29, 29));
}

#[test]
fn player_mode_cannot_pause_or_resume() {
    // A player-mode config (parse forces pvpve on, rivals).
    let (cfg, _w) = crate::config::parse("pvpve = false\npvpve_scheme = \"all\"\n").expect("ok");
    let f = Fake::new();
    let mut ctl = f.ctl_with(&cfg);
    assert_eq!(ctl.pvpve_state(), PvpveState::On);
    ctl.set_pvpve(false, "console");
    assert_eq!(ctl.pvpve_state(), PvpveState::On);
}

#[test]
fn pvpve_write_failures_count_as_nothing() {
    // Without vtables the gate refuses every write: no panic, no count, memory untouched.
    let f = Fake::new();
    let mut ctl = f.ctl_with(&pvpve_cfg(29));
    ctl.vts = PlayerVtables::default();
    ctl.pvpve_tick();
    assert_eq!(ctl.pvpve_writes(), 0);
    assert_eq!((f.team(0), f.team(1), f.team(2)), (2, 2, 2));
}

#[test]
fn the_resolve_log_ignores_changing_evidence_but_not_changing_entries() {
    let f = Fake::new();
    let r1 = resolve(f.wcm.as_ptr() as usize, &vts()).expect("ok");
    let mut lg = ResolveLog::default();
    assert!(!lg.lines(&r1, 0, (0, 0)).is_empty());
    // Same entry bytes and players, but the evidence (e.g. an obfuscated pointer that differs on every read)
    // changes: nothing is logged, not even after the coalescing gap.
    let mut r2 = resolve(f.wcm.as_ptr() as usize, &vts()).expect("ok");
    for p in &mut r2.probes {
        for s in &mut p.signals {
            s.value = s.value.map(|v| v ^ 0x1234_5678);
        }
    }
    assert!(lg.lines(&r2, 1000, (0, 0)).is_empty());
    assert!(lg.lines(&r2, 60_000, (0, 0)).is_empty());
    // A different player set is a real change.
    let mut r3 = resolve(f.wcm.as_ptr() as usize, &vts()).expect("ok");
    r3.players.pop();
    assert!(!lg.lines(&r3, 120_000, (0, 0)).is_empty());
}

#[test]
fn persist_is_per_player() {
    let f = Fake::new();
    let mut ctl = f.ctl();
    let (a, b, c) = (f.addr(0), f.addr(1), f.addr(2));

    // Persist on for a remote row: starts with the selected candidate (nothing written yet).
    ctl.set_persist_for(a, true, "overlay");
    assert_eq!(ctl.persist.get(&a).map(|p| p.value), Some(6));
    // Writing a value to a persisted player updates its persist value.
    ctl.set_for(a, 29, "overlay");
    assert_eq!(ctl.persist.get(&a).map(|p| p.value), Some(29));
    // Persist on for the local player (nothing written to it): the selected candidate.
    ctl.set_persist_for(c, true, "console");
    assert_eq!(ctl.persist.get(&c).map(|p| p.value), Some(6));
    assert_eq!(ctl.persist.len(), 2);
    assert!(!ctl.persist.contains_key(&b));
    assert_eq!(
        ctl.poll_interval(Duration::from_millis(500)),
        ctl.persist_every
    );

    // A tick applies each player's own value to that player alone.
    f_set(&f, 0, 2); // the game "reverts" a
    ctl.persist_tick();
    assert_eq!((f.team(0), f.team(1), f.team(2)), (29, 2, 6));
    // The game reverts c: counted for c only, re-applied.
    f_set(&f, 2, 2);
    ctl.persist_tick();
    assert_eq!(f.team(2), 6);
    assert_eq!(ctl.persist[&c].tracker.total_reverts(), 1);
    assert_eq!(
        ctl.persist[&a].tracker.total_reverts(),
        0,
        "a: only its first application so far"
    );
    let mut snap = Snapshot::default();
    ctl.fill_snapshot(&mut snap);
    let by_addr = |x: usize| {
        snap.players
            .iter()
            .find(|p| p.addr == x)
            .cloned()
            .expect("row")
    };
    assert_eq!(by_addr(a).persist, Some(29));
    assert_eq!(by_addr(b).persist, None);
    assert_eq!(by_addr(c).persist, Some(6));
    assert!(
        snap.persist,
        "the global checkbox shows the local player's persist"
    );
    assert_eq!(snap.reverts_total, 1);

    // Turning one off leaves the other running.
    ctl.set_persist_for(a, false, "overlay");
    assert_eq!(ctl.persist.len(), 1);
    f_set(&f, 0, 2);
    ctl.persist_tick();
    assert_eq!(f.team(0), 2, "a is no longer persisted");
    // A new WorldChrMan instance drops all persist entries.
    ctl.set_instance(0);
    assert!(ctl.persist.is_empty());
    assert_eq!(
        ctl.poll_interval(Duration::from_millis(500)),
        Duration::from_millis(500)
    );
}

/// Set a fake player's team byte directly (the "game" changing it).
fn f_set(f: &Fake, i: usize, v: u8) {
    let p = f.addr(i) + CHR_TEAM;
    // SAFETY: test-only; `p` is inside a Vec owned by `f` that outlives this call.
    unsafe { *(p as *mut u8) = v };
}

#[test]
fn mark_counts_across_sources_and_instance_changes() {
    let f = Fake::new();
    let mut ctl = f.ctl();
    assert_eq!(ctl.marker.count(), 0);
    ctl.mark(Some("A hits B"));
    ctl.mark(None);
    ctl.set_instance(0); // the numbering survives a WorldChrMan change
    ctl.mark(Some(""));
    let mut snap = Snapshot::default();
    ctl.fill_snapshot(&mut snap);
    assert_eq!(snap.marks, 3);
}

#[test]
fn snapshot_and_commands_on_fake_memory() {
    let mut player = fake_obj(VT_PLAYER, 2);
    let pa = player.as_mut_ptr() as u64;
    let entries = entries_with_state(&[pa, 0], &[]);
    let (wcm, _e) = fake_wcm(2, &entries);

    let cfg = Config::default();
    let mut ctl = TeamCtl::new(&cfg, 0, 0);
    let mut snap = Snapshot::default();

    // WorldChrMan not live / no vtables: nothing resolved, and the note says why.
    ctl.fill_snapshot(&mut snap);
    assert_eq!((snap.local_player, snap.team_type), (None, None));
    assert!(!snap.vtables_known);
    assert!(snap.note.contains("not live"));
    assert!(snap.players.is_empty());
    assert_eq!(snap.selected, 6);
    assert_eq!(snap.candidates, crate::config::DEFAULT_CANDIDATES.to_vec());

    ctl.set_instance(wcm.as_ptr() as usize);
    ctl.fill_snapshot(&mut snap); // still no vtables
    assert!(snap.note.contains("vtables"));
    ctl.vts = vts();
    ctl.next_live = Instant::now();
    ctl.fill_snapshot(&mut snap);
    assert!(snap.vtables_known);
    assert_eq!(snap.local_player, Some(pa as usize));
    assert_eq!(snap.team_type, Some(2));
    assert_eq!(snap.original, None);
    assert!(snap.note.is_empty());
    assert_eq!(snap.players.len(), 1, "a lone PlayerIns is one LOCAL row");
    assert_eq!(snap.players[0].role, Role::Local);
    assert_eq!(snap.local_basis, Basis::FirstEntry.label());

    // Overlay button: select 29 and write it. The byte changes, the original is captured.
    ctl.set_and_write(29, "overlay");
    assert_eq!(player[CHR_TEAM], 29);
    assert_eq!(ctl.cycler.current(), 29);
    ctl.next_live = Instant::now();
    ctl.fill_snapshot(&mut snap);
    assert_eq!(
        (snap.team_type, snap.original, snap.selected),
        (Some(29), Some(2), 29)
    );
    assert_eq!(snap.players[0].original, Some(2));

    // A value that is not a candidate is refused and nothing is written.
    ctl.set_and_write(99, "overlay");
    assert_eq!((player[CHR_TEAM], ctl.cycler.current()), (29, 29));

    // Persist toggle shows up in the snapshot; the revert window is only reported while persisting.
    ctl.set_persist_for(pa as usize, true, "console");
    ctl.fill_snapshot(&mut snap);
    assert!(snap.persist);
    ctl.set_persist_for(pa as usize, false, "console");
    ctl.fill_snapshot(&mut snap);
    assert!(!snap.persist);
    assert_eq!(snap.reverts_last_sec, 0);

    // Restore puts the captured original back.
    ctl.restore("overlay");
    assert_eq!(player[CHR_TEAM], 2);

    // A new WorldChrMan instance forgets the original and the live view.
    ctl.set_instance(0);
    ctl.fill_snapshot(&mut snap);
    assert_eq!((snap.local_player, snap.original), (None, None));
}

// ---- T-026: rosters, the rivals scheme ----

fn pid(n: u64) -> u64 {
    ((0x6000 + n) << 32) | 0xFFFF_FFFF
}

type Synth = (Vec<(usize, Option<u64>)>, RosterView);

/// Synthetic players: address 0x1000 * (i + 1), id = pid(ordinals[i]); local = index `local`.
fn synth(ordinals: &[u64], local: usize, size: usize) -> Synth {
    let players: Vec<(usize, Option<u64>)> = ordinals
        .iter()
        .enumerate()
        .map(|(i, &n)| (0x1000 * (i + 1), Some(pid(n))))
        .collect();
    let view = build_view(&players, Some(players[local].0), size, None);
    (players, view)
}

/// Plan for synthetic players that all sit at team_type 2 and were never written.
fn plan_all_at_2(scheme: Scheme, players: &[(usize, Option<u64>)], view: &RosterView) -> PvpvePlan {
    let live: Vec<(usize, Option<u8>)> = players.iter().map(|&(a, _)| (a, Some(2))).collect();
    plan_pvpve(scheme, view, &live, &HashSet::new())
}

#[test]
fn rivals_scheme_writes_only_rivals_for_every_size_and_count() {
    for n in [2usize, 3, 4, 6] {
        for size in 1..=3usize {
            for local in 0..n {
                let ords: Vec<u64> = (1..=n as u64).collect();
                let (players, view) = synth(&ords, local, size);
                let plan = plan_all_at_2(Scheme::Rivals, &players, &view);
                // Ids are sorted ordinals, chunked by size.
                let local_team = local / size;
                let expect: Vec<usize> = (0..n)
                    .filter(|i| i / size != local_team)
                    .map(|i| players[i].0)
                    .collect();
                assert_eq!(plan.writes, expect, "n={n} size={size} local={local}");
                assert!(!plan.writes.contains(&players[local].0));
                assert!(plan.restores.is_empty());
                for (i, &(addr, _)) in players.iter().enumerate() {
                    let want = if i == local {
                        Relation::Local
                    } else if i / size == local_team {
                        Relation::Ally
                    } else {
                        Relation::Rival
                    };
                    assert_eq!(view.relation[&addr], want);
                    assert_eq!(usize::from(view.team_no[&players[i].0]), i / size);
                }
            }
        }
    }
}

#[test]
fn rivals_scheme_only_writes_value_2_and_never_allies() {
    // Teams [1,2] [3,4]; local = id 1.
    let (players, view) = synth(&[1, 2, 3, 4], 0, 2);
    let live = vec![
        (players[0].0, Some(2)), // local
        (players[1].0, Some(2)), // ally
        (players[2].0, Some(1)), // rival with the Roundtable value
        (players[3].0, Some(2)), // rival
    ];
    let plan = plan_pvpve(Scheme::Rivals, &view, &live, &HashSet::new());
    assert_eq!(plan.writes, vec![players[3].0]);
    let live = vec![(players[2].0, None), (players[3].0, Some(29))];
    let plan = plan_pvpve(Scheme::Rivals, &view, &live, &HashSet::new());
    assert!(plan.writes.is_empty());
}

#[test]
fn mm_team_size_override_changes_the_rosters_only_when_set() {
    // MM-2: the size passed to `build_view` comes from `mm::pick_team_size(configured, override)`.
    let configured = 1;
    let (players, _) = synth(&[1, 2, 3, 4], 0, configured);
    let at = |ov: usize| {
        let size = crate::mm::pick_team_size(configured, ov);
        build_view(&players, Some(players[0].0), size, None)
    };
    // No override (queue off, or not MATCH READY): free-for-all as configured.
    assert_eq!(at(0).relation[&players[1].0], Relation::Rival);
    // Duo match: the lowest two ids are allies, the rest rivals.
    let duo = at(2);
    assert_eq!(duo.relation[&players[1].0], Relation::Ally);
    assert_eq!(duo.relation[&players[2].0], Relation::Rival);
    // Solo match over a configured team_size of 2 forces free-for-all.
    let (players2, _) = synth(&[1, 2, 3, 4], 0, 2);
    let solo = build_view(
        &players2,
        Some(players2[0].0),
        crate::mm::pick_team_size(2, 1),
        None,
    );
    assert_eq!(solo.relation[&players2[1].0], Relation::Rival);
}
#[test]
fn a_rival_turning_ally_is_planned_for_restore() {
    let (players, view) = synth(&[1, 2, 3], 0, 1);
    let written: HashSet<usize> = [players[1].0, players[2].0].into_iter().collect();
    let live: Vec<_> = players.iter().map(|&(a, _)| (a, Some(29))).collect();
    let plan = plan_pvpve(Scheme::Rivals, &view, &live, &written);
    assert!(plan.restores.is_empty() && plan.writes.is_empty());
    // Same players with team_size 2: id 2 is now an ally of LOCAL.
    let (_, view2) = synth(&[1, 2, 3], 0, 2);
    let plan = plan_pvpve(Scheme::Rivals, &view2, &live, &written);
    assert_eq!(plan.restores, vec![players[1].0]);
    assert!(plan.writes.is_empty());
}

#[test]
fn scheme_all_is_unchanged_and_ignores_the_rosters() {
    let (players, view) = synth(&[1, 2, 3, 4], 0, 2);
    let plan = plan_all_at_2(Scheme::All, &players, &view);
    assert_eq!(plan.writes, players.iter().map(|p| p.0).collect::<Vec<_>>());
    let live = vec![(players[0].0, Some(1)), (players[1].0, Some(2))];
    let written: HashSet<usize> = [players[0].0].into_iter().collect();
    let plan = plan_pvpve(Scheme::All, &view, &live, &written);
    assert_eq!(plan.writes, vec![players[1].0]);
    assert!(plan.restores.is_empty());
}

#[test]
fn unreadable_or_duplicate_ids_fail_open_to_free_for_all() {
    let players = vec![
        (0x1000, Some(pid(1))),
        (0x2000, None),
        (0x3000, Some(pid(3))),
    ];
    let view = build_view(&players, Some(0x1000), 2, None);
    assert!(view.warning.is_some());
    assert!(view.summary.starts_with("teams: WARNING"));
    assert_eq!(view.relation[&0x1000], Relation::Local);
    assert_eq!(view.relation[&0x2000], Relation::Rival);
    assert_eq!(view.relation[&0x3000], Relation::Rival);
    assert!(view.team_no.is_empty());
    let plan = plan_all_at_2(Scheme::Rivals, &players, &view);
    assert_eq!(plan.writes, vec![0x2000, 0x3000]);

    let dup = vec![(0x1000, Some(pid(1))), (0x2000, Some(pid(1)))];
    let view = build_view(&dup, Some(0x1000), 1, None);
    assert!(view.warning.as_deref().unwrap_or("").contains("same id"));
    assert_eq!(view.relation[&0x2000], Relation::Rival);

    // An id of 0 counts as unreadable.
    let zero = vec![(0x1000, Some(pid(1))), (0x2000, Some(0))];
    assert!(build_view(&zero, Some(0x1000), 1, None).warning.is_some());

    // No local player: nobody is written in the rivals scheme.
    let none = build_view(&dup, None, 1, None);
    assert!(plan_all_at_2(Scheme::Rivals, &dup, &none).writes.is_empty());
}

#[test]
fn roster_summary_matches_the_log_format_and_key_tracks_changes() {
    let players = vec![
        (0x1000, Some(0x6001_FFFF_FFFFu64)),
        (0x2000, Some(0x6002_FFFF_FFFF)),
    ];
    let v = build_view(&players, Some(0x1000), 1, None);
    assert_eq!(
        v.summary,
        "teams: team 0 = [0x6001FFFFFFFF(LOCAL)], team 1 = [0x6002FFFFFFFF]"
    );
    let v2 = build_view(&players, Some(0x1000), 2, None);
    assert_eq!(
        v2.summary,
        "teams: team 0 = [0x6001FFFFFFFF(LOCAL), 0x6002FFFFFFFF]"
    );
    assert_eq!(v.key, build_view(&players, Some(0x1000), 1, None).key);
    assert_ne!(v.key, build_view(&players, Some(0x2000), 1, None).key);
    assert_eq!(
        crate::overlay::shared::short_id(Some(0x6001_FFFF_FFFF)),
        "0x6001.."
    );
}

/// Set the per-player id of fake object `i`.
fn set_id(f: &mut Fake, i: usize, ord: u64) {
    f.objs[i][CHR_P2P_HANDLE..CHR_P2P_HANDLE + 8].copy_from_slice(&pid(ord).to_le_bytes());
}

fn rivals_cfg(size: u64) -> Config {
    Config {
        team_size: size,
        pvpve_scheme: "rivals".to_string(),
        ..pvpve_cfg(29)
    }
}

#[test]
fn rivals_tick_on_fake_memory_spares_local_and_allies_and_restores() {
    // Entry order: objs[2] (LOCAL), objs[0], objs[1].
    let mut f = Fake::new();
    set_id(&mut f, 2, 1);
    set_id(&mut f, 0, 4);
    set_id(&mut f, 1, 2);
    // Teams [1,2] [4]: objs[1] is an ally, objs[0] a rival.
    let mut ctl = f.ctl_with(&rivals_cfg(2));
    ctl.pvpve_tick();
    assert_eq!((f.team(2), f.team(0), f.team(1)), (2, 29, 2));
    assert_eq!(ctl.pvpve_writes(), 1);
    // Id of objs[1] becomes 7: teams [1,4] [7]; objs[0] is an ally now, objs[1] a rival.
    set_id(&mut f, 1, 7);
    ctl.pvpve_tick();
    assert_eq!((f.team(2), f.team(0), f.team(1)), (2, 2, 29));
    // Pausing restores what the tick wrote.
    ctl.set_pvpve(false, "test");
    assert_eq!((f.team(2), f.team(0), f.team(1)), (2, 2, 2));
    assert!(ctl.pvpve_written.is_empty());
    // Resuming writes again.
    ctl.set_pvpve(true, "test");
    ctl.pvpve_tick();
    assert_eq!(f.team(1), 29);
}

#[test]
fn scheme_all_tick_on_fake_memory_writes_everyone_with_value_2() {
    let mut f = Fake::new();
    set_id(&mut f, 2, 1);
    set_id(&mut f, 0, 2);
    set_id(&mut f, 1, 3);
    let mut cfg = rivals_cfg(3);
    cfg.pvpve_scheme = "all".to_string();
    let mut ctl = f.ctl_with(&cfg);
    ctl.pvpve_tick();
    assert_eq!((f.team(2), f.team(0), f.team(1)), (29, 29, 29));
    assert_eq!(ctl.pvpve_writes(), 3);
    assert!(ctl.pvpve_written.is_empty());
}

#[test]
fn unreadable_ids_on_fake_memory_write_every_remote_and_not_local() {
    // All ids are zero, which counts as unreadable.
    let f = Fake::new();
    let mut ctl = f.ctl_with(&rivals_cfg(2));
    ctl.pvpve_tick();
    assert_eq!((f.team(2), f.team(0), f.team(1)), (2, 29, 29));
}

// ---- T-032: inert downed switch, MARK events, snap-back test on fake memory ----

fn dev_cfg() -> Config {
    Config {
        dev_mode: true,
        ..Config::default()
    }
}

fn put3(f: &mut Fake, i: usize, off: usize, v: [f32; 3]) {
    for (k, x) in v.iter().enumerate() {
        f.objs[i][off + k * 4..off + k * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
}

fn get3(f: &Fake, i: usize, off: usize) -> [f32; 3] {
    let b = &f.objs[i][off..off + 12];
    let g = |k: usize| f32::from_le_bytes([b[k], b[k + 1], b[k + 2], b[k + 3]]);
    [g(0), g(4), g(8)]
}

#[test]
fn snap_back_writes_only_the_selected_candidate_of_the_local_player() {
    let mut f = Fake::new();
    // LOCAL is objs[2]; give every object a position at 0x6C0 and 0x6D0.
    for i in 0..3 {
        put3(&mut f, i, 0x6C0, [10.0 + i as f32, 0.0, 0.0]);
        put3(&mut f, i, 0x6D0, [20.0 + i as f32, 0.0, 0.0]);
    }
    let mut ctl = f.ctl_with(&dev_cfg());
    // Nothing saved yet: F6 does nothing.
    ctl.pos_write("F6");
    assert_eq!(get3(&f, 2, 0x6C0), [12.0, 0.0, 0.0]);
    ctl.pos_save("F7");
    assert!(ctl.pos.has_saved());
    for i in 0..3 {
        put3(&mut f, i, 0x6C0, [100.0 + i as f32, 1.0, 1.0]);
        put3(&mut f, i, 0x6D0, [200.0 + i as f32, 1.0, 1.0]);
    }
    // Candidate 1 (Physics+0x70) has no physics module in this fake: F6 is refused and nothing is written.
    ctl.pos_write("F6");
    assert_eq!(get3(&f, 2, 0x6C0), [102.0, 1.0, 1.0]);
    assert!(ctl.pos_follow.is_empty());
    // Candidate 2 (PlayerIns+0x6C0) is written on the LOCAL player only; 0x6D0 is not.
    ctl.pos_cycle();
    ctl.pos_write("F6");
    assert_eq!(get3(&f, 2, 0x6C0), [12.0, 0.0, 0.0]);
    assert_eq!(get3(&f, 2, 0x6D0), [202.0, 1.0, 1.0]);
    for i in 0..2 {
        assert_eq!(get3(&f, i, 0x6C0), [100.0 + i as f32, 1.0, 1.0]);
    }
    assert_eq!(ctl.pos_follow.len(), 3);
    // F9 selects the next candidate; F6 writes it.
    ctl.pos_cycle();
    ctl.pos_write("F6");
    assert_eq!(get3(&f, 2, 0x6D0), [22.0, 0.0, 0.0]);
    // The follow-ups only read.
    for e in ctl.pos_follow.iter_mut() {
        e.0 = Instant::now();
    }
    ctl.pos_follow_tick();
    assert!(ctl.pos_follow.is_empty());
    // A new WorldChrMan instance forgets the saved values and the pending follow-ups.
    ctl.set_instance(0);
    assert!(!ctl.pos.has_saved());
    assert!(ctl.pos_follow.is_empty());
}

#[test]
fn snap_back_is_refused_without_vtables_and_absent_in_the_player_build() {
    let mut f = Fake::new();
    put3(&mut f, 2, 0x6C0, [1.0, 2.0, 3.0]);
    // Player build: every call is a no-op.
    let mut p = f.ctl_with(&Config::default());
    p.pos_save("F7");
    put3(&mut f, 2, 0x6C0, [9.0, 9.0, 9.0]);
    p.pos_write("F6");
    p.pos_cycle();
    assert_eq!(get3(&f, 2, 0x6C0), [9.0, 9.0, 9.0]);
    assert!(!p.pos.has_saved());
    assert!(p.pos_follow.is_empty());
    // Dev build, but the PlayerIns vtable check fails (no vtables known): nothing is saved or written.
    let mut d = f.ctl_with(&dev_cfg());
    d.vts = PlayerVtables::default();
    d.pos_save("F7");
    assert!(!d.pos.has_saved());
    // The object's vtable changed after the resolve: refused by the re-check.
    let mut d = f.ctl_with(&dev_cfg());
    d.pos_save("F7");
    assert!(d.pos.has_saved());
    f.objs[2][..8].copy_from_slice(&(VT_ENEMY as u64).to_le_bytes());
    d.pos_write("F6");
    assert_eq!(get3(&f, 2, 0x6C0), [9.0, 9.0, 9.0]);
}

// ---- MM-FILL part B: teams from the queue roster ----

/// `(ordinal, team)` -> session id -> team (the ids `synth` makes).
fn roster_map(pairs: &[(u64, u8)]) -> HashMap<u64, u8> {
    pairs.iter().map(|&(n, t)| (pid(n), t)).collect()
}

#[test]
fn a_complete_roster_map_decides_the_teams_where_chunking_would_split_a_pair() {
    // Session ids 1..6; sorted-id chunking of 2 would make [1,2] [3,4] [5,6]. The roster paired 1+4, 2+6, 3+5.
    let (players, _) = synth(&[1, 2, 3, 4, 5, 6], 0, 2);
    let map = roster_map(&[(1, 0), (4, 0), (2, 1), (6, 1), (3, 2), (5, 2)]);
    let ids = build_view(&players, Some(players[0].0), 2, None);
    assert_eq!(ids.relation[&players[1].0], Relation::Ally, "chunking: 1+2");
    let v = build_view(&players, Some(players[0].0), 2, Some(&map));
    assert_eq!(v.source, 1);
    assert!(v.summary.starts_with("teams (roster):"), "{}", v.summary);
    assert_eq!(v.relation[&players[0].0], Relation::Local);
    assert_eq!(
        v.relation[&players[3].0],
        Relation::Ally,
        "id 4 is 1's pair"
    );
    assert_eq!(v.relation[&players[1].0], Relation::Rival);
    assert_eq!(v.relation[&players[5].0], Relation::Rival);
    assert_eq!(v.team_no[&players[0].0], 0);
    assert_eq!(v.team_no[&players[5].0], 1);
    assert_eq!(v.team_no[&players[4].0], 2);
}

#[test]
fn an_incomplete_roster_map_leaves_unknown_players_neutral_and_says_so() {
    let (players, _) = synth(&[1, 2, 3, 4], 0, 2);
    let map = roster_map(&[(1, 0), (4, 0), (2, 1)]);
    let v = build_view(&players, Some(players[0].0), 2, Some(&map));
    assert_eq!(v.source, 2);
    assert!(
        v.summary.starts_with("teams (roster map 3/4, others neutral):"),
        "{}",
        v.summary
    );
    // The map, not chunking (which would pair 1+2): 4 is the ally, 2 the rival, 3 not known yet = no relation (never hostile).
    assert_eq!(v.relation[&players[3].0], Relation::Ally);
    assert_eq!(v.relation[&players[1].0], Relation::Rival);
    assert!(!v.relation.contains_key(&players[2].0), "3 is neutral");
    // LOCAL itself not mapped yet (its id just changed): LOCAL stays LOCAL, nobody is a rival or an ally.
    let other = roster_map(&[(2, 1), (3, 1)]);
    let u = build_view(&players, Some(players[0].0), 2, Some(&other));
    assert_eq!(u.relation[&players[0].0], Relation::Local);
    assert_eq!(u.relation.len(), 1, "{:?}", u.relation);
    // An empty map (nobody published yet): today's chunking, said so.
    let empty = HashMap::new();
    let e = build_view(&players, Some(players[0].0), 2, Some(&empty));
    assert!(
        e.summary.starts_with("teams (ids, roster map 0/4):"),
        "{}",
        e.summary
    );
    // No map at all: exactly today's text.
    let none = build_view(&players, Some(players[0].0), 2, None);
    assert!(
        none.summary.starts_with("teams: team 0"),
        "{}",
        none.summary
    );
    assert_eq!(none.source, 0);
    // An unreadable id still wins over any map.
    let bad = vec![(0x1000, Some(1)), (0x2000, None)];
    let w = build_view(&bad, Some(0x1000), 2, Some(&map));
    assert!(w.warning.is_some());
}

#[test]
fn solo_gives_the_same_result_with_or_without_the_roster_map() {
    let (players, _) = synth(&[7, 3, 9], 0, 1);
    let map = roster_map(&[(3, 0), (7, 1), (9, 2)]);
    let a = build_view(&players, Some(players[0].0), 1, None);
    let b = build_view(&players, Some(players[0].0), 1, Some(&map));
    for (p, _) in &players {
        assert_eq!(a.relation[p], b.relation[p]);
    }
    assert_eq!(b.relation[&players[1].0], Relation::Rival);
}
