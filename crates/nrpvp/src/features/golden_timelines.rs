//! R7-1 (P2-1): the scripted timelines of the match rules. Each test builds a trace with the [`Sim`] of `testkit` and compares it
//! with `features/golden/<name>.txt` (`UPDATE_GOLDEN=1` rewrites). The times are fake milliseconds; the steps sit on the debounce
//! edges (`DEAD_DEBOUNCE_MS` 799 / 800, `RIVAL_WINDOW_MS`, `OUT_DEAD_MS`, `GONE_MS`, `DECIDE_MS`, `FALLBACK_MS`, ...).

use super::testkit::*;

/// LOCAL (the host) and two rivals, every player on its own team.
fn ffa3(me_hp: u32, me_state: u8) -> WorldBuilder {
    WorldBuilder::new()
        .local(HOST, me_hp, me_state)
        .other(P2, 240, 0)
        .other(P3, 240, 0)
}

fn dead_me() -> WorldBuilder {
    ffa3(0, 0)
}

#[test]
fn rival_defeat_hold_and_defeat_exit() {
    let mut s = Sim::new("A rival's kill: defeat, hold, Esc menu, defeat screen");
    s.case("exit after 20 s, spectate off", 1020, 20, false);
    s.tick(0, &ffa3(240, 0));
    s.note("HP 0 without the downed state, a rival hit 2 s ago");
    let dead = dead_me().rival_hit(2_000);
    s.tick(100, &dead);
    s.tick(899, &dead); // 799 ms after HP 0: not a death yet
    s.tick(900, &dead); // 800 ms: judged
    s.tick(1_000, &dead);
    s.note("the hold has hidden the body; the menu unlocks MENU_AFTER_MS (9 s) after the defeat");
    s.tick(5_000, &dead.clone().held());
    s.tick(9_899, &dead.clone().held());
    s.tick(9_900, &dead.clone().held());
    s.note("the hold goes away and comes back: the menu follows it");
    s.tick(12_000, &dead);
    s.tick(12_500, &dead.clone().held());
    s.note("the defeat screen: 20 s after the defeat (900), once");
    s.tick(20_899, &dead.clone().held());
    s.tick(20_900, &dead.clone().held());
    s.tick(22_000, &dead.clone().held());

    s.case("defeat_exit_secs 0: no defeat screen, ever", 1020, 0, false);
    s.tick(0, &ffa3(240, 0));
    let dead = dead_me().rival_hit(1_000);
    s.tick(100, &dead);
    s.tick(900, &dead);
    s.tick(120_000, &dead.clone().held());

    s.case("a defeated LOCAL that stands again", 1020, 20, false);
    s.tick(0, &ffa3(240, 0));
    let dead = dead_me().rival_hit(500);
    s.tick(100, &dead);
    s.tick(900, &dead);
    s.tick(1_000, &dead.clone().held());
    s.note("a dev release: up again (state 0, HP > 0)");
    s.tick(2_000, &ffa3(120, 0));
    s.tick(2_100, &ffa3(120, 0));

    s.case("the match ends before the defeat screen", 1020, 20, false);
    s.tick(0, &ffa3(240, 0));
    let dead = dead_me().rival_hit(500);
    s.tick(100, &dead);
    s.tick(900, &dead);
    s.note(
        "P3 leaves: out after GONE_MS 6 s; P2 alone decides the match, the exit screen is never sent",
    );
    let only = |hp: u32, p3: bool| {
        let b = WorldBuilder::new()
            .local(HOST, hp, 0)
            .other(P2, 240, 0)
            .rival_hit(500);
        if p3 {
            b.other(P3, 240, 0)
        } else {
            b
        }
    };
    s.tick(1_000, &only(0, false));
    s.tick(6_999, &only(0, false));
    s.tick(7_000, &only(0, false));
    s.tick(7_999, &only(0, false));
    s.tick(8_000, &only(0, false));
    s.tick(20_900, &only(0, false));
    s.tick(30_000, &only(0, false));

    s.case(
        "downed is not dead; a shorter HP 0 is no death",
        1020,
        20,
        false,
    );
    s.tick(0, &ffa3(240, 0));
    let downed = ffa3(0, 1).rival_hit(100);
    s.tick(100, &downed);
    s.tick(60_000, &downed);
    s.note("HP 0 at 61000 for 799 ms, then downed again: still no judgment");
    let dead = dead_me().rival_hit(100);
    s.tick(61_000, &dead);
    s.tick(61_799, &dead);
    s.tick(61_800, &downed);
    s.tick(62_000, &dead);
    s.tick(62_799, &dead);
    s.tick(62_800, &dead);
    assert_golden("rival_defeat_hold_and_defeat_exit", &s.finish());
}

#[test]
fn defeat_judge_edges_rival_window_and_tide() {
    let mut s = Sim::new("Who killed LOCAL: the rival window, the tide, a mob");
    let judge = |s: &mut Sim, title: &str, b: WorldBuilder| {
        s.case(title, 1020, 20, false);
        s.tick(0, &b.clone().local(HOST, 240, 0).other(P2, 240, 0));
        let dead = |b: &WorldBuilder| b.clone().local(HOST, 0, 0).other(P2, 240, 0);
        s.tick(100, &dead(&b));
        s.tick(899, &dead(&b));
        s.tick(900, &dead(&b));
    };
    let day1 = WorldBuilder::new;
    let day2 = |h: u8, m: u8, sec: u8| {
        WorldBuilder::new()
            .flags(|f| f.day2 = true)
            .clock(h, m, sec, 72.9)
    };
    judge(
        &mut s,
        "rival hit 30000 ms ago = a rival",
        day1().rival_hit(30_000),
    );
    judge(
        &mut s,
        "rival hit 30001 ms ago = a mob's or a fall's death",
        day1().rival_hit(30_001),
    );
    judge(&mut s, "no hit at all = a mob's or a fall's death", day1());
    judge(&mut s, "a mob's hit = a mob's death", day1().mob_hit(500));
    judge(
        &mut s,
        "Day 1, hit with no attacker (not the tide)",
        day1().env_hit(1_000),
    );
    judge(
        &mut s,
        "Day 2 at 12:29:59, hit with no attacker (the circle has not started)",
        day2(12, 29, 59).env_hit(1_000),
    );
    judge(
        &mut s,
        "Day 2 at 12:30:00, hit with no attacker 10000 ms ago = the tide",
        day2(12, 30, 0).env_hit(10_000),
    );
    judge(
        &mut s,
        "Day 2 at 13:00:00, hit with no attacker 10001 ms ago = not the tide",
        day2(13, 0, 0).env_hit(10_001),
    );
    judge(
        &mut s,
        "Day 2 at 13:00:00, no clock reading is not the tide",
        day2(13, 0, 0).no_clock().env_hit(1_000),
    );
    judge(
        &mut s,
        "a rival hit wins over the tide",
        day2(14, 0, 0).rival_hit(29_000),
    );
    s.case(
        "tide defeat on Day 2: hold, menu, defeat screen, no item drop",
        1020,
        20,
        false,
    );
    let day2 = |hp: u32| {
        WorldBuilder::new()
            .flags(|f| f.day2 = true)
            .clock(13, 0, 0, 72.9)
            .local(HOST, hp, 0)
            .other(P2, 240, 0)
            .other(P3, 240, 0)
    };
    s.tick(0, &day2(240));
    let dead = day2(0).env_hit(4_000);
    s.tick(5_000, &dead);
    s.tick(5_799, &dead);
    s.tick(5_800, &dead);
    s.tick(6_000, &dead.clone().held());
    s.tick(14_799, &dead.clone().held());
    s.tick(14_800, &dead.clone().held());
    s.tick(25_799, &dead.clone().held());
    s.tick(25_800, &dead.clone().held());
    assert_golden("defeat_judge_edges_rival_window_and_tide", &s.finish());
}

/// Fighters 0..2 with positions: HOST dead at the origin, P2 far away, P3 near.
fn spec_world(p2_hp: u32, p3_hp: u32) -> WorldBuilder {
    WorldBuilder::new()
        .local(HOST, 0, 0)
        .rival_hit(100)
        .local_pos(Some(([0.0, 0.0, 0.0], true)))
        .other(P2, p2_hp, 0)
        .at([500.0, 10.0, 0.0])
        .other(P3, p3_hp, 0)
        .at([50.0, 12.0, 5.0])
}

#[test]
fn spectate_follow_switch_and_f10() {
    let mut s = Sim::new("Spectating a defeated body: follow, next / prev, F10");
    let alive = || {
        WorldBuilder::new()
            .local(HOST, 240, 0)
            .other(P2, 240, 0)
            .other(P3, 240, 0)
    };

    s.case(
        "spectate on, the hold never comes: follows after 12 s",
        1020,
        0,
        true,
    );
    s.tick(0, &alive());
    s.tick(100, &spec_world(240, 240));
    s.tick(900, &spec_world(240, 240)); // judged at 900
    s.tick(5_000, &spec_world(240, 240));
    s.tick(12_899, &spec_world(240, 240)); // 11999 ms after the defeat
    s.tick(12_900, &spec_world(240, 240)); // 12000 ms: follows the nearer P3
    s.tick(13_000, &spec_world(240, 240));

    s.case(
        "spectate on, held: switch next / prev, a target dies, nobody left, F10",
        1020,
        0,
        true,
    );
    s.tick(0, &alive());
    s.tick(100, &spec_world(240, 240).held());
    s.tick(900, &spec_world(240, 240).held()); // defeat; follows P3 at once
    s.tick(1_000, &spec_world(240, 240).held());
    s.note("P2 comes closer: the target stays P3");
    s.tick(1_100, &spec_world_p2_close());
    s.note("next: P3 -> wraps to P2");
    s.tick(1_200, &spec_world(240, 240).held().next());
    s.note("prev: P2 -> wraps to P3");
    s.tick(1_300, &spec_world(240, 240).held().prev());
    s.note("prev again: P3 -> P2");
    s.tick(1_400, &spec_world(240, 240).held().prev());
    s.note("next and prev in the same tick: next wins");
    s.tick(1_500, &spec_world(240, 240).held().next().prev());
    s.note("P3 (the target) dies: not watchable (HP 0), P2 is picked");
    s.tick(1_600, &spec_world(240, 0).held());
    s.tick(1_700, &spec_world(240, 0).held());
    s.note("P2 leaves the list too: nobody left to watch");
    let alone = || {
        WorldBuilder::new()
            .local(HOST, 0, 0)
            .rival_hit(100)
            .held()
            .local_pos(Some(([0.0, 0.0, 0.0], true)))
    };
    s.tick(1_800, &alone());
    s.tick(1_900, &alone());
    s.note("F10: the defeat screen, once; then no more follow");
    s.tick(2_000, &spec_world(240, 240).held().leave());
    s.tick(2_100, &spec_world(240, 240).held());

    s.case(
        "spectate off: the body stays; F10 still leaves",
        1020,
        0,
        false,
    );
    s.tick(0, &alive());
    s.tick(100, &spec_world(240, 240).held());
    s.tick(900, &spec_world(240, 240).held());
    s.tick(1_000, &spec_world(240, 240).held().next());
    s.tick(2_000, &spec_world(240, 240).held().leave());
    s.tick(2_100, &spec_world(240, 240).held().leave());

    s.case(
        "spectate on, no positions known: nobody to follow",
        1020,
        0,
        true,
    );
    s.tick(0, &alive());
    let blind = || {
        WorldBuilder::new()
            .local(HOST, 0, 0)
            .rival_hit(100)
            .held()
            .other(P2, 240, 0)
            .other(P3, 240, 0)
    };
    s.tick(100, &blind());
    s.tick(900, &blind());
    s.tick(1_000, &blind().next());

    s.case(
        "spectate on, a downed LOCAL is never dragged about",
        1020,
        0,
        true,
    );
    s.tick(0, &alive());
    s.tick(100, &spec_world(240, 240).held());
    s.tick(900, &spec_world(240, 240).held());
    s.note("LOCAL is revived into the downed state (HP 0, state 1): the defeat stays but no move");
    let downed = WorldBuilder::new()
        .local(HOST, 0, 1)
        .rival_hit(100)
        .held()
        .local_pos(Some(([0.0, 0.0, 0.0], true)))
        .other(P2, 240, 0)
        .at([500.0, 10.0, 0.0])
        .other(P3, 240, 0)
        .at([50.0, 12.0, 5.0]);
    s.tick(1_000, &downed);

    s.case(
        "spectate on, the match is over: no follow, F10 ignored",
        1020,
        0,
        true,
    );
    s.tick(0, &alive());
    s.tick(100, &spec_world(240, 240).held());
    s.tick(900, &spec_world(240, 240).held());
    s.note("P2 and P3 gone: the count decides at DECIDE_MS");
    let gone = || {
        WorldBuilder::new()
            .local(HOST, 0, 0)
            .rival_hit(100)
            .held()
            .local_pos(Some(([0.0, 0.0, 0.0], true)))
    };
    s.tick(1_000, &gone());
    s.tick(7_000, &gone());
    s.tick(8_000, &gone());
    s.tick(8_100, &gone().leave());
    assert_golden("spectate_follow_switch_and_f10", &s.finish());
}

/// Like [`spec_world`] with both rivals alive, but P2 1 m from the origin (closer than P3).
fn spec_world_p2_close() -> WorldBuilder {
    WorldBuilder::new()
        .local(HOST, 0, 0)
        .rival_hit(100)
        .held()
        .local_pos(Some(([0.0, 0.0, 0.0], true)))
        .other(P2, 240, 0)
        .at([1.0, 0.0, 0.0])
        .other(P3, 240, 0)
        .at([50.0, 12.0, 5.0])
}

/// Day-1 ground positions: `stand` ticks every second at `pos` from `from` to `to` (inclusive); a place is kept after 2 s
/// of slow movement, at most one per 5 s, at least 100 m from the others.
fn stand(s: &mut Sim, from: u64, to: u64, pos: [f32; 3]) {
    let mut t = from;
    while t <= to {
        s.tick(
            t,
            &WorldBuilder::new()
                .local(HOST, 240, 0)
                .local_pos(Some((pos, true)))
                .other(P2, 240, 0)
                .at([5_000.0, 0.0, 0.0]),
        );
        t += 1_000;
    }
}

/// LOCAL died to a mob at `pos`; P2 stands at `rival`.
fn mob_dead(pos: [f32; 3], rival: [f32; 3], hp: u32) -> WorldBuilder {
    WorldBuilder::new()
        .local(HOST, hp, 0)
        .local_pos(Some((pos, true)))
        .other(P2, 240, 0)
        .at(rival)
}

#[test]
fn mob_death_respawn_far() {
    let mut s =
        Sim::new("A mob's death: the game's respawn, then RW-7 moves LOCAL far from the others");
    s.case(
        "two Day-1 places, respawn next to a rival: moved to the farthest",
        1020,
        20,
        false,
    );
    stand(&mut s, 0, 3_000, [0.0, 0.0, 0.0]);
    stand(&mut s, 4_000, 8_000, [600.0, 0.0, 0.0]);
    s.note("a mob kills LOCAL at B; P2 stands 20 m away");
    let dead = mob_dead([600.0, 0.0, 0.0], [620.0, 0.0, 0.0], 0);
    s.tick(9_000, &dead);
    s.tick(9_799, &dead);
    s.tick(9_800, &dead);
    s.tick(15_000, &dead);
    s.note("the game's own respawn (about 14 s): HP back at 24000");
    let up = mob_dead([590.0, 0.0, 0.0], [620.0, 0.0, 0.0], 240);
    s.tick(24_000, &up);
    s.tick(26_999, &up);
    s.tick(27_000, &up); // 3000 ms standing: moved
    s.tick(28_000, &up);

    s.case(
        "respawned already 300 m or more from every player: no move",
        1020,
        20,
        false,
    );
    stand(&mut s, 0, 3_000, [0.0, 0.0, 0.0]);
    stand(&mut s, 4_000, 8_000, [600.0, 0.0, 0.0]);
    let dead = mob_dead([600.0, 0.0, 0.0], [5_000.0, 0.0, 0.0], 0);
    s.tick(9_000, &dead);
    s.tick(9_800, &dead);
    let up = mob_dead([600.0, 0.0, 0.0], [5_000.0, 0.0, 0.0], 240);
    s.tick(24_000, &up);
    s.tick(27_000, &up);

    s.case(
        "respawned 299 m from the nearest player: moved when a place is better",
        1020,
        20,
        false,
    );
    stand(&mut s, 0, 3_000, [0.0, 0.0, 0.0]);
    stand(&mut s, 4_000, 8_000, [600.0, 0.0, 0.0]);
    let dead = mob_dead([600.0, 0.0, 0.0], [899.0, 0.0, 0.0], 0);
    s.tick(9_000, &dead);
    s.tick(9_800, &dead);
    let up = mob_dead([600.0, 0.0, 0.0], [899.0, 0.0, 0.0], 240);
    s.tick(24_000, &up);
    s.tick(27_000, &up);

    s.case(
        "no place recorded (died right away): the game's point stays",
        1020,
        20,
        false,
    );
    let dead = mob_dead([600.0, 0.0, 0.0], [620.0, 0.0, 0.0], 0);
    s.tick(0, &mob_dead([600.0, 0.0, 0.0], [620.0, 0.0, 0.0], 240));
    s.tick(1_000, &dead);
    s.tick(1_800, &dead);
    let up = mob_dead([600.0, 0.0, 0.0], [620.0, 0.0, 0.0], 240);
    s.tick(15_000, &up);
    s.tick(18_000, &up);

    s.case(
        "Day 2 after the circle starts: respawn left to the game",
        1020,
        20,
        false,
    );
    stand(&mut s, 0, 3_000, [0.0, 0.0, 0.0]);
    stand(&mut s, 4_000, 8_000, [600.0, 0.0, 0.0]);
    let d2 = |hp: u32| {
        mob_dead([600.0, 0.0, 0.0], [620.0, 0.0, 0.0], hp)
            .flags(|f| f.day2 = true)
            .clock(13, 0, 0, 72.9)
    };
    s.tick(9_000, &d2(0));
    s.tick(9_800, &d2(0));
    s.tick(24_000, &d2(240));
    s.tick(27_000, &d2(240));

    s.case(
        "Day 2 morning (08:00): the circle is not open, moved as on Day 1",
        1020,
        20,
        false,
    );
    stand(&mut s, 0, 3_000, [0.0, 0.0, 0.0]);
    stand(&mut s, 4_000, 8_000, [600.0, 0.0, 0.0]);
    let d2 = |hp: u32| {
        mob_dead([600.0, 0.0, 0.0], [620.0, 0.0, 0.0], hp)
            .flags(|f| f.day2 = true)
            .clock(8, 0, 0, 72.9)
    };
    s.tick(9_000, &d2(0));
    s.tick(9_800, &d2(0));
    s.tick(24_000, &d2(240));
    s.tick(27_000, &d2(240));

    s.case(
        "Day 2 without a clock reading counts as an open circle",
        1020,
        20,
        false,
    );
    stand(&mut s, 0, 3_000, [0.0, 0.0, 0.0]);
    stand(&mut s, 4_000, 8_000, [600.0, 0.0, 0.0]);
    let d2 = |hp: u32| {
        mob_dead([600.0, 0.0, 0.0], [620.0, 0.0, 0.0], hp)
            .flags(|f| f.day2 = true)
            .no_clock()
    };
    s.tick(9_000, &d2(0));
    s.tick(9_800, &d2(0));
    s.tick(24_000, &d2(240));
    s.tick(27_000, &d2(240));

    s.case(
        "standing up resets the 3 s wait (a second death in between)",
        1020,
        20,
        false,
    );
    stand(&mut s, 0, 3_000, [0.0, 0.0, 0.0]);
    stand(&mut s, 4_000, 8_000, [600.0, 0.0, 0.0]);
    let dead = mob_dead([600.0, 0.0, 0.0], [620.0, 0.0, 0.0], 0);
    let up = mob_dead([600.0, 0.0, 0.0], [620.0, 0.0, 0.0], 240);
    s.tick(9_000, &dead);
    s.tick(9_800, &dead);
    s.tick(20_000, &up);
    s.tick(22_000, &dead);
    s.tick(23_000, &up);
    s.tick(25_999, &up);
    s.tick(26_000, &up);
    assert_golden("mob_death_respawn_far", &s.finish());
}

/// One 2-player expedition tick: LOCAL and P2, with the given HP / state.
fn duel(me: (u32, u8), p2: Option<(u32, u8)>) -> WorldBuilder {
    let b = WorldBuilder::new().local(HOST, me.0, me.1);
    match p2 {
        Some((hp, st)) => b.other(P2, hp, st),
        None => b,
    }
}

#[test]
fn match_two_players_win_and_loss() {
    let mut s = Sim::new("2 players: the last team standing decides the match");
    s.case(
        "win: the rival dies and stays dead for OUT_DEAD_MS (30 s)",
        1020,
        20,
        false,
    );
    s.tick(0, &duel((240, 0), Some((240, 0))));
    let rival_dead = duel((240, 0), Some((0, 0)));
    s.tick(1_000, &rival_dead);
    s.tick(30_999, &rival_dead); // 29999 ms dead
    s.tick(31_000, &rival_dead); // 30000 ms dead: out
    s.tick(31_999, &rival_dead); // 999 ms alone
    s.tick(32_000, &rival_dead); // 1000 ms alone: decided, win
    s.tick(33_000, &rival_dead);

    s.case(
        "win: the rival leaves the player list (GONE_MS 6 s)",
        1020,
        20,
        false,
    );
    s.tick(0, &duel((240, 0), Some((240, 0))));
    s.tick(1_000, &duel((240, 0), None));
    s.tick(6_999, &duel((240, 0), None));
    s.tick(7_000, &duel((240, 0), None));
    s.tick(7_999, &duel((240, 0), None));
    s.tick(8_000, &duel((240, 0), None));

    s.case("a downed rival is still standing", 1020, 20, false);
    s.tick(0, &duel((240, 0), Some((240, 0))));
    s.tick(1_000, &duel((240, 0), Some((0, 1))));
    s.tick(120_000, &duel((240, 0), Some((0, 1))));

    s.case(
        "one stray reading never ends it: the rival is back before DECIDE_MS",
        1020,
        20,
        false,
    );
    s.tick(0, &duel((240, 0), Some((240, 0))));
    s.tick(1_000, &duel((240, 0), None));
    s.tick(7_000, &duel((240, 0), None)); // out; alone since 7000
    s.tick(7_500, &duel((240, 0), Some((240, 0)))); // back
    s.tick(8_000, &duel((240, 0), Some((240, 0))));
    s.tick(9_000, &duel((240, 0), Some((240, 0))));

    s.case(
        "loss: LOCAL is defeated by the rival; the count decides before the defeat screen",
        1020,
        20,
        false,
    );
    s.tick(0, &duel((240, 0), Some((240, 0))));
    let dead = duel((0, 0), Some((240, 0))).rival_hit(100);
    s.tick(100, &dead);
    s.tick(899, &dead);
    s.tick(900, &dead); // defeated: LOCAL is out at once, alone since 900
    s.tick(1_899, &dead);
    s.tick(1_900, &dead); // decided: the rival's team won, FLAG_LOSE from the count
    s.tick(20_900, &dead); // the exit screen is not sent any more
    s.tick(25_000, &dead);

    s.case(
        "loss: the defeat screen went first, the count adds no second flag",
        1020,
        1,
        false,
    );
    s.tick(0, &duel((240, 0), Some((240, 0))));
    s.tick(100, &dead);
    s.tick(900, &dead);
    s.tick(1_899, &dead);
    s.tick(1_900, &dead);
    s.tick(2_900, &dead);

    s.case(
        "nobody left standing: both are out at once",
        1020,
        20,
        false,
    );
    s.tick(0, &duel((240, 0), Some((240, 0))));
    s.tick(1_000, &duel((0, 0), Some((0, 0))));
    s.tick(30_999, &duel((0, 0), Some((0, 0))));
    s.tick(31_000, &duel((0, 0), Some((0, 0))));
    s.tick(31_999, &duel((0, 0), Some((0, 0))));
    s.tick(32_000, &duel((0, 0), Some((0, 0))));

    s.case(
        "night_count 0: the match never ends by count",
        1020,
        20,
        false,
    );
    s.count_off(true);
    s.tick(0, &duel((240, 0), Some((240, 0))));
    s.tick(1_000, &duel((240, 0), None));
    s.tick(60_000, &duel((240, 0), None));
    s.count_off(false);
    s.tick(61_000, &duel((240, 0), None));
    s.tick(62_000, &duel((240, 0), None));

    s.case(
        "a client's game: the same win, no 'host' in the line",
        1020,
        20,
        false,
    );
    let c = |p: Option<(u32, u8)>| {
        let b = WorldBuilder::new().local(P2, 240, 0);
        match p {
            Some((hp, st)) => b.other(HOST, hp, st),
            None => b,
        }
    };
    s.tick(0, &c(Some((240, 0))));
    s.tick(1_000, &c(None));
    s.tick(7_000, &c(None));
    s.tick(8_000, &c(None));

    s.case(
        "alone from the start: fewer than two teams were seen, nothing is decided",
        1020,
        20,
        false,
    );
    s.tick(0, &duel((240, 0), None));
    s.tick(60_000, &duel((240, 0), None));
    assert_golden("match_two_players_win_and_loss", &s.finish());
}

#[test]
fn match_four_players_ffa_and_two_vs_two() {
    let mut s = Sim::new("4 players: free for all and 2 v 2");
    let ffa =
        |me: (u32, u8), p2: Option<(u32, u8)>, p3: Option<(u32, u8)>, p4: Option<(u32, u8)>| {
            let mut b = WorldBuilder::new().local(HOST, me.0, me.1);
            for (id, p) in [(P2, p2), (P3, p3), (P4, p4)] {
                if let Some((hp, st)) = p {
                    b = b.other(id, hp, st);
                }
            }
            b
        };
    let up = Some((240, 0));
    s.case("FFA win: P2 dies, P3 leaves, P4 is dead", 1020, 20, false);
    s.tick(0, &ffa((240, 0), up, up, up));
    s.tick(1_000, &ffa((240, 0), Some((0, 0)), up, up));
    s.tick(2_000, &ffa((240, 0), Some((0, 0)), None, up));
    s.tick(3_000, &ffa((240, 0), Some((0, 0)), None, Some((0, 0))));
    s.tick(8_000, &ffa((240, 0), Some((0, 0)), None, Some((0, 0)))); // P3 out
    s.tick(31_000, &ffa((240, 0), Some((0, 0)), None, Some((0, 0)))); // P2 out
    s.tick(33_000, &ffa((240, 0), Some((0, 0)), None, Some((0, 0)))); // P4 out: alone
    s.tick(33_999, &ffa((240, 0), Some((0, 0)), None, Some((0, 0))));
    s.tick(34_000, &ffa((240, 0), Some((0, 0)), None, Some((0, 0))));

    s.case(
        "FFA loss: LOCAL defeated, the defeat screen first, P2 wins later (no second flag)",
        1020,
        20,
        false,
    );
    s.tick(0, &ffa((240, 0), up, up, up));
    let dead =
        |p3: Option<(u32, u8)>, p4: Option<(u32, u8)>| ffa((0, 0), up, p3, p4).rival_hit(100);
    s.tick(100, &dead(up, up));
    s.tick(900, &dead(up, up));
    s.tick(2_000, &dead(None, up));
    s.tick(3_000, &dead(None, Some((0, 0))));
    s.tick(8_000, &dead(None, Some((0, 0))));
    s.tick(20_900, &dead(None, Some((0, 0)))); // the defeat screen
    s.tick(33_000, &dead(None, Some((0, 0)))); // P4 out
    s.tick(34_000, &dead(None, Some((0, 0)))); // decided: P2 wins, LOCAL already lost

    s.case(
        "FFA: LOCAL dies to a mob and comes back; the others still count it",
        1020,
        20,
        false,
    );
    s.tick(0, &ffa((240, 0), up, up, up));
    s.tick(1_000, &ffa((0, 0), up, up, up));
    s.tick(1_800, &ffa((0, 0), up, up, up));
    s.tick(15_000, &ffa((240, 0), up, up, up));
    s.tick(60_000, &ffa((240, 0), up, up, up));

    let tm =
        |me: (u32, u8), p2: Option<(u32, u8)>, p3: Option<(u32, u8)>, p4: Option<(u32, u8)>| {
            let mut b = WorldBuilder::new().local_in(HOST, TEAM_A, me.0, me.1);
            if let Some((hp, st)) = p2 {
                b = b.other_in(P2, TEAM_A, hp, st);
            }
            if let Some((hp, st)) = p3 {
                b = b.other_in(P3, TEAM_B, hp, st);
            }
            if let Some((hp, st)) = p4 {
                b = b.other_in(P4, TEAM_B, hp, st);
            }
            b
        };
    s.case("2 v 2 win: the other team is out", 1020, 20, false);
    s.tick(0, &tm((240, 0), up, up, up));
    s.tick(1_000, &tm((240, 0), up, Some((0, 0)), up));
    s.tick(1_500, &tm((240, 0), up, Some((0, 0)), None));
    s.tick(7_500, &tm((240, 0), up, Some((0, 0)), None)); // P4 out
    s.tick(30_999, &tm((240, 0), up, Some((0, 0)), None));
    s.tick(31_000, &tm((240, 0), up, Some((0, 0)), None)); // P3 out: team A alone
    s.tick(31_999, &tm((240, 0), up, Some((0, 0)), None));
    s.tick(32_000, &tm((240, 0), up, Some((0, 0)), None));

    s.case(
        "2 v 2 loss: LOCAL defeated, the downed mate keeps team A in, then dies",
        1020,
        20,
        false,
    );
    s.tick(0, &tm((240, 0), up, up, up));
    let loss = |p2: Option<(u32, u8)>| tm((0, 0), p2, up, up).rival_hit(100);
    s.tick(100, &loss(Some((0, 1))));
    s.tick(900, &loss(Some((0, 1))));
    s.tick(20_900, &loss(Some((0, 1)))); // the defeat screen
    s.tick(30_000, &loss(Some((0, 0)))); // the mate dies for good
    s.tick(59_999, &loss(Some((0, 0))));
    s.tick(60_000, &loss(Some((0, 0)))); // out: team B alone since now
    s.tick(60_999, &loss(Some((0, 0))));
    s.tick(61_000, &loss(Some((0, 0)))); // decided: B wins, LOCAL already lost

    s.case(
        "2 v 2: LOCAL's mate is still standing when LOCAL is defeated: no decision",
        1020,
        20,
        false,
    );
    s.tick(0, &tm((240, 0), up, up, up));
    s.tick(100, &loss(up));
    s.tick(900, &loss(up));
    s.tick(5_000, &loss(up));
    assert_golden("match_four_players_ffa_and_two_vs_two", &s.finish());
}

/// A Day-1 expedition tick at a clock reading.
fn day1_at(local: u64, other: u64, h: u8, m: u8, sec: u8, speed: f32) -> WorldBuilder {
    WorldBuilder::new()
        .local(local, 240, 0)
        .other(other, 240, 0)
        .clock(h, m, sec, speed)
}

#[test]
fn day1_free_host_client_and_day2_handover() {
    let mut s = Sim::new("RW-4: the free Day 1, host vs client, and the Day 2 handover");
    let t = DAY1_1020_SPEED;
    let vanilla = 72.9;

    s.case(
        "host: slow the clock, set night-1-done at 11:50, restore on Day 2",
        1020,
        20,
        false,
    );
    s.tick(0, &day1_at(HOST, P2, 7, 0, 0, vanilla));
    s.tick(100, &day1_at(HOST, P2, 7, 0, 0, vanilla)); // the write has not landed: asked again, no new line
    s.tick(1_000, &day1_at(HOST, P2, 7, 0, 1, t));
    s.tick(2_000, &day1_at(HOST, P2, 11, 49, 59, t));
    s.tick(3_000, &day1_at(HOST, P2, 11, 50, 0, t)); // the host ends Day 1
    s.tick(4_000, &day1_at(HOST, P2, 11, 50, 1, t)); // once only
    s.note("the game's night-1-done flag is up (Day 2 not yet)");
    let n1 = |h, m, sec| day1_at(HOST, P2, h, m, sec, t).flags(|f| f.night1_done = true);
    s.tick(5_000, &n1(11, 50, 2));
    s.note("Day 2 begins, the clock is still 11:51: the slow speed stays");
    let d2 = |h, m, sec, speed| {
        day1_at(HOST, P2, h, m, sec, speed).flags(|f| {
            f.night1_done = true;
            f.day2 = true;
        })
    };
    s.tick(6_000, &d2(11, 51, 0, t));
    s.tick(7_000, &d2(12, 19, 59, t)); // before the 12:20 guard: still slow
    s.note("the game's morning reset: the clock is back at 07:00");
    s.tick(8_000, &d2(7, 0, 0, t));
    s.tick(9_000, &d2(7, 0, 5, vanilla));

    s.case(
        "client: slows the clock, never sets 7502, restores at the 12:20 guard",
        1020,
        20,
        false,
    );
    s.tick(0, &day1_at(P2, HOST, 7, 0, 0, vanilla));
    s.tick(1_000, &day1_at(P2, HOST, 7, 0, 1, t));
    s.tick(2_000, &day1_at(P2, HOST, 11, 50, 0, t)); // a client sets nothing
    s.tick(3_000, &day1_at(P2, HOST, 11, 55, 0, t));
    let c2 = |h, m, sec, speed| {
        day1_at(P2, HOST, h, m, sec, speed).flags(|f| {
            f.night1_done = true;
            f.day2 = true;
        })
    };
    s.tick(4_000, &c2(11, 56, 0, t));
    s.tick(5_000, &c2(12, 19, 59, t));
    s.tick(6_000, &c2(12, 20, 0, t)); // the reset never came: speed back at the guard
    s.tick(7_000, &c2(12, 20, 1, vanilla));

    let h2 = |h, m, sec, speed| {
        day1_at(HOST, P2, h, m, sec, speed).flags(|f| {
            f.night1_done = true;
            f.day2 = true;
        })
    };
    s.case(
        "Day 2 handover edges: 10:00:00 does not restore, 09:59:59 does",
        1020,
        20,
        false,
    );
    s.tick(0, &day1_at(HOST, P2, 7, 0, 0, vanilla));
    s.tick(1_000, &day1_at(HOST, P2, 7, 0, 1, t));
    s.tick(2_000, &h2(10, 0, 0, t));
    s.tick(3_000, &h2(9, 59, 59, t));
    s.tick(4_000, &h2(9, 59, 59, vanilla));

    s.case(
        "joined after the slowdown (no vanilla speed seen): restores 72.9",
        1020,
        20,
        false,
    );
    s.tick(0, &day1_at(HOST, P2, 8, 0, 0, t));
    s.tick(1_000, &day1_at(HOST, P2, 8, 30, 0, t));
    s.tick(2_000, &h2(7, 0, 0, t));

    s.case(
        "the speed drifts: asked again without a new line",
        1020,
        20,
        false,
    );
    s.tick(0, &day1_at(HOST, P2, 8, 0, 0, vanilla));
    s.tick(1_000, &day1_at(HOST, P2, 8, 0, 1, t));
    s.tick(2_000, &day1_at(HOST, P2, 8, 0, 2, 40.0));
    s.tick(3_000, &day1_at(HOST, P2, 8, 0, 3, t + 0.04));

    s.case(
        "the night flag on Day 1 with the clock past 11:50: Day 1 ends at once",
        1020,
        20,
        false,
    );
    s.tick(0, &day1_at(HOST, P2, 7, 0, 0, vanilla));
    s.tick(
        1_000,
        &day1_at(HOST, P2, 23, 59, 59, t).flags(|f| f.night = true),
    );

    s.case("day1_secs 0: the free day is off", 0, 20, false);
    s.tick(0, &day1_at(HOST, P2, 7, 0, 0, vanilla));
    s.tick(1_000, &day1_at(HOST, P2, 11, 50, 0, vanilla));
    s.tick(2_000, &h2(7, 0, 0, vanilla));

    s.case("no clock reading: nothing to do", 1020, 20, false);
    s.tick(0, &day1_at(HOST, P2, 7, 0, 0, vanilla).no_clock());

    s.case("the match is decided: Day 1 is left alone", 1020, 20, false);
    s.tick(0, &day1_at(HOST, P2, 7, 0, 0, vanilla));
    s.tick(
        1_000,
        &WorldBuilder::new().local(HOST, 240, 0).clock(8, 0, 0, t),
    );
    s.tick(
        2_000,
        &WorldBuilder::new().local(HOST, 240, 0).clock(8, 0, 1, t),
    );
    s.tick(
        7_000,
        &WorldBuilder::new()
            .local(HOST, 240, 0)
            .clock(8, 0, 2, vanilla),
    );
    s.tick(
        8_000,
        &WorldBuilder::new()
            .local(HOST, 240, 0)
            .clock(8, 0, 3, vanilla),
    );
    s.tick(
        9_000,
        &WorldBuilder::new()
            .local(HOST, 240, 0)
            .clock(8, 0, 4, vanilla),
    );
    assert_golden("day1_free_host_client_and_day2_handover", &s.finish());
}

#[test]
fn event_file_fallback_writes_the_games_flags() {
    let mut s =
        Sim::new("The event file is not loaded: our flag stays ON, the game's flag is written");
    let w = |win: bool, lose: bool| {
        duel((240, 0), Some((240, 0))).flags(|f| {
            f.our_win = win;
            f.our_lose = lose;
        })
    };
    s.case(
        "win flag ON for good: written at FALLBACK_MS, again every FALLBACK_MS",
        1020,
        20,
        false,
    );
    s.tick(0, &w(false, false));
    s.tick(1_000, &w(true, false));
    s.tick(2_499, &w(true, false)); // 1499 ms
    s.tick(2_500, &w(true, false)); // 1500 ms: fallback
    s.tick(3_000, &w(true, false));
    s.tick(4_000, &w(true, false));
    s.tick(4_500, &w(true, false)); // 1500 ms after re-arming
    s.note("the event file did work this time: the flag goes OFF");
    s.tick(5_000, &w(false, false));

    s.case(
        "lose flag, and a flag that goes OFF in time",
        1020,
        20,
        false,
    );
    s.tick(0, &w(false, false));
    s.tick(1_000, &w(false, true));
    s.tick(2_499, &w(false, true));
    s.tick(2_500, &w(false, false)); // off at 1500 ms is not 'stuck': the timer was reset by this tick
    s.tick(3_000, &w(false, true));
    s.tick(4_500, &w(false, true));

    s.case("both flags at once", 1020, 20, false);
    s.tick(0, &w(true, true));
    s.tick(1_499, &w(true, true));
    s.tick(1_500, &w(true, true));

    s.case(
        "a client's own flags go through the same fallback",
        1020,
        20,
        false,
    );
    let c = |lose: bool| {
        WorldBuilder::new()
            .local(P2, 240, 0)
            .other(HOST, 240, 0)
            .flags(|f| f.our_lose = lose)
    };
    s.tick(0, &c(true));
    s.tick(1_500, &c(true));
    assert_golden("event_file_fallback_writes_the_games_flags", &s.finish());
}

#[test]
fn leaving_the_expedition_resets_the_rules() {
    let mut s = Sim::new("Back in the Roundtable (or a load): the rules reset");
    s.case(
        "defeated and held, the menu open, our die flag ON",
        1020,
        20,
        true,
    );
    s.tick(0, &ffa3(240, 0));
    let dead = dead_me().rival_hit(500);
    s.tick(100, &dead);
    s.tick(900, &dead.clone().held());
    s.tick(9_900, &dead.clone().held());
    s.note("Roundtable: hold off, menu off, one line (the night flags are not read here)");
    let rt = dead_me().roundtable();
    s.tick(10_000, &rt);
    s.tick(10_100, &rt);
    s.note("a new expedition starts clean: the die flag left ON in the save goes off (FS7-6), a mob death is judged again, a team count starts again");
    s.tick(20_000, &ffa3(240, 0).flags(|f| f.our_die = true));
    s.tick(21_000, &ffa3(0, 0));
    s.tick(21_800, &ffa3(0, 0));

    s.case(
        "Roundtable without ever having been in an expedition: silent",
        1020,
        20,
        false,
    );
    s.tick(0, &ffa3(240, 0).roundtable());
    s.tick(100, &ffa3(240, 0).roundtable());

    s.case(
        "night_count 0 survives the reset; seen players and the match decision do not",
        1020,
        20,
        false,
    );
    s.count_off(true);
    s.tick(0, &duel((240, 0), Some((240, 0))));
    s.tick(1_000, &duel((240, 0), None));
    s.tick(2_000, &duel((240, 0), None).roundtable());
    s.tick(3_000, &duel((240, 0), None));
    s.tick(60_000, &duel((240, 0), None));
    s.count_off(false);
    s.tick(61_000, &duel((240, 0), None));
    s.note("one team was seen since the reset: nothing to decide");
    s.tick(62_000, &duel((240, 0), None));

    s.case(
        "a match that was decided: the next expedition can be decided again",
        1020,
        20,
        false,
    );
    s.tick(0, &duel((240, 0), Some((240, 0))));
    s.tick(1_000, &duel((240, 0), None));
    s.tick(7_000, &duel((240, 0), None));
    s.tick(8_000, &duel((240, 0), None));
    s.tick(9_000, &duel((240, 0), None).roundtable());
    s.tick(10_000, &duel((240, 0), Some((240, 0))));
    s.tick(11_000, &duel((240, 0), None));
    s.tick(17_000, &duel((240, 0), None));
    s.tick(18_000, &duel((240, 0), None));

    s.case(
        "ticks without the flags or without LOCAL in the list decide nothing and forget nothing",
        1020,
        20,
        false,
    );
    s.tick(0, &ffa3(240, 0));
    s.tick(100, &ffa3(240, 0).no_flags());
    s.tick(
        200,
        &WorldBuilder::new().other(P2, 240, 0).other(P3, 240, 0),
    );
    s.tick(300, &WorldBuilder::new());
    let dead = dead_me().rival_hit(100);
    s.tick(400, &dead);
    s.tick(1_200, &dead);
    assert_golden("leaving_the_expedition_resets_the_rules", &s.finish());
}

#[test]
fn every_act_variant_appears_in_the_goldens() {
    // A guard for the coverage list of P2-1: each Act variant must be produced by a timeline above. Checked on the golden
    // texts so the list cannot rot silently.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/features/golden");
    let mut all = String::new();
    for e in std::fs::read_dir(&dir).expect("golden dir").flatten() {
        if e.path().extension().is_some_and(|x| x == "txt") {
            all.push_str(&std::fs::read_to_string(e.path()).expect("golden"));
        }
    }
    for v in [
        "HawkRespawn(",
        "Follow(",
        "GameFlag(",
        "DeathHold(true)",
        "DeathHold(false)",
        "ClockSpeed(",
        "MenuGate(true)",
        "MenuGate(false)",
        "DropItems",
    ] {
        assert!(all.contains(v), "no golden shows {v}");
    }
}
