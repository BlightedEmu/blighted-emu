// SPDX-License-Identifier: AGPL-3.0-or-later
//! UDP sockets with chosen buffers (M5 load spec sections 3 and 7.2).

use std::net::SocketAddr;

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;

/// A UDP socket on `address` with `buffer_bytes` receive and send buffers (the kernel may cap
/// them). With `share_port`, other sockets with the flag may bind the same port
/// (`SO_REUSEPORT`); the kernel keeps one remote address on one socket.
///
/// # Errors
///
/// Returns the socket error, also when `share_port` is asked for on a system without
/// `SO_REUSEPORT`.
pub fn bind_udp(
    address: SocketAddr,
    buffer_bytes: usize,
    share_port: bool,
) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(
        Domain::for_address(address),
        Type::DGRAM,
        Some(Protocol::UDP),
    )?;
    if share_port {
        reuse_port(&socket)?;
    }
    socket.set_recv_buffer_size(buffer_bytes)?;
    socket.set_send_buffer_size(buffer_bytes)?;
    socket.set_nonblocking(true)?;
    socket.bind(&address.into())?;
    UdpSocket::from_std(socket.into())
}

#[cfg(unix)]
fn reuse_port(socket: &Socket) -> std::io::Result<()> {
    socket.set_reuse_port(true)
}

#[cfg(not(unix))]
fn reuse_port(_: &Socket) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "sharing a UDP port needs SO_REUSEPORT",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUFFER_BYTES: usize = 1 << 20;
    const LOOPBACK: &str = "127.0.0.1:0";

    #[tokio::test]
    async fn a_socket_gets_its_buffers_and_can_share_a_port() {
        let address = LOOPBACK.parse().expect("address");
        let first = bind_udp(address, BUFFER_BYTES, true).expect("first");
        let shared = first.local_addr().expect("address");
        let second = bind_udp(shared, BUFFER_BYTES, true).expect("second");
        assert_eq!(second.local_addr().expect("address"), shared);
        assert!(
            bind_udp(shared, BUFFER_BYTES, false).is_err(),
            "no sharing without the flag"
        );
    }
}
