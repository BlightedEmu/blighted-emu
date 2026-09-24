// SPDX-License-Identifier: AGPL-3.0-or-later
//! Matchmaking queue calls (6211, 6272, 1980, 5741, 5450, 6254, 6824), war board calls
//! (3206, 5953, 6845) and territory war sign-up calls (2767, 2772, 2791, 2771, 2762, 2784)
//! of a player, with the server answers the retail captures show (5449, 6295, 1234, 3530,
//! 2726, 2729, 2169).
//!
//! Retail flow of a queue join (`opr_que_and_lost`): the client joins a queue (6254), the
//! server asks for a matchmaking login (5449), the client confirms (5450) and the server
//! opens role selection for the first queued activity (6295). A role pick (6272) makes the
//! match here: this server has no matchmaking service, so the match is local to the sim.
//! A ready (6824) and an accept (5741) of that match end it: the queue group goes away
//! (1234), as at a match start. An empty 6254 leaves the queue the same way.
//!
//! Three calls have no server answer in any capture and change no state here: the group
//! eligibility subscription (6211, a subscription to replicated group data, which the
//! social service sends), a client event (1980, meaning Unknown), the war board receipt
//! (5953), the remote board use (2791, after which the client itself asks for the counts),
//! and the spectator stop (6845, a client camera call; this server has no spectator
//! state). Their arms only take the call.
//!
//! War sign-ups are kept per war on the sim. A sign-up (2762) is answered with a 2169 of
//! result [`SIGNUP_RESULT_OK`] and value [`SIGNUP_SUCCESS_VALUE`] (the one retail success;
//! the retail failures of a closed war are result 1 and value 0, and this server holds no
//! war schedule, so every sign-up succeeds). The list (2767, and the roster 2772) is
//! answered with a 2726 and the count (2771) with a 2729, as in the captures.

use std::collections::HashMap;

use nw_bus::ConnectionRef;
use nw_messages::social::CharacterRef;
use nw_messages::{
    GameModeClientMsg, GameModeServerMsg, QueueActivity, SIGNUP_LIST_TRAILER, ServerMessage,
    SignupEntry,
    WarClientMsg, WarServerMsg, WarSignupClientMsg, WarSignupServerMsg, WarboardManifest,
};
use uuid::Uuid;

use super::{Player, SimWorld, send_now};

/// The 2169 result of a sign-up that went through (0 in the retail success).
const SIGNUP_RESULT_OK: u8 = 0;
/// The 2169 value of a sign-up that went through (12 after the retail sign-up for
/// territory 11; meaning Unknown).
const SIGNUP_SUCCESS_VALUE: u32 = 12;
/// The second `u32` of every retail 2726 (1; meaning Unknown).
const SIGNUP_LIST_EXTRA: u32 = 1;
/// The byte after the name of every retail 2726 entry (2; meaning Unknown).
const SIGNUP_ENTRY_FLAG: u8 = 2;
/// The first byte of every retail 6295 (1; meaning Unknown).
const ROLE_SELECTION_FLAG: u8 = 1;
/// The last `u16` of every retail 6295 (0; meaning Unknown).
const ROLE_SELECTION_EXTRA: u16 = 0;

/// A player's matchmaking and war sign-up state.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct GameModePlayer {
    /// The activities of its newest queue join (6254); empty in no queue.
    queued: Vec<QueueActivity>,
    /// The role it picked (6272); a ready counts only with a role, as in retail order.
    role: Option<u8>,
    /// The match this server made at its role pick: the 6824/6414 value and the 5741 text.
    match_id: Option<(u32, Uuid)>,
    /// Whether it readied up for the match (6824).
    ready: bool,
    /// Whether it accepted the match (5741).
    accepted: bool,
    /// The war of its newest sign-up or list call, for the roster (2772).
    war_last: Option<u32>,
}

impl GameModePlayer {
    /// Ends the match when it is readied and accepted: the queue and the match clear.
    /// `true` when the match ended (the queue group goes away, 1234).
    fn finish_match(&mut self) -> bool {
        if !self.ready || !self.accepted || self.match_id.is_none() {
            return false;
        }
        self.queued.clear();
        self.match_id = None;
        self.role = None;
        self.ready = false;
        self.accepted = false;
        true
    }
}

/// One signed-up player of a war.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Signup {
    /// The character.
    character: Uuid,
    /// Its name.
    name: String,
    /// The territory it signed up at.
    territory: u16,
}

/// The war sign-ups of this sim, by war.
#[derive(Debug, Default)]
pub(super) struct WarSignups {
    /// The sign-ups of each war, in sign-up order.
    by_war: HashMap<u32, Vec<Signup>>,
    /// The value of the newest match made (6824/6414 values are `u32`).
    next_match: u32,
}

impl WarSignups {
    /// The entries of war `war` as a 2726 lists them.
    fn entries(&self, war: u32) -> Vec<SignupEntry> {
        self.by_war
            .get(&war)
            .map(|signups| {
                signups
                    .iter()
                    .map(|signup| SignupEntry {
                        character: signup.character.hyphenated().to_string(),
                        name: signup.name.clone(),
                        flag: SIGNUP_ENTRY_FLAG,
                        id: *signup.character.as_bytes(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The lowest war `character` signed up for, if any.
    fn war_of(&self, character: Uuid) -> Option<u32> {
        let mut wars: Vec<u32> = self.by_war.keys().copied().collect();
        wars.sort_unstable();
        wars.into_iter().find(|war| {
            self.by_war
                .get(war)
                .is_some_and(|signups| signups.iter().any(|signup| signup.character == character))
        })
    }

    /// How many signed up for war `war`.
    fn count(&self, war: u32) -> u32 {
        u32::try_from(self.by_war.get(&war).map_or(0, Vec::len)).unwrap_or(u32::MAX)
    }
}

impl SimWorld {
    /// Carries out the matchmaking call `call` of the client of `connection`.
    pub fn game_mode_call(&mut self, connection: &ConnectionRef, call: GameModeClientMsg) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        let header = call.header();
        let mut answers: Vec<ServerMessage> = Vec::new();
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let mode = &mut player.game_mode;
        match call {
            // No answer in any capture; see the module text.
            GameModeClientMsg::ModifyGroupEligibilitySubscription { .. }
            | GameModeClientMsg::ReceivedClientEvent { .. } => {}
            GameModeClientMsg::JoinQueueForActivities { activities, .. } => {
                mode.queued = activities;
                mode.ready = false;
                mode.accepted = false;
                if mode.queued.is_empty() {
                    mode.match_id = None;
                    answers.push(ServerMessage::GameMode(
                        GameModeServerMsg::QueueGroupRemoved { header },
                    ));
                } else {
                    answers.push(ServerMessage::GameMode(
                        GameModeServerMsg::MatchmakingLoginRequested { header },
                    ));
                }
            }
            GameModeClientMsg::MatchmakingLoginConfirmed { .. } => {
                if let Some(activity) = mode.queued.first() {
                    answers.push(ServerMessage::GameMode(
                        GameModeServerMsg::StartMatchmakingRoleSelection {
                            header,
                            flag: ROLE_SELECTION_FLAG,
                            activity: activity.activity,
                            extra: ROLE_SELECTION_EXTRA,
                        },
                    ));
                }
            }
            GameModeClientMsg::SetMatchmakingRole { role, .. } => {
                mode.role = Some(role);
                if mode.match_id.is_none() && !mode.queued.is_empty() {
                    self.war_signups.next_match = self.war_signups.next_match.wrapping_add(1);
                    mode.match_id = Some((self.war_signups.next_match, Uuid::new_v4()));
                }
            }
            GameModeClientMsg::SetReadiedUpStatus { match_id, ready, .. } => {
                if mode.match_id.is_none() {
                    mode.match_id = Some((match_id, Uuid::new_v4()));
                }
                if mode.role.is_some()
                    && mode.match_id.is_some_and(|(known, _)| known == match_id)
                {
                    mode.ready = ready;
                }
                if mode.finish_match() {
                    answers.push(ServerMessage::GameMode(
                        GameModeServerMsg::QueueGroupRemoved { header },
                    ));
                }
            }
            GameModeClientMsg::AcceptMatch { match_id, flag, .. } => {
                let known = mode
                    .match_id
                    .is_some_and(|(_, text)| text.hyphenated().to_string() == match_id);
                mode.accepted = known && flag != 0;
                if mode.finish_match() {
                    answers.push(ServerMessage::GameMode(
                        GameModeServerMsg::QueueGroupRemoved { header },
                    ));
                }
            }
        }
        drop(player);
        send_now(&mut self.combat_messages, connection, &answers);
    }

    /// Carries out the war board call `call` of the client of `connection`.
    pub fn war_call(&mut self, connection: &ConnectionRef, call: WarClientMsg) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        let header = call.header();
        let mut answers: Vec<ServerMessage> = Vec::new();
        match call {
            WarClientMsg::WarboardEnabled { .. } => {
                let participants: Vec<CharacterRef> = self
                    .world
                    .get::<Player>(entity)
                    .and_then(|player| player.game_mode.war_last)
                    .map(|war| {
                        self.war_signups
                            .by_war
                            .get(&war)
                            .map(|signups| {
                                signups
                                    .iter()
                                    .map(|signup| {
                                        CharacterRef(Some(*signup.character.as_bytes()))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();
                match WarServerMsg::full_manifest(
                    header,
                    &WarboardManifest::of_participants(participants),
                ) {
                    Ok(manifest) => answers.push(ServerMessage::War(manifest)),
                    Err(error) => {
                        tracing::warn!(%error, "war board manifest not sent");
                    }
                }
            }
            // No answer in any capture; see the module text.
            WarClientMsg::WarboardManifestReceived { .. }
            | WarClientMsg::StopSpectating { .. } => {}
        }
        send_now(&mut self.combat_messages, connection, &answers);
    }

    /// Carries out the war sign-up call `call` of the client of `connection`.
    pub fn war_signup_call(&mut self, connection: &ConnectionRef, call: WarSignupClientMsg) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        let header = call.header();
        let Some((character, name)) = self
            .world
            .get::<Player>(entity)
            .map(|player| (player.character, player.name.clone()))
        else {
            return;
        };
        let mut answers: Vec<ServerMessage> = Vec::new();
        match call {
            WarSignupClientMsg::RequestSignup { war, territory, .. } => {
                let signups = self.war_signups.by_war.entry(war).or_default();
                if !signups.iter().any(|signup| signup.character == character) {
                    signups.push(Signup {
                        character,
                        name,
                        territory,
                    });
                }
                if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                    player.game_mode.war_last = Some(war);
                }
                answers.push(ServerMessage::WarSignup(
                    WarSignupServerMsg::SignupResponseReceived {
                        header,
                        result: SIGNUP_RESULT_OK,
                        value: SIGNUP_SUCCESS_VALUE,
                    },
                ));
            }
            WarSignupClientMsg::RequestLeave { territory, .. } => {
                for signups in self.war_signups.by_war.values_mut() {
                    signups.retain(|signup| {
                        signup.character != character || signup.territory != territory
                    });
                }
            }
            WarSignupClientMsg::RequestSignupList { war, .. } => {
                if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                    player.game_mode.war_last = Some(war);
                }
                answers.push(ServerMessage::WarSignup(
                    WarSignupServerMsg::SignupListReceived {
                        header,
                        war,
                        extra: SIGNUP_LIST_EXTRA,
                        entries: self.war_signups.entries(war),
                        trailer: SIGNUP_LIST_TRAILER,
                    },
                ));
            }
            WarSignupClientMsg::RequestRoster { .. } => {
                let war = self.war_signups.war_of(character).or_else(|| {
                    self.world
                        .get::<Player>(entity)
                        .and_then(|player| player.game_mode.war_last)
                });
                if let Some(war) = war {
                    answers.push(ServerMessage::WarSignup(
                        WarSignupServerMsg::SignupListReceived {
                            header,
                            war,
                            extra: SIGNUP_LIST_EXTRA,
                            entries: self.war_signups.entries(war),
                            trailer: SIGNUP_LIST_TRAILER,
                        },
                    ));
                }
            }
            WarSignupClientMsg::RequestSignupCount { war, .. } => {
                answers.push(ServerMessage::WarSignup(
                    WarSignupServerMsg::SignupCountReceived {
                        header,
                        war,
                        count: self.war_signups.count(war),
                    },
                ));
            }
            // No answer in any capture; the client itself asks for the counts next.
            WarSignupClientMsg::RequestRemoteInteract { .. } => {}
        }
        send_now(&mut self.combat_messages, connection, &answers);
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::FacetHeader;

    use super::*;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, connection, messages_to, world_with_spawns};

    /// The player number of the test player.
    const PLAYER: u64 = 41;
    /// The facet header of the test calls.
    const HEADER: FacetHeader = FacetHeader {
        target: 0,
        entity: 0,
    };
    /// The Outpost Rush activity of the retail queue calls.
    const ACTIVITY: u16 = 0x3116;
    /// A war and its territory.
    const WAR: u32 = 1;
    const TERRITORY: u16 = 11;

    fn world() -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        let admit = admit_at(PLAYER, 0.0, true);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        world
    }

    #[test]
    fn a_queue_join_asks_for_a_login_and_a_confirm_opens_role_selection() {
        let mut world = world();
        world.game_mode_call(
            &connection(PLAYER),
            GameModeClientMsg::JoinQueueForActivities {
                header: HEADER,
                activities: vec![QueueActivity {
                    activity: ACTIVITY,
                    extra: 0,
                    flag: 1,
                }],
            },
        );
        let sent = world.take_messages();
        assert_eq!(
            messages_to(&sent, PLAYER),
            [ServerMessage::GameMode(
                GameModeServerMsg::MatchmakingLoginRequested { header: HEADER }
            )]
        );
        world.game_mode_call(
            &connection(PLAYER),
            GameModeClientMsg::MatchmakingLoginConfirmed { header: HEADER },
        );
        let sent = world.take_messages();
        assert_eq!(
            messages_to(&sent, PLAYER),
            [ServerMessage::GameMode(
                GameModeServerMsg::StartMatchmakingRoleSelection {
                    header: HEADER,
                    flag: ROLE_SELECTION_FLAG,
                    activity: ACTIVITY,
                    extra: ROLE_SELECTION_EXTRA,
                }
            )]
        );
        world.game_mode_call(
            &connection(PLAYER),
            GameModeClientMsg::JoinQueueForActivities {
                header: HEADER,
                activities: Vec::new(),
            },
        );
        let sent = world.take_messages();
        assert_eq!(
            messages_to(&sent, PLAYER),
            [ServerMessage::GameMode(
                GameModeServerMsg::QueueGroupRemoved { header: HEADER }
            )]
        );
    }

    #[test]
    fn a_readied_and_accepted_match_ends_the_queue() {
        let mut world = world();
        world.game_mode_call(
            &connection(PLAYER),
            GameModeClientMsg::JoinQueueForActivities {
                header: HEADER,
                activities: vec![QueueActivity {
                    activity: ACTIVITY,
                    extra: 0,
                    flag: 1,
                }],
            },
        );
        world.game_mode_call(
            &connection(PLAYER),
            GameModeClientMsg::SetMatchmakingRole {
                header: HEADER,
                role: 1,
            },
        );
        world.take_messages();
        let entity = world.player_by_number(PLAYER).expect("player");
        let (match_value, match_text) = world
            .world
            .get::<Player>(entity)
            .expect("player")
            .game_mode
            .match_id
            .expect("a match");
        world.game_mode_call(
            &connection(PLAYER),
            GameModeClientMsg::SetReadiedUpStatus {
                header: HEADER,
                match_id: match_value,
                ready: true,
            },
        );
        assert!(world.take_messages().is_empty(), "no answer before the accept");
        world.game_mode_call(
            &connection(PLAYER),
            GameModeClientMsg::AcceptMatch {
                header: HEADER,
                match_id: match_text.hyphenated().to_string(),
                flag: 1,
            },
        );
        let sent = world.take_messages();
        assert_eq!(
            messages_to(&sent, PLAYER),
            [ServerMessage::GameMode(
                GameModeServerMsg::QueueGroupRemoved { header: HEADER }
            )]
        );
        let mode = &world
            .world
            .get::<Player>(entity)
            .expect("player")
            .game_mode;
        assert!(mode.queued.is_empty() && mode.match_id.is_none());
    }

    #[test]
    fn a_sign_up_is_answered_and_listed_and_counted() {
        let mut world = world();
        world.war_signup_call(
            &connection(PLAYER),
            WarSignupClientMsg::RequestSignup {
                header: HEADER,
                war: WAR,
                territory: TERRITORY,
            },
        );
        let sent = world.take_messages();
        assert_eq!(
            messages_to(&sent, PLAYER),
            [ServerMessage::WarSignup(
                WarSignupServerMsg::SignupResponseReceived {
                    header: HEADER,
                    result: SIGNUP_RESULT_OK,
                    value: SIGNUP_SUCCESS_VALUE,
                }
            )]
        );
        world.war_signup_call(
            &connection(PLAYER),
            WarSignupClientMsg::RequestSignupCount {
                header: HEADER,
                war: WAR,
            },
        );
        let sent = world.take_messages();
        assert_eq!(
            messages_to(&sent, PLAYER),
            [ServerMessage::WarSignup(
                WarSignupServerMsg::SignupCountReceived {
                    header: HEADER,
                    war: WAR,
                    count: 1,
                }
            )]
        );
        world.war_signup_call(
            &connection(PLAYER),
            WarSignupClientMsg::RequestSignupList {
                header: HEADER,
                war: WAR,
            },
        );
        let sent = world.take_messages();
        let listed = messages_to(&sent, PLAYER);
        let [ServerMessage::WarSignup(WarSignupServerMsg::SignupListReceived {
            entries,
            trailer,
            ..
        })] = listed.as_slice()
        else {
            panic!("a sign-up list");
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].flag, SIGNUP_ENTRY_FLAG);
        assert_eq!(*trailer, SIGNUP_LIST_TRAILER);
        world.war_signup_call(
            &connection(PLAYER),
            WarSignupClientMsg::RequestLeave {
                header: HEADER,
                territory: TERRITORY,
            },
        );
        assert_eq!(world.war_signups.count(WAR), 0);
    }
}
