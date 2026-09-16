// SPDX-License-Identifier: AGPL-3.0-or-later
//! `REPClient::TimeSynchMsg` (bodies note, section 335): one big-endian `u64` time. Reader
//! `FUN_1407cc8f0`. The server sends it about every 3 s (sequence note).

use nw_reflect::TIME_SYNCH_MSG_TYPE_INDEX;

use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// Nanoseconds per millisecond.
pub const NS_PER_MS: u64 = 1_000_000;

/// The server clock for the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSynchMsg {
    /// Server nanoseconds since 2000-01-01 (mob-animation note, section 1: retail 845730277410915425
    /// 0.9 s before a mob time anchor of 845730278284365 µs). The client's render clock for the
    /// entity state timelines follows it.
    pub time: u64,
}

impl TimeSynchMsg {
    /// The clock at server time `server_ms` (milliseconds since 2000-01-01).
    pub const fn at_server_ms(server_ms: u64) -> Self {
        Self {
            time: server_ms.saturating_mul(NS_PER_MS),
        }
    }
}

impl BodyCodec for TimeSynchMsg {
    const TYPE_INDEX: u64 = TIME_SYNCH_MSG_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        Ok(Writer::new().u64(self.time).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let time = reader.u64()?;
        reader.finish()?;
        Ok(Self { time })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIME: u64 = 0x0102_0304_0506_0708;

    #[test]
    fn the_clock_is_in_nanoseconds() {
        const MS: u64 = 845_730_277_410;
        assert_eq!(TimeSynchMsg::at_server_ms(MS).time, 845_730_277_410_000_000);
    }

    #[test]
    fn the_body_is_one_big_endian_u64() {
        let body = TimeSynchMsg { time: TIME }.encode().expect("encode");
        assert_eq!(body, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(TimeSynchMsg::decode(&body), Ok(TimeSynchMsg { time: TIME }));
        assert!(TimeSynchMsg::decode(&body[..7]).is_err());
    }
}
