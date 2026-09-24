// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gems and charms (run-15 items2 study): which empty perk a gem or a charm fills, and the item
//! names the success notification shows.
//!
//! **Observed (retail MSQ15, 9 of 9 requests; data).** A gem or charm resource has a
//! `ResourceItemDefinitions.SocketType` (`Gem`, `Offense`, `Defense`, `Skill`) and a
//! `PerkBucket`; it goes into a perk slot that holds the empty perk of its socket type
//! ([`empty_perk`]), and the slot takes the bucket's perk that fits the item. The notification
//! `CharmSlotRequestsSucceeded` names the perk (`perkname`: its `DisplayName`) and the item
//! (`itemname`: its `MasterItemDefinitions.Name`).

use std::collections::HashMap;

use crate::Tables;
use crate::item_rolls::SOCKET_GEM;

/// The socket type of an offensive charm.
pub const SOCKET_OFFENSE: &str = "Offense";
/// The socket type of a defensive charm.
pub const SOCKET_DEFENSE: &str = "Defense";
/// The socket type of a skill charm.
pub const SOCKET_SKILL: &str = "Skill";
/// The `PerkType` of a socketed gem perk.
pub const GEM_PERK_TYPE: &str = "Gem";
/// The empty perk of each socket type (`ItemPerks_Gems`, `ItemPerks_2025`).
const EMPTY_PERKS: [(&str, &str); 4] = [
    (SOCKET_GEM, "PerkID_Gem_EmptyGemSlot"),
    (SOCKET_OFFENSE, "PerkID_Charm_EmptyOffense"),
    (SOCKET_DEFENSE, "PerkID_Charm_EmptyDefense"),
    (SOCKET_SKILL, "PerkID_Charm_EmptySkillPerk"),
];

/// The empty perk a gem or charm of socket type `socket` (any case) fills; `None` for another
/// type.
pub fn empty_perk(socket: &str) -> Option<&'static str> {
    EMPTY_PERKS
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(socket.trim()))
        .map(|(_, perk)| *perk)
}

/// Whether `perk_id` (any case) is the empty perk of a socket.
pub fn is_empty_perk(perk_id: &str) -> bool {
    EMPTY_PERKS
        .iter()
        .any(|(_, perk)| perk.eq_ignore_ascii_case(perk_id))
}

/// The `Name` text key of every item, by lowercase item id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemNames {
    names: HashMap<String, String>,
}

impl ItemNames {
    /// The names of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .master_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| Some((row.item_id.clone(), row.name.clone()?))),
        )
    }

    /// The names of `rows` (item id, `Name`); the first row of an id wins.
    pub fn build(rows: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut names = HashMap::new();
        for (item_id, name) in rows {
            names.entry(item_id.to_lowercase()).or_insert(name);
        }
        Self { names }
    }

    /// The `Name` of `item_id` (any case).
    pub fn of(&self, item_id: &str) -> Option<&str> {
        self.names.get(&item_id.to_lowercase()).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_socket_fills_its_empty_perk() {
        assert_eq!(empty_perk("gem"), Some("PerkID_Gem_EmptyGemSlot"));
        assert_eq!(empty_perk("Offense"), Some("PerkID_Charm_EmptyOffense"));
        assert_eq!(empty_perk("Defense"), Some("PerkID_Charm_EmptyDefense"));
        assert_eq!(empty_perk("Skill"), Some("PerkID_Charm_EmptySkillPerk"));
        assert_eq!(empty_perk("Rune"), None);
        assert!(is_empty_perk("perkid_charm_emptyoffense"));
        assert!(!is_empty_perk("PerkID_Gem_GambitGem4"));
    }

    #[test]
    fn names_are_found_in_any_case() {
        let names = ItemNames::build([
            ("1hSwordT5".to_owned(), "@1hSwordT5_MasterName".to_owned()),
            ("1HSWORDT5".to_owned(), "@Other".to_owned()),
        ]);
        assert_eq!(names.of("1hswordt5"), Some("@1hSwordT5_MasterName"));
        assert_eq!(names.of("Unknown"), None);
    }
}
