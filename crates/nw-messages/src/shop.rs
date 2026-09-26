// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shops of the world (economy note `local-data/scratch-work/run15/economy/REPORT.md`,
//! section 7): faction shops, war and outpost rush camp shops, the stables and the event shops.
//!
//! 3741 `ObjectiveInteractorComponentServerFacet_OnPurchaseItemRequest`: the client buys `count`
//! times one row of `ItemCurrencyConversionData`, named by the CRC-32 of its lowercase
//! `ConversionID` (Observed: all 43 retail calls of 17 sessions are 24 bytes, header + CRC +
//! count, and every CRC is a `ConversionID`; counts 1 to 6). Retail answers with no message of
//! its own: the bag (7048 and the bundle) and the currencies change.

use crate::facet::FacetHeader;
use crate::wire::{MessageError, Reader, Writer};

/// `ObjectiveInteractorComponentServerFacet_OnPurchaseItemRequest`.
pub const PURCHASE_ITEM_TYPE_INDEX: u64 = 3741;

/// A shop purchase (3741).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShopPurchaseCall {
    /// The header (the player's `ObjectiveInteractorComponent`).
    pub header: FacetHeader,
    /// The CRC-32 of the lowercase `ConversionID` of the bought row.
    pub conversion: u32,
    /// How many times the row is bought.
    pub count: u32,
}

impl ShopPurchaseCall {
    /// Decodes `body` when `type_index` is a shop purchase; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        if type_index != PURCHASE_ITEM_TYPE_INDEX {
            return None;
        }
        let mut reader = Reader::new(body);
        let read = |reader: &mut Reader<'_>| -> Result<Self, MessageError> {
            Ok(Self {
                header: FacetHeader::read(reader)?,
                conversion: reader.u32()?,
                count: reader.u32()?,
            })
        };
        Some(read(&mut reader).and_then(|call| {
            reader.finish()?;
            Ok(call)
        }))
    }

    /// The body bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.u32(self.conversion).u32(self.count);
        writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::hex_bytes;

    /// The retail form: header, then the CRC-32 of `awardachievement_mount_horse_2` (the stable's
    /// row bought in `Mount-quest`) and the count 1.
    const PURCHASE: &str = "65cd42195901da562896c8c14210296e87c053c500000001";

    #[test]
    fn a_purchase_reads_and_writes_back() {
        let body = hex_bytes(PURCHASE);
        let call = ShopPurchaseCall::decode(PURCHASE_ITEM_TYPE_INDEX, &body)
            .expect("a purchase")
            .expect("decodes");
        assert_eq!((call.conversion, call.count), (0x87c0_53c5, 1));
        assert_eq!(call.encode(), body);
    }

    #[test]
    fn a_short_or_other_body_is_not_a_purchase() {
        let body = hex_bytes(PURCHASE);
        assert!(
            ShopPurchaseCall::decode(PURCHASE_ITEM_TYPE_INDEX, &body[..20])
                .expect("a purchase")
                .is_err()
        );
        assert!(ShopPurchaseCall::decode(PURCHASE_ITEM_TYPE_INDEX + 1, &body).is_none());
    }
}
