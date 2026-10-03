// SPDX-License-Identifier: AGPL-3.0-or-later
//! Quest NPCs (beach-quests note, sections 2 and 3). The quest givers of the start beach
//! (Aelstrom, Urda, Grace) are actors of the client's level data: the server never cards them.
//! The client picks what an NPC offers from the player's achievements; the server answers a talk
//! (interaction kind 63) and takes the accept of a quest (3772) and the turn-in of a finished one
//! (794).

use bevy_ecs::entity::Entity;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::{
    ConversationClientMsg, GameModeSchedulerRefreshedMsg, InteractionRequest, ServerMessage,
};

use super::{Player, SimWorld, gather_answer, send_now};
use crate::player_content::requirement_met;

/// The status of an accepted talk.
const TALK_ACCEPTED: u8 = 1;
/// The name a talk's answer carries: retail `@01_Reese` for Aelstrom and `@7507_Swap` for Urda
/// (Inferred: the client takes the NPC from its own level data and does not read it, so one name
/// serves every NPC).
const TALK_NAME: &[u8] = b"@01_Reese";

impl SimWorld {
    /// Player `number` (`entity`) talks to an NPC: the talk is accepted at once (3743 kind 63
    /// status 1). The client opens the conversation from its own data and ends it with its cancel
    /// (2473); retail sends no 3746.
    pub(super) fn talk(&mut self, entity: Entity, number: u64, request: InteractionRequest) {
        let Some(player) = self.world.get::<Player>(entity) else {
            return;
        };
        tracing::debug!(player = number, target = ?request.target, "talk");
        let answer = gather_answer(number, request, TALK_ACCEPTED, TALK_NAME);
        send_now(&mut self.combat_messages, &player.connection, &[answer]);
    }

    /// Player `number` (`entity`) accepts the quest whose lowercase id has the CRC-32
    /// `objective` at `now_ms`: it starts when an NPC would offer it, that is when the player
    /// has the achievements of its `RequiredAchievementID` (the archetype variants of a quest
    /// differ only there).
    pub(super) fn accept_objective(
        &mut self,
        entity: Entity,
        number: u64,
        objective: u32,
        now_ms: u64,
    ) {
        let Some(def) = self.content.quests.quest_by_crc(objective) else {
            tracing::debug!(player = number, objective, "accept of an unknown quest");
            return;
        };
        let rules = &self.achievement_rules;
        let offered = self.world.get::<Player>(entity).is_some_and(|player| {
            def.requirement.as_deref().is_none_or(|requirement| {
                requirement_met(requirement, |id| player.flow.has_achievement(rules, id))
            })
        });
        if !offered {
            tracing::debug!(player = number, quest = %def.id, "accept of a quest not offered");
            return;
        }
        match self.accept_quest(number, &def.id, now_ms) {
            Ok(()) => tracing::info!(player = number, quest = %def.id, "quest accepted"),
            Err(refusal) => {
                tracing::debug!(player = number, quest = %def.id, %refusal, "accept refused");
            }
        }
    }

    /// Player `number` (`entity`) turns in the quest whose lowercase id has the CRC-32
    /// `objective`: when it is one of the player's running quests and its tasks are done, the tracker
    /// shows it completed, the player gets its rewards and gives back the items it brought, and
    /// its game mode schedule is refreshed (6426; retail 2026-09-18: 27 ms after each 794). The
    /// client asks only at the quest's NPC (`NpcDestinationId`); the 794 names no NPC, only the
    /// objective. Whether retail checks the NPC or its distance is Unknown: no capture holds a
    /// refused 794, and the NPC's place (world file `npcData`) and the reach of its interact
    /// volume (the master NPC slice's 3 m cylinder, which slice variants may override with
    /// `InteractOverride_Radius`) are not checked against a retail turn-in yet.
    pub(super) fn turn_in(&mut self, entity: Entity, number: u64, objective: u32) {
        let Some(quest) = self
            .quests_of_player(number)
            .iter()
            .find(|quest| lowercase_crc32(&quest.def.id) == objective)
        else {
            tracing::debug!(player = number, objective, "turn-in of a quest not active");
            return;
        };
        if !quest.run.finished() {
            tracing::debug!(player = number, quest = %quest.def.id, "turn-in before its end");
            return;
        }
        let tasks = quest.run.progress();
        let id = quest.def.id.clone();
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            if let Some(journal) = player.quest.as_mut() {
                journal.update(&id, tasks, true);
            }
            let refreshed = ServerMessage::GameModeSchedulerRefreshed(
                GameModeSchedulerRefreshedMsg::to(number),
            );
            send_now(&mut self.combat_messages, &player.connection, &[refreshed]);
        }
        self.complete_quest(number, &id);
    }
}

impl SimWorld {
    /// A conversation call of the player of `connection` at `now_ms` (conversation-notify note,
    /// section 1). Retail answers neither: a picked topic (3158) changes nothing, and each
    /// topic the client names as finished (5397) completes a `TaskConversationTopic` of that id
    /// in the player's quests or tutorial.
    pub fn conversation_call(
        &mut self,
        connection: &nw_bus::ConnectionRef,
        call: ConversationClientMsg,
        now_ms: u64,
    ) {
        let ConversationClientMsg::FinishedTopics { topics, .. } = call else {
            return;
        };
        let Some(number) = self
            .by_connection
            .get(connection)
            .and_then(|entity| self.world.get::<Player>(*entity))
            .map(|player| player.number)
        else {
            return;
        };
        for topic in topics {
            let event = crate::objectives::Event::TopicFinished(topic);
            match self.tutorials.get_mut(&number) {
                Some(run) => {
                    let orders = run.event(event, &mut self.mobs, now_ms);
                    self.carry_out(number, orders, now_ms);
                }
                None => self.quest_event(number, event, now_ms),
            }
        }
    }
}
