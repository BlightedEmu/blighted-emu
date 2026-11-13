// SPDX-License-Identifier: AGPL-3.0-or-later
//! The matchmaking queue of the world (the 3v3 arena: arena flow study 2026-11-11; Outpost Rush:
//! Outpost Rush study 2026-11-12; capture the flag: capture the flag study 2026-11-12), with no
//! I/O: who waits, which players form a match and in
//! which team, the accepts, the cancel of a match not every player accepted, and the opening of
//! the match.
//!
//! Observed (retail `3v3`, `20261010-125035-658`, `3v3_arena_group_2`, `Hive_3v3_group`):
//!
//! - a pop names the match's size (2 in the 1v1 matches of the `3v3` and `20261010-125035-658`
//!   ledgers, 6 in the one 3v3 match);
//! - every accept of any player raises the accepted count of every player of the match (`3v3_arena_group_2`
//!   667.5 to 670.3 s: 2, 3, 4, 5);
//! - when every player accepted, the move starts (5449, 0 to 2 s after the last accept);
//! - a match not every player accepted ends: the players who did not accept leave the queue
//!   (`3v3_arena_group_2` 476.94 s, 26.4 s after the pop), the players who accepted stay queued
//!   with a new join time (6228; `Hive_3v3_group` 1573.390 s, 27.0 s after the pop; `3v3` 922.022 s,
//!   46.7 s after the pop).
//!
//! Outpost Rush ([`MatchFlow::EachAccept`]; retail `opr_que_and_lost`, `OPR-coral-divide2`,
//! `OPR_TEST3`): a pop names 40 desired players and status 10; the status turns to 1 (the
//! invite) 42.8 s later (`opr_que_and_lost` 530.716 to 573.565 s), or at once when the match
//! already runs (`OPR-coral-divide2` 96.6 s, `OPR_TEST3` 212.7 s: status 1 with the match id and
//! no status 10); each player moves 0.3 to 0.4 s after its own accept (5449 in 3 of 3 ledgers),
//! and no accepted count is sent.
//!
//! Capture the flag uses the same flow (retail `CTF01`,
//! `CTF_QueueAndJoin_PickUpSpeedHealthAndFlagThenDied`): a pop names 20 desired players and
//! status 10, the status turns to 1 1.2 s and 2.6 s later (`CTF01` 312.929 to 314.145 s;
//! 37.023 to 39.588 s), the player moves 0.43 s after its own accept (5741 43.389 s, 5449
//! 43.821 s), and no accepted count is sent.
//!
//! The catacombs ([`MatchSettings::catacombs`]; retail `CATACOMBS`, `CATACOMBS2`, `CATACOMBS5`,
//! `CATACOMBS6`, `Hive catacomes test 2`, `Hive catacombs test 4`) play one team of 3. A join
//! without the fill byte is matched alone at once, with no pop: status 1 and the match id 4.8 s
//! (`CATACOMBS2` 7.006 to 11.805 s) and 5.6 s (`CATACOMBS` 2.455 to 8.055 s) after the queue
//! entry. A join with it waits for other players: a full team of 3 pops 1.45 s after the entry
//! (`Hive catacombs test 4` 106.205 to 107.657 s), a team of 1 or 2 pops 62.2, 63.3 and 65.6 s
//! after it (`CATACOMBS5`, `Hive catacomes test 2`, `CATACOMBS6`) with the entry's match size
//! the players found and 3 desired players; status 1 follows 2.0 to 4.9 s after the pop, and
//! each player moves 0.5 to 0.7 s after its own accept, as in Outpost Rush. Inferred: the fill
//! wait is one setting (60 s by default); the status 1 goes out when the instance opened, as
//! Outpost Rush's invite.
//!
//! Inferred: the queue is first come, first served; a group plays in one team; a team takes at
//! most [`MatchSettings::max_healers_per_team`] healers (`MatchmakingMaxHealers` 1 of the
//! `Arena3v3` row; for Outpost Rush the row's 1 per group of [`GROUP_SIZE`]); the accept window
//! is one setting per game mode (the retail windows differ: Unknown rule); the invite of an
//! Outpost Rush match is sent when its instance opened (the 42.8 s wait matches an opening, and
//! a running match invites at once).

use std::collections::BTreeSet;
use std::time::Duration;

use nw_bus::{BusId, MatchPlace, QueueJoin, QueueNews, QueueNewsKind, SimSlot};
use std::collections::HashMap;

use nw_content::instances::{
    CAPTURE_THE_FLAG_GAME_MODE, CATACOMBS_GAME_MODE, CATACOMBS_TEAM_PLAYERS, OUTPOST_RUSH_GAME_MODE,
};
use uuid::Uuid;

/// The teams of a match.
pub const TEAMS: u32 = 2;
/// The role of a healer (6272; Inferred: the role the life-staff player of
/// `20261010-125035-658` picked).
pub const HEALER_ROLE: u8 = 4;
/// Healers a team takes at most (`MatchmakingMaxHealers` of `Arena3v3`), and an Outpost Rush
/// group of [`GROUP_SIZE`] (`MatchmakingMaxHealers` of `OutpostRush`).
pub const MAX_HEALERS_PER_TEAM: usize = 1;
/// Players of one group of a raid (Inferred: the role limits of the `OutpostRush` row, 4 DPS,
/// 4 tanks, 1 healer, fit a group of five).
pub const GROUP_SIZE: usize = 5;
/// Healers a capture the flag team takes at most (Observed: `MatchmakingMaxHealers` 2 of the
/// `CaptureTheFlag` row; Inferred: per team, as the arena's 1).
pub const CAPTURE_THE_FLAG_MAX_HEALERS: usize = 2;

/// How the players of a found match go to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchFlow {
    /// The instance opens when every player accepted, then every player moves (the arena).
    AllAccept,
    /// The instance opens at the pop and each player moves at its own accept (Outpost Rush).
    EachAccept,
}

/// The teams of a catacombs run (`NumTeams` 1 of the retail row).
pub const CATACOMBS_TEAMS: u32 = 1;
/// Healers a catacombs team takes at most (`MatchmakingMaxHealers` 1 of the `Catacombs` row).
pub const CATACOMBS_MAX_HEALERS: usize = 1;

/// The rules of the queue of one game mode, from the configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchSettings {
    /// Players of a match: [`MatchSettings::teams`] teams of the same size.
    pub match_size: u32,
    /// Teams of a match.
    pub teams: u32,
    /// How long a join with the fill byte ([`QueueJoin::fill`]) waits for other players before
    /// its team forms with the players found; `None` when a match forms only when full.
    pub fill_wait: Option<Duration>,
    /// How long the players have to accept a match.
    pub accept_timeout: Duration,
    /// How the players go to the match.
    pub flow: MatchFlow,
    /// Healers a team takes at most.
    pub max_healers_per_team: usize,
}

impl MatchSettings {
    /// The arena's rules with `match_size` players and the accept window `accept_timeout`.
    pub const fn arena(match_size: u32, accept_timeout: Duration) -> Self {
        Self {
            match_size,
            teams: TEAMS,
            fill_wait: None,
            accept_timeout,
            flow: MatchFlow::AllAccept,
            max_healers_per_team: MAX_HEALERS_PER_TEAM,
        }
    }

    /// Outpost Rush's rules with `match_size` players and the accept window `accept_timeout`:
    /// one healer per group of a team.
    pub fn outpost_rush(match_size: u32, accept_timeout: Duration) -> Self {
        let team_size = usize::try_from(match_size / TEAMS).unwrap_or(usize::MAX);
        Self {
            match_size,
            teams: TEAMS,
            fill_wait: None,
            accept_timeout,
            flow: MatchFlow::EachAccept,
            max_healers_per_team: team_size.div_ceil(GROUP_SIZE) * MAX_HEALERS_PER_TEAM,
        }
    }

    /// Capture the flag's rules with `match_size` players and the accept window
    /// `accept_timeout`: each player moves at its own accept, as in Outpost Rush.
    pub const fn capture_the_flag(match_size: u32, accept_timeout: Duration) -> Self {
        Self {
            match_size,
            teams: TEAMS,
            fill_wait: None,
            accept_timeout,
            flow: MatchFlow::EachAccept,
            max_healers_per_team: CAPTURE_THE_FLAG_MAX_HEALERS,
        }
    }

    /// The catacombs' rules with the accept window `accept_timeout` and the fill wait
    /// `fill_wait`: one team of [`CATACOMBS_TEAM_PLAYERS`], each player moves at its own
    /// accept.
    pub const fn catacombs(accept_timeout: Duration, fill_wait: Duration) -> Self {
        Self {
            match_size: CATACOMBS_TEAM_PLAYERS,
            teams: CATACOMBS_TEAMS,
            fill_wait: Some(fill_wait),
            accept_timeout,
            flow: MatchFlow::EachAccept,
            max_healers_per_team: CATACOMBS_MAX_HEALERS,
        }
    }
}

/// The rules of every game mode of the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueRules {
    /// The 3v3 arena.
    pub arena: MatchSettings,
    /// Outpost Rush.
    pub outpost_rush: MatchSettings,
    /// Capture the flag.
    pub capture_the_flag: MatchSettings,
    /// The catacombs.
    pub catacombs: MatchSettings,
}

impl QueueRules {
    /// The rules of game mode `game_mode`.
    pub fn of(&self, game_mode: &str) -> &MatchSettings {
        match game_mode {
            OUTPOST_RUSH_GAME_MODE => &self.outpost_rush,
            CAPTURE_THE_FLAG_GAME_MODE => &self.capture_the_flag,
            CATACOMBS_GAME_MODE => &self.catacombs,
            _ => &self.arena,
        }
    }
}

/// A player of a match.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seat {
    join: QueueJoin,
    team: u8,
    place: u8,
}

/// A match the players may accept.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Proposal {
    id: Uuid,
    game_mode: String,
    level: String,
    seats: Vec<Seat>,
    accepted: BTreeSet<Uuid>,
    /// Unix milliseconds when the accept window ends.
    deadline_ms: u64,
    /// The instance is opening (an arena match: every player accepted; Outpost Rush: from the
    /// pop).
    opening: bool,
    flow: MatchFlow,
    /// The open instance of an Outpost Rush match.
    opened: Option<OpenedMatch>,
    /// The players got the pop; a catacombs join without the fill byte gets none
    /// ([`QueueNewsKind::Matched`] at the opening instead of [`QueueNewsKind::Invited`]).
    popped: bool,
}

impl Proposal {
    /// The place of seat `seat` in the open instance `opened`.
    fn ready(&self, seat: &Seat, opened: &OpenedMatch) -> Outcome {
        Outcome::News {
            sim: seat.join.sim.clone(),
            news: QueueNews {
                character: seat.join.character,
                game_mode: self.game_mode.clone(),
                kind: QueueNewsKind::Ready(MatchPlace {
                    match_id: self.id,
                    instance: opened.instance,
                    sim: opened.sim.clone(),
                    phase: opened.phase,
                    level: self.level.clone(),
                    team: seat.team,
                    place: seat.place,
                }),
            },
        }
    }

    /// The opening of the instance of the match.
    fn opening(&self) -> MatchOpening {
        MatchOpening {
            match_id: self.id,
            game_mode: self.game_mode.clone(),
            level: self.level.clone(),
            members: self.seats.iter().map(|seat| seat.join.character).collect(),
        }
    }
}

/// A match to open: its instance on an instance sim of `level`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchOpening {
    /// The match.
    pub match_id: Uuid,
    /// The game mode.
    pub game_mode: String,
    /// The level folder.
    pub level: String,
    /// Its players.
    pub members: Vec<Uuid>,
}

/// What the loop does after a step of the queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Send `news` to the sim `sim`.
    News {
        /// The player's sim.
        sim: BusId,
        /// The news.
        news: QueueNews,
    },
    /// Open the match's instance.
    Open(MatchOpening),
}

/// The open instance of an accepted match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedMatch {
    /// The instance.
    pub instance: Uuid,
    /// The instance sim.
    pub sim: SimSlot,
    /// The phase id.
    pub phase: u64,
}

/// The queue.
#[derive(Debug, Default)]
pub struct Matchmaker {
    /// The queued players, oldest first.
    queue: Vec<QueueJoin>,
    proposals: Vec<Proposal>,
    /// Unix ms of the first step that saw each queued player.
    waiting_since: HashMap<Uuid, u64>,
}

impl Matchmaker {
    /// Player `join.character` queues; a player already queued queues again (its newest call
    /// holds, at the end of the queue), one in a match is not queued twice.
    pub fn join(&mut self, join: QueueJoin) {
        if self.proposal_of(join.character).is_some() {
            return;
        }
        self.queue
            .retain(|queued| queued.character != join.character);
        self.waiting_since.remove(&join.character);
        self.queue.push(join);
    }

    /// Player `character` leaves the queue; a match it was in and that is not opening yet is
    /// cancelled for the others, who stay queued; an Outpost Rush match only loses its seat
    /// (Inferred).
    pub fn leave(&mut self, character: Uuid) -> Vec<Outcome> {
        self.queue.retain(|queued| queued.character != character);
        self.waiting_since.remove(&character);
        let Some(index) = self.proposal_of(character) else {
            return Vec::new();
        };
        if self.proposals[index].flow == MatchFlow::EachAccept {
            self.proposals[index]
                .seats
                .retain(|seat| seat.join.character != character);
            return Vec::new();
        }
        if self.proposals[index].opening {
            return Vec::new();
        }
        let proposal = self.proposals.remove(index);
        let others: Vec<QueueJoin> = proposal
            .seats
            .into_iter()
            .map(|seat| seat.join)
            .filter(|join| join.character != character)
            .collect();
        self.requeue(others)
    }

    /// Player `character` accepted match `match_id`: in an arena match the count goes to every
    /// player of the match and the last accept opens it; in an open Outpost Rush match the
    /// player moves.
    pub fn accept(&mut self, character: Uuid, match_id: Uuid) -> Vec<Outcome> {
        let Some(proposal) = self.proposals.iter_mut().find(|proposal| {
            proposal.id == match_id && (proposal.flow == MatchFlow::EachAccept || !proposal.opening)
        }) else {
            return Vec::new();
        };
        let Some(seat) = proposal
            .seats
            .iter()
            .find(|seat| seat.join.character == character)
            .cloned()
        else {
            return Vec::new();
        };
        if !proposal.accepted.insert(character) {
            return Vec::new();
        }
        if proposal.flow == MatchFlow::EachAccept {
            return proposal
                .opened
                .clone()
                .map(|opened| proposal.ready(&seat, &opened))
                .into_iter()
                .collect();
        }
        let count = u32::try_from(proposal.accepted.len()).unwrap_or(u32::MAX);
        let mut outcomes = news_to_all(proposal, &QueueNewsKind::Accepted { count });
        if proposal.accepted.len() == proposal.seats.len() {
            proposal.opening = true;
            outcomes.push(Outcome::Open(proposal.opening()));
        }
        outcomes
    }

    /// The instance of match `match_id` opened (`Some`) or did not: every player of an arena
    /// match gets its place, the players of an Outpost Rush match get the invite (and the ones
    /// that accepted already their place), or the match is cancelled and every player stays
    /// queued.
    pub fn opened(&mut self, match_id: Uuid, opened: Option<OpenedMatch>) -> Vec<Outcome> {
        let Some(index) = self
            .proposals
            .iter()
            .position(|proposal| proposal.id == match_id)
        else {
            return Vec::new();
        };
        let Some(opened) = opened else {
            let proposal = self.proposals.remove(index);
            let joins = proposal.seats.into_iter().map(|seat| seat.join).collect();
            return self.requeue(joins);
        };
        if self.proposals[index].flow == MatchFlow::EachAccept {
            let proposal = &mut self.proposals[index];
            proposal.opened = Some(opened.clone());
            let proposal = &self.proposals[index];
            return proposal
                .seats
                .iter()
                .map(|seat| {
                    if proposal.accepted.contains(&seat.join.character) {
                        proposal.ready(seat, &opened)
                    } else {
                        let kind = if proposal.popped {
                            QueueNewsKind::Invited
                        } else {
                            QueueNewsKind::Matched {
                                match_id: proposal.id,
                            }
                        };
                        Outcome::News {
                            sim: seat.join.sim.clone(),
                            news: QueueNews {
                                character: seat.join.character,
                                game_mode: proposal.game_mode.clone(),
                                kind,
                            },
                        }
                    }
                })
                .collect();
        }
        let proposal = self.proposals.remove(index);
        proposal
            .seats
            .iter()
            .map(|seat| proposal.ready(seat, &opened))
            .collect()
    }

    /// One step at Unix millisecond `now_ms`: the matches whose accept window ended are
    /// cancelled (an open Outpost Rush match only drops the players that did not accept), then
    /// the queue forms the matches it can (`new_id` names each) with the rules of their game
    /// mode.
    pub fn tick(
        &mut self,
        rules: &QueueRules,
        now_ms: u64,
        mut new_id: impl FnMut() -> Uuid,
    ) -> Vec<Outcome> {
        let mut outcomes = Vec::new();
        let (ended, open): (Vec<Proposal>, Vec<Proposal>) = std::mem::take(&mut self.proposals)
            .into_iter()
            .partition(|proposal| {
                (proposal.flow == MatchFlow::EachAccept || !proposal.opening)
                    && now_ms >= proposal.deadline_ms
            });
        self.proposals = open;
        for proposal in ended {
            let moved = proposal.opened.is_some();
            let (accepted, declined): (Vec<Seat>, Vec<Seat>) = proposal
                .seats
                .into_iter()
                .partition(|seat| proposal.accepted.contains(&seat.join.character));
            for seat in declined {
                outcomes.push(Outcome::News {
                    sim: seat.join.sim.clone(),
                    news: QueueNews {
                        character: seat.join.character,
                        game_mode: seat.join.game_mode.clone(),
                        kind: QueueNewsKind::Dropped,
                    },
                });
            }
            if !moved {
                outcomes.extend(self.requeue(accepted.into_iter().map(|seat| seat.join).collect()));
            }
        }
        for queued in &self.queue {
            self.waiting_since.entry(queued.character).or_insert(now_ms);
        }
        while let Some((settings, seats, popped)) = self.form(rules, now_ms) {
            let Some(first) = seats.first() else {
                break;
            };
            let window_ms = u64::try_from(settings.accept_timeout.as_millis()).unwrap_or(u64::MAX);
            let characters: Vec<Uuid> = seats.iter().map(|seat| seat.join.character).collect();
            self.queue
                .retain(|queued| !characters.contains(&queued.character));
            for character in &characters {
                self.waiting_since.remove(character);
            }
            let players = u32::try_from(seats.len()).unwrap_or(u32::MAX);
            let proposal = Proposal {
                id: new_id(),
                game_mode: first.join.game_mode.clone(),
                level: first.join.level.clone(),
                seats,
                accepted: BTreeSet::new(),
                deadline_ms: now_ms.saturating_add(window_ms),
                opening: settings.flow == MatchFlow::EachAccept,
                flow: settings.flow,
                opened: None,
                popped,
            };
            if popped {
                outcomes.extend(news_to_all(
                    &proposal,
                    &QueueNewsKind::Popped {
                        match_id: proposal.id,
                        size: players,
                    },
                ));
            }
            if proposal.opening {
                outcomes.push(Outcome::Open(proposal.opening()));
            }
            tracing::info!(match_id = %proposal.id, players = proposal.seats.len(), level = %proposal.level, "match found");
            self.proposals.push(proposal);
        }
        outcomes
    }

    /// The rules and the players of the next match at Unix ms `now_ms`, of the queued game modes
    /// in the order of their oldest player, and whether its players get the pop.
    fn form(&self, rules: &QueueRules, now_ms: u64) -> Option<(MatchSettings, Vec<Seat>, bool)> {
        let mut modes: Vec<&str> = Vec::new();
        for queued in &self.queue {
            if !modes.contains(&queued.game_mode.as_str()) {
                modes.push(&queued.game_mode);
            }
        }
        modes.into_iter().find_map(|game_mode| {
            let settings = *rules.of(game_mode);
            match settings.fill_wait {
                Some(wait) => self.form_filled(game_mode, settings, wait, now_ms),
                None => self
                    .form_teams(game_mode, settings)
                    .map(|seats| (settings, seats, true)),
            }
        })
    }

    /// The queued players of `game_mode` in units that play together, oldest first: a group
    /// whole, else the player alone.
    fn units(&self, game_mode: &str) -> Vec<Vec<&QueueJoin>> {
        let mut taken: BTreeSet<Uuid> = BTreeSet::new();
        let mut units = Vec::new();
        for queued in self
            .queue
            .iter()
            .filter(|queued| queued.game_mode == game_mode)
        {
            if taken.contains(&queued.character) {
                continue;
            }
            let unit: Vec<&QueueJoin> = match queued.group {
                Some(group) => self
                    .queue
                    .iter()
                    .filter(|other| other.group == Some(group) && other.game_mode == game_mode)
                    .collect(),
                None => vec![queued],
            };
            taken.extend(unit.iter().map(|join| join.character));
            units.push(unit);
        }
        units
    }

    /// The one-team match of a mode with a fill wait (the catacombs): a unit without the fill
    /// byte plays alone at once, with no pop; a unit with it takes the later units with it that
    /// fit, and forms with the pop when the team is full or when it waited `wait`.
    fn form_filled(
        &self,
        game_mode: &str,
        settings: MatchSettings,
        wait: Duration,
        now_ms: u64,
    ) -> Option<(MatchSettings, Vec<Seat>, bool)> {
        let team_size = usize::try_from(settings.match_size / settings.teams.max(1)).ok()?;
        let wait_ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
        let units = self.units(game_mode);
        let one_team = |team: Vec<&QueueJoin>| -> Vec<Seat> {
            team.into_iter()
                .zip(0_u8..)
                .map(|(join, place)| Seat {
                    join: join.clone(),
                    team: 0,
                    place,
                })
                .collect()
        };
        let healers =
            |joins: &[&QueueJoin]| joins.iter().filter(|join| join.role == HEALER_ROLE).count();
        for (index, unit) in units.iter().enumerate() {
            if !unit.iter().any(|join| join.fill) {
                return Some((settings, one_team(unit.clone()), false));
            }
            let mut team = unit.clone();
            for other in units.iter().skip(index + 1) {
                if other.iter().any(|join| join.fill)
                    && team.len() + other.len() <= team_size
                    && healers(&team) + healers(other) <= settings.max_healers_per_team
                {
                    team.extend(other.iter().copied());
                }
            }
            let waited = unit.first().is_some_and(|lead| {
                self.waiting_since
                    .get(&lead.character)
                    .is_some_and(|since| now_ms >= since.saturating_add(wait_ms))
            });
            if team.len() >= team_size || waited {
                return Some((settings, one_team(team), true));
            }
        }
        None
    }

    /// The match of `game_mode` in `settings.teams` teams: first come, first served, a group
    /// whole in one team, at most the rules' healers a team.
    fn form_teams(&self, game_mode: &str, settings: MatchSettings) -> Option<Vec<Seat>> {
        let team_count = usize::try_from(settings.teams)
            .ok()
            .filter(|count| *count > 0)?;
        let team_size = usize::try_from(settings.match_size / settings.teams)
            .ok()
            .filter(|size| *size > 0)?;
        let mut teams: Vec<Vec<QueueJoin>> = vec![Vec::new(); team_count];
        for unit in self.units(game_mode) {
            let healers = unit.iter().filter(|join| join.role == HEALER_ROLE).count();
            let fits = |team: &Vec<QueueJoin>| {
                team.len() + unit.len() <= team_size
                    && team.iter().filter(|join| join.role == HEALER_ROLE).count() + healers
                        <= settings.max_healers_per_team
            };
            if let Some(team) = teams.iter_mut().find(|team| fits(team)) {
                team.extend(unit.into_iter().cloned());
            }
            if teams.iter().all(|team| team.len() == team_size) {
                return Some(
                    teams
                        .into_iter()
                        .zip(0_u8..)
                        .flat_map(|(team, index)| {
                            team.into_iter().zip(0_u8..).map(move |(join, place)| Seat {
                                join,
                                team: index,
                                place,
                            })
                        })
                        .collect(),
                );
            }
        }
        None
    }

    /// The players `joins` queue again (their sims give them a new join time, as retail) and get
    /// the cancel.
    fn requeue(&mut self, joins: Vec<QueueJoin>) -> Vec<Outcome> {
        joins
            .into_iter()
            .map(|join| {
                let outcome = Outcome::News {
                    sim: join.sim.clone(),
                    news: QueueNews {
                        character: join.character,
                        game_mode: join.game_mode.clone(),
                        kind: QueueNewsKind::Cancelled,
                    },
                };
                self.queue.push(join);
                outcome
            })
            .collect()
    }

    fn proposal_of(&self, character: Uuid) -> Option<usize> {
        self.proposals.iter().position(|proposal| {
            proposal
                .seats
                .iter()
                .any(|seat| seat.join.character == character)
        })
    }

    /// Players waiting in the queue.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }
}

/// The news `kind` for every player of `proposal`.
fn news_to_all(proposal: &Proposal, kind: &QueueNewsKind) -> Vec<Outcome> {
    proposal
        .seats
        .iter()
        .map(|seat| Outcome::News {
            sim: seat.join.sim.clone(),
            news: QueueNews {
                character: seat.join.character,
                game_mode: proposal.game_mode.clone(),
                kind: kind.clone(),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARENA: &str = "Arena3v3";
    const LEVEL: &str = "nw_arena01";
    const DPS_ROLE: u8 = 3;
    const NOW_MS: u64 = 1_000_000;
    const WINDOW: Duration = Duration::from_secs(45);
    /// The test size of an Outpost Rush match: two teams of two.
    const SMALL_RUSH: u32 = 4;
    /// The test size of a capture the flag match: two teams of one.
    const SMALL_FLAG: u32 = 2;
    /// The test fill wait of a catacombs join.
    const FILL_WAIT: Duration = Duration::from_secs(60);
    const THREE_VS_THREE: QueueRules = rules(6);
    const ONE_VS_ONE: QueueRules = rules(2);

    /// The rules of an arena of `arena_size` players and a small Outpost Rush.
    const fn rules(arena_size: u32) -> QueueRules {
        QueueRules {
            arena: MatchSettings::arena(arena_size, WINDOW),
            outpost_rush: MatchSettings {
                match_size: SMALL_RUSH,
                teams: TEAMS,
                fill_wait: None,
                accept_timeout: WINDOW,
                flow: MatchFlow::EachAccept,
                max_healers_per_team: MAX_HEALERS_PER_TEAM,
            },
            capture_the_flag: MatchSettings::capture_the_flag(SMALL_FLAG, WINDOW),
            catacombs: MatchSettings::catacombs(WINDOW, FILL_WAIT),
        }
    }

    fn sim() -> BusId {
        BusId::try_from("sim-1".to_owned()).expect("id")
    }

    fn join(number: u128, group: Option<u128>, role: u8) -> QueueJoin {
        QueueJoin {
            character: Uuid::from_u128(number),
            sim: sim(),
            group: group.map(Uuid::from_u128),
            role,
            game_mode: ARENA.to_owned(),
            level: LEVEL.to_owned(),
            fill: group.is_some(),
        }
    }

    fn ids() -> impl FnMut() -> Uuid {
        let mut next = 100;
        move || {
            next += 1;
            Uuid::from_u128(next)
        }
    }

    fn news(outcomes: &[Outcome]) -> Vec<(Uuid, QueueNewsKind)> {
        outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                Outcome::News { news, .. } => Some((news.character, news.kind.clone())),
                Outcome::Open(_) => None,
            })
            .collect()
    }

    #[test]
    fn six_players_form_two_teams_and_the_last_accept_opens_the_match() {
        let mut queue = Matchmaker::default();
        for number in 1..=5 {
            queue.join(join(number, None, DPS_ROLE));
        }
        assert!(
            queue.tick(&THREE_VS_THREE, NOW_MS, ids()).is_empty(),
            "five wait"
        );
        queue.join(join(6, None, DPS_ROLE));
        let popped = queue.tick(&THREE_VS_THREE, NOW_MS, ids());
        let popped = news(&popped);
        assert_eq!(popped.len(), 6);
        let QueueNewsKind::Popped { match_id, size } = popped[0].1 else {
            panic!("a pop");
        };
        assert_eq!(size, 6);
        assert_eq!(queue.queued(), 0);
        for number in 1..=5 {
            let counted = queue.accept(Uuid::from_u128(number), match_id);
            assert_eq!(counted.len(), 6, "every player gets the count");
            assert!(
                !counted
                    .iter()
                    .any(|outcome| matches!(outcome, Outcome::Open(_)))
            );
        }
        let last = queue.accept(Uuid::from_u128(6), match_id);
        let Some(Outcome::Open(opening)) = last.last() else {
            panic!("the match opens");
        };
        assert_eq!(opening.members.len(), 6);
        assert_eq!(opening.level, LEVEL);
        let opened = OpenedMatch {
            instance: Uuid::from_u128(50),
            sim: SimSlot {
                id: sim(),
                generation: 1,
            },
            phase: 9,
        };
        let ready = news(&queue.opened(match_id, Some(opened)));
        let teams: Vec<(u8, u8)> = ready
            .iter()
            .map(|(_, kind)| match kind {
                QueueNewsKind::Ready(place) => (place.team, place.place),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(teams, [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2)]);
    }

    #[test]
    fn a_group_plays_in_one_team_and_a_team_takes_one_healer() {
        let mut queue = Matchmaker::default();
        queue.join(join(1, None, HEALER_ROLE));
        queue.join(join(2, Some(9), DPS_ROLE));
        queue.join(join(3, Some(9), HEALER_ROLE));
        queue.join(join(4, None, HEALER_ROLE));
        queue.join(join(5, None, DPS_ROLE));
        queue.join(join(6, None, DPS_ROLE));
        queue.join(join(7, None, DPS_ROLE));
        let popped = news(&queue.tick(&THREE_VS_THREE, NOW_MS, ids()));
        let players: BTreeSet<u128> = popped
            .iter()
            .map(|(character, _)| character.as_u128())
            .collect();
        assert!(!players.contains(&4), "a third healer has no team");
        assert_eq!(players.len(), 6);
        assert_eq!(queue.queued(), 1);
    }

    #[test]
    fn a_window_that_ends_drops_who_did_not_accept_and_requeues_the_others() {
        let mut queue = Matchmaker::default();
        queue.join(join(1, None, DPS_ROLE));
        queue.join(join(2, None, DPS_ROLE));
        let popped = news(&queue.tick(&ONE_VS_ONE, NOW_MS, ids()));
        let QueueNewsKind::Popped { match_id, size } = popped[0].1 else {
            panic!("a pop");
        };
        assert_eq!(size, 2);
        queue.accept(Uuid::from_u128(1), match_id);
        let window_ms = u64::try_from(WINDOW.as_millis()).expect("window");
        assert!(
            news(&queue.tick(&ONE_VS_ONE, NOW_MS + window_ms - 1, ids())).is_empty(),
            "the window is still open"
        );
        let ended = news(&queue.tick(&ONE_VS_ONE, NOW_MS + window_ms, ids()));
        assert!(ended.contains(&(Uuid::from_u128(1), QueueNewsKind::Cancelled)));
        assert!(ended.contains(&(Uuid::from_u128(2), QueueNewsKind::Dropped)));
        assert_eq!(queue.queued(), 1, "the player who accepted waits again");
    }

    #[test]
    fn a_leave_cancels_the_match_for_the_others_and_a_failed_opening_requeues_all() {
        let mut queue = Matchmaker::default();
        queue.join(join(1, None, DPS_ROLE));
        queue.join(join(2, None, DPS_ROLE));
        let popped = news(&queue.tick(&ONE_VS_ONE, NOW_MS, ids()));
        let QueueNewsKind::Popped { match_id, .. } = popped[0].1 else {
            panic!("a pop");
        };
        assert_eq!(
            news(&queue.leave(Uuid::from_u128(1))),
            [(Uuid::from_u128(2), QueueNewsKind::Cancelled)]
        );
        assert!(
            queue.accept(Uuid::from_u128(2), match_id).is_empty(),
            "no match left"
        );
        queue.join(join(1, None, DPS_ROLE));
        let popped = news(&queue.tick(&ONE_VS_ONE, NOW_MS, ids()));
        let QueueNewsKind::Popped { match_id, .. } = popped[0].1 else {
            panic!("a pop");
        };
        queue.accept(Uuid::from_u128(1), match_id);
        queue.accept(Uuid::from_u128(2), match_id);
        let cancelled = news(&queue.opened(match_id, None));
        assert_eq!(cancelled.len(), 2);
        assert!(
            cancelled
                .iter()
                .all(|(_, kind)| *kind == QueueNewsKind::Cancelled)
        );
        assert_eq!(queue.queued(), 2);
    }

    fn rush_join(number: u128) -> QueueJoin {
        QueueJoin {
            game_mode: OUTPOST_RUSH_GAME_MODE.to_owned(),
            level: "nw_opr_004_trench".to_owned(),
            ..join(number, None, DPS_ROLE)
        }
    }

    #[test]
    fn an_outpost_rush_match_opens_at_its_pop_and_each_player_moves_at_its_accept() {
        let mut queue = Matchmaker::default();
        for number in 1..=4 {
            queue.join(rush_join(number));
        }
        let popped = queue.tick(&THREE_VS_THREE, NOW_MS, ids());
        let Some(Outcome::Open(opening)) = popped.last() else {
            panic!("the match opens at its pop: {popped:?}");
        };
        let match_id = opening.match_id;
        assert_eq!(news(&popped).len(), 4, "every player gets the pop");
        assert!(
            news(&queue.accept(Uuid::from_u128(1), match_id)).is_empty(),
            "no count, not open"
        );
        let opened = OpenedMatch {
            instance: Uuid::from_u128(50),
            sim: SimSlot {
                id: sim(),
                generation: 1,
            },
            phase: 9,
        };
        let invited = news(&queue.opened(match_id, Some(opened)));
        assert!(
            matches!(invited[0].1, QueueNewsKind::Ready(_)),
            "the player that accepted moves"
        );
        assert!(
            invited[1..]
                .iter()
                .all(|(_, kind)| *kind == QueueNewsKind::Invited)
        );
        let moved = news(&queue.accept(Uuid::from_u128(2), match_id));
        assert_eq!(moved.len(), 1);
        assert!(
            matches!(&moved[0], (character, QueueNewsKind::Ready(place)) if *character == Uuid::from_u128(2) && place.team == 0)
        );
        let window_ms = u64::try_from(WINDOW.as_millis()).expect("window");
        let ended = news(&queue.tick(&THREE_VS_THREE, NOW_MS + window_ms, ids()));
        assert_eq!(
            ended,
            [
                (Uuid::from_u128(3), QueueNewsKind::Dropped),
                (Uuid::from_u128(4), QueueNewsKind::Dropped)
            ],
            "the players that did not accept leave the queue, the others stay in the match"
        );
        assert_eq!(queue.queued(), 0);
    }

    #[test]
    fn an_outpost_rush_team_takes_one_healer_per_group() {
        let rush = MatchSettings::outpost_rush(40, WINDOW);
        assert_eq!(rush.max_healers_per_team, 4);
        assert_eq!(rush.flow, MatchFlow::EachAccept);
        let rules = THREE_VS_THREE;
        assert_eq!(rules.of(OUTPOST_RUSH_GAME_MODE).match_size, SMALL_RUSH);
        assert_eq!(rules.of(ARENA).flow, MatchFlow::AllAccept);
    }

    #[test]
    fn a_capture_the_flag_match_opens_at_its_pop_and_its_player_moves_at_its_accept() {
        let flag = MatchSettings::capture_the_flag(20, WINDOW);
        assert_eq!(
            (flag.flow, flag.max_healers_per_team),
            (MatchFlow::EachAccept, CAPTURE_THE_FLAG_MAX_HEALERS)
        );
        let mut queue = Matchmaker::default();
        for number in 1..=2 {
            queue.join(QueueJoin {
                game_mode: CAPTURE_THE_FLAG_GAME_MODE.to_owned(),
                level: "nw_ctf_003_long".to_owned(),
                ..join(number, None, DPS_ROLE)
            });
        }
        let popped = queue.tick(&THREE_VS_THREE, NOW_MS, ids());
        assert!(
            news(&popped)
                .iter()
                .all(|(_, kind)| matches!(kind, QueueNewsKind::Popped { size, .. } if *size == SMALL_FLAG)),
            "both players get the pop with the match size"
        );
        let Some(Outcome::Open(opening)) = popped.last() else {
            panic!("the match opens at its pop: {popped:?}");
        };
        let match_id = opening.match_id;
        let opened = OpenedMatch {
            instance: Uuid::from_u128(51),
            sim: SimSlot {
                id: sim(),
                generation: 1,
            },
            phase: 7,
        };
        let invited = news(&queue.opened(match_id, Some(opened)));
        assert!(
            invited
                .iter()
                .all(|(_, kind)| *kind == QueueNewsKind::Invited)
        );
        let moved = news(&queue.accept(Uuid::from_u128(1), match_id));
        assert!(
            matches!(&moved[..], [(character, QueueNewsKind::Ready(place))] if *character == Uuid::from_u128(1) && place.level == "nw_ctf_003_long"),
            "only the player that accepted moves: {moved:?}"
        );
    }

    /// A catacombs join of player `number` with the fill byte `fill`.
    fn catacomb_join(number: u128, fill: bool, role: u8) -> QueueJoin {
        QueueJoin {
            game_mode: CATACOMBS_GAME_MODE.to_owned(),
            level: "nw_catacomb_00".to_owned(),
            fill,
            ..join(number, None, role)
        }
    }

    fn opened_match() -> OpenedMatch {
        OpenedMatch {
            instance: Uuid::from_u128(61),
            sim: SimSlot {
                id: sim(),
                generation: 1,
            },
            phase: 9,
        }
    }

    /// Retail `CATACOMBS2`: a join without the fill byte is matched alone with no pop; the
    /// opening gives it the match id, and its accept moves it.
    #[test]
    fn a_catacombs_join_without_fill_is_matched_alone_with_no_pop() {
        let mut queue = Matchmaker::default();
        queue.join(catacomb_join(1, false, DPS_ROLE));
        let outcomes = queue.tick(&THREE_VS_THREE, NOW_MS, ids());
        assert!(news(&outcomes).is_empty(), "no pop: {outcomes:?}");
        let [Outcome::Open(opening)] = &outcomes[..] else {
            panic!("the run opens at once: {outcomes:?}");
        };
        assert_eq!(opening.members, [Uuid::from_u128(1)]);
        let match_id = opening.match_id;
        assert_eq!(
            news(&queue.opened(match_id, Some(opened_match()))),
            [(Uuid::from_u128(1), QueueNewsKind::Matched { match_id })]
        );
        let moved = news(&queue.accept(Uuid::from_u128(1), match_id));
        assert!(
            matches!(&moved[..], [(_, QueueNewsKind::Ready(place))] if place.team == 0 && place.place == 0),
            "{moved:?}"
        );
    }

    /// Retail `Hive catacombs test 4` and `CATACOMBS6`: a full team of fill joins pops at once,
    /// a short team pops with the players found after the fill wait.
    #[test]
    fn catacombs_fill_joins_pop_when_full_or_after_the_wait() {
        let mut queue = Matchmaker::default();
        for number in 1..=3 {
            queue.join(catacomb_join(number, true, DPS_ROLE));
        }
        let full = queue.tick(&THREE_VS_THREE, NOW_MS, ids());
        assert_eq!(
            news(&full)
                .iter()
                .filter(|(_, kind)| matches!(kind, QueueNewsKind::Popped { size: 3, .. }))
                .count(),
            3,
            "{full:?}"
        );
        let mut queue = Matchmaker::default();
        for number in 4..=5 {
            queue.join(catacomb_join(number, true, DPS_ROLE));
        }
        assert!(queue.tick(&THREE_VS_THREE, NOW_MS, ids()).is_empty());
        let wait_ms = u64::try_from(FILL_WAIT.as_millis()).expect("ms");
        assert!(
            queue
                .tick(&THREE_VS_THREE, NOW_MS + wait_ms - 1, ids())
                .is_empty(),
            "the team waits for a third player"
        );
        let short = queue.tick(&THREE_VS_THREE, NOW_MS + wait_ms, ids());
        assert_eq!(
            news(&short)
                .iter()
                .filter(|(_, kind)| matches!(kind, QueueNewsKind::Popped { size: 2, .. }))
                .count(),
            2,
            "{short:?}"
        );
        let Some(Outcome::Open(opening)) = short.last() else {
            panic!("the run opens at its pop: {short:?}");
        };
        let invited = news(&queue.opened(opening.match_id, Some(opened_match())));
        assert!(
            invited
                .iter()
                .all(|(_, kind)| *kind == QueueNewsKind::Invited)
        );
    }

    /// A catacombs team takes one healer (`MatchmakingMaxHealers` 1), and a waiting fill team
    /// does not hold back a join of another mode.
    #[test]
    fn a_catacombs_team_takes_one_healer_and_does_not_block_other_modes() {
        let mut queue = Matchmaker::default();
        queue.join(catacomb_join(1, true, HEALER_ROLE));
        queue.join(catacomb_join(2, true, HEALER_ROLE));
        queue.join(join(3, None, DPS_ROLE));
        queue.join(join(4, None, DPS_ROLE));
        queue.join(catacomb_join(5, true, DPS_ROLE));
        let outcomes = queue.tick(&ONE_VS_ONE, NOW_MS, ids());
        assert!(
            news(&outcomes)
                .iter()
                .all(|(character, _)| [Uuid::from_u128(3), Uuid::from_u128(4)].contains(character)),
            "the arena match forms behind the waiting catacombs team: {outcomes:?}"
        );
        let wait_ms = u64::try_from(FILL_WAIT.as_millis()).expect("ms");
        let late = queue.tick(&ONE_VS_ONE, NOW_MS + wait_ms, ids());
        let size_of = |number: u128| {
            news(&late).iter().find_map(|(character, kind)| match kind {
                QueueNewsKind::Popped { size, .. } if *character == Uuid::from_u128(number) => {
                    Some(*size)
                }
                _ => None,
            })
        };
        assert_eq!(
            (size_of(1), size_of(5), size_of(2)),
            (Some(2), Some(2), Some(1)),
            "the first healer's team takes the DPS player, the second healer plays alone: {late:?}"
        );
        assert_eq!(queue.queued(), 0);
    }
}
