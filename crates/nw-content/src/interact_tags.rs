// SPDX-License-Identifier: AGPL-3.0-or-later
//! The interact tags of the level objects a player can interact with (finding "task
//! interact"), by the facet entity an interaction request names. An object's tags come from its
//! slice (`m_interactTagString`, the world file's interact tags) and from its gatherable variant
//! (`VariationData` `InteractTag`, the quest objects `IO_*`). A `TaskInteract` task counts an
//! interact whose tag matches its `InteractTag`.

use std::collections::HashMap;

use crate::Tables;
use crate::gathering::{FACET_FREE_BITS, capital_key};

/// The interact tags of the level objects, by object key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InteractTags {
    by_key: HashMap<u64, Vec<String>>,
}

/// The `InteractTag` of each `VariationData` row that has one, by lowercase `VariantID`.
pub fn variant_tags(tables: &Tables) -> HashMap<String, String> {
    tables
        .variation_data
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| {
            let tag = row.interact_tag.clone().filter(|tag| !tag.is_empty())?;
            Some((row.variant_id.clone()?.to_lowercase(), tag))
        })
        .collect()
}

impl InteractTags {
    /// The tags of the capitals (GUID text, variant name, slice tags) and the spawned slices
    /// (GDE id, variant name, slice tags), with the tags of their variants in `variants` (by
    /// lowercase variant id). Variant names match in any case.
    pub fn build<'a>(
        variants: &HashMap<String, String>,
        capitals: impl IntoIterator<Item = (&'a str, Option<&'a str>, &'a [String])>,
        spawned: impl IntoIterator<Item = (u64, &'a str, &'a [String])>,
    ) -> Self {
        let tags_of = |variant: Option<&str>, slice: &[String]| -> Vec<String> {
            let mut tags = slice.to_vec();
            if let Some(tag) = variant.and_then(|variant| variants.get(&variant.to_lowercase()))
                && !tags.contains(tag)
            {
                tags.push(tag.clone());
            }
            tags
        };
        let mut by_key = HashMap::new();
        let capitals = capitals.into_iter().filter_map(|(guid, variant, slice)| {
            Some((capital_key(guid)?, tags_of(variant, slice)))
        });
        let spawned = spawned
            .into_iter()
            .map(|(id, variant, slice)| (id & !FACET_FREE_BITS, tags_of(Some(variant), slice)));
        for (key, tags) in capitals.chain(spawned) {
            if !tags.is_empty() {
                by_key.insert(key, tags);
            }
        }
        Self { by_key }
    }

    /// The interact tags of the object with facet entity `facet`; empty for an object with none.
    pub fn of(&self, facet: u64) -> &[String] {
        self.by_key
            .get(&(facet & !FACET_FREE_BITS))
            .map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The retail Monarch's Bluffs settlement capital (vault capture 2026-09-27): its storage
    /// interaction names facet entity `0xE0F2_6061_7543_BB93`.
    const SETTLEMENT_GUID: &str = "f7763200-249e-87c4-1784-526151dd3c56";
    const SETTLEMENT_FACET: u64 = 0xE0F2_6061_7543_BB93;
    const JOURNAL_ID: u64 = 0x1234_5678_9abc_def0;

    #[test]
    fn an_object_has_its_slice_tags_and_its_variant_tag() {
        let variants = HashMap::from([(
            "io_99b_msq_02_galahadjournal".to_owned(),
            "IO_99B_MSQ_02_GalahadJournal".to_owned(),
        )]);
        let storage = ["SettlementStorage".to_owned()];
        let tags = InteractTags::build(
            &variants,
            [(SETTLEMENT_GUID, None, storage.as_slice())],
            [(JOURNAL_ID, "IO_99B_MSQ_02_GalahadJournal", &[][..])],
        );
        assert_eq!(tags.of(SETTLEMENT_FACET), ["SettlementStorage"]);
        assert_eq!(tags.of(JOURNAL_ID), ["IO_99B_MSQ_02_GalahadJournal"]);
        assert!(tags.of(1).is_empty());
    }
}
