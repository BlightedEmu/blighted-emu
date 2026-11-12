// SPDX-License-Identifier: AGPL-3.0-or-later
//! The matchmaking queue of the 3v3 arena and of Outpost Rush on the player's sim (arena flow
//! study 2026-11-11, `local-data/findings/2026-11-11-arena-3v3-flow.md` sections 1 and 2;
//! Outpost Rush study 2026-11-12, `local-data/findings/2026-11-12-outpost-rush.md`): the
//! client's queue calls, the player's participant state (3312) at each step, the queue calls to
//! the director ([`nw_bus::matchmaking`]) and what the director sends back.
//!
//! Observed (retail `3v3`, `20261010-125035-658`, `3v3_arena_group`, `3v3_arena_group_2`,
//! `Hive_3v3_group`):
//!
//! 1. 6211 (open the screen) is answered with the 3312 `isReplicatingGroupActivity` and, on a
//!    subscribe, `groupActivityEligibility` (every call of the ledgers).
//! 2. 6254 `01 3316 0001 00` solo, `01 3316 0000 01` in a group (16 of 16 joins) is answered
//!    with 6295 `01 3316 <the u16>` 25 to 50 ms later.
//! 3. The client sends 6272 twice (1 at once, then the picked role); after the second the 3312
//!    holds the queue entry `(1fad9d42, <join ns>, 0, 0, 1)` and `matchmakingServiceActivity` 2.
//! 4. A pop: the entry's match size, status 10, the match id and the desired players; 2 s later
//!    status 1 (7 of 7 pops). Each accept of any player: the accepted count.
//! 5. 5741 (with the match id) and 946 come in one millisecond; when every player accepted: 5449,
//!    the client's 5450, status 5, and the move (3951) to the arena level.
//! 6. A match not every player accepted: the player who did not accept gets 4970 and an empty
//!    queue (activity `16`, status 0, no match); the others get 6228 and a renewed entry.
//! 7. 1066 is answered with 4970 and an empty queue; 6297 with 6296.
//! 8. A leave of a running match bars the queue for 30 minutes: the 3312
//!    `queueEligibleTimesForGameModes` (Unix ns), sent on every sim join.
//!
//! Observed for Outpost Rush (retail `opr_que_and_lost`, `OPR_TEST3`, `OPR-coral-divide2`): the
//! join is 6254 `01 3116 0001 00` solo (`OPR_TEST3` 12.510 s) or `01 3116 0000 01` in a group
//! (`opr_que_and_lost` 522.066 s), the same 6295 and two 6272; the queue entry names `91b99a18`
//! with `matchmakingServiceActivity` 0 (377.312 s, 526.751 s, `OPR_TEST3` 14.691 s); a pop sets
//! status 10, the match id and 40 desired players (530.716 s); the invite sets status 1 (573.565
//! s, or at once in a running match); the 5741 accept is answered with 5449 for that player
//! alone 0.3 to 0.4 s later (3 of 3), then 5450, status 5 and the move.
//!
//! Inferred: a queue join during the bar is refused with 1234 (no capture holds one); a player
//! who leaves the sim (a logout, a move to another sim) leaves the queue; the activity values of
//! the other modes (capture the flag `4316`, catacombs `3016`) are refused as before (their game
//! modes do not run here).

use bevy_ecs::entity::Entity;
use nw_bus::{ArenaSeat, MatchPlace, QueueCall, QueueJoin, QueueNews, QueueNewsKind};
use nw_content::instances::InstanceKind;
use nw_messages::facet::{NO_GAME_MODE, component};
use nw_messages::game_mode_participant::{
    ELIGIBILITY_LEN, GameModeParticipantState, MatchmakingService, QueueEntry,
};
use nw_messages::{FacetHeader, GameModeServerMsg, LeftGameModeQueueMsg, ServerMessage};
use nw_sim_core::Position;
use uuid::Uuid;

use super::game_mode_entity::full;
use super::{Player, SimWorld, send_now, unix_ns};

/// The 6254 activity of the 3v3 arena (Observed: 16 retail arena joins).
pub(super) const ARENA_ACTIVITY: u16 = 0x3316;
/// The 6254 activity of Outpost Rush (Observed: `opr_que_and_lost` 522.066 s, `OPR_TEST3`
/// 12.510 s).
pub(super) const OUTPOST_RUSH_ACTIVITY: u16 = 0x3116;
/// The first byte of every retail 6295.
const ROLE_SELECTION_FLAG: u8 = 1;
/// The 6272 calls of one join; the last one queues (Observed: two per join).
const ROLE_PICKS: u8 = 2;
/// `matchmakingServiceActivity` while queued for the arena, for Outpost Rush, and with no queue
/// (Observed).
const ARENA_QUEUED_ACTIVITY: u8 = 2;
const OUTPOST_RUSH_QUEUED_ACTIVITY: u8 = 0;
const IDLE_ACTIVITY: u8 = 0x16;
/// `matchmakingServiceStatus` values (Observed): none, a pop, the wait after it, the move.
const STATUS_NONE: u32 = 0;
const STATUS_POPPED: u32 = 10;
const STATUS_WAITING: u32 = 1;
const STATUS_MOVING: u32 = 5;
/// The status changes from a pop's to the wait this long after the pop (Observed: 2.0 s, 7 of
/// 7 pops).
const POPPED_STATUS_MS: u64 = 2_000;
/// The two values of every retail queue entry that are not named (Observed: 0 and 1).
const ENTRY_UNKNOWN: u32 = 0;
const ENTRY_FLAG: u8 = 1;
/// The match size of an entry before a pop.
const NOT_MATCHED: u32 = 0;
/// The `groupActivityEligibility` the sim sends with a subscribe: the bits of retail `3v3`
/// 661.769 s (their meaning is Unknown; every character had other bits).
const GROUP_ELIGIBILITY: [u8; ELIGIBILITY_LEN] = [
    0xbf, 0xf9, 0x7e, 0xfe, 0xbf, 0xef, 0x7f, 0xff, 0xff, 0xe7, 0x2f, 0, 0, 0, 0, 0,
];
/// Nanoseconds per second.
const NS_PER_SECOND: u64 = 1_000_000_000;

/// A join that waits for its roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RolePick {
    /// The game mode's CRC.
    crc: u32,
    /// Whether it queues with its group (the 6254's flag).
    group: bool,
    /// The 6272 calls so far.
    picks: u8,
    /// The newest role.
    role: u8,
}

/// The queue of the player.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Queued {
    crc: u32,
    /// When it joined: ns of server time since 2000-01-01.
    joined_ns: u64,
    /// The players of its found match; [`NOT_MATCHED`] before a pop.
    match_size: u32,
}

/// The match found for the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Proposal {
    match_id: Uuid,
    /// When it was found (sim server ms); `None` once the wait status went out.
    popped_ms: Option<u64>,
}

/// A player's queue state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct QueuePlayer {
    picking: Option<RolePick>,
    queued: Option<Queued>,
    proposal: Option<Proposal>,
    /// The opened match it goes to at its 5450, with the CRC of its game mode.
    place: Option<(MatchPlace, u32)>,
}

impl QueuePlayer {
    /// Whether it waits in a queue or for a match.
    pub(super) const fn is_queued(&self) -> bool {
        self.queued.is_some() || self.place.is_some()
    }
}

/// The header of a call to player `number`'s `GameModeParticipantComponent`.
pub(super) fn participant_header(number: u64) -> FacetHeader {
    FacetHeader::of(component::GAME_MODE_PARTICIPANT, number)
}

/// The queue list of `queued` at `version` (one entry, or none).
fn queue_list(version: u64, queued: Option<&Queued>) -> nw_messages::items::FullList<QueueEntry> {
    full(
        version,
        queued
            .map(|queued| QueueEntry {
                mode: queued.crc,
                joined_ns: queued.joined_ns,
                unknown: ENTRY_UNKNOWN,
                match_size: queued.match_size,
                flag: ENTRY_FLAG,
            })
            .into_iter()
            .collect(),
    )
}

/// The matchmaking service of no match: status 0, no match id, no players.
fn no_match() -> MatchmakingService {
    MatchmakingService {
        status: Some(STATUS_NONE),
        match_id: Some(String::new()),
        desired: Some(0),
        accepted: Some(0),
        ..MatchmakingService::default()
    }
}

impl SimWorld {
    /// 6211: the group eligibility subscription of player `entity`.
    pub(super) fn group_eligibility(&mut self, entity: Entity, subscribed: bool) {
        self.put_participant_state(
            entity,
            GameModeParticipantState {
                service: MatchmakingService {
                    replicating_group: Some(subscribed),
                    ..MatchmakingService::default()
                },
                eligibility: subscribed.then_some(GROUP_ELIGIBILITY),
                ..GameModeParticipantState::default()
            },
        );
    }

    /// The game mode of the queue activity `activity`, when it runs here: the instance
    /// catalog's arena or Outpost Rush (each is entered by its queue only).
    fn queued_mode(&self, activity: u16) -> Option<(u32, String)> {
        let kind = match activity {
            ARENA_ACTIVITY => InstanceKind::Arena,
            OUTPOST_RUSH_ACTIVITY => InstanceKind::OutpostRush,
            _ => return None,
        };
        if self.instance_sim {
            return None;
        }
        self.content
            .instances
            .modes()
            .find(|mode| mode.kind == kind)
            .map(|mode| (mode.crc, mode.id.clone()))
    }

    /// 6254: player `entity` (`number`) joins the queue of `activity`; `group` from the call's
    /// flag. The answer: 6295, or 1234 when the mode does not run here or the player is barred.
    pub(super) fn join_queue(
        &mut self,
        entity: Entity,
        number: u64,
        (activity, extra, group): (u16, u16, bool),
    ) -> Option<ServerMessage> {
        let header = participant_header(number);
        let now_ns = unix_ns(self.server_ms);
        let mode = self.queued_mode(activity);
        let mut player = self.world.get_mut::<Player>(entity)?;
        let barred = mode.as_ref().is_some_and(|(crc, _)| {
            player
                .arena
                .queue_eligible()
                .iter()
                .any(|(mode, from_ns)| mode == crc && now_ns < *from_ns)
        });
        let Some((crc, _)) = mode.filter(|_| !barred && !player.queue.is_queued()) else {
            return Some(ServerMessage::GameMode(
                GameModeServerMsg::QueueGroupRemoved { header },
            ));
        };
        player.queue.picking = Some(RolePick {
            crc,
            group,
            picks: 0,
            role: 0,
        });
        Some(ServerMessage::GameMode(
            GameModeServerMsg::StartMatchmakingRoleSelection {
                header,
                flag: ROLE_SELECTION_FLAG,
                activity,
                extra,
            },
        ))
    }

    /// 6272: player `entity` picked `role`; the last pick of a join queues it.
    pub(super) fn pick_role(&mut self, entity: Entity, role: u8) {
        let content = std::sync::Arc::clone(&self.content);
        let server_ns = self.server_ms.saturating_mul(nw_messages::NS_PER_MS);
        let unix_s = unix_ns(self.server_ms) / NS_PER_SECOND;
        let sim = self.me.id.clone();
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let Some(pick) = player.queue.picking.as_mut() else {
            return;
        };
        pick.picks = pick.picks.saturating_add(1);
        pick.role = role;
        if pick.picks < ROLE_PICKS {
            return;
        }
        let pick = *pick;
        player.queue.picking = None;
        let Some(mode) = content.instances.by_crc(pick.crc) else {
            return;
        };
        let queued = Queued {
            crc: pick.crc,
            joined_ns: server_ns,
            match_size: NOT_MATCHED,
        };
        player.list_version += 1;
        let activity = if mode.kind == InstanceKind::OutpostRush {
            OUTPOST_RUSH_QUEUED_ACTIVITY
        } else {
            ARENA_QUEUED_ACTIVITY
        };
        let state = GameModeParticipantState {
            queuing: Some(queue_list(player.list_version, Some(&queued))),
            service: MatchmakingService {
                activity: Some(activity),
                ..MatchmakingService::default()
            },
            ..GameModeParticipantState::default()
        };
        player.queue.queued = Some(queued);
        let call = QueueCall::Join(QueueJoin {
            character: player.character,
            sim,
            group: pick.group.then(|| player.social.group()).flatten(),
            role: pick.role,
            game_mode: mode.id.clone(),
            level: mode.rotation_map(unix_s).folder.clone(),
        });
        tracing::info!(player = player.number, game_mode = %mode.id, "player queued");
        self.queue_calls.push(call);
        self.put_participant_state(entity, state);
    }

    /// 6297: the player closed the role picking screen.
    pub(super) fn cancel_role_pick(&mut self, entity: Entity, number: u64) -> ServerMessage {
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.queue.picking = None;
        }
        ServerMessage::GameMode(GameModeServerMsg::RoleSelectionCanceled {
            header: participant_header(number),
        })
    }

    /// Player `entity` leaves its queue (1066, 2385): the director hears it, and its participant
    /// state holds no queue (Observed: `3v3_arena_group_2` 572.779 s, the empty list and
    /// activity `16`). Whether it was queued.
    pub(super) fn leave_queue(&mut self, entity: Entity) -> bool {
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return false;
        };
        player.queue.picking = None;
        if player.queue.queued.take().is_none() {
            return false;
        }
        player.queue.proposal = None;
        player.list_version += 1;
        let state = GameModeParticipantState {
            queuing: Some(queue_list(player.list_version, None)),
            service: MatchmakingService {
                activity: Some(IDLE_ACTIVITY),
                ..MatchmakingService::default()
            },
            ..GameModeParticipantState::default()
        };
        let character = player.character;
        self.queue_calls.push(QueueCall::Leave { character });
        self.put_participant_state(entity, state);
        true
    }

    /// A player who leaves the sim leaves its queue (Inferred).
    pub(super) fn queue_left_sim(&mut self, entity: Entity) {
        let Some(player) = self.world.get::<Player>(entity) else {
            return;
        };
        if player.queue.is_queued() {
            self.queue_calls.push(QueueCall::Leave {
                character: player.character,
            });
        }
    }

    /// 5741: the player accepted the match `match_id` (its text).
    pub(super) fn accept_match(&mut self, entity: Entity, match_id: &str) {
        let Some(player) = self.world.get::<Player>(entity) else {
            return;
        };
        let accepted = Uuid::parse_str(match_id).ok().filter(|id| {
            player
                .queue
                .proposal
                .is_some_and(|proposal| proposal.match_id == *id)
        });
        if let Some(match_id) = accepted {
            self.queue_calls.push(QueueCall::Accept {
                character: player.character,
                match_id,
            });
        }
    }

    /// 5450: the client signed in to the matchmaking service: a player with an opened match
    /// moves to it (status 5, then the move).
    pub(super) fn matchmaking_login_confirmed(&mut self, entity: Entity, now_ms: u64) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let Some((place, crc)) = player.queue.place.take() else {
            return;
        };
        let Some(mode) = content.instances.by_crc(crc) else {
            return;
        };
        let Some(spot) = mode
            .map_of(&place.level)
            .and_then(|map| team_spot(map, place.team, place.place))
        else {
            tracing::warn!(player = player.number, level = %place.level, "arena map has no spot for the team");
            return;
        };
        let crc = mode.crc;
        self.put_participant_state(
            entity,
            GameModeParticipantState {
                service: MatchmakingService {
                    status: Some(STATUS_MOVING),
                    ..MatchmakingService::default()
                },
                ..GameModeParticipantState::default()
            },
        );
        self.enter_match(
            entity,
            (place.instance, place.sim.clone(), place.phase),
            crc,
            ArenaSeat {
                team: place.team,
                place: place.place,
            },
            spot,
            now_ms,
        );
    }

    /// The director's news `news` of a queued player of this sim.
    pub fn queue_news(&mut self, news: QueueNews) {
        let Some(entity) = self.player_of(news.character) else {
            return;
        };
        let server_ms = self.server_ms;
        let server_ns = server_ms.saturating_mul(nw_messages::NS_PER_MS);
        // An Outpost Rush pop waits for the director's invite; an arena pop turns to the wait
        // after [`POPPED_STATUS_MS`].
        let invited_later = self
            .content
            .instances
            .by_id(&news.game_mode)
            .is_some_and(|mode| mode.kind == InstanceKind::OutpostRush);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let number = player.number;
        let header = participant_header(number);
        let mut messages = Vec::new();
        let state = match news.kind {
            QueueNewsKind::Popped { match_id, size } => {
                let Some(queued) = player.queue.queued.as_mut() else {
                    return;
                };
                queued.match_size = size;
                let queued = queued.clone();
                player.queue.proposal = Some(Proposal {
                    match_id,
                    popped_ms: (!invited_later).then_some(server_ms),
                });
                player.list_version += 1;
                GameModeParticipantState {
                    queuing: Some(queue_list(player.list_version, Some(&queued))),
                    service: MatchmakingService {
                        status: Some(STATUS_POPPED),
                        match_id: Some(match_id.hyphenated().to_string()),
                        desired: u8::try_from(size).ok(),
                        ..MatchmakingService::default()
                    },
                    ..GameModeParticipantState::default()
                }
            }
            QueueNewsKind::Invited => GameModeParticipantState {
                service: MatchmakingService {
                    status: Some(STATUS_WAITING),
                    ..MatchmakingService::default()
                },
                ..GameModeParticipantState::default()
            },
            QueueNewsKind::Accepted { count } => GameModeParticipantState {
                service: MatchmakingService {
                    accepted: Some(u8::try_from(count).unwrap_or(u8::MAX)),
                    ..MatchmakingService::default()
                },
                ..GameModeParticipantState::default()
            },
            QueueNewsKind::Cancelled => {
                let Some(queued) = player.queue.queued.as_mut() else {
                    return;
                };
                queued.joined_ns = server_ns;
                let queued = queued.clone();
                player.queue.proposal = None;
                player.list_version += 1;
                messages.push(ServerMessage::GameMode(GameModeServerMsg::MatchCancelled {
                    header,
                }));
                GameModeParticipantState {
                    queuing: Some(queue_list(player.list_version, Some(&queued))),
                    service: no_match(),
                    ..GameModeParticipantState::default()
                }
            }
            QueueNewsKind::Dropped => {
                let Some(queued) = player.queue.queued.take() else {
                    return;
                };
                player.queue.proposal = None;
                player.list_version += 1;
                messages.push(ServerMessage::LeftGameModeQueue(LeftGameModeQueueMsg::of(
                    header, queued.crc,
                )));
                GameModeParticipantState {
                    queuing: Some(queue_list(player.list_version, None)),
                    service: MatchmakingService {
                        activity: Some(IDLE_ACTIVITY),
                        ..no_match()
                    },
                    ..GameModeParticipantState::default()
                }
            }
            QueueNewsKind::Ready(place) => {
                let Some(crc) = self
                    .content
                    .instances
                    .by_id(&news.game_mode)
                    .map(|mode| mode.crc)
                else {
                    return;
                };
                let Some(mut player) = self.world.get_mut::<Player>(entity) else {
                    return;
                };
                player.queue.queued = None;
                player.queue.proposal = None;
                player.queue.place = Some((place, crc));
                messages.push(ServerMessage::GameMode(
                    GameModeServerMsg::MatchmakingLoginRequested { header },
                ));
                tracing::info!(player = number, game_mode = %news.game_mode, "match ready");
                let connection = player.connection.clone();
                send_now(&mut self.combat_messages, &connection, &messages);
                return;
            }
        };
        let connection = player.connection.clone();
        send_now(&mut self.combat_messages, &connection, &messages);
        self.put_participant_state(entity, state);
    }

    /// Each tick: a pop's status turns to the wait [`POPPED_STATUS_MS`] after it.
    pub(super) fn tick_queue(&mut self) {
        let now_ms = self.server_ms;
        let mut waiting = Vec::new();
        let mut query = self.world.query::<(Entity, &mut Player)>();
        for (entity, mut player) in query.iter_mut(&mut self.world) {
            if let Some(proposal) = player.queue.proposal.as_mut()
                && proposal
                    .popped_ms
                    .is_some_and(|popped| now_ms >= popped.saturating_add(POPPED_STATUS_MS))
            {
                proposal.popped_ms = None;
                waiting.push(entity);
            }
        }
        for entity in waiting {
            self.put_participant_state(
                entity,
                GameModeParticipantState {
                    service: MatchmakingService {
                        status: Some(STATUS_WAITING),
                        ..MatchmakingService::default()
                    },
                    ..GameModeParticipantState::default()
                },
            );
        }
    }

    /// Player `entity` (`number`) arrived on this open-world sim (a join or a handoff): its
    /// participant state holds no queue, its queue bars and the idle matchmaking service
    /// (Observed: the first 3312 of every retail open-world join); a player back from an arena
    /// match also gets 4970 of no mode (Observed: 3 of 3 returns).
    pub(super) fn queue_arrived(&mut self, entity: Entity, number: u64, from_match: bool) {
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        player.list_version += 1;
        let version = player.list_version;
        let eligible = player.arena.queue_eligible().to_vec();
        let connection = player.connection.clone();
        if from_match {
            let left = ServerMessage::LeftGameModeQueue(LeftGameModeQueueMsg::of(
                participant_header(number),
                NO_GAME_MODE,
            ));
            send_now(&mut self.combat_messages, &connection, &[left]);
        }
        self.put_participant_state(
            entity,
            GameModeParticipantState {
                queuing: Some(queue_list(version, None)),
                eligible: (!eligible.is_empty()).then(|| full(version, eligible)),
                service: MatchmakingService {
                    activity: Some(IDLE_ACTIVITY),
                    ..no_match()
                },
                ..GameModeParticipantState::default()
            },
        );
    }

    /// The queue calls for the director since the last call.
    pub fn take_queue_calls(&mut self) -> Vec<QueueCall> {
        std::mem::take(&mut self.queue_calls)
    }
}

/// The spot of player `place` of team `team` on `map`: the team's teleport spot and the
/// place's formation offset (Observed: the players appeared about 1.5 m from the team's
/// `TeamTeleportData` entry).
pub(super) fn team_spot(
    map: &nw_content::instances::InstanceMap,
    team: u8,
    place: u8,
) -> Option<Position> {
    let teleport = map.teleports.get(usize::from(team))?;
    let [east, north] = if map.formation.is_empty() {
        [0.0, 0.0]
    } else {
        map.formation[usize::from(place) % map.formation.len()]
    };
    let [x, y, z] = teleport.position;
    Some(Position {
        x: x + east,
        y: y + north,
        z,
    })
}

#[cfg(test)]
mod tests {
    use nw_content::instances::{InstanceMap, Teleport};

    use super::*;

    /// The retail `NW_Arena01` map (`Arena3v3` map row).
    fn arena01() -> InstanceMap {
        InstanceMap {
            folder: "nw_arena01".to_owned(),
            map_name: "NW_Arena01".to_owned(),
            coatlicue_path: "coatlicue/NW_Arena01".to_owned(),
            frame: [864.0, 832.0, 992.0, 960.0],
            teleports: vec![
                Teleport {
                    position: [939.0, 935.0, 226.5],
                    yaw_deg: 180.0,
                },
                Teleport {
                    position: [939.0, 873.0, 227.0],
                    yaw_deg: 0.0,
                },
            ],
            formation: vec![[1.5, 0.0], [-1.5, 0.0]],
            map_crc: 0x1fad_9d42,
        }
    }

    #[test]
    fn a_team_appears_at_its_spot_with_the_formation_offset() {
        let map = arena01();
        assert_eq!(
            team_spot(&map, 1, 0),
            Some(Position {
                x: 940.5,
                y: 873.0,
                z: 227.0
            })
        );
        assert_eq!(team_spot(&map, 0, 1).map(|spot| spot.x), Some(937.5));
        // The third player of a team takes the first offset again.
        assert_eq!(team_spot(&map, 0, 2), team_spot(&map, 0, 0));
        assert_eq!(team_spot(&map, 2, 0), None, "two teams");
    }

    #[test]
    fn a_queue_list_holds_the_retail_entry_or_nothing() {
        let queued = Queued {
            crc: 0x1fad_9d42,
            joined_ns: 7,
            match_size: NOT_MATCHED,
        };
        let list = queue_list(3, Some(&queued));
        assert_eq!(
            list.elements,
            [QueueEntry {
                mode: 0x1fad_9d42,
                joined_ns: 7,
                unknown: ENTRY_UNKNOWN,
                match_size: NOT_MATCHED,
                flag: ENTRY_FLAG,
            }]
        );
        assert!(queue_list(4, None).elements.is_empty());
    }

    /// The test player.
    const PLAYER: u64 = 41;

    /// The open-world sim with the arena in its catalog and one player.
    fn open_world() -> SimWorld {
        let mut world = crate::world::tests::world_with_spawns(Vec::new());
        world.content = std::sync::Arc::new(crate::player_content::PlayerContent {
            instances: nw_content::instances::InstanceCatalog::build([
                super::super::arena_match::tests::arena(),
            ]),
            ..crate::player_content::PlayerContent::default()
        });
        crate::world::tests::join(&mut world, PLAYER, 100.0);
        world
    }

    fn call(world: &mut SimWorld, call: nw_messages::GameModeClientMsg) -> Vec<ServerMessage> {
        world.game_mode_call(&crate::world::tests::connection(PLAYER), call);
        crate::world::tests::messages_to(&world.take_messages(), PLAYER)
    }

    fn news(world: &mut SimWorld, kind: QueueNewsKind) -> Vec<ServerMessage> {
        let entity = world.player_by_number(PLAYER).expect("player");
        let character = world.world.get::<Player>(entity).expect("player").character;
        world.queue_news(QueueNews {
            character,
            game_mode: "Arena3v3".to_owned(),
            kind,
        });
        crate::world::tests::messages_to(&world.take_messages(), PLAYER)
    }

    /// Joins the queue as retail `20261010-125035-658` 286.462 s: 6254, then two 6272.
    fn queued(world: &mut SimWorld) {
        let header = participant_header(PLAYER);
        let answers = call(
            world,
            nw_messages::GameModeClientMsg::JoinQueueForActivities {
                header,
                activities: vec![nw_messages::QueueActivity {
                    activity: ARENA_ACTIVITY,
                    extra: 1,
                    flag: 0,
                }],
            },
        );
        assert_eq!(
            answers,
            [ServerMessage::GameMode(
                GameModeServerMsg::StartMatchmakingRoleSelection {
                    header,
                    flag: ROLE_SELECTION_FLAG,
                    activity: ARENA_ACTIVITY,
                    extra: 1,
                }
            )]
        );
        for role in [1, 4] {
            call(
                world,
                nw_messages::GameModeClientMsg::SetMatchmakingRole { header, role },
            );
        }
    }

    #[test]
    fn a_join_queues_after_the_second_role_and_the_accepted_match_moves_the_player() {
        let mut world = open_world();
        queued(&mut world);
        let calls = world.take_queue_calls();
        let [QueueCall::Join(join)] = calls.as_slice() else {
            panic!("one join: {calls:?}");
        };
        assert_eq!(
            (join.role, join.level.as_str(), join.group),
            (4, "nw_arena01", None)
        );
        let match_id = Uuid::from_u128(5);
        news(&mut world, QueueNewsKind::Popped { match_id, size: 2 });
        call(
            &mut world,
            nw_messages::GameModeClientMsg::AcceptMatch {
                header: participant_header(PLAYER),
                match_id: match_id.hyphenated().to_string(),
                flag: 1,
            },
        );
        let character = join.character;
        assert_eq!(
            world.take_queue_calls(),
            [QueueCall::Accept {
                character,
                match_id
            }]
        );
        let place = MatchPlace {
            match_id,
            instance: Uuid::from_u128(7),
            sim: crate::world::tests::slot("inst-1"),
            phase: 9,
            level: "nw_arena01".to_owned(),
            team: 1,
            place: 0,
        };
        assert_eq!(
            news(&mut world, QueueNewsKind::Ready(place)),
            [ServerMessage::GameMode(
                GameModeServerMsg::MatchmakingLoginRequested {
                    header: participant_header(PLAYER)
                }
            )]
        );
        call(
            &mut world,
            nw_messages::GameModeClientMsg::MatchmakingLoginConfirmed {
                header: participant_header(PLAYER),
            },
        );
        let now = crate::world::tests::NOW;
        crate::world::tests::tick(&mut world, now + 1);
        let out = crate::world::tests::tick(
            &mut world,
            now + 2 + super::super::PHASING_BEFORE_HANDOFF_MS,
        );
        let [start] = out.handoffs.as_slice() else {
            panic!("one handoff");
        };
        let stay = start.offer.progression.arena.stay.clone().expect("a stay");
        assert_eq!(stay.seat, Some(ArenaSeat { team: 1, place: 0 }));
        assert_eq!(
            (start.offer.position.x, start.offer.position.y),
            (940.5, 873.0)
        );
    }

    #[test]
    fn a_cancelled_match_keeps_the_queue_and_a_dropped_or_left_one_ends_it() {
        let mut world = open_world();
        queued(&mut world);
        world.take_queue_calls();
        let header = participant_header(PLAYER);
        news(
            &mut world,
            QueueNewsKind::Popped {
                match_id: Uuid::from_u128(5),
                size: 6,
            },
        );
        assert_eq!(
            news(&mut world, QueueNewsKind::Cancelled),
            [ServerMessage::GameMode(GameModeServerMsg::MatchCancelled {
                header
            })]
        );
        assert_eq!(
            news(&mut world, QueueNewsKind::Dropped),
            [ServerMessage::LeftGameModeQueue(LeftGameModeQueueMsg::of(
                header,
                nw_formats::objectstream::lowercase_crc32("Arena3v3"),
            ))]
        );
        queued(&mut world);
        world.take_queue_calls();
        let entity = world.player_by_number(PLAYER).expect("player");
        assert!(world.leave_queue(entity));
        let character = world.world.get::<Player>(entity).expect("player").character;
        assert_eq!(world.take_queue_calls(), [QueueCall::Leave { character }]);
    }

    #[test]
    fn an_outpost_rush_join_queues_its_mode_and_its_pop_waits_for_the_invite() {
        let mut world = crate::world::tests::world_with_spawns(Vec::new());
        world.content = std::sync::Arc::new(crate::player_content::PlayerContent {
            instances: nw_content::instances::InstanceCatalog::build([
                super::super::outpost_rush::tests::outpost_rush(),
            ]),
            ..crate::player_content::PlayerContent::default()
        });
        crate::world::tests::join(&mut world, PLAYER, 100.0);
        let header = participant_header(PLAYER);
        // Retail `OPR_TEST3` 12.510 s: 6254 `01 3116 0001 00`, answered with 6295.
        let answers = call(
            &mut world,
            nw_messages::GameModeClientMsg::JoinQueueForActivities {
                header,
                activities: vec![nw_messages::QueueActivity {
                    activity: OUTPOST_RUSH_ACTIVITY,
                    extra: 1,
                    flag: 0,
                }],
            },
        );
        assert!(matches!(
            answers.as_slice(),
            [ServerMessage::GameMode(
                GameModeServerMsg::StartMatchmakingRoleSelection {
                    activity: OUTPOST_RUSH_ACTIVITY,
                    ..
                }
            )]
        ));
        for role in [1, 3] {
            call(
                &mut world,
                nw_messages::GameModeClientMsg::SetMatchmakingRole { header, role },
            );
        }
        let calls = world.take_queue_calls();
        let [QueueCall::Join(join)] = calls.as_slice() else {
            panic!("one join: {calls:?}");
        };
        assert_eq!(
            (join.game_mode.as_str(), join.level.as_str()),
            (
                nw_content::instances::OUTPOST_RUSH_GAME_MODE,
                "nw_opr_004_trench"
            )
        );
        let entity = world.player_by_number(PLAYER).expect("player");
        let character = join.character;
        world.queue_news(QueueNews {
            character,
            game_mode: nw_content::instances::OUTPOST_RUSH_GAME_MODE.to_owned(),
            kind: QueueNewsKind::Popped {
                match_id: Uuid::from_u128(5),
                size: 40,
            },
        });
        let proposal = world
            .world
            .get::<Player>(entity)
            .expect("player")
            .queue
            .proposal;
        assert_eq!(
            proposal.map(|proposal| proposal.popped_ms),
            Some(None),
            "the invite comes from the director, not a timer"
        );
    }

    #[test]
    fn a_barred_player_and_another_mode_are_refused() {
        let mut world = open_world();
        let header = participant_header(PLAYER);
        let entity = world.player_by_number(PLAYER).expect("player");
        let crc = nw_formats::objectstream::lowercase_crc32("Arena3v3");
        if let Some(mut player) = world.world.get_mut::<Player>(entity) {
            player.arena.bar_queue(crc, u64::MAX);
        }
        let refused = [ServerMessage::GameMode(
            GameModeServerMsg::QueueGroupRemoved { header },
        )];
        for activity in [ARENA_ACTIVITY, OUTPOST_RUSH_ACTIVITY] {
            assert_eq!(
                call(
                    &mut world,
                    nw_messages::GameModeClientMsg::JoinQueueForActivities {
                        header,
                        activities: vec![nw_messages::QueueActivity {
                            activity,
                            extra: 1,
                            flag: 0,
                        }],
                    },
                ),
                refused
            );
        }
        assert_eq!(
            call(
                &mut world,
                nw_messages::GameModeClientMsg::CancelRoleSelection { header }
            ),
            [ServerMessage::GameMode(
                GameModeServerMsg::RoleSelectionCanceled { header }
            )]
        );
    }
}
