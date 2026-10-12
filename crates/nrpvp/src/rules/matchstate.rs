//! Deferred, exploratory match-state model (PROJECT_PLAN.md section 4B). It is not wired into the game
//! and does not establish accepted product rules. No game access or clocks: time is passed in as
//! milliseconds by the caller.
//!
//! Flow: the caller feeds [`Event`]s with [`MatchState::apply`] (or [`MatchState::apply_batch`] for
//! events that happened at the same instant) and applies the returned [`Effect`]s to the game. Queries
//! (`may_respawn`, `is_eliminated`, `alive_teams`, `winner`, `is_protected`) read the current state.
//!
//! Rules from the earlier design draft are marked `SCOPE:`; anything not stated there is marked
//! `ASSUMPTION:`. Revisit these rules against scope v3 before integrating this model.
//!
//! Core rule, SCOPE: "death before the final closure is a loot setback, not an elimination ... Only the
//! closing moments of Night 2 determine the ultimate survivor." So dying never eliminates anyone until
//! sudden death.

use std::collections::{BTreeMap, BTreeSet};

use super::teams::{PlayerId, Rosters, TeamId};

/// Milliseconds on whatever monotonic clock the caller uses. Only differences matter.
pub type Millis = u64;

/// Match phases. SCOPE sections 2 and 4: Day 1, Night 1 (no boss), Day 2, Night 2 stage 1 (arena
/// contraction), Night 2 stage 2 (sudden death).
///
/// `Finished.winner` is `None` only for a draw (see the tie rule on [`MatchState::apply_batch`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Lobby,
    Day1,
    Night1,
    Day2,
    Night2Contraction,
    SuddenDeath,
    Finished { winner: Option<TeamId> },
}

impl Phase {
    /// Position in the `PhaseAdvanced`-driven sequence; `None` for SuddenDeath/Finished (reached only
    /// through `ArenaFullyClosed` / the win check).
    fn advance_rank(self) -> Option<u8> {
        match self {
            Phase::Lobby => Some(0),
            Phase::Day1 => Some(1),
            Phase::Night1 => Some(2),
            Phase::Day2 => Some(3),
            Phase::Night2Contraction => Some(4),
            Phase::SuddenDeath | Phase::Finished { .. } => None,
        }
    }

    /// Day 1 through sudden death.
    fn is_active(self) -> bool {
        !matches!(self, Phase::Lobby | Phase::Finished { .. })
    }
}

/// SCOPE section 5: the winner chooses "Escape with loot" or "Challenge the Nightlord".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinnerChoice {
    Undecided,
    Escape,
    ChallengeNightlord,
}

/// Inputs. The caller translates game observations into these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The game's own day/night signal moved to `phase`. No durations are hard-coded here.
    /// Only forward moves among Day1, Night1, Day2, Night2Contraction are accepted (skipping is
    /// tolerated). SuddenDeath comes from `ArenaFullyClosed`, Finished from the win check.
    PhaseAdvanced(Phase),
    PlayerDowned(PlayerId),
    PlayerRevived(PlayerId),
    PlayerDied(PlayerId),
    PlayerRespawned(PlayerId),
    /// The player disconnected or left the expedition for good.
    PlayerLeft(PlayerId),
    /// The player attacked something; ends respawn protection.
    PlayerAttacked(PlayerId),
    /// SCOPE: "Once the arena reaches its final size, respawns are disabled for everyone."
    ArenaFullyClosed,
    WinnerChose(WinnerChoice),
}

/// Outputs: things the caller must do to the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// SCOPE: "Death drops all inventory". Emitted on every death while the match is active.
    DropInventory {
        player: PlayerId,
    },
    /// SCOPE: "respawns are permanently disabled".
    DisableRespawns,
    PlayerEliminated {
        player: PlayerId,
    },
    TeamEliminated {
        team: TeamId,
    },
    /// Respawn protection started; lasts until `until` or the player's first attack.
    GrantProtection {
        player: PlayerId,
        until: Millis,
    },
    EndProtection {
        player: PlayerId,
    },
    DeclareWinner {
        team: TeamId,
    },
    /// No team survived (simultaneous last deaths, or everyone left).
    DeclareDraw,
    /// The winner chose to escape with the loot.
    ExtractWinner {
        team: TeamId,
    },
    /// The winner chose to challenge the Nightlord.
    StartNightlord {
        team: TeamId,
    },
}

/// Why an event was rejected (state unchanged).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventError {
    UnknownPlayer(PlayerId),
    /// The event makes no sense in the current phase.
    WrongPhase,
    /// A phase change that is not a forward move.
    IllegalPhaseChange,
    /// The player is not in a state where this event applies (e.g. reviving someone not downed).
    InvalidPlayerState,
    /// `WinnerChose(Undecided)`.
    InvalidChoice,
    AlreadyChosen,
}

/// Per-player state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerState {
    Alive,
    /// Downed, teammates may revive.
    Downed,
    /// Dead, waiting to respawn (only possible before sudden death).
    Dead,
    /// Permanently out (died, or was waiting to respawn, during sudden death).
    Eliminated,
    Left,
}

#[derive(Clone, Debug)]
pub struct MatchConfig {
    /// Respawn protection length. ASSUMPTION: SCOPE only says "a short protection period"; 5000 ms.
    pub protection_ms: Millis,
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self {
            protection_ms: 5000,
        }
    }
}

#[derive(Clone, Debug)]
struct Slot {
    team: TeamId,
    state: PlayerState,
    protected_until: Option<Millis>,
}

/// The state machine. Rosters are frozen at construction.
///
/// ASSUMPTION: no late joiners once the match object exists (rosters are fixed at match start); a
/// player who leaves is recorded as `Left` and never replaced.
#[derive(Debug)]
pub struct MatchState {
    config: MatchConfig,
    phase: Phase,
    slots: BTreeMap<PlayerId, Slot>,
    team_ids: Vec<TeamId>,
    announced: BTreeSet<TeamId>,
    choice: WinnerChoice,
    respawns_disabled: bool,
}

impl MatchState {
    /// New match in the Lobby phase.
    pub fn new(rosters: &Rosters, config: MatchConfig) -> Self {
        let mut slots = BTreeMap::new();
        let mut team_ids = Vec::new();
        for (team, members) in rosters.teams() {
            team_ids.push(team);
            for p in members {
                slots.insert(
                    *p,
                    Slot {
                        team,
                        state: PlayerState::Alive,
                        protected_until: None,
                    },
                );
            }
        }
        Self {
            config,
            phase: Phase::Lobby,
            slots,
            team_ids,
            announced: BTreeSet::new(),
            choice: WinnerChoice::Undecided,
            respawns_disabled: false,
        }
    }

    // ---- queries -------------------------------------------------------------------------------

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn choice(&self) -> WinnerChoice {
        self.choice
    }

    /// The winning team once the match is finished with a winner.
    pub fn winner(&self) -> Option<TeamId> {
        match self.phase {
            Phase::Finished { winner } => winner,
            _ => None,
        }
    }

    pub fn respawns_disabled(&self) -> bool {
        self.respawns_disabled
    }

    pub fn player_state(&self, player: PlayerId) -> Option<PlayerState> {
        self.slots.get(&player).map(|s| s.state)
    }

    pub fn team_of(&self, player: PlayerId) -> Option<TeamId> {
        self.slots.get(&player).map(|s| s.team)
    }

    /// SCOPE: "Respawning: Enabled until the final arena finishes closing". True when the player is
    /// dead and the phase is Day 1 through Night 2 contraction.
    pub fn may_respawn(&self, player: PlayerId) -> bool {
        self.phase.is_active()
            && !self.respawns_disabled
            && self.player_state(player) == Some(PlayerState::Dead)
    }

    /// SCOPE: "Team elimination: Permanent only after final arena closure". A player is eliminated
    /// only by dying (or waiting to respawn) during sudden death.
    pub fn is_eliminated(&self, player: PlayerId) -> bool {
        self.player_state(player) == Some(PlayerState::Eliminated)
    }

    pub fn has_left(&self, player: PlayerId) -> bool {
        self.player_state(player) == Some(PlayerState::Left)
    }

    /// A team is eliminated when it has no member who is neither Eliminated nor Left. Before sudden
    /// death a team whose members are all merely dead is NOT eliminated (SCOPE: death is "not an
    /// elimination"); only leavers can remove a team then.
    pub fn is_team_eliminated(&self, team: TeamId) -> bool {
        self.team_ids.contains(&team) && !self.team_has_standing(team)
    }

    /// Teams that are not eliminated, in team-id order.
    pub fn alive_teams(&self) -> Vec<TeamId> {
        self.team_ids
            .iter()
            .copied()
            .filter(|t| self.team_has_standing(*t))
            .collect()
    }

    /// True while the player's respawn protection runs (`now` before the deadline, no attack yet).
    pub fn is_protected(&self, player: PlayerId, now: Millis) -> bool {
        self.slots
            .get(&player)
            .and_then(|s| s.protected_until)
            .is_some_and(|until| now < until)
    }

    fn team_has_standing(&self, team: TeamId) -> bool {
        self.slots.values().any(|s| {
            s.team == team && !matches!(s.state, PlayerState::Eliminated | PlayerState::Left)
        })
    }

    // ---- inputs --------------------------------------------------------------------------------

    /// Apply one event at time `now`. On error the state is unchanged.
    pub fn apply(&mut self, event: Event, now: Millis) -> Result<Vec<Effect>, EventError> {
        let mut fx = self.apply_inner(event, now)?;
        self.post_checks(&mut fx);
        Ok(fx)
    }

    /// Apply events that happened at the same instant. Elimination announcements and the win check run
    /// once, after all of them.
    ///
    /// TIE RULE (ASSUMPTION: SCOPE does not cover it): if the batch leaves no standing team in a match
    /// that is in sudden death, the match is a draw (`Finished { winner: None }`, `DeclareDraw`).
    /// Events applied one at a time are ordered, so there the team that still stands after the earlier
    /// event wins. Rejected events are returned; the rest still apply.
    pub fn apply_batch(&mut self, events: &[Event], now: Millis) -> (Vec<Effect>, Vec<EventError>) {
        let mut fx = Vec::new();
        let mut errors = Vec::new();
        for e in events {
            match self.apply_inner(*e, now) {
                Ok(mut f) => fx.append(&mut f),
                Err(err) => errors.push(err),
            }
        }
        self.post_checks(&mut fx);
        (fx, errors)
    }

    /// Expire protection windows whose deadline has passed.
    pub fn tick(&mut self, now: Millis) -> Vec<Effect> {
        let mut fx = Vec::new();
        for (player, slot) in &mut self.slots {
            if slot.protected_until.is_some_and(|until| now >= until) {
                slot.protected_until = None;
                fx.push(Effect::EndProtection { player: *player });
            }
        }
        fx
    }

    fn slot_mut(&mut self, player: PlayerId) -> Result<&mut Slot, EventError> {
        self.slots
            .get_mut(&player)
            .ok_or(EventError::UnknownPlayer(player))
    }

    fn require_active(&self) -> Result<(), EventError> {
        if self.phase.is_active() {
            Ok(())
        } else {
            Err(EventError::WrongPhase)
        }
    }

    fn apply_inner(&mut self, event: Event, now: Millis) -> Result<Vec<Effect>, EventError> {
        let mut fx = Vec::new();
        match event {
            Event::PhaseAdvanced(next) => {
                let from = self.phase.advance_rank().ok_or(EventError::WrongPhase)?;
                // Only Day1..Night2Contraction may be requested; the rank check also rejects
                // SuddenDeath and Finished (rank None).
                match next.advance_rank() {
                    Some(to) if to > from => self.phase = next,
                    _ => return Err(EventError::IllegalPhaseChange),
                }
            }
            Event::PlayerDowned(p) => {
                self.require_active()?;
                let slot = self.slot_mut(p)?;
                if slot.state != PlayerState::Alive {
                    return Err(EventError::InvalidPlayerState);
                }
                slot.state = PlayerState::Downed;
            }
            Event::PlayerRevived(p) => {
                self.require_active()?;
                let slot = self.slot_mut(p)?;
                if slot.state != PlayerState::Downed {
                    return Err(EventError::InvalidPlayerState);
                }
                slot.state = PlayerState::Alive;
            }
            Event::PlayerDied(p) => {
                self.require_active()?;
                let sudden = self.phase == Phase::SuddenDeath;
                let slot = self.slot_mut(p)?;
                if !matches!(slot.state, PlayerState::Alive | PlayerState::Downed) {
                    return Err(EventError::InvalidPlayerState);
                }
                slot.protected_until = None;
                // SCOPE: "Death drops all inventory but allows respawning" (before sudden death);
                // "From this point onward, deaths are permanent."
                fx.push(Effect::DropInventory { player: p });
                if sudden {
                    slot.state = PlayerState::Eliminated;
                    fx.push(Effect::PlayerEliminated { player: p });
                } else {
                    slot.state = PlayerState::Dead;
                }
            }
            Event::PlayerRespawned(p) => {
                self.require_active()?;
                if !self.slots.contains_key(&p) {
                    return Err(EventError::UnknownPlayer(p));
                }
                if !self.may_respawn(p) {
                    return Err(EventError::InvalidPlayerState);
                }
                let until = now.saturating_add(self.config.protection_ms);
                let slot = self.slot_mut(p)?;
                slot.state = PlayerState::Alive;
                // SCOPE: "receive a short protection period that ends immediately upon attacking."
                slot.protected_until = Some(until);
                fx.push(Effect::GrantProtection { player: p, until });
            }
            Event::PlayerLeft(p) => {
                // Accepted in every phase. ASSUMPTION: leaving drops nothing (the inventory goes
                // away with the player) and never counts as a death.
                let slot = self.slot_mut(p)?;
                slot.protected_until = None;
                if slot.state != PlayerState::Eliminated {
                    slot.state = PlayerState::Left;
                }
            }
            Event::PlayerAttacked(p) => {
                self.require_active()?;
                let slot = self.slot_mut(p)?;
                if slot.protected_until.take().is_some() {
                    fx.push(Effect::EndProtection { player: p });
                }
            }
            Event::ArenaFullyClosed => {
                if self.phase != Phase::Night2Contraction {
                    return Err(EventError::WrongPhase);
                }
                self.phase = Phase::SuddenDeath;
                self.respawns_disabled = true;
                fx.push(Effect::DisableRespawns);
                // ASSUMPTION: a player who is dead and waiting to respawn at the moment of closure
                // is eliminated (respawns are "permanently disabled"; there is no way back in).
                // Downed players stay downed: they can still be revived, and are eliminated only if
                // they die.
                for (player, slot) in &mut self.slots {
                    slot.protected_until = None;
                    if slot.state == PlayerState::Dead {
                        slot.state = PlayerState::Eliminated;
                        fx.push(Effect::PlayerEliminated { player: *player });
                    }
                }
            }
            Event::WinnerChose(choice) => {
                let Phase::Finished {
                    winner: Some(team), ..
                } = self.phase
                else {
                    return Err(EventError::WrongPhase);
                };
                if choice == WinnerChoice::Undecided {
                    return Err(EventError::InvalidChoice);
                }
                if self.choice != WinnerChoice::Undecided {
                    return Err(EventError::AlreadyChosen);
                }
                self.choice = choice;
                fx.push(match choice {
                    WinnerChoice::Escape => Effect::ExtractWinner { team },
                    _ => Effect::StartNightlord { team },
                });
            }
        }
        Ok(fx)
    }

    /// Announce newly eliminated teams, then run the win check.
    fn post_checks(&mut self, fx: &mut Vec<Effect>) {
        if !self.phase.is_active() {
            return;
        }
        let teams = self.team_ids.clone();
        for team in &teams {
            if !self.team_has_standing(*team) && self.announced.insert(*team) {
                fx.push(Effect::TeamEliminated { team: *team });
            }
        }
        // SCOPE: "A team wins as soon as every opposing team has been eliminated after respawns are
        // disabled." / "The battle continues until only one team has living players."
        //
        // ASSUMPTION: the same check also runs before sudden death, but there only leavers can
        // remove a team (dying never does), and only in a match that started with 2+ teams (so a
        // solo test with one team never ends by itself). Otherwise a match whose rivals all
        // disconnected would never end.
        let standing = self.alive_teams();
        let applies = self.phase == Phase::SuddenDeath || teams.len() >= 2;
        if !applies {
            return;
        }
        match standing.as_slice() {
            [only] => {
                self.phase = Phase::Finished {
                    winner: Some(*only),
                };
                fx.push(Effect::DeclareWinner { team: *only });
            }
            [] => {
                self.phase = Phase::Finished { winner: None };
                fx.push(Effect::DeclareDraw);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::teams::assign_teams;

    fn pid(n: u64) -> PlayerId {
        PlayerId(((0x6000 + n) << 32) | 0xFFFF_FFFF)
    }

    /// Players 1..=n in teams of `size`: team k holds ids k*size+1 ..= k*size+size.
    fn setup(teams: usize, size: usize) -> (MatchState, Vec<Vec<PlayerId>>) {
        let ids: Vec<PlayerId> = (1..=(teams * size) as u64).map(pid).collect();
        let r = assign_teams(&ids, size);
        let groups: Vec<Vec<PlayerId>> = r.teams().map(|(_, m)| m.to_vec()).collect();
        assert_eq!(groups.len(), teams);
        (MatchState::new(&r, MatchConfig::default()), groups)
    }

    fn adv(m: &mut MatchState, p: Phase) {
        m.apply(Event::PhaseAdvanced(p), 0).unwrap();
    }

    fn to_contraction(m: &mut MatchState) {
        for p in [
            Phase::Day1,
            Phase::Night1,
            Phase::Day2,
            Phase::Night2Contraction,
        ] {
            adv(m, p);
        }
    }

    fn die(m: &mut MatchState, p: PlayerId) -> Vec<Effect> {
        m.apply(Event::PlayerDied(p), 0).unwrap()
    }

    #[test]
    fn lobby_rejects_gameplay() {
        let (mut m, g) = setup(2, 1);
        assert_eq!(m.phase(), Phase::Lobby);
        assert_eq!(
            m.apply(Event::PlayerDied(g[0][0]), 0),
            Err(EventError::WrongPhase)
        );
        assert_eq!(
            m.apply(Event::ArenaFullyClosed, 0),
            Err(EventError::WrongPhase)
        );
        assert_eq!(
            m.apply(Event::PhaseAdvanced(Phase::SuddenDeath), 0),
            Err(EventError::IllegalPhaseChange)
        );
    }

    #[test]
    fn phase_only_moves_forward() {
        let (mut m, _) = setup(2, 1);
        adv(&mut m, Phase::Day1);
        adv(&mut m, Phase::Night1);
        assert_eq!(
            m.apply(Event::PhaseAdvanced(Phase::Day1), 0),
            Err(EventError::IllegalPhaseChange)
        );
        assert_eq!(
            m.apply(Event::PhaseAdvanced(Phase::Night1), 0),
            Err(EventError::IllegalPhaseChange)
        );
        // Skipping is tolerated.
        adv(&mut m, Phase::Night2Contraction);
        assert_eq!(m.phase(), Phase::Night2Contraction);
        assert_eq!(
            m.apply(Event::PhaseAdvanced(Phase::Finished { winner: None }), 0),
            Err(EventError::IllegalPhaseChange)
        );
    }

    #[test]
    fn unknown_player_rejected() {
        let (mut m, _) = setup(2, 1);
        adv(&mut m, Phase::Day1);
        assert_eq!(
            m.apply(Event::PlayerDied(pid(99)), 0),
            Err(EventError::UnknownPlayer(pid(99)))
        );
    }

    #[test]
    fn death_before_closure_drops_and_allows_respawn() {
        let (mut m, g) = setup(2, 3);
        to_contraction(&mut m);
        let a = g[0][0];
        assert!(!m.may_respawn(a));
        let fx = die(&mut m, a);
        assert_eq!(fx, vec![Effect::DropInventory { player: a }]);
        assert_eq!(m.player_state(a), Some(PlayerState::Dead));
        assert!(m.may_respawn(a));
        assert!(!m.is_eliminated(a));
        m.apply(Event::PlayerRespawned(a), 100).unwrap();
        assert_eq!(m.player_state(a), Some(PlayerState::Alive));
        assert!(!m.may_respawn(a));
    }

    #[test]
    fn death_after_closure_is_permanent() {
        let (mut m, g) = setup(2, 3);
        to_contraction(&mut m);
        let fx = m.apply(Event::ArenaFullyClosed, 0).unwrap();
        assert_eq!(fx, vec![Effect::DisableRespawns]);
        assert!(m.respawns_disabled());
        let a = g[0][0];
        let fx = die(&mut m, a);
        assert_eq!(
            fx,
            vec![
                Effect::DropInventory { player: a },
                Effect::PlayerEliminated { player: a }
            ]
        );
        assert!(m.is_eliminated(a));
        assert!(!m.may_respawn(a));
        assert_eq!(
            m.apply(Event::PlayerRespawned(a), 0),
            Err(EventError::InvalidPlayerState)
        );
        // Team 0 still has two members standing.
        assert!(!m.is_team_eliminated(TeamId(0)));
    }

    #[test]
    fn dead_waiting_to_respawn_at_closure_is_eliminated() {
        let (mut m, g) = setup(2, 1);
        to_contraction(&mut m);
        let a = g[0][0];
        die(&mut m, a);
        let fx = m.apply(Event::ArenaFullyClosed, 0).unwrap();
        assert!(fx.contains(&Effect::PlayerEliminated { player: a }));
        assert!(fx.contains(&Effect::TeamEliminated { team: TeamId(0) }));
        assert_eq!(fx.last(), Some(&Effect::DeclareWinner { team: TeamId(1) }));
        assert_eq!(m.winner(), Some(TeamId(1)));
    }

    #[test]
    fn team_wiped_before_sudden_death_is_not_eliminated() {
        let (mut m, g) = setup(2, 3);
        to_contraction(&mut m);
        for p in &g[0] {
            die(&mut m, *p);
        }
        assert!(!m.is_team_eliminated(TeamId(0)));
        assert_eq!(m.alive_teams(), vec![TeamId(0), TeamId(1)]);
        assert_eq!(m.winner(), None);
        assert!(matches!(m.phase(), Phase::Night2Contraction));
        for p in &g[0] {
            assert!(m.may_respawn(*p));
            m.apply(Event::PlayerRespawned(*p), 0).unwrap();
        }
        assert_eq!(m.alive_teams().len(), 2);
    }

    #[test]
    fn downed_and_revive() {
        let (mut m, g) = setup(2, 2);
        to_contraction(&mut m);
        let a = g[0][0];
        m.apply(Event::PlayerDowned(a), 0).unwrap();
        assert_eq!(m.player_state(a), Some(PlayerState::Downed));
        assert_eq!(
            m.apply(Event::PlayerDowned(a), 0),
            Err(EventError::InvalidPlayerState)
        );
        m.apply(Event::PlayerRevived(a), 0).unwrap();
        assert_eq!(m.player_state(a), Some(PlayerState::Alive));
        assert_eq!(
            m.apply(Event::PlayerRevived(a), 0),
            Err(EventError::InvalidPlayerState)
        );
        // Dying while downed counts.
        m.apply(Event::PlayerDowned(a), 0).unwrap();
        die(&mut m, a);
        assert_eq!(m.player_state(a), Some(PlayerState::Dead));
    }

    #[test]
    fn downed_player_keeps_team_standing_in_sudden_death() {
        let (mut m, g) = setup(2, 1);
        to_contraction(&mut m);
        m.apply(Event::ArenaFullyClosed, 0).unwrap();
        // ASSUMPTION: downed is not dead; the team stands until the death is finalized.
        m.apply(Event::PlayerDowned(g[0][0]), 0).unwrap();
        assert_eq!(m.winner(), None);
        m.apply(Event::PlayerRevived(g[0][0]), 0).unwrap();
        m.apply(Event::PlayerDowned(g[0][0]), 0).unwrap();
        die(&mut m, g[0][0]);
        assert_eq!(m.winner(), Some(TeamId(1)));
    }

    fn full_walkthrough(teams: usize, size: usize) {
        let (mut m, g) = setup(teams, size);
        assert_eq!(m.alive_teams().len(), teams);
        adv(&mut m, Phase::Day1);
        // A death and respawn on Day 1.
        let v = g[teams - 1][0];
        assert_eq!(die(&mut m, v), vec![Effect::DropInventory { player: v }]);
        m.apply(Event::PlayerRespawned(v), 10).unwrap();
        adv(&mut m, Phase::Night1);
        adv(&mut m, Phase::Day2);
        // A whole team dies on Day 2 and is still in the game.
        for p in &g[1] {
            die(&mut m, *p);
        }
        assert_eq!(m.alive_teams().len(), teams);
        for p in &g[1] {
            m.apply(Event::PlayerRespawned(*p), 20).unwrap();
        }
        adv(&mut m, Phase::Night2Contraction);
        m.apply(Event::ArenaFullyClosed, 30).unwrap();
        assert_eq!(m.phase(), Phase::SuddenDeath);
        // Eliminate teams 1.. in order; team 0 should win with the last death.
        for (ti, team) in g.iter().enumerate().skip(1) {
            for (i, p) in team.iter().enumerate() {
                let fx = die(&mut m, *p);
                let last_of_team = i + 1 == team.len();
                assert_eq!(
                    fx.contains(&Effect::TeamEliminated {
                        team: TeamId(ti as u8)
                    }),
                    last_of_team
                );
                let last_team = ti + 1 == g.len();
                assert_eq!(
                    fx.contains(&Effect::DeclareWinner { team: TeamId(0) }),
                    last_of_team && last_team
                );
            }
        }
        assert_eq!(
            m.phase(),
            Phase::Finished {
                winner: Some(TeamId(0))
            }
        );
        assert_eq!(m.alive_teams(), vec![TeamId(0)]);
        for p in &g[0] {
            assert!(!m.is_eliminated(*p));
        }
        // Winner's choice.
        assert_eq!(m.choice(), WinnerChoice::Undecided);
        assert_eq!(
            m.apply(Event::WinnerChose(WinnerChoice::Undecided), 0),
            Err(EventError::InvalidChoice)
        );
        let fx = m
            .apply(Event::WinnerChose(WinnerChoice::ChallengeNightlord), 0)
            .unwrap();
        assert_eq!(fx, vec![Effect::StartNightlord { team: TeamId(0) }]);
        assert_eq!(
            m.apply(Event::WinnerChose(WinnerChoice::Escape), 0),
            Err(EventError::AlreadyChosen)
        );
        // Nothing else is accepted after the end.
        assert_eq!(
            m.apply(Event::PlayerDied(g[0][0]), 0),
            Err(EventError::WrongPhase)
        );
    }

    #[test]
    fn walkthrough_2_teams_of_3() {
        full_walkthrough(2, 3);
    }

    #[test]
    fn walkthrough_3_teams_of_2() {
        full_walkthrough(3, 2);
    }

    #[test]
    fn walkthrough_6_teams_of_3() {
        full_walkthrough(6, 3);
    }

    #[test]
    fn escape_choice() {
        let (mut m, g) = setup(2, 1);
        to_contraction(&mut m);
        m.apply(Event::ArenaFullyClosed, 0).unwrap();
        assert_eq!(
            m.apply(Event::WinnerChose(WinnerChoice::Escape), 0),
            Err(EventError::WrongPhase)
        );
        die(&mut m, g[1][0]);
        let fx = m
            .apply(Event::WinnerChose(WinnerChoice::Escape), 0)
            .unwrap();
        assert_eq!(fx, vec![Effect::ExtractWinner { team: TeamId(0) }]);
    }

    #[test]
    fn simultaneous_last_deaths_in_batch_is_a_draw() {
        let (mut m, g) = setup(2, 1);
        to_contraction(&mut m);
        m.apply(Event::ArenaFullyClosed, 0).unwrap();
        let (fx, errs) =
            m.apply_batch(&[Event::PlayerDied(g[0][0]), Event::PlayerDied(g[1][0])], 5);
        assert!(errs.is_empty());
        assert!(fx.contains(&Effect::TeamEliminated { team: TeamId(0) }));
        assert!(fx.contains(&Effect::TeamEliminated { team: TeamId(1) }));
        assert_eq!(fx.last(), Some(&Effect::DeclareDraw));
        assert_eq!(m.phase(), Phase::Finished { winner: None });
        assert_eq!(m.winner(), None);
        // No winner means no choice to make.
        assert_eq!(
            m.apply(Event::WinnerChose(WinnerChoice::Escape), 0),
            Err(EventError::WrongPhase)
        );
    }

    #[test]
    fn sequential_last_deaths_first_survivor_wins() {
        let (mut m, g) = setup(2, 1);
        to_contraction(&mut m);
        m.apply(Event::ArenaFullyClosed, 0).unwrap();
        die(&mut m, g[0][0]);
        // Team 1 already won; its member's later death is rejected.
        assert_eq!(m.winner(), Some(TeamId(1)));
        assert_eq!(
            m.apply(Event::PlayerDied(g[1][0]), 0),
            Err(EventError::WrongPhase)
        );
    }

    #[test]
    fn batch_with_survivor_is_not_a_draw() {
        let (mut m, g) = setup(3, 1);
        to_contraction(&mut m);
        m.apply(Event::ArenaFullyClosed, 0).unwrap();
        let (fx, _) = m.apply_batch(&[Event::PlayerDied(g[0][0]), Event::PlayerDied(g[1][0])], 5);
        assert_eq!(fx.last(), Some(&Effect::DeclareWinner { team: TeamId(2) }));
    }

    #[test]
    fn batch_reports_errors_and_applies_the_rest() {
        let (mut m, g) = setup(2, 1);
        to_contraction(&mut m);
        let (fx, errs) = m.apply_batch(
            &[Event::PlayerRevived(g[0][0]), Event::PlayerDied(g[1][0])],
            0,
        );
        assert_eq!(errs, vec![EventError::InvalidPlayerState]);
        assert_eq!(fx, vec![Effect::DropInventory { player: g[1][0] }]);
    }

    #[test]
    fn leaver_pre_sudden_death_removes_team_and_can_end_match() {
        let (mut m, g) = setup(2, 2);
        to_contraction(&mut m);
        m.apply(Event::PlayerLeft(g[1][0]), 0).unwrap();
        assert!(m.has_left(g[1][0]));
        assert!(!m.is_team_eliminated(TeamId(1)));
        assert_eq!(m.winner(), None);
        let fx = m.apply(Event::PlayerLeft(g[1][1]), 0).unwrap();
        assert!(fx.contains(&Effect::TeamEliminated { team: TeamId(1) }));
        assert_eq!(m.winner(), Some(TeamId(0)));
    }

    #[test]
    fn leaver_in_sudden_death_counts_as_gone() {
        let (mut m, g) = setup(2, 2);
        to_contraction(&mut m);
        m.apply(Event::ArenaFullyClosed, 0).unwrap();
        die(&mut m, g[1][0]);
        let fx = m.apply(Event::PlayerLeft(g[1][1]), 0).unwrap();
        assert!(fx.is_empty() || fx.contains(&Effect::TeamEliminated { team: TeamId(1) }));
        assert_eq!(m.winner(), Some(TeamId(0)));
        // The eliminated player stays eliminated, not "left".
        assert!(m.is_eliminated(g[1][0]));
    }

    #[test]
    fn dead_leaver_does_not_block_respawn_of_others_and_cannot_respawn() {
        let (mut m, g) = setup(2, 2);
        to_contraction(&mut m);
        die(&mut m, g[0][0]);
        m.apply(Event::PlayerLeft(g[0][0]), 0).unwrap();
        assert!(!m.may_respawn(g[0][0]));
        assert_eq!(
            m.apply(Event::PlayerRespawned(g[0][0]), 0),
            Err(EventError::InvalidPlayerState)
        );
        assert_eq!(
            m.apply(Event::PlayerLeft(g[0][0]), 0).map(|_| ()),
            Ok(()) // leaving twice is harmless
        );
    }

    #[test]
    fn single_team_never_auto_wins() {
        let (mut m, g) = setup(1, 2);
        to_contraction(&mut m);
        m.apply(Event::PlayerLeft(g[0][0]), 0).unwrap();
        assert_eq!(m.winner(), None);
        assert_eq!(m.phase(), Phase::Night2Contraction);
    }

    #[test]
    fn everyone_leaves_in_sudden_death_is_a_draw() {
        let (mut m, g) = setup(2, 1);
        to_contraction(&mut m);
        m.apply(Event::ArenaFullyClosed, 0).unwrap();
        let (fx, _) = m.apply_batch(&[Event::PlayerLeft(g[0][0]), Event::PlayerLeft(g[1][0])], 0);
        assert_eq!(fx.last(), Some(&Effect::DeclareDraw));
    }

    #[test]
    fn protection_expires_by_time() {
        let (mut m, g) = setup(2, 1);
        to_contraction(&mut m);
        let a = g[0][0];
        die(&mut m, a);
        let fx = m.apply(Event::PlayerRespawned(a), 1000).unwrap();
        assert_eq!(
            fx,
            vec![Effect::GrantProtection {
                player: a,
                until: 6000
            }]
        );
        assert!(m.is_protected(a, 1000));
        assert!(m.is_protected(a, 5999));
        assert!(!m.is_protected(a, 6000));
        assert!(m.tick(5999).is_empty());
        assert_eq!(m.tick(6000), vec![Effect::EndProtection { player: a }]);
        assert!(m.tick(7000).is_empty());
    }

    #[test]
    fn protection_ends_on_attack() {
        let (mut m, g) = setup(2, 1);
        to_contraction(&mut m);
        let a = g[0][0];
        // Attack without protection: nothing.
        assert!(m.apply(Event::PlayerAttacked(a), 0).unwrap().is_empty());
        die(&mut m, a);
        m.apply(Event::PlayerRespawned(a), 0).unwrap();
        let fx = m.apply(Event::PlayerAttacked(a), 100).unwrap();
        assert_eq!(fx, vec![Effect::EndProtection { player: a }]);
        assert!(!m.is_protected(a, 101));
        assert!(m.apply(Event::PlayerAttacked(a), 200).unwrap().is_empty());
        assert!(m.tick(10_000).is_empty());
    }

    #[test]
    fn late_attack_after_expiry_still_clears_once() {
        let (mut m, g) = setup(2, 1);
        to_contraction(&mut m);
        let a = g[0][0];
        die(&mut m, a);
        m.apply(Event::PlayerRespawned(a), 0).unwrap();
        let fx = m.apply(Event::PlayerAttacked(a), 9000).unwrap();
        assert_eq!(fx, vec![Effect::EndProtection { player: a }]);
    }

    #[test]
    fn dying_clears_protection_and_closure_clears_all() {
        let (mut m, g) = setup(2, 2);
        to_contraction(&mut m);
        let (a, b) = (g[0][0], g[1][0]);
        die(&mut m, a);
        m.apply(Event::PlayerRespawned(a), 0).unwrap();
        die(&mut m, a);
        assert!(!m.is_protected(a, 1));
        die(&mut m, b);
        m.apply(Event::PlayerRespawned(b), 0).unwrap();
        m.apply(Event::ArenaFullyClosed, 1).unwrap();
        assert!(!m.is_protected(b, 2));
    }

    #[test]
    fn configurable_protection_length() {
        let ids = [pid(1), pid(2)];
        let r = assign_teams(&ids, 1);
        let mut m = MatchState::new(&r, MatchConfig { protection_ms: 250 });
        to_contraction(&mut m);
        die(&mut m, ids[0]);
        m.apply(Event::PlayerRespawned(ids[0]), 100).unwrap();
        assert!(m.is_protected(ids[0], 349));
        assert!(!m.is_protected(ids[0], 350));
    }

    #[test]
    fn respawn_requires_death() {
        let (mut m, g) = setup(2, 1);
        to_contraction(&mut m);
        assert_eq!(
            m.apply(Event::PlayerRespawned(g[0][0]), 0),
            Err(EventError::InvalidPlayerState)
        );
    }
}
