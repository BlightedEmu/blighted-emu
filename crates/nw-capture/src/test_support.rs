// SPDX-License-Identifier: AGPL-3.0-or-later
//! Made-up ledgers for tests (feature `test-util`). No byte here comes from a capture.

use nw_carrier::system::CARRIER_VERSION;
use nw_carrier::{Frame, SYSTEM_CHANNEL, SystemMessage, encode_datagram, encode_frames};
use nw_messages::{
    BundleGroup, BundleMember, BundlePayload, MemberState, PositionInTheWorldReplicatedState,
    ReplicatedStateBundle, WorldPosition,
};
use nw_reflect::{Envelope, TypeId, encode_client_message, encode_server_messages};

use crate::LedgerWriter;

/// Context of every test record.
pub const TEST_CONTEXT: u64 = 7;
/// Time of the first test record (Unix milliseconds).
pub const TEST_TIME_MS: u64 = 1_782_067_831_638;
/// Datagram sequence of a test connect request.
pub const DATAGRAM_SEQUENCE: u16 = 2;
/// First reliable sequence of a test stream that starts mid-stream.
pub const RELIABLE_START: u16 = 10;
/// The session bytes of a client message.
pub const ZERO_SESSION: [u8; 16] = [0; 16];
/// Entity key of [`position_bundle`].
pub const TEST_ENTITY: u64 = 2;

/// One ledger record at [`TEST_TIME_MS`] in [`TEST_CONTEXT`].
pub fn record(direction: u32, datagram: &[u8]) -> Vec<u8> {
    record_at(TEST_TIME_MS, direction, datagram)
}

/// One ledger record at `time_ms` in [`TEST_CONTEXT`].
pub fn record_at(time_ms: u64, direction: u32, datagram: &[u8]) -> Vec<u8> {
    let mut writer = LedgerWriter::new(Vec::new());
    writer
        .record(time_ms, direction, TEST_CONTEXT, datagram)
        .expect("write to memory");
    writer.into_inner()
}

/// A datagram with one reliable frame on channel 0 at [`RELIABLE_START`].
pub fn reliable_datagram(payload: Vec<u8>) -> Vec<u8> {
    reliable_datagram_at(RELIABLE_START, payload)
}

/// A datagram with one reliable frame on channel 0; the datagram sequence is the reliable one,
/// so the datagrams of one stream differ.
pub fn reliable_datagram_at(reliable_sequence: u16, payload: Vec<u8>) -> Vec<u8> {
    datagram(
        reliable_sequence,
        &[reliable_frame(reliable_sequence, payload)],
    )
}

/// One envelope of `index` with `body`.
pub fn envelope(index: u64, body: Vec<u8>) -> Vec<u8> {
    Envelope::message(TypeId::FileSlot(index), body)
        .encode()
        .expect("envelope")
}

/// A bundle body around `payload`.
pub fn bundle_of(payload: Vec<u8>) -> Vec<u8> {
    use nw_messages::BodyCodec;
    ReplicatedStateBundle {
        leading: None,
        byte_3: 0,
        byte_4: 0,
        flag_5: false,
        index_block: None,
        payload,
    }
    .encode()
    .expect("bundle")
}

/// A bundle whose payload is one group with one position member of [`TEST_ENTITY`] at `(x, y, z)`.
pub fn position_bundle_at(x: f32, y: f32, z: f32) -> Vec<u8> {
    let payload = BundlePayload {
        groups: vec![BundleGroup {
            leading: 1,
            members: vec![BundleMember {
                u64_at_0: TEST_ENTITY,
                state: MemberState::Position(PositionInTheWorldReplicatedState {
                    position: WorldPosition::from_metres(x, y, z),
                    ..PositionInTheWorldReplicatedState::default()
                }),
            }],
        }],
    };
    bundle_of(payload.encode().expect("payload"))
}

/// [`position_bundle_at`] at (1, 2, 3).
pub fn position_bundle() -> Vec<u8> {
    position_bundle_at(1.0, 2.0, 3.0)
}

/// One server datagram that carries `envelopes`, reliable, at [`RELIABLE_START`].
pub fn server_datagram(envelopes: &[Vec<u8>]) -> Vec<u8> {
    server_datagram_at(RELIABLE_START, envelopes)
}

/// One server datagram that carries `envelopes` in one reliable frame.
pub fn server_datagram_at(reliable_sequence: u16, envelopes: &[Vec<u8>]) -> Vec<u8> {
    let slices: Vec<&[u8]> = envelopes.iter().map(Vec::as_slice).collect();
    reliable_datagram_at(
        reliable_sequence,
        encode_server_messages(&slices).expect("server"),
    )
}

/// One client datagram with one reliable frame per envelope, from `first_sequence` on; the
/// datagram sequence is `first_sequence`.
pub fn client_datagram(first_sequence: u16, envelopes: &[Vec<u8>]) -> Vec<u8> {
    let frames: Vec<Frame> = envelopes
        .iter()
        .zip(first_sequence..)
        .map(|(envelope, sequence)| {
            reliable_frame(
                sequence,
                encode_client_message(&ZERO_SESSION, envelope).expect("client"),
            )
        })
        .collect();
    datagram(first_sequence, &frames)
}

/// A client connect request, as the first datagram of a new connection sends it.
pub fn connect_request_datagram() -> Vec<u8> {
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
    datagram(DATAGRAM_SEQUENCE, &[frame])
}

fn reliable_frame(reliable_sequence: u16, payload: Vec<u8>) -> Frame {
    Frame {
        channel: 0,
        reliable: true,
        connecting: false,
        chunks_remaining: None,
        sequence: reliable_sequence,
        reliable_sequence,
        payload,
    }
}

fn datagram(sequence: u16, frames: &[Frame]) -> Vec<u8> {
    encode_datagram(sequence, &encode_frames(frames).expect("frames"))
}
