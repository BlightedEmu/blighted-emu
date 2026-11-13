// SPDX-License-Identifier: AGPL-3.0-or-later
//! The siege supply generators of a war fort (siege structure study 2026-11-12; the recipe and
//! the capacity from [`nw_content::war_forts::SiegeBlueprints::generator`], the state codec
//! [`nw_messages::turrets::ItemGenerationState`]).
//!
//! **A generator** (Observed, the T1 generator of territory 11, `joining_defending_war_2`): it
//! spawns full and idle: its container (1755) holds one `SiegeSupply` stack of its capacity (300;
//! 400 on the T2 of `CK_offensive_war`, 500 on the T3 of `20261008-105106-362`) and its item
//! generation state (3002) is not active (0.714 s). A player opens it with the interaction of
//! kind [`CONTAINER_KIND`] (978.686 s, accepted 47 ms later; its interact shows one interactor,
//! 978.768 s) and takes with 2942 (980.284 s) or 2939 (982.436 s): the taken stack leaves the
//! container (its slot sent empty with sync reason 6, `was_emptied`; 980.336 s) and the state
//! turns active with the take's time as the cycle's start. Each `Tickrate` (10 s) after it the
//! container gets `OutputQty` (5 on a T1) as a new stack (sync reason 3, slot 0, the next
//! counter, a new instance) and the state sends the cycle's new start alone (990.345 s). The
//! interactor goes when the client cancels the interaction (2473; 993.101 s).
//!
//! Inferred: a cycle adds its output up to the capacity and the generator is idle again when it
//! is full (as retail showed every full generator idle); the taken items go into the taker's bag
//! when it has room for them, else nothing moves (as the loot bags of [`super::item_calls`]);
//! only a player of the generator's side opens it, never while it is broken.

use bevy_ecs::entity::Entity;
use nw_content::war_forts::{FortStructure, GeneratorRecipe};
use nw_messages::items::{ItemRecord, SYNC_CONTAINER, SYNC_MOVED};
use nw_messages::list_delta::ReplicatedList;
use nw_messages::turrets::{ITEM_GENERATION_TYPE_INDEX, ItemGenerationState};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BundleMember, ContainerState, FullList, InteractState, InteractionRequest, MemberState,
};

use super::siege_structures::{NO_INTERACTORS, StructureRun};
use super::turrets::{USE_ACCEPTED, USE_REFUSED};
use super::war_fight::{WarFight, replace_member};
use super::{Player, SimWorld, gather_answer, send_now};

/// The interaction kind that opens a generator's container (Observed: kind 24 on the T1
/// generator of `joining_defending_war_2` 978.686 s).
pub(super) const CONTAINER_KIND: u8 = 24;
/// The container slot of a generator's stack (Observed: slot 0).
const STACK_SLOT: u16 = 0;
/// Unix ms of 2000-01-01 00:00 UTC, the epoch of `itemGenerationTime`.
const GENERATION_EPOCH_UNIX_MS: u64 = 946_684_800_000;
const NS_PER_MS: u64 = 1_000_000;
/// A take of quantity 0 takes the whole stack (as [`super::item_calls`]' takes).
const WHOLE_STACK: u32 = 0;

/// The `itemGenerationTime` of Unix ms `unix_ms`.
fn generation_time_ns(unix_ms: u64) -> u64 {
    unix_ms
        .saturating_sub(GENERATION_EPOCH_UNIX_MS)
        .saturating_mul(NS_PER_MS)
}

/// A generator of the fight (module doc).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GeneratorRun {
    recipe: GeneratorRecipe,
    /// The table index of its item.
    item_index: u32,
    /// What its container holds.
    held: u32,
    active: bool,
    /// Sim ms of its next output, while active.
    next_ms: Option<u64>,
    /// The version of its container list and the counter and instance of its stack.
    version: u64,
    counter: u32,
    instance: [u8; RAW_ID_LEN],
    /// The players that have it open, with their interaction.
    open_by: Vec<(Entity, u64)>,
}

impl GeneratorRun {
    /// The generator of `structure` with `recipe` as retail first showed it (its container's
    /// stack, version, counter and instance); `None` for a structure without a generator or a
    /// container, or an item `item_index` cannot name.
    pub(super) fn new(
        structure: &FortStructure,
        recipe: &GeneratorRecipe,
        item_index: u32,
    ) -> Option<Self> {
        structure.generator_component?;
        let component = structure.container_component?;
        let container = structure.members.iter().find_map(|bytes| {
            let (member, _) = BundleMember::read_from(bytes).ok()?;
            match member.state {
                MemberState::Container(state) if member.component == component => Some(state),
                _ => None,
            }
        });
        let (version, stack) = match container.and_then(|state| state.items) {
            Some(ReplicatedList::Full(list)) => (
                list.version.unwrap_or_default(),
                list.elements.into_iter().find(|record| record.quantity > 0),
            ),
            _ => (0, None),
        };
        Some(Self {
            recipe: recipe.clone(),
            item_index,
            held: stack
                .as_ref()
                .map_or(0, |record| record.quantity.min(recipe.capacity)),
            active: false,
            next_ms: None,
            version,
            counter: stack.as_ref().map_or(0, |record| record.counter),
            instance: stack
                .as_ref()
                .map_or([0; RAW_ID_LEN], |record| record.instance),
            open_by: Vec::new(),
        })
    }

    /// Its container state: its stack, or its emptied slot.
    fn container(&self) -> ContainerState {
        let record = if self.held > 0 {
            ItemRecord {
                quantity: self.held,
                sync_reason: SYNC_CONTAINER,
                container_slot: Some(STACK_SLOT),
                item_index: self.item_index,
                counter: self.counter,
                instance: self.instance,
                ..ItemRecord::EMPTY_SLOT
            }
        } else {
            ItemRecord {
                sync_reason: SYNC_MOVED,
                ..ItemRecord::EMPTY_SLOT
            }
        };
        ContainerState {
            items: Some(ReplicatedList::Full(FullList {
                version: Some(self.version),
                elements: vec![record],
            })),
            was_emptied: (self.held == 0).then_some(true),
            ..ContainerState::default()
        }
    }
}

/// The new instance id of the `counter`th stack of the generator `number` (a made-up id, unique
/// per generator and stack).
fn stack_instance(number: u64, counter: u32) -> [u8; RAW_ID_LEN] {
    let mut id = [0; RAW_ID_LEN];
    id[..8].copy_from_slice(&number.to_be_bytes());
    id[8..12].copy_from_slice(&counter.to_be_bytes());
    id
}

impl SimWorld {
    /// The generator state member of `run` with `state`.
    fn generation_member(run: &StructureRun, state: ItemGenerationState) -> Option<BundleMember> {
        Some(BundleMember {
            component: run.body.structure.generator_component?,
            state: MemberState::Recorded {
                class: ITEM_GENERATION_TYPE_INDEX,
                body: state.encode(),
            },
        })
    }

    /// The container member of `run`'s generator.
    fn container_member(run: &StructureRun) -> Option<BundleMember> {
        Some(BundleMember {
            component: run.body.structure.container_component?,
            state: MemberState::Container(run.generation.as_ref()?.container()),
        })
    }

    /// Sends `partial` to the viewers of `run` and puts `full` into its spawn members.
    fn sync_generator(
        &mut self,
        run: &StructureRun,
        full: &[BundleMember],
        partial: Vec<BundleMember>,
    ) {
        self.stations.change_camp(
            run.body.number,
            |members| {
                for member in full {
                    replace_member(members, member);
                }
            },
            partial,
        );
    }

    /// The generator of `structure`, a supply generator of `tier`, as it spawns (module doc):
    /// idle, its cycle time now; its generation member goes into `members`.
    pub(super) fn spawn_generator(
        &self,
        structure: &FortStructure,
        tier: u8,
        members: &mut [BundleMember],
    ) -> Option<GeneratorRun> {
        let recipe = self.content.siege_blueprints.generator(tier)?;
        let item_index = self.content.kits.item(&recipe.item)?.item_index;
        let run = GeneratorRun::new(structure, recipe, item_index)?;
        let member = BundleMember {
            component: structure.generator_component?,
            state: MemberState::Recorded {
                class: ITEM_GENERATION_TYPE_INDEX,
                body: ItemGenerationState {
                    active: Some(false),
                    generation_time_ns: Some(generation_time_ns(self.unix_ms())),
                }
                .encode(),
            },
        };
        replace_member(members, &member);
        Some(run)
    }

    /// The interaction `request` of kind [`CONTAINER_KIND`] of player `entity` (`number`) on a
    /// generator of this sim's war (module doc): `false` when it names none.
    pub(super) fn open_generator(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
    ) -> bool {
        let side = self.war_side(entity);
        let Some(connection) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.connection.clone())
        else {
            return false;
        };
        self.with_turrets(|world, fight| {
            let Some(index) = fight.structures.iter().position(|run| {
                run.body.structure.gde_ref == request.target && run.generation.is_some()
            }) else {
                return false;
            };
            let mut run = fight.structures[index].clone();
            if run.body.broken || side != Some(run.body.structure.side) {
                let refused = gather_answer(number, request, USE_REFUSED, name);
                send_now(&mut world.combat_messages, &connection, &[refused]);
                return true;
            }
            let accepted = gather_answer(number, request, USE_ACCEPTED, name);
            send_now(&mut world.combat_messages, &connection, &[accepted]);
            if let Some(generator) = run.generation.as_mut() {
                generator.open_by.retain(|(open, _)| *open != entity);
                generator.open_by.push((entity, request.interaction));
            }
            world.sync_interactors(&run);
            fight.structures[index] = run;
            true
        })
        .unwrap_or(false)
    }

    /// The interact of `run` with the players that have its generator open.
    fn sync_interactors(&mut self, run: &StructureRun) {
        let count = run.generation.as_ref().map_or(NO_INTERACTORS, |generator| {
            u32::try_from(generator.open_by.len()).unwrap_or(u32::MAX)
        });
        let Some(full) = run.body.interact_member(count) else {
            return;
        };
        let partial = BundleMember {
            state: MemberState::Interact(InteractState {
                enabled: None,
                has_interactors: Some(count),
            }),
            ..full.clone()
        };
        self.sync_generator(run, &[full], vec![partial]);
    }

    /// The client of player `entity` cancelled `interaction` (2473): a generator it had open
    /// with it is closed for it.
    pub(super) fn generator_interaction_ended(&mut self, entity: Entity, interaction: u64) {
        self.with_turrets(|world, fight| {
            for index in 0..fight.structures.len() {
                let mut run = fight.structures[index].clone();
                let Some(generator) = run.generation.as_mut() else {
                    continue;
                };
                let before = generator.open_by.len();
                generator
                    .open_by
                    .retain(|open| *open != (entity, interaction));
                if generator.open_by.len() != before {
                    world.sync_interactors(&run);
                    fight.structures[index] = run;
                }
            }
        });
    }

    /// Player `entity` takes `quantity` ([`WHOLE_STACK`]: all) of the stack of the generator it
    /// has open (2942, or 2939 with all) at sim ms `now_ms` (module doc): whether it had one
    /// open.
    pub(super) fn take_generator_supply(
        &mut self,
        entity: Entity,
        quantity: Option<u32>,
        now_ms: u64,
    ) -> bool {
        let unix_ms = self.unix_ms();
        self.with_turrets(|world, fight| {
            let Some(index) = fight.structures.iter().position(|run| {
                run.generation.as_ref().is_some_and(|generator| {
                    generator.open_by.iter().any(|(open, _)| *open == entity)
                })
            }) else {
                return false;
            };
            let mut run = fight.structures[index].clone();
            let Some(generator) = run.generation.as_mut() else {
                return false;
            };
            let wanted = quantity.unwrap_or(WHOLE_STACK);
            let taken = if wanted == WHOLE_STACK {
                generator.held
            } else {
                wanted.min(generator.held)
            };
            let item = world.content.kits.item(&generator.recipe.item).cloned();
            let moved = taken > 0
                && item.is_some_and(|item| {
                    world
                        .world
                        .get_mut::<Player>(entity)
                        .is_some_and(|mut player| {
                            player
                                .gear
                                .as_mut()
                                .is_some_and(|gear| gear.add(&item, taken))
                        })
                });
            if !moved {
                return true;
            }
            generator.held -= taken;
            generator.version = generator.version.saturating_add(1);
            let started = !generator.active;
            if started {
                generator.active = true;
                generator.next_ms = Some(now_ms.saturating_add(generator.recipe.tick_ms));
            }
            let mut changed: Vec<BundleMember> = Self::container_member(&run).into_iter().collect();
            if started {
                changed.extend(Self::generation_member(
                    &run,
                    ItemGenerationState {
                        active: Some(true),
                        generation_time_ns: Some(generation_time_ns(unix_ms)),
                    },
                ));
            }
            world.sync_generator(&run, &changed, changed.clone());
            fight.structures[index] = run;
            true
        })
        .unwrap_or(false)
    }

    /// The outputs of the generators of `fight` that are due at sim ms `now_ms` (module doc).
    pub(super) fn tick_generators(&mut self, fight: &mut WarFight, now_ms: u64) {
        let unix_ms = self.unix_ms();
        for index in 0..fight.structures.len() {
            let mut run = fight.structures[index].clone();
            let number = run.body.number;
            let Some(generator) = run.generation.as_mut() else {
                continue;
            };
            if generator.next_ms.is_none_or(|at| now_ms < at) {
                continue;
            }
            generator.held = generator
                .held
                .saturating_add(generator.recipe.quantity)
                .min(generator.recipe.capacity);
            generator.counter = generator.counter.saturating_add(1);
            generator.instance = stack_instance(number, generator.counter);
            generator.version = generator.version.saturating_add(1);
            let full = generator.held >= generator.recipe.capacity;
            generator.active = !full;
            generator.next_ms = (!full).then(|| now_ms.saturating_add(generator.recipe.tick_ms));
            let state = ItemGenerationState {
                active: full.then_some(false),
                generation_time_ns: Some(generation_time_ns(unix_ms)),
            };
            let mut changed: Vec<BundleMember> = Self::container_member(&run).into_iter().collect();
            changed.extend(Self::generation_member(&run, state));
            self.sync_generator(&run, &changed, changed.clone());
            fight.structures[index] = run;
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_content::war_forts::{
        SIDE_ATTACKER, SIDE_DEFENDER, SIEGE_SUPPLY, SiegeBlueprints, WarForts,
    };

    use nw_messages::ServerMessage;

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::tests::{NOW, messages_to};
    use crate::world::turrets::tests::{
        DEFENDER, KIT_CLASS, TABLE, bag_count, use_request, war_world_with,
    };

    /// A T1 generator of the defenders with the retail members of the first T1 generator of
    /// `joining_defending_war_2` (0.714 s: its container with one stack of 300, made-up
    /// instance id, and its idle generation state), the test repeater's vitals and buildable,
    /// and the retail T1 recipe (`SiegeSupplyGeneratorT1`: 10 s, 5 `SiegeSupply`, 300).
    const GENERATOR_ASSET: &str = "55245fed30cd5320a05871053318e674";
    const GENERATOR_REF: [u8; 16] = [0x46; 16];
    const CONTAINER: &str = "0e9b1b011500013e01011f53d6eb2d5fe2003b00000000000000000000f84b5da517a64a8288e15bc0d5480bd40000000000000000000000000000";
    const GENERATION: &str = "0aba2e0103000b97914c9f293993";
    const CAPACITY: u32 = 300;
    const OUTPUT: u32 = 5;
    const TICK_MS: u64 = 10_000;
    const TAKEN: u32 = 120;

    fn generator_world(side: u8) -> (SimWorld, Entity) {
        let mut table: serde_json::Value = serde_json::from_str(TABLE).expect("test table");
        table["forts"][0]["structures"].as_array_mut().expect("structures").push(serde_json::json!({
            "asset": GENERATOR_ASSET, "gde_ref": hex::encode(GENERATOR_REF), "card_component": 0,
            "interact_component": 11, "generator_component": 10, "container_component": 14,
            "spawner_component": null, "ownership_component": 4,
            "place": [13660.0, 6330.0, 55.0], "side": SIDE_DEFENDER, "yaw": 0.0,
            "members": [CONTAINER, GENERATION, "0bb22d01030100000000",
                "020f01ff47afc80000000000000b9790c0aee250f80000000000000000ff0047afc80000003f8000005300001eaefb73015f90"]
        }));
        table["structure_kinds"] = serde_json::json!([
            {"asset": GENERATOR_ASSET, "slice": "slices/structures/siege/siege_supplygenerator_t1.dynamicslice", "template": null}
        ]);
        let forts = WarForts::parse(&table.to_string()).expect("table");
        war_world_with(forts, side, |content| PlayerContent {
            kits: crate::world::gear::tests::kits_with(&[(SIEGE_SUPPLY, KIT_CLASS)]),
            siege_blueprints: SiegeBlueprints::build(
                [],
                [(
                    1,
                    GeneratorRecipe {
                        tick_ms: TICK_MS,
                        item: SIEGE_SUPPLY.to_owned(),
                        quantity: OUTPUT,
                        capacity: CAPACITY,
                    },
                )],
            ),
            ..content
        })
    }

    fn generator(world: &SimWorld) -> (&StructureRun, &GeneratorRun) {
        let run = world
            .war_fight()
            .expect("fight")
            .structures
            .iter()
            .find(|run| run.body.structure.gde_ref == GENERATOR_REF)
            .expect("generator");
        (run, run.generation.as_ref().expect("a generator"))
    }

    fn generation_body(world: &SimWorld) -> Vec<u8> {
        let (run, _) = generator(world);
        world
            .stations
            .members(run.body.number)
            .expect("members")
            .iter()
            .find_map(|member| match &member.state {
                MemberState::Recorded { class, body } if *class == ITEM_GENERATION_TYPE_INDEX => {
                    Some(body.clone())
                }
                _ => None,
            })
            .expect("generation state")
    }

    fn open_request() -> InteractionRequest {
        InteractionRequest {
            kind: CONTAINER_KIND,
            ..use_request(GENERATOR_REF)
        }
    }

    #[test]
    fn a_full_generator_spawns_idle_and_a_take_starts_its_cycles_up_to_its_capacity() {
        let (mut world, entity) = generator_world(SIDE_DEFENDER);
        let (_, run) = generator(&world);
        assert_eq!(
            (run.held, run.active),
            (CAPACITY, false),
            "the retail full stack"
        );
        assert_eq!(generation_body(&world)[..3], [0x01, 0x03, 0x00], "idle");
        assert!(
            !world.take_generator_supply(entity, Some(TAKEN), NOW),
            "not open"
        );
        assert!(world.turret_interaction(entity, DEFENDER, open_request(), b"", NOW));
        let sent = messages_to(&world.take_messages(), DEFENDER);
        assert!(sent.iter().any(|message| matches!(message, ServerMessage::InteractionValidated(answer) if answer.status == USE_ACCEPTED)));
        assert!(world.take_generator_supply(entity, Some(TAKEN), NOW + 1));
        assert_eq!(bag_count(&world, entity, SIEGE_SUPPLY), TAKEN);
        let (_, run) = generator(&world);
        assert_eq!(
            (run.held, run.active, run.next_ms),
            (CAPACITY - TAKEN, true, Some(NOW + 1 + TICK_MS))
        );
        assert_eq!(
            generation_body(&world)[..3],
            [0x01, 0x03, 0x01],
            "running from the take"
        );
        world.tick_turrets(NOW + 1 + TICK_MS);
        let (_, run) = generator(&world);
        assert_eq!(run.held, CAPACITY - TAKEN + OUTPUT, "one output");
        assert_eq!(
            generation_body(&world)[..2],
            [0x01, 0x02],
            "the new cycle's time alone"
        );
        for cycle in 2..=(TAKEN / OUTPUT) {
            world.tick_turrets(NOW + 1 + TICK_MS * u64::from(cycle));
        }
        let (_, run) = generator(&world);
        assert_eq!(
            (run.held, run.active, run.next_ms),
            (CAPACITY, false, None),
            "full: idle again"
        );
        assert!(world.take_generator_supply(entity, None, NOW + TICK_MS * 100));
        assert_eq!(
            bag_count(&world, entity, SIEGE_SUPPLY),
            TAKEN + CAPACITY,
            "2939 takes all"
        );
        let (run, generator_run) = generator(&world);
        assert_eq!(generator_run.held, 0);
        let shown = world.stations.members(run.body.number).expect("members");
        let emptied = shown.iter().find_map(|member| match &member.state {
            MemberState::Container(state) => state.was_emptied,
            _ => None,
        });
        assert_eq!(emptied, Some(true));
        world.generator_interaction_ended(entity, open_request().interaction);
        let (_, generator_run) = generator(&world);
        assert!(generator_run.open_by.is_empty());
    }

    /// The retail recipes and capacities of the content bundle (`NW_CONTENT_DIR`):
    /// `SiegeSupplyGeneratorT1` to `T3` make 5, 10 and 15 `SiegeSupply` every 10 s and hold 300,
    /// 400 and 500 (`javelindata_generatorrecipes`, `javelindata_encumbrancelimits`).
    #[test]
    fn the_bundle_gives_each_generator_tier_its_recipe_and_capacity() {
        let dir = std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR");
        let build = crate::world::turrets::tests::CONTENT_BUILD;
        let path = nw_content::bundle::bundle_path(std::path::Path::new(&dir), build);
        let bundle = nw_content::ContentBundle::load(&path, build).expect("bundle");
        let blueprints = SiegeBlueprints::from_tables(&bundle.tables);
        for (tier, quantity, capacity) in [(1, 5, 300), (2, 10, 400), (3, 15, 500)] {
            let recipe = blueprints.generator(tier).expect("a recipe");
            assert_eq!(
                (recipe.tick_ms, recipe.item.as_str()),
                (TICK_MS, SIEGE_SUPPLY),
                "T{tier}"
            );
            assert_eq!(
                (recipe.quantity, recipe.capacity),
                (quantity, capacity),
                "T{tier}"
            );
        }
    }

    #[test]
    fn the_other_side_cannot_open_a_generator() {
        let (mut world, entity) = generator_world(SIDE_ATTACKER);
        assert!(world.turret_interaction(entity, DEFENDER, open_request(), b"", NOW));
        let sent = messages_to(&world.take_messages(), DEFENDER);
        assert!(sent.iter().any(|message| matches!(message, ServerMessage::InteractionValidated(answer) if answer.status == USE_REFUSED)));
        assert!(!world.take_generator_supply(entity, None, NOW + 1));
    }
}
