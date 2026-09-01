// SPDX-License-Identifier: AGPL-3.0-or-later
//! UDP listen socket and the observed cipher list.
//!
//! Protocol note `docs/re/2026-08-29-client-protocol-login-gridmate.md`, section 1.4:
//! `FUN_145dce750` sets the cipher list to `ECDHE-RSA-AES256-GCM-SHA384` through `FUN_1478efe40`.
//! The same note's implications section says the `gridmate-udp` and `gridmate-udp-bsd` path
//! receives that cipher list and an embedded PEM. This crate does not contain that certificate.
//! Binding a socket does not make the stock client trust a certificate generated elsewhere.

use std::net::{SocketAddr, UdpSocket};

use crate::error::{CarrierError, DATAGRAM_HEADER_UNKNOWN};

/// Cipher list literal from the protocol note.
pub const CIPHER_ECDHE_RSA_AES256_GCM_SHA384: &str = "ECDHE-RSA-AES256-GCM-SHA384";

/// The stock client is not shown to trust a certificate this crate would generate.
/// This crate does not generate a certificate and does not bundle the embedded PEM.
pub const STOCK_CLIENT_TRUSTS_GENERATED_CERTIFICATE: bool = false;

/// UDP socket that can bind.
///
/// [`Self::send_to`] does not send. The datagram header is unknown.
pub struct CarrierSocket {
    socket: UdpSocket,
}

impl CarrierSocket {
    /// Bind a UDP socket. This does not perform a DTLS handshake.
    ///
    /// # Errors
    ///
    /// Returns the operating-system error when the bind fails.
    pub fn bind(addr: SocketAddr) -> std::io::Result<Self> {
        Ok(Self {
            socket: UdpSocket::bind(addr)?,
        })
    }

    /// Local address of the bound socket.
    ///
    /// # Errors
    ///
    /// Returns the operating-system error when the address cannot be read.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Refuse to send a UDP datagram.
    ///
    /// # Errors
    ///
    /// Always returns [`CarrierError::DatagramHeaderUnknown`].
    pub fn send_to(&self, destination: SocketAddr, bytes: &[u8]) -> Result<(), CarrierError> {
        send_udp_datagram(&self.socket, destination, bytes)
    }
}

/// Refuse to send a UDP datagram.
///
/// The protocol note did not find a writer for the 4-byte mode/format/sequence header.
/// `FUN_140f7fc60` copies an existing buffer into the send slot and writes no header byte.
/// This function does not write that header and does not call `send`.
///
/// # Errors
///
/// Always returns [`CarrierError::DatagramHeaderUnknown`].
pub fn send_udp_datagram(
    _socket: &UdpSocket,
    _destination: SocketAddr,
    _bytes: &[u8],
) -> Result<(), CarrierError> {
    Err(DATAGRAM_HEADER_UNKNOWN)
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::time::Duration;

    use super::*;

    const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;
    const EPHEMERAL_PORT: u16 = 0;
    const RECV_TIMEOUT_MS: u64 = 50;
    const SAMPLE_PAYLOAD: [u8; 3] = [1, 2, 3];
    const OBSERVED_CIPHER: &str = "ECDHE-RSA-AES256-GCM-SHA384";
    const DATAGRAM_HEADER_UNKNOWN_TEXT: &str = "DATAGRAM_HEADER_UNKNOWN";

    #[test]
    fn cipher_constant_matches_the_note() {
        assert_eq!(CIPHER_ECDHE_RSA_AES256_GCM_SHA384, OBSERVED_CIPHER);
        const {
            assert!(!STOCK_CLIENT_TRUSTS_GENERATED_CERTIFICATE);
        };
    }

    #[test]
    fn bind_listens_and_send_is_refused() {
        let listener = CarrierSocket::bind(SocketAddr::from((LOOPBACK, EPHEMERAL_PORT)))
            .expect("bind listener");
        let sender =
            CarrierSocket::bind(SocketAddr::from((LOOPBACK, EPHEMERAL_PORT))).expect("bind sender");
        let destination = listener.local_addr().expect("listener address");
        assert_ne!(destination.port(), EPHEMERAL_PORT);

        let error = sender
            .send_to(destination, &SAMPLE_PAYLOAD)
            .expect_err("send must fail");
        assert!(error.is_datagram_header_unknown());
        assert_eq!(error, DATAGRAM_HEADER_UNKNOWN);
        assert_eq!(error.to_string(), DATAGRAM_HEADER_UNKNOWN_TEXT);
        assert_eq!(
            send_udp_datagram(&sender.socket, destination, &[]).expect_err("empty send must fail"),
            DATAGRAM_HEADER_UNKNOWN
        );

        listener
            .socket
            .set_read_timeout(Some(Duration::from_millis(RECV_TIMEOUT_MS)))
            .expect("set timeout");
        let mut buffer = [0_u8; SAMPLE_PAYLOAD.len()];
        assert!(listener.socket.recv(&mut buffer).is_err());
    }
}
