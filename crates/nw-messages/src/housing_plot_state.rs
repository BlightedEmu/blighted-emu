// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::HousingPlotReplicatedState` (class 3055): the houses of one house plot, on the plot
//! entity. Layout from the client's member table: slot 1 holds `enterHousingData`, an array,
//! slot 2 `purchaseHousingData` (8 bytes). The retail bodies (housing states study 2026-11-11,
//! 6 bodies of 5 ledgers; `local-data/findings/2026-11-11-housing-states.md`):
//!
//! - every body sets slot 1 alone, with 20 houses in falling score order; each names a
//!   character id as hyphenated lowercase text (Inferred: the house's owner), the house score,
//!   and the house data entity as its raw id and facet entity (Observed: each named house data
//!   that a ledger shows is a `housedatamaster` card with that `GdeRef`);
//! - it came on the plot entity (component 2) 150 ms after the plot info request (3717) in 4
//!   of 5 ledgers, and 3.9 s after a purchase (3700) in the fifth;
//! - no body sets slot 2: the form of `purchaseHousingData` beyond its 8 bytes is Unknown, so
//!   this codec does not write it.

use crate::slots::write_children;
use crate::travel_states::write_text;
use crate::wire::{MessageError, RAW_ID_LEN, Writer};

/// Type index of `MB::HousingPlotReplicatedState`.
pub const HOUSING_PLOT_TYPE_INDEX: u64 = 3055;
/// The plot entity's component of the state (Observed: all 6 retail bodies).
pub const HOUSING_PLOT_COMPONENT: u64 = 2;
/// Most houses a plot lists (Observed: 20 in all 6 retail bodies).
pub const MOST_PLOT_HOUSES: usize = 20;
/// The slot byte of slot 1 alone.
const SLOT_1: u8 = 1 << 1;
/// The children of slot 1, and the bitmap of `enterHousingData`.
const SLOT_1_CHILDREN: u8 = 1;
const ENTER_HOUSING_DATA: u16 = 1;

/// One house of `enterHousingData`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlotHouse {
    /// The owner's character id, hyphenated lowercase text.
    pub owner: Vec<u8>,
    /// The house score.
    pub score: u32,
    /// The house data entity's raw id.
    pub house_data: [u8; RAW_ID_LEN],
    /// The house data entity's facet entity.
    pub house_data_facet: u64,
}

/// The state of a plot entity. A `None` child is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HousingPlotState {
    /// Slot 1 child 0, `enterHousingData`: the plot's houses, best first.
    pub houses: Option<Vec<PlotHouse>>,
}

impl HousingPlotState {
    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the house count does not fit the prefix code.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let Some(houses) = &self.houses else {
            writer.u8(0);
            return Ok(());
        };
        writer.u8(SLOT_1);
        write_children(writer, ENTER_HOUSING_DATA, SLOT_1_CHILDREN, |writer, _| {
            // A `usize` count fits 64 bits.
            writer.prefixed(houses.len() as u64)?;
            for house in houses {
                write_text(writer, &house.owner);
                writer
                    .u32(house.score)
                    .bytes(&house.house_data)
                    .u64(house.house_data_facet);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made-up owners and house data ids, in the form of retail's.
    const FIRST_OWNER: &str = "11111111-2222-4333-8444-555555555555";
    const SECOND_OWNER: &str = "66666666-7777-4888-9999-aaaaaaaaaaaa";
    const FIRST_HOUSE: [u8; RAW_ID_LEN] = [
        0x3a, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae,
        0xaf,
    ];
    const SECOND_HOUSE: [u8; RAW_ID_LEN] = [
        0x92, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd, 0xbe,
        0xbf,
    ];

    fn facet(raw: &[u8; RAW_ID_LEN]) -> u64 {
        let mut bytes = [0; size_of::<u64>()];
        bytes.copy_from_slice(&raw[..size_of::<u64>()]);
        u64::from_le_bytes(bytes)
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Retail `Hive house plot` 1782815047192: the first two houses (scores 7718 and 6976) of
    /// the plot's list, with made-up owners and house ids, as a list of two.
    #[test]
    fn a_plot_list_writes_the_retail_form() {
        let state = HousingPlotState {
            houses: Some(vec![
                PlotHouse {
                    owner: FIRST_OWNER.as_bytes().to_vec(),
                    score: 7718,
                    house_data: FIRST_HOUSE,
                    house_data_facet: facet(&FIRST_HOUSE),
                },
                PlotHouse {
                    owner: SECOND_OWNER.as_bytes().to_vec(),
                    score: 6976,
                    house_data: SECOND_HOUSE,
                    house_data_facet: facet(&SECOND_HOUSE),
                },
            ]),
        };
        let mut writer = Writer::new();
        state.write(&mut writer).expect("the state writes");
        let expected = [
            "020102",
            "24",
            &hex(FIRST_OWNER.as_bytes()),
            "00001e26",
            &hex(&FIRST_HOUSE),
            "a7a6a5a4a3a2a13a",
            "24",
            &hex(SECOND_OWNER.as_bytes()),
            "00001b40",
            &hex(&SECOND_HOUSE),
            "b7b6b5b4b3b2b192",
        ]
        .concat();
        assert_eq!(hex(&writer.finish()), expected);
        let mut none = Writer::new();
        HousingPlotState::default()
            .write(&mut none)
            .expect("the state writes");
        assert_eq!(none.finish(), [0], "no child: no slot");
    }
}
