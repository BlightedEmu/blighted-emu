// SPDX-License-Identifier: AGPL-3.0-or-later
//! How messages sit in a delivered carrier payload (carrier captures note section 5).
//!
//! Client to server: one message, `[u32 BE CRC-32][u32 BE length][16 bytes][envelope]`. The CRC
//! covers every byte after the first 8. The length is the message size minus 8.
//!
//! Server to client: one or more `[prefix-coded length][envelope]` items.

use thiserror::Error;

use crate::type_index::{TypeIndexError, decode_type_index_prefix, encode_type_index_prefix};

/// Bytes of the CRC field.
const CHECK_LEN: usize = 4;
/// Bytes of the length field.
const LENGTH_LEN: usize = 4;
/// Bytes before the part that the CRC and the length cover.
const CLIENT_PREFIX_LEN: usize = CHECK_LEN + LENGTH_LEN;
/// Bytes of the session field after the length. Zero in registration; meaning Unknown.
pub const CLIENT_SESSION_LEN: usize = 16;
/// Bytes before the envelope of a client message.
pub const CLIENT_HEADER_LEN: usize = CLIENT_PREFIX_LEN + CLIENT_SESSION_LEN;

/// Failure from the message framing.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FramingError {
    /// The payload ended before a field was complete.
    #[error("truncated message: need {needed} bytes, have {available}")]
    Truncated {
        /// Bytes this read needs.
        needed: usize,
        /// Bytes left in the payload.
        available: usize,
    },
    /// The length field does not match the payload.
    #[error("message length field {declared} does not match {actual}")]
    LengthMismatch {
        /// Value of the length field.
        declared: u64,
        /// Payload size minus 8.
        actual: usize,
    },
    /// The CRC field does not match the bytes after it.
    #[error("message CRC {declared:#010x} does not match {computed:#010x}")]
    ChecksumMismatch {
        /// Value of the CRC field.
        declared: u32,
        /// CRC-32 of the bytes after the first 8.
        computed: u32,
    },
    /// A length prefix could not be read or written.
    #[error(transparent)]
    Prefix(#[from] TypeIndexError),
    /// A message is too long for the 32-bit length field.
    #[error("message of {length} bytes does not fit the length field")]
    TooLong {
        /// Bytes after the first 8.
        length: usize,
    },
}

/// A client message after its header is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientMessage<'a> {
    /// The 16 bytes after the length.
    pub session: [u8; CLIENT_SESSION_LEN],
    /// The envelope bytes.
    pub envelope: &'a [u8],
}

/// Writes one client message around `envelope`.
///
/// # Errors
///
/// Returns [`FramingError::TooLong`] when the message does not fit the 32-bit length field.
pub fn encode_client_message(
    session: &[u8; CLIENT_SESSION_LEN],
    envelope: &[u8],
) -> Result<Vec<u8>, FramingError> {
    let covered_len = CLIENT_SESSION_LEN + envelope.len();
    let length = u32::try_from(covered_len).map_err(|_| FramingError::TooLong {
        length: covered_len,
    })?;
    let mut covered = Vec::with_capacity(covered_len);
    covered.extend_from_slice(session);
    covered.extend_from_slice(envelope);
    let mut out = Vec::with_capacity(CLIENT_PREFIX_LEN + covered_len);
    out.extend_from_slice(&crc32fast::hash(&covered).to_be_bytes());
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&covered);
    Ok(out)
}

/// Reads one client message and checks its length and CRC.
///
/// # Errors
///
/// Returns [`FramingError`] when the payload is short, the length field is wrong, or the CRC
/// does not match.
pub fn decode_client_message(payload: &[u8]) -> Result<ClientMessage<'_>, FramingError> {
    let truncated = FramingError::Truncated {
        needed: CLIENT_HEADER_LEN,
        available: payload.len(),
    };
    if payload.len() < CLIENT_HEADER_LEN {
        return Err(truncated);
    }
    let (check, rest) = payload
        .split_first_chunk::<CHECK_LEN>()
        .ok_or_else(|| truncated.clone())?;
    let (length, covered) = rest
        .split_first_chunk::<LENGTH_LEN>()
        .ok_or_else(|| truncated.clone())?;
    let declared_length = u64::from(u32::from_be_bytes(*length));
    if usize::try_from(declared_length).ok() != Some(covered.len()) {
        return Err(FramingError::LengthMismatch {
            declared: declared_length,
            actual: covered.len(),
        });
    }
    let declared = u32::from_be_bytes(*check);
    let computed = crc32fast::hash(covered);
    if declared != computed {
        return Err(FramingError::ChecksumMismatch { declared, computed });
    }
    let (session, envelope) = covered
        .split_first_chunk::<CLIENT_SESSION_LEN>()
        .ok_or(truncated)?;
    Ok(ClientMessage {
        session: *session,
        envelope,
    })
}

/// Writes server envelopes, each after its prefix-coded length.
///
/// # Errors
///
/// Returns [`FramingError::Prefix`] when a length does not fit the prefix code.
pub fn encode_server_messages(envelopes: &[&[u8]]) -> Result<Vec<u8>, FramingError> {
    let mut out = Vec::new();
    for envelope in envelopes {
        out.extend(encode_type_index_prefix(envelope.len() as u64)?);
        out.extend_from_slice(envelope);
    }
    Ok(out)
}

/// Splits a server payload into its envelopes.
///
/// # Errors
///
/// Returns [`FramingError`] when a length prefix is cut short or an envelope runs past the end.
pub fn decode_server_messages(payload: &[u8]) -> Result<Vec<&[u8]>, FramingError> {
    let mut envelopes = Vec::new();
    let mut rest = payload;
    while !rest.is_empty() {
        let prefix = decode_type_index_prefix(rest)?;
        let after = &rest[prefix.len..];
        let length = usize::try_from(prefix.index)
            .ok()
            .filter(|length| *length <= after.len())
            .ok_or(FramingError::Truncated {
                needed: usize::try_from(prefix.index).unwrap_or(usize::MAX),
                available: after.len(),
            })?;
        let (envelope, next) = after.split_at(length);
        envelopes.push(envelope);
        rest = next;
    }
    Ok(envelopes)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// An envelope as the captures start it: flags 0, presence 1, `PingMsg` (349).
    const PING_ENVELOPE: [u8; 12] = [0x00, 0x01, 0x9D, 0x05, 1, 2, 3, 4, 5, 6, 7, 8];
    const ZERO_SESSION: [u8; CLIENT_SESSION_LEN] = [0; CLIENT_SESSION_LEN];
    /// Length field of a client message around [`PING_ENVELOPE`]: 16 + 12.
    const PING_COVERED_LEN: u32 = 28;
    /// A server envelope of 130 bytes needs a two-byte length prefix.
    const LONG_ENVELOPE_LEN: usize = 130;

    #[test]
    fn client_message_has_crc_length_session_and_envelope() {
        let message = encode_client_message(&ZERO_SESSION, &PING_ENVELOPE).expect("encode");
        assert_eq!(message.len(), CLIENT_HEADER_LEN + PING_ENVELOPE.len());
        assert_eq!(
            &message[CHECK_LEN..CLIENT_PREFIX_LEN],
            &PING_COVERED_LEN.to_be_bytes()
        );
        assert_eq!(
            &message[..CHECK_LEN],
            &crc32fast::hash(&message[CLIENT_PREFIX_LEN..]).to_be_bytes()
        );
        let decoded = decode_client_message(&message).expect("decode");
        assert_eq!(decoded.session, ZERO_SESSION);
        assert_eq!(decoded.envelope, PING_ENVELOPE);
    }

    #[test]
    fn client_message_rejects_a_wrong_crc_or_length() {
        let message = encode_client_message(&ZERO_SESSION, &PING_ENVELOPE).expect("encode");
        let mut bad_check = message.clone();
        bad_check[0] ^= 0xFF;
        assert!(matches!(
            decode_client_message(&bad_check),
            Err(FramingError::ChecksumMismatch { .. })
        ));
        let mut bad_length = message.clone();
        bad_length[CLIENT_PREFIX_LEN - 1] ^= 0x01;
        assert!(matches!(
            decode_client_message(&bad_length),
            Err(FramingError::LengthMismatch { .. })
        ));
        assert!(matches!(
            decode_client_message(&message[..CLIENT_HEADER_LEN - 1]),
            Err(FramingError::Truncated { .. })
        ));
    }

    #[test]
    fn server_payload_holds_length_prefixed_envelopes() {
        let long = [0xAA_u8; LONG_ENVELOPE_LEN];
        let payload = encode_server_messages(&[&PING_ENVELOPE, &long]).expect("encode");
        assert_eq!(payload[0], PING_ENVELOPE.len() as u8);
        let envelopes = decode_server_messages(&payload).expect("decode");
        assert_eq!(envelopes, vec![&PING_ENVELOPE[..], &long[..]]);
    }

    #[test]
    fn server_payload_rejects_an_envelope_past_the_end() {
        let payload = encode_server_messages(&[&PING_ENVELOPE]).expect("encode");
        assert!(matches!(
            decode_server_messages(&payload[..payload.len() - 1]),
            Err(FramingError::Truncated { .. })
        ));
    }

    proptest! {
        #[test]
        fn client_messages_round_trip(session in any::<[u8; CLIENT_SESSION_LEN]>(), envelope in proptest::collection::vec(any::<u8>(), 0..512)) {
            let message = encode_client_message(&session, &envelope).expect("encode");
            let decoded = decode_client_message(&message).expect("decode");
            prop_assert_eq!(decoded.session, session);
            prop_assert_eq!(decoded.envelope, &envelope[..]);
        }

        #[test]
        fn server_messages_round_trip(envelopes in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..300), 0..6)) {
            let slices: Vec<&[u8]> = envelopes.iter().map(Vec::as_slice).collect();
            let payload = encode_server_messages(&slices).expect("encode");
            prop_assert_eq!(decode_server_messages(&payload).expect("decode"), slices);
        }

        #[test]
        fn random_payloads_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
            let _ = decode_client_message(&bytes);
            let _ = decode_server_messages(&bytes);
        }
    }
}
