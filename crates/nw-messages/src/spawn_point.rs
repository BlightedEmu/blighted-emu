// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Javelin::ClientViewListenerTrait::ReceivePlayerSpawnPointMsg` (bodies note, section 1617):
//! an empty body. Reader `FUN_1414e3930`.

use crate::wire::{BodyCodec, MessageError, Reader};

/// File slot of `ReceivePlayerSpawnPointMsg`.
pub const RECEIVE_PLAYER_SPAWN_POINT_MSG_TYPE_INDEX: u64 = 1617;

/// Tells the client that its character is placed in the world.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReceivePlayerSpawnPointMsg;

impl BodyCodec for ReceivePlayerSpawnPointMsg {
    const TYPE_INDEX: u64 = RECEIVE_PLAYER_SPAWN_POINT_MSG_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        Ok(Vec::new())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        Reader::new(body).finish()?;
        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_is_empty() {
        assert_eq!(ReceivePlayerSpawnPointMsg.encode(), Ok(Vec::new()));
        assert_eq!(
            ReceivePlayerSpawnPointMsg::decode(&[]),
            Ok(ReceivePlayerSpawnPointMsg)
        );
        assert_eq!(
            ReceivePlayerSpawnPointMsg::decode(&[0]),
            Err(MessageError::TrailingBytes { left: 1 })
        );
    }
}
