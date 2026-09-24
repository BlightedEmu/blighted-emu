// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::ObjectiveInteractorComponentReplicatedState` (type 3829, own player component 10): the
//! board the player has open (a faction representative, a town project board) and the missions
//! it offers (conversation-notify note, section 4).
//!
//! Slot 0 has four children (constructor `FUN_143a1cb00`; readers at `+0x30` of each field
//! vtable): `objectiveProviderId` (16 raw bytes, `FUN_14279f200`), `expirationTime` (a big-endian
//! `u64`, `FUN_14279f420`), `missionParams` (a replicated list of [`MissionParams`],
//! `FUN_143a69120`, full form `FUN_143a07710`) and `m_communityGoals` (a count and
//! [`CommunityGoal`]s, `FUN_14495eed0`).

use crate::items::read_number;
use crate::list_delta::ReplicatedList;
use crate::slots::{SLOT_0, children_mask, read_slot_0, write_known_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::ObjectiveInteractorComponentReplicatedState`.
pub const OBJECTIVE_INTERACTOR_TYPE_INDEX: u64 = 3829;
/// The own player component of the objective interactor.
pub const OBJECTIVE_INTERACTOR_COMPONENT: u64 = 10;
/// Bytes of an objective provider id.
pub const PROVIDER_ID_LEN: usize = 16;
/// The time from a board's open to its `expirationTime` (retail: 300 s after each of 29 opens
/// of a faction board; the source of the value is Unknown).
pub const BOARD_EXPIRATION_NS: u64 = 300_000_000_000;

/// Children of slot 0.
const CHILDREN: u8 = 4;
const PROVIDER_CHILD: u8 = 0;
const EXPIRATION_CHILD: u8 = 1;
const MISSIONS_CHILD: u8 = 2;
/// The most values of a mission's last list (`FUN_145b55b70` refuses a count above 7).
const MAX_MISSION_VALUES: u32 = 7;
/// Bytes of one `u32` of a community goal list.
const U32_LEN: usize = 4;
/// The fewest bytes of one community goal: the territory, the value and two empty counts.
const MIN_GOAL_LEN: usize = 8;
const FIELD: &str = "objective interactor";

/// One mission a board offers (`FUN_145cea3e0`, then `FUN_145b520f0` and `FUN_145b55b70`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionParams {
    /// The mission instance: the id the client sends to turn it in (2461).
    pub instance: u64,
    /// CRC-32 of the lowercase `MissionID` (`mission_data`).
    pub mission: u32,
    /// The objective provider that offers it (the same bytes as the state's provider).
    pub provider: [u8; PROVIDER_ID_LEN],
    /// When the player took it: ns since 1970-01-01; 0 while it is only offered.
    pub taken_at_ns: u64,
    /// A big-endian `u16` (name Unknown; retail 0).
    pub word: u16,
    /// Whether it is still offered (Inferred: retail true on each offered mission, false on the
    /// one the player took).
    pub available: bool,
    /// A flag (name Unknown; retail 0).
    pub flag_b: bool,
    /// A flag (name Unknown; retail 0).
    pub flag_c: bool,
    /// A flag (name Unknown; retail 0).
    pub flag_d: bool,
    /// A big-endian `u64` (name Unknown; retail 0).
    pub time_b: u64,
    /// A flag (name Unknown; retail 0).
    pub flag_e: bool,
    /// Up to 7 big-endian `u32` (name Unknown; retail empty).
    pub values: Vec<u32>,
}

impl MissionParams {
    /// Mission `mission` offered by `provider` as instance `instance`: available, not taken,
    /// every other field as retail sends it.
    pub const fn offered(instance: u64, mission: u32, provider: [u8; PROVIDER_ID_LEN]) -> Self {
        Self {
            instance,
            mission,
            provider,
            taken_at_ns: 0,
            word: 0,
            available: true,
            flag_b: false,
            flag_c: false,
            flag_d: false,
            time_b: 0,
            flag_e: false,
            values: Vec::new(),
        }
    }

    /// The same mission taken at `now_ns` (ns since 1970-01-01): retail sends this element alone
    /// (a delta of its index) 30 to 50 ms after the client's 3816 or 3818.
    pub fn taken(&self, now_ns: u64) -> Self {
        Self {
            taken_at_ns: now_ns,
            available: false,
            ..self.clone()
        }
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let instance = reader.u64()?;
        let mission = reader.u32()?;
        let provider = reader.array()?;
        let taken_at_ns = reader.u64()?;
        let word = reader.u16()?;
        let available = reader.boolean()?;
        let flag_b = reader.boolean()?;
        let flag_c = reader.boolean()?;
        let flag_d = reader.boolean()?;
        let time_b = reader.u64()?;
        let flag_e = reader.boolean()?;
        let count = read_number(reader)?;
        if count > MAX_MISSION_VALUES {
            return Err(MessageError::CountTooLarge {
                count: u64::from(count),
                limit: u64::from(MAX_MISSION_VALUES),
            });
        }
        let values = (0..count).map(|_| reader.u32()).collect::<Result<_, _>>()?;
        Ok(Self {
            instance,
            mission,
            provider,
            taken_at_ns,
            word,
            available,
            flag_b,
            flag_c,
            flag_d,
            time_b,
            flag_e,
            values,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let count = u32::try_from(self.values.len())
            .ok()
            .filter(|count| *count <= MAX_MISSION_VALUES)
            .ok_or(MessageError::CountTooLarge {
                // A `usize` count fits 64 bits.
                count: self.values.len() as u64,
                limit: u64::from(MAX_MISSION_VALUES),
            })?;
        writer
            .u64(self.instance)
            .u32(self.mission)
            .bytes(&self.provider)
            .u64(self.taken_at_ns)
            .u16(self.word)
            .boolean(self.available)
            .boolean(self.flag_b)
            .boolean(self.flag_c)
            .boolean(self.flag_d)
            .u64(self.time_b)
            .boolean(self.flag_e)
            .wide_prefixed(u64::from(count));
        for value in &self.values {
            writer.u32(*value);
        }
        Ok(())
    }
}

/// One community goal of a territory (`FUN_144950450`): the town project missions a town board
/// offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityGoal {
    /// The territory id (`TerritoryDefinitions`; retail 2 to 17).
    pub territory: u16,
    /// A big-endian `u32` (name Unknown; retail values near 2.37e8, different per territory).
    pub value: u32,
    /// CRC-32 of the lowercase `MissionID` of each offered project mission (`mission_data`
    /// rows with a `TerritoryProjectID`).
    pub missions: Vec<u32>,
    /// A second list of `u32` (name Unknown; retail empty).
    pub second: Vec<u32>,
}

impl CommunityGoal {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            territory: reader.u16()?,
            value: reader.u32()?,
            missions: read_u32_list(reader)?,
            second: read_u32_list(reader)?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer.u16(self.territory).u32(self.value);
        write_u32_list(writer, &self.missions);
        write_u32_list(writer, &self.second);
    }
}

/// The objective interactor state; `None` sends no change of that child.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectiveInteractorState {
    /// `objectiveProviderId`: the board the player has open.
    pub provider: Option<[u8; PROVIDER_ID_LEN]>,
    /// `expirationTime`: when the offer ends, ns since 2000-01-01 (retail: the open plus
    /// [`BOARD_EXPIRATION_NS`]; 0 on the login card).
    pub expiration_time: Option<u64>,
    /// `missionParams`: the offered missions.
    pub missions: Option<ReplicatedList<MissionParams>>,
    /// `m_communityGoals`: the town project goals of each territory.
    pub community_goals: Option<Vec<CommunityGoal>>,
}

impl ObjectiveInteractorState {
    /// The first body of a new character: no community goals (retail `01 08 00`, both captured
    /// new characters).
    pub const fn new_character() -> Self {
        Self {
            provider: None,
            expiration_time: None,
            missions: None,
            community_goals: Some(Vec::new()),
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets a slot other than 0, or a list
    /// does not read.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, CHILDREN, |reader, child| {
            match child {
                PROVIDER_CHILD => state.provider = Some(reader.array()?),
                EXPIRATION_CHILD => state.expiration_time = Some(reader.u64()?),
                MISSIONS_CHILD => {
                    state.missions = Some(ReplicatedList::read(reader, FIELD, MissionParams::read)?);
                }
                _ => state.community_goals = Some(read_goals(reader)?),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a mission has more than 7 values.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let children = children_mask(&[
            self.provider.is_some(),
            self.expiration_time.is_some(),
            self.missions.is_some(),
            self.community_goals.is_some(),
        ]);
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        let mut result = Ok(());
        write_known_children(writer, children, CHILDREN, |writer, child| match child {
            PROVIDER_CHILD => {
                self.provider.map(|provider| writer.bytes(&provider));
            }
            EXPIRATION_CHILD => {
                self.expiration_time.map(|time| writer.u64(time));
            }
            MISSIONS_CHILD => {
                if let Some(missions) = &self.missions {
                    result = missions.write(writer, |writer, mission| mission.write(writer));
                }
            }
            _ => {
                if let Some(goals) = &self.community_goals {
                    // A `usize` count fits 64 bits.
                    writer.wide_prefixed(goals.len() as u64);
                    for goal in goals {
                        goal.write(writer);
                    }
                }
            }
        });
        result
    }
}

/// A count, then that many community goals.
fn read_goals(reader: &mut Reader<'_>) -> Result<Vec<CommunityGoal>, MessageError> {
    let count = read_number(reader)?;
    // Each goal takes at least `MIN_GOAL_LEN` bytes.
    if count as usize > reader.remaining() / MIN_GOAL_LEN {
        return Err(MessageError::LengthMismatch { field: FIELD });
    }
    (0..count).map(|_| CommunityGoal::read(reader)).collect()
}

/// A count, then that many big-endian `u32` (`FUN_14293da60`).
fn read_u32_list(reader: &mut Reader<'_>) -> Result<Vec<u32>, MessageError> {
    let count = read_number(reader)?;
    if count as usize > reader.remaining() / U32_LEN {
        return Err(MessageError::LengthMismatch { field: FIELD });
    }
    (0..count).map(|_| reader.u32()).collect()
}

fn write_u32_list(writer: &mut Writer, values: &[u32]) {
    // A `usize` count fits 64 bits.
    writer.wide_prefixed(values.len() as u64);
    for value in values {
        writer.u32(*value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::FullList;
    use crate::list_delta::ListChange;

    /// A synthetic provider id.
    const PROVIDER: [u8; PROVIDER_ID_LEN] = [7; PROVIDER_ID_LEN];
    /// CRC-32 of `fm_c_hunt_0607`, a faction mission of territory 6.
    const HUNT_MISSION: u32 = 0x66f9_81bb;
    const INSTANCE: u64 = 0x1122_3344_5566_7788;
    /// A synthetic open time: ns since 2000-01-01.
    const OPENED_AT_NS: u64 = 834_682_236_844_266_691;
    /// A synthetic pick time: ns since 1970-01-01.
    const TAKEN_AT_NS: u64 = 1_781_367_053_382_130_100;

    fn round_trip(state: &ObjectiveInteractorState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes);
        assert_eq!(&ObjectiveInteractorState::read(&mut reader).expect("read"), state);
        reader.finish().expect("whole body");
        bytes
    }

    #[test]
    fn the_new_character_body_is_retails() {
        assert_eq!(
            round_trip(&ObjectiveInteractorState::new_character()),
            [0x01, 0x08, 0x00]
        );
        assert_eq!(round_trip(&ObjectiveInteractorState::default()), [0x00]);
    }

    #[test]
    fn a_board_open_writes_the_retail_layout() {
        // Retail at a faction board: children 0, 1 and 2, the missions in the full form.
        let state = ObjectiveInteractorState {
            provider: Some(PROVIDER),
            expiration_time: Some(OPENED_AT_NS + BOARD_EXPIRATION_NS),
            missions: Some(ReplicatedList::Full(FullList {
                version: Some(777),
                elements: vec![MissionParams::offered(INSTANCE, HUNT_MISSION, PROVIDER)],
            })),
            community_goals: None,
        };
        let bytes = round_trip(&state);
        // Slot byte, child byte 0b0111, 16 provider bytes, 8 time bytes, then the full form.
        const HEAD: usize = 2 + PROVIDER_ID_LEN + 8;
        assert_eq!(&bytes[..2], [0x01, 0x07]);
        assert_eq!(&bytes[HEAD..HEAD + 4], [0x00, 0x01, 0x89, 0x0c]);
        // One element of 8 + 4 + 16 + 8 + 2 + 4 + 8 + 1 + 1 bytes.
        assert_eq!(bytes.len(), HEAD + 5 + 52);
    }

    #[test]
    fn a_pick_is_one_delta_entry_of_the_taken_mission() {
        let offered = MissionParams::offered(INSTANCE, HUNT_MISSION, PROVIDER);
        let taken = offered.taken(TAKEN_AT_NS);
        assert!(!taken.available);
        assert_eq!(taken.taken_at_ns, TAKEN_AT_NS);
        let state = ObjectiveInteractorState {
            missions: Some(ReplicatedList::Changes(vec![ListChange {
                index: 2,
                version: 1665,
                value: taken,
            }])),
            ..ObjectiveInteractorState::default()
        };
        let bytes = round_trip(&state);
        // Slot, child 2, count 1, mask 1, index 2, version flag, version `81 1a`.
        assert_eq!(&bytes[..9], [0x01, 0x04, 0x01, 0x01, 0x02, 0x01, 0x81, 0x1a, 0x11]);
    }

    #[test]
    fn community_goals_read_and_refuse_short_lists() {
        let state = ObjectiveInteractorState {
            community_goals: Some(vec![CommunityGoal {
                territory: 12,
                value: 0x0e1f_7421,
                missions: vec![0x6579_7156, 0xd852_dd13],
                second: Vec::new(),
            }]),
            ..ObjectiveInteractorState::default()
        };
        let bytes = round_trip(&state);
        assert_eq!(&bytes[..5], [0x01, 0x08, 0x01, 0x00, 0x0c]);
        // A list count beyond the bytes left.
        let short = [0x01, 0x08, 0x01, 0x00, 0x0c, 0, 0, 0, 0, 0x05, 0x00];
        assert!(matches!(
            ObjectiveInteractorState::read(&mut Reader::new(&short)),
            Err(MessageError::LengthMismatch { .. })
        ));
    }

    #[test]
    fn too_many_mission_values_are_refused() {
        let mut mission = MissionParams::offered(INSTANCE, HUNT_MISSION, PROVIDER);
        mission.values = vec![0; 8];
        let state = ObjectiveInteractorState {
            missions: Some(ReplicatedList::Full(FullList {
                version: None,
                elements: vec![mission],
            })),
            ..ObjectiveInteractorState::default()
        };
        assert!(matches!(
            state.write(&mut Writer::new()),
            Err(MessageError::CountTooLarge { .. })
        ));
    }
}
