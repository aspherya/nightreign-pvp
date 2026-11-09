//! T-035 rule v1 (a rival's completed revive downs the player again) and the T-039 downed rule (a rival's hit never keeps bar
//! progress and takes seconds off the bleed-out), driven by [`Core`] with an explicit clock.

use std::collections::{HashMap, HashSet};

use super::*;
use crate::game::mem::is_ptr;
use crate::overlay::shared::Relation;

// ---------------------------------------------------------------------------------------------------
// Rival revive rule (T-035)
// ---------------------------------------------------------------------------------------------------

/// `rival_revive = "redown" | "vanilla"` (the player build always uses `Redown`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RevivePolicy {
    /// A rival completing the revive meter downs the player again at once (rule v1).
    #[default]
    Redown,
    /// No rule: revives work as in vanilla (dev comparison tests).
    Vanilla,
}

impl RevivePolicy {
    pub(crate) fn label(self) -> &'static str {
        match self {
            RevivePolicy::Redown => "redown",
            RevivePolicy::Vanilla => "vanilla",
        }
    }
}

// HP chain: game::layout (R2), MEMORY_MAP §5 (FS-0 runs 7 and 9).
pub(crate) use crate::game::layout::HP_FIELD as HP_PTR_FIELD;
pub(crate) use crate::game::layout::PLAYER_HP_MIRROR as OFF_HP_MIRROR;
pub(crate) use crate::game::layout::PLAYER_HP_PTR as OFF_HP_PTR;
// T-040: the character data module route (MEMORY_MAP §5, T-040 run 1).
pub(crate) use crate::game::layout::{
    DATA_MODULE_HP as DM_HP, DATA_MODULE_MAX_HP as DM_MAX_HP, GAMEDATA_MAX_HP as GD_MAX_HP,
    PLAYER_MODULES,
};

/// In the player build the rule's log lines are limited to one per this many milliseconds (the write itself is not limited).
pub(crate) const PLAYER_LOG_MS: u64 = 5000;

/// T-040: how long a failed re-down is retried (the state byte must read 0 and the gates must pass).
pub(crate) const REDOWN_RETRY_MS: u64 = 2000;
/// T-040: at most this many `T-040 LOCAL ...` diagnostic lines per [`DIAG_WINDOW_MS`] (the rest are counted).
pub(crate) const DIAG_MAX_LINES: u32 = 30;
pub(crate) const DIAG_WINDOW_MS: u64 = 60_000;

/// T-040: a re-down that failed at the UP event and is retried every step until `deadline_ms`.
#[derive(Clone, Debug)]
pub(crate) struct PendingRedown {
    pub(super) addr: usize,
    pub(super) first_ms: u64,
    pub(super) deadline_ms: u64,
    pub(super) tries: u32,
    pub(super) first_err: String,
    pub(super) last_err: String,
}

/// T-040: the HP field of LOCAL's character data module, `[[PlayerIns+0x1B8]+0x0]+0x140` (MEMORY_MAP section 5, NR 1.3.3.0,
/// T-040 run 1: read on both Nucleus games, writing 0 downed the player). Gate: HP > 0 and equal to PlayerGameData's HP, and the
/// module's max HP equal to PlayerGameData's max HP. `Ok((field address, hp))`.
pub(crate) fn data_module_hp(mem: &dyn CombatMem, addr: usize) -> Result<(usize, u32), String> {
    let bag = mem
        .read_u64(addr + PLAYER_MODULES)
        .and_then(|v| usize::try_from(v).ok())
        .filter(|&v| is_ptr(v))
        .ok_or("no module list")?;
    let dm = mem
        .read_u64(bag)
        .and_then(|v| usize::try_from(v).ok())
        .filter(|&v| is_ptr(v))
        .ok_or("no data module")?;
    let g = gamedata_of(mem, addr).ok_or("no PlayerGameData")?;
    let (hp, max) = (mem.read_u32(dm + DM_HP), mem.read_u32(dm + DM_MAX_HP));
    let (ghp, gmax) = (mem.read_u32(g + GD_HP), mem.read_u32(g + GD_MAX_HP));
    match (hp, max, ghp, gmax) {
        // `god 1` raises only the module's maximum (the game recomputes PlayerGameData's), so that value is accepted too: without
        // it `hp0` was refused after `god 1` (2026-11-01, scenario `defeat-rival`).
        (Some(h), Some(m), Some(gh), Some(gm))
            if h > 0 && h == gh && (m == gm || m == super::GOD_MAX_HP) =>
        {
            Ok((dm + DM_HP, h))
        }
        _ => Err(format!(
            "gate failed (hp {}, max {}, PlayerGameData {} / {})",
            opt(hp),
            opt(max),
            opt(ghp),
            opt(gmax)
        )),
    }
}

/// The write gate: the authoritative value must be non-zero and equal to the mirror or the PlayerGameData HP (at least one must
/// agree). Everything else means the offsets do not describe this build and nothing is written.
pub(crate) fn hp_gate(auth: Option<u32>, mirror: Option<u32>, gamedata: Option<u32>) -> bool {
    match auth {
        Some(a) if a > 0 => mirror == Some(a) || gamedata == Some(a),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------------
// Core: tracker + rules, driven by an explicit clock
// ---------------------------------------------------------------------------------------------------

pub(crate) struct Core {
    pub(super) dev: bool,
    pub(super) policy: RevivePolicy,
    pub(super) tracker: Tracker,
    pub(super) last_rule_log_ms: Option<u64>,
    pub(super) suppressed: u32,
    /// T-039: seconds taken off the bleed-out clock per rival hit.
    pub(super) hit_seconds: f32,
    pub(super) dn: Option<DownedMem>,
    pub(super) downed_line: Option<String>,
    /// Dev: last logged (state byte, revive bar) per tracked player, see [`Core::watch_block`].
    pub(super) block_watch: HashMap<usize, (u8, f32)>,
    /// Downed solo rivals (remote copies): the bar value before any hit, see [`Core::remote_step`].
    pub(super) remote_bars: HashMap<usize, f32>,
    /// Try the re-down as soon as a rival's hit starts LOCAL's get-up (state 2) instead of waiting for state 0, see
    /// [`Core::getup_step`]. EXPERIMENT, off: the effect of the store during state 2 is UNKNOWN (FS-0 run 20 never reached a get-up).
    pub(crate) early_redown: bool,
    /// Nothing (more) to do for a get-up: no down was seen before it, the store was made, or the reviver is not a rival.
    pub(super) getup_done: bool,
    pub(super) getup_gate_logged: bool,
    /// T-040 follow-up: when the current get-up (state 2) was first seen, for the early re-down timing line.
    pub(super) getup_since_ms: Option<u64>,
    pub(super) last_dn_log_ms: Option<u64>,
    /// Rule writes made this session.
    pub(crate) fired: u32,
    /// T-040: the re-down that is being retried, see [`Core::pending_step`].
    pub(super) pending: Option<PendingRedown>,
    /// T-040: LOCAL's last seen (PlayerIns, state byte), for the `T-040 LOCAL state` lines.
    pub(super) diag_state: Option<(usize, u8)>,
    pub(super) diag_window_ms: u64,
    pub(super) diag_count: u32,
    pub(super) diag_suppressed: u32,
    /// T-040 (console `revive_fail`; only the dev console ever sets it): the next n `try_rule` calls fail without reading memory.
    pub(super) forced_fails: u32,
    /// M5-5: lowest clock the rival-hit penalty writes ([`CLOCK_FLOOR`] by day; higher at night, so the night match sees the
    /// bleed-out end before the game kills the player).
    pub(super) clock_floor: f32,
    /// M5-6: rule v1 and its retries stay off until this time (our own morning get-up must not be taken for a rival's revive).
    pub(super) exempt_until_ms: u64,
}

impl Core {
    pub(crate) fn new(dev: bool, policy: RevivePolicy) -> Self {
        Core {
            dev,
            policy: if dev { policy } else { RevivePolicy::Redown },
            tracker: Tracker::default(),
            last_rule_log_ms: None,
            suppressed: 0,
            hit_seconds: DEFAULT_RIVAL_HIT_SECONDS,
            dn: None,
            downed_line: None,
            block_watch: HashMap::new(),
            remote_bars: HashMap::new(),
            early_redown: false,
            getup_done: true,
            getup_gate_logged: false,
            getup_since_ms: None,
            last_dn_log_ms: None,
            fired: 0,
            pending: None,
            diag_state: None,
            diag_window_ms: 0,
            diag_count: 0,
            diag_suppressed: 0,
            forced_fails: 0,
            clock_floor: CLOCK_FLOOR,
            exempt_until_ms: 0,
        }
    }

    /// T-040: the console's `revive_fail <n>`.
    pub(crate) fn set_forced_fails(&mut self, n: u32) {
        self.forced_fails = n;
    }

    pub(crate) fn forced_fails(&self) -> u32 {
        self.forced_fails
    }

    /// T-039: `rival_hit_seconds` (clamped again here, so a bad value can never reach the clock).
    pub(crate) fn set_hit_seconds(&mut self, secs: f32) {
        self.hit_seconds = if secs.is_finite() {
            secs.clamp(RIVAL_HIT_SECONDS_RANGE.0, RIVAL_HIT_SECONDS_RANGE.1)
        } else {
            DEFAULT_RIVAL_HIT_SECONDS
        };
    }

    /// T-039: the dev status line for LOCAL while downed (tests).
    #[cfg(test)]
    pub(crate) fn downed_line(&self) -> Option<&str> {
        self.downed_line.as_deref()
    }

    pub(crate) fn policy(&self) -> RevivePolicy {
        self.policy
    }

    /// One pass; returns the log lines.
    pub(crate) fn step(
        &mut self,
        now_ms: u64,
        subjects: &[Subject],
        mem: &mut dyn CombatMem,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        let events = self.tracker.tick(now_ms, subjects, mem);
        if self.dev {
            lines.extend(events.iter().map(Event::line));
        }
        // ARENA-2: in an Arena match the loser of a round gets up on purpose (our get-up write): no re-down at the UP transition
        // (rule v1) nor during the get-up (`getup_step`). The downed rule stays: a rival's hit must not revive a downed player.
        let exempt = now_ms < self.exempt_until_ms || crate::mm::arena_match();
        if exempt {
            self.pending = None;
        } else {
            lines.extend(self.pending_step(now_ms, subjects, mem));
        }
        for ev in &events {
            if !exempt && rule_should_fire(self.policy, ev) {
                lines.extend(self.apply_rule(now_ms, ev.addr, mem));
            }
        }
        lines.extend(self.downed_step(now_ms, subjects, mem));
        lines.extend(self.remote_step(subjects, mem));
        if self.dev {
            lines.extend(self.watch_block(subjects, mem));
        }
        lines.extend(self.state_diag(now_ms, subjects, mem));
        lines
    }

    /// T-040: `None` = this diagnostic line is over the cap; otherwise the `(n suppressed)` suffix to append (maybe empty).
    pub(super) fn diag_allow(&mut self, now_ms: u64) -> Option<String> {
        if self.diag_count == 0 || now_ms.saturating_sub(self.diag_window_ms) >= DIAG_WINDOW_MS {
            self.diag_window_ms = now_ms;
            self.diag_count = 0;
        }
        if self.diag_count >= DIAG_MAX_LINES {
            self.diag_suppressed += 1;
            return None;
        }
        self.diag_count += 1;
        let n = std::mem::take(&mut self.diag_suppressed);
        Some(if n > 0 {
            format!(" ({n} suppressed)")
        } else {
            String::new()
        })
    }

    /// T-040, both builds, every call, read-only: one line per change of LOCAL's state byte (0 up, 1 downed, 2 getting up) with
    /// the downed block and the HP chain, so a missed re-down can be explained from the player-build log. Offsets: MEMORY_MAP
    /// sections 5 and 6 (NR 1.3.3.0). The first sight of a PlayerIns only seeds.
    pub(super) fn state_diag(
        &mut self,
        now_ms: u64,
        subjects: &[Subject],
        mem: &dyn CombatMem,
    ) -> Vec<String> {
        let Some(addr) = subjects
            .iter()
            .find(|s| s.local && mem.is_player(s.addr))
            .map(|s| s.addr)
        else {
            self.diag_state = None;
            return Vec::new();
        };
        let Some(state) = mem.read_u8(addr + OFF_DOWNED) else {
            return Vec::new();
        };
        let prev = self.diag_state.replace((addr, state));
        let Some((pa, ps)) = prev else {
            return Vec::new();
        };
        if pa != addr || ps == state {
            return Vec::new();
        }
        let Some(note) = self.diag_allow(now_ms) else {
            return Vec::new();
        };
        let ptr = mem
            .read_u64(addr + OFF_HP_PTR)
            .map_or("?".to_string(), |v| format!("0x{v:X}"));
        let gd = gamedata_of(mem, addr).and_then(|g| mem.read_u32(g + GD_HP));
        vec![format!(
            "T-040 LOCAL state {ps} -> {state}: bar {}, clock {}, pause {}, [+0x{OFF_HP_PTR:X}] = {ptr}, hp {} (mirror {}){note}",
            opt(read_f32(mem, addr + OFF_REVIVE_BAR)),
            opt(read_f32(mem, addr + OFF_BLEED_CLOCK)),
            opt(read_f32(mem, addr + OFF_PAUSE)),
            opt(gd),
            opt(mem.read_u32(addr + OFF_HP_MIRROR)),
        )]
    }

    /// T-040: retry a failed re-down. Cancels (one line, nothing written) when LOCAL is another PlayerIns or none, the state byte
    /// reads 1 or 2 (a new down / the get-up), or every readable HP is 0 (dead or down); gives up at the deadline. Otherwise
    /// [`Core::try_rule`] runs again with all its gates. Lines are never rate-limited (they are rare).
    pub(super) fn pending_step(
        &mut self,
        now_ms: u64,
        subjects: &[Subject],
        mem: &mut dyn CombatMem,
    ) -> Vec<String> {
        let Some(mut p) = self.pending.take() else {
            return Vec::new();
        };
        let local = subjects
            .iter()
            .find(|s| s.local && mem.is_player(s.addr))
            .map(|s| s.addr);
        let cancel = |p: &PendingRedown, why: &str| {
            vec![format!(
                "rule: pending re-down of LOCAL cancelled after {} tries, nothing written: {why}",
                p.tries
            )]
        };
        if local != Some(p.addr) {
            return cancel(&p, "LOCAL is not that PlayerIns any more");
        }
        match mem.read_u8(p.addr + OFF_DOWNED) {
            Some(1) => return cancel(&p, "LOCAL was downed again (state 1)"),
            Some(2) => return cancel(&p, "LOCAL is getting up (state 2)"),
            _ => {}
        }
        let hps = [
            mem.read_u32(p.addr + OFF_HP_MIRROR),
            gamedata_of(mem, p.addr).and_then(|g| mem.read_u32(g + GD_HP)),
        ];
        if hps.iter().any(Option::is_some) && hps.iter().flatten().all(|&h| h == 0) {
            return cancel(&p, "HP reads 0 (dead or already down)");
        }
        if now_ms >= p.deadline_ms {
            return vec![format!(
                "rule: rival completed the revive meter of LOCAL but the re-down never went through: gave up after {} tries in {} ms, nothing written (first error: {}; last error: {})",
                p.tries,
                now_ms.saturating_sub(p.first_ms),
                p.first_err,
                p.last_err
            )];
        }
        if mem.read_u8(p.addr + OFF_DOWNED) != Some(0) {
            // Unreadable state byte: wait, the deadline still applies.
            self.pending = Some(p);
            return Vec::new();
        }
        p.tries += 1;
        match self.try_rule(p.addr, mem) {
            Ok((before, back)) => vec![format!(
                "rule: rival completed the revive meter of LOCAL; downed again after {} tries ({} ms late) (hp {before} -> 0, readback {})",
                p.tries,
                now_ms.saturating_sub(p.first_ms),
                opt(back)
            )],
            Err(why) => {
                p.last_err = why;
                self.pending = Some(p);
                Vec::new()
            }
        }
    }

    /// T-039, every call (the main loop's 50 ms tick), LOCAL only. While LOCAL's state byte is 1 it remembers the revive bar; when
    /// the bar drops (a hit landed) and the attacker is a RIVAL, the bar is put back, the bleed-out clock loses `hit_seconds` and
    /// the pause timer is cleared. Other attackers: nothing. If a rival's hit completes the bar before we react, the state byte
    /// leaves 1 and rule v1 (the UP transition) is the safety net.
    pub(super) fn downed_step(
        &mut self,
        now_ms: u64,
        subjects: &[Subject],
        mem: &mut dyn CombatMem,
    ) -> Vec<String> {
        self.downed_line = None;
        let Some(addr) = subjects
            .iter()
            .find(|s| s.local && mem.is_player(s.addr))
            .map(|s| s.addr)
        else {
            self.dn = None;
            return Vec::new();
        };
        let state = mem.read_u8(addr + OFF_DOWNED);
        // A get-up is only acted on when it follows a down that was seen here.
        if state != Some(2) {
            self.getup_done = state != Some(1);
            self.getup_gate_logged = false;
            self.getup_since_ms = None;
        }
        if state != Some(1) {
            self.dn = None;
            return if state == Some(2) {
                self.getup_step(now_ms, addr, subjects, mem)
            } else {
                Vec::new()
            };
        }
        let (Some(bar), Some(clock), Some(pause)) = (
            read_f32(mem, addr + OFF_REVIVE_BAR),
            read_f32(mem, addr + OFF_BLEED_CLOCK),
            read_f32(mem, addr + OFF_PAUSE),
        ) else {
            return Vec::new();
        };
        if self.dev {
            self.downed_line = Some(format!(
                "downed: bar {bar:.1} left, clock {clock:.1} s, pause {pause:.1}"
            ));
        }
        if !bar.is_finite() {
            return Vec::new();
        }
        let mut st = match self.dn {
            Some(d) if d.addr == addr => d,
            _ => DownedMem {
                addr,
                bar_before: bar,
                gate_logged: false,
            },
        };
        let mut lines = Vec::new();
        if st.bar_before - bar > BAR_HIT_EPS {
            // A hit landed since the last tick.
            let infos = player_infos(mem, subjects);
            let raw = [OFF_LAST_ATTACKER, OFF_CUR_ATTACKER]
                .into_iter()
                .filter_map(|o| mem.read_u64(addr + o))
                .find(|&h| valid_handle(h));
            if classify(raw, &infos).kind == AttackerKind::Rival {
                match self.restore_after_rival_hit(addr, st.bar_before, bar, clock, pause, mem) {
                    // Keep bar_before: several rival hits never accumulate on the bar.
                    Ok(new_clock) => {
                        // T-040: the "bar 0 while state 1" frame, the suspected cause of a missed re-down.
                        if bar <= BAR_HIT_EPS {
                            if let Some(note) = self.diag_allow(now_ms) {
                                lines.push(format!(
                                    "T-040 LOCAL bar read {bar} while state 1 (before {}): downed rule restored it{note}",
                                    st.bar_before
                                ));
                            }
                        }
                        if self.may_log_downed(now_ms) {
                            lines.push(format!(
                                "rule: rival hit on downed LOCAL: bar {bar} -> {} restored, clock {clock} -> {new_clock}, pause {pause} -> 0",
                                st.bar_before
                            ));
                        }
                    }
                    Err(why) => {
                        if !st.gate_logged {
                            st.gate_logged = true;
                            lines.push(format!(
                                "rule: rival hit on downed LOCAL but nothing was written: {why}"
                            ));
                        }
                        st.bar_before = bar;
                    }
                }
            } else {
                st.bar_before = bar;
            }
        } else {
            st.bar_before = bar;
        }
        self.dn = Some(st);
        lines
    }

    /// LOCAL is getting up (state byte 2, about 3.5 s) because the revive bar was completed. If the last attacker is a RIVAL (one
    /// hit removed the whole bar before the downed rule could undo it; seen in FS-0 runs 18 and 19), make rule v1's store now
    /// instead of at state 0, through the same gates ([`Core::try_rule`]). Retried every call while the HP gate fails; rule v1 at
    /// the UP transition stays as the fallback. EXPERIMENT: what the game does with health 0 during state 2 is UNKNOWN.
    pub(super) fn getup_step(
        &mut self,
        now_ms: u64,
        addr: usize,
        subjects: &[Subject],
        mem: &mut dyn CombatMem,
    ) -> Vec<String> {
        if !self.early_redown
            || self.getup_done
            || self.policy != RevivePolicy::Redown
            || crate::mm::arena_match()
        {
            return Vec::new();
        }
        let into = now_ms.saturating_sub(*self.getup_since_ms.get_or_insert(now_ms));
        let infos = player_infos(mem, subjects);
        let raw = [OFF_LAST_ATTACKER, OFF_CUR_ATTACKER]
            .into_iter()
            .filter_map(|o| mem.read_u64(addr + o))
            .find(|&h| valid_handle(h));
        if classify(raw, &infos).kind != AttackerKind::Rival {
            self.getup_done = true;
            return Vec::new();
        }
        match self.try_rule(addr, mem) {
            Ok((before, back)) => {
                self.getup_done = true;
                if self.may_log(now_ms) {
                    vec![format!(
                        "rule: rival completed the revive meter of LOCAL; health set to 0 during the get-up, {into} ms in (hp {before} -> 0, readback {})",
                        opt(back)
                    )]
                } else {
                    Vec::new()
                }
            }
            Err(why) => {
                if self.dev && !self.getup_gate_logged {
                    self.getup_gate_logged = true;
                    vec![format!(
                        "rule: LOCAL is getting up after a rival's hit; nothing written yet ({into} ms in): {why}"
                    )]
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// The display half of the downed rule, every call. A remote copy holds the same block as the owner (+0x710 state, +0x714
    /// bar, +0x718 clock, +0x71C pause) but it is only refreshed when a hit lands, with the owner's values at that moment
    /// [VERIFIED on NR 1.3.3.0 by FS-0 run 18, re/MEMORY_MAP.md section 6]. The owner's client then undoes a rival's hit, which
    /// this copy never hears about, so the gauge on this screen kept the first hit. Here the same three stores are made on the
    /// copy. A copy does not say who hit (its +0x1A0 is not valid), so this is limited to a downed RIVAL with no teammate in the
    /// session: every hit on such a player comes from a rival of theirs.
    pub(super) fn remote_step(
        &mut self,
        subjects: &[Subject],
        mem: &mut dyn CombatMem,
    ) -> Vec<String> {
        let solo_rival = |s: &Subject| {
            !s.local
                && s.relation == Some(Relation::Rival)
                && s.team_no.is_some()
                && !subjects
                    .iter()
                    .any(|o| o.addr != s.addr && o.team_no == s.team_no)
        };
        let mut seen = HashSet::new();
        let mut lines = Vec::new();
        for s in subjects.iter().filter(|s| solo_rival(s)) {
            if !mem.is_player(s.addr) || mem.read_u8(s.addr + OFF_DOWNED) != Some(1) {
                continue;
            }
            let (Some(bar), Some(clock), Some(pause)) = (
                read_f32(mem, s.addr + OFF_REVIVE_BAR),
                read_f32(mem, s.addr + OFF_BLEED_CLOCK),
                read_f32(mem, s.addr + OFF_PAUSE),
            ) else {
                continue;
            };
            if !bar.is_finite() {
                continue;
            }
            seen.insert(s.addr);
            let before = *self.remote_bars.entry(s.addr).or_insert(bar);
            if before - bar <= BAR_HIT_EPS {
                // Never lowered: the value at the start of the down stays the reference.
                self.remote_bars.insert(s.addr, before.max(bar));
                continue;
            }
            match self.restore_after_rival_hit(s.addr, before, bar, clock, pause, mem) {
                Ok(new_clock) => {
                    if self.dev {
                        lines.push(format!(
                            "rule: rival hit on downed {} (display copy): bar {bar} -> {before} restored, clock {clock} -> {new_clock}, pause {pause} -> 0",
                            s.label
                        ));
                    }
                }
                Err(why) => {
                    if self.dev {
                        lines.push(format!(
                            "rule: rival hit on downed {} (display copy) but nothing was written: {why}",
                            s.label
                        ));
                    }
                    self.remote_bars.insert(s.addr, bar);
                }
            }
        }
        self.remote_bars.retain(|a, _| seen.contains(a));
        lines
    }

    /// Dev, read-only, every call: one line whenever a tracked player's state byte or revive bar changes while the block is in
    /// use, for LOCAL (read after the rule, so a rival hit that was undone does not show) and for the remote copies. Purpose:
    /// find out what the copy of a downed rival holds on the attacker's client (UNKNOWN as of 2026-10-12).
    pub(super) fn watch_block(&mut self, subjects: &[Subject], mem: &dyn CombatMem) -> Vec<String> {
        self.block_watch
            .retain(|a, _| subjects.iter().any(|s| s.addr == *a));
        let mut lines = Vec::new();
        for s in subjects.iter().filter(|s| mem.is_player(s.addr)) {
            let (Some(state), Some(bar), Some(clock), Some(pause)) = (
                mem.read_u8(s.addr + OFF_DOWNED),
                read_f32(mem, s.addr + OFF_REVIVE_BAR),
                read_f32(mem, s.addr + OFF_BLEED_CLOCK),
                read_f32(mem, s.addr + OFF_PAUSE),
            ) else {
                continue;
            };
            let prev = self.block_watch.insert(s.addr, (state, bar));
            let changed = match prev {
                Some((ps, pb)) => {
                    ps != state || (pb - bar).abs() > BAR_HIT_EPS || pb.is_nan() != bar.is_nan()
                }
                // First sight: only worth a line if the block is in use.
                None => state != 0,
            };
            if changed {
                let was = prev.map_or("?".to_string(), |(ps, pb)| format!("state {ps}, bar {pb}"));
                lines.push(format!(
                    "T-039 BLOCK who={} {was} -> state {state}, bar {bar} (clock {clock}, pause {pause})",
                    s.label
                ));
            }
        }
        lines
    }

    /// The gated stores of the downed rule; `Ok(new clock)`. Gates right before the writes: PlayerIns vtable, state byte still 1,
    /// finite values, bar in 0..=1000, clock in 0..=60.
    pub(super) fn restore_after_rival_hit(
        &mut self,
        addr: usize,
        bar_before: f32,
        bar: f32,
        clock: f32,
        pause: f32,
        mem: &mut dyn CombatMem,
    ) -> Result<f32, String> {
        if !mem.is_player(addr) {
            return Err(format!("0x{addr:X} no longer has a PlayerIns vtable"));
        }
        if mem.read_u8(addr + OFF_DOWNED) != Some(1) {
            return Err("state byte is no longer 1".to_string());
        }
        if !(bar.is_finite() && clock.is_finite() && pause.is_finite() && bar_before.is_finite()) {
            return Err("a value is not finite".to_string());
        }
        if !(0.0..=1000.0).contains(&bar_before) {
            return Err(format!("bar {bar_before} outside 0..=1000"));
        }
        if !(0.0..=60.0).contains(&clock) {
            return Err(format!("clock {clock} outside 0..=60"));
        }
        let new_clock = (clock - self.hit_seconds).max(self.clock_floor);
        for (off, v) in [
            (OFF_REVIVE_BAR, bar_before),
            (OFF_BLEED_CLOCK, new_clock),
            (OFF_PAUSE, 0.0),
        ] {
            mem.write(addr + off, &v.to_le_bytes())
                .map_err(|e| format!("write refused: {e}"))?;
        }
        self.fired += 1;
        Ok(new_clock)
    }

    /// Player build: at most one downed-rule line per [`PLAYER_LOG_MS`]; dev mode logs every one.
    pub(super) fn may_log_downed(&mut self, now_ms: u64) -> bool {
        if self.dev
            || self
                .last_dn_log_ms
                .is_none_or(|t| now_ms.saturating_sub(t) >= PLAYER_LOG_MS)
        {
            self.last_dn_log_ms = Some(now_ms);
            true
        } else {
            false
        }
    }

    /// Player build: at most one rule line per [`PLAYER_LOG_MS`]; dev mode logs every one.
    pub(super) fn may_log(&mut self, now_ms: u64) -> bool {
        if self.dev
            || self
                .last_rule_log_ms
                .is_none_or(|t| now_ms.saturating_sub(t) >= PLAYER_LOG_MS)
        {
            self.last_rule_log_ms = Some(now_ms);
            true
        } else {
            self.suppressed += 1;
            false
        }
    }

    /// Rule v1: u32 0 into `[PlayerIns+0xB78]+0x70` of LOCAL (the caller passes LOCAL's PlayerIns after the RIVAL/UP/hp checks).
    /// Offsets VERIFIED on NR 1.3.3.0 by FS-0 run 9, TECHNICAL_NOTES section 4. Gates here: PlayerIns vtable, plausible readable
    /// `[PlayerIns+0xB78]`, value > 0 agreeing with the mirror or the PlayerGameData HP. Anything else: skip and log.
    pub(super) fn apply_rule(
        &mut self,
        now_ms: u64,
        addr: usize,
        mem: &mut dyn CombatMem,
    ) -> Vec<String> {
        let (before, back) = match self.try_rule(addr, mem) {
            Ok(v) => v,
            Err(why) => {
                // T-040: keep trying for REDOWN_RETRY_MS; these lines are rare and never go through the 5 s limit.
                let line = format!(
                    "rule: rival completed the revive meter of LOCAL but nothing was written yet: {why}; retrying for {REDOWN_RETRY_MS} ms"
                );
                self.pending = Some(PendingRedown {
                    addr,
                    first_ms: now_ms,
                    deadline_ms: now_ms + REDOWN_RETRY_MS,
                    tries: 1,
                    first_err: why.clone(),
                    last_err: why,
                });
                return vec![line];
            }
        };
        if !self.may_log(now_ms) {
            return Vec::new();
        }
        let dropped = std::mem::take(&mut self.suppressed);
        let note = if dropped > 0 {
            format!(" ({dropped} earlier line(s) suppressed)")
        } else {
            String::new()
        };
        vec![format!(
            "rule: rival completed the revive meter of LOCAL; downed again (hp {before} -> 0, readback {}){note}",
            opt(back)
        )]
    }

    /// The gated write; `Ok((hp before, readback))`.
    pub(super) fn try_rule(
        &mut self,
        addr: usize,
        mem: &mut dyn CombatMem,
    ) -> Result<(u32, Option<u32>), String> {
        // T-040: console test switch (`revive_fail`); nothing but the dev console ever sets it.
        if self.forced_fails > 0 {
            self.forced_fails -= 1;
            return Err("forced by console".to_string());
        }
        if !mem.is_player(addr) {
            return Err(format!("0x{addr:X} no longer has a PlayerIns vtable"));
        }
        // T-040 run 1: the character data module is the first route; `[PlayerIns+0xB78]+0x70` held no HP on either Nucleus game.
        let dm_err = match data_module_hp(mem, addr) {
            Ok((target, before)) => {
                mem.write(target, &0u32.to_le_bytes())
                    .map_err(|e| format!("write refused: {e}"))?;
                self.fired += 1;
                return Ok((before, mem.read_u32(target)));
            }
            Err(why) => why,
        };
        self.try_hp_ptr(addr, mem)
            .map_err(|why| format!("data module: {dm_err}; old route: {why}"))
    }

    /// The pre-T-040 route: `[PlayerIns+0xB78]+0x70`, gated by the mirror or PlayerGameData. Only a fallback now.
    fn try_hp_ptr(
        &mut self,
        addr: usize,
        mem: &mut dyn CombatMem,
    ) -> Result<(u32, Option<u32>), String> {
        // T-040: the error carries the raw value (MEMORY_MAP section 5).
        let raw = mem
            .read_u64(addr + OFF_HP_PTR)
            .ok_or_else(|| format!("[PlayerIns+0x{OFF_HP_PTR:X}] unreadable"))?;
        let p = usize::try_from(raw)
            .ok()
            .filter(|&v| is_ptr(v))
            .ok_or_else(|| {
                format!("[PlayerIns+0x{OFF_HP_PTR:X}] = 0x{raw:X} is not a plausible pointer")
            })?;
        let target = p + HP_PTR_FIELD;
        let auth = mem.read_u32(target);
        let mirror = mem.read_u32(addr + OFF_HP_MIRROR);
        let gd = gamedata_of(mem, addr).and_then(|g| mem.read_u32(g + GD_HP));
        if !hp_gate(auth, mirror, gd) {
            return Err(format!(
                "hp gate failed (authoritative {}, mirror {}, PlayerGameData {})",
                opt(auth),
                opt(mirror),
                opt(gd)
            ));
        }
        let before = auth.unwrap_or(0);
        mem.write(target, &0u32.to_le_bytes())
            .map_err(|e| format!("write refused: {e}"))?;
        self.fired += 1;
        Ok((before, mem.read_u32(target)))
    }
}
