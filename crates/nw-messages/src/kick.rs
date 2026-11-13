// SPDX-License-Identifier: AGPL-3.0-or-later
//! The server's two ways to end a player's session (run-15 core study).
//!
//! - 271 `KickClientTrait::KickClientMsg`: a reason `u32` and nine bytes. All 40 retail kicks
//!   read (the 40 smallest of the 124 ledgers with one) are `00000005` and nine zero bytes, sent
//!   while the client still played (its last input 3 to 48 ms before); the client closed the
//!   connection 50 to 150 ms later. Observed (client handler `FUN_146447430`): reason `r` from 0
//!   to 8 is the client error `Errors::kicked` + `r` (`FUN_1410037c0`: 0x39 `kicked`, 0x3a
//!   `kicked_duplicate_session`, 0x3b `kicked_character_deactivated`, 0x3c
//!   `kicked_maintenence`, 0x3d `kicked_abuse`, 0x3e `kicked_easyanticheat`, 0x3f
//!   `kicked_unknown`, 0x40 `kicked_expansion_ownership`, 0x41 `kicked_server_error`), 9 to 24
//!   are the bans (0x42 to 0x51), and the text `Cheating_EAC_Ban` on reason 5 is 0x46
//!   `banned_cheating`. So the retail reason 5 is the EasyAntiCheat kick
//!   (`@mm_rejected_Client_Kicked_EasyAntiCheat`, `lyshineui/popup/uierrors.luac`). No 271
//!   reason gives the AFK error. The nine bytes are Unknown.
//! - 1630 `Javelin::ClientMessagesTrait::PlayerManagerRejectedMsg`: a reason `u32` and a text.
//!   The one retail message (`MSQ5-lv25`, at a maintenance) is 14 and `Maintenance_Mode`.
//!   Observed (client handler `FUN_146455c70`): reason 0 to 13 is the client error
//!   `Errors::aoi_errors` (0x17) + reason, so 13 is 0x24 `Errors::aoi_afk_kicked`, and 14 is
//!   0x3c `kicked_maintenence`; `uierrors.luac` maps `UIError_aoi_afk_kicked` to
//!   `@mm_rejected_Client_Kicked_AFK` ("You have been kicked from the game for being away too
//!   long.") and `UIError_kicked_maintenence` to the maintenance text. The client's popup takes
//!   its text from the error; the message text goes with the error to its log.

use crate::combat::{read_string, write_string};
use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// Type index of `KickClientTrait::KickClientMsg`.
pub const KICK_CLIENT_TYPE_INDEX: u64 = 271;
/// Type index of `Javelin::ClientMessagesTrait::PlayerManagerRejectedMsg`.
pub const PLAYER_MANAGER_REJECTED_TYPE_INDEX: u64 = 1630;
/// The reason of every retail kick: the EasyAntiCheat kick (`FUN_146447430`). This server sends
/// it on a game master's kick, as the retail captures show.
pub const RETAIL_KICK_REASON: u32 = 5;
/// The nine bytes after the reason: zero on every retail kick.
pub const KICK_TAIL: [u8; KICK_TAIL_LEN] = [0; KICK_TAIL_LEN];
const KICK_TAIL_LEN: usize = 9;
/// The rejection of a world that closes for maintenance (`Errors::aoi_maintenance_mode`).
pub const MAINTENANCE_REJECTION: u32 = 14;
/// The text of [`MAINTENANCE_REJECTION`].
pub const MAINTENANCE_REJECTION_TEXT: &str = "Maintenance_Mode";
/// The rejection of a player that was away too long: 13, `Errors::aoi_afk_kicked`
/// (`FUN_146455c70`), the client's `@mm_rejected_Client_Kicked_AFK` popup.
pub const AFK_REJECTION: u32 = 13;
/// The text of [`AFK_REJECTION`]. Inferred: no capture holds an AFK removal; the text follows
/// the retail maintenance one, the error's name after `aoi_` (`aoi_maintenance_mode`,
/// `Maintenance_Mode`). The client's popup does not show it.
pub const AFK_REJECTION_TEXT: &str = "Afk_Kicked";

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

    /// The removal of a player that was away too long ([`AFK_REJECTION`]).
    pub fn afk() -> Self {
        Self {
            reason: AFK_REJECTION,
            text: AFK_REJECTION_TEXT.as_bytes().to_vec(),
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

    /// The client's error of a 1630 reason (`FUN_146455c70`): `Errors::aoi_errors` (0x17) plus
    /// the reason for 0 to 13.
    const AOI_ERRORS: u32 = 0x17;
    /// `Errors::aoi_afk_kicked` (`FUN_1410037c0`).
    const AOI_AFK_KICKED: u32 = 0x24;

    #[test]
    fn the_afk_removal_is_the_rejection_of_the_afk_error() {
        let afk = PlayerManagerRejectedMsg::afk();
        assert_eq!(AOI_ERRORS + afk.reason, AOI_AFK_KICKED);
        assert_ne!(afk.reason, MAINTENANCE_REJECTION);
        let body = afk.encode().expect("encodes");
        assert_eq!(body, hex("0000000d0a41666b5f4b69636b6564"));
        assert_eq!(PlayerManagerRejectedMsg::decode(&body), Ok(afk));
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
