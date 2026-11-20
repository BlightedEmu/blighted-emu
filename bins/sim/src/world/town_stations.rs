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
//!
//! A shown station's slice stands in every player's ground with its solids, which the level
//! collision keeps as the deferred slice its spawner places ([`crate::asset_solids::SliceSolids`]):
//! the client stands on the station's floor, not on the terrain under it.

use std::sync::Arc;

use nw_content::town_stations::{TownStationSlot, TownStations, station_tier};
use nw_messages::DYNAMIC_SLICE_SUB_ID;
use nw_world::collision::Body;
use nw_world::collision_index::CollisionIndex;

use crate::asset_solids::SliceSolids;

use super::crafting::{Station, Stations, dynamic_ref};
use super::town_props::spawned_station;
use super::{Player, SimWorld};

/// Names the ids this server makes for settlement stations.
const TOWN_STATION_NAMESPACE: u64 = 0x746f_776e_7374_6174;
/// The ids of one slot: one per station type it can show.
const IDS_PER_SLOT: usize = 8;

/// The settlement stations this sim shows: per slot of the table, its slot key and the station
/// type it shows.
#[derive(Debug, Clone, Default)]
pub(super) struct TownStationsShown {
    shown: Vec<Option<(u64, u8)>>,
    /// The solids of each slot's shown slice.
    bodies: Vec<Vec<Body>>,
    /// The solids of all shown slices, as the players' ground has them.
    solids: Option<Arc<CollisionIndex>>,
}

/// Two are equal when they show the same slices: the index only places their solids.
impl PartialEq for TownStationsShown {
    fn eq(&self, other: &Self) -> bool {
        self.shown == other.shown && self.bodies == other.bodies
    }
}

impl TownStationsShown {
    /// Shows each slot of `table` among `stations` as the slice of the claim tier `tier_of` gives
    /// its territory and upgrade, with the slice's solids of `solids`; a slot whose station type
    /// changed leaves every view and comes back as the new one (module doc).
    fn sync(
        &mut self,
        table: &TownStations,
        stations: &mut Stations,
        solids: &SliceSolids,
        tier_of: impl Fn(u16, u8) -> u8,
    ) {
        let slots = table.slots();
        self.shown.resize(slots.len(), None);
        self.bodies.resize(slots.len(), Vec::new());
        let mut changed = false;
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
            let station = town_station(table, index, slot, tier);
            self.bodies[index] = station
                .as_ref()
                .zip(table.slice_of(slot, tier))
                .map(|(station, slice)| {
                    let at = station.position;
                    solids.bodies_of(slice.asset, DYNAMIC_SLICE_SUB_ID, [at.x, at.y, at.z])
                })
                .unwrap_or_default();
            self.shown[index] =
                station.map(|station| (stations.place_camp(|_| station), station_type));
            changed = true;
        }
        if changed {
            self.solids = solids.index(self.bodies.concat());
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
            &self.content.slice_solids,
            |territory, upgrade| dominion.tier(territory, upgrade),
        );
        let solids = &self.town_stations.solids;
        let mut query = self.world.query::<&mut Player>();
        for mut player in query.iter_mut(&mut self.world) {
            let same = match (&player.solids, solids) {
                (Some(held), Some(shown)) => Arc::ptr_eq(held, shown),
                (held, shown) => held.is_none() && shown.is_none(),
            };
            if !same {
                player.solids.clone_from(solids);
            }
        }
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
        assert!(
            hex.starts_with("000a01033c6f2d1aa36a5a1b8a4f2e7b9d1c0e5500000002"),
            "{hex}"
        );
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
    fn a_shown_station_s_solids_stand_in_the_players_ground() {
        use nw_world::collision::{DeferredSlice, Geometry, IDENTITY_AFFINE, LevelCollision};
        /// The smelter's floor: a box 0.5 m high on the station's place, as the carpentry
        /// station's floor stood 0.4 to 0.5 m above the terrain in the retail towns session.
        const FLOOR_HALF_M: f32 = 0.25;
        const SMELTER: &str = "3c6f2d1aa36a5a1b8a4f2e7b9d1c0e55";
        let table = TownStations::parse(TABLE).expect("table");
        let at = town_station(&table, 0, &table.slots()[0], 0)
            .expect("station")
            .position;
        let mut place = IDENTITY_AFFINE;
        place[0][3] = at.x;
        place[1][3] = at.y;
        place[2][3] = at.z + FLOOR_HALF_M;
        let mut slice = [0; 16];
        hex::decode_to_slice(SMELTER, &mut slice).expect("asset id");
        let collision = LevelCollision {
            geometries: vec![Geometry::Box {
                half_extents: [2.0, 2.0, FLOOR_HALF_M],
            }],
            deferred: vec![DeferredSlice {
                slice,
                sub_id: DYNAMIC_SLICE_SUB_ID,
                origin: [at.x, at.y, at.z],
                bodies: vec![Body { geometry: 0, place }],
            }],
            ..LevelCollision::default()
        };
        let index = Arc::new(CollisionIndex::new(collision.clone()));
        let mut world = world();
        world.content = Arc::new(PlayerContent {
            town_stations: table,
            slice_solids: SliceSolids::of(&collision, &index),
            ..PlayerContent::default()
        });
        join(&mut world, 1, 0.0);
        world.tick_town_stations();
        let entity = world.player_by_number(1).expect("player");
        let solids = world
            .world
            .get::<Player>(entity)
            .and_then(|player| player.solids.clone())
            .expect("the station's solids");
        let floor = solids.floor_at(at.x, at.y, at.z + 1.0).expect("its floor");
        assert!(
            (floor - (at.z + 2.0 * FLOOR_HALF_M)).abs() < 1e-3,
            "{floor}"
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
        let player = world
            .world
            .get::<crate::world::Player>(entity)
            .expect("player");
        assert_eq!(player.station.as_deref(), Some("Smelting3"));
        assert_eq!(player.station_interaction, Some(INTERACTION));
    }

    #[test]
    fn a_raised_claim_tier_puts_the_next_slice_in_the_old_one_s_place() {
        let mut world = world();
        world.tick_town_stations();
        let (old, _) = world.town_stations.shown[0].expect("shown");
        let table = Arc::clone(&world.content);
        world.town_stations.sync(
            &table.town_stations,
            &mut world.stations,
            &table.slice_solids,
            |territory, upgrade| u8::from(territory == 6 && upgrade == 9),
        );
        let (new, station_type) = world.town_stations.shown[0].expect("shown again");
        assert_eq!(station_type, 4, "claim tier 1: Smelting4");
        assert_ne!(new, old, "a new slot key: the old station left the views");
        assert!(world.stations.members(old).is_none());
        let card = hex::encode(
            world.stations.members(new).expect("new")[0]
                .encode()
                .expect("card"),
        );
        assert!(
            card.starts_with("000a01031170a97191f759ac97698f128ab0609a"),
            "{card}"
        );
    }
}

/// The retail table of `local-data/content` in the open world level as a live sim loads it:
/// a guard of the town stations a player used live (2026-11-14, Monarch's Bluffs: all ten
/// opened).
#[cfg(all(test, feature = "local-data"))]
mod retail_tests {
    use std::sync::Arc;

    use nw_messages::client_input::{CRAFTING_STATION_KIND, InteractionRequest};

    use crate::world::Player;
    use crate::world::tests::{NOW, connection, join};

    /// 13 settlements x 10 crafts (town crafting stations study, 2026-11-14).
    const RETAIL_SLOTS: usize = 130;
    const CRAFTS_PER_SETTLEMENT: usize = 10;
    /// Monarch's Bluffs (`12A_MSQ` story).
    const MONARCHS_BLUFFS: u16 = 12;
    const INTERACTION: u64 = 1;

    #[test]
    fn every_retail_settlement_station_stands_and_opens_with_its_station_type() {
        let (_level, mut world) = crate::quest_walk::tests::retail_world("town-stations");
        world.tick_town_stations();
        // At the claim tiers a new world starts with, one slot's slice has solids in the level
        // collision; in the retail towns session of 2026-11-19 the server showed the slices of
        // 68 slots, with 687 solids (the replay's station ground).
        assert!(
            world.town_stations.solids.is_some(),
            "the stations stand in the ground"
        );
        let content = Arc::clone(&world.content);
        let slots = content.town_stations.slots();
        assert_eq!(slots.len(), RETAIL_SLOTS);
        assert_eq!(
            slots
                .iter()
                .filter(|slot| slot.territory == MONARCHS_BLUFFS)
                .count(),
            CRAFTS_PER_SETTLEMENT
        );
        for ((index, slot), number) in slots.iter().enumerate().zip(1..) {
            let tier = world
                .territories
                .dominion()
                .tier(slot.territory, slot.upgrade);
            let slice = content
                .town_stations
                .slice_of(slot, tier)
                .unwrap_or_else(|| panic!("slot {index}: no slice at tier {tier}"));
            let (shown, _) = world.town_stations.shown[index]
                .unwrap_or_else(|| panic!("slot {index}: not shown"));
            let card = world
                .stations
                .members(shown)
                .and_then(|members| match &members[0].state {
                    nw_messages::MemberState::GdeMetadata(card) => card.gde_ref,
                    _ => None,
                })
                .unwrap_or_else(|| panic!("slot {index}: no card"));
            join(&mut world, number, 0.0);
            world.interact(
                &connection(number),
                InteractionRequest {
                    target: card,
                    target_entity: nw_messages::facet_of_raw_id(&card),
                    interaction: INTERACTION,
                    kind: CRAFTING_STATION_KIND,
                    gatherable: None,
                },
                b"",
                NOW,
            );
            let entity = world.player_by_number(number).expect("player");
            let opened = world
                .world
                .get::<Player>(entity)
                .and_then(|player| player.station.clone());
            assert_eq!(
                opened.as_deref(),
                Some(slice.station_type.as_str()),
                "slot {index} (territory {}, upgrade {})",
                slot.territory,
                slot.upgrade
            );
        }
    }
}
