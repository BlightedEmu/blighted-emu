// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Javelin::ClientMessagesTrait::PlayerManagerSelfIdentificationMsg` (type 1628). Receiving and
//! parsing this message is what moves the real client out of `WaitingForActorGameConnection`:
//! its handler (`FUN_146454c00`, build 25810808 `FUN_146455ec0`) always calls the
//! `CJavelinActorGame` `ConnectionSuccess` setter (`FUN_145a87010`) at its end, which sets the
//! actor-game state field that the login state machine (`FUN_14644a070`) polls.
//!
//! The unmarshal (`FUN_1414fd530`, build 25810808 `FUN_141516e40`) reads a fixed field set; each
//! field reader fails only when the stream runs out (the two flag bytes must also be 0 or 1).
//! See `docs/re/2026-09-02-post-registration-sequence.md` and the world-entry note. This server
//! sends the smallest body that the unmarshal accepts: every id and number is zero, and the two
//! presence flags are set so the reader takes its fixed-size path. No capture byte is copied.

use crate::wire::{BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer};

/// File slot of `PlayerManagerSelfIdentificationMsg`.
pub const PLAYER_MANAGER_SELF_IDENTIFICATION_MSG_TYPE_INDEX: u64 = 1628;

/// The flag byte of a presence block (`FUN_14087ac50`). Bit 0 set makes the reader take the
/// fixed 16-byte path instead of the variable sub-reader path; bits 1 to 3 only pick a text
/// format and read no stream bytes.
const PRESENCE_FLAG_FIXED: u8 = 0x01;

/// Count of `FUN_1415acc30` blocks between the two presence blocks (unmarshal reads three).
const MIDDLE_BLOCK_COUNT: usize = 3;

/// Tells the real client who its character is in this session. Receiving it ends the client's
/// `WaitingForActorGameConnection` wait.
///
/// The emulator sends the smallest body the client accepts, so this type holds no field: every
/// value in the body is zero. [`decode`](Self::decode) still checks the whole wire grammar, so a
/// body that does not match its layout is rejected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerManagerSelfIdentificationMsg;

impl BodyCodec for PlayerManagerSelfIdentificationMsg {
    const TYPE_INDEX: u64 = PLAYER_MANAGER_SELF_IDENTIFICATION_MSG_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        // Presence block one (`FUN_14087ac50`): flag, then a 16-byte id.
        writer.u8(PRESENCE_FLAG_FIXED).bytes(&[0; RAW_ID_LEN]);
        // Three `FUN_1415acc30` blocks: a big-endian u32 and two 16-byte ids each.
        for _ in 0..MIDDLE_BLOCK_COUNT {
            writer
                .u32(0)
                .bytes(&[0; RAW_ID_LEN])
                .bytes(&[0; RAW_ID_LEN]);
        }
        // The `FUN_1415bad60` block: a u32, a prefix-coded array count (empty), two flags, then
        // two big-endian u32 values.
        writer.u32(0);
        writer.prefixed(0)?;
        writer.boolean(false).boolean(false).u32(0).u32(0);
        // Presence block two: flag, then a 16-byte id.
        writer.u8(PRESENCE_FLAG_FIXED).bytes(&[0; RAW_ID_LEN]);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        read_presence_block(&mut reader)?;
        for _ in 0..MIDDLE_BLOCK_COUNT {
            reader.u32()?;
            reader.raw_id()?;
            reader.raw_id()?;
        }
        reader.u32()?;
        let array_len = reader.prefixed()?;
        for _ in 0..array_len {
            reader.u32()?;
        }
        reader.boolean()?;
        reader.boolean()?;
        reader.u32()?;
        reader.u32()?;
        read_presence_block(&mut reader)?;
        reader.finish()?;
        Ok(Self)
    }
}

/// Reads one presence block (`FUN_14087ac50`): a flag byte whose bit 0 must be set, then a
/// 16-byte id. The variable sub-reader path (bit 0 clear) is not a form this server sends.
fn read_presence_block(reader: &mut Reader) -> Result<(), MessageError> {
    let flag = reader.u8()?;
    if flag & PRESENCE_FLAG_FIXED == 0 {
        return Err(MessageError::BadTag {
            field: "PlayerManagerSelfIdentificationMsg presence flag",
            tag: flag,
        });
    }
    reader.raw_id()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The byte length of the smallest body: two presence blocks (1 + 16 each), three middle
    /// blocks (4 + 16 + 16 each), and the `FUN_1415bad60` block (4 + 1 + 1 + 1 + 4 + 4).
    const MINIMAL_BODY_LEN: usize = (1 + RAW_ID_LEN) * 2
        + MIDDLE_BLOCK_COUNT * (4 + RAW_ID_LEN + RAW_ID_LEN)
        + (4 + 1 + 1 + 1 + 4 + 4);

    #[test]
    fn the_minimal_body_round_trips() {
        let body = PlayerManagerSelfIdentificationMsg.encode().expect("encode");
        assert_eq!(body.len(), MINIMAL_BODY_LEN);
        assert_eq!(
            PlayerManagerSelfIdentificationMsg::decode(&body),
            Ok(PlayerManagerSelfIdentificationMsg)
        );
    }

    #[test]
    fn a_presence_flag_without_bit_zero_is_rejected() {
        let mut body = PlayerManagerSelfIdentificationMsg.encode().expect("encode");
        body[0] = 0;
        assert!(matches!(
            PlayerManagerSelfIdentificationMsg::decode(&body),
            Err(MessageError::BadTag { .. })
        ));
    }

    #[test]
    fn a_short_body_is_rejected() {
        let body = PlayerManagerSelfIdentificationMsg.encode().expect("encode");
        assert!(matches!(
            PlayerManagerSelfIdentificationMsg::decode(&body[..body.len() - 1]),
            Err(MessageError::Truncated { .. })
        ));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut body = PlayerManagerSelfIdentificationMsg.encode().expect("encode");
        body.push(0);
        assert_eq!(
            PlayerManagerSelfIdentificationMsg::decode(&body),
            Err(MessageError::TrailingBytes { left: 1 })
        );
    }
}
