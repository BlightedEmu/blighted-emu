// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Javelin::GameModeReplicatedState` (class 2343): the state of a game mode entity, the one
//! network entity of a duel, an arena match, an Outpost Rush match, a war or an expedition run.
//! Layout from the client's member table (30 children). The children Observed in the retail
//! game modes (game mode study 2026-11-11: 2 duels of `duel_capture`, 2 expedition entries):
//!
//! - the first state of a mode holds its script state, an empty spawner list, the mode's id
//!   twice (CRC-32 of the lowercase `GameModeId`), each participant (its entity's raw id, its
//!   facet entity and a second id), its status, its team and its character id, empty raid ids
//!   and values, its timers, the empty global affliction data and the linked mode 0;
//! - a later state holds the children that changed: a list in its full form with its new
//!   version (the timers and the values of a duel), or the changed elements of a list (one
//!   participant's status).
//!
//! The sim writes this state; the captures' own states read through the member table, which
//! also holds the children the sim does not send (the events, the map tiles, the icons).

use crate::items::FullList;
use crate::list_delta::{ReplicatedList, write_child};
use crate::slots::{SLOT_0, write_known_wide_children};
use crate::wire::{MessageError, RAW_ID_LEN, Writer};

/// Type index of `Javelin::GameModeReplicatedState`.
pub const GAME_MODE_TYPE_INDEX: u64 = 2343;
/// The children of the class.
const CHILDREN: u8 = 30;
const SCRIPT_STATE_CHILD: u8 = 0;
const SPAWNED_CHILD: u8 = 2;
const MODE_CHILD: u8 = 3;
const MAP_CHILD: u8 = 4;
const PARTICIPANTS_CHILD: u8 = 5;
const STATUSES_CHILD: u8 = 6;
const TEAMS_CHILD: u8 = 7;
const CHARACTERS_CHILD: u8 = 8;
const RAIDS_CHILD: u8 = 9;
const VALUES_CHILD: u8 = 10;
const TIMERS_CHILD: u8 = 11;
const AFFLICTION_CHILD: u8 = 22;
const LINKED_MODE_CHILD: u8 = 29;
/// The flag of a participant's character id in its raw form (Observed: `05`, then 16 bytes).
const RAW_CHARACTER_ID: u8 = 0x05;
/// The global affliction data of a mode with none (Observed in every first state: three zero
/// ids with two zero times, then `ff 00`).
const NO_AFFLICTION: [u8; 30] = {
    let mut bytes = [0; 30];
    bytes[28] = 0xff;
    bytes
};

/// One participant of a game mode (`participantFacetRefs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticipantRef {
    /// The raw id of its entity.
    pub raw_id: [u8; RAW_ID_LEN],
    /// Its facet entity.
    pub facet: u64,
    /// A second id (name Unknown; retail: constant per player over its game modes).
    pub second: u64,
}

/// A keyed number of a game mode: the CRC-32 of its name and its value (`values`: a count;
/// `syncedTimers`: server ms since 2000-01-01).
pub type Keyed = (u32, u64);

/// The state of a game mode entity. A `None` child is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameModeState {
    /// Child 0, `curScriptStateId`.
    pub script_state: Option<u8>,
    /// Child 2, `spawnedEntityIdsBySpawnerId`: each spawner id and its entity.
    pub spawned: Option<FullList<Keyed>>,
    /// Child 3, `gameModeId`: CRC-32 of the lowercase `GameModeId`.
    pub mode: Option<u32>,
    /// Child 4, `gameModeMapId` (Observed: the same value as the mode's id).
    pub map: Option<u32>,
    /// Child 5, `participantFacetRefs`.
    pub participants: Option<ReplicatedList<ParticipantRef>>,
    /// Child 6, `participantStatuses`, in participant order.
    pub statuses: Option<ReplicatedList<u8>>,
    /// Child 7, `participantTeamIndexes`, in participant order.
    pub teams: Option<ReplicatedList<u8>>,
    /// Child 8, `participantCharacterIds`, in participant order.
    pub characters: Option<ReplicatedList<[u8; RAW_ID_LEN]>>,
    /// Child 9, `raidIds`.
    pub raids: Option<ReplicatedList<u64>>,
    /// Child 10, `values`.
    pub values: Option<FullList<Keyed>>,
    /// Child 11, `syncedTimers`.
    pub timers: Option<FullList<Keyed>>,
    /// Child 22, `globalAfflictionData`: sent empty with the first state.
    pub no_affliction: bool,
    /// Child 29, `linkedMode`.
    pub linked_mode: Option<u8>,
}

fn write_keyed(writer: &mut Writer, (key, value): &Keyed) {
    writer.u32(*key).u64(*value);
}

fn write_keyed_list(
    writer: &mut Writer,
    list: Option<&FullList<Keyed>>,
) -> Result<(), MessageError> {
    list.map_or(Ok(()), |list| {
        list.write(writer, |writer, entry| {
            write_keyed(writer, entry);
            Ok(())
        })
    })
}

impl GameModeState {
    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a list does not write.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let present = [
            (SCRIPT_STATE_CHILD, self.script_state.is_some()),
            (SPAWNED_CHILD, self.spawned.is_some()),
            (MODE_CHILD, self.mode.is_some()),
            (MAP_CHILD, self.map.is_some()),
            (PARTICIPANTS_CHILD, self.participants.is_some()),
            (STATUSES_CHILD, self.statuses.is_some()),
            (TEAMS_CHILD, self.teams.is_some()),
            (CHARACTERS_CHILD, self.characters.is_some()),
            (RAIDS_CHILD, self.raids.is_some()),
            (VALUES_CHILD, self.values.is_some()),
            (TIMERS_CHILD, self.timers.is_some()),
            (AFFLICTION_CHILD, self.no_affliction),
            (LINKED_MODE_CHILD, self.linked_mode.is_some()),
        ];
        let mask = present
            .iter()
            .filter(|(_, present)| *present)
            .fold(0_u32, |mask, (child, _)| mask | 1 << child);
        if mask == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        let mut result = Ok(());
        write_known_wide_children(writer, mask, CHILDREN, |writer, child| {
            let written = match child {
                SCRIPT_STATE_CHILD => {
                    self.script_state.map(|state| writer.u8(state));
                    Ok(())
                }
                SPAWNED_CHILD => write_keyed_list(writer, self.spawned.as_ref()),
                MODE_CHILD => {
                    self.mode.map(|mode| writer.u32(mode));
                    Ok(())
                }
                MAP_CHILD => {
                    self.map.map(|map| writer.u32(map));
                    Ok(())
                }
                PARTICIPANTS_CHILD => {
                    write_child(writer, self.participants.as_ref(), |writer, participant| {
                        writer
                            .bytes(&participant.raw_id)
                            .u64(participant.facet)
                            .u64(participant.second);
                    })
                }
                STATUSES_CHILD => write_child(writer, self.statuses.as_ref(), |writer, status| {
                    writer.u8(*status);
                }),
                TEAMS_CHILD => write_child(writer, self.teams.as_ref(), |writer, team| {
                    writer.u8(*team);
                }),
                CHARACTERS_CHILD => {
                    write_child(writer, self.characters.as_ref(), |writer, character| {
                        writer.u8(RAW_CHARACTER_ID).bytes(character);
                    })
                }
                RAIDS_CHILD => write_child(writer, self.raids.as_ref(), |writer, raid| {
                    writer.u64(*raid);
                }),
                VALUES_CHILD => write_keyed_list(writer, self.values.as_ref()),
                TIMERS_CHILD => write_keyed_list(writer, self.timers.as_ref()),
                AFFLICTION_CHILD => {
                    writer.bytes(&NO_AFFLICTION);
                    Ok(())
                }
                _ => {
                    self.linked_mode.map(|mode| writer.u8(mode));
                    Ok(())
                }
            };
            if result.is_ok() {
                result = written;
            }
        });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::list_delta::ListChange;

    /// CRC-32 of `duel`: the mode of a duel.
    const DUEL: u32 = 0x9bb4_a762;
    /// The keys of a duel's timers (names Unknown): the countdown's end, the fight's end.
    const COUNTDOWN_TIMER: u32 = 0xf920_f724;
    const FIGHT_TIMER: u32 = 0x47d6_e652;
    /// The key of a duel's end value (name Unknown).
    const END_VALUE: u32 = 0x428a_d202;
    /// Made-up participants.
    const FIRST: ParticipantRef = ParticipantRef {
        raw_id: [0xa1; RAW_ID_LEN],
        facet: 0xa2a2_a2a2_a2a2_a2a2,
        second: 0xa3a3_a3a3_a3a3_a3a3,
    };
    const SECOND: ParticipantRef = ParticipantRef {
        raw_id: [0xb1; RAW_ID_LEN],
        facet: 0xb2b2_b2b2_b2b2_b2b2,
        second: 0xb3b3_b3b3_b3b3_b3b3,
    };
    const FIRST_CHARACTER: [u8; RAW_ID_LEN] = [0xc1; RAW_ID_LEN];
    const SECOND_CHARACTER: [u8; RAW_ID_LEN] = [0xc2; RAW_ID_LEN];

    fn hex(state: &GameModeState) -> String {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("the state writes");
        writer
            .finish()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn full<T>(version: u64, elements: Vec<T>) -> FullList<T> {
        FullList {
            version: Some(version),
            elements,
        }
    }

    /// Retail `duel_capture` at 67.299 s: the first state of a duel (192 bytes), with made-up
    /// participant and character ids.
    #[test]
    fn the_first_state_of_a_retail_duel_writes_the_same_bytes() {
        let first = GameModeState {
            script_state: Some(1),
            spawned: Some(full(2, Vec::new())),
            mode: Some(DUEL),
            map: Some(DUEL),
            participants: Some(ReplicatedList::Full(full(2, vec![FIRST, SECOND]))),
            statuses: Some(ReplicatedList::Full(full(2, vec![3, 3]))),
            teams: Some(ReplicatedList::Full(full(2, vec![0, 1]))),
            characters: Some(ReplicatedList::Full(full(
                2,
                vec![FIRST_CHARACTER, SECOND_CHARACTER],
            ))),
            raids: Some(ReplicatedList::Full(full(2, Vec::new()))),
            values: Some(full(2, Vec::new())),
            timers: Some(full(2, vec![(COUNTDOWN_TIMER, 0x00c2_58a8_cef9)])),
            no_affliction: true,
            linked_mode: Some(0),
        };
        let expected = [
            "01fd01000102009bb4a7629bb4a76200010202",
            "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a2a2a2a2a2a2a2a2a3a3a3a3a3a3a3a3",
            "b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b2b2b2b2b2b2b2b2b3b3b3b3b3b3b3b3",
            "0001020203039f0001020200010001020205c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1",
            "05c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2000102000001020000010201",
            "f920f724000000c258a8cef98082",
            "00000000000000000000000000000000000000000000000000000000ff000200",
        ]
        .concat();
        assert_eq!(hex(&first), expected);
    }

    /// Retail `duel_capture`: the fight's start at 73.315 s, the duel's end at 83.914 s and the
    /// loser's status back at 86.947 s.
    #[test]
    fn the_later_states_of_a_retail_duel_write_the_same_bytes() {
        let fight = GameModeState {
            script_state: Some(3),
            timers: Some(full(4, vec![(FIGHT_TIMER, 0x00c2_58aa_2e96)])),
            ..GameModeState::default()
        };
        assert_eq!(hex(&fight), "018103100001040147d6e652000000c258aa2e96");
        let status = |version, value| {
            Some(ReplicatedList::Changes(vec![ListChange {
                index: 1,
                version,
                value,
            }]))
        };
        let end = GameModeState {
            script_state: Some(0),
            statuses: status(0x12, 0x0b),
            values: Some(full(0x12, vec![(END_VALUE, 0)])),
            timers: Some(full(0x12, vec![(FIGHT_TIMER, 0x00c2_58aa_57ec)])),
            ..GameModeState::default()
        };
        assert_eq!(
            hex(&end),
            "01c10001010101120b1800011201428ad20200000000000000000001120147d6e652000000c258aa57ec"
        );
        let back = GameModeState {
            statuses: status(0x13, 3),
            ..GameModeState::default()
        };
        assert_eq!(hex(&back), "0140010101011303");
        assert_eq!(hex(&GameModeState::default()), "00", "no child: no slot");
    }
}
