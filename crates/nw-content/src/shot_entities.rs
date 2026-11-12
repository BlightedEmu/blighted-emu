// SPDX-License-Identifier: AGPL-3.0-or-later
//! The projectile entity the retail server spawns for a player's shot (projectile study,
//! 2026-11-11: 203,948 projectiles of 670 retail ledgers). Each shot of a bow, a musket, a
//! staff, a gauntlet or a thrown weapon is a new entity: a card of the projectile's slice and
//! one projectile state (class 16) on the slice's own component number. The shooter's client
//! gets the same entity as every other client. Without it no player sees a shot fly.
//!
//! The table file holds one entry per damage row (the row a shot's hit uses), as retail sent
//! it most: the slice asset, the component of the projectile state, the flat index of the
//! ranged attack in the ranged attack database, the row's index in the damage table, whether
//! the card names the weapon's effect, and the slice's speed and lifetimes. It is mined from
//! the retail captures (`projshots` and its scripts of the capture tools). A damage row that no
//! capture shows has no entity here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::town_props::ASSET_ID_LEN;

const FILE_PREFIX: &str = "shot-entities-";
const FILE_SUFFIX: &str = ".json";

/// The table file of client build `client_build` in content directory `dir`.
pub fn shot_entities_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum ShotEntitiesError {
    /// The file cannot be read.
    #[error("shot entity table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("shot entity table: {0}")]
    Json(#[from] serde_json::Error),
    /// A row's asset id is not 16 bytes of hex.
    #[error("shot entity table: damage row {damage_row}: a bad asset id")]
    Asset {
        /// The damage row.
        damage_row: String,
    },
}

#[derive(Deserialize)]
struct TableFile {
    shots: Vec<ShotRow>,
}

#[derive(Deserialize)]
struct ShotRow {
    damage_row: String,
    row_index: u16,
    asset: String,
    component: u64,
    position_component: Option<u64>,
    ranged_attack: u64,
    weapon_effect: bool,
    base_speed_mps: f32,
    flying_lifetime_s: f32,
    stuck_lifetime_s: f32,
}

/// The entity of the shots of one damage row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotEntity {
    /// `damageTableRowIndex`: the row's place in the player damage table.
    pub row_index: u16,
    /// The projectile slice asset's id.
    pub asset: [u8; ASSET_ID_LEN],
    /// The component of its projectile state.
    pub component: u64,
    /// The component of its position, when its spawn group holds one (the ice gauntlet's
    /// heavy bolt).
    pub position_component: Option<u64>,
    /// `rangedAttackIndex`: the flat index of its attack in the ranged attack database.
    pub ranged_attack: u64,
    /// Its card names the weapon's damage effect (`weaponEffectCRC`: a bow, a musket, a thrown
    /// weapon).
    pub weapon_effect: bool,
    /// `m_baseSpeed`: the length of its spawn velocity, m/s.
    pub base_speed_mps: f32,
    /// `m_flyingLifetime`: how long it flies when it hits nothing, s.
    pub flying_lifetime_s: f32,
    /// `m_stuckLifetime`: how long it stays where it hit, s.
    pub stuck_lifetime_s: f32,
}

/// The shot entities, by damage row (CRC-free: the lowercase `DamageID`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShotEntities {
    by_row: HashMap<String, ShotEntity>,
}

impl ShotEntities {
    /// The table of client build `client_build` in content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`ShotEntitiesError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, ShotEntitiesError> {
        let path = shot_entities_path(dir, client_build);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| ShotEntitiesError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`ShotEntitiesError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, ShotEntitiesError> {
        let file: TableFile = serde_json::from_str(text)?;
        let by_row = file
            .shots
            .into_iter()
            .map(|row| {
                let asset = hex::decode(&row.asset)
                    .ok()
                    .and_then(|bytes| <[u8; ASSET_ID_LEN]>::try_from(bytes.as_slice()).ok())
                    .ok_or_else(|| ShotEntitiesError::Asset {
                        damage_row: row.damage_row.clone(),
                    })?;
                Ok((
                    row.damage_row.to_lowercase(),
                    ShotEntity {
                        row_index: row.row_index,
                        asset,
                        component: row.component,
                        position_component: row.position_component,
                        ranged_attack: row.ranged_attack,
                        weapon_effect: row.weapon_effect,
                        base_speed_mps: row.base_speed_mps,
                        flying_lifetime_s: row.flying_lifetime_s,
                        stuck_lifetime_s: row.stuck_lifetime_s,
                    },
                ))
            })
            .collect::<Result<_, ShotEntitiesError>>()?;
        Ok(Self { by_row })
    }

    /// The entity of the shots of damage row `damage_row` (any case).
    pub fn of(&self, damage_row: &str) -> Option<&ShotEntity> {
        self.by_row.get(&damage_row.to_lowercase())
    }

    /// How many damage rows it holds.
    pub fn len(&self) -> usize {
        self.by_row.len()
    }

    /// Whether it holds no damage row.
    pub fn is_empty(&self) -> bool {
        self.by_row.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build 25810808: the bow's hip shot and its arrow slice.
    const TABLE: &str = r#"{
        "source": "test",
        "shots": [
            {"damage_row": "BowAttack_Hipshot", "row_index": 178,
             "asset": "62020140260a501ba88c5f0f781b76e4", "component": 1,
             "position_component": null, "ranged_attack": 250, "weapon_effect": true,
             "base_speed_mps": 100.0, "flying_lifetime_s": 10.0, "stuck_lifetime_s": 8.0,
             "seen": 1234}
        ]
    }"#;

    #[test]
    fn a_shot_entity_is_found_by_its_damage_row_in_any_case() {
        let table = ShotEntities::parse(TABLE).expect("table");
        assert_eq!(table.len(), 1);
        let arrow = table.of("bowattack_hipshot").expect("the arrow");
        assert_eq!((arrow.row_index, arrow.ranged_attack), (178, 250));
        assert_eq!(arrow.asset[..2], [0x62, 0x02]);
        assert!(arrow.weapon_effect);
        assert_eq!(table.of("BowAttack1"), None);
    }

    #[test]
    fn a_bad_asset_id_is_an_error() {
        let bad = TABLE.replace("62020140260a501ba88c5f0f781b76e4", "6202");
        assert!(matches!(
            ShotEntities::parse(&bad),
            Err(ShotEntitiesError::Asset { .. })
        ));
    }
}
