// SPDX-License-Identifier: AGPL-3.0-or-later
//! What one test client learns from the world messages (M4 spec section 9): the order of the
//! login messages, its own entity number (from `ClientAddEntryMsg`), and every position of every
//! entity in the replication bundles, in order. A bundle carries one group per entity slot; a
//! slot is tied to its entity by the spawn card in it ([`EntitySlots`]), and slot 1 is the
//! client's own player. A hunt bot also keeps the mobs it is sent and counts the combat messages
//! for it (M6-B spec section 9.1). The own groups state tells the own group and the invites to
//! it (an expedition group's bots, [`crate::group`]). The own game mode participant state
//! (3312) tells the queue's pop, the match id and the game modes the player is in.

use std::collections::{BTreeMap, BTreeSet};

use nw_content::town_props::ASSET_ID_LEN;
use nw_messages::entity_ids::IdHolder;
use nw_messages::fishing::StateTransition;
use nw_messages::game_mode_participant::GameModeParticipantState;
use nw_messages::gde::LOOT_BAG_ASSET;
use nw_messages::social::states::GroupsState;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BAG_COMPONENT, EntitySlots, ItemRecord, LOGIN_MESSAGE_TYPES, MemberState, MessageError,
    PLAYER_VITALS_ID, ReplicatedList, ServerMessage, SlotRead, SlotUpdate, VitalsData,
    WorldPosition, entity_of_facet, number_of_seen_raw_id, release_raw_id,
};
use thiserror::Error;

use crate::achievements::OwnAchievements;
use crate::rep::Received;
use nw_messages::objective_interactor::MissionParams;

/// A world message that does not decode.
#[derive(Debug, Error)]
pub enum ViewError {
    /// A message of a world type, or a bundle payload, does not decode.
    #[error("message type {type_index} does not decode: {source}")]
    Message {
        /// Its type index.
        type_index: u64,
        /// Why.
        #[source]
        source: MessageError,
    },
}

/// A combat message, by the entity it is for; `None` for an entity this process never saw.
enum CombatEvent {
    /// 2071: a hit the entity made.
    Dealt { attacker: Option<u64> },
    /// 3601: a hit the entity took.
    Taken { target: Option<u64> },
    /// 4299: the entity died.
    Died { player: Option<u64> },
}

/// The server's answer to an interaction of the bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionAnswer {
    /// 3743: the interaction `interaction` was taken (`status` 1) or refused (another status).
    Validated {
        /// The interaction id the bot sent.
        interaction: u64,
        /// The result.
        status: u8,
    },
    /// 3746: the interaction is over (the gather time ran out).
    Ended,
}

/// One world message, read.
enum Read {
    /// Not a world message.
    Skipped,
    /// A bundle, before its slots are resolved.
    Bundle(SlotRead),
    /// A combat message.
    Combat { type_index: u64, event: CombatEvent },
    /// `ClientAddEntryMsg`, with the client's own entity number.
    Entry { type_index: u64, own: Option<u64> },
    /// An answer to an interaction (3743, 3746).
    Interaction {
        type_index: u64,
        answer: InteractionAnswer,
    },
    /// Another world message.
    Other { type_index: u64 },
}

/// Numbers the entity of every spawn card in `read` by its raw id ([`number_of_seen_raw_id`]),
/// held by `holder`: the server's raw ids carry no entity number, so the clients of this process
/// key the entities they see by these numbers, and all of them agree.
fn number_cards(read: &mut SlotRead, holder: IdHolder) {
    for update in &mut read.updates {
        if let Some(gde_ref) = update.card.and_then(|card| card.gde_ref) {
            update.entity = Some(number_of_seen_raw_id(holder, &gde_ref));
        }
    }
}

/// How [`decode`] numbers the entities of the spawn cards: by `holder`, or not at all.
#[derive(Debug, Clone, Copy)]
enum Cards {
    /// The cards' entities are numbered and held by the holder.
    Numbered,
    /// The cards are read without numbers (a view that keeps only its own player).
    Skipped,
}

fn decode(received: &Received, holder: IdHolder, cards: Cards) -> Result<Read, ViewError> {
    let envelope = &received.envelope;
    let Some(decoded) = ServerMessage::from_envelope(envelope) else {
        return Ok(Read::Skipped);
    };
    let type_index = envelope.type_index().unwrap_or_default();
    let failed = |source| ViewError::Message { type_index, source };
    Ok(match decoded.map_err(failed)? {
        ServerMessage::Bundle(bundle) => {
            let mut read = SlotRead::of(&bundle).map_err(failed)?;
            if matches!(cards, Cards::Numbered) {
                number_cards(&mut read, holder);
            }
            Read::Bundle(read)
        }
        ServerMessage::DamageDealt(hit) => Read::Combat {
            type_index,
            event: CombatEvent::Dealt {
                attacker: entity_of_facet(hit.head_b),
            },
        },
        ServerMessage::OnDamage(hit) => Read::Combat {
            type_index,
            event: CombatEvent::Taken {
                target: entity_of_facet(hit.head_b),
            },
        },
        ServerMessage::DeathRecap(death) => Read::Combat {
            type_index,
            event: CombatEvent::Died {
                player: entity_of_facet(death.head_b),
            },
        },
        ServerMessage::InteractionValidated(answer) => Read::Interaction {
            type_index,
            answer: InteractionAnswer::Validated {
                interaction: answer.interaction,
                status: answer.status,
            },
        },
        ServerMessage::InteractionEnded(_) => Read::Interaction {
            type_index,
            answer: InteractionAnswer::Ended,
        },
        ServerMessage::ClientAddEntry(entry) => Read::Entry {
            type_index,
            own: Some(number_of_seen_raw_id(holder, &entry.entry_id)),
        },
        _ => Read::Other { type_index },
    })
}

/// What a bot keeps of the world messages: its own entity number only. Many bots run in one
/// process, so the others' positions are read and dropped (M5 load spec section 7.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OwnView {
    own: Option<u64>,
    /// Its hold on the ids of its own player and the entities its owner view keeps.
    holder: IdHolder,
    /// Whether a bundle named the own player: the own spawn card came.
    own_card_seen: bool,
    /// Whether [`OwnView::time_sync_due`] gave the own number already.
    time_sync_given: bool,
}

impl OwnView {
    /// Reads one more message; returns the client's own position when it is a bundle with it.
    ///
    /// # Errors
    ///
    /// Returns [`ViewError`] for a world message or a bundle payload that does not decode.
    pub fn read(&mut self, received: &Received) -> Result<Option<WorldPosition>, ViewError> {
        Ok(self.apply(&decode(received, self.holder, Cards::Skipped)?))
    }

    /// Learns the own number from `read`; returns the own position when it is in a bundle.
    fn apply(&mut self, read: &Read) -> Option<WorldPosition> {
        match read {
            Read::Bundle(bundle) => {
                if bundle.updates.iter().any(SlotUpdate::is_local_player) {
                    self.own_card_seen = true;
                }
                bundle.local_player_position()
            }
            Read::Entry { own, .. } => {
                // A level change sends the entry again (the end of the tutorial, `login.rs` of
                // the sim): the new level holds every other entity until the client's time sync
                // request after its own new card, as a client sends one after each level load.
                self.own = *own;
                self.own_card_seen = false;
                self.time_sync_given = false;
                None
            }
            Read::Skipped | Read::Combat { .. } | Read::Interaction { .. } | Read::Other { .. } => {
                None
            }
        }
    }

    /// Whether `number` is the own entity number.
    fn is_own(&self, number: u64) -> bool {
        self.own == Some(number)
    }

    /// The own entity number, once the entry message named it.
    pub const fn own_number(&self) -> Option<u64> {
        self.own
    }

    /// The own entity number, once per level entry, when the first time sync request is due: the
    /// own spawn card came. The client sends that request when it is in the game, and the sim
    /// holds every other entity until then (`SimWorld::heard_from`; with the local player card
    /// on, as the real-client stack runs). Before the card the sim ignores the request.
    pub fn time_sync_due(&mut self) -> Option<u64> {
        let own = self
            .own
            .filter(|_| self.own_card_seen && !self.time_sync_given)?;
        self.time_sync_given = true;
        Some(own)
    }
}

/// What a swarm bot keeps to measure how fast others see a player's action (replication note):
/// its own position, and the block state (`isBlockActive` of 1528) of each other player as it
/// changes in its bundles.
#[derive(Debug, Clone, Default)]
pub struct SyncView {
    own: OwnView,
    slots: EntitySlots,
}

/// One message as a [`SyncView`] read it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncRead {
    /// The own position, when the message is a bundle with it.
    pub own_position: Option<WorldPosition>,
    /// The block states of other entities it carried, by entity number.
    pub blocks: Vec<(u64, bool)>,
    /// The own player's states it carried, in order ([`crate::crossing_state`]).
    pub own_states: Vec<MemberState>,
}

impl SyncView {
    /// Reads one more message.
    ///
    /// # Errors
    ///
    /// Returns [`ViewError`] for a world message or a bundle payload that does not decode.
    pub fn read(&mut self, received: &Received) -> Result<SyncRead, ViewError> {
        let mut read = decode(received, self.own.holder, Cards::Numbered)?;
        let own_position = self.own.apply(&read);
        let mut blocks = Vec::new();
        let mut own_states = Vec::new();
        if let Read::Bundle(bundle) = &mut read {
            for gone in self.slots.resolve(bundle) {
                release_raw_id(self.own.holder, gone);
            }
            own_states.extend(
                bundle
                    .updates
                    .iter()
                    .filter(|update| update.is_local_player())
                    .flat_map(|update| update.members.iter().map(|member| member.state.clone())),
            );
            for update in &bundle.updates {
                let Some(entity) = update.entity.filter(|entity| !self.own.is_own(*entity)) else {
                    continue;
                };
                blocks.extend(
                    update
                        .members
                        .iter()
                        .filter_map(|member| match &member.state {
                            MemberState::DamageReceiver(receiver) => {
                                receiver.is_block_active.map(|blocking| (entity, blocking))
                            }
                            _ => None,
                        }),
                );
            }
        }
        Ok(SyncRead {
            own_position,
            blocks,
            own_states,
        })
    }

    /// The own entity number, once the entry message named it.
    pub const fn own_number(&self) -> Option<u64> {
        self.own.own_number()
    }

    /// The own number when the first time sync request is due ([`OwnView::time_sync_due`]).
    pub fn time_sync_due(&mut self) -> Option<u64> {
        self.own.time_sync_due()
    }
}

/// What a hunt bot keeps: its own position, the mobs it is sent, the bundles it got, and counts
/// of the combat messages for it. An entity is a mob from its spawn card ([`is_mob_card`]):
/// players, statics, gather nodes, stations and quest objects have cards too, and a bot that took
/// a rock for a mob swung at it all run (live hunt 2026-10-31). A mob stays in view until its slot is stopped: the server sends a mob again only
/// when it has something new (a group with nothing to send is left out), so a mob that stands
/// still is not in every bundle. A mob whose vitals say it died is dropped at once; a respawned
/// mob comes with a new card.
#[derive(Debug, Clone, Default)]
pub struct CombatView {
    own: OwnView,
    bundles: u64,
    /// Mob cards read so far.
    mob_cards: u64,
    slots: EntitySlots,
    /// The mobs by entity number; `None` until a mob's first position.
    mobs: BTreeMap<u64, Option<WorldPosition>>,
    hits_dealt: u64,
    hits_taken: u64,
    deaths: u64,
    /// The bot's own newest vitals said it is dead: no health left, or the dead block.
    own_dead: bool,
    /// The bot's bag: one item per bag slot, empty slots included (quantity 0), as its own
    /// bag container sent them.
    bag: Vec<ItemRecord>,
    /// The bot's own achievements, from each state of its own player.
    achievements: OwnAchievements,
    /// The missions of the board the bot has open: the list of its own objective interactor
    /// state (3829), a full list or its changed elements.
    board: Vec<MissionParams>,
    /// Mobs in view whose vitals said they died.
    mob_deaths: u64,
    /// The vitals id of each mob in view whose card named one.
    mob_vitals: BTreeMap<u64, u32>,
    /// The mobs in view whose vitals say they take no hits now (`invulnerability` not 0).
    invulnerable: BTreeSet<u64>,
    /// The mobs in view that died, oldest first, while their slots have not stopped: the
    /// corpses a skin step gathers.
    corpses: Vec<Corpse>,
    /// The answers to the bot's interactions since the last [`CombatView::take_interactions`].
    interactions: Vec<InteractionAnswer>,
    /// The id of the newest card seen of each slice asset.
    cards: BTreeMap<[u8; ASSET_ID_LEN], [u8; RAW_ID_LEN]>,
    /// The ids of the loot bags seen ([`LOOT_BAG_ASSET`] cards), oldest first.
    bags: Vec<[u8; RAW_ID_LEN]>,
    /// The bot's own fishing steps seen, by sequence (each listing repeats the steps the client
    /// has not acknowledged yet).
    fishing: Vec<StateTransition>,
}

/// Adds the steps of a fishing listing `listed` that `seen` does not hold yet (a later
/// sequence than its last).
fn note_fishing(seen: &mut Vec<StateTransition>, listed: &[StateTransition]) {
    for step in listed {
        if seen.last().is_none_or(|last| step.sequence > last.sequence) {
            seen.push(*step);
        }
    }
}

impl CombatView {
    /// Reads one more message; returns the client's own position when it is a bundle with it.
    ///
    /// # Errors
    ///
    /// Returns [`ViewError`] for a world message or a bundle payload that does not decode.
    pub fn read(&mut self, received: &Received) -> Result<Option<WorldPosition>, ViewError> {
        let read = decode(received, self.own.holder, Cards::Numbered)?;
        let own_position = self.own.apply(&read);
        match read {
            Read::Bundle(mut bundle) => {
                self.bundles += 1;
                // A saved dead player logs in with its own vitals at no health and gets no 4299
                // (bot soak 2026-11-21: 18 of 30 hunt characters came back dead and stayed so).
                for update in bundle
                    .updates
                    .iter()
                    .filter(|update| update.is_local_player())
                {
                    if let Some(dead) = vitals_dead(update) {
                        self.own_dead = dead;
                    }
                    for member in &update.members {
                        if let (BAG_COMPONENT, MemberState::Container(container)) =
                            (member.component, &member.state)
                            && let Some(items) = &container.items
                        {
                            apply_bag(&mut self.bag, items);
                        }
                        if let MemberState::Achievement(state) = &member.state {
                            self.achievements.apply(state);
                        }
                        if let MemberState::ObjectiveInteractor(state) = &member.state
                            && let Some(missions) = &state.missions
                        {
                            apply_list(&mut self.board, missions);
                        }
                        if let MemberState::Fishing(state) = &member.state
                            && let Some(listing) = &state.transitions
                        {
                            note_fishing(&mut self.fishing, &listing.elements);
                        }
                    }
                }
                for gone in self.slots.resolve(&mut bundle) {
                    self.mobs.remove(&gone);
                    self.mob_vitals.remove(&gone);
                    self.invulnerable.remove(&gone);
                    // The corpse went: no skin is left.
                    self.corpses.retain(|corpse| corpse.entity != gone);
                    release_raw_id(self.own.holder, gone);
                }
                for update in bundle
                    .updates
                    .iter()
                    .filter(|update| !update.is_local_player())
                {
                    if let Some((asset, gde_ref)) =
                        update.card.and_then(|card| card.asset.zip(card.gde_ref))
                    {
                        self.cards.insert(asset.id, gde_ref);
                        if asset == LOOT_BAG_ASSET {
                            self.bags.push(gde_ref);
                        }
                    }
                    let Some(entity) = update.entity else {
                        continue;
                    };
                    match vitals_invulnerability(update) {
                        Some(0) => {
                            self.invulnerable.remove(&entity);
                        }
                        Some(_) => {
                            self.invulnerable.insert(entity);
                        }
                        None => {}
                    }
                    if is_mob_card(update) {
                        // A card again keeps the place: a mob that stands still sends none.
                        self.mobs.entry(entity).or_insert(None);
                        self.mob_cards += 1;
                        if let Some(vitals) = creature_vitals_id(update) {
                            self.mob_vitals.insert(entity, vitals);
                        }
                    }
                    if died(update) {
                        if let Some(place) = self.mobs.remove(&entity) {
                            self.mob_deaths += 1;
                            if let Some(at) = place.or(update.position) {
                                self.corpses.push(Corpse {
                                    entity,
                                    at,
                                    vitals: self
                                        .mob_vitals
                                        .get(&entity)
                                        .copied()
                                        .or_else(|| creature_vitals_id(update)),
                                });
                            }
                        }
                    } else if let Some(mob) = self.mobs.get_mut(&entity)
                        && update.position.is_some()
                    {
                        *mob = update.position;
                    }
                }
            }
            Read::Combat { event, .. } => {
                let (counter, number) = match event {
                    CombatEvent::Dealt { attacker } => (&mut self.hits_dealt, attacker),
                    CombatEvent::Taken { target } => (&mut self.hits_taken, target),
                    CombatEvent::Died { player } => (&mut self.deaths, player),
                };
                if number.is_some_and(|number| self.own.is_own(number)) {
                    *counter += 1;
                }
            }
            Read::Interaction { answer, .. } => self.interactions.push(answer),
            Read::Entry { .. } => {
                // A new level: the client unloads the old level's entities and their slots.
                for gone in std::mem::take(&mut self.mobs).into_keys() {
                    release_raw_id(self.own.holder, gone);
                }
                self.slots = EntitySlots::default();
                self.cards.clear();
                self.mob_vitals.clear();
                self.invulnerable.clear();
                self.corpses.clear();
            }
            Read::Skipped | Read::Other { .. } => {}
        }
        Ok(own_position)
    }

    /// The bot's own fishing steps seen, oldest first.
    pub fn fishing_steps(&self) -> &[StateTransition] {
        &self.fishing
    }

    /// The ids of the loot bags seen, oldest first.
    pub fn bags(&self) -> &[[u8; RAW_ID_LEN]] {
        &self.bags
    }

    /// The id of the newest card seen of one of the slice assets `assets`.
    pub fn card_of(&self, assets: &[[u8; ASSET_ID_LEN]]) -> Option<[u8; RAW_ID_LEN]> {
        assets
            .iter()
            .find_map(|asset| self.cards.get(asset).copied())
    }

    /// The answers to the bot's interactions since the last call, oldest first.
    pub fn take_interactions(&mut self) -> Vec<InteractionAnswer> {
        std::mem::take(&mut self.interactions)
    }

    /// The mobs with a position, by entity number.
    pub fn mobs(&self) -> impl Iterator<Item = (u64, WorldPosition)> + '_ {
        self.mobs
            .iter()
            .filter_map(|(number, position)| Some((*number, (*position)?)))
    }

    /// The vitals id (CRC-32 of the lowercase id) that the card of mob `entity` named.
    pub fn vitals_of(&self, entity: u64) -> Option<u32> {
        self.mob_vitals.get(&entity).copied()
    }

    /// Whether the newest vitals of mob `entity` say it takes no hits now.
    pub fn takes_no_hits(&self, entity: u64) -> bool {
        self.invulnerable.contains(&entity)
    }

    /// The bundles read so far.
    pub const fn bundles(&self) -> u64 {
        self.bundles
    }

    /// The mob cards read so far.
    pub const fn mob_cards(&self) -> u64 {
        self.mob_cards
    }

    /// The own number when the first time sync request is due ([`OwnView::time_sync_due`]).
    pub fn time_sync_due(&mut self) -> Option<u64> {
        self.own.time_sync_due()
    }

    /// 2071 messages for hits this bot made.
    pub const fn hits_dealt(&self) -> u64 {
        self.hits_dealt
    }

    /// 3601 messages for hits this bot took.
    pub const fn hits_taken(&self) -> u64 {
        self.hits_taken
    }

    /// 4299 messages for this bot's deaths.
    pub const fn deaths(&self) -> u64 {
        self.deaths
    }

    /// Whether the bot's own newest vitals say it is dead.
    pub const fn own_dead(&self) -> bool {
        self.own_dead
    }

    /// The bot's own achievements.
    pub const fn achievements(&self) -> &OwnAchievements {
        &self.achievements
    }

    /// The missions of the board the bot has open.
    pub fn board(&self) -> &[MissionParams] {
        &self.board
    }

    /// The bot's bag: one record per bag slot, empty slots included (quantity 0).
    pub fn bag(&self) -> &[ItemRecord] {
        &self.bag
    }

    /// The bag slot and instance of the first stack of the item of client item index
    /// `item_index` in the bot's bag.
    pub fn bag_slot_of(&self, item_index: u32) -> Option<(u32, [u8; RAW_ID_LEN])> {
        self.bag.iter().enumerate().find_map(|(slot, item)| {
            (item.quantity > 0 && item.item_index == item_index)
                .then(|| Some((u32::try_from(slot).ok()?, item.instance)))
                .flatten()
        })
    }

    /// The mobs in view that died.
    pub const fn mob_deaths(&self) -> u64 {
        self.mob_deaths
    }

    /// The mobs in view of vitals id `vitals` (CRC-32 of the lowercase id) that died, newest
    /// first, each with its last place, while their slots have not stopped. Live 2026-11-22
    /// 10:46:13: a scorpion died after the bot's lion (10:45:45), and the skin of the newest
    /// corpse gave no lion heart.
    pub fn corpses_of(&self, vitals: u32) -> impl Iterator<Item = (u64, WorldPosition)> + '_ {
        self.corpses
            .iter()
            .rev()
            .filter(move |corpse| corpse.vitals == Some(vitals))
            .map(|corpse| (corpse.entity, corpse.at))
    }
}

/// A mob in view that died.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Corpse {
    entity: u64,
    /// Its last place.
    at: WorldPosition,
    /// Its vitals id, when its card named one.
    vitals: Option<u32>,
}

/// The vitals id of the creature vitals of `update` (not the player's).
fn creature_vitals_id(update: &SlotUpdate) -> Option<u32> {
    update
        .members
        .iter()
        .find_map(|member| match &member.state {
            MemberState::Vitals(vitals) => vitals
                .slot_0
                .as_ref()
                .and_then(|slot| slot.vitals_id)
                .filter(|id| *id != PLAYER_VITALS_ID),
            _ => None,
        })
}

/// The `invulnerability` the vitals of `update` send; `None` when they send none.
fn vitals_invulnerability(update: &SlotUpdate) -> Option<u8> {
    update
        .members
        .iter()
        .find_map(|member| match &member.state {
            MemberState::Vitals(vitals) => vitals.slot_0.as_ref()?.invulnerability,
            _ => None,
        })
}

/// Whether `update` is the card of a mob: one with creature vitals (a mob of the prefab table: its
/// card group carries its whole vitals), or one whose asset is its own id (a mob without a
/// prefab: the sim names its card's asset by the mob's raw id, so the client finds no slice).
/// The ids are random per spawn, as retail's, so the number of an entity tells nothing.
fn is_mob_card(update: &SlotUpdate) -> bool {
    let Some(card) = update.card else {
        return false;
    };
    let creature_vitals = creature_vitals_id(update).is_some();
    let self_named = card
        .asset
        .zip(card.gde_ref)
        .is_some_and(|(asset, raw_id)| asset.id == raw_id);
    creature_vitals || self_named
}

/// Takes the bag list `items` into `bag`: a full list is the whole bag, a change sets one slot
/// (a slot past the end appends, with empty slots before it).
fn apply_bag(bag: &mut Vec<ItemRecord>, items: &ReplicatedList<ItemRecord>) {
    apply_list(bag, items);
}

/// Takes the replicated list `changes` into `list`: a full list is the whole list, a change
/// sets one element (an index past the end appends, with default elements before it).
fn apply_list<T: Clone + Default>(list: &mut Vec<T>, changes: &ReplicatedList<T>) {
    match changes {
        ReplicatedList::Full(full) => list.clone_from(&full.elements),
        ReplicatedList::Changes(changes) => {
            for change in changes {
                let Ok(index) = usize::try_from(change.index) else {
                    continue;
                };
                if index >= list.len() {
                    list.resize(index + 1, T::default());
                }
                list[index] = change.value.clone();
            }
        }
    }
}

/// Whether the vitals of `update` say its entity died: no health left, or the dead block.
fn died(update: &SlotUpdate) -> bool {
    vitals_dead(update) == Some(true)
}

/// What the vitals of `update` say of its entity's life: dead with no health left or the dead
/// block, alive with health left; `None` when its vitals name neither (a mana-only vitals, or no
/// vitals).
fn vitals_dead(update: &SlotUpdate) -> Option<bool> {
    let says = update
        .members
        .iter()
        .filter_map(|member| match &member.state {
            MemberState::Vitals(vitals) => {
                let slot = vitals.slot_0.as_ref()?;
                let dead_block = slot.vitals_data.as_ref().is_some_and(VitalsData::is_dead);
                match slot.health_amount {
                    Some(health) => Some(health <= 0.0 || dead_block),
                    None => dead_block.then_some(true),
                }
            }
            _ => None,
        });
    says.reduce(|dead, also| dead || also)
}

/// The world as one client saw it.
#[derive(Debug, Clone, Default)]
pub struct WorldView {
    /// Type indexes of the world messages that are not bundles, in arrival order.
    login: Vec<u64>,
    /// How many of them came before the first bundle.
    login_before_first_bundle: Option<usize>,
    /// The own number and the first time sync. Its hold on the ids it numbered: a world view
    /// keeps every track of its run, so it holds them for its life (it reads one short run).
    own: OwnView,
    slots: EntitySlots,
    tracks: BTreeMap<u64, Vec<WorldPosition>>,
    /// The listed tasks of the own player's objectives, from the last full task list of its
    /// objectives component: each one's objective instance and task (the CRC-32 of its lowercase
    /// id). A mission's turn-in names the instance whose tasks the bot played.
    listed_tasks: Vec<(u64, u32)>,
    /// The own group, from the own groups state (`None`: none, or the all-zero id).
    group: Option<[u8; RAW_ID_LEN]>,
    /// The invites to the own player's group, from the last full list of the own groups state.
    inbound_invites: Vec<[u8; RAW_ID_LEN]>,
    /// The own queue and game modes, from the own game mode participant state.
    matchmaking: Matchmaking,
}

/// The own queue and game modes, from each part of the own game mode participant state (3312)
/// that the server sends.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matchmaking {
    /// `matchmakingServiceStatus` (10 at a pop, 1 while the match waits, 5 at the move).
    pub status: Option<u32>,
    /// The match id of the newest pop; `None` with no match (the empty id).
    pub match_id: Option<String>,
    /// The game modes the player is in: the CRC-32 of each lowercase `GameModeId`.
    pub active: Vec<u32>,
    /// `lastTeamIndex`: the team of the player's match.
    pub team: Option<u8>,
}

impl Matchmaking {
    /// Keeps each part of `state` that the server sent.
    fn read(&mut self, state: &GameModeParticipantState) {
        if let Some(status) = state.service.status {
            self.status = Some(status);
        }
        if let Some(match_id) = &state.service.match_id {
            self.match_id = (!match_id.is_empty()).then(|| match_id.clone());
        }
        if let Some(active) = &state.active {
            self.active = active.elements.iter().map(|mode| mode.mode).collect();
        }
        if let Some(team) = state.last_team {
            self.team = Some(team);
        }
    }
}

/// Two views are equal when they saw the same; their id holders differ.
impl PartialEq for WorldView {
    fn eq(&self, other: &Self) -> bool {
        self.login == other.login
            && self.login_before_first_bundle == other.login_before_first_bundle
            && self.own.own_number() == other.own.own_number()
            && self.slots == other.slots
            && self.tracks == other.tracks
    }
}

impl WorldView {
    /// Reads `messages` in order. Messages of other types are skipped.
    ///
    /// # Errors
    ///
    /// Returns [`ViewError`] for a world message or a bundle payload that does not decode.
    pub fn from_messages(messages: &[Received]) -> Result<Self, ViewError> {
        let mut view = Self::default();
        for received in messages {
            view.read(received)?;
        }
        Ok(view)
    }

    /// Reads one more message. Messages of other types are skipped.
    ///
    /// # Errors
    ///
    /// Returns [`ViewError`] for a world message or a bundle payload that does not decode.
    pub fn read(&mut self, received: &Received) -> Result<(), ViewError> {
        let mut read = decode(received, self.own.holder, Cards::Numbered)?;
        self.own.apply(&read);
        let own = self.own.own_number();
        match &mut read {
            Read::Skipped => {}
            Read::Bundle(bundle) => {
                self.login_before_first_bundle
                    .get_or_insert(self.login.len());
                self.slots.resolve(bundle);
                for update in &bundle.updates {
                    let entity = update
                        .entity
                        .or_else(|| update.is_local_player().then_some(own).flatten());
                    if let (Some(entity), Some(position)) = (entity, update.position) {
                        self.tracks.entry(entity).or_default().push(position);
                    }
                    if entity.is_some() && entity == own {
                        self.read_own_tasks(update);
                        self.read_own_groups(update);
                        self.read_own_matchmaking(update);
                    }
                }
            }
            Read::Entry { type_index, .. }
            | Read::Combat { type_index, .. }
            | Read::Interaction { type_index, .. }
            | Read::Other { type_index } => {
                self.login.push(*type_index);
            }
        }
        Ok(())
    }

    /// Whether the login messages came first, in the captured order, and bundles after them.
    pub fn in_captured_order(&self) -> bool {
        self.login.starts_with(LOGIN_MESSAGE_TYPES)
            && self
                .login_before_first_bundle
                .is_some_and(|count| count >= LOGIN_MESSAGE_TYPES.len())
    }

    /// The client's own entity number.
    pub const fn own_number(&self) -> Option<u64> {
        self.own.own_number()
    }

    /// Keeps the listed tasks of the own player's objectives component in `update`.
    fn read_own_tasks(&mut self, update: &SlotUpdate) {
        let tasks = update
            .members
            .iter()
            .find_map(|member| match &member.state {
                MemberState::Objectives(state) => state.tasks.as_ref(),
                _ => None,
            });
        if let Some(tasks) = tasks {
            self.listed_tasks = tasks
                .elements
                .iter()
                .map(|entry| (entry.instance, entry.task))
                .collect();
        }
    }

    /// Keeps the own group and the invites to it from the own groups state in `update`: each
    /// part that the state sends.
    fn read_own_groups(&mut self, update: &SlotUpdate) {
        let groups = update
            .members
            .iter()
            .filter_map(|member| match &member.state {
                MemberState::Groups(state) => Some(state),
                _ => None,
            });
        for GroupsState {
            group_id,
            inbound_invites,
            ..
        } in groups
        {
            if let Some(group) = group_id {
                self.group = (*group != [0; RAW_ID_LEN]).then_some(*group);
            }
            if let Some(invites) = inbound_invites {
                self.inbound_invites = invites.elements.iter().map(|data| data.invite).collect();
            }
        }
    }

    /// Keeps the own queue and game modes from the own game mode participant state in
    /// `update`.
    fn read_own_matchmaking(&mut self, update: &SlotUpdate) {
        for member in &update.members {
            if let MemberState::GameModeParticipant(state) = &member.state {
                self.matchmaking.read(state);
            }
        }
    }

    /// The own queue and game modes.
    pub const fn matchmaking(&self) -> &Matchmaking {
        &self.matchmaking
    }

    /// The own group, once the own groups state names one.
    pub const fn own_group(&self) -> Option<[u8; RAW_ID_LEN]> {
        self.group
    }

    /// The first invite to the own player's group.
    pub fn inbound_group_invite(&self) -> Option<[u8; RAW_ID_LEN]> {
        self.inbound_invites.first().copied()
    }

    /// The instances of the own player's objectives that list one of the tasks `tasks` (the
    /// CRC-32 of each lowercase id), in list order, each once. A mission's record names only its
    /// template objective (live 2026-11-21: six missions of one template, `m_missionId` 0), and
    /// a task is listed once it has progress, so the tasks a bot played tell its mission.
    pub fn instances_listing(&self, tasks: &[u32]) -> Vec<u64> {
        let mut instances = Vec::new();
        for (instance, task) in &self.listed_tasks {
            if tasks.contains(task) && !instances.contains(instance) {
                instances.push(*instance);
            }
        }
        instances
    }

    /// The own number when the first time sync request is due ([`OwnView::time_sync_due`]).
    pub fn time_sync_due(&mut self) -> Option<u64> {
        self.own.time_sync_due()
    }

    /// The last position of entity `number`.
    pub fn position(&self, number: u64) -> Option<WorldPosition> {
        self.track(number).last().copied()
    }

    /// Every position of entity `number`, in order.
    pub fn track(&self, number: u64) -> &[WorldPosition] {
        self.tracks.get(&number).map_or(&[], Vec::as_slice)
    }

    /// The client's own last position.
    pub fn own_position(&self) -> Option<WorldPosition> {
        self.position(self.own_number()?)
    }

    /// Every other entity number the client saw, in ascending order.
    pub fn others(&self) -> Vec<u64> {
        self.tracks
            .keys()
            .copied()
            .filter(|number| Some(*number) != self.own_number())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::test_support::{
        FIRST_OTHER_SLOT, entity_group, login_samples, melee_sample, players_bundle,
    };
    use nw_messages::{
        AssetId, BodyCodec, BundleGroup, BundleMember, ClientAddEntryMsg, DYNAMIC_SLICE_SUB_ID,
        DamageDealtMsg, DeathRecapMsg, FullList, GDE_ID_LEN, LOCAL_PLAYER_INTEREST,
        LOGIN_CLIENT_CONTEXT, ObjectivesState, ReplicatedStateBundle, TaskEntry, TaskWireState,
        VitalsState,
    };
    use nw_reflect::{Envelope, TypeId};

    use super::*;

    const OWN: u64 = 1;
    const OTHER: u64 = 2;
    const LOGIN_CHANNEL: u8 = 0;
    const BUNDLE_CHANNEL: u8 = 1;
    /// `PingMsg`: not a world message.
    const OTHER_TYPE_INDEX: u64 = 349;
    const SHORT_BODY: [u8; 3] = [1, 2, 3];

    fn received(channel: u8, message: &ServerMessage) -> Received {
        let body = message.encode().expect("encode");
        Received {
            channel,
            envelope: Envelope::message(TypeId::FileSlot(message.type_index()), body),
        }
    }

    fn login() -> Vec<Received> {
        login_samples(OWN)
            .iter()
            .map(|message| received(LOGIN_CHANNEL, message))
            .collect()
    }

    fn at(x: f32) -> WorldPosition {
        WorldPosition::from_metres(x, x, x).expect("finite")
    }

    fn bundle(sequence: u64, positions: &[(u64, WorldPosition)]) -> Received {
        let bundle = players_bundle(sequence, OWN, positions);
        received(BUNDLE_CHANNEL, &ServerMessage::Bundle(bundle))
    }

    #[test]
    fn the_login_order_the_own_number_and_the_last_positions_are_learned() {
        let mut messages = login();
        messages.push(bundle(0, &[(OWN, at(1.0)), (OTHER, at(2.0))]));
        messages.push(Received {
            channel: LOGIN_CHANNEL,
            envelope: Envelope::message(TypeId::FileSlot(OTHER_TYPE_INDEX), SHORT_BODY.to_vec()),
        });
        messages.push(bundle(1, &[(OWN, at(1.0)), (OTHER, at(3.0))]));
        let view = WorldView::from_messages(&messages).expect("view");
        assert!(view.in_captured_order());
        assert_eq!(view.own_number(), Some(OWN));
        assert_eq!(view.own_position(), Some(at(1.0)));
        assert_eq!(view.position(OTHER), Some(at(3.0)));
        assert_eq!(view.track(OTHER), [at(2.0), at(3.0)]);
        assert!(view.track(OTHER + 1).is_empty());
        assert_eq!(view.others(), vec![OTHER]);
    }

    #[test]
    fn the_light_view_keeps_only_the_own_position() {
        let mut view = OwnView::default();
        let before = bundle(0, &[(OWN, at(1.0))]);
        assert_eq!(
            view.read(&before).expect("read"),
            Some(at(1.0)),
            "slot 1 is the own player before the own number is known"
        );
        for message in login() {
            assert_eq!(view.read(&message).expect("read"), None);
        }
        let seen = bundle(1, &[(OTHER, at(2.0)), (OWN, at(3.0))]);
        assert_eq!(view.read(&seen).expect("read"), Some(at(3.0)));
        let others_only = bundle(2, &[(OTHER, at(4.0))]);
        assert_eq!(view.read(&others_only).expect("read"), None);
    }

    #[test]
    fn the_first_time_sync_is_due_once_after_the_own_card() {
        let mut view = OwnView::default();
        for message in login() {
            view.read(&message).expect("read");
        }
        assert_eq!(
            view.time_sync_due(),
            None,
            "the entry alone is not the own card"
        );
        view.read(&bundle(0, &[(OTHER, at(2.0))])).expect("read");
        assert_eq!(view.time_sync_due(), None, "a bundle of others only");
        view.read(&bundle(1, &[(OWN, at(1.0))])).expect("read");
        assert_eq!(view.time_sync_due(), Some(OWN));
        view.read(&bundle(2, &[(OWN, at(1.5))])).expect("read");
        assert_eq!(view.time_sync_due(), None, "only the first time sync");
    }

    #[test]
    fn a_combat_view_gives_the_first_time_sync_after_the_own_card() {
        let mut view = CombatView::default();
        for message in login() {
            view.read(&message).expect("read");
        }
        view.read(&bundle(0, &[(OWN, at(1.0))])).expect("read");
        assert_eq!(view.time_sync_due(), Some(OWN));
        assert_eq!(view.time_sync_due(), None);
    }

    #[test]
    fn a_level_change_makes_the_time_sync_due_again_after_the_new_own_card() {
        let mut view = CombatView::default();
        for message in login() {
            view.read(&message).expect("read");
        }
        view.read(&bundle(0, &[(OWN, at(1.0))])).expect("read");
        assert_eq!(view.time_sync_due(), Some(OWN));
        // The end of the tutorial sends the entry again, then the new level's bundles.
        for message in login() {
            view.read(&message).expect("read");
        }
        assert_eq!(view.time_sync_due(), None, "not before the new own card");
        view.read(&bundle(0, &[(OWN, at(2.0))])).expect("read");
        assert_eq!(
            view.time_sync_due(),
            Some(OWN),
            "the new level waits for it"
        );
        assert_eq!(view.time_sync_due(), None);
    }

    #[test]
    fn messages_can_be_read_one_by_one() {
        let mut messages = login();
        messages.push(bundle(0, &[(OWN, at(1.0))]));
        let mut view = WorldView::default();
        for received in &messages {
            view.read(received).expect("read");
        }
        assert_eq!(view, WorldView::from_messages(&messages).expect("view"));
    }

    #[test]
    fn a_bundle_before_the_login_or_a_changed_order_is_out_of_order() {
        let mut early = vec![bundle(0, &[(OWN, at(1.0))])];
        early.extend(login());
        assert!(
            !WorldView::from_messages(&early)
                .expect("view")
                .in_captured_order()
        );
        let mut swapped = login();
        swapped.swap(0, 1);
        swapped.push(bundle(0, &[(OWN, at(1.0))]));
        assert!(
            !WorldView::from_messages(&swapped)
                .expect("view")
                .in_captured_order()
        );
        assert!(
            !WorldView::from_messages(&login())
                .expect("view")
                .in_captured_order()
        );
    }

    const MOB: u64 = (1 << 34) + 5;
    /// The mob of the slot stop test: the stop releases its raw id from the process registry,
    /// which the other tests, running at the same time, must not lose for [`MOB`].
    const STOPPED_MOB: u64 = (1 << 34) + 6;
    /// The mobs of the level change and corpse tests, which release their raw ids too.
    const LEVEL_MOB: u64 = (1 << 34) + 7;
    const CORPSE_MOB: u64 = (1 << 34) + 8;
    const HIT: f32 = 42.0;
    /// Bundles in the still-mob test: far more than a tick or two.
    const STILL_BUNDLES: u64 = 30;
    /// The vitals component of a mob group (sim `MobCard`).
    const MOB_VITALS_COMPONENT: u64 = 10;
    const MOB_HEALTH: u32 = 370;

    const MOB_SLOT: u16 = FIRST_OTHER_SLOT;
    const OTHER_SLOT: u16 = FIRST_OTHER_SLOT + 1;
    const ROCK_SLOT: u16 = FIRST_OTHER_SLOT + 2;
    /// A static's entity number (the sim's statics start at 2^40).
    const ROCK: u64 = 1 << 40;
    /// A level object's asset: no mob's.
    const STATIC_ASSET: AssetId = AssetId {
        id: [0; GDE_ID_LEN],
        sub_id: DYNAMIC_SLICE_SUB_ID,
    };
    /// The prefab of a mob of the prefab table.
    const CREATURE_ASSET: AssetId = AssetId {
        id: [1; GDE_ID_LEN],
        sub_id: DYNAMIC_SLICE_SUB_ID,
    };
    const CREATURE_VITALS_ID: u32 = 7;
    /// The vitals id the death states of the corpse tests name.
    const DEAD_VITALS_ID: u32 = 1;
    /// The mobs of the skin-the-planned-kill test.
    const LION_MOB: u64 = (1 << 34) + 9;
    const SCORPION_MOB: u64 = (1 << 34) + 10;
    const LION_VITALS_ID: u32 = 11;
    const SCORPION_VITALS_ID: u32 = 12;

    /// The card asset of a mob without a prefab: its own raw id (sim `EntityKind::mob`).
    fn self_named_asset(mob: u64) -> AssetId {
        AssetId {
            id: nw_messages::raw_id_of_entity(mob),
            sub_id: DYNAMIC_SLICE_SUB_ID,
        }
    }

    /// A bundle that stops `stops`, with the own position and then `groups`.
    fn mob_bundle(sequence: u64, stops: Vec<u16>, groups: Vec<BundleGroup>) -> Received {
        let own = entity_group(LOCAL_PLAYER_INTEREST, OWN, None, Some(at(1.0)));
        let groups = std::iter::once(own).chain(groups).collect();
        let bundle =
            ReplicatedStateBundle::of_groups(sequence, LOGIN_CLIENT_CONTEXT, stops, groups)
                .expect("bundle");
        received(BUNDLE_CHANNEL, &ServerMessage::Bundle(bundle))
    }

    fn mob_at(mob: u64, x: f32, carded: bool) -> BundleGroup {
        entity_group(
            MOB_SLOT,
            mob,
            carded.then(|| self_named_asset(mob)),
            Some(at(x)),
        )
    }

    fn combat_view() -> CombatView {
        let mut view = CombatView::default();
        for message in login() {
            view.read(&message).expect("read");
        }
        view
    }

    #[test]
    fn a_level_change_forgets_the_old_level_s_mobs() {
        let mut view = combat_view();
        view.read(&mob_bundle(
            0,
            Vec::new(),
            vec![mob_at(LEVEL_MOB, 2.0, true)],
        ))
        .expect("read");
        assert_eq!(view.mobs().count(), 1);
        for message in login() {
            view.read(&message).expect("read");
        }
        assert_eq!(
            view.mobs().count(),
            0,
            "the new level's mobs come with new cards"
        );
        view.read(&mob_bundle(
            0,
            Vec::new(),
            vec![mob_at(LEVEL_MOB, 5.0, true)],
        ))
        .expect("read");
        assert_eq!(view.mobs().collect::<Vec<_>>(), vec![(LEVEL_MOB, at(5.0))]);
    }

    #[test]
    fn the_view_keeps_a_mob_until_its_slot_stops() {
        let mut view = combat_view();
        let other = entity_group(
            OTHER_SLOT,
            OTHER,
            Some(nw_messages::PLAYER_SLICE_ASSET),
            Some(at(3.0)),
        );
        let rock = entity_group(ROCK_SLOT, ROCK, Some(STATIC_ASSET), Some(at(1.5)));
        let first = mob_bundle(0, Vec::new(), vec![mob_at(MOB, 2.0, true), other, rock]);
        assert_eq!(view.read(&first).expect("read"), Some(at(1.0)));
        assert_eq!(
            view.mobs().collect::<Vec<_>>(),
            vec![(MOB, at(2.0))],
            "a player and a static are no mobs"
        );
        view.read(&mob_bundle(1, Vec::new(), vec![mob_at(MOB, 4.0, false)]))
            .expect("read");
        assert_eq!(view.mobs().collect::<Vec<_>>(), vec![(MOB, at(4.0))]);
        for sequence in 2..STILL_BUNDLES {
            view.read(&mob_bundle(sequence, Vec::new(), Vec::new()))
                .expect("read");
        }
        assert_eq!(
            view.mobs().collect::<Vec<_>>(),
            vec![(MOB, at(4.0))],
            "a mob that stands still is not sent again, and stays"
        );
        assert_eq!(view.bundles(), STILL_BUNDLES);
        let card_again = entity_group(MOB_SLOT, MOB, Some(self_named_asset(MOB)), None);
        view.read(&mob_bundle(STILL_BUNDLES, Vec::new(), vec![card_again]))
            .expect("read");
        assert_eq!(
            view.mobs().collect::<Vec<_>>(),
            vec![(MOB, at(4.0))],
            "its card again keeps its place"
        );
        assert_eq!(view.mob_cards(), 2);
    }

    #[test]
    fn the_view_lists_the_loot_bags_of_their_cards_and_no_other_card() {
        const BAG: u64 = 1 << 41;
        let mut view = combat_view();
        let bag = entity_group(OTHER_SLOT, BAG, Some(LOOT_BAG_ASSET), Some(at(2.5)));
        let rock = entity_group(ROCK_SLOT, ROCK, Some(STATIC_ASSET), Some(at(1.5)));
        view.read(&mob_bundle(0, Vec::new(), vec![bag, rock]))
            .expect("read");
        assert_eq!(view.bags(), [nw_messages::raw_id_of_entity(BAG)]);
        assert_eq!(view.mobs().count(), 0, "a bag is no mob");
    }

    #[test]
    fn a_creature_card_with_its_vitals_is_a_mob() {
        let mut view = combat_view();
        let mut creature = entity_group(MOB_SLOT, MOB, Some(CREATURE_ASSET), Some(at(2.0)));
        creature.members.push(BundleMember {
            component: MOB_VITALS_COMPONENT,
            state: MemberState::Vitals(VitalsState::creature(
                MOB_HEALTH,
                MOB_HEALTH,
                CREATURE_VITALS_ID,
                1,
                1,
            )),
        });
        view.read(&mob_bundle(0, Vec::new(), vec![creature]))
            .expect("read");
        assert_eq!(view.mobs().collect::<Vec<_>>(), vec![(MOB, at(2.0))]);
    }

    #[test]
    fn the_own_vitals_say_whether_the_bot_is_dead_and_a_bundle_without_them_keeps_it() {
        const MAX_HEALTH: u32 = 1_000;
        let own_vitals = |sequence: u64, health: u32| {
            let mut own = entity_group(LOCAL_PLAYER_INTEREST, OWN, None, Some(at(1.0)));
            own.members.push(BundleMember {
                component: nw_messages::PLAYER_VITALS_COMPONENT,
                state: MemberState::Vitals(VitalsState::player(health, MAX_HEALTH)),
            });
            let bundle = ReplicatedStateBundle::of_groups(
                sequence,
                LOGIN_CLIENT_CONTEXT,
                Vec::new(),
                vec![own],
            )
            .expect("bundle");
            received(BUNDLE_CHANNEL, &ServerMessage::Bundle(bundle))
        };
        let mut view = combat_view();
        assert!(!view.own_dead(), "no vitals yet");
        view.read(&own_vitals(0, 0)).expect("read");
        assert!(view.own_dead(), "logged in at no health");
        view.read(&mob_bundle(1, Vec::new(), Vec::new()))
            .expect("read");
        assert!(view.own_dead(), "a bundle without own vitals keeps it");
        view.read(&own_vitals(2, MAX_HEALTH)).expect("read");
        assert!(!view.own_dead(), "respawned at full health");
        assert_eq!(view.deaths(), 0, "no 4299");
    }

    #[test]
    fn the_own_bag_names_the_slot_and_instance_of_an_item_until_it_leaves() {
        const BOX_INDEX: u32 = 9_401;
        const BOX_SLOT: u32 = 2;
        const BOX_INSTANCE: [u8; RAW_ID_LEN] = [7; RAW_ID_LEN];
        let own_bag = |sequence: u64, items: ReplicatedList<ItemRecord>| {
            let mut own = entity_group(LOCAL_PLAYER_INTEREST, OWN, None, Some(at(1.0)));
            own.members.push(BundleMember {
                component: BAG_COMPONENT,
                state: MemberState::Container(nw_messages::ContainerState {
                    items: Some(items),
                    ..nw_messages::ContainerState::default()
                }),
            });
            let bundle = ReplicatedStateBundle::of_groups(
                sequence,
                LOGIN_CLIENT_CONTEXT,
                Vec::new(),
                vec![own],
            )
            .expect("bundle");
            received(BUNDLE_CHANNEL, &ServerMessage::Bundle(bundle))
        };
        let reward_box = ItemRecord {
            quantity: 1,
            item_index: BOX_INDEX,
            instance: BOX_INSTANCE,
            ..ItemRecord::default()
        };
        let mut view = combat_view();
        view.read(&own_bag(
            0,
            ReplicatedList::Full(nw_messages::FullList::first(vec![
                ItemRecord::default(),
                ItemRecord::default(),
            ])),
        ))
        .expect("read");
        assert_eq!(view.bag_slot_of(BOX_INDEX), None, "an empty bag");
        let change = |value| {
            ReplicatedList::Changes(vec![nw_messages::ListChange {
                index: BOX_SLOT,
                version: 0,
                value,
            }])
        };
        view.read(&own_bag(1, change(reward_box))).expect("read");
        assert_eq!(
            view.bag_slot_of(BOX_INDEX),
            Some((BOX_SLOT, BOX_INSTANCE)),
            "a slot past the end appends"
        );
        view.read(&own_bag(2, change(ItemRecord::default())))
            .expect("read");
        assert_eq!(
            view.bag_slot_of(BOX_INDEX),
            None,
            "opened: the slot is empty"
        );
    }

    #[test]
    fn a_mob_that_died_is_dropped_and_comes_back_with_a_new_card() {
        let mut view = combat_view();
        view.read(&mob_bundle(0, Vec::new(), vec![mob_at(MOB, 2.0, true)]))
            .expect("read");
        let mut dead = entity_group(MOB_SLOT, MOB, None, None);
        dead.members.push(BundleMember {
            component: MOB_VITALS_COMPONENT,
            state: MemberState::Vitals(VitalsState::creature(0, MOB_HEALTH, DEAD_VITALS_ID, 1, 1)),
        });
        view.read(&mob_bundle(1, Vec::new(), vec![dead]))
            .expect("read");
        assert_eq!(view.mobs().count(), 0, "dead");
        assert_eq!(view.mob_deaths(), 1);
        assert_eq!(
            view.corpses_of(DEAD_VITALS_ID).next(),
            Some((MOB, at(2.0))),
            "its corpse to skin"
        );
        view.read(&mob_bundle(2, Vec::new(), vec![mob_at(MOB, 3.0, false)]))
            .expect("read");
        assert_eq!(view.mobs().count(), 0, "no card yet");
        view.read(&mob_bundle(3, Vec::new(), vec![mob_at(MOB, 3.0, true)]))
            .expect("read");
        assert_eq!(view.mobs().collect::<Vec<_>>(), vec![(MOB, at(3.0))]);
    }

    #[test]
    fn a_skin_finds_the_newest_corpse_of_its_kill_not_the_newest_corpse() {
        // Live 2026-11-22: the lion died 10:45:45, a scorpion 10:46:13, and the skin at
        // 10:46:32 went to the scorpion.
        let mut view = combat_view();
        let creature = |slot, mob, vitals, health, x| {
            let mut group = entity_group(slot, mob, Some(CREATURE_ASSET), Some(at(x)));
            group.members.push(BundleMember {
                component: MOB_VITALS_COMPONENT,
                state: MemberState::Vitals(VitalsState::creature(health, MOB_HEALTH, vitals, 1, 1)),
            });
            group
        };
        view.read(&mob_bundle(
            0,
            Vec::new(),
            vec![
                creature(MOB_SLOT, LION_MOB, LION_VITALS_ID, MOB_HEALTH, 2.0),
                creature(
                    OTHER_SLOT,
                    SCORPION_MOB,
                    SCORPION_VITALS_ID,
                    MOB_HEALTH,
                    4.0,
                ),
            ],
        ))
        .expect("read");
        let death = |slot, mob| {
            let mut dead = entity_group(slot, mob, None, None);
            dead.members.push(BundleMember {
                component: MOB_VITALS_COMPONENT,
                state: MemberState::Vitals(VitalsState {
                    slot_0: Some(nw_messages::VitalsSlot0 {
                        health_amount: Some(0.0),
                        ..nw_messages::VitalsSlot0::default()
                    }),
                    slot_1: None,
                }),
            });
            dead
        };
        view.read(&mob_bundle(1, Vec::new(), vec![death(MOB_SLOT, LION_MOB)]))
            .expect("read");
        view.read(&mob_bundle(
            2,
            Vec::new(),
            vec![death(OTHER_SLOT, SCORPION_MOB)],
        ))
        .expect("read");
        assert_eq!(view.mob_deaths(), 2);
        assert_eq!(
            view.corpses_of(LION_VITALS_ID).next(),
            Some((LION_MOB, at(2.0))),
            "the lion's corpse, older than the scorpion's"
        );
        assert_eq!(
            view.corpses_of(SCORPION_VITALS_ID).next(),
            Some((SCORPION_MOB, at(4.0)))
        );
        assert_eq!(view.corpses_of(CREATURE_VITALS_ID).next(), None);
    }

    #[test]
    fn a_stopped_slot_drops_its_mob_at_once() {
        let mut view = combat_view();
        view.read(&mob_bundle(
            0,
            Vec::new(),
            vec![mob_at(STOPPED_MOB, 2.0, true)],
        ))
        .expect("read");
        assert_eq!(view.mobs().count(), 1);
        view.read(&mob_bundle(1, vec![MOB_SLOT], Vec::new()))
            .expect("read");
        assert_eq!(view.mobs().count(), 0);
    }

    #[test]
    fn a_corpse_goes_with_its_stopped_slot() {
        let mut view = combat_view();
        view.read(&mob_bundle(
            0,
            Vec::new(),
            vec![mob_at(CORPSE_MOB, 2.0, true)],
        ))
        .expect("read");
        let mut dead = entity_group(MOB_SLOT, CORPSE_MOB, None, None);
        dead.members.push(BundleMember {
            component: MOB_VITALS_COMPONENT,
            state: MemberState::Vitals(VitalsState::creature(0, MOB_HEALTH, DEAD_VITALS_ID, 1, 1)),
        });
        view.read(&mob_bundle(1, Vec::new(), vec![dead]))
            .expect("read");
        assert!(view.corpses_of(DEAD_VITALS_ID).next().is_some());
        view.read(&mob_bundle(2, vec![MOB_SLOT], Vec::new()))
            .expect("read");
        assert_eq!(
            view.corpses_of(DEAD_VITALS_ID).next(),
            None,
            "the corpse went"
        );
    }

    #[test]
    fn the_view_counts_hits_damage_and_deaths() {
        let mut view = combat_view();
        let messages = [
            ServerMessage::DamageDealt(DamageDealtMsg::hit(OWN, MOB, HIT, false)),
            ServerMessage::DamageDealt(DamageDealtMsg::hit(OTHER, MOB, HIT, false)),
            ServerMessage::OnDamage(melee_sample(OWN, MOB, HIT)),
            ServerMessage::OnDamage(melee_sample(OTHER, MOB, HIT)),
            ServerMessage::DeathRecap(DeathRecapMsg::of(OWN)),
            ServerMessage::DeathRecap(DeathRecapMsg::of(OTHER)),
        ];
        for message in &messages {
            assert_eq!(
                view.read(&received(LOGIN_CHANNEL, message)).expect("read"),
                None
            );
        }
        assert_eq!(
            (view.hits_dealt(), view.hits_taken(), view.deaths()),
            (1, 1, 1)
        );
    }

    #[test]
    fn the_own_tasks_name_the_instances_that_list_them() {
        // Live 2026-11-21: missions of one template share their task ids, and a task is listed
        // once it has progress (a turn-in task with none is not).
        const KILL: u32 = 3_110_136_768;
        const GATHER: u32 = 4_096_978_439;
        const KILL_MISSION: u64 = 10;
        const GATHER_MISSION: u64 = 20;
        const UNKNOWN: u32 = 1;
        let entry = |instance, task| TaskEntry {
            instance,
            index: 0,
            task,
            progress: 1,
            state: TaskWireState::Complete,
        };
        let tasks = vec![
            entry(KILL_MISSION, KILL),
            entry(GATHER_MISSION, GATHER),
            entry(KILL_MISSION, GATHER),
        ];
        let update = SlotUpdate {
            interest: LOCAL_PLAYER_INTEREST,
            entity: Some(OWN),
            card: None,
            position: None,
            members: vec![BundleMember {
                component: 0,
                state: MemberState::Objectives(ObjectivesState {
                    tasks: Some(FullList::first(tasks)),
                    ..ObjectivesState::default()
                }),
            }],
        };
        let mut view = WorldView::default();
        view.read_own_tasks(&update);
        assert_eq!(view.instances_listing(&[KILL]), [KILL_MISSION]);
        assert_eq!(
            view.instances_listing(&[GATHER, KILL]),
            [KILL_MISSION, GATHER_MISSION],
            "in list order, each once"
        );
        assert!(view.instances_listing(&[UNKNOWN]).is_empty());
    }

    #[test]
    fn the_own_groups_state_names_the_invite_and_then_the_group() {
        use nw_messages::social::CharacterRef;
        use nw_messages::social::states::GroupInviteData;
        const INVITE: [u8; RAW_ID_LEN] = [5; RAW_ID_LEN];
        const GROUP: [u8; RAW_ID_LEN] = [6; RAW_ID_LEN];
        let update = |state: GroupsState| SlotUpdate {
            interest: LOCAL_PLAYER_INTEREST,
            entity: Some(OWN),
            card: None,
            position: None,
            members: vec![BundleMember {
                component: 0,
                state: MemberState::Groups(state),
            }],
        };
        let mut view = WorldView::default();
        assert_eq!(
            (view.own_group(), view.inbound_group_invite()),
            (None, None)
        );
        view.read_own_groups(&update(GroupsState {
            group_id: Some([0; RAW_ID_LEN]),
            inbound_invites: Some(FullList::first(vec![GroupInviteData::plain(
                INVITE,
                [0; RAW_ID_LEN],
                CharacterRef(None),
                CharacterRef(None),
                0,
            )])),
            ..GroupsState::default()
        }));
        assert_eq!(view.own_group(), None, "the all-zero group is none");
        assert_eq!(view.inbound_group_invite(), Some(INVITE));
        view.read_own_groups(&update(GroupsState {
            group_id: Some(GROUP),
            inbound_invites: Some(FullList::first(Vec::new())),
            ..GroupsState::default()
        }));
        assert_eq!(view.own_group(), Some(GROUP));
        assert_eq!(view.inbound_group_invite(), None, "the invite was answered");
    }

    #[test]
    fn the_own_participant_state_names_the_pop_then_the_match_then_its_end() {
        use nw_messages::game_mode_participant::{ActiveGameMode, MatchmakingService};
        /// CRC-32 of `arena3v3`.
        const ARENA: u32 = 0x1fad_9d42;
        const MATCH: &str = "00000000-0000-7000-8000-000000000001";
        let update = |state: GameModeParticipantState| SlotUpdate {
            interest: LOCAL_PLAYER_INTEREST,
            entity: Some(OWN),
            card: None,
            position: None,
            members: vec![BundleMember {
                component: 0,
                state: MemberState::GameModeParticipant(state),
            }],
        };
        let mut view = WorldView::default();
        view.read_own_matchmaking(&update(GameModeParticipantState {
            service: MatchmakingService {
                status: Some(10),
                match_id: Some(MATCH.to_owned()),
                ..MatchmakingService::default()
            },
            ..GameModeParticipantState::default()
        }));
        assert_eq!(view.matchmaking().match_id.as_deref(), Some(MATCH));
        view.read_own_matchmaking(&update(GameModeParticipantState {
            service: MatchmakingService {
                status: Some(1),
                ..MatchmakingService::default()
            },
            ..GameModeParticipantState::default()
        }));
        assert_eq!(
            (
                view.matchmaking().status,
                view.matchmaking().match_id.as_deref()
            ),
            (Some(1), Some(MATCH)),
            "a part without the id keeps it"
        );
        view.read_own_matchmaking(&update(GameModeParticipantState {
            active: Some(FullList::first(vec![ActiveGameMode {
                raw_id: [1; RAW_ID_LEN],
                facet: 2,
                second: 3,
                mode: ARENA,
                map: ARENA,
                joined_ns: 4,
                phase: 5,
            }])),
            last_team: Some(1),
            ..GameModeParticipantState::default()
        }));
        assert_eq!(
            (
                view.matchmaking().active.as_slice(),
                view.matchmaking().team
            ),
            ([ARENA].as_slice(), Some(1))
        );
        view.read_own_matchmaking(&update(GameModeParticipantState {
            active: Some(FullList::first(Vec::new())),
            service: MatchmakingService {
                match_id: Some(String::new()),
                ..MatchmakingService::default()
            },
            ..GameModeParticipantState::default()
        }));
        assert!(view.matchmaking().active.is_empty(), "the player left");
        assert_eq!(
            view.matchmaking().match_id,
            None,
            "the empty id is no match"
        );
    }

    #[test]
    fn a_world_message_that_does_not_decode_is_an_error() {
        let broken = Received {
            channel: LOGIN_CHANNEL,
            envelope: Envelope::message(
                TypeId::FileSlot(ClientAddEntryMsg::TYPE_INDEX),
                SHORT_BODY.to_vec(),
            ),
        };
        assert!(matches!(
            WorldView::from_messages(&[broken]),
            Err(ViewError::Message {
                type_index: ClientAddEntryMsg::TYPE_INDEX,
                ..
            })
        ));
    }
}
