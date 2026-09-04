// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Amazon::Hub::ReplicatedStateBundle` (bodies note, section 8): the outer header only. Reader
//! `FUN_14646c2b0`, header `FUN_146b07870`. The payload bytes after the header are kept as they
//! are; their member layout is not decoded here.

use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// File slot of `ReplicatedStateBundle`.
pub const REPLICATED_STATE_BUNDLE_TYPE_INDEX: u64 = 8;
/// Most `u16` values in the index block (`FUN_146a6ddb0`).
pub const MAX_BUNDLE_INDEXES: u64 = 100;

/// The optional block of step 7: a prefix-coded count, then a list of big-endian `u16`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexBlock {
    /// The first prefix-coded count. Its meaning is Unknown.
    pub count: u64,
    /// The `u16` values; at most [`MAX_BUNDLE_INDEXES`].
    pub indexes: Vec<u16>,
}

/// The header and the raw payload of one bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicatedStateBundle {
    /// Steps 1 and 2: a prefix-coded integer, present when the first byte is 1.
    pub leading: Option<u64>,
    /// Step 3: one byte.
    pub byte_3: u8,
    /// Step 4: one byte.
    pub byte_4: u8,
    /// Step 5: a 0 or 1 byte.
    pub flag_5: bool,
    /// Steps 6 and 7: present when the step 6 byte is 1.
    pub index_block: Option<IndexBlock>,
    /// Steps 8 and 9: the payload, after its prefix-coded length.
    pub payload: Vec<u8>,
}

impl BodyCodec for ReplicatedStateBundle {
    const TYPE_INDEX: u64 = REPLICATED_STATE_BUNDLE_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer.boolean(self.leading.is_some());
        if let Some(leading) = self.leading {
            writer.prefixed(leading)?;
        }
        writer
            .u8(self.byte_3)
            .u8(self.byte_4)
            .boolean(self.flag_5)
            .boolean(self.index_block.is_some());
        if let Some(block) = &self.index_block {
            let count = u64::try_from(block.indexes.len()).unwrap_or(u64::MAX);
            if count > MAX_BUNDLE_INDEXES {
                return Err(MessageError::CountTooLarge {
                    count,
                    limit: MAX_BUNDLE_INDEXES,
                });
            }
            writer.prefixed(block.count)?.prefixed(count)?;
            for index in &block.indexes {
                writer.u16(*index);
            }
        }
        let length = u64::try_from(self.payload.len()).unwrap_or(u64::MAX);
        writer.prefixed(length)?.bytes(&self.payload);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let leading = if reader.boolean()? {
            Some(reader.prefixed()?)
        } else {
            None
        };
        let byte_3 = reader.u8()?;
        let byte_4 = reader.u8()?;
        let flag_5 = reader.boolean()?;
        let index_block = if reader.boolean()? {
            let count = reader.prefixed()?;
            let len = reader.prefixed()?;
            if len > MAX_BUNDLE_INDEXES {
                return Err(MessageError::CountTooLarge {
                    count: len,
                    limit: MAX_BUNDLE_INDEXES,
                });
            }
            let mut indexes = Vec::new();
            for _ in 0..len {
                indexes.push(reader.u16()?);
            }
            Some(IndexBlock { count, indexes })
        } else {
            None
        };
        // A length above the address space is cut off by the reader as a short body.
        let length = usize::try_from(reader.prefixed()?).unwrap_or(usize::MAX);
        let payload = reader.bytes(length)?.to_vec();
        reader.finish()?;
        Ok(Self {
            leading,
            byte_3,
            byte_4,
            flag_5,
            index_block,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const PAYLOAD_LEN: usize = 300;
    const LEADING: u64 = 1000;

    fn full() -> ReplicatedStateBundle {
        ReplicatedStateBundle {
            leading: Some(LEADING),
            byte_3: 3,
            byte_4: 4,
            flag_5: true,
            index_block: Some(IndexBlock {
                count: 2,
                indexes: vec![7, 0x0102],
            }),
            payload: vec![5; PAYLOAD_LEN],
        }
    }

    #[test]
    fn every_optional_part_round_trips() {
        let bundle = full();
        let body = bundle.encode().expect("encode");
        assert_eq!(ReplicatedStateBundle::decode(&body), Ok(bundle));
        let bare = ReplicatedStateBundle {
            leading: None,
            index_block: None,
            payload: Vec::new(),
            ..full()
        };
        let body = bare.encode().expect("encode");
        assert_eq!(body, [0, 3, 4, 1, 0, 0]);
        assert_eq!(ReplicatedStateBundle::decode(&body), Ok(bare));
    }

    #[test]
    fn too_many_indexes_and_short_payloads_are_errors() {
        let mut bundle = full();
        bundle.index_block = Some(IndexBlock {
            count: 0,
            indexes: vec![0; MAX_BUNDLE_INDEXES as usize + 1],
        });
        assert!(matches!(
            bundle.encode(),
            Err(MessageError::CountTooLarge { .. })
        ));
        let body = full().encode().expect("encode");
        assert!(ReplicatedStateBundle::decode(&body[..body.len() - 1]).is_err());
        let mut flag = body.clone();
        flag[0] = 2;
        assert_eq!(
            ReplicatedStateBundle::decode(&flag),
            Err(MessageError::NotBoolean { value: 2 })
        );
    }

    proptest! {
        #[test]
        fn random_bodies_never_panic(body in proptest::collection::vec(any::<u8>(), 0..64)) {
            let _ = ReplicatedStateBundle::decode(&body);
        }
    }
}
