// SPDX-License-Identifier: AGPL-3.0-or-later
//! `ftue-entities`: builds the tutorial entity table (`ftue-entities-<build>.json`, read by
//! `nw_content::ftue_entities`) from the capture of a new character's tutorial run (FTUE static
//! entity note, FTUE level entity note).
//!
//! A static entity comes into view as a bundle group whose spawn card names only its `GdeRef`
//! (no asset); the `GdeRef` ends with the tutorial phase id of the session, byte reversed. An
//! asset entity comes into view with a card that names an asset; it is not the player and has no
//! vitals state (a card with a vitals state is a mob, kept only so that bodies and events can
//! name it). The scan keeps, per entity, every group with its card (a spawn), the later groups
//! of its slot (changes) until the slot is stopped (a despawn), and in each body the offsets of
//! the player's `GdeRef`, the player's facet entity and the phase id suffix; in the bodies of
//! asset entities also the offsets of the ids of asset entities and mobs. Each group names the
//! nearest event before it: a client message of the tutorial, a server message about a mob or
//! new achievement bits.
//!
//! A member body has no length, so a group is read whole only when every member has a reader:
//! the `nw-messages` readers of the classes these entities use, or [`crate::extra_members`]. A
//! group is taken only where the payload holds a whole readable group that ends at the payload
//! end or at the start of another group. A spawn group (a group with a static card or an asset
//! card) that also holds a member without a reader is read at a group start with that member
//! left out (`spawn_group`), so the card and the following groups and cards are kept.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use nw_capture::{CLIENT_TO_SERVER, Decoder, SERVER_TO_CLIENT, ledger_records};
use nw_content::ftue_entities::{
    FACET_LEN, GDE_REF_LEN, GDE_REF_PREFIX_LEN, IdForm, RefTarget, facet_of, phase_suffix,
};
use nw_messages::facet::{
    CINEMATIC_ENDED_TYPE_INDEX, CINEMATIC_STARTED_TYPE_INDEX, CUTSCENE_ENDED_TYPE_INDEX,
    DISABLE_COLLISION_TYPE_INDEX, EQUIP_ITEM_TYPE_INDEX, QTE_COMPLETED_TYPE_INDEX,
    REQUEST_CUTSCENE_TYPE_INDEX, RETURN_TO_MAIN_WORLD_TYPE_INDEX, TAKE_ALL_ITEMS_TYPE_INDEX,
    UNSELECTED_AS_TARGET_TYPE_INDEX,
};
use nw_messages::{
    ACHIEVEMENT_COMPONENT, ACHIEVEMENT_TYPE_INDEX, ALC_TYPE_INDEX, AchievementBits,
    AchievementState, AlcState, AssetId, BodyCodec, CLIENT_ADD_ENTRY_MSG_TYPE_INDEX,
    ClientAddEntryMsg, DAMAGE_RECEIVER_TYPE_INDEX, DELAYED_EVENT_TYPE_INDEX, DamageReceiverState,
    DelayedEventState, GATHERABLE_CONTROLLER_TYPE_INDEX, GDE_METADATA_COMPONENT,
    GDE_METADATA_TYPE_INDEX, GatherableControllerState, GdeMetadataReplicatedState,
    LEVEL_INFO_CHANGED_MSG_TYPE_INDEX, LevelInfoChangedMsg, PAPERDOLL_TYPE_INDEX,
    PLAYER_SLICE_ASSET, POSITION_IN_THE_WORLD_TYPE_INDEX, PaperdollState,
    PositionInTheWorldReplicatedState, REPLICATED_STATE_BUNDLE_TYPE_INDEX, Reader,
    ReplicatedStateBundle, TUTORIAL_LEVEL, VITALS_TYPE_INDEX, VitalsState, Writer,
};
use nw_reflect::{TypeId, decode_type_id, encode_type_id};
use serde::Serialize;

use crate::extra_members;

/// Mask of the slot id in a group integer: the client keeps its low 16 bits.
const INTEREST_MASK: u64 = 0xFFFF;
/// Bytes of a spawn card member header and body before the `GdeRef`: component 0, class 10,
/// slot byte, child mask with only the `GdeRef` child.
const STATIC_CARD_HEAD: [u8; 4] = [0x00, 0x0a, 0x01, 0x02];
/// Type index of the player component's `TriggerAreaEntityPassthrough` (the client names a
/// static entity whose trigger area it passed; live-tutorial note).
const TRIGGER_AREA_PASSTHROUGH_TYPE_INDEX: u64 = 560;
/// The client messages of the tutorial that a group can follow.
const CLIENT_EVENT_TYPES: [u64; 9] = [
    CINEMATIC_STARTED_TYPE_INDEX,
    CINEMATIC_ENDED_TYPE_INDEX,
    REQUEST_CUTSCENE_TYPE_INDEX,
    CUTSCENE_ENDED_TYPE_INDEX,
    QTE_COMPLETED_TYPE_INDEX,
    TAKE_ALL_ITEMS_TYPE_INDEX,
    EQUIP_ITEM_TYPE_INDEX,
    RETURN_TO_MAIN_WORLD_TYPE_INDEX,
    TRIGGER_AREA_PASSTHROUGH_TYPE_INDEX,
];
/// Bytes of a facet header: the target id and the entity.
const FACET_HEADER_LEN: usize = 16;
/// Body offset of the mob's facet entity in a 3916: after the header to the player.
const UNTARGET_MOB_AT: usize = FACET_HEADER_LEN;
/// Body offset of the mob's facet entity in a 1793: after the target id.
const DISABLE_COLLISION_MOB_AT: usize = 8;
/// Achievement bits per bitset byte.
const BITS_PER_BYTE: u32 = u8::BITS;
/// Bytes of the shortest member body: its slot byte.
const MIN_BODY_LEN: usize = 1;

/// A failure of the tutorial entity scan.
#[derive(Debug, thiserror::Error)]
pub enum FtueEntitiesError {
    /// A file cannot be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The ledger has no `ClientAddEntry` (164) before its first tutorial bundle.
    #[error("the ledger names no player before the tutorial")]
    NoPlayer,
    /// The ledger never enters the tutorial level.
    #[error("the ledger never enters the tutorial level")]
    NoTutorial,
    /// The table cannot be written as JSON.
    #[error("tutorial entity JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// The id of a table entity or a mob in a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RefEntry {
    /// The body offset.
    pub at: usize,
    /// How the id is written.
    pub form: IdForm,
    /// The entity, by index in [`FtueTable::asset_entities`] or [`FtueTable::mobs`].
    pub target: RefTarget,
}

/// One state of a group, as written to the table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemberEntry {
    /// The component index.
    pub component: u64,
    /// The class type index.
    pub class: u64,
    /// The body bytes, as lowercase hex.
    pub body: String,
    /// Body offsets of the player's `GdeRef`.
    pub player_gde_ref_at: Vec<usize>,
    /// Body offsets of the player's facet entity id.
    pub player_facet_at: Vec<usize>,
    /// Body offsets of the phase id suffix.
    pub phase_suffix_at: Vec<usize>,
    /// The ids of asset entities and mobs in the body (asset entities only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<RefEntry>,
}

/// An event that a group can follow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventEntry {
    /// A client message of [`CLIENT_EVENT_TYPES`].
    Client {
        /// The type index.
        message_type: u64,
        /// The bytes after the facet header, as lowercase hex.
        id: String,
    },
    /// A server 3916 or 1793 about a mob.
    Mob {
        /// The type index.
        message_type: u64,
        /// The mob, by index in [`FtueTable::mobs`].
        mob: usize,
    },
    /// New achievement bits of the player.
    Achievements {
        /// The achievement indexes.
        bits: Vec<u32>,
    },
}

/// The nearest event before a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CauseEntry {
    /// Milliseconds from the first ledger record to the event.
    pub event_ms: u64,
    /// Milliseconds from the event to the group.
    pub delay_ms: u64,
    /// The event.
    pub event: EventEntry,
}

/// One group of an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupEntry {
    /// Milliseconds from the first ledger record.
    pub time_ms: u64,
    /// The slot of the group.
    pub interest: u16,
    /// The members, in wire order.
    pub members: Vec<MemberEntry>,
    /// The nearest event before the group; none before the first event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<CauseEntry>,
}

/// The groups of one entity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Timeline {
    /// The groups with its spawn card.
    pub spawns: Vec<GroupEntry>,
    /// The later groups of its slot.
    pub changes: Vec<GroupEntry>,
    /// The times its slot was stopped.
    pub despawns: Vec<u64>,
}

/// One static entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntityEntry {
    /// The first 8 bytes of its `GdeRef`, as lowercase hex.
    pub gde_ref_prefix: String,
    /// Its groups.
    #[serde(flatten)]
    pub timeline: Timeline,
}

/// One asset entity.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssetEntityEntry {
    /// The asset id of its card, as lowercase hex.
    pub asset: String,
    /// The sub id after the asset id.
    pub asset_sub_id: u32,
    /// Its `GdeRef` in the capture, as lowercase hex.
    pub retail_gde_ref: String,
    /// Its position in metres (east, north, height), from the first position or ALC state of
    /// its spawns.
    pub position: Option<[f32; 3]>,
    /// Its groups.
    #[serde(flatten)]
    pub timeline: Timeline,
}

/// One mob: a card with an asset and a vitals state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MobEntry {
    /// Milliseconds from the first ledger record to its first card.
    pub first_card_ms: u64,
    /// The `vitalsId` of its first vitals state.
    pub vitals_id: u32,
    /// The asset id of its card, as lowercase hex.
    pub asset: String,
    /// Its `GdeRef` in the capture, as lowercase hex.
    pub retail_gde_ref: String,
}

/// The table.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FtueTable {
    /// The level folder.
    pub level: String,
    /// The static entities, by first spawn time.
    pub entities: Vec<EntityEntry>,
    /// The asset entities, by first spawn time.
    pub asset_entities: Vec<AssetEntityEntry>,
    /// The mobs, by first card time.
    pub mobs: Vec<MobEntry>,
}

/// What the scan saw, for the report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanStats {
    /// Tutorial bundles read.
    pub bundles: u64,
    /// Spawn groups kept.
    pub spawns: u64,
    /// Spawn groups equal to the one before in the same slot (sent again), left out.
    pub repeated_spawns: u64,
    /// Change groups kept.
    pub changes: u64,
    /// Change groups equal to the change before, left out.
    pub repeated_changes: u64,
    /// Mob card groups read.
    pub mob_cards: u64,
    /// Static spawn cards outside every group read: groups that cannot be read, even with
    /// their members without a reader left out. Each is `(time, GdeRef prefix)`.
    pub unread_cards: Vec<(u64, [u8; GDE_REF_PREFIX_LEN])>,
    /// Cards with an asset (not the player's) outside every group read. Each is
    /// `(time, asset id)`.
    pub unread_asset_cards: Vec<(u64, [u8; GDE_REF_LEN])>,
    /// Members without a reader left out of the spawn groups taken. Each is `(time, class)`.
    pub skipped_members: Vec<(u64, u64)>,
}

/// The table and the report of one scan.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FtueScan {
    /// The table.
    pub table: FtueTable,
    /// What the scan saw.
    pub stats: ScanStats,
}

/// The player's ids, from the first `ClientAddEntry`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlayerIds {
    gde_ref: [u8; GDE_REF_LEN],
    facet: [u8; FACET_LEN],
}

impl PlayerIds {
    fn of(gde_ref: [u8; GDE_REF_LEN]) -> Self {
        Self {
            gde_ref,
            facet: facet_of(&gde_ref),
        }
    }
}

/// Reads a ledger and returns the tutorial's entities.
///
/// # Errors
///
/// Returns [`FtueEntitiesError::NoTutorial`] when the ledger never enters the tutorial, and
/// [`FtueEntitiesError::NoPlayer`] when no player id comes before it.
pub fn scan_ledger(bytes: &[u8]) -> Result<FtueScan, FtueEntitiesError> {
    let mut scan = Scan::default();
    let mut decoder = Decoder::new();
    let mut first_ms = None;
    for record in ledger_records(bytes) {
        let Ok(record) = record else {
            break;
        };
        let time_ms = record.time_ms - *first_ms.get_or_insert(record.time_ms);
        let output = decoder.record(&record);
        if output.new_connection {
            scan.live
                .retain(|(context, _), _| *context != record.context);
        }
        for delivered in &output.envelopes {
            let body = &delivered.envelope.body;
            let Some(type_index) = delivered.envelope.type_index() else {
                continue;
            };
            match record.direction {
                CLIENT_TO_SERVER => scan.client_message(time_ms, type_index, body),
                SERVER_TO_CLIENT => match type_index {
                    CLIENT_ADD_ENTRY_MSG_TYPE_INDEX => scan.player_entry(body),
                    LEVEL_INFO_CHANGED_MSG_TYPE_INDEX => scan.level(body),
                    REPLICATED_STATE_BUNDLE_TYPE_INDEX => {
                        scan.bundle(record.context, time_ms, body)?;
                    }
                    UNSELECTED_AS_TARGET_TYPE_INDEX => {
                        scan.mob_message(time_ms, type_index, body, UNTARGET_MOB_AT);
                    }
                    DISABLE_COLLISION_TYPE_INDEX => {
                        scan.mob_message(time_ms, type_index, body, DISABLE_COLLISION_MOB_AT);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
    if !scan.entered {
        return Err(FtueEntitiesError::NoTutorial);
    }
    let mut table = FtueTable {
        level: TUTORIAL_LEVEL.folder.to_owned(),
        entities: scan.entities,
        asset_entities: scan.assets,
        mobs: scan.mobs,
    };
    add_refs(&mut table);
    Ok(FtueScan {
        table,
        stats: scan.stats,
    })
}

/// The owner of a slot in view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    /// A static entity, by index.
    Static(usize),
    /// An asset entity, by index.
    Asset(usize),
    /// A mob.
    Mob,
}

/// The scan of one ledger.
#[derive(Debug, Default)]
struct Scan {
    player: Option<PlayerIds>,
    /// The phase id suffix of the tutorial, while the player is in it.
    suffix: Option<[u8; GDE_REF_PREFIX_LEN]>,
    /// Whether the ledger entered the tutorial.
    entered: bool,
    /// Owners in view, by connection context and slot.
    live: HashMap<(u64, u16), Owner>,
    /// Static entity index by `GdeRef` prefix.
    index: HashMap<[u8; GDE_REF_PREFIX_LEN], usize>,
    /// Asset entity index by `GdeRef`.
    asset_index: HashMap<[u8; GDE_REF_LEN], usize>,
    /// Mob index by facet entity.
    mob_facets: HashMap<[u8; FACET_LEN], usize>,
    entities: Vec<EntityEntry>,
    assets: Vec<AssetEntityEntry>,
    mobs: Vec<MobEntry>,
    /// The last event and its time.
    last_event: Option<(u64, EventEntry)>,
    /// The player's achievement bitset bytes, by byte index.
    achievement_bytes: HashMap<u32, u8>,
    stats: ScanStats,
}

impl Scan {
    fn player_entry(&mut self, body: &[u8]) {
        if self.player.is_none()
            && let Ok(entry) = ClientAddEntryMsg::decode(body)
        {
            self.player = Some(PlayerIds::of(entry.entry_id));
        }
    }

    fn level(&mut self, body: &[u8]) {
        let Ok(level) = LevelInfoChangedMsg::decode(body) else {
            return;
        };
        let phase = level.phasing.first().map(|entry| entry.phase_id);
        match phase.filter(|_| level.level == TUTORIAL_LEVEL) {
            Some(phase_id) => {
                self.suffix = Some(phase_suffix(phase_id));
                self.entered = true;
            }
            None => {
                self.suffix = None;
                self.live.clear();
            }
        }
    }

    /// Keeps a tutorial client message of [`CLIENT_EVENT_TYPES`] as the last event.
    fn client_message(&mut self, time_ms: u64, message_type: u64, body: &[u8]) {
        if self.suffix.is_none() || !CLIENT_EVENT_TYPES.contains(&message_type) {
            return;
        }
        let Some(id) = body.get(FACET_HEADER_LEN..) else {
            return;
        };
        let event = EventEntry::Client {
            message_type,
            id: hex::encode(id),
        };
        self.last_event = Some((time_ms, event));
    }

    /// Keeps a server message that names a known mob's facet entity at `mob_at` as the last
    /// event.
    fn mob_message(&mut self, time_ms: u64, message_type: u64, body: &[u8], mob_at: usize) {
        let Some(facet) = body
            .get(mob_at..mob_at + FACET_LEN)
            .and_then(|bytes| <[u8; FACET_LEN]>::try_from(bytes).ok())
        else {
            return;
        };
        if let Some(&mob) = self.mob_facets.get(&facet)
            && self.suffix.is_some()
        {
            self.last_event = Some((time_ms, EventEntry::Mob { message_type, mob }));
        }
    }

    /// The last event as the cause of a group at `time_ms`.
    fn cause(&self, time_ms: u64) -> Option<CauseEntry> {
        self.last_event
            .as_ref()
            .map(|(event_ms, event)| CauseEntry {
                event_ms: *event_ms,
                delay_ms: time_ms - event_ms,
                event: event.clone(),
            })
    }

    /// One server bundle in the tutorial: its stopped slots first, then its groups, then the
    /// player's achievement state.
    fn bundle(&mut self, context: u64, time_ms: u64, body: &[u8]) -> Result<(), FtueEntitiesError> {
        let Ok(bundle) = ReplicatedStateBundle::decode(body) else {
            return Ok(());
        };
        let payload = &bundle.payload;
        let Some(suffix) = self.suffix else {
            self.achievements(time_ms, payload);
            return Ok(());
        };
        let player = self.player.ok_or(FtueEntitiesError::NoPlayer)?;
        self.stats.bundles += 1;
        if let Some(block) = &bundle.index_block {
            let stops = usize::try_from(block.stop_count).unwrap_or(usize::MAX);
            for interest in block.indexes.iter().take(stops) {
                if let Some(owner) = self.live.remove(&(context, *interest))
                    && let Some(timeline) = self.timeline(owner)
                {
                    timeline.despawns.push(time_ms);
                }
            }
        }
        let static_card_offsets = static_cards(payload, &suffix);
        let asset_card_offsets: Vec<(usize, [u8; GDE_REF_LEN])> = asset_cards(payload)
            .into_iter()
            .filter(|(_, asset)| *asset != PLAYER_SLICE_ASSET.id)
            .collect();
        let cards: Vec<usize> = static_card_offsets
            .iter()
            .copied()
            .chain(asset_card_offsets.iter().map(|(card, _)| *card))
            .collect();
        let mut read: Vec<Range<usize>> = Vec::new();
        let mut at = 0;
        // The end of the last group read: the start of the next group.
        let mut group_start = 0;
        while at < payload.len() {
            let group = whole_group(payload, at).or_else(|| {
                if at == group_start && cards.iter().any(|card| *card >= at) {
                    spawn_group(payload, at, &suffix, &cards)
                } else {
                    None
                }
            });
            if let Some(group) = &group {
                group_start = group.range.end;
            }
            let taken = group
                .filter(|group| self.group(context, time_ms, payload, group, &player, &suffix));
            match taken {
                Some(group) => {
                    at = group.range.end;
                    self.stats
                        .skipped_members
                        .extend(group.skipped.iter().map(|class| (time_ms, *class)));
                    read.push(group.range);
                }
                None => at += 1,
            }
        }
        let unread = |card: &usize| !read.iter().any(|range| range.contains(card));
        for card in static_card_offsets.iter().filter(|card| unread(card)) {
            let start = card + STATIC_CARD_HEAD.len();
            let mut prefix = [0; GDE_REF_PREFIX_LEN];
            prefix.copy_from_slice(&payload[start..start + GDE_REF_PREFIX_LEN]);
            self.stats.unread_cards.push((time_ms, prefix));
        }
        for (card, asset) in &asset_card_offsets {
            if unread(card) {
                self.stats.unread_asset_cards.push((time_ms, *asset));
            }
        }
        self.achievements(time_ms, payload);
        Ok(())
    }

    /// Reads the player's achievement states in `payload`; new bits become the last event
    /// while the player is in the tutorial.
    fn achievements(&mut self, time_ms: u64, payload: &[u8]) {
        for state in achievement_states(payload) {
            let mut bits = Vec::new();
            match state {
                AchievementBits::Full(list) => {
                    self.achievement_bytes = (0..).zip(list.elements).collect();
                }
                AchievementBits::Changes(changes) => {
                    for change in changes {
                        let old = self.achievement_bytes.insert(change.byte, change.value);
                        let new = change.value & !old.unwrap_or(0);
                        bits.extend(
                            (0..BITS_PER_BYTE)
                                .filter(|bit| new >> bit & 1 != 0)
                                .map(|bit| change.byte * BITS_PER_BYTE + bit),
                        );
                    }
                }
            }
            if !bits.is_empty() && self.suffix.is_some() {
                self.last_event = Some((time_ms, EventEntry::Achievements { bits }));
            }
        }
    }

    fn timeline(&mut self, owner: Owner) -> Option<&mut Timeline> {
        match owner {
            Owner::Static(index) => Some(&mut self.entities[index].timeline),
            Owner::Asset(index) => Some(&mut self.assets[index].timeline),
            Owner::Mob => None,
        }
    }

    /// Takes a group read from the payload: a spawn when it holds a static spawn card or the
    /// card of an asset entity, a mob when it holds a mob's card, a change when it belongs to an
    /// entity in view. Returns whether it was taken.
    fn group(
        &mut self,
        context: u64,
        time_ms: u64,
        payload: &[u8],
        group: &WalkedGroup,
        player: &PlayerIds,
        suffix: &[u8; GDE_REF_PREFIX_LEN],
    ) -> bool {
        let slot = (context, group.interest);
        let entry = GroupEntry {
            time_ms,
            interest: group.interest,
            members: group
                .members
                .iter()
                .map(|member| member_entry(member, payload, player, suffix))
                .collect(),
            after: self.cause(time_ms),
        };
        if let Some(gde_ref) = group.static_card(suffix) {
            let entity = self.static_entity(gde_ref);
            self.spawn(slot, Owner::Static(entity), entry);
            return true;
        }
        if let Some((asset, gde_ref)) = group.asset_card() {
            if asset == PLAYER_SLICE_ASSET {
                return false;
            }
            if let Some(vitals_id) = group.vitals_id() {
                self.mob(time_ms, asset, gde_ref, vitals_id);
                self.occupy(slot, Owner::Mob, time_ms);
                return true;
            }
            let entity = self.asset_entity(asset, gde_ref, group.position());
            self.spawn(slot, Owner::Asset(entity), entry);
            return true;
        }
        let owner = match self.live.get(&slot) {
            None => return false,
            Some(Owner::Mob) => return true,
            Some(&owner) => owner,
        };
        if group.members.iter().any(|member| member.card.is_some())
            || !self.is_change_of(owner, group)
        {
            return false;
        }
        let Some(timeline) = self.timeline(owner) else {
            return false;
        };
        let changes = &mut timeline.changes;
        if changes
            .last()
            .is_some_and(|last| last.members == entry.members)
        {
            self.stats.repeated_changes += 1;
        } else {
            changes.push(entry);
            self.stats.changes += 1;
        }
        true
    }

    /// Whether every member of `group` is of a class that the owner's spawns hold or that
    /// [`crate::extra_members`] reads.
    fn is_change_of(&mut self, owner: Owner, group: &WalkedGroup) -> bool {
        let Some(timeline) = self.timeline(owner) else {
            return false;
        };
        let spawns = &timeline.spawns;
        group.members.iter().all(|member| {
            extra_members::has_reader(member.class)
                || spawns.iter().any(|spawn| {
                    spawn
                        .members
                        .iter()
                        .any(|known| known.class == member.class)
                })
        })
    }

    /// The index of the static entity with `gde_ref`, made when new.
    fn static_entity(&mut self, gde_ref: [u8; GDE_REF_LEN]) -> usize {
        let mut prefix = [0; GDE_REF_PREFIX_LEN];
        prefix.copy_from_slice(&gde_ref[..GDE_REF_PREFIX_LEN]);
        let next = self.entities.len();
        let entity = *self.index.entry(prefix).or_insert(next);
        if entity == next {
            self.entities.push(EntityEntry {
                gde_ref_prefix: hex::encode(prefix),
                timeline: Timeline::default(),
            });
        }
        entity
    }

    /// The index of the asset entity with `gde_ref`, made when new; its position is the first
    /// one a spawn gives.
    fn asset_entity(
        &mut self,
        asset: AssetId,
        gde_ref: [u8; GDE_REF_LEN],
        position: Option<[f32; 3]>,
    ) -> usize {
        let next = self.assets.len();
        let entity = *self.asset_index.entry(gde_ref).or_insert(next);
        if entity == next {
            self.assets.push(AssetEntityEntry {
                asset: hex::encode(asset.id),
                asset_sub_id: asset.sub_id,
                retail_gde_ref: hex::encode(gde_ref),
                position: None,
                timeline: Timeline::default(),
            });
        }
        let known = &mut self.assets[entity].position;
        *known = known.or(position);
        entity
    }

    /// Keeps a mob's first card.
    fn mob(&mut self, time_ms: u64, asset: AssetId, gde_ref: [u8; GDE_REF_LEN], vitals_id: u32) {
        self.stats.mob_cards += 1;
        let next = self.mobs.len();
        if *self.mob_facets.entry(facet_of(&gde_ref)).or_insert(next) == next {
            self.mobs.push(MobEntry {
                first_card_ms: time_ms,
                vitals_id,
                asset: hex::encode(asset.id),
                retail_gde_ref: hex::encode(gde_ref),
            });
        }
    }

    /// Gives `slot` to `owner`; the entity that had it leaves view at `time_ms`.
    fn occupy(&mut self, slot: (u64, u16), owner: Owner, time_ms: u64) {
        if let Some(other) = self.live.insert(slot, owner)
            && other != owner
            && let Some(timeline) = self.timeline(other)
        {
            timeline.despawns.push(time_ms);
        }
    }

    fn spawn(&mut self, slot: (u64, u16), owner: Owner, entry: GroupEntry) {
        let in_view = self.live.get(&slot) == Some(&owner);
        let time_ms = entry.time_ms;
        let Some(timeline) = self.timeline(owner) else {
            return;
        };
        if in_view
            && timeline
                .spawns
                .last()
                .is_some_and(|last| last.members == entry.members)
        {
            self.stats.repeated_spawns += 1;
            return;
        }
        timeline.spawns.push(entry);
        self.stats.spawns += 1;
        self.occupy(slot, owner, time_ms);
    }
}

/// One member read from a payload.
#[derive(Debug, Clone, PartialEq)]
struct WalkedMember {
    component: u64,
    class: u64,
    /// The body bytes in the payload.
    body: Range<usize>,
    /// The state, for a spawn card.
    card: Option<GdeMetadataReplicatedState>,
    /// The `vitalsId` of a vitals state.
    vitals_id: Option<u32>,
    /// The position in metres of a position or ALC state.
    position: Option<[f32; 3]>,
}

/// What reading one member gave.
#[derive(Debug, Clone, PartialEq)]
enum MemberRead {
    /// A member whose class has a reader that took its body.
    Read(WalkedMember),
    /// A member whose class has no reader: its body length is not on the wire.
    NoReader {
        class: u64,
        /// The payload offset of its body.
        body_start: usize,
    },
}

/// One group read from a payload.
#[derive(Debug, Clone, PartialEq)]
struct WalkedGroup {
    interest: u16,
    /// The members read, in wire order.
    members: Vec<WalkedMember>,
    /// The classes of the members left out because they have no reader ([`spawn_group`]).
    skipped: Vec<u64>,
    /// The payload offset of its first member.
    members_start: usize,
    /// Its bytes in the payload.
    range: Range<usize>,
}

impl WalkedGroup {
    /// The state of the spawn card member.
    fn card(&self) -> Option<&GdeMetadataReplicatedState> {
        self.members
            .iter()
            .filter(|member| member.component == GDE_METADATA_COMPONENT)
            .find_map(|member| member.card.as_ref())
    }

    /// The `GdeRef` of a spawn card without an asset whose `GdeRef` ends with `suffix`.
    fn static_card(&self, suffix: &[u8; GDE_REF_PREFIX_LEN]) -> Option<[u8; GDE_REF_LEN]> {
        let card = self.card()?;
        let gde_ref = card.gde_ref?;
        (card.asset.is_none() && gde_ref[GDE_REF_PREFIX_LEN..] == suffix[..]).then_some(gde_ref)
    }

    /// The asset and `GdeRef` of a spawn card that names both.
    fn asset_card(&self) -> Option<(AssetId, [u8; GDE_REF_LEN])> {
        let card = self.card()?;
        Some((card.asset?, card.gde_ref?))
    }

    fn vitals_id(&self) -> Option<u32> {
        self.members.iter().find_map(|member| member.vitals_id)
    }

    fn position(&self) -> Option<[f32; 3]> {
        self.members.iter().find_map(|member| member.position)
    }
}

/// The whole group read from `at` that ends at a boundary. When a later start before its first
/// member also gives a whole group with the same end, that group is the real one: the bytes
/// before it only looked like a longer group integer.
fn whole_group(payload: &[u8], at: usize) -> Option<WalkedGroup> {
    let whole =
        |start| read_group(payload, start).filter(|group| is_boundary(payload, group.range.end));
    let group = whole(at)?;
    let later = (at + 1..group.members_start)
        .rev()
        .filter_map(whole)
        .find(|later| later.range.end == group.range.end);
    Some(later.unwrap_or(group))
}

/// Reads the group at `at`: its integer, its count and that many members, each with a reader.
fn read_group(payload: &[u8], at: usize) -> Option<WalkedGroup> {
    let (interest, count, members_start) = group_head(payload, at)?;
    let mut reader = Reader::new(&payload[members_start..]);
    let mut members = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let MemberRead::Read(member) = read_member(payload, &mut reader)? else {
            return None;
        };
        members.push(member);
    }
    Some(WalkedGroup {
        interest,
        members,
        skipped: Vec::new(),
        members_start,
        range: at..payload.len() - reader.remaining(),
    })
}

/// Reads the head of the group at `at`: its slot, its member count (never 0) and the payload
/// offset of its first member.
fn group_head(payload: &[u8], at: usize) -> Option<(u16, u8, usize)> {
    let mut reader = Reader::new(payload.get(at..)?);
    let interest = u16::try_from(reader.prefixed().ok()? & INTEREST_MASK).ok()?;
    let count = reader.u8().ok()?;
    (count > 0).then_some((interest, count, payload.len() - reader.remaining()))
}

/// The spawn group at `at` that [`read_group`] refuses because members have no reader. Such a
/// member is left out of the group. Its body ends at the start of the next member, which must
/// have a reader, or, for the last member, at its [`raw_limit`], which must be a group start or
/// the payload end. Of the ends that let the rest of the group read to a boundary, the
/// earliest is taken. The group must hold a static card or an asset card; `cards` are the
/// payload offsets of the spawn card members.
fn spawn_group(
    payload: &[u8],
    at: usize,
    suffix: &[u8; GDE_REF_PREFIX_LEN],
    cards: &[usize],
) -> Option<WalkedGroup> {
    let (interest, count, members_start) = group_head(payload, at)?;
    let (read, end) = members_to_end(payload, members_start, count, cards, &mut HashSet::new())?;
    let mut members = Vec::new();
    let mut skipped = Vec::new();
    for member in read {
        match member {
            MemberRead::Read(member) => members.push(member),
            MemberRead::NoReader { class, .. } => skipped.push(class),
        }
    }
    let group = WalkedGroup {
        interest,
        members,
        skipped,
        members_start,
        range: at..end,
    };
    (group.static_card(suffix).is_some() || group.asset_card().is_some()).then_some(group)
}

/// Reads the last `left` members of a group from `at` as [`spawn_group`] says, and returns them
/// with the group end. `dead` keeps the `(offset, members left)` from which the group cannot
/// end.
fn members_to_end(
    payload: &[u8],
    at: usize,
    left: u8,
    cards: &[usize],
    dead: &mut HashSet<(usize, u8)>,
) -> Option<(Vec<MemberRead>, usize)> {
    let Some(rest) = left.checked_sub(1) else {
        return is_boundary(payload, at).then_some((Vec::new(), at));
    };
    if dead.contains(&(at, left)) {
        return None;
    }
    let found = match member_at(payload, at)? {
        MemberRead::Read(member) => {
            members_to_end(payload, member.body.end, rest, cards, dead).map(
                |(mut tail, end)| {
                    tail.insert(0, MemberRead::Read(member));
                    (tail, end)
                },
            )
        }
        MemberRead::NoReader { class, body_start } => {
            let limit = raw_limit(payload, body_start, cards);
            let tail = if rest == 0 {
                (limit >= body_start + MIN_BODY_LEN && !cards.contains(&limit))
                    .then_some((Vec::new(), limit))
            } else {
                (body_start + MIN_BODY_LEN..=limit)
                    .filter(|&end| matches!(member_at(payload, end), Some(MemberRead::Read(_))))
                    .find_map(|end| members_to_end(payload, end, rest, cards, dead))
            };
            tail.map(|(mut tail, end)| {
                tail.insert(0, MemberRead::NoReader { class, body_start });
                (tail, end)
            })
        }
    };
    if found.is_none() {
        dead.insert((at, left));
    }
    found
}

/// The furthest end of the body of a member without a reader whose body starts at
/// `body_start`: the first later offset where a spawn card member of `cards` or a group that
/// [`whole_group`] reads starts, else the payload end. So the body never holds a card or a
/// whole readable group.
fn raw_limit(payload: &[u8], body_start: usize, cards: &[usize]) -> usize {
    (body_start + MIN_BODY_LEN..payload.len())
        .find(|&at| cards.contains(&at) || whole_group(payload, at).is_some())
        .unwrap_or(payload.len())
}

/// Reads the member at payload offset `at`.
fn member_at(payload: &[u8], at: usize) -> Option<MemberRead> {
    read_member(payload, &mut Reader::new(payload.get(at..)?))
}

/// Reads one member: its integer, its class and, when the class has a reader, its body.
/// `None` when the header does not read or the reader refuses the body.
fn read_member(payload: &[u8], reader: &mut Reader<'_>) -> Option<MemberRead> {
    let (component, class) = extra_members::read_member_header(reader)?;
    let start = payload.len() - reader.remaining();
    let mut card = None;
    let mut vitals_id = None;
    let mut position = None;
    let read = match class {
        GDE_METADATA_TYPE_INDEX => GdeMetadataReplicatedState::read(reader)
            .map(|state| card = Some(state))
            .is_ok(),
        DELAYED_EVENT_TYPE_INDEX => DelayedEventState::read(reader).is_ok(),
        PAPERDOLL_TYPE_INDEX => PaperdollState::read(reader).is_ok(),
        GATHERABLE_CONTROLLER_TYPE_INDEX => GatherableControllerState::read(reader).is_ok(),
        DAMAGE_RECEIVER_TYPE_INDEX => DamageReceiverState::read(reader).is_ok(),
        POSITION_IN_THE_WORLD_TYPE_INDEX => PositionInTheWorldReplicatedState::read(reader)
            .map(|state| position = state.position.map(|at| [at.x, at.y, at.z()]))
            .is_ok(),
        ALC_TYPE_INDEX => AlcState::read(reader)
            .map(|state| position = state.pos10.map(|at| [at.x, at.y, at.z()]))
            .is_ok(),
        VITALS_TYPE_INDEX => VitalsState::read(reader)
            .map(|state| vitals_id = state.slot_0.and_then(|slot| slot.vitals_id))
            .is_ok(),
        _ if extra_members::has_reader(class) => {
            extra_members::read_body(class, reader).is_some()
        }
        _ => {
            return Some(MemberRead::NoReader {
                class,
                body_start: start,
            });
        }
    };
    if !read {
        return None;
    }
    Some(MemberRead::Read(WalkedMember {
        component,
        class,
        body: start..payload.len() - reader.remaining(),
        card,
        vitals_id,
        position,
    }))
}

/// Whether `at` is the payload end or the start of a group whose first member header reads.
fn is_boundary(payload: &[u8], at: usize) -> bool {
    if at == payload.len() {
        return true;
    }
    let mut reader = Reader::new(&payload[at..]);
    reader.prefixed().is_ok()
        && reader.u8().is_ok_and(|count| count > 0)
        && reader.prefixed().is_ok()
        && decode_type_id(reader.rest()).is_ok()
}

/// Payload offsets of every static spawn card member: [`STATIC_CARD_HEAD`] and a `GdeRef`
/// ending with `suffix`.
fn static_cards(payload: &[u8], suffix: &[u8; GDE_REF_PREFIX_LEN]) -> Vec<usize> {
    let card_len = STATIC_CARD_HEAD.len() + GDE_REF_LEN;
    payload
        .windows(card_len)
        .enumerate()
        .filter(|(_, window)| {
            window.starts_with(&STATIC_CARD_HEAD) && window.ends_with(suffix.as_slice())
        })
        .map(|(at, _)| at)
        .collect()
}

/// Payload offsets and assets of every spawn card member with an asset and a `GdeRef`.
fn asset_cards(payload: &[u8]) -> Vec<(usize, [u8; GDE_REF_LEN])> {
    (0..payload.len())
        .filter_map(|at| {
            let mut reader = Reader::new(&payload[at..]);
            let (component, class) = extra_members::read_member_header(&mut reader)?;
            if (component, class) != (GDE_METADATA_COMPONENT, GDE_METADATA_TYPE_INDEX) {
                return None;
            }
            let card = GdeMetadataReplicatedState::read(&mut reader).ok()?;
            card.gde_ref?;
            Some((at, card.asset?.id))
        })
        .collect()
}

/// The bitset child of every player achievement member in `payload`.
fn achievement_states(payload: &[u8]) -> Vec<AchievementBits> {
    let mut writer = Writer::new();
    let (Ok(class), Ok(writer)) = (
        encode_type_id(&TypeId::FileSlot(ACHIEVEMENT_TYPE_INDEX)),
        writer.prefixed(ACHIEVEMENT_COMPONENT),
    ) else {
        return Vec::new();
    };
    let head = writer.bytes(&class).finish();
    payload
        .windows(head.len())
        .enumerate()
        .filter(|(_, window)| *window == head.as_slice())
        .filter_map(|(at, _)| {
            AchievementState::read(&mut Reader::new(&payload[at + head.len()..]))
                .ok()?
                .bits
        })
        .collect()
}

/// The table entry of a member, with the patch offsets in its body.
fn member_entry(
    member: &WalkedMember,
    payload: &[u8],
    player: &PlayerIds,
    suffix: &[u8; GDE_REF_PREFIX_LEN],
) -> MemberEntry {
    let body = &payload[member.body.clone()];
    let player_gde_ref_at = offsets(body, &player.gde_ref, &[]);
    let taken = ranges(&player_gde_ref_at, GDE_REF_LEN);
    MemberEntry {
        component: member.component,
        class: member.class,
        body: hex::encode(body),
        player_facet_at: offsets(body, &player.facet, &taken),
        phase_suffix_at: offsets(body, suffix, &taken),
        player_gde_ref_at,
        refs: Vec::new(),
    }
}

/// The ranges of `len` bytes from each of `offsets`.
fn ranges(offsets: &[usize], len: usize) -> Vec<Range<usize>> {
    offsets.iter().map(|&at| at..at + len).collect()
}

/// Offsets of `value` in `body` that do not overlap a `taken` range.
fn offsets(body: &[u8], value: &[u8], taken: &[Range<usize>]) -> Vec<usize> {
    body.windows(value.len())
        .enumerate()
        .filter(|(at, window)| {
            *window == value
                && !taken
                    .iter()
                    .any(|range| *at < range.end && range.start < at + value.len())
        })
        .map(|(at, _)| at)
        .collect()
}

/// Writes into every member of the asset entities the offsets of the ids of asset entities
/// and mobs; a whole `GdeRef` is taken before a facet entity, and no id overlaps the player's
/// ids or the phase suffix.
fn add_refs(table: &mut FtueTable) {
    let decode = |text: &str| {
        hex::decode(text)
            .ok()
            .and_then(|bytes| <[u8; GDE_REF_LEN]>::try_from(bytes).ok())
    };
    let targets: Vec<(RefTarget, [u8; GDE_REF_LEN])> =
        table
            .asset_entities
            .iter()
            .enumerate()
            .filter_map(|(index, entity)| {
                Some((RefTarget::Asset(index), decode(&entity.retail_gde_ref)?))
            })
            .chain(table.mobs.iter().enumerate().filter_map(|(index, mob)| {
                Some((RefTarget::Mob(index), decode(&mob.retail_gde_ref)?))
            }))
            .collect();
    let members = table.asset_entities.iter_mut().flat_map(|entity| {
        let timeline = &mut entity.timeline;
        timeline
            .spawns
            .iter_mut()
            .chain(timeline.changes.iter_mut())
    });
    for member in members.flat_map(|group| group.members.iter_mut()) {
        let Ok(body) = hex::decode(&member.body) else {
            continue;
        };
        let mut taken: Vec<Range<usize>> = ranges(&member.player_gde_ref_at, GDE_REF_LEN)
            .into_iter()
            .chain(ranges(&member.player_facet_at, FACET_LEN))
            .chain(ranges(&member.phase_suffix_at, GDE_REF_PREFIX_LEN))
            .collect();
        let mut refs = Vec::new();
        for form in [IdForm::GdeRef, IdForm::Facet] {
            for (target, gde_ref) in &targets {
                for at in offsets(&body, &form.bytes(gde_ref), &taken) {
                    refs.push(RefEntry {
                        at,
                        form,
                        target: *target,
                    });
                }
            }
            taken.extend(
                refs.iter()
                    .map(|found| found.at..found.at + found.form.byte_len()),
            );
        }
        refs.sort_by_key(|found| found.at);
        member.refs = refs;
    }
}

/// Writes the table as JSON, making the folder when needed.
///
/// # Errors
///
/// Returns [`FtueEntitiesError`] when the file cannot be written.
pub fn write_table(path: &Path, table: &FtueTable) -> Result<(), FtueEntitiesError> {
    let io = |source| FtueEntitiesError::Io {
        path: path.to_path_buf(),
        source,
    };
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(io)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(table)?).map_err(io)
}

/// Reads a ledger file.
///
/// # Errors
///
/// Returns [`FtueEntitiesError::Io`] when the file cannot be read.
pub fn read_ledger(path: &Path) -> Result<Vec<u8>, FtueEntitiesError> {
    std::fs::read(path).map_err(|source| FtueEntitiesError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use nw_capture::test_support::{client_datagram, envelope, record_at, server_datagram_at};
    use nw_content::ftue_entities::{FtueEntities, FtueLevel, LevelEvent, SessionIds};
    use nw_messages::{
        BundleMember, ByteChange, FullList, MemberState, VitalsSlot0, WorldPosition,
    };

    use super::*;

    const PHASE_ID: u64 = 0x1020_3040_5060_7080;
    const CONTEXT_ID: u8 = 2;
    const INSTANCE: u64 = 9;
    const PLAYER: [u8; GDE_REF_LEN] = [
        0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x5b, 0x5c, 0x5d, 0x5e, 0x5f,
        0x60,
    ];
    const PREFIX: [u8; GDE_REF_PREFIX_LEN] = [0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8];
    const OTHER_PREFIX: [u8; GDE_REF_PREFIX_LEN] = [0xb1; GDE_REF_PREFIX_LEN];
    const INTEREST: u16 = 9;
    const ISLAND_COMPONENT: u64 = 2;
    const EVENT_COMPONENT: u64 = 7;
    const INTERACT_COMPONENT: u64 = 4;
    /// The FtueIsland tail after the player's ids.
    const ISLAND_TAIL: [u8; 4] = [0x00, 0x00, 0x80, 0x10];
    /// Slot 0 and children 0 to 3.
    const ISLAND_HEAD: [u8; 2] = [0x01, 0x0f];
    /// Slot 0 and child 1 (a `u32`) of an Interact state.
    const INTERACT_HEAD: [u8; 2] = [0x01, 0x02];
    const IN_USE: u32 = 1;
    const SPAWN_MS: u64 = 100;
    const CHANGE_MS: u64 = 200;
    const STOP_MS: u64 = 300;
    const OTHER_MS: u64 = 400;
    /// Bytes of a card body before its `GdeRef`: the slot byte and the child mask.
    const CARD_GDE_REF_AT: usize = 2;
    /// Bytes of a card body with an asset before its `GdeRef`: the slot byte, the child mask,
    /// the asset id and its sub id.
    const ASSET_CARD_GDE_REF_AT: usize = 22;
    const SUB_ID: u32 = 2;
    const ENCOUNTER_ASSET: [u8; GDE_REF_LEN] = [0xe1; GDE_REF_LEN];
    const MARKER_ASSET: [u8; GDE_REF_LEN] = [0xe2; GDE_REF_LEN];
    const MOB_ASSET: [u8; GDE_REF_LEN] = [0xe3; GDE_REF_LEN];
    const ENCOUNTER: [u8; GDE_REF_LEN] = [0xc1; GDE_REF_LEN];
    const MARKER: [u8; GDE_REF_LEN] = [0xc2; GDE_REF_LEN];
    const MOB: [u8; GDE_REF_LEN] = [
        0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd, 0xde, 0xdf,
        0xd0,
    ];
    const MOB_VITALS_ID: u32 = 0x8869_82c9;
    const ENCOUNTER_INTEREST: u16 = 5;
    const MOB_INTEREST: u16 = 6;
    const MARKER_INTEREST: u16 = 7;
    const PLAYER_INTEREST: u16 = 1;
    const POSITION_COMPONENT: u64 = 1;
    const WAVE_COMPONENT: u64 = 6;
    const SPELL_COMPONENT: u64 = 1;
    const VITALS_COMPONENT: u64 = 9;
    const AT: [f32; 3] = [484.35, 848.6, 91.37];
    /// The largest height step of a raw position height.
    const HEIGHT_STEP: f32 = 0.05;
    /// Slot 0, child 0, two history entries; then the same with three.
    const WAVE_WAITING: &str = "01010200000000000000000000000100000001";
    const WAVE_ACTIVE: &str = "010103000000000000000000000001000000010000000200000002";
    /// Slot 0, one grouped mask byte with child 6 (the attachment ids), no other mask byte.
    const SPELL_ATTACHMENT_HEAD: [u8; 2] = [0x01, 0x40];
    /// Slot 0, one grouped mask byte with child 0 (the spell data id).
    const SPELL_DATA_HEAD: [u8; 2] = [0x01, 0x01];
    const SPELL_DATA_ID: u32 = 7;
    const QTE_ID: [u8; 4] = [0xf9, 0xd8, 0x0a, 0x82];
    const QTE_MS: u64 = 1_000;
    const ENCOUNTER_MS: u64 = 1_042;
    const MOB_MS: u64 = 1_100;
    const DEATH_MS: u64 = 2_000;
    const MARKER_MS: u64 = 2_031;
    const FULL_ACHIEVEMENTS_MS: u64 = 3_000;
    const NEW_ACHIEVEMENT_MS: u64 = 3_010;
    const SPELL_CHANGE_MS: u64 = 3_020;
    /// Achievement bitset bytes of the full state.
    const ACHIEVEMENT_BYTES: usize = 4;
    /// A changed byte that sets bit 2 of byte 1: achievement 10.
    const NEW_ACHIEVEMENT_BYTE: u32 = 1;
    const NEW_ACHIEVEMENT_VALUE: u8 = 0b100;
    const NEW_ACHIEVEMENT: u32 = 10;
    const ACHIEVEMENT_VERSION: u64 = 2;

    fn static_ref(prefix: [u8; GDE_REF_PREFIX_LEN]) -> [u8; GDE_REF_LEN] {
        let mut gde_ref = [0; GDE_REF_LEN];
        gde_ref[..GDE_REF_PREFIX_LEN].copy_from_slice(&prefix);
        gde_ref[GDE_REF_PREFIX_LEN..].copy_from_slice(&phase_suffix(PHASE_ID));
        gde_ref
    }

    fn card(prefix: [u8; GDE_REF_PREFIX_LEN]) -> Vec<u8> {
        BundleMember {
            component: GDE_METADATA_COMPONENT,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: None,
                gde_ref: Some(static_ref(prefix)),
            }),
        }
        .encode()
        .expect("card")
    }

    fn event(completed: bool) -> Vec<u8> {
        BundleMember {
            component: EVENT_COMPONENT,
            state: MemberState::DelayedEvent(DelayedEventState {
                total_delay: Some(0),
                completed: Some(completed),
            }),
        }
        .encode()
        .expect("event")
    }

    /// A member of a class read by [`extra_members`].
    fn extra(component: u64, class: u64, body: &[u8]) -> Vec<u8> {
        let mut writer = Writer::new();
        writer
            .prefixed(component)
            .expect("component")
            .bytes(&encode_type_id(&TypeId::FileSlot(class)).expect("class"))
            .bytes(body);
        writer.finish()
    }

    fn island_body() -> Vec<u8> {
        let mut body = ISLAND_HEAD.to_vec();
        body.extend(PLAYER);
        body.extend(PlayerIds::of(PLAYER).facet);
        body.extend(ISLAND_TAIL);
        body
    }

    fn interact(value: u32) -> Vec<u8> {
        let mut body = INTERACT_HEAD.to_vec();
        body.extend(value.to_be_bytes());
        extra(
            INTERACT_COMPONENT,
            extra_members::INTERACT_TYPE_INDEX,
            &body,
        )
    }

    fn group(interest: u16, members: &[Vec<u8>]) -> Vec<u8> {
        let mut writer = Writer::new();
        writer
            .prefixed(u64::from(interest))
            .expect("interest")
            .u8(u8::try_from(members.len()).expect("few"));
        for member in members {
            writer.bytes(member);
        }
        writer.finish()
    }

    fn bundle(stops: Vec<u16>, payload: Vec<u8>) -> Vec<u8> {
        let empty =
            ReplicatedStateBundle::of_groups(1, CONTEXT_ID, stops, Vec::new()).expect("bundle");
        ReplicatedStateBundle { payload, ..empty }
            .encode()
            .expect("bundle")
    }

    struct Ledger {
        bytes: Vec<u8>,
        sequence: u16,
        client_sequence: u16,
    }

    impl Ledger {
        fn new() -> Self {
            let mut ledger = Self {
                bytes: Vec::new(),
                sequence: 1,
                client_sequence: 1,
            };
            ledger.server(
                0,
                CLIENT_ADD_ENTRY_MSG_TYPE_INDEX,
                ClientAddEntryMsg { entry_id: PLAYER }
                    .encode()
                    .expect("entry"),
            );
            ledger.server(
                1,
                LEVEL_INFO_CHANGED_MSG_TYPE_INDEX,
                LevelInfoChangedMsg::tutorial(CONTEXT_ID, INSTANCE, PHASE_ID)
                    .encode()
                    .expect("level"),
            );
            ledger
        }

        fn server(&mut self, time_ms: u64, index: u64, body: Vec<u8>) {
            let datagram = server_datagram_at(self.sequence, &[envelope(index, body)]);
            self.sequence += 1;
            self.bytes
                .extend(record_at(time_ms, SERVER_TO_CLIENT, &datagram));
        }

        fn bundle(&mut self, time_ms: u64, stops: Vec<u16>, payload: Vec<u8>) {
            self.server(
                time_ms,
                REPLICATED_STATE_BUNDLE_TYPE_INDEX,
                bundle(stops, payload),
            );
        }

        /// A client facet call to the player with `id` after its header.
        fn client(&mut self, time_ms: u64, index: u64, id: &[u8]) {
            let mut body = vec![0; FACET_LEN];
            body.extend(PlayerIds::of(PLAYER).facet);
            body.extend(id);
            let datagram = client_datagram(self.client_sequence, &[envelope(index, body)]);
            self.client_sequence += 1;
            self.bytes
                .extend(record_at(time_ms, CLIENT_TO_SERVER, &datagram));
        }
    }

    fn spawn_payload() -> Vec<u8> {
        let island = extra(
            ISLAND_COMPONENT,
            extra_members::FTUE_ISLAND_TYPE_INDEX,
            &island_body(),
        );
        group(INTEREST, &[island, card(PREFIX), event(false)])
    }

    fn scan(ledger: &Ledger) -> FtueScan {
        scan_ledger(&ledger.bytes).expect("scan")
    }

    #[test]
    fn a_static_entity_gives_spawns_changes_and_despawns_with_patch_offsets() {
        let mut ledger = Ledger::new();
        ledger.bundle(SPAWN_MS, Vec::new(), spawn_payload());
        ledger.bundle(SPAWN_MS + 1, Vec::new(), spawn_payload());
        let change = group(INTEREST, &[event(true), interact(IN_USE)]);
        ledger.bundle(CHANGE_MS, Vec::new(), change.clone());
        ledger.bundle(CHANGE_MS + 1, Vec::new(), change);
        ledger.bundle(STOP_MS, vec![INTEREST], Vec::new());
        ledger.bundle(OTHER_MS, Vec::new(), group(INTEREST, &[event(false)]));
        let scan = scan(&ledger);
        assert_eq!(
            (
                scan.stats.spawns,
                scan.stats.repeated_spawns,
                scan.stats.changes,
                scan.stats.repeated_changes
            ),
            (1, 1, 1, 1)
        );
        assert!(scan.stats.unread_cards.is_empty());
        assert_eq!(scan.table.level, TUTORIAL_LEVEL.folder);
        let [entity] = scan.table.entities.as_slice() else {
            panic!("one entity: {:?}", scan.table.entities);
        };
        assert_eq!(entity.gde_ref_prefix, hex::encode(PREFIX));
        let entity = &entity.timeline;
        assert_eq!(entity.despawns, vec![STOP_MS]);
        let spawn = &entity.spawns[0];
        assert_eq!((spawn.time_ms, spawn.interest), (SPAWN_MS, INTEREST));
        let island = &spawn.members[0];
        assert_eq!(island.body, hex::encode(island_body()));
        assert_eq!(island.player_gde_ref_at, vec![ISLAND_HEAD.len()]);
        assert_eq!(
            island.player_facet_at,
            vec![ISLAND_HEAD.len() + GDE_REF_LEN]
        );
        let card_member = &spawn.members[1];
        assert_eq!(
            (card_member.component, card_member.class),
            (GDE_METADATA_COMPONENT, GDE_METADATA_TYPE_INDEX)
        );
        assert_eq!(
            card_member.phase_suffix_at,
            vec![CARD_GDE_REF_AT + GDE_REF_PREFIX_LEN]
        );
        let change = &entity.changes[0];
        assert_eq!(change.time_ms, CHANGE_MS);
        assert_eq!(
            change
                .members
                .iter()
                .map(|member| member.class)
                .collect::<Vec<_>>(),
            vec![DELAYED_EVENT_TYPE_INDEX, extra_members::INTERACT_TYPE_INDEX]
        );
    }

    #[test]
    fn the_table_json_loads_in_nw_content() {
        let mut ledger = Ledger::new();
        ledger.bundle(SPAWN_MS, Vec::new(), spawn_payload());
        let table = scan(&ledger).table;
        let loaded =
            FtueEntities::parse(&serde_json::to_string(&table).expect("json")).expect("table");
        let entity = loaded.entity(&PREFIX).expect("entity");
        assert_eq!(entity.gde_ref(PHASE_ID), static_ref(PREFIX));
        assert_eq!(entity.spawns[0].members.len(), 3);
    }

    /// A class without a reader (`StatusEffectsComponentReplicatedState`).
    const UNREAD_CLASS: u64 = 4236;
    /// A body of [`UNREAD_CLASS`]: a slot byte and two bytes that read as no member.
    const UNREAD_BODY: [u8; 3] = [1, 2, 3];

    fn unread_member() -> Vec<u8> {
        extra(EVENT_COMPONENT, UNREAD_CLASS, &UNREAD_BODY)
    }

    #[test]
    fn cards_in_groups_without_readers_are_reported() {
        // Two members without a reader in a row: the first has no end that a readable member
        // shows, so the group stays unread.
        let mut ledger = Ledger::new();
        ledger.bundle(
            SPAWN_MS,
            Vec::new(),
            group(
                INTEREST,
                &[card(OTHER_PREFIX), unread_member(), unread_member()],
            ),
        );
        ledger.bundle(
            OTHER_MS,
            Vec::new(),
            group(
                MARKER_INTEREST,
                &[
                    asset_card(MARKER_ASSET, MARKER),
                    unread_member(),
                    unread_member(),
                ],
            ),
        );
        let scan = scan(&ledger);
        assert!(scan.table.entities.is_empty());
        assert!(scan.table.asset_entities.is_empty());
        assert!(scan.stats.skipped_members.is_empty());
        assert_eq!(scan.stats.unread_cards, vec![(SPAWN_MS, OTHER_PREFIX)]);
        assert_eq!(
            scan.stats.unread_asset_cards,
            vec![(OTHER_MS, MARKER_ASSET)]
        );
    }

    #[test]
    fn a_member_without_a_reader_keeps_the_assets_second_card() {
        let mut ledger = Ledger::new();
        ledger.bundle(
            SPAWN_MS,
            Vec::new(),
            group(
                MARKER_INTEREST,
                &[asset_card(MARKER_ASSET, MARKER), position()],
            ),
        );
        ledger.bundle(STOP_MS, vec![MARKER_INTEREST], Vec::new());
        // The second card of the marker with a member without a reader before its last member,
        // then the card of another asset whose last member has no reader.
        let mut payload = group(
            MARKER_INTEREST,
            &[
                asset_card(MARKER_ASSET, MARKER),
                unread_member(),
                position(),
            ],
        );
        payload.extend(group(
            ENCOUNTER_INTEREST,
            &[asset_card(ENCOUNTER_ASSET, ENCOUNTER), unread_member()],
        ));
        ledger.bundle(OTHER_MS, Vec::new(), payload);
        let scan = scan(&ledger);
        assert!(scan.stats.unread_asset_cards.is_empty());
        assert_eq!(
            scan.stats.skipped_members,
            vec![(OTHER_MS, UNREAD_CLASS), (OTHER_MS, UNREAD_CLASS)]
        );
        let [marker, encounter] = scan.table.asset_entities.as_slice() else {
            panic!("two asset entities: {:?}", scan.table.asset_entities);
        };
        assert_eq!(marker.asset, hex::encode(MARKER_ASSET));
        assert_eq!(marker.timeline.despawns, vec![STOP_MS]);
        let [first, second] = marker.timeline.spawns.as_slice() else {
            panic!("two marker spawns: {:?}", marker.timeline.spawns);
        };
        assert_eq!((first.time_ms, second.time_ms), (SPAWN_MS, OTHER_MS));
        assert_eq!(second.members, first.members);
        assert_eq!(encounter.asset, hex::encode(ENCOUNTER_ASSET));
        let [encounter_spawn] = encounter.timeline.spawns.as_slice() else {
            panic!("one encounter spawn: {:?}", encounter.timeline.spawns);
        };
        assert_eq!(
            (encounter_spawn.time_ms, encounter_spawn.interest),
            (OTHER_MS, ENCOUNTER_INTEREST)
        );
        assert_eq!(
            encounter_spawn
                .members
                .iter()
                .map(|member| member.class)
                .collect::<Vec<_>>(),
            vec![GDE_METADATA_TYPE_INDEX]
        );
    }

    #[test]
    fn a_member_without_a_reader_makes_no_change_and_keeps_the_next_card() {
        let mut ledger = Ledger::new();
        ledger.bundle(SPAWN_MS, Vec::new(), spawn_payload());
        let mut payload = group(INTEREST, &[unread_member(), interact(IN_USE)]);
        payload.extend(group(
            MARKER_INTEREST,
            &[asset_card(MARKER_ASSET, MARKER)],
        ));
        ledger.bundle(CHANGE_MS, Vec::new(), payload);
        let scan = scan(&ledger);
        assert_eq!(scan.stats.changes, 0);
        assert!(scan.stats.skipped_members.is_empty());
        assert!(scan.table.entities[0].timeline.changes.is_empty());
        let [marker] = scan.table.asset_entities.as_slice() else {
            panic!("one asset entity: {:?}", scan.table.asset_entities);
        };
        assert_eq!(
            marker
                .timeline
                .spawns
                .iter()
                .map(|spawn| spawn.time_ms)
                .collect::<Vec<_>>(),
            vec![CHANGE_MS]
        );
    }

    #[test]
    fn a_new_entity_in_a_slot_despawns_the_old_one() {
        let mut ledger = Ledger::new();
        ledger.bundle(SPAWN_MS, Vec::new(), spawn_payload());
        ledger.bundle(OTHER_MS, Vec::new(), group(INTEREST, &[card(OTHER_PREFIX)]));
        let entities = scan(&ledger).table.entities;
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].timeline.despawns, vec![OTHER_MS]);
        assert!(entities[1].timeline.despawns.is_empty());
    }

    #[test]
    fn a_ledger_without_the_tutorial_is_refused() {
        let mut ledger = Ledger {
            bytes: Vec::new(),
            sequence: 1,
            client_sequence: 1,
        };
        ledger.bundle(SPAWN_MS, Vec::new(), spawn_payload());
        assert!(matches!(
            scan_ledger(&ledger.bytes),
            Err(FtueEntitiesError::NoTutorial)
        ));
    }

    fn asset_card(asset: [u8; GDE_REF_LEN], gde_ref: [u8; GDE_REF_LEN]) -> Vec<u8> {
        BundleMember {
            component: GDE_METADATA_COMPONENT,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: Some(AssetId {
                    id: asset,
                    sub_id: SUB_ID,
                }),
                gde_ref: Some(gde_ref),
            }),
        }
        .encode()
        .expect("card")
    }

    fn position() -> Vec<u8> {
        let [x, y, z] = AT;
        BundleMember {
            component: POSITION_COMPONENT,
            state: MemberState::Position(PositionInTheWorldReplicatedState {
                position: WorldPosition::from_metres(x, y, z),
                ..PositionInTheWorldReplicatedState::default()
            }),
        }
        .encode()
        .expect("position")
    }

    fn wave(body: &str) -> Vec<u8> {
        extra(
            WAVE_COMPONENT,
            extra_members::ENCOUNTER_TYPE_INDEX,
            &hex::decode(body).expect("hex"),
        )
    }

    fn vitals() -> Vec<u8> {
        BundleMember {
            component: VITALS_COMPONENT,
            state: MemberState::Vitals(VitalsState {
                slot_0: Some(VitalsSlot0 {
                    vitals_id: Some(MOB_VITALS_ID),
                    ..VitalsSlot0::default()
                }),
                slot_1: None,
            }),
        }
        .encode()
        .expect("vitals")
    }

    /// A spell state whose attachment is the mob.
    fn spell_attached_to_mob() -> Vec<u8> {
        let mut body = SPELL_ATTACHMENT_HEAD.to_vec();
        body.extend(MOB);
        body.extend(facet_of(&MOB));
        extra(SPELL_COMPONENT, extra_members::SPELL_TYPE_INDEX, &body)
    }

    fn spell_data() -> Vec<u8> {
        let mut body = SPELL_DATA_HEAD.to_vec();
        body.extend(SPELL_DATA_ID.to_be_bytes());
        extra(SPELL_COMPONENT, extra_members::SPELL_TYPE_INDEX, &body)
    }

    fn achievements(bits: AchievementBits) -> Vec<u8> {
        let member = BundleMember {
            component: ACHIEVEMENT_COMPONENT,
            state: MemberState::Achievement(AchievementState { bits: Some(bits) }),
        }
        .encode()
        .expect("achievements");
        group(PLAYER_INTEREST, &[member])
    }

    /// The encounter after a quick-time event, a mob, the mob's death, a marker that names the
    /// mob after it, and new achievement bits before a marker change.
    fn level_ledger() -> Ledger {
        let mut ledger = Ledger::new();
        ledger.client(QTE_MS, QTE_COMPLETED_TYPE_INDEX, &QTE_ID);
        ledger.bundle(
            ENCOUNTER_MS,
            Vec::new(),
            group(
                ENCOUNTER_INTEREST,
                &[
                    position(),
                    asset_card(ENCOUNTER_ASSET, ENCOUNTER),
                    wave(WAVE_WAITING),
                ],
            ),
        );
        ledger.bundle(
            MOB_MS,
            Vec::new(),
            group(MOB_INTEREST, &[asset_card(MOB_ASSET, MOB), vitals()]),
        );
        let mut death = vec![0; FACET_LEN];
        death.extend(facet_of(&MOB));
        ledger.server(DEATH_MS, DISABLE_COLLISION_TYPE_INDEX, death);
        let mut payload = group(
            MARKER_INTEREST,
            &[asset_card(MARKER_ASSET, MARKER), spell_attached_to_mob()],
        );
        payload.extend(group(ENCOUNTER_INTEREST, &[wave(WAVE_ACTIVE)]));
        ledger.bundle(MARKER_MS, Vec::new(), payload);
        let full = FullList {
            version: Some(1),
            elements: vec![0; ACHIEVEMENT_BYTES],
        };
        ledger.bundle(
            FULL_ACHIEVEMENTS_MS,
            Vec::new(),
            achievements(AchievementBits::Full(full)),
        );
        let change = ByteChange {
            byte: NEW_ACHIEVEMENT_BYTE,
            version: ACHIEVEMENT_VERSION,
            value: NEW_ACHIEVEMENT_VALUE,
        };
        ledger.bundle(
            NEW_ACHIEVEMENT_MS,
            Vec::new(),
            achievements(AchievementBits::Changes(vec![change])),
        );
        ledger.bundle(
            SPELL_CHANGE_MS,
            Vec::new(),
            group(MARKER_INTEREST, &[spell_data()]),
        );
        ledger
    }

    #[test]
    fn asset_entities_name_their_position_refs_and_causes() {
        let scan = scan(&level_ledger());
        let table = &scan.table;
        assert_eq!(scan.stats.mob_cards, 1);
        let [mob] = table.mobs.as_slice() else {
            panic!("one mob: {:?}", table.mobs);
        };
        assert_eq!(
            (
                mob.first_card_ms,
                mob.vitals_id,
                mob.retail_gde_ref.as_str()
            ),
            (MOB_MS, MOB_VITALS_ID, hex::encode(MOB).as_str())
        );
        let [encounter, marker] = table.asset_entities.as_slice() else {
            panic!("two asset entities: {:?}", table.asset_entities);
        };
        assert_eq!(encounter.asset, hex::encode(ENCOUNTER_ASSET));
        assert_eq!(encounter.asset_sub_id, SUB_ID);
        let position = encounter.position.expect("position");
        assert!(
            position
                .iter()
                .zip(AT)
                .all(|(got, want)| (got - want).abs() < HEIGHT_STEP),
            "{position:?}"
        );
        let spawn = &encounter.timeline.spawns[0];
        assert_eq!(
            spawn.after,
            Some(CauseEntry {
                event_ms: QTE_MS,
                delay_ms: ENCOUNTER_MS - QTE_MS,
                event: EventEntry::Client {
                    message_type: QTE_COMPLETED_TYPE_INDEX,
                    id: hex::encode(QTE_ID),
                },
            })
        );
        let own_card = RefEntry {
            at: ASSET_CARD_GDE_REF_AT,
            form: IdForm::GdeRef,
            target: RefTarget::Asset(0),
        };
        assert_eq!(spawn.members[1].refs, vec![own_card]);
        let change = &encounter.timeline.changes[0];
        let death = EventEntry::Mob {
            message_type: DISABLE_COLLISION_TYPE_INDEX,
            mob: 0,
        };
        assert_eq!(
            (change.time_ms, change.members[0].body.as_str()),
            (MARKER_MS, WAVE_ACTIVE)
        );
        assert_eq!(
            change.after.as_ref().map(|cause| &cause.event),
            Some(&death)
        );
        let spell = &marker.timeline.spawns[0].members[1];
        let mob_ref = |at, form| RefEntry {
            at,
            form,
            target: RefTarget::Mob(0),
        };
        let attachment_at = SPELL_ATTACHMENT_HEAD.len();
        assert_eq!(
            spell.refs,
            vec![
                mob_ref(attachment_at, IdForm::GdeRef),
                mob_ref(attachment_at + GDE_REF_LEN, IdForm::Facet)
            ]
        );
        let marker_change = &marker.timeline.changes[0];
        assert_eq!(
            marker_change.after,
            Some(CauseEntry {
                event_ms: NEW_ACHIEVEMENT_MS,
                delay_ms: SPELL_CHANGE_MS - NEW_ACHIEVEMENT_MS,
                event: EventEntry::Achievements {
                    bits: vec![NEW_ACHIEVEMENT],
                },
            })
        );
    }

    #[test]
    fn the_asset_entities_load_in_nw_content_and_take_session_ids() {
        const SERVER_MOB: [u8; GDE_REF_LEN] = [0x99; GDE_REF_LEN];
        const SERVER_MARKER: [u8; GDE_REF_LEN] = [0x98; GDE_REF_LEN];
        let table = scan(&level_ledger()).table;
        let level = FtueLevel::parse(&serde_json::to_string(&table).expect("json")).expect("level");
        assert_eq!(level.mobs[0].vitals_id, MOB_VITALS_ID);
        let marker = &level.assets[1];
        assert_eq!(marker.retail_gde_ref, MARKER);
        let spawn = &marker.spawns[0];
        let ids = SessionIds {
            player_gde_ref: PLAYER,
            player_facet: PlayerIds::of(PLAYER).facet,
            phase_id: PHASE_ID,
        };
        let bodies = spawn.patched_bodies(&ids, |target| match target {
            RefTarget::Asset(1) => Some(SERVER_MARKER),
            RefTarget::Mob(0) => Some(SERVER_MOB),
            RefTarget::Asset(_) | RefTarget::Mob(_) => None,
        });
        let card_end = ASSET_CARD_GDE_REF_AT + GDE_REF_LEN;
        assert_eq!(bodies[0][ASSET_CARD_GDE_REF_AT..card_end], SERVER_MARKER);
        let attachment_at = SPELL_ATTACHMENT_HEAD.len();
        let facet_at = attachment_at + GDE_REF_LEN;
        assert_eq!(bodies[1][attachment_at..facet_at], SERVER_MOB);
        assert_eq!(bodies[1][facet_at..], facet_of(&SERVER_MOB));
        assert!(matches!(
            marker.changes[0].cause.as_ref().map(|cause| &cause.event),
            Some(LevelEvent::Achievements { .. })
        ));
        assert_eq!(level.statics.entities.len(), table.entities.len());
    }
}
