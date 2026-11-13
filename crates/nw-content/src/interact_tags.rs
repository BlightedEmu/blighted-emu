// SPDX-License-Identifier: AGPL-3.0-or-later
//! The interact tags of the level objects a player can interact with (finding "task
//! interact"), by the facet entity an interaction request names. An object's tags come from its
//! slice (`m_interactTagString`, the world file's interact tags) and from its gatherable variant
//! (`VariationData` `InteractTag`, the quest objects `IO_*`). A gather of a region gatherable
//! point, which has no slice, has the tags of its gatherable's variants (the Gawain crown
//! `MB_MSQ2_DefeatGawain_Crown` of `12A_MSQ_24_DefeatGawain`). A `TaskInteract` task counts an
//! interact whose tag matches its `InteractTag`.
//!
//! A variant with `InteractOnlyOnQuest` (1,378 rows of build 25810808: the lighthouse lure
//! `0513_LighthouseLure`, the powder kegs `0511_PowderkegA`) gates its object's interact on its
//! tag ([`InteractTags::quest_gate`]). Observed (client, finding 2026-11-13 "gather progress and
//! quest interacts"): the interact check `FUN_1435c3ed0` asks, for an interact whose tag CRC
//! (component offset 0x3e0) is set and whose `m_interactOnlyOnQuest` (0x3e4) is on, whether an
//! active objective task holds the tag, and refuses the interact when none does; each objective
//! task answers for its own `InteractTag` while it is active (`DeactivateTask`, `FUN_143ad40d0`).
//! The flag is the variant's, not its gatherable's: 18 gatherables have variants with and without
//! it (`1204_CorruptedNoduleA`).

use std::collections::{HashMap, HashSet};

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;
use crate::gathering::{FACET_FREE_BITS, capital_key};

/// The interact tag a gatherable variant sets and whether its object's interact waits for a
/// quest that holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantTag {
    /// `InteractTag`.
    pub tag: String,
    /// `InteractOnlyOnQuest`: only a player with an active objective task whose `InteractTag` is
    /// this tag may interact.
    pub only_on_quest: bool,
}

/// The interact tags of the level objects, by object key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InteractTags {
    by_key: HashMap<u64, Vec<String>>,
    /// The tag an object's interact waits for, by object key: its variant's when the variant is
    /// only on quest.
    quest_gates: HashMap<u64, String>,
    /// The tags of the variants of each gatherable, by the CRC-32 of its lowercase
    /// `GatherableEntryID`.
    by_gatherable: HashMap<u32, Vec<String>>,
    /// The tags of the variants of each gatherable that are only on quest.
    gatherable_gates: HashMap<u32, Vec<String>>,
    /// The gatherables with a tagged variant that is not only on quest: a gather of one of their
    /// objects whose variant is not known waits for no quest.
    gatherables_open: HashSet<u32>,
}

/// The variant tag of `row`: its `InteractTag`, if any.
fn variant_tag(row: &crate::generated::VariationData) -> Option<VariantTag> {
    let tag = row.interact_tag.clone().filter(|tag| !tag.is_empty())?;
    Some(VariantTag {
        tag,
        only_on_quest: row.interact_only_on_quest == Some(true),
    })
}

/// The variant tag of each `VariationData` row that has one and a `GatherableEntryID`: the
/// CRC-32 of the lowercase entry id and the tag.
pub fn gatherable_tags(tables: &Tables) -> Vec<(u32, VariantTag)> {
    tables
        .variation_data
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| {
            let tag = variant_tag(row)?;
            let entry = row
                .gatherable_entry_id
                .as_deref()
                .filter(|id| !id.is_empty())?;
            Some((lowercase_crc32(entry), tag))
        })
        .collect()
}

/// The variant tag of each `VariationData` row that has one, by lowercase `VariantID`.
pub fn variant_tags(tables: &Tables) -> HashMap<String, VariantTag> {
    tables
        .variation_data
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| Some((row.variant_id.clone()?.to_lowercase(), variant_tag(row)?)))
        .collect()
}

impl InteractTags {
    /// The tags of the capitals (GUID text, variant name, slice tags) and the spawned slices
    /// (GDE id, variant name, slice tags), with the tags of their variants in `variants` (by
    /// lowercase variant id). Variant names match in any case.
    pub fn build<'a>(
        variants: &HashMap<String, VariantTag>,
        capitals: impl IntoIterator<Item = (&'a str, Option<&'a str>, &'a [String])>,
        spawned: impl IntoIterator<Item = (u64, &'a str, &'a [String])>,
    ) -> Self {
        let of_variant =
            |variant: Option<&str>| variant.and_then(|variant| variants.get(&variant.to_lowercase()));
        let mut tags = Self::default();
        let capitals = capitals.into_iter().filter_map(|(guid, variant, slice)| {
            Some((capital_key(guid)?, of_variant(variant), slice))
        });
        let spawned = spawned
            .into_iter()
            .map(|(id, variant, slice)| (id & !FACET_FREE_BITS, of_variant(Some(variant)), slice));
        for (key, variant, slice) in capitals.chain(spawned) {
            let mut own = slice.to_vec();
            if let Some(variant) = variant {
                if !own.contains(&variant.tag) {
                    own.push(variant.tag.clone());
                }
                if variant.only_on_quest {
                    tags.quest_gates.insert(key, variant.tag.clone());
                }
            }
            if !own.is_empty() {
                tags.by_key.insert(key, own);
            }
        }
        tags
    }

    /// The same tags, and `gatherables` (the CRC-32 of a lowercase `GatherableEntryID` and a
    /// variant's tag, [`gatherable_tags`]) for the gathers of objects without tags of their own.
    #[must_use]
    pub fn with_gatherables(
        mut self,
        gatherables: impl IntoIterator<Item = (u32, VariantTag)>,
    ) -> Self {
        for (gatherable, variant) in gatherables {
            if variant.only_on_quest {
                let gates = self.gatherable_gates.entry(gatherable).or_default();
                if !gates.contains(&variant.tag) {
                    gates.push(variant.tag.clone());
                }
            } else {
                self.gatherables_open.insert(gatherable);
            }
            let tags = self.by_gatherable.entry(gatherable).or_default();
            if !tags.contains(&variant.tag) {
                tags.push(variant.tag);
            }
        }
        self
    }

    /// The interact tags of an interaction with the object of facet entity `facet`: its own,
    /// else those of the gatherable it gathers, if any.
    pub fn of_interaction(&self, facet: u64, gatherable: Option<u32>) -> &[String] {
        let own = self.of(facet);
        if !own.is_empty() {
            return own;
        }
        gatherable
            .and_then(|gatherable| self.by_gatherable.get(&gatherable))
            .map_or(&[], Vec::as_slice)
    }

    /// The tags of which an active objective task of the player must hold one for an
    /// interaction with the object of facet entity `facet` (gathering `gatherable`); empty for an
    /// interact that waits for no quest. An object with tags of its own waits for its variant's
    /// tag when that variant is only on quest. Inferred: an object without them (a region
    /// gatherable point, whose variant is not known) waits for the tags of its gatherable's
    /// variants only when every tagged variant of that gatherable is only on quest.
    pub fn quest_gate(&self, facet: u64, gatherable: Option<u32>) -> &[String] {
        let key = facet & !FACET_FREE_BITS;
        if self.by_key.contains_key(&key) {
            return self.quest_gates.get(&key).map_or(&[], std::slice::from_ref);
        }
        gatherable
            .filter(|gatherable| !self.gatherables_open.contains(gatherable))
            .and_then(|gatherable| self.gatherable_gates.get(&gatherable))
            .map_or(&[], Vec::as_slice)
    }

    /// Whether an object or a gatherable variant of the level carries tag `tag` (any case).
    pub fn knows(&self, tag: &str) -> bool {
        self.by_key
            .values()
            .chain(self.by_gatherable.values())
            .flatten()
            .any(|known| known.eq_ignore_ascii_case(tag))
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

    fn tag_of(tag: &str, only_on_quest: bool) -> VariantTag {
        VariantTag {
            tag: tag.to_owned(),
            only_on_quest,
        }
    }

    #[test]
    fn an_object_has_its_slice_tags_and_its_variant_tag() {
        let variants = HashMap::from([(
            "io_99b_msq_02_galahadjournal".to_owned(),
            tag_of("IO_99B_MSQ_02_GalahadJournal", false),
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
        assert!(tags.knows("settlementstorage"), "an object's tag, any case");
        assert!(!tags.knows("NoSuchTag"));
    }

    #[test]
    fn a_region_gatherable_s_gather_has_its_variant_tag() {
        const CROWN: &str = "MB_MSQ2_DefeatGawain_Crown";
        const CROWN_TAG: &str = "IO_MB_MSQ2_DefeatGawain_Crown";
        const REGION_POINT_FACET: u64 = 0x5555_0000_0000_0001;
        let tags = InteractTags::build(
            &HashMap::new(),
            [(
                SETTLEMENT_GUID,
                None,
                ["SettlementStorage".to_owned()].as_slice(),
            )],
            [],
        )
        .with_gatherables([(lowercase_crc32(CROWN), tag_of(CROWN_TAG, false))]);
        let crown = Some(lowercase_crc32(CROWN));
        assert!(tags.knows(CROWN_TAG), "a gatherable variant's tag");
        assert_eq!(tags.of_interaction(REGION_POINT_FACET, crown), [CROWN_TAG]);
        assert!(tags.of_interaction(REGION_POINT_FACET, None).is_empty());
        assert_eq!(
            tags.of_interaction(SETTLEMENT_FACET, crown),
            ["SettlementStorage"],
            "an object's own tags come first"
        );
    }

    #[test]
    fn an_object_of_a_quest_only_variant_waits_for_its_tag() {
        // Build 25810808: `0513_LighthouseLure` is only on quest, the journal is not; the
        // corrupted nodule A gatherable has variants with and without the flag.
        const LURE_ID: u64 = 0x2222_0000_0000_0010;
        const LURE_TAG: &str = "IO_0513_LighthouseLure";
        const NODULE: &str = "1204_CorruptedNoduleA";
        const NODULE_TAG: &str = "IO_1204_CorruptedNoduleA";
        const LURE: &str = "0513_LighthouseLure";
        const REGION_POINT_FACET: u64 = 0x5555_0000_0000_0001;
        let variants = HashMap::from([
            ("0513_lighthouselure".to_owned(), tag_of(LURE_TAG, true)),
            (
                "io_99b_msq_02_galahadjournal".to_owned(),
                tag_of("IO_99B_MSQ_02_GalahadJournal", false),
            ),
        ]);
        let tags = InteractTags::build(
            &variants,
            [],
            [
                (LURE_ID, "0513_LighthouseLure", &[][..]),
                (JOURNAL_ID, "IO_99B_MSQ_02_GalahadJournal", &[][..]),
            ],
        )
        .with_gatherables([
            (lowercase_crc32(LURE), tag_of(LURE_TAG, true)),
            (lowercase_crc32(NODULE), tag_of(NODULE_TAG, true)),
            (lowercase_crc32(NODULE), tag_of(NODULE_TAG, false)),
        ]);
        assert_eq!(tags.quest_gate(LURE_ID, None), [LURE_TAG]);
        assert_eq!(tags.of(LURE_ID), [LURE_TAG], "the gate tag is an interact tag too");
        assert!(tags.quest_gate(JOURNAL_ID, None).is_empty());
        assert_eq!(
            tags.quest_gate(REGION_POINT_FACET, Some(lowercase_crc32(LURE))),
            [LURE_TAG],
            "a region point of a gatherable whose every variant is only on quest"
        );
        assert!(
            tags.quest_gate(REGION_POINT_FACET, Some(lowercase_crc32(NODULE)))
                .is_empty(),
            "a variant without the flag opens a region point of its gatherable"
        );
        assert!(tags.quest_gate(REGION_POINT_FACET, None).is_empty());
    }
}
