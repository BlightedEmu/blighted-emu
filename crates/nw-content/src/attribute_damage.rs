// SPDX-License-Identifier: AGPL-3.0-or-later
//! How a player's level and attributes scale the damage of its weapon (damage study 2026-11-11:
//! `blighted-emu-tools` `scratch-work/capdecode/src/bin/hitinputs.rs` on the retail own hits).
//!
//! Observed: a hit is `BaseDamage x G(gear score) x DmgCoef x W x (1 + M + crit)`, with
//! `W = 1 + Level Damage Multiplier x (level - 1) + sum of Scaling x ModifierValueSum`: each
//! attribute the weapon scales with adds its weapon `Scaling*` cell times the
//! `ModifierValueSum` of the attribute's value in its `AttributeDefinition` table. All four
//! attributes add, not the highest one alone. It fits 1,276 retail hits of characters up to
//! level 30 to 5 digits (the tutorial sword at level 1 with every attribute at 5: W = 1; a
//! hatchet at level 8 with Strength 40: `1 + 0.175 + 0.0075 x 43.75`). Unknown: the hits of
//! characters past level 36 are 5 to 50 % above it (more additive modifiers of their perks
//! and passives).
//!
//! A heal whose effect scales with its healer (`UseHealScalingValue`) takes the level's share as
//! a hit does and the `ScalingValue` of the healer's Focus in place of the weapon's attribute
//! shares ([`AttributeDamage::heal_scaling`]).

use std::collections::BTreeMap;

use crate::Tables;
use crate::character_points::{ATTRIBUTE_COUNT, ATTRIBUTE_NAMES, FOCUS};
use crate::generated::AttributeDefinition;

/// `Level Damage Multiplier` of `playerbaseattributes.pbadb`: the share of its damage a weapon
/// gains per player level past the first. A heal that scales with its healer gains it too
/// ([`AttributeDamage::heal_scaling`]).
pub const LEVEL_DAMAGE_MULTIPLIER: f64 = 0.025;

/// The multiple of a weapon's damage or of a heal before the level and the attributes add to it.
const PLAIN_SCALING: f64 = 1.0;

/// The values of one column of each attribute's `AttributeDefinition` table, by attribute id,
/// then by the attribute's value (`Level`).
type ByValue = [BTreeMap<u32, f64>; ATTRIBUTE_COUNT];

/// The `ModifierValueSum` and the `ScalingValue` of each value of each attribute.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AttributeDamage {
    /// `ModifierValueSum`: the damage scaling.
    sums: ByValue,
    /// `ScalingValue`: the heal scaling (the sum of the `HealScaling` cells up to the value;
    /// only Focus has them in build 25810808).
    heal_values: ByValue,
}

impl AttributeDamage {
    /// The sums of the rows `rows`: attribute id, `Level`, `ModifierValueSum`.
    pub fn new(rows: impl IntoIterator<Item = (usize, u32, f64)>) -> Self {
        Self {
            sums: by_value(rows),
            heal_values: ByValue::default(),
        }
    }

    /// The same table with the heal scaling rows `rows`: attribute id, `Level`, `ScalingValue`.
    #[must_use]
    pub fn with_heal_values(mut self, rows: impl IntoIterator<Item = (usize, u32, f64)>) -> Self {
        self.heal_values = by_value(rows);
        self
    }

    /// The sums and the heal scaling of the `AttributeDefinition` tables of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let column = |cell: fn(&AttributeDefinition) -> Option<f64>| {
            tables.attribute_definition.iter().flat_map(move |table| {
                let attribute = ATTRIBUTE_NAMES
                    .iter()
                    .position(|name| table.unique_id.eq_ignore_ascii_case(name));
                table.rows.iter().filter_map(move |row| {
                    Some((attribute?, u32::try_from(row.level).ok()?, cell(row)?))
                })
            })
        };
        Self::new(column(|row| row.modifier_value_sum))
            .with_heal_values(column(|row| row.scaling_value))
    }

    /// The `ModifierValueSum` of value `value` of attribute `attribute`: its row's, else the
    /// row below it; 0 below the first row or without the table.
    pub fn modifier_value_sum(&self, attribute: usize, value: u32) -> f64 {
        at_value(&self.sums, attribute, value)
    }

    /// `W`: the multiple of a weapon's damage for a player of `level` with the attribute
    /// `values` (by attribute id) and the weapon's `scaling` (by attribute id).
    pub fn scaling(
        &self,
        level: u32,
        values: [u32; ATTRIBUTE_COUNT],
        scaling: [f64; ATTRIBUTE_COUNT],
    ) -> f64 {
        let attributes: f64 = values
            .iter()
            .zip(scaling)
            .enumerate()
            .map(|(attribute, (value, scaling))| {
                scaling * self.modifier_value_sum(attribute, *value)
            })
            .sum();
        PLAIN_SCALING + level_share(level) + attributes
    }

    /// The multiple of a heal that scales with its healer (`UseHealScalingValue`) for a healer of
    /// `level` with `focus` Focus: one plus the level's share, as for a weapon's damage, plus
    /// the `ScalingValue` of its Focus. Observed (retail `general-other-01`, from 727.537 s):
    /// the own player at level 65 with Focus 250 (its final value, gear included) held its own
    /// `Status_Ability_Lifestaff_SacredGround` (0.16) and healed 356.69 a second (727.203 to
    /// 728.172 s, 729.205 to 730.188 s) with the life staff
    /// `2hStaffLife_ConquerorsMercyT5_V2` of gear score 625 (base damage 55) in the hand: with
    /// this multiple (8.219), `Upgrade2`'s and the staff passive's `NonConsumableHealMod` (0.5,
    /// 0.1) and its outgoing heal mods (3 stacks of `ATB_Foc_Bonus_250_1_Effect`, the staff's
    /// `PerkID_StatSwap_OutgoingHealing` at potency 1.2) the heal is 354.0, 0.75 % short.
    /// Unknown: the rest (the hits of characters past level 36 are above the damage rule too).
    pub fn heal_scaling(&self, level: u32, focus: u32) -> f64 {
        PLAIN_SCALING + level_share(level) + at_value(&self.heal_values, focus_index(), focus)
    }
}

/// The share of a weapon's damage or of a heal that player `level` adds.
fn level_share(level: u32) -> f64 {
    LEVEL_DAMAGE_MULTIPLIER * f64::from(level.saturating_sub(1))
}

/// The index of Focus in [`ATTRIBUTE_NAMES`] order.
fn focus_index() -> usize {
    usize::try_from(FOCUS).unwrap_or(usize::MAX)
}

/// The rows `rows` (attribute id, `Level`, value) by attribute, then by value.
fn by_value(rows: impl IntoIterator<Item = (usize, u32, f64)>) -> ByValue {
    let mut by_value = ByValue::default();
    for (attribute, value, cell) in rows {
        if let Some(by_attribute) = by_value.get_mut(attribute) {
            by_attribute.insert(value, cell);
        }
    }
    by_value
}

/// The cell of value `value` of attribute `attribute` in `by_value`: its row's, else the row
/// below it; 0 below the first row or without the table.
fn at_value(by_value: &ByValue, attribute: usize, value: u32) -> f64 {
    by_value
        .get(attribute)
        .and_then(|by_attribute| by_attribute.range(..=value).next_back())
        .map_or(0.0, |(_, cell)| *cell)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character_points::{DEXTERITY, STRENGTH};

    const CLOSE: f64 = 1e-9;
    /// Build 25810808: `ModifierValueSum` of Strength 5 and 40.
    const SUM_AT_5: f64 = 0.0;
    const SUM_AT_40: f64 = 43.75;
    /// `SoldierArchetype_1hHatchet`: `ScalingStrength`.
    const HATCHET_STRENGTH: f64 = 0.0075;

    fn damage() -> AttributeDamage {
        AttributeDamage::new([
            (STRENGTH as usize, 5, SUM_AT_5),
            (STRENGTH as usize, 40, SUM_AT_40),
            (DEXTERITY as usize, 5, SUM_AT_5),
        ])
    }

    fn only(attribute: u32, value: f64) -> [f64; ATTRIBUTE_COUNT] {
        let mut scaling = [0.0; ATTRIBUTE_COUNT];
        scaling[attribute as usize] = value;
        scaling
    }

    #[test]
    fn a_new_character_s_weapon_does_its_plain_damage() {
        // Retail tutorial: level 1, every attribute 5: 68.000 = 80 x (1 - 0.15), so W = 1.
        let w = damage().scaling(1, [5; ATTRIBUTE_COUNT], only(STRENGTH, 0.008));
        assert!((w - 1.0).abs() < CLOSE);
    }

    #[test]
    fn level_and_attributes_add_to_the_scaling() {
        // Retail `Playthrough_Part_01` at 953.936 s: level 8, Strength 40, the hatchet.
        let mut values = [5; ATTRIBUTE_COUNT];
        values[STRENGTH as usize] = 40;
        let w = damage().scaling(8, values, only(STRENGTH, HATCHET_STRENGTH));
        assert!((w - 1.503_125).abs() < CLOSE);
    }

    /// Build 25810808 `javelindata_attributefocus`: `ScalingValue` of Focus 5 and 250.
    const FOCUS_HEAL_AT_5: f64 = 0.0;
    const FOCUS_HEAL_AT_250: f64 = 5.619;

    #[test]
    fn a_heal_scales_with_the_level_and_the_focus_scaling_value() {
        use crate::character_points::FOCUS;
        let damage = damage().with_heal_values([
            (FOCUS as usize, 5, FOCUS_HEAL_AT_5),
            (FOCUS as usize, 250, FOCUS_HEAL_AT_250),
        ]);
        // Retail `general-other-01` from 727.537 s: level 65, Focus 250.
        assert!((damage.heal_scaling(65, 250) - 8.219).abs() < CLOSE);
        // Level 22 with Focus 5 and below the first row: the level's share alone.
        assert!((damage.heal_scaling(22, 5) - 1.525).abs() < CLOSE);
        assert!((damage.heal_scaling(1, 4) - 1.0).abs() < CLOSE);
        // The damage sums stay the damage's.
        assert!((damage.modifier_value_sum(FOCUS as usize, 250)).abs() < CLOSE);
    }

    #[test]
    fn a_value_between_two_rows_takes_the_row_below() {
        let damage = damage();
        assert!((damage.modifier_value_sum(STRENGTH as usize, 39) - SUM_AT_5).abs() < CLOSE);
        assert!((damage.modifier_value_sum(STRENGTH as usize, 4)).abs() < CLOSE);
        assert!((damage.modifier_value_sum(ATTRIBUTE_COUNT, 40)).abs() < CLOSE);
    }
}
