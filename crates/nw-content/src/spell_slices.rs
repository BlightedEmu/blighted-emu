// SPDX-License-Identifier: AGPL-3.0-or-later
//! The slice the retail server spawns for a spell (spell entity study, 2026-11-10): Sacred
//! Ground's circle, an impact, a detection volume. A cast spawns an entity with a card of the
//! slice, a spell state (class 2912: the spell, its place, its caster) and the other states of
//! the slice. Without it a client shows nothing where the spell is (live 2026-11-10: "ground
//! spells are broken (the display and the actual effect)").
//!
//! The table file holds, per spell data id, the slice asset, the component numbers of the
//! spell state, the ownership and the position, and the states other than those three, as
//! retail sent them most (mined from the retail captures: the `cardsurvey` bin and
//! `spellslices.py` of the capture tools). A spell that no capture shows has no slice here.
//!
//! Each slice numbers its replicated components in its own order (Observed: the upgraded Flail
//! Slam sends its position on component 2 and its spell state on 4; Sacred Ground its spell
//! state on 1 and its ownership on 2). A state on another component's number is read by the
//! wrong component: the spell state on component 1 of the Flail Slam crashed the client in
//! `FUN_14361d110` (live 2026-11-10 and 2026-11-11).
//!
//! `unfilled` names the states (`component:class`) that retail sent but whose bytes the
//! captures do not give (ALC 11, vitals 15): the server cannot fill them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::town_props::ASSET_ID_LEN;

const FILE_PREFIX: &str = "spell-slices-";
const FILE_SUFFIX: &str = ".json";

/// The table file of client build `client_build` in content directory `dir`.
pub fn spell_slices_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum SpellSlicesError {
    /// The file cannot be read.
    #[error("spell slice table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("spell slice table: {0}")]
    Json(#[from] serde_json::Error),
    /// A spell's hex does not read.
    #[error("spell slice table: spell {spell}: {source}")]
    Hex {
        /// The spell data id.
        spell: u32,
        /// Why.
        source: hex::FromHexError,
    },
    /// A spell's asset id is not 16 bytes.
    #[error("spell slice table: spell {spell}: an asset id of {len} bytes")]
    AssetLen {
        /// The spell data id.
        spell: u32,
        /// The bytes it has.
        len: usize,
    },
}

#[derive(Deserialize)]
struct TableFile {
    spells: Vec<SpellRow>,
}

#[derive(Deserialize)]
struct SpellRow {
    spell: u32,
    asset: String,
    spell_component: u64,
    ownership_component: Option<u64>,
    position_component: Option<u64>,
    members: Vec<String>,
    unfilled: Vec<String>,
}

/// The slice of one spell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellSlice {
    /// The slice asset's id.
    pub asset: [u8; ASSET_ID_LEN],
    /// The component of its spell state.
    pub spell_component: u64,
    /// The component of the caster's ownership, when its spawn group holds one.
    pub ownership_component: Option<u64>,
    /// The component of its position, when its spawn group holds one.
    pub position_component: Option<u64>,
    /// Its other states, each as the bytes of one bundle member (component, class, body).
    pub members: Vec<Vec<u8>>,
    /// The states retail sent that the captures give no bytes for (`component:class`).
    pub unfilled: Vec<String>,
}

/// The slices of the spells, by spell data id (CRC-32 of the lowercase `SpellData` id).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpellSlices {
    by_spell: HashMap<u32, SpellSlice>,
}

impl SpellSlices {
    /// The table of client build `client_build` in content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`SpellSlicesError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, SpellSlicesError> {
        let path = spell_slices_path(dir, client_build);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| SpellSlicesError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`SpellSlicesError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, SpellSlicesError> {
        let file: TableFile = serde_json::from_str(text)?;
        let by_spell = file
            .spells
            .iter()
            .map(|row| {
                let spell = row.spell;
                let bytes = |text: &str| {
                    hex::decode(text).map_err(|source| SpellSlicesError::Hex { spell, source })
                };
                let asset = bytes(&row.asset)?;
                let asset = <[u8; ASSET_ID_LEN]>::try_from(asset.as_slice()).map_err(|_| {
                    SpellSlicesError::AssetLen {
                        spell,
                        len: asset.len(),
                    }
                })?;
                let members = row
                    .members
                    .iter()
                    .map(|member| bytes(member))
                    .collect::<Result<_, _>>()?;
                Ok((
                    spell,
                    SpellSlice {
                        asset,
                        spell_component: row.spell_component,
                        ownership_component: row.ownership_component,
                        position_component: row.position_component,
                        members,
                        unfilled: row.unfilled.clone(),
                    },
                ))
            })
            .collect::<Result<_, SpellSlicesError>>()?;
        Ok(Self { by_spell })
    }

    /// The slice of spell `spell`.
    pub fn of(&self, spell: u32) -> Option<&SpellSlice> {
        self.by_spell.get(&spell)
    }

    /// How many spells it holds.
    pub fn len(&self) -> usize {
        self.by_spell.len()
    }

    /// Whether it holds no spell.
    pub fn is_empty(&self) -> bool {
        self.by_spell.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Spell_LifeStaff_SacredGround_Upgrade2` of the retail captures.
    const SPELL: u32 = 0x4b41_4d79;
    const TABLE: &str = r#"{"spells": [{
        "spell": 1262570873,
        "asset": "094588f7778354589543449b79e2a386",
        "spell_component": 1,
        "ownership_component": 2,
        "position_component": null,
        "members": ["0483310103420000", "05833101032a6600"],
        "unfilled": [],
        "cards": 8253
    }, {
        "spell": 3276096686,
        "asset": "3e1b0819440a5b919418bd82cf136b90",
        "spell_component": 4,
        "ownership_component": null,
        "position_component": 2,
        "members": ["0583310103450000"],
        "unfilled": [],
        "cards": 581
    }]}"#;
    /// `Spell_Flail_Slam_AOE_Upgrade` of the retail captures.
    const FLAIL_SLAM: u32 = 3_276_096_686;

    #[test]
    fn a_spell_reads_with_its_slice_and_states() {
        let table = SpellSlices::parse(TABLE).expect("table");
        assert_eq!(table.len(), 2);
        let slice = table.of(SPELL).expect("slice");
        assert_eq!(
            (slice.spell_component, slice.ownership_component),
            (1, Some(2))
        );
        assert_eq!(slice.position_component, None);
        assert_eq!(slice.asset[..2], [0x09, 0x45]);
        assert_eq!(slice.members.len(), 2);
        assert_eq!(table.of(1), None);
    }

    #[test]
    fn each_slice_keeps_its_own_component_numbers() {
        let table = SpellSlices::parse(TABLE).expect("table");
        let slam = table.of(FLAIL_SLAM).expect("slice");
        assert_eq!(slam.spell_component, 4);
        assert_eq!(slam.position_component, Some(2));
        assert_eq!(slam.ownership_component, None);
    }

    #[test]
    fn a_bad_asset_is_an_error() {
        let short = r#"{"spells": [{"spell": 7, "asset": "09", "spell_component": 1,
            "ownership_component": null, "position_component": null, "members": [],
            "unfilled": []}]}"#;
        assert!(matches!(
            SpellSlices::parse(short),
            Err(SpellSlicesError::AssetLen { spell: 7, len: 1 })
        ));
    }
}
