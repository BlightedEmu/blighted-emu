// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::VitalsComponentReplicatedState` (type 15), slot 0, on the bundle path (vitals note,
//! Round 3). The vitals constructor `FUN_14671e040` appends 14 children to slot 0, in this
//! order: `HealthAmount`, `StaminaAmount` (big-endian `f32`), `vitalsData` (`FUN_1469515b0`: a
//! byte, two big-endian `u64`, a byte), `healthChangeFlags` (a byte), `HealthMax` (`f32`),
//! `HealthTickRate` (`u16`), `StaminaMax` (`f32`), `StaminaTickRate` (`u16`), `vitalsId`,
//! `vitalsCategoryId`, `vitalsLevel` (`u32`), `invulnerability`, `displayImmuneWhenInvulnerable`
//! (a byte each) and `maxHealth` (`u16`). That table ends 99.997 % of 2,396,697 captured bodies.
//! A body that uses another slot is refused: slot 1 (mana and the afflictions) is not sized.

use crate::slots::{read_slot_0, write_slot_0};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of the class.
pub const VITALS_TYPE_INDEX: u64 = 15;
const CHILDREN: u8 = 14;
const FIELD: &str = "vitals";
const CHILD_HEALTH: u16 = 1 << 0;
const CHILD_STAMINA: u16 = 1 << 1;
const CHILD_DATA: u16 = 1 << 2;
const CHILD_FLAGS: u16 = 1 << 3;
const CHILD_HEALTH_MAX: u16 = 1 << 4;
const CHILD_HEALTH_TICK: u16 = 1 << 5;
const CHILD_STAMINA_MAX: u16 = 1 << 6;
const CHILD_STAMINA_TICK: u16 = 1 << 7;
const CHILD_ID: u16 = 1 << 8;
const CHILD_CATEGORY: u16 = 1 << 9;
const CHILD_LEVEL: u16 = 1 << 10;
const CHILD_INVULNERABILITY: u16 = 1 << 11;
const CHILD_DISPLAY_IMMUNE: u16 = 1 << 12;
const CHILD_MAX_HEALTH: u16 = 1 << 13;

/// The `vitalsData` child (reader `FUN_1469515b0`): a byte, two big-endian `u64`, a byte. The
/// meaning is Unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VitalsData {
    /// The first byte.
    pub head: u8,
    /// The first `u64`.
    pub a: u64,
    /// The second `u64`.
    pub b: u64,
    /// The last byte.
    pub tail: u8,
}

/// Slot 0 of the vitals state; every child is optional.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VitalsSlot0 {
    /// `HealthAmount`: the current health.
    pub health_amount: Option<f32>,
    /// `StaminaAmount`.
    pub stamina_amount: Option<f32>,
    /// `vitalsData`.
    pub vitals_data: Option<VitalsData>,
    /// `healthChangeFlags`.
    pub health_change_flags: Option<u8>,
    /// `HealthMax`.
    pub health_max: Option<f32>,
    /// `HealthTickRate`.
    pub health_tick_rate: Option<u16>,
    /// `StaminaMax`.
    pub stamina_max: Option<f32>,
    /// `StaminaTickRate`.
    pub stamina_tick_rate: Option<u16>,
    /// `vitalsId`.
    pub vitals_id: Option<u32>,
    /// `vitalsCategoryId`.
    pub vitals_category_id: Option<u32>,
    /// `vitalsLevel`.
    pub vitals_level: Option<u32>,
    /// `invulnerability`.
    pub invulnerability: Option<u8>,
    /// `displayImmuneWhenInvulnerable`.
    pub display_immune: Option<u8>,
    /// `maxHealth`.
    pub max_health: Option<u16>,
}

impl VitalsSlot0 {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, uses a slot other than 0, or has a form
    /// `write` cannot make.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let Some(children) = read_slot_0(reader, FIELD, CHILDREN)? else {
            return Ok(Self::default());
        };
        let has = |child| children & child != 0;
        Ok(Self {
            health_amount: has(CHILD_HEALTH).then(|| reader.f32()).transpose()?,
            stamina_amount: has(CHILD_STAMINA).then(|| reader.f32()).transpose()?,
            vitals_data: has(CHILD_DATA).then(|| read_data(reader)).transpose()?,
            health_change_flags: has(CHILD_FLAGS).then(|| reader.u8()).transpose()?,
            health_max: has(CHILD_HEALTH_MAX).then(|| reader.f32()).transpose()?,
            health_tick_rate: has(CHILD_HEALTH_TICK).then(|| reader.u16()).transpose()?,
            stamina_max: has(CHILD_STAMINA_MAX).then(|| reader.f32()).transpose()?,
            stamina_tick_rate: has(CHILD_STAMINA_TICK).then(|| reader.u16()).transpose()?,
            vitals_id: has(CHILD_ID).then(|| reader.u32()).transpose()?,
            vitals_category_id: has(CHILD_CATEGORY).then(|| reader.u32()).transpose()?,
            vitals_level: has(CHILD_LEVEL).then(|| reader.u32()).transpose()?,
            invulnerability: has(CHILD_INVULNERABILITY)
                .then(|| reader.u8())
                .transpose()?,
            display_immune: has(CHILD_DISPLAY_IMMUNE).then(|| reader.u8()).transpose()?,
            max_health: has(CHILD_MAX_HEALTH).then(|| reader.u16()).transpose()?,
        })
    }

    /// Writes the state for the bundle path, children in their order.
    pub fn write(&self, writer: &mut Writer) {
        let flags = [
            (self.health_amount.is_some(), CHILD_HEALTH),
            (self.stamina_amount.is_some(), CHILD_STAMINA),
            (self.vitals_data.is_some(), CHILD_DATA),
            (self.health_change_flags.is_some(), CHILD_FLAGS),
            (self.health_max.is_some(), CHILD_HEALTH_MAX),
            (self.health_tick_rate.is_some(), CHILD_HEALTH_TICK),
            (self.stamina_max.is_some(), CHILD_STAMINA_MAX),
            (self.stamina_tick_rate.is_some(), CHILD_STAMINA_TICK),
            (self.vitals_id.is_some(), CHILD_ID),
            (self.vitals_category_id.is_some(), CHILD_CATEGORY),
            (self.vitals_level.is_some(), CHILD_LEVEL),
            (self.invulnerability.is_some(), CHILD_INVULNERABILITY),
            (self.display_immune.is_some(), CHILD_DISPLAY_IMMUNE),
            (self.max_health.is_some(), CHILD_MAX_HEALTH),
        ];
        let children = flags
            .iter()
            .filter(|(present, _)| *present)
            .fold(0, |bits, (_, child)| bits | child);
        write_slot_0(writer, Some(children), CHILDREN);
        for value in [self.health_amount, self.stamina_amount]
            .into_iter()
            .flatten()
        {
            writer.f32(value);
        }
        if let Some(data) = &self.vitals_data {
            writer.u8(data.head).u64(data.a).u64(data.b).u8(data.tail);
        }
        if let Some(flags) = self.health_change_flags {
            writer.u8(flags);
        }
        for (max, tick_rate) in [
            (self.health_max, self.health_tick_rate),
            (self.stamina_max, self.stamina_tick_rate),
        ] {
            if let Some(max) = max {
                writer.f32(max);
            }
            if let Some(tick_rate) = tick_rate {
                writer.u16(tick_rate);
            }
        }
        for value in [self.vitals_id, self.vitals_category_id, self.vitals_level]
            .into_iter()
            .flatten()
        {
            writer.u32(value);
        }
        for byte in [self.invulnerability, self.display_immune]
            .into_iter()
            .flatten()
        {
            writer.u8(byte);
        }
        if let Some(max_health) = self.max_health {
            writer.u16(max_health);
        }
    }
}

fn read_data(reader: &mut Reader<'_>) -> Result<VitalsData, MessageError> {
    Ok(VitalsData {
        head: reader.u8()?,
        a: reader.u64()?,
        b: reader.u64()?,
        tail: reader.u8()?,
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const HEALTH: f32 = 700.5;
    const STAMINA: f32 = 100.0;
    const TICK_RATE: u16 = 5;

    fn round(state: &VitalsSlot0) -> VitalsSlot0 {
        let mut writer = Writer::new();
        state.write(&mut writer);
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes);
        let read = VitalsSlot0::read(&mut reader).expect("read");
        reader.finish().expect("to the end");
        read
    }

    fn read_all(bytes: &[u8]) -> VitalsSlot0 {
        let mut reader = Reader::new(bytes);
        let state = VitalsSlot0::read(&mut reader).expect("read");
        reader.finish().expect("to the end");
        state
    }

    /// A finite float: a NaN is not equal to itself.
    fn float() -> impl Strategy<Value = Option<f32>> {
        proptest::option::of(-1.0e9_f32..1.0e9_f32)
    }

    fn data() -> impl Strategy<Value = Option<VitalsData>> {
        proptest::option::of(
            (any::<u8>(), any::<u64>(), any::<u64>(), any::<u8>())
                .prop_map(|(head, a, b, tail)| VitalsData { head, a, b, tail }),
        )
    }

    proptest! {
        #[test]
        fn vitals_slot_0_round_trips(
            (health_amount, stamina_amount, health_max, stamina_max) in (float(), float(), float(), float()),
            vitals_data in data(),
            health_change_flags in proptest::option::of(any::<u8>()),
            (health_tick_rate, stamina_tick_rate) in (proptest::option::of(any::<u16>()), proptest::option::of(any::<u16>())),
            (vitals_id, vitals_category_id, vitals_level) in (proptest::option::of(any::<u32>()), proptest::option::of(any::<u32>()), proptest::option::of(any::<u32>())),
            (invulnerability, display_immune) in (proptest::option::of(any::<u8>()), proptest::option::of(any::<u8>())),
            max_health in proptest::option::of(any::<u16>()),
        ) {
            let state = VitalsSlot0 {
                health_amount, stamina_amount, vitals_data, health_change_flags, health_max,
                health_tick_rate, stamina_max, stamina_tick_rate, vitals_id, vitals_category_id,
                vitals_level, invulnerability, display_immune, max_health,
            };
            prop_assert_eq!(round(&state), state);
        }
    }

    #[test]
    fn a_vitals_body_on_another_slot_is_refused() {
        let mut reader = Reader::new(&[0b10, 0x01, 0]);
        assert!(VitalsSlot0::read(&mut reader).is_err());
    }

    #[test]
    fn health_and_stamina_are_the_first_two_floats() {
        let mut bytes = vec![0x01, 0b11];
        bytes.extend(HEALTH.to_be_bytes());
        bytes.extend(STAMINA.to_be_bytes());
        let state = read_all(&bytes);
        assert_eq!(
            (state.health_amount, state.stamina_amount),
            (Some(HEALTH), Some(STAMINA))
        );
    }

    #[test]
    fn the_vitals_data_block_is_child_2_of_18_bytes() {
        let mut bytes = vec![0x01, 1 << 2, 9];
        bytes.extend(1_u64.to_be_bytes());
        bytes.extend(2_u64.to_be_bytes());
        bytes.push(3);
        let data = read_all(&bytes).vitals_data.expect("vitals data");
        assert_eq!((data.head, data.a, data.b, data.tail), (9, 1, 2, 3));
    }

    #[test]
    fn a_tick_rate_is_2_bytes() {
        let mut bytes = vec![0x01, 1 << 5];
        bytes.extend(TICK_RATE.to_be_bytes());
        assert_eq!(read_all(&bytes).health_tick_rate, Some(TICK_RATE));
    }

    #[test]
    fn only_max_health_is_child_13() {
        let state = VitalsSlot0 {
            max_health: Some(500),
            ..VitalsSlot0::default()
        };
        let mut writer = Writer::new();
        state.write(&mut writer);
        assert_eq!(writer.finish(), vec![0x01, 0x80, 0x40, 0x01, 0xf4]);
    }
}
