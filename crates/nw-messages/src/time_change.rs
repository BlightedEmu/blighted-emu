// SPDX-License-Identifier: AGPL-3.0-or-later
//! `SyncTimeChange` (bodies note, section 4246). Reader `FUN_1440713a0`, body `FUN_144071dc0`:
//! two `u64`, three `f32` and six `u32` (`FUN_1469ad860`), one `u64`, four `f32`. All big-endian.

use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// File slot of `SyncTimeChange`.
pub const SYNC_TIME_CHANGE_MSG_TYPE_INDEX: u64 = 4246;
/// Floats at offset 16 on all 43 captured logins: 6, -1, and `0x39f5f5b8` (about 0.00046913).
pub const OBSERVED_TIME_CHANGE_F32S_AT_16: [f32; 3] = [6.0, -1.0, f32::from_bits(0x39f5_f5b8)];
/// `u32` values at offset 28 on all 43 captured logins.
pub const OBSERVED_TIME_CHANGE_U32S_AT_28: [u32; 6] = [21_300, 0, 65_100, 0, 3600, 1800];
/// Floats at offset 60 on all 43 captured logins: 1/6, 1/6, 5/6, 5/6.
pub const OBSERVED_TIME_CHANGE_F32S_AT_60: [f32; 4] = [
    f32::from_bits(0x3e2a_aaab),
    f32::from_bits(0x3e2a_aaab),
    f32::from_bits(0x3f55_5555),
    f32::from_bits(0x3f55_5555),
];

/// The game clock settings.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SyncTimeChangeMsg {
    /// Offsets 0 and 8: two ids, fixed inside one session.
    pub ids: [u64; 2],
    /// Offset 16.
    pub f32s_at_16: [f32; 3],
    /// Offset 28.
    pub u32s_at_28: [u32; 6],
    /// Offset 52: a time.
    pub time: u64,
    /// Offset 60.
    pub f32s_at_60: [f32; 4],
}

impl BodyCodec for SyncTimeChangeMsg {
    const TYPE_INDEX: u64 = SYNC_TIME_CHANGE_MSG_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        for id in self.ids {
            writer.u64(id);
        }
        for value in self.f32s_at_16 {
            writer.f32(value);
        }
        for value in self.u32s_at_28 {
            writer.u32(value);
        }
        writer.u64(self.time);
        for value in self.f32s_at_60 {
            writer.f32(value);
        }
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let mut message = Self::default();
        for id in &mut message.ids {
            *id = reader.u64()?;
        }
        for value in &mut message.f32s_at_16 {
            *value = reader.f32()?;
        }
        for value in &mut message.u32s_at_28 {
            *value = reader.u32()?;
        }
        message.time = reader.u64()?;
        for value in &mut message.f32s_at_60 {
            *value = reader.f32()?;
        }
        reader.finish()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY_LEN: usize = 76;
    const RATES_AT: usize = 16;
    const FRACTIONS_AT: usize = 60;

    fn observed() -> SyncTimeChangeMsg {
        SyncTimeChangeMsg {
            ids: [1, 2],
            f32s_at_16: OBSERVED_TIME_CHANGE_F32S_AT_16,
            u32s_at_28: OBSERVED_TIME_CHANGE_U32S_AT_28,
            time: 3,
            f32s_at_60: OBSERVED_TIME_CHANGE_F32S_AT_60,
        }
    }

    #[test]
    fn the_body_has_the_reader_widths() {
        let body = observed().encode().expect("encode");
        assert_eq!(body.len(), BODY_LEN);
        assert_eq!(&body[RATES_AT..RATES_AT + 4], &[0x40, 0xc0, 0, 0]);
        assert_eq!(
            &body[FRACTIONS_AT..FRACTIONS_AT + 4],
            &[0x3e, 0x2a, 0xaa, 0xab]
        );
        assert_eq!(SyncTimeChangeMsg::decode(&body), Ok(observed()));
        assert!(SyncTimeChangeMsg::decode(&body[..BODY_LEN - 1]).is_err());
    }
}
