// SPDX-License-Identifier: AGPL-3.0-or-later
//! The answers to a storage search (run-15 items2 study): the results (7046) and the item the
//! results name ([`StorageItemSimple`], also the element of the 7048 change list,
//! [`crate::StorageSearchItemsMsg`]).
//!
//! Retail MSQ16 (37 sessions with 7044 and 7046): a new search (7044) is answered by 7046 about
//! 40 ms later with every item of the player's storages that a recipe of the station takes and
//! its total over all storages; the order of the list is not fixed (two orders alternate).

use crate::facet::{FacetHeader, component};
use crate::wire::{BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `GlobalStorageComponentClientFacet_ReceiveStorageSearchResults`.
pub const RECEIVE_STORAGE_SEARCH_RESULTS_TYPE_INDEX: u64 = 7046;
/// Type index of `Javelin::StorageItemSimple`, the class of each listed item (retail prefix
/// bytes `89 6e`).
pub const STORAGE_ITEM_SIMPLE_TYPE_INDEX: u64 = 7049;
/// Type index of `GlobalStorageComponentClientFacet_OnStorageSearchResponseReady`.
pub const STORAGE_SEARCH_RESPONSE_READY_TYPE_INDEX: u64 = 7047;
/// The last byte of every retail 7047 (the answer is ready).
pub const SEARCH_READY: u8 = 1;
/// The byte before a listed item's class: an item is present.
const ITEM_PRESENT: u8 = 1;
/// Bytes of one listed item: the present byte, the 2-byte class prefix, the item and the total.
const ITEM_BYTES: usize = 11;
/// Most items of a list this crate reads.
const MAX_ITEMS: u64 = 0x1_0000;
const FIELD: &str = "storage search";

/// The header of the player's `GlobalStorageComponent` for the player of entity number
/// `entity_number` (the header of 7044, 7039, 7046, 7047 and 7048).
pub fn global_storage_header(entity_number: u64) -> FacetHeader {
    FacetHeader::of_child(
        component::GLOBAL_STORAGE,
        component::GLOBAL_STORAGE_CHILD,
        entity_number,
    )
}

/// One item a search found: the CRC-32 of its lowercase item id and its total over the
/// player's storages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageItemSimple {
    /// CRC-32 of the lowercase `ItemID`.
    pub item: u32,
    /// The total quantity.
    pub quantity: u32,
}

impl StorageItemSimple {
    /// Writes the item with its class prefix.
    ///
    /// # Errors
    ///
    /// Never for the class of this crate; the prefix code's error otherwise.
    pub(crate) fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer
            .u8(ITEM_PRESENT)
            .prefixed(STORAGE_ITEM_SIMPLE_TYPE_INDEX)?
            .u32(self.item)
            .u32(self.quantity);
        Ok(())
    }

    /// Reads one item with its class prefix.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Unsized`] for another class or a missing item.
    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let present = reader.u8()?;
        let class = reader.prefixed()?;
        if present != ITEM_PRESENT || class != STORAGE_ITEM_SIMPLE_TYPE_INDEX {
            return Err(MessageError::Unsized {
                field: FIELD,
                byte: present,
            });
        }
        Ok(Self {
            item: reader.u32()?,
            quantity: reader.u32()?,
        })
    }
}

/// A count of list entries that the bytes left can hold.
pub(crate) fn read_count(reader: &mut Reader<'_>, entry_bytes: usize) -> Result<u64, MessageError> {
    let count = reader.wide_prefixed()?;
    // `usize` fits 64 bits.
    let fits = (reader.remaining() / entry_bytes) as u64;
    if count > MAX_ITEMS || count > fits {
        return Err(MessageError::LengthMismatch { field: FIELD });
    }
    Ok(count)
}

/// 7046: the results of search `search`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageSearchResultsMsg {
    /// The header (the player's `GlobalStorageComponent`).
    pub header: FacetHeader,
    /// The search id of the 7044 it answers.
    pub search: [u8; RAW_ID_LEN],
    /// The items found.
    pub items: Vec<StorageItemSimple>,
}

impl BodyCodec for StorageSearchResultsMsg {
    const TYPE_INDEX: u64 = RECEIVE_STORAGE_SEARCH_RESULTS_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        // A `usize` count fits 64 bits.
        writer
            .bytes(&self.search)
            .wide_prefixed(self.items.len() as u64);
        for item in &self.items {
            item.write(&mut writer)?;
        }
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let search = reader.raw_id()?;
        let count = read_count(&mut reader, ITEM_BYTES)?;
        let items = (0..count)
            .map(|_| StorageItemSimple::read(&mut reader))
            .collect::<Result<_, MessageError>>()?;
        reader.finish()?;
        Ok(Self {
            header,
            search,
            items,
        })
    }
}

/// 7047: the answer to search `search` is ready (retail: the search id of the 7044/7046 it
/// belongs to, then [`SEARCH_READY`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageSearchReadyMsg {
    /// The header (the player's `GlobalStorageComponent`).
    pub header: FacetHeader,
    /// The search id.
    pub search: [u8; RAW_ID_LEN],
    /// Whether the answer is ready (1 in every retail body).
    pub ready: u8,
}

impl BodyCodec for StorageSearchReadyMsg {
    const TYPE_INDEX: u64 = STORAGE_SEARCH_RESPONSE_READY_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.bytes(&self.search).u8(self.ready);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
            search: reader.raw_id()?,
            ready: reader.u8()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// Retail MSQ16: the empty answer of a search (33 bytes).
    const RETAIL_EMPTY: &str = concat!(
        "fd34bbfc0f73dcfe5ad5e1c8869e39e2",
        "f37807994a434749b0c191e321899bd8",
        "00"
    );
    /// Retail MSQ16: the first two items of an answer of 11 (the count set to 2).
    const RETAIL_TWO: &str = concat!(
        "fd34bbfc0f73dcfe5ad5e1c8869e39e2",
        "1b86c5de7622407a92ee6430968861df",
        "02",
        "01896e148f161c00000006",
        "01896e2997d99400000001"
    );

    #[test]
    fn the_results_are_the_retail_bodies() {
        let empty = hex(RETAIL_EMPTY);
        let decoded = StorageSearchResultsMsg::decode(&empty).expect("empty");
        assert!(decoded.items.is_empty());
        assert_eq!(decoded.encode(), Ok(empty));
        let two = hex(RETAIL_TWO);
        let decoded = StorageSearchResultsMsg::decode(&two).expect("two");
        assert_eq!(
            decoded.items,
            [
                StorageItemSimple {
                    item: 0x148f_161c,
                    quantity: 6,
                },
                StorageItemSimple {
                    item: 0x2997_d994,
                    quantity: 1,
                },
            ]
        );
        assert_eq!(decoded.encode(), Ok(two));
        let number = crate::entity_ids::number_of_facet(decoded.header.entity);
        assert_eq!(decoded.header, global_storage_header(number));
    }

    /// Retail `cooking_mount_vittles`: a search answer is ready.
    const RETAIL_READY: &str = "961a3817b4f9a64ec3cf6424ec03f33248717e4f7c504782b52934917f28b12d01";

    #[test]
    fn the_ready_note_reads_and_writes_back() {
        let ready = StorageSearchReadyMsg::decode(&hex(RETAIL_READY)).expect("decodes");
        assert_eq!(ready.ready, SEARCH_READY);
        assert_eq!(ready.encode().expect("encodes"), hex(RETAIL_READY));
    }

    #[test]
    fn a_wrong_class_or_count_is_refused() {
        let mut wrong = hex(RETAIL_TWO);
        // The first item's class prefix.
        wrong[34] = 0x8a;
        assert!(StorageSearchResultsMsg::decode(&wrong).is_err());
        let mut long = hex(RETAIL_TWO);
        long[32] = 3;
        assert!(StorageSearchResultsMsg::decode(&long).is_err());
    }
}
