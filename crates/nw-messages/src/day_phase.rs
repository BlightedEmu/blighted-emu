// SPDX-License-Identifier: AGPL-3.0-or-later
//! `SyncDayPhase` (bodies note, section 4247): two big-endian `u64`, then a big-endian `u32`.
//! Reader `FUN_1440716c0`.

use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// File slot of `SyncDayPhase`.
pub const SYNC_DAY_PHASE_MSG_TYPE_INDEX: u64 = 4247;
/// The last field on all 43 captured logins. Sessions that start mid-way also show 0.
pub const OBSERVED_DAY_PHASE: u32 = 2;

/// The day phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncDayPhaseMsg {
    /// Offset 0; varies per session.
    pub component: u64,
    /// Offset 8; varies per session.
    pub u64_at_8: u64,
    /// Offset 16.
    pub u32_at_16: u32,
}

impl BodyCodec for SyncDayPhaseMsg {
    const TYPE_INDEX: u64 = SYNC_DAY_PHASE_MSG_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        Ok(Writer::new()
            .u64(self.component)
            .u64(self.u64_at_8)
            .u32(self.u32_at_16)
            .finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            component: reader.u64()?,
            u64_at_8: reader.u64()?,
            u32_at_16: reader.u32()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY_LEN: usize = 20;

    #[test]
    fn the_body_is_two_u64_and_one_u32() {
        let message = SyncDayPhaseMsg {
            component: 1,
            u64_at_8: 2,
            u32_at_16: OBSERVED_DAY_PHASE,
        };
        let body = message.encode().expect("encode");
        assert_eq!(body.len(), BODY_LEN);
        assert_eq!(body[BODY_LEN - 1], 2);
        assert_eq!(SyncDayPhaseMsg::decode(&body), Ok(message));
        assert!(SyncDayPhaseMsg::decode(&body[..BODY_LEN - 1]).is_err());
    }
}
