// SPDX-License-Identifier: AGPL-3.0-or-later
//! The forts of the territory wars (wars and sieges study 2026-11-12, findings
//! `2026-11-12-wars-and-sieges.md`, section "The framed instance and the war fight"): for each
//! fort that a retail war capture shows, the frame of its war instance, where the players of
//! each side arrived, the siege data entity and the four capture points with their spawn cards.
//!
//! Observed (the `warforts` bin of the capture tools over every retail ledger with the siege
//! warfare data, 4103): a war runs in an instance of the open world level whose 1635 names
//! `NewWorld_VitaeEterna` with a 512 m frame and a private phase (`arvaysa_ES_attacker_war_45vs45`
//! 1783536302144, frame 5632, 5120, 512, 512). Each of the six forts of the war captures
//! (territories 4, 8, 10, 11, 15, 17) has its frame at the 512 m grid square that holds its claim
//! point, and its claim point (number [`CLAIM_POINT_NUMBER`]) at the territory's claim position.
//! Inside the instance retail spawns a group with the siege data (card and 4103) and, for each
//! capture point, a group of its card, its place, its ownership and its 333 state; the 4103 keys
//! a point by the first eight bytes of its `GdeRef` read as a little-endian `u64`
//! ([`FortPoint::key`]), and the point's 4062 names the point's facet id and that key.
//!
//! The table file is mined from the captures (no tracked file holds capture data): a fort that no
//! capture shows has no entry, and it gets no war instance. The invasion forts (territories 2, 5
//! and 16) showed a frame but no capture point in the instance: their entries have no point
//! ([`WarFort::is_invasion_fort`]), and only an invasion runs there.
//!
//! **Turrets** (turret study 2026-11-12): each fort lists the turrets its war capture showed in
//! the frame ([`FortStructure`]: territory 4 13, 8 18, 10 13, 11 12 repeaters, 15 three cannons, 17
//! none seen), each with the members of its first state as retail sent them; the table's turret
//! kinds ([`TurretKind`]) hold the values of each turret slice (`slices/structures/siege/`: the
//! `TurretComponent` and `ProjectileSpawnerComponent` fields, the ranged attack database's range
//! and radius of the spawner's attack, the ammo of its `AddAmmo` and the health of its `AddHealth`
//! interaction option) and the facet rule of its turret and spawner components ([`FacetRule`],
//! Observed on the four turrets whose calls the captures hold).
//!
//! **Siege structures** (siege structure study 2026-11-12): the fort's list holds every siege
//! structure retail showed in the frame ([`FortStructure`]: the turrets, the fire barrel droppers,
//! the horns of resilience and the supply generators), and the table's structure kinds
//! ([`StructureKind`]) every defensive structure and generator slice of the asset catalog with
//! the first group retail sent of it in any ledger (its template for a new build; Observed: the
//! T3 ballista and explosive launcher only in `fort-equipements-territory`, the T1 repeater and the
//! T1 and T2 explosive launchers in no ledger). The build costs and the supply generator recipes
//! come from the data tables ([`SiegeBlueprints`]).
//!
//! **Outpost Rush vats** (game mode helper B16, 2026-11-13): the burning oil vats of the Outpost
//! Rush layouts (`or_turretfire`, `or_turretfire_trench`) are structure kinds too, T1 fire
//! barrel droppers with the use, drop and area rows of their slices (their barrel and area are
//! the war T1 slices); they are outpost structures, never a war build
//! ([`StructureKind::is_war_siege`]).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::Tables;

const FILE_PREFIX: &str = "war-forts-";
const FILE_SUFFIX: &str = ".json";
/// The bytes of an asset id and of a `GdeRef`.
pub const ID_LEN: usize = 16;
/// The bytes of a capture point's key: the first part of its `GdeRef`.
const KEY_LEN: usize = 8;
/// The number of a fort's claim point (Observed: point 4 is at the claim position in all six
/// forts, and its capture ended the ES war).
pub const CLAIM_POINT_NUMBER: u32 = 4;
/// The war side of an attacker and of a defender (the 2762 side).
pub const SIDE_ATTACKER: u8 = 1;
/// The war side of a defender.
pub const SIDE_DEFENDER: u8 = 2;

/// The table file of client build `client_build` in content directory `dir`.
pub fn war_forts_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum WarFortsError {
    /// The file cannot be read.
    #[error("war forts table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("war forts table: {0}")]
    Json(#[from] serde_json::Error),
    /// A hex field does not read.
    #[error("war forts table: territory {territory}: {source}")]
    Hex {
        /// The fort's territory.
        territory: u16,
        /// Why.
        source: hex::FromHexError,
    },
    /// An id or a facet has the wrong length.
    #[error("war forts table: territory {territory}: a field of {len} bytes")]
    Len {
        /// The fort's territory.
        territory: u16,
        /// The bytes it has.
        len: usize,
    },
    /// A fort has no claim point.
    #[error("war forts table: territory {territory} has no claim point")]
    NoClaimPoint {
        /// The fort's territory.
        territory: u16,
    },
}

#[derive(Deserialize)]
struct TableFile {
    forts: Vec<FortRow>,
    #[serde(default)]
    turret_kinds: Vec<KindRow>,
    #[serde(default)]
    structure_kinds: Vec<StructureKindRow>,
    #[serde(default)]
    level_droppers: Vec<LevelDropperRow>,
    #[serde(default)]
    encounter_turrets: Vec<StructureRow>,
}

#[derive(Deserialize)]
struct LevelDropperRow {
    capital: String,
    #[serde(default)]
    members_inferred: bool,
    structure: StructureRow,
    kind: StructureKindRow,
}

#[derive(Deserialize)]
struct StructureRow {
    asset: String,
    gde_ref: String,
    #[serde(default)]
    place: [f32; 3],
    #[serde(default)]
    yaw: f32,
    #[serde(default)]
    side: Option<u8>,
    card_component: u64,
    #[serde(default)]
    turret_component: Option<u64>,
    interact_component: Option<u64>,
    spawner_component: Option<u64>,
    ownership_component: Option<u64>,
    #[serde(default)]
    generator_component: Option<u64>,
    #[serde(default)]
    container_component: Option<u64>,
    members: Vec<String>,
}

#[derive(Deserialize)]
struct StructureKindRow {
    asset: String,
    slice: String,
    template: Option<StructureRow>,
    #[serde(default)]
    template_inferred: bool,
    #[serde(default)]
    trigger: Option<FacetRow>,
    #[serde(default)]
    use_timers: Option<Vec<TimerRow>>,
    #[serde(default)]
    drop: Option<DropRow>,
    #[serde(default)]
    area: Option<AreaRow>,
    #[serde(default)]
    use_interact: Option<u64>,
    #[serde(default)]
    gate: Option<GateRow>,
}

#[derive(Deserialize)]
struct GateRow {
    physical_radius_m: Option<f32>,
    repair_delay: Option<TimerRow>,
    #[serde(default)]
    door_interact: Option<u64>,
}

#[derive(Deserialize)]
struct TimerRow {
    component: u64,
    delay_s: f32,
    /// Older tables have no flag: every timer they hold starts at once.
    #[serde(default = "starts_at_once")]
    on_open: bool,
}

/// The start flag of a timer row of a table that names none.
const fn starts_at_once() -> bool {
    true
}

#[derive(Deserialize)]
struct DamageRow {
    table: String,
    row: String,
    base_damage: u32,
    rate_s: f32,
}

#[derive(Deserialize)]
struct AreaRow {
    asset: String,
    ownership_component: Option<u64>,
    position_component: Option<u64>,
    timers: Vec<TimerRow>,
    radius_m: Option<f32>,
    once: bool,
    damage: Option<DamageRow>,
    effects: Vec<String>,
}

#[derive(Deserialize)]
struct DropRow {
    projectile: ProjectileRow,
    ownership_component: Option<u64>,
    gravity_mps2: f32,
    spawner_facet: FacetRow,
    pour: TimerRow,
}

#[derive(Deserialize)]
struct FacetRow {
    xor: String,
    component: String,
}

#[derive(Deserialize)]
struct ProjectileRow {
    asset: String,
    component: u64,
    ranged_attack: u64,
    damage_table_crc: u32,
    row_index: u16,
}

#[derive(Deserialize)]
struct KindRow {
    asset: String,
    slice: String,
    hold_to_fire: bool,
    repeat_fire_delay_s: f32,
    fire_speed_mps: f32,
    yaw_max_deg: f32,
    pitch_min_deg: f32,
    pitch_max_deg: f32,
    heat_capacity: f32,
    heat_gain_per_s: f32,
    heat_loss_per_s: f32,
    heat_loss_delay_s: f32,
    fire_cooldown_s: f32,
    gravity_mps2: f32,
    uses_ammo: bool,
    max_ammo: u16,
    #[serde(default)]
    ammo_refill: Option<u16>,
    repair_health: f32,
    damage_table: String,
    damage_row: String,
    range_m: f32,
    radius_m: f32,
    turret_facet: FacetRow,
    spawner_facet: FacetRow,
    projectile: Option<ProjectileRow>,
    #[serde(default)]
    area: Option<AreaRow>,
    #[serde(default)]
    ammo_effect: Option<String>,
}

#[derive(Deserialize)]
struct FortRow {
    territory: u16,
    frame: [f32; 4],
    arrivals: Vec<ArrivalRow>,
    siege: SiegeRow,
    points: Vec<PointRow>,
    #[serde(default)]
    structures: Vec<StructureRow>,
    #[serde(default)]
    major_structures: Vec<MajorRow>,
}

#[derive(Deserialize)]
struct MajorRow {
    gde_ref: String,
    kind: u32,
}

#[derive(Deserialize)]
struct ArrivalRow {
    side: u8,
    position: [f32; 3],
}

#[derive(Deserialize)]
struct SiegeRow {
    asset: String,
    gde_ref: String,
    card_component: u64,
    state_component: u64,
}

#[derive(Deserialize)]
struct PointRow {
    number: u32,
    asset: String,
    gde_ref: String,
    facet: String,
    place: [f32; 3],
    kind: u8,
    position: String,
    card_component: u64,
    position_component: u64,
    ownership_component: u64,
    point_component: u64,
}

/// Where a player of a side arrived in a war instance (Observed: the capture player's first place
/// inside the frame after the instance's 1635).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrival {
    /// [`SIDE_ATTACKER`] or [`SIDE_DEFENDER`].
    pub side: u8,
    /// Metres: east, north, up.
    pub position: [f32; 3],
}

/// The entity of a fort's siege warfare data (4103).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SiegeEntity {
    /// Its slice asset.
    pub asset: [u8; ID_LEN],
    /// Its `GdeRef`.
    pub gde_ref: [u8; ID_LEN],
    /// The component of its card.
    pub card_component: u64,
    /// The component of its 4103 state.
    pub state_component: u64,
}

/// One capture point of a fort.
#[derive(Debug, Clone, PartialEq)]
pub struct FortPoint {
    /// Its number: 1 to 3 the outer points, [`CLAIM_POINT_NUMBER`] the claim point.
    pub number: u32,
    /// Its slice asset.
    pub asset: [u8; ID_LEN],
    /// Its `GdeRef`.
    pub gde_ref: [u8; ID_LEN],
    /// The facet id of its 4062 and 4063.
    pub facet: u64,
    /// Its place (the 333 cold data's).
    pub place: [f32; 3],
    /// Its kind in its first 333 state of the instance.
    pub kind: u8,
    /// Its place state as one bundle member's bytes (component, class, body), as retail sent it.
    pub position: Vec<u8>,
    /// The component of its card.
    pub card_component: u64,
    /// The component of its place state (in `position`).
    pub position_component: u64,
    /// The component of its ownership state.
    pub ownership_component: u64,
    /// The component of its 333 state.
    pub point_component: u64,
}

impl FortPoint {
    /// The key of the point in the siege warfare data and in its 4062: the first eight bytes of
    /// its `GdeRef` as a little-endian `u64` (Observed in the six forts).
    pub fn key(&self) -> u64 {
        let mut key = [0; KEY_LEN];
        key.copy_from_slice(&self.gde_ref[..KEY_LEN]);
        u64::from_le_bytes(key)
    }

    /// Whether this is the fort's claim point.
    pub const fn is_claim(&self) -> bool {
        self.number == CLAIM_POINT_NUMBER
    }
}

/// One fort.
#[derive(Debug, Clone, PartialEq)]
pub struct WarFort {
    /// The territory id.
    pub territory: u16,
    /// The frame of its war instance: x, y, width, height.
    pub frame: [f32; 4],
    /// Where the players arrived, by side.
    pub arrivals: Vec<Arrival>,
    /// Its siege warfare data entity.
    pub siege: SiegeEntity,
    /// Its capture points, by number.
    pub points: Vec<FortPoint>,
    /// Its siege structures: turrets, droppers, horns, supply generators, fort gates and war
    /// camp gates.
    pub structures: Vec<FortStructure>,
    /// Its major structures: the entries of the siege data's `fortMajorStructureStates`.
    pub major_structures: Vec<MajorStructure>,
}

/// A major structure of a fort (4103 child 2, `fortMajorStructureStates`): a fort gate.
///
/// Observed (`RS_offensive_war`, the five fort gates of territory 15): each entry is keyed by
/// its gate's facet entity ([`FortStructure::facet_entity`]), with the gate's place, a kind of
/// its own (5 to 9, one per gate, not by size: Unknown meaning, mined per gate), its health in
/// percent (100, 85, 66, 43, 32, 0 as the gates fell) and a byte that is the gate's buildable
/// state (5 whole, 6 broken: the `l_t2` gate's entry went to 0 and 6 when it broke, 933.736 s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MajorStructure {
    /// The `GdeRef` of its gate.
    pub gde_ref: [u8; ID_LEN],
    /// Its kind in the siege data.
    pub kind: u32,
}

/// One siege structure of a fort, as its war capture showed it first (or, as the template of a
/// [`StructureKind`], as retail first sent its slice anywhere: then its place, facing and side
/// are not used).
#[derive(Debug, Clone, PartialEq)]
pub struct FortStructure {
    /// Its slice asset (a [`StructureKind`]; a turret's is also a [`TurretKind`]).
    pub asset: [u8; ID_LEN],
    /// Its `GdeRef`.
    pub gde_ref: [u8; ID_LEN],
    /// Its place, metres.
    pub place: [f32; 3],
    /// Its facing about the up axis, radians (its place state's rotation; yaw 0 faces +y and
    /// grows toward -x).
    pub yaw: f32,
    /// The war side whose company owns it ([`SIDE_ATTACKER`] or [`SIDE_DEFENDER`]; Observed:
    /// its ownership's company is the war entry's attacker or defender company: the 56
    /// repeaters of the fort walls the defender's, the three cannons of `RS_offensive_war` the
    /// attacker's). Inferred: a structure of no side's company (a template) is the defender's.
    pub side: u8,
    /// The component of its card.
    pub card_component: u64,
    /// The component of its turret state (4276), for a turret.
    pub turret_component: Option<u64>,
    /// The component of its interact state (2930).
    pub interact_component: Option<u64>,
    /// The component of its projectile spawner state (57).
    pub spawner_component: Option<u64>,
    /// The component of its ownership state (3217).
    pub ownership_component: Option<u64>,
    /// The component of its item generation state (3002), for a supply generator.
    pub generator_component: Option<u64>,
    /// The component of its container state (1755).
    pub container_component: Option<u64>,
    /// Every other member of its first state (place, vitals, damage receiver, buildable
    /// controller, status effects, container, placement obstruction, delayed events, its turret,
    /// interact and projectile spawner states), each as one bundle member's bytes, as retail sent
    /// it.
    pub members: Vec<Vec<u8>>,
}

impl FortStructure {
    /// Its facet entity: the first eight bytes of its `GdeRef` read as a little-endian `u64`
    /// (Observed: the 2720 and 327 headers of the four turrets the captures used).
    pub fn facet_entity(&self) -> u64 {
        let mut key = [0; KEY_LEN];
        key.copy_from_slice(&self.gde_ref[..KEY_LEN]);
        u64::from_le_bytes(key)
    }
}

/// How the facet target of a component of a slice follows from the entity: the entity XOR the
/// slice's main entity id XOR the component's entity id, plus the component's id (Observed: the
/// 4273, 4145, 2720 and 327 targets of the four turrets the captures used).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FacetRule {
    /// The main entity id XOR the component's entity id.
    pub xor: u64,
    /// The component's id.
    pub component: u64,
}

impl FacetRule {
    /// The facet target of the component on the entity whose facet entity is `entity`.
    pub const fn target(&self, entity: u64) -> u64 {
        (entity ^ self.xor).wrapping_add(self.component)
    }
}

/// The projectile entity of a turret shot (Observed: the 327 projectiles of `RS_offensive_war`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurretProjectile {
    /// Its slice asset.
    pub asset: [u8; ID_LEN],
    /// The component of its projectile state (class 16).
    pub component: u64,
    /// `rangedAttackIndex`.
    pub ranged_attack: u64,
    /// `damageTableId`.
    pub damage_table_crc: u32,
    /// `damageTableRowIndex`.
    pub row_index: u16,
}

/// A timer of a siege slice: a `DelayedEvent` component (class 3139) and the delay it counts
/// once it starts (the slice's value; Observed equal to the `total_delay_s` retail sent when
/// the dropper's pour and the horn's blow started, `joining_defending_war_2` 124.741 s and
/// `invasion` 95.691 s).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SiegeTimer {
    /// Its component.
    pub component: u64,
    /// Its delay, seconds.
    pub delay_s: f32,
    /// It starts when its entity opens (`m_startDelayOnInit`); else a trigger of its slice
    /// starts it later, and its entity opens with it at 0 (Observed: the cow launcher's area of
    /// `invasion` 335.079 s opened with its three start-on-init timers at 3.8, 0.2 and 4.0 s and
    /// its three start-on-trigger timers, `DestroyDelay` 2 s, `Eggsplode` 3 s and `Die_Eggsplode`
    /// 3 s, at 0, and the last two started at its death, 338.315 s; every timer of the dropper and
    /// horn areas starts on init).
    pub on_open: bool,
}

impl SiegeTimer {
    /// Its delay in whole milliseconds.
    pub fn delay_ms(&self) -> u64 {
        seconds_ms(self.delay_s)
    }

    /// The delay its entity shows when it opens, seconds: its delay when it starts then, else 0
    /// ([`Self::on_open`]).
    pub const fn open_delay_s(&self) -> f32 {
        if self.on_open { self.delay_s } else { 0.0 }
    }
}

/// Whole milliseconds of `seconds` (0 for a negative or not finite value).
fn seconds_ms(seconds: f32) -> u64 {
    let ms = f64::from(seconds) * MS_PER_SECOND_F64;
    if ms.is_finite() && ms > 0.0 {
        // A siege delay of at most minutes.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a delay of minutes"
        )]
        let whole = ms.round() as u64;
        whole
    } else {
        0
    }
}

/// The damage of a siege area: its slice's `m_damageData`.
#[derive(Debug, Clone, PartialEq)]
pub struct AreaDamage {
    /// The damage table of a hit.
    pub table: String,
    /// The damage row of a hit.
    pub row: String,
    /// `m_baseDamage`.
    pub base_damage: u32,
    /// `m_damageRate`, seconds between two hits of a lasting area.
    pub rate_s: f32,
}

impl AreaDamage {
    /// `m_damageRate` in whole milliseconds, at least one.
    pub fn rate_ms(&self) -> u64 {
        seconds_ms(self.rate_s).max(1)
    }
}

/// An area slice of a siege structure (`<slice>_aoe`): the entity that opens where a fire barrel
/// or an explosive shell lands or where a horn blows, and what it does.
#[derive(Debug, Clone, PartialEq)]
pub struct SiegeArea {
    /// The slice asset.
    pub asset: [u8; ID_LEN],
    /// The component of its ownership state (3217).
    pub ownership_component: Option<u64>,
    /// The component of its position state (13).
    pub position_component: Option<u64>,
    /// Its timers; those that start on init start when it opens, and it goes when the longest of
    /// them ends (Observed: the barrel's area left 15.0 s after it opened,
    /// `joining_defending_war_2` 181.773 -> 196.771 s, its timers 15, 4 and 4 s).
    pub timers: Vec<SiegeTimer>,
    /// The radius its hits or effects reach, metres.
    pub radius_m: f32,
    /// It hits once when it opens (an explosion); else every `rate_s` while it lasts (a damage
    /// volume).
    pub once: bool,
    /// Its hit; `None` for an area that only gives effects.
    pub damage: Option<AreaDamage>,
    /// The status effects it gives (`m_effectId`; the horn's `HornOfResilience_T<n>`).
    pub effects: Vec<String>,
}

impl SiegeArea {
    /// How long it lasts: its longest timer that starts when it opens, ms.
    pub fn life_ms(&self) -> u64 {
        self.timers
            .iter()
            .filter(|timer| timer.on_open)
            .map(SiegeTimer::delay_ms)
            .max()
            .unwrap_or_default()
    }
}

/// The drop of a fire barrel dropper: the barrel its spawner lets fall after its pour timer.
#[derive(Debug, Clone, PartialEq)]
pub struct SiegeDrop {
    /// The barrel's projectile entity.
    pub projectile: TurretProjectile,
    /// The component of the barrel's ownership state.
    pub ownership_component: Option<u64>,
    /// The spawner's `m_projectileGravity`, m/s².
    pub gravity_mps2: f32,
    /// The facet rule of the spawner (the 2900 that names the drop).
    pub spawner_facet: FacetRule,
    /// The pour (`ProjectileSpawnDelay`): the barrel falls when it ends.
    pub pour: SiegeTimer,
}

/// The values of one turret slice.
#[derive(Debug, Clone, PartialEq)]
pub struct TurretKind {
    /// The slice asset.
    pub asset: [u8; ID_LEN],
    /// The slice's pak path.
    pub slice: String,
    /// `m_holdToFireRepeatedly`: a repeater (hit scans while the trigger is held).
    pub hold_to_fire: bool,
    /// `m_repeatFireDelay`, seconds between two shots of a repeater.
    pub repeat_fire_delay_s: f32,
    /// `m_fireSpeed`, m/s of a projectile.
    pub fire_speed_mps: f32,
    /// `m_yawAngleMax`, degrees either side of the facing.
    pub yaw_max_deg: f32,
    /// `m_pitchAngleMin`, degrees.
    pub pitch_min_deg: f32,
    /// `m_pitchAngleMax`, degrees.
    pub pitch_max_deg: f32,
    /// `m_heatMeterCapacity`; 0: no heat.
    pub heat_capacity: f32,
    /// `m_heatMeterGainRate`, per second of fire.
    pub heat_gain_per_s: f32,
    /// `m_heatMeterLossRate`, per second.
    pub heat_loss_per_s: f32,
    /// `m_heatLossDelay`, seconds after the fire stops.
    pub heat_loss_delay_s: f32,
    /// The spawner's `m_fireCooldown`, seconds a projectile turret is blocked after a shot.
    pub fire_cooldown_s: f32,
    /// The spawner's `m_projectileGravity`, m/s².
    pub gravity_mps2: f32,
    /// The spawner's `m_usesAmmo`.
    pub uses_ammo: bool,
    /// The spawner's `m_maxAmmo`.
    pub max_ammo: u16,
    /// The `m_ammoAmount` of the slice's `InteractWithItemCostType_AddProjectileSpawnerAmmo`
    /// (its `AddAmmo` option): the ammo one refill adds; `None` for a slice with no `AddAmmo`
    /// option (Observed: `slices/outpostrush/structures/or_turretcannon` has none).
    pub ammo_refill: Option<u16>,
    /// The `m_amount` of the slice's `InteractWithItemCostType_AddHealth` (its `AddHealth`
    /// option): the health one repair adds.
    pub repair_health: f32,
    /// The damage table of a hit.
    pub damage_table: String,
    /// The damage row of a hit.
    pub damage_row: String,
    /// The range of the spawner's ranged attack, metres.
    pub range_m: f32,
    /// The radius of the spawner's ranged attack, metres.
    pub radius_m: f32,
    /// The facet rule of its `TurretComponent`.
    pub turret_facet: FacetRule,
    /// The facet rule of its `ProjectileSpawnerComponent`.
    pub spawner_facet: FacetRule,
    /// Its projectile entity, for a turret that fires projectiles.
    pub projectile: Option<TurretProjectile>,
    /// The area its projectile opens where it lands (the explosive launcher's
    /// `turret_t<n>_explosivelauncher_aoe`); `None` for a turret whose shot opens none.
    pub area: Option<SiegeArea>,
    /// The status effect an ammo add costs in place of an ammo kit, for an encounter's turret
    /// (the Isle of Night ballista: `Raid_IoN_Balista_Bolt`, the effect of the gatherable
    /// `IoN_Balista_Bolt`; Inferred: no data table names the cost of its `AddAmmo` option).
    pub ammo_effect: Option<String>,
}

impl WarFort {
    /// Whether it is the fort of an invasion capture only: it has no capture point, so no war
    /// runs there (a war needs its claim point).
    pub fn is_invasion_fort(&self) -> bool {
        self.points.is_empty()
    }

    /// Where a player of `side` arrives: the first Observed arrival of its side, else the first
    /// Observed arrival of the fort (Inferred: the spawn camps of the sides are Unknown).
    pub fn arrival(&self, side: u8) -> Option<[f32; 3]> {
        self.arrivals
            .iter()
            .find(|arrival| arrival.side == side)
            .or_else(|| self.arrivals.first())
            .map(|arrival| arrival.position)
    }

    /// The siege data kind of the major structure whose gate is `gde_ref`.
    pub fn major_kind(&self, gde_ref: &[u8; ID_LEN]) -> Option<u32> {
        self.major_structures
            .iter()
            .find(|major| &major.gde_ref == gde_ref)
            .map(|major| major.kind)
    }

    /// Whether `[x, y]` is inside the frame.
    pub fn frames(&self, x: f32, y: f32) -> bool {
        let [left, bottom, width, height] = self.frame;
        (left..left + width).contains(&x) && (bottom..bottom + height).contains(&y)
    }
}

/// What a siege structure slice is (from its slice name: `turret_t<n>_ballista`,
/// `turret_t<n>_repeater`, `turret_t<n>_explosivelauncher`, `dropper_t<n>_firebarrel`,
/// `support_t<n>_hornofresilience`, `siege_supplygenerator_t<n>`, `platform_t<n>_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructureRole {
    /// A ballista turret (build choice 0).
    Ballista,
    /// A repeater turret (build choice 1).
    Repeater,
    /// An explosive launcher turret (build choice 2).
    ExplosiveLauncher,
    /// A fire barrel dropper (build choice 3).
    FireBarrelDropper,
    /// A horn of resilience (build choice 4).
    HornOfResilience,
    /// A siege supply generator.
    SupplyGenerator,
    /// A war camp platform weapon (cannon, fire launcher or repeater platform).
    Platform,
    /// A fort gate (`siege_fortgate_<s|m|l>_t<n>`): the defender company's, with vitals; it
    /// breaks at no health and is whole again at the war's result (Observed,
    /// `RS_offensive_war` 933.736 to 957.321 s).
    FortGate,
    /// A war camp gate (`siege_warcamp_gate_t<n>`): of no company and no vitals, the attackers'
    /// in the war's run (Observed: it takes the attacker's raid id at the start,
    /// `joining_defending_war_2` 83.710 s).
    WarCampGate,
}

/// The defensive structures of a 2646 build by its choice (Observed, `fort-equipements-territory`:
/// choices 0 to 4 on a T3 fort socket built `turret_t3_ballista`, `turret_t3_repeater`,
/// `turret_t3_explosivelauncher`, `dropper_t3_firebarrel` and `support_t3_hornofresilience`).
pub const DEFENSE_CHOICES: [StructureRole; 5] = [
    StructureRole::Ballista,
    StructureRole::Repeater,
    StructureRole::ExplosiveLauncher,
    StructureRole::FireBarrelDropper,
    StructureRole::HornOfResilience,
];

/// The pak folder of the war fort structure slices.
pub const WAR_SIEGE_SLICES: &str = "slices/structures/siege/";
/// The name of the Outpost Rush burning oil vats (`or_turretfire`, `or_turretfire_trench`) and
/// their role and tier: a fire barrel dropper of tier 1. Observed: nwdb names `OR_TurretFire`
/// "Burning Oil Vat Tier 1"; the slices' projectile spawner fires
/// `Dropper_T1_FireBarrel_Projectile` (the war T1 barrel), and the use of one in `OPR_TEST3`
/// (1714.326 s) dropped that barrel and opened `dropper_t1_firebarrel_aoe` where it landed.
const OUTPOST_RUSH_VAT: &str = "or_turretfire";
const OUTPOST_RUSH_VAT_TIER: u8 = 1;
/// The name of the open world's level fire droppers (`Dropper_Fire_POI_<n>`, the CoatGen slices
/// of the Hedet canyon capitals) and their role and tier: a fire barrel dropper of tier 1.
/// Observed: the slice names the blueprint `DropperFireBarrelT1` and its spawner fires
/// `Dropper_T3_FireBarrel_Projectile` (`coatgen/a4041713/dropper_fire_poi_2209365890`).
const LEVEL_DROPPER: &str = "dropper_fire_poi_";
const LEVEL_DROPPER_TIER: u8 = 1;
/// The tier part of a slice name, and the parts that name each role.
const TIER_PREFIX: &str = "_t";
const ROLE_PARTS: [(&str, StructureRole); 9] = [
    ("_ballista", StructureRole::Ballista),
    ("_repeater", StructureRole::Repeater),
    ("_explosivelauncher", StructureRole::ExplosiveLauncher),
    ("dropper_", StructureRole::FireBarrelDropper),
    ("_hornofresilience", StructureRole::HornOfResilience),
    ("supplygenerator_", StructureRole::SupplyGenerator),
    ("platform_", StructureRole::Platform),
    ("siege_fortgate_", StructureRole::FortGate),
    ("siege_warcamp_gate_", StructureRole::WarCampGate),
];

impl StructureRole {
    /// The role and the tier of the slice at `path` (an Outpost Rush vat is a T1 dropper,
    /// [`OUTPOST_RUSH_VAT`]); `None` for a slice of none.
    pub fn of_slice(path: &str) -> Option<(Self, u8)> {
        let name = path.rsplit('/').next()?.to_ascii_lowercase();
        if name.starts_with(OUTPOST_RUSH_VAT) {
            return Some((Self::FireBarrelDropper, OUTPOST_RUSH_VAT_TIER));
        }
        if name.starts_with(LEVEL_DROPPER) {
            return Some((Self::FireBarrelDropper, LEVEL_DROPPER_TIER));
        }
        // A platform's own name also holds the weapon (`platform_t3_repeater`): the platform
        // part goes first.
        let role = if name.starts_with("platform_") {
            Self::Platform
        } else {
            ROLE_PARTS
                .iter()
                .find(|(part, _)| name.contains(part))
                .map(|(_, role)| *role)?
        };
        let tier = name.match_indices(TIER_PREFIX).find_map(|(at, _)| {
            let digit = name[at + TIER_PREFIX.len()..].chars().next()?;
            u8::try_from(digit.to_digit(10)?).ok()
        })?;
        Some((role, tier))
    }

    /// The `BlueprintID` of the role's structure of `tier` (`BlueprintItemDefinitions`); `None`
    /// for a platform (its blueprints name the weapon, not the role) and a gate (no blueprint
    /// names one).
    pub fn blueprint(self, tier: u8) -> Option<String> {
        let name = match self {
            Self::Ballista => "TurretBallista",
            Self::Repeater => "TurretRepeater",
            Self::ExplosiveLauncher => "TurretExplosive",
            Self::FireBarrelDropper => "DropperFireBarrel",
            Self::HornOfResilience => "SupportHornOfResilience",
            Self::SupplyGenerator => return Some(format!("Siege_SupplyGenerator_T{tier}")),
            Self::Platform | Self::FortGate | Self::WarCampGate => return None,
        };
        Some(format!("{name}T{tier}"))
    }
}

/// One siege structure slice of the catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct StructureKind {
    /// The slice asset.
    pub asset: [u8; ID_LEN],
    /// The slice's pak path.
    pub slice: String,
    /// What it is.
    pub role: StructureRole,
    /// Its tier, 1 to 3.
    pub tier: u8,
    /// The first group retail sent of it (its card's component, its members); `None` when no
    /// ledger holds one.
    pub template: Option<FortStructure>,
    /// The template is not retail's: no capture holds the kind, and the table built it from
    /// the template of its role at another tier with the vitals and buildable members of its
    /// own tier (Inferred; the T1 and T2 explosive launchers).
    pub template_inferred: bool,
    /// The facet rule of its trigger component (the 3135 of a use), for a structure a player
    /// uses with a trigger (a dropper, a horn).
    pub trigger: Option<FacetRule>,
    /// The timers a use starts (the horn's `BlowDelay` timers).
    pub use_timers: Vec<SiegeTimer>,
    /// The barrel a dropper's use drops.
    pub drop: Option<SiegeDrop>,
    /// The area a use opens (the horn's), or that its drop opens where it lands (the
    /// dropper's).
    pub area: Option<SiegeArea>,
    /// The component of the interact whose options hold a dropper's use (`DropFlameBarrel`):
    /// the slice's interact that shows the user (Observed: component 17 of
    /// `dropper_t1_firebarrel`, the war captures' first states; component 16 of
    /// `or_turretfire`, `OPR_TEST3` 1714.326 s, while its build interact is component 19).
    /// `None` for a slice with no such interact.
    pub use_interact: Option<u64>,
    /// The gate parts of a fort gate or war camp gate.
    pub gate: Option<GateParts>,
}

/// What a gate slice holds besides its members.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateParts {
    /// `m_slicePhysicalGridRadius`: the reach of its body, metres (`siege_fortgate_l_t2` 10.23).
    pub physical_radius_m: f32,
    /// The `RepairDelay` timer its break starts (Observed: component 8 of `siege_fortgate_l_t2`
    /// at 30 s with the break, `RS_offensive_war` 933.736 s, and 0 again when it was whole,
    /// 957.321 s); `None` for a slice with none.
    pub repair_delay: Option<SiegeTimer>,
    /// The component of the interact whose options pass a player through its door
    /// (`TeleportToOtherSideOfDoor*`): off while it is broken, on when it is whole again
    /// (Observed: component 17 of `siege_fortgate_l_t2`, `RS_offensive_war` 933.736 and
    /// 957.321 s; its first state holds none).
    pub door_interact: Option<u64>,
}

impl StructureKind {
    /// Whether it is a structure of the war forts (its slice is under [`WAR_SIEGE_SLICES`]);
    /// the Outpost Rush vats are outpost structures.
    pub fn is_war_siege(&self) -> bool {
        self.slice.starts_with(WAR_SIEGE_SLICES)
    }
}

/// The item a siege structure costs and a supply generator makes (`StructureCost_Ingredient1` of
/// every siege blueprint, `OutputItem` of the siege generator recipes).
pub const SIEGE_SUPPLY: &str = "SiegeSupply";
/// The id prefix of the supply generator recipes (`GeneratorRecipes` `SiegeSupplyGeneratorT1`
/// to `T3`).
const GENERATOR_RECIPE_PREFIX: &str = "SiegeSupplyGeneratorT";
const MS_PER_SECOND: u64 = 1_000;
const MS_PER_SECOND_F64: f64 = 1_000.0;

/// The recipe of a supply generator tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorRecipe {
    /// `Tickrate`: how often it makes its output, ms.
    pub tick_ms: u64,
    /// `OutputItem`.
    pub item: String,
    /// `OutputQty`.
    pub quantity: u32,
    /// The most its container holds: the `EncumbranceBaseMax` of the `EncumbranceLimits` row
    /// named as the recipe (`SiegeSupplyGeneratorT<n>`: 300, 400, 500; Observed: the full
    /// generators of `joining_defending_war_2` 0.714 s, `CK_offensive_war` and
    /// `20261008-105106-362` held one stack of 300, 400 and 500 and made none).
    pub capacity: u32,
}

/// The build costs of the siege structures and the supply generator recipes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SiegeBlueprints {
    /// The `SiegeSupply` cost by `BlueprintID` (lowercase).
    costs: std::collections::HashMap<String, u32>,
    /// The generator recipes by tier.
    generators: std::collections::HashMap<u8, GeneratorRecipe>,
}

impl SiegeBlueprints {
    /// The siege blueprints and generator recipes of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let costs = tables
            .blueprint_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .filter(|row| row.structure_cost_ingredient1.as_deref() == Some(SIEGE_SUPPLY))
            .filter_map(|row| {
                let cost = u32::try_from(row.structure_cost_qty1?).ok()?;
                Some((row.blueprint_id.to_ascii_lowercase(), cost))
            });
        let capacity = |id: &str| {
            tables
                .encumbrance_data
                .iter()
                .flat_map(|table| &table.rows)
                .find(|row| row.container_type_id.eq_ignore_ascii_case(id))
                .and_then(|row| u32::try_from(row.encumbrance_base_max).ok())
        };
        let generators = tables
            .generator_recipes
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let tier = row
                    .structure_id
                    .strip_prefix(GENERATOR_RECIPE_PREFIX)?
                    .parse()
                    .ok()?;
                let recipe = GeneratorRecipe {
                    tick_ms: u64::try_from(row.tickrate)
                        .ok()?
                        .saturating_mul(MS_PER_SECOND),
                    item: row.output_item.clone()?,
                    quantity: u32::try_from(row.output_qty?).ok()?,
                    capacity: capacity(&row.structure_id)?,
                };
                Some((tier, recipe))
            });
        Self::build(costs, generators)
    }

    /// The blueprints of `costs` (`BlueprintID`, `SiegeSupply` cost) and the generator recipes
    /// `generators` by tier.
    pub fn build(
        costs: impl IntoIterator<Item = (String, u32)>,
        generators: impl IntoIterator<Item = (u8, GeneratorRecipe)>,
    ) -> Self {
        Self {
            costs: costs
                .into_iter()
                .map(|(id, cost)| (id.to_ascii_lowercase(), cost))
                .collect(),
            generators: generators.into_iter().collect(),
        }
    }

    /// The `SiegeSupply` a structure of `kind` costs; `None` when no blueprint names it.
    pub fn cost(&self, kind: &StructureKind) -> Option<u32> {
        let id = kind.role.blueprint(kind.tier)?;
        self.costs.get(&id.to_ascii_lowercase()).copied()
    }

    /// The recipe of a supply generator of `tier`.
    pub fn generator(&self, tier: u8) -> Option<&GeneratorRecipe> {
        self.generators.get(&tier)
    }
}

/// A fire dropper of the open world's level (a capital of the Hedet canyon, findings
/// `2026-11-13-outpost-rush-turrets.md`, **SQ-Brimestone-Sands-2 level entity**): its entity as
/// retail shows it (the level card form: no asset, the capital id and 8 zero bytes; no place
/// state, the client has the level's) and its own slice's use parts.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelDropper {
    /// Its capital's GUID.
    pub capital: String,
    /// Its entity: the capital's slice asset, place and facing, its first retail members.
    pub structure: FortStructure,
    /// Its members are another level dropper's (no capture shows it; Inferred).
    pub members_inferred: bool,
    /// Its slice's use parts (trigger, pour, drop, area, use interact).
    pub kind: StructureKind,
}

/// The forts of the wars, by territory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WarForts {
    forts: Vec<WarFort>,
    kinds: Vec<TurretKind>,
    structure_kinds: Vec<StructureKind>,
    level_droppers: Vec<LevelDropper>,
    encounter_turrets: Vec<FortStructure>,
}

impl WarForts {
    /// The table of client build `client_build` in content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`WarFortsError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, WarFortsError> {
        let path = war_forts_path(dir, client_build);
        let text =
            std::fs::read_to_string(&path).map_err(|source| WarFortsError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`WarFortsError`] when the text is not the table, or a fort has no claim point or
    /// no arrival.
    pub fn parse(text: &str) -> Result<Self, WarFortsError> {
        let file: TableFile = serde_json::from_str(text)?;
        let forts = file
            .forts
            .into_iter()
            .map(fort_of)
            .collect::<Result<Vec<_>, _>>()?;
        let kinds = file
            .turret_kinds
            .into_iter()
            .map(kind_of)
            .collect::<Result<Vec<_>, _>>()?;
        let structure_kinds = file
            .structure_kinds
            .into_iter()
            .filter_map(|row| structure_kind_of(row).transpose())
            .collect::<Result<Vec<_>, _>>()?;
        let level_droppers = file
            .level_droppers
            .into_iter()
            .filter_map(|row| {
                let structure = match structure_of(NO_TERRITORY, row.structure) {
                    Ok(structure) => structure,
                    Err(error) => return Some(Err(error)),
                };
                structure_kind_of(row.kind).transpose().map(|kind| {
                    Ok(LevelDropper {
                        capital: row.capital,
                        structure,
                        members_inferred: row.members_inferred,
                        kind: kind?,
                    })
                })
            })
            .collect::<Result<Vec<_>, WarFortsError>>()?;
        let encounter_turrets = file
            .encounter_turrets
            .into_iter()
            .map(|row| structure_of(NO_TERRITORY, row))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            forts,
            kinds,
            structure_kinds,
            level_droppers,
            encounter_turrets,
        })
    }

    /// Whether a build can make a structure of `kind`: it has a template, and a turret's kind
    /// fires (a repeater's hit scan, or a projectile the table holds).
    pub fn buildable(&self, kind: &StructureKind) -> bool {
        let Some(template) = &kind.template else {
            return false;
        };
        template.turret_component.is_none()
            || self
                .kind(&kind.asset)
                .is_some_and(|turret| turret.hold_to_fire || turret.projectile.is_some())
    }

    /// Every structure kind of the catalog.
    pub fn structure_kinds(&self) -> impl Iterator<Item = &StructureKind> {
        self.structure_kinds.iter()
    }

    /// The template and the kind of the turret of slice `slice` (its pak path) that an
    /// instance's encounter spawns (the template is the first group retail sent of it: the Isle
    /// of Night ballista, `ION-a9280e800e2eccdc` 440.476 s); `None` when no capture holds one.
    pub fn encounter_turret(&self, slice: &str) -> Option<(&FortStructure, &TurretKind)> {
        let kind = self
            .kinds
            .iter()
            .find(|kind| kind.slice.eq_ignore_ascii_case(slice))?;
        let template = self
            .encounter_turrets
            .iter()
            .find(|turret| turret.asset == kind.asset)?;
        Some((template, kind))
    }

    /// The fire droppers of the open world's level.
    pub fn level_droppers(&self) -> &[LevelDropper] {
        &self.level_droppers
    }

    /// The use parts of `structure`: a level dropper's own slice's (the level droppers share one
    /// slice asset, each slice with its own entity ids), else its asset's structure kind.
    pub fn use_kind(&self, structure: &FortStructure) -> Option<&StructureKind> {
        self.level_droppers
            .iter()
            .find(|dropper| dropper.structure.gde_ref == structure.gde_ref)
            .map(|dropper| &dropper.kind)
            .or_else(|| self.structure_kind(&structure.asset))
    }

    /// The structure kind of slice asset `asset`.
    pub fn structure_kind(&self, asset: &[u8; ID_LEN]) -> Option<&StructureKind> {
        self.structure_kinds
            .iter()
            .find(|kind| &kind.asset == asset)
    }

    /// The structure a 2646 build of `choice` on a structure of slice `asset` makes: the
    /// [`DEFENSE_CHOICES`] role of the choice at the tier of `asset`'s kind (Observed for T3;
    /// Inferred: a T1 or T2 socket builds the same roles of its own tier). `None` for a choice
    /// out of the list, a structure of no defensive kind, or a kind no slice has. Only war fort
    /// structures build and are built ([`StructureKind::is_war_siege`]).
    pub fn replacement(&self, asset: &[u8; ID_LEN], choice: u32) -> Option<&StructureKind> {
        let old = self.structure_kind(asset)?;
        if !DEFENSE_CHOICES.contains(&old.role) || !old.is_war_siege() {
            return None;
        }
        let role = *DEFENSE_CHOICES.get(usize::try_from(choice).ok()?)?;
        self.structure_kinds
            .iter()
            .find(|kind| kind.role == role && kind.tier == old.tier && kind.is_war_siege())
    }

    /// The turret kind of slice asset `asset`.
    pub fn kind(&self, asset: &[u8; ID_LEN]) -> Option<&TurretKind> {
        self.kinds.iter().find(|kind| &kind.asset == asset)
    }

    /// The fort of `territory`, when a capture shows it.
    pub fn fort(&self, territory: u16) -> Option<&WarFort> {
        self.forts.iter().find(|fort| fort.territory == territory)
    }

    /// Every fort.
    pub fn iter(&self) -> impl Iterator<Item = &WarFort> {
        self.forts.iter()
    }

    /// The number of forts.
    pub fn len(&self) -> usize {
        self.forts.len()
    }

    /// Whether the table has no fort.
    pub fn is_empty(&self) -> bool {
        self.forts.is_empty()
    }
}

/// The bytes of hex `text` of the fort or kind `territory` names (0 for a kind).
fn bytes_of(territory: u16, text: &str) -> Result<Vec<u8>, WarFortsError> {
    hex::decode(text).map_err(|source| WarFortsError::Hex { territory, source })
}

/// The 16-byte id in hex `text`.
fn id_of(territory: u16, text: &str) -> Result<[u8; ID_LEN], WarFortsError> {
    let bytes = bytes_of(territory, text)?;
    let len = bytes.len();
    bytes
        .try_into()
        .map_err(|_| WarFortsError::Len { territory, len })
}

/// The big-endian `u64` in hex `text`.
fn facet_of(territory: u16, text: &str) -> Result<u64, WarFortsError> {
    let bytes = bytes_of(territory, text)?;
    let len = bytes.len();
    let bytes: [u8; KEY_LEN] = bytes
        .try_into()
        .map_err(|_| WarFortsError::Len { territory, len })?;
    Ok(u64::from_be_bytes(bytes))
}

/// The territory a turret kind's errors name: kinds belong to no fort.
const NO_TERRITORY: u16 = 0;

fn rule_of(facet: &FacetRow) -> Result<FacetRule, WarFortsError> {
    Ok(FacetRule {
        xor: facet_of(NO_TERRITORY, &facet.xor)?,
        component: facet_of(NO_TERRITORY, &facet.component)?,
    })
}

fn projectile_of(projectile: &ProjectileRow) -> Result<TurretProjectile, WarFortsError> {
    Ok(TurretProjectile {
        asset: id_of(NO_TERRITORY, &projectile.asset)?,
        component: projectile.component,
        ranged_attack: projectile.ranged_attack,
        damage_table_crc: projectile.damage_table_crc,
        row_index: projectile.row_index,
    })
}

const fn timer_of(row: &TimerRow) -> SiegeTimer {
    SiegeTimer {
        component: row.component,
        delay_s: row.delay_s,
        on_open: row.on_open,
    }
}

/// The area of `row`; `None` for an area slice with no radius (it reaches nothing).
fn area_of(row: AreaRow) -> Result<Option<SiegeArea>, WarFortsError> {
    let Some(radius_m) = row.radius_m else {
        return Ok(None);
    };
    Ok(Some(SiegeArea {
        asset: id_of(NO_TERRITORY, &row.asset)?,
        ownership_component: row.ownership_component,
        position_component: row.position_component,
        timers: row.timers.iter().map(timer_of).collect(),
        radius_m,
        once: row.once,
        damage: row.damage.map(|damage| AreaDamage {
            table: damage.table,
            row: damage.row,
            base_damage: damage.base_damage,
            rate_s: damage.rate_s,
        }),
        effects: row.effects,
    }))
}

fn kind_of(row: KindRow) -> Result<TurretKind, WarFortsError> {
    Ok(TurretKind {
        asset: id_of(NO_TERRITORY, &row.asset)?,
        turret_facet: rule_of(&row.turret_facet)?,
        spawner_facet: rule_of(&row.spawner_facet)?,
        projectile: row.projectile.as_ref().map(projectile_of).transpose()?,
        area: row.area.map(area_of).transpose()?.flatten(),
        slice: row.slice,
        hold_to_fire: row.hold_to_fire,
        repeat_fire_delay_s: row.repeat_fire_delay_s,
        fire_speed_mps: row.fire_speed_mps,
        yaw_max_deg: row.yaw_max_deg,
        pitch_min_deg: row.pitch_min_deg,
        pitch_max_deg: row.pitch_max_deg,
        heat_capacity: row.heat_capacity,
        heat_gain_per_s: row.heat_gain_per_s,
        heat_loss_per_s: row.heat_loss_per_s,
        heat_loss_delay_s: row.heat_loss_delay_s,
        fire_cooldown_s: row.fire_cooldown_s,
        gravity_mps2: row.gravity_mps2,
        uses_ammo: row.uses_ammo,
        max_ammo: row.max_ammo,
        ammo_refill: row.ammo_refill,
        repair_health: row.repair_health,
        damage_table: row.damage_table,
        damage_row: row.damage_row,
        range_m: row.range_m,
        radius_m: row.radius_m,
        ammo_effect: row.ammo_effect,
    })
}

fn fort_of(row: FortRow) -> Result<WarFort, WarFortsError> {
    let territory = row.territory;
    let bytes = |text: &str| bytes_of(territory, text);
    let id = |text: &str| id_of(territory, text);
    let facet = |text: &str| facet_of(territory, text);
    let structures = row
        .structures
        .into_iter()
        .map(|structure| structure_of(territory, structure))
        .collect::<Result<Vec<_>, WarFortsError>>()?;
    let mut points = row
        .points
        .into_iter()
        .map(|point| {
            Ok(FortPoint {
                number: point.number,
                asset: id(&point.asset)?,
                gde_ref: id(&point.gde_ref)?,
                facet: facet(&point.facet)?,
                place: point.place,
                kind: point.kind,
                position: bytes(&point.position)?,
                card_component: point.card_component,
                position_component: point.position_component,
                ownership_component: point.ownership_component,
                point_component: point.point_component,
            })
        })
        .collect::<Result<Vec<_>, WarFortsError>>()?;
    points.sort_by_key(|point| point.number);
    // An invasion fort has no capture point (Observed: the instances of `invasion`,
    // `Reekwater_Invasion` and `Playthrough_Part_21` hold none); a war fort has its claim point.
    let war_fort_without_claim = !points.is_empty() && !points.iter().any(FortPoint::is_claim);
    if war_fort_without_claim || row.arrivals.is_empty() {
        return Err(WarFortsError::NoClaimPoint { territory });
    }
    Ok(WarFort {
        territory,
        frame: row.frame,
        arrivals: row
            .arrivals
            .into_iter()
            .map(|arrival| Arrival {
                side: arrival.side,
                position: arrival.position,
            })
            .collect(),
        siege: SiegeEntity {
            asset: id(&row.siege.asset)?,
            gde_ref: id(&row.siege.gde_ref)?,
            card_component: row.siege.card_component,
            state_component: row.siege.state_component,
        },
        points,
        structures,
        major_structures: row
            .major_structures
            .iter()
            .map(|major| {
                Ok(MajorStructure {
                    gde_ref: id(&major.gde_ref)?,
                    kind: major.kind,
                })
            })
            .collect::<Result<Vec<_>, WarFortsError>>()?,
    })
}

/// The structure of `row` of the fort of `territory` ([`NO_TERRITORY`] for a template).
fn structure_of(territory: u16, row: StructureRow) -> Result<FortStructure, WarFortsError> {
    Ok(FortStructure {
        asset: id_of(territory, &row.asset)?,
        gde_ref: id_of(territory, &row.gde_ref)?,
        place: row.place,
        yaw: row.yaw,
        side: row.side.unwrap_or(SIDE_DEFENDER),
        card_component: row.card_component,
        turret_component: row.turret_component,
        interact_component: row.interact_component,
        spawner_component: row.spawner_component,
        ownership_component: row.ownership_component,
        generator_component: row.generator_component,
        container_component: row.container_component,
        members: row
            .members
            .iter()
            .map(|member| bytes_of(territory, member))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn structure_kind_of(row: StructureKindRow) -> Result<Option<StructureKind>, WarFortsError> {
    let Some((role, tier)) = StructureRole::of_slice(&row.slice) else {
        return Ok(None);
    };
    Ok(Some(StructureKind {
        asset: id_of(NO_TERRITORY, &row.asset)?,
        slice: row.slice,
        role,
        tier,
        template: row
            .template
            .map(|template| structure_of(NO_TERRITORY, template))
            .transpose()?,
        template_inferred: row.template_inferred,
        trigger: row.trigger.as_ref().map(rule_of).transpose()?,
        use_timers: row
            .use_timers
            .unwrap_or_default()
            .iter()
            .map(timer_of)
            .collect(),
        drop: row
            .drop
            .map(|drop| {
                Ok::<_, WarFortsError>(SiegeDrop {
                    projectile: projectile_of(&drop.projectile)?,
                    ownership_component: drop.ownership_component,
                    gravity_mps2: drop.gravity_mps2,
                    spawner_facet: rule_of(&drop.spawner_facet)?,
                    pour: timer_of(&drop.pour),
                })
            })
            .transpose()?,
        area: row.area.map(area_of).transpose()?.flatten(),
        use_interact: row.use_interact,
        // A gate slice with no reach is no gate part (every gate slice of the catalog has one).
        gate: row.gate.and_then(|gate| {
            Some(GateParts {
                physical_radius_m: gate.physical_radius_m?,
                repair_delay: gate.repair_delay.as_ref().map(timer_of),
                door_interact: gate.door_interact,
            })
        }),
    }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A made-up fort in the table form: made-up ids, an outer point and the claim point with
    /// the retail places and place members of ES points 2 and 4 (`arvaysa_ES_attacker_war_45vs45`).
    pub(crate) const TABLE: &str = r#"{"forts": [{
        "territory": 8,
        "ledger": "made-up",
        "frame": [5632.0, 5120.0, 512.0, 512.0],
        "arrivals": [{"side": 1, "position": [6001.5, 5291.0, 56.6], "ledger": "made-up", "unix_ms": 1}],
        "siege": {"asset": "11111111111111111111111111111111", "gde_ref": "22222222222222222222222222222222",
                  "card_component": 0, "state_component": 2},
        "points": [
          {"number": 4, "asset": "33333333333333333333333333333333",
           "gde_ref": "08070605040302011111111111111111", "facet": "0a0b0c0d0e0f0001",
           "place": [5902.4795, 5428.118, 57.691372], "kind": 6, "position": "020d010345b873d645a9a0f2133903b309",
           "card_component": 0, "position_component": 2, "ownership_component": 1, "point_component": 4},
          {"number": 1, "asset": "33333333333333333333333333333333",
           "gde_ref": "aabbccddeeff00112222222222222222", "facet": "0a0b0c0d0e0f0002",
           "place": [5972.198, 5363.1045, 56.932175], "kind": 4, "position": "020d010345baa19645a798d6132103b309",
           "card_component": 0, "position_component": 2, "ownership_component": 1, "point_component": 3}
        ]}]}"#;

    /// The retail key rule with made-up ids (`arvaysa_ES_attacker_war_45vs45` 1783537199924: the
    /// 4103 key and the 4062 entity of each point are its `GdeRef`'s first eight bytes reversed).
    #[test]
    fn a_fort_reads_with_its_points_in_number_order_and_their_keys() {
        let forts = WarForts::parse(TABLE).expect("table");
        assert_eq!(forts.len(), 1);
        let fort = forts.fort(8).expect("territory 8");
        assert_eq!(fort.points[0].number, 1, "sorted by number");
        let claim = &fort.points[1];
        assert!(claim.is_claim());
        assert_eq!(claim.key(), 0x0102_0304_0506_0708);
        assert_eq!(claim.facet, 0x0a0b_0c0d_0e0f_0001);
        assert_eq!(fort.points[0].key(), 0x1100_ffee_ddcc_bbaa);
        assert!(fort.frames(5902.48, 5428.12));
        assert!(!fort.frames(5631.0, 5428.12));
        assert_eq!(fort.arrival(SIDE_ATTACKER), Some([6001.5, 5291.0, 56.6]));
        assert_eq!(
            fort.arrival(SIDE_DEFENDER),
            Some([6001.5, 5291.0, 56.6]),
            "no defender arrival: the fort's first"
        );
        assert!(forts.fort(12).is_none(), "no capture shows that fort");
    }

    /// A made-up fort with one made-up turret and the turret kind of a made-up slice whose facet
    /// rule is the retail cannon's of `RS_offensive_war` (its 4273 and 327 targets).
    pub(crate) const TURRET_TABLE: &str = r#"{"forts": [{
        "territory": 15, "frame": [13824.0, 4608.0, 512.0, 512.0],
        "arrivals": [{"side": 1, "position": [14000.0, 4900.0, 58.0]}],
        "siege": {"asset": "11111111111111111111111111111111", "gde_ref": "22222222222222222222222222222222",
                  "card_component": 0, "state_component": 2},
        "points": [{"number": 4, "asset": "33333333333333333333333333333333",
           "gde_ref": "08070605040302011111111111111111", "facet": "0a0b0c0d0e0f0001",
           "place": [14046.733, 4820.699, 57.647453], "kind": 6, "position": "020d0103465b7aef4596a598133743ffc1",
           "card_component": 0, "position_component": 2, "ownership_component": 1, "point_component": 4}],
        "structures": [{"asset": "44444444444444444444444444444444", "gde_ref": "c8e5b15b50b29d585555555555555555",
           "place": [14038.0, 4943.0, 58.2], "yaw": -0.5, "side": 1, "card_component": 0, "turret_component": 16,
           "interact_component": 13, "spawner_component": 15, "ownership_component": 5,
           "members": ["020d0103465b53e14599307e132943ff42"]}]}],
      "turret_kinds": [{"asset": "44444444444444444444444444444444", "slice": "made-up.dynamicslice",
        "hold_to_fire": false, "repeat_fire_delay_s": 0.0, "fire_speed_mps": 50.0, "yaw_max_deg": 75.0,
        "pitch_min_deg": -20.0, "pitch_max_deg": 25.0, "heat_capacity": 0.0, "heat_gain_per_s": 0.0,
        "heat_loss_per_s": 0.0, "heat_loss_delay_s": 0.0, "fire_cooldown_s": 3.0, "gravity_mps2": 13.0,
        "uses_ammo": true, "max_ammo": 15, "ammo_refill": 15, "repair_health": 5000.0,
        "damage_table": "DamageTable_Structures",
        "damage_row": "Cannon_Projectile", "range_m": 115.0, "radius_m": 0.3,
        "turret_facet": {"xor": "9aaa09e499dfb635", "component": "26eb5da726214d6d"},
        "spawner_facet": {"xor": "9aaa09ee99dfb635", "component": "603842316ab22336"},
        "projectile": {"asset": "66666666666666666666666666666666", "component": 1, "ranged_attack": 261,
                       "damage_table_crc": 81017683, "row_index": 6}}]}"#;

    #[test]
    fn a_turret_reads_with_its_kind_and_the_retail_facet_rule() {
        let forts = WarForts::parse(TURRET_TABLE).expect("table");
        let turret = &forts.fort(15).expect("fort").structures[0];
        assert_eq!(turret.facet_entity(), 0x589d_b250_5bb1_e5c8);
        assert_eq!(turret.members.len(), 1);
        assert_eq!(turret.spawner_component, Some(15));
        let kind = forts.kind(&turret.asset).expect("kind");
        assert_eq!(
            kind.turret_facet.target(turret.facet_entity()),
            0xe923_195b_e88f_a16a,
            "the retail 4273 target"
        );
        assert_eq!(
            kind.spawner_facet.target(turret.facet_entity()),
            0x226f_fdf0_2d20_7733,
            "the retail 327 target"
        );
        assert_eq!((kind.ammo_refill, kind.repair_health), (Some(15), 5000.0));
        let projectile = kind.projectile.expect("a projectile turret");
        assert_eq!((projectile.row_index, projectile.ranged_attack), (6, 261));
        assert!(forts.kind(&[0; ID_LEN]).is_none());
        assert!(
            WarForts::parse(TABLE)
                .expect("table")
                .fort(8)
                .expect("fort")
                .structures
                .is_empty()
        );
        assert_eq!(turret.turret_component, Some(16));
        assert_eq!(turret.generator_component, None);
    }

    /// Made-up assets of the five T3 defensive slices and one T2 repeater (the retail slice
    /// paths), the T3 horn with a template.
    const KINDS_TABLE: &str = r#"{"forts": [], "structure_kinds": [
        {"asset": "a0000000000000000000000000000000", "slice": "slices/structures/siege/turret/turret_t3_ballista.dynamicslice", "template": null},
        {"asset": "a1000000000000000000000000000000", "slice": "slices/structures/siege/turret/turret_t3_repeater.dynamicslice", "template": null},
        {"asset": "a2000000000000000000000000000000", "slice": "slices/structures/siege/turret/turret_t3_explosivelauncher.dynamicslice", "template": null},
        {"asset": "a3000000000000000000000000000000", "slice": "slices/structures/siege/dropper/dropper_t3_firebarrel.dynamicslice", "template": null},
        {"asset": "a4000000000000000000000000000000", "slice": "slices/structures/siege/support/support_t3_hornofresilience.dynamicslice",
         "template": {"asset": "a4000000000000000000000000000000", "gde_ref": "0e0e148b31bfb9ea896394551f6d5c1c", "card_component": 0,
                      "interact_component": 13, "spawner_component": null, "ownership_component": 4, "members": ["0a8b22010100"]}},
        {"asset": "b1000000000000000000000000000000", "slice": "slices/structures/siege/turret/turret_t2_repeater.dynamicslice", "template": null},
        {"asset": "c1000000000000000000000000000000", "slice": "slices/structures/siege/siege_supplygenerator_t2.dynamicslice", "template": null},
        {"asset": "d1000000000000000000000000000000", "slice": "slices/structures/siege/platform/platform_t3_repeater.dynamicslice", "template": null}]}"#;

    fn asset(first: u8) -> [u8; ID_LEN] {
        let mut asset = [0; ID_LEN];
        asset[0] = first;
        asset
    }

    #[test]
    fn the_slice_names_give_the_role_and_the_tier() {
        let forts = WarForts::parse(KINDS_TABLE).expect("table");
        let horn = forts.structure_kind(&asset(0xa4)).expect("horn");
        assert_eq!((horn.role, horn.tier), (StructureRole::HornOfResilience, 3));
        let template = horn.template.as_ref().expect("template");
        assert_eq!(
            (template.interact_component, template.side),
            (Some(13), SIDE_DEFENDER)
        );
        let generator = forts.structure_kind(&asset(0xc1)).expect("generator");
        assert_eq!(
            (generator.role, generator.tier),
            (StructureRole::SupplyGenerator, 2)
        );
        let platform = forts.structure_kind(&asset(0xd1)).expect("platform");
        assert_eq!(
            platform.role,
            StructureRole::Platform,
            "not a repeater turret"
        );
        assert_eq!(
            StructureRole::of_slice("slices/structures/siege/siege_claim.dynamicslice"),
            None
        );
    }

    /// Retail `fort-equipements-territory` 26.024 to 47.325 s: choices 0 to 4 on the T3 socket.
    #[test]
    fn a_build_choice_names_the_role_of_the_old_structures_tier() {
        let forts = WarForts::parse(KINDS_TABLE).expect("table");
        for (choice, first) in [(0, 0xa0), (1, 0xa1), (2, 0xa2), (3, 0xa3), (4, 0xa4)] {
            let new = forts.replacement(&asset(0xa4), choice).expect("a T3 kind");
            assert_eq!(new.asset, asset(first), "choice {choice}");
        }
        assert!(
            forts.replacement(&asset(0xa4), 5).is_none(),
            "out of the list"
        );
        assert!(
            forts.replacement(&asset(0xb1), 0).is_none(),
            "no T2 ballista slice in the table"
        );
        assert_eq!(
            forts.replacement(&asset(0xb1), 1).map(|kind| kind.asset),
            Some(asset(0xb1))
        );
        assert!(
            forts.replacement(&asset(0xc1), 1).is_none(),
            "a generator is not a socket"
        );
        assert!(
            forts.replacement(&asset(0xd1), 1).is_none(),
            "a platform is not a socket"
        );
    }

    /// The use, drop and area rows of the T1 dropper and the T2 horn as the table holds them
    /// (the slice values: `ProjectileSpawnDelay` 0.3 s, `BlowDelay` 7, 7, 10 and 90 s, the barrel
    /// area's 1,200 `FireBarrelDropper_DoT` every 0.5 s in 5 m for 15 s, the horn area's
    /// `HornOfResilience_T2` in 40 m), and the T3 explosive launcher's area (5,265
    /// `BombTurret_Explosion` once in 5 m).
    const USE_TABLE_KINDS: &str = r#", "structure_kinds": [
        {"asset": "4906e0ed66b052588dfadb3b8bb8988e", "slice": "slices/structures/siege/dropper/dropper_t1_firebarrel.dynamicslice",
         "template": null, "template_inferred": false,
         "trigger": {"xor": "0000000000000001", "component": "0000000000000002"}, "use_timers": [],
         "drop": {"projectile": {"asset": "05932228880d5bdf8fb1cc50513e7c62", "component": 2, "ranged_attack": 531,
                  "damage_table_crc": 81017683, "row_index": 17}, "ownership_component": 1, "gravity_mps2": 13.0,
                  "spawner_facet": {"xor": "0000000000000003", "component": "0000000000000004"},
                  "pour": {"component": 19, "delay_s": 0.3}},
         "area": {"asset": "b488e3f31cb65871baaf312aa2a52dc4", "ownership_component": 3, "position_component": 1,
                  "timers": [{"component": 4, "delay_s": 4.0}, {"component": 5, "delay_s": 15.0}, {"component": 6, "delay_s": 4.0}],
                  "radius_m": 5.0, "once": false,
                  "damage": {"table": "DamageTable_Structures", "row": "FireBarrelDropper_DoT", "base_damage": 1200, "rate_s": 0.5},
                  "effects": []}},
        {"asset": "09ea044a0f525bcbaaddc0b5f9e51067", "slice": "slices/structures/siege/support/support_t2_hornofresilience.dynamicslice",
         "template": null, "trigger": {"xor": "0000000000000005", "component": "0000000000000006"},
         "use_timers": [{"component": 17, "delay_s": 7.0}, {"component": 18, "delay_s": 7.0},
                        {"component": 19, "delay_s": 10.0}, {"component": 20, "delay_s": 90.0}],
         "drop": null,
         "area": {"asset": "2b622d29df025e9c91825ed757b1245e", "ownership_component": 1, "position_component": 2,
                  "timers": [{"component": 3, "delay_s": 1.0}], "radius_m": 40.0, "once": true, "damage": null,
                  "effects": ["HornOfResilience_T2"]}}]}"#;

    #[test]
    fn a_dropper_and_a_horn_read_with_their_use_drop_and_area() {
        let table = format!("{}{USE_TABLE_KINDS}", &TABLE[..TABLE.len() - 1]);
        let forts = WarForts::parse(&table).expect("table");
        let dropper = forts
            .structure_kind(&id_of(0, "4906e0ed66b052588dfadb3b8bb8988e").expect("id"))
            .expect("dropper");
        assert_eq!(dropper.role, StructureRole::FireBarrelDropper);
        let drop = dropper.drop.as_ref().expect("drop");
        assert_eq!(drop.pour.delay_ms(), 300);
        assert_eq!(drop.projectile.row_index, 17);
        assert_eq!(drop.spawner_facet.target(0x10), 0x17);
        let area = dropper.area.as_ref().expect("barrel area");
        assert_eq!(area.life_ms(), 15_000, "the longest timer");
        assert!(!area.once);
        assert_eq!(
            area.damage.as_ref().map(|damage| damage.base_damage),
            Some(1_200)
        );
        let horn = forts
            .structure_kind(&id_of(0, "09ea044a0f525bcbaaddc0b5f9e51067").expect("id"))
            .expect("horn");
        assert!(horn.drop.is_none());
        assert_eq!(
            horn.use_timers
                .iter()
                .map(SiegeTimer::delay_ms)
                .collect::<Vec<_>>(),
            [7_000, 7_000, 10_000, 90_000]
        );
        let area = horn.area.as_ref().expect("horn area");
        assert_eq!(area.effects, ["HornOfResilience_T2"]);
        assert!(area.damage.is_none());
        assert_eq!(area.life_ms(), 1_000);
        assert_eq!(horn.trigger.map(|rule| rule.target(0x10)), Some(0x1b));
    }

    /// The retail Outpost Rush vat (`or_turretfire`, asset `1326dd54..`): the facet rules of its
    /// slice (trigger and spawner), its pour timer (component 18, 0.3 s) and its use interact
    /// (component 16), with the war T1 barrel and area (Observed, `OPR_TEST3` 1714.326 to
    /// 1715.608 s); the facet entity of the retail vat of slot 123.
    const VAT_KINDS: &str = r#"{"forts": [], "structure_kinds": [
        {"asset": "1326dd54e1345e24a15063bf88cfcff4", "slice": "slices/outpostrush/structures/or_turretfire.dynamicslice",
         "template": null, "template_inferred": false, "use_interact": 16,
         "trigger": {"xor": "0000022e8d2bb976", "component": "bfcf71e4ebd21074"}, "use_timers": [],
         "drop": {"projectile": {"asset": "05932228880d5bdf8fb1cc50513e7c62", "component": 2, "ranged_attack": 275,
                  "damage_table_crc": 81017683, "row_index": 17}, "ownership_component": 1, "gravity_mps2": 13.0,
                  "spawner_facet": {"xor": "0000003700000000", "component": "603842316ab22336"},
                  "pour": {"component": 18, "delay_s": 0.3}},
         "area": {"asset": "b488e3f31cb65871baaf312aa2a52dc4", "ownership_component": 3, "position_component": 1,
                  "timers": [{"component": 4, "delay_s": 4.0}, {"component": 5, "delay_s": 15.0}, {"component": 6, "delay_s": 4.0}],
                  "radius_m": 5.0, "once": false,
                  "damage": {"table": "DamageTable_Structures", "row": "FireBarrelDropper_DoT", "base_damage": 1200, "rate_s": 0.5},
                  "effects": []}},
        {"asset": "a3000000000000000000000000000000", "slice": "slices/structures/siege/dropper/dropper_t1_firebarrel.dynamicslice", "template": null}]}"#;
    const VAT_FACET_ENTITY: u64 = 0xd08e_a7b0_680d_f518;

    #[test]
    fn an_outpost_rush_vat_reads_as_a_t1_dropper_with_the_retail_headers_and_is_no_war_build() {
        let forts = WarForts::parse(VAT_KINDS).expect("table");
        let vat = forts
            .structure_kind(&id_of(0, "1326dd54e1345e24a15063bf88cfcff4").expect("id"))
            .expect("vat");
        assert_eq!((vat.role, vat.tier), (StructureRole::FireBarrelDropper, 1));
        assert!(!vat.is_war_siege());
        assert_eq!(vat.use_interact, Some(16));
        assert_eq!(
            vat.trigger.map(|rule| rule.target(VAT_FACET_ENTITY)),
            Some(0x905e_1783_d0f8_5ce2),
            "the retail 3135 header"
        );
        let drop = vat.drop.as_ref().expect("drop");
        assert_eq!(
            drop.spawner_facet.target(VAT_FACET_ENTITY),
            0x30c6_e9b8_d2c0_184e,
            "the retail 2900 header"
        );
        assert_eq!((drop.pour.component, drop.pour.delay_ms()), (18, 300));
        assert_eq!(
            StructureRole::of_slice("slices/outpostrush/structures/or_turretfire_trench.dynamicslice"),
            Some((StructureRole::FireBarrelDropper, 1))
        );
        let war_dropper = id_of(0, "a3000000000000000000000000000000").expect("id");
        assert_eq!(
            forts.replacement(&war_dropper, 3).map(|kind| kind.asset),
            Some(war_dropper),
            "a war T1 socket builds the war dropper, not the vat"
        );
        assert!(forts.replacement(&vat.asset, 3).is_none(), "a vat is no war socket");
    }

    /// The retail blueprint rows (`BlueprintItemDefinitions`: `TurretBallistaT3` 120
    /// `SiegeSupply`) and recipe (`GeneratorRecipes` `SiegeSupplyGeneratorT2`: 10 s, 10).
    #[test]
    fn the_blueprints_give_each_kinds_cost_and_the_generator_recipe() {
        let forts = WarForts::parse(KINDS_TABLE).expect("table");
        let blueprints = SiegeBlueprints::build(
            [
                ("TurretBallistaT3".to_owned(), 120),
                ("SupportHornOfResilienceT3".to_owned(), 120),
                ("Siege_SupplyGenerator_T2".to_owned(), 125),
            ],
            [(
                2,
                GeneratorRecipe {
                    tick_ms: 10_000,
                    item: SIEGE_SUPPLY.to_owned(),
                    quantity: 10,
                    capacity: 400,
                },
            )],
        );
        let cost = |first| blueprints.cost(forts.structure_kind(&asset(first)).expect("kind"));
        assert_eq!(cost(0xa0), Some(120));
        assert_eq!(cost(0xa4), Some(120));
        assert_eq!(cost(0xc1), Some(125));
        assert_eq!(cost(0xa1), None, "no row");
        assert_eq!(cost(0xd1), None, "a platform names no role blueprint");
        assert_eq!(
            blueprints.generator(2).map(|recipe| recipe.quantity),
            Some(10)
        );
        assert!(blueprints.generator(1).is_none());
    }

    #[test]
    fn an_invasion_fort_has_no_capture_point_and_reads() {
        let start = TABLE.find("\"points\": [").expect("points");
        let end = TABLE[start..].find("        ]}]}").expect("end") + start;
        let table = format!("{}\"points\": [\n{}", &TABLE[..start], &TABLE[end..]);
        let forts = WarForts::parse(&table).expect("table");
        let fort = forts.fort(8).expect("fort");
        assert!(fort.is_invasion_fort());
        let war_forts = WarForts::parse(TABLE).expect("table");
        assert!(!war_forts.fort(8).expect("fort").is_invasion_fort());
    }

    #[test]
    fn a_fort_without_a_claim_point_is_refused() {
        let table = TABLE.replace("\"number\": 4", "\"number\": 2");
        assert!(matches!(
            WarForts::parse(&table),
            Err(WarFortsError::NoClaimPoint { territory: 8 })
        ));
    }

    #[test]
    fn a_turret_kind_without_an_add_ammo_option_has_no_refill() {
        let table = TURRET_TABLE.replace("\"ammo_refill\": 15, ", "");
        assert_ne!(table, TURRET_TABLE, "the row lost its refill");
        let forts = WarForts::parse(&table).expect("table");
        let turret = &forts.fort(15).expect("fort").structures[0];
        let kind = forts.kind(&turret.asset).expect("kind");
        assert_eq!(kind.ammo_refill, None);
        assert_eq!(kind.max_ammo, 15);
    }

    /// Observed (the gate slices of `joining_defending_war_2` and `RS_offensive_war`): the fort
    /// gates and war camp gates are structures of their own roles with a tier, built by no
    /// blueprint and no defensive build choice.
    #[test]
    fn the_gate_slices_are_gates_of_their_tier_with_no_blueprint() {
        assert_eq!(
            StructureRole::of_slice("slices/structures/siege/siege_fortgate_l_t2.dynamicslice"),
            Some((StructureRole::FortGate, 2))
        );
        assert_eq!(
            StructureRole::of_slice("slices/structures/siege/siege_fortgate_s_t3.dynamicslice"),
            Some((StructureRole::FortGate, 3))
        );
        assert_eq!(
            StructureRole::of_slice("slices/structures/siege/siege_warcamp_gate_t3.dynamicslice"),
            Some((StructureRole::WarCampGate, 3))
        );
        assert_eq!(StructureRole::FortGate.blueprint(2), None);
        assert_eq!(StructureRole::WarCampGate.blueprint(3), None);
        assert!(!DEFENSE_CHOICES.contains(&StructureRole::FortGate));
    }

    /// A fort gate kind with its slice's parts, and a fort with its major structure kinds (the
    /// retail `l_t2` gate of territory 15: kind 6, its `RepairDelay` component 8 at 30 s).
    #[test]
    fn a_gate_kind_and_the_major_structures_read() {
        const GATE: &str = "847074e0e8035352b95d12460d479b5f";
        const GATE_REF: &str = "4a3b68394a3652fbcc38065d07a87038";
        let table = TABLE
            .replace(
                "\"points\": [",
                &format!("\"major_structures\": [{{\"gde_ref\": \"{GATE_REF}\", \"kind\": 6}}],\n        \"points\": ["),
            )
            .replacen(
                "]}]}",
                &format!(
                    "]}}], \"structure_kinds\": [{{\"asset\": \"{GATE}\", \"slice\": \"slices/structures/siege/siege_fortgate_l_t2.dynamicslice\",
                     \"template\": null, \"gate\": {{\"physical_radius_m\": 10.23,
                     \"repair_delay\": {{\"component\": 8, \"delay_s\": 30.0, \"on_open\": false}},
                     \"door_interact\": 17}}}}]}}"
                ),
                1,
            );
        let forts = WarForts::parse(&table).expect("table");
        let fort = forts.fort(8).expect("fort");
        let gate_ref: [u8; ID_LEN] = hex::decode(GATE_REF).expect("hex").try_into().expect("16");
        assert_eq!(fort.major_kind(&gate_ref), Some(6));
        assert_eq!(fort.major_kind(&[0; ID_LEN]), None);
        let asset: [u8; ID_LEN] = hex::decode(GATE).expect("hex").try_into().expect("16");
        let kind = forts.structure_kind(&asset).expect("kind");
        assert_eq!((kind.role, kind.tier), (StructureRole::FortGate, 2));
        let gate = kind.gate.expect("gate parts");
        assert_eq!(gate.physical_radius_m, 10.23);
        let delay = gate.repair_delay.expect("repair delay");
        assert_eq!((delay.component, delay.delay_ms(), delay.on_open), (8, 30_000, false));
        assert_eq!(gate.door_interact, Some(17));
    }

    /// Observed (`invasion` 335.079 s, the cow launcher's area): a start-on-trigger timer opens
    /// at 0 and does not hold the area open; a table row with no flag starts at once.
    #[test]
    fn only_the_timers_that_start_on_open_show_their_delay_and_hold_the_area() {
        let row = |text: &str| -> TimerRow { serde_json::from_str(text).expect("row") };
        let old = timer_of(&row(r#"{"component": 4, "delay_s": 4.0}"#));
        assert!(old.on_open, "an old table's timer starts at once");
        let later = timer_of(&row(r#"{"component": 13, "delay_s": 3.0, "on_open": false}"#));
        assert_eq!(old.open_delay_s(), 4.0);
        assert_eq!(later.open_delay_s(), 0.0);
        let area = SiegeArea {
            asset: [0; ID_LEN],
            ownership_component: Some(3),
            position_component: Some(5),
            timers: vec![
                timer_of(&row(r#"{"component": 8, "delay_s": 3.8, "on_open": true}"#)),
                timer_of(&row(r#"{"component": 10, "delay_s": 4.0, "on_open": true}"#)),
                later,
                timer_of(&row(r#"{"component": 12, "delay_s": 9.0, "on_open": false}"#)),
            ],
            radius_m: 4.0,
            once: false,
            damage: None,
            effects: Vec::new(),
        };
        assert_eq!(area.life_ms(), 4_000, "the longest start-on-init timer");
    }

    /// A level dropper reads with its own slice's use parts, a T1 fire barrel dropper by its
    /// name (`Dropper_Fire_POI_<n>`), and is found by its `GdeRef` before its shared asset.
    #[test]
    fn a_level_dropper_reads_and_its_use_kind_is_its_own() {
        const LEVEL_REF: &str = "b3dc34cf7e2e82c60000000000000000";
        const SLICE: &str = "coatgen/a4041713/dropper_fire_poi_2209365890.dynamicslice";
        let table = format!(
            r#"{{"forts": [], "level_droppers": [{{"capital": "b212b88f-c633-a507-7490-96f1090779b4",
              "structure": {{"asset": "cbb06cbea8f05c6d8307709715324c8c", "gde_ref": "{LEVEL_REF}",
                "place": [5380.2793, 7385.836, 279.2911], "yaw": -0.401, "card_component": 0,
                "interact_component": 16, "spawner_component": 15, "ownership_component": 2,
                "members": ["0f390103ffff00"]}},
              "kind": {{"asset": "cbb06cbea8f05c6d8307709715324c8c", "slice": "{SLICE}", "template": null,
                "use_interact": 16}}}}]}}"#
        );
        let forts = WarForts::parse(&table).expect("table");
        let dropper = &forts.level_droppers()[0];
        assert!(!dropper.members_inferred);
        assert_eq!(
            (dropper.kind.role, dropper.kind.tier),
            (StructureRole::FireBarrelDropper, 1)
        );
        assert!(!dropper.kind.is_war_siege());
        let kind = forts.use_kind(&dropper.structure).expect("its kind");
        assert_eq!(kind.slice, SLICE);
        assert_eq!(kind.use_interact, Some(16));
    }
}
