// SPDX-License-Identifier: AGPL-3.0-or-later
//! Quest NPCs (beach-quests note, sections 2 and 3). The quest givers of the start beach
//! (Aelstrom, Urda, Grace) are actors of the client's level data: the server never cards them.
//! The client picks what an NPC offers from the player's achievements; the server answers a talk
//! (interaction kind 63) and takes the accept of a quest (3772) and the turn-in of a finished one
//! (794).

use bevy_ecs::entity::Entity;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::InteractionRequest;

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
        match self.start_quest(number, &def.id, now_ms) {
            Ok(()) => tracing::info!(player = number, quest = %def.id, "quest accepted"),
            Err(refusal) => {
                tracing::debug!(player = number, quest = %def.id, %refusal, "accept refused");
            }
        }
    }

    /// Player `number` (`entity`) turns in the quest whose lowercase id has the CRC-32
    /// `objective`: when it is the player's active quest and its tasks are done, the tracker
    /// shows it completed and the player gets its rewards. The client asks only at the quest's
    /// NPC; the server does not check the NPC (our choice: it has no NPC positions).
    pub(super) fn turn_in(&mut self, entity: Entity, number: u64, objective: u32) {
        let Some(quest) = self
            .quests
            .get(&number)
            .filter(|quest| lowercase_crc32(&quest.def.id) == objective)
        else {
            tracing::debug!(player = number, objective, "turn-in of a quest not active");
            return;
        };
        if !quest.run.finished() {
            tracing::debug!(player = number, quest = %quest.def.id, "turn-in before its end");
            return;
        }
        let tasks = quest.run.progress();
        if let Some(mut player) = self.world.get_mut::<Player>(entity)
            && let Some(tracker) = player.quest.as_mut()
        {
            tracker.update(tasks, true);
        }
        self.complete_quest(number);
    }
}
