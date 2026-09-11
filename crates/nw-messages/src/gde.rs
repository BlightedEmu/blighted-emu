// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::GdeMetadataReplicatedState` (type 10) on the bundle path: the spawn card of an entity
//! (mob-spawn note section 1). Children of slot 0: `AssetId` (16 raw bytes and a big-endian
//! `u32`, reader `FUN_1417b3670`) and `GdeRef` (16 raw bytes, `FUN_1417b3ae0`). The `AssetId`
//! is an `AZ::Data::AssetId` of the asset catalog: a 16-byte GUID and a sub id. The client starts
//! portraying an entity only from this card (`FUN_141717fc0`, "StartPortrayingGDE").

use crate::slots::{read_slot_0, write_slot_0};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer, entity_of_raw_id, raw_id_of_entity};

/// Type index of the class.
pub const GDE_METADATA_TYPE_INDEX: u64 = 10;
/// Length of a raw id.
pub const GDE_ID_LEN: usize = RAW_ID_LEN;
/// The component index of the spawn card in a bundle member: 0 on every captured card.
pub const GDE_METADATA_COMPONENT: u64 = 0;
/// The `AssetId` sub id of a `.dynamicslice` product in the asset catalog.
pub const DYNAMIC_SLICE_SUB_ID: u32 = 2;
/// The player prefab `slices/player.dynamicslice` of the pinned client build: its catalog GUID
/// (record name `player.dynamicslice`, size 367,528 bytes, in `assetcatalog.catalog` of
/// `Engine.pak`) with the dynamic-slice sub id. The same id is on 464 player spawn cards of 60
/// captured sessions. The local player's card must carry it: only a portrayed player entity
/// enables the `PlayerComponent` client facet that registers the local player.
pub const PLAYER_SLICE_ASSET: AssetId = AssetId {
    id: [
        0xa6, 0x60, 0xee, 0xeb, 0xeb, 0xc7, 0x5c, 0xb7, 0xbe, 0x6b, 0xab, 0x11, 0xeb, 0x83, 0x17,
        0x31,
    ],
    sub_id: DYNAMIC_SLICE_SUB_ID,
};
const CHILD_ASSET: u16 = 1 << 0;
const CHILD_GDE_REF: u16 = 1 << 1;
const CHILDREN: u8 = 2;
const FIELD: &str = "gde metadata";

/// The `AssetId` child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetId {
    /// The 16 raw bytes.
    pub id: [u8; GDE_ID_LEN],
    /// The big-endian `u32` after them (always 2 per the Azoth catalog field note).
    pub sub_id: u32,
}

/// The spawn card of an entity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GdeMetadataReplicatedState {
    /// The asset, when sent.
    pub asset: Option<AssetId>,
    /// The entity reference, when sent.
    pub gde_ref: Option<[u8; GDE_ID_LEN]>,
}

impl GdeMetadataReplicatedState {
    /// The card of entity `number` built from `asset`; its `GdeRef` is the entity's raw id
    /// ([`raw_id_of_entity`]).
    pub fn for_entity(asset: AssetId, number: u64) -> Self {
        Self {
            asset: Some(asset),
            gde_ref: Some(raw_id_of_entity(number)),
        }
    }

    /// The entity number of a card that [`GdeMetadataReplicatedState::for_entity`] made; `None`
    /// for a card without a `GdeRef` or with another id.
    pub fn entity_number(&self) -> Option<u64> {
        entity_of_raw_id(self.gde_ref.as_ref()?)
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or a form `write` cannot make is set.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let Some(children) = read_slot_0(reader, FIELD, CHILDREN)? else {
            return Ok(Self::default());
        };
        let asset = if children & CHILD_ASSET != 0 {
            Some(AssetId {
                id: reader.array()?,
                sub_id: reader.u32()?,
            })
        } else {
            None
        };
        let gde_ref = if children & CHILD_GDE_REF != 0 {
            Some(reader.array()?)
        } else {
            None
        };
        Ok(Self { asset, gde_ref })
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let mut children = 0;
        if self.asset.is_some() {
            children |= CHILD_ASSET;
        }
        if self.gde_ref.is_some() {
            children |= CHILD_GDE_REF;
        }
        write_slot_0(writer, Some(children), CHILDREN);
        if let Some(asset) = &self.asset {
            writer.bytes(&asset.id).u32(asset.sub_id);
        }
        if let Some(gde_ref) = &self.gde_ref {
            writer.bytes(gde_ref);
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn round(state: &GdeMetadataReplicatedState) -> GdeMetadataReplicatedState {
        let mut writer = Writer::new();
        state.write(&mut writer);
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes);
        let read = GdeMetadataReplicatedState::read(&mut reader).expect("read");
        reader.finish().expect("to the end");
        read
    }

    proptest! {
        #[test]
        fn gde_metadata_round_trips(
            asset in proptest::option::of((any::<[u8; 16]>(), any::<u32>())),
            gde_ref in proptest::option::of(any::<[u8; 16]>()),
        ) {
            let state = GdeMetadataReplicatedState {
                asset: asset.map(|(id, sub_id)| AssetId { id, sub_id }),
                gde_ref,
            };
            prop_assert_eq!(round(&state), state);
        }
    }

    #[test]
    fn a_card_for_an_entity_gives_its_number_back() {
        const NUMBER: u64 = (1 << 34) + 7;
        let card = GdeMetadataReplicatedState::for_entity(PLAYER_SLICE_ASSET, NUMBER);
        assert_eq!(card.asset, Some(PLAYER_SLICE_ASSET));
        assert_eq!(card.entity_number(), Some(NUMBER));
        assert_eq!(round(&card), card);
        let mut foreign_id = [7; GDE_ID_LEN];
        foreign_id[0] = 8;
        let foreign = GdeMetadataReplicatedState {
            asset: None,
            gde_ref: Some(foreign_id),
        };
        assert_eq!(foreign.entity_number(), None);
    }

    #[test]
    fn a_gde_body_with_only_the_ref_is_16_bytes_after_the_masks() {
        let state = GdeMetadataReplicatedState {
            asset: None,
            gde_ref: Some([7; 16]),
        };
        let mut writer = Writer::new();
        state.write(&mut writer);
        assert_eq!(writer.finish().len(), 2 + 16);
    }

    #[test]
    fn a_full_gde_body_is_38_bytes() {
        let state = GdeMetadataReplicatedState {
            asset: Some(AssetId {
                id: [1; 16],
                sub_id: 2,
            }),
            gde_ref: Some([7; 16]),
        };
        let mut writer = Writer::new();
        state.write(&mut writer);
        assert_eq!(writer.finish().len(), 2 + 20 + 16);
    }
}
