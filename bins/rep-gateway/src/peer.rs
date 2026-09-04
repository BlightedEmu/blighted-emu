// SPDX-License-Identifier: AGPL-3.0-or-later
//! One player connection in the gateway, with no sockets: DTLS, the carrier, the registration
//! step, pings, and the time and rate limits (M3 spec sections 3.1, 6, and 10).
//!
//! The server loop passes in datagrams and the time. When a V3 request arrives, [`Peer::receive`]
//! returns [`PeerEvent::RegistrationRequested`]; the loop checks the ticket (an async store call)
//! and hands the result to [`Peer::complete_registration`]. Any error closes the connection.

use std::net::SocketAddr;
use std::time::SystemTime;

use nw_carrier::{
    CarrierError, Connection, ConnectionState, Event, MESSAGE_CHANNEL, Message, ReceiveLimits,
};
use nw_dtls::{DtlsError, DtlsServerConfig, DtlsSession};
use nw_login::queue::EntryRecord;
use nw_reflect::{
    Envelope, EnvelopeError, FramingError, PING_MSG_TYPE_INDEX,
    REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX, REGISTRATION_RESPONSE_MSG_TYPE_INDEX,
    RegistrationError, RegistrationRequestV3Msg, TypeId, decode_client_message,
    encode_server_messages,
};
use thiserror::Error;

use crate::registration::{Refusal, STATUS_AUTHORIZED, STATUS_REFUSED, TicketClaims, response};

/// Length of the rate window.
pub const RATE_WINDOW_MS: u64 = 1000;

/// Limits and timers of one connection (config `[limits]`, M3 spec section 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerSettings {
    /// DTLS MTU.
    pub mtu: u32,
    /// Time from the first datagram to the registration response.
    pub registration_timeout_ms: u64,
    /// Time without a received datagram before the connection is dropped.
    pub silence_timeout_ms: u64,
    /// Time between server pings after registration.
    pub ping_interval_ms: u64,
    /// Datagrams one connection may send per second.
    pub datagrams_per_second: u32,
}

/// What the server loop must do for a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerEvent {
    /// A V3 request arrived; check the ticket and call [`Peer::complete_registration`].
    RegistrationRequested(TicketClaims),
}

/// Why a connection ends.
#[derive(Debug, Error)]
pub enum PeerError {
    /// DTLS failed.
    #[error(transparent)]
    Dtls(#[from] DtlsError),
    /// The carrier failed or got a malformed frame.
    #[error(transparent)]
    Carrier(#[from] CarrierError),
    /// A client message has a bad length or CRC.
    #[error(transparent)]
    Framing(#[from] FramingError),
    /// An envelope did not decode.
    #[error(transparent)]
    Envelope(#[from] EnvelopeError),
    /// A registration body did not decode or encode.
    #[error(transparent)]
    Registration(#[from] RegistrationError),
    /// No random bytes for the session token.
    #[error("no random bytes: {0}")]
    Random(getrandom::Error),
    /// No registration in time.
    #[error("no registration in time")]
    RegistrationTimeout,
    /// No datagram in time.
    #[error("connection silent")]
    Silent,
    /// More datagrams per second than allowed.
    #[error("too many datagrams")]
    TooManyDatagrams,
    /// A message that this phase does not take.
    #[error("unexpected message type {type_index:?}")]
    UnexpectedMessage {
        /// Its type index; `None` for an inline UUID.
        type_index: Option<u64>,
    },
}

impl PeerError {
    /// A short name for logs and metrics.
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::Dtls(_) => "dtls",
            Self::Carrier(_) | Self::Framing(_) | Self::Envelope(_) | Self::Registration(_) => {
                "malformed"
            }
            Self::Random(_) => "random",
            Self::RegistrationTimeout => "registration_timeout",
            Self::Silent => "silent",
            Self::TooManyDatagrams => "rate",
            Self::UnexpectedMessage { .. } => "unexpected_message",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    AwaitingRegistration,
    Checking,
    Registered,
    Refused,
}

/// One player connection.
pub struct Peer {
    settings: PeerSettings,
    dtls: DtlsSession,
    carrier: Connection,
    phase: Phase,
    created_ms: u64,
    last_received_ms: u64,
    last_ping_ms: u64,
    window_start_ms: u64,
    window_count: u32,
}

impl Peer {
    /// A connection for the peer at `address`; its first datagram follows.
    ///
    /// # Errors
    ///
    /// Returns [`PeerError::Dtls`] when the DTLS session cannot start.
    pub fn new(
        config: &DtlsServerConfig,
        address: SocketAddr,
        settings: PeerSettings,
        now_ms: u64,
    ) -> Result<Self, PeerError> {
        Ok(Self {
            settings,
            dtls: DtlsSession::accept(config, address, settings.mtu)?,
            carrier: Connection::server(now_ms),
            phase: Phase::AwaitingRegistration,
            created_ms: now_ms,
            last_received_ms: now_ms,
            last_ping_ms: now_ms,
            window_start_ms: now_ms,
            window_count: 0,
        })
    }

    /// Whether the registration was accepted.
    pub fn is_registered(&self) -> bool {
        self.phase == Phase::Registered
    }

    /// Whether DTLS is done (the handshake no longer counts against the handshake limit).
    pub fn is_established(&self) -> bool {
        self.dtls.is_established()
    }

    /// Whether the connection is over and its last datagrams are sent.
    pub fn is_finished(&self) -> bool {
        self.carrier.state() == ConnectionState::Closed
    }

    /// Handles one received datagram.
    ///
    /// # Errors
    ///
    /// Returns [`PeerError`] for a rate breach, a failure of any layer, or an unexpected message.
    pub fn receive(&mut self, now_ms: u64, datagram: &[u8]) -> Result<Vec<PeerEvent>, PeerError> {
        self.count_datagram(now_ms)?;
        self.last_received_ms = now_ms;
        let mut events = Vec::new();
        for plaintext in self.dtls.receive(datagram)? {
            for event in self.carrier.receive(now_ms, &plaintext)? {
                if let Event::Message(message) = event
                    && let Some(event) = self.message(&message)?
                {
                    events.push(event);
                }
            }
        }
        Ok(events)
    }

    fn count_datagram(&mut self, now_ms: u64) -> Result<(), PeerError> {
        if now_ms.saturating_sub(self.window_start_ms) >= RATE_WINDOW_MS {
            self.window_start_ms = now_ms;
            self.window_count = 0;
        }
        self.window_count += 1;
        if self.window_count > self.settings.datagrams_per_second {
            return Err(PeerError::TooManyDatagrams);
        }
        Ok(())
    }

    fn message(&mut self, message: &Message) -> Result<Option<PeerEvent>, PeerError> {
        let client = decode_client_message(&message.payload)?;
        let envelope = Envelope::decode(client.envelope)?;
        match (self.phase, envelope.type_index()) {
            (Phase::AwaitingRegistration, Some(REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX)) => {
                let request = RegistrationRequestV3Msg::decode(&envelope.body)?;
                self.phase = Phase::Checking;
                Ok(Some(PeerEvent::RegistrationRequested(
                    TicketClaims::from_token(&request.login_token()),
                )))
            }
            (Phase::Registered, Some(index)) if index != REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX => {
                Ok(None)
            }
            (_, type_index) => Err(PeerError::UnexpectedMessage { type_index }),
        }
    }

    /// Sends the registration response for the checked ticket. A refused connection sends status
    /// 1 and then a disconnect.
    ///
    /// # Errors
    ///
    /// Returns [`PeerError`] when the response cannot be built or queued.
    pub fn complete_registration(
        &mut self,
        now: SystemTime,
        result: Result<&EntryRecord, Refusal>,
        server_version: &str,
    ) -> Result<(), PeerError> {
        let status = if result.is_ok() {
            STATUS_AUTHORIZED
        } else {
            STATUS_REFUSED
        };
        let body = response(status, now, server_version)
            .map_err(PeerError::Random)?
            .encode()?;
        self.send_message(REGISTRATION_RESPONSE_MSG_TYPE_INDEX, body)?;
        if result.is_ok() {
            self.phase = Phase::Registered;
            self.carrier.set_receive_limits(ReceiveLimits::SESSION);
        } else {
            self.phase = Phase::Refused;
            self.carrier.close()?;
        }
        Ok(())
    }

    fn send_message(&mut self, type_index: u64, body: Vec<u8>) -> Result<(), PeerError> {
        let envelope = Envelope::message(TypeId::FileSlot(type_index), body).encode()?;
        let payload = encode_server_messages(&[&envelope])?;
        self.carrier.send(MESSAGE_CHANNEL, true, &payload)?;
        Ok(())
    }

    /// Datagrams to send now: DTLS flights, carrier datagrams, and pings.
    ///
    /// # Errors
    ///
    /// Returns [`PeerError::RegistrationTimeout`] or [`PeerError::Silent`] when a timer runs out,
    /// and any layer's error.
    pub fn poll(&mut self, now_ms: u64) -> Result<Vec<Vec<u8>>, PeerError> {
        let registering = matches!(self.phase, Phase::AwaitingRegistration | Phase::Checking);
        if registering
            && now_ms.saturating_sub(self.created_ms) >= self.settings.registration_timeout_ms
        {
            return Err(PeerError::RegistrationTimeout);
        }
        if now_ms.saturating_sub(self.last_received_ms) >= self.settings.silence_timeout_ms {
            return Err(PeerError::Silent);
        }
        self.dtls.poll_timer()?;
        if self.phase == Phase::Registered
            && now_ms.saturating_sub(self.last_ping_ms) >= self.settings.ping_interval_ms
        {
            self.last_ping_ms = now_ms;
            self.send_message(PING_MSG_TYPE_INDEX, now_ms.to_be_bytes().to_vec())?;
        }
        if self.dtls.is_established() {
            for datagram in self.carrier.poll(now_ms)? {
                self.dtls.send(&datagram)?;
            }
        }
        Ok(self.dtls.take_transmit())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;
    use std::time::UNIX_EPOCH;

    use nw_dtls::{COOKIE_SECRET_LEN, DtlsClientConfig, Identity, self_signed_identity};
    use nw_reflect::LoginToken;
    use nw_testclient::rep::{RepClient, RepEvent};
    use uuid::Uuid;

    use super::*;

    const MTU: u32 = 1200;
    const START_MS: u64 = 50_000;
    const STEP_MS: u64 = 10;
    const ROUNDS: u64 = 400;
    const SECRET: [u8; COOKIE_SECRET_LEN] = [9; COOKIE_SECRET_LEN];
    const PEER: &str = "192.0.2.30:41000";
    const VERSION: &str = "[TEST].Javelin.1.365.6031.1";
    const VALID_DAYS: u32 = 30;
    const SETTINGS: PeerSettings = PeerSettings {
        mtu: MTU,
        registration_timeout_ms: 10_000,
        silence_timeout_ms: 10_000,
        ping_interval_ms: 1000,
        datagrams_per_second: 400,
    };
    static IDENTITY: LazyLock<Identity> =
        LazyLock::new(|| self_signed_identity("test-gateway", VALID_DAYS).expect("identity"));

    fn record() -> EntryRecord {
        EntryRecord {
            ticket_id: "ticket".to_owned(),
            account_id: Uuid::new_v4(),
            world_id: Uuid::new_v4(),
            character_id: Uuid::new_v4(),
            expires_at: 0,
        }
    }

    fn token() -> LoginToken {
        LoginToken {
            ticket_id: b"ticket".to_vec(),
            signature: b"signature".to_vec(),
            character_id: b"character".to_vec(),
            ..LoginToken::default()
        }
    }

    fn sides(settings: PeerSettings) -> (RepClient, Peer) {
        let server =
            DtlsServerConfig::new(&IDENTITY.certificate_pem, &IDENTITY.private_key_pem, SECRET)
                .expect("server config");
        let client = DtlsClientConfig::new(&IDENTITY.certificate_pem).expect("client config");
        (
            RepClient::new(&client, MTU, token(), START_MS).expect("client"),
            Peer::new(&server, PEER.parse().expect("peer"), settings, START_MS).expect("peer"),
        )
    }

    /// Runs both sides from `start` until `stop` says so; `answer` decides each registration.
    fn run(
        client: &mut RepClient,
        peer: &mut Peer,
        start: u64,
        answer: Result<&EntryRecord, Refusal>,
        stop: impl Fn(&[RepEvent]) -> bool,
    ) -> (Vec<RepEvent>, Vec<PeerEvent>, u64) {
        let mut client_events = Vec::new();
        let mut peer_events = Vec::new();
        let mut now = start;
        for _ in 0..ROUNDS {
            now += STEP_MS;
            for datagram in client.poll(now).expect("client poll") {
                for event in peer.receive(now, &datagram).expect("peer receive") {
                    let PeerEvent::RegistrationRequested(_) = &event;
                    peer.complete_registration(UNIX_EPOCH, answer, VERSION)
                        .expect("complete");
                    peer_events.push(event);
                }
            }
            for datagram in peer.poll(now).expect("peer poll") {
                client_events.extend(client.receive(now, &datagram).expect("client receive"));
            }
            if stop(&client_events) {
                break;
            }
        }
        (client_events, peer_events, now)
    }

    fn status(events: &[RepEvent]) -> Option<u32> {
        events.iter().find_map(|event| match event {
            RepEvent::Registered(response) => Some(response.uint32_at_0x8),
            _ => None,
        })
    }

    #[test]
    fn an_accepted_registration_gets_status_0_and_pings() {
        let (mut client, mut peer) = sides(SETTINGS);
        let entry = record();
        let (events, peer_events, _) =
            run(&mut client, &mut peer, START_MS, Ok(&entry), |events| {
                events.contains(&RepEvent::Pinged)
            });
        assert_eq!(
            peer_events,
            vec![PeerEvent::RegistrationRequested(TicketClaims::from_token(
                &token()
            ))]
        );
        assert_eq!(status(&events), Some(STATUS_AUTHORIZED));
        assert!(peer.is_registered());
        assert!(events.contains(&RepEvent::Pinged));
    }

    #[test]
    fn a_refused_registration_gets_status_1_and_a_disconnect() {
        let (mut client, mut peer) = sides(SETTINGS);
        let (events, _, _) = run(
            &mut client,
            &mut peer,
            START_MS,
            Err(Refusal::UnknownTicket),
            |events| {
                events
                    .iter()
                    .any(|event| matches!(event, RepEvent::Closed { .. }))
            },
        );
        assert_eq!(status(&events), Some(STATUS_REFUSED));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RepEvent::Closed { .. }))
        );
        assert!(peer.is_finished());
    }

    #[test]
    fn a_connection_without_registration_times_out() {
        let (_, mut peer) = sides(SETTINGS);
        let late = START_MS + SETTINGS.registration_timeout_ms;
        assert!(matches!(
            peer.poll(late),
            Err(PeerError::RegistrationTimeout)
        ));
    }

    #[test]
    fn a_silent_registered_connection_is_dropped() {
        let (mut client, mut peer) = sides(SETTINGS);
        let entry = record();
        let (_, _, now) = run(&mut client, &mut peer, START_MS, Ok(&entry), |events| {
            status(events).is_some()
        });
        assert!(matches!(
            peer.poll(now + SETTINGS.silence_timeout_ms),
            Err(PeerError::Silent)
        ));
    }

    #[test]
    fn too_many_datagrams_close_the_connection() {
        let few = PeerSettings {
            datagrams_per_second: 2,
            ..SETTINGS
        };
        let (_, mut peer) = sides(few);
        let noise = [0_u8; 4];
        let _ = peer.receive(START_MS, &noise);
        let _ = peer.receive(START_MS, &noise);
        assert!(matches!(
            peer.receive(START_MS, &noise),
            Err(PeerError::TooManyDatagrams)
        ));
    }
}
