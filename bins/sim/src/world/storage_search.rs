// SPDX-License-Identifier: AGPL-3.0-or-later
//! The storage search items (7048) that go with each change of a player's items.
//!
//! Retail sends one 7048 with no search and no items before the bundle of a change of the
//! player's items (tutorial-quest note, section 10). Newchar-retail-2026-09-18 (106 sends):
//! 5 around the first card, whose gear is five worn starter items; one within 30 ms after each
//! equip (11 of 11 `ServerEquipItem`) and each quest turn-in (6 of 6
//! `RequestCompleteObjective`); 2 after the take-all of a loot bag (2939) and 3 after the end of
//! a chest's use (3746). Inferred: one per item whose stacks changed. Which items the two loot
//! cases changed is not in the decoded members, so whether their counts follow that rule is
//! Unknown.
//!
//! **Searches (run-15 items2 study, retail MSQ16).** A new search (7044, `refresh` 0) is
//! answered with 7046: every item of the player's storage chests that an ingredient of a
//! recipe of the searched station takes (an `Item` ingredient by id, a `Category_Only` one by
//! the item's `IngredientCategories`; 14 of 14 retail results fit) with its total over all
//! storages. A refresh comes in bursts of up to 20 in one tick and retail answered each burst
//! once (260 requests, 14 answers). A 7039 asks for the last results again (best guess: retail
//! sent 7047, then 7046 after the client's 7039 in 3 of 7 searches; this server always answers
//! 7044 at once, as retail did in the other 4). When the storages' totals of listed items change,
//! a 7048 with the search id lists those results' indexes and new totals and the count of the
//! results (retail 4 to 10 s after the change; this server sends it with the change).

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use nw_bus::{CarriedItem, ItemPlace};
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::item_calls::SEARCH_FILTER_HOUSING;
use nw_messages::storage_search::SEARCH_READY;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    SearchedItem, ServerMessage, StorageItemSimple, StorageSearchItemsMsg, StorageSearchReadyMsg,
    StorageSearchResultsMsg, global_storage_header,
};

use super::{Player, SimWorld, send_now};

/// The last byte of a 7048 (0 in every retail body).
const SEARCH_ITEMS_LAST: u8 = 0;
/// The station of a search that names none (the housing and dye screens).
const NO_STATION: u32 = 0;

/// The stacks of one item: each place and quantity, in order.
type Stacks = Vec<(ItemPlace, u32)>;

/// The items a player held at its last change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ItemChanges {
    held: BTreeMap<String, Stacks>,
}

impl ItemChanges {
    /// How many items changed from the last call to `carried`: an item counts once when it
    /// came, went, moved or changed quantity. The first call counts every item.
    pub(super) fn changes(&mut self, carried: &[CarriedItem]) -> usize {
        let held = stacks_by_item(carried);
        let changed = held
            .iter()
            .filter(|(item, stacks)| self.held.get(*item) != Some(*stacks))
            .count()
            + self
                .held
                .keys()
                .filter(|item| !held.contains_key(*item))
                .count();
        self.held = held;
        changed
    }
}

/// The stacks of `carried` by item id, each item's stacks in place order.
fn stacks_by_item(carried: &[CarriedItem]) -> BTreeMap<String, Stacks> {
    let mut held: BTreeMap<String, Stacks> = BTreeMap::new();
    for item in carried {
        held.entry(item.item_id.clone())
            .or_default()
            .push((item.place, item.quantity));
    }
    for stacks in held.values_mut() {
        stacks.sort_unstable_by_key(|(place, quantity)| (place_order(*place), *quantity));
    }
    held
}

/// A sort key of `place`: worn slots, then bag slots.
fn place_order(place: ItemPlace) -> (u8, u32) {
    match place {
        ItemPlace::Worn(slot) => (0, slot),
        ItemPlace::Bag(slot) => (1, u32::from(slot)),
    }
}

/// What a storage search looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SearchFor {
    /// The ingredients of the recipes of the station whose lowercase name without tier digits
    /// has this CRC-32.
    Station(u32),
    /// Housing items: the item catalog leaves `HousingItem` rows out (the client's item index
    /// skips them), so no storage of this server holds one and the results are empty.
    Housing,
    /// A filter whose meaning is Unknown: no results.
    Unknown,
}

impl SearchFor {
    /// What a 7044 with `filter` and `station` looks for.
    pub(super) const fn of(filter: u32, station: u32) -> Self {
        if station != NO_STATION {
            Self::Station(station)
        } else if filter == SEARCH_FILTER_HOUSING {
            Self::Housing
        } else {
            Self::Unknown
        }
    }
}

/// A player's last storage search: its id and the results last sent (item id and total).
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct StorageSearch {
    search: [u8; RAW_ID_LEN],
    results: Vec<(String, u32)>,
    /// When the last answer went: a refresh in the same tick is not answered again.
    answered_ms: Option<u64>,
}

impl StorageSearch {
    /// The results as sent.
    fn items(&self) -> Vec<StorageItemSimple> {
        self.results
            .iter()
            .map(|(item_id, total)| StorageItemSimple {
                item: lowercase_crc32(item_id),
                quantity: *total,
            })
            .collect()
    }

    /// The results whose totals differ in `totals` (by item id; a missing item has none): each
    /// one's index and new total, which the search keeps.
    fn changes(&mut self, totals: &BTreeMap<String, u32>) -> Vec<SearchedItem> {
        let mut changed = Vec::new();
        for (index, (item_id, total)) in (0_u32..).zip(&mut self.results) {
            let now = totals.get(item_id).copied().unwrap_or_default();
            if now != *total {
                *total = now;
                changed.push(SearchedItem {
                    index,
                    item: StorageItemSimple {
                        item: lowercase_crc32(item_id),
                        quantity: now,
                    },
                });
            }
        }
        changed
    }
}

/// The results of a search: the items of `totals` (item id, total over the storages) that
/// `wanted` takes, in item id order, with a total above 0.
fn search_results(
    totals: &BTreeMap<String, u32>,
    wanted: impl Fn(&str) -> bool,
) -> Vec<(String, u32)> {
    totals
        .iter()
        .filter(|(item_id, total)| **total > 0 && wanted(item_id))
        .map(|(item_id, total)| (item_id.clone(), *total))
        .collect()
}

impl SimWorld {
    /// Player `number` (`entity`) searches its storages (7044) at `now_ms` for `what` with
    /// search id `search`; a refresh answered in this tick already is not answered again.
    pub(super) fn search_storage(
        &mut self,
        entity: Entity,
        number: u64,
        search: [u8; RAW_ID_LEN],
        what: SearchFor,
        refresh: bool,
        now_ms: u64,
    ) {
        let content = Arc::clone(&self.content);
        let answered = self
            .world
            .get::<StorageSearch>(entity)
            .is_some_and(|last| last.search == search && last.answered_ms == Some(now_ms));
        if refresh && answered {
            return;
        }
        let Some(player) = self.world.get::<Player>(entity) else {
            return;
        };
        let connection = player.connection.clone();
        let totals = player
            .gear
            .as_ref()
            .map(super::gear::Gear::stored_totals)
            .unwrap_or_default();
        let results = match what {
            SearchFor::Station(station) => {
                let ingredients: Vec<_> = content.recipes.ingredients_at(station).collect();
                search_results(&totals, |item_id| {
                    ingredients
                        .iter()
                        .any(|ingredient| content.recipes.fits(ingredient, item_id))
                })
            }
            SearchFor::Housing | SearchFor::Unknown => Vec::new(),
        };
        let state = StorageSearch {
            search,
            results,
            answered_ms: Some(now_ms),
        };
        let answer = ServerMessage::StorageSearchResults(StorageSearchResultsMsg {
            header: global_storage_header(number),
            search,
            items: state.items(),
        });
        self.world.entity_mut(entity).insert(state);
        send_now(&mut self.combat_messages, &connection, &[answer]);
    }

    /// Player `number` (`entity`) asks for the results of its last search again (7039):
    /// the ready note (7047) goes first, then the results (7046), as retail sent them in
    /// 3 of 7 searches.
    pub(super) fn replicate_search(&mut self, entity: Entity, number: u64) {
        let Some(last) = self.world.get::<StorageSearch>(entity) else {
            return;
        };
        let ready = ServerMessage::StorageSearchReady(StorageSearchReadyMsg {
            header: global_storage_header(number),
            search: last.search,
            ready: SEARCH_READY,
        });
        let answer = ServerMessage::StorageSearchResults(StorageSearchResultsMsg {
            header: global_storage_header(number),
            search: last.search,
            items: last.items(),
        });
        let Some(player) = self.world.get::<Player>(entity) else {
            return;
        };
        let connection = player.connection.clone();
        send_now(&mut self.combat_messages, &connection, &[ready, answer]);
    }

    /// After an item call of player `number` (`entity`): when its storages' totals of the
    /// results of its last search changed, the 7048 of those results.
    pub(super) fn search_results_changed(&mut self, entity: Entity, number: u64) {
        let Some(player) = self.world.get::<Player>(entity) else {
            return;
        };
        let connection = player.connection.clone();
        let Some(totals) = player.gear.as_ref().map(super::gear::Gear::stored_totals) else {
            return;
        };
        let Some(mut last) = self.world.get_mut::<StorageSearch>(entity) else {
            return;
        };
        let items = last.changes(&totals);
        if items.is_empty() {
            return;
        }
        let message = ServerMessage::StorageSearchItems(StorageSearchItemsMsg {
            header: global_storage_header(number),
            search: last.search,
            items,
            total: u32::try_from(last.results.len()).unwrap_or(u32::MAX),
            last: SEARCH_ITEMS_LAST,
        });
        send_now(&mut self.combat_messages, &connection, &[message]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SWORD: &str = "1hSwordT2_FTUE";
    const CHEST: &str = "SwordsmanArchetypeChest";
    const POTION: &str = "PotionHealthT1";
    const MAIN_HAND: u32 = 24;
    const CHEST_SLOT: u32 = 1;

    fn item(item_id: &str, quantity: u32, place: ItemPlace) -> CarriedItem {
        CarriedItem {
            item_id: item_id.to_owned(),
            quantity,
            place,
            state: nw_bus::ItemState::default(),
        }
    }

    #[test]
    fn a_search_lists_the_wanted_stored_items_and_their_changes() {
        const INGOT: &str = "IngotT2";
        const ORE: &str = "OreT1";
        const SEARCH: [u8; RAW_ID_LEN] = [7; RAW_ID_LEN];
        let totals = BTreeMap::from([
            (INGOT.to_owned(), 12),
            (ORE.to_owned(), 40),
            ("FlintT1".to_owned(), 3),
        ]);
        let results = search_results(&totals, |item| item != "FlintT1");
        assert_eq!(results, [(INGOT.to_owned(), 12), (ORE.to_owned(), 40)]);
        let mut search = StorageSearch {
            search: SEARCH,
            results,
            answered_ms: None,
        };
        assert_eq!(
            search.items()[1],
            StorageItemSimple {
                item: lowercase_crc32(ORE),
                quantity: 40,
            }
        );
        assert!(search.changes(&totals).is_empty());
        let mut after = totals;
        after.insert(ORE.to_owned(), 39);
        after.remove(INGOT);
        let changes = search.changes(&after);
        assert_eq!(
            changes
                .iter()
                .map(|change| (change.index, change.item.quantity))
                .collect::<Vec<_>>(),
            [(0, 0), (1, 39)]
        );
        assert!(search.changes(&after).is_empty(), "kept");
        assert_eq!(
            SearchFor::of(0, 0x2294_152c),
            SearchFor::Station(0x2294_152c)
        );
        assert_eq!(
            SearchFor::of(SEARCH_FILTER_HOUSING, NO_STATION),
            SearchFor::Housing
        );
        assert_eq!(SearchFor::of(1, NO_STATION), SearchFor::Unknown);
    }

    #[test]
    fn the_first_call_counts_every_item() {
        let mut changes = ItemChanges::default();
        let worn = [
            item(SWORD, 1, ItemPlace::Worn(MAIN_HAND)),
            item(CHEST, 1, ItemPlace::Worn(CHEST_SLOT)),
        ];
        assert_eq!(changes.changes(&worn), 2);
        assert_eq!(changes.changes(&worn), 0, "nothing changed");
    }

    #[test]
    fn a_move_a_gain_a_quantity_and_a_removal_count_once_each() {
        let mut changes = ItemChanges::default();
        changes.changes(&[
            item(SWORD, 1, ItemPlace::Bag(0)),
            item(POTION, 1, ItemPlace::Bag(1)),
        ]);
        assert_eq!(
            changes.changes(&[
                item(SWORD, 1, ItemPlace::Worn(MAIN_HAND)),
                item(POTION, 1, ItemPlace::Bag(1)),
            ]),
            1,
            "an equip moves one item"
        );
        assert_eq!(
            changes.changes(&[
                item(SWORD, 1, ItemPlace::Worn(MAIN_HAND)),
                item(POTION, 2, ItemPlace::Bag(1)),
                item(CHEST, 1, ItemPlace::Bag(2)),
            ]),
            2,
            "a new quantity and a new item"
        );
        assert_eq!(
            changes.changes(&[
                item(POTION, 2, ItemPlace::Bag(1)),
                item(CHEST, 1, ItemPlace::Bag(2)),
            ]),
            1,
            "a removal"
        );
    }
}
