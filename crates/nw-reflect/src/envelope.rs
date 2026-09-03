// SPDX-License-Identifier: AGPL-3.0-or-later
//! The envelope around each message (carrier captures note section 5).
//!
//! Layout: flags byte; 8 sender bytes when flag `0x01` is set; 8 recipient bytes when flag
//! `0x02` is set; presence byte (always 1 in the captures); type id (prefix code, index 0 is
//! followed by a 16-byte UUID); body. Flag `0x04` adds a routing header whose layout is Unknown,
//! so it is refused.

use thiserror::Error;

use crate::type_index::{TypeId, TypeIndexError, decode_type_id, encode_type_id};

/// Envelope flag: 8 sender bytes follow the flags byte.
pub const ENVELOPE_SENDER: u8 = 0x01;
/// Envelope flag: 8 recipient bytes follow (after the sender bytes).
pub const ENVELOPE_RECIPIENT: u8 = 0x02;
/// Bytes of a sender or recipient id.
pub const PEER_ID_LEN: usize = 8;
/// The presence byte of every envelope in the captures.
pub const TYPE_PRESENT: u8 = 1;

/// Flags this codec reads. Any other bit (for example the `0x04` routing header) is refused.
const KNOWN_FLAGS: u8 = ENVELOPE_SENDER | ENVELOPE_RECIPIENT;
const FLAGS_LEN: usize = 1;
const PRESENCE_LEN: usize = 1;

/// Failure from the envelope codec.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EnvelopeError {
    /// The envelope ended before a field was complete.
    #[error("truncated envelope: need {needed} bytes, have {available}")]
    Truncated {
        /// Bytes this read needs.
        needed: usize,
        /// Bytes left.
        available: usize,
    },
    /// The flags byte has a bit this codec does not read.
    #[error("envelope flags {flags:#04x} are not supported")]
    UnsupportedFlags {
        /// The flags byte.
        flags: u8,
    },
    /// The presence byte is not 1.
    #[error("envelope presence byte {byte:#04x} is not 1")]
    UnsupportedPresence {
        /// The presence byte.
        byte: u8,
    },
    /// The type id could not be read or written.
    #[error(transparent)]
    TypeId(#[from] TypeIndexError),
}

/// One message envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// Sender id (flag `0x01`).
    pub sender: Option<[u8; PEER_ID_LEN]>,
    /// Recipient id (flag `0x02`).
    pub recipient: Option<[u8; PEER_ID_LEN]>,
    /// Message type.
    pub type_id: TypeId,
    /// Message body.
    pub body: Vec<u8>,
}

impl Envelope {
    /// An envelope with no sender and no recipient, as the client and the gateway send them.
    pub const fn message(type_id: TypeId, body: Vec<u8>) -> Self {
        Self {
            sender: None,
            recipient: None,
            type_id,
            body,
        }
    }

    /// The file slot of the message type, or `None` for an inline UUID.
    pub const fn type_index(&self) -> Option<u64> {
        match self.type_id {
            TypeId::FileSlot(index) => Some(index),
            TypeId::InlineUuid(_) => None,
        }
    }

    /// Writes the envelope.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeError::TypeId`] when the type id cannot be encoded.
    pub fn encode(&self) -> Result<Vec<u8>, EnvelopeError> {
        let mut flags = 0;
        if self.sender.is_some() {
            flags |= ENVELOPE_SENDER;
        }
        if self.recipient.is_some() {
            flags |= ENVELOPE_RECIPIENT;
        }
        let mut out = vec![flags];
        for id in [self.sender, self.recipient].into_iter().flatten() {
            out.extend_from_slice(&id);
        }
        out.push(TYPE_PRESENT);
        out.extend(encode_type_id(&self.type_id)?);
        out.extend_from_slice(&self.body);
        Ok(out)
    }

    /// Reads an envelope.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeError`] when the bytes are short, a flag or the presence byte is not
    /// supported, or the type id is cut short.
    pub fn decode(bytes: &[u8]) -> Result<Self, EnvelopeError> {
        let mut rest = bytes;
        let flags = take(&mut rest, FLAGS_LEN)?[0];
        if flags & !KNOWN_FLAGS != 0 {
            return Err(EnvelopeError::UnsupportedFlags { flags });
        }
        let sender = read_peer(&mut rest, flags & ENVELOPE_SENDER != 0)?;
        let recipient = read_peer(&mut rest, flags & ENVELOPE_RECIPIENT != 0)?;
        let presence = take(&mut rest, PRESENCE_LEN)?[0];
        if presence != TYPE_PRESENT {
            return Err(EnvelopeError::UnsupportedPresence { byte: presence });
        }
        let (type_id, used) = decode_type_id(rest)?;
        Ok(Self {
            sender,
            recipient,
            type_id,
            body: rest[used..].to_vec(),
        })
    }
}

fn take<'a>(rest: &mut &'a [u8], len: usize) -> Result<&'a [u8], EnvelopeError> {
    if rest.len() < len {
        return Err(EnvelopeError::Truncated {
            needed: len,
            available: rest.len(),
        });
    }
    let (head, tail) = rest.split_at(len);
    *rest = tail;
    Ok(head)
}

fn read_peer(rest: &mut &[u8], present: bool) -> Result<Option<[u8; PEER_ID_LEN]>, EnvelopeError> {
    if !present {
        return Ok(None);
    }
    let (id, tail) = rest
        .split_first_chunk::<PEER_ID_LEN>()
        .ok_or(EnvelopeError::Truncated {
            needed: PEER_ID_LEN,
            available: rest.len(),
        })?;
    *rest = tail;
    Ok(Some(*id))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::type_index::{INLINE_UUID_LEN, PING_MSG_TYPE_INDEX};

    /// `00 01 9d 05` then an 8-byte body: a ping as the captures show it.
    const CAPTURED_PING: [u8; 12] = [0x00, 0x01, 0x9D, 0x05, 0, 0, 0, 0, 0, 0, 0, 7];
    const PING_BODY: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 7];
    const SENDER: [u8; PEER_ID_LEN] = [1; PEER_ID_LEN];
    const RECIPIENT: [u8; PEER_ID_LEN] = [2; PEER_ID_LEN];
    /// Flag `0x04`: the routing header with an Unknown layout.
    const ROUTING_FLAG: u8 = 0x04;
    const ABSENT_PRESENCE: u8 = 0;
    /// Slots in the client file `typeindex.json` of the pinned build.
    const TYPE_TABLE_LEN: u64 = 7052;

    #[test]
    fn decodes_a_captured_ping() {
        let envelope = Envelope::decode(&CAPTURED_PING).expect("decode");
        assert_eq!(envelope.type_index(), Some(PING_MSG_TYPE_INDEX));
        assert_eq!(envelope.body, PING_BODY);
        assert_eq!(envelope.sender, None);
        assert_eq!(envelope.encode().expect("encode"), CAPTURED_PING);
    }

    #[test]
    fn sender_and_recipient_follow_the_flags_byte() {
        let envelope = Envelope {
            sender: Some(SENDER),
            recipient: Some(RECIPIENT),
            type_id: TypeId::FileSlot(PING_MSG_TYPE_INDEX),
            body: PING_BODY.to_vec(),
        };
        let bytes = envelope.encode().expect("encode");
        assert_eq!(bytes[0], ENVELOPE_SENDER | ENVELOPE_RECIPIENT);
        assert_eq!(&bytes[1..=PEER_ID_LEN], &SENDER);
        assert_eq!(Envelope::decode(&bytes).expect("decode"), envelope);
    }

    #[test]
    fn inline_uuid_envelopes_round_trip() {
        let envelope =
            Envelope::message(TypeId::InlineUuid([9; INLINE_UUID_LEN]), PING_BODY.to_vec());
        assert_eq!(envelope.type_index(), None);
        let bytes = envelope.encode().expect("encode");
        assert_eq!(Envelope::decode(&bytes).expect("decode"), envelope);
    }

    #[test]
    fn refuses_routing_headers_and_absent_types() {
        let mut routed = CAPTURED_PING;
        routed[0] = ROUTING_FLAG;
        assert_eq!(
            Envelope::decode(&routed),
            Err(EnvelopeError::UnsupportedFlags {
                flags: ROUTING_FLAG
            })
        );
        let mut absent = CAPTURED_PING;
        absent[1] = ABSENT_PRESENCE;
        assert_eq!(
            Envelope::decode(&absent),
            Err(EnvelopeError::UnsupportedPresence {
                byte: ABSENT_PRESENCE
            })
        );
        assert!(matches!(
            Envelope::decode(&[ENVELOPE_SENDER, 0]),
            Err(EnvelopeError::Truncated { .. })
        ));
    }

    proptest! {
        #[test]
        fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
            let _ = Envelope::decode(&bytes);
        }

        #[test]
        fn file_slot_envelopes_round_trip(index in 1..TYPE_TABLE_LEN, body in proptest::collection::vec(any::<u8>(), 0..64)) {
            let envelope = Envelope::message(TypeId::FileSlot(index), body);
            let bytes = envelope.encode().expect("encode");
            prop_assert_eq!(Envelope::decode(&bytes).expect("decode"), envelope);
        }
    }
}
