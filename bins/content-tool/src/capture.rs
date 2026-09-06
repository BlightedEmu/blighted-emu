// SPDX-License-Identifier: AGPL-3.0-or-later
//! `capture-check`: decodes local capture ledgers with `nw-carrier` and `nw-reflect` (M3 spec
//! section 9; carrier captures note). Server bodies of the M4 types are decoded with
//! `nw-messages`, encoded again, and compared (M4 spec section 10); so are the bundle members at
//! the front of each payload whose class has a reader.
//!
//! The ledgers hold other players' data. This module keeps counts, type indexes, and the
//! registration response values that the RE note records. It never keeps payload bytes, ids,
//! or tokens.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use nw_capture::{
    CLIENT_TO_SERVER, CaptureError, LedgerRecord, SERVER_TO_CLIENT, is_connect_request,
    parse_ledger,
};
use nw_carrier::{Inbound, ReceiveLimits, Receiver, decode_datagram};
use nw_messages::{
    BodyCodec, BundlePayload, ClientInputMsg, MessageError, RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX,
    REQUEST_RESPAWN_TYPE_INDEX, ReplicatedStateBundle, RequestRespawnMsg, ServerMessage,
};
use nw_reflect::{
    Envelope, REGISTRATION_RESPONSE_MSG_TYPE_INDEX, RegistrationResponseMsg, decode_client_message,
    decode_server_messages,
};
/// Unix milliseconds at 2000-01-01 00:00:00 UTC, the zero of the registration clock.
pub const CLOCK_EPOCH_UNIX_MS: u64 = 946_684_800_000;
/// Nanoseconds per millisecond.
pub const NANOS_PER_MILLI: u64 = 1_000_000;

const DIRECTIONS: usize = 2;

/// A decode failure: the stream of that connection and direction stops there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeFailure {
    /// Ledger file.
    pub file: PathBuf,
    /// Record number in the file.
    pub record: usize,
    /// What failed.
    pub reason: String,
}

/// Values of the registration responses (carrier captures note section 7).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RegistrationSummary {
    /// Responses decoded.
    pub responses: u64,
    /// Count per status value.
    pub statuses: BTreeMap<u32, u64>,
    /// Count per four-byte value.
    pub flags: BTreeMap<[u8; 4], u64>,
    /// Count per server version text.
    pub versions: BTreeMap<String, u64>,
    /// Largest distance between the response clock and the record time, in milliseconds.
    pub max_clock_offset_ms: u64,
    /// Count per session token length.
    pub token_lengths: BTreeMap<usize, u64>,
}

/// Totals over every checked ledger.
#[derive(Debug, Default)]
pub struct CaptureReport {
    /// Ledger files read.
    pub files: usize,
    /// Datagrams by direction.
    pub datagrams: [u64; DIRECTIONS],
    /// Messages by direction.
    pub messages: [u64; DIRECTIONS],
    /// Messages by direction and type index.
    pub types: BTreeMap<(u32, u64), u64>,
    /// Messages whose type is an inline UUID.
    pub inline_uuid_messages: u64,
    /// Registration responses.
    pub registration: RegistrationSummary,
    /// Bodies of the M4 and M6-B types (server messages, the client input 1589 and the respawn
    /// request 1938) that decode and encode to the same bytes, by type index.
    pub bodies: BTreeMap<u64, u64>,
    /// Bundle members with a reader that decode and encode to the same bytes, by class index.
    pub members: BTreeMap<u64, u64>,
    /// Bundle payloads whose members all have a reader, decoded and encoded to the same bytes.
    pub complete_payloads: u64,
    /// Client connect requests that started new streams in a context that had streams.
    pub connections: u64,
    /// Decode failures.
    pub failures: Vec<DecodeFailure>,
}

impl CaptureReport {
    /// Decodes every record of one ledger into this report.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] when the ledger structure itself is broken. Carrier and message
    /// failures are recorded in [`CaptureReport::failures`].
    ///
    /// One capture context can hold several connections in a row (a move into or out of an
    /// instance): a client connect request after the server delivered messages starts new
    /// streams for both directions of that context.
    pub fn check_ledger(&mut self, file: &Path, bytes: &[u8]) -> Result<(), CaptureError> {
        self.files += 1;
        let mut streams: HashMap<(u64, u32), Option<Receiver>> = HashMap::new();
        let mut established: HashSet<u64> = HashSet::new();
        for (index, record) in parse_ledger(bytes)?.into_iter().enumerate() {
            let direction = record.direction as usize;
            self.datagrams[direction] += 1;
            if record.direction == CLIENT_TO_SERVER
                && is_connect_request(record.datagram)
                && established.remove(&record.context)
            {
                self.connections += 1;
                for direction in [SERVER_TO_CLIENT, CLIENT_TO_SERVER] {
                    streams.remove(&(record.context, direction));
                }
            }
            let server_messages = self.messages[SERVER_TO_CLIENT as usize];
            let stream = streams
                .entry((record.context, record.direction))
                .or_insert_with(|| Some(Receiver::mid_stream(ReceiveLimits::SESSION)));
            let Some(receiver) = stream else {
                continue;
            };
            if let Err(reason) = self.check_record(receiver, &record) {
                self.failures.push(DecodeFailure {
                    file: file.to_path_buf(),
                    record: index,
                    reason,
                });
                *stream = None;
            }
            if self.messages[SERVER_TO_CLIENT as usize] > server_messages {
                established.insert(record.context);
            }
        }
        Ok(())
    }

    fn check_record(
        &mut self,
        receiver: &mut Receiver,
        record: &LedgerRecord<'_>,
    ) -> Result<(), String> {
        let datagram = decode_datagram(record.datagram).map_err(|error| error.to_string())?;
        let delivered = receiver
            .receive(&datagram)
            .map_err(|error| error.to_string())?;
        for item in delivered {
            let Inbound::Message(message) = item else {
                continue;
            };
            let envelopes = if record.direction == CLIENT_TO_SERVER {
                vec![
                    decode_client_message(&message.payload)
                        .map_err(|error| error.to_string())?
                        .envelope,
                ]
            } else {
                decode_server_messages(&message.payload).map_err(|error| error.to_string())?
            };
            for bytes in envelopes {
                let envelope = Envelope::decode(bytes).map_err(|error| error.to_string())?;
                self.count(record, &envelope)?;
            }
        }
        Ok(())
    }

    fn count(&mut self, record: &LedgerRecord<'_>, envelope: &Envelope) -> Result<(), String> {
        self.messages[record.direction as usize] += 1;
        let Some(index) = envelope.type_index() else {
            self.inline_uuid_messages += 1;
            return Ok(());
        };
        *self.types.entry((record.direction, index)).or_default() += 1;
        if record.direction == SERVER_TO_CLIENT && index == REGISTRATION_RESPONSE_MSG_TYPE_INDEX {
            let response = RegistrationResponseMsg::decode(&envelope.body)
                .map_err(|error| error.to_string())?;
            let summary = &mut self.registration;
            summary.responses += 1;
            *summary.statuses.entry(response.uint32_at_0x8).or_default() += 1;
            *summary.flags.entry(response.bytes_at_0x58).or_default() += 1;
            let version = String::from_utf8_lossy(&response.bytes_at_0x38).into_owned();
            *summary.versions.entry(version).or_default() += 1;
            *summary
                .token_lengths
                .entry(response.bytes_at_0x18.len())
                .or_default() += 1;
            let clock_ms = response.uint64 / NANOS_PER_MILLI + CLOCK_EPOCH_UNIX_MS;
            summary.max_clock_offset_ms = summary
                .max_clock_offset_ms
                .max(clock_ms.abs_diff(record.time_ms));
        }
        if record.direction == SERVER_TO_CLIENT
            && let Some(decoded) = ServerMessage::decode(index, &envelope.body)
        {
            let message = decoded.map_err(|error| format!("type {index}: {error}"))?;
            self.same_bytes(index, message.encode(), &envelope.body)?;
            if let ServerMessage::Bundle(bundle) = message {
                self.check_members(&bundle)?;
            }
        }
        if record.direction == CLIENT_TO_SERVER && index == RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX {
            let input = ClientInputMsg::decode(&envelope.body)
                .map_err(|error| format!("type {index}: {error}"))?;
            self.same_bytes(index, input.encode(), &envelope.body)?;
        }
        if record.direction == CLIENT_TO_SERVER && index == REQUEST_RESPAWN_TYPE_INDEX {
            let request = RequestRespawnMsg::decode(&envelope.body)
                .map_err(|error| format!("type {index}: {error}"))?;
            self.same_bytes(index, request.encode(), &envelope.body)?;
        }
        Ok(())
    }

    /// Counts a body of type `index` whose encoding is the captured `body`.
    fn same_bytes(
        &mut self,
        index: u64,
        encoded: Result<Vec<u8>, MessageError>,
        body: &[u8],
    ) -> Result<(), String> {
        let encoded = encoded.map_err(|error| format!("type {index}: {error}"))?;
        if encoded != body {
            return Err(format!(
                "type {index}: encoded body differs from the capture"
            ));
        }
        *self.bodies.entry(index).or_default() += 1;
        Ok(())
    }

    /// Decodes the members at the front of a bundle payload, encodes each again, and compares.
    fn check_members(&mut self, bundle: &ReplicatedStateBundle) -> Result<(), String> {
        let known = BundlePayload::decode_known(&bundle.payload)
            .map_err(|error| format!("bundle payload: {error}"))?;
        for (member, range) in known.members {
            let encoded = member
                .encode()
                .map_err(|error| format!("bundle member: {error}"))?;
            if encoded != bundle.payload[range] {
                return Err("bundle member: encoded member differs from the capture".to_owned());
            }
            *self.members.entry(member.state.type_index()).or_default() += 1;
        }
        if known.stopped_at.is_none() {
            let whole = BundlePayload::decode(&bundle.payload)
                .and_then(|payload| payload.encode())
                .map_err(|error| format!("bundle payload: {error}"))?;
            if whole != bundle.payload {
                return Err("bundle payload: encoded payload differs from the capture".to_owned());
            }
            self.complete_payloads += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nw_capture::LEDGER_MAGIC;
    use nw_carrier::receiver::FIRST_RELIABLE_SEQUENCE;
    use nw_carrier::system::CARRIER_VERSION;
    use nw_carrier::{Frame, SYSTEM_CHANNEL, SystemMessage, encode_datagram, encode_frames};
    use nw_reflect::{
        PING_MSG_TYPE_INDEX, REGISTRATION_RESPONSE_OBSERVED_FLAGS, REGISTRATION_STATUS_AUTHORIZED,
        TIME_SYNCH_MSG_TYPE_INDEX, TypeId, encode_client_message, encode_server_messages,
    };

    use nw_messages::{
        BodyCodec, BundleGroup, BundleMember, BundlePayload, ClientInputMsg,
        DAMAGE_DEALT_TYPE_INDEX, DEATH_RECAP_TYPE_INDEX, DamageDealtMsg, DeathRecapMsg,
        InputPosition, MemberState, ON_DAMAGE_TYPE_INDEX, OnDamageMsg,
        POSITION_IN_THE_WORLD_TYPE_INDEX, PositionInTheWorldReplicatedState,
        RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX, REPLICATED_STATE_BUNDLE_TYPE_INDEX,
        REQUEST_RESPAWN_TYPE_INDEX, ReplicatedStateBundle, RequestRespawnMsg, VITALS_TYPE_INDEX,
        WorldPosition, Writer,
    };

    use super::*;

    const CONTEXT: u64 = 7;
    const TIME_MS: u64 = 1_782_067_831_638;
    const DATAGRAM_SEQUENCE: u16 = 2;
    const RELIABLE_START: u16 = 10;
    const PING_BODY: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];
    const ZERO_SESSION: [u8; 16] = [0; 16];
    const TOKEN: &[u8; 32] = b"0123456789ABCDEF0123456789ABCDEF";
    const VERSION: &str = "[TEST].Javelin.1.365.6031.1";
    const CLOCK_OFFSET_MS: u64 = 250;
    /// The integer 1 in the two-byte form of the prefix code.
    const LONG_FORM_ONE: [u8; 2] = [0x81, 0x00];

    fn record(direction: u32, datagram: &[u8]) -> Vec<u8> {
        let mut out = LEDGER_MAGIC.to_vec();
        out.extend_from_slice(&TIME_MS.to_le_bytes());
        out.extend_from_slice(&direction.to_le_bytes());
        out.extend_from_slice(&CONTEXT.to_le_bytes());
        out.extend_from_slice(&u32::try_from(datagram.len()).expect("small").to_le_bytes());
        out.extend_from_slice(datagram);
        out
    }

    fn reliable_datagram(payload: Vec<u8>) -> Vec<u8> {
        reliable_datagram_at(RELIABLE_START, payload)
    }

    fn reliable_datagram_at(reliable_sequence: u16, payload: Vec<u8>) -> Vec<u8> {
        let frame = Frame {
            channel: 0,
            reliable: true,
            connecting: false,
            chunks_remaining: None,
            sequence: reliable_sequence,
            reliable_sequence,
            payload,
        };
        encode_datagram(DATAGRAM_SEQUENCE, &encode_frames(&[frame]).expect("frames"))
    }

    fn envelope(index: u64, body: Vec<u8>) -> Vec<u8> {
        Envelope::message(TypeId::FileSlot(index), body)
            .encode()
            .expect("envelope")
    }

    /// A bundle whose payload is one group with one position member.
    fn position_bundle() -> Vec<u8> {
        let payload = BundlePayload {
            groups: vec![BundleGroup {
                leading: 1,
                members: vec![BundleMember {
                    u64_at_0: 2,
                    state: MemberState::Position(PositionInTheWorldReplicatedState {
                        position: WorldPosition::from_metres(1.0, 2.0, 3.0),
                        ..PositionInTheWorldReplicatedState::default()
                    }),
                }],
            }],
        };
        ReplicatedStateBundle {
            leading: None,
            byte_3: 0,
            byte_4: 0,
            flag_5: false,
            index_block: None,
            payload: payload.encode().expect("payload"),
        }
        .encode()
        .expect("bundle")
    }

    fn sample_ledger() -> Vec<u8> {
        let response = RegistrationResponseMsg {
            uint32_at_0x8: REGISTRATION_STATUS_AUTHORIZED,
            uint64: (TIME_MS - CLOCK_EPOCH_UNIX_MS + CLOCK_OFFSET_MS) * NANOS_PER_MILLI,
            bytes_at_0x18: TOKEN.to_vec(),
            bytes_at_0x38: VERSION.as_bytes().to_vec(),
            bytes_at_0x58: REGISTRATION_RESPONSE_OBSERVED_FLAGS,
        };
        let registration = envelope(
            REGISTRATION_RESPONSE_MSG_TYPE_INDEX,
            response.encode().expect("response"),
        );
        let ping = envelope(PING_MSG_TYPE_INDEX, PING_BODY.to_vec());
        let time = envelope(TIME_SYNCH_MSG_TYPE_INDEX, PING_BODY.to_vec());
        let bundle = envelope(REPLICATED_STATE_BUNDLE_TYPE_INDEX, position_bundle());
        let server =
            encode_server_messages(&[&registration, &ping, &time, &bundle]).expect("server");
        let client = encode_client_message(&ZERO_SESSION, &ping).expect("client");
        let mut ledger = record(SERVER_TO_CLIENT, &reliable_datagram(server));
        ledger.extend(record(CLIENT_TO_SERVER, &reliable_datagram(client)));
        ledger
    }

    #[test]
    fn counts_messages_and_registration_values() {
        let mut report = CaptureReport::default();
        report
            .check_ledger(Path::new("sample"), &sample_ledger())
            .expect("check");
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(report.datagrams, [1, 1]);
        assert_eq!(report.messages, [4, 1]);
        assert_eq!(report.types[&(CLIENT_TO_SERVER, PING_MSG_TYPE_INDEX)], 1);
        assert_eq!(report.bodies[&TIME_SYNCH_MSG_TYPE_INDEX], 1);
        assert_eq!(report.members[&POSITION_IN_THE_WORLD_TYPE_INDEX], 1);
        assert_eq!(report.complete_payloads, 1);
        let summary = &report.registration;
        assert_eq!(summary.responses, 1);
        assert_eq!(summary.statuses[&REGISTRATION_STATUS_AUTHORIZED], 1);
        assert_eq!(summary.flags[&REGISTRATION_RESPONSE_OBSERVED_FLAGS], 1);
        assert_eq!(summary.versions[VERSION], 1);
        assert_eq!(summary.token_lengths[&TOKEN.len()], 1);
        assert_eq!(summary.max_clock_offset_ms, CLOCK_OFFSET_MS);
    }

    /// A client connect request, as the first datagram of a new connection sends it.
    fn connect_request_datagram() -> Vec<u8> {
        let frame = Frame {
            channel: SYSTEM_CHANNEL,
            reliable: false,
            connecting: true,
            chunks_remaining: None,
            sequence: 0,
            reliable_sequence: 0,
            payload: SystemMessage::ConnectRequest {
                version: CARRIER_VERSION,
                extra: Vec::new(),
            }
            .encode(),
        };
        encode_datagram(DATAGRAM_SEQUENCE, &encode_frames(&[frame]).expect("frames"))
    }

    #[test]
    fn a_whole_payload_must_encode_to_the_same_bytes() {
        let member = BundleMember {
            u64_at_0: 2,
            state: MemberState::Position(PositionInTheWorldReplicatedState::default()),
        };
        // The group integer 1 in the two-byte form; the encoder writes the one-byte form.
        let mut payload = LONG_FORM_ONE.to_vec();
        payload.push(1);
        payload.extend(member.encode().expect("member"));
        let bundle = ReplicatedStateBundle {
            leading: None,
            byte_3: 0,
            byte_4: 0,
            flag_5: false,
            index_block: None,
            payload,
        };
        let body = bundle.encode().expect("bundle");
        let server = encode_server_messages(&[&envelope(REPLICATED_STATE_BUNDLE_TYPE_INDEX, body)])
            .expect("server");
        let mut report = CaptureReport::default();
        report
            .check_ledger(
                Path::new("long form"),
                &record(SERVER_TO_CLIENT, &reliable_datagram(server)),
            )
            .expect("check");
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].reason.starts_with("bundle payload:"));
    }

    #[test]
    fn a_new_connection_in_the_same_context_starts_new_streams() {
        let ping = envelope(PING_MSG_TYPE_INDEX, PING_BODY.to_vec());
        let server = encode_server_messages(&[&ping]).expect("server");
        let mut ledger = record(SERVER_TO_CLIENT, &reliable_datagram(server.clone()));
        ledger.extend(record(CLIENT_TO_SERVER, &connect_request_datagram()));
        ledger.extend(record(
            SERVER_TO_CLIENT,
            &reliable_datagram_at(FIRST_RELIABLE_SEQUENCE, server),
        ));
        let mut report = CaptureReport::default();
        report
            .check_ledger(Path::new("reconnect"), &ledger)
            .expect("check");
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(report.messages, [2, 0]);
        assert_eq!(report.connections, 1);
    }

    #[test]
    fn a_bad_crc_is_a_failure_and_stops_that_stream() {
        let ping = envelope(PING_MSG_TYPE_INDEX, PING_BODY.to_vec());
        let mut client = encode_client_message(&ZERO_SESSION, &ping).expect("client");
        client[0] ^= 0xFF;
        let mut ledger = record(CLIENT_TO_SERVER, &reliable_datagram(client));
        ledger.extend(record(CLIENT_TO_SERVER, &reliable_datagram(Vec::new())));
        let mut report = CaptureReport::default();
        report
            .check_ledger(Path::new("bad"), &ledger)
            .expect("check");
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].record, 0);
        assert!(report.failures[0].reason.contains("CRC"));
        assert_eq!(report.messages, [0, 0]);
    }

    #[test]
    fn a_known_body_that_does_not_decode_is_a_failure() {
        let short = envelope(TIME_SYNCH_MSG_TYPE_INDEX, PING_BODY[1..].to_vec());
        let server = encode_server_messages(&[&short]).expect("server");
        let mut report = CaptureReport::default();
        report
            .check_ledger(
                Path::new("short"),
                &record(SERVER_TO_CLIENT, &reliable_datagram(server)),
            )
            .expect("check");
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].reason.contains("type 335"));
        assert!(report.bodies.is_empty());
    }

    /// One server datagram that carries `envelopes`, reliable, in order.
    fn server_datagram(envelopes: &[Vec<u8>]) -> Vec<u8> {
        let slices: Vec<&[u8]> = envelopes.iter().map(Vec::as_slice).collect();
        reliable_datagram(encode_server_messages(&slices).expect("server"))
    }

    #[test]
    fn new_messages_are_byte_checked() {
        const TARGET: u64 = (1 << 34) + 3;
        const ATTACKER: u64 = 7;
        let envelopes = [
            envelope(
                DAMAGE_DEALT_TYPE_INDEX,
                DamageDealtMsg::hit(TARGET, ATTACKER, 42.0)
                    .encode()
                    .expect("2071"),
            ),
            envelope(
                ON_DAMAGE_TYPE_INDEX,
                OnDamageMsg::hit(ATTACKER, TARGET, 9.0)
                    .encode()
                    .expect("3601"),
            ),
            envelope(
                DEATH_RECAP_TYPE_INDEX,
                DeathRecapMsg::of(ATTACKER, TARGET).encode().expect("4299"),
            ),
        ];
        let mut report = CaptureReport::default();
        report
            .check_ledger(
                Path::new("combat"),
                &record(SERVER_TO_CLIENT, &server_datagram(&envelopes)),
            )
            .expect("check");
        for index in [
            DAMAGE_DEALT_TYPE_INDEX,
            ON_DAMAGE_TYPE_INDEX,
            DEATH_RECAP_TYPE_INDEX,
        ] {
            assert_eq!(report.bodies.get(&index), Some(&1), "type {index}");
        }
        let respawn = RequestRespawnMsg {
            head_a: ATTACKER,
            head_b: 0,
            flag: false,
            payload: vec![1; 22],
        };
        let request = envelope(REQUEST_RESPAWN_TYPE_INDEX, respawn.encode().expect("1938"));
        let frame = Frame {
            channel: 0,
            reliable: true,
            connecting: false,
            chunks_remaining: None,
            sequence: RELIABLE_START,
            reliable_sequence: RELIABLE_START,
            payload: encode_client_message(&ZERO_SESSION, &request).expect("client"),
        };
        let datagram =
            encode_datagram(DATAGRAM_SEQUENCE, &encode_frames(&[frame]).expect("frames"));
        report
            .check_ledger(Path::new("respawn"), &record(CLIENT_TO_SERVER, &datagram))
            .expect("check");
        assert_eq!(report.bodies.get(&REQUEST_RESPAWN_TYPE_INDEX), Some(&1));
        assert!(report.failures.is_empty(), "{:?}", report.failures);
    }

    #[test]
    fn a_changed_byte_is_a_difference() {
        /// The item count of a 2071: after two u64, 16 bytes, a u64, two u32 and 2 bytes.
        const COUNT_AT: usize = 50;
        /// The count 1 in the two-byte wide form: it decodes, but encodes to one byte.
        const LONG_ONE: [u8; 2] = [0x81, 0x00];
        let mut body = DamageDealtMsg::hit((1 << 34) + 3, 7, 42.0)
            .encode()
            .expect("2071");
        assert_eq!(body[COUNT_AT], 1, "one item");
        body.splice(COUNT_AT..=COUNT_AT, LONG_ONE);
        assert!(
            DamageDealtMsg::decode(&body).is_ok(),
            "a form the reader takes"
        );
        let mut report = CaptureReport::default();
        report
            .check_ledger(
                Path::new("combat"),
                &record(
                    SERVER_TO_CLIENT,
                    &server_datagram(&[envelope(DAMAGE_DEALT_TYPE_INDEX, body)]),
                ),
            )
            .expect("check");
        assert_eq!(report.failures.len(), 1, "{:?}", report.failures);
        assert!(report.failures[0].reason.contains("differs"));
        assert_eq!(report.bodies.get(&DAMAGE_DEALT_TYPE_INDEX), None);
    }

    #[test]
    fn a_vitals_member_on_another_slot_is_skipped_not_failed() {
        let mut payload = Writer::new();
        payload
            .prefixed(1)
            .expect("leading")
            .u8(1)
            .prefixed(2)
            .expect("entity");
        payload.bytes(
            &nw_reflect::encode_type_id(&TypeId::FileSlot(VITALS_TYPE_INDEX)).expect("class"),
        );
        payload.u8(0b10).u8(0);
        let bundle = ReplicatedStateBundle {
            leading: None,
            byte_3: 0,
            byte_4: 0,
            flag_5: false,
            index_block: None,
            payload: payload.finish(),
        }
        .encode()
        .expect("bundle");
        let mut report = CaptureReport::default();
        report
            .check_ledger(
                Path::new("vitals"),
                &record(
                    SERVER_TO_CLIENT,
                    &server_datagram(&[envelope(REPLICATED_STATE_BUNDLE_TYPE_INDEX, bundle)]),
                ),
            )
            .expect("check");
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(report.complete_payloads, 0);
    }

    #[test]
    fn client_inputs_are_decoded_and_compared() {
        let position = InputPosition::from_metres(1.0, 2.0, 3.0).expect("finite");
        let input = ClientInputMsg::position(position).encode().expect("input");
        let good = envelope(RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX, input.clone());
        let short = envelope(
            RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX,
            input[..input.len() - 1].to_vec(),
        );
        let frames: Vec<Frame> = [good, short]
            .iter()
            .zip(RELIABLE_START..)
            .map(|(envelope, reliable_sequence)| Frame {
                channel: 0,
                reliable: true,
                connecting: false,
                chunks_remaining: None,
                sequence: reliable_sequence,
                reliable_sequence,
                payload: encode_client_message(&ZERO_SESSION, envelope).expect("client"),
            })
            .collect();
        let datagram = encode_datagram(DATAGRAM_SEQUENCE, &encode_frames(&frames).expect("frames"));
        let mut report = CaptureReport::default();
        report
            .check_ledger(Path::new("inputs"), &record(CLIENT_TO_SERVER, &datagram))
            .expect("check");
        assert_eq!(report.bodies[&RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX], 1);
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].reason.contains("type 1589"));
    }
}
