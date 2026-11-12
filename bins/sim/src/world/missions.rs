// SPDX-License-Identifier: AGPL-3.0-or-later
//! Mission boards (finding "mission board", retail board capture 2026-06-13). The player opens
//! a town project board (interaction kind 51); the server accepts the interaction and names the
//! board in the player's objective interactor state (3829) with an empty mission list (retail
//! 21.328 s and 21.342 s). The same interaction on a faction representative (an NPC) opens the
//! faction board, whose state lists the missions it offers ([`super::faction_board`]). The client takes a town mission by its id and its project (3819); the
//! mission runs its objective template with its own values ([`crate::player_content::MissionRow`])
//! and counts against the missions journal limit. The client turns a mission in at the board by
//! its instance (2461): that is the interact of its `ReturnToGiver` task.

use bevy_ecs::entity::Entity;
use nw_bus::{ConnectionRef, MISSION_BOARD_ID_LEN, MissionTake};
use nw_messages::items::FullList;
use nw_messages::list_delta::ReplicatedList;
use nw_messages::objective_interactor::{OBJECTIVE_INTERACTOR_COMPONENT, ObjectiveInteractorState};
use nw_messages::{BundleMember, InteractionRequest, MemberState, ObjectiveInteractorClientMsg};

use super::conversation::NPC_NAME_MARK;
use super::faction_board::Taken;
use super::gathering::GATHER_ACCEPTED;
use super::gear::unix_ns;
use super::{Body, Player, SimWorld, gather_answer, send_now};
use crate::objectives::Event;
use nw_sim_core::Position;

/// The interact tags of a mission's turn-in task (`M_ReturnToGiver`, `CM_ReturnToGiver`,
/// `M_ReturnToAnyGiver` of the templates).
pub(super) const TURN_IN_TAGS: [&str; 2] = ["ReturnToGiver", "ReturnToAnyGiver"];

impl SimWorld {
    /// Player `number` (`entity`) opens the board of `request` (its name `name`) at `now_ms`:
    /// the interaction is accepted and the player's objective interactor names the board. A
    /// town project board has no offered missions (retail: the client lists the town missions
    /// itself and takes one by its id); a faction representative's board (the name of an NPC)
    /// offers its missions until they expire.
    pub(super) fn open_board(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
        now_ms: u64,
    ) {
        let at = self
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position());
        let provider = at
            .and_then(|at| self.zone_plot_near(at))
            .unwrap_or(request.target);
        let faction_board = name
            .first()
            .is_some_and(|byte| char::from(*byte) == NPC_NAME_MARK);
        let (expiration_time, elements) = if faction_board {
            let (expiration_ns, missions) = self.faction_offers(entity, provider, now_ms);
            (Some(expiration_ns), missions)
        } else {
            (None, Vec::new())
        };
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let answer = gather_answer(number, request, GATHER_ACCEPTED, name);
        send_now(&mut self.combat_messages, &player.connection, &[answer]);
        player.open_board = Some(provider);
        player.board_version += 1;
        let state = ObjectiveInteractorState {
            provider: Some(provider),
            expiration_time,
            missions: Some(ReplicatedList::Full(FullList {
                version: Some(player.board_version),
                elements,
            })),
            community_goals: None,
        };
        player.extra_members.push(BundleMember {
            component: OBJECTIVE_INTERACTOR_COMPONENT,
            state: MemberState::ObjectiveInteractor(state),
        });
    }

    /// The raw id of the zone capital plot nearest to `at`: the provider retail names for a
    /// board in that zone (Inferred: both retail Windsward boards named the nearest one,
    /// `Zone_CapitalPlot_SouthWindsward`, 690 m from the town board).
    fn zone_plot_near(&self, at: Position) -> Option<[u8; MISSION_BOARD_ID_LEN]> {
        let distance = |place: &[f32; 3]| (place[0] - at.x).powi(2) + (place[1] - at.y).powi(2);
        self.content
            .zone_plots
            .iter()
            .min_by(|a, b| distance(&a.0).total_cmp(&distance(&b.0)))
            .map(|(_, id)| *id)
    }

    /// A board call of the player of `connection` at `now_ms`.
    pub fn board_call(
        &mut self,
        connection: &ConnectionRef,
        call: ObjectiveInteractorClientMsg,
        now_ms: u64,
    ) {
        let Some(number) = self
            .by_connection
            .get(connection)
            .and_then(|entity| self.world.get::<Player>(*entity))
            .map(|player| player.number)
        else {
            return;
        };
        match call {
            ObjectiveInteractorClientMsg::SelectCommunityGoal { mission, .. } => {
                self.take_mission(number, mission, now_ms);
            }
            ObjectiveInteractorClientMsg::CompleteObjective { instance, .. } => {
                self.turn_in_mission(number, instance, now_ms);
            }
            ObjectiveInteractorClientMsg::SelectObjective { index, .. } => {
                self.take_faction_mission(number, Taken::Index(index), now_ms);
            }
            ObjectiveInteractorClientMsg::SelectPvpMission { mission, .. } => {
                self.take_faction_mission(number, Taken::Mission(mission), now_ms);
            }
        }
    }

    /// Player `number` takes the mission whose lowercase id has the CRC-32 `mission` at its open
    /// board, at `now_ms`.
    fn take_mission(&mut self, number: u64, mission: u32, now_ms: u64) {
        let Some(row) = self
            .content
            .quests
            .mission_id_by_crc(mission)
            .and_then(|id| self.content.quests.mission(id))
            .cloned()
        else {
            tracing::debug!(player = number, mission, "take of an unknown mission");
            return;
        };
        let board = self
            .player_by_number(number)
            .and_then(|entity| self.world.get::<Player>(entity))
            .and_then(|player| player.open_board)
            .unwrap_or([0; MISSION_BOARD_ID_LEN]);
        let take = MissionTake {
            board,
            accepted_ns: unix_ns(self.server_ms),
            territory: row.territory,
        };
        match self.accept_quest(number, &row.id, Some(take), now_ms) {
            Ok(()) => tracing::info!(player = number, mission = %row.id, "mission taken"),
            Err(refusal) => {
                tracing::debug!(player = number, mission = %row.id, %refusal, "mission refused");
            }
        }
    }

    /// Player `number` turns in the mission of instance `instance` at `now_ms`: its turn-in
    /// task is done, and the mission ends when its other tasks are.
    fn turn_in_mission(&mut self, number: u64, instance: u64, now_ms: u64) {
        let journal = self
            .player_by_number(number)
            .and_then(|entity| self.world.get::<Player>(entity))
            .and_then(|player| player.quest.clone());
        let Some(journal) = journal else {
            return;
        };
        let Some(id) = self
            .quests_of_player(number)
            .iter()
            .find(|quest| {
                quest.def.mission.is_some() && journal.instance(&quest.def.id) == instance
            })
            .map(|quest| quest.def.id.clone())
        else {
            tracing::debug!(
                player = number,
                instance,
                "turn-in of a mission not running"
            );
            return;
        };
        for tag in TURN_IN_TAGS {
            self.quest_event_for(number, &id, Event::Interacted(tag), now_ms);
        }
        self.update_quest_tracker(number);
    }
}
