// SPDX-License-Identifier: AGPL-3.0-or-later
//! Artifacts: the `MasterItemDefinitions_Artifacts` items, the kind each one counts as for the
//! equip limit, and the artifact objective each one starts (finding
//! `local-data/findings/2026-11-12-artifacts-and-mob-items.md`).
//!
//! **Items (Observed, build 25810808).** 73 rows of `MasterItemDefinitions_Artifacts` carry an
//! `Artifact` part in their `ItemClass` (`Artifact+EquippableHead+Armor+Heavy`,
//! `Artifact+Weapon+EquippableTwoHand+Bow+Ranged+NoGem`, `Artifact+EquippableRing+Jewelry`) and
//! an `OnEquipObjectiveID` (`Objective_Artifact_Set1_HeavyHead`). Each objective is the same
//! tree: a simple container of a consecutive container (the stay-active `TaskEquipItem` of the
//! item, then a hidden simple container of three perk tasks) and a `TaskAchievement` on the
//! crafted copy (`FC_<item> || FC_<item>_GS725`).
//!
//! **Equip limit (Observed: client).** The client keeps `javelin.enable-artifact-limit`,
//! default on (`FUN_140263a80`); the retail remote config of 2026-09-18 does not set it. The
//! client counts worn artifacts with `PaperdollComponent::GetArtifactCount` (three flags and a
//! slot type), and its gear set text says "can only have 1 artifact of each item type"
//! (`loadout_notification_add_artifact_to_loadout_failure`). The three kinds are the weapon,
//! armour and jewellery parts of the class (Inferred: the three flags; jewellery rows carry
//! `ItemType` `Armor`, so the class part, not the `ItemType`, tells jewellery apart).

use std::collections::HashMap;

use crate::Tables;
use crate::gameplay_config::ITEM_CLASS_SEPARATOR;

/// The `ItemClass` part every artifact carries.
const ARTIFACT_CLASS: &str = "Artifact";
/// The `ItemClass` part of a weapon.
const WEAPON_CLASS: &str = "Weapon";
/// The `ItemClass` part of an amulet, a ring or an earring.
const JEWELRY_CLASS: &str = "Jewelry";
/// The `ItemClass` part of a piece of armour.
const ARMOR_CLASS: &str = "Armor";

/// The kind of an artifact: a player wears at most one artifact of each kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactKind {
    /// A weapon or a shield (`Weapon`).
    Weapon,
    /// A piece of armour (`Armor`).
    Armor,
    /// An amulet, a ring or an earring (`Jewelry`).
    Jewelry,
}

impl ArtifactKind {
    /// The kind of an `ItemClass` cell; `None` for an item that is not an artifact.
    pub fn of_item_class(item_class: &str) -> Option<Self> {
        let parts: Vec<&str> = item_class.split(ITEM_CLASS_SEPARATOR).collect();
        if !parts.contains(&ARTIFACT_CLASS) {
            return None;
        }
        // A jewellery row also names no `Weapon`; a weapon row names no `Armor`.
        [
            (WEAPON_CLASS, Self::Weapon),
            (JEWELRY_CLASS, Self::Jewelry),
            (ARMOR_CLASS, Self::Armor),
        ]
        .into_iter()
        .find_map(|(part, kind)| parts.contains(&part).then_some(kind))
    }
}

/// The artifact objective of each artifact item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactObjectives {
    /// The `OnEquipObjectiveID` of each artifact, by lowercase item id.
    by_item: HashMap<String, String>,
}

impl ArtifactObjectives {
    /// The objectives of the artifact rows of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .master_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| {
                    (
                        row.item_id.as_str(),
                        row.item_class.as_deref(),
                        row.on_equip_objective_id.as_deref(),
                    )
                }),
        )
    }

    /// The objectives of `rows` (`ItemID`, `ItemClass`, `OnEquipObjectiveID`): only an artifact
    /// row counts (a season resource row carries the cell `3`).
    pub fn build<'a>(
        rows: impl IntoIterator<Item = (&'a str, Option<&'a str>, Option<&'a str>)>,
    ) -> Self {
        let by_item = rows
            .into_iter()
            .filter(|(_, class, _)| class.and_then(ArtifactKind::of_item_class).is_some())
            .filter_map(|(item_id, _, objective)| {
                objective
                    .filter(|objective| !objective.is_empty())
                    .map(|objective| (item_id.to_lowercase(), objective.to_owned()))
            })
            .collect();
        Self { by_item }
    }

    /// The objective that equipping `item_id` (any case) starts; `None` for another item.
    pub fn of_item(&self, item_id: &str) -> Option<&str> {
        self.by_item
            .get(&item_id.to_lowercase())
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Retail `ItemClass` cells of build 25810808.
    const HEAVY_HEAD: &str = "Artifact+EquippableHead+Armor+Heavy";
    const BOW: &str = "Artifact+Weapon+EquippableTwoHand+Bow+Ranged+NoGem";
    const TOWER_SHIELD: &str = "Artifact+Weapon+EquippableOffHand+Shield+TowerShield+NoGem";
    const RING: &str = "Artifact+EquippableRing+Jewelry";
    const PLAIN_HEAD: &str = "EquippableHead+Armor+Heavy";

    #[test]
    fn the_class_tells_the_kind_and_jewellery_is_not_armour() {
        assert_eq!(
            ArtifactKind::of_item_class(HEAVY_HEAD),
            Some(ArtifactKind::Armor)
        );
        assert_eq!(ArtifactKind::of_item_class(BOW), Some(ArtifactKind::Weapon));
        assert_eq!(
            ArtifactKind::of_item_class(TOWER_SHIELD),
            Some(ArtifactKind::Weapon)
        );
        assert_eq!(
            ArtifactKind::of_item_class(RING),
            Some(ArtifactKind::Jewelry)
        );
        assert_eq!(ArtifactKind::of_item_class(PLAIN_HEAD), None);
    }

    #[test]
    fn only_artifact_rows_name_an_objective() {
        let objectives = ArtifactObjectives::build([
            (
                "Artifact_Set1_HeavyHead",
                Some(HEAVY_HEAD),
                Some("Objective_Artifact_Set1_HeavyHead"),
            ),
            ("SpringtideArmorPattern_GS715", Some("Resource"), Some("3")),
            ("HeavyHead_Plain", Some(PLAIN_HEAD), None),
        ]);
        assert_eq!(
            objectives.of_item("artifact_set1_heavyhead"),
            Some("Objective_Artifact_Set1_HeavyHead")
        );
        assert_eq!(objectives.of_item("SpringtideArmorPattern_GS715"), None);
        assert_eq!(objectives.of_item("HeavyHead_Plain"), None);
    }
}
