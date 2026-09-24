// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Javelin::GameEventComponentReplicatedState` (type 497, own player component 68): the last
//! game events of the player (`game_event_data` rows: kills, map discoveries, lore, rank ups) and
//! the daily bonuses it used (conversation-notify note, section 4).
//!
//! Slot 0 has two children (constructor `FUN_1431e6880`): `gameEvents` (a replicated list of at
//! most 10 [`GameEventEntry`], reader `FUN_1431d3620`, full form `FUN_1432068f0`; retail uses it
//! as a ring: a new event replaces index 0 to 9 in turn) and `dailyBonusesUsed` (a replicated
//! list of at most 5 [`DailyBonus`], reader `FUN_1432065a0`, full form `FUN_1431e0e20`).

use crate::combat::{RECAP_LAST_LIST_LIMIT, RECAP_LIST_LIMIT, read_list, write_list};
use crate::items::read_number;
use crate::list_delta::ReplicatedList;
use crate::slots::{SLOT_0, children_mask, read_slot_0, write_known_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `Javelin::GameEventComponentReplicatedState`.
pub const GAME_EVENT_TYPE_INDEX: u64 = 497;
/// The own player component of the game events.
pub const GAME_EVENT_COMPONENT: u64 = 68;
/// The most game events of the full form (`FUN_1432068f0` refuses a count above 10).
pub const MAX_GAME_EVENTS: usize = 10;
/// The most daily bonuses of the full form (`FUN_1431e0e20` refuses a count above 5).
pub const MAX_DAILY_BONUSES: usize = 5;

/// Children of slot 0.
const CHILDREN: u8 = 2;
const EVENTS_CHILD: u8 = 0;
/// The fewest bytes of one reward item: the fixed fields and three empty list counts.
const MIN_REWARD_LEN: usize = 25;
const FIELD: &str = "game events";

/// One item of a game event's rewards (`FUN_145ce9c50`, `FUN_145b50570`, `FUN_145b52460`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewardItem {
    /// CRC-32 of the lowercase item id.
    pub item: u32,
    /// A big-endian `u16` (name Unknown).
    pub word: u16,
    /// The quantity.
    pub quantity: u32,
    /// A flag (name Unknown).
    pub flag_a: bool,
    /// A big-endian `u32` (name Unknown).
    pub value_a: u32,
    /// A big-endian `u32` (name Unknown).
    pub value_b: u32,
    /// A flag (name Unknown).
    pub flag_b: bool,
    /// A flag (name Unknown).
    pub flag_c: bool,
    /// Fewer than 6 big-endian `u32` (name Unknown).
    pub list_a: Vec<u32>,
    /// Fewer than 6 big-endian `u32` (name Unknown).
    pub list_b: Vec<u32>,
    /// A flag (name Unknown).
    pub flag_d: bool,
    /// Fewer than 4 big-endian `u32` (name Unknown).
    pub list_c: Vec<u32>,
}

impl RewardItem {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            item: reader.u32()?,
            word: reader.u16()?,
            quantity: reader.u32()?,
            flag_a: reader.boolean()?,
            value_a: reader.u32()?,
            value_b: reader.u32()?,
            flag_b: reader.boolean()?,
            flag_c: reader.boolean()?,
            list_a: read_list(reader, RECAP_LIST_LIMIT)?,
            list_b: read_list(reader, RECAP_LIST_LIMIT)?,
            flag_d: reader.boolean()?,
            list_c: read_list(reader, RECAP_LAST_LIST_LIMIT)?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer
            .u32(self.item)
            .u16(self.word)
            .u32(self.quantity)
            .boolean(self.flag_a)
            .u32(self.value_a)
            .u32(self.value_b)
            .boolean(self.flag_b)
            .boolean(self.flag_c);
        write_list(writer, &self.list_a, RECAP_LIST_LIMIT)?;
        write_list(writer, &self.list_b, RECAP_LIST_LIMIT)?;
        writer.boolean(self.flag_d);
        write_list(writer, &self.list_c, RECAP_LAST_LIST_LIMIT)
    }
}

/// One game event the player triggered (`FUN_1431d3490`, `FUN_1431a8920`, `FUN_1431a8450`).
/// Values of 88,000 retail entries are given per field.
#[derive(Debug, Clone, PartialEq)]
pub struct GameEventEntry {
    /// When it happened: ns since 1970-01-01.
    pub time_ns: u64,
    /// CRC-32 of the lowercase `EventID` (`game_event_data`).
    pub event: u32,
    /// A big-endian `f32` (Inferred: the experience; `Chart_Location` 75 to 400,
    /// `Read_LoreItem` 120 to 710, 1.0 on kills).
    pub amount_a: f32,
    /// A big-endian `f32` (name Unknown; retail 1.0).
    pub amount_b: f32,
    /// A big-endian `u32` (name Unknown; `KillEvent` 1012 to 1309, `SalvageCoinGrantT5` 300).
    pub value_a: u32,
    /// A flag (name Unknown; true on 1,807 of 28,418 kills).
    pub flag: bool,
    /// A big-endian `u32` (Inferred: the territory standing; `Chart_Location` 50 to 56, its
    /// `TerritoryStanding` column is 50).
    pub standing: u32,
    /// A big-endian `u32` (name Unknown; retail 0).
    pub value_c: u32,
    /// A big-endian `u32` (name Unknown; retail 0).
    pub value_d: u32,
    /// The reward items (retail: none on 80,542 entries, 1 to 9 on 7,519).
    pub rewards: Vec<RewardItem>,
    /// A byte (name Unknown; retail 0).
    pub byte_a: u8,
    /// A byte (name Unknown; retail 0).
    pub byte_b: u8,
    /// A big-endian `u32` (name Unknown; kills 0, 10 or 15).
    pub value_f: u32,
    /// A big-endian `u16` (Inferred: the territory id; 0 outside a territory).
    pub territory: u16,
    /// A big-endian `u32` (name Unknown; kills 0 or 15).
    pub value_g: u32,
    /// A big-endian `u32` (name Unknown; retail 0).
    pub value_h: u32,
}

impl GameEventEntry {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let time_ns = reader.u64()?;
        let event = reader.u32()?;
        let amount_a = reader.f32()?;
        let amount_b = reader.f32()?;
        let value_a = reader.u32()?;
        let flag = reader.boolean()?;
        let standing = reader.u32()?;
        let value_c = reader.u32()?;
        let value_d = reader.u32()?;
        let count = read_number(reader)?;
        // Each reward takes at least `MIN_REWARD_LEN` bytes.
        if count as usize > reader.remaining() / MIN_REWARD_LEN {
            return Err(MessageError::LengthMismatch { field: FIELD });
        }
        let rewards = (0..count)
            .map(|_| RewardItem::read(reader))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            time_ns,
            event,
            amount_a,
            amount_b,
            value_a,
            flag,
            standing,
            value_c,
            value_d,
            rewards,
            byte_a: reader.u8()?,
            byte_b: reader.u8()?,
            value_f: reader.u32()?,
            territory: reader.u16()?,
            value_g: reader.u32()?,
            value_h: reader.u32()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer
            .u64(self.time_ns)
            .u32(self.event)
            .f32(self.amount_a)
            .f32(self.amount_b)
            .u32(self.value_a)
            .boolean(self.flag)
            .u32(self.standing)
            .u32(self.value_c)
            .u32(self.value_d)
            // A `usize` count fits 64 bits.
            .wide_prefixed(self.rewards.len() as u64);
        for reward in &self.rewards {
            reward.write(writer)?;
        }
        writer
            .u8(self.byte_a)
            .u8(self.byte_b)
            .u32(self.value_f)
            .u16(self.territory)
            .u32(self.value_g)
            .u32(self.value_h);
        Ok(())
    }
}

/// One daily bonus the player used (`FUN_1431e0e20`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyBonus {
    /// CRC-32 of the lowercase `RewardModifiers` id (`reward_modifier_data`; retail
    /// `FactionMissionBonus` only).
    pub bonus: u32,
    /// How many times it was used (retail 1 to 3).
    pub used: u8,
}

impl DailyBonus {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            bonus: reader.u32()?,
            used: reader.u8()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer.u32(self.bonus).u8(self.used);
    }
}

/// The game event state; `None` sends no change of that child.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameEventState {
    /// `gameEvents`: the last events, as a ring of [`MAX_GAME_EVENTS`].
    pub game_events: Option<ReplicatedList<GameEventEntry>>,
    /// `dailyBonusesUsed`.
    pub daily_bonuses_used: Option<ReplicatedList<DailyBonus>>,
}

impl GameEventState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets a slot other than 0, a list does
    /// not read, or a full list is longer than the client takes.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, CHILDREN, |reader, child| {
            if child == EVENTS_CHILD {
                let events = ReplicatedList::read(reader, FIELD, GameEventEntry::read)?;
                check_full_length(&events, MAX_GAME_EVENTS)?;
                state.game_events = Some(events);
            } else {
                let bonuses = ReplicatedList::read(reader, FIELD, DailyBonus::read)?;
                check_full_length(&bonuses, MAX_DAILY_BONUSES)?;
                state.daily_bonuses_used = Some(bonuses);
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a full list is longer than the client takes or a reward
    /// list is too long.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        if let Some(events) = &self.game_events {
            check_full_length(events, MAX_GAME_EVENTS)?;
        }
        if let Some(bonuses) = &self.daily_bonuses_used {
            check_full_length(bonuses, MAX_DAILY_BONUSES)?;
        }
        let children = children_mask(&[
            self.game_events.is_some(),
            self.daily_bonuses_used.is_some(),
        ]);
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        let mut result = Ok(());
        write_known_children(writer, children, CHILDREN, |writer, child| {
            if child == EVENTS_CHILD {
                if let Some(events) = &self.game_events {
                    result = events.write(writer, |writer, event| event.write(writer));
                }
            } else if let Some(bonuses) = &self.daily_bonuses_used {
                result = bonuses.write(writer, |writer, bonus| {
                    bonus.write(writer);
                    Ok(())
                });
            }
        });
        result
    }
}

/// Refuses a full list longer than `limit`.
fn check_full_length<T>(list: &ReplicatedList<T>, limit: usize) -> Result<(), MessageError> {
    match list {
        ReplicatedList::Full(full) if full.elements.len() > limit => {
            Err(MessageError::CountTooLarge {
                // `usize` values fit 64 bits.
                count: full.elements.len() as u64,
                limit: limit as u64,
            })
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::FullList;
    use crate::list_delta::ListChange;

    /// CRC-32 of `chart_location`.
    const CHART_LOCATION: u32 = 0x09bb_ecb4;
    /// A synthetic event time: ns since 1970-01-01.
    const TIME_NS: u64 = 1_781_367_053_382_130_100;
    /// Bytes of an entry without rewards.
    const ENTRY_LEN: usize = 54;

    fn chart(territory: u16) -> GameEventEntry {
        GameEventEntry {
            time_ns: TIME_NS,
            event: CHART_LOCATION,
            amount_a: 75.0,
            amount_b: 1.0,
            value_a: 0,
            flag: false,
            standing: 50,
            value_c: 0,
            value_d: 0,
            rewards: Vec::new(),
            byte_a: 0,
            byte_b: 0,
            value_f: 0,
            territory,
            value_g: 0,
            value_h: 0,
        }
    }

    fn round_trip(state: &GameEventState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes);
        assert_eq!(&GameEventState::read(&mut reader).expect("read"), state);
        reader.finish().expect("whole body");
        bytes
    }

    #[test]
    fn the_new_character_body_is_an_empty_bonus_list() {
        // Retail first body `01 02 00 01 <version> 00` (player components, first bodies).
        let state = GameEventState {
            game_events: None,
            daily_bonuses_used: Some(ReplicatedList::Full(FullList::first(Vec::new()))),
        };
        assert_eq!(round_trip(&state), [0x01, 0x02, 0x00, 0x01, 0x03, 0x00]);
    }

    #[test]
    fn a_new_event_is_one_delta_entry_of_the_ring() {
        let state = GameEventState {
            game_events: Some(ReplicatedList::Changes(vec![ListChange {
                index: 4,
                version: 9,
                value: chart(12),
            }])),
            daily_bonuses_used: None,
        };
        let bytes = round_trip(&state);
        // Slot, child 0, count 1, mask 1, index 4, version flag, version 9, then the entry.
        assert_eq!(&bytes[..7], [0x01, 0x01, 0x01, 0x01, 0x04, 0x01, 0x09]);
        assert_eq!(bytes.len(), 7 + ENTRY_LEN);
    }

    #[test]
    fn rewards_round_trip_and_long_full_lists_are_refused() {
        let mut entry = chart(4);
        entry.rewards = vec![RewardItem {
            item: 0x0b21_32dc,
            word: 0,
            quantity: 2,
            flag_a: false,
            value_a: u32::MAX,
            value_b: 0x33,
            flag_b: false,
            flag_c: false,
            list_a: Vec::new(),
            list_b: Vec::new(),
            flag_d: false,
            list_c: Vec::new(),
        }];
        let state = GameEventState {
            game_events: Some(ReplicatedList::Full(FullList {
                version: Some(5),
                elements: vec![entry],
            })),
            daily_bonuses_used: Some(ReplicatedList::Full(FullList {
                version: Some(5),
                elements: vec![DailyBonus {
                    bonus: 0x85d3_0f59,
                    used: 2,
                }],
            })),
        };
        let bytes = round_trip(&state);
        assert_eq!(bytes.len(), 2 + 4 + ENTRY_LEN + MIN_REWARD_LEN + 4 + 5);
        let long = GameEventState {
            game_events: Some(ReplicatedList::Full(FullList {
                version: None,
                elements: vec![chart(0); MAX_GAME_EVENTS + 1],
            })),
            daily_bonuses_used: None,
        };
        assert!(matches!(
            long.write(&mut Writer::new()),
            Err(MessageError::CountTooLarge { .. })
        ));
    }
}
