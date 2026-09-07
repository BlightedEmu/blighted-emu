// SPDX-License-Identifier: AGPL-3.0-or-later
//! Three small replicated states with slot 0 only (component tables note, sections 1 to 3):
//! `MB::StaminaComponentReplicatedState` (type 4297), `MB::CombatStatusComponentReplicatedState`
//! (type 2913) and `MB::DelayedEventComponentReplicatedState` (type 3139). Each table ends every
//! reached captured body of its class.

use crate::slots::{children_mask, read_slot_0, write_slot_0};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::StaminaComponentReplicatedState`.
pub const STAMINA_TYPE_INDEX: u64 = 4297;
/// Type index of `MB::CombatStatusComponentReplicatedState`.
pub const COMBAT_STATUS_TYPE_INDEX: u64 = 2913;
/// Type index of `MB::DelayedEventComponentReplicatedState`.
pub const DELAYED_EVENT_TYPE_INDEX: u64 = 3139;
const STAMINA_CHILDREN: u8 = 6;
const COMBAT_STATUS_CHILDREN: u8 = 4;
const DELAYED_EVENT_CHILDREN: u8 = 2;

/// `MB::StaminaComponentReplicatedState`: six big-endian `f32` children (`FUN_14671a230`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StaminaState {
    /// `cur`.
    pub cur: Option<f32>,
    /// `max`.
    pub max: Option<f32>,
    /// `winded`.
    pub winded: Option<f32>,
    /// `regen`.
    pub regen: Option<f32>,
    /// `multMax`.
    pub mult_max: Option<f32>,
    /// `multRegen`.
    pub mult_regen: Option<f32>,
}

impl StaminaState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or has a slot or child the table has
    /// not.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let children = read_slot_0(reader, "stamina", STAMINA_CHILDREN)?.unwrap_or_default();
        let mut value = |child: u16| {
            (children & 1 << child != 0)
                .then(|| reader.f32())
                .transpose()
        };
        Ok(Self {
            cur: value(0)?,
            max: value(1)?,
            winded: value(2)?,
            regen: value(3)?,
            mult_max: value(4)?,
            mult_regen: value(5)?,
        })
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let values = [
            self.cur,
            self.max,
            self.winded,
            self.regen,
            self.mult_max,
            self.mult_regen,
        ];
        let present = values.map(|value| value.is_some());
        write_slot_0(writer, Some(children_mask(&present)), STAMINA_CHILDREN);
        for value in values.into_iter().flatten() {
            writer.f32(value);
        }
    }
}

/// `MB::CombatStatusComponentReplicatedState` (`FUN_1466f1f10`): two flags, two big-endian
/// `u64` time points.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CombatStatusState {
    /// `replicatedInCombatFlag`.
    pub in_combat: Option<bool>,
    /// `replicatedInPvpCombatFlag`.
    pub in_pvp_combat: Option<bool>,
    /// `replicatedCombatLoggedOutTimePoint`.
    pub logged_out_time: Option<u64>,
    /// `replicatedCombatConcludedTimePoint`.
    pub concluded_time: Option<u64>,
}

impl CombatStatusState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, a flag is not 0 or 1, or a slot or
    /// child is not in the table.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let children =
            read_slot_0(reader, "combat status", COMBAT_STATUS_CHILDREN)?.unwrap_or_default();
        let has = |child: u16| children & 1 << child != 0;
        Ok(Self {
            in_combat: has(0).then(|| reader.boolean()).transpose()?,
            in_pvp_combat: has(1).then(|| reader.boolean()).transpose()?,
            logged_out_time: has(2).then(|| reader.u64()).transpose()?,
            concluded_time: has(3).then(|| reader.u64()).transpose()?,
        })
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let present = [
            self.in_combat.is_some(),
            self.in_pvp_combat.is_some(),
            self.logged_out_time.is_some(),
            self.concluded_time.is_some(),
        ];
        write_slot_0(
            writer,
            Some(children_mask(&present)),
            COMBAT_STATUS_CHILDREN,
        );
        for flag in [self.in_combat, self.in_pvp_combat].into_iter().flatten() {
            writer.boolean(flag);
        }
        for time in [self.logged_out_time, self.concluded_time]
            .into_iter()
            .flatten()
        {
            writer.u64(time);
        }
    }
}

/// `MB::DelayedEventComponentReplicatedState` (`FUN_143066370`): a big-endian `u16` and a flag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DelayedEventState {
    /// `totalDelayDuration`.
    pub total_delay: Option<u16>,
    /// `delayedEventCompleted`.
    pub completed: Option<bool>,
}

impl DelayedEventState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, the flag is not 0 or 1, or a slot or
    /// child is not in the table.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let children =
            read_slot_0(reader, "delayed event", DELAYED_EVENT_CHILDREN)?.unwrap_or_default();
        let has = |child: u16| children & 1 << child != 0;
        Ok(Self {
            total_delay: has(0).then(|| reader.u16()).transpose()?,
            completed: has(1).then(|| reader.boolean()).transpose()?,
        })
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let present = [self.total_delay.is_some(), self.completed.is_some()];
        write_slot_0(
            writer,
            Some(children_mask(&present)),
            DELAYED_EVENT_CHILDREN,
        );
        if let Some(total_delay) = self.total_delay {
            writer.u16(total_delay);
        }
        if let Some(completed) = self.completed {
            writer.boolean(completed);
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const STAMINA: f32 = 120.5;
    const DELAY_MS: u16 = 1500;
    const TIME_POINT: u64 = 0x0102_0304_0506_0708;

    fn encoded(write: impl FnOnce(&mut Writer)) -> Vec<u8> {
        let mut writer = Writer::new();
        write(&mut writer);
        writer.finish()
    }

    fn finite() -> impl Strategy<Value = Option<f32>> {
        proptest::option::of(-1.0e9_f32..1.0e9_f32)
    }

    proptest! {
        #[test]
        fn stamina_round_trips(
            (cur, max, winded) in (finite(), finite(), finite()),
            (regen, mult_max, mult_regen) in (finite(), finite(), finite()),
        ) {
            let state = StaminaState { cur, max, winded, regen, mult_max, mult_regen };
            let bytes = encoded(|writer| state.write(writer));
            let mut reader = Reader::new(&bytes);
            prop_assert_eq!(StaminaState::read(&mut reader).expect("read"), state);
            prop_assert!(reader.is_empty());
        }

        #[test]
        fn combat_status_round_trips(
            in_combat in proptest::option::of(any::<bool>()),
            in_pvp_combat in proptest::option::of(any::<bool>()),
            logged_out_time in proptest::option::of(any::<u64>()),
            concluded_time in proptest::option::of(any::<u64>()),
        ) {
            let state = CombatStatusState { in_combat, in_pvp_combat, logged_out_time, concluded_time };
            let bytes = encoded(|writer| state.write(writer));
            let mut reader = Reader::new(&bytes);
            prop_assert_eq!(CombatStatusState::read(&mut reader).expect("read"), state);
            prop_assert!(reader.is_empty());
        }

        #[test]
        fn delayed_event_round_trips(
            total_delay in proptest::option::of(any::<u16>()),
            completed in proptest::option::of(any::<bool>()),
        ) {
            let state = DelayedEventState { total_delay, completed };
            let bytes = encoded(|writer| state.write(writer));
            let mut reader = Reader::new(&bytes);
            prop_assert_eq!(DelayedEventState::read(&mut reader).expect("read"), state);
            prop_assert!(reader.is_empty());
        }
    }

    #[test]
    fn stamina_children_are_floats_in_table_order() {
        let mut bytes = vec![0x01, 0b10_0010];
        bytes.extend(STAMINA.to_be_bytes());
        bytes.extend(STAMINA.to_be_bytes());
        let state = StaminaState::read(&mut Reader::new(&bytes)).expect("read");
        assert_eq!(
            (state.max, state.mult_regen),
            (Some(STAMINA), Some(STAMINA))
        );
        assert_eq!(state.cur, None);
    }

    #[test]
    fn combat_status_flags_then_time_points() {
        let mut bytes = vec![0x01, 0b1001, 1];
        bytes.extend(TIME_POINT.to_be_bytes());
        let state = CombatStatusState::read(&mut Reader::new(&bytes)).expect("read");
        assert_eq!(
            (state.in_combat, state.concluded_time),
            (Some(true), Some(TIME_POINT))
        );
    }

    #[test]
    fn a_combat_flag_above_one_is_refused() {
        assert!(CombatStatusState::read(&mut Reader::new(&[0x01, 0b01, 2])).is_err());
    }

    #[test]
    fn delayed_event_is_a_u16_then_a_flag() {
        let mut bytes = vec![0x01, 0b11];
        bytes.extend(DELAY_MS.to_be_bytes());
        bytes.push(0);
        let state = DelayedEventState::read(&mut Reader::new(&bytes)).expect("read");
        assert_eq!(
            (state.total_delay, state.completed),
            (Some(DELAY_MS), Some(false))
        );
    }

    #[test]
    fn a_child_beyond_the_table_is_refused() {
        assert!(DelayedEventState::read(&mut Reader::new(&[0x01, 0b100])).is_err());
    }
}
