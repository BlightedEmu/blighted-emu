// SPDX-License-Identifier: AGPL-3.0-or-later
//! The replicated states of wars and sieges: the war data of the world entity (1739), the siege
//! warfare data (4103), a capture point (333), a siege turret (4276), the player's builder
//! (2270) and a land claim entity (62). Layouts from the client's member table
//! ([`crate::member_layouts`]); the meaning of each field from the retail bodies of the capture
//! index (wars and sieges study 2026-11-12: 5,917 war data, 25,038 siege warfare, 65,674 capture
//! point, 29,093 turret, 2,510 builder and 81 land claim bodies, every one read by these readers
//! and written back to the same bytes).
//!
//! **War data** (1739, component 5 of the slot-2 world entity, Observed in 837 ledgers): child 0
//! is every war and invasion of the world ([`WarEntry`]), a list whose delta removes a war 180 s
//! after its result (`Playthrough_Part_21` 1793819069152); child 1 the siege window changes of
//! the territories ([`ScheduleAdjustment`]); child 2 the influence race of each territory
//! ([`InfluenceRace`]). In `arvaysa_ES_attacker_war_45vs45` the war of territory 8 shows phase
//! [`WAR_PHASE_SCHEDULED`] with its start time (1783536276201), phase [`WAR_PHASE_RUNNING`] with
//! its end time 1,800 s later and the war game mode at the start (1783537199924), and phase
//! [`WAR_PHASE_RESOLVED`] with the winner's company and the result's end 180 s later
//! (1783538416596).

use crate::combat::{read_string, write_string};
use crate::keyed_list::{IndexedList, KeyedList};
use crate::slots::{
    read_children, read_slot_0, read_slot_byte, write_children, write_known, write_known_children,
};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `MB::WarDataComponentReplicatedState`.
pub const WAR_DATA_TYPE_INDEX: u64 = 1739;
/// The component of the war data in the slot-2 world entity (Observed in every retail group).
pub const WAR_DATA_COMPONENT: u64 = 5;
/// Type index of `MB::SiegeWarfareDataReplicatedState`.
pub const SIEGE_WARFARE_DATA_TYPE_INDEX: u64 = 4103;
/// Type index of `Javelin::CapturePointReplicatedState`.
pub const CAPTURE_POINT_TYPE_INDEX: u64 = 333;
/// Type index of `MB::TurretReplicatedState`.
pub const TURRET_TYPE_INDEX: u64 = 4276;
/// Type index of `MB::BuilderComponentReplicatedState`.
pub const BUILDER_TYPE_INDEX: u64 = 2270;
/// The player component of the builder state (Observed: component 23 of the own player in every
/// retail camp ledger, `12a_msq_12_camptutorial`, `Hive_camping`, `Playthrough_Part_05`).
pub const BUILDER_STATE_COMPONENT: u64 = 23;
/// Type index of `MB::LandClaimComponentReplicatedState`.
pub const LAND_CLAIM_TYPE_INDEX: u64 = 62;

/// War phase: the war waits for its start time (Observed: the time field is the start).
pub const WAR_PHASE_SCHEDULED: u32 = 0;
/// War phase: the war runs (Observed: the time field is its end, the start + 1,800 s for a war
/// and + 1,500 s for an invasion).
pub const WAR_PHASE_RUNNING: u32 = 2;
/// War phase: the war has a result (Observed: the winner's company is set for a won war, and the
/// time field is the result's end, 180 s on).
pub const WAR_PHASE_RESOLVED: u32 = 4;
/// War kind: a company war (Observed: an attacker company and both sides' factions).
pub const WAR_KIND_WAR: u32 = 1;
/// War kind: an invasion (Observed: no attacker company, attacker faction 0).
pub const WAR_KIND_INVASION: u32 = 2;
/// The value of an empty side ([`WarSide::key`]; Observed on every invasion's attacker side).
pub const NO_SIDE_KEY: u64 = 0xffff_ffff;
/// The `instance_state` of a running war (Observed: 2 from the start, 0 before).
pub const INSTANCE_RUNNING: u8 = 2;
/// The `ready` byte of every retail war entry (Observed: always 1; meaning Unknown).
pub const WAR_READY: u8 = 1;
/// Capture point kind: open, held by the defenders; the attackers may take it (Observed in the
/// 333 and 4103 bodies of the six retail war forts: the outer points at the start, the claim
/// point once the outer points are taken).
pub const CAPTURE_POINT_OPEN: u8 = 4;
/// Capture point kind: a capture runs (Observed: attackers in the point; the hot data rises).
pub const CAPTURE_POINT_CAPTURING: u8 = 5;
/// Capture point kind: locked (Observed: the claim point from the war's first state until the
/// outer points are taken, and every point the attackers took).
pub const CAPTURE_POINT_LOCKED: u8 = 6;
/// The last byte of a capture point's cold data (Observed: 1 in every retail body).
pub const CAPTURE_POINT_FLAG: u8 = 1;

const FIELD: &str = "war state";
/// The most entries of an id or byte array of these states (a body is at most `0x30000` bytes).
const MAX_ARRAY: u64 = 0x2_FFFF;
const WAR_DATA_CHILDREN: u8 = 3;
const WARS_CHILD: u8 = 0;
const SCHEDULE_CHILD: u8 = 1;
const INFLUENCE_CHILD: u8 = 2;
const SIEGE_CHILDREN: u8 = 6;
const CAPTURE_POINT_CHILDREN: u8 = 2;
const TURRET_CHILDREN: u8 = 7;
const BUILDER_SLOTS: u8 = 2;
const BUILDER_STATE_SLOT: u8 = 0x01;
const BUILDER_PLACED_SLOT: u8 = 0x02;
const ONE_CHILD: u8 = 1;
const FIRST_CHILD: u16 = 1;
/// The GDE reference of a turret's interactor: a 16-byte id and 8 bytes (Observed: all zero
/// with `ffffffff` last for a turret no player uses).
pub const GDE_REF_LEN: usize = 24;

fn read_ids(reader: &mut Reader<'_>) -> Result<Vec<[u8; RAW_ID_LEN]>, MessageError> {
    let count = read_array_count(reader)?;
    (0..count).map(|_| reader.raw_id()).collect()
}

fn write_ids(writer: &mut Writer, ids: &[[u8; RAW_ID_LEN]]) -> Result<(), MessageError> {
    writer.prefixed(ids.len() as u64)?;
    ids.iter().for_each(|id| {
        writer.bytes(id);
    });
    Ok(())
}

fn read_array_count(reader: &mut Reader<'_>) -> Result<u64, MessageError> {
    let count = reader.prefixed()?;
    if count > MAX_ARRAY || count > reader.remaining() as u64 {
        return Err(MessageError::CountTooLarge {
            count,
            limit: MAX_ARRAY.min(reader.remaining() as u64),
        });
    }
    Ok(count)
}

/// One side of a war: its raid (Observed: 10 group ids and a raid id on each side of a war;
/// the attacker side of an invasion is empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarSide {
    /// A number of the side (Observed: the defender's is the attacker's + 1, as 9428 and 9429;
    /// 0 for an empty side).
    pub number: u64,
    /// The ids of its raid's groups.
    pub groups: Vec<[u8; RAW_ID_LEN]>,
    /// The id of its raid.
    pub raid: [u8; RAW_ID_LEN],
    /// A key of the side (meaning Unknown; [`NO_SIDE_KEY`] for an empty side).
    pub key: u64,
}

impl WarSide {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            number: reader.u64()?,
            groups: read_ids(reader)?,
            raid: reader.raw_id()?,
            key: reader.u64()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.u64(self.number);
        write_ids(writer, &self.groups)?;
        writer.bytes(&self.raid).u64(self.key);
        Ok(())
    }
}

/// The run of a war: phase, times and game mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarStatus {
    /// The company that won (Observed: set at [`WAR_PHASE_RESOLVED`] for a won war; zero
    /// before, and for a lost invasion).
    pub winner: [u8; RAW_ID_LEN],
    /// A byte (Observed: 0 or 2, 2 with `value` 1,500,000; meaning Unknown).
    pub flag: u8,
    /// A value (Observed: 1,500,000, 750,000 or 150,000 for a war, 0 for an invasion; meaning
    /// Unknown).
    pub value: u32,
    /// The state of the war's instance ([`INSTANCE_RUNNING`] once it runs, 0 before).
    pub instance_state: u8,
    /// A byte ([`WAR_READY`] in every retail entry).
    pub ready: u8,
    /// The phase ([`WAR_PHASE_SCHEDULED`], [`WAR_PHASE_RUNNING`], [`WAR_PHASE_RESOLVED`]).
    pub phase: u32,
    /// The time of the phase, Unix ns: the start, the end, or the result's end.
    pub time_ns: u64,
    /// The CRC-32 of the war's game mode (Observed: set from the start).
    pub game_mode: u32,
    /// The id of the war's instance (Observed: set from the start).
    pub instance: [u8; RAW_ID_LEN],
    /// A second id of the instance (Observed: the same in the wars of one capture; meaning
    /// Unknown).
    pub host: [u8; RAW_ID_LEN],
}

impl WarStatus {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            winner: reader.raw_id()?,
            flag: reader.u8()?,
            value: reader.u32()?,
            instance_state: reader.u8()?,
            ready: reader.u8()?,
            phase: reader.u32()?,
            time_ns: reader.u64()?,
            game_mode: reader.u32()?,
            instance: reader.raw_id()?,
            host: reader.raw_id()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .bytes(&self.winner)
            .u8(self.flag)
            .u32(self.value)
            .u8(self.instance_state)
            .u8(self.ready)
            .u32(self.phase)
            .u64(self.time_ns)
            .u32(self.game_mode)
            .bytes(&self.instance)
            .bytes(&self.host);
    }
}

/// One war or invasion of the war data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarEntry {
    /// The attacking company (zero for an invasion).
    pub attacker_company: [u8; RAW_ID_LEN],
    /// The defending company (the territory's owner).
    pub defender_company: [u8; RAW_ID_LEN],
    /// The attacker side.
    pub attacker: WarSide,
    /// The defender side.
    pub defender: WarSide,
    /// The attacker's faction (0 for an invasion).
    pub attacker_faction: u8,
    /// The defender's faction.
    pub defender_faction: u8,
    /// The territory id.
    pub territory: u16,
    /// [`WAR_KIND_WAR`] or [`WAR_KIND_INVASION`].
    pub kind: u32,
    /// The war's id.
    pub war: [u8; RAW_ID_LEN],
    /// A number (Observed: 0 in every retail entry; meaning Unknown).
    pub extra: u64,
    /// Its run.
    pub status: WarStatus,
}

impl WarEntry {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            attacker_company: reader.raw_id()?,
            defender_company: reader.raw_id()?,
            attacker: WarSide::read(reader)?,
            defender: WarSide::read(reader)?,
            attacker_faction: reader.u8()?,
            defender_faction: reader.u8()?,
            territory: reader.u16()?,
            kind: reader.u32()?,
            war: reader.raw_id()?,
            extra: reader.u64()?,
            status: WarStatus::read(reader)?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer
            .bytes(&self.attacker_company)
            .bytes(&self.defender_company);
        self.attacker.write(writer)?;
        self.defender.write(writer)?;
        writer
            .u8(self.attacker_faction)
            .u8(self.defender_faction)
            .u16(self.territory)
            .u32(self.kind)
            .bytes(&self.war)
            .u64(self.extra);
        self.status.write(writer);
        Ok(())
    }
}

/// A change of a territory's siege window (Observed: one entry per territory; the call 3615
/// `RequestSetGuildSiegeWindow` sets the window).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleAdjustment {
    /// The company that changed it (Observed: the owner).
    pub company: [u8; RAW_ID_LEN],
    /// A value (Observed: 1 or 2; meaning Unknown).
    pub first: u32,
    /// The territory id.
    pub territory: u16,
    /// A value (Observed: 1 or 2; meaning Unknown).
    pub second: u32,
    /// When it changed, Unix ns.
    pub time_ns: u64,
}

impl ScheduleAdjustment {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            company: reader.raw_id()?,
            first: reader.u32()?,
            territory: reader.u16()?,
            second: reader.u32()?,
            time_ns: reader.u64()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .bytes(&self.company)
            .u32(self.first)
            .u16(self.territory)
            .u32(self.second)
            .u64(self.time_ns);
    }
}

/// The influence race of a territory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfluenceRace {
    /// A time, Unix ns (meaning Unknown: hours to days before the capture).
    pub time_ns: u64,
    /// The territory id.
    pub territory: u16,
    /// Four bytes (Observed: 0 to 3 each; meaning Unknown).
    pub flags: [u8; 4],
    /// The influence of each faction (Observed: 3 values from 0 to 73).
    pub influence: Vec<u8>,
}

impl InfluenceRace {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let time_ns = reader.u64()?;
        let territory = reader.u16()?;
        let flags = reader.array()?;
        let count = read_array_count(reader)?;
        let influence = (0..count).map(|_| reader.u8()).collect::<Result<_, _>>()?;
        Ok(Self {
            time_ns,
            territory,
            flags,
            influence,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer
            .u64(self.time_ns)
            .u16(self.territory)
            .bytes(&self.flags)
            .prefixed(self.influence.len() as u64)?
            .bytes(&self.influence);
        Ok(())
    }
}

/// The war data state; a `None` child is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WarDataState {
    /// Child 0, `m_warData`.
    pub wars: Option<IndexedList<WarEntry>>,
    /// Child 1, `m_warScheduleAdjustments`, by territory id.
    pub schedule: Option<KeyedList<u16, ScheduleAdjustment>>,
    /// Child 2, `m_influenceRaceData`, by territory id.
    pub influence: Option<KeyedList<u16, InfluenceRace>>,
}

impl WarDataState {
    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match the layout.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, WAR_DATA_CHILDREN, |reader, child| {
            match child {
                WARS_CHILD => state.wars = Some(IndexedList::read(reader, WarEntry::read)?),
                SCHEDULE_CHILD => {
                    state.schedule = Some(KeyedList::read(reader, ScheduleAdjustment::read)?);
                }
                _ => state.influence = Some(KeyedList::read(reader, InfluenceRace::read)?),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a list does not write.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let children = (u16::from(self.wars.is_some()) << WARS_CHILD)
            | (u16::from(self.schedule.is_some()) << SCHEDULE_CHILD)
            | (u16::from(self.influence.is_some()) << INFLUENCE_CHILD);
        write_slot_0(
            writer,
            children,
            WAR_DATA_CHILDREN,
            |writer, child| match child {
                WARS_CHILD => self.wars.as_ref().map_or(Ok(()), |wars| {
                    wars.write(writer, |writer, war| war.write(writer))
                }),
                SCHEDULE_CHILD => self.schedule.as_ref().map_or(Ok(()), |schedule| {
                    schedule.write(writer, |writer, entry| {
                        entry.write(writer);
                        Ok(())
                    })
                }),
                _ => self.influence.as_ref().map_or(Ok(()), |influence| {
                    influence.write(writer, |writer, race| race.write(writer))
                }),
            },
        )
    }
}

/// Writes the slot byte and, when `children` sets a child, slot 0 with a child writer that may
/// fail.
fn write_slot_0(
    writer: &mut Writer,
    children: u16,
    count: u8,
    write_child: impl FnMut(&mut Writer, u8) -> Result<(), MessageError>,
) -> Result<(), MessageError> {
    if children == 0 {
        writer.u8(0);
        return Ok(());
    }
    writer.u8(crate::slots::SLOT_0);
    write_children(writer, children, count, write_child)
}

/// A place in the world: three `f32` (Observed: X, Y, Z of a capture point and a fort
/// structure, near the fort of the war's territory).
pub type Place = [f32; 3];

fn read_place(reader: &mut Reader<'_>) -> Result<Place, MessageError> {
    Ok([reader.f32()?, reader.f32()?, reader.f32()?])
}

fn write_place(writer: &mut Writer, place: &Place) {
    place.iter().for_each(|axis| {
        writer.f32(*axis);
    });
}

/// The cold data of a capture point (333 child 0, and each entry of 4103 child 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapturePointCold {
    /// Its place.
    pub place: Place,
    /// Its number (Observed: 1 to 4 within a fort).
    pub number: u32,
    /// Its kind ([`CAPTURE_POINT_OPEN`], [`CAPTURE_POINT_CAPTURING`], [`CAPTURE_POINT_LOCKED`]).
    pub kind: u8,
    /// A number (Observed: 0 before the war's start, from the start the attacker side's number
    /// of the war data, 9428 in `arvaysa_ES_attacker_war_45vs45`).
    pub value: u64,
    /// A byte ([`CAPTURE_POINT_FLAG`]).
    pub flag: u8,
}

impl CapturePointCold {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            place: read_place(reader)?,
            number: reader.u32()?,
            kind: reader.u8()?,
            value: reader.u64()?,
            flag: reader.u8()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        write_place(writer, &self.place);
        writer
            .u32(self.number)
            .u8(self.kind)
            .u64(self.value)
            .u8(self.flag);
    }
}

/// A capture point state (333); a `None` child is not sent.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CapturePointState {
    /// Child 0, `coldData`.
    pub cold: Option<CapturePointCold>,
    /// Child 1, `hotData`: the capture progress (Observed: it rises while attackers take the
    /// point, and the capture ends after 99).
    pub hot: Option<u8>,
}

impl CapturePointState {
    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match the layout.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, CAPTURE_POINT_CHILDREN, |reader, child| {
            if child == 0 {
                state.cold = Some(CapturePointCold::read(reader)?);
            } else {
                state.hot = Some(reader.u8()?);
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state.
    pub fn write(&self, writer: &mut Writer) {
        let children = u16::from(self.cold.is_some()) | (u16::from(self.hot.is_some()) << 1);
        write_known(writer, children, CAPTURE_POINT_CHILDREN, |writer, child| {
            if child == 0 {
                self.cold.iter().for_each(|cold| cold.write(writer));
            } else {
                self.hot.iter().for_each(|hot| {
                    writer.u8(*hot);
                });
            }
        });
    }
}

/// A major structure of a fort (4103 child 2: Observed the gates and walls of the fort).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FortStructure {
    /// Its place.
    pub place: Place,
    /// Its kind (meaning Unknown).
    pub kind: u32,
    /// Its health in percent (Inferred: 100 for a whole structure).
    pub health: u8,
    /// A byte (meaning Unknown).
    pub flag: u8,
}

/// A siege weapon of a fort (4103 child 3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SiegeWeapon {
    /// Its place, as 12 bytes (Observed: three big-endian `f32`).
    pub place: [u8; 12],
    /// Its kind (Observed: 1, 3 and 4).
    pub kind: u32,
    /// Its health in percent (Observed: 100 before the war).
    pub health: u8,
    /// A byte (Observed: 0; meaning Unknown).
    pub flag: u8,
}

/// The siege warfare data (4103); a `None` child is not sent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SiegeWarfareDataState {
    /// Child 0, `capturePointStatesColdData`, by capture point entity.
    pub points_cold: Option<KeyedList<u64, CapturePointCold>>,
    /// Child 1, `capturePointStatesHotData`, by capture point entity.
    pub points_hot: Option<KeyedList<u64, u8>>,
    /// Child 2, `fortMajorStructureStates`, by structure entity.
    pub structures: Option<KeyedList<u64, FortStructure>>,
    /// Child 3, `siegeWeaponStates`, by weapon entity.
    pub weapons: Option<KeyedList<u64, SiegeWeapon>>,
    /// Child 4, `numAttackerBuildablesBoundByTeamLimit`, by buildable kind.
    pub attacker_buildables: Option<KeyedList<u32, u8>>,
    /// Child 5, `numDefenderBuildablesBoundByTeamLimit`, by buildable kind.
    pub defender_buildables: Option<KeyedList<u32, u8>>,
}

fn read_byte(reader: &mut Reader<'_>) -> Result<u8, MessageError> {
    reader.u8()
}

fn write_byte(writer: &mut Writer, value: &u8) -> Result<(), MessageError> {
    writer.u8(*value);
    Ok(())
}

impl SiegeWarfareDataState {
    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match the layout.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, SIEGE_CHILDREN, |reader, child| {
            match child {
                0 => state.points_cold = Some(KeyedList::read(reader, CapturePointCold::read)?),
                1 => state.points_hot = Some(KeyedList::read(reader, read_byte)?),
                2 => {
                    state.structures = Some(KeyedList::read(reader, |reader| {
                        Ok(FortStructure {
                            place: read_place(reader)?,
                            kind: reader.u32()?,
                            health: reader.u8()?,
                            flag: reader.u8()?,
                        })
                    })?);
                }
                3 => {
                    state.weapons = Some(KeyedList::read(reader, |reader| {
                        Ok(SiegeWeapon {
                            place: reader.array()?,
                            kind: reader.u32()?,
                            health: reader.u8()?,
                            flag: reader.u8()?,
                        })
                    })?);
                }
                4 => state.attacker_buildables = Some(KeyedList::read(reader, read_byte)?),
                _ => state.defender_buildables = Some(KeyedList::read(reader, read_byte)?),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a list does not write.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let present = [
            self.points_cold.is_some(),
            self.points_hot.is_some(),
            self.structures.is_some(),
            self.weapons.is_some(),
            self.attacker_buildables.is_some(),
            self.defender_buildables.is_some(),
        ];
        let children = crate::slots::children_mask(&present);
        write_slot_0(
            writer,
            children,
            SIEGE_CHILDREN,
            |writer, child| match child {
                0 => self.points_cold.as_ref().map_or(Ok(()), |list| {
                    list.write(writer, |writer, point| {
                        point.write(writer);
                        Ok(())
                    })
                }),
                1 => self
                    .points_hot
                    .as_ref()
                    .map_or(Ok(()), |list| list.write(writer, write_byte)),
                2 => self.structures.as_ref().map_or(Ok(()), |list| {
                    list.write(writer, |writer, structure| {
                        write_place(writer, &structure.place);
                        writer
                            .u32(structure.kind)
                            .u8(structure.health)
                            .u8(structure.flag);
                        Ok(())
                    })
                }),
                3 => self.weapons.as_ref().map_or(Ok(()), |list| {
                    list.write(writer, |writer, weapon| {
                        writer
                            .bytes(&weapon.place)
                            .u32(weapon.kind)
                            .u8(weapon.health)
                            .u8(weapon.flag);
                        Ok(())
                    })
                }),
                4 => self
                    .attacker_buildables
                    .as_ref()
                    .map_or(Ok(()), |list| list.write(writer, write_byte)),
                _ => self
                    .defender_buildables
                    .as_ref()
                    .map_or(Ok(()), |list| list.write(writer, write_byte)),
            },
        )
    }
}

/// A siege turret state (4276); a `None` child is not sent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TurretState {
    /// Child 0, `yaw` (packed).
    pub yaw: Option<u16>,
    /// Child 1, `pitch` (packed).
    pub pitch: Option<u16>,
    /// Child 2, `heat` (packed; Observed: rises while a player fires).
    pub heat: Option<u16>,
    /// Child 3, `initialOrientation`: a packed turn, its flag byte and its bytes (Observed: in
    /// the first bodies of 25 turret ledgers, flag `46`, `47` or `66` and one byte, or flag `40`
    /// and three bytes).
    pub orientation: Option<PackedTurn>,
    /// Child 4, `interactorGDERef`: the player who uses it.
    pub interactor: Option<[u8; GDE_REF_LEN]>,
    /// Child 5, `isOverheated`.
    pub overheated: Option<bool>,
    /// Child 6, `isInteractionBlocked`.
    pub blocked: Option<bool>,
}

/// A packed turn (`FUN_14087aa70`): a flag byte and 0 to 3 bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PackedTurn {
    /// The flag byte.
    pub flag: u8,
    /// The bytes after it; the first `len` are used.
    pub bytes: [u8; TURN_FULL_LEN],
}

/// The longest packed turn after its flag.
const TURN_FULL_LEN: usize = 3;
const TURN_SHORT_BIT: u8 = 0x02;
const TURN_DROP_BITS: [u8; 2] = [0x04, 0x08];
const TURN_LAST_DROP_BIT: u8 = 0x10;

impl PackedTurn {
    /// The bytes after the flag (`FUN_14087aa70`: 3, 2 with bit 1; one fewer for each of bits 2
    /// and 3, and for bit 4 while any are left).
    pub fn len(&self) -> usize {
        let mut len = if self.flag & TURN_SHORT_BIT == 0 {
            TURN_FULL_LEN
        } else {
            TURN_FULL_LEN - 1
        };
        len -= TURN_DROP_BITS
            .iter()
            .filter(|bit| self.flag & **bit != 0)
            .count();
        if self.flag & TURN_LAST_DROP_BIT != 0 {
            len = len.saturating_sub(1);
        }
        len
    }

    /// Whether no byte follows the flag.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut turn = Self {
            flag: reader.u8()?,
            bytes: [0; TURN_FULL_LEN],
        };
        let len = turn.len();
        turn.bytes[..len].copy_from_slice(reader.bytes(len)?);
        Ok(turn)
    }

    fn write(&self, writer: &mut Writer) {
        writer.u8(self.flag).bytes(&self.bytes[..self.len()]);
    }
}

impl TurretState {
    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match the layout.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, TURRET_CHILDREN, |reader, child| {
            match child {
                0 => state.yaw = Some(reader.u16()?),
                1 => state.pitch = Some(reader.u16()?),
                2 => state.heat = Some(reader.u16()?),
                3 => state.orientation = Some(PackedTurn::read(reader)?),
                4 => state.interactor = Some(reader.array()?),
                5 => state.overheated = Some(reader.boolean()?),
                _ => state.blocked = Some(reader.boolean()?),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state.
    pub fn write(&self, writer: &mut Writer) {
        let present = [
            self.yaw.is_some(),
            self.pitch.is_some(),
            self.heat.is_some(),
            self.orientation.is_some(),
            self.interactor.is_some(),
            self.overheated.is_some(),
            self.blocked.is_some(),
        ];
        let children = crate::slots::children_mask(&present);
        write_known(
            writer,
            children,
            TURRET_CHILDREN,
            |writer, child| match child {
                0 => self.yaw.iter().for_each(|value| {
                    writer.u16(*value);
                }),
                1 => self.pitch.iter().for_each(|value| {
                    writer.u16(*value);
                }),
                2 => self.heat.iter().for_each(|value| {
                    writer.u16(*value);
                }),
                3 => self.orientation.iter().for_each(|turn| turn.write(writer)),
                4 => self.interactor.iter().for_each(|gde| {
                    writer.bytes(gde);
                }),
                5 => self.overheated.iter().for_each(|flag| {
                    writer.boolean(*flag);
                }),
                _ => self.blocked.iter().for_each(|flag| {
                    writer.boolean(*flag);
                }),
            },
        );
    }
}

/// The player's builder state (2270, a player component); a `None` slot is not sent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuilderState {
    /// Slot 0, `playerBuilderState` (Observed: 0 to 2). Observed in every retail camp build:
    /// [`BUILDER_BUILDING`] from the accepted build interaction (`12a_msq_12_camptutorial`
    /// 286.303 s) to its end (290.904 s) or its cancel (`Playthrough_Part_05` 3209.646 s),
    /// [`BUILDER_IDLE`] after.
    pub builder: Option<u8>,
    /// Slot 1, `numberOfPlacedDeployables` (Observed: 0 to 2): 1 from a camp's placement
    /// (`12a_msq_12_camptutorial` 284.402 s), 0 when the camp goes with no new one
    /// (`99g_msq_sm_excavateartifacts` 222.871 s).
    pub placed: Option<u8>,
}

/// `playerBuilderState` of a player who builds nothing.
pub const BUILDER_IDLE: u8 = 0;
/// `playerBuilderState` of a player who commits resources to a structure.
pub const BUILDER_BUILDING: u8 = 1;

impl BuilderState {
    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match the layout.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let slots = read_slot_byte(reader, FIELD, BUILDER_SLOTS)?;
        let mut state = Self::default();
        if slots & BUILDER_STATE_SLOT != 0 {
            read_children(reader, FIELD, ONE_CHILD, |reader, _| {
                state.builder = Some(reader.u8()?);
                Ok(())
            })?;
        }
        if slots & BUILDER_PLACED_SLOT != 0 {
            read_children(reader, FIELD, ONE_CHILD, |reader, _| {
                state.placed = Some(reader.u8()?);
                Ok(())
            })?;
        }
        Ok(state)
    }

    /// Writes the state.
    pub fn write(&self, writer: &mut Writer) {
        let state_bit = if self.builder.is_some() {
            BUILDER_STATE_SLOT
        } else {
            0
        };
        let placed_bit = if self.placed.is_some() {
            BUILDER_PLACED_SLOT
        } else {
            0
        };
        writer.u8(state_bit | placed_bit);
        for value in [self.builder, self.placed].into_iter().flatten() {
            write_known_children(writer, FIRST_CHILD, ONE_CHILD, |writer, _| {
                writer.u8(value);
            });
        }
    }
}

/// A land claim entity's state (62): its claim (Observed: the territory id of a fort or a
/// settlement claim entity, 2 to 14).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LandClaimState {
    /// Child 0, `claimKey`.
    pub claim: Option<u16>,
}

impl LandClaimState {
    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match the layout.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, ONE_CHILD, |reader, _| {
            state.claim = Some(reader.u16()?);
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state.
    pub fn write(&self, writer: &mut Writer) {
        write_known(
            writer,
            u16::from(self.claim.is_some()),
            ONE_CHILD,
            |writer, _| {
                self.claim.iter().for_each(|claim| {
                    writer.u16(*claim);
                });
            },
        );
    }
}

/// Reads a text field of a war state (a prefix-coded length and the bytes).
pub(crate) fn read_war_text(reader: &mut Reader<'_>) -> Result<String, MessageError> {
    Ok(String::from_utf8_lossy(&read_string(reader)?).into_owned())
}

/// Writes a text field of a war state.
pub(crate) fn write_war_text(writer: &mut Writer, text: &str) -> Result<(), MessageError> {
    write_string(writer, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;
    use crate::items::FullList;
    use crate::keyed_list::{ListIndex, MapChange};

    fn round<T: PartialEq + std::fmt::Debug>(
        body: &[u8],
        read: fn(&mut Reader<'_>) -> Result<T, MessageError>,
        write: fn(&T, &mut Writer),
    ) -> T {
        let mut reader = Reader::new(body);
        let state = read(&mut reader).expect("reads");
        reader.finish().expect("used up");
        let mut writer = Writer::new();
        write(&state, &mut writer);
        assert_eq!(writer.finish(), body);
        state
    }

    fn war() -> WarEntry {
        let side = |number, fill| WarSide {
            number,
            groups: vec![[fill; RAW_ID_LEN]; 10],
            raid: [fill + 1; RAW_ID_LEN],
            key: u64::from(fill),
        };
        WarEntry {
            attacker_company: [0xa1; RAW_ID_LEN],
            defender_company: [0xd1; RAW_ID_LEN],
            attacker: side(9428, 0x10),
            defender: side(9429, 0x20),
            attacker_faction: 2,
            defender_faction: 1,
            territory: 8,
            kind: WAR_KIND_WAR,
            war: [0x33; RAW_ID_LEN],
            extra: 0,
            status: WarStatus {
                winner: [0; RAW_ID_LEN],
                flag: 2,
                value: 1_500_000,
                instance_state: 0,
                ready: WAR_READY,
                phase: WAR_PHASE_SCHEDULED,
                time_ns: 1_783_537_199_999_284_323,
                game_mode: 0,
                instance: [0; RAW_ID_LEN],
                host: [0; RAW_ID_LEN],
            },
        }
    }

    fn write_war_data(state: &WarDataState, writer: &mut Writer) {
        state.write(writer).expect("writes");
    }

    /// A first war data body in the retail form (`arvaysa_ES_attacker_war_45vs45`
    /// 1783536276201, made-up ids): the full list of wars, one siege window change and one
    /// influence race; and the retail removal of a resolved war (`Playthrough_Part_21`
    /// 1793819069152: index 2, the version, no value).
    #[test]
    fn war_data_reads_and_writes_back() {
        let state = WarDataState {
            wars: Some(IndexedList::Full(FullList {
                version: Some(102_407),
                elements: vec![war()],
            })),
            schedule: Some(KeyedList::full(
                102_407,
                vec![(
                    8,
                    ScheduleAdjustment {
                        company: [0xd1; RAW_ID_LEN],
                        first: 2,
                        territory: 8,
                        second: 1,
                        time_ns: 1_753_821_619_041_997_900,
                    },
                )],
            )),
            influence: Some(KeyedList::full(
                102_407,
                vec![(
                    8,
                    InfluenceRace {
                        time_ns: 1_783_443_608_780_965_300,
                        territory: 8,
                        flags: [1, 1, 2, 1],
                        influence: vec![0, 73, 0],
                    },
                )],
            )),
        };
        let mut writer = Writer::new();
        write_war_data(&state, &mut writer);
        let body = writer.finish();
        assert_eq!(body[..2], [crate::slots::SLOT_0, 0b111]);
        assert_eq!(round(&body, WarDataState::read, write_war_data), state);
        let removal = hex("010101000201c8b711");
        let read = round(&removal, WarDataState::read, write_war_data);
        let Some(IndexedList::Changes(changes)) = read.wars else {
            panic!("a delta");
        };
        assert_eq!(
            changes,
            [MapChange {
                key: ListIndex(2),
                version: changes[0].version,
                value: None,
            }]
        );
        assert!(changes[0].version.is_some());
    }

    /// Retail bodies with no ids (`CK_won_offensive_war`, `fort_capping_pvp_flagged`, a
    /// builder of `aasdf`, a land claim entity) and a siege body of a made-up entity.
    #[test]
    fn the_small_states_read_and_write_back() {
        let point = round(
            &hex("010345d60e8d44987acf42701ac8000000030400000000000000000100"),
            CapturePointState::read,
            CapturePointState::write,
        );
        assert_eq!(point.cold.map(|cold| cold.number), Some(3));
        assert_eq!(point.hot, Some(0));
        let turret = round(
            &hex("017f00000000000066550000000000000000000000000000000000000000ffffffff0000"),
            TurretState::read,
            TurretState::write,
        );
        assert_eq!(turret.heat, Some(0));
        assert_eq!(turret.overheated, Some(false));
        assert_eq!(
            turret.orientation,
            Some(PackedTurn {
                flag: 0x66,
                bytes: [0x55, 0, 0],
            })
        );
        round(&hex("01043a27"), TurretState::read, TurretState::write);
        for body in ["010100", "020101", "010102"] {
            round(&hex(body), BuilderState::read, BuilderState::write);
        }
        let claim = round(
            &hex("01010006"),
            LandClaimState::read,
            LandClaimState::write,
        );
        assert_eq!(claim.claim, Some(6));
        let siege = round(
            &hex("010201010102030405060708010901"),
            SiegeWarfareDataState::read,
            |state, writer| state.write(writer).expect("writes"),
        );
        assert!(siege.points_hot.is_some());
    }
}
