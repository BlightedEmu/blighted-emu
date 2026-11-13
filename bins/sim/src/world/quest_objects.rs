// SPDX-License-Identifier: AGPL-3.0-or-later
//! The start beach's quest steps besides talks, kills and crafts (retail 2026-09-18, quests 4 to 7
//! of a new character): the points of interest the client says the player enters and leaves
//! (2297, 6416), the game events a task fires (a whisper, 5070), the lore notes the player reads
//! (interaction kind 47), and the level's quest objects that the player breaks with its weapons
//! (Grace's corruption nodules). The quest objects the player gathers (a treasure, a flag) go
//! through [`super::gathering`].

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use nw_content::character_points::{
    territory_experience_point_crc, territory_experience_share, territory_pool_crc,
};
use nw_content::gameplay_config::{GameplayConfig, XpMultipliers};
use nw_content::player_attacks::{AttackHit, AttackKind};
use nw_content::poi_tags::PoiTags;
use nw_content::rewards::Reward;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::facet::component;
use nw_messages::{
    BundleGroup, BundleMember, DamageDealtMsg, DamageReceiverState, FacetHeader,
    GatheringInterruptedMsg, GdeMetadataReplicatedState, InputRecord, InteractionRequest,
    MemberState, ServerMessage, StartWhisperMsg,
};
use nw_sim_core::Position;

use super::attack::counter_is_newer;
use super::crafting::{CARD_COMPONENT, gde_ref_of};
use super::slots::Slot;
use super::statics::object_gathered_whole;
use super::{Body, Player, SimWorld, gather_answer, send_now};
use crate::objectives::Event;
use crate::player_content::{QuestObjects, lore_title_key};
use crate::stats::{GAME_EVENT_STAT, ONE_COMPLETION, StatEvent};
use nw_bus::ConnectionRef;

/// The status of an accepted lore read (retail: 1, with the note's title as its name and no
/// 3746 after it).
const LORE_ACCEPTED: u8 = 1;
/// The status of a refused lore read (Inferred: the refusal of a busy gather, 2).
const LORE_REFUSED: u8 = 2;
/// How far from the player a lore note it reads may be, metres, across the ground (Inferred:
/// the client's interaction reach is Unknown; generous for a position a moment old).
const LORE_REACH_M: f32 = 15.0;
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
/// Hits that break a damageable quest object (`DamageableGatherable` slices such as
/// `Master_Quest_Damageable_Instanced`). Observed: `12a_msq_08_destroyrune`, the tablet broke on
/// its only 2071 (133.92 s, flags `0x68`, a crit). Inferred: every damageable drop object breaks
/// on one hit, as this one did.
const DAMAGEABLE_HITS_TO_BREAK: u32 = 1;
/// The game events one way into a detection sphere sets off.
const ONE_DETECTION: u32 = 1;
/// How far from the player a swing breaks a quest object (Inferred: about a greatsword's reach
/// plus the object's size; the client's hit shapes are Unknown here).
const SWING_REACH_M: f32 = 4.0;

/// First slot key of a quest object: its index in the level's objects is added. Keys are never
/// sent; they stay above every other entity range.
pub(super) const QUEST_OBJECT_ENTITY_BASE: u64 = 1 << 42;
/// The quest object's `DamageReceiverComponent` (Observed: component 1 of the retail nodule
/// groups, not blocking, block weapon slot alias 0x20, the values of a player's).
const OBJECT_DAMAGE_RECEIVER_COMPONENT: u64 = 1;

/// The slot keys of the quest objects with a GDE id within `radius_m` of `at`.
pub(super) fn numbers_near(
    objects: &QuestObjects,
    at: Position,
    radius_m: f32,
) -> impl Iterator<Item = u64> + '_ {
    objects
        .known_near(at, radius_m)
        .map(|index| QUEST_OBJECT_ENTITY_BASE + index as u64)
}

/// The spawn group of the quest object of slot key `number` in `slot` when the slot is new (it
/// came into view): its card and its damage receiver (retail nodule groups).
pub(super) fn group(objects: &QuestObjects, number: u64, slot: Slot) -> Option<BundleGroup> {
    let index = usize::try_from(number.checked_sub(QUEST_OBJECT_ENTITY_BASE)?).ok()?;
    let id = objects.get(index)?.id?;
    slot.new.then(|| BundleGroup {
        interest: u64::from(slot.interest),
        members: vec![
            BundleMember {
                component: CARD_COMPONENT,
                state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                    asset: None,
                    gde_ref: Some(gde_ref_of(id)),
                }),
            },
            BundleMember {
                component: OBJECT_DAMAGE_RECEIVER_COMPONENT,
                state: MemberState::DamageReceiver(DamageReceiverState::player()),
            },
        ],
    })
}

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
pub(super) fn with_coins(coins: u64, gained: u64, cap: Option<u64>) -> u64 {
    let total = coins.saturating_add(gained);
    cap.map_or(total, |cap| total.min(cap).max(coins))
}

/// Gives `points` of character experience to `player`, times the remote config's character
/// multiplier (`javelin.xp-multipliers.character`) and one plus its territory experience share
/// ([`SimWorld::refresh_territory_experience`]), rounded down once
/// ([`XpMultipliers::character_gain`]), and, while its faction holds the fort of its
/// territory, with the fort's bonus ([`nw_db::FORT_EXPERIENCE_BONUS_PERCENT`], rounded down).
/// The share is Observed on kills and game events (`Chart_Location`); on the other gains it is
/// Inferred (the bonus reads "XP gain" in its territory).
///
/// While its rested pool lasts, the gain takes the same again from the pool
/// ([`super::rested::RestedPool::take_bonus`]; Observed, retail `Playthrough_Part_02`: every gain
/// doubled, kills and quests and `Chart_Location`, and the last one +588 = 314 + 274). Every
/// gain here is a game event's, and no game event that gives experience has `UseRestedExp` 0.
pub(super) fn gain_experience(player: &mut Player, points: u32, xp: &XpMultipliers) {
    let mut gained = xp.character_gain(points, player.territory_experience_share);
    if player.fort_experience_bonus {
        gained = u32::try_from(nw_db::with_bonus(
            u64::from(gained),
            nw_db::FORT_EXPERIENCE_BONUS_PERCENT,
        ))
        .unwrap_or(u32::MAX);
    }
    if gained > 0 {
        let bonus = player.rested.take_bonus(gained, player.experience);
        player.experience = player
            .experience
            .saturating_add(gained)
            .saturating_add(bonus);
        player.experience_changed = true;
    }
}

impl SimWorld {
    /// The client of player `entity` says it came into territory `territory` (2297), or left it
    /// (6416); the quests' go-to and exit tasks read it at the next tick, and the zone it lies in
    /// is the player's last known zone.
    pub(super) fn territory_changed(&mut self, entity: Entity, territory: u16, inside: bool) {
        let zone = self.content.charting.territory_of(u32::from(territory));
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        player.territories.retain(|known| *known != territory);
        if inside {
            player.territories.push(territory);
        }
        if zone.is_some() {
            player.last_zone = zone;
        }
        tracing::debug!(
            player = player.number,
            territory,
            inside,
            ?zone,
            "territory"
        );
        self.refresh_territory_experience(entity);
    }

    /// Sets the territory experience share of player `entity`: the share
    /// ([`territory_experience_share`]) of its
    /// [`nw_content::character_points::TERRITORY_EXPERIENCE_BONUS`] points in the
    /// standing bonus pool of the territory it is in, 0 outside every territory (Observed: the
    /// share follows the territory of the kill's game event entry, findings
    /// `2026-11-13-kill-xp-rounding.md`). It changes when the player changes territory or
    /// spends a territory standing point.
    pub(super) fn refresh_territory_experience(&mut self, entity: Entity) {
        let Some(number) = self.world.get::<Player>(entity).map(|player| player.number) else {
            return;
        };
        let pool = self
            .current_territory(number)
            .map(|territory| territory_pool_crc(&territory));
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let points = pool.map_or(0, |pool| {
            player
                .masteries
                .territory_point_level(pool, territory_experience_point_crc())
        });
        player.territory_experience_share = territory_experience_share(points);
    }

    /// Player `number` (`entity`) reads the lore note of interaction `request`, whose title the
    /// client names `name`: the read is accepted (3743 kind 47 status 1 with the title, as
    /// retail) and the note's achievement is set, an event of its quest; the first read also
    /// fires the achievement's game event. A title that is no
    /// `LoreData` title, or whose lore point is not within reach of the player, is refused
    /// (status 2, Inferred as for a busy gather): a client cannot read lore from anywhere.
    pub(super) fn read_lore(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
        now_ms: u64,
    ) {
        let title = String::from_utf8_lossy(name);
        let in_reach = self
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position())
            .zip(self.content.lore_by_title.get(&lore_title_key(&title)))
            .is_some_and(|(at, lore)| {
                !self
                    .content
                    .lore_points
                    .within(*lore, at, LORE_REACH_M)
                    .is_empty()
            });
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        if !in_reach {
            tracing::debug!(player = number, %title, "lore read of no lore in reach");
            let refusal = gather_answer(number, request, LORE_REFUSED, &[]);
            send_now(&mut self.combat_messages, &player.connection, &[refusal]);
            return;
        }
        let answer = gather_answer(number, request, LORE_ACCEPTED, name);
        send_now(&mut self.combat_messages, &player.connection, &[answer]);
        let achievement = format!(
            "{LORE_ACHIEVEMENT_PREFIX}{}",
            title.trim_start_matches(LOCALIZED_MARK)
        );
        let first_read = !player
            .flow
            .has_achievement(&self.achievement_rules, &achievement);
        player
            .flow
            .set_achievement(&self.achievement_rules, &achievement);
        tracing::info!(player = number, %achievement, first_read, "lore read");
        self.mark_quests_unsaved(number);
        self.quest_event(number, Event::AchievementSet(&achievement), now_ms);
        // The first read fires the note's game event: its experience and standing (Inferred
        // from the achievement row's `GameEventID`; a note read again gives nothing more).
        if first_read
            && let Some(event) = self
                .achievement_rules
                .game_event(&achievement)
                .map(str::to_owned)
        {
            self.fire_game_event(number, &event, now_ms);
        }
    }

    /// Fires game event `event` for player `number`: its whisper plays on the client (5070),
    /// its coins, experience, territory standing and item go to the player, it counts on the leaderboards it feeds,
    /// and it is an event of the quest.
    pub(super) fn fire_game_event(&mut self, number: u64, event: &str, now_ms: u64) {
        let dice = &mut self.dice;
        let reward = self
            .content
            .rewards
            .rolled(event, &mut |below| dice.below(below))
            .unwrap_or_default();
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
        self.grant_event_standing(number, event);
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
    /// object in reach whose game event its quest counts, or whose gatherable a waiting item task
    /// drops from, takes the hits. A broken one is that game event for the quest and gives the
    /// drops of its gatherable (retail `12a_msq_08_destroyrune` 133.92 s: the tablet's 2071 and
    /// 2724, then `12A_MSQ_08_RuneShards` in the bag). The client is told when the object's
    /// capital is known (2071 and 2724 to it), and the break is a whole gather of it
    /// ([`Self::quest_object_gathered`]). A broken object stays broken for the player for its
    /// lock, else (a shared object) for its gatherable's `MinRespawnRate` (Inferred).
    fn strike_quest_objects(&mut self, entity: Entity, strike: Strike, reach_m: f32, now_ms: u64) {
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
        let quests = self.quests_of_player(number);
        if quests.is_empty() {
            return;
        }
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
                !broken
                    && quests.iter().any(|quest| {
                        quest.run.counts_game_event(&object.event)
                            || (object.drops
                                && !quest.run.gather_drops(&object.gatherable).is_empty())
                    })
            })
            .map(|(index, object)| {
                let known = object.id;
                let drops = object.drops.then(|| lowercase_crc32(&object.gatherable));
                (index, object.event.clone(), object.respawn_ms, known, drops)
            });
        let Some((index, event, respawn_ms, known, drops)) = target else {
            return;
        };
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        // Retail answers each hit with a 2071 on the object and a 2724 to its gatherable
        // controller (newchar 2026-09-18, every nodule hit).
        if let Some(id) = known {
            let hit =
                DamageDealtMsg::on_level_object(number, gde_ref_of(id), id, strike == Strike::Shot);
            let ended = GatheringInterruptedMsg {
                header: FacetHeader {
                    target: id.wrapping_add(component::NODULE_GATHERABLE_CONTROLLER),
                    entity: id,
                },
            };
            send_now(
                &mut self.combat_messages,
                &player.connection,
                &[
                    ServerMessage::DamageDealt(hit),
                    ServerMessage::GatheringInterrupted(ended),
                ],
            );
        }
        let (hits, until) = player.strikes.objects.entry(index).or_default();
        *hits = if until.is_some() { 0 } else { *hits } + strike.hits();
        *until = None;
        let needed = if drops.is_some() {
            DAMAGEABLE_HITS_TO_BREAK
        } else {
            HITS_TO_BREAK
        };
        if *hits < needed {
            return;
        }
        *hits = 0;
        *until = Some(now_ms.saturating_add(respawn_ms));
        // The break's whole gather (retail: with the break's 2724); an instanced object stays
        // broken for the player as long as its lock.
        let locked_ms = known.and_then(|_| self.quest_object_gathered(entity, index, at));
        if let Some(locked_ms) = locked_ms
            && let Some(mut player) = self.world.get_mut::<Player>(entity)
        {
            player
                .strikes
                .objects
                .insert(index, (0, Some(now_ms.saturating_add(locked_ms))));
        }
        tracing::info!(player = number, object = index, %event, "quest object broken");
        if let Some(gatherable) = drops
            .and_then(|crc| self.content.gatherables.get(crc))
            .cloned()
        {
            let loot = self.quest_drops(number, None, &gatherable);
            self.give_loot(number, loot, now_ms);
        }
        self.quest_event(number, Event::GameEvent(&event), now_ms);
    }

    /// Player `entity`, standing at `at`, gathers quest object `index` whole at the server's
    /// time: it broke the object or came near it. The player's gather state names the object
    /// with one whole gather ([`object_gathered_whole`]); an instanced object (its capital in
    /// [`nw_content::gathering::InstancedGathers`]) also leaves its lock in the player's loot
    /// tracker ([`SimWorld::instanced_lock`]: state 2, its slice's loot type and range, its
    /// variant's interact tag, the player's place as its position), a shared one none. Returns
    /// how long the lock lasts, ms; `None` for a shared object, an object whose capital is not
    /// known, or a world without a reset clock.
    ///
    /// Observed (finding D11, the retail states of children 4, 7 and 8 of
    /// `local-data/scratch-work/d10-tracker/states-478.tsv`): 265 whole gathers of
    /// `Master_Quest_Proximity_Instanced` (`m_instancedLootType` 4, `m_aoiRadius` 100) and 179
    /// of `Master_Quest_Damageable_Instanced` and the Monarch's Bluffs crates sent the tracker
    /// entry in the same record as the state (235 of 236 proximity ones), type 4; the entry's
    /// position is where the player stood (the 99C coffins 9.8 to 10.2 m from their capitals of
    /// a 10 m sphere, damageable breaks up to 48 m away by ranged hits); 402 of shared objects
    /// (`IO_World_Damageable_CollisionVines`, `Master_Quest_Proximity`, the fire droppers) sent
    /// no entry.
    fn quest_object_gathered(&mut self, entity: Entity, index: usize, at: Position) -> Option<u64> {
        let object = self.content.quest_objects.get(index)?;
        let id = object.id?;
        let crc = lowercase_crc32(&object.gatherable);
        let instanced = self.content.instanced_gathers.of(id);
        let gatherable = self.content.gatherables.get(crc).cloned();
        let tag = self.tracked_tag(id, Some(crc));
        let lock = instanced
            .zip(gatherable)
            .and_then(|(slice, gatherable)| self.instanced_lock(slice, &gatherable, Some(crc), at));
        let server_ms = self.server_ms;
        let mut player = self.world.get_mut::<Player>(entity)?;
        if let Some(lock) = lock {
            lock.list_for(&mut player, gde_ref_of(id), tag, server_ms);
        }
        let whole = object_gathered_whole(id, &mut player.gathers, instanced.is_none());
        player.extra_members.push(whole);
        lock.map(|lock| lock.reset_ms.saturating_sub(server_ms))
    }

    /// Sets off the game event of each quest object whose detection sphere a player with quests
    /// came into, at `now_ms`: once per way in, and only while one of its quests counts that
    /// event, so a player already inside when its task starts sets it off then (Observed: retail
    /// `9401_befriendhorse`, `9401_Torch` complete at 133.69 s in the record where the player came
    /// within 9 m of a torch, with no interaction from the client). Coming in is a whole gather
    /// of the object ([`Self::quest_object_gathered`]), and an instanced object whose lock is in
    /// the player's loot tracker sets off nothing until its reset.
    ///
    /// Observed (finding D11, `local-data/scratch-work/d11-quest-objects/prox-states.tsv`): the
    /// tracker entries of 184 of 232 retail proximity gathers lie within 0.5 m of their
    /// variant's `DetectionVolumeSphereRadius` around the capital (3D; the 99C coffins of 10 m at
    /// 9.8 to 10.2 m, the `99a_MSQ_05_PopClot` clots of 0.75 m at 0.4 to 1.2 m); no instanced
    /// proximity object of the base slice (257 states in 49 ledgers) gathered twice in a ledger,
    /// while shared ones did on each way in (`99f_msq_er_fightliang`: the same objects about
    /// 20 s apart). Unknown: why 48 entries lie 1 to 4 m beyond the sphere (the
    /// `17_MSQ_GothicTorch` torches of 7 m at 9.6 to 11.4 m).
    pub(super) fn detect_quest_objects(&mut self, now_ms: u64) {
        let numbers: Vec<u64> = self.quests.keys().copied().collect();
        for number in numbers {
            let Some(entity) = self.player_by_number(number) else {
                continue;
            };
            let Some(position) = self
                .world
                .get::<Body>(entity)
                .map(|body| body.motion.position())
            else {
                continue;
            };
            let inside = self.content.quest_objects.detected_at(position);
            let entered: Vec<(usize, String)> = {
                let Some(mut player) = self.world.get_mut::<Player>(entity) else {
                    continue;
                };
                player.detected.retain(|index| inside.contains(index));
                let quests = self.quests.get(&number);
                inside
                    .iter()
                    .filter(|index| !player.detected.contains(index))
                    .filter_map(|index| {
                        let object = self.content.quest_objects.get(*index)?;
                        let locked = object
                            .id
                            .is_some_and(|id| player.loot.opened(&gde_ref_of(id)));
                        let event = (!locked).then_some(&object.event)?;
                        quests
                            .is_some_and(|quests| {
                                quests
                                    .iter()
                                    .any(|quest| quest.run.counts_game_event(event))
                            })
                            .then(|| (*index, event.clone()))
                    })
                    .collect()
            };
            for (index, event) in entered {
                if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                    player.detected.insert(index);
                }
                self.quest_object_gathered(entity, index, position);
                tracing::info!(player = number, object = index, %event, "quest object detected");
                self.grant_event(number, &event, ONE_DETECTION, now_ms);
                self.quest_event(number, Event::GameEvent(&event), now_ms);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_content::achievements::AchievementIndexes;
    use nw_content::rewards::{Reward, Rewards};
    use nw_messages::client_input::LORE_KIND;

    use nw_content::gathering::{InstancedGathers, InstancedObject};
    use nw_content::interact_tags::{InteractTags, VariantTag};
    use nw_messages::LOOT_QUEST_OBJECT;
    use nw_world::capitals::capital_id;

    use super::*;
    use crate::placed_points::PlacedPoints;
    use crate::player_content::PlayerContent;
    use crate::world::gathering::tests::with_resets;
    use crate::world::level_flow::AchievementRules;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, world_with_spawns};

    const PLAYER: u64 = 4;

    #[test]
    fn a_gain_takes_its_size_again_from_the_rested_pool_while_it_lasts() {
        use nw_content::progression::Progression;

        use crate::world::rested::RestedPool;

        /// Retail `XPToLevel` of levels 0 to 5.
        const STARTS: [i64; 6] = [0, 150, 480, 1_080, 1_970, 3_070];
        /// Long enough away for the full 1.5 bars: 200 hours.
        const LONG_AWAY_NS: u64 = 720_000_000_000_000;
        /// Level 2 at its start; its full pool is half of 600 + 445.
        const LEVEL_TWO: u32 = 480;
        const FULL_POOL: u32 = 522;
        /// A gain of these points, twice: the second one finds less than its size left.
        const POINTS: u32 = 150;
        let mut world = world_with_spawns(Vec::new());
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let entity = world.player_by_number(PLAYER).expect("player");
        let levels = Progression::build((0..).zip(STARTS), [], []);
        let xp = world.content.gameplay.xp.clone();
        let base = xp.character_gain(POINTS, 0.0);
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        player.experience = LEVEL_TWO;
        player.rested = RestedPool::at_join(&levels, LEVEL_TWO, 0, LONG_AWAY_NS);
        assert_eq!(player.rested.experience(), FULL_POOL);
        gain_experience(&mut player, POINTS, &xp);
        assert_eq!(player.experience, LEVEL_TWO + base + base, "doubled");
        let left = FULL_POOL.saturating_sub(base);
        assert_eq!(player.rested.experience(), left);
        let before = player.experience;
        gain_experience(&mut player, POINTS, &xp);
        assert_eq!(
            player.experience,
            before + base + base.min(left),
            "the rest of the pool"
        );
    }

    /// A journal lore note of build 25810808 and its game event (2 experience points).
    const TITLE: &str = "@Weavers_Title_01_01";
    const ACHIEVEMENT: &str = "Lore_unlock_Weavers_Title_01_01";
    const READ_EVENT: &str = "Read_LoreItem";
    const READ_EXPERIENCE: u32 = 2;
    const ACHIEVEMENT_BIT: u32 = 774;
    const LORE: u32 = 0x1234;

    /// The `m_aoiRadius` of `master_quest_instanced` and its proximity and damageable forms, m.
    const QUEST_AOI_RADIUS_M: f32 = 100.0;

    /// The capital `guid` as an instanced quest object (`m_instancedLootType` 4, radius
    /// [`QUEST_AOI_RADIUS_M`]).
    fn quest_instanced(guid: &str) -> InstancedGathers {
        InstancedGathers::build(
            [(
                guid,
                InstancedObject {
                    loot_type: LOOT_QUEST_OBJECT,
                    aoi_radius_m: Some(QUEST_AOI_RADIUS_M),
                },
            )],
            [],
        )
    }

    #[test]
    fn one_hit_breaks_a_damageable_tablet_for_its_quest_drop() {
        use nw_content::gathering::{DetectionSpheres, Gatherable, Gatherables};
        use nw_world::records::{Capital, GatherablePoint};

        use crate::objectives::{TaskBook, TaskRow, counted_game_events, quest_drop_sources};
        use crate::player_content::{ObjectiveRow, QuestBook};
        use crate::world::gear;
        // Build 25810808 `12A_MSQ_08_DestroyRune_DestroyRune`, cut to its tablet step.
        const QUEST: &str = "12A_MSQ_08_DestroyRune";
        const ROOT: &str = "BossContainer";
        const STEP: &str = "DestroyRune";
        const TABLET: &str = "MB_MSQ2_DestroyRune_Tablet";
        const TABLET_EVENT: &str = "MB_MSQ2_DestroyRune_Tablet_interact";
        const SHARDS: &str = "12A_MSQ_08_RuneShards";
        const TABLET_GUID: &str = "c38829ed-596e-cc79-0473-a96ac4ccb5a5";
        const TABLET_TAG: &str = "IO_MB_MSQ2_DestroyRune_Tablet";
        const RESPAWN_MS: u64 = 200_000;
        let rows = vec![
            TaskRow {
                task_id: ROOT.to_owned(),
                kind: "SimpleTaskContainer".to_owned(),
                sub_tasks: vec![STEP.to_owned()],
                ..TaskRow::default()
            },
            TaskRow {
                task_id: STEP.to_owned(),
                kind: "TaskHaveAndReturnItems".to_owned(),
                item_name: Some(SHARDS.to_owned()),
                item_drop_vc: Some(TABLET.to_owned()),
                item_drop_probability: Some(100),
                target_qty: Some(1),
                ..TaskRow::default()
            },
        ];
        let gatherables = Gatherables::build([Gatherable {
            id: TABLET.to_owned(),
            gather_ms: 3_000,
            respawn_ms: (RESPAWN_MS, RESPAWN_MS),
            loot_table: None,
            loot_container: true,
            tradeskill: Some("None".to_owned()),
            tool_required: false,
            by_hand: true,
            game_event: Some(TABLET_EVENT.to_owned()),
        }]);
        let mut world = world_with_spawns(Vec::new());
        // The kit and the quest come before the admit: the bag is made from the kit.
        world.content = Arc::new(PlayerContent {
            kits: gear::tests::kits_with(&[(SHARDS, "Resource+Quest")]),
            quests: QuestBook::build(
                [ObjectiveRow {
                    id: QUEST.to_owned(),
                    root: ROOT.to_owned(),
                    ..ObjectiveRow::default()
                }],
                TaskBook::new(rows.clone()),
            ),
            gatherables: gatherables.clone(),
            ..(*world.content).clone()
        });
        let mut admit = admit_at(PLAYER, 0.0, true);
        admit.backstory = Some(gear::tests::BACKSTORY.to_owned());
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        let entity = world.player_by_number(PLAYER).expect("player");
        let at = world
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position())
            .expect("body");
        let point = GatherablePoint {
            gatherable_id: lowercase_crc32(TABLET),
            position: [at.x, at.y, at.z],
            ..GatherablePoint::default()
        };
        let capital = Capital {
            id: TABLET_GUID.to_owned(),
            slice_name: "Master_Quest_Damageable_Instanced".to_owned(),
            position: [at.x, at.y, at.z],
            ..Capital::default()
        };
        let quest_objects = QuestObjects::with_drops(
            &[point],
            &gatherables,
            &DetectionSpheres::default(),
            &counted_game_events(&rows),
            &quest_drop_sources(&rows),
            &[capital],
        );
        let mut content = PlayerContent {
            quest_objects,
            instanced_gathers: quest_instanced(TABLET_GUID),
            interact_tags: InteractTags::build(&HashMap::new(), [], []).with_gatherables([(
                lowercase_crc32(TABLET),
                VariantTag {
                    tag: TABLET_TAG.to_owned(),
                    only_on_quest: true,
                },
            )]),
            ..(*world.content).clone()
        };
        with_resets(&mut content);
        world.content = Arc::new(content);
        world.start_quest(PLAYER, QUEST, NOW).expect("accepted");
        world.strike_quest_objects(entity, Strike::Swing, SWING_REACH_M, NOW);
        let tablet = gde_ref_of(capital_id(TABLET_GUID).expect("guid"));
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        let entry = player.loot.listed(&tablet).expect("the break's lock");
        assert_eq!(
            (entry.kind, entry.interact_tag),
            (LOOT_QUEST_OBJECT, lowercase_crc32(TABLET_TAG)),
            "retail 133.96 s: type 4, the variant's tag"
        );
        assert_eq!(
            entry.reset_ns - entry.looted_ns,
            RESPAWN_MS * nw_messages::NS_PER_MS,
            "its respawn time"
        );
        let far = Position {
            x: at.x + QUEST_AOI_RADIUS_M + 1.0,
            ..at
        };
        assert!(player.loot.due(far, SERVER_TIME + 1, None, 2).is_some());
        assert!(
            player.loot.listed(&tablet).is_none() && player.loot.opened(&tablet),
            "beyond the slice's 100 m: off the list, still locked"
        );
        let shards: u32 = world
            .world
            .get::<Player>(entity)
            .and_then(|player| player.gear.as_ref())
            .map(gear::Gear::carried)
            .unwrap_or_default()
            .iter()
            .filter(|stack| stack.item_id == SHARDS)
            .map(|stack| stack.quantity)
            .sum();
        assert_eq!(shards, 1, "one swing breaks it and gives its drop");
        assert!(
            world
                .quests_of_player(PLAYER)
                .first()
                .is_some_and(|quest| quest.run.is_done(STEP)),
            "the step is done"
        );
    }

    /// Retail `99C_MSQ2_ShakingCoffinB` (build 25810808): its gatherable (respawn 200 to 300 s,
    /// its game event), its variant's tag and sphere, its `Master_Quest_Proximity_Instanced`
    /// capital.
    const COFFIN: &str = "99C_MSQ2_ShakingCoffinB";
    const COFFIN_EVENT: &str = "IO_99C_MSQ2_ShakingCoffinB_interact";
    const COFFIN_TAG: &str = "IO_99C_MSQ2_ShakingCoffinB";
    const COFFIN_GUID: &str = "f4cea255-7922-4a83-3fda-eb9ca236c82a";
    const COFFIN_SPHERE_M: f32 = 10.0;
    const COFFIN_RESPAWN_MS: (u64, u64) = (200_000, 300_000);
    const COFFIN_QUEST: &str = "q_coffin";
    const COFFIN_TASK: &str = "t_coffin";
    /// The coffin events the test quest waits for: two, so a second way in can count.
    const COFFIN_EVENTS: i64 = 2;
    /// The coffin stands this far east of the player's admit place.
    const COFFIN_EAST_M: f32 = 30.0;
    /// 9.8 m from the coffin, as retail's entry of `99c_msq_01_periville` 302.52 s.
    const INSIDE_M: f32 = 9.8;

    /// A world whose player is on a quest that waits for the coffin's event, the coffin
    /// [`COFFIN_EAST_M`] east of it, its capital `instanced` or shared; the player's entity and
    /// the place inside the coffin's sphere.
    fn with_coffin(instanced: bool) -> (SimWorld, Entity, Position) {
        use nw_content::gathering::{DetectionSpheres, Gatherable, Gatherables};
        use nw_world::records::{Capital, GatherablePoint};

        use crate::objectives::{TaskBook, TaskRow, counted_game_events};
        use crate::player_content::{ObjectiveRow, QuestBook};
        let coffin_at = Position {
            x: COFFIN_EAST_M,
            y: 10.0,
            z: 50.0,
        };
        let rows = vec![
            TaskRow {
                task_id: "t_root".to_owned(),
                kind: "ConsecutiveTaskContainer".to_owned(),
                sub_tasks: vec![COFFIN_TASK.to_owned()],
                ..TaskRow::default()
            },
            TaskRow {
                task_id: COFFIN_TASK.to_owned(),
                kind: "TaskGameEvent".to_owned(),
                kill_enemy_type: Some(COFFIN_EVENT.to_owned()),
                target_qty: Some(COFFIN_EVENTS),
                ..TaskRow::default()
            },
        ];
        let gatherables = Gatherables::build([Gatherable {
            id: COFFIN.to_owned(),
            gather_ms: 1_000,
            respawn_ms: COFFIN_RESPAWN_MS,
            loot_table: None,
            loot_container: true,
            tradeskill: Some("None".to_owned()),
            tool_required: false,
            by_hand: true,
            game_event: Some(COFFIN_EVENT.to_owned()),
        }]);
        let quest_objects = QuestObjects::build(
            &[GatherablePoint {
                gatherable_id: lowercase_crc32(COFFIN),
                position: [coffin_at.x, coffin_at.y, coffin_at.z],
                ..GatherablePoint::default()
            }],
            &gatherables,
            &DetectionSpheres::build([(COFFIN.to_owned(), COFFIN_SPHERE_M)]),
            &counted_game_events(&rows),
            &[Capital {
                id: COFFIN_GUID.to_owned(),
                slice_name: "Master_Quest_Proximity_Instanced".to_owned(),
                variant_name: Some(COFFIN.to_owned()),
                position: [coffin_at.x, coffin_at.y, coffin_at.z],
                ..Capital::default()
            }],
        );
        let mut world = world_with_spawns(Vec::new());
        let mut content = PlayerContent {
            quests: QuestBook::build(
                [ObjectiveRow {
                    id: COFFIN_QUEST.to_owned(),
                    root: "t_root".to_owned(),
                    ..ObjectiveRow::default()
                }],
                TaskBook::new(rows),
            ),
            gatherables,
            quest_objects,
            interact_tags: InteractTags::build(
                &HashMap::from([(
                    COFFIN.to_lowercase(),
                    VariantTag {
                        tag: COFFIN_TAG.to_owned(),
                        only_on_quest: true,
                    },
                )]),
                [(COFFIN_GUID, Some(COFFIN), &[][..])],
                [],
            ),
            instanced_gathers: if instanced {
                quest_instanced(COFFIN_GUID)
            } else {
                InstancedGathers::default()
            },
            ..(*world.content).clone()
        };
        with_resets(&mut content);
        world.content = Arc::new(content);
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        world
            .start_quest(PLAYER, COFFIN_QUEST, NOW)
            .expect("accepted");
        let entity = world.player_by_number(PLAYER).expect("player");
        let inside = Position {
            x: coffin_at.x - INSIDE_M,
            ..coffin_at
        };
        (world, entity, inside)
    }

    /// Puts player `entity` at `to` and runs the detection at `now_ms`.
    fn detect_at(world: &mut SimWorld, entity: Entity, to: Position, now_ms: u64) {
        world.place_player(entity, to, None, crate::world::Placement::Stand, now_ms);
        world.detect_quest_objects(now_ms);
    }

    /// The `(m_gatherRefCount, m_syncGatherRefCount)` of the gather states of `entity` that name
    /// the object of facet entity `facet`.
    fn whole_gathers_of(world: &SimWorld, entity: Entity, facet: u64) -> Vec<(u32, u32)> {
        world
            .world
            .get::<Player>(entity)
            .expect("player")
            .extra_members
            .iter()
            .filter_map(|member| match &member.state {
                MemberState::Gathering(state) if state.gatherable == Some(facet) => {
                    Some((state.gather_ref_count?, state.sync_ref_count?))
                }
                _ => None,
            })
            .collect()
    }

    /// Finding D11 (retail `99c_msq_01_periville` 336.59 s and 264 other proximity gathers of
    /// `Master_Quest_Proximity_Instanced`): coming into the sphere sends, in one record, the
    /// whole gather state (+2, +2) and a type 4 entry with the variant's tag at the player's
    /// place, locked for a roll of the respawn time and ranged by the slice's 100 m; the object
    /// sets off nothing more while locked (no retail instanced proximity object gathered twice
    /// in a ledger).
    #[test]
    fn coming_near_an_instanced_proximity_object_locks_it_with_a_ranged_entry() {
        let (mut world, entity, inside) = with_coffin(true);
        let coffin = capital_id(COFFIN_GUID).expect("guid");
        let away = Position {
            x: inside.x - COFFIN_SPHERE_M,
            ..inside
        };
        detect_at(&mut world, entity, away, NOW);
        assert!(
            whole_gathers_of(&world, entity, coffin).is_empty(),
            "outside"
        );
        detect_at(&mut world, entity, inside, NOW + 1);
        assert_eq!(whole_gathers_of(&world, entity, coffin), [(2, 2)]);
        let player = world.world.get::<Player>(entity).expect("player");
        let stood = world
            .world
            .get::<Body>(entity)
            .map(|body| body.motion.position())
            .expect("body");
        let entry = player.loot.listed(&gde_ref_of(coffin)).expect("its lock");
        assert_eq!(
            (entry.kind, entry.interact_tag),
            (LOOT_QUEST_OBJECT, lowercase_crc32(COFFIN_TAG))
        );
        assert_eq!(
            entry.position,
            [stood.x, stood.y, stood.z],
            "where the player stood"
        );
        let span_ms = (entry.reset_ns - entry.looted_ns) / nw_messages::NS_PER_MS;
        assert!(
            (COFFIN_RESPAWN_MS.0..=COFFIN_RESPAWN_MS.1).contains(&span_ms),
            "a roll of its respawn time: {span_ms}"
        );
        detect_at(&mut world, entity, away, NOW + 2);
        detect_at(&mut world, entity, inside, NOW + 3);
        assert_eq!(
            whole_gathers_of(&world, entity, coffin).len(),
            1,
            "locked: no second gather on the way back in"
        );
        assert!(
            !world.quests_of_player(PLAYER)[0].run.is_done(COFFIN_TASK),
            "nor a second event"
        );
        let mut player = world.world.get_mut::<Player>(entity).expect("player");
        let far = Position {
            x: inside.x + QUEST_AOI_RADIUS_M + COFFIN_SPHERE_M,
            ..inside
        };
        assert!(player.loot.due(far, SERVER_TIME + 1, None, 2).is_some());
        assert!(
            player.loot.listed(&gde_ref_of(coffin)).is_none(),
            "beyond 100 m: off the list"
        );
    }

    /// Finding D10 (402 retail whole gathers of shared objects, `Master_Quest_Proximity`
    /// `17_ZQBS1_SpiderCave_Footprints1` of `quest_16004` 93.15 s among them): a shared
    /// object's whole gather steps the ref counter by 3, leaves no tracker entry and comes
    /// again on each way in (retail `99f_msq_er_fightliang`: the same objects about 20 s apart).
    #[test]
    fn coming_near_a_shared_proximity_object_sends_a_whole_gather_of_three_and_no_entry() {
        let (mut world, entity, inside) = with_coffin(false);
        let coffin = capital_id(COFFIN_GUID).expect("guid");
        let away = Position {
            x: inside.x - COFFIN_SPHERE_M,
            ..inside
        };
        detect_at(&mut world, entity, inside, NOW);
        assert_eq!(whole_gathers_of(&world, entity, coffin), [(3, 2)]);
        let player = world.world.get::<Player>(entity).expect("player");
        assert!(!player.loot.opened(&gde_ref_of(coffin)), "no lock");
        detect_at(&mut world, entity, away, NOW + 1);
        detect_at(&mut world, entity, inside, NOW + 2);
        assert_eq!(whole_gathers_of(&world, entity, coffin), [(3, 2), (6, 4)]);
        assert!(
            world.quests_of_player(PLAYER)[0].run.is_done(COFFIN_TASK),
            "both ways in counted"
        );
    }

    #[test]
    fn a_first_lore_read_keeps_the_note_and_fires_its_game_event_once() {
        let mut world = world_with_spawns(Vec::new());
        let indexes = AchievementIndexes::build([(ACHIEVEMENT.to_owned(), ACHIEVEMENT_BIT)])
            .with_game_events([(ACHIEVEMENT.to_owned(), READ_EVENT.to_owned())]);
        world.achievement_rules = AchievementRules::of(&indexes, &[]);
        let note_at = Position {
            x: 0.0,
            y: 10.0,
            z: 50.0,
        };
        world.content = Arc::new(PlayerContent {
            lore_by_title: [(lore_title_key(TITLE), LORE)].into_iter().collect(),
            lore_points: PlacedPoints::of([(LORE, note_at)]),
            rewards: Rewards::build([(
                READ_EVENT.to_owned(),
                Reward {
                    experience: READ_EXPERIENCE,
                    ..Reward::default()
                },
            )]),
            achievements: indexes,
            ..PlayerContent::default()
        });
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let entity = world.player_by_number(PLAYER).expect("player");
        let request = InteractionRequest {
            target: [0; nw_messages::wire::RAW_ID_LEN],
            target_entity: 0,
            interaction: 1,
            kind: LORE_KIND,
            gatherable: None,
        };
        let experience = |world: &SimWorld| {
            world
                .world
                .get::<Player>(entity)
                .expect("player")
                .experience
        };
        let before = experience(&world);
        world.read_lore(entity, PLAYER, request, TITLE.as_bytes(), NOW);
        let player = world.world.get::<Player>(entity).expect("player");
        assert!(
            player
                .flow
                .has_achievement(&world.achievement_rules, ACHIEVEMENT),
            "kept"
        );
        let after_first = experience(&world);
        assert!(after_first > before, "its game event's experience");
        world.read_lore(entity, PLAYER, request, TITLE.as_bytes(), NOW);
        assert_eq!(
            experience(&world),
            after_first,
            "a note read again gives nothing more"
        );
    }
}
