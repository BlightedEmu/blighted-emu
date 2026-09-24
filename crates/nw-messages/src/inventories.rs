// SPDX-License-Identifier: AGPL-3.0-or-later
//! The inventory service's answers (run-15 items2 study): 5792
//! `InventoriesComponentClientFacet_OnQueryInventories` (one inventory with its items) and 5771
//! `InventoriesComponentClientFacet_OnGetItems` (items).
//!
//! Retail 206 sessions: at login the server sends two 5792, the character's empty global
//! `ItemPool` and the account's `SkinCollection` (one item per collected appearance, member
//! `isCollected`); a skin enable, disable and dye answer with a 5771 of the worn item's
//! `ItemPool` entry and the character's `EquippedSkins` entry of the skin (members `a`, `b`, `g`,
//! `r`: the skin's dye). No client call of the service is in any capture.
//!
//! Layout (Observed, reflected classes `RequestContext`, `InventoryKey`, `ItemKey`,
//! `ItemVersionData`, `Inventory`): the [`FacetHeader`] of [`INVENTORIES`]; the request context
//! (an object of type 5828: regionality, request id, request group id, the requester (the
//! player's transmog component), request type, state, status, error text); then the
//! inventories (5792) or the items (5771). An item is its key (item type, id, scope), its
//! version data (an object of type 5762: source action, inventory key, version, transaction id,
//! the item data object of type 5946 holding a JSON object of integer members, the child links)
//! and a run of bytes after the links whose meaning is not known: each item form here writes
//! the run retail sent with it ([`ItemData`]). Strings are a length byte and the bytes;
//! integers are big-endian.

use crate::facet::FacetHeader;
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `InventoriesComponentClientFacet_OnQueryInventories`.
pub const QUERY_INVENTORIES_TYPE_INDEX: u64 = 5792;
/// Type index of `InventoriesComponentClientFacet_OnGetItems`.
pub const GET_ITEMS_TYPE_INDEX: u64 = 5771;
/// The inventories component of the player entity (Observed: target − entity is this in all
/// 101 retail 5792 headers of 40 sessions).
pub const INVENTORIES: u64 = 0xaa49_7e53_6bf9_a7f3;
/// The type index of a `RequestContext` object.
const REQUEST_CONTEXT_TYPE: u64 = 5828;
/// The type index of an `ItemVersionData` object.
const ITEM_VERSION_DATA_TYPE: u64 = 5762;
/// The type index of the item data object (`m_jsonValue`).
const ITEM_DATA_TYPE: u64 = 5946;
/// An object pointer that is set.
const OBJECT_PRESENT: u8 = 1;
/// The JSON kind of an object.
const JSON_OBJECT: u8 = 6;
/// The JSON kind of an integer (8 bytes).
const JSON_INTEGER: u8 = 1;
/// The JSON kind of an absent value (an item that is not found).
const JSON_NONE: u8 = 0;
/// `eRequestType` of an inventories query (5792).
pub const REQUEST_QUERY_INVENTORIES: u32 = 10;
/// `eRequestType` of a get-items request (5771).
pub const REQUEST_GET_ITEMS: u32 = 9;
/// `eRequestState` of a finished request (retail 2).
const REQUEST_STATE_DONE: u32 = 2;
/// The status of a request that went well (retail 200).
const STATUS_OK: u32 = 200;
/// Regionality of the global item pool (retail 0).
pub const REGIONALITY_GLOBAL: u32 = 0;
/// Regionality of an account's or character's own inventory (retail 1, `SkinCollection`).
pub const REGIONALITY_PERSONAL: u32 = 1;
/// Item type of an item instance in the item pool (retail 1).
pub const ITEM_TYPE_INSTANCE: u32 = 1;
/// Item type of a skin (retail 2).
pub const ITEM_TYPE_SKIN: u32 = 2;
/// The run after the links of a listed inventory item (5792; 423 of 423 retail items).
const LISTED_TAIL: [u8; 1] = [0];
/// The run after the links of an item-pool entry (5771; every retail entry).
const POOL_TAIL: [u8; 4] = [0, 0, 1, 1];
/// The run after the links of a found skin entry (5771, retail first enable).
const SKIN_TAIL: [u8; 2] = [0, 1];
/// The run after the links of an entry that is not found (5771: version 0, no data).
const MISSING_TAIL: [u8; 3] = [0, 0, 0];
/// The zero bytes that end a 5771 after its items (17 in every retail body).
const GET_ITEMS_TRAILER: [u8; 17] = [0; 17];
/// Longest string this server writes and reads: below the one-byte length form's limit.
const MAX_STRING_BYTES: usize = 127;
/// Most items of one answer this server reads.
const MAX_ITEMS: u64 = 0x10_0000;
const FIELD: &str = "inventory service";

/// The request an answer belongs to (`RequestContext`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRequest {
    /// The header (the player's inventories component).
    pub header: FacetHeader,
    /// [`REGIONALITY_GLOBAL`] or [`REGIONALITY_PERSONAL`].
    pub regionality: u32,
    /// The request id.
    pub request: [u8; RAW_ID_LEN],
    /// The request group id.
    pub group: [u8; RAW_ID_LEN],
    /// The requester: the target of the player's transmog component header.
    pub requester: u64,
}

/// An inventory's key: its type (`ItemPool`, `SkinCollection`, `EquippedSkins`) and owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryKey {
    /// The inventory type.
    pub kind: String,
    /// The owner: a character id, or the account's owner text.
    pub owner: String,
}

/// What an item holds and the byte run retail sent after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemData {
    /// An item of a listed inventory (5792) with integer members (`isCollected`).
    Listed(Vec<(String, u64)>),
    /// A found skin entry (5771, `EquippedSkins`) with integer members (`a`, `b`, `g`, `r`).
    Skin(Vec<(String, u64)>),
    /// An item-pool entry (5771) with no members and at most one child link, named.
    Pool(Option<String>),
    /// An entry that is not found (5771).
    Missing,
}

/// One item of the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceItem {
    /// [`ITEM_TYPE_INSTANCE`] or [`ITEM_TYPE_SKIN`].
    pub item_type: u32,
    /// Its id: a skin's decimal id, or an item instance's upper-case hex id.
    pub id: String,
    /// Its scope: the owner text, a character id or `global`.
    pub scope: String,
    /// The source action of its version (retail 0 for a new entry, 2 for a changed one).
    pub source: u32,
    /// The inventory it is in.
    pub inventory: InventoryKey,
    /// Its version (0 for an entry that is not found).
    pub version: u64,
    /// The transaction id of its version.
    pub transaction: [u8; RAW_ID_LEN],
    /// What it holds.
    pub data: ItemData,
}

/// One inventory with its items (5792).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceInventory {
    /// Its key.
    pub key: InventoryKey,
    /// Its items.
    pub items: Vec<ServiceItem>,
    /// Its current version.
    pub version: u64,
    /// The transaction id of its version.
    pub transaction: [u8; RAW_ID_LEN],
}

/// An answer of the inventory service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoriesMsg {
    /// 5792: one inventory.
    QueryInventories {
        /// The request.
        request: ServiceRequest,
        /// The inventory.
        inventory: ServiceInventory,
    },
    /// 5771: items.
    GetItems {
        /// The request.
        request: ServiceRequest,
        /// The items.
        items: Vec<ServiceItem>,
    },
}

fn write_string(writer: &mut Writer, text: &str) -> Result<(), MessageError> {
    if text.len() > MAX_STRING_BYTES {
        return Err(MessageError::CountTooLarge {
            count: text.len() as u64,
            limit: MAX_STRING_BYTES as u64,
        });
    }
    // At most `MAX_STRING_BYTES`.
    writer.u8(text.len() as u8).bytes(text.as_bytes());
    Ok(())
}

fn read_string(reader: &mut Reader<'_>) -> Result<String, MessageError> {
    let length = usize::from(reader.u8()?);
    if length > MAX_STRING_BYTES {
        return Err(MessageError::CountTooLarge {
            count: length as u64,
            limit: MAX_STRING_BYTES as u64,
        });
    }
    Ok(String::from_utf8_lossy(reader.bytes(length)?).into_owned())
}

fn write_object(writer: &mut Writer, type_index: u64) {
    writer.u8(OBJECT_PRESENT).wide_prefixed(type_index);
}

fn read_object(reader: &mut Reader<'_>, type_index: u64) -> Result<(), MessageError> {
    let present = reader.u8()?;
    let read = reader.wide_prefixed()?;
    if present != OBJECT_PRESENT || read != type_index {
        return Err(MessageError::BadTag {
            field: FIELD,
            tag: present,
        });
    }
    Ok(())
}

/// Reads `expected` and fails on other bytes.
fn read_run(reader: &mut Reader<'_>, expected: &[u8]) -> Result<(), MessageError> {
    if reader.bytes(expected.len())? != expected {
        return Err(MessageError::LengthMismatch { field: FIELD });
    }
    Ok(())
}

fn write_members(writer: &mut Writer, members: &[(String, u64)]) -> Result<(), MessageError> {
    let count = u8::try_from(members.len()).map_err(|_| MessageError::CountTooLarge {
        count: members.len() as u64,
        limit: u64::from(u8::MAX),
    })?;
    writer.u8(JSON_OBJECT).u8(count);
    for (name, value) in members {
        write_string(writer, name)?;
        writer.u8(JSON_INTEGER).u64(*value);
    }
    Ok(())
}

fn read_members(reader: &mut Reader<'_>) -> Result<Vec<(String, u64)>, MessageError> {
    let count = reader.u8()?;
    (0..count)
        .map(|_| {
            let name = read_string(reader)?;
            let kind = reader.u8()?;
            if kind != JSON_INTEGER {
                return Err(MessageError::BadTag {
                    field: FIELD,
                    tag: kind,
                });
            }
            Ok((name, reader.u64()?))
        })
        .collect()
}

impl ServiceRequest {
    fn write(&self, writer: &mut Writer, request_type: u32) {
        self.header.write(writer);
        write_object(writer, REQUEST_CONTEXT_TYPE);
        writer
            .u32(self.regionality)
            .bytes(&self.request)
            .bytes(&self.group)
            .u64(self.requester)
            .u32(request_type)
            .u32(REQUEST_STATE_DONE)
            .u32(STATUS_OK)
            .u8(0);
    }

    fn read(reader: &mut Reader<'_>, request_type: u32) -> Result<Self, MessageError> {
        let header = FacetHeader::read(reader)?;
        read_object(reader, REQUEST_CONTEXT_TYPE)?;
        let request = Self {
            header,
            regionality: reader.u32()?,
            request: reader.raw_id()?,
            group: reader.raw_id()?,
            requester: reader.u64()?,
        };
        if reader.u32()? != request_type {
            return Err(MessageError::BadTag {
                field: FIELD,
                tag: u8::try_from(request_type).unwrap_or(u8::MAX),
            });
        }
        let _state = reader.u32()?;
        let _status = reader.u32()?;
        let _error = read_string(reader)?;
        Ok(request)
    }
}

impl InventoryKey {
    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        write_string(writer, &self.kind)?;
        write_string(writer, &self.owner)
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            kind: read_string(reader)?,
            owner: read_string(reader)?,
        })
    }
}

impl ServiceItem {
    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.u32(self.item_type);
        write_string(writer, &self.id)?;
        write_string(writer, &self.scope)?;
        write_object(writer, ITEM_VERSION_DATA_TYPE);
        writer.u32(self.source);
        self.inventory.write(writer)?;
        writer.u64(self.version).bytes(&self.transaction);
        write_object(writer, ITEM_DATA_TYPE);
        match &self.data {
            ItemData::Listed(members) => {
                write_members(writer, members)?;
                writer.u8(0).bytes(&LISTED_TAIL);
            }
            ItemData::Skin(members) => {
                write_members(writer, members)?;
                writer.u8(0).bytes(&SKIN_TAIL);
            }
            ItemData::Pool(link) => {
                write_members(writer, &[])?;
                match link {
                    Some(name) => {
                        writer.u8(1);
                        write_string(writer, name)?;
                    }
                    None => {
                        writer.u8(0);
                    }
                }
                writer.bytes(&POOL_TAIL);
            }
            ItemData::Missing => {
                writer.u8(JSON_NONE).u8(0).bytes(&MISSING_TAIL);
            }
        }
        Ok(())
    }

    /// Reads one item; `listed` is true in a 5792.
    fn read(reader: &mut Reader<'_>, listed: bool) -> Result<Self, MessageError> {
        let item_type = reader.u32()?;
        let id = read_string(reader)?;
        let scope = read_string(reader)?;
        read_object(reader, ITEM_VERSION_DATA_TYPE)?;
        let source = reader.u32()?;
        let inventory = InventoryKey::read(reader)?;
        let version = reader.u64()?;
        let transaction = reader.raw_id()?;
        read_object(reader, ITEM_DATA_TYPE)?;
        let data = match reader.u8()? {
            JSON_NONE => {
                read_run(reader, &[0])?;
                read_run(reader, &MISSING_TAIL)?;
                ItemData::Missing
            }
            JSON_OBJECT => {
                let members = read_members(reader)?;
                let links = reader.u8()?;
                if listed {
                    read_run(reader, &LISTED_TAIL)?;
                    ItemData::Listed(members)
                } else if members.is_empty() {
                    let link = match links {
                        0 => None,
                        _ => Some(read_string(reader)?),
                    };
                    read_run(reader, &POOL_TAIL)?;
                    ItemData::Pool(link)
                } else {
                    read_run(reader, &SKIN_TAIL)?;
                    ItemData::Skin(members)
                }
            }
            kind => {
                return Err(MessageError::BadTag {
                    field: FIELD,
                    tag: kind,
                });
            }
        };
        Ok(Self {
            item_type,
            id,
            scope,
            source,
            inventory,
            version,
            transaction,
            data,
        })
    }
}

fn read_items(reader: &mut Reader<'_>, listed: bool) -> Result<Vec<ServiceItem>, MessageError> {
    let count = reader.wide_prefixed()?;
    if count > MAX_ITEMS {
        return Err(MessageError::CountTooLarge {
            count,
            limit: MAX_ITEMS,
        });
    }
    (0..count)
        .map(|_| ServiceItem::read(reader, listed))
        .collect()
}

impl InventoriesMsg {
    /// Whether `type_index` is an answer of the inventory service.
    pub const fn is_type(type_index: u64) -> bool {
        matches!(
            type_index,
            QUERY_INVENTORIES_TYPE_INDEX | GET_ITEMS_TYPE_INDEX
        )
    }

    /// The type index of the answer.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::QueryInventories { .. } => QUERY_INVENTORIES_TYPE_INDEX,
            Self::GetItems { .. } => GET_ITEMS_TYPE_INDEX,
        }
    }

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for a string longer than this server writes or
    /// an item with more than 255 members.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        match self {
            Self::QueryInventories { request, inventory } => {
                request.write(&mut writer, REQUEST_QUERY_INVENTORIES);
                // One inventory.
                writer
                    .wide_prefixed(1)
                    .wide_prefixed(inventory.items.len() as u64);
                for item in &inventory.items {
                    item.write(&mut writer)?;
                }
                inventory.key.write(&mut writer)?;
                writer
                    .u64(inventory.version)
                    .bytes(&inventory.transaction)
                    // The pagination key: none.
                    .u8(0);
            }
            Self::GetItems { request, items } => {
                request.write(&mut writer, REQUEST_GET_ITEMS);
                writer.wide_prefixed(items.len() as u64);
                for item in items {
                    item.write(&mut writer)?;
                }
                writer.bytes(&GET_ITEMS_TRAILER);
            }
        }
        Ok(writer.finish())
    }

    /// Decodes the body of the answer of `type_index` in the forms this server writes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body is not one of those forms or `type_index` is not
    /// an answer of the service.
    pub fn decode(type_index: u64, body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = match type_index {
            QUERY_INVENTORIES_TYPE_INDEX => {
                let request = ServiceRequest::read(&mut reader, REQUEST_QUERY_INVENTORIES)?;
                let inventories = reader.wide_prefixed()?;
                if inventories != 1 {
                    return Err(MessageError::CountTooLarge {
                        count: inventories,
                        limit: 1,
                    });
                }
                let items = read_items(&mut reader, true)?;
                let key = InventoryKey::read(&mut reader)?;
                let version = reader.u64()?;
                let transaction = reader.raw_id()?;
                let _pagination = read_string(&mut reader)?;
                Self::QueryInventories {
                    request,
                    inventory: ServiceInventory {
                        key,
                        items,
                        version,
                        transaction,
                    },
                }
            }
            GET_ITEMS_TYPE_INDEX => {
                let request = ServiceRequest::read(&mut reader, REQUEST_GET_ITEMS)?;
                let items = read_items(&mut reader, false)?;
                read_run(&mut reader, &GET_ITEMS_TRAILER)?;
                Self::GetItems { request, items }
            }
            _ => {
                return Err(MessageError::BadTag {
                    field: FIELD,
                    tag: u8::try_from(type_index).unwrap_or(u8::MAX),
                });
            }
        };
        reader.finish()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// The shape of the retail first 5792 of a new character (newchar 2026-09-18): its empty
    /// item pool, with the ids, the owner and the transaction replaced.
    const POOL_HEAD: &str = concat!(
        "d15f78100d8986c12715f9bca18fdece",
        "01845b00000000",
        "11111111111111111111111111111111",
        "22222222222222222222222222222222",
        "113f49e93bb0598d0000000a00000002000000c800",
        "0100"
    );
    const OWNER: &str = "00000000-0000-0000-0000-000000000001";
    const TRANSACTION: [u8; RAW_ID_LEN] = [0x33; RAW_ID_LEN];

    fn request(regionality: u32) -> ServiceRequest {
        ServiceRequest {
            header: FacetHeader {
                target: 0xd15f_7810_0d89_86c1,
                entity: 0x2715_f9bc_a18f_dece,
            },
            regionality,
            request: [0x11; RAW_ID_LEN],
            group: [0x22; RAW_ID_LEN],
            requester: 0x113f_49e9_3bb0_598d,
        }
    }

    fn key(kind: &str) -> InventoryKey {
        InventoryKey {
            kind: kind.to_owned(),
            owner: OWNER.to_owned(),
        }
    }

    fn round_trip(message: &InventoriesMsg) -> Vec<u8> {
        let bytes = message.encode().expect("encode");
        assert_eq!(
            InventoriesMsg::decode(message.type_index(), &bytes).as_ref(),
            Ok(message)
        );
        bytes
    }

    #[test]
    fn an_empty_item_pool_is_the_retail_shape() {
        let message = InventoriesMsg::QueryInventories {
            request: request(REGIONALITY_GLOBAL),
            inventory: ServiceInventory {
                key: key("ItemPool"),
                items: Vec::new(),
                version: 1,
                transaction: TRANSACTION,
            },
        };
        let bytes = round_trip(&message);
        let mut expected = hex(POOL_HEAD);
        expected.extend_from_slice(b"\x08ItemPool\x24");
        expected.extend_from_slice(OWNER.as_bytes());
        expected.extend_from_slice(&1_u64.to_be_bytes());
        expected.extend_from_slice(&TRANSACTION);
        expected.push(0);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn a_collected_skin_is_the_retail_item_shape() {
        let item = ServiceItem {
            item_type: ITEM_TYPE_SKIN,
            id: "1011852927".to_owned(),
            scope: OWNER.to_owned(),
            source: 0,
            inventory: key("SkinCollection"),
            version: 1,
            transaction: TRANSACTION,
            data: ItemData::Listed(vec![("isCollected".to_owned(), 1)]),
        };
        let mut writer = Writer::new();
        item.write(&mut writer).expect("item");
        let bytes = writer.finish();
        // The retail tail after the transaction id: the data object, its one member, the
        // links and the run after them.
        let mut tail = hex("01ba5c06010b");
        tail.extend_from_slice(b"isCollected");
        tail.extend_from_slice(&hex("010000000000000001"));
        tail.extend_from_slice(&[0, 0]);
        assert_eq!(bytes[bytes.len() - tail.len()..], tail[..]);
        assert_eq!(bytes[..5], hex("000000020a")[..]);
        let message = InventoriesMsg::QueryInventories {
            request: request(REGIONALITY_PERSONAL),
            inventory: ServiceInventory {
                key: key("SkinCollection"),
                items: vec![item],
                version: 1,
                transaction: TRANSACTION,
            },
        };
        round_trip(&message);
    }

    #[test]
    fn the_skin_answers_round_trip_with_their_retail_runs() {
        let pool = |link: Option<&str>| ServiceItem {
            item_type: ITEM_TYPE_INSTANCE,
            id: "D4D83CF8DDF74050B0862AC3F4F7E6F7".to_owned(),
            scope: "global".to_owned(),
            source: 0,
            inventory: key("ItemPool"),
            version: 11,
            transaction: TRANSACTION,
            data: ItemData::Pool(link.map(str::to_owned)),
        };
        let skin = |data| ServiceItem {
            item_type: ITEM_TYPE_SKIN,
            id: "2958132517".to_owned(),
            scope: OWNER.to_owned(),
            source: 0,
            inventory: key("EquippedSkins"),
            version: 2,
            transaction: TRANSACTION,
            data,
        };
        let dyed = ItemData::Skin(
            [("a", 0x20), ("b", 0x3e), ("g", 0x94), ("r", 0x06)]
                .map(|(name, value)| (name.to_owned(), value))
                .to_vec(),
        );
        let enabled = InventoriesMsg::GetItems {
            request: request(REGIONALITY_GLOBAL),
            items: vec![pool(None), skin(dyed)],
        };
        let bytes = round_trip(&enabled);
        // Retail 411-byte body: the pool entry's run, then the skin's members and run, then
        // the 17-byte end.
        let end = [0x06, 0x00, 0x00, 0x01];
        let at = bytes.len() - GET_ITEMS_TRAILER.len() - end.len();
        assert_eq!(bytes[at..at + end.len()], end);
        let disabled = InventoriesMsg::GetItems {
            request: request(REGIONALITY_GLOBAL),
            items: vec![pool(Some("SkinSlot")), skin(ItemData::Missing)],
        };
        let bytes = round_trip(&disabled);
        assert!(bytes.ends_with(&[0; 5 + GET_ITEMS_TRAILER.len()]));
        assert!(InventoriesMsg::is_type(GET_ITEMS_TYPE_INDEX));
        assert!(InventoriesMsg::decode(1, &bytes).is_err());
    }
}
