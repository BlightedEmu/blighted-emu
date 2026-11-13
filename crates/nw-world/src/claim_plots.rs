// SPDX-License-Identifier: AGPL-3.0-or-later
//! The claim plot slices (`claim-plots-<build>.json`, written by `content-tool claim-plots`,
//! read by the open world sims): each `.dynamicslice` under [`CLAIM_PLOT_FOLDER`] with a land
//! claim component, and the components of the spawn group of its land claim entity.
//!
//! Observed (every class-62 body of the capture index, 81 bodies in 22 ledgers): the land claim
//! entity of a territory is the level capital `Darkness/Zone_CapitalPlot_<name>` at the
//! territory's claim position (`fort_capping_pvp_flagged` 1.932 s: the card's `GdeRef`
//! `7b455362d7d03651` is the capital `Zone_CapitalPlot_WindswardShores`, 0.004 m from claim 10's
//! position). Its group holds the card (component 0), the ownership, the buildable controller,
//! the placement obstruction, the land claim state and one interact state; each member's
//! component number is the `m_replicationIndex` of the slice component of its class (1, 2, 5, 7
//! and 8 in the slices with nine replicated components, 1, 2, 4, 5 and 7 in `Dunwood` and
//! `Edengrove`, `Playthrough_Part_21` 5376.747 s). The slices have two interact components;
//! retail sent the state of the one with the highest index ([`ClaimPlotSlice::interact`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nw_formats::objectstream::{Element, lowercase_crc32};
use serde::{Deserialize, Serialize};

use crate::camps::INTERACT;
use crate::capitals::capital_id;
use crate::records::{Capital, Position};
use crate::stations::{
    BUILDABLE_CONTROLLER, OWNERSHIP, PLACEMENT_OBSTRUCTION, find, replication_index, small, text,
};
use crate::travel_slices::{UUID_LEN, descendants, uuid};

/// The file name prefix; the client build and [`CLAIM_PLOTS_EXTENSION`] follow.
pub const CLAIM_PLOTS_PREFIX: &str = "claim-plots-";
/// The file extension.
pub const CLAIM_PLOTS_EXTENSION: &str = ".json";
/// The pak folder of the claim plot slices.
pub const CLAIM_PLOT_FOLDER: &str = "slices/darkness/";
/// The pak path's suffix of a dynamic slice.
const DYNAMIC_SLICE_SUFFIX: &str = ".dynamicslice";
/// The pak path's prefix of a slice whose capital names it without the prefix.
const SLICES_PREFIX: &str = "slices/";
/// `MB::LandClaimComponent` (retail member 7 of claim 10, class 62; Observed at the index the
/// retail groups name in the slices of both layouts).
const LAND_CLAIM: [u8; UUID_LEN] = uuid(0xe362_6a07_d134_434f_8ae3_84b4_f832_8c33);

/// What a claim plot slice gives.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimPlotSlice {
    /// The slice's pak path.
    pub slice: String,
    /// The ownership component and `m_initialPermissionRule`.
    pub ownership: (u8, u8),
    /// The buildable controller component.
    pub buildable: u8,
    /// The buildable controller's `m_structureName`: the blueprint whose costs it holds
    /// (`ClaimT1_30m`).
    pub structure: String,
    /// The placement obstruction component.
    pub obstruction: u8,
    /// The land claim component.
    pub land_claim: u8,
    /// The interact component whose state retail sent: the one with the highest index.
    pub interact: u8,
}

impl ClaimPlotSlice {
    /// The claim plot of the dynamic slice `slice` (pak path) whose root element is `root`;
    /// `None` for a slice without a land claim, an ownership, a buildable controller, a
    /// placement obstruction or an interact component.
    pub fn of(slice: &str, root: &Element) -> Option<Self> {
        let all = descendants(root);
        let component = |class: &[u8; UUID_LEN]| {
            all.iter()
                .copied()
                .find(|element| &element.type_id == class)
        };
        let land_claim = replication_index(component(&LAND_CLAIM)?)?;
        let ownership = component(&OWNERSHIP)?;
        let rule = small(find(ownership, lowercase_crc32("m_initialPermissionRule")))?;
        let buildable = component(&BUILDABLE_CONTROLLER)?;
        let interact = all
            .iter()
            .filter(|element| element.type_id == INTERACT)
            .filter_map(|element| replication_index(element))
            .max()?;
        Some(Self {
            slice: slice.to_owned(),
            ownership: (replication_index(ownership)?, rule),
            buildable: replication_index(buildable)?,
            structure: text(find(buildable, lowercase_crc32("m_structureName"))),
            obstruction: replication_index(component(&PLACEMENT_OBSTRUCTION)?)?,
            land_claim,
            interact,
        })
    }

    /// The slice name a capital gives for this slice: the pak path without its folder prefix
    /// and suffix, lowercase (`darkness/zone_capitalplot_windswardshores`).
    fn capital_name(&self) -> String {
        let path = self.slice.to_lowercase();
        let path = path.strip_prefix(SLICES_PREFIX).unwrap_or(&path);
        path.strip_suffix(DYNAMIC_SLICE_SUFFIX)
            .unwrap_or(path)
            .to_owned()
    }
}

/// A claim plot of the level: a capital whose slice is a claim plot slice.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedClaimPlot {
    /// The capital's entity id (its key).
    pub id: u64,
    /// Where it stands.
    pub position: Position,
    /// Its slice.
    pub slice: ClaimPlotSlice,
}

/// The claim plot slices, by the lowercase slice name their capitals give.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimPlots {
    /// The slices by lowercase capital slice name.
    pub by_name: BTreeMap<String, ClaimPlotSlice>,
}

/// Why the file cannot be read or written.
#[derive(Debug, thiserror::Error)]
pub enum ClaimPlotsError {
    /// The file cannot be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not claim plots.
    #[error("{path}: {source}")]
    Json {
        /// The file.
        path: PathBuf,
        /// Why.
        source: serde_json::Error,
    },
}

/// The claim plot file of `client_build` in `dir`.
pub fn claim_plots_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!(
        "{CLAIM_PLOTS_PREFIX}{client_build}{CLAIM_PLOTS_EXTENSION}"
    ))
}

impl ClaimPlots {
    /// The table of `slices`.
    pub fn of(slices: impl IntoIterator<Item = ClaimPlotSlice>) -> Self {
        Self {
            by_name: slices
                .into_iter()
                .map(|slice| (slice.capital_name(), slice))
                .collect(),
        }
    }

    /// Reads `path`; a missing file has no claim plots (content from before this file).
    ///
    /// # Errors
    ///
    /// Returns [`ClaimPlotsError`] when the file cannot be read or is not claim plots.
    pub fn load(path: &Path) -> Result<Self, ClaimPlotsError> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| ClaimPlotsError::Json {
                path: path.to_path_buf(),
                source,
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ClaimPlotsError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Writes the file to `path`.
    ///
    /// # Errors
    ///
    /// Returns [`ClaimPlotsError`] when a value cannot be written or the file cannot be.
    pub fn write(&self, path: &Path) -> Result<(), ClaimPlotsError> {
        let text = serde_json::to_string_pretty(self).map_err(|source| ClaimPlotsError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        std::fs::write(path, text).map_err(|source| ClaimPlotsError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// The slice of the capital whose slice name is `capital_slice` (any case).
    pub fn of_capital(&self, capital_slice: &str) -> Option<&ClaimPlotSlice> {
        self.by_name.get(&capital_slice.to_lowercase())
    }

    /// The claim plots among `capitals`: each capital whose slice is in the table and whose id
    /// reads.
    pub fn placed(&self, capitals: &[Capital]) -> Vec<PlacedClaimPlot> {
        capitals
            .iter()
            .filter_map(|capital| {
                Some(PlacedClaimPlot {
                    id: capital_id(&capital.id)?,
                    position: capital.position,
                    slice: self.of_capital(&capital.slice_name)?.clone(),
                })
            })
            .collect()
    }

    /// Claim plot slices in the table.
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Whether the table has no slice.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(name: &str, bytes: &[u8]) -> Element {
        Element::with_value(lowercase_crc32(name), bytes.to_vec())
    }

    fn component(class: [u8; UUID_LEN], index: u32, fields: Vec<Element>) -> Element {
        let mut children = vec![Element::with_children(
            lowercase_crc32("BaseClass1"),
            vec![value("m_replicationIndex", &index.to_be_bytes())],
        )];
        children.extend(fields);
        Element {
            type_id: class,
            children,
            ..Element::default()
        }
    }

    /// The replicated components of `zone_capitalplot_windswardshores.dynamicslice` (pak
    /// values: the ownership's rule 1, the blueprint `ClaimT1_30m`, interacts at 6 and 8).
    fn windsward_shores(land_claim: bool) -> Element {
        let mut components = vec![
            component(
                OWNERSHIP,
                1,
                vec![value("m_initialPermissionRule", &1_u32.to_be_bytes())],
            ),
            component(
                BUILDABLE_CONTROLLER,
                2,
                vec![value("m_structureName", b"ClaimT1_30m")],
            ),
            component(PLACEMENT_OBSTRUCTION, 5, Vec::new()),
            component(INTERACT, 6, Vec::new()),
            component(INTERACT, 8, Vec::new()),
        ];
        if land_claim {
            components.push(component(LAND_CLAIM, 7, Vec::new()));
        }
        Element::with_children(lowercase_crc32("Components"), components)
    }

    /// A capital GUID (made up) whose key is the Observed key of `Zone_CapitalPlot_WindswardShores`:
    /// the retail card's `GdeRef` `7b455362d7d03651` read as a little-endian `u64`.
    const WINDSWARD_SHORES_CAPITAL: &str = "5136d0d7-6253-457b-0000-000000000000";
    const WINDSWARD_SHORES_ID: u64 = 0x5136_d0d7_6253_457b;
    const SLICE: &str = "slices/darkness/zone_capitalplot_windswardshores.dynamicslice";

    #[test]
    fn a_claim_plot_gives_the_components_of_the_retail_group() {
        let plot = ClaimPlotSlice::of(SLICE, &windsward_shores(true)).expect("a claim plot");
        assert_eq!(plot.ownership, (1, 1));
        assert_eq!(plot.buildable, 2);
        assert_eq!(plot.structure, "ClaimT1_30m");
        assert_eq!(plot.obstruction, 5);
        assert_eq!(plot.land_claim, 7);
        assert_eq!(
            plot.interact, 8,
            "the interact retail sent: the highest index"
        );
        assert_eq!(ClaimPlotSlice::of(SLICE, &windsward_shores(false)), None);
    }

    #[test]
    fn a_capital_finds_its_slice_by_name_and_the_table_reads_back() {
        let plot = ClaimPlotSlice::of(SLICE, &windsward_shores(true)).expect("a claim plot");
        let table = ClaimPlots::of([plot.clone()]);
        assert_eq!(
            table.of_capital("Darkness/Zone_CapitalPlot_WindswardShores"),
            Some(&plot)
        );
        assert_eq!(
            table.of_capital("Darkness/Zone_CapitalPlot_Brightwoods"),
            None
        );
        let capital = |slice: &str| Capital {
            id: WINDSWARD_SHORES_CAPITAL.to_owned(),
            position: [6851.2, 1330.7, 62.9],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: None,
            slice_name: slice.to_owned(),
            variant_name: None,
            slice_asset_id: String::new(),
        };
        let placed = table.placed(&[
            capital("Darkness/Zone_CapitalPlot_WindswardShores"),
            capital("Structures/Siege/Siege_Fort_CutlassKeys"),
        ]);
        assert_eq!(placed.len(), 1, "the plot capital alone");
        assert_eq!(placed[0].id, WINDSWARD_SHORES_ID);
        assert_eq!(placed[0].slice, plot);
        let dir = tempfile::tempdir().expect("dir");
        let path = claim_plots_path(dir.path(), "1");
        table.write(&path).expect("write");
        assert_eq!(ClaimPlots::load(&path).expect("read"), table);
        assert_eq!(
            ClaimPlots::load(&dir.path().join("none.json")).expect("missing"),
            ClaimPlots::default()
        );
    }
}
