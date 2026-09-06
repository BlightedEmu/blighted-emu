// SPDX-License-Identifier: AGPL-3.0-or-later
//! The index of one session: counts per message type and second, every message with where it
//! sits, bundle members, entity tracks, and the player's own track. Built once; bytes are read
//! again from the ledger on request ([`message_bytes`]).

use std::collections::{BTreeMap, HashMap};

use crate::decode::{Decoded, Decoder, Detail, describe};
use crate::{CaptureError, ledger_records};

/// A key back after this long is a new appearance (mob replication rate note, "Distance").
pub const TRACK_GAP_MS: u64 = 5_000;
const MS_PER_SECOND: u64 = 1_000;
/// Seconds of rate buckets per type; later messages count in the last bucket (a broken record
/// time must not size a bucket list).
pub const MAX_RATE_SECONDS: usize = 86_400;

/// Totals of a session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    /// Time of the first record (Unix milliseconds).
    pub first_ms: u64,
    /// Time of the last record.
    pub last_ms: u64,
    /// Records read.
    pub records: u64,
    /// Messages delivered.
    pub messages: u64,
    /// Datagram bytes.
    pub bytes: u64,
}

/// Counts of one message type in one direction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeStats {
    /// Type index.
    pub type_index: u64,
    /// Direction.
    pub direction: u32,
    /// Messages.
    pub count: u64,
    /// Body bytes.
    pub bytes: u64,
    /// First time seen.
    pub first_ms: u64,
    /// Last time seen.
    pub last_ms: u64,
    /// Messages per second from the session's first record.
    pub per_second: Vec<u32>,
}

/// One point of a track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPoint {
    /// Unix milliseconds.
    pub time_ms: u64,
    /// East, in metres.
    pub x: f32,
    /// North, in metres.
    pub y: f32,
    /// Height, in metres.
    pub z: f32,
}

/// One appearance of one replication key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    /// The replication key.
    pub key: u64,
    /// Appearance number of this key, from 0.
    pub appearance: u32,
    /// Classes of its members, ascending.
    pub classes: Vec<u64>,
    /// Its positions.
    pub points: Vec<TrackPoint>,
    /// First time a member of it was seen.
    pub first_ms: u64,
    /// Last time a member of it was seen.
    pub last_ms: u64,
}

/// A stream that stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamFailure {
    /// Record number.
    pub record: u32,
    /// What failed.
    pub reason: String,
}

/// The index of one session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionIndex {
    /// Totals.
    pub summary: Summary,
    /// Counts per type and direction, by type index.
    pub types: Vec<TypeStats>,
    /// Every message, in order.
    pub messages: Vec<Decoded>,
    /// Tracks, by key and appearance.
    pub tracks: Vec<Track>,
    /// The player's own positions (client input).
    pub player: Vec<TrackPoint>,
    /// Classes where bundle walks stopped (no reader, or a failing reader), with counts, by class.
    pub coverage: Vec<(u64, u64)>,
    /// Streams that stopped.
    pub failures: Vec<StreamFailure>,
    /// The ledger error that ended the build.
    pub error: Option<String>,
}

#[derive(Default)]
struct OpenTrack {
    appearances: u32,
    current: Track,
}

impl SessionIndex {
    /// Indexes a ledger. A broken record ends the build with [`SessionIndex::error`] set.
    pub fn build(ledger: &[u8]) -> Self {
        let mut index = Self::default();
        let mut decoder = Decoder::new();
        let mut types: BTreeMap<(u64, u32), TypeStats> = BTreeMap::new();
        let mut tracks: HashMap<u64, OpenTrack> = HashMap::new();
        let mut done: Vec<Track> = Vec::new();
        let mut coverage: BTreeMap<u64, u64> = BTreeMap::new();
        for (number, record) in ledger_records(ledger).enumerate() {
            let record = match record {
                Ok(record) => record,
                Err(error) => {
                    index.error = Some(error.to_string());
                    break;
                }
            };
            let number = u32::try_from(number).unwrap_or(u32::MAX);
            let summary = &mut index.summary;
            if summary.records == 0 {
                summary.first_ms = record.time_ms;
            }
            summary.records += 1;
            summary.last_ms = summary.last_ms.max(record.time_ms);
            summary.bytes += record.datagram.len() as u64;
            let output = decoder.record(&record);
            if let Some(reason) = output.error {
                index.failures.push(StreamFailure {
                    record: number,
                    reason,
                });
            }
            for (slot, delivered) in output.envelopes.iter().enumerate() {
                let slot = u16::try_from(slot).unwrap_or(u16::MAX);
                let decoded = describe(number, slot, &record, delivered);
                index.summary.messages += 1;
                if let Some(type_index) = decoded.type_index {
                    let stats = types
                        .entry((type_index, decoded.direction))
                        .or_insert_with(|| TypeStats {
                            type_index,
                            direction: decoded.direction,
                            first_ms: decoded.time_ms,
                            ..TypeStats::default()
                        });
                    stats.count += 1;
                    stats.bytes += u64::from(decoded.size);
                    stats.last_ms = decoded.time_ms;
                    let second = usize::try_from(
                        decoded.time_ms.saturating_sub(index.summary.first_ms) / MS_PER_SECOND,
                    )
                    .map_or(MAX_RATE_SECONDS, |second| second.min(MAX_RATE_SECONDS));
                    if stats.per_second.len() <= second {
                        stats.per_second.resize(second + 1, 0);
                    }
                    stats.per_second[second] += 1;
                }
                match &decoded.detail {
                    Detail::Bundle {
                        members,
                        stopped_at,
                        failed,
                    } => {
                        for member in members {
                            add_to_track(&mut tracks, &mut done, member, decoded.time_ms);
                        }
                        let end = stopped_at.or(failed.as_ref().map(|failure| failure.class));
                        if let Some(class) = end {
                            *coverage.entry(class).or_default() += 1;
                        }
                    }
                    Detail::Input {
                        position: Some([x, y, z]),
                    } => index.player.push(TrackPoint {
                        time_ms: decoded.time_ms,
                        x: *x,
                        y: *y,
                        z: *z,
                    }),
                    Detail::Input { position: None } | Detail::None | Detail::Failed(_) => {}
                }
                index.messages.push(decoded);
            }
        }
        done.extend(tracks.into_values().map(|open| open.current));
        done.sort_by_key(|track| (track.key, track.appearance));
        index.tracks = done;
        index.types = types.into_values().collect();
        index.coverage = coverage.into_iter().collect();
        index
    }
}

fn add_to_track(
    tracks: &mut HashMap<u64, OpenTrack>,
    done: &mut Vec<Track>,
    member: &crate::decode::MemberRef,
    time_ms: u64,
) {
    let open = tracks.entry(member.key).or_insert_with(|| OpenTrack {
        appearances: 1,
        current: Track {
            key: member.key,
            first_ms: time_ms,
            last_ms: time_ms,
            ..Track::default()
        },
    });
    if time_ms.saturating_sub(open.current.last_ms) >= TRACK_GAP_MS {
        let next = Track {
            key: member.key,
            appearance: open.appearances,
            first_ms: time_ms,
            last_ms: time_ms,
            ..Track::default()
        };
        open.appearances += 1;
        done.push(std::mem::replace(&mut open.current, next));
    }
    let track = &mut open.current;
    track.last_ms = time_ms;
    if let Err(at) = track.classes.binary_search(&member.class) {
        track.classes.insert(at, member.class);
    }
    if let Some([x, y, z]) = member.position {
        track.points.push(TrackPoint { time_ms, x, y, z });
    }
}

/// The body of envelope `slot` of record `record`, decoded again from the start of the ledger.
///
/// # Errors
///
/// Returns [`CaptureError`] when the ledger is broken before that record, or
/// [`CaptureError::NoMessage`] when that record did not deliver that envelope.
pub fn message_bytes(ledger: &[u8], record: u32, slot: u16) -> Result<Vec<u8>, CaptureError> {
    let mut decoder = Decoder::new();
    let missing = CaptureError::NoMessage { record, slot };
    for (number, item) in ledger_records(ledger).enumerate() {
        let item = item?;
        let output = decoder.record(&item);
        if u32::try_from(number).ok() == Some(record) {
            return output
                .envelopes
                .into_iter()
                .nth(usize::from(slot))
                .map(|delivered| delivered.envelope.body)
                .ok_or(missing);
        }
    }
    Err(missing)
}

#[cfg(test)]
mod tests {
    use nw_messages::{
        BodyCodec, ClientInputMsg, InputPosition, POSITION_IN_THE_WORLD_TYPE_INDEX,
        RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX, REPLICATED_STATE_BUNDLE_TYPE_INDEX,
    };
    use nw_reflect::PING_MSG_TYPE_INDEX;

    use super::*;
    use crate::test_support::{
        RELIABLE_START, TEST_ENTITY, TEST_TIME_MS, client_datagram, envelope, position_bundle_at,
        record_at, server_datagram_at,
    };
    use crate::{CLIENT_TO_SERVER, SERVER_TO_CLIENT};

    const PING_BODY: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];
    const HALF_SECOND_MS: u64 = 500;
    const CUT_BYTES: usize = 5;

    /// Server records at `offsets_ms` after the test time, each with one envelope from `make`.
    fn server_ledger(offsets_ms: &[u64], make: impl Fn(usize) -> Vec<u8>) -> Vec<u8> {
        let mut ledger = Vec::new();
        for (index, offset) in offsets_ms.iter().enumerate() {
            let sequence = RELIABLE_START + u16::try_from(index).expect("small");
            ledger.extend(record_at(
                TEST_TIME_MS + offset,
                SERVER_TO_CLIENT,
                &server_datagram_at(sequence, &[make(index)]),
            ));
        }
        ledger
    }

    fn ping(_: usize) -> Vec<u8> {
        envelope(PING_MSG_TYPE_INDEX, PING_BODY.to_vec())
    }

    #[test]
    fn types_are_counted_per_direction_and_second() {
        let index = SessionIndex::build(&server_ledger(&[0, HALF_SECOND_MS, 1_500], ping));
        let stats = index
            .types
            .iter()
            .find(|stats| stats.type_index == PING_MSG_TYPE_INDEX)
            .expect("ping");
        assert_eq!(
            (stats.direction, stats.count, stats.bytes),
            (SERVER_TO_CLIENT, 3, 3 * PING_BODY.len() as u64)
        );
        assert_eq!(stats.per_second, vec![2, 1]);
        assert_eq!(index.summary.messages, 3);
        assert_eq!(index.summary.last_ms - index.summary.first_ms, 1_500);
    }

    #[test]
    fn client_input_gives_the_player_track() {
        let position = InputPosition::from_metres(5.0, 6.0, 7.0).expect("finite");
        let input = ClientInputMsg::position(position).encode().expect("input");
        let ledger = record_at(
            TEST_TIME_MS,
            CLIENT_TO_SERVER,
            &client_datagram(10, &[envelope(RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX, input)]),
        );
        let index = SessionIndex::build(&ledger);
        assert_eq!(index.player.len(), 1);
        assert_eq!((index.player[0].x, index.player[0].y), (5.0, 6.0));
        assert_eq!(index.player[0].time_ms, TEST_TIME_MS);
    }

    #[test]
    fn a_key_back_after_five_seconds_is_a_new_appearance() {
        let offsets = [0, 4_900, 4_900 + TRACK_GAP_MS];
        let index = SessionIndex::build(&server_ledger(&offsets, |n| {
            let x = f32::from(u8::try_from(n).expect("small"));
            envelope(
                REPLICATED_STATE_BUNDLE_TYPE_INDEX,
                position_bundle_at(x, 2.0, 3.0),
            )
        }));
        let tracks: Vec<(u64, u32, usize)> = index
            .tracks
            .iter()
            .map(|track| (track.key, track.appearance, track.points.len()))
            .collect();
        assert_eq!(tracks, vec![(TEST_ENTITY, 0, 2), (TEST_ENTITY, 1, 1)]);
        assert_eq!(
            index.tracks[0].classes,
            vec![POSITION_IN_THE_WORLD_TYPE_INDEX]
        );
    }

    #[test]
    fn message_bytes_returns_the_body_of_that_record() {
        let ledger = server_ledger(&[0, HALF_SECOND_MS], |n| {
            envelope(
                PING_MSG_TYPE_INDEX,
                vec![u8::try_from(n).expect("small"); 3],
            )
        });
        let index = SessionIndex::build(&ledger);
        let second = &index.messages[1];
        assert_eq!(
            message_bytes(&ledger, second.record, second.slot).expect("bytes"),
            vec![1; 3]
        );
        assert!(matches!(
            message_bytes(&ledger, second.record, second.slot + 1),
            Err(CaptureError::NoMessage { .. })
        ));
    }

    #[test]
    fn a_cut_ledger_builds_an_index_with_its_error() {
        let mut ledger = server_ledger(&[0, HALF_SECOND_MS], ping);
        ledger.truncate(ledger.len() - CUT_BYTES);
        let index = SessionIndex::build(&ledger);
        assert_eq!(index.summary.messages, 1);
        assert!(index.error.is_some());
    }

    #[test]
    fn stop_classes_are_counted_in_the_coverage() {
        let mut payload = nw_messages::Writer::new();
        payload
            .prefixed(1)
            .expect("group")
            .u8(1)
            .prefixed(TEST_ENTITY)
            .expect("key");
        payload
            .bytes(&nw_reflect::encode_type_id(&nw_reflect::TypeId::FileSlot(12)).expect("class"));
        let bundle = crate::test_support::bundle_of(payload.finish());
        let index = SessionIndex::build(&server_ledger(&[0, 1], |_| {
            envelope(REPLICATED_STATE_BUNDLE_TYPE_INDEX, bundle.clone())
        }));
        assert_eq!(index.coverage, vec![(12, 2)]);
    }

    #[test]
    fn a_far_future_record_does_not_blow_up_the_rates() {
        let mut ledger = server_ledger(&[0], ping);
        ledger.extend(record_at(
            u64::MAX,
            SERVER_TO_CLIENT,
            &server_datagram_at(RELIABLE_START + 1, &[ping(1)]),
        ));
        let index = SessionIndex::build(&ledger);
        let stats = &index.types[0];
        assert_eq!(stats.count, 2);
        assert!(stats.per_second.len() <= MAX_RATE_SECONDS + 1);
        assert_eq!(stats.per_second.iter().sum::<u32>(), 2);
    }

    #[test]
    fn a_failing_member_class_is_counted_in_the_coverage() {
        let bundle = crate::test_support::bundle_with_a_failing_member();
        let index = SessionIndex::build(&server_ledger(&[0], |_| {
            envelope(REPLICATED_STATE_BUNDLE_TYPE_INDEX, bundle.clone())
        }));
        assert_eq!(index.coverage, vec![(nw_messages::ALC_TYPE_INDEX, 1)]);
        assert_eq!(
            index.tracks.len(),
            1,
            "the member before it still makes a track"
        );
    }
}
