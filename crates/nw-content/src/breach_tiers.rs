// SPDX-License-Identifier: AGPL-3.0-or-later
//! The breach tiers file ([`BREACH_TIERS`]): the difficulty of each corrupted breach
//! place. `content-tool breach-tiers` mines it from the world map points of the retail ledgers;
//! `content-tool dungeon-scripts` and the realm data pack read it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::layout::{BREACH_TIERS, ContentLayout, write_atomic};

/// The difficulty of one breach place.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BreachTier {
    /// The place's x, metres.
    pub x: f32,
    /// The place's y, metres.
    pub y: f32,
    /// The difficulty (the map point's number).
    pub difficulty: u32,
    /// In how many ledgers the place showed that difficulty.
    pub ledgers: u32,
}

/// Why the file cannot be read or written.
#[derive(Debug, thiserror::Error)]
pub enum BreachTiersError {
    /// A file cannot be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table's JSON.
    #[error("breach tiers: {0}")]
    Json(#[from] serde_json::Error),
}

/// The breach tiers table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BreachTiers {
    /// One entry per breach place.
    pub tiers: Vec<BreachTier>,
}

impl BreachTiers {
    /// Parses the table's JSON (a list of tiers).
    ///
    /// # Errors
    ///
    /// Returns [`BreachTiersError::Json`] when the text is not the table's JSON.
    pub fn parse(text: &str) -> Result<Self, BreachTiersError> {
        Ok(Self {
            tiers: serde_json::from_str(text)?,
        })
    }

    /// The table of `layout`; an empty table when there is no file.
    ///
    /// # Errors
    ///
    /// Returns [`BreachTiersError`] when the file is there but cannot be read.
    pub fn load(layout: &ContentLayout) -> Result<Self, BreachTiersError> {
        let path = layout.file(BREACH_TIERS);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(BreachTiersError::Io { path, source }),
        }
    }

    /// Writes the table as the file of `layout`; its path.
    ///
    /// # Errors
    ///
    /// Returns [`BreachTiersError`] when the file cannot be written.
    pub fn write(&self, layout: &ContentLayout) -> Result<PathBuf, BreachTiersError> {
        let path = layout.file(BREACH_TIERS);
        write_atomic(&path, &serde_json::to_vec_pretty(&self.tiers)?).map_err(|source| {
            BreachTiersError::Io {
                path: path.clone(),
                source,
            }
        })?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILD: &str = "12345";

    fn table() -> BreachTiers {
        BreachTiers {
            tiers: vec![
                BreachTier {
                    x: 9158.6,
                    y: 5684.1,
                    difficulty: 2,
                    ledgers: 4,
                },
                BreachTier {
                    x: 7000.0,
                    y: 3000.0,
                    difficulty: 3,
                    ledgers: 1,
                },
            ],
        }
    }

    #[test]
    fn parses_a_written_table() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        let path = table().write(&layout).unwrap();
        assert_eq!(path, layout.file(BREACH_TIERS));
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(BreachTiers::parse(&text).unwrap(), table());
        assert_eq!(BreachTiers::load(&layout).unwrap(), table());
    }

    #[test]
    fn no_file_is_an_empty_table() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            BreachTiers::load(&ContentLayout::new(dir.path(), BUILD)).unwrap(),
            BreachTiers::default()
        );
    }

    #[test]
    fn other_json_is_an_error() {
        assert!(matches!(
            BreachTiers::parse("{\"x\": 1}"),
            Err(BreachTiersError::Json(_))
        ));
    }
}
