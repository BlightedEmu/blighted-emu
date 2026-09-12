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

/// One level the client can load: what `LevelInfoChangedMsg` names and the frame it sends
/// (new-player-start note: the open world and the tutorial `FTUE_V2`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelSpec {
    /// Our level folder name (world files and sim config), for example `ftue_v2`.
    pub folder: &'static str,
    /// The level name the client loads (`map <name>`).
    pub map_name: &'static [u8],
    /// Where the level's terrain data lives.
    pub coatlicue_path: &'static [u8],
    /// Phasing-grid origin x and y, metres.
    pub phasing_origin: [f32; 2],
    /// Phasing-grid width and height, metres.
    pub phasing_bounds: [f32; 2],
    /// `flagA3`: 0 in the open world, 1 in the tutorial.
    pub flag_a3: u8,
}

/// The open world.
pub const OPEN_WORLD_LEVEL: LevelSpec = LevelSpec {
    folder: "newworld_vitaeeterna",
    map_name: b"NewWorld_VitaeEterna",
    coatlicue_path: b"coatlicue/NewWorld_VitaeEterna",
    // 12272 m = 48 cells of 256 m; 10250 m = 41 cells of 256 m (Azoth catalog 1635 spec).
    phasing_origin: [2048.0, 16.0],
    phasing_bounds: [12272.0, 10250.0],
    flag_a3: 0,
};

/// The new-player tutorial: a private level before the open world.
pub const TUTORIAL_LEVEL: LevelSpec = LevelSpec {
    folder: "ftue_v2",
    map_name: b"FTUE_V2",
    coatlicue_path: b"coatlicue/FTUE_V2",
    phasing_origin: [400.0, 150.0],
    phasing_bounds: [416.0, 736.0],
    flag_a3: 1,
};

/// Every level this server can name.
pub const LEVELS: [LevelSpec; 2] = [OPEN_WORLD_LEVEL, TUTORIAL_LEVEL];

impl LevelSpec {
    /// The level whose folder name is `folder`.
    pub fn of_folder(folder: &str) -> Option<Self> {
        LEVELS.into_iter().find(|level| level.folder == folder)
    }
}

/// The `clientCtx` of the first level of a session; each later level change takes the next one.
pub const LOGIN_CLIENT_CONTEXT: u8 = 1;

/// `flagA0` in the spec: 1 in every capture.
const FLAG_A0: u8 = 1;
/// `is_master`: must be 1 or the client drops the message.
const IS_MASTER: u8 = 1;
/// The `reserved` field of the open world: 0 in the captures.
const OPEN_WORLD_RESERVED: u64 = 0;
/// `keyType` of the single open-world phasing entry: a territory area.
const AREA_KEY_TYPE: u8 = 1;
/// `keyType` of the tutorial's phasing entry: a private instance.
const INSTANCE_KEY_TYPE: u8 = 3;
/// `extra` of the tutorial's phasing entry.
const INSTANCE_EXTRA: u32 = 2;

/// One phasing entry: which phase the player is in for one sector of the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhasingEntry {
    /// `keyType`: 1 = territory/point-of-interest area, 2 = background grid cell, 3 = private
    /// instance (tutorial).
    pub key_type: u8,
    /// The sector key, read according to `key_type`.
    pub key_id: u32,
    /// The phase the player is in for that sector. Must not be 0.
    pub phase_id: u64,
    /// A trailing value the captures show as 0 or 2.
    pub extra: u32,
}

/// Tells the client which level to load and the session context to use.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelInfoChangedMsg {
    /// The level.
    pub level: LevelSpec,
    /// `reserved`: 0 in the open world; the instance phase id in the tutorial.
    pub reserved: u64,
    /// `clientCtx`: this level's client-context id. Every state bundle that follows carries the
    /// same number.
    pub client_context: u8,
    /// `instance`: a clock stamp for this level instance; the client only compares it with the
    /// previous one, so it must differ from the last and (first time) be non-zero.
    pub instance: u64,
    /// The phasing entries.
    pub phasing: Vec<PhasingEntry>,
}

impl LevelInfoChangedMsg {
    /// The open-world message: one phasing entry that places the player in `phase_id` of the
    /// whole-world area, with the given client-context id and instance stamp.
    pub fn open_world(client_context: u8, instance: u64, phase_id: u64) -> Self {
        Self {
            level: OPEN_WORLD_LEVEL,
            reserved: OPEN_WORLD_RESERVED,
            client_context,
            instance,
            phasing: vec![PhasingEntry {
                key_type: AREA_KEY_TYPE,
                key_id: 0,
                phase_id,
                extra: 0,
            }],
        }
    }

    /// The tutorial message: the private instance `phase_id` (also in `reserved`, as in the
    /// captures).
    pub fn tutorial(client_context: u8, instance: u64, phase_id: u64) -> Self {
        Self {
            level: TUTORIAL_LEVEL,
            reserved: phase_id,
            client_context,
            instance,
            phasing: vec![PhasingEntry {
                key_type: INSTANCE_KEY_TYPE,
                key_id: 0,
                phase_id,
                extra: INSTANCE_EXTRA,
            }],
        }
    }

    /// The message of `level`: [`Self::tutorial`] for the tutorial, else [`Self::open_world`].
    pub fn of_level(level: LevelSpec, client_context: u8, instance: u64, phase_id: u64) -> Self {
        if level == TUTORIAL_LEVEL {
            Self::tutorial(client_context, instance, phase_id)
        } else {
            Self::open_world(client_context, instance, phase_id)
        }
    }
}

impl BodyCodec for LevelInfoChangedMsg {
    const TYPE_INDEX: u64 = LEVEL_INFO_CHANGED_MSG_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer.prefixed(self.level.map_name.len() as u64)?;
        writer.bytes(self.level.map_name);
        writer.prefixed(self.level.coatlicue_path.len() as u64)?;
        writer.bytes(self.level.coatlicue_path);
        writer
            .f32(self.level.phasing_origin[0])
            .f32(self.level.phasing_origin[1])
            .f32(self.level.phasing_bounds[0])
            .f32(self.level.phasing_bounds[1])
            .u64(self.reserved);
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
            .u8(self.level.flag_a3)
            .u64(self.instance);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let name = read_string(&mut reader)?;
        let level = LEVELS
            .into_iter()
            .find(|level| level.map_name == name)
            .ok_or(MessageError::LengthMismatch {
                field: "LevelInfoChangedMsg level name",
            })?;
        if read_string(&mut reader)? != level.coatlicue_path {
            return Err(MessageError::LengthMismatch {
                field: "LevelInfoChangedMsg terrain path",
            });
        }
        reader.f32()?;
        reader.f32()?;
        reader.f32()?;
        reader.f32()?;
        let reserved = reader.u64()?;
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
            level,
            reserved,
            client_context,
            instance,
            phasing,
        })
    }
}

/// Reads a prefix-coded string.
fn read_string<'a>(reader: &mut Reader<'a>) -> Result<&'a [u8], MessageError> {
    let len = reader.prefixed()?;
    reader.bytes(usize::try_from(len).unwrap_or(usize::MAX))
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
            level: OPEN_WORLD_LEVEL,
            reserved: 0,
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

    /// The captured tutorial body length (new-player-start note).
    const TUTORIAL_BODY_LEN: usize = 80;

    #[test]
    fn the_tutorial_body_is_the_captured_shape_and_round_trips() {
        const PHASE: u64 = 0x8b2c_bbb8_6aa9_9537;
        let message = LevelInfoChangedMsg::tutorial(LOGIN_CLIENT_CONTEXT, 7, PHASE);
        let body = message.encode().expect("encode");
        assert_eq!(body.len(), TUTORIAL_BODY_LEN);
        assert_eq!(&body[1..8], b"FTUE_V2");
        let decoded = LevelInfoChangedMsg::decode(&body).expect("decode");
        assert_eq!(decoded, message);
        assert_eq!(decoded.reserved, PHASE);
        assert_eq!(decoded.phasing[0].key_type, INSTANCE_KEY_TYPE);
        assert_eq!(LevelSpec::of_folder("ftue_v2"), Some(TUTORIAL_LEVEL));
        assert_eq!(
            LevelInfoChangedMsg::of_level(TUTORIAL_LEVEL, 1, 7, PHASE),
            message
        );
    }
}
