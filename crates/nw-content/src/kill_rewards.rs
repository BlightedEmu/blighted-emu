// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a mob kill gives its killer besides loot (progression note, section 7): character
//! experience and weapon mastery experience.
//!
//! The kill's raw experience is `RewardData.KillExperience` of the mob's level times the
//! `RewardModifierData.Experience` of the mob's creature type (`VitalsBaseData.CreatureType`,
//! a level variant takes its base's) and one plus the `LevelDisparityData.KillExpModifier` of
//! the level gap. A mob whose `NoExpNoCoin` is set gives none (Observed, content bundle
//! 25810808: the tutorial ship's sailors `Corrupted_Sailor_1H_Club_FTUE`; retail gave no
//! experience for them and 20 for each island foe, which are `Solo`).
//!
//! The weapon that made the kill trains its mastery (`WeaponItemDefinitions.WeaponMasteryCategoryId`
//! of the item's `ItemStatsRef`) by the raw experience rounded up; the other worn weapon trains
//! its mastery by the raw experience times [`OTHER_WEAPON_SHARE`], rounded up. Each gain is then
//! multiplied by the remote config's multiplier of its progression (Observed, retail newchar
//! 2026-09-18: character +20, the killing greatsword +20 and the worn blunderbuss +4 at a level 1
//! kill, multipliers 2; a kill of raw 5.5 gave character +10, mastery +12 and +2).

use std::collections::{BTreeMap, HashMap};

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The share of a kill's raw experience that the worn weapon which did not make the kill gains.
/// Fitted to retail (Observed): 19 kills of newchar 2026-09-18 and levelling_weapon_up (raw 5.5,
/// 10, 11, 12, 22, 24, 26, 36, 73 gave 1, 2, 2, 2, 4, 4, 4, 6, 11 before the multiplier) fit
/// `ceil(raw x share)` for every share above 0.1389 and up to 0.1507; no table holds the value.
pub const OTHER_WEAPON_SHARE: f64 = 0.15;
/// The `NoExpNoCoin` cell of a mob that gives no reward.
const NO_REWARD: &str = "1";
/// The factor of a kill without a level gap modifier.
const PLAIN: f64 = 1.0;

/// The reward kind of one mob type.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MobKind {
    /// `CreatureType`.
    creature_type: Option<String>,
    /// Whether `NoExpNoCoin` is set.
    no_reward: bool,
}

/// The rules of the kill rewards.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KillRewards {
    /// The kind of each mob type, by the CRC-32 of its lowercase `VitalsID`.
    mobs: HashMap<u32, MobKind>,
    /// `RewardModifierData.Experience` by lowercase `Modifiers` (the creature type).
    experience_mods: HashMap<String, f64>,
    /// `LevelDisparityData.KillExpModifier` by `LevelDisparity`.
    disparity: BTreeMap<i64, f64>,
    /// The weapon mastery of each weapon item, by lowercase item id.
    weapon_masteries: HashMap<String, String>,
}

/// One `VitalsBaseData` row as [`KillRewards::build`] takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobRow<'a> {
    /// `VitalsID`.
    pub vitals_id: &'a str,
    /// `CreatureType`.
    pub creature_type: Option<&'a str>,
    /// `NoExpNoCoin`.
    pub no_exp_no_coin: Option<&'a str>,
}

impl KillRewards {
    /// The rules of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let mobs = tables
            .vitals_base_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| MobRow {
                vitals_id: &row.vitals_id,
                creature_type: row.creature_type.as_deref(),
                no_exp_no_coin: row.no_exp_no_coin.as_deref(),
            });
        let variants = tables
            .vitals_level_variant_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.vitals_id.as_str(), row.base_vitals_id.as_str()));
        let experience_mods = tables
            .reward_modifier_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.modifiers.as_str(), row.experience));
        let disparity = tables
            .level_disparity_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.level_disparity, row.kill_exp_modifier));
        let weapon_masteries: HashMap<String, &str> = tables
            .weapon_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                Some((
                    row.weapon_id.to_lowercase(),
                    row.weapon_mastery_category_id.as_deref()?,
                ))
            })
            .collect();
        let items = tables
            .master_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let stats = row.item_stats_ref.as_deref()?.to_lowercase();
                Some((row.item_id.as_str(), *weapon_masteries.get(&stats)?))
            });
        Self::build(mobs, variants, experience_mods, disparity, items)
    }

    /// The rules of the mob rows `mobs`, the level variants `variants` (variant id, base id),
    /// the creature type `experience_mods` (creature type, `Experience`), the level gap
    /// modifiers `disparity` (gap, `KillExpModifier`) and the weapon `items` (item id, mastery
    /// id). The first row of an id wins.
    pub fn build<'a>(
        mobs: impl IntoIterator<Item = MobRow<'a>>,
        variants: impl IntoIterator<Item = (&'a str, &'a str)>,
        experience_mods: impl IntoIterator<Item = (&'a str, f64)>,
        disparity: impl IntoIterator<Item = (i64, f64)>,
        items: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        let mut by_id: HashMap<String, MobKind> = HashMap::new();
        for row in mobs {
            by_id
                .entry(row.vitals_id.to_lowercase())
                .or_insert_with(|| MobKind {
                    creature_type: row.creature_type.map(str::to_owned),
                    no_reward: row
                        .no_exp_no_coin
                        .is_some_and(|cell| cell.trim() == NO_REWARD),
                });
        }
        let mut mobs: HashMap<u32, MobKind> = by_id
            .iter()
            .map(|(id, kind)| (lowercase_crc32(id), kind.clone()))
            .collect();
        for (variant, base) in variants {
            if let Some(kind) = by_id.get(&base.to_lowercase()) {
                mobs.entry(lowercase_crc32(variant))
                    .or_insert_with(|| kind.clone());
            }
        }
        let mut mods = HashMap::new();
        for (kind, experience) in experience_mods {
            mods.entry(kind.to_lowercase()).or_insert(experience);
        }
        let mut gaps = BTreeMap::new();
        for (gap, modifier) in disparity {
            gaps.entry(gap).or_insert(modifier);
        }
        let mut weapons = HashMap::new();
        for (item, mastery) in items {
            weapons
                .entry(item.to_lowercase())
                .or_insert_with(|| mastery.to_owned());
        }
        Self {
            mobs,
            experience_mods: mods,
            disparity: gaps,
            weapon_masteries: weapons,
        }
    }

    /// The factor of a kill of the mob type `vitals_crc` of level `mob_level` by a player of
    /// `player_level`: its creature type's experience modifier times one plus the level gap's
    /// modifier (the gap is clamped to the table's rows). 0 for a mob without a reward, an
    /// unknown mob type or a creature type without a modifier row.
    pub fn factor(&self, vitals_crc: u32, mob_level: u32, player_level: u32) -> f64 {
        let Some(kind) = self.mobs.get(&vitals_crc).filter(|kind| !kind.no_reward) else {
            return 0.0;
        };
        let Some(creature) = kind
            .creature_type
            .as_ref()
            .and_then(|kind| self.experience_mods.get(&kind.to_lowercase()))
        else {
            return 0.0;
        };
        let gap = i64::from(mob_level) - i64::from(player_level);
        (creature * (PLAIN + self.gap_modifier(gap))).max(0.0)
    }

    /// The `CreatureType` of the mob type `vitals_crc` (a level variant has its base's); `None`
    /// for an unknown mob type or one without a type.
    pub fn creature_type(&self, vitals_crc: u32) -> Option<&str> {
        self.mobs.get(&vitals_crc)?.creature_type.as_deref()
    }

    /// The `KillExpModifier` of level gap `gap`, clamped to the table's rows; 0 between rows
    /// or without a table.
    fn gap_modifier(&self, gap: i64) -> f64 {
        let (Some((low, _)), Some((high, _))) =
            (self.disparity.first_key_value(), self.disparity.last_key_value())
        else {
            return 0.0;
        };
        let gap = gap.clamp(*low, *high);
        self.disparity.get(&gap).copied().unwrap_or_default()
    }

    /// The weapon mastery (`CategoricalProgressionId`) that item `item_id` trains; `None` for
    /// an item that is no player weapon. Case does not matter.
    pub fn weapon_mastery(&self, item_id: &str) -> Option<&str> {
        self.weapon_masteries
            .get(&item_id.to_lowercase())
            .map(String::as_str)
    }
}

/// The whole points of a gain of `raw`, rounded up; 0 for none.
pub fn whole_up(raw: f64) -> u32 {
    let points = raw.ceil();
    if points >= f64::from(u32::MAX) {
        u32::MAX
    } else if points > 0.0 {
        // In range: checked above.
        points as u32
    } else {
        0
    }
}

/// The whole points of a gain of `raw`, rounded down; 0 for none.
pub fn whole_down(raw: f64) -> u32 {
    whole_up(raw.floor())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGION: &str = "Legion_Legionnaire_FTUE";
    const SAILOR: &str = "Corrupted_Sailor_1H_Club_FTUE";
    const VARIANT: &str = "Legion_Legionnaire_FTUE_L3";
    const ELITE: &str = "Elite_Boar";
    /// Retail `RewardModifierData` rows (content bundle 25810808).
    const SOLO_MOD: f64 = 1.0;
    const ELITE_MOD: f64 = 3.0;
    /// Retail `LevelDisparityData`: -20 and -19 to -10 cut kill experience.
    const GAPS: [(i64, f64); 3] = [(-20, -0.8), (-10, -0.4), (3, 0.0)];
    const SWORD_ITEM: &str = "2hGreatSword_StarterT1";
    const SWORD_MASTERY: &str = "GreatSwordAbilityTable";

    fn rewards() -> KillRewards {
        KillRewards::build(
            [
                MobRow {
                    vitals_id: LEGION,
                    creature_type: Some("Solo"),
                    no_exp_no_coin: None,
                },
                MobRow {
                    vitals_id: SAILOR,
                    creature_type: Some("Solo_StarterBeach"),
                    no_exp_no_coin: Some("1"),
                },
                MobRow {
                    vitals_id: ELITE,
                    creature_type: Some("Elite"),
                    no_exp_no_coin: None,
                },
                MobRow {
                    vitals_id: "Unmodified",
                    creature_type: Some("NoRow"),
                    no_exp_no_coin: None,
                },
            ],
            [(VARIANT, LEGION), ("Orphan", "Missing")],
            [("Solo", SOLO_MOD), ("Elite", ELITE_MOD)],
            GAPS,
            [(SWORD_ITEM, SWORD_MASTERY)],
        )
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn the_creature_type_and_the_level_gap_scale_a_kill() {
        let rewards = rewards();
        assert!(close(rewards.factor(lowercase_crc32(LEGION), 1, 0), SOLO_MOD));
        assert!(close(
            rewards.factor(lowercase_crc32(VARIANT), 3, 2),
            SOLO_MOD
        ));
        assert!(close(rewards.factor(lowercase_crc32(ELITE), 5, 5), ELITE_MOD));
        // Retail: the ship's sailors give nothing.
        assert!(close(rewards.factor(lowercase_crc32(SAILOR), 1, 0), 0.0));
        assert!(close(rewards.factor(lowercase_crc32("Unmodified"), 1, 1), 0.0));
        assert!(close(rewards.factor(lowercase_crc32("Orphan"), 1, 1), 0.0));
        // A mob 10 levels below the player gives 60 %, 30 below (clamped to -20) 20 %.
        assert!(close(rewards.factor(lowercase_crc32(LEGION), 1, 11), 0.6));
        assert!(close(rewards.factor(lowercase_crc32(LEGION), 1, 31), 0.2));
        assert!(close(rewards.factor(lowercase_crc32(LEGION), 40, 1), SOLO_MOD));
    }

    #[test]
    fn a_mob_type_and_its_level_variant_name_their_creature_type() {
        let rewards = rewards();
        assert_eq!(rewards.creature_type(lowercase_crc32(ELITE)), Some("Elite"));
        assert_eq!(rewards.creature_type(lowercase_crc32(VARIANT)), Some("Solo"));
        assert_eq!(rewards.creature_type(lowercase_crc32("Orphan")), None);
    }

    #[test]
    fn weapons_name_their_mastery_and_gains_round_as_retail() {
        let rewards = rewards();
        assert_eq!(
            rewards.weapon_mastery("2HGREATSWORD_STARTERT1"),
            Some(SWORD_MASTERY)
        );
        assert_eq!(rewards.weapon_mastery("Flint"), None);
        // Retail raw 5.5: character 5, mastery 6, the other weapon 1 (before the multiplier).
        assert_eq!(whole_down(5.5), 5);
        assert_eq!(whole_up(5.5), 6);
        assert_eq!(whole_up(5.5 * OTHER_WEAPON_SHARE), 1);
        for (raw, other) in [(10.0, 2), (22.0, 4), (24.0, 4), (36.0, 6), (73.0, 11)] {
            assert_eq!(whole_up(raw * OTHER_WEAPON_SHARE), other, "raw {raw}");
        }
        assert_eq!(whole_up(-1.0), 0);
        assert_eq!(whole_up(f64::MAX), u32::MAX);
    }
}
