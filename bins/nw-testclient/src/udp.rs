// SPDX-License-Identifier: AGPL-3.0-or-later
//! Runs a [`RepClient`] over a UDP socket: registration, then a hold with ping echo, then close.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::net::UdpSocket;

use crate::rep::{RepClient, RepError, RepEvent};

/// Time between polls of the client.
pub const TICK: Duration = Duration::from_millis(10);
/// Largest UDP datagram read.
pub const MAX_UDP_DATAGRAM: usize = 64 * 1024;
/// Any local address and port.
const ANY_LOCAL: &str = "0.0.0.0:0";

/// What happened on the connection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepOutcome {
    /// Registration status, when a response arrived.
    pub status: Option<u32>,
    /// Server version text of the response.
    pub server_version: Option<String>,
    /// Pings echoed during the hold.
    pub pings: u32,
    /// Whether the server closed the connection.
    pub closed_by_server: bool,
}

/// A failed REP run.
#[derive(Debug, Error)]
pub enum RunError {
    /// The UDP socket failed.
    #[error("UDP socket failed: {0}")]
    Socket(#[from] std::io::Error),
    /// The protocol failed.
    #[error(transparent)]
    Rep(#[from] RepError),
}

/// Runs `client` against `gateway` until the server closes, `timeout` passes without a
/// registration response, or `hold` has passed after the response. Then it closes the
/// connection.
///
/// # Errors
///
/// Returns [`RunError`] when the socket or a protocol layer fails.
pub async fn run_rep(
    gateway: SocketAddr,
    mut client: RepClient,
    hold: Duration,
    timeout: Duration,
) -> Result<RepOutcome, RunError> {
    let socket = UdpSocket::bind(ANY_LOCAL).await?;
    let started = Instant::now();
    let now = || u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut outcome = RepOutcome::default();
    let mut registered_at: Option<Instant> = None;
    let mut buffer = vec![0_u8; MAX_UDP_DATAGRAM];
    loop {
        let held = registered_at.is_some_and(|at| at.elapsed() >= hold);
        let waited = registered_at.is_none() && started.elapsed() >= timeout;
        if outcome.closed_by_server || held || waited {
            break;
        }
        for datagram in client.poll(now())? {
            socket.send_to(&datagram, gateway).await?;
        }
        let Ok(received) = tokio::time::timeout(TICK, socket.recv_from(&mut buffer)).await else {
            continue;
        };
        let (len, _) = received?;
        for event in client.receive(now(), &buffer[..len])? {
            match event {
                RepEvent::Connected => {}
                RepEvent::Registered(response) => {
                    outcome.status = Some(response.uint32_at_0x8);
                    outcome.server_version =
                        Some(String::from_utf8_lossy(&response.bytes_at_0x38).into_owned());
                    registered_at = Some(Instant::now());
                }
                RepEvent::Pinged => outcome.pings += 1,
                RepEvent::Closed { .. } => outcome.closed_by_server = true,
            }
        }
    }
    if !outcome.closed_by_server {
        client.close()?;
        for datagram in client.poll(now())? {
            socket.send_to(&datagram, gateway).await?;
        }
    }
    Ok(outcome)
}
