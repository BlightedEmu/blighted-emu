// SPDX-License-Identifier: AGPL-3.0-or-later
//! Combat in a `sim` (M6-A spec sections 7 and 8).

use std::sync::Arc;

use nw_content::combat::CombatData;
use nw_sim_core::BrainSettings;

/// What combat needs from the configuration and the datasheets.
#[derive(Debug, Clone)]
pub struct CombatSettings {
    /// The combat rows.
    pub data: Arc<CombatData>,
    /// The numbers of the mob brain.
    pub brain: BrainSettings,
    /// Share of a player's damage that mob armor takes off.
    pub armor_share: f64,
    /// Share of a mob's damage that player armor takes off.
    pub player_armor_share: f64,
    /// Factor of a mob's attack.
    pub mob_attack_factor: f64,
    /// Extra reach allowed for lag.
    pub reach_margin_m: f32,
    /// Shortest time between two attacks of a player.
    pub min_attack_interval_ms: u64,
    /// Time from a player's death to its respawn.
    pub player_respawn_ms: u64,
    /// Seed of the combat dice.
    pub seed: u64,
}

#[cfg(test)]
pub(crate) mod tests {
    use nw_content::combat::{AttackRow, CombatRows, CombatSource, LevelRow, MobType, WeaponRow};

    use super::*;
    use crate::config::tests::committed;

    /// The vitals id of the test mob type.
    pub(crate) const TEST_MOB: &str = "Test_Boar";
    /// The level of the test spawns.
    pub(crate) const TEST_LEVEL: u32 = 20;
    /// The test player's max health.
    pub(crate) const TEST_PLAYER_HEALTH: u32 = 856;
    /// Base health and damage of the test level.
    pub(crate) const TEST_LEVEL_HEALTH: f64 = 1000.0;
    pub(crate) const TEST_LEVEL_DAMAGE: f64 = 100.0;
    /// Base damage of the test sword.
    pub(crate) const TEST_SWORD_DAMAGE: f64 = 64.0;

    /// Hand-made combat rows with the committed config's ids.
    pub(crate) fn test_data() -> CombatData {
        let rows = CombatRows::default();
        let source = CombatSource {
            base_types: vec![(TEST_MOB.to_owned(), MobType::PLAIN)],
            levels: vec![(
                TEST_LEVEL,
                LevelRow {
                    base_max_health: TEST_LEVEL_HEALTH,
                    base_damage: TEST_LEVEL_DAMAGE,
                },
            )],
            weapons: vec![(
                rows.weapon_id.clone(),
                WeaponRow {
                    base_damage: TEST_SWORD_DAMAGE,
                    crit_chance: 0.0,
                    crit_multiplier: 1.0,
                },
            )],
            attacks: rows
                .attack_damage_ids
                .iter()
                .map(|id| AttackRow {
                    damage_id: id.clone(),
                    coefficient: 1.0,
                    can_crit: false,
                    threat: 0,
                })
                .collect(),
            player_health: vec![(rows.player_vitals_id.clone(), TEST_PLAYER_HEALTH)],
            ..CombatSource::default()
        };
        CombatData::build(source, &rows).expect("test combat data")
    }

    /// The committed config's combat settings on the test rows.
    pub(crate) fn test_combat() -> CombatSettings {
        committed().combat_settings(Arc::new(test_data()))
    }
}
