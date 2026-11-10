// SPDX-License-Identifier: AGPL-3.0-or-later
//! The props of the settlements and forts that the retail server spawns (empty towns study,
//! 2026-11-10): station buildings, the trading post building, lanterns, torches, fireplaces
//! and house plots. None is a level object for the client: retail sends each one a spawn card
//! with its slice asset and its place, and a client that gets no card shows nothing there
//! (live 2026-11-10: no trading post in Prydwen, and towns that looked empty).
//!
//! The table file holds one prop per slice folder and point, mined from the retail captures
//! (the `cardsurvey` bin and `townprops.py` of the capture tools): its slice asset and the
//! states of its spawn group as retail sent them. A settlement that no capture shows has no
//! props here.

use std::path::{Path, PathBuf};

use serde::Deserialize;

const FILE_PREFIX: &str = "town-props-";
const FILE_SUFFIX: &str = ".json";
/// The bytes of an asset id.
pub const ASSET_ID_LEN: usize = 16;

/// The table file of client build `client_build` in content directory `dir`.
pub fn town_props_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum TownPropsError {
    /// The file cannot be read.
    #[error("town props table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("town props table: {0}")]
    Json(#[from] serde_json::Error),
    /// A prop's hex does not read.
    #[error("town props table: prop {index}: {source}")]
    Hex {
        /// The prop's place in the file.
        index: usize,
        /// Why.
        source: hex::FromHexError,
    },
    /// A prop's asset id is not 16 bytes.
    #[error("town props table: prop {index}: an asset id of {len} bytes")]
    AssetLen {
        /// The prop's place in the file.
        index: usize,
        /// The bytes it has.
        len: usize,
    },
}

#[derive(Deserialize)]
struct TableFile {
    props: Vec<PropRow>,
}

#[derive(Deserialize)]
struct PropRow {
    asset: String,
    members: Vec<String>,
}

/// One prop: its slice asset and the states of its spawn group, each as the bytes of one
/// bundle member (component, class, body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TownProp {
    /// The slice asset's id.
    pub asset: [u8; ASSET_ID_LEN],
    /// Its states.
    pub members: Vec<Vec<u8>>,
}

/// The props of a level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TownProps {
    props: Vec<TownProp>,
}

impl TownProps {
    /// The table of client build `client_build` in content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`TownPropsError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, TownPropsError> {
        let path = town_props_path(dir, client_build);
        let text =
            std::fs::read_to_string(&path).map_err(|source| TownPropsError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`TownPropsError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, TownPropsError> {
        let file: TableFile = serde_json::from_str(text)?;
        let props = file
            .props
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let bytes = |text: &str| {
                    hex::decode(text).map_err(|source| TownPropsError::Hex { index, source })
                };
                let asset = bytes(&row.asset)?;
                let asset = <[u8; ASSET_ID_LEN]>::try_from(asset.as_slice()).map_err(|_| {
                    TownPropsError::AssetLen {
                        index,
                        len: asset.len(),
                    }
                })?;
                let members = row
                    .members
                    .iter()
                    .map(|member| bytes(member))
                    .collect::<Result<_, _>>()?;
                Ok(TownProp { asset, members })
            })
            .collect::<Result<_, TownPropsError>>()?;
        Ok(Self { props })
    }

    /// The props.
    pub fn iter(&self) -> impl Iterator<Item = &TownProp> {
        self.props.iter()
    }

    /// How many props it holds.
    pub const fn len(&self) -> usize {
        self.props.len()
    }

    /// Whether it holds no prop.
    pub const fn is_empty(&self) -> bool {
        self.props.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Prydwen trading post building of the retail captures.
    const TABLE: &str = r#"{"props": [{
        "asset": "8ef76855695d54529c2cb3e727280bbf",
        "slice": "slices/structures/crafting/trading/craftingstation_t5_tradingpost_complete.dynamicslice",
        "members": ["010d010345e5aba94569fb6d19c803986b"],
        "cards": 74
    }]}"#;

    #[test]
    fn a_prop_reads_with_its_asset_and_states() {
        let table = TownProps::parse(TABLE).expect("table");
        assert_eq!(table.len(), 1);
        let prop = table.iter().next().expect("prop");
        assert_eq!(prop.asset[..2], [0x8e, 0xf7]);
        assert_eq!(
            prop.members,
            [hex::decode("010d010345e5aba94569fb6d19c803986b").expect("hex")]
        );
    }

    #[test]
    fn a_bad_asset_or_hex_is_an_error() {
        let short = r#"{"props": [{"asset": "8ef7", "members": []}]}"#;
        assert!(matches!(
            TownProps::parse(short),
            Err(TownPropsError::AssetLen { index: 0, len: 2 })
        ));
        let bad = r#"{"props": [{"asset": "zz", "members": []}]}"#;
        assert!(matches!(
            TownProps::parse(bad),
            Err(TownPropsError::Hex { index: 0, .. })
        ));
    }
}
