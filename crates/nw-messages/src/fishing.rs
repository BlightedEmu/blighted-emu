// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fishing (run 15 fishing study, retail captures `open-world/Fishing`, `gather_node_20260912`
//! and `SQ-Brimestone-Sands-1` feed-the-fire).
//!
//! The server drives the fishing state machine. It replicates the steps in player component 18,
//! `MB::FishingComponentReplicatedState` (type 5338): slot 0 child 0 `stateTransitions`, a full
//! list of (sequence `u16`, state `u32`) entries that the client has not acknowledged yet. The
//! client acknowledges with 5339 (`OnFishingStateReceived`, the next sequence it expects), and the
//! server drops the acknowledged entries from the next list (retail: the list after an ack of
//! `0006` held only sequence 6). Retail sent the whole list after every change, with a new list
//! version each time.
//!
//! The facet calls: the client applies a bait (2309) and gets it back (557); the server sends
//! the bobber's landing place (2444) and, at the hook, the catch it rolled (3172). Every body
//! starts with the facet header of the player's fishing component ([`FISHING_FACET`]: the target
//! is the entity plus it; Observed on four characters).

use crate::facet::FacetHeader;
use crate::items::FullList;
use crate::slots::{SLOT_0, read_slot_0, write_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::FishingComponentReplicatedState`.
pub const FISHING_TYPE_INDEX: u64 = 5338;
/// The player component of the fishing state.
pub const FISHING_COMPONENT: u64 = 18;
/// The target-id constant of the player's `FishingComponent`: target = entity + it (Observed:
/// the same for all four characters of the fishing captures).
pub const FISHING_FACET: u64 = 0x0538_84ab_33a0_e17c;
/// `FishingComponentServerFacet_OnFishingStateReceived`.
pub const STATE_RECEIVED_TYPE_INDEX: u64 = 5339;
/// `FishingComponentServerFacet_ApplyBaitAction`.
pub const APPLY_BAIT_TYPE_INDEX: u64 = 2309;
/// `FishingComponentClientFacet_OnBobberPositionReceived`.
pub const BOBBER_POSITION_TYPE_INDEX: u64 = 2444;
/// `FishingComponentClientFacet_OnFishingCatchDataReceived`.
pub const CATCH_DATA_TYPE_INDEX: u64 = 3172;
/// `FishingComponentClientFacet_OnBaitDataReceived`.
pub const BAIT_DATA_TYPE_INDEX: u64 = 557;
/// Children of slot 0: `stateTransitions`.
const SLOT_0_CHILDREN: u8 = 1;
/// The child bit of `stateTransitions`.
const TRANSITIONS_CHILD: u16 = 1;
/// The slot byte of a state that sends no slot.
const NO_SLOTS: u8 = 0;
/// The most roll modifiers a catch carries: the client enum `FishingLootRollModifierType` has
/// `Count` 6.
pub const MAX_ROLL_MODIFIERS: u64 = 6;
/// Most entries of a transition list: the client keeps at most a `u16` sequence apart.
const MAX_TRANSITIONS: usize = u16::MAX as usize;
const FIELD: &str = "fishing";

/// The client enum `FishingState` (`FishingEnumsData`, `FUN_14311dcb0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum FishingState {
    /// 0.
    FishingDisabled = 0,
    /// 1: no pole in the hands.
    FishingUnequipped = 1,
    /// 2: the pole is being taken out.
    StartedEquippingFishing = 2,
    /// 3: the pole is in the hands.
    FishingEquipped = 3,
    /// 4: the bait window is open.
    ApplyingBait = 4,
    /// 5: the cast charges.
    CastStart = 5,
    /// 6: the cast is let go.
    CastEnd = 6,
    /// 7: the bobber landed in water.
    CastHitWater = 7,
    /// 8: waiting for a fish.
    FishingStarted = 8,
    /// 9: a fish nibbles.
    FishNibbleWindowOpen = 9,
    /// 10: a fish bites: the hook may be set.
    FishBiteWindowOpen = 10,
    /// 11: the hook is set.
    HookHit = 11,
    /// 12: the player reels.
    ReelingActive = 12,
    /// 13: the player lets the line rest.
    ReelingRecover = 13,
    /// 14: the bobber landed on land.
    FailedCastHitLand = 14,
    /// 15: the hook was set during the nibble.
    FailedHookMissedTooEarly = 15,
    /// 16: the bite window closed without the hook.
    FailedHookMissedTooLate = 16,
    /// 17: the line tension reached its top.
    FailedLineBreak = 17,
    /// 18: the fish got away.
    FailedFishSwamAway = 18,
    /// 19: the fish is caught.
    SucceededFishCaught = 19,
    /// 20: the cast is over.
    FishingEnded = 20,
}

impl FishingState {
    /// Every state in client enum order: index = the wire value.
    const ALL: [Self; 21] = [
        Self::FishingDisabled,
        Self::FishingUnequipped,
        Self::StartedEquippingFishing,
        Self::FishingEquipped,
        Self::ApplyingBait,
        Self::CastStart,
        Self::CastEnd,
        Self::CastHitWater,
        Self::FishingStarted,
        Self::FishNibbleWindowOpen,
        Self::FishBiteWindowOpen,
        Self::HookHit,
        Self::ReelingActive,
        Self::ReelingRecover,
        Self::FailedCastHitLand,
        Self::FailedHookMissedTooEarly,
        Self::FailedHookMissedTooLate,
        Self::FailedLineBreak,
        Self::FailedFishSwamAway,
        Self::SucceededFishCaught,
        Self::FishingEnded,
    ];

    /// The state of wire value `value`; `None` for a value the client enum does not have.
    pub fn of(value: u32) -> Option<Self> {
        usize::try_from(value)
            .ok()
            .and_then(|index| Self::ALL.get(index))
            .copied()
    }

    /// The wire value.
    pub const fn value(self) -> u32 {
        self as u32
    }
}

/// One step of the state machine: its sequence and the state entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateTransition {
    /// The sequence, from 0, one per step of the player's session.
    pub sequence: u16,
    /// The state entered.
    pub state: FishingState,
}

impl StateTransition {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let sequence = reader.u16()?;
        let value = reader.u32()?;
        let state = FishingState::of(value).ok_or(MessageError::BadTag {
            field: FIELD,
            tag: u8::try_from(value).unwrap_or(u8::MAX),
        })?;
        Ok(Self { sequence, state })
    }

    fn write(&self, writer: &mut Writer) {
        writer.u16(self.sequence).u32(self.state.value());
    }
}

/// `MB::FishingComponentReplicatedState`; `None` sends no slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FishingComponentState {
    /// Slot 0 child 0 `stateTransitions`: the steps the client has not acknowledged.
    pub transitions: Option<FullList<StateTransition>>,
}

impl FishingComponentState {
    /// The state that lists `transitions` at list version `version`.
    pub fn listing(version: u64, transitions: Vec<StateTransition>) -> Self {
        Self {
            transitions: Some(FullList {
                version: Some(version),
                elements: transitions,
            }),
        }
    }

    /// Reads a body.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets another slot or child, the list
    /// is a delta or a state is not of the client enum.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, SLOT_0_CHILDREN, |reader, _| {
            let list = FullList::read(reader, StateTransition::read)?;
            if list.elements.len() > MAX_TRANSITIONS {
                return Err(MessageError::LengthMismatch { field: FIELD });
            }
            state.transitions = Some(list);
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the body.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the list does not encode.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let Some(list) = &self.transitions else {
            writer.u8(NO_SLOTS);
            return Ok(());
        };
        writer.u8(SLOT_0);
        write_children(writer, TRANSITIONS_CHILD, SLOT_0_CHILDREN, |writer, _| {
            list.write(writer, |writer, transition| {
                transition.write(writer);
                Ok(())
            })
        })
    }
}

/// A fishing call of the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FishingClientMsg {
    /// 5339: the client has every step before `next_sequence`.
    StateReceived {
        /// The header.
        header: FacetHeader,
        /// The next sequence the client expects.
        next_sequence: u16,
    },
    /// 2309: the bait window closes on a bait.
    ApplyBait {
        /// The header.
        header: FacetHeader,
        /// The bait: the CRC-32 of its lowercase `FishingBaitData` id.
        bait: u32,
    },
}

impl FishingClientMsg {
    /// Decodes `body` when `type_index` is a fishing call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |read_rest: fn(FacetHeader, &mut Reader<'_>) -> Result<Self, MessageError>| {
            let mut reader = Reader::new(body);
            let message = read_rest(FacetHeader::read(&mut reader)?, &mut reader)?;
            reader.finish()?;
            Ok(message)
        };
        Some(match type_index {
            STATE_RECEIVED_TYPE_INDEX => read(|header, reader| {
                Ok(Self::StateReceived {
                    header,
                    next_sequence: reader.u16()?,
                })
            }),
            APPLY_BAIT_TYPE_INDEX => read(|header, reader| {
                Ok(Self::ApplyBait {
                    header,
                    bait: reader.u32()?,
                })
            }),
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::StateReceived { .. } => STATE_RECEIVED_TYPE_INDEX,
            Self::ApplyBait { .. } => APPLY_BAIT_TYPE_INDEX,
        }
    }

    /// The header.
    pub const fn header(&self) -> FacetHeader {
        match self {
            Self::StateReceived { header, .. } | Self::ApplyBait { header, .. } => *header,
        }
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        match self {
            Self::StateReceived { next_sequence, .. } => writer.u16(*next_sequence),
            Self::ApplyBait { bait, .. } => writer.u32(*bait),
        };
        writer.finish()
    }
}

/// One roll modifier of a catch: the client enum `FishingLootRollModifierType` and the bonus
/// the loot roll got (on the tables' `MaxRoll` scale).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollModifier {
    /// `FishingLootRollModifierType`: 0 `FishSize`, 1 `FishRarity`, 2 `SummerFishRarity`, 3 the
    /// loot table diversion (the catch log's order; Observed 0 in every capture).
    pub kind: u8,
    /// The bonus.
    pub value: u32,
}

/// What the hook caught (3172), as the client's catch log names it: the catch item, the fish's
/// behavior, its length and weight and the roll modifiers.
#[derive(Debug, Clone, PartialEq)]
pub struct FishCatch {
    /// The CRC-32 of the lowercase item id caught (`FishingSalmonMediumT1`).
    pub item: u32,
    /// The CRC-32 of the lowercase `FishingBehaviorsData` id (`SporadicEasy`).
    pub behavior: u32,
    /// The behavior's `FishBehaviorPattern`: character `i` of the pattern is bit `i`.
    pub behavior_bits: u8,
    /// The length.
    pub length: f32,
    /// The weight.
    pub weight: f32,
    /// The roll modifiers.
    pub modifiers: Vec<RollModifier>,
}

impl FishCatch {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let item = reader.u32()?;
        let behavior = reader.u32()?;
        let behavior_bits = reader.u8()?;
        let length = reader.f32()?;
        let weight = reader.f32()?;
        let count = reader.wide_prefixed()?;
        if count > MAX_ROLL_MODIFIERS {
            return Err(MessageError::CountTooLarge {
                count,
                limit: MAX_ROLL_MODIFIERS,
            });
        }
        let modifiers = (0..count)
            .map(|_| {
                Ok(RollModifier {
                    kind: reader.u8()?,
                    value: reader.u32()?,
                })
            })
            .collect::<Result<_, MessageError>>()?;
        Ok(Self {
            item,
            behavior,
            behavior_bits,
            length,
            weight,
            modifiers,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .u32(self.item)
            .u32(self.behavior)
            .u8(self.behavior_bits)
            .f32(self.length)
            .f32(self.weight);
        // At most [`MAX_ROLL_MODIFIERS`]: a `usize` count fits 64 bits.
        writer.wide_prefixed(self.modifiers.len() as u64);
        for modifier in &self.modifiers {
            writer.u8(modifier.kind).u32(modifier.value);
        }
    }
}

/// A fishing call of the server.
#[derive(Debug, Clone, PartialEq)]
pub enum FishingServerMsg {
    /// 2444: where the bobber landed.
    BobberPosition {
        /// The header.
        header: FacetHeader,
        /// The landing place, m (x, y, z; z is the water surface).
        position: [f32; 3],
        /// The hotspot the bobber is in: the CRC-32 of its lowercase `FishingHotspotsData` id;
        /// 0 for none (Observed 0 in every capture; the client passes it to `ModifyHotspot`).
        hotspot: u32,
    },
    /// 3172: what the hook caught.
    CatchData {
        /// The header.
        header: FacetHeader,
        /// The catch.
        catch: FishCatch,
    },
    /// 557: the bait the player applied.
    BaitData {
        /// The header.
        header: FacetHeader,
        /// The bait: the CRC-32 of its lowercase `FishingBaitData` id.
        bait: u32,
    },
}

impl FishingServerMsg {
    /// Whether `type_index` is a fishing call of the server.
    pub const fn is_type(type_index: u64) -> bool {
        matches!(
            type_index,
            BOBBER_POSITION_TYPE_INDEX | CATCH_DATA_TYPE_INDEX | BAIT_DATA_TYPE_INDEX
        )
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::BobberPosition { .. } => BOBBER_POSITION_TYPE_INDEX,
            Self::CatchData { .. } => CATCH_DATA_TYPE_INDEX,
            Self::BaitData { .. } => BAIT_DATA_TYPE_INDEX,
        }
    }

    /// Decodes the body of the call of `type_index`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match its layout or `type_index` is not a
    /// fishing call ([`MessageError::BadTag`]).
    pub fn decode(type_index: u64, body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let message = match type_index {
            BOBBER_POSITION_TYPE_INDEX => Self::BobberPosition {
                header,
                position: [reader.f32()?, reader.f32()?, reader.f32()?],
                hotspot: reader.u32()?,
            },
            CATCH_DATA_TYPE_INDEX => Self::CatchData {
                header,
                catch: FishCatch::read(&mut reader)?,
            },
            BAIT_DATA_TYPE_INDEX => Self::BaitData {
                header,
                bait: reader.u32()?,
            },
            _ => {
                return Err(MessageError::BadTag {
                    field: FIELD,
                    tag: u8::try_from(type_index).unwrap_or(u8::MAX),
                });
            }
        };
        reader.finish()?;
        Ok(message)
    }

    /// The body bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::BobberPosition {
                header,
                position,
                hotspot,
            } => {
                header.write(&mut writer);
                for axis in position {
                    writer.f32(*axis);
                }
                writer.u32(*hotspot);
            }
            Self::CatchData { header, catch } => {
                header.write(&mut writer);
                catch.write(&mut writer);
            }
            Self::BaitData { header, bait } => {
                header.write(&mut writer);
                writer.u32(*bait);
            }
        }
        writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header (synthetic ids).
    const HEADER: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    /// The retail login state: an empty list at version 3943 (`open-world/Fishing`, 353 ms).
    const RETAIL_LOGIN: [u8; 6] = [0x01, 0x01, 0x00, 0x01, 0xa7, 0x3d];
    const RETAIL_LOGIN_VERSION: u64 = 3943;
    /// The retail toggle: sequence 0, `StartedEquippingFishing`, version 4137 (9553 ms).
    const RETAIL_EQUIPPING: [u8; 13] = [
        0x01, 0x01, 0x00, 0x01, 0xa9, 0x40, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
    ];
    const RETAIL_EQUIPPING_VERSION: u64 = 4137;
    /// The retail landing: sequences 4 and 5, `CastHitWater` and `FishingStarted`, version 4263
    /// (15687 ms).
    const RETAIL_LANDED: [u8; 19] = [
        0x01, 0x01, 0x00, 0x01, 0xa7, 0x42, 0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x07, 0x00, 0x05,
        0x00, 0x00, 0x00, 0x08,
    ];
    const RETAIL_LANDED_VERSION: u64 = 4263;
    /// Retail 3172 arguments: a medium salmon on `SporadicEasy` (bits `0x92`), 28.74 long,
    /// 19.57 heavy, FishSize 25000 and FishRarity 12650 (28704 ms).
    const RETAIL_CATCH: [u8; 38] = [
        0x2e, 0x7f, 0x46, 0x3d, 0xd2, 0x6d, 0xad, 0x61, 0x92, 0x41, 0xe5, 0xf4, 0x39, 0x41, 0x9c,
        0x8c, 0x08, 0x04, 0x00, 0x00, 0x00, 0x61, 0xa8, 0x01, 0x00, 0x00, 0x31, 0x6a, 0x02, 0x00,
        0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00,
    ];
    /// The CRC-32 of `fishingsalmonmediumt1` and of `sporadiceasy`.
    const SALMON_MEDIUM: u32 = 0x2e7f_463d;
    const SPORADIC_EASY: u32 = 0xd26d_ad61;
    /// Retail 2444 arguments (15637 ms): x 9104.67, y 2778.32, z 50.78 (the surface), no hotspot.
    const RETAIL_BOBBER: [u8; 16] = [
        0x46, 0x0e, 0x42, 0xb2, 0x45, 0x2d, 0xa5, 0x21, 0x42, 0x4b, 0x23, 0x92, 0x00, 0x00, 0x00,
        0x00,
    ];
    /// The CRC-32 of `baitfreshfireflyt3` (retail 2309 and 557, 75152 ms).
    const FIREFLY: u32 = 0xa9d9_f243;

    fn body(arguments: &[u8]) -> Vec<u8> {
        [&HEADER[..], arguments].concat()
    }

    fn header() -> FacetHeader {
        FacetHeader::read(&mut Reader::new(&HEADER)).expect("header")
    }

    fn encode(state: &FishingComponentState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        writer.finish()
    }

    fn decode(bytes: &[u8]) -> FishingComponentState {
        let mut reader = Reader::new(bytes);
        let state = FishingComponentState::read(&mut reader).expect("read");
        reader.finish().expect("whole");
        state
    }

    fn step(sequence: u16, state: FishingState) -> StateTransition {
        StateTransition { sequence, state }
    }

    #[test]
    fn the_retail_state_lists_read_and_write_back() {
        let login = [&RETAIL_LOGIN[..], &[0x00]].concat();
        let empty = FishingComponentState::listing(RETAIL_LOGIN_VERSION, Vec::new());
        assert_eq!(decode(&login), empty);
        assert_eq!(encode(&empty), login);
        let equipping = FishingComponentState::listing(
            RETAIL_EQUIPPING_VERSION,
            vec![step(0, FishingState::StartedEquippingFishing)],
        );
        assert_eq!(decode(&RETAIL_EQUIPPING), equipping);
        assert_eq!(encode(&equipping), RETAIL_EQUIPPING);
        let landed = FishingComponentState::listing(
            RETAIL_LANDED_VERSION,
            vec![
                step(4, FishingState::CastHitWater),
                step(5, FishingState::FishingStarted),
            ],
        );
        assert_eq!(decode(&RETAIL_LANDED), landed);
        assert_eq!(encode(&landed), RETAIL_LANDED);
        assert_eq!(encode(&FishingComponentState::default()), [0x00]);
    }

    #[test]
    fn the_states_keep_the_client_enum_values() {
        assert_eq!(FishingState::FishingUnequipped.value(), 1);
        assert_eq!(FishingState::StartedEquippingFishing.value(), 2);
        assert_eq!(FishingState::FishingEquipped.value(), 3);
        assert_eq!(FishingState::HookHit.value(), 11);
        assert_eq!(FishingState::FailedHookMissedTooLate.value(), 16);
        assert_eq!(FishingState::SucceededFishCaught.value(), 19);
        assert_eq!(FishingState::FishingEnded.value(), 20);
        assert_eq!(FishingState::of(21), None);
        for value in 0..21 {
            assert_eq!(FishingState::of(value).map(FishingState::value), Some(value));
        }
        let unknown = [0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x16];
        assert!(FishingComponentState::read(&mut Reader::new(&unknown)).is_err());
    }

    #[test]
    fn the_retail_calls_read_and_write_back() {
        let ack = FishingClientMsg::decode(STATE_RECEIVED_TYPE_INDEX, &body(&[0x00, 0x06]))
            .expect("a fishing call")
            .expect("reads");
        assert_eq!(
            ack,
            FishingClientMsg::StateReceived {
                header: header(),
                next_sequence: 6,
            }
        );
        assert_eq!(ack.encode(), body(&[0x00, 0x06]));
        let bait = FishingClientMsg::decode(APPLY_BAIT_TYPE_INDEX, &body(&FIREFLY.to_be_bytes()))
            .expect("a fishing call")
            .expect("reads");
        assert_eq!(bait.type_index(), APPLY_BAIT_TYPE_INDEX);
        assert_eq!(
            bait,
            FishingClientMsg::ApplyBait {
                header: header(),
                bait: FIREFLY,
            }
        );
        assert!(FishingClientMsg::decode(1, &HEADER).is_none());
        assert!(matches!(
            FishingClientMsg::decode(STATE_RECEIVED_TYPE_INDEX, &body(&[0, 6, 0])),
            Some(Err(_))
        ));
    }

    #[test]
    fn the_retail_server_calls_read_and_write_back() {
        let catch = FishingServerMsg::decode(CATCH_DATA_TYPE_INDEX, &body(&RETAIL_CATCH))
            .expect("reads");
        let FishingServerMsg::CatchData { catch: fish, .. } = &catch else {
            panic!("a catch");
        };
        assert_eq!(fish.item, SALMON_MEDIUM);
        assert_eq!(fish.behavior, SPORADIC_EASY);
        assert_eq!(fish.behavior_bits, 0x92);
        assert!((fish.length - 28.744_25).abs() < 1e-3);
        assert_eq!(
            fish.modifiers,
            [
                RollModifier {
                    kind: 0,
                    value: 25_000
                },
                RollModifier {
                    kind: 1,
                    value: 12_650
                },
                RollModifier { kind: 2, value: 0 },
                RollModifier { kind: 3, value: 0 },
            ]
        );
        assert_eq!(catch.encode(), body(&RETAIL_CATCH));
        let bobber = FishingServerMsg::decode(BOBBER_POSITION_TYPE_INDEX, &body(&RETAIL_BOBBER))
            .expect("reads");
        let FishingServerMsg::BobberPosition {
            position, hotspot, ..
        } = bobber
        else {
            panic!("a bobber");
        };
        assert!((position[2] - 50.784_74).abs() < 1e-3);
        assert_eq!(hotspot, 0);
        assert_eq!(bobber.encode(), body(&RETAIL_BOBBER));
        let bait = FishingServerMsg::BaitData {
            header: header(),
            bait: FIREFLY,
        };
        assert_eq!(bait.encode(), body(&FIREFLY.to_be_bytes()));
        assert_eq!(
            FishingServerMsg::decode(BAIT_DATA_TYPE_INDEX, &bait.encode()),
            Ok(bait)
        );
        assert!(FishingServerMsg::is_type(CATCH_DATA_TYPE_INDEX));
        assert!(!FishingServerMsg::is_type(STATE_RECEIVED_TYPE_INDEX));
        assert!(FishingServerMsg::decode(1, &HEADER).is_err());
    }
}
