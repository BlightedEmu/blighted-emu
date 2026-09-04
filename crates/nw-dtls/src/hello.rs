// SPDX-License-Identifier: AGPL-3.0-or-later
//! The first ClientHello of a peer, read without OpenSSL (RFC 6347 sections 4.1, 4.2.1, 4.2.2).
//!
//! The server keeps no state for an address until its ClientHello carries that address's cookie.
//! OpenSSL only starts a session from a ClientHello without a cookie, so the server rebuilds the
//! client's first ClientHello from the second one: the same bytes without the cookie, with
//! message sequence 0 and the record sequence number before the second one.

/// DTLS record header: content type, version, epoch, sequence number, length.
pub(crate) const RECORD_HEADER_LEN: usize = 13;
/// Content type of a handshake record.
pub(crate) const CONTENT_TYPE_HANDSHAKE: u8 = 22;
const EPOCH_OFFSET: usize = 3;
const RECORD_SEQUENCE_OFFSET: usize = 5;
const RECORD_SEQUENCE_LEN: usize = 6;
const RECORD_LENGTH_OFFSET: usize = 11;
/// Handshake header: type, length, message sequence, fragment offset, fragment length.
const HANDSHAKE_HEADER_LEN: usize = 12;
const HANDSHAKE_TYPE_CLIENT_HELLO: u8 = 1;
const HANDSHAKE_LENGTH_OFFSET: usize = 1;
const MESSAGE_SEQUENCE_OFFSET: usize = 4;
const FRAGMENT_OFFSET_OFFSET: usize = 6;
const FRAGMENT_LENGTH_OFFSET: usize = 9;
const U24_LEN: usize = 3;
/// In the ClientHello body: client version (2 bytes) and random (32 bytes), then the session id.
const SESSION_ID_LENGTH_OFFSET: usize = 34;
const BODY_START: usize = RECORD_HEADER_LEN + HANDSHAKE_HEADER_LEN;

/// One whole, unfragmented ClientHello in epoch 0, alone in its datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClientHello<'a> {
    datagram: &'a [u8],
    /// Index of the cookie length byte.
    cookie_length_at: usize,
    record_sequence: u64,
}

impl<'a> ClientHello<'a> {
    /// The ClientHello in `datagram`, or `None` when the datagram is anything else.
    pub(crate) fn parse(datagram: &'a [u8]) -> Option<Self> {
        let record_length = read_be(datagram, RECORD_LENGTH_OFFSET, 2)?;
        let handshake_length = read_be(
            datagram,
            RECORD_HEADER_LEN + HANDSHAKE_LENGTH_OFFSET,
            U24_LEN,
        )?;
        let fragment_offset = read_be(
            datagram,
            RECORD_HEADER_LEN + FRAGMENT_OFFSET_OFFSET,
            U24_LEN,
        )?;
        let fragment_length = read_be(
            datagram,
            RECORD_HEADER_LEN + FRAGMENT_LENGTH_OFFSET,
            U24_LEN,
        )?;
        let whole = datagram.first() == Some(&CONTENT_TYPE_HANDSHAKE)
            && read_be(datagram, EPOCH_OFFSET, 2)? == 0
            && usize::try_from(record_length).ok()? == datagram.len() - RECORD_HEADER_LEN
            && datagram.get(RECORD_HEADER_LEN) == Some(&HANDSHAKE_TYPE_CLIENT_HELLO)
            && fragment_offset == 0
            && fragment_length == handshake_length
            && usize::try_from(handshake_length).ok()? == datagram.len() - BODY_START;
        if !whole {
            return None;
        }
        let session_id_length = usize::from(*datagram.get(BODY_START + SESSION_ID_LENGTH_OFFSET)?);
        let cookie_length_at = BODY_START + SESSION_ID_LENGTH_OFFSET + 1 + session_id_length;
        let cookie_length = usize::from(*datagram.get(cookie_length_at)?);
        if cookie_length_at + cookie_length >= datagram.len() {
            return None;
        }
        Some(Self {
            datagram,
            cookie_length_at,
            record_sequence: read_be(datagram, RECORD_SEQUENCE_OFFSET, RECORD_SEQUENCE_LEN)?,
        })
    }

    /// The cookie; empty in a first ClientHello.
    pub(crate) fn cookie(&self) -> &'a [u8] {
        let start = self.cookie_length_at + 1;
        &self.datagram[start..start + usize::from(self.datagram[self.cookie_length_at])]
    }

    /// The client's first ClientHello: these bytes without the cookie, message sequence 0, and
    /// the record sequence number before this one. `None` when this record has number 0.
    pub(crate) fn without_cookie(&self) -> Option<Vec<u8>> {
        let earlier_sequence = self.record_sequence.checked_sub(1)?;
        let removed = self.cookie().len();
        let start = self.cookie_length_at + 1;
        let mut earlier = Vec::with_capacity(self.datagram.len() - removed);
        earlier.extend_from_slice(&self.datagram[..self.cookie_length_at]);
        earlier.push(0);
        earlier.extend_from_slice(&self.datagram[start + removed..]);
        write_be(
            &mut earlier,
            RECORD_SEQUENCE_OFFSET,
            RECORD_SEQUENCE_LEN,
            earlier_sequence,
        );
        let lengths = [
            (RECORD_LENGTH_OFFSET, 2),
            (RECORD_HEADER_LEN + HANDSHAKE_LENGTH_OFFSET, U24_LEN),
            (RECORD_HEADER_LEN + FRAGMENT_LENGTH_OFFSET, U24_LEN),
        ];
        for (offset, len) in lengths {
            let value = read_be(&earlier, offset, len)?;
            write_be(
                &mut earlier,
                offset,
                len,
                value - u64::try_from(removed).ok()?,
            );
        }
        write_be(
            &mut earlier,
            RECORD_HEADER_LEN + MESSAGE_SEQUENCE_OFFSET,
            2,
            0,
        );
        Some(earlier)
    }
}

/// The big-endian number in `len` bytes at `offset`.
fn read_be(bytes: &[u8], offset: usize, len: usize) -> Option<u64> {
    let field = bytes.get(offset..offset + len)?;
    Some(
        field
            .iter()
            .fold(0, |value, byte| value << 8 | u64::from(*byte)),
    )
}

/// Writes `value` as a big-endian number in `len` bytes at `offset`; `bytes` holds that field.
fn write_be(bytes: &mut [u8], offset: usize, len: usize, value: u64) {
    for (index, byte) in bytes[offset..offset + len].iter_mut().rev().enumerate() {
        *byte = (value >> (8 * index)) as u8;
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::*;
    use crate::{DtlsClientConfig, DtlsServerConfig, DtlsSession, self_signed_identity};

    const MTU: u32 = 1200;
    const VALID_DAYS: u32 = 30;
    const SECRET: [u8; crate::COOKIE_SECRET_LEN] = [4; crate::COOKIE_SECRET_LEN];
    const PEER: &str = "192.0.2.40:40000";

    /// The client's first and second ClientHello.
    fn hellos() -> (Vec<u8>, Vec<u8>) {
        let identity = self_signed_identity("hello", VALID_DAYS).expect("identity");
        let server_config =
            DtlsServerConfig::new(&identity.certificate_pem, &identity.private_key_pem, SECRET)
                .expect("server config");
        let client_config = DtlsClientConfig::new(&identity.certificate_pem).expect("client");
        let mut client = DtlsSession::connect(&client_config, MTU).expect("connect");
        let peer: SocketAddr = PEER.parse().expect("peer");
        let mut server = DtlsSession::accept(&server_config, peer, MTU).expect("accept");
        let first = client.take_transmit().remove(0);
        server.receive(&first).expect("hello");
        for datagram in server.take_transmit() {
            client.receive(&datagram).expect("cookie");
        }
        (first, client.take_transmit().remove(0))
    }

    #[test]
    fn the_second_hello_without_its_cookie_is_the_first_hello() {
        let (first, second) = hellos();
        let first_hello = ClientHello::parse(&first).expect("first");
        let second_hello = ClientHello::parse(&second).expect("second");
        assert!(first_hello.cookie().is_empty());
        assert_eq!(second_hello.cookie().len(), crate::COOKIE_LEN);
        assert_eq!(second_hello.without_cookie(), Some(first));
    }

    #[test]
    fn a_first_record_has_no_earlier_hello() {
        let (first, _) = hellos();
        assert_eq!(
            ClientHello::parse(&first).expect("first").without_cookie(),
            None
        );
    }

    #[test]
    fn anything_else_is_not_a_hello() {
        let (first, _) = hellos();
        let mut other_type = first.clone();
        other_type[0] = CONTENT_TYPE_HANDSHAKE + 1;
        let mut later_epoch = first.clone();
        later_epoch[EPOCH_OFFSET + 1] = 1;
        let mut fragment = first.clone();
        fragment[RECORD_HEADER_LEN + FRAGMENT_OFFSET_OFFSET + 2] = 1;
        let mut two_records = first.clone();
        two_records.extend_from_slice(&first);
        for bytes in [
            &first[..first.len() - 1],
            &other_type[..],
            &later_epoch[..],
            &fragment[..],
            &two_records[..],
            &[][..],
        ] {
            assert_eq!(ClientHello::parse(bytes), None);
        }
    }
}
