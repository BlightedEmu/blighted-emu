// SPDX-License-Identifier: AGPL-3.0-or-later
//! The UDP loop (M3 spec sections 3.1 and 6): one socket, one [`Peer`] per remote address, the
//! connection limits, and the ticket checks.
//!
//! One task owns every peer. Ticket checks run as their own tasks and send their result back, so
//! a slow store never stops the loop. Every [`TICK`] the loop polls each peer for timers, pings,
//! and resends.

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nw_dtls::DtlsServerConfig;
use nw_login::queue::{EntryRecord, QueueStore};
use nw_login::secrets::SigningKey;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::GatewayLimits;
use crate::peer::{Peer, PeerError, PeerEvent, PeerSettings};
use crate::registration::{Refusal, TicketClaims, check_ticket};

/// Time between polls of every peer.
pub const TICK: Duration = Duration::from_millis(10);
/// Largest UDP datagram the loop reads.
pub const MAX_UDP_DATAGRAM: usize = 64 * 1024;
/// Ticket results that may wait for the loop.
pub const RESULT_QUEUE: usize = 1024;
/// Length of the per-address handshake window.
pub const HANDSHAKE_WINDOW: Duration = Duration::from_secs(1);
/// IPv6 addresses are counted per /64: the first 4 of 8 segments.
const IPV6_PREFIX_SEGMENTS: usize = 4;

/// Metric: open connections.
pub const METRIC_CONNECTIONS: &str = "rep_gateway_connections";
/// Metric: DTLS handshakes in progress.
pub const METRIC_HANDSHAKES: &str = "rep_gateway_handshakes";
/// Metric: registrations by `result`.
pub const METRIC_REGISTRATIONS: &str = "rep_gateway_registrations_total";
/// Metric: dropped connections and datagrams by `reason`.
pub const METRIC_DROPPED: &str = "rep_gateway_dropped_total";
/// Metric: received datagrams.
pub const METRIC_DATAGRAMS_IN: &str = "rep_gateway_datagrams_in_total";
/// Metric: sent datagrams.
pub const METRIC_DATAGRAMS_OUT: &str = "rep_gateway_datagrams_out_total";
const RESULT_LABEL: &str = "result";
const REASON_LABEL: &str = "reason";
const ACCEPTED: &str = "accepted";

/// What the loop needs besides the socket.
pub struct Gateway<Q> {
    /// DTLS identity and cookie secret.
    pub dtls: Arc<DtlsServerConfig>,
    /// Entry records.
    pub store: Q,
    /// Entry ticket key, shared with `edge-http`.
    pub signing_key: Arc<SigningKey>,
    /// World this gateway serves.
    pub world_id: Uuid,
    /// Server version text.
    pub server_version: String,
    /// Settings of one connection.
    pub peer_settings: PeerSettings,
    /// Gateway-wide limits.
    pub limits: GatewayLimits,
}

type TicketResult = (SocketAddr, Result<EntryRecord, Refusal>);

struct Loop<Q> {
    gateway: Gateway<Q>,
    socket: UdpSocket,
    peers: HashMap<SocketAddr, Peer>,
    handshake_windows: HashMap<IpAddr, (Instant, u32)>,
    results: mpsc::Sender<TicketResult>,
    started: Instant,
}

/// Serves `socket` until `shutdown` finishes.
///
/// # Errors
///
/// Returns the socket's error when receiving fails.
pub async fn serve<Q: QueueStore>(
    gateway: Gateway<Q>,
    socket: UdpSocket,
    shutdown: impl Future<Output = ()>,
) -> std::io::Result<()> {
    let (results, mut incoming) = mpsc::channel(RESULT_QUEUE);
    let mut state = Loop {
        gateway,
        socket,
        peers: HashMap::new(),
        handshake_windows: HashMap::new(),
        results,
        started: Instant::now(),
    };
    let mut buffer = vec![0_u8; MAX_UDP_DATAGRAM];
    let mut tick = tokio::time::interval(TICK);
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => return Ok(()),
            received = state.socket.recv_from(&mut buffer) => {
                let (len, address) = received?;
                state.datagram(address, &buffer[..len]).await;
            }
            Some((address, result)) = incoming.recv() => state.ticket_result(address, result).await,
            _ = tick.tick() => state.poll_all().await,
        }
    }
}

impl<Q: QueueStore> Loop<Q> {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    async fn datagram(&mut self, address: SocketAddr, bytes: &[u8]) {
        metrics::counter!(METRIC_DATAGRAMS_IN).increment(1);
        let now = self.now_ms();
        if !self.peers.contains_key(&address) {
            let Some(peer) = self.admit(address, now) else {
                return;
            };
            self.peers.insert(address, peer);
        }
        let Some(peer) = self.peers.get_mut(&address) else {
            return;
        };
        match peer.receive(now, bytes) {
            Ok(events) => {
                for PeerEvent::RegistrationRequested(claims) in events {
                    self.check(address, claims);
                }
                self.flush(address, now).await;
            }
            Err(error) => self.drop_peer(address, &error),
        }
        self.report();
    }

    /// A new peer for `address`, or `None` when a limit refuses it.
    fn admit(&mut self, address: SocketAddr, now: u64) -> Option<Peer> {
        let limits = self.gateway.limits;
        let handshakes = self
            .peers
            .values()
            .filter(|peer| !peer.is_established())
            .count();
        let refused = if self.peers.len() >= limits.max_connections {
            Some("max_connections")
        } else if handshakes >= limits.max_handshakes {
            Some("max_handshakes")
        } else if !self.handshake_allowed(address.ip()) {
            Some("handshake_rate")
        } else {
            None
        };
        if let Some(reason) = refused {
            metrics::counter!(METRIC_DROPPED, REASON_LABEL => reason).increment(1);
            return None;
        }
        Peer::new(&self.gateway.dtls, address, self.gateway.peer_settings, now)
            .inspect_err(|error| tracing::warn!(%error, "cannot start a connection"))
            .ok()
    }

    fn handshake_allowed(&mut self, ip: IpAddr) -> bool {
        let key = rate_key(ip);
        let now = Instant::now();
        self.handshake_windows
            .retain(|_, (start, _)| now.duration_since(*start) < HANDSHAKE_WINDOW);
        let (_, count) = self.handshake_windows.entry(key).or_insert((now, 0));
        *count += 1;
        *count <= self.gateway.limits.handshakes_per_ip_per_second
    }

    fn check(&self, address: SocketAddr, claims: TicketClaims) {
        let store = self.gateway.store.clone();
        let key = Arc::clone(&self.gateway.signing_key);
        let world = self.gateway.world_id;
        let results = self.results.clone();
        tokio::spawn(async move {
            let result = check_ticket(&store, &key, world, &claims, unix_now_seconds()).await;
            match &result {
                Ok(record) => tracing::info!(
                    account = %record.account_id,
                    character = %record.character_id,
                    "registration accepted"
                ),
                Err(refusal) => tracing::warn!(
                    ticket = %claims.masked_ticket(),
                    reason = refusal.reason(),
                    "registration refused"
                ),
            }
            // The loop may have stopped; then nobody waits for the result.
            let _ = results.send((address, result)).await;
        });
    }

    async fn ticket_result(&mut self, address: SocketAddr, result: Result<EntryRecord, Refusal>) {
        let label = result
            .as_ref()
            .map_or_else(|refusal| refusal.reason(), |_| ACCEPTED);
        metrics::counter!(METRIC_REGISTRATIONS, RESULT_LABEL => label).increment(1);
        let Some(peer) = self.peers.get_mut(&address) else {
            return;
        };
        let completed = peer.complete_registration(
            SystemTime::now(),
            result.as_ref().map_err(|refusal| *refusal),
            &self.gateway.server_version,
        );
        match completed {
            Ok(()) => self.flush(address, self.now_ms()).await,
            Err(error) => self.drop_peer(address, &error),
        }
    }

    async fn poll_all(&mut self) {
        let addresses: Vec<SocketAddr> = self.peers.keys().copied().collect();
        let now = self.now_ms();
        for address in addresses {
            self.flush(address, now).await;
        }
        self.report();
    }

    /// Sends what a peer has to send, and removes it when it is over.
    async fn flush(&mut self, address: SocketAddr, now: u64) {
        let Some(peer) = self.peers.get_mut(&address) else {
            return;
        };
        match peer.poll(now) {
            Ok(datagrams) => {
                for datagram in datagrams {
                    metrics::counter!(METRIC_DATAGRAMS_OUT).increment(1);
                    if let Err(error) = self.socket.send_to(&datagram, address).await {
                        tracing::debug!(%error, %address, "send failed");
                    }
                }
                if peer.is_finished() {
                    self.peers.remove(&address);
                }
            }
            Err(error) => self.drop_peer(address, &error),
        }
    }

    fn drop_peer(&mut self, address: SocketAddr, error: &PeerError) {
        tracing::debug!(%error, %address, "connection dropped");
        metrics::counter!(METRIC_DROPPED, REASON_LABEL => error.reason()).increment(1);
        self.peers.remove(&address);
    }

    fn report(&self) {
        let handshakes = self
            .peers
            .values()
            .filter(|peer| !peer.is_established())
            .count();
        metrics::gauge!(METRIC_CONNECTIONS).set(self.peers.len() as f64);
        metrics::gauge!(METRIC_HANDSHAKES).set(handshakes as f64);
    }
}

/// The address an IP counts against: itself for IPv4, its /64 for IPv6.
fn rate_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => {
            let mut segments = v6.segments();
            segments[IPV6_PREFIX_SEGMENTS..].fill(0);
            IpAddr::V6(segments.into())
        }
    }
}

fn unix_now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::LazyLock;

    use nw_dtls::{COOKIE_SECRET_LEN, DtlsClientConfig, Identity, self_signed_identity};
    use nw_login::signature::sign_entry;
    use nw_login::test_support::test_signing_key;
    use nw_reflect::LoginToken;
    use nw_testclient::rep::RepClient;
    use nw_testclient::udp::{RepOutcome, run_rep};

    use super::*;
    use crate::registration::tests::Records;
    use nw_reflect::{REGISTRATION_STATUS_AUTHORIZED, REGISTRATION_STATUS_REFUSED};

    const SECRET: [u8; COOKIE_SECRET_LEN] = [5; COOKIE_SECRET_LEN];
    const MTU: u32 = 1200;
    pub(crate) const VERSION: &str = "[TEST].Javelin.1.365.6031.1";
    const VALID_DAYS: u32 = 30;
    const LIFETIME_SECONDS: i64 = 60;
    pub(crate) const WAIT: Duration = Duration::from_secs(5);
    /// Long enough for two pings 200 ms apart.
    pub(crate) const HOLD: Duration = Duration::from_millis(500);
    /// The first client stays connected while the second one tries.
    const HOLD_FIRST: Duration = Duration::from_secs(2);
    /// Time for the first client to register before the second one starts.
    const SETTLE: Duration = Duration::from_millis(500);
    /// The second client gives up after this time without a reply.
    const REFUSED_WAIT: Duration = Duration::from_secs(1);
    const LOOPBACK: &str = "127.0.0.1:0";
    pub(crate) const LIMITS: GatewayLimits = GatewayLimits {
        max_connections: 16,
        max_handshakes: 16,
        handshakes_per_ip_per_second: 16,
        registration_timeout_ms: 10_000,
        silence_timeout_ms: 10_000,
        datagrams_per_connection_per_second: 400,
        ping_interval_ms: 200,
    };
    static IDENTITY: LazyLock<Identity> =
        LazyLock::new(|| self_signed_identity("test-gateway", VALID_DAYS).expect("identity"));

    /// Starts a gateway for `world_id` with `store` on a loopback port; returns its address.
    pub(crate) async fn spawn_gateway<Q: QueueStore>(
        store: Q,
        world_id: Uuid,
        limits: GatewayLimits,
    ) -> SocketAddr {
        let gateway = Gateway {
            dtls: Arc::new(
                DtlsServerConfig::new(&IDENTITY.certificate_pem, &IDENTITY.private_key_pem, SECRET)
                    .expect("dtls"),
            ),
            store,
            signing_key: Arc::new(test_signing_key()),
            world_id,
            server_version: VERSION.to_owned(),
            peer_settings: PeerSettings {
                mtu: MTU,
                registration_timeout_ms: limits.registration_timeout_ms,
                silence_timeout_ms: limits.silence_timeout_ms,
                ping_interval_ms: limits.ping_interval_ms,
                datagrams_per_second: limits.datagrams_per_connection_per_second,
            },
            limits,
        };
        let socket = UdpSocket::bind(LOOPBACK).await.expect("bind");
        let address = socket.local_addr().expect("address");
        tokio::spawn(serve(gateway, socket, std::future::pending()));
        address
    }

    /// Starts a gateway with one live ticket; returns its address and the token a client
    /// registers with.
    async fn start(limits: GatewayLimits) -> (SocketAddr, LoginToken) {
        let world_id = Uuid::new_v4();
        let record = EntryRecord {
            ticket_id: Uuid::new_v4().to_string(),
            account_id: Uuid::new_v4(),
            world_id,
            character_id: Uuid::new_v4(),
            expires_at: unix_now_seconds() + LIFETIME_SECONDS,
        };
        let token = LoginToken {
            ticket_id: record.ticket_id.clone().into_bytes(),
            signature: sign_entry(&test_signing_key(), &record.claims()).into_bytes(),
            character_id: record.character_id.to_string().into_bytes(),
            ..LoginToken::default()
        };
        let store = Records::default();
        store
            .records
            .lock()
            .expect("lock")
            .insert(record.ticket_id.clone(), record);
        (spawn_gateway(store, world_id, limits).await, token)
    }

    /// A client that trusts the test gateway.
    pub(crate) fn client(token: LoginToken) -> RepClient {
        let config = DtlsClientConfig::new(&IDENTITY.certificate_pem).expect("client config");
        RepClient::new(&config, MTU, token, 0).expect("client")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_client_registers_once_over_udp_and_gets_pings() {
        let (gateway, token) = start(LIMITS).await;
        let outcome = run_rep(gateway, client(token.clone()), HOLD, WAIT)
            .await
            .expect("run");
        assert_eq!(outcome.status, Some(REGISTRATION_STATUS_AUTHORIZED));
        assert_eq!(outcome.server_version.as_deref(), Some(VERSION));
        assert!(outcome.pings > 0);
        let again = run_rep(gateway, client(token), HOLD, WAIT)
            .await
            .expect("again");
        assert_eq!(again.status, Some(REGISTRATION_STATUS_REFUSED));
        assert!(again.closed_by_server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_connection_limit_refuses_new_peers() {
        let one = GatewayLimits {
            max_connections: 1,
            ..LIMITS
        };
        let (gateway, token) = start(one).await;
        let first = tokio::spawn(run_rep(gateway, client(token.clone()), HOLD_FIRST, WAIT));
        tokio::time::sleep(SETTLE).await;
        let second = run_rep(gateway, client(token), HOLD, REFUSED_WAIT)
            .await
            .expect("second");
        assert_eq!(second, RepOutcome::default());
        let first = first.await.expect("join").expect("first");
        assert_eq!(first.status, Some(REGISTRATION_STATUS_AUTHORIZED));
    }

    #[test]
    fn ipv6_addresses_count_per_64() {
        let one: IpAddr = "2001:db8:1:2:aaaa::1".parse().expect("ip");
        let two: IpAddr = "2001:db8:1:2:bbbb::2".parse().expect("ip");
        let other: IpAddr = "2001:db8:1:3::1".parse().expect("ip");
        assert_eq!(rate_key(one), rate_key(two));
        assert_ne!(rate_key(one), rate_key(other));
        let v4: IpAddr = "192.0.2.1".parse().expect("ip");
        assert_eq!(rate_key(v4), v4);
    }
}
