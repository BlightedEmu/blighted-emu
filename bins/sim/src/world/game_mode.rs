// SPDX-License-Identifier: AGPL-3.0-or-later
//! Matchmaking queue calls (6211, 6272, 1980, 5741, 5450, 6254, 6824) and war board calls
//! (3206, 5953, 6845) of a player, with the server answers the retail captures show (1234,
//! 4970).
//!
//! The queued activities are the PvP game modes and the catacombs (6254 activity values in the
//! 17 retail queue ledgers: 0x3116 Outpost Rush, 0x3316 3v3 arena, 0x3016 capture the flag (its
//! queue entry names `930fce8f` `CaptureTheFlag`: `20261010-125035-658` 169.450 s, `CTF01`,
//! `CTF_QueueAndJoin_PickUpSpeedHealthAndFlagThenDied` 29.573 s), 0x0c16 catacombs (entry
//! `2736ba35`: `CATACOMBS2` 7.006 s, `CATACOMBS6` and three Hive catacombs ledgers), and 0x4316
//! once in an Amrine group run). Retail matched the queued players and moved them into
//! an instance of the game mode: in `opr_que_and_lost` the accept (5741 at 1781894841233) is
//! followed by the loading screen (3951) and the Outpost Rush level (1635
//! `NW_OPR_005_Vertical` at 1781894842131).
//!
//! The 3v3 arena and Outpost Rush run here: their queue ([`super::queue`]: 6211, 6254, 6272,
//! 6297, 5741, 5450, 1066, 2385) and their match on an instance sim ([`super::arena_match`],
//! [`super::outpost_rush`]: 6824, 1980, 3309). The 6211 group eligibility subscription is
//! answered with the player's 3312 slot 1 and 2 (arena flow study: every call of the arena
//! ledgers). The capture the flag queue runs as Outpost Rush's ([`super::queue`]); its match on
//! the instance sim is not built yet. The other modes do not run: their rows are not instances
//! of the game data catalog (`nw_content::instances`). So their queue join is refused at once:
//! the queue group goes away (1234), and no role selection follows.
//! Inferred: retail's own answer to a mode that cannot run is `OnQueueForGameModeFailedMsg`
//! (3310), which no capture holds (its layout is Unknown); 1234 is the captured message that
//! ends a queue.
//!
//! Calls with no server answer in any capture: a client event other than the arena's `Leave`
//! (1980), the war board receipt (5953), the remote board use (2791, after which the client
//! itself asks for the counts), and the spectator stop (6845, a client camera call; this
//! server has no spectator state). Their arms only take the call.
//!
//! A queue leave (1066, and 2385 for every queue) is answered with a 4970 of the left game mode
//! (retail 3v3 and expedition group runs: the 1066's own CRC; for a 2385, the CRC of the
//! expedition the group waits for, else 0). A group queue (1066 group byte 1, and every 2385)
//! gets the 1234 first. Retail then sends a second 4970 of a group mate's leave, which this sim
//! does not hold (Inferred from the two group ledgers). A game mode leave (3309) takes the player
//! out of its instance as a 926 does (retail: 3951, the open world level, then 4970).
//!
//! The territory war sign-up and roster calls (2767, 2772, 2791, 2771, 2762, 2784 and the roster
//! calls) are [`super::wars`]'s.

use nw_bus::ConnectionRef;
use nw_messages::facet::NO_GAME_MODE;
use nw_messages::{
    GameModeClientMsg, GameModeServerMsg, LeftGameModeQueueMsg, ServerMessage, WarClientMsg,
    WarServerMsg, WarboardManifest,
};

use super::arena_match::LEAVE_EVENT;
use super::{Player, SimWorld, send_now};

/// The 6254 flag of a group's queue join (Observed: 1 in the 6 group joins, 0 in the 10 solo
/// ones).
const GROUP_QUEUE_FLAG: u8 = 1;
/// Metric: queue joins refused because the game mode does not run here.
const METRIC_REFUSED_QUEUES: &str = "sim_game_mode_queue_refused_total";

impl SimWorld {
    /// Carries out the matchmaking call `call` of the client of `connection`.
    pub fn game_mode_call(&mut self, connection: &ConnectionRef, call: GameModeClientMsg) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        let header = call.header();
        let mut answers: Vec<ServerMessage> = Vec::new();
        let Some((number, queued_game_mode)) = self
            .world
            .get::<Player>(entity)
            .map(|player| (player.number, player.arena.queued_game_mode()))
        else {
            return;
        };
        let now_ms = self.last_now_ms;
        match call {
            GameModeClientMsg::ModifyGroupEligibilitySubscription { subscribed, .. } => {
                self.group_eligibility(entity, subscribed);
            }
            GameModeClientMsg::ReceivedClientEvent { event, .. } => {
                if event == LEAVE_EVENT {
                    self.leave_arena(entity);
                }
            }
            GameModeClientMsg::JoinQueueForActivities { activities, .. } => {
                let answer = activities.first().and_then(|activity| {
                    self.join_queue(
                        entity,
                        number,
                        (
                            activity.activity,
                            activity.extra,
                            activity.flag == GROUP_QUEUE_FLAG,
                        ),
                    )
                });
                let refused = matches!(
                    answer,
                    None | Some(ServerMessage::GameMode(
                        GameModeServerMsg::QueueGroupRemoved { .. }
                    ))
                );
                if refused && !activities.is_empty() {
                    metrics::counter!(METRIC_REFUSED_QUEUES).increment(1);
                    tracing::info!(
                        player = number,
                        ?activities,
                        "queue join refused: the game mode does not run here or the player may not queue"
                    );
                }
                answers.push(answer.unwrap_or(ServerMessage::GameMode(
                    GameModeServerMsg::QueueGroupRemoved { header },
                )));
            }
            GameModeClientMsg::SetMatchmakingRole { role, .. } => self.pick_role(entity, role),
            GameModeClientMsg::CancelRoleSelection { .. } => {
                answers.push(self.cancel_role_pick(entity, number));
            }
            GameModeClientMsg::AcceptMatch { match_id, .. } => self.accept_match(entity, &match_id),
            GameModeClientMsg::MatchmakingLoginConfirmed { .. } => {
                self.matchmaking_login_confirmed(entity, now_ms);
            }
            GameModeClientMsg::SetReadiedUpStatus { ready, .. } => self.arena_ready(number, ready),
            GameModeClientMsg::LeaveQueueForGameMode {
                game_mode, group, ..
            } => {
                self.leave_queue(entity);
                if group {
                    answers.push(ServerMessage::GameMode(
                        GameModeServerMsg::QueueGroupRemoved { header },
                    ));
                }
                answers.push(ServerMessage::LeftGameModeQueue(LeftGameModeQueueMsg::of(
                    header, game_mode,
                )));
            }
            GameModeClientMsg::LeaveQueueForAllGameModes { .. } => {
                self.leave_queue(entity);
                let game_mode = queued_game_mode.unwrap_or(NO_GAME_MODE);
                answers.push(ServerMessage::GameMode(
                    GameModeServerMsg::QueueGroupRemoved { header },
                ));
                answers.push(ServerMessage::LeftGameModeQueue(LeftGameModeQueueMsg::of(
                    header, game_mode,
                )));
            }
            GameModeClientMsg::LeaveGameMode { .. } => self.leave_arena(entity),
        }
        send_now(&mut self.combat_messages, connection, &answers);
    }

    /// Carries out the war board call `call` of the client of `connection`.
    pub fn war_call(&mut self, connection: &ConnectionRef, call: WarClientMsg) {
        if !self.by_connection.contains_key(connection) {
            return;
        }
        let header = call.header();
        let mut answers: Vec<ServerMessage> = Vec::new();
        match call {
            WarClientMsg::WarboardEnabled { .. } => {
                match WarServerMsg::full_manifest(
                    header,
                    &WarboardManifest::of_participants(Vec::new()),
                ) {
                    Ok(manifest) => answers.push(ServerMessage::War(manifest)),
                    Err(error) => {
                        tracing::warn!(%error, "war board manifest not sent");
                    }
                }
            }
            // No answer in any capture; see the module text.
            WarClientMsg::WarboardManifestReceived { .. } | WarClientMsg::StopSpectating { .. } => {
            }
        }
        send_now(&mut self.combat_messages, connection, &answers);
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::{FacetHeader, QueueActivity};
    use uuid::Uuid;

    use super::*;
    use crate::world::queue::participant_header;
    use crate::world::tests::{
        NOW, SERVER_TIME, admit_at, connection, messages_to, world_with_spawns,
    };

    /// The player number of the test player.
    const PLAYER: u64 = 41;
    /// The facet header of the test calls.
    const HEADER: FacetHeader = FacetHeader {
        target: 0,
        entity: 0,
    };
    /// The Outpost Rush activity of the retail queue calls.
    const ACTIVITY: u16 = 0x3116;
    /// The capture the flag activity of the retail queue calls (Observed: `CTF01` and
    /// `20261010-125035-658` 165.466 s): the test sim's catalog holds no such game mode.
    const CAPTURE_THE_FLAG_ACTIVITY: u16 = 0x3016;

    fn world() -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        let admit = admit_at(PLAYER, 0.0, true);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        world
    }

    /// Retail 3v3 arena group and expedition runs: a 1066 is answered with a 4970 of its own
    /// game mode (a group queue gets the 1234 first), a 2385 with the 1234 and a 4970 of no
    /// queued expedition, and both clear the queue.
    #[test]
    fn a_queue_leave_answers_with_the_left_game_mode_and_clears_the_queue() {
        const ARENA_CRC: u32 = 0x1fad_9d42;
        let mut world = world();
        let join = GameModeClientMsg::JoinQueueForActivities {
            header: HEADER,
            activities: vec![QueueActivity {
                activity: ACTIVITY,
                extra: 0,
                flag: 1,
            }],
        };
        world.game_mode_call(&connection(PLAYER), join.clone());
        let _ = world.take_messages();
        world.game_mode_call(
            &connection(PLAYER),
            GameModeClientMsg::LeaveQueueForGameMode {
                header: HEADER,
                game_mode: ARENA_CRC,
                group: false,
            },
        );
        let sent = world.take_messages();
        assert_eq!(
            messages_to(&sent, PLAYER),
            [ServerMessage::LeftGameModeQueue(LeftGameModeQueueMsg::of(
                HEADER, ARENA_CRC
            ))]
        );
        world.game_mode_call(
            &connection(PLAYER),
            GameModeClientMsg::MatchmakingLoginConfirmed { header: HEADER },
        );
        assert!(
            messages_to(&world.take_messages(), PLAYER).is_empty(),
            "no queue left to open roles for"
        );
        world.game_mode_call(&connection(PLAYER), join);
        let _ = world.take_messages();
        world.game_mode_call(
            &connection(PLAYER),
            GameModeClientMsg::LeaveQueueForGameMode {
                header: HEADER,
                game_mode: ARENA_CRC,
                group: true,
            },
        );
        let sent = world.take_messages();
        assert_eq!(
            messages_to(&sent, PLAYER),
            [
                ServerMessage::GameMode(GameModeServerMsg::QueueGroupRemoved { header: HEADER }),
                ServerMessage::LeftGameModeQueue(LeftGameModeQueueMsg::of(HEADER, ARENA_CRC)),
            ]
        );
        world.game_mode_call(
            &connection(PLAYER),
            GameModeClientMsg::LeaveQueueForAllGameModes {
                header: HEADER,
                flags: 1,
            },
        );
        let sent = world.take_messages();
        assert_eq!(
            messages_to(&sent, PLAYER),
            [
                ServerMessage::GameMode(GameModeServerMsg::QueueGroupRemoved { header: HEADER }),
                ServerMessage::LeftGameModeQueue(LeftGameModeQueueMsg::of(HEADER, NO_GAME_MODE)),
            ]
        );
    }

    /// A queue join of a game mode the sim's catalog does not hold (the test sim holds no
    /// capture the flag) is refused with the 1234 to the player's participant component at once,
    /// and the later queue calls name no match.
    #[test]
    fn a_queue_join_is_refused_and_no_match_follows() {
        let mut world = world();
        world.game_mode_call(
            &connection(PLAYER),
            GameModeClientMsg::JoinQueueForActivities {
                header: HEADER,
                activities: vec![QueueActivity {
                    activity: CAPTURE_THE_FLAG_ACTIVITY,
                    extra: 0,
                    flag: 1,
                }],
            },
        );
        assert_eq!(
            messages_to(&world.take_messages(), PLAYER),
            [ServerMessage::GameMode(
                GameModeServerMsg::QueueGroupRemoved {
                    header: participant_header(PLAYER)
                }
            )]
        );
        for call in [
            GameModeClientMsg::MatchmakingLoginConfirmed { header: HEADER },
            GameModeClientMsg::SetMatchmakingRole {
                header: HEADER,
                role: 1,
            },
            GameModeClientMsg::SetReadiedUpStatus {
                header: HEADER,
                game_mode: 1,
                ready: true,
            },
            GameModeClientMsg::AcceptMatch {
                header: HEADER,
                match_id: Uuid::nil().hyphenated().to_string(),
                flag: 1,
            },
        ] {
            world.game_mode_call(&connection(PLAYER), call);
            assert!(
                messages_to(&world.take_messages(), PLAYER).is_empty(),
                "no role selection, no match"
            );
        }
    }
}
