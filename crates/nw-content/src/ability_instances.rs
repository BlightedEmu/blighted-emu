// SPDX-License-Identifier: AGPL-3.0-or-later
//! The ability instance types of the spell slices and the player's instance limits (ability
//! instance tracking study, 2026-11-12), written by `content-tool ability-instances` from the
//! game paks.
//!
//! Observed (build 25810808): 26 slices hold an `AbilityInstanceComponent` (type
//! [`INSTANCE_COMPONENT`]) whose `m_type` names the `AbilityInstanceType` of the entity, for
//! example `spell_aoe_heal_foe` 3 (Sacred Ground), `spell_life_staff_beacon` 4, `trap_bear` 0.
//! `slices/player.dynamicslice` holds the `AbilityInstanceTrackingComponent` (type
//! [`TRACKING_COMPONENT`]) whose `m_instanceLimits` gives `m_maxNumInstances` per type: 0 → 2,
//! 1 → 0, 2 → 0, 3 → 1, 4 → 1, 5 → 2, 6 → 5, 7 → 0, 8 → 1; types 9 and 10 have no entry.
//!
//! Observed (71 ledgers): a new instance takes the first free slot of its type's row (Ice Shower
//! slots 0 to 4 with 0 to 4 others there; a trap in slot 0 while slot 1 is busy), also for a type
//! whose limit is 0 (Ice Pylon, Sandworm, Oblivion in slot 0). Inferred: a limit of 0 or a type
//! without an entry has no limit but the row's [`SLOTS_PER_TYPE`] slots; a new instance of a full
//! row takes the place of the oldest (no capture shows a full row).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nw_formats::objectstream::{Element, lowercase_crc32};
use serde::{Deserialize, Serialize};

use crate::town_props::ASSET_ID_LEN;

/// `AbilityInstanceComponent` (`{F36CE16E-FFA3-441E-ABA6-0A5F20874347}`, `FUN_14259c500`).
pub const INSTANCE_COMPONENT: [u8; ASSET_ID_LEN] = [
    0xf3, 0x6c, 0xe1, 0x6e, 0xff, 0xa3, 0x44, 0x1e, 0xab, 0xa6, 0x0a, 0x5f, 0x20, 0x87, 0x43, 0x47,
];
/// `AbilityInstanceTrackingComponent` (`{230A148B-879D-49DC-AD39-8AEA81152BA1}`,
/// `FUN_14259c700`).
pub const TRACKING_COMPONENT: [u8; ASSET_ID_LEN] = [
    0x23, 0x0a, 0x14, 0x8b, 0x87, 0x9d, 0x49, 0xdc, 0xad, 0x39, 0x8a, 0xea, 0x81, 0x15, 0x2b, 0xa1,
];
/// Slots of one type's row of the replicated state (client `FUN_142593330`).
pub const SLOTS_PER_TYPE: usize = 5;
/// The player slice that holds the limits.
pub const PLAYER_SLICE: &str = "slices/player.dynamicslice";
const TYPE_FIELD: &str = "m_type";
const LIMITS_FIELD: &str = "m_instanceLimits";
const MAX_FIELD: &str = "m_maxNumInstances";
const FILE_PREFIX: &str = "ability-instances-";
const FILE_SUFFIX: &str = ".json";

/// The table file of client build `client_build` in content directory `dir`.
pub fn ability_instances_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A slice with an ability instance type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceSlice {
    /// The slice's pak path.
    pub path: String,
    /// Its `AbilityInstanceType`.
    pub instance_type: u8,
}

/// Why the file cannot be read or written.
#[derive(Debug, thiserror::Error)]
pub enum AbilityInstancesError {
    /// The file cannot be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("{path}: {source}")]
    Json {
        /// The file.
        path: PathBuf,
        /// Why.
        source: serde_json::Error,
    },
}

/// The instance types of the slices, by slice asset id (lowercase hex), and the limits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbilityInstanceTable {
    /// `m_maxNumInstances` by `AbilityInstanceType`.
    pub limits: BTreeMap<u8, u32>,
    /// The slices by asset id.
    pub slices: BTreeMap<String, InstanceSlice>,
}

/// The `AbilityInstanceType` of a slice: the `m_type` of its `AbilityInstanceComponent`.
pub fn instance_type_of(root: &Element) -> Option<u8> {
    find(root, &INSTANCE_COMPONENT)?
        .child(lowercase_crc32(TYPE_FIELD))?
        .as_u8()
        .ok()
}

/// The `m_maxNumInstances` by type of the player slice's `AbilityInstanceTrackingComponent`.
pub fn limits_of(player_root: &Element) -> BTreeMap<u8, u32> {
    let Some(limits) = find(player_root, &TRACKING_COMPONENT)
        .and_then(|component| component.child(lowercase_crc32(LIMITS_FIELD)))
    else {
        return BTreeMap::new();
    };
    limits
        .children
        .iter()
        .filter_map(|limit| {
            let instance_type = limit.child(lowercase_crc32(TYPE_FIELD))?.as_u8().ok()?;
            let max = limit.child(lowercase_crc32(MAX_FIELD))?.as_u32().ok()?;
            Some((instance_type, max))
        })
        .collect()
}

/// The first element of type `type_id` in `root` and below.
fn find<'a>(root: &'a Element, type_id: &[u8; ASSET_ID_LEN]) -> Option<&'a Element> {
    if &root.type_id == type_id {
        return Some(root);
    }
    root.children.iter().find_map(|child| find(child, type_id))
}

impl AbilityInstanceTable {
    /// Reads `path`; a missing file has no instance types (content from before this file).
    ///
    /// # Errors
    ///
    /// Returns [`AbilityInstancesError`] when the file cannot be read or is not the table.
    pub fn load(path: &Path) -> Result<Self, AbilityInstancesError> {
        match std::fs::read(path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|source| AbilityInstancesError::Json {
                    path: path.to_path_buf(),
                    source,
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(AbilityInstancesError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Writes the file to `path`.
    ///
    /// # Errors
    ///
    /// Returns [`AbilityInstancesError`] when the table or the file cannot be written.
    pub fn write(&self, path: &Path) -> Result<(), AbilityInstancesError> {
        let text =
            serde_json::to_string_pretty(self).map_err(|source| AbilityInstancesError::Json {
                path: path.to_path_buf(),
                source,
            })?;
        std::fs::write(path, text).map_err(|source| AbilityInstancesError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// The instance type of the slice asset `asset`.
    pub fn type_of(&self, asset: &[u8; ASSET_ID_LEN]) -> Option<u8> {
        self.slices
            .get(&hex::encode(asset))
            .map(|slice| slice.instance_type)
    }

    /// How many instances of `instance_type` the player may have at once: its limit, or the
    /// whole row for a limit of 0 or no limit (Inferred, see the module notes).
    pub fn slots_for(&self, instance_type: u8) -> usize {
        match self.limits.get(&instance_type) {
            Some(&max) if max > 0 => {
                usize::try_from(max).map_or(SLOTS_PER_TYPE, |max| max.min(SLOTS_PER_TYPE))
            }
            _ => SLOTS_PER_TYPE,
        }
    }

    /// Slices in the table.
    pub fn len(&self) -> usize {
        self.slices.len()
    }

    /// Whether the table has no slice.
    pub fn is_empty(&self) -> bool {
        self.slices.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ELEMENT: &str = "element";
    const BASE_TYPE: [u8; ASSET_ID_LEN] = [7; ASSET_ID_LEN];

    fn typed(type_id: [u8; ASSET_ID_LEN], children: Vec<Element>) -> Element {
        Element {
            name_crc: Some(lowercase_crc32(ELEMENT)),
            type_id,
            children,
            ..Element::default()
        }
    }

    fn limit(instance_type: u8, max: u32) -> Element {
        Element::with_children(
            lowercase_crc32(ELEMENT),
            vec![
                Element::with_value(lowercase_crc32(TYPE_FIELD), vec![instance_type]),
                Element::with_value(lowercase_crc32(MAX_FIELD), max.to_be_bytes().to_vec()),
            ],
        )
    }

    /// The shape of `spell_aoe_heal_foe.dynamicslice`: the component under the entity, with its
    /// base class and `m_type` 3.
    #[test]
    fn a_slice_names_the_type_of_its_instance_component() {
        let slice = Element::with_children(
            0,
            vec![typed(
                INSTANCE_COMPONENT,
                vec![
                    typed(BASE_TYPE, Vec::new()),
                    Element::with_value(lowercase_crc32(TYPE_FIELD), vec![3]),
                ],
            )],
        );
        assert_eq!(instance_type_of(&slice), Some(3));
        assert_eq!(instance_type_of(&typed(BASE_TYPE, Vec::new())), None);
    }

    /// The retail player slice's limits (build 25810808).
    #[test]
    fn the_player_slice_gives_the_limits_and_the_slots() {
        let retail = [
            (0, 2),
            (1, 0),
            (2, 0),
            (3, 1),
            (4, 1),
            (5, 2),
            (6, 5),
            (7, 0),
            (8, 1),
        ];
        let player = typed(
            TRACKING_COMPONENT,
            vec![Element::with_children(
                lowercase_crc32(LIMITS_FIELD),
                retail.iter().map(|&(t, max)| limit(t, max)).collect(),
            )],
        );
        let table = AbilityInstanceTable {
            limits: limits_of(&player),
            slices: BTreeMap::new(),
        };
        assert_eq!(table.limits, retail.into_iter().collect());
        assert_eq!(table.slots_for(3), 1, "one Sacred Ground");
        assert_eq!(table.slots_for(0), 2, "two traps");
        assert_eq!(table.slots_for(6), SLOTS_PER_TYPE, "five Ice Showers");
        assert_eq!(
            table.slots_for(1),
            SLOTS_PER_TYPE,
            "a limit of 0 has the row"
        );
        assert_eq!(table.slots_for(9), SLOTS_PER_TYPE, "no entry has the row");
    }

    #[test]
    fn the_file_round_trips_and_a_missing_one_is_empty() {
        let dir = std::env::temp_dir().join(format!("ability-instances-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = ability_instances_path(&dir, "1");
        let asset = [0x09; ASSET_ID_LEN];
        let table = AbilityInstanceTable {
            limits: [(3, 1)].into_iter().collect(),
            slices: [(
                hex::encode(asset),
                InstanceSlice {
                    path: "slices/items/weapons/spell_aoe_heal_foe.dynamicslice".into(),
                    instance_type: 3,
                },
            )]
            .into_iter()
            .collect(),
        };
        table.write(&path).expect("write");
        let read = AbilityInstanceTable::load(&path).expect("load");
        assert_eq!(read, table);
        assert_eq!(read.type_of(&asset), Some(3));
        assert_eq!(read.type_of(&[0; ASSET_ID_LEN]), None);
        std::fs::remove_file(&path).expect("remove");
        assert!(
            AbilityInstanceTable::load(&path)
                .expect("missing")
                .is_empty()
        );
    }
}
