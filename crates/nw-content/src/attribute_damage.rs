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

use std::collections::BTreeMap;

use crate::Tables;
use crate::character_points::{ATTRIBUTE_COUNT, ATTRIBUTE_NAMES};

/// `Level Damage Multiplier` of `playerbaseattributes.pbadb`: the share of its damage a weapon
/// gains per player level past the first.
pub const LEVEL_DAMAGE_MULTIPLIER: f64 = 0.025;

/// The `ModifierValueSum` of each value of each attribute.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AttributeDamage {
    /// By attribute id, then by the attribute's value (`Level`).
    sums: [BTreeMap<u32, f64>; ATTRIBUTE_COUNT],
}

impl AttributeDamage {
    /// The sums of the rows `rows`: attribute id, `Level`, `ModifierValueSum`.
    pub fn new(rows: impl IntoIterator<Item = (usize, u32, f64)>) -> Self {
        let mut sums: [BTreeMap<u32, f64>; ATTRIBUTE_COUNT] = Default::default();
        for (attribute, value, sum) in rows {
            if let Some(by_value) = sums.get_mut(attribute) {
                by_value.insert(value, sum);
            }
        }
        Self { sums }
    }

    /// The sums of the `AttributeDefinition` tables of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::new(tables.attribute_definition.iter().flat_map(|table| {
            let attribute = ATTRIBUTE_NAMES
                .iter()
                .position(|name| table.unique_id.eq_ignore_ascii_case(name));
            table.rows.iter().filter_map(move |row| {
                Some((
                    attribute?,
                    u32::try_from(row.level).ok()?,
                    row.modifier_value_sum?,
                ))
            })
        }))
    }

    /// The `ModifierValueSum` of value `value` of attribute `attribute`: its row's, else the
    /// row below it; 0 below the first row or without the table.
    pub fn modifier_value_sum(&self, attribute: usize, value: u32) -> f64 {
        self.sums
            .get(attribute)
            .and_then(|by_value| by_value.range(..=value).next_back())
            .map_or(0.0, |(_, sum)| *sum)
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
        1.0 + LEVEL_DAMAGE_MULTIPLIER * f64::from(level.saturating_sub(1)) + attributes
    }
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

    #[test]
    fn a_value_between_two_rows_takes_the_row_below() {
        let damage = damage();
        assert!((damage.modifier_value_sum(STRENGTH as usize, 39) - SUM_AT_5).abs() < CLOSE);
        assert!((damage.modifier_value_sum(STRENGTH as usize, 4)).abs() < CLOSE);
        assert!((damage.modifier_value_sum(ATTRIBUTE_COUNT, 40)).abs() < CLOSE);
    }
}
