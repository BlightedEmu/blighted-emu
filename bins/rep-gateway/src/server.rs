// SPDX-License-Identifier: AGPL-3.0-or-later
//! The UDP loop (M3 spec sections 3.1 and 6, M4 spec section 6): one socket, one [`Peer`] per
//! remote address, the connection limits, the ticket checks, and the forwarding to the world.
//!
//! One task owns every peer. Ticket checks and world joins run as their own tasks and send their
//! result back, so a slow store or world never stops the loop. Every [`TICK`] the loop polls each
//! peer for timers, pings, and resends. World events arrive on their own channel.

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use nw_common::time::unix_seconds;
use nw_dtls::{DtlsServerConfig, FirstDatagram};
use nw_login::queue::{EntryRecord, QueueStore};
use nw_login::secrets::SigningKey;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::GatewayLimits;
use crate::peer::{Peer, PeerError, PeerEvent, PeerSettings};
use crate::registration::{Refusal, TicketClaims, check_ticket};
use crate::world::{World, WorldEvent};

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
/// Drop reason: a first datagram that is not a ClientHello, or has a cookie for another address.
const NO_COOKIE: &str = "no_cookie";
/// Drop reason: receiving one datagram failed.
const RECEIVE: &str = "receive";
/// Drop reason: OpenSSL refused a first ClientHello.
const DTLS: &str = "dtls";

/// What the loop needs besides the socket and the world events.
pub struct Gateway<Q, W> {
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
    /// The world the players join.
    pub world: W,
}

/// A ticket check result for one connection: address, connection id, result.
type TicketResult = (SocketAddr, u64, Result<EntryRecord, Refusal>);

struct Loop<Q, W> {
    gateway: Gateway<Q, W>,
    socket: UdpSocket,
    peers: HashMap<SocketAddr, Peer>,
    /// The address of each connection id, for world events.
    connections: HashMap<u64, SocketAddr>,
    handshake_windows: HashMap<IpAddr, (Instant, u32)>,
    results: mpsc::Sender<TicketResult>,
    started: Instant,
    /// Id of the next connection; a ticket result goes only to the connection that asked.
    next_connection: u64,
}

/// Serves `socket` and the world `events` until `shutdown` finishes. An error that belongs to one
/// datagram (for example an ICMP "port unreachable" on Windows) is logged and the loop goes on.
///
/// # Errors
///
/// Returns the socket's error when the socket itself fails.
pub async fn serve<Q: QueueStore, W: World>(
    gateway: Gateway<Q, W>,
    socket: UdpSocket,
    mut events: mpsc::Receiver<WorldEvent>,
    shutdown: impl Future<Output = ()>,
) -> std::io::Result<()> {
    let (results, mut incoming) = mpsc::channel(RESULT_QUEUE);
    let mut state = Loop {
        gateway,
        socket,
        peers: HashMap::new(),
        connections: HashMap::new(),
        handshake_windows: HashMap::new(),
        results,
        started: Instant::now(),
        next_connection: 0,
    };
    let mut buffer = vec![0_u8; MAX_UDP_DATAGRAM];
    let mut tick = tokio::time::interval(TICK);
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => return Ok(()),
            received = state.socket.recv_from(&mut buffer) => match received {
                Ok((len, address)) => state.datagram(address, &buffer[..len]).await,
                Err(error) if is_datagram_error(&error) => {
                    tracing::debug!(%error, "receive failed for one datagram");
                    metrics::counter!(METRIC_DROPPED, REASON_LABEL => RECEIVE).increment(1);
                }
                Err(error) => return Err(error),
            },
            Some((address, connection, result)) = incoming.recv() => {
                state.ticket_result(address, connection, result).await;
            }
            Some(event) = events.recv() => state.world_event(event).await,
            _ = tick.tick() => state.poll_all().await,
        }
    }
}

impl<Q: QueueStore, W: World> Loop<Q, W> {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    async fn datagram(&mut self, address: SocketAddr, bytes: &[u8]) {
        metrics::counter!(METRIC_DATAGRAMS_IN).increment(1);
        let now = self.now_ms();
        let Some(peer) = self.peers.get_mut(&address) else {
            self.first_datagram(address, bytes, now).await;
            return;
        };
        match peer.receive(now, bytes) {
            Ok(events) => {
                let connection = peer.id();
                for event in events {
                    match event {
                        PeerEvent::RegistrationRequested(claims) => {
                            self.check(address, connection, claims);
                        }
                        PeerEvent::ClientMessage(envelope) => {
                            self.gateway.world.client_message(connection, envelope);
                        }
                    }
                }
                self.flush(address, now).await;
            }
            Err(error) => self.drop_peer(address, &error),
        }
        self.report();
    }

    /// The first datagram from an address without a peer. The DTLS cookie keeps spoofed
    /// addresses from costing more than one reply; the limits count only real addresses, and
    /// they run before the key exchange, so a replayed cookie costs no signature.
    async fn first_datagram(&mut self, address: SocketAddr, bytes: &[u8], now: u64) {
        let mtu = self.gateway.peer_settings.mtu;
        let hello = match self.gateway.dtls.first_datagram(address, mtu, bytes) {
            Ok(FirstDatagram::Reply(datagrams)) => {
                for datagram in datagrams {
                    send(&self.socket, address, &datagram).await;
                }
                return;
            }
            Ok(FirstDatagram::Verified(hello)) => hello,
            Ok(FirstDatagram::Ignore) => {
                metrics::counter!(METRIC_DROPPED, REASON_LABEL => NO_COOKIE).increment(1);
                return;
            }
            Err(error) => {
                tracing::debug!(%error, %address, "first datagram refused");
                metrics::counter!(METRIC_DROPPED, REASON_LABEL => DTLS).increment(1);
                return;
            }
        };
        if let Some(reason) = self.refusal(address) {
            metrics::counter!(METRIC_DROPPED, REASON_LABEL => reason).increment(1);
            return;
        }
        let session = match hello.start(&self.gateway.dtls, mtu) {
            Ok(session) => session,
            Err(error) => {
                tracing::debug!(%error, %address, "handshake refused");
                metrics::counter!(METRIC_DROPPED, REASON_LABEL => DTLS).increment(1);
                return;
            }
        };
        self.peers.insert(
            address,
            Peer::new(
                self.next_connection,
                session,
                self.gateway.peer_settings,
                now,
            ),
        );
        self.connections.insert(self.next_connection, address);
        self.next_connection += 1;
        self.flush(address, now).await;
        self.report();
    }

    /// The limit that refuses a new peer at `address`, if any.
    fn refusal(&mut self, address: SocketAddr) -> Option<&'static str> {
        let limits = self.gateway.limits;
        let handshakes = self
            .peers
            .values()
            .filter(|peer| !peer.is_established())
            .count();
        if self.peers.len() >= limits.max_connections {
            Some("max_connections")
        } else if handshakes >= limits.max_handshakes {
            Some("max_handshakes")
        } else if !self.handshake_allowed(address.ip()) {
            Some("handshake_rate")
        } else {
            None
        }
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

    /// Checks the ticket, then asks the world to take the character.
    fn check(&self, address: SocketAddr, connection: u64, claims: TicketClaims) {
        let store = self.gateway.store.clone();
        let key = Arc::clone(&self.gateway.signing_key);
        let world_id = self.gateway.world_id;
        let world = self.gateway.world.clone();
        let results = self.results.clone();
        tokio::spawn(async move {
            let result = match check_ticket(&store, &key, world_id, &claims, unix_seconds()).await {
                Ok(record) => world.join(connection, &record).await.map(|()| record),
                Err(refusal) => Err(refusal),
            };
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
            let _ = results.send((address, connection, result)).await;
        });
    }

    async fn ticket_result(
        &mut self,
        address: SocketAddr,
        connection: u64,
        result: Result<EntryRecord, Refusal>,
    ) {
        let label = result
            .as_ref()
            .map_or_else(|refusal| refusal.reason(), |_| ACCEPTED);
        metrics::counter!(METRIC_REGISTRATIONS, RESULT_LABEL => label).increment(1);
        let Some(peer) = self
            .peers
            .get_mut(&address)
            .filter(|peer| peer.id() == connection)
        else {
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
                    send(&self.socket, address, &datagram).await;
                }
                if peer.is_finished() {
                    self.remove_peer(address);
                }
            }
            Err(error) => self.drop_peer(address, &error),
        }
    }

    fn drop_peer(&mut self, address: SocketAddr, error: &PeerError) {
        tracing::debug!(%error, %address, "connection dropped");
        metrics::counter!(METRIC_DROPPED, REASON_LABEL => error.reason()).increment(1);
        self.remove_peer(address);
    }

    /// Removes a peer; the world hears that a registered player left.
    fn remove_peer(&mut self, address: SocketAddr) {
        if let Some(peer) = self.peers.remove(&address) {
            self.connections.remove(&peer.id());
            if peer.is_registered() {
                self.gateway.world.left(peer.id());
            }
        }
    }

    async fn world_event(&mut self, event: WorldEvent) {
        match event {
            WorldEvent::Messages {
                sim,
                connection,
                channel,
                reliable,
                envelopes,
            } => {
                let known = self
                    .apply(connection, |peer| {
                        peer.send_envelopes(channel, reliable, &envelopes)
                    })
                    .await;
                if !known {
                    // The sim still has a player on a connection this gateway no longer has (for
                    // example after a join that timed out); it hears that the player left.
                    tracing::debug!(connection, "world message for a connection that is gone");
                    self.gateway.world.unknown(connection, &sim);
                }
            }
            WorldEvent::Close { connection } => {
                self.apply(connection, Peer::close).await;
            }
        }
    }

    /// Runs `action` on the peer of `connection` and sends what it queued. Returns `false` when
    /// this gateway has no such connection.
    async fn apply(
        &mut self,
        connection: u64,
        action: impl FnOnce(&mut Peer) -> Result<(), PeerError>,
    ) -> bool {
        let Some(&address) = self.connections.get(&connection) else {
            return false;
        };
        let Some(peer) = self.peers.get_mut(&address) else {
            return false;
        };
        match action(peer) {
            Ok(()) => self.flush(address, self.now_ms()).await,
            Err(error) => self.drop_peer(address, &error),
        }
        true
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

async fn send(socket: &UdpSocket, address: SocketAddr, datagram: &[u8]) {
    metrics::counter!(METRIC_DATAGRAMS_OUT).increment(1);
    if let Err(error) = socket.send_to(datagram, address).await {
        tracing::debug!(%error, %address, "send failed");
    }
}

/// Whether a receive error belongs to one datagram, so the socket still works.
fn is_datagram_error(error: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    matches!(
        error.kind(),
        ErrorKind::ConnectionReset
            | ErrorKind::ConnectionRefused
            | ErrorKind::Interrupted
            | ErrorKind::WouldBlock
    )
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

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::LazyLock;

    use nw_dtls::{
        COOKIE_SECRET_LEN, DtlsClientConfig, DtlsSession, Identity, self_signed_identity,
    };
    use nw_login::signature::sign_entry;
    use nw_login::test_support::test_signing_key;
    use nw_reflect::LoginToken;
    use nw_testclient::rep::RepClient;
    use nw_testclient::udp::{RepOutcome, run_rep};

    use nw_carrier::MESSAGE_CHANNEL;
    use nw_reflect::{Envelope, TypeId};

    use super::*;
    use crate::registration::tests::Records;
    use crate::world::tests::{Sent, TestWorld, test_sim};
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
    /// Shorter than the first DTLS resend (1 s), so a refused client tries one time only.
    const ONE_HELLO_WAIT: Duration = Duration::from_millis(700);
    /// The second client gives up after this time without a reply.
    const REFUSED_WAIT: Duration = Duration::from_secs(1);
    const LOOPBACK: &str = "127.0.0.1:0";
    /// ClientHellos from one address that never answers the cookie.
    const SPOOFED_HELLOS: usize = 3;
    /// A login message (type 446).
    const LOGIN_TYPE_INDEX: u64 = 446;
    const LOGIN_BODY: [u8; 2] = [1, 2];
    /// A connection id the gateway never gave out.
    const UNKNOWN_CONNECTION: u64 = 999_999;
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

    /// Starts a gateway for `world_id` with `store` and `world` on a loopback port; returns its
    /// address.
    pub(crate) async fn spawn_gateway<Q: QueueStore, W: World>(
        store: Q,
        world_id: Uuid,
        limits: GatewayLimits,
        world: W,
        events: mpsc::Receiver<WorldEvent>,
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
            world,
        };
        let socket = UdpSocket::bind(LOOPBACK).await.expect("bind");
        let address = socket.local_addr().expect("address");
        tokio::spawn(serve(gateway, socket, events, std::future::pending()));
        address
    }

    /// A world that takes every character and sends no login messages.
    pub(crate) fn accepting() -> (TestWorld, mpsc::Receiver<WorldEvent>) {
        TestWorld::new(false, Vec::new())
    }

    /// Starts a gateway with `world` and one live ticket; returns its address and the token a
    /// client registers with.
    async fn start_with(
        limits: GatewayLimits,
        (world, events): (TestWorld, mpsc::Receiver<WorldEvent>),
    ) -> (SocketAddr, LoginToken) {
        let world_id = Uuid::new_v4();
        let record = EntryRecord {
            ticket_id: Uuid::new_v4().to_string(),
            account_id: Uuid::new_v4(),
            world_id,
            character_id: Uuid::new_v4(),
            expires_at: unix_seconds() + LIFETIME_SECONDS,
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
        (
            spawn_gateway(store, world_id, limits, world, events).await,
            token,
        )
    }

    /// [`start_with`] an accepting world.
    async fn start(limits: GatewayLimits) -> (SocketAddr, LoginToken) {
        start_with(limits, accepting()).await
    }

    /// Waits until `done` is true.
    async fn wait_for(done: impl Fn() -> bool) {
        let deadline = Instant::now() + WAIT;
        while !done() {
            assert!(Instant::now() < deadline, "condition not met in time");
            tokio::time::sleep(TICK).await;
        }
    }

    /// The connection of the first join.
    async fn first_join(world: &TestWorld) -> u64 {
        wait_for(|| !world.joins.lock().expect("lock").is_empty()).await;
        world.joins.lock().expect("lock")[0]
    }

    fn sent(world: &TestWorld) -> Vec<Sent> {
        world.sent.lock().expect("lock").clone()
    }

    fn login_envelope() -> Vec<u8> {
        Envelope::message(TypeId::FileSlot(LOGIN_TYPE_INDEX), LOGIN_BODY.to_vec())
            .encode()
            .expect("envelope")
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

    #[tokio::test(flavor = "multi_thread")]
    async fn the_handshake_rate_per_address_refuses_new_peers() {
        let one = GatewayLimits {
            handshakes_per_ip_per_second: 1,
            ..LIMITS
        };
        let (gateway, token) = start(one).await;
        let first = run_rep(gateway, client(token.clone()), HOLD, WAIT)
            .await
            .expect("first");
        assert_eq!(first.status, Some(REGISTRATION_STATUS_AUTHORIZED));
        let second = run_rep(gateway, client(token), HOLD, ONE_HELLO_WAIT)
            .await
            .expect("second");
        assert_eq!(second, RepOutcome::default());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hellos_without_a_cookie_take_no_handshake_slot() {
        let one = GatewayLimits {
            max_handshakes: 1,
            ..LIMITS
        };
        let (gateway, token) = start(one).await;
        let hello = DtlsSession::connect(
            &DtlsClientConfig::new(&IDENTITY.certificate_pem).expect("client config"),
            MTU,
        )
        .expect("connect")
        .take_transmit();
        let spoofer = UdpSocket::bind(LOOPBACK).await.expect("bind");
        for _ in 0..SPOOFED_HELLOS {
            for datagram in &hello {
                spoofer.send_to(datagram, gateway).await.expect("send");
            }
        }
        let outcome = run_rep(gateway, client(token), HOLD, WAIT)
            .await
            .expect("run");
        assert_eq!(outcome.status, Some(REGISTRATION_STATUS_AUTHORIZED));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn world_messages_follow_the_response_and_the_world_hears_the_departure() {
        let (world, events) = TestWorld::new(false, vec![login_envelope()]);
        let (gateway, token) = start_with(LIMITS, (world.clone(), events)).await;
        let outcome = run_rep(gateway, client(token), HOLD, WAIT)
            .await
            .expect("run");
        assert_eq!(outcome.status, Some(REGISTRATION_STATUS_AUTHORIZED));
        assert!(outcome.pings > 0);
        let received: Vec<(u8, Option<u64>)> = outcome
            .messages
            .iter()
            .map(|received| (received.channel, received.envelope.type_index()))
            .collect();
        assert_eq!(received, vec![(MESSAGE_CHANNEL, Some(LOGIN_TYPE_INDEX))]);
        let connection = first_join(&world).await;
        wait_for(|| !sent(&world).is_empty()).await;
        assert_eq!(sent(&world), vec![Sent::Left(connection)]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_refused_join_gets_status_1() {
        let (world, events) = TestWorld::new(true, Vec::new());
        let (gateway, token) = start_with(LIMITS, (world.clone(), events)).await;
        let outcome = run_rep(gateway, client(token), HOLD, WAIT)
            .await
            .expect("run");
        assert_eq!(outcome.status, Some(REGISTRATION_STATUS_REFUSED));
        assert!(outcome.closed_by_server);
        assert_eq!(world.joins.lock().expect("lock").len(), 1);
        assert!(sent(&world).is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_world_close_ends_the_connection_and_unknown_connections_are_left() {
        let (world, events) = TestWorld::new(false, Vec::new());
        let (gateway, token) = start_with(LIMITS, (world.clone(), events)).await;
        let run = tokio::spawn(run_rep(gateway, client(token), HOLD_FIRST, WAIT));
        let connection = first_join(&world).await;
        tokio::time::sleep(SETTLE).await;
        world.emit(WorldEvent::Close { connection }).await;
        let outcome = run.await.expect("join").expect("run");
        assert_eq!(outcome.status, Some(REGISTRATION_STATUS_AUTHORIZED));
        assert!(outcome.closed_by_server);
        wait_for(|| sent(&world).contains(&Sent::Left(connection))).await;
        world
            .emit(WorldEvent::Messages {
                sim: test_sim(),
                connection: UNKNOWN_CONNECTION,
                channel: MESSAGE_CHANNEL,
                reliable: true,
                envelopes: vec![login_envelope()],
            })
            .await;
        wait_for(|| sent(&world).contains(&Sent::Left(UNKNOWN_CONNECTION))).await;
    }

    #[test]
    fn only_errors_of_one_datagram_keep_the_loop_running() {
        use std::io::{Error, ErrorKind};
        for kind in [
            ErrorKind::ConnectionReset,
            ErrorKind::ConnectionRefused,
            ErrorKind::Interrupted,
            ErrorKind::WouldBlock,
        ] {
            assert!(is_datagram_error(&Error::from(kind)), "{kind:?}");
        }
        for kind in [ErrorKind::PermissionDenied, ErrorKind::Other] {
            assert!(!is_datagram_error(&Error::from(kind)), "{kind:?}");
        }
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
