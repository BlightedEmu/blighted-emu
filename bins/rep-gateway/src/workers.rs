// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gateway workers on one UDP port (M5 load spec section 3): each worker has its own socket
//! (`SO_REUSEPORT`, the kernel keeps one client on one worker), its own peers, and connection ids
//! that carry its number in the top bits.

use std::net::SocketAddr;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nw_common::net::bind_udp;
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

/// The open connections of every worker of a gateway, below one limit. The kernel does not
/// spread clients evenly over the workers, so the limit is shared, not cut in parts.
#[derive(Debug, Clone)]
pub struct ConnectionCount {
    live: Arc<AtomicUsize>,
    max: usize,
}

impl ConnectionCount {
    /// No connection yet, at most `max`.
    pub fn new(max: usize) -> Self {
        Self {
            live: Arc::default(),
            max,
        }
    }

    /// Takes a place for one more connection; `false` when all places are taken.
    pub fn try_add(&self) -> bool {
        self.live
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |live| {
                (live < self.max).then_some(live + 1)
            })
            .is_ok()
    }

    /// Gives a place back.
    pub fn remove(&self) {
        // The count never goes below 0: a place is given back only after it was taken.
        let _ = self
            .live
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |live| {
                live.checked_sub(1)
            });
    }

    /// Open connections of every worker.
    pub fn live(&self) -> usize {
        self.live.load(Ordering::Acquire)
    }
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
    let share_port = workers > 1;
    let first = bind_udp(address, buffer_bytes, share_port)?;
    let shared = first.local_addr()?;
    let mut sockets = vec![first];
    for _ in 1..workers {
        sockets.push(bind_udp(shared, buffer_bytes, share_port)?);
    }
    Ok(sockets)
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

    #[test]
    fn the_connection_limit_counts_every_worker() {
        let first = ConnectionCount::new(2);
        let second = first.clone();
        assert!(first.try_add());
        assert!(second.try_add());
        assert!(!first.try_add(), "two connections in all");
        assert!(!second.try_add());
        second.remove();
        assert!(first.try_add());
        assert_eq!(first.live(), 2);
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
