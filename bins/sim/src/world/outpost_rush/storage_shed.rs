// SPDX-License-Identifier: AGPL-3.0-or-later
//! The storage sheds of an Outpost Rush match (game mode helper B13; findings
//! `local-data/findings/2026-11-13-outpost-rush-shed-and-points.md`).
//!
//! Observed:
//! - One shed per outpost (`StorageShedA/B/C`, slice `or_storage`, asset [`SHED_ASSET`]), spawned
//!   with the outpost structures ([`super::super::outpost_rush_structures`]): its ownership takes
//!   the raid of the team that owns the outpost (0 with no owner), its container (1755) spawns
//!   empty and its interact (2930) counts the players who have it open.
//! - One container per shed, seen by every viewer of the shed: `opr_que_and_lost` saw three
//!   stacks come in from 1592.284 to 1595.086 s with no item call of its own, and a later card of
//!   the shed (1609.569 s) held them all.
//! - The open is kind [`OPEN_STORAGE_KIND`] with the name `Storage` (`OPR_Original_map`
//!   102.350 s), accepted 46 ms later; the interact shows 1 (102.400 s) until the client's 2473
//!   (107.217 s, 0 at 107.267 s).
//! - A deposit (2801 `GiveItem` naming the shed's `GdeRef`, 105.349 s, 50 of a stack) empties the
//!   bag stack and the shed's container goes out whole (version 39: slot 0, sync reason 6, a new
//!   instance id) 52 ms later, and the depositor gets an empty 7048 with its bag change (the
//!   7048 every change of a player's items sends, [`super::super::storage_search`]: the bag's
//!   change of the deposit counts as one changed item); a take (2942 `TakeItem` of that
//!   instance, 106.416 s) sends it
//!   whole again, empty (version 40), and the stack is back in the bag.
//! - Every stack seen in a shed was an Outpost Rush item (the four resources; `OR_*` block).
//! - A deposit there is no war board deposit (the own 839 stats changed only their score around
//!   the 105.349 s deposit).
//!
//! Inferred: only the team that owns the outpost opens its shed and moves items in or out (the
//! ownership's permission rule 4 and raid, as the armory); only items of the mode's
//! `ItemClassesToRemoveOnExit` go in (`OutpostRushOnly`: every stack seen; a store for the match's
//! items, not a trade of others), and of them only plain stacks (no durability, gear score or
//! perks of their own); a deposit makes a stack of its own; the contents stay with the shed when
//! the outpost changes owner; a shed holds at most [`SHED_STACKS`] stacks; the version starts at
//! [`FIRST_VERSION`] (retail's first seen versions were 24, 39 and 49, after earlier changes).

use bevy_ecs::entity::Entity;
use nw_bus::ItemState;
use nw_content::starter::KitItem;
use nw_messages::items::{ItemRecord, SYNC_MOVED};
use nw_messages::list_delta::ReplicatedList;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{BundleMember, ContainerState, FullList, InteractState, InteractionRequest, MemberState};

use super::super::gates::merge_members;
use super::super::gear::MovedStack;
use super::super::item_calls::OPEN_STORAGE_KIND;
use super::super::outpost_rush_structures::OutpostStructures;
use super::super::turrets::{USE_ACCEPTED, USE_REFUSED};
use super::super::{Player, gather_answer};
use super::SimWorld;

/// The slice asset of the storage shed (Observed: `or_storage`, every ledger).
pub(super) const SHED_ASSET: [u8; RAW_ID_LEN] = [
    0xa4, 0x27, 0x6e, 0xfd, 0x80, 0x86, 0x5c, 0x7f, 0x95, 0x8b, 0x74, 0x53, 0x9e, 0xf5, 0x4a, 0x4a,
];
/// The most stacks a shed holds (Inferred, module doc).
pub(super) const SHED_STACKS: usize = 64;
/// The version of a shed's list at its spawn (Inferred, module doc).
const FIRST_VERSION: u64 = 1;
/// A move of quantity 0 moves the whole stack (as the item calls' moves).
const WHOLE_STACK: u32 = 0;
/// The namespace of the instance ids of the shed stacks ("oprshed").
const SHED_NAMESPACE: u64 = 0x6f70_7273_6865_6400;

/// One stack in a shed.
#[derive(Debug, Clone, PartialEq)]
struct ShedStack {
    item_id: String,
    item_index: u32,
    quantity: u32,
    values: Option<ItemState>,
    instance: [u8; RAW_ID_LEN],
}

/// One shed of the match.
#[derive(Debug, Clone, PartialEq)]
struct StorageShed {
    /// Its slot key and `GdeRef`.
    number: u64,
    gde_ref: [u8; RAW_ID_LEN],
    container_component: u64,
    interact_component: Option<u64>,
    stacks: Vec<ShedStack>,
    version: u64,
    /// The stacks it made so far (each one's instance id).
    made: u32,
    /// The players who have it open, with the interaction they opened it with.
    open_by: Vec<(Entity, u64)>,
}

impl StorageShed {
    /// Its container: every stack in its slot (module doc).
    fn container(&self) -> ContainerState {
        let elements = (0_u16..)
            .zip(&self.stacks)
            .map(|(slot, stack)| ItemRecord {
                quantity: stack.quantity,
                sync_reason: SYNC_MOVED,
                container_slot: Some(slot),
                item_index: stack.item_index,
                instance: stack.instance,
                ..ItemRecord::EMPTY_SLOT
            })
            .collect();
        ContainerState {
            items: Some(ReplicatedList::Full(FullList {
                version: Some(self.version),
                elements,
            })),
            ..ContainerState::default()
        }
    }

    /// A new instance id for its next stack (a made-up id, unique per shed and stack).
    fn next_instance(&mut self) -> [u8; RAW_ID_LEN] {
        self.made = self.made.saturating_add(1);
        let mut id = [0; RAW_ID_LEN];
        id[..8].copy_from_slice(&(SHED_NAMESPACE ^ self.number).to_be_bytes());
        id[8..12].copy_from_slice(&self.made.to_be_bytes());
        id
    }
}

/// The sheds of the match and the item classes that may go in.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct StorageSheds {
    sheds: Vec<StorageShed>,
    match_classes: Vec<String>,
}

impl StorageSheds {
    fn index_of(&self, gde_ref: &[u8; RAW_ID_LEN]) -> Option<usize> {
        self.sheds.iter().position(|shed| &shed.gde_ref == gde_ref)
    }

    fn open_index(&self, entity: Entity, gde_ref: &[u8; RAW_ID_LEN]) -> Option<usize> {
        self.index_of(gde_ref)
            .filter(|index| self.sheds[*index].open_by.iter().any(|(open, _)| *open == entity))
    }
}

/// Whether a stack of `item` goes into a shed: an item of the match's classes (`of_match`) with
/// no gear values (no paperdoll slot, durability or perks; module doc).
fn storable(item: &KitItem, of_match: bool) -> bool {
    of_match && item.slot.is_none() && item.durability == 0 && item.perk_ids.is_empty()
}

/// The interact member of `shed` with its open count.
fn interact_member(shed: &StorageShed) -> Option<BundleMember> {
    Some(BundleMember {
        component: shed.interact_component?,
        state: MemberState::Interact(InteractState {
            enabled: None,
            has_interactors: Some(u32::try_from(shed.open_by.len()).unwrap_or(u32::MAX)),
        }),
    })
}

impl SimWorld {
    /// The sheds among `structures` (by their slice) with their container and interact
    /// components, taking the items of `match_classes` (the mode's `ItemClassesToRemoveOnExit`).
    pub(super) fn storage_sheds_of(&self, structures: &OutpostStructures, match_classes: &[String]) -> StorageSheds {
        let sheds = structures
            .structures
            .iter()
            .filter(|structure| structure.asset == SHED_ASSET)
            .filter_map(|structure| {
                let members = self.stations.members(structure.number)?;
                let component_of = |wanted: fn(&MemberState) -> bool| {
                    members.iter().find(|member| wanted(&member.state)).map(|member| member.component)
                };
                Some(StorageShed {
                    number: structure.number,
                    gde_ref: structure.gde_ref,
                    container_component: component_of(|state| matches!(state, MemberState::Container(_)))?,
                    interact_component: component_of(|state| matches!(state, MemberState::Interact(_))),
                    stacks: Vec::new(),
                    version: FIRST_VERSION,
                    made: 0,
                    open_by: Vec::new(),
                })
            })
            .collect();
        StorageSheds {
            sheds,
            match_classes: match_classes.to_vec(),
        }
    }

    /// Whether player `entity`'s team owns the outpost of the shed `gde_ref` (its raid).
    fn owns_shed(&self, entity: Entity, gde_ref: &[u8; RAW_ID_LEN]) -> bool {
        let owner = self.arena_matches.outpost_rush.as_ref().and_then(|running| {
            running
                .structures
                .built_raid(gde_ref, nw_content::outpost_structures::StructureRole::StorageShed)
        });
        owner.is_some() && self.outpost_rush_raid_of(entity) == owner
    }

    /// Sends `members` of shed `index` to its viewers and keeps them in its spawn members.
    fn change_shed(&mut self, index: usize, members: Vec<BundleMember>) {
        let Some(number) = self
            .arena_matches
            .outpost_rush
            .as_ref()
            .and_then(|running| running.sheds.sheds.get(index))
            .map(|shed| shed.number)
        else {
            return;
        };
        self.stations
            .change_camp(number, |held| merge_members(held, &members), members.clone());
    }

    /// The open (kind [`OPEN_STORAGE_KIND`]) `request` of player `entity` (`number`, named
    /// `name`) on a shed of the match (module doc): accepted for the team that owns its outpost,
    /// else refused; `false` when it names no shed.
    pub(in crate::world) fn open_outpost_rush_shed(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
    ) -> bool {
        if request.kind != OPEN_STORAGE_KIND {
            return false;
        }
        let Some(index) = self
            .arena_matches
            .outpost_rush
            .as_ref()
            .and_then(|running| running.sheds.index_of(&request.target))
        else {
            return false;
        };
        let owns = self.owns_shed(entity, &request.target);
        let status = if owns { USE_ACCEPTED } else { USE_REFUSED };
        self.send_travel(entity, &[gather_answer(number, request, status, name)]);
        if !owns {
            return true;
        }
        let interact = self.arena_matches.outpost_rush.as_mut().and_then(|running| {
            let shed = running.sheds.sheds.get_mut(index)?;
            shed.open_by.retain(|(open, _)| *open != entity);
            shed.open_by.push((entity, request.interaction));
            interact_member(shed)
        });
        self.change_shed(index, interact.into_iter().collect());
        true
    }

    /// The client of player `entity` ended `interaction` (2473): a shed it opened with it, or
    /// every shed when `interaction` is `None` (it left the match), closes for it.
    pub(in crate::world) fn outpost_rush_shed_closed(&mut self, entity: Entity, interaction: Option<u64>) {
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let mut closed = Vec::new();
        for (index, shed) in running.sheds.sheds.iter_mut().enumerate() {
            let before = shed.open_by.len();
            shed.open_by
                .retain(|(open, with)| *open != entity || interaction.is_some_and(|wanted| wanted != *with));
            if shed.open_by.len() != before {
                closed.extend(interact_member(shed).map(|member| (index, member)));
            }
        }
        for (index, member) in closed {
            self.change_shed(index, vec![member]);
        }
    }

    /// Player `entity` puts `quantity` ([`WHOLE_STACK`]: all) of its bag stack `instance` into
    /// the shed `container` it has open (2801, module doc): `false` when `container` is no shed
    /// of the match.
    pub(in crate::world) fn outpost_rush_shed_give(
        &mut self,
        entity: Entity,
        container: &[u8; RAW_ID_LEN],
        instance: [u8; RAW_ID_LEN],
        quantity: u32,
    ) -> bool {
        let Some(running) = self.arena_matches.outpost_rush.as_ref() else {
            return false;
        };
        if running.sheds.index_of(container).is_none() {
            return false;
        }
        let Some(index) = running.sheds.open_index(entity, container) else {
            return true;
        };
        let classes = running.sheds.match_classes.clone();
        let full = running.sheds.sheds[index].stacks.len() >= SHED_STACKS;
        if full || !self.owns_shed(entity, container) {
            return true;
        }
        let content = std::sync::Arc::clone(&self.content);
        let moved = self.world.get_mut::<Player>(entity).and_then(|mut player| {
            let gear = player.gear.as_mut()?;
            let (bag_slot, item_id, _, _) = gear
                .bag_stacks()
                .into_iter()
                .find(|(_, _, held, locked)| *held == instance && !locked)?;
            let item = content.kits.item(&item_id)?;
            if !storable(item, content.instances.item_has_class(&item_id, &classes)) {
                return None;
            }
            let moved = gear.take_moved(bag_slot, instance, quantity).ok()?;
            Some((moved, item.item_index))
        });
        let Some((moved, item_index)) = moved else {
            tracing::debug!("a shed deposit refused");
            return true;
        };
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return true;
        };
        let shed = &mut running.sheds.sheds[index];
        let instance = shed.next_instance();
        shed.stacks.push(ShedStack {
            item_id: moved.item_id,
            item_index,
            quantity: moved.quantity,
            values: moved.values,
            instance,
        });
        shed.version = shed.version.saturating_add(1);
        let member = BundleMember {
            component: shed.container_component,
            state: MemberState::Container(shed.container()),
        };
        self.change_shed(index, vec![member]);
        true
    }

    /// Player `entity` takes `quantity` ([`WHOLE_STACK`]: all) of the stack `instance` of the
    /// shed `container` it has open into its bag (2942, module doc): `false` when `container` is
    /// no shed of the match. A bag with no room for it takes nothing.
    pub(in crate::world) fn outpost_rush_shed_take(
        &mut self,
        entity: Entity,
        container: &[u8; RAW_ID_LEN],
        instance: [u8; RAW_ID_LEN],
        quantity: u32,
    ) -> bool {
        let Some(running) = self.arena_matches.outpost_rush.as_ref() else {
            return false;
        };
        if running.sheds.index_of(container).is_none() {
            return false;
        }
        let Some(index) = running.sheds.open_index(entity, container) else {
            return true;
        };
        let Some(at) = running.sheds.sheds[index]
            .stacks
            .iter()
            .position(|stack| stack.instance == instance)
        else {
            return true;
        };
        if !self.owns_shed(entity, container) {
            return true;
        }
        let stack = running.sheds.sheds[index].stacks[at].clone();
        let taken = if quantity == WHOLE_STACK {
            stack.quantity
        } else {
            quantity.min(stack.quantity)
        };
        let wanted = MovedStack {
            item_id: stack.item_id.clone(),
            quantity: taken,
            values: stack.values.clone(),
        };
        let content = std::sync::Arc::clone(&self.content);
        let Some(item) = content.kits.item(&stack.item_id) else {
            return true;
        };
        let put = self.has_room(entity, std::slice::from_ref(&wanted))
            && self.world.get_mut::<Player>(entity).is_some_and(|mut player| {
                player.gear.as_mut().is_some_and(|gear| {
                    gear.put_moved(&content.kits, item, taken, stack.values.as_ref(), SYNC_MOVED)
                })
            });
        if !put {
            tracing::debug!("a shed take with no room");
            return true;
        }
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return true;
        };
        let shed = &mut running.sheds.sheds[index];
        if taken >= stack.quantity {
            shed.stacks.remove(at);
        } else {
            shed.stacks[at].quantity -= taken;
        }
        shed.version = shed.version.saturating_add(1);
        let member = BundleMember {
            component: shed.container_component,
            state: MemberState::Container(shed.container()),
        };
        self.change_shed(index, vec![member]);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shed() -> StorageShed {
        StorageShed {
            number: 7,
            gde_ref: [1; RAW_ID_LEN],
            container_component: 4,
            interact_component: Some(5),
            stacks: Vec::new(),
            version: FIRST_VERSION,
            made: 0,
            open_by: Vec::new(),
        }
    }

    #[test]
    fn only_plain_items_of_the_match_classes_go_in() {
        use super::super::super::outpost_rush_structures::tests::{WOOD, WOOD_CLASS};
        let kits = crate::world::gear::tests::kits_with_resources(&[(WOOD, WOOD_CLASS)]);
        let resource = kits.item(WOOD).cloned().expect("wood");
        assert!(storable(&resource, true));
        assert!(!storable(&resource, false), "not an item of the match");
        let worn = KitItem {
            durability: 100,
            ..resource
        };
        assert!(!storable(&worn, true), "gear keeps values of its own");
    }

    #[test]
    fn an_empty_shed_sends_a_whole_list_with_no_record() {
        let state = shed().container();
        assert_eq!(
            state.items,
            Some(ReplicatedList::Full(FullList {
                version: Some(FIRST_VERSION),
                elements: Vec::new(),
            })),
            "retail version 40 after the take: no record"
        );
    }

    #[test]
    fn each_stack_takes_its_slot_with_sync_reason_six_and_a_new_instance() {
        let mut shed = shed();
        for (quantity, item_index) in [(4, 3183), (10, 3184)] {
            let instance = shed.next_instance();
            shed.stacks.push(ShedStack {
                item_id: String::new(),
                item_index,
                quantity,
                values: None,
                instance,
            });
        }
        let Some(ReplicatedList::Full(list)) = shed.container().items else {
            panic!("a whole list");
        };
        let slots: Vec<(Option<u16>, u32, u8)> = list
            .elements
            .iter()
            .map(|record| (record.container_slot, record.quantity, record.sync_reason))
            .collect();
        assert_eq!(slots, [(Some(0), 4, SYNC_MOVED), (Some(1), 10, SYNC_MOVED)]);
        assert_ne!(list.elements[0].instance, list.elements[1].instance);
    }

    mod in_a_match {
        use nw_messages::ServerMessage;

        use super::super::super::super::item_calls::OPEN_STORAGE_KIND;
        use super::super::super::super::tests::messages_to;
        use super::super::super::super::turrets::tests::{bag_count, give, use_request};
        use super::super::super::super::outpost_rush_structures::tests as structure_tests;
        use super::super::super::tests::{
            FIRST, SECOND, START_MS, capture_ticks, outpost_rush, running, rush_sim_with, shed_and_ward_table,
            stand_in_outpost, tick_at, tick_sent,
        };
        use super::super::super::{CAPTURE_TICK_MS, PREGAME_MS};
        use super::*;

        const STORED: u32 = 25;
        const PART: u32 = 10;

        /// The match with outpost A (and its shed) owned by the team of [`FIRST`], and the
        /// shed's `GdeRef`.
        fn owned_shed() -> (SimWorld, [u8; RAW_ID_LEN]) {
            let mode = outpost_rush();
            let table = shed_and_ward_table(&mode.map.folder);
            let mut world = rush_sim_with(mode, table);
            let start_ms = START_MS + PREGAME_MS;
            tick_at(&mut world, start_ms);
            stand_in_outpost(&mut world, FIRST, 0);
            tick_at(&mut world, start_ms + capture_ticks(1) * CAPTURE_TICK_MS);
            let gde_ref = running(&world).sheds.sheds[0].gde_ref;
            (world, gde_ref)
        }

        fn open(gde_ref: [u8; RAW_ID_LEN]) -> InteractionRequest {
            InteractionRequest {
                kind: OPEN_STORAGE_KIND,
                ..use_request(gde_ref)
            }
        }

        fn status_to(outgoing: &[crate::world::Outgoing], number: u64) -> Option<u8> {
            messages_to(outgoing, number).iter().find_map(|message| match message {
                ServerMessage::InteractionValidated(answer) => Some(answer.status),
                _ => None,
            })
        }

        #[test]
        fn the_owner_team_opens_its_shed_and_the_foes_are_refused() {
            let (mut world, gde_ref) = owned_shed();
            let first = world.player_by_number(FIRST).expect("player");
            let second = world.player_by_number(SECOND).expect("player");
            world.take_messages();
            assert!(world.open_outpost_rush_shed(first, FIRST, open(gde_ref), b"Storage"));
            assert!(world.open_outpost_rush_shed(second, SECOND, open(gde_ref), b"Storage"));
            let outgoing = world.take_messages();
            assert_eq!(status_to(&outgoing, FIRST), Some(USE_ACCEPTED));
            assert_eq!(status_to(&outgoing, SECOND), Some(USE_REFUSED));
            let shed = &running(&world).sheds.sheds[0];
            assert_eq!(shed.open_by.len(), 1, "the interact shows one interactor");
            assert!(!world.open_outpost_rush_shed(first, FIRST, open([9; RAW_ID_LEN]), b"Storage"));
            world.outpost_rush_shed_closed(first, Some(open(gde_ref).interaction));
            assert!(running(&world).sheds.sheds[0].open_by.is_empty(), "2473 closes it");
        }

        #[test]
        fn a_stack_of_no_match_class_stays_in_the_bag() {
            let (mut world, gde_ref) = owned_shed();
            let first = world.player_by_number(FIRST).expect("player");
            give(&mut world, first, structure_tests::WOOD, STORED);
            assert!(world.open_outpost_rush_shed(first, FIRST, open(gde_ref), b"Storage"));
            let instance = world
                .world
                .get::<Player>(first)
                .and_then(|player| player.gear.as_ref().map(|gear| gear.bag_stacks()))
                .and_then(|stacks| stacks.into_iter().find(|(_, id, _, _)| id == structure_tests::WOOD))
                .map(|(_, _, instance, _)| instance)
                .expect("the wood stack");
            assert!(world.outpost_rush_shed_give(first, &gde_ref, instance, WHOLE_STACK));
            assert_eq!(bag_count(&world, first, structure_tests::WOOD), STORED, "the test catalog names no class");
            assert!(running(&world).sheds.sheds[0].stacks.is_empty());
            assert!(!world.outpost_rush_shed_give(first, &[9; RAW_ID_LEN], instance, WHOLE_STACK));
        }

        /// The class of the match's items (`ItemClassesToRemoveOnExit` of `OutpostRush`).
        const MATCH_CLASS: &str = "OutpostRushOnly";

        /// Retail `OPR_Original_map` 105.349 s: a deposit empties the bag slot, the shed lists the
        /// stack, and the depositor gets an empty 7048 with the bag change.
        #[test]
        fn the_owner_team_deposits_a_stack_and_gets_an_empty_7048_with_its_bag_change() {
            let (mut world, gde_ref) = owned_shed();
            let mut content = (*world.content).clone();
            content.instances = content.instances.with_item_classes(structure_tests::WOOD, &[MATCH_CLASS]);
            world.content = std::sync::Arc::new(content);
            let first = world.player_by_number(FIRST).expect("player");
            give(&mut world, first, structure_tests::WOOD, STORED);
            let now_ms = world.server_ms;
            tick_at(&mut world, now_ms);
            assert!(world.open_outpost_rush_shed(first, FIRST, open(gde_ref), b"Storage"));
            world.take_messages();
            let instance = world
                .world
                .get::<Player>(first)
                .and_then(|player| player.gear.as_ref().map(|gear| gear.bag_stacks()))
                .and_then(|stacks| stacks.into_iter().find(|(_, id, _, _)| id == structure_tests::WOOD))
                .map(|(_, _, instance, _)| instance)
                .expect("the wood stack");
            let version = running(&world).sheds.sheds[0].version;
            assert!(world.outpost_rush_shed_give(first, &gde_ref, instance, WHOLE_STACK));
            assert_eq!(bag_count(&world, first, structure_tests::WOOD), 0, "the bag slot empties");
            let shed = &running(&world).sheds.sheds[0];
            assert_eq!(shed.stacks.len(), 1);
            assert_eq!(shed.stacks[0].quantity, STORED);
            assert_eq!(shed.version, version + 1, "a new full list");
            let outgoing = tick_sent(&mut world, now_ms);
            assert!(
                messages_to(&outgoing, FIRST).iter().any(|message| matches!(
                    message,
                    ServerMessage::StorageSearchItems(items) if items.items.is_empty()
                )),
                "an empty 7048 with the bag change"
            );
        }

        #[test]
        fn the_owner_team_takes_part_of_a_stack_then_the_rest() {
            let (mut world, gde_ref) = owned_shed();
            let first = world.player_by_number(FIRST).expect("player");
            let second = world.player_by_number(SECOND).expect("player");
            let item_index = world.content.kits.item(structure_tests::WOOD).expect("wood").item_index;
            let instance = {
                let running = world.arena_matches.outpost_rush.as_mut().expect("the match");
                let shed = &mut running.sheds.sheds[0];
                let instance = shed.next_instance();
                shed.stacks.push(ShedStack {
                    item_id: structure_tests::WOOD.to_owned(),
                    item_index,
                    quantity: STORED,
                    values: None,
                    instance,
                });
                instance
            };
            assert!(world.outpost_rush_shed_take(second, &gde_ref, instance, PART), "a shed of the match");
            assert_eq!(bag_count(&world, second, structure_tests::WOOD), 0, "not open for the foe");
            assert!(world.open_outpost_rush_shed(first, FIRST, open(gde_ref), b"Storage"));
            let version = running(&world).sheds.sheds[0].version;
            assert!(world.outpost_rush_shed_take(first, &gde_ref, instance, PART));
            assert_eq!(bag_count(&world, first, structure_tests::WOOD), PART);
            let shed = &running(&world).sheds.sheds[0];
            assert_eq!(shed.stacks[0].quantity, STORED - PART);
            assert_eq!(shed.version, version + 1);
            assert!(world.outpost_rush_shed_take(first, &gde_ref, instance, WHOLE_STACK));
            assert_eq!(bag_count(&world, first, structure_tests::WOOD), STORED);
            assert!(running(&world).sheds.sheds[0].stacks.is_empty(), "retail version 40: empty");
        }
    }
}
