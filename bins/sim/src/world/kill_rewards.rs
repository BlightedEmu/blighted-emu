// SPDX-License-Identifier: AGPL-3.0-or-later
//! The experience of a mob kill (progression note, section 7; [`nw_content::kill_rewards`]):
//! the killer's character experience, the mastery of the weapon in its hand and, by a share,
//! the mastery of its other worn weapon, each times the remote config's multiplier of its
//! progression, and the kill's meta achievement counters.
//!
//! A killer on another sim gets nothing here, as it gets no loot (its sim does not hear of the
//! kill). A tutorial mob's character experience comes from the tutorial's own order (its
//! `KillExperience`); its masteries come from here.

use std::sync::Arc;

use nw_content::kill_rewards::{OTHER_WEAPON_SHARE, whole_down, whole_up};
use nw_messages::paperdoll::slot;

use super::gear::Gear;
use super::meta_achievements::MetaEvent;
use super::quest_objects::gain_experience;
use super::tradeskill::progression_gain;
use super::{Player, SimWorld};
use crate::player_content::PlayerContent;

/// One firing of a gain.
const ONCE: u32 = 1;

impl SimWorld {
    /// Mob `mob` died by a hit of player `killer` (entity number): the killer gains the kill's
    /// character and weapon mastery experience, and its meta achievements count the kill.
    pub(super) fn reward_kill(&mut self, mob: u64, killer: u64) {
        let Some(entity) = self.player_by_number(killer) else {
            return;
        };
        let Some(vitals) = self.mobs.vitals_of(mob) else {
            return;
        };
        let content = Arc::clone(&self.content);
        let mob_level = self.mobs.level(mob).unwrap_or_default();
        let tutorial_mob = self.mobs.owner(mob).is_some();
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            let player = &mut *player;
            let player_level = content.progression.level_of(player.experience);
            let base = content.kill_experience.of(mob_level).unwrap_or_default();
            let raw =
                f64::from(base) * content.kill_rewards.factor(vitals, mob_level, player_level);
            if !tutorial_mob {
                gain_experience(player, whole_down(raw), &content.gameplay.xp);
            }
            for (mastery_id, points) in mastery_gains(&content, player.gear.as_ref(), raw) {
                let Some(mastery) = content.progression.mastery(&mastery_id) else {
                    continue;
                };
                let multiplier = content.gameplay.xp.progression(&mastery_id);
                player.masteries.gain(
                    mastery,
                    progression_gain(f64::from(points), ONCE, multiplier),
                );
            }
        }
        let categories = content
            .kill_types
            .of(vitals)
            .map(|(_, categories)| categories.to_vec())
            .unwrap_or_default();
        self.meta_event(
            entity,
            MetaEvent::Kill {
                categories: &categories,
            },
        );
    }
}

/// The weapon mastery points of a kill of raw experience `raw` by a player wearing `gear`
/// (before the multipliers): the weapon in the hand gains `raw` rounded up, each other worn
/// main-hand weapon of another mastery `raw` times [`OTHER_WEAPON_SHARE`] rounded up.
fn mastery_gains(content: &PlayerContent, gear: Option<&Gear>, raw: f64) -> Vec<(String, u32)> {
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
        .map(|mastery| (mastery.clone(), whole_up(raw)))
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
        gains.push((mastery.to_owned(), whole_up(raw * OTHER_WEAPON_SHARE)));
    }
    gains
}

#[cfg(test)]
mod tests {
    use nw_content::kill_rewards::{KillRewards, MobRow};
    use nw_content::progression::Progression;
    use nw_content::rewards::KillExperience;
    use nw_formats::objectstream::lowercase_crc32;
    use nw_world::records::VitalsSpawn;

    use super::*;
    use crate::mobs::mob_entity;
    use crate::world::gear::tests::{STAFF, SWORD, gear_with_staff};
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, world_with_spawns};

    const PLAYER: u64 = 41;
    const SWORD_MASTERY: &str = "SwordAbilityTable";
    const STAFF_MASTERY: &str = "LifeMagicAbilityTable";
    const LEGION: &str = "Legion_Legionnaire_FTUE";
    /// Retail `RewardData` level 1 `KillExperience`.
    const LEVEL_ONE_KILL: i64 = 10;
    /// Retail `RewardModifierData` `Solo`.
    const SOLO: f64 = 1.0;
    /// Retail `javelin.xp-multipliers` of the character and of each weapon mastery.
    const RETAIL_MULTIPLIER: f64 = 2.0;

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
                }],
                [],
                [("Solo", SOLO)],
                [],
                [(SWORD, SWORD_MASTERY), (STAFF, STAFF_MASTERY)],
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
        let mob = mob_entity(0);
        world.reward_kill(mob, PLAYER);
        let player = world.world.get::<Player>(entity).expect("player");
        // Retail: character +20, the held weapon +20, the other +4.
        assert_eq!(player.experience, 20);
        let gained: Vec<u32> = [SWORD_MASTERY, STAFF_MASTERY]
            .iter()
            .map(|id| {
                let mastery = world.content.progression.mastery(id).expect("mastery");
                player.masteries.experience(mastery)
            })
            .collect();
        assert!(gained == [20, 4] || gained == [4, 20], "{gained:?}");
        world.reward_kill(mob, PLAYER + 1);
        let player = world.world.get::<Player>(entity).expect("player");
        assert_eq!(
            player.experience, 20,
            "a killer that is not here gets nothing"
        );
    }
}
