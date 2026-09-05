// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gateway workers on one UDP port (M5 load spec section 3): each worker has its own socket
//! (`SO_REUSEPORT`, the kernel keeps one client on one worker), its own peers, and connection ids
//! that carry its number in the top bits.

use std::net::SocketAddr;

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;

/// Bit position of the worker number in a connection id.
pub const WORKER_SHIFT: u32 = 48;

/// The first connection id of `worker`.
pub const fn first_connection(worker: u16) -> u64 {
    (worker as u64) << WORKER_SHIFT
}

/// The worker of connection id `connection`.
pub const fn worker_of(connection: u64) -> usize {
    (connection >> WORKER_SHIFT) as usize
}

/// One worker's part of a gateway-wide `limit`, rounded up.
pub const fn share(limit: usize, workers: usize) -> usize {
    limit.div_ceil(workers)
}

/// `workers` sockets on `address` (port 0 picks one port for all) with `buffer_bytes` receive
/// and send buffers. More than one worker needs `SO_REUSEPORT`.
///
/// # Errors
///
/// Returns the socket error of the first socket that cannot be set up.
pub fn bind_workers(
    address: SocketAddr,
    workers: usize,
    buffer_bytes: usize,
) -> std::io::Result<Vec<UdpSocket>> {
    let first = bind(address, workers, buffer_bytes)?;
    let shared = first.local_addr()?;
    let mut sockets = vec![first];
    for _ in 1..workers {
        sockets.push(bind(shared, workers, buffer_bytes)?);
    }
    Ok(sockets)
}

fn bind(address: SocketAddr, workers: usize, buffer_bytes: usize) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(
        Domain::for_address(address),
        Type::DGRAM,
        Some(Protocol::UDP),
    )?;
    if workers > 1 {
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
        "more than one gateway worker needs SO_REUSEPORT",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKERS: usize = 4;
    const BUFFER_BYTES: usize = 1 << 20;
    const LOOPBACK: &str = "127.0.0.1:0";

    #[test]
    fn connection_ids_name_their_worker() {
        for worker in [0_u16, 1, 7, u16::MAX] {
            let first = first_connection(worker);
            assert_eq!(worker_of(first), usize::from(worker));
            assert_eq!(worker_of(first + 1_000_000), usize::from(worker));
        }
        assert_ne!(first_connection(0), first_connection(1));
    }

    #[test]
    fn limits_are_shared_out() {
        assert_eq!(share(2000, WORKERS), 500);
        assert_eq!(share(10, WORKERS), 3);
        assert_eq!(share(1, WORKERS), 1);
        assert_eq!(share(7, 1), 7);
    }

    #[tokio::test]
    async fn workers_share_one_port() {
        let address = LOOPBACK.parse().expect("address");
        let sockets = bind_workers(address, WORKERS, BUFFER_BYTES).expect("bind");
        assert_eq!(sockets.len(), WORKERS);
        let port = sockets[0].local_addr().expect("address").port();
        assert_ne!(port, 0);
        assert!(
            sockets
                .iter()
                .all(|socket| socket.local_addr().expect("address").port() == port)
        );
    }
}
