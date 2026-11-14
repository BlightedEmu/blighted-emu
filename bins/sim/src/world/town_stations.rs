// SPDX-License-Identifier: AGPL-3.0-or-later
//! The settlements' crafting and refining stations ([`nw_content::town_stations`]): the open
//! world's sim spawns each slot as the slice of the tier its territory's claim has for the slot's
//! upgrade, with a card of that slice, its place and the slice's retail states. A station comes
//! into a view as a camp does; its use opens it as any crafting station
//! ([`SimWorld::open_station`]), and recipes take its station type.
//!
//! A claim tier changes with a town project or a lost invasion ([`super::invasions`]): the slot
//! then leaves every view and comes back as the new tier's slice, with a new id (Inferred: retail
//! sent each tier as its own slice card at one place).

use nw_content::town_stations::{TownStationSlot, TownStations, station_tier};

use super::SimWorld;
use super::crafting::{Station, Stations, dynamic_ref};
use super::town_props::spawned_station;

/// Names the ids this server makes for settlement stations.
const TOWN_STATION_NAMESPACE: u64 = 0x746f_776e_7374_6174;
/// The ids of one slot: one per station type it can show.
const IDS_PER_SLOT: usize = 8;

/// The settlement stations this sim shows: per slot of the table, its slot key and the station
/// type it shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct TownStationsShown {
    shown: Vec<Option<(u64, u8)>>,
}

impl TownStationsShown {
    /// Shows each slot of `table` among `stations` as the slice of the claim tier `tier_of` gives
    /// its territory and upgrade; a slot whose station type changed leaves every view and comes
    /// back as the new one (module doc).
    fn sync(
        &mut self,
        table: &TownStations,
        stations: &mut Stations,
        tier_of: impl Fn(u16, u8) -> u8,
    ) {
        let slots = table.slots();
        self.shown.resize(slots.len(), None);
        for (index, slot) in slots.iter().enumerate() {
            let tier = tier_of(slot.territory, slot.upgrade);
            let station_type = station_tier(slot.upgrade, tier);
            let shown = self.shown[index];
            if shown.is_some_and(|(_, shown_type)| shown_type == station_type) {
                continue;
            }
            if let Some((number, _)) = shown {
                stations.remove_camp(number);
            }
            self.shown[index] = town_station(table, index, slot, tier)
                .map(|station| (stations.place_camp(|_| station), station_type));
        }
    }
}

impl SimWorld {
    /// Shows each settlement station at the tier its claim has now ([`TownStationsShown::sync`]).
    /// Only the open world's sim has the settlements.
    pub(super) fn tick_town_stations(&mut self) {
        if self.is_tutorial() || self.settings.instance_role {
            return;
        }
        let dominion = self.territories.dominion();
        self.town_stations.sync(
            &self.content.town_stations,
            &mut self.stations,
            |territory, upgrade| dominion.tier(territory, upgrade),
        );
    }
}

/// The station slot `slot` (index `index` of `table`) shows at the claim tier `tier`; `None`
/// when the table has no slice for it or its states do not read.
fn town_station(
    table: &TownStations,
    index: usize,
    slot: &TownStationSlot,
    tier: u8,
) -> Option<Station> {
    let slice = table.slice_of(slot, tier)?;
    let id = index * IDS_PER_SLOT + usize::from(slice.tier);
    spawned_station(
        dynamic_ref(TOWN_STATION_NAMESPACE, id),
        slice.asset,
        std::iter::once(slot.place.as_slice()).chain(slice.members.iter().map(Vec::as_slice)),
        slice.station_type.clone(),
        slice.name.as_bytes().to_vec(),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_content::town_stations::TownStations;

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::tests::{join, world_with_spawns};

    /// The Windsward smelter slot and its type-3 and type-4 slices (retail captures).
    const TABLE: &str = r#"{"slots": [
        {"territory": 6, "upgrade": 9, "place": "020d01034611c11f45284f5b164443f4ee", "alt": false}
      ], "slices": [
        {"upgrade": 9, "station_type": "Smelting3", "name": "@SmeltingT2_MasterName", "alt": false,
         "asset": "3c6f2d1aa36a5a1b8a4f2e7b9d1c0e55",
         "members": ["01ba180107050000017a00", "038b22010100", "04b22d010200000000"]},
        {"upgrade": 9, "station_type": "Smelting4", "name": "@SmeltingT3_MasterName", "alt": false,
         "asset": "1170a97191f759ac97698f128ab0609a",
         "members": ["01ba180107050000017a00", "038b22010100", "04b22d010200000000"]}
    ]}"#;

    fn world() -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        world.content = Arc::new(PlayerContent {
            town_stations: TownStations::parse(TABLE).expect("table"),
            ..PlayerContent::default()
        });
        world
    }

    #[test]
    fn a_settlement_station_stands_at_its_place_as_its_tier_s_slice() {
        let mut world = world();
        world.tick_town_stations();
        let (number, station_type) = world.town_stations.shown[0].expect("shown");
        assert_eq!(station_type, 3, "a new town's smelter: claim tier 0 + 3");
        let members = world.stations.members(number).expect("members");
        assert_eq!(members.len(), 5, "card, place and the three slice states");
        let hex = hex::encode(members[0].encode().expect("card"));
        assert!(hex.starts_with("000a01033c6f2d1aa36a5a1b8a4f2e7b9d1c0e5500000002"), "{hex}");
        let near: Vec<u64> = world
            .stations
            .numbers_near(
                nw_sim_core::Position {
                    x: 9328.0,
                    y: 2693.0,
                    z: 57.0,
                },
                10.0,
            )
            .collect();
        assert_eq!(near, [number], "it stands at the retail smelter's place");
        world.tick_town_stations();
        assert_eq!(
            world.town_stations.shown[0].map(|(shown, _)| shown),
            Some(number),
            "an unchanged tier keeps the station"
        );
    }

    #[test]
    fn a_use_of_a_settlement_station_opens_it_with_its_station_type() {
        const INTERACTION: u64 = 18_148_271_865_582_212_903;
        let mut world = world();
        world.tick_town_stations();
        let (number, _) = world.town_stations.shown[0].expect("shown");
        let gde_ref = world
            .stations
            .members(number)
            .and_then(|members| match &members[0].state {
                nw_messages::MemberState::GdeMetadata(card) => card.gde_ref,
                _ => None,
            })
            .expect("card id");
        join(&mut world, 1, 0.0);
        let entity = world.player_by_number(1).expect("player");
        world.open_station(
            entity,
            1,
            nw_messages::client_input::InteractionRequest {
                target: gde_ref,
                target_entity: nw_messages::facet_of_raw_id(&gde_ref),
                interaction: INTERACTION,
                kind: nw_messages::client_input::CRAFTING_STATION_KIND,
                gatherable: None,
            },
        );
        let player = world.world.get::<crate::world::Player>(entity).expect("player");
        assert_eq!(player.station.as_deref(), Some("Smelting3"));
        assert_eq!(player.station_interaction, Some(INTERACTION));
    }

    #[test]
    fn a_raised_claim_tier_puts_the_next_slice_in_the_old_one_s_place() {
        let mut world = world();
        world.tick_town_stations();
        let (old, _) = world.town_stations.shown[0].expect("shown");
        let table = Arc::clone(&world.content);
        world
            .town_stations
            .sync(&table.town_stations, &mut world.stations, |territory, upgrade| {
                u8::from(territory == 6 && upgrade == 9)
            });
        let (new, station_type) = world.town_stations.shown[0].expect("shown again");
        assert_eq!(station_type, 4, "claim tier 1: Smelting4");
        assert_ne!(new, old, "a new slot key: the old station left the views");
        assert!(world.stations.members(old).is_none());
        let card = hex::encode(world.stations.members(new).expect("new")[0].encode().expect("card"));
        assert!(card.starts_with("000a01031170a97191f759ac97698f128ab0609a"), "{card}");
    }
}
