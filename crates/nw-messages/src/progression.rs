// SPDX-License-Identifier: AGPL-3.0-or-later
//! `ProgressionComponentReplicatedState` (type 899, own player component 70): the player's
//! level and experience (xp-potion-exit note, section 1).
//!
//! Slot 0 holds the level (child 0, zero-based) and the bonus level (child 1, never sent); slot 1
//! the experience points (child 0) and the rested experience (child 1). Each value is a
//! big-endian `u32`. Retail sends both slot 1 children at login and the experience alone after a
//! kill.

use crate::slots::{read_children, read_slot_byte, write_known_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `ProgressionComponentReplicatedState`.
pub const PROGRESSION_TYPE_INDEX: u64 = 899;
/// The own player component of the progression.
pub const PROGRESSION_COMPONENT: u64 = 70;

/// Slots of the class.
const SLOTS: u8 = 2;
const SLOT_0: u8 = 0x01;
const SLOT_1: u8 = 0x02;
/// Children per slot.
const CHILDREN: u8 = 2;
const LEVEL_CHILD: u8 = 0;
const XP_CHILD: u8 = 0;
const RESTED_CHILD: u8 = 1;
const FIELD: &str = "progression";

/// The progression state; a value that is `None` is not sent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProgressionState {
    /// The level, zero-based (0 is level 1).
    pub level: Option<u32>,
    /// The experience points.
    pub experience: Option<u32>,
    /// The rested experience.
    pub rested: Option<u32>,
}

impl ProgressionState {
    /// The login state of a player with `experience` points and no rested experience.
    pub const fn login(experience: u32) -> Self {
        Self {
            level: None,
            experience: Some(experience),
            rested: Some(0),
        }
    }

    /// The login state of a player at `level` (zero-based) with `experience` points and no
    /// rested experience. Retail sends the level only above 0: a new character's login has no
    /// level (`02 03 <xp> <rested>`), the level 67 character's has (`03 01 <level> 03 <xp>
    /// <rested>`, progression note, section 3).
    pub const fn login_at_level(level: u32, experience: u32) -> Self {
        Self {
            level: if level > 0 { Some(level) } else { None },
            ..Self::login(experience)
        }
    }

    /// The change at a level up to `level` (zero-based) with the new total `experience`: retail
    /// `03 01 <level> 01 <xp>`, without the rested experience (progression note, section 3).
    pub const fn level_up(level: u32, experience: u32) -> Self {
        Self {
            level: Some(level),
            ..Self::experience(experience)
        }
    }

    /// The change after the experience became `experience`.
    pub const fn experience(experience: u32) -> Self {
        Self {
            level: None,
            experience: Some(experience),
            rested: None,
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or sends the bonus level.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let slots = read_slot_byte(reader, FIELD, SLOTS)?;
        let mut state = Self::default();
        if slots & SLOT_0 != 0 {
            read_children(reader, FIELD, CHILDREN, |reader, child| {
                if child != LEVEL_CHILD {
                    return Err(MessageError::Unsized {
                        field: FIELD,
                        byte: child,
                    });
                }
                state.level = Some(reader.u32()?);
                Ok(())
            })?;
        }
        if slots & SLOT_1 != 0 {
            read_children(reader, FIELD, CHILDREN, |reader, child| {
                let value = Some(reader.u32()?);
                if child == XP_CHILD {
                    state.experience = value;
                } else {
                    state.rested = value;
                }
                Ok(())
            })?;
        }
        Ok(state)
    }

    /// Writes the state.
    pub fn write(&self, writer: &mut Writer) {
        let slot_1 = u16::from(self.experience.is_some()) << XP_CHILD
            | u16::from(self.rested.is_some()) << RESTED_CHILD;
        let slots =
            if self.level.is_some() { SLOT_0 } else { 0 } | if slot_1 == 0 { 0 } else { SLOT_1 };
        writer.u8(slots);
        if let Some(level) = self.level {
            write_known_children(writer, 1 << LEVEL_CHILD, CHILDREN, |writer, _| {
                writer.u32(level);
            });
        }
        if slot_1 != 0 {
            write_known_children(writer, slot_1, CHILDREN, |writer, child| {
                let value = if child == XP_CHILD {
                    self.experience
                } else {
                    self.rested
                };
                writer.u32(value.unwrap_or_default());
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(state: ProgressionState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer);
        writer.finish()
    }

    #[test]
    fn the_retail_login_and_kill_states_read_and_write_the_same_bytes() {
        // Retail (xp-potion-exit note, section 1.2): login `02 03 00000000 00000000`, the first
        // kill `02 01 00000014`.
        const LOGIN: [u8; 10] = [0x02, 0x03, 0, 0, 0, 0, 0, 0, 0, 0];
        const KILL: [u8; 6] = [0x02, 0x01, 0, 0, 0, 0x14];
        assert_eq!(bytes(ProgressionState::login(0)), LOGIN);
        assert_eq!(bytes(ProgressionState::experience(20)), KILL);
        for body in [&LOGIN[..], &KILL[..]] {
            let mut reader = Reader::new(body);
            let state = ProgressionState::read(&mut reader).expect("read");
            reader.finish().expect("whole");
            assert_eq!(bytes(state), body);
        }
        assert_eq!(bytes(ProgressionState::default()), [0]);
    }

    #[test]
    fn the_retail_level_login_and_level_up_read_and_write_the_same_bytes() {
        // Retail level 67 login (NH-MSQ-2.1, t526359): level 66 zero-based, 2,194,660 points.
        const LEVEL_LOGIN: [u8; 15] = [
            0x03, 0x01, 0x00, 0x00, 0x00, 0x42, 0x03, 0x00, 0x21, 0x7c, 0xe4, 0x00, 0x00, 0x00,
            0x00,
        ];
        const LOGIN_LEVEL: u32 = 66;
        const LOGIN_EXPERIENCE: u32 = 2_194_660;
        // Retail level up of the new character (2026-09-18, t453313): level 2, 670 points.
        const LEVEL_UP: [u8; 11] = [0x03, 0x01, 0, 0, 0, 0x02, 0x01, 0, 0, 0x02, 0x9e];
        const UP_LEVEL: u32 = 2;
        const UP_EXPERIENCE: u32 = 670;
        assert_eq!(
            bytes(ProgressionState::login_at_level(
                LOGIN_LEVEL,
                LOGIN_EXPERIENCE
            )),
            LEVEL_LOGIN
        );
        assert_eq!(
            ProgressionState::login_at_level(0, LOGIN_EXPERIENCE),
            ProgressionState::login(LOGIN_EXPERIENCE)
        );
        assert_eq!(
            bytes(ProgressionState::level_up(UP_LEVEL, UP_EXPERIENCE)),
            LEVEL_UP
        );
        for body in [&LEVEL_LOGIN[..], &LEVEL_UP[..]] {
            let mut reader = Reader::new(body);
            let state = ProgressionState::read(&mut reader).expect("read");
            reader.finish().expect("whole");
            assert_eq!(bytes(state), body);
        }
    }
}
