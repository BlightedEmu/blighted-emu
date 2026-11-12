// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Javelin::HouseDataReplicatedState` (class 3663): the furniture of one house, on the house
//! data entity (slice `slices/housing/housedatamaster.dynamicslice`). Layout from the client's
//! member table: one child, `housingItems`, a replicated list. The retail bodies (housing states
//! study 2026-11-11, 10,064 bodies of 412 ledgers; `local-data/findings/2026-11-11-housing-states.md`):
//!
//! - an element is the item's position in the house frame in centimetres (three `i16`; Observed:
//!   the 3277 metres times 100, rounded, in every placement of the `furniture` and
//!   `20261010-125035-658` ledgers), its rotation as a packed quaternion ([`PackedRotation`]),
//!   the client item index of the 3277 and a use flag (Observed: 1 after a 3730, 0 after the
//!   695 at `20261010-125035-658` 1791629479751 and 1791629486849);
//! - the list index is the item index the 3277 answer gives and the 3723 and 1435 name; an
//!   item picked up leaves its index as [`HousingItemEntry::EMPTY`] (Observed: `furniture`
//!   1782304232412, a list of 26 after the 1435 of index 25);
//! - the first state of a house holds the whole list; a change sends the changed indexes, and a
//!   new item its index twice, first [`HousingItemEntry::EMPTY`] and then the item (Observed:
//!   the six 3277 answers of `20261010-125035-658`, 1791629657843 to 1791629713142).
//!
//! The sim writes this state; the captures' own states read through the member table.

use crate::list_delta::ReplicatedList;
use crate::slots::{SLOT_0, write_children};
use crate::wire::{MessageError, Writer};

/// Type index of `Javelin::HouseDataReplicatedState`.
pub const HOUSE_DATA_TYPE_INDEX: u64 = 3663;
/// The children of slot 0.
const CHILDREN: u8 = 1;
/// The child bitmap of `housingItems`.
const HOUSING_ITEMS: u16 = 1;
/// Centimetres per metre of an item position (Observed: see the module doc).
const CM_PER_M: f32 = 100.0;

/// The quaternion parts of a packed rotation.
const PARTS: usize = 4;
/// Most bytes of a packed rotation after its flag.
const MOST_BYTES: usize = PARTS - 1;
/// Flag bit 0: the largest part is negative (`FUN_14087aa70`).
const NEGATIVE_LARGEST: u8 = 0x01;
/// Flag bit `1 + n`: part `n` is zero and has no byte.
const ZERO_PART_SHIFT: u8 = 1;
/// Flag bits 5 and 6: the index of the largest part, which has no byte either.
const LARGEST_SHIFT: u8 = 5;
const LARGEST_MASK: u8 = 0x03;
/// A part byte `b` is `b * PART_STEP - PART_OFFSET` (the client's constants `DAT_147f516c0`,
/// `0x3bb5baae`, and `DAT_147f516e8`, `0x3f3504f3`: a byte spans plus and minus 1/sqrt 2).
const PART_STEP: f32 = std::f32::consts::SQRT_2 / 255.0;
const PART_OFFSET: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// The flag of the rotation of an empty index (Observed: `5f` with no byte in all six empty
/// elements of `20261010-125035-658`; the June `furniture` ledger had `57`).
const EMPTY_FLAG: u8 = 0x5f;

/// A quaternion in the client's packed form (`FUN_14087aa70`): a flag byte, then one byte for
/// each part that is neither the largest nor zero. The client gets the largest part back as
/// `sqrt(1 - the others squared)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedRotation {
    /// The flag byte.
    pub flag: u8,
    /// The part bytes, in part order; only the first [`PackedRotation::len`] are sent.
    pub parts: [u8; MOST_BYTES],
}

impl PackedRotation {
    /// The rotation of an empty index.
    pub const EMPTY: Self = Self {
        flag: EMPTY_FLAG,
        parts: [0; MOST_BYTES],
    };

    /// The packed form of the unit quaternion `rotation` (x, y, z, w): the inverse of the
    /// client's reader, each part byte rounded (Inferred: 2 of the 6 retail placements of
    /// `20261010-125035-658` give the same bytes; in the others retail's byte is 1 lower, the
    /// server's own rounding is Unknown).
    pub fn of_quaternion(rotation: [f32; PARTS]) -> Self {
        let largest = (0..PARTS).fold(0, |best, part| {
            if rotation[part].abs() > rotation[best].abs() {
                part
            } else {
                best
            }
        });
        let mut flag = (largest as u8 & LARGEST_MASK) << LARGEST_SHIFT;
        if rotation[largest].is_sign_negative() {
            flag |= NEGATIVE_LARGEST;
        }
        let mut parts = [0; MOST_BYTES];
        let mut len = 0;
        for (part, value) in rotation.iter().enumerate() {
            if part == largest {
                continue;
            }
            if *value == 0.0 {
                flag |= 1 << (ZERO_PART_SHIFT + part as u8);
                continue;
            }
            // The value is within 0 and 255 after the clamp.
            parts[len] = ((value + PART_OFFSET) / PART_STEP)
                .round()
                .clamp(0.0, f32::from(u8::MAX)) as u8;
            len += 1;
        }
        Self { flag, parts }
    }

    /// How many part bytes follow the flag (the client reader's count).
    pub fn len(&self) -> usize {
        (0..PARTS)
            .filter(|&part| {
                part != usize::from(self.flag >> LARGEST_SHIFT & LARGEST_MASK)
                    && self.flag & 1 << (ZERO_PART_SHIFT + part as u8) == 0
            })
            .count()
    }

    /// Whether no part byte follows.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn write(&self, writer: &mut Writer) {
        writer.u8(self.flag).bytes(&self.parts[..self.len()]);
    }
}

/// One element of `housingItems`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HousingItemEntry {
    /// The position in the house frame, centimetres.
    pub position_cm: [i16; 3],
    /// The rotation.
    pub rotation: PackedRotation,
    /// The client item index of the placed item (0 for an empty index).
    pub item: u32,
    /// Whether a player uses the item (sits on it, plays it).
    pub in_use: bool,
}

impl HousingItemEntry {
    /// An index whose item was picked up.
    pub const EMPTY: Self = Self {
        position_cm: [0; 3],
        rotation: PackedRotation::EMPTY,
        item: 0,
        in_use: false,
    };

    /// The element of `item` placed at `position` (metres) with `rotation` (x, y, z, w).
    pub fn placed(position: [f32; 3], rotation: [f32; PARTS], item: u32) -> Self {
        Self {
            // A house is far smaller than 327 m: the value fits after the clamp.
            position_cm: position.map(|metres| {
                (metres * CM_PER_M)
                    .round()
                    .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
            }),
            rotation: PackedRotation::of_quaternion(rotation),
            item,
            in_use: false,
        }
    }

    fn write(&self, writer: &mut Writer) {
        for part in self.position_cm {
            writer.bytes(&part.to_be_bytes());
        }
        self.rotation.write(writer);
        writer.u32(self.item).boolean(self.in_use);
    }
}

/// The state of a house data entity. A `None` child is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HouseDataState {
    /// Child 0, `housingItems`.
    pub items: Option<ReplicatedList<HousingItemEntry>>,
}

impl HouseDataState {
    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the list does not write.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let Some(items) = &self.items else {
            writer.u8(0);
            return Ok(());
        };
        writer.u8(SLOT_0);
        write_children(writer, HOUSING_ITEMS, CHILDREN, |writer, _| {
            items.write(writer, |writer, entry| {
                entry.write(writer);
                Ok(())
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::FullList;
    use crate::list_delta::ListChange;

    /// Made-up client item indexes.
    const CHAIR: u32 = 0x0000_b5f2;
    const GONG: u32 = 0x0000_b622;
    const BENCH: u32 = 0x0000_b623;

    fn hex(state: &HouseDataState) -> String {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("the state writes");
        writer
            .finish()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn quaternion(bits: [u32; PARTS]) -> [f32; PARTS] {
        bits.map(f32::from_bits)
    }

    /// Retail `20261010-125035-658` 1791629657843: the 3277 at 1791629657809 placed its item at
    /// `3ff55000 bef60000 407fb000` turned `bebc3909 3f6e2f58` about z; the answer's change
    /// sends index 0x49 empty, then the item.
    #[test]
    fn a_placed_item_writes_the_retail_change() {
        let placed = HousingItemEntry::placed(
            [0x3ff5_5000, 0xbef6_0000, 0x407f_b000].map(f32::from_bits),
            quaternion([0, 0, 0xbebc_3909, 0x3f6e_2f58]),
            GONG,
        );
        let state = HouseDataState {
            items: Some(ReplicatedList::Changes(vec![
                ListChange {
                    index: 0x49,
                    version: 0x11,
                    value: HousingItemEntry::EMPTY,
                },
                ListChange {
                    index: 0x49,
                    version: 0,
                    value: placed,
                },
            ])),
        };
        assert_eq!(
            hex(&state),
            "010102034901110000000000005f000000000049010000c0ffd00190663d0000b62200"
        );
    }

    /// Retail `20261010-125035-658` 1791629674741: the 3277 at 1791629674709 (`be844000
    /// 3f128000 407e6180`, turned `bed0c605 3f69dd4f`) gives the turn byte `36`.
    #[test]
    fn a_turn_about_z_packs_as_retail() {
        let placed = HousingItemEntry::placed(
            [0xbe84_4000, 0x3f12_8000, 0x407e_6180].map(f32::from_bits),
            quaternion([0, 0, 0xbed0_c605, 0x3f69_dd4f]),
            BENCH,
        );
        assert_eq!(placed.position_cm, [-26, 57, 397]);
        assert_eq!(
            placed.rotation,
            PackedRotation {
                flag: 0x66,
                parts: [0x36, 0, 0],
            }
        );
        assert_eq!(placed.rotation.len(), 1);
    }

    /// Retail `20261010-125035-658` 1791629479751: the 3730 of index 6 sets its use flag.
    #[test]
    fn the_use_flag_writes_the_retail_change() {
        let chair = HousingItemEntry {
            position_cm: [197, -769, 431],
            rotation: PackedRotation {
                flag: 0x46,
                parts: [0xfc, 0, 0],
            },
            item: CHAIR,
            in_use: true,
        };
        let state = HouseDataState {
            items: Some(ReplicatedList::Changes(vec![ListChange {
                index: 6,
                version: 0x0e,
                value: chair,
            }])),
        };
        assert_eq!(hex(&state), "0101010106010e00c5fcff01af46fc0000b5f201");
    }

    /// Retail `Hive house plot` 1782815151328: the house data of a house just bought.
    #[test]
    fn an_empty_house_writes_the_retail_first_state() {
        let state = HouseDataState {
            items: Some(ReplicatedList::Full(FullList {
                version: Some(2),
                elements: Vec::new(),
            })),
        };
        assert_eq!(hex(&state), "010100010200");
        assert_eq!(hex(&HouseDataState::default()), "00", "no child: no slot");
    }

    /// The client reader gets every part back within half a byte step.
    #[test]
    fn a_packed_rotation_reads_back_within_half_a_step() {
        let rotation = [-0.660_0_f32, -0.253_5, 0.253_5, 0.660_1];
        let norm = rotation.iter().map(|part| part * part).sum::<f32>().sqrt();
        let unit = rotation.map(|part| part / norm);
        let packed = PackedRotation::of_quaternion(unit);
        assert_eq!(
            packed.flag >> LARGEST_SHIFT & LARGEST_MASK,
            3,
            "w is the largest"
        );
        assert_eq!(packed.len(), 3);
        for (byte, part) in packed.parts.iter().zip(&unit[..3]) {
            let read = f32::from(*byte) * PART_STEP - PART_OFFSET;
            assert!((read - part).abs() <= PART_STEP / 2.0, "{read} {part}");
        }
        assert!(PackedRotation::EMPTY.is_empty());
    }
}
