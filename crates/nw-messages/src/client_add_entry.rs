// SPDX-License-Identifier: AGPL-3.0-or-later
//! `ClientActorRoutingAuthorizationTrait::ClientAddEntryMsg` (bodies note, section 164): one
//! 16-byte id. Reader `FUN_1407c8a70`.
//!
//! What the retail id means is Unknown. This server puts the player's entity number in it
//! ([`raw_id_of_entity`]), so a test client knows which bundle member it is:
//! [`ClientAddEntryMsg::for_entity`] and [`ClientAddEntryMsg::entity_number`].

use crate::wire::{
    BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer, entity_of_raw_id, raw_id_of_entity,
};

/// File slot of `ClientAddEntryMsg`.
pub const CLIENT_ADD_ENTRY_MSG_TYPE_INDEX: u64 = 164;

/// Adds a routing entry for one id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientAddEntryMsg {
    /// The entry id. In the captures it is not the id of `PlayerManagerSelfIdentificationMsg`.
    pub entry_id: [u8; RAW_ID_LEN],
}

impl ClientAddEntryMsg {
    /// The entry of entity `number` (see the module note).
    pub fn for_entity(number: u64) -> Self {
        Self {
            entry_id: raw_id_of_entity(number),
        }
    }

    /// The entity number of an entry that [`ClientAddEntryMsg::for_entity`] made; `None` for
    /// another id.
    pub fn entity_number(&self) -> Option<u64> {
        entity_of_raw_id(&self.entry_id)
    }
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

    const ENTITY: u64 = 0x0102_0304_0506_0708;

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

    #[test]
    fn only_an_entity_entry_gives_an_entity_number() {
        assert_eq!(
            ClientAddEntryMsg::for_entity(ENTITY).entity_number(),
            Some(ENTITY)
        );
        let mut other_id = [1; RAW_ID_LEN];
        other_id[0] = 2;
        let other = ClientAddEntryMsg { entry_id: other_id };
        assert_eq!(other.entity_number(), None);
    }
}
