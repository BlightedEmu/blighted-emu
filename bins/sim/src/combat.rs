// SPDX-License-Identifier: AGPL-3.0-or-later
//! Combat in a `sim` (M6-A spec sections 7 and 8).

use std::sync::Arc;

use nw_bus::{BusId, Hit};
use nw_content::combat::CombatData;
use nw_sim_core::BrainSettings;

use crate::mobs::VitalsSpawn;

/// Why an attack did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AttackRefusal {
    /// The connection has no player in the world.
    #[error("no player on this connection")]
    NotInWorld,
    /// The attacker is dead.
    #[error("the attacker is dead")]
    Dead,
    /// No such target here (players are no targets: there is no PvP in M6-A).
    #[error("no such target")]
    UnknownTarget,
    /// The target is dead.
    #[error("the target is dead")]
    TargetDead,
    /// The target walks back to its spawn and takes no damage.
    #[error("the target is returning")]
    Returning,
    /// The attack is not one of the weapon's.
    #[error("not an attack of the weapon")]
    NotWeaponAttack,
    /// The target is out of reach (parent spec section 9: range).
    #[error("out of reach")]
    OutOfReach,
    /// Attacks come faster than the weapon allows (parent spec section 9: attack rate).
    #[error("faster than the weapon allows")]
    TooFast,
    /// No living, not returning mob is in reach (M6-B spec section 6.2).
    #[error("no target in reach")]
    NoTarget,
}

impl AttackRefusal {
    /// The metric label.
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::NotInWorld => "not_in_world",
            Self::Dead => "dead",
            Self::UnknownTarget => "unknown_target",
            Self::TargetDead => "target_dead",
            Self::Returning => "returning",
            Self::NotWeaponAttack => "not_weapon_attack",
            Self::OutOfReach => "out_of_reach",
            Self::TooFast => "too_fast",
            Self::NoTarget => "no_target",
        }
    }
}

/// Why a respawn request did not respawn (M6-B spec section 8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RespawnRefusal {
    /// The connection has no player in the world.
    #[error("no player on this connection")]
    NotInWorld,
    /// The player is alive.
    #[error("the player is not dead")]
    NotDead,
    /// The player died less than `min_respawn_ms` ago.
    #[error("too soon after the death")]
    TooSoon,
}

impl RespawnRefusal {
    /// The metric label.
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::NotInWorld => "not_in_world",
            Self::NotDead => "not_dead",
            Self::TooSoon => "too_soon",
        }
    }
}

/// A hit for the sim that owns the target (M6-A spec section 7.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHit {
    /// The sim that owns the target.
    pub owner: BusId,
    /// The hit.
    pub hit: Hit,
}

/// What a hit did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttackOutcome {
    /// The target lost `amount` of health.
    Hit {
        /// The target.
        target: u64,
        /// Health taken.
        amount: u32,
    },
    /// The hit killed the target.
    Killed {
        /// The target.
        target: u64,
    },
    /// The target belongs to another sim; the hit goes there.
    Remote(RemoteHit),
}

impl AttackOutcome {
    /// The metric label.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Hit { .. } => "hit",
            Self::Killed { .. } => "killed",
            Self::Remote(_) => "remote",
        }
    }
}

/// What combat needs from the configuration and the datasheets.
#[derive(Debug, Clone)]
pub struct CombatSettings {
    /// The combat rows.
    pub data: Arc<CombatData>,
    /// The vitals spawns of the level.
    pub spawns: Arc<[VitalsSpawn]>,
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
    /// Shortest time from a player's death to a respawn on its request.
    pub min_respawn_ms: u64,
    /// Seed of the combat dice.
    pub seed: u64,
    /// The `u32` of a mob's spawn card `AssetId` (M6-B spec section 5.4).
    pub asset_sub_id: u32,
}

/// Hand-made combat rows for tests (feature `test-util`).
#[cfg(any(test, feature = "test-util"))]
pub mod test_support {
    use nw_content::combat::{
        AttackRow, CombatData, CombatRows, CombatSource, LevelRow, MobType, WeaponRow,
    };

    /// The vitals id of the test mob type.
    pub const TEST_MOB: &str = "Test_Boar";
    /// The level of the test spawns.
    pub const TEST_LEVEL: u32 = 20;
    /// The test player's max health.
    pub const TEST_PLAYER_HEALTH: u32 = 856;
    /// Base health of the test level.
    pub const TEST_LEVEL_HEALTH: f64 = 1000.0;
    /// Base damage of the test level.
    pub const TEST_LEVEL_DAMAGE: f64 = 100.0;
    /// Base damage of the test sword.
    pub const TEST_SWORD_DAMAGE: f64 = 64.0;

    /// Rows with the default config ids: one plain mob type, one level, the sword and its
    /// attacks, the player.
    pub fn test_data() -> CombatData {
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
}

#[cfg(test)]
pub(crate) mod tests {
    use super::test_support::test_data;
    use super::*;
    use crate::config::tests::committed;

    /// The committed config's combat settings on the test rows.
    pub(crate) fn test_combat() -> CombatSettings {
        committed().combat_settings(Arc::new(test_data()), Arc::from([]))
    }
}
