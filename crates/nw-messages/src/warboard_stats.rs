// SPDX-License-Identifier: AGPL-3.0-or-later
//! The list parts of the war board pushes of a one-team game mode: the stats 839
//! (`WarboardComponentClientFacet::OnUpdateWarboardStats`) and the manifests 3530 (full) and
//! 2968 (update). [`crate::war::WarServerMsg`] carries them as the bytes after the header.
//!
//! Observed (retail `CATACOMBS`, `CATACOMBS3`, `CATACOMBS5`; every body reads to its end):
//!
//! - 839: the sync (`u16`, the sync of the last manifest; the client drops a body whose sync is
//!   not its manifest's, `FUN_1442fc6d0`), the index of the receiving player (`u8`), its own
//!   stats, then a `u16` count and per participant its index (`u8`) and stats. Nothing follows
//!   (the second team the client fills, `[Populating defenders]`, is empty: Inferred from the
//!   bodies' end).
//! - Stats (`FUN_1442f2aa0`): one wide prefix-coded integer ([`Reader::wide_prefixed`]) whose
//!   bit 0 is set when the participant left the board (the client keeps the inverse at +0x98)
//!   and whose bits 1 to 58 name the stat slots that follow; then one wide prefix-coded integer
//!   per named slot, from the lowest slot.
//! - 3530 and 2968: the sync, a `u16` count, the participants' references, one index byte per
//!   participant, then six zero bytes (`CATACOMBS5` 80.949 s and 902.057 s; meaning of the six
//!   bytes Unknown: they never differ).
//!
//! The two-team board of Outpost Rush ([`TeamsWarboardStats`], [`TeamsManifest`]; Observed: all
//! 12 retail Outpost Rush ledgers, 13,129 pushes of 839, 3530 and 2968, every body read to its
//! end) shows the same bytes as two lists:
//!
//! - 839: the sync, the receiver's index (`u8`), its own stats, then two lists, each a `u8`
//!   count and per participant its index (`u8`) and stats. The client fills `[Populating
//!   attackers]` and then `[Populating defenders]` (`FUN_1442fc6d0`). A catacombs 839's `u16`
//!   count is the same two bytes: an empty first list and the run's list.
//! - 3530 and 2968: the sync, then per list a `u16` count, the references, one index byte per
//!   participant and a second `u16` count that is 0 in every body (its list is Unknown). The
//!   catacombs' six zero bytes are the first list's second count and the empty second list.

use std::collections::BTreeMap;

use crate::social::CharacterRef;
use crate::wire::{MessageError, Reader, Writer};

/// The stat slots of a war board (`FUN_1442f2aa0` reads slots 0 to 0x39).
pub const STAT_SLOTS: u8 = 58;
/// The bit of the stats integer that marks a participant who left.
const LEFT_BIT: u64 = 1;
/// The bytes after the participant indexes of every manifest.
const MANIFEST_TAIL: [u8; 6] = [0; 6];
const FIELD: &str = "war board stats";

/// The stats of one participant in one push: the slots that changed and whether it left.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParticipantStats {
    /// It left the board.
    pub left: bool,
    /// The changed slots and their values.
    pub slots: BTreeMap<u8, u32>,
}

impl ParticipantStats {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let head = reader.wide_prefixed()?;
        let mask = head >> 1;
        let above = mask >> STAT_SLOTS;
        if above != 0 {
            return Err(MessageError::UnknownBits {
                field: FIELD,
                // Only six bits of the integer lie above the last slot.
                bits: above as u8,
            });
        }
        let mut slots = BTreeMap::new();
        for slot in (0..STAT_SLOTS).filter(|slot| mask >> slot & 1 == 1) {
            let value = reader.wide_prefixed()?;
            let value = u32::try_from(value).map_err(|_| MessageError::CountTooLarge {
                count: value,
                limit: u64::from(u32::MAX),
            })?;
            slots.insert(slot, value);
        }
        Ok(Self {
            left: head & LEFT_BIT == LEFT_BIT,
            slots,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let mut head = u64::from(self.left);
        for slot in self.slots.keys() {
            if *slot >= STAT_SLOTS {
                return Err(MessageError::UnknownBits {
                    field: FIELD,
                    bits: *slot,
                });
            }
            head |= 1 << (u64::from(*slot) + 1);
        }
        writer.wide_prefixed(head);
        for value in self.slots.values() {
            writer.wide_prefixed(u64::from(*value));
        }
        Ok(())
    }
}

/// The list part of an 839: the stats one player gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarboardStats {
    /// The sync of the manifest the stats belong to.
    pub sync: u16,
    /// The receiving player's index on the board.
    pub local_index: u8,
    /// The receiving player's own stats.
    pub local: ParticipantStats,
    /// Each participant's index and stats, in list order.
    pub participants: Vec<(u8, ParticipantStats)>,
}

impl WarboardStats {
    /// Reads the bytes after the header.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the bytes do not match the layout (module text).
    pub fn decode_payload(payload: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(payload);
        let sync = reader.u16()?;
        let local_index = reader.u8()?;
        let local = ParticipantStats::read(&mut reader)?;
        let count = reader.u16()?;
        let participants = (0..count)
            .map(|_| Ok((reader.u8()?, ParticipantStats::read(&mut reader)?)))
            .collect::<Result<_, MessageError>>()?;
        reader.finish()?;
        Ok(Self {
            sync,
            local_index,
            local,
            participants,
        })
    }

    /// The bytes after the header.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] for more participants than a `u16` counts or a stat slot out
    /// of order or range.
    pub fn encode_payload(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer.u16(self.sync).u8(self.local_index);
        self.local.write(&mut writer)?;
        writer.u16(count_of(self.participants.len())?);
        for (index, stats) in &self.participants {
            writer.u8(*index);
            stats.write(&mut writer)?;
        }
        Ok(writer.finish())
    }
}

/// The list part of a 3530 or 2968 of a one-team board: who is on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamManifest {
    /// The manifest's sync.
    pub sync: u16,
    /// Each participant's character and index on the board, in list order.
    pub participants: Vec<(CharacterRef, u8)>,
}

impl TeamManifest {
    /// Reads the bytes after the header.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the bytes do not match the layout (module text).
    pub fn decode_payload(payload: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(payload);
        let sync = reader.u16()?;
        let count = reader.u16()?;
        let characters = (0..count)
            .map(|_| CharacterRef::read(&mut reader))
            .collect::<Result<Vec<_>, MessageError>>()?;
        let participants = characters
            .into_iter()
            .map(|character| Ok((character, reader.u8()?)))
            .collect::<Result<_, MessageError>>()?;
        if let Some(bits) = reader
            .bytes(MANIFEST_TAIL.len())?
            .iter()
            .find(|byte| **byte != 0)
        {
            return Err(MessageError::UnknownBits {
                field: FIELD,
                bits: *bits,
            });
        }
        reader.finish()?;
        Ok(Self { sync, participants })
    }

    /// The bytes after the header.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for more participants than a `u16` counts.
    pub fn encode_payload(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer
            .u16(self.sync)
            .u16(count_of(self.participants.len())?);
        for (character, _) in &self.participants {
            character.write(&mut writer);
        }
        for (_, index) in &self.participants {
            writer.u8(*index);
        }
        writer.bytes(&MANIFEST_TAIL);
        Ok(writer.finish())
    }
}

/// The lists of a two-team board, in wire order.
pub const BOARD_LISTS: usize = 2;
/// The second count of each list of a two-team manifest (Observed: 0 in every body).
const EMPTY_SECOND_LIST: u16 = 0;

/// The list part of an 839 of a two-team board (module text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamsWarboardStats {
    /// The sync of the manifest the stats belong to.
    pub sync: u16,
    /// The receiving player's index in its list.
    pub local_index: u8,
    /// The receiving player's own stats.
    pub local: ParticipantStats,
    /// The two lists in wire order, each participant's index and stats.
    pub lists: [Vec<(u8, ParticipantStats)>; BOARD_LISTS],
}

impl TeamsWarboardStats {
    /// Reads the bytes after the header.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the bytes do not match the layout (module text).
    pub fn decode_payload(payload: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(payload);
        let sync = reader.u16()?;
        let local_index = reader.u8()?;
        let local = ParticipantStats::read(&mut reader)?;
        let mut lists: [Vec<(u8, ParticipantStats)>; BOARD_LISTS] = Default::default();
        for list in &mut lists {
            let count = reader.u8()?;
            *list = (0..count)
                .map(|_| Ok((reader.u8()?, ParticipantStats::read(&mut reader)?)))
                .collect::<Result<_, MessageError>>()?;
        }
        reader.finish()?;
        Ok(Self {
            sync,
            local_index,
            local,
            lists,
        })
    }

    /// The bytes after the header.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] for more participants in a list than a `u8` counts or a stat
    /// slot out of range.
    pub fn encode_payload(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer.u16(self.sync).u8(self.local_index);
        self.local.write(&mut writer)?;
        for list in &self.lists {
            let count = u8::try_from(list.len()).map_err(|_| MessageError::CountTooLarge {
                count: list.len() as u64,
                limit: u64::from(u8::MAX),
            })?;
            writer.u8(count);
            for (index, stats) in list {
                writer.u8(*index);
                stats.write(&mut writer)?;
            }
        }
        Ok(writer.finish())
    }
}

/// The list part of a 3530 or 2968 of a two-team board (module text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamsManifest {
    /// The manifest's sync.
    pub sync: u16,
    /// The two lists in wire order, each participant's character and index.
    pub lists: [Vec<(CharacterRef, u8)>; BOARD_LISTS],
}

impl TeamsManifest {
    /// Reads the bytes after the header.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the bytes do not match the layout, and
    /// [`MessageError::UnknownBits`] for a second count that is not 0 (its list is Unknown).
    pub fn decode_payload(payload: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(payload);
        let sync = reader.u16()?;
        let mut lists: [Vec<(CharacterRef, u8)>; BOARD_LISTS] = Default::default();
        for list in &mut lists {
            let count = reader.u16()?;
            let characters = (0..count)
                .map(|_| CharacterRef::read(&mut reader))
                .collect::<Result<Vec<_>, MessageError>>()?;
            *list = characters
                .into_iter()
                .map(|character| Ok((character, reader.u8()?)))
                .collect::<Result<_, MessageError>>()?;
            let second = reader.u16()?;
            if second != EMPTY_SECOND_LIST {
                return Err(MessageError::UnknownBits {
                    field: FIELD,
                    bits: second.to_be_bytes()[1],
                });
            }
        }
        reader.finish()?;
        Ok(Self { sync, lists })
    }

    /// The bytes after the header.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for more participants in a list than a `u16`
    /// counts.
    pub fn encode_payload(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer.u16(self.sync);
        for list in &self.lists {
            writer.u16(count_of(list.len())?);
            for (character, _) in list {
                character.write(&mut writer);
            }
            for (_, index) in list {
                writer.u8(*index);
            }
            writer.u16(EMPTY_SECOND_LIST);
        }
        Ok(writer.finish())
    }
}

/// `len` as a `u16` count.
fn count_of(len: usize) -> Result<u16, MessageError> {
    u16::try_from(len).map_err(|_| MessageError::CountTooLarge {
        count: len as u64,
        limit: u64::from(u16::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header of every retail catacombs board push.
    const HEADER: &str = "d56a5012fd7adbdbce27d68aba595a83";

    fn payload(body: &str) -> Vec<u8> {
        let body = hex::decode(body).expect("hex");
        assert_eq!(hex::encode(&body[..16]), HEADER);
        body[16..].to_vec()
    }

    fn stats(slots: &[(u8, u32)]) -> ParticipantStats {
        ParticipantStats {
            left: false,
            slots: slots.iter().copied().collect(),
        }
    }

    /// `CATACOMBS5` 80.949 s (start: every catacombs slot 0), 157.558 s (a chest), 984.049 s
    /// (a late joiner's own count), 1675.555 s (the end) and 1687.557 s (participant 0 left).
    #[test]
    fn the_retail_stats_read_and_write_back() {
        let zero: Vec<(u8, u32)> = (44..56).map(|slot| (slot, 0)).collect();
        let end = [
            (47, 3),
            (48, 741),
            (49, 2),
            (50, 132),
            (52, 300),
            (53, 1),
            (54, 186),
        ];
        let cases = [
            (
                "d56a5012fd7adbdbce27d68aba595a8300020100000201ff0000000000e0ff0100000000000000000000000000ff0000000000e0ff01000000000000000000000000",
                WarboardStats {
                    sync: 2,
                    local_index: 1,
                    local: stats(&[]),
                    participants: vec![(1, stats(&zero)), (0, stats(&zero))],
                },
            ),
            (
                "d56a5012fd7adbdbce27d68aba595a83000201fc00000000004001000201fc0000000000400100fc00000000004001",
                WarboardStats {
                    sync: 2,
                    local_index: 1,
                    local: stats(&[(46, 1)]),
                    participants: vec![(1, stats(&[(46, 1)])), (0, stats(&[(46, 1)]))],
                },
            ),
            (
                "d56a5012fd7adbdbce27d68aba595a83000301fc00000000004003000302fc0000000000400101fc0000000000400300fc00000000004003",
                WarboardStats {
                    sync: 3,
                    local_index: 1,
                    local: stats(&[(46, 3)]),
                    participants: vec![
                        (2, stats(&[(46, 1)])),
                        (1, stats(&[(46, 3)])),
                        (0, stats(&[(46, 3)])),
                    ],
                },
            ),
            (
                "d56a5012fd7adbdbce27d68aba595a83000301fe000000000000ef03a50b028402ac0401ba02000302fe000000000000ef03a50b028402ac0401ba0201fe000000000000ef03a50b028402ac0401ba0200fe000000000000ef03a50b028402ac0401ba02",
                WarboardStats {
                    sync: 3,
                    local_index: 1,
                    local: stats(&end),
                    participants: vec![(2, stats(&end)), (1, stats(&end)), (0, stats(&end))],
                },
            ),
            (
                "d56a5012fd7adbdbce27d68aba595a830003010000010001",
                WarboardStats {
                    sync: 3,
                    local_index: 1,
                    local: stats(&[]),
                    participants: vec![(
                        0,
                        ParticipantStats {
                            left: true,
                            slots: BTreeMap::new(),
                        },
                    )],
                },
            ),
        ];
        for (body, expected) in cases {
            let payload = payload(body);
            assert_eq!(WarboardStats::decode_payload(&payload), Ok(expected.clone()));
            assert_eq!(expected.encode_payload(), Ok(payload));
        }
    }

    /// `CATACOMBS` 948.904 s: nobody extracted, the zero slots left out.
    #[test]
    fn an_end_with_zero_stats_names_only_the_others() {
        let payload = payload(
            "d56a5012fd7adbdbce27d68aba595a83000201fe000000000000a22c647c000201fe000000000000a22c647c00fe000000000000a22c647c",
        );
        let read = WarboardStats::decode_payload(&payload).expect("read");
        assert_eq!(read.local, stats(&[(48, 44), (52, 100), (54, 124)]));
    }

    /// A slot above the last can be neither written nor read.
    #[test]
    fn stat_slots_above_the_last_are_refused() {
        let write = |slots: &[(u8, u32)]| {
            WarboardStats {
                sync: 1,
                local_index: 0,
                local: stats(slots),
                participants: Vec::new(),
            }
            .encode_payload()
        };
        assert!(write(&[(44, 1), (57, 1)]).is_ok());
        assert!(write(&[(STAT_SLOTS, 1)]).is_err());
        // Slot 58 named in the mask (bit 59 of the integer).
        let mut writer = Writer::new();
        writer.u16(1).u8(0).wide_prefixed(1 << 59).wide_prefixed(1).u16(0);
        assert!(WarboardStats::decode_payload(&writer.finish()).is_err());
    }

    /// `CATACOMBS5` 80.949 s (3530, two participants) and 902.057 s (2968, the late joiner).
    #[test]
    fn the_retail_manifests_read_and_write_back() {
        let character = |hex_id: &str| {
            let mut id = [0; 16];
            id.copy_from_slice(&hex::decode(hex_id).expect("hex"));
            CharacterRef(Some(id))
        };
        let cases = [
            (
                "d56a5012fd7adbdbce27d68aba595a830002000205885a178271b2461490e5aed2711551dd058b04473164a340d7b19e99ee07f4e7250001000000000000",
                TeamManifest {
                    sync: 2,
                    participants: vec![
                        (character("885a178271b2461490e5aed2711551dd"), 0),
                        (character("8b04473164a340d7b19e99ee07f4e725"), 1),
                    ],
                },
            ),
            (
                "d56a5012fd7adbdbce27d68aba595a83000300010509fcd785ae7a45e0b3f4a17eb3378e0802000000000000",
                TeamManifest {
                    sync: 3,
                    participants: vec![(character("09fcd785ae7a45e0b3f4a17eb3378e08"), 2)],
                },
            ),
        ];
        for (body, expected) in cases {
            let payload = payload(body);
            assert_eq!(TeamManifest::decode_payload(&payload), Ok(expected.clone()));
            assert_eq!(expected.encode_payload(), Ok(payload));
        }
    }

    /// The Outpost Rush pushes of `opr_que_and_lost` (header `b98f65ef..`): 579.097 s (a 2968
    /// of a joiner of the second list), 579.499 s (an 839 naming three joiners with no slot) and
    /// 696.799 s (an 839 with the receiver's own score and both lists).
    #[test]
    fn the_retail_two_team_pushes_read_and_write_back() {
        const OPR_HEADER: &str = "b98f65efa00acc99581828d411cfec1b";
        let payload = |body: &str| {
            let body = hex::decode(body).expect("hex");
            assert_eq!(hex::encode(&body[..16]), OPR_HEADER);
            body[16..].to_vec()
        };
        let mut joiner = [0; 16];
        joiner.copy_from_slice(&hex::decode("d3342e33431d4d33932bff1f15e97a2c").expect("hex"));
        let manifest = TeamsManifest {
            sync: 9,
            lists: [Vec::new(), vec![(CharacterRef(Some(joiner)), 3)]],
        };
        let body = payload(
            "b98f65efa00acc99581828d411cfec1b000900000000000105d3342e33431d4d33932bff1f15e97a2c030000",
        );
        assert_eq!(TeamsManifest::decode_payload(&body), Ok(manifest.clone()));
        assert_eq!(manifest.encode_payload(), Ok(body));
        let cases = [
            (
                "b98f65efa00acc99581828d411cfec1b000903000202000300010400",
                TeamsWarboardStats {
                    sync: 9,
                    local_index: 3,
                    local: stats(&[]),
                    lists: [vec![(2, stats(&[])), (3, stats(&[]))], vec![(4, stats(&[]))]],
                },
            ),
            (
                "b98f65efa00acc99581828d411cfec1b001803020201050251020302020702a202",
                TeamsWarboardStats {
                    sync: 0x18,
                    local_index: 3,
                    local: stats(&[(0, 2)]),
                    lists: [
                        vec![(5, stats(&[(0, 81)]))],
                        vec![(3, stats(&[(0, 2)])), (7, stats(&[(0, 162)]))],
                    ],
                },
            ),
        ];
        for (body, expected) in cases {
            let body = payload(body);
            assert_eq!(TeamsWarboardStats::decode_payload(&body), Ok(expected.clone()));
            assert_eq!(expected.encode_payload(), Ok(body));
        }
    }

    #[test]
    fn a_second_manifest_list_that_is_not_empty_is_refused() {
        let body = [0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00];
        assert!(TeamsManifest::decode_payload(&body).is_err());
    }
}
