use super::*;
use crate::overlay::shared::Relation;
use std::collections::{HashMap, HashSet};

const P0: usize = 0x1000_0000; // LOCAL
const P1: usize = 0x1001_0000; // rival
const P2: usize = 0x1002_0000; // ally
const G0: usize = 0x2000_0000;
const G1: usize = 0x2001_0000;
const H0: u64 = 0xFFFF_FFFF_10C0_0000;
const H1: u64 = 0xFFFF_FFFF_10C0_0001;
const H2: u64 = 0xFFFF_FFFF_10C0_0002;
/// Offset of the fake `[PlayerIns+0xB78]` block from the game-data address.
const BLK: usize = 0x4000;

#[derive(Default)]
struct Fake {
    bytes: HashMap<usize, u8>,
    players: HashSet<usize>,
    gds: HashSet<usize>,
    writes: Vec<(usize, Vec<u8>)>,
    refuse_write: bool,
    /// Addresses whose next N stores are accepted but do not change the value (the game puts the value back).
    no_take: HashMap<usize, u32>,
}

impl Fake {
    fn set(&mut self, addr: usize, data: &[u8]) {
        for (i, b) in data.iter().enumerate() {
            self.bytes.insert(addr + i, *b);
        }
    }
    fn get(&self, addr: usize, n: usize) -> Option<Vec<u8>> {
        (0..n)
            .map(|i| self.bytes.get(&(addr + i)).copied())
            .collect()
    }
    /// A standing player with handle `h`, game data at `g`.
    fn player(&mut self, p: usize, g: usize, h: u64, hp: u32) {
        self.players.insert(p);
        self.gds.insert(g);
        self.set(p + OFF_DOWNED, &[0]);
        self.set(p + OFF_DOWNS, &[0]);
        self.set(p + CHR_HANDLE, &h.to_le_bytes());
        self.set(p + OFF_LAST_ATTACKER, &u64::MAX.to_le_bytes());
        self.set(p + OFF_GAMEDATA, &(g as u64).to_le_bytes());
        self.set(p + OFF_HP_PTR, &((g + BLK) as u64).to_le_bytes());
        self.hp3(p, g, hp);
    }
    /// All three HP locations of the player at `p` (game data `g`).
    fn hp3(&mut self, p: usize, g: usize, hp: u32) {
        self.set(g + GD_HP, &hp.to_le_bytes());
        self.set(p + OFF_HP_MIRROR, &hp.to_le_bytes());
        self.set(g + BLK + HP_PTR_FIELD, &hp.to_le_bytes());
    }
    fn downed(&mut self, p: usize, downs: u8, v: bool) {
        self.set(p + OFF_DOWNED, &[u8::from(v)]);
        self.set(p + OFF_DOWNS, &[downs]);
    }
    fn attacker(&mut self, p: usize, h: u64) {
        self.set(p + OFF_LAST_ATTACKER, &h.to_le_bytes());
    }
    fn hp(&mut self, g: usize, hp: u32) {
        self.set(g + GD_HP, &hp.to_le_bytes());
    }
}

impl CombatMem for Fake {
    fn is_player(&self, a: usize) -> bool {
        self.players.contains(&a)
    }
    fn is_gamedata(&self, a: usize) -> bool {
        self.gds.contains(&a)
    }
    fn read_u8(&self, a: usize) -> Option<u8> {
        self.get(a, 1).map(|v| v[0])
    }
    fn read_u32(&self, a: usize) -> Option<u32> {
        self.get(a, 4)
            .map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
    }
    fn read_u64(&self, a: usize) -> Option<u64> {
        self.get(a, 8)
            .map(|v| u64::from_le_bytes(v.try_into().unwrap_or([0; 8])))
    }
    fn write(&mut self, a: usize, d: &[u8]) -> Result<(), &'static str> {
        if self.refuse_write {
            return Err("refused by test");
        }
        self.writes.push((a, d.to_vec()));
        if let Some(n) = self.no_take.get_mut(&a).filter(|n| **n > 0) {
            *n -= 1;
        } else {
            self.set(a, d);
        }
        Ok(())
    }
}

fn subjects() -> Vec<Subject> {
    vec![
        Subject {
            addr: P0,
            label: "LOCAL[0]".into(),
            local: true,
            id: Some(0x6001),
            relation: Some(Relation::Local),
            team_no: Some(0),
        },
        Subject {
            addr: P1,
            label: "REMOTE[1]".into(),
            local: false,
            id: Some(0x6002),
            relation: Some(Relation::Rival),
            team_no: Some(1),
        },
        Subject {
            addr: P2,
            label: "REMOTE[2]".into(),
            local: false,
            id: Some(0x6003),
            relation: Some(Relation::Ally),
            team_no: Some(0),
        },
    ]
}

fn world() -> Fake {
    let mut f = Fake::default();
    f.player(P0, G0, H0, 240);
    f.player(P1, G1, H1, 240);
    f.player(P2, 0x2002_0000, H2, 240);
    f
}

#[test]
fn classification_covers_every_kind_and_missing_data() {
    let infos = [
        PlayerInfo {
            handle: Some(H0),
            id: Some(1),
            relation: Some(Relation::Local),
        },
        PlayerInfo {
            handle: Some(H1),
            id: Some(2),
            relation: Some(Relation::Rival),
        },
        PlayerInfo {
            handle: Some(H2),
            id: Some(3),
            relation: Some(Relation::Ally),
        },
        PlayerInfo {
            handle: None,
            id: Some(4),
            relation: Some(Relation::Rival),
        },
        PlayerInfo {
            handle: Some(0x1234),
            id: Some(5),
            relation: None,
        },
    ];
    let k = |raw| classify(raw, &infos).kind;
    assert_eq!(k(Some(H0)), AttackerKind::SelfHit);
    assert_eq!(k(Some(H1)), AttackerKind::Rival);
    assert_eq!(classify(Some(H1), &infos).id, Some(2));
    assert_eq!(k(Some(H2)), AttackerKind::Ally);
    // Unknown handle (a PvE enemy), none, all-ones, zero, and a player with no roster relation: all OTHER.
    assert_eq!(k(Some(0xDEAD_BEEF)), AttackerKind::Other);
    assert_eq!(k(None), AttackerKind::Other);
    assert_eq!(k(Some(u64::MAX)), AttackerKind::Other);
    assert_eq!(k(Some(0)), AttackerKind::Other);
    assert_eq!(k(Some(0x1234)), AttackerKind::Other);
    // No players at all (rosters missing).
    assert_eq!(classify(Some(H1), &[]).kind, AttackerKind::Other);
    assert!(classify(Some(0xDEAD_BEEF), &infos)
        .text()
        .contains("not a player"));
    assert!(classify(Some(H1), &infos)
        .text()
        .starts_with("RIVAL id=0x2"));
}

/// T-033 tracker lines only (the T-040 state lines are tested on their own).
fn step33(c: &mut Core, t: u64, f: &mut Fake) -> Vec<String> {
    let mut l = c.step(t, &subjects(), f);
    l.retain(|x| !x.starts_with("T-040"));
    l
}

#[test]
fn transitions_down_up_and_respawn_are_detected_and_logged_once() {
    // Tracker only: the rule is off (its own tests are below).
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Vanilla);
    // First tick seeds.
    let l = step33(&mut c, 0, &mut f);
    assert_eq!(l.iter().filter(|x| x.contains("INIT")).count(), 3, "{l:?}");
    // Nothing changes: nothing logged.
    assert!(step33(&mut c, 100, &mut f).is_empty());
    // The rival downs LOCAL.
    f.downed(P0, 1, true);
    f.hp(G0, 0);
    f.attacker(P0, H1);
    let l = step33(&mut c, 200, &mut f);
    assert_eq!(l.len(), 1, "{l:?}");
    assert!(
        l[0].contains("who=LOCAL[0] id=0x6001 DOWN (downs=1, hp=0, last_attacker=RIVAL id=0x6002")
    );
    // Same state again (and a tick inside the 100 ms gate): no new line.
    assert!(step33(&mut c, 250, &mut f).is_empty());
    assert!(step33(&mut c, 300, &mut f).is_empty());
    // Revived at 50% HP, the field has been cleared: the latch supplies the attacker.
    f.downed(P0, 1, false);
    f.hp(G0, 120);
    f.attacker(P0, u64::MAX);
    let l = step33(&mut c, 400, &mut f);
    assert_eq!(l.len(), 1);
    assert!(
        l[0].contains("UP (downs=1, hp=120, last_attacker=RIVAL"),
        "{l:?}"
    );
    assert!(l[0].contains("(latched)"));
    assert!(step33(&mut c, 500, &mut f).is_empty());
    // Down again, bleed out: HP back to the maximum means RESPAWN.
    f.downed(P0, 2, true);
    f.hp(G0, 0);
    f.attacker(P0, 0xBEEF);
    let l = step33(&mut c, 600, &mut f);
    assert!(
        l[0].contains("DOWN (downs=2, hp=0, last_attacker=OTHER handle=0xBEEF"),
        "{l:?}"
    );
    f.downed(P0, 2, false);
    f.hp(G0, 240);
    let l = step33(&mut c, 700, &mut f);
    assert!(l[0].contains("RESPAWN (downs=2, hp=240"), "{l:?}");
    // A remote copy: no attacker is claimed.
    f.downed(P1, 1, true);
    f.hp(0x2001_0000, 0);
    let l = step33(&mut c, 800, &mut f);
    assert!(
        l[0].contains("who=REMOTE[1] id=0x6002 DOWN") && l[0].contains("n/a (remote copy)"),
        "{l:?}"
    );
    // HP restored without a downed change.
    f.hp(G0, 100);
    f.hp(G0, 0);
    step33(&mut c, 900, &mut f);
    f.hp(G0, 120);
    let l = step33(&mut c, 1000, &mut f);
    assert!(l[0].contains("HP restored (downs=2, hp=120"), "{l:?}");
}

#[test]
fn a_player_whose_class_check_fails_is_skipped_and_hp_needs_the_class() {
    let mut f = world();
    // PlayerGameData pointer leads to something that is not the class: HP unknown, no crash.
    f.gds.remove(&G0);
    let r = read_state(&f, P0);
    assert_eq!(r.hp, None);
    assert_eq!(r.downed, Some(false));
    // Not a PlayerIns at all: skipped entirely.
    f.players.remove(&P2);
    let mut c = Core::new(true, RevivePolicy::Redown);
    let l = c.step(0, &subjects(), &mut f);
    assert_eq!(l.iter().filter(|x| x.contains("INIT")).count(), 2, "{l:?}");
}

// ---- T-035: rule v1 ----

/// Address of the authoritative HP of the player whose game data is `g`.
fn auth_addr(g: usize) -> usize {
    g + BLK + HP_PTR_FIELD
}

/// LOCAL is downed by `attacker` at t=100 and revived (HP `hp_after` in all three places) at t=200. Returns the lines of the
/// UP tick.
fn drive_revive(f: &mut Fake, c: &mut Core, attacker: u64, hp_after: u32) -> Vec<String> {
    c.step(0, &subjects(), f);
    f.downed(P0, 1, true);
    f.hp3(P0, G0, 0);
    f.attacker(P0, attacker);
    c.step(100, &subjects(), f);
    f.downed(P0, 1, false);
    f.hp3(P0, G0, hp_after);
    f.attacker(P0, u64::MAX);
    c.step(200, &subjects(), f)
}

fn rule_lines(l: &[String]) -> Vec<&String> {
    l.iter().filter(|x| x.starts_with("rule:")).collect()
}

#[test]
fn rule_fires_once_for_local_up_with_hp_and_a_rival_in_both_modes() {
    for dev in [true, false] {
        let mut f = world();
        let mut c = Core::new(dev, RevivePolicy::Redown);
        let l = drive_revive(&mut f, &mut c, H1, 120);
        // One store, four zero bytes, to the authoritative HP only.
        assert_eq!(
            f.writes,
            vec![(auth_addr(G0), vec![0, 0, 0, 0])],
            "dev={dev}"
        );
        assert_eq!(c.fired, 1);
        let r = rule_lines(&l);
        assert_eq!(r.len(), 1, "{l:?}");
        assert_eq!(
            r[0],
            "rule: rival completed the revive meter of LOCAL; downed again (hp 120 -> 0, readback 0)"
        );
        // The game downs the player again and later the player is revived by the rival again: no extra write from the
        // same UP, and the next UP is a separate event.
        c.step(300, &subjects(), &mut f);
        c.step(400, &subjects(), &mut f);
        assert_eq!(f.writes.len(), 1, "once per UP");
        f.downed(P0, 2, true);
        f.hp3(P0, G0, 0);
        f.attacker(P0, H1);
        c.step(500, &subjects(), &mut f);
        f.downed(P0, 2, false);
        f.hp3(P0, G0, 90);
        f.attacker(P0, u64::MAX);
        let l = c.step(600, &subjects(), &mut f);
        assert_eq!(f.writes.len(), 2);
        // The player build limits the log to one rule line per 5 s; dev mode logs every one.
        assert_eq!(rule_lines(&l).len(), usize::from(dev), "{l:?}");
    }
}

#[test]
fn rule_never_fires_for_ally_self_other_respawn_zero_hp_or_remote_players() {
    for dev in [true, false] {
        for att in [H2, H0, 0xDEAD_BEEF, u64::MAX] {
            let mut f = world();
            let mut c = Core::new(dev, RevivePolicy::Redown);
            let l = drive_revive(&mut f, &mut c, att, 120);
            assert!(f.writes.is_empty(), "dev={dev} attacker {att:#X}");
            assert!(rule_lines(&l).is_empty(), "{l:?}");
        }
        // A respawn (HP back to the maximum) is not a revive.
        let mut f = world();
        let mut c = Core::new(dev, RevivePolicy::Redown);
        drive_revive(&mut f, &mut c, H1, 240);
        assert!(f.writes.is_empty(), "dev={dev} respawn");
        // A revive that left HP at 0 is not eligible either.
        let mut f = world();
        let mut c = Core::new(dev, RevivePolicy::Redown);
        drive_revive(&mut f, &mut c, H1, 0);
        assert!(f.writes.is_empty(), "dev={dev} hp 0");
        // A remote player's own down and revive never writes, whoever the attacker was.
        let mut f = world();
        let mut c = Core::new(dev, RevivePolicy::Redown);
        c.step(0, &subjects(), &mut f);
        f.downed(P1, 1, true);
        f.hp3(P1, G1, 0);
        f.attacker(P1, H0);
        c.step(100, &subjects(), &mut f);
        f.downed(P1, 1, false);
        f.hp3(P1, G1, 120);
        c.step(200, &subjects(), &mut f);
        f.downed(P2, 1, true);
        f.hp3(P2, 0x2002_0000, 0);
        c.step(300, &subjects(), &mut f);
        f.downed(P2, 1, false);
        f.hp3(P2, 0x2002_0000, 120);
        c.step(400, &subjects(), &mut f);
        assert!(f.writes.is_empty(), "dev={dev} remote");
    }
}

#[test]
fn rival_revive_vanilla_disables_the_rule_in_dev_mode_only() {
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Vanilla);
    let l = drive_revive(&mut f, &mut c, H1, 120);
    assert!(f.writes.is_empty());
    assert!(rule_lines(&l).is_empty());
    // The tracker still reports the UP in dev mode.
    assert!(l.iter().any(|x| x.contains("UP (downs=1")), "{l:?}");
    // The player build cannot be switched off, whatever the policy it is given.
    let mut f = world();
    let mut c = Core::new(false, RevivePolicy::Vanilla);
    assert_eq!(c.policy(), RevivePolicy::Redown);
    drive_revive(&mut f, &mut c, H1, 120);
    assert_eq!(f.writes.len(), 1);
    // The gate function itself.
    let ev = Event {
        label: "LOCAL[0]".into(),
        id: None,
        addr: P0,
        local: true,
        kind: EventKind::Up,
        downed: false,
        downs: Some(1),
        hp: Some(120),
        attacker: Some((
            Attacker {
                kind: AttackerKind::Rival,
                handle: Some(H1),
                id: None,
            },
            false,
        )),
    };
    assert!(rule_should_fire(RevivePolicy::Redown, &ev));
    assert!(!rule_should_fire(RevivePolicy::Vanilla, &ev));
    let mut remote = ev.clone();
    remote.local = false;
    assert!(!rule_should_fire(RevivePolicy::Redown, &remote));
}

#[test]
fn hp_gate_needs_a_positive_value_that_the_mirror_or_game_data_confirms() {
    assert!(hp_gate(Some(120), Some(120), Some(120)));
    assert!(hp_gate(Some(120), Some(120), None));
    assert!(hp_gate(Some(120), None, Some(120)));
    assert!(hp_gate(Some(120), Some(7), Some(120)));
    assert!(hp_gate(Some(120), Some(120), Some(7)));
    assert!(!hp_gate(Some(120), Some(7), Some(8)));
    assert!(!hp_gate(Some(120), None, None));
    assert!(!hp_gate(Some(0), Some(0), Some(0)));
    assert!(!hp_gate(None, Some(120), Some(120)));
}

#[test]
fn rule_skips_and_logs_when_a_gate_fails() {
    // Mirror and game data both disagree with the authoritative value.
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Redown);
    c.step(0, &subjects(), &mut f);
    f.downed(P0, 1, true);
    f.hp3(P0, G0, 0);
    f.attacker(P0, H1);
    c.step(100, &subjects(), &mut f);
    f.downed(P0, 1, false);
    f.hp3(P0, G0, 120);
    f.set(auth_addr(G0), &77u32.to_le_bytes());
    f.attacker(P0, u64::MAX);
    let l = c.step(200, &subjects(), &mut f);
    assert!(f.writes.is_empty());
    let r = rule_lines(&l);
    assert_eq!(r.len(), 1, "{l:?}");
    assert!(
        r[0].contains("nothing was written") && r[0].contains("authoritative 77"),
        "{r:?}"
    );
    // Without a class-checked PlayerGameData the tracker has no HP at all.
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Redown);
    f.gds.remove(&G0); // PlayerGameData cannot be class-checked any more
    drive_revive(&mut f, &mut c, H1, 120);
    assert!(f.writes.is_empty());
    // Authoritative value already 0 at the time of the UP.
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Redown);
    c.step(0, &subjects(), &mut f);
    f.downed(P0, 1, true);
    f.hp3(P0, G0, 0);
    f.attacker(P0, H1);
    c.step(100, &subjects(), &mut f);
    f.downed(P0, 1, false);
    f.hp(G0, 120); // only the PlayerGameData copy shows the new HP
    f.attacker(P0, u64::MAX);
    let l = c.step(200, &subjects(), &mut f);
    assert!(
        f.writes.is_empty(),
        "authoritative HP is 0: nothing to write"
    );
    assert!(rule_lines(&l)[0].contains("authoritative 0"), "{l:?}");
    // Null / implausible pointer at +0xB78.
    for bad in [0u64, 0x1234_5679, u64::MAX] {
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        f.set(P0 + OFF_HP_PTR, &bad.to_le_bytes());
        let l = drive_revive(&mut f, &mut c, H1, 120);
        assert!(f.writes.is_empty(), "pointer {bad:#X}");
        // T-040: no longer "log and stop": the failure starts a pending re-down.
        assert!(c.pending.is_some(), "pointer {bad:#X}");
        assert!(
            rule_lines(&l)[0].contains("not a plausible pointer"),
            "{l:?}"
        );
    }
    // Unreadable authoritative HP (pointer plausible but nothing mapped there).
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Redown);
    f.set(P0 + OFF_HP_PTR, &0x7000_0000u64.to_le_bytes());
    let l = drive_revive(&mut f, &mut c, H1, 120);
    assert!(f.writes.is_empty());
    assert!(rule_lines(&l)[0].contains("authoritative ?"), "{l:?}");
    // The PlayerIns stopped being one between the event and the write.
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Redown);
    let r = c.try_rule(P0, &mut f);
    assert!(r.is_ok());
    f.players.remove(&P0);
    let r = c.try_rule(P0, &mut f);
    assert!(
        r.is_err_and(|e| e.contains("no longer has a PlayerIns vtable")),
        "vtable check"
    );
    // A refused store is logged and not counted.
    let mut f = world();
    f.refuse_write = true;
    let mut c = Core::new(true, RevivePolicy::Redown);
    let l = drive_revive(&mut f, &mut c, H1, 120);
    assert!(rule_lines(&l)[0].contains("write refused"), "{l:?}");
    assert_eq!(c.fired, 0);
}

#[test]
fn rule_mirror_gate_accepts_either_confirmation_in_a_full_run() {
    // Mirror differs, PlayerGameData agrees: fires.
    let mut f = world();
    let mut c = Core::new(false, RevivePolicy::Redown);
    c.step(0, &subjects(), &mut f);
    f.downed(P0, 1, true);
    f.hp3(P0, G0, 0);
    f.attacker(P0, H1);
    c.step(100, &subjects(), &mut f);
    f.downed(P0, 1, false);
    f.hp3(P0, G0, 120);
    f.set(P0 + OFF_HP_MIRROR, &5u32.to_le_bytes());
    f.attacker(P0, u64::MAX);
    c.step(200, &subjects(), &mut f);
    assert_eq!(f.writes.len(), 1);
    // PlayerGameData differs, mirror agrees: fires.
    let mut f = world();
    let mut c = Core::new(false, RevivePolicy::Redown);
    c.step(0, &subjects(), &mut f);
    f.downed(P0, 1, true);
    f.hp3(P0, G0, 0);
    f.attacker(P0, H1);
    c.step(100, &subjects(), &mut f);
    f.downed(P0, 1, false);
    f.hp3(P0, G0, 120);
    f.set(auth_addr(G0), &120u32.to_le_bytes());
    f.set(G0 + GD_HP, &121u32.to_le_bytes()); // tracker sees 121 > 0, the mirror still agrees with the authoritative 120
    f.attacker(P0, u64::MAX);
    c.step(200, &subjects(), &mut f);
    assert_eq!(f.writes.len(), 1);
}

#[test]
fn player_mode_logs_only_the_rule_line_and_rate_limits_it() {
    let mut f = world();
    let mut c = Core::new(false, RevivePolicy::Redown);
    // No tracker lines in the player build, not even for the INIT or a remote player's down.
    assert!(c.step(0, &subjects(), &mut f).is_empty());
    f.downed(P1, 1, true);
    assert!(c.step(100, &subjects(), &mut f).is_empty());
    // First rival revive: one line. Two more within 5 s: written but not logged. After 5 s: logged again with the count.
    let mut t = 1000;
    let mut logged = Vec::new();
    for _ in 0..4 {
        f.downed(P0, 1, true);
        f.hp3(P0, G0, 0);
        f.attacker(P0, H1);
        t += 100;
        c.step(t, &subjects(), &mut f);
        f.downed(P0, 1, false);
        f.hp3(P0, G0, 120);
        f.attacker(P0, u64::MAX);
        t += 100;
        let l = c.step(t, &subjects(), &mut f);
        logged.push(rule_lines(&l).len());
        t += if logged.len() == 3 { 5000 } else { 500 };
    }
    assert_eq!(logged, vec![1, 0, 0, 1]);
    assert_eq!(
        f.writes.len(),
        4,
        "every UP is acted on, only the log is limited"
    );
    // Dev mode logs every one.
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Redown);
    let mut t = 0;
    c.step(t, &subjects(), &mut f);
    let mut n = 0;
    for _ in 0..3 {
        f.downed(P0, 1, true);
        f.hp3(P0, G0, 0);
        f.attacker(P0, H1);
        t += 100;
        c.step(t, &subjects(), &mut f);
        f.downed(P0, 1, false);
        f.hp3(P0, G0, 120);
        f.attacker(P0, u64::MAX);
        t += 100;
        n += rule_lines(&c.step(t, &subjects(), &mut f)).len();
    }
    assert_eq!(n, 3);
}

// ---- T-039 downed rule ----

fn setf(f: &mut Fake, a: usize, v: f32) {
    f.set(a, &v.to_le_bytes());
}
fn getf(f: &Fake, a: usize) -> f32 {
    f32::from_bits(f.read_u32(a).expect("set"))
}
/// LOCAL downed with the given bar / clock / pause.
fn down_local(f: &mut Fake, bar: f32, clock: f32, pause: f32) {
    f.downed(P0, 1, true);
    setf(f, P0 + OFF_REVIVE_BAR, bar);
    setf(f, P0 + OFF_BLEED_CLOCK, clock);
    setf(f, P0 + OFF_PAUSE, pause);
}
/// A hit: the bar drops to `bar`, the pause starts and `attacker` is recorded in +0x1A0.
fn hit(f: &mut Fake, bar: f32, attacker: u64) {
    setf(f, P0 + OFF_REVIVE_BAR, bar);
    setf(f, P0 + OFF_PAUSE, 3.5);
    f.attacker(P0, attacker);
}
fn dn_lines(l: &[String]) -> Vec<&String> {
    l.iter()
        .filter(|x| x.starts_with("rule: rival hit"))
        .collect()
}
/// A core already past INIT, LOCAL downed (bar 90, clock 14.9, pause 0) and seen once at t=50.
fn downed_world(dev: bool) -> (Fake, Core, u64) {
    let mut f = world();
    let mut c = Core::new(dev, RevivePolicy::Redown);
    c.step(0, &subjects(), &mut f);
    down_local(&mut f, 90.0, 14.9, 0.0);
    c.step(50, &subjects(), &mut f);
    (f, c, 50)
}

#[test]
fn rival_hit_restores_the_bar_takes_seconds_off_the_clock_and_clears_the_pause() {
    for dev in [true, false] {
        let (mut f, mut c, t) = downed_world(dev);
        hit(&mut f, 70.0, H1);
        let l = c.step(t + 50, &subjects(), &mut f);
        assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 90.0);
        assert!((getf(&f, P0 + OFF_BLEED_CLOCK) - 11.9).abs() < 1e-4);
        assert_eq!(getf(&f, P0 + OFF_PAUSE), 0.0);
        assert_eq!(dn_lines(&l).len(), 1, "{l:?}");
        assert!(dn_lines(&l)[0].contains("bar 70 -> 90 restored"));
        assert_eq!(c.fired, 1);
        assert_eq!(f.writes.len(), 3);
        // Nothing else was touched (HP, state byte).
        assert_eq!(f.read_u8(P0 + OFF_DOWNED), Some(1));
    }
}

#[test]
fn clock_floor_is_0_05_and_the_configured_seconds_are_used() {
    let (mut f, mut c, t) = downed_world(true);
    setf(&mut f, P0 + OFF_BLEED_CLOCK, 2.0);
    c.step(t + 50, &subjects(), &mut f);
    hit(&mut f, 70.0, H1);
    c.step(t + 100, &subjects(), &mut f);
    assert!((getf(&f, P0 + OFF_BLEED_CLOCK) - 0.05).abs() < 1e-6);
    // Custom seconds, clamped inside Core as well.
    let (mut f, mut c, t) = downed_world(true);
    c.set_hit_seconds(5.5);
    hit(&mut f, 70.0, H1);
    c.step(t + 50, &subjects(), &mut f);
    assert!((getf(&f, P0 + OFF_BLEED_CLOCK) - 9.4).abs() < 1e-4);
    let (mut f, mut c, t) = downed_world(true);
    c.set_hit_seconds(500.0);
    hit(&mut f, 70.0, H1);
    c.step(t + 50, &subjects(), &mut f);
    assert!((getf(&f, P0 + OFF_BLEED_CLOCK) - (14.9 - 15.0f32).max(CLOCK_FLOOR)).abs() < 1e-6);
}

#[test]
fn consecutive_rival_hits_never_reduce_the_bar() {
    let (mut f, mut c, mut t) = downed_world(false);
    for i in 0..4 {
        // The game subtracts 20 from the (restored) bar on every hit.
        let cur = getf(&f, P0 + OFF_REVIVE_BAR);
        hit(&mut f, cur - 20.0, H1);
        t += 50;
        c.step(t, &subjects(), &mut f);
        assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 90.0, "hit {i}");
        // A quiet tick in between changes nothing.
        t += 50;
        c.step(t, &subjects(), &mut f);
        assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 90.0);
    }
    assert_eq!(c.fired, 4);
    // 14.9 - 4 * 3.0 floors at 0.05 on the last hit.
    assert!(getf(&f, P0 + OFF_BLEED_CLOCK) < 3.0);
}

#[test]
fn ally_self_and_other_hits_are_untouched_and_the_bar_is_tracked() {
    for attacker in [H2, H0, 0xFFFF_FFFF_10C0_0099, u64::MAX] {
        let (mut f, mut c, t) = downed_world(true);
        hit(&mut f, 70.0, attacker);
        let l = c.step(t + 50, &subjects(), &mut f);
        assert!(f.writes.is_empty(), "attacker {attacker:X}");
        assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 70.0);
        assert!(dn_lines(&l).is_empty());
        // bar_before follows: a later rival hit restores to 70, not 90.
        setf(&mut f, P0 + OFF_PAUSE, 0.0);
        hit(&mut f, 50.0, H1);
        c.step(t + 100, &subjects(), &mut f);
        assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 70.0);
    }
}

#[test]
fn bar_regrowth_is_tracked_and_the_current_attacker_is_the_fallback() {
    let (mut f, mut c, t) = downed_world(true);
    setf(&mut f, P0 + OFF_REVIVE_BAR, 95.0);
    assert!(c
        .step(t + 50, &subjects(), &mut f)
        .iter()
        .all(|l| !l.starts_with("rule:")));
    assert!(f.writes.is_empty());
    // +0x1A0 holds nothing valid, +0x198 names the rival.
    setf(&mut f, P0 + OFF_REVIVE_BAR, 80.0);
    f.attacker(P0, u64::MAX);
    f.set(P0 + OFF_CUR_ATTACKER, &H1.to_le_bytes());
    c.step(t + 100, &subjects(), &mut f);
    assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 95.0);
}

#[test]
fn no_writes_when_not_downed_for_remote_players_or_when_a_gate_fails() {
    // Not downed: a "bar drop" in memory is ignored.
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Redown);
    c.step(0, &subjects(), &mut f);
    setf(&mut f, P0 + OFF_REVIVE_BAR, 90.0);
    setf(&mut f, P0 + OFF_BLEED_CLOCK, 10.0);
    c.step(50, &subjects(), &mut f);
    hit(&mut f, 70.0, H1);
    c.step(100, &subjects(), &mut f);
    assert!(f.writes.is_empty());
    // State 2 (getting up) is not downed either.
    f.set(P0 + OFF_DOWNED, &[2]);
    setf(&mut f, P0 + OFF_REVIVE_BAR, 50.0);
    c.step(150, &subjects(), &mut f);
    assert!(f.writes.is_empty());
    // A downed REMOTE player is never written, even when a rival "hits" it.
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Redown);
    c.step(0, &subjects(), &mut f);
    f.downed(P1, 1, true);
    setf(&mut f, P1 + OFF_REVIVE_BAR, 90.0);
    setf(&mut f, P1 + OFF_BLEED_CLOCK, 10.0);
    c.step(50, &subjects(), &mut f);
    setf(&mut f, P1 + OFF_REVIVE_BAR, 70.0);
    f.attacker(P1, H0);
    c.step(100, &subjects(), &mut f);
    assert!(f.writes.is_empty());
    // Gates: bad clock, non-finite clock, refused write, vanished PlayerIns. Nothing written (or a refusal) and one log line.
    for case in 0..4 {
        let (mut f, mut c, t) = downed_world(true);
        match case {
            0 => setf(&mut f, P0 + OFF_BLEED_CLOCK, 99.0),
            1 => setf(&mut f, P0 + OFF_BLEED_CLOCK, f32::NAN),
            2 => f.refuse_write = true,
            _ => {
                f.players.remove(&P0);
            }
        }
        hit(&mut f, 70.0, H1);
        let l = c.step(t + 50, &subjects(), &mut f);
        if case != 2 {
            assert!(f.writes.is_empty(), "case {case}");
        }
        assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 70.0, "case {case}");
        if case < 3 {
            let msgs: Vec<_> = l
                .iter()
                .filter(|x| x.contains("nothing was written"))
                .collect();
            assert_eq!(msgs.len(), 1, "case {case}: {l:?}");
            // Logged once: a second rival hit stays quiet.
            hit(&mut f, 50.0, H1);
            let l2 = c.step(t + 100, &subjects(), &mut f);
            assert!(l2.iter().all(|x| !x.contains("nothing was written")));
        }
    }
}

#[test]
fn rule_v1_still_fires_when_a_rival_completes_the_bar() {
    let mut f = world();
    let mut c = Core::new(false, RevivePolicy::Redown);
    c.early_redown = false;
    c.step(0, &subjects(), &mut f);
    f.hp3(P0, G0, 0);
    down_local(&mut f, 5.0, 10.0, 0.0);
    f.attacker(P0, H1);
    c.step(100, &subjects(), &mut f);
    c.step(150, &subjects(), &mut f);
    // The hit completes the bar in one tick: state leaves 1, HP comes back.
    f.downed(P0, 1, false);
    f.set(P0 + OFF_DOWNED, &[2]);
    setf(&mut f, P0 + OFF_REVIVE_BAR, 0.0);
    f.hp3(P0, G0, 120);
    f.attacker(P0, H1);
    let w = f.writes.len();
    c.step(200, &subjects(), &mut f);
    // State 2 reads as unknown for the tracker, so the revive is seen when it returns to 0.
    f.set(P0 + OFF_DOWNED, &[0]);
    f.attacker(P0, u64::MAX);
    let l = c.step(300, &subjects(), &mut f);
    assert_eq!(f.writes.len(), w + 1);
    assert_eq!(f.writes[w].0, auth_addr(G0));
    assert_eq!(rule_lines(&l).len(), 1, "{l:?}");
}

#[test]
fn a_rival_completing_the_bar_is_answered_during_the_get_up_and_a_teammate_is_not() {
    for (attacker, expect) in [(H1, 1usize), (H2, 0)] {
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        c.early_redown = true;
        c.step(0, &subjects(), &mut f);
        f.hp3(P0, G0, 0);
        down_local(&mut f, 40.0, 10.0, 0.0);
        f.attacker(P0, attacker);
        c.step(100, &subjects(), &mut f);
        // One hit completes the bar: state 2, HP not granted yet.
        f.set(P0 + OFF_DOWNED, &[2]);
        setf(&mut f, P0 + OFF_REVIVE_BAR, 0.0);
        let l = c.step(150, &subjects(), &mut f);
        assert!(f.writes.is_empty(), "hp 0: gate holds");
        assert_eq!(rule_lines(&l).len(), expect, "{l:?}");
        // HP arrives: one store to the authoritative HP, once.
        f.hp3(P0, G0, 120);
        let l = c.step(200, &subjects(), &mut f);
        c.step(250, &subjects(), &mut f);
        assert_eq!(f.writes.len(), expect, "attacker {attacker:X}");
        if expect == 1 {
            assert_eq!(f.writes[0], (auth_addr(G0), vec![0, 0, 0, 0]));
            assert!(rule_lines(&l)[0].contains("during the get-up"), "{l:?}");
        }
    }
}

/// The remote copy at `p` goes down (bar/clock as the owner sent them) and is then hit.
fn remote_down_and_hit(f: &mut Fake, c: &mut Core, subs: &[Subject], p: usize) {
    c.step(0, subs, f);
    f.downed(p, 1, true);
    setf(f, p + OFF_REVIVE_BAR, 40.0);
    setf(f, p + OFF_BLEED_CLOCK, 15.0);
    setf(f, p + OFF_PAUSE, 0.0);
    c.step(50, subs, f);
    assert!(f.writes.is_empty());
    setf(f, p + OFF_REVIVE_BAR, 3.0);
    setf(f, p + OFF_BLEED_CLOCK, 12.5);
    setf(f, p + OFF_PAUSE, 3.48);
}

#[test]
fn a_hit_on_a_downed_solo_rival_is_undone_on_the_display_copy_too() {
    for dev in [true, false] {
        let mut f = world();
        let mut c = Core::new(dev, RevivePolicy::Redown);
        let subs = subjects();
        remote_down_and_hit(&mut f, &mut c, &subs, P1);
        let l = c.step(100, &subs, &mut f);
        assert_eq!(getf(&f, P1 + OFF_REVIVE_BAR), 40.0);
        assert!((getf(&f, P1 + OFF_BLEED_CLOCK) - 9.5).abs() < 1e-4);
        assert_eq!(getf(&f, P1 + OFF_PAUSE), 0.0);
        assert_eq!(f.writes.len(), 3);
        let n = l.iter().filter(|x| x.contains("(display copy)")).count();
        assert_eq!(n, usize::from(dev), "{l:?}");
        // A second hit is measured against the same full bar.
        setf(&mut f, P1 + OFF_REVIVE_BAR, 2.0);
        c.step(150, &subs, &mut f);
        assert_eq!(getf(&f, P1 + OFF_REVIVE_BAR), 40.0);
        assert_eq!(f.writes.len(), 6);
    }
}

#[test]
fn display_copies_of_allies_and_of_rivals_with_a_teammate_are_left_alone() {
    // An ally of LOCAL.
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Redown);
    let subs = subjects();
    remote_down_and_hit(&mut f, &mut c, &subs, P2);
    c.step(100, &subs, &mut f);
    assert!(f.writes.is_empty());
    // Two rivals on one team: the copy cannot tell a teammate's hit from a rival's. Unknown rosters: the same.
    for team in [Some(1), None] {
        let mut f = world();
        let mut c = Core::new(true, RevivePolicy::Redown);
        let mut subs = subjects();
        subs[1].team_no = team;
        subs[2].team_no = team;
        subs[2].relation = Some(Relation::Rival);
        remote_down_and_hit(&mut f, &mut c, &subs, P1);
        c.step(100, &subs, &mut f);
        assert!(f.writes.is_empty(), "team {team:?}");
    }
}

#[test]
fn player_mode_downed_rule_log_is_rate_limited_but_every_hit_is_acted_on() {
    let (mut f, mut c, mut t) = downed_world(false);
    let mut logged = Vec::new();
    for i in 0..4 {
        let cur = getf(&f, P0 + OFF_REVIVE_BAR);
        setf(&mut f, P0 + OFF_BLEED_CLOCK, 14.0);
        hit(&mut f, cur - 10.0, H1);
        t += 50;
        let l = c.step(t, &subjects(), &mut f);
        logged.push(dn_lines(&l).len());
        t += if i == 2 { 5000 } else { 100 };
        c.step(t, &subjects(), &mut f);
    }
    assert_eq!(logged, vec![1, 0, 0, 1]);
    assert_eq!(c.fired, 4);
    // Dev mode logs every hit and shows the status line.
    let (mut f, mut c, mut t) = downed_world(true);
    assert_eq!(
        c.downed_line(),
        Some("downed: bar 90.0 left, clock 14.9 s, pause 0.0")
    );
    let mut n = 0;
    for _ in 0..3 {
        let cur = getf(&f, P0 + OFF_REVIVE_BAR);
        hit(&mut f, cur - 10.0, H1);
        t += 50;
        n += dn_lines(&c.step(t, &subjects(), &mut f)).len();
    }
    assert_eq!(n, 3);
    // No status line outside dev mode or when not downed.
    let (_, c, _) = downed_world(false);
    assert!(c.downed_line().is_none());
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Redown);
    c.step(0, &subjects(), &mut f);
    assert!(c.downed_line().is_none());
}

// ---- T-040 pending re-down and diagnostics ----

fn t040(l: &[String], prefix: &str) -> Vec<String> {
    l.iter()
        .filter(|x| x.starts_with(prefix))
        .cloned()
        .collect()
}
fn set_hp_ptr(f: &mut Fake, v: u64) {
    f.set(P0 + OFF_HP_PTR, &v.to_le_bytes());
}
fn good_hp_ptr() -> u64 {
    (G0 + BLK) as u64
}

#[test]
fn a_failed_redown_is_retried_each_step_and_succeeds_when_the_pointer_comes_back() {
    for dev in [true, false] {
        let mut f = world();
        let mut c = Core::new(dev, RevivePolicy::Redown);
        set_hp_ptr(&mut f, 0);
        // Inside the 5 s limit of an earlier line: the pending line must still be logged.
        c.last_rule_log_ms = Some(190);
        let l = drive_revive(&mut f, &mut c, H1, 120);
        let r = rule_lines(&l);
        assert_eq!(r.len(), 1, "{l:?}");
        assert_eq!(
            r[0],
            "rule: rival completed the revive meter of LOCAL but nothing was written yet: data module: no module list; old route: [PlayerIns+0xB78] = 0x0 is not a plausible pointer; retrying for 2000 ms"
        );
        assert!(f.writes.is_empty());
        assert!(rule_lines(&c.step(250, &subjects(), &mut f)).is_empty());
        assert!(rule_lines(&c.step(300, &subjects(), &mut f)).is_empty());
        assert!(f.writes.is_empty());
        set_hp_ptr(&mut f, good_hp_ptr());
        let l = c.step(350, &subjects(), &mut f);
        let r = rule_lines(&l);
        assert_eq!(r.len(), 1, "{l:?}");
        assert_eq!(
            r[0],
            "rule: rival completed the revive meter of LOCAL; downed again after 4 tries (150 ms late) (hp 120 -> 0, readback 0)"
        );
        assert_eq!(f.writes, vec![(auth_addr(G0), vec![0, 0, 0, 0])]);
        assert_eq!(c.fired, 1);
        assert!(c.pending.is_none());
        // Done: nothing more is written.
        assert!(rule_lines(&c.step(400, &subjects(), &mut f)).is_empty());
        assert_eq!(f.writes.len(), 1);
    }
}

#[test]
fn a_redown_that_never_works_gives_up_at_2_s_and_writes_nothing() {
    for dev in [true, false] {
        let mut f = world();
        let mut c = Core::new(dev, RevivePolicy::Redown);
        set_hp_ptr(&mut f, 0x1234_5679);
        drive_revive(&mut f, &mut c, H1, 120);
        let mut gave_up = Vec::new();
        for t in (250..2400u64).step_by(50) {
            let l = c.step(t, &subjects(), &mut f);
            if t < 2200 {
                assert!(rule_lines(&l).is_empty(), "t={t} {l:?}");
            }
            gave_up.extend(rule_lines(&l).into_iter().cloned().map(|x| (t, x)));
        }
        assert_eq!(gave_up.len(), 1, "{gave_up:?}");
        let (t, line) = &gave_up[0];
        assert_eq!(*t, 2200);
        assert!(
            line.contains("gave up after 40 tries in 2000 ms, nothing written")
                && line.contains(
                    "first error: data module: no module list; old route: [PlayerIns+0xB78] = 0x12345679 is not a plausible pointer"
                )
                && line.contains("last error: data module: no module list; old route: [PlayerIns+0xB78] = 0x12345679"),
            "{line}"
        );
        assert!(f.writes.is_empty());
        assert!(c.pending.is_none());
    }
}

#[test]
fn a_pending_redown_is_cancelled_by_a_new_down_the_get_up_zero_hp_or_a_lost_player() {
    type Change = fn(&mut Fake);
    let cases: [(Change, &str); 4] = [
        (|f| f.downed(P0, 2, true), "downed again (state 1)"),
        (|f| f.set(P0 + OFF_DOWNED, &[2]), "getting up (state 2)"),
        (|f| f.hp3(P0, G0, 0), "HP reads 0"),
        (
            |f| {
                f.players.remove(&P0);
            },
            "not that PlayerIns",
        ),
    ];
    for (change, why) in cases {
        let mut f = world();
        let mut c = Core::new(false, RevivePolicy::Redown);
        set_hp_ptr(&mut f, 0);
        drive_revive(&mut f, &mut c, H1, 120);
        assert!(c.pending.is_some());
        change(&mut f);
        let l = c.step(250, &subjects(), &mut f);
        let r = rule_lines(&l);
        assert_eq!(r.len(), 1, "{why}: {l:?}");
        assert!(
            r[0].starts_with(
                "rule: pending re-down of LOCAL cancelled after 1 tries, nothing written"
            ) && r[0].contains(why),
            "{r:?}"
        );
        assert!(c.pending.is_none());
        // Even with the pointer fixed afterwards, nothing is written.
        set_hp_ptr(&mut f, good_hp_ptr());
        c.step(300, &subjects(), &mut f);
        assert!(f.writes.is_empty(), "{why}");
    }
}

#[test]
fn the_pointer_error_carries_the_raw_value_or_says_unreadable() {
    let mut f = world();
    let mut c = Core::new(true, RevivePolicy::Redown);
    set_hp_ptr(&mut f, u64::MAX);
    let e = c.try_rule(P0, &mut f).err().unwrap_or_default();
    assert_eq!(
        e,
        "data module: no module list; old route: [PlayerIns+0xB78] = 0xFFFFFFFFFFFFFFFF is not a plausible pointer"
    );
    f.bytes
        .retain(|a, _| !(P0 + OFF_HP_PTR..P0 + OFF_HP_PTR + 8).contains(a));
    let e = c.try_rule(P0, &mut f).err().unwrap_or_default();
    assert_eq!(
        e,
        "data module: no module list; old route: [PlayerIns+0xB78] unreadable"
    );
}

#[test]
fn local_state_changes_are_logged_in_both_builds_with_the_block_and_the_hp_chain() {
    for dev in [true, false] {
        let mut f = world();
        let mut c = Core::new(dev, RevivePolicy::Vanilla);
        let l = c.step(0, &subjects(), &mut f);
        assert!(t040(&l, "T-040").is_empty(), "first sight only seeds");
        down_local(&mut f, 90.0, 14.9, 0.0);
        f.hp3(P0, G0, 0);
        let l = c.step(50, &subjects(), &mut f);
        let s = t040(&l, "T-040 LOCAL state");
        assert_eq!(s.len(), 1, "{l:?}");
        assert!(
            s[0].starts_with(
                "T-040 LOCAL state 0 -> 1: bar 90, clock 14.9, pause 0, [+0xB78] = 0x20004000, hp 0 (mirror 0)"
            ),
            "{s:?}"
        );
        // The bar changing alone is not a state change.
        setf(&mut f, P0 + OFF_REVIVE_BAR, 80.0);
        let l = c.step(100, &subjects(), &mut f);
        assert!(t040(&l, "T-040 LOCAL state").is_empty());
        f.set(P0 + OFF_DOWNED, &[2]);
        let l = c.step(150, &subjects(), &mut f);
        assert!(t040(&l, "T-040 LOCAL state")[0].contains("state 1 -> 2: bar 80"));
        f.set(P0 + OFF_DOWNED, &[0]);
        let l = c.step(200, &subjects(), &mut f);
        assert!(t040(&l, "T-040 LOCAL state")[0].contains("state 2 -> 0"));
    }
}

#[test]
fn state_lines_are_capped_at_30_a_minute_and_the_next_line_reports_the_rest() {
    let mut f = world();
    let mut c = Core::new(false, RevivePolicy::Vanilla);
    c.step(0, &subjects(), &mut f);
    let mut n = 0;
    for i in 1..=40u64 {
        f.set(P0 + OFF_DOWNED, &[u8::try_from(i % 2).unwrap_or(0)]);
        n += t040(&c.step(i * 50, &subjects(), &mut f), "T-040 LOCAL state").len();
    }
    assert_eq!(n, 30);
    assert_eq!(c.diag_suppressed, 10);
    // A minute after the window started the cap is open again and the line says what was dropped.
    f.set(P0 + OFF_DOWNED, &[1]);
    let l = c.step(70_000, &subjects(), &mut f);
    let s = t040(&l, "T-040 LOCAL state");
    assert_eq!(s.len(), 1, "{l:?}");
    assert!(s[0].ends_with("(10 suppressed)"), "{s:?}");
    assert_eq!(c.diag_suppressed, 0);
}

#[test]
fn restoring_a_bar_that_read_zero_while_downed_is_logged() {
    let (mut f, mut c, t) = downed_world(false);
    hit(&mut f, 0.0, H1);
    let l = c.step(t + 50, &subjects(), &mut f);
    let s = t040(&l, "T-040 LOCAL bar read");
    assert_eq!(s.len(), 1, "{l:?}");
    assert_eq!(
        s[0],
        "T-040 LOCAL bar read 0 while state 1 (before 90): downed rule restored it"
    );
    assert_eq!(getf(&f, P0 + OFF_REVIVE_BAR), 90.0);
    // An ordinary hit (bar 90 -> 70) does not produce it.
    hit(&mut f, 70.0, H1);
    let l = c.step(t + 100, &subjects(), &mut f);
    assert!(t040(&l, "T-040 LOCAL bar read").is_empty());
}

#[test]
fn revive_fail_forces_the_next_tries_to_fail_without_touching_memory() {
    let mut f = world();
    let mut c = Core::new(false, RevivePolicy::Redown);
    c.set_forced_fails(2);
    assert_eq!(c.forced_fails(), 2);
    let l = drive_revive(&mut f, &mut c, H1, 120);
    assert!(rule_lines(&l)[0].contains("forced by console"), "{l:?}");
    assert_eq!(c.forced_fails(), 1);
    assert!(rule_lines(&c.step(250, &subjects(), &mut f)).is_empty());
    assert_eq!(c.forced_fails(), 0);
    assert!(f.writes.is_empty());
    let l = c.step(300, &subjects(), &mut f);
    assert!(
        rule_lines(&l)[0].contains("downed again after 3 tries (100 ms late)"),
        "{l:?}"
    );
    assert_eq!(f.writes.len(), 1);
}

// ---- T-040 run 1: the character data module route ----

const BAG: usize = 0x3000_0000;
const DM: usize = 0x3001_0000;

/// LOCAL's module list -> data module with `hp` / `max`, PlayerGameData max HP `max`; the old `[+0xB78]+0x70` route holds
/// 0xFFFFFFFF as on both Nucleus games in run 1.
fn data_module(f: &mut Fake, hp: u32, max: u32) {
    f.set(P0 + PLAYER_MODULES, &(BAG as u64).to_le_bytes());
    f.set(BAG, &(DM as u64).to_le_bytes());
    f.set(DM + DM_HP, &hp.to_le_bytes());
    f.set(DM + DM_MAX_HP, &max.to_le_bytes());
    f.set(G0 + GD_MAX_HP, &max.to_le_bytes());
    f.set(auth_addr(G0), &u32::MAX.to_le_bytes());
}

#[test]
fn the_redown_writes_the_data_module_when_the_old_route_holds_no_hp() {
    for dev in [true, false] {
        let mut f = world();
        let mut c = Core::new(dev, RevivePolicy::Redown);
        c.step(0, &subjects(), &mut f);
        f.downed(P0, 1, true);
        f.hp(G0, 0);
        f.attacker(P0, H1);
        c.step(100, &subjects(), &mut f);
        f.downed(P0, 1, false);
        f.hp(G0, 120);
        data_module(&mut f, 120, 240);
        f.attacker(P0, u64::MAX);
        let l = c.step(200, &subjects(), &mut f);
        let r = rule_lines(&l);
        assert_eq!(r.len(), 1, "{l:?}");
        assert_eq!(
            r[0],
            "rule: rival completed the revive meter of LOCAL; downed again (hp 120 -> 0, readback 0)"
        );
        assert_eq!(f.writes, vec![(DM + DM_HP, vec![0, 0, 0, 0])]);
        assert!(c.pending.is_none());
    }
}

#[test]
fn the_data_module_gate_needs_hp_and_max_hp_to_agree_with_the_game_data() {
    let mut f = world();
    f.hp(G0, 120);
    data_module(&mut f, 120, 240);
    assert_eq!(data_module_hp(&f, P0), Ok((DM + DM_HP, 120)));
    f.set(DM + DM_HP, &0u32.to_le_bytes());
    f.hp(G0, 0);
    assert!(data_module_hp(&f, P0).is_err(), "HP 0");
    f.set(DM + DM_HP, &120u32.to_le_bytes());
    f.hp(G0, 119);
    assert!(data_module_hp(&f, P0).is_err(), "HP differs");
    f.hp(G0, 120);
    f.set(G0 + GD_MAX_HP, &260u32.to_le_bytes());
    assert_eq!(
        data_module_hp(&f, P0),
        Err("gate failed (hp 120, max 240, PlayerGameData 120 / 260)".to_string())
    );
    // Both routes fail: the error names both.
    let mut c = Core::new(true, RevivePolicy::Redown);
    let e = c.try_rule(P0, &mut f).err().unwrap_or_default();
    assert!(
        e.starts_with("data module: gate failed") && e.contains("old route: hp gate failed"),
        "{e}"
    );
    assert!(f.writes.is_empty());
}

#[test]
fn our_morning_get_up_is_not_taken_for_a_rival_revive_while_exempt() {
    let mut f = world();
    let mut c = Core::new(false, RevivePolicy::Redown);
    // M5-6: the night match exempts rule v1 around its own get-up (LOCAL was last hit by a rival).
    c.exempt_until_ms = 10_000;
    drive_revive(&mut f, &mut c, H1, 120);
    assert!(f.writes.is_empty(), "no re-down during the exemption");
    assert_eq!(c.fired, 0);
    // After it, the rule is back.
    c.exempt_until_ms = 0;
    f.downed(P0, 2, true);
    f.hp3(P0, G0, 0);
    f.attacker(P0, H1);
    c.step(500, &subjects(), &mut f);
    f.downed(P0, 2, false);
    f.hp3(P0, G0, 90);
    f.attacker(P0, u64::MAX);
    c.step(600, &subjects(), &mut f);
    assert_eq!(f.writes.len(), 1);
}

#[test]
fn the_night_floor_keeps_the_clock_above_the_elimination_point() {
    let (mut f, mut c, t) = downed_world(true);
    c.clock_floor = crate::features::night_match::NIGHT_CLOCK_FLOOR;
    setf(&mut f, P0 + OFF_BLEED_CLOCK, 2.0);
    c.step(t + 50, &subjects(), &mut f);
    hit(&mut f, 70.0, H1);
    c.step(t + 100, &subjects(), &mut f);
    let clock = getf(&f, P0 + OFF_BLEED_CLOCK);
    assert!((clock - 0.9).abs() < 1e-6);
    assert!(clock <= crate::features::night_match::ELIM_CLOCK);
}
