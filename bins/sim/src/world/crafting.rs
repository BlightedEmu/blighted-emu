// SPDX-License-Identifier: AGPL-3.0-or-later
//! Crafting at a station (beach-quests note, section 4). The player opens a station (interaction
//! kind 20; the server answers with the station's name and the player's interactor target), sends
//! the recipe and the ingredients it picked (2997), and 1,057 ms later the server answers with
//! the item crafted and an idle update (2993 twice). The close of the window (2999) gets one more
//! idle update.
//!
//! The stations are the level's ([`nw_world::stations`]): a client shows a station's interaction
//! only once it has the station's spawn group (campfire note: retail sends it when the station
//! comes into view; run 12 sent none and the beach campfire had no prompt). The group holds the
//! card and a state per replicated component of the station's slice, at the slice's component
//! numbers ([`station_members`]).

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use nw_content::crafting::{Recipe, StructureCosts};
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::items::FIRST_VERSION;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BuildableControllerState, BuildableGridState, BundleGroup, BundleMember, CommittedResource,
    CraftIngredient, CraftProgressMsg, DelayedEventState, FullList, GdeMetadataReplicatedState,
    GridSide, GuildCrest, InteractionRequest, MemberState, OwnerIdentification, OwnershipState,
    PlacementObstructionState, ServerMessage, TerritoryInterfaceState,
};
use nw_sim_core::Position;
use nw_world::stations::CraftingStation;

use super::slots::Slot;
use super::statics::interactor_target;
use super::{Player, SimWorld, gather_answer, send_now};
use crate::objectives::Event;

/// First slot key of a crafting station: its index is added. Keys are never sent; they stay
/// above every player and mob number and below the loot bags.
pub(super) const STATION_ENTITY_BASE: u64 = 1 << 37;
/// The component of a spawn card.
const CARD_COMPONENT: u64 = 0;
/// The status of an accepted and of a refused station interaction.
const STATION_ACCEPTED: u8 = 1;
const STATION_REFUSED: u8 = 2;
/// How long a craft takes (retail: 2993 1,057 ms after the 2997, knife and ration alike).
const CRAFT_MS: u64 = 1_057;
/// The crafts one request makes when it asks for none.
const ONE_CRAFT: u32 = 1;
/// The values of a built station that no data sets, as the retail campfire and town station
/// groups have them: no foundation events, a territory whose references are set, a ruin timer
/// that is not armed, and no guild, owner, raid or faction.
const CHECK_FOUNDATION_EVENTS: bool = false;
const TERRITORY_REFS_SET: bool = true;
const RUIN_DELAY: u16 = 0;
const RUIN_COMPLETED: bool = false;
const NO_GUILD_ID: [u8; nw_messages::buildables::GUILD_ID_LEN] =
    [0; nw_messages::buildables::GUILD_ID_LEN];
const NO_RAID: u64 = 0;
const NO_FACTION: u8 = 0;
const NO_PERMISSION_OPTIONS: u8 = 0;

/// A crafting station of the level.
#[derive(Debug, Clone, PartialEq)]
struct Station {
    /// The `GdeRef` its card and the client's interaction name.
    gde_ref: [u8; RAW_ID_LEN],
    /// Where it stands.
    position: Position,
    /// Its station type, as recipes list them (`StationType1` to `StationType4`).
    station_type: String,
    /// The name its interaction's answer carries.
    name: Vec<u8>,
    /// Its spawn group's members.
    members: Vec<BundleMember>,
}

/// The crafting stations of one sim.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Stations {
    stations: Vec<Station>,
    by_ref: HashMap<[u8; RAW_ID_LEN], usize>,
}

/// The `GdeRef` of a level entity with GDE id `id`: the id's little-endian bytes, then the
/// open world's zero phase.
fn gde_ref_of(id: u64) -> [u8; RAW_ID_LEN] {
    let mut gde_ref = [0; RAW_ID_LEN];
    gde_ref[..size_of::<u64>()].copy_from_slice(&id.to_le_bytes());
    gde_ref
}

/// The spawn group members of `station`, whose blueprint costs `costs` lists, with replicated
/// lists at `version`: the card, then a state per replicated component the slice has, by
/// component number (retail orders them differently in each group).
fn station_members(
    station: &CraftingStation,
    costs: &StructureCosts,
    version: u64,
) -> Vec<BundleMember> {
    let slice = &station.components;
    let member = |component: u8, state| BundleMember {
        component: u64::from(component),
        state,
    };
    let mut members = vec![BundleMember {
        component: CARD_COMPONENT,
        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
            asset: None,
            gde_ref: Some(gde_ref_of(station.id)),
        }),
    }];
    if let Some((component, state)) = slice.buildable {
        let resources = costs
            .of(&slice.structure)
            .iter()
            .map(|(item, quantity)| CommittedResource::of(item, lowercase_crc32(item), *quantity))
            .collect();
        members.push(member(
            component,
            MemberState::BuildableController(BuildableControllerState {
                current_state: Some(state),
                check_foundation_events: Some(CHECK_FOUNDATION_EVENTS),
                committed_resources: Some(list_at(version, resources)),
                ..BuildableControllerState::default()
            }),
        ));
    }
    if let Some((component, completion)) = slice.obstruction {
        members.push(member(
            component,
            MemberState::PlacementObstruction(PlacementObstructionState {
                has_completion_obstruction: Some(u8::from(completion)),
            }),
        ));
    }
    if let Some(component) = slice.territory {
        members.push(member(
            component,
            MemberState::TerritoryInterface(TerritoryInterfaceState {
                functionality_down: None,
                refs_set: Some(TERRITORY_REFS_SET),
            }),
        ));
    }
    if let Some((component, sides)) = &slice.grid {
        let sides = sides
            .iter()
            .map(|side| GridSide {
                side: side.side,
                points: side.points,
                active: side.active,
            })
            .collect();
        members.push(member(
            *component,
            MemberState::BuildableGrid(BuildableGridState {
                grid_sides_active: Some(list_at(version, sides)),
            }),
        ));
    }
    if let Some(component) = slice.delayed_event {
        members.push(member(
            component,
            MemberState::DelayedEvent(DelayedEventState {
                total_delay: Some(RUIN_DELAY),
                completed: Some(RUIN_COMPLETED),
            }),
        ));
    }
    if let Some((component, rule)) = slice.ownership {
        members.push(member(
            component,
            MemberState::Ownership(OwnershipState {
                guild_id: Some(NO_GUILD_ID),
                guild_owner: Some(Vec::new()),
                guild_crest: Some(GuildCrest::NONE),
                owner: Some(OwnerIdentification::none()),
                permission_rule: Some(rule),
                structure_name: Some(slice.structure_name.as_bytes().to_vec()),
                raid_id: Some(NO_RAID),
                faction_type: Some(NO_FACTION),
                owner_pvp: Some(false),
                owner_ffa: Some(false),
                permission_options: Some(NO_PERMISSION_OPTIONS),
            }),
        ));
    }
    members.sort_by_key(|member| member.component);
    members
}

/// A replicated list of `elements` at `version`.
const fn list_at<T>(version: u64, elements: Vec<T>) -> FullList<T> {
    FullList {
        version: Some(version),
        elements,
    }
}

impl Stations {
    /// The level's `stations`, whose blueprints cost `costs`.
    pub(super) fn build(stations: &[CraftingStation], costs: &StructureCosts) -> Self {
        let stations: Vec<Station> = stations
            .iter()
            .map(|station| {
                let [x, y, z] = station.position;
                Station {
                    gde_ref: gde_ref_of(station.id),
                    position: Position { x, y, z },
                    station_type: station.components.station_type.clone(),
                    name: station.components.name.as_bytes().to_vec(),
                    members: station_members(station, costs, FIRST_VERSION),
                }
            })
            .collect();
        let by_ref = stations
            .iter()
            .enumerate()
            .map(|(index, station)| (station.gde_ref, index))
            .collect();
        Self { stations, by_ref }
    }

    /// The slot keys of the stations within `radius_m` of `position`.
    pub(super) fn numbers_near(
        &self,
        position: Position,
        radius_m: f32,
    ) -> impl Iterator<Item = u64> + '_ {
        (0_u64..)
            .zip(&self.stations)
            .filter(move |(_, station)| station.position.horizontal_distance(position) <= radius_m)
            .map(|(index, _)| STATION_ENTITY_BASE + index)
    }

    /// The group of the station of slot key `number` in `slot`: its spawn members when the slot
    /// is new (the station came into view), else none.
    pub(super) fn group(&self, number: u64, slot: Slot) -> Option<BundleGroup> {
        let index = usize::try_from(number.checked_sub(STATION_ENTITY_BASE)?).ok()?;
        let station = self.stations.get(index)?;
        slot.new.then(|| BundleGroup {
            interest: u64::from(slot.interest),
            members: station.members.clone(),
        })
    }

    /// The station whose `GdeRef` is `gde_ref`.
    fn named(&self, gde_ref: &[u8; RAW_ID_LEN]) -> Option<&Station> {
        self.by_ref.get(gde_ref).map(|index| &self.stations[*index])
    }
}

/// A craft that runs: its items are taken, its output comes at `due_ms`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingCraft {
    number: u64,
    recipe: String,
    item: String,
    quantity: u32,
    due_ms: u64,
}

impl SimWorld {
    /// Player `number` (`entity`) uses the crafting station of `request`: a known station is
    /// accepted and becomes the player's interactor target; another is refused.
    pub(super) fn open_station(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
    ) {
        let station = self.stations.named(&request.target);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let Some(station) = station else {
            tracing::debug!(
                player = number,
                ?request,
                "use of an unknown crafting station"
            );
            let refusal = gather_answer(number, request, STATION_REFUSED, &[]);
            send_now(&mut self.combat_messages, &player.connection, &[refusal]);
            return;
        };
        tracing::debug!(
            player = number,
            station = %station.station_type,
            "crafting station opened"
        );
        let answer = gather_answer(number, request, STATION_ACCEPTED, &station.name);
        send_now(&mut self.combat_messages, &player.connection, &[answer]);
        player
            .extra_members
            .push(interactor_target(request.target_entity));
        player.station = Some(station.station_type.clone());
    }

    /// Player `number` (`entity`) crafts `crafts` of the recipe whose lowercase id has the CRC-32
    /// `recipe` at `now_ms`, from the `picked` ingredients: at a station of the recipe's type,
    /// with every ingredient in the bag. The ingredients are taken at once; the output comes
    /// [`CRAFT_MS`] later ([`SimWorld::crafts_due`]). A refused craft gets an idle update.
    pub(super) fn craft(
        &mut self,
        entity: Entity,
        number: u64,
        recipe: u32,
        crafts: u32,
        picked: &[CraftIngredient],
        now_ms: u64,
    ) {
        let crafts = crafts.max(ONE_CRAFT);
        let Some(recipe) = self.content.recipes.get(recipe).cloned() else {
            tracing::debug!(player = number, recipe, "craft of an unknown recipe");
            self.send_idle(entity, number);
            return;
        };
        let taken = self.take_ingredients(entity, &recipe, crafts, picked);
        if !taken {
            self.send_idle(entity, number);
            return;
        }
        tracing::debug!(player = number, recipe = %recipe.id, crafts, "craft");
        self.crafts.push(PendingCraft {
            number,
            recipe: recipe.id.clone(),
            item: recipe.item,
            quantity: recipe.quantity.saturating_mul(crafts),
            due_ms: now_ms.saturating_add(CRAFT_MS),
        });
    }

    /// Takes the ingredients of `crafts` of `recipe` out of the bag of player `entity`: for each
    /// ingredient, the item the player picked that fits it, else the first carried one that
    /// fits. Whether the player stands at a station of the recipe and the bag held them all
    /// (nothing is taken when not).
    fn take_ingredients(
        &mut self,
        entity: Entity,
        recipe: &Recipe,
        crafts: u32,
        picked: &[CraftIngredient],
    ) -> bool {
        let recipes = &self.content.recipes;
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return false;
        };
        let number = player.number;
        if !player
            .station
            .as_deref()
            .is_some_and(|station| recipe.made_at(station))
        {
            tracing::debug!(player = number, recipe = %recipe.id, "craft away from its station");
            return false;
        }
        let Some(gear) = player.gear.as_mut() else {
            return false;
        };
        let carried: Vec<String> = gear
            .carried()
            .into_iter()
            .map(|item| item.item_id)
            .collect();
        let mut takes = Vec::new();
        for ingredient in &recipe.ingredients {
            let needed = ingredient.quantity.saturating_mul(crafts);
            let fits = |item: &&String| recipes.fits(ingredient, item);
            let chosen = carried
                .iter()
                .filter(fits)
                .find(|item| picked.iter().any(|pick| pick.item == lowercase_crc32(item)))
                .or_else(|| carried.iter().find(fits));
            match chosen {
                Some(item) if gear.bag_count(item) >= needed => takes.push((item.clone(), needed)),
                _ => {
                    tracing::debug!(player = number, recipe = %recipe.id, ?ingredient, "craft without an ingredient");
                    return false;
                }
            }
        }
        for (item, quantity) in &takes {
            gear.take(item, *quantity);
        }
        true
    }

    /// The crafts whose time came at `now_ms` end: the output goes into the bag, the client gets
    /// the craft and an idle update, and the player's quest takes the craft.
    pub(super) fn crafts_due(&mut self, now_ms: u64) {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.crafts)
            .into_iter()
            .partition(|craft| now_ms >= craft.due_ms);
        self.crafts = later;
        for craft in due {
            let Some(entity) = self.player_by_number(craft.number) else {
                continue;
            };
            self.give_loot(craft.number, vec![(craft.item, craft.quantity)], now_ms);
            if let Some(player) = self.world.get::<Player>(entity) {
                let messages = [
                    ServerMessage::CraftProgress(CraftProgressMsg::crafted(
                        craft.number,
                        &craft.recipe,
                        craft.quantity,
                    )),
                    ServerMessage::CraftProgress(CraftProgressMsg::idle(craft.number)),
                ];
                send_now(&mut self.combat_messages, &player.connection, &messages);
            }
            self.quest_event(craft.number, Event::Crafted(&craft.recipe), now_ms);
        }
    }

    /// Player `number` (`entity`) closes the crafting window: it leaves the station and gets an
    /// idle update.
    pub(super) fn close_station(&mut self, entity: Entity, number: u64) {
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.station = None;
        }
        self.send_idle(entity, number);
    }

    fn send_idle(&mut self, entity: Entity, number: u64) {
        if let Some(player) = self.world.get::<Player>(entity) {
            let idle = ServerMessage::CraftProgress(CraftProgressMsg::idle(number));
            send_now(&mut self.combat_messages, &player.connection, &[idle]);
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_world::stations::{StationGridSide, StationSlice};

    use super::*;

    /// The retail WW campfire group (`newchar-retail-2026-09-18`, 448630 ms): its members as
    /// (component, class, body), the card naming `a761c93f095ab84d`. Retail lists them in another
    /// order; the order differs between retail groups.
    const WW_MEMBERS: [(u64, u64, &str); 7] = [
        (0, 10, "0102a761c93f095ab84d0000000000000000"),
        (
            1,
            1594,
            "010705000001ac100206576f6f6454318a91442700000000000500ffffffff0000003300000000000007466c696e7454310c9280c800000000000100ffffffff00000033000000000000",
        ),
        (3, 1746, "010201"),
        (6, 3139, "0103000000"),
        (7, 2134, "01010001ac1004010100020100030100040100"),
        (8, 2187, "010101"),
        (
            9,
            3218,
            "01ff000000000000000000000000000000000000003f8000003f8000003f8000003f80000000003f8000003f8000003f8000003f800000000002010000000000000000000f00000000",
        ),
    ];
    /// The WW group's list version (`ac10`): the retail server's counter.
    const WW_LIST_VERSION: u64 = 1068;
    const WW_CAMPFIRE: u64 = 0x4db8_5a09_3fc9_61a7;
    const NEAR: Position = Position {
        x: 9_606.5,
        y: 2_367.8,
        z: 44.9,
    };
    const RADIUS_M: f32 = 100.0;

    /// The WW campfire as the world file has it (`int_watchtowercampt1`'s components).
    fn campfire() -> CraftingStation {
        CraftingStation {
            id: WW_CAMPFIRE,
            slice: "slices/pois/interactions/int_watchtowercampt1".to_owned(),
            position: [NEAR.x, NEAR.y, NEAR.z],
            components: StationSlice {
                station_type: "Camp1".to_owned(),
                name: "@ui_watcherscampfire".to_owned(),
                buildable: Some((1, 5)),
                structure: "CampT1".to_owned(),
                ownership: Some((9, 1)),
                structure_name: String::new(),
                obstruction: Some((8, true)),
                territory: Some(3),
                grid: Some((
                    7,
                    (1..=4)
                        .map(|side| StationGridSide {
                            side,
                            points: 1,
                            active: false,
                        })
                        .collect(),
                )),
                delayed_event: Some(6),
            },
        }
    }

    fn costs() -> StructureCosts {
        StructureCosts::build([(
            "CampT1".to_owned(),
            vec![("WoodT1".to_owned(), 5), ("FlintT1".to_owned(), 1)],
        )])
    }

    #[test]
    fn the_campfire_group_holds_the_retail_member_bodies() {
        let retail: Vec<BundleMember> = WW_MEMBERS
            .iter()
            .map(|(component, class, body)| BundleMember {
                component: *component,
                state: MemberState::decode(*class, &hex::decode(body).expect("hex"))
                    .expect("retail body"),
            })
            .collect();
        let members = station_members(&campfire(), &costs(), WW_LIST_VERSION);
        assert_eq!(members, retail);
        let encoded = |members: &[BundleMember]| -> Vec<Vec<u8>> {
            members
                .iter()
                .map(|member| member.encode().expect("encode"))
                .collect()
        };
        assert_eq!(encoded(&members), encoded(&retail));
        let served = station_members(&campfire(), &costs(), FIRST_VERSION);
        let MemberState::BuildableController(controller) = &served[1].state else {
            panic!("controller");
        };
        assert_eq!(
            controller
                .committed_resources
                .as_ref()
                .and_then(|list| list.version),
            Some(FIRST_VERSION),
            "this server's lists start at the version of its other first states"
        );
    }

    #[test]
    fn a_station_goes_out_on_view_entry_only_and_names_its_answer() {
        let stations = Stations::build(&[campfire()], &costs());
        let number = STATION_ENTITY_BASE;
        assert_eq!(
            stations.numbers_near(NEAR, RADIUS_M).collect::<Vec<_>>(),
            [number]
        );
        let far = Position {
            x: NEAR.x + RADIUS_M + 1.0,
            ..NEAR
        };
        assert_eq!(stations.numbers_near(far, RADIUS_M).count(), 0);
        let new = Slot {
            interest: 5,
            new: true,
        };
        let group = stations.group(number, new).expect("group");
        assert_eq!(group.interest, 5);
        assert_eq!(group.members.len(), WW_MEMBERS.len());
        let kept = Slot { new: false, ..new };
        assert_eq!(stations.group(number, kept), None, "once per view entry");
        let station = stations
            .named(&gde_ref_of(WW_CAMPFIRE))
            .expect("named by its GdeRef");
        assert_eq!(station.name, b"@ui_watcherscampfire");
        assert_eq!(station.station_type, "Camp1");
    }
}
