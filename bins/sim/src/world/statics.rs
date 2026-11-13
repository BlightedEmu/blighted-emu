// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tutorial's static level entities that one player sees (ftue-static-entities note).
//!
//! The client has the tutorial's static entities (the beach bag, the ruin chest, trigger volumes,
//! timelines) in its level slices. The server still sends each one a spawn card that names only
//! its `GdeRef`, with the states of some of its components; the client uses the bag and the
//! chest through them. The cards and states are the retail ones of the static entity table
//! ([`FtueEntities`]), with this player's ids written in. Retail shows the ship's entities until
//! the move into the water, and the island's after it. This server sends only the island's: with
//! the ship's, the live client showed a black world on the ship (2026-09-12), and the ship needs
//! none of them.

use std::collections::{BTreeMap, VecDeque};

use nw_content::ftue_entities::{EntityMember, FtueEntities, GDE_REF_PREFIX_LEN, SessionIds};
use nw_content::starter::KitItem;
use nw_messages::items::{FullList, ITEM_BOUND, ItemRecord, SYNC_CONTAINER, SYNC_MOVED};
use nw_messages::{
    BundleGroup, BundleMember, ContainerState, GatheringState, InteractState, InteractorState,
    MemberState, NO_GATHERABLE, ReplicatedList,
};
use uuid::Uuid;

use super::gear::instance_id;
use super::slots::Slot;

/// First slot key of a static entity: its index in the table is added. Keys are never sent; they
/// stay above every player and mob number.
pub(super) const STATIC_ENTITY_BASE: u64 = 1 << 40;
/// When the table's capture moved the player into the water: entities carded before it belong to
/// the ship.
const RETAIL_WATER_MOVE_MS: u64 = 118_546;

/// The bag entity's container component (retail take-all target).
const BAG_CONTAINER_COMPONENT: u64 = 2;
/// The bag entity's components: interact state, loot-drop glow, delayed events.
const BAG_INTERACT_COMPONENT: u64 = 3;
const BAG_LOOT_COMPONENT: u64 = 6;
const BAG_EVENT_COMPONENTS: [u64; 2] = [7, 8];
/// The player's component whose state names the interaction target (retail take group).
const INTERACTOR_COMPONENT: u64 = 26;
/// `MB::LootDropReplicatedState` (class 2027): the loot glow, on and off (retail bodies).
const LOOT_DROP_TYPE_INDEX: u64 = 2027;
const LOOT_DROP_ON: &[u8] = &[0x01, 0x01, 0x00, 0x00, 0x00, 0x01];
const LOOT_DROP_OFF: &[u8] = &[0x01, 0x01, 0x00, 0x00, 0x00, 0x00];
/// The interact state of a taken container (retail take group): off.
const INTERACT_TAKEN: InteractState = InteractState {
    enabled: Some(false),
    has_interactors: None,
};
/// `MB::DelayedEventComponentReplicatedState` (class 3139) bodies the bag's event components
/// send at the take (retail values, recorded).
const DELAYED_EVENT_TYPE_INDEX: u64 = 3139;
const BAG_EVENT_BODIES: [&[u8]; 2] = [&[0x01, 0x01, 0x3d, 0x00], &[0x01, 0x01, 0x2e, 0x66]];
/// The player's component of its gather state (`MB::GatheringComponentReplicatedState`).
const GATHERING_COMPONENT: u64 = 36;
/// `m_status` of a running and a done gather (retail chest 1 and 3).
const GATHER_RUNNING: u8 = 1;
const GATHER_DONE: u8 = 3;
/// The chest's gather (retail 237342): 2.5 left, efficiency 1, no other gatherer.
const CHEST_AMOUNT: f32 = 2.5;
const CHEST_EFFICIENCY: f32 = 1.0;
const CHEST_GATHERERS: u8 = 0;
/// The gatherer count of a gather state once its gather ends (Observed: retail sends 0 at the end
/// of a gather whose start counted 1, `Playthrough_Part_04` 4545.40 s and 4620.13 s).
const NO_GATHERERS: u8 = 0;
/// List version of the bag's filled and emptied containers (retail values).
const BAG_FILLED_VERSION: u64 = 5;
const BAG_EMPTIED_VERSION: u64 = 6;

/// One of the groups [`StaticViews::group`] sends after the card: `Recorded` replays a change of
/// the static entity table; `Built` is a group this server made itself (the bag's loot contents
/// and the states of its take).
#[derive(Debug, Clone, PartialEq)]
enum Pending {
    Recorded(Vec<EntityMember>),
    Built(Vec<BundleMember>),
}

/// The static entities one player sees, with the state changes not sent yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct StaticViews {
    /// Table index of each shown entity, with its change groups to send in order.
    shown: BTreeMap<usize, VecDeque<Pending>>,
}

impl StaticViews {
    /// Shows the island's entities of `entities` (those carded after the move into the water).
    pub(super) fn show_island(&mut self, entities: &FtueEntities) {
        self.shown.clear();
        for (index, entity) in entities.entities.iter().enumerate() {
            let carded_at = entity.spawns.first().map(|spawn| spawn.time_ms);
            if carded_at.is_some_and(|at| at >= RETAIL_WATER_MOVE_MS) {
                self.shown.insert(index, VecDeque::new());
            }
        }
    }

    /// Queues change group `change` of the shown entity whose `GdeRef` starts with `prefix`.
    pub(super) fn change(
        &mut self,
        entities: &FtueEntities,
        prefix: &[u8; GDE_REF_PREFIX_LEN],
        change: usize,
    ) {
        let Some((index, group)) = find(entities, prefix)
            .and_then(|index| Some((index, entities.entities[index].changes.get(change)?)))
        else {
            return;
        };
        if let Some(pending) = self.shown.get_mut(&index) {
            pending.push_back(Pending::Recorded(group.members.clone()));
        }
    }

    /// Queues `members` as the next change of the shown entity whose `GdeRef` starts with
    /// `prefix`: a state this server makes, not a replay of the table (the bag's loot contents
    /// and its take).
    pub(super) fn queue(
        &mut self,
        entities: &FtueEntities,
        prefix: &[u8; GDE_REF_PREFIX_LEN],
        members: Vec<BundleMember>,
    ) {
        if let Some(pending) = find(entities, prefix).and_then(|index| self.shown.get_mut(&index)) {
            pending.push_back(Pending::Built(members));
        }
    }

    /// Hides the entity whose `GdeRef` starts with `prefix`.
    pub(super) fn hide(&mut self, entities: &FtueEntities, prefix: &[u8; GDE_REF_PREFIX_LEN]) {
        if let Some(index) = find(entities, prefix) {
            self.shown.remove(&index);
        }
    }

    /// The slot keys of the shown entities.
    pub(super) fn numbers(&self) -> impl Iterator<Item = u64> + '_ {
        self.shown
            .keys()
            .map(|index| STATIC_ENTITY_BASE + *index as u64)
    }

    /// The group of the entity of slot key `number` in `slot`: its spawn states when the slot is
    /// new, else its next change; `None` for no shown entity or nothing to send.
    pub(super) fn group(
        &mut self,
        number: u64,
        slot: Slot,
        entities: &FtueEntities,
        ids: &SessionIds,
    ) -> Option<BundleGroup> {
        let index = usize::try_from(number.checked_sub(STATIC_ENTITY_BASE)?).ok()?;
        let pending = self.shown.get_mut(&index)?;
        let members = if slot.new {
            Pending::Recorded(
                entities
                    .entities
                    .get(index)?
                    .spawns
                    .first()?
                    .members
                    .clone(),
            )
        } else {
            pending.pop_front()?
        };
        Some(BundleGroup {
            interest: u64::from(slot.interest),
            members: match members {
                Pending::Recorded(members) => members
                    .iter()
                    .map(|member| BundleMember {
                        component: member.component,
                        state: MemberState::Recorded {
                            class: member.class,
                            body: member.patched_body(ids),
                        },
                    })
                    .collect(),
                Pending::Built(members) => members,
            },
        })
    }
}

/// The bag's loot group, sent after its card (retail: ~300 ms): the container's contents and
/// the loot glow. The client's `CanTakeItems` refuses a take-all on a container whose item list
/// is empty (`PlayerTradeComponent - CanTakeItems - Invalid parameters`, live run of
/// 2026-09-14), so the bag cannot be looted until this state is out.
pub(super) fn bag_loot(character: Uuid, items: &[KitItem]) -> Vec<BundleMember> {
    vec![
        BundleMember {
            component: BAG_CONTAINER_COMPONENT,
            state: MemberState::Container(ContainerState {
                items: Some(ReplicatedList::Full(FullList {
                    version: Some(BAG_FILLED_VERSION),
                    elements: items
                        .iter()
                        .enumerate()
                        .map(|(slot, item)| container_item(character, slot as u16, item))
                        .collect(),
                })),
                bonus_max_encumbrance: Some(0),
                was_emptied: Some(false),
                ..ContainerState::default()
            }),
        },
        BundleMember {
            component: BAG_LOOT_COMPONENT,
            state: MemberState::Recorded {
                class: LOOT_DROP_TYPE_INDEX,
                body: LOOT_DROP_ON.to_vec(),
            },
        },
    ]
}

/// The bag group of a take (retail): the emptied container, the interact flag off, the glow
/// off, and the two delayed events' bodies.
pub(super) fn bag_emptied(item_count: usize) -> Vec<BundleMember> {
    let emptied = ItemRecord {
        sync_reason: SYNC_MOVED,
        ..ItemRecord::EMPTY_SLOT
    };
    vec![
        BundleMember {
            component: BAG_CONTAINER_COMPONENT,
            state: MemberState::Container(ContainerState {
                items: Some(ReplicatedList::Full(FullList {
                    version: Some(BAG_EMPTIED_VERSION),
                    elements: vec![emptied; item_count],
                })),
                was_emptied: Some(true),
                ..ContainerState::default()
            }),
        },
        BundleMember {
            component: BAG_INTERACT_COMPONENT,
            state: MemberState::Interact(INTERACT_TAKEN),
        },
        BundleMember {
            component: BAG_LOOT_COMPONENT,
            state: MemberState::Recorded {
                class: LOOT_DROP_TYPE_INDEX,
                body: LOOT_DROP_OFF.to_vec(),
            },
        },
    ]
    .into_iter()
    .chain(
        BAG_EVENT_COMPONENTS
            .into_iter()
            .zip(BAG_EVENT_BODIES)
            .map(|(component, body)| BundleMember {
                component,
                state: MemberState::Recorded {
                    class: DELAYED_EVENT_TYPE_INDEX,
                    body: body.to_vec(),
                },
            }),
    )
    .collect()
}

/// The facet entity of the static entity whose `GdeRef` starts with `prefix`: the prefix in
/// reverse byte order (retail: chest `efa5a09f81357058`, facet `587035819fa0a5ef`).
pub(super) const fn facet_of(prefix: &[u8; GDE_REF_PREFIX_LEN]) -> u64 {
    u64::from_le_bytes(*prefix)
}

/// The player's interactor state naming facet entity `facet` (retail: the bag at its take, the
/// chest at its accept, 237308).
pub(super) const fn interactor_target(facet: u64) -> BundleMember {
    BundleMember {
        component: INTERACTOR_COMPONENT,
        state: MemberState::Interactor(InteractorState {
            target: Some(facet),
            enabled: None,
        }),
    }
}

/// The player's interactor state that lets it interact or not: retail turns it off when a fast
/// travel's channel starts and on again after the arrival (2026-11-07, `general-other-01` at
/// 619407 ms off and `Playthrough_Part_09` at 1386950 ms on, 36 to 104 ms after the 3696).
pub(super) const fn interaction_enabled(enabled: bool) -> BundleMember {
    BundleMember {
        component: INTERACTOR_COMPONENT,
        state: MemberState::Interactor(InteractorState {
            target: None,
            enabled: Some(enabled),
        }),
    }
}

/// `m_syncGatherRefCount` grows by one with each of a gather's two states (retail chest: 1 at the
/// start, 2 at the end; open-world gathers count on over the session).
const STATES_PER_GATHER: u32 = 2;

/// The two counters of a player's own gather state, counted over its session.
///
/// Observed (finding D8 "`m_gatherRefCount`", 11,437 of 11,944 steps of the own gather states of
/// 694 retail ledgers): `m_gatherRefCount` (child 4) steps by one for each change of the gather's
/// status and one for each change of its gatherer count: a shared object's start and end step it
/// by 2 each (status, count 0 to 1 and back), an instanced object's by 1 each, a count change in
/// the middle of a gather by 1 (monolith `EXP-The-Depths` 735.30 s 38 to 39, totem
/// `Playthrough_Part_18` 3334.58 s 133 to 134); at a group node's end in one tick each gatherer's
/// steps once more for each gatherer that ended before it (the count fell while it still
/// gathered: +3 for the second ender `HiveNormaleTheDepths` 1754.06 s, +4 for the third
/// `the_depths_group_run_tank_pov` 1708.60 s). A state that holds a whole gather (a broken quest
/// object, 12-5 nodules: +2; a water gather's end, `Hive_Dye` 74.05 s: 2 after none) steps it by
/// its start's and its end's status. Observed (finding D10, the 867 retail states of children 4,
/// 7 and 8, 787 of them with no interaction request of their player in the 6 s before): a whole
/// gather of an instanced object steps it by 2 (465 states; 399 with the object's tracker entry
/// within 1 s, the proximity and damageable quest objects of `master_quest_*_instanced`), one of
/// a shared object by 3 (402 states, 397 without a tracker entry; every one placed by a known
/// slice is shared: the Edengrove vines `IO_World_Damageable_CollisionVines`, the crypt coins,
/// the fire droppers; finding D11: 27 + 7 states of the vines, none with a tracker entry). The
/// sim sends a whole gather for its quest objects that a player breaks or comes near
/// ([`object_gathered_whole`]). `m_syncGatherRefCount` (child 8) steps by one at each start and end
/// ([`STATES_PER_GATHER`]). Unknown: the 431 retail states that send child 4 alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct GatherCounters {
    /// Gathers ended (`m_syncGatherRefCount` is twice this at an end).
    gathers: u32,
    /// `m_gatherRefCount`.
    refs: u32,
}

/// One step of `m_gatherRefCount`: a status change or a gatherer count change.
const REF_STEP: u32 = 1;

impl GatherCounters {
    /// The gathers ended so far.
    #[cfg(test)]
    pub(super) const fn gathers(&self) -> u32 {
        self.gathers
    }

    /// One step for a change of the gatherer count to or from `gatherers` (none for a gather
    /// that counts no gatherer).
    const fn count_step(gatherers: u8) -> u32 {
        if gatherers > NO_GATHERERS { REF_STEP } else { 0 }
    }

    /// A gather that counts `gatherers` starts: `(m_gatherRefCount, m_syncGatherRefCount)`.
    fn started(&mut self, gatherers: u8) -> (u32, u32) {
        self.refs = self
            .refs
            .saturating_add(REF_STEP)
            .saturating_add(Self::count_step(gatherers));
        let sync = self
            .gathers
            .saturating_mul(STATES_PER_GATHER)
            .saturating_add(1);
        (self.refs, sync)
    }

    /// The gatherer count of the running gather changes.
    fn gatherers_changed(&mut self) -> u32 {
        self.refs = self.refs.saturating_add(REF_STEP);
        self.refs
    }

    /// The gather whose start counted `counted` gatherers ends, after `ended_before` other
    /// gatherers of its group node ended in the same tick: the counters of its done state.
    fn ended(&mut self, counted: u8, ended_before: u32) -> (u32, u32) {
        self.refs = self
            .refs
            .saturating_add(REF_STEP)
            .saturating_add(Self::count_step(counted))
            .saturating_add(ended_before);
        self.gathers = self.gathers.saturating_add(1);
        (self.refs, self.gathers.saturating_mul(STATES_PER_GATHER))
    }

    /// A gather that counts `gatherers` and whose one state holds its start and its end (a quest
    /// object broken or come near, a water gather's end): the counters of that state, its
    /// start's status and gatherer count steps and its end's ([`Self::ended`]).
    fn whole_gather(&mut self, gatherers: u8) -> (u32, u32) {
        self.refs = self.refs.saturating_add(REF_STEP);
        self.ended(gatherers, 0)
    }
}

/// The own members when the next gather of player counters `counters` starts at the chest with
/// facet entity `facet`: the interactor target and the running gather (retail 237308 and
/// 237342).
pub(super) fn chest_gather_started(
    facet: u64,
    counters: &mut GatherCounters,
) -> [BundleMember; 2] {
    gather_started(
        facet,
        counters,
        GatherRun {
            amount: CHEST_AMOUNT,
            efficiency: CHEST_EFFICIENCY,
            gatherers: CHEST_GATHERERS,
        },
    )
}

/// The numbers of a running gather: the amount left (its `BaseGatherTime`), the efficiency and
/// the gatherers (gather note: retail flint 0.25, 1.0, 1; the chest 2.5, 1.0, 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GatherRun {
    pub(super) amount: f32,
    pub(super) efficiency: f32,
    pub(super) gatherers: u8,
}

/// The own members when the next gather of player counters `counters` starts at the object
/// with facet entity `facet`: the interactor target and the running gather `run`.
pub(super) fn gather_started(
    facet: u64,
    counters: &mut GatherCounters,
    run: GatherRun,
) -> [BundleMember; 2] {
    let (refs, sync) = counters.started(run.gatherers);
    [
        interactor_target(facet),
        BundleMember {
            component: GATHERING_COMPONENT,
            state: MemberState::Gathering(GatheringState {
                status: Some(GATHER_RUNNING),
                amount_remaining: Some(run.amount),
                efficiency: Some(run.efficiency),
                gatherer_count: Some(run.gatherers),
                gather_ref_count: Some(refs),
                is_gathering: Some(true),
                gatherable: Some(NO_GATHERABLE),
                sync_ref_count: Some(sync),
            }),
        },
    ]
}

/// The own member that gives a gatherer (player counters `counters`) of a shared object the
/// object's new gatherer count `gatherers` and, when the object's work done so far was counted,
/// the work `amount` left, with its `m_gatherRefCount` one step on ([`GatherCounters`]).
/// Observed (retail `HiveNormaleTheDepths` 1687.61 s, a group monolith: `{amount 7.5,
/// gatherers 2, ref 70}` when the third gatherer left; `EXP-The-Depths` 735.30 s: `{gatherers 3,
/// ref 39}` when a third came).
pub(super) fn gatherers_changed(
    counters: &mut GatherCounters,
    gatherers: u8,
    amount: Option<f32>,
) -> BundleMember {
    BundleMember {
        component: GATHERING_COMPONENT,
        state: MemberState::Gathering(GatheringState {
            status: None,
            amount_remaining: amount,
            efficiency: None,
            gatherer_count: Some(gatherers),
            gather_ref_count: Some(counters.gatherers_changed()),
            is_gathering: None,
            gatherable: None,
            sync_ref_count: None,
        }),
    }
}

/// The own member when the water gather of player counters `counters` ends: a done state that
/// names no gatherable, amount or efficiency (Observed: retail `Hive_Dye` 74.05 s, status 3,
/// counters 2, after the 3746; the client ends its gather animation on it, and the 3746 alone left
/// it running live 2026-11-03). It holds the whole gather ([`GatherCounters`]): a water gather
/// sends no start state.
pub(super) fn water_gather_ended(counters: &mut GatherCounters) -> BundleMember {
    let (refs, sync) = counters.whole_gather(NO_GATHERERS);
    BundleMember {
        component: GATHERING_COMPONENT,
        state: MemberState::Gathering(GatheringState {
            status: Some(GATHER_DONE),
            amount_remaining: None,
            efficiency: None,
            gatherer_count: None,
            gather_ref_count: Some(refs),
            is_gathering: Some(false),
            gatherable: None,
            sync_ref_count: Some(sync),
        }),
    }
}

/// The own member when the gather of player counters `counters` at the chest with facet entity
/// `facet` ends: the done state of a gather that counted [`CHEST_GATHERERS`] ([`gather_ended`]).
pub(super) fn chest_gather_ended(facet: u64, counters: &mut GatherCounters) -> BundleMember {
    gather_ended(facet, counters, CHEST_GATHERERS, 0)
}

/// The own member when the gather of player counters `counters` at the object with facet entity
/// `facet`, whose start counted `counted` gatherers, ends after `ended_before` other gatherers of
/// its group node ended in the same tick ([`GatherCounters`]) (retail chest 239343 and flint,
/// with the 3746), or stops at a cancel (retail `NH-MSQ-1` 1789560435207: the same done state).
/// Its gatherer count goes back to [`NO_GATHERERS`] when the start counted any, and is not sent
/// when it counted none (Observed: retail `Playthrough_Part_04` 114.01 s after a start that
/// counted 0).
pub(super) fn gather_ended(
    facet: u64,
    counters: &mut GatherCounters,
    counted: u8,
    ended_before: u32,
) -> BundleMember {
    let (refs, sync) = counters.ended(counted, ended_before);
    BundleMember {
        component: GATHERING_COMPONENT,
        state: MemberState::Gathering(GatheringState {
            status: Some(GATHER_DONE),
            amount_remaining: Some(0.0),
            efficiency: Some(0.0),
            gatherer_count: (counted > NO_GATHERERS).then_some(NO_GATHERERS),
            gather_ref_count: Some(refs),
            is_gathering: Some(false),
            gatherable: Some(facet),
            sync_ref_count: Some(sync),
        }),
    }
}

/// The gatherers a whole gather of a shared object counts in its counter steps: the player
/// (Inferred: the +3 of the retail shared whole gathers is the +1 of an instanced object's start
/// and end each, plus the start's count change from none; the end sends no count change).
const SHARED_WHOLE_GATHERERS: u8 = 1;

/// The own member when the player of counters `counters` gathers the quest object of facet entity
/// `facet` in one state: it breaks it or comes near it. Observed (retail `12-5_survivors`
/// 1790515520732 ms: with the loot tracker's broken entry, the player's gather state names the
/// nodule and counts on, both counters by 2 (156.57 s, 159.44 s, 163.76 s); finding D10, 867
/// states of children 4, 7 and 8 only): status, amount, efficiency and gathering are not sent,
/// `m_syncGatherRefCount` steps by 2, `m_gatherRefCount` by 2 for an instanced object (465
/// states) and by 3 for a `shared` one (402 states, [`SHARED_WHOLE_GATHERERS`]).
pub(super) fn object_gathered_whole(
    facet: u64,
    counters: &mut GatherCounters,
    shared: bool,
) -> BundleMember {
    let gatherers = if shared {
        SHARED_WHOLE_GATHERERS
    } else {
        NO_GATHERERS
    };
    let (refs, sync) = counters.whole_gather(gatherers);
    BundleMember {
        component: GATHERING_COMPONENT,
        state: MemberState::Gathering(GatheringState {
            status: None,
            amount_remaining: None,
            efficiency: None,
            gatherer_count: None,
            gather_ref_count: Some(refs),
            is_gathering: None,
            gatherable: Some(facet),
            sync_ref_count: Some(sync),
        }),
    }
}

/// The record of `item` in a loot container's slot `slot` (retail: quantity 1, sync reason
/// [`SYNC_CONTAINER`], the slot as the counter) with its fixed perks (retail: the Destroyer
/// weapons carry wire perk 1203, `PerkID_StatSwap_BaseDamage`).
fn container_item(character: Uuid, slot: u16, item: &KitItem) -> ItemRecord {
    ItemRecord {
        quantity: 1,
        sync_reason: SYNC_CONTAINER,
        container_slot: Some(slot),
        item_index: item.item_index,
        durability: item.durability,
        max_durability: item.durability,
        counter: u32::from(slot),
        gear_score: item.gear_score,
        perks: item.perks,
        instance: instance_id(character, &item.item_id),
        flags: if item.bound { ITEM_BOUND } else { 0 },
        ..ItemRecord::EMPTY_SLOT
    }
}

/// The table index of the entity whose `GdeRef` starts with `prefix`.
fn find(entities: &FtueEntities, prefix: &[u8; GDE_REF_PREFIX_LEN]) -> Option<usize> {
    entities
        .entities
        .iter()
        .position(|entity| &entity.gde_ref_prefix == prefix)
}

#[cfg(test)]
mod tests {
    use nw_content::ftue_entities::{EntityGroup, FtueEntity};
    use nw_content::starter::{EquipSlot, KitItem};
    use nw_messages::wire::{Reader, Writer};
    use nw_messages::{CONTAINER_TYPE_INDEX, INTERACT_TYPE_INDEX};

    use super::*;

    const SHIP_PREFIX: [u8; GDE_REF_PREFIX_LEN] = [1; GDE_REF_PREFIX_LEN];
    const BAG_PREFIX: [u8; GDE_REF_PREFIX_LEN] = [2; GDE_REF_PREFIX_LEN];
    const CARD_CLASS: u64 = 10;
    const DELAYED_EVENT_CLASS: u64 = 3139;
    const PHASE: u64 = 0x79bb_231b_6ad2_7648;

    fn group(time_ms: u64, class: u64, body: Vec<u8>, phase_at: Vec<usize>) -> EntityGroup {
        EntityGroup {
            time_ms,
            interest: 9,
            members: vec![EntityMember {
                component: 0,
                class,
                body,
                player_gde_ref_at: Vec::new(),
                player_facet_at: Vec::new(),
                phase_suffix_at: phase_at,
            }],
        }
    }

    /// A ship entity and a bag with one change.
    fn entities() -> FtueEntities {
        let card = |prefix: [u8; GDE_REF_PREFIX_LEN]| {
            let mut body = vec![0x01, 0x02];
            body.extend(prefix);
            body.extend([0; GDE_REF_PREFIX_LEN]);
            body
        };
        FtueEntities {
            level: "ftue_v2".to_owned(),
            entities: vec![
                FtueEntity {
                    gde_ref_prefix: SHIP_PREFIX,
                    spawns: vec![group(5100, CARD_CLASS, card(SHIP_PREFIX), vec![10])],
                    changes: Vec::new(),
                    despawns: vec![RETAIL_WATER_MOVE_MS],
                },
                FtueEntity {
                    gde_ref_prefix: BAG_PREFIX,
                    spawns: vec![group(164_262, CARD_CLASS, card(BAG_PREFIX), vec![10])],
                    changes: vec![group(
                        169_211,
                        DELAYED_EVENT_CLASS,
                        vec![1, 3, 0, 0, 1],
                        vec![],
                    )],
                    despawns: Vec::new(),
                },
            ],
        }
    }

    fn ids() -> SessionIds {
        SessionIds {
            player_gde_ref: [7; 16],
            player_facet: [8; 8],
            phase_id: PHASE,
        }
    }

    const fn slot(interest: u16, new: bool) -> Slot {
        Slot { interest, new }
    }

    #[test]
    fn the_island_shows_its_entities_with_the_session_phase() {
        let entities = entities();
        let mut views = StaticViews::default();
        views.show_island(&entities);
        let bag = STATIC_ENTITY_BASE + 1;
        assert_eq!(views.numbers().collect::<Vec<_>>(), [bag], "not the ship's");
        let group = views
            .group(bag, slot(6, true), &entities, &ids())
            .expect("card");
        assert_eq!(group.interest, 6);
        let MemberState::Recorded { body, .. } = &group.members[0].state else {
            panic!("recorded");
        };
        assert_eq!(body[10..], PHASE.to_le_bytes());
        assert_eq!(
            views.group(bag, slot(6, false), &entities, &ids()),
            None,
            "nothing more to send"
        );
    }

    #[test]
    fn a_change_goes_after_the_card_and_a_hidden_entity_leaves() {
        let entities = entities();
        let mut views = StaticViews::default();
        views.show_island(&entities);
        views.change(&entities, &BAG_PREFIX, 0);
        views.change(&entities, &BAG_PREFIX, 1);
        let bag = STATIC_ENTITY_BASE + 1;
        let card = views.group(bag, slot(9, true), &entities, &ids());
        assert_eq!(
            card.expect("card").members[0].state.type_index(),
            CARD_CLASS
        );
        let change = views.group(bag, slot(9, false), &entities, &ids());
        assert_eq!(
            change.expect("change").members[0].state.type_index(),
            DELAYED_EVENT_CLASS
        );
        views.hide(&entities, &BAG_PREFIX);
        assert_eq!(views.numbers().count(), 0);
    }

    const CHARACTER: Uuid = Uuid::from_u128(0x42);

    /// The wire perk the retail Destroyer weapons carry (`PerkID_StatSwap_BaseDamage`).
    const DESTROYER_WEAPON_PERK: u32 = 1203;

    /// The retail Destroyer bag: greataxe, head, warhammer (fill group of 164578).
    fn bag_items() -> [KitItem; 3] {
        let kit = |item_id: &str, item_index, gear_score, durability, slot| KitItem {
            item_id: item_id.to_owned(),
            item_index,
            slot,
            gear_score,
            durability,
            bound: true,
            max_stack: Some(1),
            perk_ids: Vec::new(),
            perks: [0; nw_messages::items::PERK_SLOTS],
            draws_shield: false,
            artifact: None,
        };
        let weapon = |item_id: &str, item_index| KitItem {
            perk_ids: vec!["PerkID_StatSwap_BaseDamage".to_owned()],
            perks: [DESTROYER_WEAPON_PERK, 0, 0, 0, 0],
            ..kit(item_id, item_index, 75, 700, Some(EquipSlot::MainHand))
        };
        [
            weapon("greataxe", 40_265),
            kit("head", 40_235, 75, 375, Some(EquipSlot::Head)),
            weapon("warhammer", 40_266),
        ]
    }

    #[test]
    fn the_bag_loot_is_the_retail_fill_group() {
        let members = bag_loot(CHARACTER, &bag_items());
        assert_eq!(members.len(), 2);
        let MemberState::Container(state) = &members[0].state else {
            panic!("container");
        };
        assert_eq!(members[0].component, 2);
        assert_eq!(state.bonus_max_encumbrance, Some(0));
        assert_eq!(state.was_emptied, Some(false));
        let items = state
            .items
            .as_ref()
            .and_then(ReplicatedList::full)
            .expect("items");
        assert_eq!(items.version, Some(BAG_FILLED_VERSION));
        assert_eq!(items.elements.len(), 3);
        // The retail greataxe record's packed number, slot 0 (quantity 1, sync reason 3).
        assert_eq!(items.elements[0].quantity, 1);
        assert_eq!(items.elements[0].sync_reason, SYNC_CONTAINER);
        assert_eq!(items.elements[0].container_slot, Some(0));
        assert_eq!(items.elements[0].item_index, 40_265);
        assert_eq!(items.elements[0].durability, 700);
        assert_eq!(items.elements[0].max_durability, 700);
        assert_eq!(items.elements[0].gear_score, 75);
        assert_eq!(items.elements[0].flags, ITEM_BOUND);
        assert_eq!(items.elements[0].perks[0], DESTROYER_WEAPON_PERK);
        assert_eq!(items.elements[1].perks[0], 0);
        assert_eq!(
            items.elements[0].instance,
            instance_id(CHARACTER, "greataxe")
        );
        let mut writer = Writer::new();
        items.elements[0].write(&mut writer).expect("record");
        // Retail packed bytes of the greataxe.
        assert_eq!(
            writer.finish()[..8],
            [0x00, 0x01, 0x00, 0xa3, 0xb1, 0x53, 0xc9, 0x45]
        );
        // The retail body of the whole member.
        let body = members[0].encode().expect("member");
        assert_eq!(
            body[..9],
            [0x02, 0x9b, 0x1b, 0x01, 0x15, 0x00, 0x01, 0x05, 0x03]
        );
        assert_eq!(members[1].component, 6);
        assert_eq!(
            members[1].state,
            MemberState::Recorded {
                class: LOOT_DROP_TYPE_INDEX,
                body: LOOT_DROP_ON.to_vec()
            }
        );
    }

    #[test]
    fn the_emptied_bag_is_the_retail_take_group() {
        let members = bag_emptied(3);
        assert_eq!(
            members
                .iter()
                .map(|member| (member.component, member.state.type_index()))
                .collect::<Vec<_>>(),
            [
                (2, CONTAINER_TYPE_INDEX),
                (3, INTERACT_TYPE_INDEX),
                (6, LOOT_DROP_TYPE_INDEX),
                (7, DELAYED_EVENT_TYPE_INDEX),
                (8, DELAYED_EVENT_TYPE_INDEX),
            ]
        );
        let MemberState::Container(state) = &members[0].state else {
            panic!("container");
        };
        assert_eq!(state.was_emptied, Some(true));
        let items = state
            .items
            .as_ref()
            .and_then(ReplicatedList::full)
            .expect("items");
        assert_eq!(items.version, Some(BAG_EMPTIED_VERSION));
        assert_eq!(items.elements.len(), 3);
        for record in &items.elements {
            assert_eq!(record.quantity, 0);
            assert_eq!(record.sync_reason, SYNC_MOVED);
            assert_eq!(record.container_slot, None);
            assert_eq!(record.item_index, 0);
        }
        // Retail take body: slot 0, children {items, was_emptied}, version 6, 3 records, true.
        let body = members[0].encode().expect("member");
        assert_eq!(
            body[..9],
            [0x02, 0x9b, 0x1b, 0x01, 0x11, 0x00, 0x01, 0x06, 0x03]
        );
        assert_eq!(body[body.len() - 1], 0x01);
        // The empty records read back.
        let mut reader = Reader::new(&body[3..]);
        let read = ContainerState::read(&mut reader).expect("read");
        assert_eq!(read, *state);
    }

    /// The retail chest's facet entity (tutorial capture).
    const CHEST_FACET: u64 = 0x5870_3581_9fa0_a5ef;

    #[test]
    fn a_facet_is_the_prefix_in_reverse() {
        assert_eq!(
            facet_of(&super::super::encounter::CHEST_PREFIX),
            CHEST_FACET
        );
    }

    #[test]
    fn the_first_chest_gather_sends_the_retail_states() {
        let mut counters = GatherCounters::default();
        let [target, running] = chest_gather_started(CHEST_FACET, &mut counters);
        // Retail 237308: 26:3752 `01 01 <chest facet>`.
        const TARGET: [u8; 10] = [0x01, 0x01, 0x58, 0x70, 0x35, 0x81, 0x9f, 0xa0, 0xa5, 0xef];
        assert_eq!(target.component, INTERACTOR_COMPONENT);
        assert_eq!(
            target.state,
            MemberState::decode(nw_messages::INTERACTOR_TYPE_INDEX, &TARGET).expect("retail")
        );
        // Retail 237342: 36:81, the whole running gather.
        const RUNNING: [u8; 30] = [
            0x02, 0xdf, 0x01, 0x40, 0x20, 0, 0, 0x3f, 0x80, 0, 0, 0, 0, 0, 0, 0x01, 0x01, 0x03, 0,
            0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0x01,
        ];
        assert_eq!(running.component, GATHERING_COMPONENT);
        assert_eq!(
            running.state,
            MemberState::decode(nw_messages::GATHERING_TYPE_INDEX, &RUNNING).expect("retail")
        );
        // Retail 239343: 36:81, the end naming the chest.
        const ENDED: [u8; 29] = [
            0x02, 0xd7, 0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02, 0x00, 0x03, 0x58, 0x70, 0x35,
            0x81, 0x9f, 0xa0, 0xa5, 0xef, 0, 0, 0, 0x02,
        ];
        let ended = chest_gather_ended(CHEST_FACET, &mut counters);
        assert_eq!(ended.component, GATHERING_COMPONENT);
        assert_eq!(
            ended.state,
            MemberState::decode(nw_messages::GATHERING_TYPE_INDEX, &ENDED).expect("retail")
        );
    }

    /// `(m_gatherRefCount, m_syncGatherRefCount)` of a gather state.
    fn counts(member: &BundleMember) -> (Option<u32>, Option<u32>) {
        match member.state {
            MemberState::Gathering(state) => (state.gather_ref_count, state.sync_ref_count),
            _ => panic!("gathering"),
        }
    }

    #[test]
    fn a_later_gather_counts_on() {
        let mut counters = GatherCounters::default();
        chest_gather_started(CHEST_FACET, &mut counters);
        chest_gather_ended(CHEST_FACET, &mut counters);
        let [_, running] = chest_gather_started(CHEST_FACET, &mut counters);
        assert_eq!(counts(&running), (Some(3), Some(3)));
        assert_eq!(
            counts(&chest_gather_ended(CHEST_FACET, &mut counters)),
            (Some(4), Some(4))
        );
        assert_eq!(counters.gathers(), 2);
    }

    /// Retail `01-Amrine_Excavation` 38.23 s to 71.11 s: an instanced object's gather (refs 1, 2),
    /// then two shared ones (refs 4, 6 and 8, 10); the sync counter steps by one per state.
    #[test]
    fn the_ref_counter_steps_by_status_and_gatherer_count_changes() {
        const SHARED: GatherRun = GatherRun {
            amount: 15.0,
            efficiency: 1.0,
            gatherers: 1,
        };
        const INSTANCED: GatherRun = GatherRun {
            gatherers: NO_GATHERERS,
            ..SHARED
        };
        let mut counters = GatherCounters::default();
        let mut gather = |run: GatherRun| {
            let [_, running] = gather_started(CHEST_FACET, &mut counters, run);
            let done = gather_ended(CHEST_FACET, &mut counters, run.gatherers, 0);
            (counts(&running), counts(&done))
        };
        assert_eq!(gather(INSTANCED), ((Some(1), Some(1)), (Some(2), Some(2))));
        assert_eq!(gather(SHARED), ((Some(4), Some(3)), (Some(6), Some(4))));
        assert_eq!(gather(SHARED), ((Some(8), Some(5)), (Some(10), Some(6))));
    }

    /// Retail group monolith (`EXP-The-Depths`): refs 38 at a start with 2 gatherers, 39 when a
    /// third came; the end of a group node in one tick steps each gatherer once more for each
    /// gatherer that ended before it (+3 for the second, +4 for the third).
    #[test]
    fn a_count_change_and_the_earlier_enders_of_a_group_node_step_the_ref_counter() {
        const GROUP: GatherRun = GatherRun {
            amount: 10.0,
            efficiency: 1.0,
            gatherers: 2,
        };
        const THIRD: u8 = 3;
        let mut counters = GatherCounters::default();
        let [_, running] = gather_started(CHEST_FACET, &mut counters, GROUP);
        assert_eq!(counts(&running).0, Some(2));
        assert_eq!(
            counts(&gatherers_changed(&mut counters, THIRD, None)).0,
            Some(3),
            "a count change in the middle of the gather"
        );
        let mut second = counters;
        let mut third = counters;
        assert_eq!(
            counts(&gather_ended(CHEST_FACET, &mut counters, GROUP.gatherers, 0)).0,
            Some(5),
            "the first ender: status and count"
        );
        assert_eq!(
            counts(&gather_ended(CHEST_FACET, &mut second, GROUP.gatherers, 1)).0,
            Some(6)
        );
        assert_eq!(
            counts(&gather_ended(CHEST_FACET, &mut third, GROUP.gatherers, 2)).0,
            Some(7)
        );
    }

    /// Retail `12-5_survivors` 156.57 s to 163.76 s: each broken nodule steps both counters by 2;
    /// retail `Hive_Dye` 74.05 s: the first water gather's end has both counters at 2.
    #[test]
    fn a_state_that_holds_a_whole_gather_steps_both_counters_by_two() {
        let mut counters = GatherCounters::default();
        assert_eq!(
            counts(&water_gather_ended(&mut counters)),
            (Some(2), Some(2))
        );
        assert_eq!(
            counts(&object_gathered_whole(CHEST_FACET, &mut counters, false)),
            (Some(4), Some(4))
        );
        assert_eq!(counters.gathers(), 2);
    }

    /// Finding D10 (402 retail states of children 4, 7 and 8 of shared objects, the Edengrove
    /// vines `IO_World_Damageable_CollisionVines` among them): a shared object's whole gather
    /// steps `m_gatherRefCount` by 3 and `m_syncGatherRefCount` by 2, and names the object.
    #[test]
    fn a_whole_gather_of_a_shared_object_steps_the_ref_counter_by_three() {
        let mut counters = GatherCounters::default();
        let shared = object_gathered_whole(CHEST_FACET, &mut counters, true);
        assert_eq!(counts(&shared), (Some(3), Some(2)));
        let MemberState::Gathering(state) = shared.state else {
            panic!("gathering");
        };
        assert_eq!(state.gatherable, Some(CHEST_FACET));
        assert_eq!(
            (state.status, state.gatherer_count, state.is_gathering),
            (None, None, None),
            "children 4, 7 and 8 only"
        );
        assert_eq!(
            counts(&object_gathered_whole(CHEST_FACET, &mut counters, false)),
            (Some(5), Some(4)),
            "an instanced one after it: +2 and +2"
        );
    }

    #[test]
    fn the_take_names_the_players_interactor_target() {
        let member = interactor_target(facet_of(&BAG_PREFIX));
        assert_eq!(member.component, 26);
        // Retail: `01 01` and the bag's entity id, the prefix in reverse order.
        let body: Vec<u8> = [0x01, 0x01]
            .into_iter()
            .chain(BAG_PREFIX.iter().rev().copied())
            .collect();
        assert_eq!(
            member.state,
            MemberState::decode(nw_messages::INTERACTOR_TYPE_INDEX, &body).expect("retail")
        );
    }

    #[test]
    fn a_queued_group_goes_out_after_the_card() {
        let entities = entities();
        let mut views = StaticViews::default();
        views.show_island(&entities);
        let bag = STATIC_ENTITY_BASE + 1;
        views.queue(&entities, &BAG_PREFIX, bag_loot(CHARACTER, &bag_items()));
        let card = views.group(bag, slot(9, true), &entities, &ids());
        assert_eq!(
            card.expect("card").members[0].state.type_index(),
            CARD_CLASS
        );
        let loot = views
            .group(bag, slot(9, false), &entities, &ids())
            .expect("loot");
        assert!(matches!(loot.members[0].state, MemberState::Container(_)));
        assert_eq!(views.group(bag, slot(9, false), &entities, &ids()), None);
    }
}
