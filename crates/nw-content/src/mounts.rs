// SPDX-License-Identifier: AGPL-3.0-or-later
//! The mount rules of the game data (mounts study, run 15:
//! `local-data/scratch-work/run15/mounts/REPORT.md`).
//!
//! - `MountData` (`javelindata_mounts`): each mount (a skin of a mount type), its unlock
//!   achievement or its entitlement and the dye channels it turns off. On the wire a mount is the
//!   CRC-32 of its lowercase `MountId` (Observed: every mount id of the 20 retail mount ledgers is
//!   one of these).
//! - `MountTypeData` (`javelindata_mounttypes`): each type's summon time (retail 5747: 3,000 ms,
//!   the `SummonTimeMs`), its dismount cooldowns (retail: the summon cooldown ended 1,000 ms after
//!   each dismount, voluntary or forced), the achievement that unlocks riding it and the
//!   tradeskill whose rank picks its movement.
//! - `MountMovementData` (`javelindata_mount_movement`): the speeds of each surface, the dash
//!   scalers and the mount stamina (retail own state: regen 10 and drain 20 at the mount-up, the
//!   `ConfigMountRiding_*` rows' `StaminaRegenRate` and `StaminaDrainRate`).
//! - The tradeskill's rank table (`javelindata_tradeskillriding`, unique id `Riding`): the
//!   `MountMovementId` of each rank.
//! - `MountDyeItemDefinitions`: the color indexes a mount dye may use.

use std::collections::{HashMap, HashSet};

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;
use crate::generated::{MountData, MountMovementData, MountTypeData};

/// A dye's mask channels, in wire order.
pub const DYE_CHANNELS: usize = 4;

/// What a mount moves on (client enum `MountMovementSurfaces`, `FUN_143922ba0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountSurface {
    /// A road.
    OnRoad = 0,
    /// Off the roads.
    OffRoad = 1,
    /// Very shallow water.
    InVeryShallowWater = 2,
    /// Shallow water.
    InShallowWater = 3,
    /// Deep water.
    InDeepWater = 4,
}

/// The surfaces, in enum order.
const SURFACES: usize = 5;

/// The speeds of one surface (m/s, m/s²).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SurfaceSpeeds {
    /// `<Surface>MaxSpeed`: the run.
    pub max_speed: f64,
    /// `<Surface>WalkSpeed`: with the walk key.
    pub walk_speed: f64,
    /// `<Surface>SlowWalkSpeed`: with the walk key and the slow walk setting.
    pub slow_walk_speed: f64,
    /// `<Surface>Accel`.
    pub acceleration: f64,
    /// `<Surface>Decel`.
    pub deceleration: f64,
    /// `<Surface>WalkAccel`.
    pub walk_acceleration: f64,
    /// `<Surface>WalkDecel`.
    pub walk_deceleration: f64,
}

/// The mount stamina of one movement row.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MountStaminaRules {
    /// `StaminaMax`.
    pub max: f64,
    /// `StaminaRegenRate`, per second.
    pub regen_rate: f64,
    /// `StaminaDrainRate`, per second of dash.
    pub drain_rate: f64,
    /// `StaminaLightHitThreshold`: a hit of at least this share of the rider's health is light.
    pub light_hit_threshold: f64,
    /// `StaminaLightHitCost`.
    pub light_hit_cost: f64,
    /// `StaminaHeavyHitThreshold`: a hit of at least this share is heavy.
    pub heavy_hit_threshold: f64,
    /// `StaminaHeavyHitCost`.
    pub heavy_hit_cost: f64,
    /// `StaminaHitRegenDelay`, seconds.
    pub hit_regen_delay_s: f64,
}

impl MountStaminaRules {
    /// The stamina a hit of `share` of the rider's health takes: the heavy cost from the heavy
    /// threshold, the light cost from the light one, else none.
    pub fn hit_cost(&self, share: f64) -> f64 {
        if share >= self.heavy_hit_threshold {
            self.heavy_hit_cost
        } else if share >= self.light_hit_threshold {
            self.light_hit_cost
        } else {
            0.0
        }
    }
}

/// One `MountMovementData` row.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MountMovement {
    /// The speeds of each surface, in [`MountSurface`] order.
    pub surfaces: [SurfaceSpeeds; SURFACES],
    /// `DashSpeedScaler`: the dash's share of the run speed.
    pub dash_speed_scale: f64,
    /// `DashAccelScaler`: the dash's share of the acceleration.
    pub dash_acceleration_scale: f64,
    /// The stamina.
    pub stamina: MountStaminaRules,
}

impl MountMovement {
    /// The speeds on `surface`.
    pub fn on(&self, surface: MountSurface) -> SurfaceSpeeds {
        self.surfaces[surface as usize]
    }

    /// The fastest speed of the row: the dash on its fastest surface.
    pub fn top_speed(&self) -> f64 {
        self.surfaces
            .iter()
            .map(|speeds| speeds.max_speed)
            .fold(0.0, f64::max)
            * self.dash_speed_scale
    }

    fn of_row(row: &MountMovementData) -> Self {
        let speeds = |max: f64,
                      walk: f64,
                      slow_walk: f64,
                      accel: f64,
                      decel: f64,
                      walk_accel: f64,
                      walk_decel: f64| SurfaceSpeeds {
            max_speed: max,
            walk_speed: walk,
            slow_walk_speed: slow_walk,
            acceleration: accel,
            deceleration: decel,
            walk_acceleration: walk_accel,
            walk_deceleration: walk_decel,
        };
        Self {
            surfaces: [
                speeds(
                    row.on_road_max_speed,
                    row.on_road_walk_speed,
                    row.on_road_slow_walk_speed,
                    row.on_road_accel as f64,
                    row.on_road_decel as f64,
                    row.on_road_walk_accel as f64,
                    row.on_road_walk_decel as f64,
                ),
                speeds(
                    row.off_road_max_speed,
                    row.off_road_walk_speed,
                    row.off_road_slow_walk_speed as f64,
                    row.off_road_accel,
                    row.off_road_decel as f64,
                    row.off_road_walk_accel,
                    row.off_road_walk_decel as f64,
                ),
                speeds(
                    row.in_very_shallow_water_max_speed,
                    row.in_very_shallow_water_walk_speed,
                    row.in_very_shallow_water_slow_walk_speed,
                    row.in_very_shallow_water_accel,
                    row.in_very_shallow_water_decel,
                    row.in_very_shallow_water_walk_accel,
                    row.in_very_shallow_water_walk_decel,
                ),
                speeds(
                    row.in_shallow_water_max_speed,
                    row.in_shallow_water_walk_speed as f64,
                    row.in_shallow_water_slow_walk_speed,
                    row.in_shallow_water_accel,
                    row.in_shallow_water_decel as f64,
                    row.in_shallow_water_walk_accel,
                    row.in_shallow_water_walk_decel as f64,
                ),
                speeds(
                    row.in_deep_water_max_speed,
                    row.in_deep_water_walk_speed,
                    row.in_deep_water_slow_walk_speed,
                    row.in_deep_water_accel as f64,
                    row.in_deep_water_decel as f64,
                    row.in_deep_water_walk_accel as f64,
                    row.in_deep_water_walk_decel as f64,
                ),
            ],
            dash_speed_scale: row.dash_speed_scaler,
            dash_acceleration_scale: row.dash_accel_scaler,
            stamina: MountStaminaRules {
                max: row.stamina_max as f64,
                regen_rate: row.stamina_regen_rate as f64,
                drain_rate: row.stamina_drain_rate as f64,
                light_hit_threshold: row.stamina_light_hit_threshold,
                light_hit_cost: row.stamina_light_hit_cost as f64,
                heavy_hit_threshold: row.stamina_heavy_hit_threshold,
                heavy_hit_cost: row.stamina_heavy_hit_cost as f64,
                hit_regen_delay_s: row.stamina_hit_regen_delay as f64,
            },
        }
    }
}

/// One mount type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountTypeRule {
    /// `MountID`, for example `Horse`.
    pub id: String,
    /// `SummonTimeMs`.
    pub summon_ms: u32,
    /// `VoluntaryDismountCooldownMs`.
    pub voluntary_dismount_cooldown_ms: u64,
    /// `ForcedDismountCooldownMs`.
    pub forced_dismount_cooldown_ms: u64,
    /// `RequiredAchievementId`: the achievement that unlocks riding the type.
    pub required_achievement: String,
    /// `TradeskillProgressionId`: the tradeskill whose rank picks the movement.
    pub tradeskill: String,
}

impl MountTypeRule {
    fn of_row(row: &MountTypeData) -> Self {
        Self {
            id: row.mount_id.clone(),
            summon_ms: u32::try_from(row.summon_time_ms.max(0)).unwrap_or(u32::MAX),
            voluntary_dismount_cooldown_ms: u64::try_from(row.voluntary_dismount_cooldown_ms)
                .unwrap_or_default(),
            forced_dismount_cooldown_ms: u64::try_from(row.forced_dismount_cooldown_ms)
                .unwrap_or_default(),
            required_achievement: row.required_achievement_id.clone(),
            tradeskill: row.tradeskill_progression_id.clone(),
        }
    }
}

/// One mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountRule {
    /// `MountId`.
    pub id: String,
    /// The CRC-32 of the lowercase id: the mount on the wire.
    pub crc: u32,
    /// `MountType`.
    pub mount_type: String,
    /// `RequiredAchievementId`: the achievement that gives the mount, when it has one.
    pub required_achievement: Option<String>,
    /// `IsEntitlement`: a store entitlement gives the mount.
    pub entitlement: bool,
    /// `SortedOrder`.
    pub sorted_order: i64,
    /// The dye channels the mount turns off, in wire order R, G, B, A
    /// (`RDyeSlotDisabled`, `GDyeSlotDisabled`, `BDyeSlotDisabled`, `ADyeSlotDisabled`;
    /// Observed: a mount that turns a channel off has 0 in that dye byte in every retail dye).
    pub disabled_dye_channels: [bool; DYE_CHANNELS],
}

impl MountRule {
    fn of_row(row: &MountData) -> Self {
        let disabled = |cell: Option<i64>| cell.is_some_and(|value| value != 0);
        Self {
            id: row.mount_id.clone(),
            crc: lowercase_crc32(&row.mount_id),
            mount_type: row.mount_type.clone(),
            required_achievement: row
                .required_achievement_id
                .clone()
                .filter(|id| !id.is_empty()),
            entitlement: row.is_entitlement != 0,
            sorted_order: row.sorted_order,
            disabled_dye_channels: [
                disabled(row.r_dye_slot_disabled),
                disabled(row.g_dye_slot_disabled),
                disabled(row.b_dye_slot_disabled),
                disabled(row.a_dye_slot_disabled),
            ],
        }
    }

    /// Whether `dye` fits the mount: no color on a channel it turns off.
    pub fn takes_dye(&self, dye: [u8; DYE_CHANNELS]) -> bool {
        dye.iter()
            .zip(self.disabled_dye_channels)
            .all(|(color, disabled)| !disabled || *color == 0)
    }
}

/// The mount rules.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MountRules {
    /// Each mount by its wire id.
    mounts: HashMap<u32, MountRule>,
    /// Each mount type by its lowercase id.
    types: HashMap<String, MountTypeRule>,
    /// Each movement row by its lowercase id.
    movements: HashMap<String, MountMovement>,
    /// The `MountMovementId` of each rank of each tradeskill (lowercase tradeskill id), by rank.
    ranks: HashMap<String, Vec<(i64, String)>>,
    /// The color indexes of the mount dyes.
    dye_colors: HashSet<u8>,
}

impl MountRules {
    /// The rules of `tables`; empty when the tables lack them.
    pub fn from_tables(tables: &Tables) -> Self {
        let types: Vec<MountTypeRule> = tables
            .mount_type_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(MountTypeRule::of_row)
            .collect();
        let ranks = types
            .iter()
            .map(|rule| {
                (
                    rule.tradeskill.to_lowercase(),
                    rank_movements(tables, &rule.tradeskill),
                )
            })
            .collect();
        Self::build(
            tables
                .mount_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(MountRule::of_row),
            types,
            tables
                .mount_movement_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| (row.id.clone(), MountMovement::of_row(row))),
            ranks,
            tables
                .mount_dye_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| u8::try_from(row.color_index).ok()),
        )
    }

    /// The rules of `mounts`, `types`, `movements` (id, row), the tradeskill `ranks` (lowercase
    /// tradeskill id to (rank, movement id)) and the dye `colors`.
    pub fn build(
        mounts: impl IntoIterator<Item = MountRule>,
        types: impl IntoIterator<Item = MountTypeRule>,
        movements: impl IntoIterator<Item = (String, MountMovement)>,
        ranks: HashMap<String, Vec<(i64, String)>>,
        colors: impl IntoIterator<Item = u8>,
    ) -> Self {
        let ranks = ranks
            .into_iter()
            .map(|(tradeskill, mut ranks)| {
                ranks.sort_by_key(|(rank, _)| *rank);
                (tradeskill, ranks)
            })
            .collect();
        Self {
            mounts: mounts.into_iter().map(|rule| (rule.crc, rule)).collect(),
            types: types
                .into_iter()
                .map(|rule| (rule.id.to_lowercase(), rule))
                .collect(),
            movements: movements
                .into_iter()
                .map(|(id, movement)| (id.to_lowercase(), movement))
                .collect(),
            ranks,
            dye_colors: colors.into_iter().collect(),
        }
    }

    /// The mount of wire id `crc`.
    pub fn mount(&self, crc: u32) -> Option<&MountRule> {
        self.mounts.get(&crc)
    }

    /// Every mount, by `SortedOrder`, then id.
    pub fn mounts(&self) -> Vec<&MountRule> {
        let mut mounts: Vec<&MountRule> = self.mounts.values().collect();
        mounts.sort_by(|a, b| a.sorted_order.cmp(&b.sorted_order).then(a.id.cmp(&b.id)));
        mounts
    }

    /// The type of `mount`.
    pub fn mount_type(&self, mount: &MountRule) -> Option<&MountTypeRule> {
        self.types.get(&mount.mount_type.to_lowercase())
    }

    /// Every mount type.
    pub fn types(&self) -> impl Iterator<Item = &MountTypeRule> {
        self.types.values()
    }

    /// The movement of a mount of `mount_type` for a rider of tradeskill rank `rank`: the
    /// `MountMovementId` of the highest rank row at or below it.
    pub fn movement(&self, mount_type: &MountTypeRule, rank: u16) -> Option<&MountMovement> {
        let id = self
            .ranks
            .get(&mount_type.tradeskill.to_lowercase())?
            .iter()
            .take_while(|(row, _)| *row <= i64::from(rank))
            .last()
            .map(|(_, id)| id)?;
        self.movements.get(&id.to_lowercase())
    }

    /// Whether every color of `dye` is a mount dye's color (0 is no color).
    pub fn is_dye(&self, dye: [u8; DYE_CHANNELS]) -> bool {
        dye.iter()
            .all(|color| *color == 0 || self.dye_colors.contains(color))
    }
}

/// The (rank, `MountMovementId`) rows of the rank table of `tradeskill` (its unique id).
fn rank_movements(tables: &Tables, tradeskill: &str) -> Vec<(i64, String)> {
    tables
        .tradeskill_rank_data
        .iter()
        .filter(|sheet| sheet.unique_id.eq_ignore_ascii_case(tradeskill))
        .flat_map(|sheet| &sheet.rows)
        .filter_map(|row| Some((row.level, row.mount_movement_id.clone()?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Mount_MTX_5_lion_basic`: its CRC-32 is the mount id of the retail captures.
    const LION_BASIC: &str = "Mount_MTX_5_lion_basic";
    const LION_BASIC_CRC: u32 = 0x0c13_84ae;
    const RIDING: &str = "Riding";
    /// The retail summon time and cooldowns.
    const SUMMON_MS: u32 = 3000;
    const DISMOUNT_COOLDOWN_MS: u64 = 1000;
    /// The `ConfigMountRiding_0` road speeds.
    const ROAD_MAX: f64 = 9.25;
    const DASH: f64 = 1.12;
    /// Dye color indexes (`DyeA025_Mounts`, White).
    const WHITE: u8 = 25;

    fn lion() -> MountRule {
        MountRule {
            id: LION_BASIC.to_owned(),
            crc: lowercase_crc32(LION_BASIC),
            mount_type: "Sabretooth".to_owned(),
            required_achievement: None,
            entitlement: true,
            sorted_order: 3,
            disabled_dye_channels: [true, true, false, true],
        }
    }

    fn rules() -> MountRules {
        let movement = |road: f64| MountMovement {
            surfaces: [SurfaceSpeeds {
                max_speed: road,
                ..SurfaceSpeeds::default()
            }; SURFACES],
            dash_speed_scale: DASH,
            ..MountMovement::default()
        };
        MountRules::build(
            [lion()],
            [MountTypeRule {
                id: "Sabretooth".to_owned(),
                summon_ms: SUMMON_MS,
                voluntary_dismount_cooldown_ms: DISMOUNT_COOLDOWN_MS,
                forced_dismount_cooldown_ms: DISMOUNT_COOLDOWN_MS,
                required_achievement: "MountType_Unlock_Sabretooth".to_owned(),
                tradeskill: RIDING.to_owned(),
            }],
            [
                ("ConfigMountRiding_0".to_owned(), movement(ROAD_MAX)),
                ("ConfigMountRiding_50".to_owned(), movement(ROAD_MAX + 1.0)),
            ],
            HashMap::from([(
                RIDING.to_lowercase(),
                vec![
                    (50, "ConfigMountRiding_50".to_owned()),
                    (0, "ConfigMountRiding_0".to_owned()),
                ],
            )]),
            [WHITE],
        )
    }

    #[test]
    fn a_mount_is_found_by_its_wire_id() {
        assert_eq!(lowercase_crc32(LION_BASIC), LION_BASIC_CRC);
        let rules = rules();
        let mount = rules.mount(LION_BASIC_CRC).expect("lion");
        assert_eq!(mount.id, LION_BASIC);
        let mount_type = rules.mount_type(mount).expect("type");
        assert_eq!(mount_type.summon_ms, SUMMON_MS);
        assert_eq!(rules.mounts().len(), 1);
        assert_eq!(rules.types().count(), 1);
    }

    #[test]
    fn the_riding_rank_picks_the_movement_row() {
        let rules = rules();
        let mount_type = rules.mount_type(&lion()).expect("type").clone();
        let at = |rank| {
            rules
                .movement(&mount_type, rank)
                .expect("row")
                .on(MountSurface::OnRoad)
                .max_speed
        };
        assert_eq!(at(0), ROAD_MAX);
        assert_eq!(at(49), ROAD_MAX);
        assert_eq!(at(50), ROAD_MAX + 1.0);
        assert_eq!(at(250), ROAD_MAX + 1.0);
        let fastest = rules.movement(&mount_type, 50).expect("row").top_speed();
        assert!((fastest - (ROAD_MAX + 1.0) * DASH).abs() < 1e-9);
    }

    #[test]
    fn a_dye_takes_mount_dye_colors_on_open_channels() {
        let rules = rules();
        let lion = lion();
        assert!(rules.is_dye([0, 0, WHITE, 0]));
        assert!(!rules.is_dye([0, 0, WHITE + 1, 0]));
        assert!(lion.takes_dye([0, 0, WHITE, 0]));
        assert!(!lion.takes_dye([WHITE, 0, 0, 0]), "the R channel is off");
    }

    #[test]
    fn a_hit_costs_by_its_share_of_the_health() {
        // `ConfigMountRiding_0`: light from 1 % for 25, heavy from 15 % for 50.
        let stamina = MountStaminaRules {
            light_hit_threshold: 0.01,
            light_hit_cost: 25.0,
            heavy_hit_threshold: 0.15,
            heavy_hit_cost: 50.0,
            ..MountStaminaRules::default()
        };
        assert_eq!(stamina.hit_cost(0.005), 0.0);
        assert_eq!(stamina.hit_cost(0.01), 25.0);
        assert_eq!(stamina.hit_cost(0.2), 50.0);
    }
}
