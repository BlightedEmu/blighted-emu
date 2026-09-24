// SPDX-License-Identifier: AGPL-3.0-or-later
//! The item descriptor (`FUN_145ce9c50`): an item's CRC-32 id, gear score, quantity and perk
//! lists, as a buildable's committed resources, a death recap and the trading post contracts
//! carry it (campfire note; economy note `run15/economy/REPORT.md`, section 2).
//!
//! The client sends a shorter form in its contract completions ([`ShortItemDescriptor`]): the
//! id, the quantity, the gear score and the perks (retail `2212`, 13 calls of 4 captures).

use crate::combat::{RECAP_LAST_LIST_LIMIT, RECAP_LIST_LIMIT, read_list, write_list};
use crate::wire::{MessageError, Reader, Writer};

/// The first unnamed `u32` as the descriptor's constructor sets it (`FUN_1447b8570`:
/// `0xffffffff`, also every retail value of 18,000 captured descriptors).
pub const DESCRIPTOR_DEFAULT_A: u32 = u32::MAX;
/// The second unnamed `u32` as the constructor sets it (`0x33`, also every retail value).
pub const DESCRIPTOR_DEFAULT_B: u32 = 0x33;

/// One item descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDescriptor {
    /// The CRC-32 of the lowercase item id; 0 for no item.
    pub item_crc: u32,
    /// The gear score (retail: 0 on resources, the item's gear score on gear).
    pub gear_score: u16,
    /// The quantity.
    pub quantity: u32,
    /// A flag (name Unknown; retail 0).
    pub flag_a: bool,
    /// A big-endian `u32` (name Unknown; [`DESCRIPTOR_DEFAULT_A`]).
    pub value_a: u32,
    /// A big-endian `u32` (name Unknown; [`DESCRIPTOR_DEFAULT_B`]).
    pub value_b: u32,
    /// A flag (name Unknown; retail 0).
    pub flag_b: bool,
    /// A flag (name Unknown; retail 0).
    pub flag_c: bool,
    /// The perks: the CRC-32 of each lowercase perk id, fewer than 6.
    pub perks: Vec<u32>,
    /// A list of fewer than 6 `u32` (name Unknown; retail empty).
    pub list_b: Vec<u32>,
    /// A flag (name Unknown; retail 0).
    pub flag_d: bool,
    /// A list of fewer than 4 `u32` (name Unknown; retail empty).
    pub list_c: Vec<u32>,
}

impl ItemDescriptor {
    /// `quantity` of the item whose lowercase id has the CRC-32 `item_crc`, every other field as
    /// the constructor and retail have it.
    pub fn of(item_crc: u32, quantity: u32) -> Self {
        Self {
            item_crc,
            gear_score: 0,
            quantity,
            flag_a: false,
            value_a: DESCRIPTOR_DEFAULT_A,
            value_b: DESCRIPTOR_DEFAULT_B,
            flag_b: false,
            flag_c: false,
            perks: Vec::new(),
            list_b: Vec::new(),
            flag_d: false,
            list_c: Vec::new(),
        }
    }

    /// Reads one descriptor.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, a flag is not 0 or 1, or a list is
    /// too long.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            item_crc: reader.u32()?,
            gear_score: reader.u16()?,
            quantity: reader.u32()?,
            flag_a: reader.boolean()?,
            value_a: reader.u32()?,
            value_b: reader.u32()?,
            flag_b: reader.boolean()?,
            flag_c: reader.boolean()?,
            perks: read_list(reader, RECAP_LIST_LIMIT)?,
            list_b: read_list(reader, RECAP_LIST_LIMIT)?,
            flag_d: reader.boolean()?,
            list_c: read_list(reader, RECAP_LAST_LIST_LIMIT)?,
        })
    }

    /// Writes the descriptor.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for a list that is too long.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer
            .u32(self.item_crc)
            .u16(self.gear_score)
            .u32(self.quantity)
            .boolean(self.flag_a)
            .u32(self.value_a)
            .u32(self.value_b)
            .boolean(self.flag_b)
            .boolean(self.flag_c);
        write_list(writer, &self.perks, RECAP_LIST_LIMIT)?;
        write_list(writer, &self.list_b, RECAP_LIST_LIMIT)?;
        writer.boolean(self.flag_d);
        write_list(writer, &self.list_c, RECAP_LAST_LIST_LIMIT)
    }
}

/// The client's short descriptor of a contract completion: id, quantity, gear score, perks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortItemDescriptor {
    /// The CRC-32 of the lowercase item id; 0 for none (a cancel).
    pub item_crc: u32,
    /// The quantity.
    pub quantity: u32,
    /// The gear score.
    pub gear_score: u16,
    /// The perks, fewer than 6.
    pub perks: Vec<u32>,
}

impl ShortItemDescriptor {
    /// Reads one short descriptor.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or the perk list is too long.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            item_crc: reader.u32()?,
            quantity: reader.u32()?,
            gear_score: reader.u16()?,
            perks: read_list(reader, RECAP_LIST_LIMIT)?,
        })
    }

    /// Writes the short descriptor.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for a perk list that is too long.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer
            .u32(self.item_crc)
            .u32(self.quantity)
            .u16(self.gear_score);
        write_list(writer, &self.perks, RECAP_LIST_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::hex_bytes;

    /// The descriptor of a retail sell contract of 100 `FoodManaT3` (retail `buy-sell-TP`, a
    /// 2538 search answer).
    const RESOURCE: &str = "bccc366f00000000006400ffffffff00000033000000000000";
    /// A gear descriptor with five perks (same capture: a crafted T5 amulet at gear score 705).
    const GEAR: &str = "ed409a1e02c10000000100ffffffff00000033000005e0575826d9b24be5cb1867575372d2f12a717b11000000";

    #[test]
    fn retail_descriptors_read_and_write_back() {
        let resource = ItemDescriptor::read(&mut Reader::new(&hex_bytes(RESOURCE))).expect("read");
        assert_eq!(resource, ItemDescriptor::of(0xbccc_366f, 100));
        let mut writer = Writer::new();
        resource.write(&mut writer).expect("write");
        assert_eq!(writer.finish(), hex_bytes(RESOURCE));
        let gear = ItemDescriptor::read(&mut Reader::new(&hex_bytes(GEAR))).expect("read");
        assert_eq!(gear.gear_score, 705);
        assert_eq!(gear.perks.len(), 5);
        let mut writer = Writer::new();
        gear.write(&mut writer).expect("write");
        assert_eq!(writer.finish(), hex_bytes(GEAR));
    }

    /// Retail `tradepost_post_sell_cancel` 373870 ms: a T5 blunderbuss at gear score 625 with
    /// four perks.
    #[test]
    fn a_retail_short_descriptor_reads() {
        let bytes = hex_bytes("f1c564e400000001027104f6bce4bba950a443879145c90e6bd6a0");
        let short = ShortItemDescriptor::read(&mut Reader::new(&bytes)).expect("read");
        assert_eq!(short.item_crc, 0xf1c5_64e4);
        assert_eq!(short.quantity, 1);
        assert_eq!(short.gear_score, 625);
        assert_eq!(short.perks.len(), 4);
        let mut writer = Writer::new();
        short.write(&mut writer).expect("write");
        assert_eq!(writer.finish(), bytes);
    }

    #[test]
    fn a_sixth_perk_is_refused() {
        let mut descriptor = ItemDescriptor::of(1, 1);
        descriptor.perks = vec![0; RECAP_LIST_LIMIT];
        assert!(descriptor.write(&mut Writer::new()).is_err());
    }
}
