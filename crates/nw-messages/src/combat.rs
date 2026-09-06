// SPDX-License-Identifier: AGPL-3.0-or-later
//! The combat messages (damage note sections 1 and 2, death note sections 2 and 3). Every field
//! is kept, by position, with a neutral name where its meaning is Unknown, so a captured body
//! encodes back to the same bytes. Counts use the wide prefix (`FUN_14087b5c0`); strings use the
//! prefix-coded length of `FUN_14087a3d0`, as in the client input.

use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// `DamageReceiverComponentClientFacet_OnDamageDealt`.
pub const DAMAGE_DEALT_TYPE_INDEX: u64 = 2071;
/// `VitalsComponentClientFacet_OnDamage`.
pub const ON_DAMAGE_TYPE_INDEX: u64 = 3601;
/// `VitalsComponentClientFacet_ClientSyncDeathRecap`.
pub const DEATH_RECAP_TYPE_INDEX: u64 = 4299;
/// `PlayerComponentServerFacet_RequestRespawn` (client to server).
pub const REQUEST_RESPAWN_TYPE_INDEX: u64 = 1938;
/// The item tag of 86 of 93 captured training-dummy hits; its meaning is Unknown.
pub const HIT_TAG: u8 = 11;
/// The scale of a hit on a training dummy.
pub const NEUTRAL_SCALE: f32 = 1.0;
/// Longest respawn payload: a count below 33 (`FUN_14669cc70`).
pub const MAX_RESPAWN_PAYLOAD: usize = 32;
/// Longest string (`FUN_14087a3d0`: a length below `0x30000`).
const MAX_STRING: u64 = 0x2_FFFF;
/// Lists of the recap: counts below 6, then below 4 (`FUN_142db7920`).
const RECAP_LIST_LIMIT: usize = 6;
const RECAP_LAST_LIST_LIMIT: usize = 4;
/// The 16-byte blocks of the recap (`FUN_1415bc800`).
const BLOCK_LEN: usize = 16;
/// The 7 raw bytes before the third `u16` of the recap.
const RAW_7_LEN: usize = 7;
/// Bytes of one damage item.
const ITEM_LEN: usize = 9;

/// One damage item (`FUN_145b56640`, `FUN_145ce6260`): a tag byte and two big-endian `f32`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageItem {
    /// A small kind code; Unknown.
    pub tag: u8,
    /// The hit amount (Inferred).
    pub amount: f32,
    /// A scale: 1 on a training dummy, below 1 in fights (Inferred).
    pub scale: f32,
}

fn read_items(reader: &mut Reader<'_>) -> Result<Vec<DamageItem>, MessageError> {
    let count = reader.wide_prefixed()?;
    let fits = reader.remaining() / ITEM_LEN;
    if count > fits as u64 {
        return Err(MessageError::CountTooLarge {
            count,
            limit: fits as u64,
        });
    }
    (0..count)
        .map(|_| {
            Ok(DamageItem {
                tag: reader.u8()?,
                amount: reader.f32()?,
                scale: reader.f32()?,
            })
        })
        .collect()
}

fn write_items(writer: &mut Writer, items: &[DamageItem]) {
    writer.wide_prefixed(items.len() as u64);
    for item in items {
        writer.u8(item.tag).f32(item.amount).f32(item.scale);
    }
}

/// One hit of `amount` for the builders.
fn one_hit(amount: f32) -> Vec<DamageItem> {
    vec![DamageItem {
        tag: HIT_TAG,
        amount,
        scale: NEUTRAL_SCALE,
    }]
}

/// Type 2071: a hit dealt.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageDealtMsg {
    /// The first big-endian `u64`; Unknown.
    pub head_a: u64,
    /// The second big-endian `u64`; Unknown.
    pub head_b: u64,
    /// 16 raw bytes.
    pub raw_16: [u8; 16],
    /// A big-endian `u64`.
    pub u64_c: u64,
    /// A big-endian `u32`.
    pub u32_d: u32,
    /// A big-endian `u32`.
    pub u32_e: u32,
    /// 2 raw bytes.
    pub raw_2: [u8; 2],
    /// The items.
    pub items: Vec<DamageItem>,
}

impl DamageDealtMsg {
    /// A hit of `amount` on `target` by `attacker`; every other field 0 (our own choice, M6-B
    /// spec section 7).
    pub fn hit(target: u64, attacker: u64, amount: f32) -> Self {
        Self {
            head_a: target,
            head_b: attacker,
            raw_16: [0; 16],
            u64_c: 0,
            u32_d: 0,
            u32_e: 0,
            raw_2: [0; 2],
            items: one_hit(amount),
        }
    }
}

impl BodyCodec for DamageDealtMsg {
    const TYPE_INDEX: u64 = DAMAGE_DEALT_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer
            .u64(self.head_a)
            .u64(self.head_b)
            .bytes(&self.raw_16)
            .u64(self.u64_c)
            .u32(self.u32_d)
            .u32(self.u32_e)
            .bytes(&self.raw_2);
        write_items(&mut writer, &self.items);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            head_a: reader.u64()?,
            head_b: reader.u64()?,
            raw_16: reader.array()?,
            u64_c: reader.u64()?,
            u32_d: reader.u32()?,
            u32_e: reader.u32()?,
            raw_2: reader.array()?,
            items: read_items(&mut reader)?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// Type 3601: damage the player takes.
#[derive(Debug, Clone, PartialEq)]
pub struct OnDamageMsg {
    /// The first big-endian `u64`; Unknown.
    pub head_a: u64,
    /// The second big-endian `u64`; Unknown.
    pub head_b: u64,
    /// A big-endian `u64`.
    pub u64_c: u64,
    /// 2 raw bytes.
    pub raw_2: [u8; 2],
    /// A big-endian `u32`.
    pub u32_d: u32,
    /// Three big-endian `u32` (`FUN_1415acfc0`).
    pub triple: [u32; 3],
    /// The items.
    pub items: Vec<DamageItem>,
}

impl OnDamageMsg {
    /// Damage of `amount` to `target` from `attacker`; every other field 0.
    pub fn hit(target: u64, attacker: u64, amount: f32) -> Self {
        Self {
            head_a: target,
            head_b: attacker,
            u64_c: 0,
            raw_2: [0; 2],
            u32_d: 0,
            triple: [0; 3],
            items: one_hit(amount),
        }
    }
}

impl BodyCodec for OnDamageMsg {
    const TYPE_INDEX: u64 = ON_DAMAGE_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer
            .u64(self.head_a)
            .u64(self.head_b)
            .u64(self.u64_c)
            .bytes(&self.raw_2)
            .u32(self.u32_d);
        for value in self.triple {
            writer.u32(value);
        }
        write_items(&mut writer, &self.items);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            head_a: reader.u64()?,
            head_b: reader.u64()?,
            u64_c: reader.u64()?,
            raw_2: reader.array()?,
            u32_d: reader.u32()?,
            triple: [reader.u32()?, reader.u32()?, reader.u32()?],
            items: read_items(&mut reader)?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// Type 1938: the client asks to respawn. The default is zero heads, flag 0 and no payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestRespawnMsg {
    /// The first big-endian `u64`; Unknown.
    pub head_a: u64,
    /// The second big-endian `u64`; Unknown.
    pub head_b: u64,
    /// A byte that must be 0 or 1.
    pub flag: bool,
    /// The request payload, at most [`MAX_RESPAWN_PAYLOAD`] bytes; Unknown.
    pub payload: Vec<u8>,
}

impl BodyCodec for RequestRespawnMsg {
    const TYPE_INDEX: u64 = REQUEST_RESPAWN_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        if self.payload.len() > MAX_RESPAWN_PAYLOAD {
            return Err(MessageError::CountTooLarge {
                count: self.payload.len() as u64,
                limit: MAX_RESPAWN_PAYLOAD as u64,
            });
        }
        let mut writer = Writer::new();
        writer
            .u64(self.head_a)
            .u64(self.head_b)
            .boolean(self.flag)
            .wide_prefixed(self.payload.len() as u64)
            .bytes(&self.payload);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let head_a = reader.u64()?;
        let head_b = reader.u64()?;
        let flag = reader.boolean()?;
        let count = reader.wide_prefixed()?;
        if count > MAX_RESPAWN_PAYLOAD as u64 {
            return Err(MessageError::CountTooLarge {
                count,
                limit: MAX_RESPAWN_PAYLOAD as u64,
            });
        }
        // `count` is at most 32.
        let payload = reader.bytes(count as usize)?.to_vec();
        reader.finish()?;
        Ok(Self {
            head_a,
            head_b,
            flag,
            payload,
        })
    }
}

/// Type 4299: the death recap. Field order of `FUN_1465b38c0`, `FUN_14669f120`,
/// `FUN_145ce8b20`, `FUN_145b4f440`, `FUN_145b51330` and `FUN_14669cf60`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathRecapMsg {
    /// The first big-endian `u64`; Unknown.
    pub head_a: u64,
    /// The second big-endian `u64`; Unknown.
    pub head_b: u64,
    /// The first string.
    pub string_a: Vec<u8>,
    /// The second string.
    pub string_b: Vec<u8>,
    /// A 0/1 byte.
    pub flag_a: bool,
    /// A big-endian `u32`.
    pub u32_a: u32,
    /// A big-endian `u32`.
    pub word_a: u32,
    /// A big-endian `u32`.
    pub word_b: u32,
    /// The third string.
    pub string_c: Vec<u8>,
    /// A big-endian `u16`.
    pub u16_a: u16,
    /// 16 bytes.
    pub block_1: [u8; BLOCK_LEN],
    /// A big-endian `u16`.
    pub u16_b: u16,
    /// 16 bytes.
    pub block_2: [u8; BLOCK_LEN],
    /// 7 raw bytes.
    pub raw_7: [u8; RAW_7_LEN],
    /// A big-endian `u16`.
    pub u16_c: u16,
    /// A 0/1 byte.
    pub flag_b: bool,
    /// A 0/1 byte.
    pub flag_c: bool,
    /// A big-endian `u32`.
    pub u32_b: u32,
    /// A big-endian `u32`.
    pub u32_c: u32,
    /// A big-endian `u32` (an item id on 89 of 280 captured bodies).
    pub u32_d: u32,
    /// A big-endian `u16`.
    pub u16_d: u16,
    /// A big-endian `u32`.
    pub u32_e: u32,
    /// A 0/1 byte.
    pub flag_d: bool,
    /// A big-endian `u32`.
    pub u32_f: u32,
    /// A big-endian `u32`.
    pub u32_g: u32,
    /// A 0/1 byte.
    pub flag_e: bool,
    /// A 0/1 byte.
    pub flag_f: bool,
    /// Big-endian `u32` values, fewer than 6.
    pub list_a: Vec<u32>,
    /// Big-endian `u32` values, fewer than 6.
    pub list_b: Vec<u32>,
    /// A 0/1 byte.
    pub flag_g: bool,
    /// Big-endian `u32` values, fewer than 4.
    pub list_c: Vec<u32>,
    /// The last big-endian `u32` (a status-effect id on 81 of 280 captured bodies).
    pub tail: u32,
}

impl DeathRecapMsg {
    /// A recap for `player`, killed by `killer`: empty strings, every other field 0 (M6-B spec
    /// section 8).
    pub fn of(player: u64, killer: u64) -> Self {
        Self {
            head_a: player,
            head_b: killer,
            string_a: Vec::new(),
            string_b: Vec::new(),
            flag_a: false,
            u32_a: 0,
            word_a: 0,
            word_b: 0,
            string_c: Vec::new(),
            u16_a: 0,
            block_1: [0; BLOCK_LEN],
            u16_b: 0,
            block_2: [0; BLOCK_LEN],
            raw_7: [0; RAW_7_LEN],
            u16_c: 0,
            flag_b: false,
            flag_c: false,
            u32_b: 0,
            u32_c: 0,
            u32_d: 0,
            u16_d: 0,
            u32_e: 0,
            flag_d: false,
            u32_f: 0,
            u32_g: 0,
            flag_e: false,
            flag_f: false,
            list_a: Vec::new(),
            list_b: Vec::new(),
            flag_g: false,
            list_c: Vec::new(),
            tail: 0,
        }
    }
}

fn write_string(writer: &mut Writer, string: &[u8]) -> Result<(), MessageError> {
    if string.len() as u64 > MAX_STRING {
        return Err(MessageError::CountTooLarge {
            count: string.len() as u64,
            limit: MAX_STRING,
        });
    }
    writer.prefixed(string.len() as u64)?.bytes(string);
    Ok(())
}

fn read_string(reader: &mut Reader<'_>) -> Result<Vec<u8>, MessageError> {
    let len = reader.prefixed()?;
    if len > MAX_STRING {
        return Err(MessageError::CountTooLarge {
            count: len,
            limit: MAX_STRING,
        });
    }
    // `len` is at most `MAX_STRING`.
    Ok(reader.bytes(len as usize)?.to_vec())
}

fn write_list(writer: &mut Writer, values: &[u32], limit: usize) -> Result<(), MessageError> {
    if values.len() >= limit {
        return Err(MessageError::CountTooLarge {
            count: values.len() as u64,
            limit: limit as u64 - 1,
        });
    }
    writer.wide_prefixed(values.len() as u64);
    for value in values {
        writer.u32(*value);
    }
    Ok(())
}

fn read_list(reader: &mut Reader<'_>, limit: usize) -> Result<Vec<u32>, MessageError> {
    let count = reader.wide_prefixed()?;
    if count >= limit as u64 {
        return Err(MessageError::CountTooLarge {
            count,
            limit: limit as u64 - 1,
        });
    }
    (0..count).map(|_| reader.u32()).collect()
}

impl BodyCodec for DeathRecapMsg {
    const TYPE_INDEX: u64 = DEATH_RECAP_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer.u64(self.head_a).u64(self.head_b);
        write_string(&mut writer, &self.string_a)?;
        write_string(&mut writer, &self.string_b)?;
        writer
            .boolean(self.flag_a)
            .u32(self.u32_a)
            .u32(self.word_a)
            .u32(self.word_b);
        write_string(&mut writer, &self.string_c)?;
        writer
            .u16(self.u16_a)
            .bytes(&self.block_1)
            .u16(self.u16_b)
            .bytes(&self.block_2)
            .bytes(&self.raw_7)
            .u16(self.u16_c)
            .boolean(self.flag_b)
            .boolean(self.flag_c)
            .u32(self.u32_b)
            .u32(self.u32_c)
            .u32(self.u32_d)
            .u16(self.u16_d)
            .u32(self.u32_e)
            .boolean(self.flag_d)
            .u32(self.u32_f)
            .u32(self.u32_g)
            .boolean(self.flag_e)
            .boolean(self.flag_f);
        write_list(&mut writer, &self.list_a, RECAP_LIST_LIMIT)?;
        write_list(&mut writer, &self.list_b, RECAP_LIST_LIMIT)?;
        writer.boolean(self.flag_g);
        write_list(&mut writer, &self.list_c, RECAP_LAST_LIST_LIMIT)?;
        writer.u32(self.tail);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            head_a: reader.u64()?,
            head_b: reader.u64()?,
            string_a: read_string(&mut reader)?,
            string_b: read_string(&mut reader)?,
            flag_a: reader.boolean()?,
            u32_a: reader.u32()?,
            word_a: reader.u32()?,
            word_b: reader.u32()?,
            string_c: read_string(&mut reader)?,
            u16_a: reader.u16()?,
            block_1: reader.array()?,
            u16_b: reader.u16()?,
            block_2: reader.array()?,
            raw_7: reader.array()?,
            u16_c: reader.u16()?,
            flag_b: reader.boolean()?,
            flag_c: reader.boolean()?,
            u32_b: reader.u32()?,
            u32_c: reader.u32()?,
            u32_d: reader.u32()?,
            u16_d: reader.u16()?,
            u32_e: reader.u32()?,
            flag_d: reader.boolean()?,
            u32_f: reader.u32()?,
            u32_g: reader.u32()?,
            flag_e: reader.boolean()?,
            flag_f: reader.boolean()?,
            list_a: read_list(&mut reader, RECAP_LIST_LIMIT)?,
            list_b: read_list(&mut reader, RECAP_LIST_LIMIT)?,
            flag_g: reader.boolean()?,
            list_c: read_list(&mut reader, RECAP_LAST_LIST_LIMIT)?,
            tail: reader.u32()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const TARGET: u64 = (1 << 34) + 5;
    const ATTACKER: u64 = 9;
    const AMOUNT: f32 = 42.0;

    fn items() -> impl Strategy<Value = Vec<DamageItem>> {
        prop::collection::vec(
            (any::<u8>(), -1.0e6_f32..1.0e6, 0.0_f32..2.0)
                .prop_map(|(tag, amount, scale)| DamageItem { tag, amount, scale }),
            0..4,
        )
    }

    fn round<T: BodyCodec + PartialEq + std::fmt::Debug>(message: &T) -> T {
        T::decode(&message.encode().expect("encode")).expect("decode")
    }

    proptest! {
        #[test]
        fn damage_dealt_round_trips(
            heads in any::<(u64, u64, u64, u32, u32)>(),
            raw_16 in any::<[u8; 16]>(),
            raw_2 in any::<[u8; 2]>(),
            items in items(),
        ) {
            let message = DamageDealtMsg {
                head_a: heads.0, head_b: heads.1, raw_16, u64_c: heads.2,
                u32_d: heads.3, u32_e: heads.4, raw_2, items,
            };
            prop_assert_eq!(round(&message), message);
        }

        #[test]
        fn on_damage_round_trips(
            heads in any::<(u64, u64, u64, u32)>(),
            triple in any::<[u32; 3]>(),
            raw_2 in any::<[u8; 2]>(),
            items in items(),
        ) {
            let message = OnDamageMsg {
                head_a: heads.0, head_b: heads.1, u64_c: heads.2, raw_2, u32_d: heads.3,
                triple, items,
            };
            prop_assert_eq!(round(&message), message);
        }

        #[test]
        fn request_respawn_round_trips(
            heads in any::<(u64, u64, bool)>(),
            payload in prop::collection::vec(any::<u8>(), 0..=32),
        ) {
            let message = RequestRespawnMsg { head_a: heads.0, head_b: heads.1, flag: heads.2, payload };
            prop_assert_eq!(round(&message), message);
        }

        #[test]
        fn death_recap_round_trips(
            heads in any::<(u64, u64)>(),
            strings in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..40), 3),
            words in any::<[u32; 9]>(),
            shorts in any::<[u16; 4]>(),
            flags in any::<[bool; 7]>(),
            blocks in any::<([u8; 16], [u8; 16], [u8; 7])>(),
            lists in (
                prop::collection::vec(any::<u32>(), 0..6),
                prop::collection::vec(any::<u32>(), 0..6),
                prop::collection::vec(any::<u32>(), 0..4),
            ),
        ) {
            let message = DeathRecapMsg {
                head_a: heads.0, head_b: heads.1,
                string_a: strings[0].clone(), string_b: strings[1].clone(),
                flag_a: flags[0], u32_a: words[0], word_a: words[1], word_b: words[2],
                string_c: strings[2].clone(), u16_a: shorts[0], block_1: blocks.0,
                u16_b: shorts[1], block_2: blocks.1, raw_7: blocks.2, u16_c: shorts[2],
                flag_b: flags[1], flag_c: flags[2], u32_b: words[3], u32_c: words[4],
                u32_d: words[5], u16_d: shorts[3], u32_e: words[6], flag_d: flags[3],
                u32_f: words[7], u32_g: words[8], flag_e: flags[4], flag_f: flags[5],
                list_a: lists.0, list_b: lists.1, flag_g: flags[6], list_c: lists.2,
                tail: words[0],
            };
            prop_assert_eq!(round(&message), message);
        }
    }

    #[test]
    fn a_2071_with_one_item_is_60_bytes() {
        let body = DamageDealtMsg::hit(TARGET, ATTACKER, AMOUNT)
            .encode()
            .expect("encode");
        assert_eq!(body.len(), 60);
        assert_eq!(body[50], 1, "the item count");
        assert_eq!(body[51], HIT_TAG);
    }

    #[test]
    fn a_3601_head_is_42_bytes() {
        let body = OnDamageMsg::hit(TARGET, ATTACKER, AMOUNT)
            .encode()
            .expect("encode");
        assert_eq!(body.len(), 42 + 1 + 9);
    }

    #[test]
    fn a_minimal_death_recap_round_trips() {
        let recap = DeathRecapMsg::of(TARGET, ATTACKER);
        assert_eq!(round(&recap), recap);
        assert_eq!((recap.head_a, recap.head_b), (TARGET, ATTACKER));
    }

    #[test]
    fn a_respawn_payload_over_32_bytes_is_refused() {
        let message = RequestRespawnMsg {
            head_a: 1,
            head_b: 2,
            flag: false,
            payload: vec![0; 33],
        };
        assert!(message.encode().is_err());
        let mut body = RequestRespawnMsg {
            payload: Vec::new(),
            ..message
        }
        .encode()
        .expect("encode");
        body.truncate(17);
        body.push(33);
        body.extend([0; 33]);
        assert!(RequestRespawnMsg::decode(&body).is_err());
    }

    #[test]
    fn a_count_over_the_reader_limit_is_refused() {
        let mut recap = DeathRecapMsg::of(TARGET, ATTACKER);
        recap.list_a = vec![0; 6];
        assert!(recap.encode().is_err());
        let mut recap = DeathRecapMsg::of(TARGET, ATTACKER);
        recap.list_c = vec![0; 4];
        assert!(recap.encode().is_err());
    }

    #[test]
    fn a_flag_byte_other_than_0_or_1_is_refused() {
        let mut body = RequestRespawnMsg {
            head_a: 1,
            head_b: 2,
            flag: true,
            payload: Vec::new(),
        }
        .encode()
        .expect("encode");
        body[16] = 2;
        assert!(RequestRespawnMsg::decode(&body).is_err());
    }
}
