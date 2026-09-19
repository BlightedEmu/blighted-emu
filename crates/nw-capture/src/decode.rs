// SPDX-License-Identifier: AGPL-3.0-or-later
//! The decode chain of a ledger (carrier captures note): datagram, carrier streams per
//! connection context and direction, messages, envelopes; then the bodies this project reads.
//!
//! One capture context can hold several connections in a row (a move into or out of an
//! instance): a client connect request after the server delivered messages starts new streams
//! for both directions of that context. A carrier or framing failure stops that stream.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use nw_carrier::{Inbound, ReceiveLimits, Receiver, decode_datagram};
use nw_messages::{
    BodyCodec, BundlePayload, ClientInputMsg, MemberState, RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX,
    REPLICATED_STATE_BUNDLE_TYPE_INDEX, ReplicatedStateBundle,
};
use nw_reflect::{Envelope, TypeId, decode_client_message, decode_server_messages};

use crate::{CLIENT_TO_SERVER, LedgerRecord, SERVER_TO_CLIENT, is_connect_request};

/// The class index of a member whose class is an inline UUID (index 0 on the wire).
pub const INLINE_UUID_CLASS: u64 = 0;

/// One envelope a record delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveredEnvelope {
    /// Carrier channel.
    pub channel: u8,
    /// Whether the message came reliable.
    pub reliable: bool,
    /// The envelope.
    pub envelope: Envelope,
}

/// What one record delivered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordOutput {
    /// The envelopes, in order; on a failure, those before it.
    pub envelopes: Vec<DeliveredEnvelope>,
    /// The failure that stopped this stream.
    pub error: Option<String>,
    /// Whether this record started new streams in its context (a client connect request after
    /// the server delivered messages).
    pub new_connection: bool,
}

/// The carrier streams of one ledger.
#[derive(Debug, Default)]
pub struct Decoder {
    /// `None` once the stream failed.
    streams: HashMap<(u64, u32), Option<Receiver>>,
    /// Contexts whose server stream delivered messages.
    established: HashSet<u64>,
}

impl Decoder {
    /// A decoder before the first record.
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes one record.
    pub fn record(&mut self, record: &LedgerRecord<'_>) -> RecordOutput {
        let mut output = RecordOutput::default();
        if record.direction == CLIENT_TO_SERVER
            && is_connect_request(record.datagram)
            && self.established.remove(&record.context)
        {
            output.new_connection = true;
            for direction in [SERVER_TO_CLIENT, CLIENT_TO_SERVER] {
                self.streams.remove(&(record.context, direction));
            }
        }
        let stream = self
            .streams
            .entry((record.context, record.direction))
            .or_insert_with(|| Some(Receiver::mid_stream(ReceiveLimits::SESSION)));
        let Some(receiver) = stream else {
            return output;
        };
        if let Err(reason) = deliver(receiver, record, &mut output.envelopes) {
            output.error = Some(reason);
            *stream = None;
        }
        if record.direction == SERVER_TO_CLIENT && !output.envelopes.is_empty() {
            self.established.insert(record.context);
        }
        output
    }

    /// Stops the stream of `context` and `direction`: its later records deliver nothing.
    pub fn stop(&mut self, context: u64, direction: u32) {
        self.streams.insert((context, direction), None);
    }
}

fn deliver(
    receiver: &mut Receiver,
    record: &LedgerRecord<'_>,
    out: &mut Vec<DeliveredEnvelope>,
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
            out.push(DeliveredEnvelope {
                channel: message.channel,
                reliable: message.reliable,
                envelope: Envelope::decode(bytes).map_err(|error| error.to_string())?,
            });
        }
    }
    Ok(())
}

/// One decoded message and where it sits.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    /// Record number in the ledger.
    pub record: u32,
    /// Envelope number among those the record delivered.
    pub slot: u16,
    /// Unix milliseconds of the record.
    pub time_ms: u64,
    /// [`SERVER_TO_CLIENT`] or [`CLIENT_TO_SERVER`].
    pub direction: u32,
    /// Carrier channel.
    pub channel: u8,
    /// Whether the message came reliable.
    pub reliable: bool,
    /// Type index; `None` for an inline UUID.
    pub type_index: Option<u64>,
    /// Body bytes.
    pub size: u32,
    /// What this project reads of the body.
    pub detail: Detail,
}

/// What this project reads of a body.
#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    /// Nothing.
    None,
    /// A replication bundle: the members at the front of its payload that have a reader.
    Bundle {
        /// The members.
        members: Vec<MemberRef>,
        /// The class of the first member without a reader.
        stopped_at: Option<u64>,
        /// The member whose reader failed; the walk ends there.
        failed: Option<MemberFailure>,
    },
    /// The client input (type 1589).
    Input {
        /// The position of its last record that has one, in metres.
        position: Option<[f32; 3]>,
    },
    /// A body of a type with a reader that does not decode.
    Failed(String),
}

/// A bundle member whose reader failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberFailure {
    /// Class type index; 0 for an inline UUID class.
    pub class: u64,
    /// The error.
    pub reason: String,
}

/// One bundle member.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberRef {
    /// The member's first integer: the entity's replication key.
    pub key: u64,
    /// Class type index.
    pub class: u64,
    /// The member's bytes in the message body.
    pub range: Range<u32>,
    /// Its position in metres (types 13 and 11), when finite.
    pub position: Option<[f32; 3]>,
}

/// What this project reads of one delivered envelope.
pub fn describe(
    record_index: u32,
    slot: u16,
    record: &LedgerRecord<'_>,
    delivered: &DeliveredEnvelope,
) -> Decoded {
    let body = &delivered.envelope.body;
    let type_index = delivered.envelope.type_index();
    let detail = match (record.direction, type_index) {
        (SERVER_TO_CLIENT, Some(REPLICATED_STATE_BUNDLE_TYPE_INDEX)) => bundle_detail(body),
        (CLIENT_TO_SERVER, Some(RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX)) => {
            match ClientInputMsg::decode(body) {
                Ok(input) => Detail::Input {
                    position: input
                        .newest_position()
                        .and_then(|at| finite([at.x, at.y, at.z()])),
                },
                Err(error) => Detail::Failed(error.to_string()),
            }
        }
        _ => Detail::None,
    };
    Decoded {
        record: record_index,
        slot,
        time_ms: record.time_ms,
        direction: record.direction,
        channel: delivered.channel,
        reliable: delivered.reliable,
        type_index,
        size: u32::try_from(body.len()).unwrap_or(u32::MAX),
        detail,
    }
}

fn bundle_detail(body: &[u8]) -> Detail {
    let bundle = match ReplicatedStateBundle::decode(body) {
        Ok(bundle) => bundle,
        Err(error) => return Detail::Failed(error.to_string()),
    };
    let known = match BundlePayload::decode_known(&bundle.payload) {
        Ok(known) => known,
        Err(error) => return Detail::Failed(error.to_string()),
    };
    // The payload is the end of the body.
    let offset = body.len() - bundle.payload.len();
    let members = known
        .members
        .iter()
        .map(|(member, range)| MemberRef {
            key: member.component,
            class: member.state.type_index(),
            range: to_u32(offset + range.start)..to_u32(offset + range.end),
            position: member_position(&member.state),
        })
        .collect();
    let stopped_at = known.stopped_at.and_then(|class| match class {
        TypeId::FileSlot(index) => Some(index),
        TypeId::InlineUuid(_) => None,
    });
    let failed = known.failed.map(|(class, reason)| MemberFailure {
        class: match class {
            TypeId::FileSlot(index) => index,
            TypeId::InlineUuid(_) => INLINE_UUID_CLASS,
        },
        reason,
    });
    Detail::Bundle {
        members,
        stopped_at,
        failed,
    }
}

fn member_position(state: &MemberState) -> Option<[f32; 3]> {
    match state {
        MemberState::Position(position) => position
            .position
            .and_then(|at| finite([at.x, at.y, at.z()])),
        MemberState::Alc(alc) => alc.pos10.and_then(|at| finite([at.x, at.y, at.z()])),
        MemberState::GdeMetadata(_)
        | MemberState::Vitals(_)
        | MemberState::Stamina(_)
        | MemberState::CombatStatus(_)
        | MemberState::DelayedEvent(_)
        | MemberState::LookTargeting(_)
        | MemberState::Gathering(_)
        | MemberState::Interactor(_)
        | MemberState::PlayerComponent(_)
        | MemberState::Appearance(_)
        | MemberState::Mana(_)
        | MemberState::Paperdoll(_)
        | MemberState::Container(_)
        | MemberState::Achievement(_)
        | MemberState::DamageReceiver(_)
        | MemberState::GatherableController(_)
        | MemberState::Interact(_)
        | MemberState::ItemManagement(_)
        | MemberState::Objectives(_)
        | MemberState::Progression(_)
        | MemberState::Cooldowns(_)
        | MemberState::Empty(_)
        | MemberState::Recorded { .. } => None,
    }
}

fn finite(position: [f32; 3]) -> Option<[f32; 3]> {
    position
        .iter()
        .all(|value| value.is_finite())
        .then_some(position)
}

fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use nw_messages::{
        BodyCodec, ClientInputMsg, InputPosition, POSITION_IN_THE_WORLD_TYPE_INDEX,
        RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX, REPLICATED_STATE_BUNDLE_TYPE_INDEX, Writer,
    };
    use nw_reflect::{PING_MSG_TYPE_INDEX, TypeId};

    use super::*;
    use crate::test_support::{
        TEST_ENTITY, bundle_of, client_datagram, connect_request_datagram, envelope,
        position_bundle, record, server_datagram, server_datagram_at,
    };
    use crate::{CLIENT_TO_SERVER, SERVER_TO_CLIENT, parse_ledger};

    const PING_BODY: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];
    /// A class without a reader.
    const UNREAD_CLASS: u64 = 14;

    fn outputs(ledger: &[u8]) -> Vec<RecordOutput> {
        let mut decoder = Decoder::new();
        parse_ledger(ledger)
            .expect("ledger")
            .iter()
            .map(|record| decoder.record(record))
            .collect()
    }

    fn described(ledger: &[u8]) -> Vec<Decoded> {
        let mut decoder = Decoder::new();
        let mut all = Vec::new();
        for (index, record) in parse_ledger(ledger).expect("ledger").iter().enumerate() {
            let output = decoder.record(record);
            for (slot, delivered) in output.envelopes.iter().enumerate() {
                all.push(describe(
                    u32::try_from(index).expect("small"),
                    u16::try_from(slot).expect("small"),
                    record,
                    delivered,
                ));
            }
        }
        all
    }

    #[test]
    fn a_connect_request_after_server_traffic_is_a_new_connection() {
        let ping = envelope(PING_MSG_TYPE_INDEX, PING_BODY.to_vec());
        let mut ledger = record(
            SERVER_TO_CLIENT,
            &server_datagram(std::slice::from_ref(&ping)),
        );
        ledger.extend(record(CLIENT_TO_SERVER, &connect_request_datagram()));
        ledger.extend(record(
            SERVER_TO_CLIENT,
            &server_datagram_at(nw_carrier::receiver::FIRST_RELIABLE_SEQUENCE, &[ping]),
        ));
        let outputs = outputs(&ledger);
        assert!(!outputs[0].new_connection);
        assert!(outputs[1].new_connection);
        assert_eq!(outputs[2].envelopes.len(), 1, "the new stream starts again");
    }

    #[test]
    fn a_carrier_error_stops_only_that_stream() {
        let ping = envelope(PING_MSG_TYPE_INDEX, PING_BODY.to_vec());
        let mut ledger = record(CLIENT_TO_SERVER, &[0xFF, 0xFF]);
        ledger.extend(record(
            CLIENT_TO_SERVER,
            &client_datagram(10, std::slice::from_ref(&ping)),
        ));
        ledger.extend(record(SERVER_TO_CLIENT, &server_datagram(&[ping])));
        let outputs = outputs(&ledger);
        assert!(outputs[0].error.is_some());
        assert!(outputs[1].envelopes.is_empty() && outputs[1].error.is_none());
        assert_eq!(outputs[2].envelopes.len(), 1);
    }

    #[test]
    fn a_position_bundle_gives_a_member_with_its_bytes() {
        let ledger = record(
            SERVER_TO_CLIENT,
            &server_datagram(&[envelope(
                REPLICATED_STATE_BUNDLE_TYPE_INDEX,
                position_bundle(),
            )]),
        );
        let mut decoder = Decoder::new();
        let records = parse_ledger(&ledger).expect("ledger");
        let output = decoder.record(&records[0]);
        let decoded = describe(0, 0, &records[0], &output.envelopes[0]);
        assert_eq!(decoded.type_index, Some(REPLICATED_STATE_BUNDLE_TYPE_INDEX));
        let Detail::Bundle {
            members,
            stopped_at,
            ..
        } = decoded.detail
        else {
            panic!("a bundle");
        };
        assert_eq!(stopped_at, None);
        assert_eq!(members.len(), 1);
        let member = &members[0];
        assert_eq!(
            (member.key, member.class),
            (TEST_ENTITY, POSITION_IN_THE_WORLD_TYPE_INDEX)
        );
        let [x, y, _] = member.position.expect("position");
        assert_eq!((x, y), (1.0, 2.0));
        let body = &output.envelopes[0].envelope.body;
        let range = member.range.start as usize..member.range.end as usize;
        let read = nw_messages::BundlePayload::decode_known(
            &nw_messages::ReplicatedStateBundle::decode(body)
                .expect("bundle")
                .payload,
        )
        .expect("members");
        assert_eq!(read.members[0].0.encode().expect("member"), body[range]);
    }

    #[test]
    fn a_bundle_with_an_unread_class_records_where_it_stopped() {
        let mut payload = Writer::new();
        payload
            .prefixed(1)
            .expect("group")
            .u8(1)
            .prefixed(TEST_ENTITY)
            .expect("key");
        payload.bytes(&nw_reflect::encode_type_id(&TypeId::FileSlot(UNREAD_CLASS)).expect("class"));
        payload.u8(0);
        let ledger = record(
            SERVER_TO_CLIENT,
            &server_datagram(&[envelope(
                REPLICATED_STATE_BUNDLE_TYPE_INDEX,
                bundle_of(payload.finish()),
            )]),
        );
        let decoded = described(&ledger);
        assert!(matches!(
            &decoded[0].detail,
            Detail::Bundle { members, stopped_at: Some(UNREAD_CLASS), .. } if members.is_empty()
        ));
    }

    #[test]
    fn a_failing_member_keeps_the_members_before_it() {
        let ledger = record(
            SERVER_TO_CLIENT,
            &server_datagram(&[envelope(
                REPLICATED_STATE_BUNDLE_TYPE_INDEX,
                crate::test_support::bundle_with_a_failing_member(),
            )]),
        );
        let Detail::Bundle {
            members,
            stopped_at,
            failed,
        } = &described(&ledger)[0].detail
        else {
            panic!("a bundle");
        };
        assert_eq!(members.len(), 1);
        assert_eq!(*stopped_at, None);
        let failed = failed.as_ref().expect("the failing member");
        assert_eq!(failed.class, nw_messages::ALC_TYPE_INDEX);
        assert!(failed.reason.starts_with("member of class"));
    }

    #[test]
    fn client_input_gives_a_position() {
        let position = InputPosition::from_metres(5.0, 6.0, 7.0).expect("finite");
        let input = ClientInputMsg::position(position).encode().expect("input");
        let ledger = record(
            CLIENT_TO_SERVER,
            &client_datagram(10, &[envelope(RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX, input)]),
        );
        let decoded = described(&ledger);
        let Detail::Input {
            position: Some([x, y, _]),
        } = decoded[0].detail
        else {
            panic!("an input with a position");
        };
        assert_eq!((x, y), (5.0, 6.0));
        assert_eq!(decoded[0].direction, CLIENT_TO_SERVER);
    }

    #[test]
    fn a_body_that_does_not_decode_is_a_failed_detail() {
        let ledger = record(
            SERVER_TO_CLIENT,
            &server_datagram(&[envelope(REPLICATED_STATE_BUNDLE_TYPE_INDEX, vec![1])]),
        );
        assert!(matches!(described(&ledger)[0].detail, Detail::Failed(_)));
    }
}
