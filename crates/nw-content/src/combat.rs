// SPDX-License-Identifier: AGPL-3.0-or-later
//! The combat rows a `sim` keeps (M6-A spec section 3). [`CombatSource`] is the plain content of
//! the datasheets that combat reads; [`CombatData`] is what a `sim` looks up during a tick.
//!
//! A mob type is found by the crc32 of its lower-case vitals id, as the world file stores it: in
//! `VitalsBaseData` first, then `VitalsData` (no mods: 1.0), then `VitalsLevelVariantData`, whose
//! row names the base type it varies. A type's mods are its own times the category mods of its
//! creature type (`VitalsModifierData`, for example `Solo+`: health 2, damage 1.2).

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;
use thiserror::Error;

use crate::Tables;

/// Separates the status effects of an `AddStatusEffects` cell.
const EFFECT_SEPARATOR: char = '+';
/// Health and damage mod of a type without them.
const PLAIN_MOD: f64 = 1.0;

/// The player row of `VitalsBaseData` (M6-A spec section 9).
pub const DEFAULT_PLAYER_VITALS_ID: &str = "Player";
/// The player sword of `WeaponItemDefinitions`.
pub const DEFAULT_WEAPON_ID: &str = "SwordT1";
/// The light and heavy sword attacks of `DamageData`.
pub const DEFAULT_ATTACK_DAMAGE_IDS: [&str; 2] = ["1H_Sword_Attack1", "1H_Sword_Attack2"];
/// The `DamageData` row of one blunderbuss pellet (ranged note: coefficient 0.19 per pellet).
pub const DEFAULT_SPREADSHOT_DAMAGE_ID: &str = "Blunderbuss_Primary_Attack";
/// The `RangedAttackProfile` of a weapon that fires spread shots.
const SPREAD_SHOT_PROFILE: &str = "Blunderbuss";

/// The config rows a `sim` needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatRows {
    /// The player row of `VitalsBaseData`.
    pub player_vitals_id: String,
    /// The player weapon row of `WeaponItemDefinitions`.
    pub weapon_id: String,
    /// The `DamageData` rows of the weapon's attacks.
    pub attack_damage_ids: Vec<String>,
    /// The `DamageData` row of one pellet of a spread shot.
    pub spreadshot_damage_id: String,
}

impl Default for CombatRows {
    fn default() -> Self {
        Self {
            player_vitals_id: DEFAULT_PLAYER_VITALS_ID.to_owned(),
            weapon_id: DEFAULT_WEAPON_ID.to_owned(),
            attack_damage_ids: DEFAULT_ATTACK_DAMAGE_IDS.map(str::to_owned).to_vec(),
            spreadshot_damage_id: DEFAULT_SPREADSHOT_DAMAGE_ID.to_owned(),
        }
    }
}

/// The `VitalsModifierData` mods of one creature type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CategoryMods {
    /// Health multiplier.
    pub health: f64,
    /// Damage multiplier.
    pub damage: f64,
}

/// A mob type.
#[derive(Debug, Clone, PartialEq)]
pub struct MobType {
    /// Multiplies the level's base health (with the creature type's category mod).
    pub health_mod: f64,
    /// Multiplies the level's base damage (with the creature type's category mod).
    pub damage_mod: f64,
    /// The creature type, for the vision offset.
    pub creature_type: Option<String>,
}

impl MobType {
    /// A type without mods.
    pub const PLAIN: Self = Self {
        health_mod: PLAIN_MOD,
        damage_mod: PLAIN_MOD,
        creature_type: None,
    };
}

/// One level of `VitalsLevelData`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelRow {
    /// Base max health.
    pub base_max_health: f64,
    /// Base damage of one hit.
    pub base_damage: f64,
    /// Physical armor rating of a mob of this level.
    pub physical_armor_rating: f64,
}

/// The player weapon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponRow {
    /// Base damage.
    pub base_damage: f64,
    /// Crit chance, 0 to 1.
    pub crit_chance: f64,
    /// Crit damage multiplier.
    pub crit_multiplier: f64,
    /// It fires spread shots (a blunderbuss): its attacks come as 2719 requests.
    pub spread_shot: bool,
    /// What kind of weapon it is.
    pub kind: WeaponKind,
}

/// The kind of a melee weapon, which sets the shape and time of its swing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WeaponKind {
    /// A sword (`1hSword…`, `1hLongsword…`).
    Sword,
    /// A hatchet.
    Hatchet,
    /// A rapier.
    Rapier,
    /// A spear.
    Spear,
    /// A great axe.
    GreatAxe,
    /// A war hammer.
    WarHammer,
    /// A greatsword.
    GreatSword,
    /// Any other weapon (ranged, magic or unknown).
    Other,
}

impl WeaponKind {
    /// The names in retail weapon ids, longest match first (`1hHatchetT1`, `2hWarHammerT2`,
    /// `1hGreatSword_HalloweenT5`): a greatsword id also holds "sword".
    const NAMES: [(&'static str, Self); 7] = [
        ("greatsword", Self::GreatSword),
        ("greataxe", Self::GreatAxe),
        ("warhammer", Self::WarHammer),
        ("hatchet", Self::Hatchet),
        ("rapier", Self::Rapier),
        ("spear", Self::Spear),
        ("sword", Self::Sword),
    ];

    /// The damage rows of its light and heavy attacks (`javelindata_damagetable`); `None` for
    /// [`WeaponKind::Other`], which uses the config's.
    pub const fn attack_ids(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            Self::Sword => ("1H_Sword_Attack1", "1H_Sword_Attack2"),
            Self::Hatchet => ("Hatchet_Damage_PrimaryAttack", "Hatchet_Damage_HeavyAttack"),
            Self::Rapier => ("1H_Rapier_PrimaryAttack", "1H_Rapier_HeavyAttack"),
            Self::Spear => ("2H_Spear_PrimaryAttack", "2H_Spear_HeavyAttack"),
            Self::GreatAxe => ("GreatAxe_PrimaryAttack", "GreatAxe_HeavyAttack"),
            Self::WarHammer => ("Warhammer_PrimaryAttack", "Warhammer_HeavyAttack"),
            Self::GreatSword => ("GreatSword_PrimaryAttack1", "GreatSword_HeavyAttack1"),
            Self::Other => return None,
        })
    }

    /// The kind that the weapon id `weapon_id` names.
    pub fn of_id(weapon_id: &str) -> Self {
        let id = weapon_id.to_lowercase();
        Self::NAMES
            .iter()
            .find(|(name, _)| id.contains(name))
            .map_or(Self::Other, |(_, kind)| *kind)
    }
}

/// A consumable that heals: a potion.
#[derive(Debug, Clone, PartialEq)]
pub struct HealingConsumable {
    /// `ConsumableItemDefinitions.CooldownDuration`, in seconds.
    pub cooldown_s: u32,
    /// `CooldownId`: consumables of one id share the cooldown.
    pub cooldown_id: String,
    /// The heal of its status effect (`StatusEffectData.Health`).
    pub health: f64,
    /// Its share of max health (`HealthModifierPercent`).
    pub health_share: f64,
}

/// One attack of `DamageData`.
#[derive(Debug, Clone, PartialEq)]
pub struct AttackRow {
    /// Its row id.
    pub damage_id: String,
    /// Damage coefficient.
    pub coefficient: f64,
    /// Whether it can crit (an empty cell allows it).
    pub can_crit: bool,
    /// Threat it adds besides its damage.
    pub threat: u64,
}

/// The plain content of the combat datasheets.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CombatSource {
    /// `VitalsBaseData` rows by id.
    pub base_types: Vec<(String, MobType)>,
    /// `VitalsData` ids.
    pub plain_types: Vec<String>,
    /// `VitalsLevelVariantData`: (id, base id).
    pub variants: Vec<(String, String)>,
    /// `VitalsLevelData` by level.
    pub levels: Vec<(u32, LevelRow)>,
    /// `WeaponItemDefinitions` by id.
    pub weapons: Vec<(String, WeaponRow)>,
    /// `MasterItemDefinitions`: (item id, `ItemStatsRef`) of the items that have stats.
    pub item_stats: Vec<(String, String)>,
    /// `ConsumableItemDefinitions`: (consumable id, cooldown seconds, cooldown id, status
    /// effects) of the consumables with a cooldown.
    pub consumables: Vec<(String, u32, String, String)>,
    /// `StatusEffectData`: (status id, health, health share) of the effects that heal.
    pub heals: Vec<(String, f64, f64)>,
    /// `DamageData` rows.
    pub attacks: Vec<AttackRow>,
    /// `AITargetingData`: out-of-combat vision offset by creature type.
    pub vision_offsets: Vec<(String, f32)>,
    /// `VitalsBaseData` initial health by id (rows whose cell is a whole number).
    pub player_health: Vec<(String, u32)>,
    /// `VitalsModifierData` by category (creature type).
    pub categories: Vec<(String, CategoryMods)>,
}

impl CombatSource {
    /// The combat content of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let base_rows = || tables.vitals_base_data.iter().flat_map(|table| &table.rows);
        Self {
            base_types: base_rows()
                .map(|row| {
                    (
                        row.vitals_id.clone(),
                        MobType {
                            health_mod: row.health_mod.unwrap_or(PLAIN_MOD),
                            damage_mod: row.damage_mod.unwrap_or(PLAIN_MOD),
                            creature_type: row.creature_type.clone(),
                        },
                    )
                })
                .collect(),
            plain_types: tables
                .vitals_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| row.vitals_id.clone())
                .collect(),
            variants: tables
                .vitals_level_variant_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| (row.vitals_id.clone(), row.base_vitals_id.clone()))
                .collect(),
            levels: tables
                .vitals_level_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let level = u32::try_from(row.level).ok()?;
                    Some((
                        level,
                        LevelRow {
                            base_max_health: row.base_max_health,
                            base_damage: row.base_damage,
                            physical_armor_rating: row.physical_armor_rating,
                        },
                    ))
                })
                .collect(),
            weapons: tables
                .weapon_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| {
                    (
                        row.weapon_id.clone(),
                        WeaponRow {
                            base_damage: row.base_damage.map_or(0.0, |damage| damage as f64),
                            crit_chance: row.crit_chance.unwrap_or(0.0),
                            crit_multiplier: row.crit_damage_multiplier.unwrap_or(PLAIN_MOD),
                            spread_shot: row.ranged_attack_profile.as_deref()
                                == Some(SPREAD_SHOT_PROFILE),
                            kind: WeaponKind::of_id(&row.weapon_id),
                        },
                    )
                })
                .collect(),
            consumables: tables
                .consumable_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    Some((
                        row.consumable_id.clone(),
                        u32::try_from(row.cooldown_duration?).ok()?,
                        row.cooldown_id.clone()?,
                        row.add_status_effects.clone()?,
                    ))
                })
                .collect(),
            heals: tables
                .status_effect_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let health = row.health?;
                    (health > 0).then(|| {
                        (
                            row.status_id.clone(),
                            health as f64,
                            row.health_modifier_percent.unwrap_or(0.0),
                        )
                    })
                })
                .collect(),
            item_stats: tables
                .master_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| Some((row.item_id.clone(), row.item_stats_ref.clone()?)))
                .collect(),
            attacks: tables
                .damage_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| AttackRow {
                    damage_id: row.damage_id.clone(),
                    coefficient: row.dmg_coef.unwrap_or(PLAIN_MOD),
                    can_crit: row.can_crit.unwrap_or(true),
                    threat: row
                        .add_threat
                        .and_then(|threat| u64::try_from(threat).ok())
                        .unwrap_or(0),
                })
                .collect(),
            vision_offsets: tables
                .ai_targeting_data
                .iter()
                .flat_map(|table| &table.rows)
                // A vision offset is a few metres; f32 holds it exactly enough.
                .map(|row| {
                    (
                        row.creature_type.clone(),
                        row.out_of_combat_vision_distance_offset as f32,
                    )
                })
                .collect(),
            categories: tables
                .vitals_modifier_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| {
                    (
                        row.category_id.clone(),
                        CategoryMods {
                            health: row.category_health_mod,
                            damage: row.category_damage_mod,
                        },
                    )
                })
                .collect(),
            player_health: base_rows()
                .filter_map(|row| {
                    Some((
                        row.vitals_id.clone(),
                        row.health_initial.trim().parse().ok()?,
                    ))
                })
                .collect(),
        }
    }
}

/// A config row that the datasheets do not have.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CombatDataError {
    /// No `VitalsBaseData` row with a whole initial health.
    #[error("no player vitals row `{0}`")]
    MissingPlayer(String),
    /// No `WeaponItemDefinitions` row.
    #[error("no weapon row `{0}`")]
    MissingWeapon(String),
    /// No `DamageData` row.
    #[error("no damage row `{0}`")]
    MissingAttack(String),
}

/// What a `sim` looks up during a tick.
#[derive(Debug, Clone, PartialEq)]
pub struct CombatData {
    mob_types: HashMap<u32, MobType>,
    levels: HashMap<u32, LevelRow>,
    weapon: WeaponRow,
    /// The weapon row of each item that is a weapon, by lower-case item id.
    item_weapons: HashMap<String, WeaponRow>,
    /// The healing consumable of each item that is one, by lower-case item id.
    item_heals: HashMap<String, HealingConsumable>,
    attacks: Vec<AttackRow>,
    /// The light and heavy attacks of each weapon kind whose rows the tables hold.
    kind_attacks: HashMap<WeaponKind, [AttackRow; 2]>,
    spreadshot: AttackRow,
    vision_offsets: HashMap<String, f32>,
    player_max_health: u32,
}

impl CombatData {
    /// The combat data of `tables` for the config `rows`.
    ///
    /// # Errors
    ///
    /// Returns [`CombatDataError`] when a config row is missing.
    pub fn from_tables(tables: &Tables, rows: &CombatRows) -> Result<Self, CombatDataError> {
        Self::build(CombatSource::from_tables(tables), rows)
    }

    /// The combat data of `source` for the config `rows`.
    ///
    /// # Errors
    ///
    /// Returns [`CombatDataError`] when a config row is missing.
    pub fn build(source: CombatSource, rows: &CombatRows) -> Result<Self, CombatDataError> {
        let player_max_health = source
            .player_health
            .iter()
            .find(|(id, _)| *id == rows.player_vitals_id)
            .map(|(_, health)| *health)
            .ok_or_else(|| CombatDataError::MissingPlayer(rows.player_vitals_id.clone()))?;
        let weapon = source
            .weapons
            .iter()
            .find(|(id, _)| *id == rows.weapon_id)
            .map(|(_, weapon)| *weapon)
            .ok_or_else(|| CombatDataError::MissingWeapon(rows.weapon_id.clone()))?;
        let attack_row = |wanted: &String| {
            source
                .attacks
                .iter()
                .find(|row| row.damage_id == *wanted)
                .cloned()
                .ok_or_else(|| CombatDataError::MissingAttack(wanted.clone()))
        };
        let attacks = rows
            .attack_damage_ids
            .iter()
            .map(attack_row)
            .collect::<Result<Vec<_>, _>>()?;
        let spreadshot = attack_row(&rows.spreadshot_damage_id)?;
        let row_of = |id: &str| {
            source
                .attacks
                .iter()
                .find(|row| row.damage_id == id)
                .cloned()
        };
        let kind_attacks = WeaponKind::NAMES
            .iter()
            .filter_map(|(_, kind)| {
                let (light, heavy) = kind.attack_ids()?;
                Some((*kind, [row_of(light)?, row_of(heavy)?]))
            })
            .collect();
        let weapons: HashMap<String, WeaponRow> = source
            .weapons
            .iter()
            .map(|(id, weapon)| (id.to_lowercase(), *weapon))
            .collect();
        let item_weapons = source
            .item_stats
            .iter()
            .filter_map(|(item, stats)| {
                Some((item.to_lowercase(), *weapons.get(&stats.to_lowercase())?))
            })
            .collect();
        let heals: HashMap<String, (f64, f64)> = source
            .heals
            .iter()
            .map(|(id, health, share)| (id.to_lowercase(), (*health, *share)))
            .collect();
        let consumables: HashMap<String, HealingConsumable> = source
            .consumables
            .iter()
            .filter_map(|(id, cooldown_s, cooldown_id, effects)| {
                let (health, health_share) = effects
                    .split(EFFECT_SEPARATOR)
                    .find_map(|effect| heals.get(&effect.trim().to_lowercase()))?;
                Some((
                    id.to_lowercase(),
                    HealingConsumable {
                        cooldown_s: *cooldown_s,
                        cooldown_id: cooldown_id.clone(),
                        health: *health,
                        health_share: *health_share,
                    },
                ))
            })
            .collect();
        // A consumable's id is its item id: every retail health potion has no `ItemStatsRef`
        // (live 2026-09-16: `PotionHealthT1` never healed). An item that names a consumable by
        // its stats is kept too; its own consumable row wins.
        let item_heals = source
            .item_stats
            .iter()
            .filter_map(|(item, stats)| {
                Some((
                    item.to_lowercase(),
                    consumables.get(&stats.to_lowercase())?.clone(),
                ))
            })
            .chain(consumables.clone())
            .collect();
        let categories: HashMap<String, CategoryMods> = source.categories.into_iter().collect();
        let base: HashMap<u32, MobType> = source
            .base_types
            .into_iter()
            .map(|(id, mut kind)| {
                if let Some(mods) = kind
                    .creature_type
                    .as_ref()
                    .and_then(|kind| categories.get(kind))
                {
                    kind.health_mod *= mods.health;
                    kind.damage_mod *= mods.damage;
                }
                (lowercase_crc32(&id), kind)
            })
            .collect();
        let mut mob_types = base.clone();
        for id in source.plain_types {
            mob_types
                .entry(lowercase_crc32(&id))
                .or_insert(MobType::PLAIN);
        }
        for (id, base_id) in source.variants {
            let kind = base
                .get(&lowercase_crc32(&base_id))
                .cloned()
                .unwrap_or(MobType::PLAIN);
            mob_types.entry(lowercase_crc32(&id)).or_insert(kind);
        }
        Ok(Self {
            mob_types,
            levels: source.levels.into_iter().collect(),
            weapon,
            item_weapons,
            item_heals,
            attacks,
            kind_attacks,
            spreadshot,
            vision_offsets: source.vision_offsets.into_iter().collect(),
            player_max_health,
        })
    }

    /// The mob type of a vitals id crc32.
    pub fn mob_type(&self, vitals_crc: u32) -> Option<&MobType> {
        self.mob_types.get(&vitals_crc)
    }

    /// The weapon row of item `item_id` (its `ItemStatsRef`); `None` for an item that is no
    /// weapon. Case does not matter.
    pub fn weapon_of_item(&self, item_id: &str) -> Option<&WeaponRow> {
        self.item_weapons.get(&item_id.to_lowercase())
    }

    /// The healing consumable of item `item_id`; `None` for an item that is none. Case does not
    /// matter.
    pub fn heal_of_item(&self, item_id: &str) -> Option<&HealingConsumable> {
        self.item_heals.get(&item_id.to_lowercase())
    }

    /// The row of `level`.
    pub fn level(&self, level: u32) -> Option<&LevelRow> {
        self.levels.get(&level)
    }

    /// Every level row.
    pub fn levels(&self) -> impl Iterator<Item = (u32, &LevelRow)> {
        self.levels.iter().map(|(level, row)| (*level, row))
    }

    /// The player weapon.
    pub const fn weapon(&self) -> &WeaponRow {
        &self.weapon
    }

    /// The first of the config's weapon attacks: the light attack.
    pub fn first_attack(&self) -> Option<&AttackRow> {
        self.attacks.first()
    }

    /// The second of the config's weapon attacks: the heavy attack.
    pub fn heavy_attack(&self) -> Option<&AttackRow> {
        self.attacks.get(1)
    }

    /// The attack of one pellet of a spread shot.
    pub const fn spreadshot_attack(&self) -> &AttackRow {
        &self.spreadshot
    }

    /// The light (`heavy` false) or heavy attack of a weapon of `kind`: its own rows, else the
    /// config's.
    pub fn weapon_attack(&self, kind: WeaponKind, heavy: bool) -> Option<&AttackRow> {
        match self.kind_attacks.get(&kind) {
            Some([light, heavy_row]) => Some(if heavy { heavy_row } else { light }),
            None if heavy => self.heavy_attack(),
            None => self.first_attack(),
        }
    }

    /// The weapon attack `damage_id`, if it is one of the config's or of a weapon kind.
    pub fn attack(&self, damage_id: &str) -> Option<&AttackRow> {
        self.attacks
            .iter()
            .chain(self.kind_attacks.values().flatten())
            .find(|row| row.damage_id == damage_id)
    }

    /// The out-of-combat vision offset of `creature_type`; 0 without a row.
    pub fn vision_offset(&self, creature_type: Option<&str>) -> f32 {
        creature_type
            .and_then(|kind| self.vision_offsets.get(kind))
            .copied()
            .unwrap_or(0.0)
    }

    /// The player's max health.
    pub const fn player_max_health(&self) -> u32 {
        self.player_max_health
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retail_weapon_ids_name_their_kind() {
        assert_eq!(WeaponKind::of_id("1hSwordT2"), WeaponKind::Sword);
        assert_eq!(WeaponKind::of_id("1hLongswordT2"), WeaponKind::Sword);
        assert_eq!(
            WeaponKind::of_id("1hGreatSword_HalloweenT5"),
            WeaponKind::GreatSword
        );
        assert_eq!(WeaponKind::of_id("1hhatchetBLostT2"), WeaponKind::Hatchet);
        assert_eq!(WeaponKind::of_id("2hWarHammerT2"), WeaponKind::WarHammer);
        assert_eq!(WeaponKind::of_id("2hGreatAxeT2"), WeaponKind::GreatAxe);
        assert_eq!(WeaponKind::of_id("BlunderbussT2"), WeaponKind::Other);
    }

    const PLAYER_HEALTH: u32 = 856;
    const LEVEL: u32 = 20;
    const LEVEL_HEALTH: f64 = 1872.2;
    const LEVEL_DAMAGE: f64 = 686.1;
    const LEVEL_ARMOR: f64 = 109.336;
    const SWORD_ITEM: &str = "1hSwordT1";
    const POTION_COOLDOWN_S: u32 = 25;
    const POTION_HEAL: f64 = 400.0;
    const POTION_SHARE: f64 = 0.1;
    const BOAR_HEALTH_MOD: f64 = 0.5;
    const BOAR_DAMAGE_MOD: f64 = 0.8;
    const SWORD_BASE: f64 = 64.0;
    const LIGHT: &str = "1H_Sword_Attack1";
    const HEAVY: &str = "1H_Sword_Attack2";
    const PELLET: &str = "Blunderbuss_Primary_Attack";
    const VISION_OFFSET: f32 = 5.0;

    fn rows() -> CombatRows {
        CombatRows {
            player_vitals_id: "Player".to_owned(),
            weapon_id: "SwordT1".to_owned(),
            attack_damage_ids: vec![LIGHT.to_owned(), HEAVY.to_owned()],
            spreadshot_damage_id: PELLET.to_owned(),
        }
    }

    fn boar() -> MobType {
        MobType {
            health_mod: BOAR_HEALTH_MOD,
            damage_mod: BOAR_DAMAGE_MOD,
            creature_type: Some("Beast".to_owned()),
        }
    }

    fn source() -> CombatSource {
        CombatSource {
            base_types: vec![("Boar_20".to_owned(), boar())],
            plain_types: vec!["Grunt".to_owned(), "Boar_20".to_owned()],
            variants: vec![("Boar_20_Elite".to_owned(), "Boar_20".to_owned())],
            levels: vec![(
                LEVEL,
                LevelRow {
                    base_max_health: LEVEL_HEALTH,
                    base_damage: LEVEL_DAMAGE,
                    physical_armor_rating: LEVEL_ARMOR,
                },
            )],
            weapons: vec![(
                "SwordT1".to_owned(),
                WeaponRow {
                    base_damage: SWORD_BASE,
                    crit_chance: 0.03,
                    crit_multiplier: 1.3,
                    spread_shot: false,
                    kind: WeaponKind::Sword,
                },
            )],
            consumables: vec![(
                "PotionHealthT1".to_owned(),
                POTION_COOLDOWN_S,
                "SurvivalPotion".to_owned(),
                "PotionHeal".to_owned(),
            )],
            heals: vec![("PotionHeal".to_owned(), POTION_HEAL, POTION_SHARE)],
            // As in the retail tables: the potion has no stats row.
            item_stats: vec![
                (SWORD_ITEM.to_owned(), "SwordT1".to_owned()),
                ("Shield".to_owned(), "NoWeaponRow".to_owned()),
            ],
            attacks: vec![
                AttackRow {
                    damage_id: LIGHT.to_owned(),
                    coefficient: 1.0,
                    can_crit: true,
                    threat: 0,
                },
                AttackRow {
                    damage_id: HEAVY.to_owned(),
                    coefficient: 1.2,
                    can_crit: true,
                    threat: 1,
                },
                AttackRow {
                    damage_id: PELLET.to_owned(),
                    coefficient: 0.19,
                    can_crit: true,
                    threat: 0,
                },
            ],
            vision_offsets: vec![("Beast".to_owned(), VISION_OFFSET)],
            player_health: vec![("Player".to_owned(), PLAYER_HEALTH)],
            categories: Vec::new(),
        }
    }

    fn data() -> CombatData {
        CombatData::build(source(), &rows()).expect("data")
    }

    #[test]
    fn a_weapon_item_has_the_row_of_its_stats() {
        let data = data();
        assert_eq!(
            data.weapon_of_item("1HSWORDT1")
                .map(|weapon| weapon.base_damage),
            Some(SWORD_BASE)
        );
        assert_eq!(data.weapon_of_item("Shield"), None, "no weapon row");
        assert_eq!(
            data.heal_of_item("potionhealtht1"),
            Some(&HealingConsumable {
                cooldown_s: POTION_COOLDOWN_S,
                cooldown_id: "SurvivalPotion".to_owned(),
                health: POTION_HEAL,
                health_share: POTION_SHARE,
            })
        );
        assert_eq!(
            data.level(LEVEL).map(|level| level.physical_armor_rating),
            Some(LEVEL_ARMOR)
        );
    }

    #[test]
    fn the_first_attack_is_the_first_config_id() {
        let mut heavy_first = rows();
        heavy_first.attack_damage_ids.reverse();
        let data = CombatData::build(source(), &heavy_first).expect("data");
        assert_eq!(
            data.first_attack().map(|row| row.damage_id.as_str()),
            Some(HEAVY)
        );
    }

    #[test]
    fn a_weapon_kind_attacks_with_its_own_rows() {
        let mut source = source();
        for (damage_id, coefficient) in [
            ("Warhammer_PrimaryAttack", 1.0),
            ("Warhammer_HeavyAttack", 1.3),
        ] {
            source.attacks.push(AttackRow {
                damage_id: damage_id.to_owned(),
                coefficient,
                can_crit: true,
                threat: 0,
            });
        }
        let data = CombatData::build(source, &rows()).expect("data");
        let id = |row: Option<&AttackRow>| row.map(|row| row.damage_id.clone());
        assert_eq!(
            id(data.weapon_attack(WeaponKind::WarHammer, true)),
            Some("Warhammer_HeavyAttack".to_owned())
        );
        assert_eq!(
            data.attack("Warhammer_HeavyAttack")
                .map(|row| row.coefficient),
            Some(1.3)
        );
        // A kind without rows, or another weapon, swings the config's.
        assert_eq!(
            id(data.weapon_attack(WeaponKind::Spear, false)),
            id(data.first_attack())
        );
        assert_eq!(
            id(data.weapon_attack(WeaponKind::Other, true)),
            id(data.heavy_attack())
        );
    }

    #[test]
    fn mob_types_come_from_vitals_base_first() {
        let data = data();
        assert_eq!(data.mob_type(lowercase_crc32("boar_20")), Some(&boar()));
        assert_eq!(data.mob_type(lowercase_crc32("BOAR_20")), Some(&boar()));
    }

    #[test]
    fn a_creature_type_multiplies_the_mods() {
        let mut source = source();
        source.categories = vec![(
            "Beast".to_owned(),
            CategoryMods {
                health: 2.0,
                damage: 1.5,
            },
        )];
        let data = CombatData::build(source, &rows()).expect("data");
        let boar = data.mob_type(lowercase_crc32("Boar_20")).expect("boar");
        assert_eq!(boar.health_mod, BOAR_HEALTH_MOD * 2.0);
        assert_eq!(boar.damage_mod, BOAR_DAMAGE_MOD * 1.5);
        let elite = data
            .mob_type(lowercase_crc32("Boar_20_Elite"))
            .expect("variant");
        assert_eq!(elite.health_mod, BOAR_HEALTH_MOD * 2.0, "a variant too");
    }

    #[test]
    fn a_type_only_in_vitals_data_gets_mods_of_one() {
        let grunt = data()
            .mob_type(lowercase_crc32("Grunt"))
            .cloned()
            .expect("grunt");
        assert_eq!(grunt, MobType::PLAIN);
    }

    #[test]
    fn a_variant_takes_the_type_of_its_base() {
        assert_eq!(
            data().mob_type(lowercase_crc32("Boar_20_Elite")),
            Some(&boar())
        );
        assert_eq!(data().mob_type(lowercase_crc32("Nobody")), None);
    }

    #[test]
    fn levels_are_read_by_level() {
        let data = data();
        assert_eq!(
            data.level(LEVEL).map(|row| row.base_damage),
            Some(LEVEL_DAMAGE)
        );
        assert_eq!(data.level(LEVEL + 1), None);
    }

    #[test]
    fn missing_config_rows_are_errors() {
        let mut no_player = rows();
        no_player.player_vitals_id = "Ghost".to_owned();
        assert!(matches!(
            CombatData::build(source(), &no_player),
            Err(CombatDataError::MissingPlayer(_))
        ));
        let mut no_weapon = rows();
        no_weapon.weapon_id = "Spoon".to_owned();
        assert!(matches!(
            CombatData::build(source(), &no_weapon),
            Err(CombatDataError::MissingWeapon(_))
        ));
        let mut no_attack = rows();
        no_attack.attack_damage_ids.push("Spoon_Attack".to_owned());
        assert!(matches!(
            CombatData::build(source(), &no_attack),
            Err(CombatDataError::MissingAttack(_))
        ));
        assert!(matches!(
            CombatData::from_tables(&Tables::default(), &rows()),
            Err(CombatDataError::MissingPlayer(_))
        ));
    }

    #[test]
    fn the_player_health_is_the_initial_of_its_row() {
        assert_eq!(data().player_max_health(), PLAYER_HEALTH);
    }

    #[test]
    fn the_weapon_and_its_attacks_are_kept() {
        let data = data();
        assert_eq!(data.weapon().base_damage, SWORD_BASE);
        assert_eq!(data.attack(HEAVY).map(|row| row.threat), Some(1));
        assert_eq!(data.attack("Other"), None);
    }

    #[test]
    fn an_unknown_creature_type_has_no_vision_offset() {
        let data = data();
        assert_eq!(data.vision_offset(Some("Beast")), VISION_OFFSET);
        assert_eq!(data.vision_offset(Some("Dragon")), 0.0);
        assert_eq!(data.vision_offset(None), 0.0);
    }
}
