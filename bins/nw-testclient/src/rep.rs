// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client side of one REP connection, with no sockets: DTLS, the carrier, V3 registration,
//! and ping echo (M3 spec section 3.1).
//!
//! The caller passes in received datagrams and the time and sends what [`RepClient::poll`]
//! returns. The registration request goes out as soon as the carrier is connected.

use nw_carrier::{CarrierError, Connection, Event, MESSAGE_CHANNEL};
use nw_dtls::{DtlsClientConfig, DtlsError, DtlsSession};
use nw_reflect::{
    CLIENT_SESSION_LEN, Envelope, EnvelopeError, FramingError, LoginToken, PING_MSG_TYPE_INDEX,
    REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX, REGISTRATION_RESPONSE_MSG_TYPE_INDEX,
    RegistrationError, RegistrationRequestV3Msg, RegistrationResponseMsg, TypeId,
    decode_server_messages, encode_client_message,
};
use thiserror::Error;

/// The 16 bytes after the length of a client message: zero during registration (carrier captures
/// note section 5).
pub const ZERO_SESSION: [u8; CLIENT_SESSION_LEN] = [0; CLIENT_SESSION_LEN];

/// Failure of the client side.
#[derive(Debug, Error)]
pub enum RepError {
    /// DTLS failed.
    #[error(transparent)]
    Dtls(#[from] DtlsError),
    /// The carrier failed.
    #[error(transparent)]
    Carrier(#[from] CarrierError),
    /// A server payload did not split into envelopes.
    #[error(transparent)]
    Framing(#[from] FramingError),
    /// An envelope did not decode.
    #[error(transparent)]
    Envelope(#[from] EnvelopeError),
    /// A registration body did not decode or encode.
    #[error(transparent)]
    Registration(#[from] RegistrationError),
}

/// Something the caller must know about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepEvent {
    /// The carrier handshake is done; the registration request is on its way.
    Connected,
    /// The registration response arrived.
    Registered(RegistrationResponseMsg),
    /// The server sent a ping; the client sent the same 8 bytes back.
    Pinged,
    /// The server closed the connection.
    Closed {
        /// Reason byte.
        reason: u8,
    },
}

/// The client side of one REP connection.
pub struct RepClient {
    dtls: DtlsSession,
    carrier: Connection,
    token: Option<LoginToken>,
}

impl RepClient {
    /// A client that registers with `token` once DTLS and the carrier are up.
    ///
    /// # Errors
    ///
    /// Returns [`RepError::Dtls`] when the DTLS session cannot start.
    pub fn new(
        config: &DtlsClientConfig,
        mtu: u32,
        token: LoginToken,
        now_ms: u64,
    ) -> Result<Self, RepError> {
        Ok(Self {
            dtls: DtlsSession::connect(config, mtu)?,
            carrier: Connection::client(now_ms),
            token: Some(token),
        })
    }

    /// Handles one received datagram.
    ///
    /// # Errors
    ///
    /// Returns [`RepError`] when a layer fails; the caller drops the connection.
    pub fn receive(&mut self, now_ms: u64, datagram: &[u8]) -> Result<Vec<RepEvent>, RepError> {
        let mut events = Vec::new();
        for plaintext in self.dtls.receive(datagram)? {
            for event in self.carrier.receive(now_ms, &plaintext)? {
                match event {
                    Event::Connected => {
                        self.register()?;
                        events.push(RepEvent::Connected);
                    }
                    Event::Message(message) => {
                        for bytes in decode_server_messages(&message.payload)? {
                            if let Some(event) = self.message(Envelope::decode(bytes)?)? {
                                events.push(event);
                            }
                        }
                    }
                    Event::Closed { reason } => events.push(RepEvent::Closed { reason }),
                    Event::ConnectionControl(_) => {}
                }
            }
        }
        Ok(events)
    }

    fn register(&mut self) -> Result<(), RepError> {
        let Some(token) = self.token.take() else {
            return Ok(());
        };
        let body = RegistrationRequestV3Msg::with_login_token(token).encode()?;
        self.send(REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX, body)
    }

    fn message(&mut self, envelope: Envelope) -> Result<Option<RepEvent>, RepError> {
        match envelope.type_index() {
            Some(REGISTRATION_RESPONSE_MSG_TYPE_INDEX) => Ok(Some(RepEvent::Registered(
                RegistrationResponseMsg::decode(&envelope.body)?,
            ))),
            Some(PING_MSG_TYPE_INDEX) => {
                self.send(PING_MSG_TYPE_INDEX, envelope.body)?;
                Ok(Some(RepEvent::Pinged))
            }
            _ => Ok(None),
        }
    }

    fn send(&mut self, type_index: u64, body: Vec<u8>) -> Result<(), RepError> {
        let envelope = Envelope::message(TypeId::FileSlot(type_index), body).encode()?;
        let message = encode_client_message(&ZERO_SESSION, &envelope)?;
        self.carrier.send(MESSAGE_CHANNEL, true, &message)?;
        Ok(())
    }

    /// Closes the carrier; the next poll sends the disconnect.
    ///
    /// # Errors
    ///
    /// Returns [`RepError::Carrier`] when the disconnect cannot be queued.
    pub fn close(&mut self) -> Result<(), RepError> {
        self.carrier.close()?;
        Ok(())
    }

    /// Datagrams to send now: DTLS handshake flights, then carrier datagrams once DTLS is up.
    ///
    /// # Errors
    ///
    /// Returns [`RepError`] when a layer fails.
    pub fn poll(&mut self, now_ms: u64) -> Result<Vec<Vec<u8>>, RepError> {
        self.dtls.poll_timer()?;
        if self.dtls.is_established() {
            for datagram in self.carrier.poll(now_ms)? {
                self.dtls.send(&datagram)?;
            }
        }
        Ok(self.dtls.take_transmit())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::net::SocketAddr;
    use std::sync::LazyLock;

    use nw_carrier::ReceiveLimits;
    use nw_dtls::{COOKIE_SECRET_LEN, DtlsServerConfig, Identity, self_signed_identity};
    use nw_reflect::{
        REGISTRATION_RESPONSE_OBSERVED_FLAGS, REGISTRATION_STATUS_AUTHORIZED,
        decode_client_message, encode_server_messages,
    };

    use super::*;

    const MTU: u32 = 1200;
    const START_MS: u64 = 10_000;
    const STEP_MS: u64 = 10;
    const ROUNDS: u64 = 300;
    const SECRET: [u8; COOKIE_SECRET_LEN] = [3; COOKIE_SECRET_LEN];
    const PEER: &str = "192.0.2.20:50000";
    const PING_BODY: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 42];
    const TEST_NAME: &str = "test-rep";
    const VALID_DAYS: u32 = 30;
    /// One server identity for every test (RSA key generation is slow).
    pub(crate) static IDENTITY: LazyLock<Identity> =
        LazyLock::new(|| self_signed_identity(TEST_NAME, VALID_DAYS).expect("identity"));

    /// A minimal server: DTLS and the carrier only.
    struct Server {
        dtls: DtlsSession,
        carrier: Connection,
    }

    impl Server {
        fn new() -> Self {
            let config =
                DtlsServerConfig::new(&IDENTITY.certificate_pem, &IDENTITY.private_key_pem, SECRET)
                    .expect("server config");
            let mut carrier = Connection::server(START_MS);
            carrier.set_receive_limits(ReceiveLimits::REGISTRATION);
            Self {
                dtls: DtlsSession::accept(&config, PEER.parse::<SocketAddr>().expect("peer"), MTU)
                    .expect("accept"),
                carrier,
            }
        }

        fn receive(&mut self, now: u64, datagram: &[u8]) -> Vec<Envelope> {
            let mut envelopes = Vec::new();
            for plaintext in self.dtls.receive(datagram).expect("server dtls") {
                for event in self
                    .carrier
                    .receive(now, &plaintext)
                    .expect("server carrier")
                {
                    if let Event::Message(message) = event {
                        let client =
                            decode_client_message(&message.payload).expect("client message");
                        envelopes.push(Envelope::decode(client.envelope).expect("envelope"));
                    }
                }
            }
            envelopes
        }

        fn send(&mut self, type_index: u64, body: Vec<u8>) {
            let envelope = Envelope::message(TypeId::FileSlot(type_index), body)
                .encode()
                .expect("envelope");
            let payload = encode_server_messages(&[&envelope]).expect("payload");
            self.carrier
                .send(MESSAGE_CHANNEL, true, &payload)
                .expect("send");
        }

        fn poll(&mut self, now: u64) -> Vec<Vec<u8>> {
            self.dtls.poll_timer().expect("timer");
            if self.dtls.is_established() {
                for datagram in self.carrier.poll(now).expect("carrier poll") {
                    self.dtls.send(&datagram).expect("dtls send");
                }
            }
            self.dtls.take_transmit()
        }
    }

    fn token() -> LoginToken {
        LoginToken {
            ticket_id: b"ticket".to_vec(),
            signature: b"signature".to_vec(),
            ..LoginToken::default()
        }
    }

    /// Runs both sides until `done` returns true; returns the client events and server envelopes.
    fn run(
        client: &mut RepClient,
        server: &mut Server,
        mut on_server: impl FnMut(&mut Server, &Envelope),
        done: impl Fn(&[RepEvent]) -> bool,
    ) -> (Vec<RepEvent>, Vec<Envelope>) {
        let mut events = Vec::new();
        let mut envelopes = Vec::new();
        for round in 0..ROUNDS {
            let now = START_MS + round * STEP_MS;
            for datagram in client.poll(now).expect("client poll") {
                for envelope in server.receive(now, &datagram) {
                    on_server(server, &envelope);
                    envelopes.push(envelope);
                }
            }
            for datagram in server.poll(now) {
                events.extend(client.receive(now, &datagram).expect("client receive"));
            }
            if done(&events) {
                break;
            }
        }
        (events, envelopes)
    }

    #[test]
    fn the_client_connects_registers_and_reads_the_response() {
        let config = DtlsClientConfig::new(&IDENTITY.certificate_pem).expect("client config");
        let mut client = RepClient::new(&config, MTU, token(), START_MS).expect("client");
        let mut server = Server::new();
        let response = RegistrationResponseMsg {
            uint32_at_0x8: REGISTRATION_STATUS_AUTHORIZED,
            uint64: 1,
            bytes_at_0x18: b"TOKEN".to_vec(),
            bytes_at_0x38: b"version".to_vec(),
            bytes_at_0x58: REGISTRATION_RESPONSE_OBSERVED_FLAGS,
        };
        let reply = response.encode().expect("response");
        let (events, envelopes) = run(
            &mut client,
            &mut server,
            |server, envelope| {
                if envelope.type_index() == Some(REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX) {
                    server.send(REGISTRATION_RESPONSE_MSG_TYPE_INDEX, reply.clone());
                }
            },
            |events| {
                events
                    .iter()
                    .any(|event| matches!(event, RepEvent::Registered(_)))
            },
        );
        assert_eq!(events.first(), Some(&RepEvent::Connected));
        assert_eq!(events.last(), Some(&RepEvent::Registered(response)));
        let v3 = RegistrationRequestV3Msg::decode(&envelopes[0].body).expect("v3");
        assert_eq!(v3.login_token(), token());
    }

    #[test]
    fn the_client_echoes_pings() {
        let config = DtlsClientConfig::new(&IDENTITY.certificate_pem).expect("client config");
        let mut client = RepClient::new(&config, MTU, token(), START_MS).expect("client");
        let mut server = Server::new();
        let (events, envelopes) = run(
            &mut client,
            &mut server,
            |server, envelope| {
                if envelope.type_index() == Some(REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX) {
                    server.send(PING_MSG_TYPE_INDEX, PING_BODY.to_vec());
                }
            },
            |_| false,
        );
        assert!(events.contains(&RepEvent::Pinged));
        let echo = envelopes
            .iter()
            .find(|envelope| envelope.type_index() == Some(PING_MSG_TYPE_INDEX))
            .expect("echo");
        assert_eq!(echo.body, PING_BODY);
    }
}
