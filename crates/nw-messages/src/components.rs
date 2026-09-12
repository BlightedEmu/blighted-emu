// SPDX-License-Identifier: AGPL-3.0-or-later
//! Four small replicated states with slot 0 only (component tables note, sections 1 to 3, and
//! player-appearance note, section 3): `MB::StaminaComponentReplicatedState` (type 4297),
//! `MB::CombatStatusComponentReplicatedState` (type 2913),
//! `MB::DelayedEventComponentReplicatedState` (type 3139) and `MB::ManaComponentReplicatedState`
//! (type 1652). Each table ends every reached captured body of its class.

use crate::slots::{children_mask, read_slot_0, write_known};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::StaminaComponentReplicatedState`.
pub const STAMINA_TYPE_INDEX: u64 = 4297;
/// Type index of `MB::CombatStatusComponentReplicatedState`.
pub const COMBAT_STATUS_TYPE_INDEX: u64 = 2913;
/// Type index of `MB::DelayedEventComponentReplicatedState`.
pub const DELAYED_EVENT_TYPE_INDEX: u64 = 3139;
/// Type index of `MB::ManaComponentReplicatedState`.
pub const MANA_TYPE_INDEX: u64 = 1652;
/// Full stamina and mana of a player: 100 on the captured first bodies (vitals row `Player`).
const PLAYER_POOL: f32 = 100.0;
/// A stamina or mana value of 0 on the captured first bodies (`winded`, `regen`, `regenDelay`).
const NONE_YET: f32 = 0.0;
/// A multiplier or rate of 1 on the captured first bodies (`multMax`, `multRegen`, `regenRate`).
const UNIT_RATE: f32 = 1.0;
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
    /// The first stamina of a player, as the usual 26-byte captured first body: 100 of 100, not
    /// winded, rates 1.
    pub const fn player() -> Self {
        Self {
            cur: Some(PLAYER_POOL),
            max: Some(PLAYER_POOL),
            winded: Some(NONE_YET),
            regen: Some(NONE_YET),
            mult_max: Some(UNIT_RATE),
            mult_regen: Some(UNIT_RATE),
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or has a slot or child the table has
    /// not.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let [cur, max, winded, regen, mult_max, mult_regen] = read_floats(reader, "stamina")?;
        Ok(Self {
            cur,
            max,
            winded,
            regen,
            mult_max,
            mult_regen,
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
        write_floats(writer, &values);
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
        let mut state = Self::default();
        read_slot_0(
            reader,
            "combat status",
            COMBAT_STATUS_CHILDREN,
            |reader, child| {
                match child {
                    0 => state.in_combat = Some(reader.boolean()?),
                    1 => state.in_pvp_combat = Some(reader.boolean()?),
                    2 => state.logged_out_time = Some(reader.u64()?),
                    _ => state.concluded_time = Some(reader.u64()?),
                }
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let present = [
            self.in_combat.is_some(),
            self.in_pvp_combat.is_some(),
            self.logged_out_time.is_some(),
            self.concluded_time.is_some(),
        ];
        write_known(
            writer,
            children_mask(&present),
            COMBAT_STATUS_CHILDREN,
            |writer, child| {
                match child {
                    0 => self.in_combat.map(|flag| writer.boolean(flag)),
                    1 => self.in_pvp_combat.map(|flag| writer.boolean(flag)),
                    2 => self.logged_out_time.map(|time| writer.u64(time)),
                    _ => self.concluded_time.map(|time| writer.u64(time)),
                };
            },
        );
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
        let mut state = Self::default();
        read_slot_0(
            reader,
            "delayed event",
            DELAYED_EVENT_CHILDREN,
            |reader, child| {
                if child == 0 {
                    state.total_delay = Some(reader.u16()?);
                } else {
                    state.completed = Some(reader.boolean()?);
                }
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let present = [self.total_delay.is_some(), self.completed.is_some()];
        write_known(
            writer,
            children_mask(&present),
            DELAYED_EVENT_CHILDREN,
            |writer, child| {
                match child {
                    0 => self.total_delay.map(|delay| writer.u16(delay)),
                    _ => self.completed.map(|completed| writer.boolean(completed)),
                };
            },
        );
    }
}

/// `MB::ManaComponentReplicatedState` (`FUN_1467088c0`): four big-endian `f32` children. The
/// names of the first two are Unknown (short strings the decompiler does not show); both are 100
/// on the captured first bodies.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ManaState {
    /// Child 0 (name Unknown).
    pub first: Option<f32>,
    /// Child 1 (name Unknown).
    pub second: Option<f32>,
    /// `regenDelay`.
    pub regen_delay: Option<f32>,
    /// `regenRate`.
    pub regen_rate: Option<f32>,
}

impl ManaState {
    /// The first mana state of a player, as the usual 18-byte captured first body: 100, 100,
    /// delay 0, rate 1.
    pub const fn player() -> Self {
        Self {
            first: Some(PLAYER_POOL),
            second: Some(PLAYER_POOL),
            regen_delay: Some(NONE_YET),
            regen_rate: Some(UNIT_RATE),
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or has a slot or child the table has
    /// not.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let [first, second, regen_delay, regen_rate] = read_floats(reader, "mana")?;
        Ok(Self {
            first,
            second,
            regen_delay,
            regen_rate,
        })
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        write_floats(
            writer,
            &[self.first, self.second, self.regen_delay, self.regen_rate],
        );
    }
}

/// Reads slot 0 of a class whose `N` children are all big-endian `f32`.
fn read_floats<const N: usize>(
    reader: &mut Reader<'_>,
    field: &'static str,
) -> Result<[Option<f32>; N], MessageError> {
    let mut values = [None; N];
    // The tables have at most 7 children.
    read_slot_0(reader, field, N as u8, |reader, child| {
        values[usize::from(child)] = Some(reader.f32()?);
        Ok(())
    })?;
    Ok(values)
}

/// Writes slot 0 of a class whose children are all big-endian `f32`.
fn write_floats(writer: &mut Writer, values: &[Option<f32>]) {
    let present: Vec<bool> = values.iter().map(Option::is_some).collect();
    // The tables have at most 7 children.
    write_known(
        writer,
        children_mask(&present),
        values.len() as u8,
        |writer, child| {
            if let Some(value) = values[usize::from(child)] {
                writer.f32(value);
            }
        },
    );
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
    fn a_player_start_stamina_is_all_six_floats() {
        let bytes = encoded(|writer| StaminaState::player().write(writer));
        let mut expected = vec![0x01, 0b11_1111];
        for value in [100.0_f32, 100.0, 0.0, 0.0, 1.0, 1.0] {
            expected.extend(value.to_be_bytes());
        }
        assert_eq!(bytes, expected, "the 26-byte retail first body");
    }

    #[test]
    fn a_player_start_mana_is_four_floats() {
        let bytes = encoded(|writer| ManaState::player().write(writer));
        let mut expected = vec![0x01, 0b1111];
        for value in [100.0_f32, 100.0, 0.0, 1.0] {
            expected.extend(value.to_be_bytes());
        }
        assert_eq!(bytes, expected, "the 18-byte retail first body");
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            ManaState::read(&mut reader).expect("read"),
            ManaState::player()
        );
        assert!(reader.is_empty());
    }

    #[test]
    fn a_child_beyond_the_table_is_refused() {
        assert!(DelayedEventState::read(&mut Reader::new(&[0x01, 0b100])).is_err());
    }
}
