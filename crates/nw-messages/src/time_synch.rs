// SPDX-License-Identifier: AGPL-3.0-or-later
//! `REPClient::TimeSynchMsg` (bodies note, section 335): one big-endian `u64` time. Reader
//! `FUN_1407cc8f0`. The server sends it about every 3 s (sequence note).

use nw_reflect::TIME_SYNCH_MSG_TYPE_INDEX;

use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// The server clock for the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSynchMsg {
    /// The time. Its unit is Unknown; the low half changes on every copy.
    pub time: u64,
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
    fn the_body_is_one_big_endian_u64() {
        let body = TimeSynchMsg { time: TIME }.encode().expect("encode");
        assert_eq!(body, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(TimeSynchMsg::decode(&body), Ok(TimeSynchMsg { time: TIME }));
        assert!(TimeSynchMsg::decode(&body[..7]).is_err());
    }
}
