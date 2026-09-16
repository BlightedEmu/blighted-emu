// SPDX-License-Identifier: AGPL-3.0-or-later
//! `config/sim.toml` with `NW_SIM_` overrides (M4 spec sections 6.3 and 7, M5 spec section 7.1,
//! M6-A spec section 9).

use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use nw_bus::{BusId, MAX_BUS_MESSAGE_BYTES, MAX_GHOST_BYTES};
use nw_common::config::{ConfigError, SECRET_ENV_PREFIX, load_config};
use nw_db::DatabaseConfig;
use std::sync::Arc;

use nw_content::combat::{CombatData, CombatRows};
use nw_messages::LevelSpec;
use nw_sim_core::{BrainSettings, CellBounds, MoveLimits, MoveLimitsError, TICKS_PER_SECOND};
use serde::Deserialize;
use uuid::Uuid;

use crate::combat::CombatSettings;
use crate::mobs::VitalsSpawn;
use crate::world::{MAX_BUNDLE_MEMBERS, WorldSettings};

/// Environment prefix of `sim` overrides.
pub const SIM_ENV_PREFIX: &str = "NW_SIM_";
/// Room in one ghost message for everything but the ghosts (the sender id and the list length).
pub const GHOSTS_HEADER_BYTES: usize = 128;
/// `tick_threads` value for every core of the PC.
pub const ALL_CORES: usize = 0;
/// One tick, rounded up to whole milliseconds: the shortest ghost lifetime.
pub const TICK_MS: u64 = 1000_u64.div_ceil(TICKS_PER_SECOND);

/// Default share of a player's damage that mob armor takes off (nwdb, M6-A spec F7).
pub const DEFAULT_ARMOR_SHARE: f64 = 0.35;
/// Default share of a mob's damage that a player's armor takes off: retail 3601 hits on a new
/// character in the tutorial's starter armor keep 0.6006 of the damage (live capture
/// 2026-09-12, all 2 ship hits and the first island hits).
pub const DEFAULT_PLAYER_ARMOR_SHARE: f64 = 0.3994;
/// Default factor of a mob's attack row.
pub const DEFAULT_MOB_ATTACK_FACTOR: f64 = 1.0;
/// Default quiet time before an idle mob sleeps.
pub const DEFAULT_SLEEP_AFTER_MS: u64 = 30_000;
/// Default sight before the creature type's offset.
pub const DEFAULT_BASE_SIGHT_M: f32 = 20.0;
/// Default wander radius.
pub const DEFAULT_WANDER_RADIUS_M: f32 = 10.0;
/// Default pause between two wander walks.
pub const DEFAULT_WANDER_PAUSE_MS: u64 = 5_000;
/// Default leash radius.
pub const DEFAULT_LEASH_RADIUS_M: f32 = 50.0;
/// Default melee reach.
pub const DEFAULT_ATTACK_REACH_M: f32 = 3.0;
/// Default reach margin for the lag between client and server.
pub const DEFAULT_REACH_MARGIN_M: f32 = 1.0;
/// Default shortest time between two attacks of a player.
pub const DEFAULT_MIN_ATTACK_INTERVAL_MS: u64 = 500;
/// Default time between two hits of a mob.
pub const DEFAULT_MOB_ATTACK_INTERVAL_MS: u64 = 2_000;
/// Default mob speed: the player combat run (M6-A spec F8).
pub const DEFAULT_MOB_SPEED_MPS: f32 = 3.25;
/// Default mob respawn: the one decoded spawner (M6-A spec F6).
pub const DEFAULT_MOB_RESPAWN_MS: u64 = 360_000;
/// Default shortest time from a death to a respawn request: below the lowest captured gap
/// (M6-B spec F7).
pub const DEFAULT_MIN_RESPAWN_MS: u64 = 2_000;
/// Default seed of the combat dice.
pub const DEFAULT_COMBAT_SEED: u64 = 1;
/// Default `u32` of a spawn card's `AssetId` (always 2 per the Azoth catalog field note).
pub const DEFAULT_ASSET_SUB_ID: u32 = 2;

/// Section `[combat]` (M6-A spec section 9).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CombatConfig {
    /// The player row of `VitalsBaseData`.
    pub player_vitals_id: String,
    /// The player weapon row of `WeaponItemDefinitions`.
    pub weapon_id: String,
    /// The `DamageData` rows of the weapon's attacks.
    pub attack_damage_ids: Vec<String>,
    /// Share of a player's damage that mob armor takes off, 0 to below 1.
    pub armor_share: f64,
    /// Share of a mob's damage that player armor takes off, 0 to below 1.
    pub player_armor_share: f64,
    /// Factor of a mob's attack.
    pub mob_attack_factor: f64,
    /// A player this near wakes a mob; absent means `interest_radius_m`.
    pub wake_radius_m: Option<f32>,
    /// Quiet time before an idle mob sleeps.
    pub sleep_after_ms: u64,
    /// Sight before the creature type's offset.
    pub base_sight_m: f32,
    /// Wander radius around the spawn.
    pub wander_radius_m: f32,
    /// Pause between two wander walks.
    pub wander_pause_ms: u64,
    /// Leash radius around the spawn.
    pub leash_radius_m: f32,
    /// Melee reach.
    pub attack_reach_m: f32,
    /// Extra reach allowed for lag.
    pub reach_margin_m: f32,
    /// Shortest time between two attacks of a player.
    pub min_attack_interval_ms: u64,
    /// Time between two hits of a mob.
    pub mob_attack_interval_ms: u64,
    /// Mob speed.
    pub mob_speed_mps: f32,
    /// Time from a mob's death to its respawn.
    pub mob_respawn_ms: u64,
    /// Shortest time from a player's death to a respawn on its request.
    pub min_respawn_ms: u64,
    /// Seed of the combat dice.
    pub seed: u64,
    /// The `u32` of a mob's spawn card `AssetId`.
    pub asset_sub_id: u32,
}

impl Default for CombatConfig {
    fn default() -> Self {
        let rows = CombatRows::default();
        Self {
            player_vitals_id: rows.player_vitals_id,
            weapon_id: rows.weapon_id,
            attack_damage_ids: rows.attack_damage_ids,
            armor_share: DEFAULT_ARMOR_SHARE,
            player_armor_share: DEFAULT_PLAYER_ARMOR_SHARE,
            mob_attack_factor: DEFAULT_MOB_ATTACK_FACTOR,
            wake_radius_m: None,
            sleep_after_ms: DEFAULT_SLEEP_AFTER_MS,
            base_sight_m: DEFAULT_BASE_SIGHT_M,
            wander_radius_m: DEFAULT_WANDER_RADIUS_M,
            wander_pause_ms: DEFAULT_WANDER_PAUSE_MS,
            leash_radius_m: DEFAULT_LEASH_RADIUS_M,
            attack_reach_m: DEFAULT_ATTACK_REACH_M,
            reach_margin_m: DEFAULT_REACH_MARGIN_M,
            min_attack_interval_ms: DEFAULT_MIN_ATTACK_INTERVAL_MS,
            mob_attack_interval_ms: DEFAULT_MOB_ATTACK_INTERVAL_MS,
            mob_speed_mps: DEFAULT_MOB_SPEED_MPS,
            mob_respawn_ms: DEFAULT_MOB_RESPAWN_MS,
            min_respawn_ms: DEFAULT_MIN_RESPAWN_MS,
            seed: DEFAULT_COMBAT_SEED,
            asset_sub_id: DEFAULT_ASSET_SUB_ID,
        }
    }
}

impl CombatConfig {
    /// The datasheet rows of this config.
    pub fn rows(&self) -> CombatRows {
        CombatRows {
            player_vitals_id: self.player_vitals_id.clone(),
            weapon_id: self.weapon_id.clone(),
            attack_damage_ids: self.attack_damage_ids.clone(),
        }
    }

    fn validate(&self, interest_radius_m: f32) -> Result<(), SimConfigError> {
        let invalid = |key, reason| Err(SimConfigError::Invalid { key, reason });
        let positive = |value: f32| value.is_finite() && value > 0.0;
        let distances = [
            ("combat.base_sight_m", self.base_sight_m),
            ("combat.wander_radius_m", self.wander_radius_m),
            ("combat.leash_radius_m", self.leash_radius_m),
            ("combat.attack_reach_m", self.attack_reach_m),
            ("combat.mob_speed_mps", self.mob_speed_mps),
        ];
        if let Some((key, _)) = distances.iter().find(|(_, value)| !positive(*value)) {
            return invalid(key, "must be a finite number above 0");
        }
        if !(self.reach_margin_m.is_finite() && self.reach_margin_m >= 0.0) {
            return invalid(
                "combat.reach_margin_m",
                "must be a finite number of at least 0",
            );
        }
        let times = [
            self.sleep_after_ms,
            self.wander_pause_ms,
            self.min_attack_interval_ms,
            self.mob_attack_interval_ms,
            self.mob_respawn_ms,
            self.min_respawn_ms,
        ];
        if times.contains(&0) {
            return invalid("combat.times", "must be above 0");
        }
        let share = |value: f64| (0.0..1.0).contains(&value);
        if !share(self.armor_share) {
            return invalid("combat.armor_share", "must be at least 0 and below 1");
        }
        if !share(self.player_armor_share) {
            return invalid(
                "combat.player_armor_share",
                "must be at least 0 and below 1",
            );
        }
        if !(self.mob_attack_factor.is_finite() && self.mob_attack_factor >= 0.0) {
            return invalid(
                "combat.mob_attack_factor",
                "must be a finite number of at least 0",
            );
        }
        let wake = self.wake_radius_m.unwrap_or(interest_radius_m);
        if !(wake.is_finite() && wake >= self.base_sight_m) {
            return invalid("combat.wake_radius_m", "must be at least base_sight_m");
        }
        if self.wander_radius_m > self.leash_radius_m {
            return invalid("combat.wander_radius_m", "must be at most leash_radius_m");
        }
        if self.attack_damage_ids.is_empty() {
            return invalid("combat.attack_damage_ids", "must name at least one attack");
        }
        Ok(())
    }
}

/// The whole `sim` configuration.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimConfig {
    /// This sim's id in bus subjects; unique in the world.
    pub sim_id: BusId,
    /// The world, the same `[[world]] id` as in `config/edge.toml`.
    pub world_id: Uuid,
    /// NATS, internal only.
    pub nats_url: String,
    /// The world database.
    pub world_database: DatabaseConfig,
    /// The environment variable with the world database password; it starts with `NW_SECRET_`.
    pub world_database_password_env: String,
    /// The pinned client build file.
    pub client_build_config: PathBuf,
    /// Folder of the world files (`world-<build>-<level>.bin`).
    pub content_dir: PathBuf,
    /// The open map level; the same as the director's.
    pub level: String,
    /// Most players in this sim.
    pub max_players: u32,
    /// Players see each other inside this radius, in metres.
    pub interest_radius_m: f32,
    /// Most other players and ghosts in a player's bundles: the nearest.
    pub max_visible_players: usize,
    /// How far a position may be below the ground, in metres.
    pub ground_tolerance_m: f32,
    /// How far a position may be above the ground, in metres.
    pub max_height_above_ground_m: f32,
    /// The run speed of the datasheets (`ServerValidatedCanRun`), in metres per second.
    pub run_speed_mps: f32,
    /// How much faster than `run_speed_mps` a move may be, as a share (1.2 is 120 % more).
    pub speed_margin: f32,
    /// Longest burst of movement that the distance budget holds, in seconds.
    pub burst_seconds: f32,
    /// How far into another sim's cells a player goes before its handoff starts, in metres.
    pub handoff_margin_m: f32,
    /// Longest wait for the answer to a handoff.
    pub handoff_timeout_ms: u64,
    /// Wait after a refused or late handoff, and after a take, before the next handoff.
    pub handoff_retry_ms: u64,
    /// How long inputs of a handed-off connection go on to the new sim.
    pub forward_ttl_ms: u64,
    /// Ghosts of another sim not renewed for this long are dropped.
    pub ghost_ttl_ms: u64,
    /// Threads of the bundle step; 0 means every core of the PC.
    pub tick_threads: usize,
    /// Time between position saves.
    pub save_interval_ms: u64,
    /// Time between heartbeats.
    pub heartbeat_interval_ms: u64,
    /// Missed gateway heartbeats before its players leave.
    pub heartbeat_misses: u32,
    /// Internal Prometheus listener.
    pub metrics_listen: SocketAddr,
    /// `tracing` filter, for example `info`.
    pub log_filter: String,
    /// Mobs and combat.
    #[serde(default)]
    pub combat: CombatConfig,
    /// The admin tool.
    #[serde(default)]
    pub admin: AdminSection,
}

/// `[admin]` (admin tools spec section 5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminSection {
    /// Time between two admin snapshots on the bus; 0 is off, and then the sim does no snapshot
    /// work.
    pub snapshot_interval_ms: u64,
}

/// Shortest snapshot interval that is not off.
pub const MIN_SNAPSHOT_INTERVAL_MS: u64 = 100;

/// A value of the configuration that is not allowed.
#[derive(Debug, thiserror::Error)]
pub enum SimConfigError {
    /// The file cannot be read or has a wrong key or type.
    #[error(transparent)]
    Load(#[from] ConfigError),
    /// A value is not allowed.
    #[error("config value `{key}` is not valid: {reason}")]
    Invalid {
        /// Key path.
        key: &'static str,
        /// Why the value is refused.
        reason: &'static str,
    },
}

/// Reads and checks the configuration.
///
/// # Errors
///
/// Returns [`SimConfigError`] when the file is missing, has a wrong key, or a value is not
/// allowed.
pub fn load_sim_config(path: &Path) -> Result<SimConfig, SimConfigError> {
    let config: SimConfig = load_config(path, SIM_ENV_PREFIX)?;
    config.validate()?;
    Ok(config)
}

impl SimConfig {
    fn validate(&self) -> Result<(), SimConfigError> {
        let invalid = |key, reason| Err(SimConfigError::Invalid { key, reason });
        if !self
            .world_database_password_env
            .starts_with(SECRET_ENV_PREFIX)
        {
            return invalid("world_database_password_env", "must start with NW_SECRET_");
        }
        if self.level.is_empty() {
            return invalid("level", "must not be empty");
        }
        let ghosts_fit = (self.max_players as usize)
            .checked_mul(MAX_GHOST_BYTES)
            .is_some_and(|bytes| bytes + GHOSTS_HEADER_BYTES <= MAX_BUS_MESSAGE_BYTES);
        if self.max_players == 0 || !ghosts_fit {
            return invalid(
                "max_players",
                "must be above 0, and its ghosts must fit one bus message",
            );
        }
        if !(self.handoff_margin_m.is_finite() && self.handoff_margin_m >= 0.0) {
            return invalid("handoff_margin_m", "must be a finite number of at least 0");
        }
        let times = [
            self.handoff_timeout_ms,
            self.handoff_retry_ms,
            self.forward_ttl_ms,
            self.ghost_ttl_ms,
        ];
        if times.contains(&0) {
            return invalid("handoff", "times must be above 0");
        }
        if self.max_visible_players >= MAX_BUNDLE_MEMBERS {
            return invalid(
                "max_visible_players",
                "must leave room for the player itself in one bundle (at most 47)",
            );
        }
        let snapshot = self.admin.snapshot_interval_ms;
        if snapshot != 0 && snapshot < MIN_SNAPSHOT_INTERVAL_MS {
            return invalid(
                "admin.snapshot_interval_ms",
                "must be 0 (off) or at least 100",
            );
        }
        if self.ghost_ttl_ms < TICK_MS {
            return invalid("ghost_ttl_ms", "must be at least one tick (34 ms)");
        }
        if self.forward_ttl_ms <= self.handoff_timeout_ms {
            return invalid("forward_ttl_ms", "must be longer than handoff_timeout_ms");
        }
        self.combat.validate(self.interest_radius_m)?;
        if !(self.interest_radius_m.is_finite() && self.interest_radius_m > 0.0) {
            return invalid("interest_radius_m", "must be a finite number above 0");
        }
        let band = [self.ground_tolerance_m, self.max_height_above_ground_m];
        if band
            .iter()
            .any(|value| !(value.is_finite() && *value >= 0.0))
        {
            return invalid("ground", "tolerances must be finite numbers of at least 0");
        }
        if self.save_interval_ms == 0 || self.heartbeat_interval_ms == 0 {
            return invalid("intervals", "must be above 0");
        }
        if self.heartbeat_misses == 0 {
            return invalid("heartbeat_misses", "must be above 0");
        }
        self.move_limits()
            .map_err(|error| SimConfigError::Invalid {
                key: error.name,
                reason: error.rule,
            })?;
        Ok(())
    }

    /// The settings of the world state on a level with the edges `bounds` and its sea at
    /// `ocean_level_m`.
    ///
    /// # Errors
    ///
    /// Returns [`SimConfigError::Invalid`] for a movement limit that is not usable.
    pub fn world_settings(
        &self,
        bounds: CellBounds,
        ocean_level_m: f32,
    ) -> Result<WorldSettings, SimConfigError> {
        let limits = self
            .move_limits()
            .map_err(|error| SimConfigError::Invalid {
                key: error.name,
                reason: error.rule,
            })?;
        Ok(WorldSettings {
            interest_radius_m: self.interest_radius_m,
            max_visible_players: self.max_visible_players,
            max_players: self.max_players,
            limits,
            bounds,
            handoff_margin_m: self.handoff_margin_m,
            handoff_retry_ms: self.handoff_retry_ms,
            ghost_ttl_ms: self.ghost_ttl_ms,
            tick_threads: self.tick_threads(),
            local_player_card: crate::world::local_player_card(),
            level: LevelSpec::of_folder(&self.level).ok_or(SimConfigError::Invalid {
                key: "level",
                reason: "not a level this server can name to the client",
            })?,
            ocean_level_m,
        })
    }

    /// The numbers of the mob brain.
    pub fn brain_settings(&self) -> BrainSettings {
        let combat = &self.combat;
        BrainSettings {
            wake_radius_m: combat.wake_radius_m.unwrap_or(self.interest_radius_m),
            sleep_after_ms: combat.sleep_after_ms,
            base_sight_m: combat.base_sight_m,
            wander_radius_m: combat.wander_radius_m,
            wander_pause_ms: combat.wander_pause_ms,
            leash_radius_m: combat.leash_radius_m,
            attack_reach_m: combat.attack_reach_m,
            mob_attack_interval_ms: combat.mob_attack_interval_ms,
            mob_speed_mps: combat.mob_speed_mps,
            mob_respawn_ms: combat.mob_respawn_ms,
        }
    }

    /// The combat settings on the rows of `data` with the vitals `spawns` of the level.
    pub fn combat_settings(
        &self,
        data: Arc<CombatData>,
        spawns: Arc<[VitalsSpawn]>,
    ) -> CombatSettings {
        let combat = &self.combat;
        CombatSettings {
            data,
            spawns,
            brain: self.brain_settings(),
            armor_share: combat.armor_share,
            player_armor_share: combat.player_armor_share,
            mob_attack_factor: combat.mob_attack_factor,
            reach_margin_m: combat.reach_margin_m,
            min_attack_interval_ms: combat.min_attack_interval_ms,
            min_respawn_ms: combat.min_respawn_ms,
            seed: combat.seed,
            asset_sub_id: combat.asset_sub_id,
        }
    }

    /// Threads of the bundle step: `tick_threads`, or every core when it is 0.
    pub fn tick_threads(&self) -> usize {
        if self.tick_threads == ALL_CORES {
            std::thread::available_parallelism().map_or(1, NonZeroUsize::get)
        } else {
            self.tick_threads
        }
    }

    /// The movement limits (M4 spec section 7.2 with sections 15 and 16).
    ///
    /// # Errors
    ///
    /// Returns [`MoveLimitsError`] for a value that is not usable.
    pub fn move_limits(&self) -> Result<MoveLimits, MoveLimitsError> {
        MoveLimits::new(
            self.run_speed_mps,
            self.speed_margin,
            self.burst_seconds,
            self.ground_tolerance_m,
            self.max_height_above_ground_m,
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const COMMITTED_CONFIG: &str = "../../config/sim.toml";
    /// Sprint (5.5 m/s) with the 15 % road boost (movement numbers note, Check section).
    const BOOSTED_SPRINT_MPS: f32 = 6.325;
    /// The longest gap between two captured movement messages (movement numbers note, section 3).
    const LONGEST_HONEST_GAP_S: f32 = 2.5;
    /// The largest captured fall (movement numbers note, Check section).
    const LARGEST_CAPTURED_FALL_M: f32 = 128.6;
    /// A bundle holds the player and 47 others (M5 spec section 7.3).
    const ONE_BUNDLE_OF_OTHERS: usize = 47;

    /// The committed example configuration.
    pub(crate) fn committed() -> SimConfig {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(COMMITTED_CONFIG);
        load_sim_config(&path).expect("committed config")
    }

    #[test]
    fn the_committed_config_is_valid() {
        let config = committed();
        assert_eq!(config.level, "newworld_vitaeeterna");
        assert_eq!(config.max_visible_players, ONE_BUNDLE_OF_OTHERS);
        assert!(config.max_height_above_ground_m > LARGEST_CAPTURED_FALL_M);
    }

    #[test]
    fn the_committed_config_has_no_snapshot() {
        assert_eq!(committed().admin.snapshot_interval_ms, 0);
    }

    #[test]
    fn a_snapshot_interval_below_100_ms_is_refused() {
        for (interval, ok) in [(0, true), (1, false), (99, false), (100, true)] {
            let mut config = committed();
            config.admin.snapshot_interval_ms = interval;
            assert_eq!(config.validate().is_ok(), ok, "{interval} ms");
        }
    }

    #[test]
    fn bad_values_are_refused() {
        let mut config = committed();
        config.world_database_password_env = "WORLD_PASSWORD".to_owned();
        assert!(config.validate().is_err());
        let mut config = committed();
        config.interest_radius_m = f32::NAN;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.heartbeat_misses = 0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.speed_margin = -1.0;
        assert!(matches!(
            config.validate(),
            Err(SimConfigError::Invalid {
                key: "speed_margin",
                ..
            })
        ));
        let mut config = committed();
        config.max_players = 0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.max_players = u32::MAX;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.handoff_margin_m = -1.0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.handoff_timeout_ms = 0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.max_visible_players = MAX_BUNDLE_MEMBERS;
        assert!(
            config.validate().is_err(),
            "the player and the others fill one bundle"
        );
        let mut config = committed();
        config.ghost_ttl_ms = 1;
        assert!(config.validate().is_err(), "ghosts live at least one tick");
        let mut config = committed();
        config.forward_ttl_ms = config.handoff_timeout_ms;
        assert!(config.validate().is_err(), "forwards outlive a handoff");
    }

    #[test]
    fn the_combat_section_has_its_defaults() {
        let config = committed();
        assert_eq!(config.combat, CombatConfig::default());
        assert_eq!(
            config.combat.rows(),
            nw_content::combat::CombatRows::default()
        );
        let brain = config.brain_settings();
        assert_eq!(
            brain.wake_radius_m, config.interest_radius_m,
            "wake defaults to interest"
        );
        assert_eq!(brain.leash_radius_m, DEFAULT_LEASH_RADIUS_M);
        let mut set = committed();
        set.combat.wake_radius_m = Some(DEFAULT_BASE_SIGHT_M * 2.0);
        assert_eq!(
            set.brain_settings().wake_radius_m,
            DEFAULT_BASE_SIGHT_M * 2.0
        );
    }

    /// A config key and the fault that must make the check refuse it.
    type CombatFault = (&'static str, fn(&mut CombatConfig));

    #[test]
    fn bad_combat_values_are_refused() {
        let cases: [CombatFault; 8] = [
            ("combat.leash_radius_m", |c| c.leash_radius_m = -1.0),
            ("combat.attack_reach_m", |c| c.attack_reach_m = 0.0),
            ("combat.mob_speed_mps", |c| c.mob_speed_mps = f32::NAN),
            ("combat.times", |c| c.mob_respawn_ms = 0),
            ("combat.armor_share", |c| c.armor_share = 1.5),
            ("combat.player_armor_share", |c| c.player_armor_share = -0.1),
            ("combat.wake_radius_m", |c| {
                c.wake_radius_m = Some(DEFAULT_BASE_SIGHT_M - 1.0)
            }),
            ("combat.wander_radius_m", |c| {
                c.wander_radius_m = DEFAULT_LEASH_RADIUS_M + 1.0
            }),
        ];
        for (key, fault) in cases {
            let mut config = committed();
            fault(&mut config.combat);
            assert!(
                matches!(config.validate(), Err(SimConfigError::Invalid { key: found, .. }) if found == key),
                "{key}: {:?}",
                config.validate()
            );
        }
    }

    #[test]
    fn zero_tick_threads_means_every_core() {
        let mut config = committed();
        config.tick_threads = ALL_CORES;
        assert!(config.tick_threads() >= 1);
        config.tick_threads = 3;
        assert_eq!(config.tick_threads(), 3);
    }

    #[test]
    fn the_speed_limit_is_above_boosted_sprint() {
        let limits = committed().move_limits().expect("limits");
        assert!(limits.max_speed_mps > BOOSTED_SPRINT_MPS);
        assert!(committed().burst_seconds > LONGEST_HONEST_GAP_S);
    }
}
