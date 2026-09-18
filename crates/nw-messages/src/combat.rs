// SPDX-License-Identifier: AGPL-3.0-or-later
//! The combat messages (damage note sections 1 and 2, death note sections 2 and 3). Every field
//! is kept, by position, with a neutral name where its meaning is Unknown, so a captured body
//! encodes back to the same bytes. Counts use the wide prefix (`FUN_14087b5c0`); strings use the
//! prefix-coded length of `FUN_14087a3d0`, as in the client input.

use crate::facet::{FacetHeader, component};
use crate::wire::{BodyCodec, MessageError, Reader, Writer, raw_id_of_entity};

/// `DamageReceiverComponentClientFacet_OnDamageDealt`.
pub const DAMAGE_DEALT_TYPE_INDEX: u64 = 2071;
/// `VitalsComponentClientFacet_OnDamage`.
pub const ON_DAMAGE_TYPE_INDEX: u64 = 3601;
/// `DamageReceiverComponentClientFacet_HandleMeleeDamage`.
pub const MELEE_HIT_TYPE_INDEX: u64 = 2218;
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
/// The hit volume of a hit: the lowercase CRC-32 of `torso` (mob-cards note, section 4.3).
const HIT_VOLUME_TORSO: u32 = 0x275f_bc85;
/// The camera shake of a hit: the lowercase CRC-32 of `small`.
const CAMERA_SHAKE_SMALL: u32 = 0x7545_ea13;
/// Hit flags (little-endian `u16`): the hit can crit.
const HIT_FLAG_CAN_CRIT: u16 = 0x0020;
/// Hit flags: the hit killed the target.
const HIT_FLAG_KILLED: u16 = 0x0008;
/// Hit flags: the target absorbed the hit (retail Thorpe intermission: `0x0420`).
const HIT_FLAG_ABSORBED: u16 = 0x0400;
/// The camera shake of an absorbed hit (retail Thorpe intermission; name Unknown).
const CAMERA_SHAKE_ABSORBED: u32 = 0x664b_2673;
/// The item tag of an absorbed hit (retail Thorpe intermission; meaning Unknown).
const ABSORBED_TAG: u8 = 6;
/// Hit flags: a pellet of a spread shot (ranged note: `0x21` on every pellet 2071).
const HIT_FLAG_PELLET: u16 = 0x0001;
/// Hit flags: the hit was a crit (ranged note: `0x61` on crit pellets).
const HIT_FLAG_CRIT: u16 = 0x0040;
/// The item tag of a pellet (ranged note: tag 5 on most pellet hits).
const PELLET_TAG: u8 = 5;
/// The camera shake of a pellet: none.
const NO_CAMERA_SHAKE: u32 = 0;
/// Flags (little-endian `u16`) of every retail 3601 of a creature's melee hit (swing note:
/// `02 00` on all tutorial hits; the Azoth catalog names it a direct hit).
const MELEE_HIT_FLAGS: u16 = 0x0002;

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

/// Type 2218: a creature's melee hit reached the player (live-tutorial note, section 7). Retail
/// sends `blocked` 0 with every hit that did damage (in the same ms as its 3601) and 1, with no
/// 3601, for a hit while the player held block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeleeHitMsg {
    /// The header: the player's damage receiver.
    pub header: FacetHeader,
    /// The player blocked the hit.
    pub blocked: bool,
}

impl MeleeHitMsg {
    /// A melee hit on player `player`.
    pub fn on(player: u64, blocked: bool) -> Self {
        Self {
            header: FacetHeader::of(component::DAMAGE_RECEIVER, player),
            blocked,
        }
    }
}

impl BodyCodec for MeleeHitMsg {
    const TYPE_INDEX: u64 = MELEE_HIT_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        Ok(writer.u8(u8::from(self.blocked)).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
            blocked: reader.boolean()?,
        };
        reader.finish()?;
        Ok(message)
    }
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
    /// A hit of `amount` by player `attacker` on entity `target`, `killed` when it killed it: a
    /// call to the attacker's damage receiver that names the target by its spawn-card reference
    /// and facet entity, with a torso hit and a small camera shake (mob-cards note, section 4.3).
    pub fn hit(attacker: u64, target: u64, amount: f32, killed: bool) -> Self {
        let header = FacetHeader::of(component::DAMAGE_RECEIVER, attacker);
        let flags = if killed {
            HIT_FLAG_CAN_CRIT | HIT_FLAG_KILLED
        } else {
            HIT_FLAG_CAN_CRIT
        };
        Self {
            head_a: header.target,
            head_b: header.entity,
            raw_16: raw_id_of_entity(target),
            // The facet entity of the target.
            u64_c: target.swap_bytes(),
            u32_d: HIT_VOLUME_TORSO,
            u32_e: CAMERA_SHAKE_SMALL,
            raw_2: flags.to_le_bytes(),
            items: one_hit(amount),
        }
    }
}

impl DamageDealtMsg {
    /// One pellet of a spread shot by player `attacker` on `target`: `amount` after the armor,
    /// which let `scale` of the raw damage through, a `crit` or not, `killed` when it killed the
    /// target (ranged note: no camera shake, flags `0x21`, `0x40` on a crit, `0x08` on the kill).
    pub fn pellet(
        attacker: u64,
        target: u64,
        amount: f32,
        scale: f32,
        crit: bool,
        killed: bool,
    ) -> Self {
        let mut flags = HIT_FLAG_CAN_CRIT | HIT_FLAG_PELLET;
        if crit {
            flags |= HIT_FLAG_CRIT;
        }
        if killed {
            flags |= HIT_FLAG_KILLED;
        }
        Self {
            u32_e: NO_CAMERA_SHAKE,
            raw_2: flags.to_le_bytes(),
            items: vec![DamageItem {
                tag: PELLET_TAG,
                amount,
                scale,
            }],
            ..Self::hit(attacker, target, amount, false)
        }
    }
}

impl DamageDealtMsg {
    /// A hit by player `attacker` that target `target` absorbed: no damage (retail Thorpe in his
    /// intermission, 494210 to 499506 ms).
    pub fn absorbed(attacker: u64, target: u64) -> Self {
        Self {
            u32_e: CAMERA_SHAKE_ABSORBED,
            raw_2: (HIT_FLAG_CAN_CRIT | HIT_FLAG_ABSORBED).to_le_bytes(),
            items: vec![DamageItem {
                tag: ABSORBED_TAG,
                amount: 0.0,
                scale: 0.0,
            }],
            ..Self::hit(attacker, target, 0.0, false)
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

/// A creature's melee hit on a player, as retail sends it in its 3601 (swing note, retail
/// tutorial 262336: `… f124cd79ce631910 0200 3cb103b6 441b5771 43d1fa3d 41a2fb39 01 05 4195a15e
/// 3f0fa8d4`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeleeDamage {
    /// The entity number of the creature.
    pub attacker: u64,
    /// The damage.
    pub amount: f32,
    /// The player's max health.
    pub max_health: f32,
    /// Where the creature stood, x, y and z in metres.
    pub attacker_at: [f32; 3],
    /// The item tag: sailor 6, signifier 5, risen 6, swarmer 4, Thorpe 4 (meaning Unknown).
    pub tag: u8,
    /// The item scale: the share of the raw damage the player's armor let through (retail 0.6006
    /// for sailors, 0.5612 for the others).
    pub scale: f32,
    /// A second, fire part: its item tag and amount (retail Thorpe in his second phase: tag 9).
    pub fire: Option<(u8, f32)>,
}

impl OnDamageMsg {
    /// The creature hit `damage` on player `target`: a call to its vitals component that names
    /// the creature's facet entity, the direct-hit flags, the share of the max health lost by
    /// all parts, the creature's position and one item per part.
    pub fn melee(target: u64, damage: &MeleeDamage) -> Self {
        let header = FacetHeader::of_child(component::VITALS, component::STATS_CHILD, target);
        let items: Vec<DamageItem> = std::iter::once((damage.tag, damage.amount))
            .chain(damage.fire)
            .map(|(tag, amount)| DamageItem {
                tag,
                amount,
                scale: damage.scale,
            })
            .collect();
        let total: f32 = items.iter().map(|item| item.amount).sum();
        Self {
            head_a: header.target,
            head_b: header.entity,
            u64_c: damage.attacker.swap_bytes(),
            raw_2: MELEE_HIT_FLAGS.to_le_bytes(),
            u32_d: (total / damage.max_health).to_bits(),
            triple: damage.attacker_at.map(f32::to_bits),
            items,
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
    /// A recap for `player`: a call to its vitals component, empty strings, every other field 0
    /// (M6-B spec section 8).
    pub fn of(player: u64) -> Self {
        let header = FacetHeader::of_child(component::VITALS, component::STATS_CHILD, player);
        Self {
            head_a: header.target,
            head_b: header.entity,
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

    #[test]
    fn an_absorbed_hit_is_the_retail_body() {
        // Retail Thorpe intermission (494210 of the tutorial capture): torso, camera shake
        // `664b2673`, flags `2004`, one item `06 00000000 00000000`.
        const RETAIL_TAIL: [u8; 20] = [
            0x27, 0x5f, 0xbc, 0x85, 0x66, 0x4b, 0x26, 0x73, 0x20, 0x04, 0x01, 0x06, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let body = DamageDealtMsg::absorbed(7, (1 << 34) + 3)
            .encode()
            .expect("encode");
        assert!(body.ends_with(&RETAIL_TAIL), "{body:02x?}");
    }

    const TARGET: u64 = (1 << 34) + 5;
    const ATTACKER: u64 = 9;
    const AMOUNT: f32 = 42.0;

    #[test]
    fn a_blocked_melee_hit_has_the_retail_bytes() {
        // Retail 2218 of a blocked hit (live-tutorial note, section 7): the player's damage
        // receiver, then 1. The facet entity is the reversed player number.
        const RETAIL: [u8; 17] = [
            0x8c, 0x6d, 0xee, 0xd1, 0x7d, 0xda, 0x2a, 0x1b, 0xbf, 0xa9, 0x6f, 0x3e, 0xa2, 0xe3,
            0x6a, 0x5a, 0x01,
        ];
        const PLAYER: u64 = 0x5a6a_e3a2_3e6f_a9bf;
        let blocked = MeleeHitMsg::on(PLAYER, true);
        assert_eq!(blocked.encode().expect("encode"), RETAIL);
        assert_eq!(MeleeHitMsg::decode(&RETAIL), Ok(blocked));
        let landed = MeleeHitMsg::on(PLAYER, false).encode().expect("encode");
        assert_eq!(landed[..16], RETAIL[..16]);
        assert_eq!(landed[16], 0);
    }

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
        let body = DamageDealtMsg::hit(ATTACKER, TARGET, AMOUNT, false)
            .encode()
            .expect("encode");
        assert_eq!(body.len(), 60);
        assert_eq!(body[50], 1, "the item count");
        assert_eq!(body[51], HIT_TAG);
    }

    #[test]
    fn a_2071_calls_the_attackers_damage_receiver_with_the_retail_fields() {
        // The Soul Survivor player and first sailor (mob-cards note, section 4.3).
        const SOUL_PLAYER: u64 = 0x650f_7a09_cf50_b550;
        const SOUL_SAILOR: u64 = 0xa0d5_6dec_a068_8d10;
        const RETAIL_TARGET: u64 = 0x31d3_f99c_aa47_7511;
        let hit = DamageDealtMsg::hit(
            SOUL_PLAYER.swap_bytes(),
            SOUL_SAILOR.swap_bytes(),
            AMOUNT,
            false,
        );
        assert_eq!((hit.head_a, hit.head_b), (RETAIL_TARGET, SOUL_PLAYER));
        assert_eq!(hit.u64_c, SOUL_SAILOR);
        assert_eq!(
            (hit.u32_d, hit.u32_e, hit.raw_2),
            (0x275f_bc85, 0x7545_ea13, [0x20, 0x00])
        );
        let killing = DamageDealtMsg::hit(ATTACKER, TARGET, AMOUNT, true);
        assert_eq!(killing.raw_2, [0x28, 0x00]);
    }

    #[test]
    fn a_pellet_has_the_retail_flags() {
        let pellet = DamageDealtMsg::pellet(ATTACKER, TARGET, AMOUNT, 0.8, false, false);
        assert_eq!((pellet.raw_2, pellet.u32_e), ([0x21, 0x00], 0));
        assert_eq!(pellet.items[0].tag, 5);
        let crit_kill = DamageDealtMsg::pellet(ATTACKER, TARGET, AMOUNT, 0.8, true, true);
        assert_eq!(crit_kill.raw_2, [0x69, 0x00]);
    }

    #[test]
    fn a_melee_hit_has_the_retail_fields() {
        // Retail 262336: the signifier hit the player for 18.70.
        const SIGNIFIER_FACET: u64 = 0xf124_cd79_ce63_1910;
        const RETAIL_TAIL: [u8; 34] = [
            0xf1, 0x24, 0xcd, 0x79, 0xce, 0x63, 0x19, 0x10, 0x02, 0x00, 0x3c, 0xb1, 0x03, 0xb6,
            0x44, 0x1b, 0x57, 0x71, 0x43, 0xd1, 0xfa, 0x3d, 0x41, 0xa2, 0xfb, 0x39, 0x01, 0x05,
            0x41, 0x95, 0xa1, 0x5e, 0x3f, 0x0f,
        ];
        let damage = MeleeDamage {
            attacker: SIGNIFIER_FACET.swap_bytes(),
            amount: f32::from_bits(0x4195_a15e),
            // The retail player's max health, 865.587.
            max_health: f32::from_bits(0x4458_6593),
            attacker_at: [0x441b_5771, 0x43d1_fa3d, 0x41a2_fb39].map(f32::from_bits),
            tag: 5,
            scale: f32::from_bits(0x3f0f_a8d4),
            fire: None,
        };
        let body = OnDamageMsg::melee(TARGET, &damage)
            .encode()
            .expect("encode");
        assert_eq!(body.len(), 42 + 1 + 9);
        assert_eq!(body[16..16 + RETAIL_TAIL.len()], RETAIL_TAIL);
    }

    #[test]
    fn a_second_phase_hit_carries_its_fire_part() {
        // Retail 513963: Thorpe's flurry, 21.13 slash and 7.043 fire.
        const RETAIL_TAIL: [u8; 37] = [
            0x02, 0x00, 0x3d, 0x05, 0x50, 0xdc, 0x43, 0xf0, 0x1c, 0x25, 0x43, 0x6a, 0x2e, 0x5c,
            0x41, 0xba, 0x14, 0x01, 0x02, 0x04, 0x41, 0xa9, 0x09, 0xaf, 0x3f, 0x0f, 0xa8, 0xd4,
            0x09, 0x40, 0xe1, 0x62, 0x3f, 0x3f, 0x0f, 0xa8, 0xd4,
        ];
        let damage = MeleeDamage {
            attacker: 0x096a_42da_e6b1_8c60_u64.swap_bytes(),
            amount: f32::from_bits(0x41a9_09af),
            max_health: f32::from_bits(0x4458_6593),
            attacker_at: [0x43f0_1c25, 0x436a_2e5c, 0x41ba_1401].map(f32::from_bits),
            tag: 4,
            scale: f32::from_bits(0x3f0f_a8d4),
            fire: Some((9, f32::from_bits(0x40e1_623f))),
        };
        let body = OnDamageMsg::melee(TARGET, &damage)
            .encode()
            .expect("encode");
        assert_eq!(body[24..], RETAIL_TAIL);
    }

    #[test]
    fn a_minimal_death_recap_round_trips() {
        let recap = DeathRecapMsg::of(TARGET);
        assert_eq!(round(&recap), recap);
        assert_eq!(recap.head_b, TARGET.swap_bytes());
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
        let mut recap = DeathRecapMsg::of(TARGET);
        recap.list_a = vec![0; 6];
        assert!(recap.encode().is_err());
        let mut recap = DeathRecapMsg::of(TARGET);
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
