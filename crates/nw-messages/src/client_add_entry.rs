// SPDX-License-Identifier: AGPL-3.0-or-later
//! `ClientActorRoutingAuthorizationTrait::ClientAddEntryMsg` (bodies note, section 164): one
//! 16-byte id. Reader `FUN_1407c8a70`.

use crate::wire::{BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer};

/// File slot of `ClientAddEntryMsg`.
pub const CLIENT_ADD_ENTRY_MSG_TYPE_INDEX: u64 = 164;

/// Adds a routing entry for one id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientAddEntryMsg {
    /// The entry id. In the captures it is not the id of `PlayerManagerSelfIdentificationMsg`.
    pub entry_id: [u8; RAW_ID_LEN],
}

impl BodyCodec for ClientAddEntryMsg {
    const TYPE_INDEX: u64 = CLIENT_ADD_ENTRY_MSG_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        Ok(Writer::new().bytes(&self.entry_id).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let entry_id = reader.raw_id()?;
        reader.finish()?;
        Ok(Self { entry_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_is_one_16_byte_id() {
        let message = ClientAddEntryMsg {
            entry_id: [9; RAW_ID_LEN],
        };
        let body = message.encode().expect("encode");
        assert_eq!(body.len(), RAW_ID_LEN);
        assert_eq!(ClientAddEntryMsg::decode(&body), Ok(message));
        assert!(ClientAddEntryMsg::decode(&body[1..]).is_err());
    }
}
