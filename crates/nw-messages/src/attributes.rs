// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::AttributeComponentReplicatedState` (type 129, player component 27): the five
//! attributes, their bonuses and the attribute points (progression note, section 6).
//!
//! Slot 0 holds four children (constructor `FUN_14276d1f0`): `attributes`, a map of attribute id
//! to final value (count, then `u32` id and `u32` value pairs); `attributeBonuses`, a replicated
//! list of (id, bonus) pairs; `placingBonuses`, a replicated list of one `u32` per attribute;
//! `persistentAttributeData` (`PersistentAttributesData`, `FUN_142797fc0`): the unspent points
//! (`u32`), `m_ignoreBackstoryPoints` and `m_hasRespec` (one byte each), the spent points per
//! attribute (count, then `u32` id and `u32` points pairs) and `m_chosenMagnifyBehavior`
//! (`u32`). Retail (newchar 2026-09-18) sends all four at login, children 1 to 3 at each level
//! up, and all four after each spend.

use crate::items::FullList;
use crate::list_delta::{ReplicatedList, write_child};
use crate::slots::{SLOT_0, children_mask, read_slot_0, write_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::AttributeComponentReplicatedState`.
pub const ATTRIBUTE_TYPE_INDEX: u64 = 129;
/// The player component of the attributes.
pub const ATTRIBUTE_COMPONENT: u64 = 27;

/// Children of slot 0.
const CHILDREN: u8 = 4;
const ATTRIBUTES_CHILD: u8 = 0;
const BONUSES_CHILD: u8 = 1;
const PLACING_BONUSES_CHILD: u8 = 2;
const FIELD: &str = "attributes";

/// An attribute id and a number of it: a final value, a bonus or spent points.
pub type AttributeValue = (u32, u32);

/// `PersistentAttributesData`: the points of a character.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersistentAttributes {
    /// `m_unspentAttributePoints`.
    pub unspent: u32,
    /// `m_ignoreBackstoryPoints`.
    pub ignore_backstory_points: bool,
    /// `m_hasRespec`.
    pub has_respec: bool,
    /// `m_spentPoints`: the points spent on each attribute, in the order they were first spent.
    pub spent: Vec<AttributeValue>,
    /// `m_chosenMagnifyBehavior`.
    pub magnify_behavior: u32,
}

impl PersistentAttributes {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            unspent: reader.u32()?,
            ignore_backstory_points: reader.boolean()?,
            has_respec: reader.boolean()?,
            spent: read_values(reader)?,
            magnify_behavior: reader.u32()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .u32(self.unspent)
            .boolean(self.ignore_backstory_points)
            .boolean(self.has_respec);
        write_values(writer, &self.spent);
        writer.u32(self.magnify_behavior);
    }
}

/// The attribute state; a child that is `None` is not sent, and a state without children sends
/// no slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributeState {
    /// Child 0: the final value of each attribute.
    pub attributes: Option<Vec<AttributeValue>>,
    /// Child 1: the bonuses of effects (attribute food).
    pub bonuses: Option<ReplicatedList<AttributeValue>>,
    /// Child 2: one placing bonus per attribute.
    pub placing_bonuses: Option<ReplicatedList<u32>>,
    /// Child 3: the points.
    pub persistent: Option<PersistentAttributes>,
}

impl AttributeState {
    /// The whole state, the lists with `version` (retail: at login and after a spend):
    /// `attributes` final values, no effect bonuses, `placing_bonuses` and `persistent`.
    pub fn full(
        version: u64,
        attributes: Vec<AttributeValue>,
        placing_bonuses: Vec<u32>,
        persistent: PersistentAttributes,
    ) -> Self {
        Self {
            attributes: Some(attributes),
            ..Self::points(version, placing_bonuses, persistent)
        }
    }

    /// Children 1 to 3, the lists with `version` (retail: at each level up, when only the
    /// unspent points changed).
    pub fn points(
        version: u64,
        placing_bonuses: Vec<u32>,
        persistent: PersistentAttributes,
    ) -> Self {
        Self {
            attributes: None,
            bonuses: Some(full_list(version, Vec::new())),
            placing_bonuses: Some(full_list(version, placing_bonuses)),
            persistent: Some(persistent),
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets another slot or child, or a list
    /// delta removes an element.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, CHILDREN, |reader, child| {
            match child {
                ATTRIBUTES_CHILD => state.attributes = Some(read_values(reader)?),
                BONUSES_CHILD => {
                    state.bonuses = Some(ReplicatedList::read(reader, FIELD, read_value)?);
                }
                PLACING_BONUSES_CHILD => {
                    state.placing_bonuses =
                        Some(ReplicatedList::read(reader, FIELD, |reader| reader.u32())?);
                }
                _ => state.persistent = Some(PersistentAttributes::read(reader)?),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] from the list writer.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let children = children_mask(&[
            self.attributes.is_some(),
            self.bonuses.is_some(),
            self.placing_bonuses.is_some(),
            self.persistent.is_some(),
        ]);
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        write_children(writer, children, CHILDREN, |writer, child| match child {
            ATTRIBUTES_CHILD => {
                write_values(writer, self.attributes.as_deref().unwrap_or_default());
                Ok(())
            }
            BONUSES_CHILD => write_child(writer, self.bonuses.as_ref(), |writer, (id, value)| {
                writer.u32(*id).u32(*value);
            }),
            PLACING_BONUSES_CHILD => {
                write_child(writer, self.placing_bonuses.as_ref(), |writer, value| {
                    writer.u32(*value);
                })
            }
            _ => {
                if let Some(persistent) = &self.persistent {
                    persistent.write(writer);
                }
                Ok(())
            }
        })
    }
}

fn full_list<T>(version: u64, elements: Vec<T>) -> ReplicatedList<T> {
    ReplicatedList::Full(FullList {
        version: Some(version),
        elements,
    })
}

fn read_value(reader: &mut Reader<'_>) -> Result<AttributeValue, MessageError> {
    Ok((reader.u32()?, reader.u32()?))
}

/// A map of (id, value) pairs: its count, then the pairs.
fn read_values(reader: &mut Reader<'_>) -> Result<Vec<AttributeValue>, MessageError> {
    let count = reader.wide_prefixed()?;
    // Each pair is 8 bytes, so a count beyond the body is cut off by the reads.
    let mut values = Vec::new();
    for _ in 0..count {
        values.push(read_value(reader)?);
    }
    Ok(values)
}

fn write_values(writer: &mut Writer, values: &[AttributeValue]) {
    // A `usize` count fits 64 bits.
    writer.wide_prefixed(values.len() as u64);
    for (id, value) in values {
        writer.u32(*id).u32(*value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::FIRST_VERSION;

    /// Constitution, Focus, Strength, Dexterity, Intelligence (client enum
    /// `CharacterAttributeType`, `FUN_142797540`).
    const STRENGTH: u32 = 2;
    const CONSTITUTION: u32 = 0;
    /// The retail order of the final values (newchar 2026-09-18).
    const RETAIL_ORDER: [u32; 5] = [4, 3, 2, 1, 0];
    const BASE: u32 = 5;
    const RETAIL_MAGNIFY: u32 = 5;
    /// The version of the retail third spend.
    const SPENT_VERSION: u64 = 24_440;

    /// Retail login of a new character (newchar 2026-09-18): every attribute 5, empty lists at
    /// version 3, no points, magnify behavior 5.
    const RETAIL_LOGIN: [u8; 82] = [
        0x01, 0x0f, 0x05, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x03,
        0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
        0x01, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x01,
        0x03, 0x00, 0x00, 0x01, 0x03, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
    ];
    /// Retail first level up (version 12276): children 1 to 3, 10 unspent points.
    const RETAIL_LEVEL_UP: [u8; 43] = [
        0x01, 0x0e, 0x00, 0x01, 0xb4, 0xbf, 0x00, 0x00, 0x01, 0xb4, 0xbf, 0x05, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
    ];
    const RETAIL_LEVEL_UP_VERSION: u64 = 12_276;

    fn values(of: impl Fn(u32) -> u32) -> Vec<AttributeValue> {
        RETAIL_ORDER.iter().map(|id| (*id, of(*id))).collect()
    }

    fn encode(state: &AttributeState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        writer.finish()
    }

    fn decode(bytes: &[u8]) -> AttributeState {
        let mut reader = Reader::new(bytes);
        let state = AttributeState::read(&mut reader).expect("read");
        reader.finish().expect("whole");
        state
    }

    fn persistent(unspent: u32, spent: Vec<AttributeValue>) -> PersistentAttributes {
        PersistentAttributes {
            unspent,
            spent,
            magnify_behavior: RETAIL_MAGNIFY,
            ..PersistentAttributes::default()
        }
    }

    #[test]
    fn the_login_of_a_new_character_is_the_retail_body() {
        let state = AttributeState::full(
            FIRST_VERSION,
            values(|_| BASE),
            vec![0; 5],
            persistent(0, Vec::new()),
        );
        assert_eq!(encode(&state), RETAIL_LOGIN);
        assert_eq!(decode(&RETAIL_LOGIN), state);
    }

    #[test]
    fn a_level_up_is_the_retail_body() {
        let state = AttributeState::points(
            RETAIL_LEVEL_UP_VERSION,
            vec![0; 5],
            persistent(10, Vec::new()),
        );
        assert_eq!(encode(&state), RETAIL_LEVEL_UP);
        assert_eq!(decode(&RETAIL_LEVEL_UP), state);
    }

    #[test]
    fn spent_points_keep_their_order_and_read_back() {
        // Retail after the third spend: Strength 20 then Constitution 5.
        let spent = vec![(STRENGTH, 20), (CONSTITUTION, 5)];
        let state = AttributeState::full(
            SPENT_VERSION,
            values(|id| BASE + spent.iter().find(|(s, _)| *s == id).map_or(0, |(_, p)| *p)),
            vec![0; 5],
            persistent(0, spent.clone()),
        );
        let decoded = decode(&encode(&state));
        assert_eq!(decoded, state);
        assert_eq!(decoded.persistent.expect("points").spent, spent);
        assert_eq!(encode(&AttributeState::default()), [0]);
    }
}
