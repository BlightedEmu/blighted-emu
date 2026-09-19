// SPDX-License-Identifier: AGPL-3.0-or-later
//! The gatherables (gather note, 2026-09-18): a gather request names its gatherable by the CRC-32
//! of its lowercase `GatherableID`, and the gatherable's `FinalLootTable` gives the items (see
//! [`crate::loot`]).

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The tradeskill of a gatherable that is skinned.
const SKINNING: &str = "Skinning";
/// The `Restriction` of a gatherable that needs its tradeskill's tool (a corpse needs a skinning
/// knife; bushes say `ItemRequired` and retail gathered them by hand).
const TOOL_REQUIRED: &str = "ToolRequired";
/// Milliseconds per second.
const MS_PER_SECOND: f64 = 1_000.0;

/// One gatherable of `javelindata_gatherables`.
#[derive(Debug, Clone, PartialEq)]
pub struct Gatherable {
    /// `GatherableID`.
    pub id: String,
    /// `BaseGatherTime`, in ms.
    pub gather_ms: u64,
    /// `MinRespawnRate` and `MaxRespawnRate`, in ms.
    pub respawn_ms: (u64, u64),
    /// `FinalLootTable`.
    pub loot_table: Option<String>,
    /// `Tradeskill`: Skinning for a corpse to skin.
    pub tradeskill: Option<String>,
    /// It needs its tradeskill's tool (`Restriction` ToolRequired).
    pub tool_required: bool,
}

/// The gatherables by the CRC-32 of their lowercase id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gatherables {
    by_crc: HashMap<u32, Gatherable>,
}

impl Gatherable {
    /// It is a corpse to skin (`Tradeskill` Skinning).
    pub fn skinning(&self) -> bool {
        self.tradeskill.as_deref() == Some(SKINNING)
    }
}

/// Seconds of a datasheet as whole ms.
fn ms_of(seconds: f64) -> u64 {
    // Gather and respawn times of a few minutes at most.
    (seconds * MS_PER_SECOND).round().max(0.0) as u64
}

impl Gatherables {
    /// The gatherables of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .gatherable_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| Gatherable {
                    id: row.gatherable_id.clone(),
                    gather_ms: ms_of(row.base_gather_time),
                    respawn_ms: (ms_of(row.min_respawn_rate), ms_of(row.max_respawn_rate)),
                    loot_table: row.final_loot_table.clone(),
                    tradeskill: row.tradeskill.clone(),
                    tool_required: row.restriction.as_deref() == Some(TOOL_REQUIRED),
                }),
        )
    }

    /// The gatherables of `gatherables`.
    pub fn build(gatherables: impl IntoIterator<Item = Gatherable>) -> Self {
        Self {
            by_crc: gatherables
                .into_iter()
                .map(|gatherable| (lowercase_crc32(&gatherable.id), gatherable))
                .collect(),
        }
    }

    /// The gatherable whose lowercase id has the CRC-32 `crc`.
    pub fn get(&self, crc: u32) -> Option<&Gatherable> {
        self.by_crc.get(&crc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gatherable_is_found_by_the_crc_of_its_lowercase_id() {
        let gatherables = Gatherables::build([Gatherable {
            id: "SingleStone".to_owned(),
            gather_ms: 250,
            respawn_ms: (60_000, 90_000),
            loot_table: Some("SingleStone".to_owned()),
            tradeskill: None,
            tool_required: false,
        }]);
        // Retail flint request: crc `a417137f`.
        let flint = gatherables.get(0xa417_137f).cloned().expect("flint");
        assert_eq!(flint.id, "SingleStone");
        assert_eq!(ms_of(0.25), 250);
    }
}
