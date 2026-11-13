// SPDX-License-Identifier: AGPL-3.0-or-later
//! `AbilityInstanceTrackingComponentReplicatedState` (type 2267, own player component 15): the
//! placed ability instances of the player (Sacred Ground, Beacon, Ice Storm, a trap) that are in
//! the world now.
//!
//! Observed (client `FUN_142593330`): `m_abilityInstances` is 11 rows of 5 slots, one row per
//! `AbilityInstanceType` (`FUN_14259b2d0`: 0 none, 1 `IcePylon`, 2 `RuneSandworm`, 3
//! `LifeStaffSacredGround`, 4 `LifeStaffBeacon`, 5 `IceGauntletIceStorm`, 6
//! `IceGauntletIceShower`, 7 `VoidGauntletOblivion`, 8 `FireStaffMeteorShower`, 9
//! `RuneSpinningSwords`, 10 `StealthPhantom`). So child `5 * type + slot` of slot 0 is one
//! instance: the time it was made (ns since 2000-01-01, the server clock), the raw id (`GdeRef`)
//! of its spell entity, and that entity's id (the raw id's first 8 bytes as a little-endian
//! `u64`), all big-endian. An empty slot is a zero time, a zero raw id and the entity id
//! `0xffffffff` (the defaults of `FUN_142592df0`).
//!
//! Observed (71 ledgers, 13,806 bodies): every body is on the own player's entity slot (the slot
//! that gets the own cooldowns 2932); the server never sends it for another entity. A body sends
//! only the changed slots, in child order (`FUN_1417b43c0`). The client facet (`FUN_142598f10`)
//! groups the filled slots by type and tells its listeners of each type whose instances changed.

use crate::payload::{BundleMember, MemberState};
use crate::slots::{SLOT_0, read_children_64, read_slot_byte, write_children_64};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `AbilityInstanceTrackingComponentReplicatedState`.
pub const ABILITY_INSTANCE_TRACKING_TYPE_INDEX: u64 = 2267;
/// The own player component of the state (Observed: `c15` on all 13,806 retail bodies).
pub const ABILITY_INSTANCE_TRACKING_COMPONENT: u64 = 15;
/// `AbilityInstanceType` values 0 to 10 (`FUN_14259b2d0`; 11 is the count).
pub const INSTANCE_TYPES: usize = 11;
/// Slots of one type's row (`FUN_142593330`).
pub const SLOTS_PER_TYPE: usize = 5;
/// The children of slot 0: every slot of every row.
const CHILDREN: u8 = 55;
/// The entity id of an empty slot (`FUN_142592df0`).
pub const NO_INSTANCE_ENTITY: u64 = 0xffff_ffff;
/// Bytes of the entity id half of a raw id.
const ENTITY_LEN: usize = 8;
const FIELD: &str = "ability instance tracking";

/// One slot of `m_abilityInstances`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbilityInstance {
    /// When the instance was made: ns since 2000-01-01; 0 when the slot is empty.
    pub created_ns: u64,
    /// The raw id of its spell entity; all zero when the slot is empty.
    pub raw_id: [u8; RAW_ID_LEN],
    /// Its spell entity's id; [`NO_INSTANCE_ENTITY`] when the slot is empty.
    pub entity: u64,
}

impl AbilityInstance {
    /// An empty slot.
    pub const EMPTY: Self = Self {
        created_ns: 0,
        raw_id: [0; RAW_ID_LEN],
        entity: NO_INSTANCE_ENTITY,
    };

    /// The instance of the spell entity `raw_id`, made at `created_ns`. Observed (6,069 of 6,069
    /// filled retail slots): the entity id is the raw id's first 8 bytes read little-endian.
    pub fn of_entity(created_ns: u64, raw_id: [u8; RAW_ID_LEN]) -> Self {
        let mut half = [0; ENTITY_LEN];
        half.copy_from_slice(&raw_id[..ENTITY_LEN]);
        Self {
            created_ns,
            raw_id,
            entity: u64::from_le_bytes(half),
        }
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            created_ns: reader.u64()?,
            raw_id: reader.raw_id()?,
            entity: reader.u64()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .u64(self.created_ns)
            .bytes(&self.raw_id)
            .u64(self.entity);
    }
}

/// The child of slot `slot` of the row of type `instance_type`.
pub fn instance_child(instance_type: usize, slot: usize) -> Option<u8> {
    (instance_type < INSTANCE_TYPES && slot < SLOTS_PER_TYPE)
        .then(|| u8::try_from(instance_type * SLOTS_PER_TYPE + slot).ok())
        .flatten()
}

/// The changed slots of the state, each as (child, value), in child order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AbilityInstanceTrackingState {
    /// The changed slots; a child below 55, each once.
    pub slots: Vec<(u8, AbilityInstance)>,
}

impl AbilityInstanceTrackingState {
    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or sends a slot or child that the class
    /// does not have.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut slots = Vec::new();
        if read_slot_byte(reader, FIELD, 1)? == 0 {
            return Ok(Self { slots });
        }
        read_children_64(reader, FIELD, CHILDREN, |reader, child| {
            slots.push((child, AbilityInstance::read(reader)?));
            Ok(())
        })?;
        Ok(Self { slots })
    }

    /// Writes the state: no slot when no child changed.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Unsized`] for a child above 54 or a child sent twice.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let mut children: u64 = 0;
        for (child, _) in &self.slots {
            let bit = 1_u64 << child;
            if *child >= CHILDREN || children & bit != 0 {
                return Err(MessageError::Unsized {
                    field: FIELD,
                    byte: *child,
                });
            }
            children |= bit;
        }
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        let mut sorted = self.slots.clone();
        sorted.sort_by_key(|(child, _)| *child);
        let mut values = sorted.iter();
        write_children_64::<MessageError>(writer, children, CHILDREN, |writer, _| {
            if let Some((_, value)) = values.next() {
                value.write(writer);
            }
            Ok(())
        })
    }

    /// The state as the own player's bundle member.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Unsized`] when the state cannot be written.
    pub fn member(&self) -> Result<BundleMember, MessageError> {
        let mut writer = Writer::new();
        self.write(&mut writer)?;
        Ok(BundleMember {
            component: ABILITY_INSTANCE_TRACKING_COMPONENT,
            state: MemberState::Recorded {
                class: ABILITY_INSTANCE_TRACKING_TYPE_INDEX,
                body: writer.finish(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail Sacred Ground made (`20260927-223955-324` 234.425 s): child 15, type 3 slot 0.
    const RETAIL_MADE: &str =
        "018080020bb5fb852d7d120180a2e6b47c385fdc11a55505ca384ec4dc5f387cb4e6a280";
    /// The same slot emptied 12.0 s later (246.422 s).
    const RETAIL_GONE: &str =
        "0180800200000000000000000000000000000000000000000000000000000000ffffffff";
    /// Two filled Ice Shower slots (`WEAPON-CODEX` 2455.333 s): children 32 and 33.
    const RETAIL_TWO: &str = "0180808080300bb272b0ad687a829c3f9cde20458690f0b69b3e0f0bbcc290864520de9c3f9c0bb272b0b11c8025ba72b907e78589ad50249f4b2e9b752ead8985e707b972ba";
    /// Three emptied slots in three child bytes (`soultrialhealervg` 161.124 s): 15, 20, 35.
    const RETAIL_THREE: &str = "018080c200000000000000000000000000000000000000000000000000000000ffffffff00000000000000000000000000000000000000000000000000000000ffffffff80800100000000000000000000000000000000000000000000000000000000ffffffff";
    const SACRED_GROUND: usize = 3;

    fn read(text: &str) -> (AbilityInstanceTrackingState, Vec<u8>) {
        let bytes = hex::decode(text).expect("hex");
        let mut reader = Reader::new(&bytes);
        let state = AbilityInstanceTrackingState::read(&mut reader).expect("read");
        reader.finish().expect("whole");
        (state, bytes)
    }

    fn written(state: &AbilityInstanceTrackingState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        writer.finish()
    }

    #[test]
    fn the_retail_made_slot_reads_and_writes_the_same_bytes() {
        let (state, bytes) = read(RETAIL_MADE);
        let raw: [u8; RAW_ID_LEN] = hex::decode("80a2e6b47c385fdc11a55505ca384ec4")
            .expect("hex")
            .try_into()
            .expect("16 bytes");
        assert_eq!(
            state.slots,
            vec![(
                instance_child(SACRED_GROUND, 0).expect("child"),
                AbilityInstance::of_entity(843_857_054_614_229_505, raw)
            )]
        );
        assert_eq!(state.slots[0].1.entity, 0xdc5f_387c_b4e6_a280);
        assert_eq!(written(&state), bytes);
    }

    #[test]
    fn an_emptied_slot_is_the_empty_value() {
        let (state, bytes) = read(RETAIL_GONE);
        assert_eq!(state.slots, vec![(15, AbilityInstance::EMPTY)]);
        assert_eq!(written(&state), bytes);
    }

    #[test]
    fn several_slots_round_trip_in_child_order() {
        let (two, bytes) = read(RETAIL_TWO);
        assert_eq!(
            two.slots
                .iter()
                .map(|(child, _)| *child)
                .collect::<Vec<_>>(),
            [32, 33]
        );
        assert!(two.slots.iter().all(|(_, slot)| {
            *slot == AbilityInstance::of_entity(slot.created_ns, slot.raw_id)
        }));
        assert_eq!(written(&two), bytes);
        let (three, bytes) = read(RETAIL_THREE);
        assert_eq!(
            three.slots,
            vec![
                (15, AbilityInstance::EMPTY),
                (20, AbilityInstance::EMPTY),
                (35, AbilityInstance::EMPTY)
            ]
        );
        let mut shuffled = three.clone();
        shuffled.slots.reverse();
        assert_eq!(written(&shuffled), bytes, "written in child order");
    }

    #[test]
    fn the_member_is_the_own_component_and_no_change_is_one_zero_byte() {
        let (state, bytes) = read(RETAIL_MADE);
        let member = state.member().expect("member");
        assert_eq!(member.component, ABILITY_INSTANCE_TRACKING_COMPONENT);
        assert_eq!(
            member.state,
            MemberState::Recorded {
                class: ABILITY_INSTANCE_TRACKING_TYPE_INDEX,
                body: bytes
            }
        );
        assert_eq!(written(&AbilityInstanceTrackingState::default()), [0]);
    }

    #[test]
    fn a_child_outside_the_rows_or_sent_twice_is_refused() {
        assert_eq!(instance_child(INSTANCE_TYPES, 0), None);
        assert_eq!(instance_child(0, SLOTS_PER_TYPE), None);
        let outside = AbilityInstanceTrackingState {
            slots: vec![(CHILDREN, AbilityInstance::EMPTY)],
        };
        assert!(outside.write(&mut Writer::new()).is_err());
        let twice = AbilityInstanceTrackingState {
            slots: vec![(1, AbilityInstance::EMPTY), (1, AbilityInstance::EMPTY)],
        };
        assert!(twice.write(&mut Writer::new()).is_err());
    }
}
