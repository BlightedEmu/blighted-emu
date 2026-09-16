// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tutorial static entity table (`ftue-entities-<build>.json`, made by
//! `content-tool ftue-entities` from a retail capture of a new character).
//!
//! The client has the static entities of the tutorial level (a satchel, a chest, trigger
//! volumes, timelines) in its level slices. The server still sends each one a spawn card that
//! names only its `GdeRef` (no asset), with the states of some of its components, and later
//! groups that change those states. A static `GdeRef` is 8 fixed bytes and then the tutorial
//! phase id of the session in byte-reversed order. Some states hold the player's `GdeRef` and
//! facet entity.
//!
//! Each member of the table names the offsets of these session values in its body, so a server
//! writes its own values there ([`EntityMember::patched_body`]); nothing is searched at load
//! time.
//!
//! The same file holds the level's entities whose card names an asset ([`FtueLevel`], FTUE level
//! entity note): the lightning slices, the encounters, the AI spawn markers, the teleporters, the
//! loot containers and the boss's spell slices. Their `GdeRef`s are made per spawn, so a server
//! writes its own ids into their bodies ([`AssetGroup::patched_bodies`]), as it does for the ids of
//! the mobs that some bodies name. Each group also names the nearest retail event before it
//! ([`GroupCause`]), so a server can send it on the same event.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::mob_prefabs::ASSET_ID_LEN;

/// The file name prefix of the table; the client build and `.json` follow.
const FILE_PREFIX: &str = "ftue-entities-";
const FILE_SUFFIX: &str = ".json";
/// Bytes of a `GdeRef`.
pub const GDE_REF_LEN: usize = 16;
/// Bytes of a facet entity id.
pub const FACET_LEN: usize = 8;
/// Bytes of the fixed first part of a static `GdeRef`.
pub const GDE_REF_PREFIX_LEN: usize = 8;
/// Bytes of the phase id at the end of a static `GdeRef`.
pub const PHASE_SUFFIX_LEN: usize = 8;

/// The path of the table of `client_build` in `dir`.
pub fn ftue_entities_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// The bytes that end the `GdeRef` of every static entity of a level instance with phase id
/// `phase_id`: the id in byte-reversed order.
pub const fn phase_suffix(phase_id: u64) -> [u8; PHASE_SUFFIX_LEN] {
    phase_id.to_le_bytes()
}

/// The facet entity id of the entity with `gde_ref`: its first [`FACET_LEN`] bytes in reverse
/// order.
pub fn facet_of(gde_ref: &[u8; GDE_REF_LEN]) -> [u8; FACET_LEN] {
    let mut facet = [0; FACET_LEN];
    facet.copy_from_slice(&gde_ref[..FACET_LEN]);
    facet.reverse();
    facet
}

/// The session values a server writes into the bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionIds {
    /// The player's `GdeRef` (the `ClientAddEntry` id).
    pub player_gde_ref: [u8; GDE_REF_LEN],
    /// The player's facet entity id, as it is on the wire.
    pub player_facet: [u8; FACET_LEN],
    /// The phase id of the tutorial instance.
    pub phase_id: u64,
}

/// One state of an entity group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityMember {
    /// The component index.
    pub component: u64,
    /// The class type index.
    pub class: u64,
    /// The body bytes of the capture.
    pub body: Vec<u8>,
    /// Body offsets of the player's `GdeRef`.
    pub player_gde_ref_at: Vec<usize>,
    /// Body offsets of the player's facet entity id.
    pub player_facet_at: Vec<usize>,
    /// Body offsets of the phase id suffix (the end of a static `GdeRef`).
    pub phase_suffix_at: Vec<usize>,
}

impl EntityMember {
    /// The body with the session values of `ids` at the patch offsets.
    pub fn patched_body(&self, ids: &SessionIds) -> Vec<u8> {
        let mut body = self.body.clone();
        let suffix = phase_suffix(ids.phase_id);
        for (offsets, value) in [
            (&self.player_gde_ref_at, ids.player_gde_ref.as_slice()),
            (&self.player_facet_at, ids.player_facet.as_slice()),
            (&self.phase_suffix_at, suffix.as_slice()),
        ] {
            for &at in offsets {
                body[at..at + value.len()].copy_from_slice(value);
            }
        }
        body
    }
}

/// One group of an entity: its states at one time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityGroup {
    /// Milliseconds from the first record of the capture.
    pub time_ms: u64,
    /// The slot (`interestId`) of the group in the capture.
    pub interest: u16,
    /// The members, in wire order.
    pub members: Vec<EntityMember>,
}

/// One static entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtueEntity {
    /// The fixed first 8 bytes of its `GdeRef`.
    pub gde_ref_prefix: [u8; GDE_REF_PREFIX_LEN],
    /// Each time it came into view: the group with its spawn card.
    pub spawns: Vec<EntityGroup>,
    /// The later groups that change its states, in time order.
    pub changes: Vec<EntityGroup>,
    /// The times its slot was stopped.
    pub despawns: Vec<u64>,
}

impl FtueEntity {
    /// Its `GdeRef` in the level instance with phase id `phase_id`.
    pub fn gde_ref(&self, phase_id: u64) -> [u8; GDE_REF_LEN] {
        let mut gde_ref = [0; GDE_REF_LEN];
        gde_ref[..GDE_REF_PREFIX_LEN].copy_from_slice(&self.gde_ref_prefix);
        gde_ref[GDE_REF_PREFIX_LEN..].copy_from_slice(&phase_suffix(phase_id));
        gde_ref
    }
}

/// How a body holds the id of another entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdForm {
    /// Its whole `GdeRef`.
    GdeRef,
    /// Its facet entity id ([`facet_of`]).
    Facet,
}

impl IdForm {
    /// Bytes of the id in a body.
    pub const fn byte_len(self) -> usize {
        match self {
            Self::GdeRef => GDE_REF_LEN,
            Self::Facet => FACET_LEN,
        }
    }

    /// The id, in this form, of the entity with `gde_ref`.
    pub fn bytes(self, gde_ref: &[u8; GDE_REF_LEN]) -> Vec<u8> {
        match self {
            Self::GdeRef => gde_ref.to_vec(),
            Self::Facet => facet_of(gde_ref).to_vec(),
        }
    }
}

/// The entity whose id a body holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefTarget {
    /// An entity of [`FtueLevel::assets`], by index; a card names its own entity this way.
    Asset(usize),
    /// A mob of [`FtueLevel::mobs`], by index.
    Mob(usize),
}

/// The id of an entity in a member body of an [`AssetGroup`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyRef {
    /// The member, by index in the group.
    pub member: usize,
    /// The body offset of the id.
    pub at: usize,
    /// How the id is written.
    pub form: IdForm,
    /// The entity.
    pub target: RefTarget,
}

/// A mob of the retail tutorial run, named by its first spawn card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailMob {
    /// Milliseconds from the first record of the capture to its first card.
    pub first_card_ms: u64,
    /// The `vitalsId` of its first vitals state.
    pub vitals_id: u32,
    /// The asset of its card.
    pub asset: [u8; ASSET_ID_LEN],
    /// Its `GdeRef` in the capture (a server uses its own).
    pub retail_gde_ref: [u8; GDE_REF_LEN],
}

/// A retail event that a group can follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelEvent {
    /// A client message: its type index and the bytes after its facet header (its id: the
    /// `u32` of a cinematic, cutscene or quick-time event, the `GdeRef` of a bag or trigger, the
    /// equip request; none for a cutscene end).
    Client {
        /// The message type index.
        message_type: u64,
        /// The bytes after the facet header.
        id: Vec<u8>,
    },
    /// A server message about a mob: 3916 (no longer the player's target; also sent when it
    /// dies) or 1793 (its collision is turned off when it dies).
    Mob {
        /// The message type index.
        message_type: u64,
        /// The mob, by index in [`FtueLevel::mobs`].
        mob: usize,
    },
    /// Achievement bits that the server set for the player.
    Achievements {
        /// The achievement indexes.
        bits: Vec<u32>,
    },
}

/// The nearest retail event before a group, so a server can send the group on that event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupCause {
    /// The event.
    pub event: LevelEvent,
    /// Milliseconds from the first record of the capture to the event.
    pub event_ms: u64,
    /// Milliseconds from the event to the group.
    pub delay_ms: u64,
}

/// The causes of the groups of one static entity, in the order of its spawns and changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntityCauses {
    /// One per spawn group; `None` before the first event of the capture.
    pub spawns: Vec<Option<GroupCause>>,
    /// One per change group.
    pub changes: Vec<Option<GroupCause>>,
}

/// One group of an asset entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetGroup {
    /// The time, slot and members; the members name the offsets of the player's ids and the
    /// phase id suffix.
    pub group: EntityGroup,
    /// The ids of table entities and mobs in the member bodies.
    pub refs: Vec<BodyRef>,
    /// The nearest retail event before the group.
    pub cause: Option<GroupCause>,
}

impl AssetGroup {
    /// The member bodies, in wire order, with the session values of `ids` and, at each
    /// [`BodyRef`], the id of the entity whose `GdeRef` `gde_ref_of` gives; where it gives none,
    /// the retail bytes stay.
    pub fn patched_bodies(
        &self,
        ids: &SessionIds,
        gde_ref_of: impl Fn(RefTarget) -> Option<[u8; GDE_REF_LEN]>,
    ) -> Vec<Vec<u8>> {
        let mut bodies: Vec<Vec<u8>> = self
            .group
            .members
            .iter()
            .map(|member| member.patched_body(ids))
            .collect();
        for body_ref in &self.refs {
            if let Some(gde_ref) = gde_ref_of(body_ref.target) {
                let value = body_ref.form.bytes(&gde_ref);
                bodies[body_ref.member][body_ref.at..body_ref.at + value.len()]
                    .copy_from_slice(&value);
            }
        }
        bodies
    }
}

/// An entity of the tutorial level whose card names an asset and that is not a mob (no vitals
/// state) and not the player.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetEntity {
    /// The asset id of its card.
    pub asset: [u8; ASSET_ID_LEN],
    /// The sub id after the asset id.
    pub asset_sub_id: u32,
    /// Its `GdeRef` in the capture. The whole id is made per spawn, so a server uses its own.
    pub retail_gde_ref: [u8; GDE_REF_LEN],
    /// Its position in metres (east, north, height), from the first position or ALC state of
    /// its spawns.
    pub position: Option<[f32; 3]>,
    /// Each time it came into view: the group with its spawn card.
    pub spawns: Vec<AssetGroup>,
    /// The later groups that change its states, in time order.
    pub changes: Vec<AssetGroup>,
    /// The times its slot was stopped.
    pub despawns: Vec<u64>,
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum FtueEntitiesError {
    /// The file cannot be read.
    #[error("tutorial entity table {path}: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table's JSON.
    #[error("tutorial entity table: {0}")]
    Json(#[from] serde_json::Error),
    /// A hex field is not hex or not the right length.
    #[error("tutorial entity table: entity {entity}: bad hex in {field}")]
    Hex {
        /// The entity's `GdeRef` (prefix), as written in the file.
        entity: String,
        /// The field.
        field: &'static str,
    },
    /// A patch offset does not leave room for its value in the body.
    #[error("tutorial entity table: entity {entity}: {field} offset {at} is outside the body")]
    Patch {
        /// The entity's `GdeRef` (prefix), as written in the file.
        entity: String,
        /// The field.
        field: &'static str,
        /// The offset.
        at: usize,
    },
    /// An index names no asset entity or mob of the table.
    #[error("tutorial entity table: entity {entity}: {field} index {index} is not in the table")]
    Index {
        /// The entity's `GdeRef` (prefix), as written in the file.
        entity: String,
        /// The field.
        field: &'static str,
        /// The index.
        index: usize,
    },
}

#[derive(Debug, Deserialize)]
struct RefRow {
    at: usize,
    form: IdForm,
    target: RefTarget,
}

#[derive(Debug, Deserialize)]
struct MemberRow {
    component: u64,
    class: u64,
    body: String,
    player_gde_ref_at: Vec<usize>,
    player_facet_at: Vec<usize>,
    phase_suffix_at: Vec<usize>,
    #[serde(default)]
    refs: Vec<RefRow>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EventRow {
    Client { message_type: u64, id: String },
    Mob { message_type: u64, mob: usize },
    Achievements { bits: Vec<u32> },
}

#[derive(Debug, Deserialize)]
struct CauseRow {
    event_ms: u64,
    delay_ms: u64,
    event: EventRow,
}

#[derive(Debug, Deserialize)]
struct GroupRow {
    time_ms: u64,
    interest: u16,
    members: Vec<MemberRow>,
    #[serde(default)]
    after: Option<CauseRow>,
}

#[derive(Debug, Deserialize)]
struct EntityRow {
    gde_ref_prefix: String,
    spawns: Vec<GroupRow>,
    changes: Vec<GroupRow>,
    despawns: Vec<u64>,
}

#[derive(Debug, Deserialize)]
struct AssetRow {
    asset: String,
    asset_sub_id: u32,
    retail_gde_ref: String,
    position: Option<[f32; 3]>,
    spawns: Vec<GroupRow>,
    changes: Vec<GroupRow>,
    despawns: Vec<u64>,
}

#[derive(Debug, Deserialize)]
struct MobRow {
    first_card_ms: u64,
    vitals_id: u32,
    asset: String,
    retail_gde_ref: String,
}

#[derive(Debug, Deserialize)]
struct TableRow {
    level: String,
    entities: Vec<EntityRow>,
    #[serde(default)]
    asset_entities: Vec<AssetRow>,
    #[serde(default)]
    mobs: Vec<MobRow>,
}

/// The static entities of the tutorial level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FtueEntities {
    /// The level folder the table was made in.
    pub level: String,
    /// The entities, by first spawn time.
    pub entities: Vec<FtueEntity>,
}

impl FtueEntities {
    /// Loads the table of `client_build` from `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`FtueEntitiesError`] when the file is missing or bad.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, FtueEntitiesError> {
        Self::parse(&read_table(dir, client_build)?)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`FtueEntitiesError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, FtueEntitiesError> {
        FtueLevel::parse(text).map(|level| level.statics)
    }

    /// The entity whose `GdeRef` starts with `prefix`.
    pub fn entity(&self, prefix: &[u8; GDE_REF_PREFIX_LEN]) -> Option<&FtueEntity> {
        self.entities
            .iter()
            .find(|entity| &entity.gde_ref_prefix == prefix)
    }
}

/// All entities of the tutorial level in the table: the static ones, the ones whose card names
/// an asset, the retail mobs they name, and the causes of their groups.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FtueLevel {
    /// The static entities.
    pub statics: FtueEntities,
    /// The causes of the static entities' groups, one per entity of `statics`.
    pub static_causes: Vec<EntityCauses>,
    /// The entities whose card names an asset, by first spawn time.
    pub assets: Vec<AssetEntity>,
    /// The retail mobs that bodies and causes name, by first card time.
    pub mobs: Vec<RetailMob>,
}

impl FtueLevel {
    /// Loads the table of `client_build` from `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`FtueEntitiesError`] when the file is missing or bad.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, FtueEntitiesError> {
        Self::parse(&read_table(dir, client_build)?)
    }

    /// The table in `text`; a table without asset entities and mobs gives none.
    ///
    /// # Errors
    ///
    /// Returns [`FtueEntitiesError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, FtueEntitiesError> {
        let table: TableRow = serde_json::from_str(text)?;
        let counts = Counts {
            assets: table.asset_entities.len(),
            mobs: table.mobs.len(),
        };
        let (entities, static_causes) = table
            .entities
            .into_iter()
            .map(|row| entity_of(row, counts))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip();
        Ok(Self {
            statics: FtueEntities {
                level: table.level,
                entities,
            },
            static_causes,
            assets: table
                .asset_entities
                .into_iter()
                .map(|row| asset_of(row, counts))
                .collect::<Result<_, _>>()?,
            mobs: table
                .mobs
                .into_iter()
                .map(mob_of)
                .collect::<Result<_, _>>()?,
        })
    }
}

fn read_table(dir: &Path, client_build: &str) -> Result<String, FtueEntitiesError> {
    let path = ftue_entities_path(dir, client_build);
    std::fs::read_to_string(&path).map_err(|source| FtueEntitiesError::Read { path, source })
}

/// The number of asset entities and mobs in the table, for the index checks.
#[derive(Debug, Clone, Copy)]
struct Counts {
    assets: usize,
    mobs: usize,
}

fn hex_array<const N: usize>(
    text: &str,
    entity: &str,
    field: &'static str,
) -> Result<[u8; N], FtueEntitiesError> {
    hex::decode(text)
        .ok()
        .and_then(|bytes| <[u8; N]>::try_from(bytes).ok())
        .ok_or_else(|| FtueEntitiesError::Hex {
            entity: entity.to_owned(),
            field,
        })
}

fn entity_of(
    row: EntityRow,
    counts: Counts,
) -> Result<(FtueEntity, EntityCauses), FtueEntitiesError> {
    let entity = &row.gde_ref_prefix;
    let gde_ref_prefix = hex_array(entity, entity, "gde_ref_prefix")?;
    // The tool writes refs only for asset entities: the static bodies name no other entity.
    let groups = |rows: Vec<GroupRow>| {
        rows.into_iter()
            .map(|group| group_of(entity, group, counts).map(|group| (group.group, group.cause)))
            .collect::<Result<(Vec<_>, Vec<_>), _>>()
    };
    let (spawns, spawn_causes) = groups(row.spawns)?;
    let (changes, change_causes) = groups(row.changes)?;
    Ok((
        FtueEntity {
            gde_ref_prefix,
            spawns,
            changes,
            despawns: row.despawns,
        },
        EntityCauses {
            spawns: spawn_causes,
            changes: change_causes,
        },
    ))
}

fn asset_of(row: AssetRow, counts: Counts) -> Result<AssetEntity, FtueEntitiesError> {
    let entity = &row.retail_gde_ref;
    let groups = |rows: Vec<GroupRow>| {
        rows.into_iter()
            .map(|group| group_of(entity, group, counts))
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(AssetEntity {
        asset: hex_array(&row.asset, entity, "asset")?,
        asset_sub_id: row.asset_sub_id,
        retail_gde_ref: hex_array(entity, entity, "retail_gde_ref")?,
        position: row.position,
        spawns: groups(row.spawns)?,
        changes: groups(row.changes)?,
        despawns: row.despawns,
    })
}

fn mob_of(row: MobRow) -> Result<RetailMob, FtueEntitiesError> {
    let entity = &row.retail_gde_ref;
    Ok(RetailMob {
        first_card_ms: row.first_card_ms,
        vitals_id: row.vitals_id,
        asset: hex_array(&row.asset, entity, "asset")?,
        retail_gde_ref: hex_array(entity, entity, "retail_gde_ref")?,
    })
}

fn group_of(entity: &str, row: GroupRow, counts: Counts) -> Result<AssetGroup, FtueEntitiesError> {
    let mut members = Vec::with_capacity(row.members.len());
    let mut refs = Vec::new();
    for (index, mut member_row) in row.members.into_iter().enumerate() {
        let ref_rows = std::mem::take(&mut member_row.refs);
        let member = member_of(entity, member_row)?;
        for ref_row in ref_rows {
            refs.push(ref_of(entity, index, &member, ref_row, counts)?);
        }
        members.push(member);
    }
    Ok(AssetGroup {
        group: EntityGroup {
            time_ms: row.time_ms,
            interest: row.interest,
            members,
        },
        refs,
        cause: row
            .after
            .map(|cause| cause_of(entity, cause, counts))
            .transpose()?,
    })
}

fn member_of(entity: &str, row: MemberRow) -> Result<EntityMember, FtueEntitiesError> {
    let body = hex::decode(&row.body).map_err(|_| FtueEntitiesError::Hex {
        entity: entity.to_owned(),
        field: "body",
    })?;
    for (field, offsets, len) in [
        ("player_gde_ref_at", &row.player_gde_ref_at, GDE_REF_LEN),
        ("player_facet_at", &row.player_facet_at, FACET_LEN),
        ("phase_suffix_at", &row.phase_suffix_at, PHASE_SUFFIX_LEN),
    ] {
        if let Some(&at) = offsets.iter().find(|&&at| !fits(at, len, &body)) {
            return Err(FtueEntitiesError::Patch {
                entity: entity.to_owned(),
                field,
                at,
            });
        }
    }
    Ok(EntityMember {
        component: row.component,
        class: row.class,
        body,
        player_gde_ref_at: row.player_gde_ref_at,
        player_facet_at: row.player_facet_at,
        phase_suffix_at: row.phase_suffix_at,
    })
}

/// Whether `len` bytes from `at` are in `body`.
fn fits(at: usize, len: usize, body: &[u8]) -> bool {
    at.checked_add(len).is_some_and(|end| end <= body.len())
}

fn ref_of(
    entity: &str,
    member: usize,
    body: &EntityMember,
    row: RefRow,
    counts: Counts,
) -> Result<BodyRef, FtueEntitiesError> {
    if !fits(row.at, row.form.byte_len(), &body.body) {
        return Err(FtueEntitiesError::Patch {
            entity: entity.to_owned(),
            field: "refs",
            at: row.at,
        });
    }
    let (index, count, field) = match row.target {
        RefTarget::Asset(index) => (index, counts.assets, "refs.asset"),
        RefTarget::Mob(index) => (index, counts.mobs, "refs.mob"),
    };
    check_index(entity, field, index, count)?;
    Ok(BodyRef {
        member,
        at: row.at,
        form: row.form,
        target: row.target,
    })
}

fn cause_of(entity: &str, row: CauseRow, counts: Counts) -> Result<GroupCause, FtueEntitiesError> {
    let event = match row.event {
        EventRow::Client { message_type, id } => LevelEvent::Client {
            message_type,
            id: hex::decode(&id).map_err(|_| FtueEntitiesError::Hex {
                entity: entity.to_owned(),
                field: "after.event.id",
            })?,
        },
        EventRow::Mob { message_type, mob } => {
            check_index(entity, "after.event.mob", mob, counts.mobs)?;
            LevelEvent::Mob { message_type, mob }
        }
        EventRow::Achievements { bits } => LevelEvent::Achievements { bits },
    };
    Ok(GroupCause {
        event,
        event_ms: row.event_ms,
        delay_ms: row.delay_ms,
    })
}

fn check_index(
    entity: &str,
    field: &'static str,
    index: usize,
    count: usize,
) -> Result<(), FtueEntitiesError> {
    if index < count {
        Ok(())
    } else {
        Err(FtueEntitiesError::Index {
            entity: entity.to_owned(),
            field,
            index,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIX: &str = "0102030405060708";
    const PHASE_ID: u64 = 0x1122_3344_5566_7788;
    /// A card body: slot 0, the `GdeRef` child only, a `GdeRef` whose suffix is zero.
    const CARD_BODY: &str = "01020102030405060708";
    const CARD_SUFFIX_AT: usize = 10;
    /// A state body: a slot byte, a mask, a zero player `GdeRef` and facet, two more bytes.
    const STATE_BODY: &str = "010f000000000000000000000000000000000000000000000000aabb";
    const STATE_GDE_REF_AT: usize = 2;
    const STATE_FACET_AT: usize = 18;

    fn table(card_suffix_at: usize) -> String {
        format!(
            r#"{{"level":"ftue","entities":[{{"gde_ref_prefix":"{PREFIX}",
               "spawns":[{{"time_ms":5,"interest":9,"members":[
                 {{"component":0,"class":10,"body":"{CARD_BODY}0000000000000000",
                   "player_gde_ref_at":[],"player_facet_at":[],"phase_suffix_at":[{card_suffix_at}]}},
                 {{"component":2,"class":3194,"body":"{STATE_BODY}",
                   "player_gde_ref_at":[{STATE_GDE_REF_AT}],"player_facet_at":[{STATE_FACET_AT}],
                   "phase_suffix_at":[]}}]}}],
               "changes":[],"despawns":[7]}}]}}"#
        )
    }

    fn ids() -> SessionIds {
        SessionIds {
            player_gde_ref: [0x5a; GDE_REF_LEN],
            player_facet: [0xbf; FACET_LEN],
            phase_id: PHASE_ID,
        }
    }

    #[test]
    fn a_table_loads_and_patches_the_session_values() {
        let table = FtueEntities::parse(&table(CARD_SUFFIX_AT)).expect("table");
        assert_eq!(table.level, "ftue");
        let prefix: [u8; GDE_REF_PREFIX_LEN] = hex::decode(PREFIX)
            .expect("hex")
            .try_into()
            .expect("8 bytes");
        let entity = table.entity(&prefix).expect("entity");
        assert_eq!(entity.despawns, vec![7]);
        let gde_ref = entity.gde_ref(PHASE_ID);
        assert_eq!(gde_ref[..GDE_REF_PREFIX_LEN], prefix);
        assert_eq!(gde_ref[GDE_REF_PREFIX_LEN..], PHASE_ID.to_le_bytes());

        let spawn = &entity.spawns[0];
        assert_eq!((spawn.time_ms, spawn.interest), (5, 9));
        let card = spawn.members[0].patched_body(&ids());
        assert_eq!(card[CARD_SUFFIX_AT..], phase_suffix(PHASE_ID));
        assert_eq!(
            card[..CARD_SUFFIX_AT],
            spawn.members[0].body[..CARD_SUFFIX_AT]
        );

        let state = spawn.members[1].patched_body(&ids());
        let facet_end = STATE_FACET_AT + FACET_LEN;
        assert_eq!(state[STATE_GDE_REF_AT..STATE_FACET_AT], [0x5a; GDE_REF_LEN]);
        assert_eq!(state[STATE_FACET_AT..facet_end], [0xbf; FACET_LEN]);
        assert_eq!(state[facet_end..], [0xaa, 0xbb]);
    }

    #[test]
    fn the_phase_suffix_is_the_byte_reversed_phase_id() {
        assert_eq!(
            phase_suffix(PHASE_ID),
            [0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]
        );
    }

    #[test]
    fn a_patch_offset_outside_the_body_is_refused() {
        let past_end = CARD_SUFFIX_AT + 1;
        assert!(matches!(
            FtueEntities::parse(&table(past_end)),
            Err(FtueEntitiesError::Patch {
                field: "phase_suffix_at",
                ..
            })
        ));
    }

    #[test]
    fn bad_hex_is_refused() {
        let text = table(CARD_SUFFIX_AT).replace(PREFIX, "xyz");
        assert!(matches!(
            FtueEntities::parse(&text),
            Err(FtueEntitiesError::Hex {
                field: "gde_ref_prefix",
                ..
            })
        ));
    }

    #[test]
    fn the_path_names_the_build() {
        assert_eq!(
            ftue_entities_path(Path::new("content"), "123"),
            Path::new("content").join("ftue-entities-123.json")
        );
    }

    const ASSET: &str = "e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1";
    const RETAIL_ENCOUNTER: &str = "c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1";
    const RETAIL_MOB: &str = "d1d2d3d4d5d6d7d8d9dadbdcdddedfd0";
    /// A card with an asset: slot 0, mask 3, the asset, sub id 2, the retail `GdeRef`.
    const ASSET_CARD_HEAD: &str = "0103e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e100000002";
    const ASSET_CARD_GDE_REF_AT: usize = 22;
    /// A body that holds the mob's `GdeRef` at 2 and its facet entity at 18.
    const MOB_IDS_BODY: &str = "0140d1d2d3d4d5d6d7d8d9dadbdcdddedfd0d8d7d6d5d4d3d2d1";
    const MOB_GDE_REF_AT: usize = 2;
    const MOB_FACET_AT: usize = 18;
    const MOB_VITALS_ID: u32 = 7;
    const MOB_CARD_MS: u64 = 30;
    const ASSET_DESPAWN_MS: u64 = 60;
    const ASSET_POSITION: [f32; 3] = [1.0, 2.0, 3.0];
    const EVENT_MS: u64 = 40;
    const DELAY_MS: u64 = 2;
    const QTE_TYPE: u64 = 6638;
    const DEATH_TYPE: u64 = 1793;

    /// The table of `table(CARD_SUFFIX_AT)` with a cause on its spawn, an asset entity whose
    /// members name it and the mob, and the mob; `card_ref_at` and `mob` place the refs.
    fn level_table(card_ref_at: usize, mob: usize) -> String {
        let statics = table(CARD_SUFFIX_AT);
        let statics = statics.replacen(
            r#""interest":9,"#,
            &format!(
                r#""interest":9,"after":{{"event_ms":{EVENT_MS},"delay_ms":{DELAY_MS},
                   "event":{{"client":{{"message_type":{QTE_TYPE},"id":"f9d80a82"}}}}}},"#
            ),
            1,
        );
        let assets = format!(
            r#","asset_entities":[{{"asset":"{ASSET}","asset_sub_id":2,
                 "retail_gde_ref":"{RETAIL_ENCOUNTER}","position":{ASSET_POSITION:?},
                 "spawns":[{{"time_ms":50,"interest":4,"members":[
                   {{"component":0,"class":10,"body":"{ASSET_CARD_HEAD}{RETAIL_ENCOUNTER}",
                     "player_gde_ref_at":[],"player_facet_at":[],"phase_suffix_at":[],
                     "refs":[{{"at":{card_ref_at},"form":"gde_ref","target":{{"asset":0}}}}]}},
                   {{"component":1,"class":2912,"body":"{MOB_IDS_BODY}",
                     "player_gde_ref_at":[],"player_facet_at":[],"phase_suffix_at":[],
                     "refs":[{{"at":{MOB_GDE_REF_AT},"form":"gde_ref","target":{{"mob":{mob}}}}},
                             {{"at":{MOB_FACET_AT},"form":"facet","target":{{"mob":0}}}}]}}],
                   "after":{{"event_ms":{EVENT_MS},"delay_ms":{DELAY_MS},
                     "event":{{"mob":{{"message_type":{DEATH_TYPE},"mob":0}}}}}}}}],
                 "changes":[],"despawns":[{ASSET_DESPAWN_MS}]}}],
               "mobs":[{{"first_card_ms":{MOB_CARD_MS},"vitals_id":{MOB_VITALS_ID},"asset":"{ASSET}",
                 "retail_gde_ref":"{RETAIL_MOB}"}}]}}"#
        );
        let end = statics.rfind('}').expect("table end");
        format!("{}{assets}", &statics[..end])
    }

    #[test]
    fn a_table_without_asset_entities_loads_as_a_level() {
        let level = FtueLevel::parse(&table(CARD_SUFFIX_AT)).expect("level");
        assert!(level.assets.is_empty() && level.mobs.is_empty());
        assert_eq!(
            level.statics,
            FtueEntities::parse(&table(CARD_SUFFIX_AT)).expect("table")
        );
        assert_eq!(
            level.static_causes,
            vec![EntityCauses {
                spawns: vec![None],
                changes: Vec::new(),
            }]
        );
    }

    #[test]
    fn asset_entities_take_the_server_ids_and_name_their_causes() {
        const SERVER_ENCOUNTER: [u8; GDE_REF_LEN] = [0x91; GDE_REF_LEN];
        const SERVER_MOB: [u8; GDE_REF_LEN] = [0x92; GDE_REF_LEN];
        let level = FtueLevel::parse(&level_table(ASSET_CARD_GDE_REF_AT, 0)).expect("level");
        assert_eq!(
            level.static_causes[0].spawns[0],
            Some(GroupCause {
                event: LevelEvent::Client {
                    message_type: QTE_TYPE,
                    id: vec![0xf9, 0xd8, 0x0a, 0x82],
                },
                event_ms: EVENT_MS,
                delay_ms: DELAY_MS,
            })
        );
        let mob = level.mobs[0];
        assert_eq!(
            (mob.first_card_ms, mob.vitals_id),
            (MOB_CARD_MS, MOB_VITALS_ID)
        );
        let encounter = &level.assets[0];
        assert_eq!(encounter.position, Some(ASSET_POSITION));
        assert_eq!(encounter.despawns, vec![ASSET_DESPAWN_MS]);
        let spawn = &encounter.spawns[0];
        assert_eq!(
            spawn.cause.as_ref().map(|cause| &cause.event),
            Some(&LevelEvent::Mob {
                message_type: DEATH_TYPE,
                mob: 0,
            })
        );
        let bodies = spawn.patched_bodies(&ids(), |target| match target {
            RefTarget::Asset(0) => Some(SERVER_ENCOUNTER),
            RefTarget::Mob(0) => Some(SERVER_MOB),
            RefTarget::Asset(_) | RefTarget::Mob(_) => None,
        });
        assert_eq!(bodies[0][ASSET_CARD_GDE_REF_AT..], SERVER_ENCOUNTER);
        assert_eq!(bodies[1][MOB_GDE_REF_AT..MOB_FACET_AT], SERVER_MOB);
        assert_eq!(bodies[1][MOB_FACET_AT..], facet_of(&SERVER_MOB));
        let retail = spawn.patched_bodies(&ids(), |_| None);
        assert_eq!(retail[1], spawn.group.members[1].body);
    }

    #[test]
    fn bad_refs_are_refused() {
        let past_end = ASSET_CARD_GDE_REF_AT + 1;
        assert!(matches!(
            FtueLevel::parse(&level_table(past_end, 0)),
            Err(FtueEntitiesError::Patch { field: "refs", .. })
        ));
        assert!(matches!(
            FtueLevel::parse(&level_table(ASSET_CARD_GDE_REF_AT, 1)),
            Err(FtueEntitiesError::Index {
                field: "refs.mob",
                index: 1,
                ..
            })
        ));
    }

    #[test]
    fn a_facet_entity_is_the_reversed_gde_ref_start() {
        let gde_ref: [u8; GDE_REF_LEN] = hex::decode(RETAIL_MOB)
            .expect("hex")
            .try_into()
            .expect("16 bytes");
        assert_eq!(
            facet_of(&gde_ref),
            [0xd8, 0xd7, 0xd6, 0xd5, 0xd4, 0xd3, 0xd2, 0xd1]
        );
        assert_eq!(IdForm::Facet.bytes(&gde_ref), facet_of(&gde_ref).to_vec());
        assert_eq!(IdForm::GdeRef.byte_len(), GDE_REF_LEN);
    }
}
