// SPDX-License-Identifier: AGPL-3.0-or-later
//! One carrier connection: the connect handshake, acks, keepalive, and close (M3 spec sections
//! 3.1 and 5; carrier captures note section 4).
//!
//! The client sends a connect request (version 5) on the system channel about every 30 ms, with
//! one more `0x01` byte per retry, until the connect ack arrives. The server answers the first
//! request with a reliable connect ack. Both sides ack received datagrams within
//! [`ACK_DELAY_MS`] and send a keepalive ack after [`KEEP_ALIVE_MS`] without traffic. The caller
//! reads the time, moves the bytes, and drops a silent connection.

use crate::datagram::decode_datagram;
use crate::error::CarrierError;
use crate::frame::SYSTEM_CHANNEL;
use crate::receiver::{Inbound, Message, ReceiveLimits, Receiver};
use crate::sender::{Delivery, Sender};
use crate::system::{Acks, CARRIER_VERSION, DISCONNECT_REASON_NONE, SystemMessage};

/// Time between connect requests (about 30 ms in the captures).
pub const CONNECT_RETRY_MS: u64 = 30;
/// Connect requests before the client gives up (3 s).
pub const MAX_CONNECT_ATTEMPTS: usize = 100;
/// Most time between a received datagram and its ack.
pub const ACK_DELAY_MS: u64 = 33;
/// Time without sent traffic before a keepalive ack.
pub const KEEP_ALIVE_MS: u64 = 1000;
/// Byte added to the connect request per retry (captures).
pub const CONNECT_RETRY_MARK: u8 = 0x01;

const SYSTEM_UNRELIABLE: Delivery = Delivery {
    channel: SYSTEM_CHANNEL,
    reliable: false,
    connecting: false,
};
const SYSTEM_RELIABLE: Delivery = Delivery {
    channel: SYSTEM_CHANNEL,
    reliable: true,
    connecting: false,
};
const CONNECT_REQUEST: Delivery = Delivery {
    channel: SYSTEM_CHANNEL,
    reliable: false,
    connecting: true,
};

/// Which end of the connection this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Sends the connect request.
    Client,
    /// Answers with the connect ack.
    Server,
}

/// Connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Before the connect handshake is done.
    Connecting,
    /// Messages can flow.
    Connected,
    /// Closed by either side.
    Closed,
}

/// Something the caller must know about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The connect handshake is done.
    Connected,
    /// A message arrived on channel 0, 1, or 2.
    Message(Message),
    /// The peer closed the connection.
    Closed {
        /// Reason byte.
        reason: u8,
    },
    /// Connection control data (meaning Unknown).
    ConnectionControl(Vec<u8>),
}

/// One carrier connection.
#[derive(Debug)]
pub struct Connection {
    role: Role,
    state: ConnectionState,
    receiver: Receiver,
    sender: Sender,
    connect_attempts: usize,
    next_connect_ms: u64,
    ack_due_ms: Option<u64>,
    last_sent_ms: u64,
    last_received_ms: u64,
}

impl Connection {
    /// A client connection that starts the handshake on the next poll.
    pub fn client(now_ms: u64) -> Self {
        Self::new(Role::Client, now_ms)
    }

    /// A server connection that waits for a connect request.
    pub fn server(now_ms: u64) -> Self {
        Self::new(Role::Server, now_ms)
    }

    fn new(role: Role, now_ms: u64) -> Self {
        Self {
            role,
            state: ConnectionState::Connecting,
            receiver: Receiver::new(ReceiveLimits::REGISTRATION),
            sender: Sender::new(),
            connect_attempts: 0,
            next_connect_ms: now_ms,
            ack_due_ms: None,
            last_sent_ms: now_ms,
            last_received_ms: now_ms,
        }
    }

    /// Current state.
    pub const fn state(&self) -> ConnectionState {
        self.state
    }

    /// Time of the last received datagram; the caller drops a silent connection.
    pub const fn last_received_ms(&self) -> u64 {
        self.last_received_ms
    }

    /// Changes the receive limits, for example after registration.
    pub fn set_receive_limits(&mut self, limits: ReceiveLimits) {
        self.receiver.set_limits(limits);
    }

    /// Handles one received datagram.
    ///
    /// # Errors
    ///
    /// Returns [`CarrierError`] for bytes that do not decode, a broken chunk or limit, or a
    /// message before the handshake. The caller closes the connection.
    pub fn receive(&mut self, now_ms: u64, bytes: &[u8]) -> Result<Vec<Event>, CarrierError> {
        if self.state == ConnectionState::Closed {
            return Ok(Vec::new());
        }
        let datagram = decode_datagram(bytes)?;
        self.last_received_ms = now_ms;
        let inbound = self.receiver.receive(&datagram)?;
        if self.receiver.wants_ack() {
            self.ack_due_ms.get_or_insert(now_ms + ACK_DELAY_MS);
        }
        let mut events = Vec::new();
        for item in inbound {
            match item {
                Inbound::Message(message) => {
                    if self.state != ConnectionState::Connected {
                        return Err(CarrierError::NotConnected);
                    }
                    events.push(Event::Message(message));
                }
                Inbound::System(system) => self.handle_system(system, &mut events)?,
            }
            if self.state == ConnectionState::Closed {
                break;
            }
        }
        Ok(events)
    }

    fn handle_system(
        &mut self,
        system: SystemMessage,
        events: &mut Vec<Event>,
    ) -> Result<(), CarrierError> {
        match system {
            SystemMessage::ConnectRequest { .. }
                if self.role == Role::Server && self.state == ConnectionState::Connecting =>
            {
                let ack = SystemMessage::ConnectAck {
                    version: CARRIER_VERSION,
                    extra: Vec::new(),
                };
                self.sender.send(SYSTEM_RELIABLE, &ack.encode())?;
                self.state = ConnectionState::Connected;
                events.push(Event::Connected);
            }
            SystemMessage::ConnectAck { .. }
                if self.role == Role::Client && self.state == ConnectionState::Connecting =>
            {
                self.state = ConnectionState::Connected;
                events.push(Event::Connected);
            }
            SystemMessage::Acks(acks) => self.sender.handle_acks(&acks),
            SystemMessage::Disconnect { reason } => {
                self.state = ConnectionState::Closed;
                events.push(Event::Closed { reason });
            }
            SystemMessage::ConnectionControl(data) => events.push(Event::ConnectionControl(data)),
            SystemMessage::ConnectRequest { .. }
            | SystemMessage::ConnectAck { .. }
            | SystemMessage::Other { .. } => {}
        }
        Ok(())
    }

    /// Queues one message.
    ///
    /// # Errors
    ///
    /// Returns [`CarrierError::NotConnected`] before the handshake or after close, and the
    /// sender's errors for a bad channel or a message that is too long.
    pub fn send(
        &mut self,
        channel: u8,
        reliable: bool,
        payload: &[u8],
    ) -> Result<(), CarrierError> {
        if self.state != ConnectionState::Connected {
            return Err(CarrierError::NotConnected);
        }
        self.sender.send(
            Delivery {
                channel,
                reliable,
                connecting: false,
            },
            payload,
        )
    }

    /// Queues a disconnect and closes; the next poll sends it.
    ///
    /// # Errors
    ///
    /// Returns the sender's error when the disconnect cannot be queued.
    pub fn close(&mut self) -> Result<(), CarrierError> {
        if self.state == ConnectionState::Closed {
            return Ok(());
        }
        let disconnect = SystemMessage::Disconnect {
            reason: DISCONNECT_REASON_NONE,
        };
        self.sender.send(SYSTEM_UNRELIABLE, &disconnect.encode())?;
        self.state = ConnectionState::Closed;
        Ok(())
    }

    /// Datagrams to send now: connect requests, acks, keepalives, messages, and resends.
    ///
    /// # Errors
    ///
    /// Returns [`CarrierError::ConnectTimeout`] when the client got no connect ack, and
    /// [`CarrierError::TooManyAttempts`] when a reliable frame was never acked.
    pub fn poll(&mut self, now_ms: u64) -> Result<Vec<Vec<u8>>, CarrierError> {
        if self.role == Role::Client
            && self.state == ConnectionState::Connecting
            && now_ms >= self.next_connect_ms
        {
            if self.connect_attempts >= MAX_CONNECT_ATTEMPTS {
                return Err(CarrierError::ConnectTimeout);
            }
            let request = SystemMessage::ConnectRequest {
                version: CARRIER_VERSION,
                extra: vec![CONNECT_RETRY_MARK; self.connect_attempts],
            };
            self.sender.send(CONNECT_REQUEST, &request.encode())?;
            self.connect_attempts += 1;
            self.next_connect_ms = now_ms + CONNECT_RETRY_MS;
        }
        if self.ack_due_ms.is_some_and(|due| now_ms >= due) {
            self.ack_due_ms = None;
            if let Some(acks) = self.receiver.take_acks() {
                self.sender
                    .send(SYSTEM_UNRELIABLE, &SystemMessage::Acks(acks).encode())?;
            }
        }
        if self.state == ConnectionState::Connected
            && self.sender.is_idle()
            && now_ms.saturating_sub(self.last_sent_ms) >= KEEP_ALIVE_MS
        {
            let keep_alive = SystemMessage::Acks(Acks::KeepAlive);
            self.sender.send(SYSTEM_UNRELIABLE, &keep_alive.encode())?;
        }
        let datagrams = self.sender.poll(now_ms)?;
        if !datagrams.is_empty() {
            self.last_sent_ms = now_ms;
        }
        Ok(datagrams)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datagram::decode_datagram;
    use crate::frame::decode_frames;
    use crate::sender::{MAX_CHUNK_PAYLOAD, RESEND_AFTER_MS};

    const START: u64 = 5_000;
    const STEP: u64 = 10;
    const DATA_CHANNEL: u8 = 0;
    const HELLO: &[u8] = b"hello";
    const BIG_LEN: usize = 3 * MAX_CHUNK_PAYLOAD;
    /// Exchanges that let every pending ack go out.
    const SETTLE_ROUNDS: u64 = 3;
    /// First connect request in the captures.
    const CAPTURED_CONNECT: [u8; 5] = [0x00, 0x00, 0x00, 0x05, 0x01];
    /// Second connect request in the captures.
    const CAPTURED_RETRY: [u8; 6] = [0x00, 0x00, 0x00, 0x05, 0x01, 0x01];

    /// Polls both sides and moves every datagram once; returns the events of client and server.
    fn exchange(
        client: &mut Connection,
        server: &mut Connection,
        now: u64,
    ) -> (Vec<Event>, Vec<Event>) {
        let mut client_events = Vec::new();
        let mut server_events = Vec::new();
        for bytes in client.poll(now).expect("client poll") {
            server_events.extend(server.receive(now, &bytes).expect("server receive"));
        }
        for bytes in server.poll(now).expect("server poll") {
            client_events.extend(client.receive(now, &bytes).expect("client receive"));
        }
        (client_events, server_events)
    }

    fn connected_pair() -> (Connection, Connection) {
        let mut client = Connection::client(START);
        let mut server = Connection::server(START);
        let (client_events, server_events) = exchange(&mut client, &mut server, START);
        assert_eq!(server_events, vec![Event::Connected]);
        assert_eq!(client_events, vec![Event::Connected]);
        (client, server)
    }

    /// Lets every pending ack go out; returns the time after.
    fn settle(client: &mut Connection, server: &mut Connection, mut now: u64) -> u64 {
        for _ in 0..SETTLE_ROUNDS {
            now += ACK_DELAY_MS;
            exchange(client, server, now);
        }
        now
    }

    fn messages(events: &[Event]) -> Vec<Vec<u8>> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::Message(message) => Some(message.payload.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_handshake_connects_both_sides() {
        let (client, server) = connected_pair();
        assert_eq!(client.state(), ConnectionState::Connected);
        assert_eq!(server.state(), ConnectionState::Connected);
    }

    #[test]
    fn the_connect_request_has_the_captured_layout() {
        let mut client = Connection::client(START);
        let first = client.poll(START).expect("first");
        let frames =
            decode_frames(&decode_datagram(&first[0]).expect("decode").body).expect("frames");
        assert_eq!(frames[0].channel, SYSTEM_CHANNEL);
        assert!(frames[0].connecting);
        assert!(!frames[0].reliable);
        assert_eq!(frames[0].payload, CAPTURED_CONNECT);
        assert!(
            client
                .poll(START + CONNECT_RETRY_MS - 1)
                .expect("wait")
                .is_empty()
        );
        let retry = client.poll(START + CONNECT_RETRY_MS).expect("retry");
        let frames =
            decode_frames(&decode_datagram(&retry[0]).expect("decode").body).expect("frames");
        assert_eq!(frames[0].payload, CAPTURED_RETRY);
    }

    #[test]
    fn the_client_gives_up_without_an_ack() {
        let mut client = Connection::client(START);
        let mut now = START;
        for _ in 0..MAX_CONNECT_ATTEMPTS {
            client.poll(now).expect("request");
            now += CONNECT_RETRY_MS;
        }
        assert_eq!(client.poll(now), Err(CarrierError::ConnectTimeout));
    }

    #[test]
    fn messages_flow_both_ways_and_long_ones_are_chunked() {
        let (mut client, mut server) = connected_pair();
        let big: Vec<u8> = (0..BIG_LEN).map(|index| index as u8).collect();
        client.send(DATA_CHANNEL, true, HELLO).expect("client send");
        server.send(DATA_CHANNEL, true, &big).expect("server send");
        let (client_events, server_events) = exchange(&mut client, &mut server, START + STEP);
        assert_eq!(messages(&server_events), vec![HELLO.to_vec()]);
        assert_eq!(messages(&client_events), vec![big]);
    }

    #[test]
    fn a_lost_datagram_is_sent_again() {
        let (mut client, mut server) = connected_pair();
        server.send(DATA_CHANNEL, true, HELLO).expect("send");
        let lost = server.poll(START + STEP).expect("poll");
        assert_eq!(lost.len(), 1);
        let mut now = START + STEP;
        let mut got = Vec::new();
        while got.is_empty() && now <= START + 2 * RESEND_AFTER_MS {
            now += STEP;
            got = messages(&exchange(&mut client, &mut server, now).0);
        }
        assert_eq!(got, vec![HELLO.to_vec()]);
        settle(&mut client, &mut server, now);
        assert!(server.sender.is_idle());
    }

    #[test]
    fn acks_wait_for_the_ack_delay() {
        let (mut client, mut server) = connected_pair();
        let now = settle(&mut client, &mut server, START) + STEP;
        client.send(DATA_CHANNEL, true, HELLO).expect("send");
        for bytes in client.poll(now).expect("poll") {
            server.receive(now, &bytes).expect("receive");
        }
        let early: Vec<Vec<u8>> = server.poll(now + ACK_DELAY_MS - 1).expect("early");
        assert!(early.is_empty());
        assert_eq!(server.poll(now + ACK_DELAY_MS).expect("due").len(), 1);
    }

    #[test]
    fn an_idle_connection_sends_a_keepalive() {
        let (mut client, mut server) = connected_pair();
        let now = settle(&mut client, &mut server, START);
        let quiet = now + KEEP_ALIVE_MS;
        let datagrams = client.poll(quiet).expect("poll");
        assert_eq!(datagrams.len(), 1);
        let frames =
            decode_frames(&decode_datagram(&datagrams[0]).expect("decode").body).expect("frames");
        assert_eq!(
            SystemMessage::decode(&frames[0].payload),
            Ok(SystemMessage::Acks(Acks::KeepAlive))
        );
    }

    #[test]
    fn close_tells_the_peer() {
        let (mut client, mut server) = connected_pair();
        client.close().expect("close");
        assert_eq!(client.state(), ConnectionState::Closed);
        let (_, server_events) = exchange(&mut client, &mut server, START + STEP);
        assert_eq!(
            server_events,
            vec![Event::Closed {
                reason: DISCONNECT_REASON_NONE
            }]
        );
        assert_eq!(
            client.send(DATA_CHANNEL, true, HELLO),
            Err(CarrierError::NotConnected)
        );
    }

    #[test]
    fn messages_before_the_handshake_are_errors() {
        let mut client = Connection::client(START);
        assert_eq!(
            client.send(DATA_CHANNEL, true, HELLO),
            Err(CarrierError::NotConnected)
        );
        let (mut talker, _) = connected_pair();
        talker.send(DATA_CHANNEL, true, HELLO).expect("send");
        let mut fresh = Connection::server(START);
        let bytes = talker.poll(START + STEP).expect("poll");
        assert_eq!(
            fresh.receive(START + STEP, &bytes[0]),
            Err(CarrierError::NotConnected)
        );
    }
}
