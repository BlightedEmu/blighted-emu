// SPDX-License-Identifier: AGPL-3.0-or-later
//! `CoatlicueTimingConfigurationConnectedMsg` and `CalendarConnectedMsg` (bodies note, section
//! "446 and 1034"): the same body, two blocks of `FUN_1415acc30`. Readers `FUN_14646c070` (446)
//! and `FUN_14646bf10` (1034), both through `FUN_1415b4de0`.

use crate::wire::{BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer};

/// File slot of `CoatlicueTimingConfigurationConnectedMsg`.
pub const TIMING_CONFIGURATION_CONNECTED_MSG_TYPE_INDEX: u64 = 446;
/// File slot of `CalendarConnectedMsg`.
pub const CALENDAR_CONNECTED_MSG_TYPE_INDEX: u64 = 1034;

/// The block that `FUN_1415acc30` reads: a big-endian `u32` and two 16-byte raw fields. The same
/// block is in `PlayerManagerSelfIdentificationMsg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block1415acc30 {
    /// The `u32`.
    pub value: u32,
    /// The first 16 bytes.
    pub first: [u8; RAW_ID_LEN],
    /// The second 16 bytes.
    pub second: [u8; RAW_ID_LEN],
}

impl Block1415acc30 {
    /// Reads one block.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Truncated`] when the body ends early.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            value: reader.u32()?,
            first: reader.raw_id()?,
            second: reader.raw_id()?,
        })
    }

    /// Writes one block.
    pub fn write(&self, writer: &mut Writer) {
        writer
            .u32(self.value)
            .bytes(&self.first)
            .bytes(&self.second);
    }
}

/// The body of both "connected" messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectedBody {
    /// The first block: the same on every captured login, different between the two classes.
    pub block_at_0: Block1415acc30,
    /// The second block: `value` and `first` are the same in both classes of one session.
    pub block_at_36: Block1415acc30,
}

impl ConnectedBody {
    fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.block_at_0.write(&mut writer);
        self.block_at_36.write(&mut writer);
        writer.finish()
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            block_at_0: Block1415acc30::read(&mut reader)?,
            block_at_36: Block1415acc30::read(&mut reader)?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// One "connected" message class over [`ConnectedBody`].
macro_rules! connected_message {
    ($(#[$doc:meta])* $name:ident, $type_index:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(pub ConnectedBody);

        impl BodyCodec for $name {
            const TYPE_INDEX: u64 = $type_index;

            fn encode(&self) -> Result<Vec<u8>, MessageError> {
                Ok(self.0.encode())
            }

            fn decode(body: &[u8]) -> Result<Self, MessageError> {
                ConnectedBody::decode(body).map(Self)
            }
        }
    };
}

connected_message!(
    /// `CoatlicueTimingConfigurationConnectedMsg`.
    TimingConfigurationConnectedMsg,
    TIMING_CONFIGURATION_CONNECTED_MSG_TYPE_INDEX
);
connected_message!(
    /// `CalendarConnectedMsg`.
    CalendarConnectedMsg,
    CALENDAR_CONNECTED_MSG_TYPE_INDEX
);

#[cfg(test)]
mod tests {
    use super::*;

    const BODY_LEN: usize = 72;
    const SECOND_BLOCK_AT: usize = 36;

    fn body() -> ConnectedBody {
        let block = |value, byte| Block1415acc30 {
            value,
            first: [byte; RAW_ID_LEN],
            second: [byte + 1; RAW_ID_LEN],
        };
        ConnectedBody {
            block_at_0: block(1, 10),
            block_at_36: block(2, 20),
        }
    }

    #[test]
    fn both_classes_have_two_36_byte_blocks() {
        let bytes = TimingConfigurationConnectedMsg(body())
            .encode()
            .expect("encode");
        assert_eq!(bytes.len(), BODY_LEN);
        assert_eq!(&bytes[SECOND_BLOCK_AT..SECOND_BLOCK_AT + 4], &[0, 0, 0, 2]);
        assert_eq!(
            CalendarConnectedMsg(body()).encode().expect("encode"),
            bytes
        );
        assert_eq!(
            TimingConfigurationConnectedMsg::decode(&bytes),
            Ok(TimingConfigurationConnectedMsg(body()))
        );
        assert_eq!(
            CalendarConnectedMsg::decode(&bytes),
            Ok(CalendarConnectedMsg(body()))
        );
        assert!(CalendarConnectedMsg::decode(&bytes[..BODY_LEN - 1]).is_err());
    }
}
