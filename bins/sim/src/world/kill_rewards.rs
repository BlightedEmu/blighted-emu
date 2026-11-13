// SPDX-License-Identifier: AGPL-3.0-or-later
//! The experience of a mob kill (progression note, section 7; [`nw_content::kill_rewards`]):
//! the killer's character experience, the mastery of the weapon in its hand and, by a share,
//! the mastery of its other worn weapon, each times the remote config's multiplier of its
//! progression and the share the killer's held effects add to it (`Token_Weapon_1`; Observed,
//! `Playthrough_Part_02` 1505.5 s with the token held: a kill of raw 15.0000006 (f32 25 x 0.6)
//! gave the held sword 64 = 16 x 2 x 2 and the other weapon 12 = ceil(2.25) x 2 x 2), and the
//! kill's meta achievement counters and season stats. The raw experience takes the level gap
//! to the killer's shown (one-based) level; the character gains the raw rounded down, at least
//! one point, times the multiplier and its territory experience share, rounded down once
//! (Observed over 2,213 retail kills, findings `2026-11-13-kill-xp-rounding.md`; the same
//! kill gave the character +33 = floor(15 x 2 x 1.122853) at 5 territory experience points).
//!
//! The kill also fires its game event ([`nw_content::kill_rewards`]): its coins, the standing of
//! the territory the killer is in, its azoth, umbral shards and item, and its entry in the
//! killer's game event log ([`super::game_event_log`]).
//!
//! In a catacombs run and an expedition every player of the instance gets the kill's game
//! event, with the share of [`kill_share`] in its entry (Observed, 62 group, expedition and
//! catacomb ledgers: Amrine expedition, 73 mob deaths and 73 entries with five players, each
//! share 1.0; `CATACOMBS5` 39 entries of share 0.7 with two players, then 28 of 0.6 after the
//! third joined at 902 s; a solo run 1.0). The share does not scale what the event gives
//! (Observed: the fodder kills gave 4 souls at 0.6 and at 0.7). Elsewhere only the killer gets
//! it. Character and weapon experience stay the killer's (Inferred: no ledger shows a group
//! member below the level cap; the Amrine player was at the cap).
//!
//! In the open world a kill credits every player of this sim that hit the mob in the credit time
//! for the credit's share of its health and stands in the credit range ([`KillCredits`]; the
//! killer always): each gets the character and weapon experience and the game event, times the
//! credit modifier of their count ([`nw_content::kill_rewards::event_credit`]; Observed, two
//! players 0.7 of the raw experience, 0.8 of the coins and 0.7 of the standing, findings
//! `2026-11-13-kill-xp-rounding.md`). Inferred: the meta achievement and season counters stay the
//! killer's, and an instance kill keeps the run share above, not the credit modifier.
//!
//! A killer on another sim gets nothing here, as it gets no loot (its sim does not hear of the
//! kill). A tutorial mob's character experience comes from the tutorial's own order (its
//! `KillExperience`); its masteries come from here.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_ecs::entity::Entity;
use nw_content::instances::InstanceKind;
use nw_content::kill_rewards::{
    CREDIT_HEALTH_SHARE, CREDIT_RANGE_M, CREDIT_TIME_LIMIT_MS, CreditModifier, KillGameEvent,
    character_points, event_credit, held_weapon_points, other_weapon_points,
};
use nw_content::rewards::Reward;
use nw_messages::paperdoll::slot;

use super::game_event_log::{GrantedEvent, UMBRAL_CURRENCY, WHOLE_SHARE};
use super::gear::Gear;
use super::meta_achievements::MetaEvent;
use super::quest_objects::{gain_experience, grant};
use super::seasons::SeasonEvent;
use super::status_effects::GainKind;
use super::tradeskill::{AZOTH_CURRENCY, ONE_FIRING, progression_gain};
use super::{Body, Player, SimWorld};
use crate::player_content::PlayerContent;

/// One firing of a gain.
const ONCE: u32 = 1;
/// The share of a kill's game event of each player of a catacombs run, by the run's players
/// (one, two, three; module text, Observed).
const CATACOMB_KILL_SHARES: [f32; 3] = [1.0, 0.7, 0.6];

/// One player's hits on one mob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CreditHits {
    /// The player's entity number.
    player: u64,
    /// The health its hits took since its credit began.
    damage: u64,
    /// When it last hit (sim ms).
    last_ms: u64,
}

/// The players' hits on each mob that may credit them for its kill (module text), by the mob's
/// entity number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct KillCredits {
    hits: HashMap<u64, Vec<CreditHits>>,
}

impl KillCredits {
    /// Player `player` hit mob `mob` for `amount` at `now_ms`; a player whose last hit is older
    /// than the credit time starts over (Inferred).
    fn note(&mut self, mob: u64, player: u64, amount: u32, now_ms: u64) {
        let hits = self.hits.entry(mob).or_default();
        match hits.iter_mut().find(|hits| hits.player == player) {
            Some(hits) => {
                if now_ms.saturating_sub(hits.last_ms) > CREDIT_TIME_LIMIT_MS {
                    hits.damage = 0;
                }
                hits.damage = hits.damage.saturating_add(u64::from(amount));
                hits.last_ms = now_ms;
            }
            None => hits.push(CreditHits {
                player,
                damage: u64::from(amount),
                last_ms: now_ms,
            }),
        }
    }

    /// The players whose hits on mob `mob` of `max_health` credit them at `now_ms`, in the
    /// order they first hit; the mob's hits go, and so do every mob's hits older than the
    /// credit time.
    fn take(&mut self, mob: u64, max_health: u32, now_ms: u64) -> Vec<u64> {
        let fresh = |hits: &CreditHits| now_ms.saturating_sub(hits.last_ms) <= CREDIT_TIME_LIMIT_MS;
        let needed = f64::from(max_health) * f64::from(CREDIT_HEALTH_SHARE);
        let credited = self
            .hits
            .remove(&mob)
            .unwrap_or_default()
            .into_iter()
            .filter(|hits| fresh(hits) && damage_amount(hits.damage) >= needed)
            .map(|hits| hits.player)
            .collect();
        self.hits.retain(|_, hits| {
            hits.retain(|hits| fresh(hits));
            !hits.is_empty()
        });
        credited
    }
}

/// A damage total as a float (far below 2^53).
#[allow(
    clippy::cast_precision_loss,
    reason = "a damage total is far below 2^53"
)]
fn damage_amount(damage: u64) -> f64 {
    damage as f64
}

/// The share of a kill's game event in an instance of `kind` with `players` players (module
/// text): a catacombs run's share of its players (a run holds three at most; Inferred: more
/// keep the share of three), a whole share elsewhere.
fn kill_share(kind: Option<InstanceKind>, players: usize) -> f32 {
    match kind {
        Some(InstanceKind::Catacombs) => CATACOMB_KILL_SHARES
            .get(players.saturating_sub(1))
            .or(CATACOMB_KILL_SHARES.last())
            .copied()
            .unwrap_or(WHOLE_SHARE),
        _ => WHOLE_SHARE,
    }
}

impl SimWorld {
    /// Mob `mob` died by a hit of player `killer` (entity number): an Outpost Rush match hears
    /// it ([`super::outpost_rush_boss`]), an expedition scores a named kill
    /// ([`SimWorld::expedition_mob_killed`]), the killer gains the kill's character and weapon
    /// mastery experience and its game event, and its meta achievements count the kill.
    pub(super) fn reward_kill(&mut self, mob: u64, killer: u64) {
        self.outpost_rush_mob_killed(mob, killer);
        self.expedition_mob_killed(mob);
        let Some(entity) = self.player_by_number(killer) else {
            return;
        };
        let Some(vitals) = self.mobs.vitals_of(mob) else {
            return;
        };
        let content = Arc::clone(&self.content);
        let mob_level = self.mobs.level(mob).unwrap_or_default();
        let kind = self.instance_kind();
        let credited = match kind {
            Some(InstanceKind::Catacombs | InstanceKind::Expedition) => {
                self.kill_credits.hits.remove(&mob);
                vec![(entity, killer)]
            }
            _ => self.credited_players(mob, (entity, killer)),
        };
        let credit = event_credit(credited.len());
        for (taker, _) in &credited {
            self.give_kill_experience(*taker, mob, vitals, mob_level, credit);
        }
        if let Some(fired) = content.kill_rewards.kill_event(vitals, mob_level) {
            if matches!(
                kind,
                Some(InstanceKind::Catacombs | InstanceKind::Expedition)
            ) {
                let takers = self.kill_event_takers(kind, (entity, killer));
                let share = kill_share(kind, takers.len());
                for (taker, number) in takers {
                    self.give_kill_event(taker, number, &fired, share);
                }
            } else {
                let fired = credit.event(&fired);
                for (taker, number) in credited {
                    self.give_kill_event(taker, number, &fired, WHOLE_SHARE);
                }
            }
        }
        let (vitals_id, categories) = content
            .kill_types
            .of(vitals)
            .map(|(id, categories)| (id, categories.to_vec()))
            .unwrap_or_default();
        self.meta_event(
            entity,
            MetaEvent::Kill {
                categories: &categories,
            },
        );
        let weapon = self
            .world
            .get::<Player>(entity)
            .and_then(|player| player.gear.as_ref()?.main_weapon())
            .and_then(|(item, _)| content.kill_rewards.weapon_mastery(item));
        self.season_event(
            entity,
            SeasonEvent::Kill {
                vitals_id,
                categories: &categories,
                level: mob_level,
                weapon,
            },
        );
    }
}

impl SimWorld {
    /// Player `attacker` (entity number) hit mob `mob` for `amount`: the hit counts for its
    /// credit of the mob's kill (module text). A hit of a mob or of a player of another sim
    /// counts for nothing.
    pub(super) fn note_mob_damage(&mut self, mob: u64, attacker: u64, amount: u32) {
        if amount == 0 || self.player_by_number(attacker).is_none() {
            return;
        }
        let now_ms = self.last_now_ms;
        self.kill_credits.note(mob, attacker, amount, now_ms);
    }

    /// The players a kill of mob `mob` by `killer` (entity, number) credits (module text): the
    /// killer, then each other player of this sim whose hits credit it and that stands in the
    /// credit range of the mob.
    fn credited_players(&mut self, mob: u64, killer: (Entity, u64)) -> Vec<(Entity, u64)> {
        let (max_health, at) = self
            .mobs
            .get(mob)
            .map(|mob| (mob.health.max(), mob.position))
            .unzip();
        let hitters = self
            .kill_credits
            .take(mob, max_health.unwrap_or_default(), self.last_now_ms);
        let mut credited = vec![killer];
        for number in hitters {
            if number == killer.1 {
                continue;
            }
            let Some(entity) = self.player_by_number(number) else {
                continue;
            };
            let in_range = self.world.get::<Body>(entity).is_some_and(|body| {
                let position = body.motion.position();
                at.is_some_and(|at| {
                    let (dx, dy, dz) = (position.x - at.x, position.y - at.y, position.z - at.z);
                    (dx * dx + dy * dy + dz * dz).sqrt() <= CREDIT_RANGE_M
                })
            });
            if in_range {
                credited.push((entity, number));
            }
        }
        credited
    }

    /// Player `entity` gets the character and weapon experience of a kill of mob `mob` (its
    /// vitals type `vitals`, level `mob_level`) under the credit modifier `credit`: a tutorial
    /// mob gives no character experience here (module text).
    fn give_kill_experience(
        &mut self,
        entity: Entity,
        mob: u64,
        vitals: u32,
        mob_level: u32,
        credit: CreditModifier,
    ) {
        let content = Arc::clone(&self.content);
        let tutorial_mob = self.mobs.owner(mob).is_some();
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let player = &mut *player;
        let player_level = content.progression.shown_level_of(player.experience);
        let raw = content
            .kill_experience
            .of(mob_level)
            .and_then(|base| {
                content
                    .kill_rewards
                    .raw_experience(vitals, base, mob_level, player_level)
            })
            .map(|raw| credit.raw_experience(raw));
        if let Some(raw) = raw
            && !tutorial_mob
        {
            gain_experience(player, character_points(raw), &content.gameplay.xp);
        }
        let raw = raw.unwrap_or_default();
        for (mastery_id, points) in mastery_gains(&content, player.gear.as_ref(), raw) {
            let Some(mastery) = content.progression.mastery(&mastery_id) else {
                continue;
            };
            let multiplier = content.gameplay.xp.progression(&mastery_id)
                * player.status_effects.gain_scale(
                    &content.status_effects,
                    &mastery_id,
                    GainKind::Progression,
                );
            player.masteries.gain(
                mastery,
                progression_gain(f64::from(points), ONCE, multiplier),
            );
        }
    }

    /// The players that get the game event of a kill by `killer` (entity, number) on a sim of
    /// the instance kind `kind` (module text): every player of a catacombs run or an
    /// expedition, else the killer alone.
    fn kill_event_takers(
        &mut self,
        kind: Option<InstanceKind>,
        killer: (Entity, u64),
    ) -> Vec<(Entity, u64)> {
        if !matches!(
            kind,
            Some(InstanceKind::Catacombs | InstanceKind::Expedition)
        ) {
            return vec![killer];
        }
        let mut query = self.world.query::<(Entity, &Player)>();
        query
            .iter(&self.world)
            .map(|(entity, player)| (entity, player.number))
            .collect()
    }

    /// Player `entity` (`number`) gets the game event `fired` of a kill with the share `share`
    /// in its entry: its coins, the standing of the territory it is in, its azoth and umbral
    /// shards (each to its cap), its item, and its entry in the log.
    fn give_kill_event(&mut self, entity: Entity, number: u64, fired: &KillGameEvent, share: f32) {
        let content = Arc::clone(&self.content);
        let event = fired.event.as_str();
        if fired.coins > 0
            && let Some(mut player) = self.world.get_mut::<Player>(entity)
        {
            let wallet = Reward {
                coins: fired.coins,
                ..Reward::default()
            };
            grant(&mut player, &wallet, &content.gameplay);
        }
        if fired.standing > 0.0
            && let Some(territory) = self.current_territory(number)
        {
            let (kind, points) = (GainKind::Standing, fired.standing);
            self.gain_categorical(number, event, &territory, kind, points, ONE_FIRING);
        }
        let currencies = [
            (AZOTH_CURRENCY, GainKind::Azoth, fired.azoth),
            (UMBRAL_CURRENCY, GainKind::Progression, fired.umbral),
        ];
        for (id, kind, points) in currencies
            .into_iter()
            .filter(|(_, _, points)| *points > 0.0)
        {
            self.gain_categorical(number, event, id, kind, points, ONE_FIRING);
        }
        let items = fired
            .item
            .clone()
            .map(|item| self.give_loot_listed(number, vec![item], self.server_ms))
            .unwrap_or_default();
        let granted = GrantedEvent {
            share,
            coins: fired.coins,
            standing: fired.standing,
            azoth: fired.azoth,
            umbral: fired.umbral,
            items,
            ..GrantedEvent::of(event)
        };
        self.log_game_event(number, &granted);
    }
}

/// The weapon mastery points of a kill of raw experience `raw` by a player wearing `gear`
/// (before the multipliers): the weapon in the hand gains [`held_weapon_points`], each other
/// worn main-hand weapon of another mastery [`other_weapon_points`].
fn mastery_gains(content: &PlayerContent, gear: Option<&Gear>, raw: f32) -> Vec<(String, u32)> {
    let Some(gear) = gear else {
        return Vec::new();
    };
    if raw <= 0.0 {
        return Vec::new();
    }
    let rewards = &content.kill_rewards;
    let held = gear
        .main_weapon()
        .and_then(|(item, _)| rewards.weapon_mastery(item))
        .map(str::to_owned);
    let mut gains: Vec<(String, u32)> = held
        .iter()
        .map(|mastery| (mastery.clone(), held_weapon_points(raw)))
        .collect();
    for option in slot::MAIN_HAND_OPTION_1..=slot::MAIN_HAND_OPTION_3 {
        let Some(mastery) = gear
            .worn_item(option)
            .and_then(|item| rewards.weapon_mastery(item))
        else {
            continue;
        };
        if gains
            .iter()
            .any(|(known, _)| known.eq_ignore_ascii_case(mastery))
        {
            continue;
        }
        gains.push((mastery.to_owned(), other_weapon_points(raw)));
    }
    gains
}

#[cfg(test)]
mod tests {
    use nw_content::combat::{CombatData, CombatRows, MobType};
    use nw_content::kill_rewards::{
        CreatureMods, KILL_EVENT, KillEventRow, KillLevel, KillRewards, MobRow,
    };
    use nw_content::progression::Progression;
    use nw_content::rewards::KillExperience;
    use nw_content::status_effects::{
        EffectSourceRow, RewardMods, StatusEffectRules, StatusEffectSource,
    };
    use nw_formats::objectstream::lowercase_crc32;
    use nw_messages::{MemberState, ReplicatedList};
    use nw_world::records::VitalsSpawn;

    use super::*;
    use crate::mobs::mob_entity;
    use crate::world::gear::tests::{STAFF, SWORD, gear_with_staff};
    use crate::world::tests::{
        MANY, MAX_PLAYERS, NOW, SERVER_TIME, admit_at, settings, table, world_with_combat,
        world_with_spawns,
    };

    const PLAYER: u64 = 41;
    const SWORD_MASTERY: &str = "SwordAbilityTable";
    const STAFF_MASTERY: &str = "LifeMagicAbilityTable";
    const LEGION: &str = "Legion_Legionnaire_FTUE";
    /// Retail `RewardData` level 1 `KillExperience`.
    const LEVEL_ONE_KILL: i64 = 10;
    /// Retail `RewardModifierData` `Solo`.
    const SOLO: CreatureMods = CreatureMods {
        experience: 1.0,
        currency: 1.0,
        standing: 5.0,
        azoth: 1.0,
    };
    /// Retail `RewardData` level 1 `KillCurrency` and `KillAzoth`.
    const LEVEL_ONE: KillLevel = KillLevel {
        currency: 100.0,
        azoth: 5.0,
    };
    /// Retail `javelin.xp-multipliers` of the character and of each weapon mastery.
    const RETAIL_MULTIPLIER: f64 = 2.0;
    /// The weapon experience boost (build 25810808 `Token_Weapon_1`).
    const TOKEN: &str = "Token_Weapon_1";

    fn content() -> PlayerContent {
        PlayerContent {
            progression: Progression::build(
                [(0, 0), (1, 150)],
                [(0, 10), (1, 50), (2, 70)],
                [(SWORD_MASTERY, 3), (STAFF_MASTERY, 3)],
            ),
            kill_experience: KillExperience::build([(1, LEVEL_ONE_KILL)]),
            kill_rewards: KillRewards::build(
                [MobRow {
                    vitals_id: LEGION,
                    creature_type: Some("Solo"),
                    no_exp_no_coin: None,
                    kill_event_override: None,
                }],
                [],
                [("Solo", SOLO)],
                [],
                [(SWORD, SWORD_MASTERY), (STAFF, STAFF_MASTERY)],
                [(1, LEVEL_ONE)],
                [(
                    KILL_EVENT,
                    KillEventRow {
                        azoth: true,
                        ..KillEventRow::default()
                    },
                )],
            ),
            ..PlayerContent::default()
        }
    }

    #[test]
    fn the_held_weapon_gains_the_kill_and_the_other_a_share() {
        let content = content();
        let gear = gear_with_staff();
        let held = gear
            .main_weapon()
            .and_then(|(item, _)| content.kill_rewards.weapon_mastery(item))
            .expect("a weapon in the hand")
            .to_owned();
        let other = if held == SWORD_MASTERY {
            STAFF_MASTERY
        } else {
            SWORD_MASTERY
        };
        // Retail newchar 2026-09-18, first island kill: the held greatsword 10 and the worn
        // blunderbuss 2 before the multiplier 2.
        assert_eq!(
            mastery_gains(&content, Some(&gear), 10.0),
            [(held.clone(), 10), (other.to_owned(), 2)]
        );
        // A kill of raw 5.5 (a `Solo-` mob): 6 and 1.
        assert_eq!(
            mastery_gains(&content, Some(&gear), 5.5),
            [(held, 6), (other.to_owned(), 1)]
        );
        assert!(mastery_gains(&content, Some(&gear), 0.0).is_empty());
        assert!(mastery_gains(&content, None, 10.0).is_empty());
    }

    /// Retail shares of a kill's game event: a catacombs run's 1.0, 0.7 and 0.6 for one, two
    /// and three players (the share of three above); an expedition and the open world 1.0.
    #[test]
    fn the_kill_share_follows_the_run_players() {
        let catacombs = Some(InstanceKind::Catacombs);
        let shares: Vec<f32> = (1..=4)
            .map(|players| kill_share(catacombs, players))
            .collect();
        assert_eq!(shares, [1.0, 0.7, 0.6, 0.6]);
        assert_eq!(kill_share(Some(InstanceKind::Expedition), 5), WHOLE_SHARE);
        assert_eq!(kill_share(None, 1), WHOLE_SHARE);
    }

    /// Retail `CATACOMBS5`: with two players in the run, each gets the kill's entry with the
    /// share 0.7 and the whole coins; on the open world only the killer gets it.
    #[test]
    fn a_run_kill_gives_every_player_the_event_with_the_run_share() {
        use crate::world::catacombs::tests::{PLAYER as KILLER, admit_run_player, arrived_run};
        const OTHER: u64 = KILLER + 1;
        const COINS: u64 = 40;
        const TWO_PLAYER_SHARE: f32 = 0.7;
        let (mut world, _) = arrived_run();
        admit_run_player(&mut world, OTHER);
        let kind = world.instance_kind();
        assert_eq!(kind, Some(InstanceKind::Catacombs));
        let killer = world.player_by_number(KILLER).expect("killer");
        let takers = world.kill_event_takers(kind, (killer, KILLER));
        assert_eq!(takers.len(), 2, "both players of the run");
        let share = kill_share(kind, takers.len());
        let fired = KillGameEvent {
            event: KILL_EVENT.to_owned(),
            coins: COINS,
            ..KillGameEvent::default()
        };
        for (entity, number) in takers {
            world.give_kill_event(entity, number, &fired, share);
        }
        for number in [KILLER, OTHER] {
            let entity = world.player_by_number(number).expect("player");
            let mut log = world
                .world
                .get::<Player>(entity)
                .expect("player")
                .game_events
                .clone();
            let member = log.take_member(1).expect("the kill's entry");
            let MemberState::GameEvents(state) = member.state else {
                panic!("a game event member");
            };
            let Some(ReplicatedList::Full(list)) = state.game_events else {
                panic!("the first entry goes in full");
            };
            let entry = list.elements.last().expect("an entry");
            assert_eq!(entry.event, lowercase_crc32(KILL_EVENT));
            let coins = u32::try_from(COINS).expect("coins");
            assert_eq!((entry.amount_a, entry.value_a), (TWO_PLAYER_SHARE, coins));
        }
        assert_eq!(
            world.kill_event_takers(None, (killer, KILLER)),
            [(killer, KILLER)],
            "the open world's killer alone"
        );
    }

    #[test]
    fn a_kill_takes_the_shown_level_gap_the_least_points_and_the_territory_share() {
        use nw_content::character_points::{
            territory_experience_point_crc, territory_experience_share, territory_pool_crc,
        };
        /// A critter (`RewardModifierData` `Critter`: experience 0, a reward all the same).
        const CRITTER: &str = "Turkey";
        /// Retail territory 12 and 5 `TB_GainXP` points (`Playthrough_Part_02`).
        const TERRITORY: u16 = 12;
        const TERRITORY_POINTS: u16 = 5;
        /// Retail `LevelDisparityData`: -10 cuts kill experience by 40 %.
        const GAPS: [(i64, f64); 2] = [(-10, -0.4), (3, 0.0)];
        /// Level 10 (zero-based; shown 11) starts at 1,000 experience in this table.
        const LEVEL_TEN: u32 = 1_000;
        const LEVEL_STEP: i64 = 100;
        let mut world = world_with_spawns(
            [LEGION, CRITTER]
                .into_iter()
                .map(|vitals| VitalsSpawn {
                    vitals_id: lowercase_crc32(vitals),
                    level: 1,
                    position: [0.0, 10.0, 50.0],
                    ..VitalsSpawn::default()
                })
                .collect(),
        );
        let mut content = content();
        content.progression = Progression::build(
            (0..=10).map(|level| (level, level * LEVEL_STEP)),
            [(0, 10), (1, 50), (2, 70)],
            [(SWORD_MASTERY, 3), (STAFF_MASTERY, 3), ("12", 3)],
        );
        content.kill_rewards = KillRewards::build(
            [
                MobRow {
                    vitals_id: LEGION,
                    creature_type: Some("Solo"),
                    no_exp_no_coin: None,
                    kill_event_override: None,
                },
                MobRow {
                    vitals_id: CRITTER,
                    creature_type: Some("Critter"),
                    no_exp_no_coin: None,
                    kill_event_override: None,
                },
            ],
            [],
            [("Solo", SOLO), ("Critter", CreatureMods::default())],
            GAPS,
            [],
            [(1, LEVEL_ONE)],
            [],
        );
        content.gameplay.xp.character = RETAIL_MULTIPLIER;
        world.content = Arc::new(content);
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let entity = world.player_by_number(PLAYER).expect("player");
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .experience = LEVEL_TEN;
        let gained = |world: &mut SimWorld, mob: u64| {
            let before = world
                .world
                .get::<Player>(entity)
                .expect("player")
                .experience;
            world.reward_kill(mob, PLAYER);
            world
                .world
                .get::<Player>(entity)
                .expect("player")
                .experience
                - before
        };
        // Retail `Playthrough_Part_03` 5863.2 s: the gap is to the shown level 11, -10, so the
        // level 1 mob gives floor(10 x 0.6) = 6 points, x 2 (the zero-based level 10 would be
        // -9: no cut, 20).
        assert_eq!(gained(&mut world, mob_entity(0)), 12);
        // A critter gives the least point (retail `Turkey` +2).
        assert_eq!(gained(&mut world, mob_entity(1)), 2);
        // In territory 12 with 5 experience points: floor(6 x 2 x 1.122853) = 13.
        {
            let mut player = world.world.get_mut::<Player>(entity).expect("player");
            player.masteries = std::mem::take(&mut player.masteries).with_territory_points(&[(
                territory_pool_crc("12"),
                territory_experience_point_crc(),
                TERRITORY_POINTS,
            )]);
        }
        world.territory_changed(entity, TERRITORY, true);
        let share = world
            .world
            .get::<Player>(entity)
            .expect("player")
            .territory_experience_share;
        assert!((share - territory_experience_share(TERRITORY_POINTS)).abs() < f64::EPSILON);
        assert_eq!(gained(&mut world, mob_entity(0)), 13);
        assert_eq!(gained(&mut world, mob_entity(1)), 2);
    }

    #[test]
    fn a_kill_that_two_players_hit_gives_each_the_shared_credit() {
        const HELPER: u64 = PLAYER + 1;
        const FAR: u64 = PLAYER + 2;
        /// Out of the credit range of the mob at x 0.
        const FAR_X: f32 = 500.0;
        /// Raw 10 x 0.7 = 7 points, x 2; alone 10 x 2.
        const SHARED: u32 = 14;
        const ALONE: u32 = 20;
        /// 100 coins x 0.8.
        const SHARED_COINS: u64 = 80;
        /// The level of the spawn: the level of the content's kill rows.
        const LEGION_LEVEL: u32 = 1;
        // The credit reads the live mob's health and place: the combat rows give the spawn's
        // type and level (the test level's row), and the sim owns its cell.
        let mut source = crate::combat::test_support::test_source();
        let level = source
            .levels
            .first()
            .map(|(_, row)| *row)
            .expect("the test level");
        source.base_types.push((LEGION.to_owned(), MobType::PLAIN));
        source.levels.push((LEGION_LEVEL, level));
        let mut combat = crate::combat::tests::test_combat();
        combat.data = Arc::new(CombatData::build(source, &CombatRows::default()).expect("rows"));
        combat.spawns = vec![VitalsSpawn {
            vitals_id: lowercase_crc32(LEGION),
            level: LEGION_LEVEL,
            position: [0.0, 10.0, 50.0],
            ..VitalsSpawn::default()
        }]
        .into();
        let mut world = world_with_combat(settings(MANY, MAX_PLAYERS), combat);
        world.set_table(&table(&["sim-a"])).expect("table");
        let mut content = content();
        content.gameplay.xp.character = RETAIL_MULTIPLIER;
        world.content = Arc::new(content);
        for (number, x) in [(PLAYER, 0.0), (HELPER, 1.0), (FAR, FAR_X)] {
            world
                .admit(&admit_at(number, x, true), NOW, SERVER_TIME)
                .expect("admit");
        }
        let mob = mob_entity(0);
        let credit = world.mobs.get(mob).expect("mob").health.max().div_ceil(100);
        for number in [PLAYER, HELPER, FAR] {
            world.note_mob_damage(mob, number, credit);
        }
        let state = |world: &mut SimWorld, number: u64| {
            let entity = world.player_by_number(number).expect("player");
            let player = world.world.get::<Player>(entity).expect("player");
            (player.experience, player.coins)
        };
        let coins = state(&mut world, HELPER).1;
        world.reward_kill(mob, PLAYER);
        assert_eq!(state(&mut world, PLAYER).0, SHARED);
        assert_eq!(state(&mut world, HELPER), (SHARED, coins + SHARED_COINS));
        assert_eq!(state(&mut world, FAR).0, 0, "out of the credit range");
        // The kill took the hits: the next kill credits its killer alone.
        world.reward_kill(mob, PLAYER);
        assert_eq!(state(&mut world, PLAYER).0, SHARED + ALONE);
        // Hits older than the credit time credit nothing.
        world.note_mob_damage(mob, HELPER, credit);
        world.last_now_ms += CREDIT_TIME_LIMIT_MS + 1;
        world.reward_kill(mob, PLAYER);
        assert_eq!(state(&mut world, PLAYER).0, SHARED + 2 * ALONE);
        assert_eq!(state(&mut world, HELPER).0, SHARED);
    }

    #[test]
    fn hits_below_the_credit_share_of_the_health_credit_nothing() {
        const MOB: u64 = 7;
        const MAX_HEALTH: u32 = 1_000;
        const ONE_PERCENT: u32 = 10;
        let mut credits = KillCredits::default();
        credits.note(MOB, 1, ONE_PERCENT - 1, 0);
        credits.note(MOB, 2, ONE_PERCENT / 2, 0);
        credits.note(MOB, 2, ONE_PERCENT / 2, 1);
        credits.note(MOB + 1, 3, ONE_PERCENT, 0);
        assert_eq!(credits.take(MOB, MAX_HEALTH, 1), [2]);
        assert!(credits.hits.contains_key(&(MOB + 1)), "a fresh hit stays");
        assert_eq!(credits.take(MOB, MAX_HEALTH, 1), Vec::<u64>::new());
        credits.take(MOB, MAX_HEALTH, CREDIT_TIME_LIMIT_MS + 1);
        assert!(credits.hits.is_empty(), "every stale hit goes");
    }

    #[test]
    fn a_kill_gives_experience_and_masteries() {
        let mut world = world_with_spawns(vec![VitalsSpawn {
            vitals_id: lowercase_crc32(LEGION),
            level: 1,
            position: [0.0, 10.0, 50.0],
            ..VitalsSpawn::default()
        }]);
        let mut content = content();
        content.gameplay.xp.character = RETAIL_MULTIPLIER;
        content.gameplay.xp.progression = [
            (SWORD_MASTERY.to_lowercase(), RETAIL_MULTIPLIER),
            (STAFF_MASTERY.to_lowercase(), RETAIL_MULTIPLIER),
        ]
        .into();
        world.content = Arc::new(content);
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let entity = world.player_by_number(PLAYER).expect("player");
        world.world.get_mut::<Player>(entity).expect("player").gear = Some(gear_with_staff());
        let coins_before = world.world.get::<Player>(entity).expect("player").coins;
        let mob = mob_entity(0);
        world.reward_kill(mob, PLAYER);
        let player = world.world.get::<Player>(entity).expect("player");
        // Retail: character +20, the held weapon +20, the other +4.
        assert_eq!(player.experience, 20);
        // The kill's `KillEvent`: its coins in the wallet and its entry in the log.
        assert_eq!(player.coins - coins_before, 100);
        let mut log = player.game_events.clone();
        let member = log.take_member(1).expect("the kill's entry");
        let MemberState::GameEvents(state) = member.state else {
            panic!("a game event member");
        };
        let Some(ReplicatedList::Full(list)) = state.game_events else {
            panic!("the first entry goes in full");
        };
        let entry = &list.elements[0];
        assert_eq!(entry.event, lowercase_crc32(KILL_EVENT));
        assert_eq!((entry.value_a, entry.standing, entry.value_f), (100, 5, 5));
        let gained: Vec<u32> = [SWORD_MASTERY, STAFF_MASTERY]
            .iter()
            .map(|id| {
                let mastery = world.content.progression.mastery(id).expect("mastery");
                player.masteries.experience(mastery)
            })
            .collect();
        assert!(gained == [20, 4] || gained == [4, 20], "{gained:?}");
        // `Token_Weapon_1` (cut to the two masteries) doubles both.
        let mut content = (*world.content).clone();
        content.status_effects = StatusEffectRules::build(StatusEffectSource {
            effects: vec![EffectSourceRow {
                status_id: TOKEN.to_owned(),
                lightweight: true,
                rewards: RewardMods {
                    progression: vec![
                        (SWORD_MASTERY.to_lowercase(), 1.0),
                        (STAFF_MASTERY.to_lowercase(), 1.0),
                    ],
                    ..RewardMods::default()
                },
                ..EffectSourceRow::default()
            }],
            ..StatusEffectSource::default()
        });
        world.content = Arc::new(content);
        {
            let rules = world.content.status_effects.clone();
            let mut player = world.world.get_mut::<Player>(entity).expect("player");
            assert!(
                player
                    .status_effects
                    .add_by_id(&rules, TOKEN, 1.0, PLAYER, 0)
            );
        }
        world.reward_kill(mob, PLAYER);
        let player = world.world.get::<Player>(entity).expect("player");
        let doubled: Vec<u32> = [SWORD_MASTERY, STAFF_MASTERY]
            .iter()
            .map(|id| {
                let mastery = world.content.progression.mastery(id).expect("mastery");
                player.masteries.experience(mastery)
            })
            .collect();
        assert!(
            doubled == [20 + 40, 4 + 8] || doubled == [4 + 8, 20 + 40],
            "{doubled:?}"
        );
        world.reward_kill(mob, PLAYER + 1);
        let player = world.world.get::<Player>(entity).expect("player");
        assert_eq!(
            player.experience, 40,
            "the token leaves the character's experience alone, and a killer that is not here \
             gets nothing"
        );
    }
}
