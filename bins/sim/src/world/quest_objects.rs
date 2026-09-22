// SPDX-License-Identifier: AGPL-3.0-or-later
//! The start beach's quest steps besides talks, kills and crafts (retail 2026-09-18, quests 4 to 7
//! of a new character): the points of interest the client says the player enters and leaves
//! (2297, 6416), the game events a task fires (a whisper, 5070), the lore notes the player reads
//! (interaction kind 47), and the level's quest objects that the player breaks with its weapons
//! (Grace's corruption nodules). The quest objects the player gathers (a treasure, a flag) go
//! through [`super::gathering`].

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use nw_content::gameplay_config::{GameplayConfig, XpMultipliers};
use nw_content::player_attacks::{AttackHit, AttackKind};
use nw_content::poi_tags::PoiTags;
use nw_content::rewards::Reward;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::{InputRecord, InteractionRequest, ServerMessage, StartWhisperMsg};

use super::attack::counter_is_newer;
use super::{Body, Player, SimWorld, gather_answer, send_now};
use crate::objectives::Event;
use crate::stats::{GAME_EVENT_STAT, ONE_COMPLETION, StatEvent};
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

/// A weapon attack that can break quest objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strike {
    /// A melee swing: one hit on the nearest object in reach.
    Swing,
    /// A spread shot: it breaks the nearest object in its reach (Observed: one blunderbuss shot
    /// broke the fourth nodule).
    Shot,
}

impl Strike {
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
    /// The counter of the newest input record its swings were read from: the client repeats a
    /// record in the older copies of its next inputs (swing note: counters 7290, 7289, 7288), so
    /// a press counts in the first input that carries it only.
    input_counter: Option<u16>,
}

/// The `POITag`s of the territories `territories`.
pub(super) fn places<'a>(poi_tags: &'a PoiTags, territories: &[u16]) -> Vec<&'a str> {
    territories
        .iter()
        .flat_map(|territory| poi_tags.of(u32::from(*territory)))
        .map(String::as_str)
        .collect()
}

/// Gives `reward`'s coins and experience to `player`, under the remote config's wallet cap
/// and character experience multiplier (`gameplay`).
pub(super) fn grant(player: &mut Player, reward: &Reward, gameplay: &GameplayConfig) {
    if reward.coins > 0 {
        player.coins = with_coins(player.coins, reward.coins, gameplay.wallet_cap);
        player.coins_changed = true;
    }
    gain_experience(player, reward.experience, &gameplay.xp);
}

/// The coins of a wallet of `coins` that gets `gained`: at most `cap`
/// (`javelin.wallet-cap-player`, Inferred from its name: no client code reads it, so the
/// server applies it), and a wallet already above the cap keeps what it has.
fn with_coins(coins: u64, gained: u64, cap: Option<u64>) -> u64 {
    let total = coins.saturating_add(gained);
    cap.map_or(total, |cap| total.min(cap).max(coins))
}

/// Gives `points` of character experience to `player`, times the remote config's character
/// multiplier (`javelin.xp-multipliers.character`).
pub(super) fn gain_experience(player: &mut Player, points: u32, xp: &XpMultipliers) {
    let gained = xp.character_gain(points);
    if gained > 0 {
        player.experience = player.experience.saturating_add(gained);
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
    /// its coins, experience and item go to the player, it counts on the leaderboards it feeds,
    /// and it is an event of the quest.
    pub(super) fn fire_game_event(&mut self, number: u64, event: &str, now_ms: u64) {
        let reward = self.content.rewards.get(event).cloned().unwrap_or_default();
        if let Some(entity) = self.player_by_number(number)
            && let Some(mut player) = self.world.get_mut::<Player>(entity)
        {
            self.stat_events.push(StatEvent {
                character: player.character,
                stat_type: GAME_EVENT_STAT,
                game_mode: event.to_owned(),
                value: ONE_COMPLETION,
            });
            if let Some(whisper) = &reward.whisper {
                let start = ServerMessage::StartWhisper(StartWhisperMsg::to(
                    number,
                    lowercase_crc32(whisper),
                ));
                send_now(&mut self.combat_messages, &player.connection, &[start]);
            }
            grant(&mut player, &reward, &self.content.gameplay);
        }
        tracing::info!(player = number, event, whisper = ?reward.whisper, "game event");
        self.give_loot(number, reward.item.into_iter().collect(), now_ms);
        self.quest_event(number, Event::GameEvent(event), now_ms);
    }

    /// The player of `connection` sent the input `records` (newest first) at `now_ms`: a new
    /// press of the attack key in them is one swing at the quest objects
    /// ([`Self::strike_quest_objects`]); a press its earlier inputs carried does not count again.
    pub fn swing_at_quest_objects(
        &mut self,
        connection: &ConnectionRef,
        records: &[InputRecord],
        now_ms: u64,
    ) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let last = player.strikes.input_counter;
        let new = |record: &&InputRecord| {
            last.is_none_or(|last| counter_is_newer(record.u16_after_mask, last))
        };
        let pressed = records.iter().filter(new).any(InputRecord::attack_pressed);
        if let Some(newest) = records.iter().find(new) {
            player.strikes.input_counter = Some(newest.u16_after_mask);
        }
        if pressed {
            self.strike_quest_objects(entity, Strike::Swing, SWING_REACH_M, now_ms);
        }
    }

    /// The player of `connection` fired a spread shot at `now_ms` that the server took: it
    /// strikes the quest objects within its hand weapon's hit-scan range (the stance's light
    /// attack: `BlunderbussPrimaryFire_Spread`, 14 m in `rangedattackdatabase.radb`).
    pub fn shoot_at_quest_objects(&mut self, connection: &ConnectionRef, now_ms: u64) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        if let Some(reach_m) = self.hit_scan_range_m(entity) {
            self.strike_quest_objects(entity, Strike::Shot, reach_m, now_ms);
        }
    }

    /// The hit-scan range of the light attack of the weapon in `entity`'s hand.
    fn hit_scan_range_m(&self, entity: Entity) -> Option<f32> {
        let data = &self.combat.data;
        let (item, _) = self
            .world
            .get::<Player>(entity)?
            .gear
            .as_ref()?
            .main_weapon()?;
        let weapon = data.weapon_of_item(item)?;
        match data.stance(weapon.stance)?.attack(AttackKind::Light)?.hit {
            AttackHit::HitScan { range_m, .. } => Some(range_m),
            AttackHit::Melee { .. } | AttackHit::Projectile { .. } => None,
        }
    }

    /// Player `entity` attacks at `now_ms` (`strike`, reaching `reach_m`): the nearest quest
    /// object in reach whose game event its quest counts takes the hits, and a broken one is
    /// that game event for the quest. The client is not told: the server does not know the
    /// object's entity id (Unknown; retail sent 2071 and 2724 to it).
    fn strike_quest_objects(
        &mut self,
        entity: Entity,
        strike: Strike,
        reach_m: f32,
        now_ms: u64,
    ) {
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
        let Some(quest) = self.quests.get(&number) else {
            return;
        };
        let target = self
            .content
            .quest_objects
            .nearest(at, reach_m, |index, object| {
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
