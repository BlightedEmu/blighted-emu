// SPDX-License-Identifier: AGPL-3.0-or-later
//! The mission weights (`MissionWeightsData`): for each board bucket and each slot of the
//! board, the goal types a mission of that slot can have and the weight of each.
//!
//! Observed (faction board study 2026-11-11, 29 opens of faction boards in 5 retail ledgers,
//! `factionboard` of the capture tools): a faction board offers three missions of the bucket
//! `FactionPvE` and then the missions of the bucket `FactionPvP`, and every offered mission's
//! goal type is one the bucket's slot holds (slot 1: `Log` or `Craft`; slot 2: `Harvest` or
//! `Log`; slot 3: `Log` or `Hunt` in `Hivefactionquests`). Inferred: a slot draws its goal type
//! by the weights, among the types that still have a mission to offer.

use std::collections::{BTreeMap, HashMap};

use crate::Tables;

/// The bucket of a faction board's PvE missions.
pub const FACTION_PVE_BUCKET: &str = "FactionPvE";
/// The bucket of a faction board's PvP missions.
pub const FACTION_PVP_BUCKET: &str = "FactionPvP";

/// The goal types of one slot: each lowercase `MissionGoalType` and its weight.
pub type SlotWeights = Vec<(String, u32)>;

/// The weights, by lowercase bucket and by slot number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MissionWeights {
    by_bucket: HashMap<String, BTreeMap<u32, SlotWeights>>,
}

impl MissionWeights {
    /// The weights of the rows `rows`: bucket, slot number, goal type, weight. A row with no
    /// weight is left out.
    pub fn new<'a>(rows: impl IntoIterator<Item = (&'a str, i64, &'a str, i64)>) -> Self {
        let mut by_bucket: HashMap<String, BTreeMap<u32, SlotWeights>> = HashMap::new();
        for (bucket, slot, goal_type, weight) in rows {
            let (Ok(slot), Ok(weight)) = (u32::try_from(slot), u32::try_from(weight)) else {
                continue;
            };
            if weight == 0 {
                continue;
            }
            by_bucket
                .entry(bucket.to_lowercase())
                .or_default()
                .entry(slot)
                .or_default()
                .push((goal_type.to_lowercase(), weight));
        }
        Self { by_bucket }
    }

    /// The weights of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::new(
            tables
                .mission_weights_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| {
                    (
                        row.bucket.as_str(),
                        row.slot_number,
                        row.mission_goal_type.as_str(),
                        row.weight,
                    )
                }),
        )
    }

    /// The slots of bucket `bucket` (any case), in slot order.
    pub fn slots(&self, bucket: &str) -> impl Iterator<Item = &SlotWeights> {
        self.by_bucket
            .get(&bucket.to_lowercase())
            .into_iter()
            .flat_map(BTreeMap::values)
    }
}

/// Draws a goal type of `slot` by its weights among the types `has` says have a mission:
/// `roll` gives a number below its argument (the sum of the weights of those types). `None`
/// when no type of the slot has a mission.
pub fn draw_goal_type(
    slot: &SlotWeights,
    has: impl Fn(&str) -> bool,
    roll: impl FnOnce(u32) -> u32,
) -> Option<&str> {
    let open: Vec<&(String, u32)> = slot
        .iter()
        .filter(|(goal_type, _)| has(goal_type))
        .collect();
    let total = open
        .iter()
        .fold(0_u32, |sum, (_, weight)| sum.saturating_add(*weight));
    if total == 0 {
        return None;
    }
    let mut left = roll(total);
    for (goal_type, weight) in open {
        if left < *weight {
            return Some(goal_type.as_str());
        }
        left -= weight;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build 25810808: slot 1 of the faction PvE bucket.
    fn weights() -> MissionWeights {
        MissionWeights::new([
            (FACTION_PVE_BUCKET, 1, "Raid", 9000),
            (FACTION_PVE_BUCKET, 1, "Log", 8000),
            (FACTION_PVE_BUCKET, 1, "Loot", 1),
            (FACTION_PVE_BUCKET, 2, "Loot", 9000),
            (FACTION_PVP_BUCKET, 1, "Intercept", 10_000),
            (FACTION_PVE_BUCKET, 3, "Hunt", 0),
        ])
    }

    #[test]
    fn a_bucket_gives_its_slots_in_order() {
        let weights = weights();
        let slots: Vec<&SlotWeights> = weights.slots("factionpve").collect();
        assert_eq!(slots.len(), 2, "a slot with no weight is no slot");
        assert_eq!(slots[0].len(), 3);
        assert_eq!(slots[1], &vec![("loot".to_owned(), 9000)]);
        assert_eq!(weights.slots("Other").count(), 0);
    }

    #[test]
    fn a_slot_draws_by_weight_among_the_types_with_a_mission() {
        let weights = weights();
        let slot = weights.slots(FACTION_PVE_BUCKET).next().expect("slot 1");
        let any = |_: &str| true;
        assert_eq!(draw_goal_type(slot, any, |_| 0), Some("raid"));
        assert_eq!(draw_goal_type(slot, any, |_| 8999), Some("raid"));
        assert_eq!(draw_goal_type(slot, any, |_| 9000), Some("log"));
        assert_eq!(
            draw_goal_type(slot, any, |total| total - 1),
            Some("loot"),
            "the last point of the weights"
        );
        let no_raid = |goal_type: &str| goal_type != "raid";
        assert_eq!(
            draw_goal_type(slot, no_raid, |total| {
                assert_eq!(total, 8001, "the raid's weight does not count");
                0
            }),
            Some("log")
        );
        assert_eq!(draw_goal_type(slot, |_| false, |_| 0), None);
    }
}
