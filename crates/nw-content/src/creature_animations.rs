// SPDX-License-Identifier: AGPL-3.0-or-later
//! The creature animation table (`creature-animations-<build>.json`, made by `content-tool
//! creature-animations` from the game paks; creature-animation note, 2026-09-18).
//!
//! A mob's replicated animation names its fragment, its action and its tag state by numbers of
//! its creature's Mannequin and CAGE files. The table keeps, per creature slice asset, the names
//! behind those numbers: the fragments (`*_controllerdefs.xml`), the scopes, the actions (the
//! CAGE grid), the bits of each tag (`*tags.xml`), and the tags the creature always has (its
//! slice's default tags and the tags of its main-hand weapon).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use nw_formats::mannequin::{TAG_STATE_LEN, TagBits, TagDefinition};
use serde::{Deserialize, Serialize};

use crate::mob_prefabs::ASSET_ID_LEN;

const FILE_PREFIX: &str = "creature-animations-";
const FILE_SUFFIX: &str = ".json";
/// Millimetres per metre.
const MM_PER_M: f32 = 1_000.0;

/// The path of the table of `client_build` in `dir`.
pub fn creature_animations_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum CreatureAnimationsError {
    /// The file cannot be read or written.
    #[error("creature animation table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table's JSON.
    #[error("creature animation table: {0}")]
    Json(#[from] serde_json::Error),
    /// An asset is not 16 bytes of hex.
    #[error("creature animation table: bad asset hex {0}")]
    Asset(String),
}

/// One tag's bits: byte, shift, value.
type TagRow = [u8; 3];

/// One row of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatureRow {
    /// The creature slice asset GUID, hex.
    pub asset: String,
    /// The creature slice (`.dynamicslice`).
    pub slice: String,
    /// The fragment names in controller order.
    pub fragments: Vec<String>,
    /// The scope names in controller order.
    pub scopes: Vec<String>,
    /// The action names in grid order.
    pub actions: Vec<String>,
    /// The bits of each tag of its tag definition.
    pub tags: BTreeMap<String, TagRow>,
    /// The tags it always has: its slice's default tags and its main-hand weapon's tags.
    pub default_tags: Vec<String>,
    /// How long each option of each fragment lasts for the creature's default tags (the entries
    /// Mannequin picks among: the most tags, in file order), in whole scope ticks (1/30 s,
    /// `DAT_14807f948`); `None` for an option that loops or whose length is Unknown. A fragment
    /// without an option of known length is left out; a table from before this field has none.
    #[serde(default)]
    pub fragment_ticks: BTreeMap<String, Vec<Option<u32>>>,
    /// The radius of its body's capsule, mm: the query shape of its slice's component whose
    /// filter is an `AICharacterCollider` one (a filter that blocks the player's,
    /// `default.collisionfilters`); `None` when its slice has none, or the table is older.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collider_radius_mm: Option<u32>,
    /// Its corpse can be skinned: it stays until it is skinned, then
    /// [`crate::corpse_times::SKINNED_CORPSE_MS`]; else it goes after its corpse time
    /// ([`crate::corpse_times::corpse_ms`]). `false` in a table from before this field.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skinnable: bool,
}

/// One creature's animation names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatureAnimation {
    /// The creature slice.
    pub slice: String,
    fragments: Vec<String>,
    scopes: Vec<String>,
    actions: Vec<String>,
    tags: TagDefinition,
    default_tags: Vec<String>,
    fragment_ticks: BTreeMap<String, Vec<Option<u32>>>,
    collider_radius_mm: Option<u32>,
    skinnable: bool,
}

/// The 1-based number of `name` in `names`.
fn number_of(names: &[String], name: &str) -> Option<u16> {
    let index = names.iter().position(|known| known == name)?;
    u16::try_from(index + 1).ok()
}

impl CreatureAnimation {
    /// Its corpse can be skinned ([`CreatureRow::skinnable`]).
    pub const fn skinnable(&self) -> bool {
        self.skinnable
    }

    /// The radius of its body's capsule, m ([`CreatureRow::collider_radius_mm`]).
    pub fn collider_radius_m(&self) -> Option<f32> {
        // A radius of a few metres in millimetres is exact in an f32.
        #[allow(clippy::cast_precision_loss)]
        self.collider_radius_mm.map(|mm| mm as f32 / MM_PER_M)
    }

    /// The number of fragment `name` (its index + 1 in the controller definitions).
    pub fn fragment(&self, name: &str) -> Option<u16> {
        number_of(&self.fragments, name)
    }

    /// The fragment names in controller order.
    pub fn fragments(&self) -> &[String] {
        &self.fragments
    }

    /// The number of action `name` (its index + 1 in the grid).
    pub fn action(&self, name: &str) -> Option<u16> {
        number_of(&self.actions, name)
    }

    /// The `scopeData` bit of scope `name` (its index in the controller definitions).
    pub fn scope_bit(&self, name: &str) -> Option<u8> {
        let index = self.scopes.iter().position(|known| known == name)?;
        u8::try_from(index).ok()
    }

    /// How long fragment `name` lasts in whole scope ticks ([`CreatureRow::fragment_ticks`]) in
    /// the option that an entry of last byte `tail` plays: option `tail` modulo the option
    /// count (Inferred from the retail dead states: the sailor's `r_Death_F` tail 0xa5 of 5
    /// options played option 0, 56 ticks, its last tick 0x38; the risen's `r_Death` tail 0x8f of
    /// 2 options played option 1, 73 ticks, 0x49).
    pub fn fragment_ticks(&self, name: &str, tail: u8) -> Option<u32> {
        let options = self.fragment_ticks.get(name)?;
        let option = usize::from(tail).checked_rem(options.len())?;
        options.get(option).copied().flatten()
    }

    /// Whether the creature has tag `name`.
    pub fn has_tag(&self, name: &str) -> bool {
        self.tags.has(name)
    }

    /// The tag state of its default tags without `removed` and with `added`; a tag its
    /// definition does not have sets nothing.
    pub fn tag_state(&self, removed: &[&str], added: &[&str]) -> [u8; TAG_STATE_LEN] {
        self.tags.state(
            self.default_tags
                .iter()
                .map(String::as_str)
                .filter(|name| !removed.contains(name))
                .chain(added.iter().copied()),
        )
    }
}

/// Every creature in the table, by slice asset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreatureAnimations {
    by_asset: HashMap<[u8; ASSET_ID_LEN], CreatureAnimation>,
}

impl CreatureAnimations {
    /// Loads the table of `client_build` from `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`CreatureAnimationsError`] when the file is missing or bad.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, CreatureAnimationsError> {
        let path = creature_animations_path(dir, client_build);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| CreatureAnimationsError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`CreatureAnimationsError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, CreatureAnimationsError> {
        let rows: Vec<CreatureRow> = serde_json::from_str(text)?;
        Self::of_rows(rows)
    }

    /// The table of `rows`.
    ///
    /// # Errors
    ///
    /// Returns [`CreatureAnimationsError::Asset`] when an asset is not 16 bytes of hex.
    pub fn of_rows(rows: Vec<CreatureRow>) -> Result<Self, CreatureAnimationsError> {
        let by_asset = rows
            .into_iter()
            .map(|row| {
                let asset = hex::decode(&row.asset)
                    .ok()
                    .and_then(|bytes| <[u8; ASSET_ID_LEN]>::try_from(bytes).ok())
                    .ok_or_else(|| CreatureAnimationsError::Asset(row.asset.clone()))?;
                let tags = row
                    .tags
                    .into_iter()
                    .map(|(name, [byte, shift, value])| (name, TagBits { byte, shift, value }))
                    .collect();
                Ok((
                    asset,
                    CreatureAnimation {
                        slice: row.slice,
                        fragments: row.fragments,
                        scopes: row.scopes,
                        actions: row.actions,
                        tags,
                        default_tags: row.default_tags,
                        fragment_ticks: row.fragment_ticks,
                        collider_radius_mm: row.collider_radius_mm,
                        skinnable: row.skinnable,
                    },
                ))
            })
            .collect::<Result<_, CreatureAnimationsError>>()?;
        Ok(Self { by_asset })
    }

    /// The creature of slice asset `asset`, when the table has it.
    pub fn get(&self, asset: &[u8; ASSET_ID_LEN]) -> Option<&CreatureAnimation> {
        self.by_asset.get(asset)
    }

    /// Every creature with its slice asset.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8; ASSET_ID_LEN], &CreatureAnimation)> {
        self.by_asset.iter()
    }

    /// Creatures in the table.
    pub fn len(&self) -> usize {
        self.by_asset.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.by_asset.is_empty()
    }
}

/// Writes `rows` as the table file at `path`, creating parent folders.
///
/// # Errors
///
/// Returns [`CreatureAnimationsError`] when the file cannot be written.
pub fn write_rows(path: &Path, rows: &[CreatureRow]) -> Result<(), CreatureAnimationsError> {
    let io = |source| CreatureAnimationsError::Io {
        path: path.to_path_buf(),
        source,
    };
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(io)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(rows)?).map_err(io)
}

/// The row of a tag's bits.
pub const fn tag_row(bits: TagBits) -> TagRow {
    [bits.byte, bits.shift, bits.value]
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOAR: &str = "db96d765b50459d98c9e04adaa9c9485";

    fn boar_row() -> CreatureRow {
        CreatureRow {
            asset: BOAR.to_owned(),
            slice: "slices/characters/boar.dynamicslice".to_owned(),
            fragments: vec!["Idle".to_owned(), "Move".to_owned()],
            scopes: vec![
                "FullBody".to_owned(),
                "Additive".to_owned(),
                "LookPose".to_owned(),
            ],
            actions: vec!["Spawn".to_owned(), "Move".to_owned()],
            tags: BTreeMap::from([
                ("Boar".to_owned(), [0, 0, 2]),
                ("Alerted".to_owned(), [0, 5, 1]),
            ]),
            default_tags: vec!["Boar".to_owned()],
            fragment_ticks: BTreeMap::from([
                ("r_Death".to_owned(), vec![Some(88)]),
                ("Alert".to_owned(), vec![Some(56), Some(73), None]),
            ]),
            collider_radius_mm: Some(500),
            skinnable: true,
        }
    }

    #[test]
    fn a_row_gives_numbers_and_tag_states() {
        let text = serde_json::to_string(&[boar_row()]).expect("json");
        let table = CreatureAnimations::parse(&text).expect("table");
        let asset = <[u8; ASSET_ID_LEN]>::try_from(hex::decode(BOAR).expect("hex")).expect("len");
        let boar = table.get(&asset).expect("boar");
        assert_eq!(
            (boar.fragment("Move"), boar.action("Move")),
            (Some(2), Some(2))
        );
        assert_eq!(boar.fragment("Attack_Gore"), None);
        assert_eq!(boar.scope_bit("LookPose"), Some(2));
        assert_eq!(boar.tag_state(&[], &[])[0], 0x02);
        assert_eq!(boar.tag_state(&[], &["Alerted"])[0], 0x22);
        assert_eq!(boar.tag_state(&["Boar"], &[])[0], 0x00);
        assert!(boar.has_tag("Alerted") && !boar.has_tag("Phase_2"));
        assert_eq!(boar.fragment_ticks("r_Death", 0xe9), Some(88));
        assert_eq!(boar.fragment_ticks("Idle", 0), None, "not in the table");
        // Three options: the tail picks one, modulo 3.
        assert_eq!(boar.fragment_ticks("Alert", 3), Some(56));
        assert_eq!(boar.fragment_ticks("Alert", 4), Some(73));
        assert_eq!(boar.fragment_ticks("Alert", 0x8f), None, "a looping option");
        assert_eq!(boar.collider_radius_m(), Some(0.5));
    }

    #[test]
    fn a_table_without_fragment_lengths_still_reads() {
        let text = r#"[{"asset":"db96d765b50459d98c9e04adaa9c9485","slice":"s","fragments":["r_Death"],"scopes":[],"actions":[],"tags":{},"default_tags":[]}]"#;
        let table = CreatureAnimations::parse(text).expect("table");
        let asset = <[u8; ASSET_ID_LEN]>::try_from(hex::decode(BOAR).expect("hex")).expect("len");
        let row = table.get(&asset).expect("row");
        assert_eq!(row.fragment_ticks("r_Death", 0), None);
        assert_eq!(row.collider_radius_m(), None, "no collider radius either");
    }

    #[test]
    fn a_bad_asset_is_refused() {
        let row = CreatureRow {
            asset: "db96".to_owned(),
            ..boar_row()
        };
        assert!(matches!(
            CreatureAnimations::of_rows(vec![row]),
            Err(CreatureAnimationsError::Asset(_))
        ));
    }
}
