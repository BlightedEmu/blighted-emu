// SPDX-License-Identifier: AGPL-3.0-or-later
//! The start beach's quest steps besides talks, kills and crafts (retail 2026-09-18, quests 4 to 7
//! of a new character): the points of interest the client says the player enters and leaves
//! (2297, 6416), the game events a task fires (a whisper, 5070), the lore notes the player reads
//! (interaction kind 47), and the level's quest objects that the player breaks with its weapons
//! (Grace's corruption nodules). The quest objects the player gathers (a treasure, a flag) go
//! through [`super::gathering`].

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use nw_content::poi_tags::PoiTags;
use nw_content::rewards::Reward;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::{InteractionRequest, ServerMessage, StartWhisperMsg};

use super::{Body, Player, SimWorld, gather_answer, send_now};
use crate::objectives::Event;
use nw_bus::ConnectionRef;

/// The status of an accepted lore read (retail: 1, with the note's title as its name and no
/// 3746 after it).
const LORE_ACCEPTED: u8 = 1;
/// Starts the achievement id of a lore note; the rest is its title without the
/// [`LOCALIZED_MARK`] (Inferred: the client names `@00_Aeternum_Title_31_01`, the ship's
/// manifest task of Lessons of a Pirate's Life waits for `Lore_unlock_00_Aeternum_Title_31_01`,
/// and every `Lore_unlock_` achievement has the game event `Read_LoreItem`).
const LORE_ACHIEVEMENT_PREFIX: &str = "Lore_unlock_";
/// Starts a localized name.
const LOCALIZED_MARK: char = '@';
/// Swing hits that break a quest object (Observed: each of Grace's nodules broke on the
/// greatsword's second 2071, 0.8 s after the first).
const HITS_TO_BREAK: u32 = 2;
/// How far from the player a swing breaks a quest object (Inferred: about a greatsword's reach
/// plus the object's size; the client's hit shapes are Unknown here).
const SWING_REACH_M: f32 = 4.0;
/// How far from the player a spread shot breaks a quest object at once (Observed: one
/// blunderbuss shot broke the fourth nodule; the distance is our choice).
const SHOT_REACH_M: f32 = 20.0;
/// The shortest time between two strikes of one player (our choice: the client repeats an
/// attack press in older input records).
const STRIKE_INTERVAL_MS: u64 = 500;

/// A weapon attack that can break quest objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strike {
    /// A melee swing: one hit on the nearest object in reach.
    Swing,
    /// A spread shot: it breaks the nearest object in its reach.
    Shot,
}

impl Strike {
    const fn reach_m(self) -> f32 {
        match self {
            Self::Swing => SWING_REACH_M,
            Self::Shot => SHOT_REACH_M,
        }
    }

    const fn hits(self) -> u32 {
        match self {
            Self::Swing => 1,
            Self::Shot => HITS_TO_BREAK,
        }
    }
}

/// One player's hits on the level's quest objects.
#[derive(Debug, Default)]
pub(super) struct Strikes {
    /// Hits so far on each object, by its index, and until when a broken one stays broken.
    objects: HashMap<usize, (u32, Option<u64>)>,
    /// When its last strike landed.
    last_ms: Option<u64>,
}

/// The `POITag`s of the territories `territories`.
pub(super) fn places<'a>(poi_tags: &'a PoiTags, territories: &[u16]) -> Vec<&'a str> {
    territories
        .iter()
        .flat_map(|territory| poi_tags.of(u32::from(*territory)))
        .map(String::as_str)
        .collect()
}

/// Gives `reward`'s coins and experience to `player`.
pub(super) fn grant(player: &mut Player, reward: &Reward) {
    if reward.coins > 0 {
        player.coins = player.coins.saturating_add(reward.coins);
        player.coins_changed = true;
    }
    if reward.experience > 0 {
        player.experience = player.experience.saturating_add(reward.experience);
        player.experience_changed = true;
    }
}

impl SimWorld {
    /// The client of player `entity` says it came into territory `territory` (2297), or left it
    /// (6416); the quests' go-to and exit tasks read it at the next tick.
    pub(super) fn territory_changed(&mut self, entity: Entity, territory: u16, inside: bool) {
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        player.territories.retain(|known| *known != territory);
        if inside {
            player.territories.push(territory);
        }
        tracing::debug!(player = player.number, territory, inside, "territory");
    }

    /// Player `number` (`entity`) reads the lore note of interaction `request`, whose title the
    /// client names `name`: the read is accepted (3743 kind 47 status 1 with the title, as
    /// retail) and the note's achievement is set, an event of its quest.
    pub(super) fn read_lore(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
        now_ms: u64,
    ) {
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let answer = gather_answer(number, request, LORE_ACCEPTED, name);
        send_now(&mut self.combat_messages, &player.connection, &[answer]);
        let title = String::from_utf8_lossy(name);
        let achievement = format!(
            "{LORE_ACHIEVEMENT_PREFIX}{}",
            title.trim_start_matches(LOCALIZED_MARK)
        );
        player
            .flow
            .set_achievement(&self.achievement_rules, &achievement);
        tracing::info!(player = number, %achievement, "lore read");
        self.mark_quests_unsaved(number);
        self.quest_event(number, Event::AchievementSet(&achievement), now_ms);
    }

    /// Fires game event `event` for player `number`: its whisper plays on the client (5070),
    /// its coins, experience and item go to the player, and it is an event of the quest.
    pub(super) fn fire_game_event(&mut self, number: u64, event: &str, now_ms: u64) {
        let reward = self.content.rewards.get(event).cloned().unwrap_or_default();
        if let Some(entity) = self.player_by_number(number)
            && let Some(mut player) = self.world.get_mut::<Player>(entity)
        {
            if let Some(whisper) = &reward.whisper {
                let start = ServerMessage::StartWhisper(StartWhisperMsg::to(
                    number,
                    lowercase_crc32(whisper),
                ));
                send_now(&mut self.combat_messages, &player.connection, &[start]);
            }
            grant(&mut player, &reward);
        }
        tracing::info!(player = number, event, whisper = ?reward.whisper, "game event");
        self.give_loot(number, reward.item.into_iter().collect(), now_ms);
        self.quest_event(number, Event::GameEvent(event), now_ms);
    }

    /// The player of `connection` attacks at `now_ms` (`strike`): the nearest quest object in
    /// reach whose game event its quest counts takes the hits, and a broken one is that game
    /// event for the quest. The client is not told: the server does not know the object's entity
    /// id (Unknown; retail sent 2071 and 2724 to it).
    pub fn strike_quest_objects(
        &mut self,
        connection: &ConnectionRef,
        strike: Strike,
        now_ms: u64,
    ) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        let Some(at) = self
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position())
        else {
            return;
        };
        let Some(player) = self.world.get::<Player>(entity) else {
            return;
        };
        let number = player.number;
        if player
            .strikes
            .last_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < STRIKE_INTERVAL_MS)
        {
            return;
        }
        let Some(quest) = self.quests.get(&number) else {
            return;
        };
        let target = self
            .content
            .quest_objects
            .nearest(at, strike.reach_m(), |index, object| {
                let broken = player
                    .strikes
                    .objects
                    .get(&index)
                    .and_then(|(_, until)| *until)
                    .is_some_and(|until| now_ms < until);
                !broken && quest.run.counts_game_event(&object.event)
            })
            .map(|(index, object)| (index, object.event.clone(), object.respawn_ms));
        let Some((index, event, respawn_ms)) = target else {
            return;
        };
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        player.strikes.last_ms = Some(now_ms);
        let (hits, until) = player.strikes.objects.entry(index).or_default();
        *hits = if until.is_some() { 0 } else { *hits } + strike.hits();
        *until = None;
        if *hits < HITS_TO_BREAK {
            return;
        }
        *hits = 0;
        *until = Some(now_ms.saturating_add(respawn_ms));
        tracing::info!(player = number, object = index, %event, "quest object broken");
        self.quest_event(number, Event::GameEvent(&event), now_ms);
    }
}
