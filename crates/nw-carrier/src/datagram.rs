// SPDX-License-Identifier: AGPL-3.0-or-later
//! The datagram header (carrier captures note section 2).
//!
//! `[mode][0x01][u16 BE sequence][body]`. Mode `0x81`: the body is one LZ4 block. Mode `0x80`:
//! the body is plain. The sender uses `0x81` only when the block is shorter.

use crate::error::CarrierError;

/// Mode byte of a plain body.
pub const MODE_PLAIN: u8 = 0x80;
/// Mode byte of an LZ4 body.
pub const MODE_LZ4: u8 = 0x81;
/// Second header byte in every captured datagram.
pub const HEADER_VERSION: u8 = 0x01;
/// Bytes of the datagram header.
pub const DATAGRAM_HEADER_LEN: usize = 4;
/// Largest datagram in the captures, plain or after LZ4 decoding (1,128 bytes).
pub const MAX_DATAGRAM_LEN: usize = 1128;
/// Largest body this decoder takes, plain or unpacked from LZ4.
pub const MAX_BODY_LEN: usize = 4096;

/// One decoded datagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datagram {
    /// Datagram sequence number; the peer acks this number.
    pub sequence: u16,
    /// Frames, after LZ4 decoding.
    pub body: Vec<u8>,
}

/// Writes a datagram; the body is LZ4 encoded when that makes it shorter.
pub fn encode_datagram(sequence: u16, body: &[u8]) -> Vec<u8> {
    let packed = lz4_flex::block::compress(body);
    let (mode, bytes) = if packed.len() < body.len() {
        (MODE_LZ4, packed.as_slice())
    } else {
        (MODE_PLAIN, body)
    };
    let mut out = Vec::with_capacity(DATAGRAM_HEADER_LEN + bytes.len());
    out.push(mode);
    out.push(HEADER_VERSION);
    out.extend_from_slice(&sequence.to_be_bytes());
    out.extend_from_slice(bytes);
    out
}

/// Reads a datagram header and decodes the body.
///
/// # Errors
///
/// Returns [`CarrierError`] when the header is short or unknown, or when the LZ4 block is
/// invalid or unpacks to more than [`MAX_BODY_LEN`] bytes.
pub fn decode_datagram(bytes: &[u8]) -> Result<Datagram, CarrierError> {
    if bytes.len() < DATAGRAM_HEADER_LEN {
        return Err(CarrierError::Truncated {
            needed: DATAGRAM_HEADER_LEN,
            available: bytes.len(),
        });
    }
    let (header, rest) = bytes.split_at(DATAGRAM_HEADER_LEN);
    if header[1] != HEADER_VERSION {
        return Err(CarrierError::BadHeader {
            mode: header[0],
            version: header[1],
        });
    }
    let sequence = u16::from_be_bytes([header[2], header[3]]);
    let body = match header[0] {
        MODE_PLAIN if rest.len() > MAX_BODY_LEN => return Err(CarrierError::BodyTooLong),
        MODE_PLAIN => rest.to_vec(),
        MODE_LZ4 => {
            let mut out = vec![0_u8; MAX_BODY_LEN];
            let len = lz4_flex::block::decompress_into(rest, &mut out)
                .map_err(|_| CarrierError::BadCompression)?;
            out.truncate(len);
            out
        }
        mode => {
            return Err(CarrierError::BadHeader {
                mode,
                version: header[1],
            });
        }
    };
    Ok(Datagram { sequence, body })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const SEQUENCE: u16 = 0x17AB;
    /// A body that LZ4 makes shorter.
    const REPEATED_BODY: [u8; 64] = [0x20; 64];
    /// A body that LZ4 cannot make shorter.
    const SHORT_BODY: [u8; 3] = [0x20, 0x06, 0x00];
    const UNKNOWN_MODE: u8 = 0x82;

    #[test]
    fn compressible_bodies_use_lz4_mode() {
        let datagram = encode_datagram(SEQUENCE, &REPEATED_BODY);
        assert_eq!(datagram[0], MODE_LZ4);
        assert_eq!(datagram[1], HEADER_VERSION);
        assert_eq!(&datagram[2..DATAGRAM_HEADER_LEN], &SEQUENCE.to_be_bytes());
        assert!(datagram.len() < DATAGRAM_HEADER_LEN + REPEATED_BODY.len());
        let decoded = decode_datagram(&datagram).expect("decode");
        assert_eq!(decoded.sequence, SEQUENCE);
        assert_eq!(decoded.body, REPEATED_BODY);
    }

    #[test]
    fn short_bodies_stay_plain() {
        let datagram = encode_datagram(SEQUENCE, &SHORT_BODY);
        assert_eq!(datagram[0], MODE_PLAIN);
        assert_eq!(&datagram[DATAGRAM_HEADER_LEN..], &SHORT_BODY);
    }

    #[test]
    fn bad_headers_are_errors() {
        assert!(matches!(
            decode_datagram(&[MODE_PLAIN, HEADER_VERSION, 0]),
            Err(CarrierError::Truncated { .. })
        ));
        assert!(matches!(
            decode_datagram(&[UNKNOWN_MODE, HEADER_VERSION, 0, 0]),
            Err(CarrierError::BadHeader { .. })
        ));
        assert!(matches!(
            decode_datagram(&[MODE_PLAIN, 0, 0, 0]),
            Err(CarrierError::BadHeader { .. })
        ));
    }

    #[test]
    fn oversized_plain_bodies_are_an_error() {
        let mut datagram = vec![MODE_PLAIN, HEADER_VERSION, 0, 0];
        datagram.extend(vec![0_u8; MAX_BODY_LEN + 1]);
        assert_eq!(decode_datagram(&datagram), Err(CarrierError::BodyTooLong));
    }

    #[test]
    fn oversized_lz4_output_is_an_error() {
        let big = vec![0_u8; MAX_BODY_LEN + 1];
        let datagram = encode_datagram(SEQUENCE, &big);
        assert_eq!(datagram[0], MODE_LZ4);
        assert_eq!(
            decode_datagram(&datagram),
            Err(CarrierError::BadCompression)
        );
    }

    proptest! {
        #[test]
        fn datagrams_round_trip(sequence in any::<u16>(), body in proptest::collection::vec(any::<u8>(), 0..MAX_DATAGRAM_LEN)) {
            let decoded = decode_datagram(&encode_datagram(sequence, &body)).expect("decode");
            prop_assert_eq!(decoded.sequence, sequence);
            prop_assert_eq!(decoded.body, body);
        }

        #[test]
        fn random_datagrams_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
            let _ = decode_datagram(&bytes);
        }
    }
}
