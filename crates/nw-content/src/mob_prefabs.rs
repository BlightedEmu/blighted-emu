// SPDX-License-Identifier: AGPL-3.0-or-later
//! The mob prefab table (`mob-prefabs-<build>.json`, made by `content-tool mob-prefabs` from the
//! retail captures; mob-cards note, sections 1 and 2).
//!
//! A mob shows on the client only when its spawn card names the asset of its creature slice; the
//! world file gives only the vitals id. The table maps a vitals id to that asset, the components
//! of its ALC and vitals states, and the bodies of the other states of its first retail group and
//! of its death. A vitals id with more than one prefab keeps the one seen most often.

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
    seen: u32,
}

/// The prefab of every vitals id in the table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MobPrefabs {
    by_vitals: HashMap<u32, MobPrefab>,
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
        let mut best: HashMap<u32, (u32, MobPrefab)> = HashMap::new();
        for row in rows {
            let seen = row.seen;
            let vitals_id = row.vitals_id;
            let prefab = prefab_of(row)?;
            if best.get(&vitals_id).is_none_or(|(most, _)| seen > *most) {
                best.insert(vitals_id, (seen, prefab));
            }
        }
        Ok(Self {
            by_vitals: best
                .into_iter()
                .map(|(vitals_id, (_, prefab))| (vitals_id, prefab))
                .collect(),
        })
    }

    /// The prefab of vitals id `vitals_id`.
    pub fn prefab(&self, vitals_id: u32) -> Option<&MobPrefab> {
        self.by_vitals.get(&vitals_id)
    }

    /// Every vitals id with its prefab.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &MobPrefab)> {
        self.by_vitals
            .iter()
            .map(|(vitals_id, prefab)| (*vitals_id, prefab))
    }

    /// Vitals ids in the table.
    pub fn len(&self) -> usize {
        self.by_vitals.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.by_vitals.is_empty()
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
    fn the_most_seen_prefab_of_a_vitals_id_is_kept() {
        let other = "00112233445566778899aabbccddeeff";
        let text = format!("[{},{}]", row(other, 1), row(SAILOR_ASSET, 3));
        let table = MobPrefabs::parse(&text).expect("table");
        assert_eq!(table.len(), 1);
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
