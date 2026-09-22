// SPDX-License-Identifier: AGPL-3.0-or-later
//! The mob prefab table (`mob-prefabs-<build>.json`, made by `content-tool mob-prefabs` from the
//! retail captures; mob-cards note, sections 1 and 2).
//!
//! A mob shows on the client only when its spawn card names the asset of its creature slice; the
//! world file gives only the vitals id. The table maps a vitals id to that asset, the components
//! of its ALC and vitals states, and the bodies of the other states of its first retail group and
//! of its death. A vitals id can have more than one prefab (retail: the two tutorial legionnaires
//! share `Legion_Legionnaire_FTUE` but have their own slice assets); the table keeps each, and the
//! one seen most often is the vitals id's default.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The file name prefix of the table; the client build and `.json` follow.
const FILE_PREFIX: &str = "mob-prefabs-";
const FILE_SUFFIX: &str = ".json";
/// Bytes of a slice asset GUID.
pub const ASSET_ID_LEN: usize = 16;

/// The path of the table of `client_build` in `dir`.
pub fn mob_prefabs_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// One state of a prefab: its component, class and body bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefabMember {
    /// The component index.
    pub component: u64,
    /// The class type index.
    pub class: u64,
    /// The body bytes.
    pub body: Vec<u8>,
}

/// One mob prefab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobPrefab {
    /// The creature slice asset GUID.
    pub asset: [u8; ASSET_ID_LEN],
    /// The asset sub id.
    pub sub_id: u32,
    /// The component of the `ALCReplicatedState`.
    pub alc_component: u64,
    /// The component of the vitals state.
    pub vitals_component: u64,
    /// The other states of the first group.
    pub first_members: Vec<PrefabMember>,
    /// The other states of the death group.
    pub death_members: Vec<PrefabMember>,
    /// The paperdoll states it sends right after a fresh spawn card (its draw).
    pub drawn_members: Vec<PrefabMember>,
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum MobPrefabsError {
    /// The file cannot be read.
    #[error("mob prefab table {path}: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table's JSON.
    #[error("mob prefab table: {0}")]
    Json(#[from] serde_json::Error),
    /// A hex field is not hex or not the right length.
    #[error("mob prefab table: vitals {vitals_id}: bad hex in {field}")]
    Hex {
        /// The entry.
        vitals_id: u32,
        /// The field.
        field: &'static str,
    },
}

#[derive(Debug, Deserialize)]
struct MemberRow {
    component: u64,
    class: u64,
    body: String,
}

#[derive(Debug, Deserialize)]
struct PrefabRow {
    vitals_id: u32,
    asset: String,
    sub_id: u32,
    alc_component: u64,
    vitals_component: u64,
    first_members: Vec<MemberRow>,
    death_members: Vec<MemberRow>,
    /// Absent in tables made before the draw was recorded.
    #[serde(default)]
    drawn_members: Vec<MemberRow>,
    seen: u32,
}

/// A vitals id and the slice asset of one of its prefabs.
pub type PrefabKey = (u32, [u8; ASSET_ID_LEN]);

/// Every prefab in the table, by vitals id and asset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MobPrefabs {
    by_key: HashMap<PrefabKey, MobPrefab>,
    /// The asset of the prefab seen most often of each vitals id.
    default_asset: HashMap<u32, [u8; ASSET_ID_LEN]>,
}

impl MobPrefabs {
    /// Loads the table of `client_build` from `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`MobPrefabsError`] when the file is missing or bad.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, MobPrefabsError> {
        let path = mob_prefabs_path(dir, client_build);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| MobPrefabsError::Read { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`MobPrefabsError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, MobPrefabsError> {
        let rows: Vec<PrefabRow> = serde_json::from_str(text)?;
        let mut by_key = HashMap::new();
        let mut most_seen: HashMap<u32, (u32, [u8; ASSET_ID_LEN])> = HashMap::new();
        for row in rows {
            let seen = row.seen;
            let vitals_id = row.vitals_id;
            let prefab = prefab_of(row)?;
            if most_seen
                .get(&vitals_id)
                .is_none_or(|(most, _)| seen > *most)
            {
                most_seen.insert(vitals_id, (seen, prefab.asset));
            }
            by_key.insert((vitals_id, prefab.asset), prefab);
        }
        Ok(Self {
            by_key,
            default_asset: most_seen
                .into_iter()
                .map(|(vitals_id, (_, asset))| (vitals_id, asset))
                .collect(),
        })
    }

    /// The prefab of vitals id `vitals_id` seen most often.
    pub fn prefab(&self, vitals_id: u32) -> Option<&MobPrefab> {
        let asset = self.default_asset.get(&vitals_id)?;
        self.by_key.get(&(vitals_id, *asset))
    }

    /// The default asset of each vitals id.
    pub fn default_assets(&self) -> impl Iterator<Item = (u32, [u8; ASSET_ID_LEN])> {
        self.default_asset
            .iter()
            .map(|(vitals_id, asset)| (*vitals_id, *asset))
    }

    /// Every prefab with its vitals id and asset.
    pub fn iter(&self) -> impl Iterator<Item = (PrefabKey, &MobPrefab)> {
        self.by_key.iter().map(|(key, prefab)| (*key, prefab))
    }

    /// Prefabs in the table.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}

fn prefab_of(row: PrefabRow) -> Result<MobPrefab, MobPrefabsError> {
    let vitals_id = row.vitals_id;
    let bad = |field| MobPrefabsError::Hex { vitals_id, field };
    let asset = hex::decode(&row.asset)
        .ok()
        .and_then(|bytes| <[u8; ASSET_ID_LEN]>::try_from(bytes).ok())
        .ok_or_else(|| bad("asset"))?;
    let members = |rows: Vec<MemberRow>, field| {
        rows.into_iter()
            .map(|member| {
                Ok(PrefabMember {
                    component: member.component,
                    class: member.class,
                    body: hex::decode(&member.body).map_err(|_| bad(field))?,
                })
            })
            .collect::<Result<Vec<_>, MobPrefabsError>>()
    };
    Ok(MobPrefab {
        asset,
        sub_id: row.sub_id,
        alc_component: row.alc_component,
        vitals_component: row.vitals_component,
        first_members: members(row.first_members, "first_members")?,
        death_members: members(row.death_members, "death_members")?,
        drawn_members: members(row.drawn_members, "drawn_members")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAILOR: u32 = 2_288_616_137;
    const SAILOR_ASSET: &str = "b7e33b294bcf57b8a476abdc3a31fae7";

    fn row(asset: &str, seen: u32) -> String {
        format!(
            r#"{{"vitals_id":{SAILOR},"vitals_name":null,"asset":"{asset}","sub_id":2,
               "alc_component":5,"vitals_component":9,
               "first_members":[{{"component":6,"class":12,"body":"010104"}}],
               "death_members":[],"seen":{seen}}}"#
        )
    }

    #[test]
    fn the_most_seen_prefab_of_a_vitals_id_is_its_default() {
        let other = "00112233445566778899aabbccddeeff";
        let text = format!("[{},{}]", row(other, 1), row(SAILOR_ASSET, 3));
        let table = MobPrefabs::parse(&text).expect("table");
        assert_eq!(table.len(), 2, "each asset's prefab is kept");
        let sailor = table.prefab(SAILOR).expect("sailor");
        assert_eq!(hex::encode(sailor.asset), SAILOR_ASSET);
        assert_eq!((sailor.alc_component, sailor.vitals_component), (5, 9));
        assert_eq!(
            sailor.first_members,
            vec![PrefabMember {
                component: 6,
                class: 12,
                body: vec![1, 1, 4],
            }]
        );
        assert_eq!(table.prefab(1), None);
    }

    #[test]
    fn bad_hex_is_refused() {
        assert!(matches!(
            MobPrefabs::parse(&format!("[{}]", row("xyz", 1))),
            Err(MobPrefabsError::Hex { .. })
        ));
    }
}
