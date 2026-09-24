// SPDX-License-Identifier: AGPL-3.0-or-later
//! The server's two ways to end a player's session (run-15 core study).
//!
//! - 271 `KickClientTrait::KickClientMsg`: a reason `u32` and nine bytes. All 40 retail kicks
//!   read (the 40 smallest of the 124 ledgers with one) are `00000005` and nine zero bytes, sent
//!   while the client still played (its last input 3 to 48 ms before); the client closed the
//!   connection 50 to 150 ms later. The meaning of reason 5 and of the nine bytes is Unknown.
//! - 1630 `Javelin::ClientMessagesTrait::PlayerManagerRejectedMsg`: a reason `u32` and a text.
//!   The one retail message (`MSQ5-lv25`, at a maintenance) is 14 and `Maintenance_Mode`; 14 is
//!   `Errors::aoi_maintenance_mode` counted from `Errors::aoi_errors` in the client's error
//!   list (`FUN_1410037c0`), and the text names the client's popup `@mm_rejected_<text>`.

use crate::combat::{read_string, write_string};
use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// Type index of `KickClientTrait::KickClientMsg`.
pub const KICK_CLIENT_TYPE_INDEX: u64 = 271;
/// Type index of `Javelin::ClientMessagesTrait::PlayerManagerRejectedMsg`.
pub const PLAYER_MANAGER_REJECTED_TYPE_INDEX: u64 = 1630;
/// The reason of every retail kick.
pub const RETAIL_KICK_REASON: u32 = 5;
/// The nine bytes after the reason: zero on every retail kick.
pub const KICK_TAIL: [u8; KICK_TAIL_LEN] = [0; KICK_TAIL_LEN];
const KICK_TAIL_LEN: usize = 9;
/// The rejection of a world that closes for maintenance (`Errors::aoi_maintenance_mode`).
pub const MAINTENANCE_REJECTION: u32 = 14;
/// The text of [`MAINTENANCE_REJECTION`].
pub const MAINTENANCE_REJECTION_TEXT: &str = "Maintenance_Mode";

/// 271: the server ends the client's session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KickClientMsg {
    /// The reason.
    pub reason: u32,
}

impl BodyCodec for KickClientMsg {
    const TYPE_INDEX: u64 = KICK_CLIENT_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        Ok(writer.u32(self.reason).bytes(&KICK_TAIL).finish())
    }

    /// Nine bytes other than [`KICK_TAIL`] are [`MessageError::RowsMismatch`]: their layout is
    /// Unknown.
    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let reason = reader.u32()?;
        if reader.array::<KICK_TAIL_LEN>()? != KICK_TAIL {
            return Err(MessageError::RowsMismatch);
        }
        reader.finish()?;
        Ok(Self { reason })
    }
}

/// 1630: the server takes the player out of the world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerManagerRejectedMsg {
    /// The reason, counted from `Errors::aoi_errors`.
    pub reason: u32,
    /// The reason's text.
    pub text: Vec<u8>,
}

impl PlayerManagerRejectedMsg {
    /// The rejection of a world that closes for maintenance.
    pub fn maintenance() -> Self {
        Self {
            reason: MAINTENANCE_REJECTION,
            text: MAINTENANCE_REJECTION_TEXT.as_bytes().to_vec(),
        }
    }
}

impl BodyCodec for PlayerManagerRejectedMsg {
    const TYPE_INDEX: u64 = PLAYER_MANAGER_REJECTED_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer.u32(self.reason);
        write_string(&mut writer, &self.text)?;
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let reason = reader.u32()?;
        let text = read_string(&mut reader)?;
        reader.finish()?;
        Ok(Self { reason, text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// Every retail 271 body.
    const RETAIL_KICK: &str = "00000005000000000000000000";
    /// The retail 1630 body.
    const RETAIL_REJECTION: &str = "0000000e104d61696e74656e616e63655f4d6f6465";

    #[test]
    fn the_retail_bodies_decode_and_encode_byte_exact() {
        let kick = KickClientMsg::decode(&hex(RETAIL_KICK)).expect("decodes");
        assert_eq!(kick.reason, RETAIL_KICK_REASON);
        assert_eq!(kick.encode().expect("encodes"), hex(RETAIL_KICK));
        let rejection = PlayerManagerRejectedMsg::decode(&hex(RETAIL_REJECTION)).expect("decodes");
        assert_eq!(rejection, PlayerManagerRejectedMsg::maintenance());
        assert_eq!(rejection.encode().expect("encodes"), hex(RETAIL_REJECTION));
    }

    #[test]
    fn a_kick_with_other_tail_bytes_is_refused() {
        let mut body = hex(RETAIL_KICK);
        body[KICK_TAIL_LEN] = 1;
        assert_eq!(
            KickClientMsg::decode(&body),
            Err(MessageError::RowsMismatch)
        );
    }
}
