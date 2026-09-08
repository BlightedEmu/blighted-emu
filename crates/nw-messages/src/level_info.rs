// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Javelin::ClientMessagesTrait::LevelInfoChangedMsg` (type 1635). It tells the client which
//! level to load and the session context to use from then on. The client handler
//! (`FUN_146446800`) runs `LoadContextAndLevel` only after both this message and
//! `PlayerManagerSelfIdentification` (type 1628) have arrived, which is one of the conditions for
//! the client to leave `WaitingForPlayerSpawn` and enter the world.
//!
//! Layout derived from existing data, not a fresh decompile: the field list of
//! `local-data/azoth-catalog/packets/1635.json` plus the captured body lengths in the
//! bootstrap-bodies note (open world is the 106-byte body). The published field list sums to 89
//! bytes; the 17-byte remainder is exactly one phasing entry (`u8` + `u32` + `u64` + `u32`), so
//! the open-world message carries `phasing.count = 1`. The byte offsets in that archive are from
//! one sample and are not reliable; the field order and the 106-byte total are.
//!
//! Two hard client rules from the handler: `is_master` must be 1 (with 0 the handler logs
//! "IsMasterPlayer is false" and drops the message), and each phasing entry's `phase_id` must not
//! be 0.

use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// File slot of `LevelInfoChangedMsg`.
pub const LEVEL_INFO_CHANGED_MSG_TYPE_INDEX: u64 = 1635;

/// The open-world level name the client loads (`map <name>`).
pub const OPEN_WORLD_MAP_NAME: &[u8] = b"NewWorld_VitaeEterna";
/// Where the open-world level's terrain data lives.
pub const OPEN_WORLD_COATLICUE_PATH: &[u8] = b"coatlicue/NewWorld_VitaeEterna";
/// Open-world phasing-grid origin X (Azoth catalog 1635 spec).
pub const OPEN_WORLD_PHASING_ORIGIN_X: f32 = 2048.0;
/// Open-world phasing-grid origin Y.
pub const OPEN_WORLD_PHASING_ORIGIN_Y: f32 = 16.0;
/// Open-world phasing-grid width (12272 m = 48 cells of 256 m).
pub const OPEN_WORLD_PHASING_BOUNDS_X: f32 = 12272.0;
/// Open-world phasing-grid height (10250 m = 41 cells of 256 m).
pub const OPEN_WORLD_PHASING_BOUNDS_Y: f32 = 10250.0;

/// The `reserved` field: 0 in the open-world captures.
const RESERVED: u64 = 0;
/// `flagA0` in the spec: 1 in the open-world captures.
const FLAG_A0: u8 = 1;
/// `is_master`: must be 1 or the client drops the message.
const IS_MASTER: u8 = 1;
/// `flagA3`: 0 in the open-world captures.
const FLAG_A3: u8 = 0;

/// One phasing entry: which phase the player is in for one sector of the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhasingEntry {
    /// `keyType`: 1 = territory/point-of-interest area, 2 = background grid cell.
    pub key_type: u8,
    /// The sector key, read according to `key_type`.
    pub key_id: u32,
    /// The phase the player is in for that sector. Must not be 0.
    pub phase_id: u64,
    /// A trailing value the captures show as 0 or 2.
    pub extra: u32,
}

/// Tells the client which level to load and the session context to use. Only the session-specific
/// fields are stored; the open-world level name, terrain path and phasing grid are constants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelInfoChangedMsg {
    /// `clientCtx`: this session's client-context id. Every state bundle that follows carries the
    /// same number.
    pub client_context: u8,
    /// `instance`: a clock stamp for this level instance; the client only compares it with the
    /// previous one, so it must differ from the last and (first time) be non-zero.
    pub instance: u64,
    /// The phasing entries. The open-world message carries one.
    pub phasing: Vec<PhasingEntry>,
}

impl LevelInfoChangedMsg {
    /// The open-world message for one session: one phasing entry that places the player in
    /// `phase_id` of the whole-world area, with the given client-context id and instance stamp.
    pub fn open_world(client_context: u8, instance: u64, phase_id: u64) -> Self {
        Self {
            client_context,
            instance,
            phasing: vec![PhasingEntry {
                key_type: 1,
                key_id: 0,
                phase_id,
                extra: 0,
            }],
        }
    }
}

impl BodyCodec for LevelInfoChangedMsg {
    const TYPE_INDEX: u64 = LEVEL_INFO_CHANGED_MSG_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer.prefixed(OPEN_WORLD_MAP_NAME.len() as u64)?;
        writer.bytes(OPEN_WORLD_MAP_NAME);
        writer.prefixed(OPEN_WORLD_COATLICUE_PATH.len() as u64)?;
        writer.bytes(OPEN_WORLD_COATLICUE_PATH);
        writer
            .f32(OPEN_WORLD_PHASING_ORIGIN_X)
            .f32(OPEN_WORLD_PHASING_ORIGIN_Y)
            .f32(OPEN_WORLD_PHASING_BOUNDS_X)
            .f32(OPEN_WORLD_PHASING_BOUNDS_Y)
            .u64(RESERVED);
        writer.prefixed(self.phasing.len() as u64)?;
        for entry in &self.phasing {
            writer
                .u8(entry.key_type)
                .u32(entry.key_id)
                .u64(entry.phase_id)
                .u32(entry.extra);
        }
        writer
            .u8(FLAG_A0)
            .u8(self.client_context)
            .u8(IS_MASTER)
            .u8(FLAG_A3)
            .u64(self.instance);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        expect_string(&mut reader, OPEN_WORLD_MAP_NAME)?;
        expect_string(&mut reader, OPEN_WORLD_COATLICUE_PATH)?;
        reader.f32()?;
        reader.f32()?;
        reader.f32()?;
        reader.f32()?;
        reader.u64()?;
        let count = reader.prefixed()?;
        let mut phasing = Vec::new();
        for _ in 0..count {
            phasing.push(PhasingEntry {
                key_type: reader.u8()?,
                key_id: reader.u32()?,
                phase_id: reader.u64()?,
                extra: reader.u32()?,
            });
        }
        reader.u8()?;
        let client_context = reader.u8()?;
        reader.u8()?;
        reader.u8()?;
        let instance = reader.u64()?;
        reader.finish()?;
        Ok(Self {
            client_context,
            instance,
            phasing,
        })
    }
}

/// Reads a prefix-coded string and checks it equals `expected`.
fn expect_string(reader: &mut Reader, expected: &[u8]) -> Result<(), MessageError> {
    let len = reader.prefixed()?;
    let bytes = reader.bytes(usize::try_from(len).unwrap_or(usize::MAX))?;
    if bytes == expected {
        Ok(())
    } else {
        Err(MessageError::LengthMismatch {
            field: "LevelInfoChangedMsg level name",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The captured open-world body length.
    const OPEN_WORLD_BODY_LEN: usize = 106;

    #[test]
    fn the_open_world_body_is_the_captured_length_and_round_trips() {
        let message = LevelInfoChangedMsg::open_world(1, 0x0102_0304_0506_0708, 1);
        let body = message.encode().expect("encode");
        assert_eq!(body.len(), OPEN_WORLD_BODY_LEN);
        assert_eq!(LevelInfoChangedMsg::decode(&body), Ok(message));
    }

    #[test]
    fn an_empty_phasing_list_round_trips() {
        let message = LevelInfoChangedMsg {
            client_context: 2,
            instance: 9,
            phasing: Vec::new(),
        };
        let body = message.encode().expect("encode");
        assert_eq!(LevelInfoChangedMsg::decode(&body), Ok(message));
    }

    #[test]
    fn a_wrong_level_name_is_rejected() {
        let message = LevelInfoChangedMsg::open_world(1, 1, 1);
        let mut body = message.encode().expect("encode");
        // Corrupt the first byte of the map name (after its 1-byte length prefix).
        body[1] ^= 0xff;
        assert!(matches!(
            LevelInfoChangedMsg::decode(&body),
            Err(MessageError::LengthMismatch { .. })
        ));
    }
}
