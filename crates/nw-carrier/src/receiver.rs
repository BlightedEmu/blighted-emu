// SPDX-License-Identifier: AGPL-3.0-or-later
//! The receiving side of one connection (carrier captures note section 3).
//!
//! Reliable frames are delivered in reliable-sequence order per channel; a frame older than the
//! next expected one is a duplicate and is dropped. Chunks are joined from in-order frames only.
//! A repeated datagram is acked again and its frames are dropped. A new connection expects
//! reliable sequence 0 on every channel, as all captured connections start; a capture that starts
//! in the middle of a session learns the start from its first reliable frame. Only datagrams with
//! a reliable
//! frame are acked: only reliable frames are sent again, and acking ack-only datagrams would make
//! two idle peers ack each other without end.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::datagram::Datagram;
use crate::error::CarrierError;
use crate::frame::{CHANNEL_COUNT, Frame, SYSTEM_CHANNEL, decode_frames};
use crate::system::{Acks, MAX_ACK_BITMAP_LEN, SystemMessage};

/// Largest message before registration (M3 spec section 5).
pub const MAX_REGISTRATION_MESSAGE_LEN: usize = 16 * 1024;
/// Largest message after registration (M3 spec section 5; the captures hold messages over 128 KiB).
pub const MAX_SESSION_MESSAGE_LEN: usize = 1024 * 1024;
/// Most reliable frames held while an earlier one is missing (M3 spec section 5).
pub const MAX_HELD_FRAMES: usize = 1024;
/// Reliable sequence number of the first reliable frame on each channel of a new connection.
pub const FIRST_RELIABLE_SEQUENCE: u16 = 0;
/// Datagram sequence numbers remembered to find repeats.
pub const RECENT_DATAGRAMS: usize = 1024;

/// A reliable sequence number this far ahead of the expected one or more is in the past.
const HALF_SEQUENCE_SPACE: u16 = 0x8000;
/// The chunk count of the frame before the last chunk.
const LAST_CHUNK_COUNT: u16 = 2;
const BITS_PER_BYTE: usize = 8;
/// Datagrams one bitmap ack can name behind its last datagram.
const ACK_HISTORY_DATAGRAMS: usize = MAX_ACK_BITMAP_LEN * BITS_PER_BYTE;

/// Limits of the receiving side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiveLimits {
    /// Largest joined message.
    pub max_message_len: usize,
    /// Most reliable frames held per channel.
    pub max_held_frames: usize,
}

impl ReceiveLimits {
    /// Limits before registration.
    pub const REGISTRATION: Self = Self {
        max_message_len: MAX_REGISTRATION_MESSAGE_LEN,
        max_held_frames: MAX_HELD_FRAMES,
    };
    /// Limits after registration.
    pub const SESSION: Self = Self {
        max_message_len: MAX_SESSION_MESSAGE_LEN,
        max_held_frames: MAX_HELD_FRAMES,
    };
}

/// One delivered message payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Channel 0, 1, or 2.
    pub channel: u8,
    /// Whether the message came reliable.
    pub reliable: bool,
    /// Payload; `nw-reflect` reads the messages in it.
    pub payload: Vec<u8>,
}

/// What one datagram delivered, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inbound {
    /// A message on channel 0, 1, or 2.
    Message(Message),
    /// A system message.
    System(SystemMessage),
}

#[derive(Debug, Default)]
struct ChannelIn {
    next_reliable: Option<u16>,
    held: HashMap<u16, Frame>,
    chunks: Vec<u8>,
    last_count: Option<u16>,
}

/// The receiving side of one connection.
#[derive(Debug)]
pub struct Receiver {
    limits: ReceiveLimits,
    channels: [ChannelIn; CHANNEL_COUNT],
    recent: VecDeque<u16>,
    recent_set: HashSet<u16>,
    unacked: Vec<u16>,
}

impl Receiver {
    /// A receiver for a new connection: every channel expects [`FIRST_RELIABLE_SEQUENCE`].
    pub fn new(limits: ReceiveLimits) -> Self {
        Self::starting_at(limits, Some(FIRST_RELIABLE_SEQUENCE))
    }

    /// A receiver for a capture that starts in the middle of a session: each channel takes its
    /// first reliable frame as the start.
    pub fn mid_stream(limits: ReceiveLimits) -> Self {
        Self::starting_at(limits, None)
    }

    fn starting_at(limits: ReceiveLimits, start: Option<u16>) -> Self {
        Self {
            limits,
            channels: std::array::from_fn(|_| ChannelIn {
                next_reliable: start,
                ..ChannelIn::default()
            }),
            recent: VecDeque::with_capacity(RECENT_DATAGRAMS),
            recent_set: HashSet::with_capacity(RECENT_DATAGRAMS),
            unacked: Vec::new(),
        }
    }

    /// Changes the limits, for example after registration.
    pub fn set_limits(&mut self, limits: ReceiveLimits) {
        self.limits = limits;
    }

    /// Handles one datagram and returns what it delivers, in order.
    ///
    /// # Errors
    ///
    /// Returns [`CarrierError`] for a frame or system message that does not decode, a chunk that
    /// breaks the count rule, an unreliable chunk, a message over the limit, or too many held
    /// frames. The caller closes the connection.
    pub fn receive(&mut self, datagram: &Datagram) -> Result<Vec<Inbound>, CarrierError> {
        if !self.remember(datagram.sequence) {
            self.unacked.push(datagram.sequence);
            return Ok(Vec::new());
        }
        let frames = decode_frames(&datagram.body)?;
        if frames.iter().any(|frame| frame.reliable) {
            self.unacked.push(datagram.sequence);
        }
        let mut delivered = Vec::new();
        for frame in frames {
            if frame.channel == SYSTEM_CHANNEL {
                delivered.push(Inbound::System(SystemMessage::decode(&frame.payload)?));
            } else if frame.reliable {
                self.order(frame, &mut delivered)?;
            } else if frame.chunks_remaining.is_some() {
                return Err(CarrierError::UnreliableChunk);
            } else {
                delivered.push(Inbound::Message(Message {
                    channel: frame.channel,
                    reliable: false,
                    payload: frame.payload,
                }));
            }
        }
        Ok(delivered)
    }

    /// Whether a received datagram waits for an ack.
    pub fn wants_ack(&self) -> bool {
        !self.unacked.is_empty()
    }

    /// An ack of every datagram with a reliable frame received since the last call, or `None`
    /// when there is none.
    pub fn take_acks(&mut self) -> Option<Acks> {
        let reference = *self.unacked.first()?;
        let last = self
            .unacked
            .iter()
            .copied()
            .max_by_key(|sequence| sequence.wrapping_sub(reference))?;
        let behind: HashSet<u16> = self
            .unacked
            .drain(..)
            .map(|sequence| last.wrapping_sub(sequence))
            .filter(|distance| *distance > 0 && usize::from(*distance) <= ACK_HISTORY_DATAGRAMS)
            .collect();
        let deepest = behind.iter().copied().max();
        Some(match deepest {
            None => Acks::Last(last),
            Some(deepest) if behind.len() == usize::from(deepest) => Acks::Range {
                last,
                first: last.wrapping_sub(deepest),
            },
            Some(deepest) => {
                let mut history = vec![0_u8; usize::from(deepest - 1) / BITS_PER_BYTE + 1];
                for distance in behind {
                    let bit = usize::from(distance - 1);
                    history[bit / BITS_PER_BYTE] |= 1 << (bit % BITS_PER_BYTE);
                }
                Acks::Bitmap { last, history }
            }
        })
    }

    /// Records a datagram sequence number; false when it was seen before.
    fn remember(&mut self, sequence: u16) -> bool {
        if !self.recent_set.insert(sequence) {
            return false;
        }
        self.recent.push_back(sequence);
        if self.recent.len() > RECENT_DATAGRAMS
            && let Some(oldest) = self.recent.pop_front()
        {
            self.recent_set.remove(&oldest);
        }
        true
    }

    fn order(&mut self, frame: Frame, delivered: &mut Vec<Inbound>) -> Result<(), CarrierError> {
        let limits = self.limits;
        let state = &mut self.channels[usize::from(frame.channel)];
        let next = *state.next_reliable.get_or_insert(frame.reliable_sequence);
        if frame.reliable_sequence.wrapping_sub(next) >= HALF_SEQUENCE_SPACE {
            return Ok(());
        }
        state.held.insert(frame.reliable_sequence, frame);
        if state.held.len() > limits.max_held_frames {
            return Err(CarrierError::TooManyHeld {
                limit: limits.max_held_frames,
            });
        }
        let mut expected = next;
        while let Some(ready) = state.held.remove(&expected) {
            expected = expected.wrapping_add(1);
            if let Some(payload) = join(state, ready, limits.max_message_len)? {
                delivered.push(Inbound::Message(Message {
                    channel: payload.0,
                    reliable: true,
                    payload: payload.1,
                }));
            }
        }
        state.next_reliable = Some(expected);
        Ok(())
    }
}

/// Adds an in-order reliable frame to the channel's chunk run; returns a complete message.
fn join(
    state: &mut ChannelIn,
    frame: Frame,
    limit: usize,
) -> Result<Option<(u8, Vec<u8>)>, CarrierError> {
    match (frame.chunks_remaining, state.last_count) {
        (Some(count), previous) => {
            let valid = match previous {
                Some(previous) => count.checked_add(1) == Some(previous),
                None => count >= LAST_CHUNK_COUNT,
            };
            if !valid {
                return Err(CarrierError::BadChunkCount {
                    previous,
                    found: Some(count),
                });
            }
            state.chunks.extend_from_slice(&frame.payload);
            state.last_count = Some(count);
            check_len(state.chunks.len(), limit)?;
            Ok(None)
        }
        (None, Some(previous)) => {
            if previous != LAST_CHUNK_COUNT {
                return Err(CarrierError::BadChunkCount {
                    previous: Some(previous),
                    found: None,
                });
            }
            state.chunks.extend_from_slice(&frame.payload);
            state.last_count = None;
            check_len(state.chunks.len(), limit)?;
            Ok(Some((frame.channel, std::mem::take(&mut state.chunks))))
        }
        (None, None) => {
            check_len(frame.payload.len(), limit)?;
            Ok(Some((frame.channel, frame.payload)))
        }
    }
}

fn check_len(length: usize, limit: usize) -> Result<(), CarrierError> {
    if length > limit {
        Err(CarrierError::MessageTooLong { length, limit })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::encode_frames;
    use crate::system::OP_CONNECT_ACK;

    const CHANNEL: u8 = 0;
    const OTHER_CHANNEL: u8 = 1;
    const START: u16 = 0xFFFE;
    const DATAGRAM: u16 = 0x0100;
    const FIRST_COUNT: u16 = 3;
    const SMALL_LIMIT: usize = 4;
    const HELD_LIMIT: usize = 2;
    const CONNECT_ACK: [u8; 5] = [0x00, 0x00, 0x00, 0x05, OP_CONNECT_ACK];

    fn frame(reliable_sequence: u16, chunks_remaining: Option<u16>, payload: &[u8]) -> Frame {
        Frame {
            channel: CHANNEL,
            reliable: true,
            connecting: false,
            chunks_remaining,
            sequence: reliable_sequence,
            reliable_sequence,
            payload: payload.to_vec(),
        }
    }

    fn datagram(sequence: u16, frames: &[Frame]) -> Datagram {
        Datagram {
            sequence,
            body: encode_frames(frames).expect("encode"),
        }
    }

    fn payloads(inbound: &[Inbound]) -> Vec<Vec<u8>> {
        inbound
            .iter()
            .filter_map(|item| match item {
                Inbound::Message(message) => Some(message.payload.clone()),
                Inbound::System(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_new_receiver_expects_sequence_0_on_every_channel() {
        let mut receiver = Receiver::new(ReceiveLimits::SESSION);
        let second = frame(FIRST_RELIABLE_SEQUENCE + 1, None, b"b");
        let first = frame(FIRST_RELIABLE_SEQUENCE, None, b"a");
        let got = receiver
            .receive(&datagram(DATAGRAM, &[second]))
            .expect("second");
        assert!(payloads(&got).is_empty());
        let got = receiver
            .receive(&datagram(DATAGRAM + 1, &[first]))
            .expect("first");
        assert_eq!(payloads(&got), vec![b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn reliable_frames_are_delivered_in_order_across_a_wrap() {
        let mut receiver = Receiver::mid_stream(ReceiveLimits::SESSION);
        let first = frame(START, None, b"a");
        let second = frame(START.wrapping_add(1), None, b"b");
        let third = frame(START.wrapping_add(2), None, b"c");
        let got = receiver
            .receive(&datagram(DATAGRAM, &[first]))
            .expect("first");
        assert_eq!(payloads(&got), vec![b"a".to_vec()]);
        let got = receiver
            .receive(&datagram(DATAGRAM + 1, &[third, second]))
            .expect("reversed");
        assert_eq!(payloads(&got), vec![b"b".to_vec(), b"c".to_vec()]);
    }

    #[test]
    fn a_frame_after_a_gap_waits_for_the_resend() {
        let mut receiver = Receiver::mid_stream(ReceiveLimits::SESSION);
        receiver
            .receive(&datagram(DATAGRAM, &[frame(START, None, b"a")]))
            .expect("a");
        let got = receiver
            .receive(&datagram(
                DATAGRAM + 2,
                &[frame(START.wrapping_add(2), None, b"c")],
            ))
            .expect("c");
        assert!(payloads(&got).is_empty());
        let got = receiver
            .receive(&datagram(
                DATAGRAM + 3,
                &[frame(START.wrapping_add(1), None, b"b")],
            ))
            .expect("b");
        assert_eq!(payloads(&got), vec![b"b".to_vec(), b"c".to_vec()]);
    }

    #[test]
    fn repeats_are_dropped() {
        let mut receiver = Receiver::mid_stream(ReceiveLimits::SESSION);
        let once = datagram(DATAGRAM, &[frame(START, None, b"a")]);
        assert_eq!(
            payloads(&receiver.receive(&once).expect("first")),
            vec![b"a".to_vec()]
        );
        assert!(receiver.receive(&once).expect("same datagram").is_empty());
        let resent = datagram(DATAGRAM + 1, &[frame(START, None, b"a")]);
        assert!(payloads(&receiver.receive(&resent).expect("resent frame")).is_empty());
    }

    #[test]
    fn chunks_join_into_one_message() {
        let mut receiver = Receiver::mid_stream(ReceiveLimits::SESSION);
        let frames = [
            frame(START, Some(FIRST_COUNT), b"ab"),
            frame(START.wrapping_add(1), Some(FIRST_COUNT - 1), b"cd"),
            frame(START.wrapping_add(2), None, b"e"),
        ];
        let got = receiver
            .receive(&datagram(DATAGRAM, &frames))
            .expect("chunks");
        assert_eq!(payloads(&got), vec![b"abcde".to_vec()]);
    }

    #[test]
    fn chunk_rules_are_enforced() {
        let mut skipped = Receiver::mid_stream(ReceiveLimits::SESSION);
        let frames = [
            frame(START, Some(FIRST_COUNT), b"a"),
            frame(START.wrapping_add(1), Some(FIRST_COUNT - 2), b"b"),
        ];
        assert!(matches!(
            skipped.receive(&datagram(DATAGRAM, &frames)),
            Err(CarrierError::BadChunkCount { .. })
        ));
        let mut early_end = Receiver::mid_stream(ReceiveLimits::SESSION);
        let frames = [
            frame(START, Some(FIRST_COUNT), b"a"),
            frame(START.wrapping_add(1), None, b"b"),
        ];
        assert!(matches!(
            early_end.receive(&datagram(DATAGRAM, &frames)),
            Err(CarrierError::BadChunkCount { .. })
        ));
        let mut unreliable = Receiver::mid_stream(ReceiveLimits::SESSION);
        let mut chunk = frame(START, Some(FIRST_COUNT), b"a");
        chunk.reliable = false;
        assert_eq!(
            unreliable.receive(&datagram(DATAGRAM, &[chunk])),
            Err(CarrierError::UnreliableChunk)
        );
    }

    #[test]
    fn limits_are_enforced() {
        let limits = ReceiveLimits {
            max_message_len: SMALL_LIMIT,
            max_held_frames: HELD_LIMIT,
        };
        let mut long = Receiver::mid_stream(limits);
        assert!(matches!(
            long.receive(&datagram(DATAGRAM, &[frame(START, None, b"too long")])),
            Err(CarrierError::MessageTooLong { .. })
        ));
        let mut held = Receiver::mid_stream(limits);
        held.receive(&datagram(DATAGRAM, &[frame(START, None, b"a")]))
            .expect("a");
        let ahead: Vec<Frame> = (2..=4)
            .map(|offset| frame(START.wrapping_add(offset), None, b"x"))
            .collect();
        assert!(matches!(
            held.receive(&datagram(DATAGRAM + 1, &ahead)),
            Err(CarrierError::TooManyHeld { .. })
        ));
    }

    #[test]
    fn unreliable_and_system_frames_pass_straight_through() {
        let mut receiver = Receiver::mid_stream(ReceiveLimits::SESSION);
        let mut loose = frame(START, None, b"u");
        loose.reliable = false;
        loose.channel = OTHER_CHANNEL;
        let mut system = frame(0, None, &CONNECT_ACK);
        system.reliable = true;
        system.channel = SYSTEM_CHANNEL;
        let got = receiver
            .receive(&datagram(DATAGRAM, &[loose, system]))
            .expect("decode");
        assert_eq!(
            got,
            vec![
                Inbound::Message(Message {
                    channel: OTHER_CHANNEL,
                    reliable: false,
                    payload: b"u".to_vec()
                }),
                Inbound::System(SystemMessage::decode(&CONNECT_ACK).expect("ack")),
            ]
        );
    }

    /// A datagram with one reliable frame; each call uses the next reliable sequence number.
    fn reliable_datagram(receiver_next: &mut u16, sequence: u16) -> Datagram {
        let frame = frame(*receiver_next, None, b"r");
        *receiver_next = receiver_next.wrapping_add(1);
        datagram(sequence, &[frame])
    }

    #[test]
    fn ack_only_datagrams_are_not_acked() {
        let mut receiver = Receiver::mid_stream(ReceiveLimits::SESSION);
        let mut ack = frame(0, None, &[0x20, 0x06]);
        ack.reliable = false;
        ack.channel = SYSTEM_CHANNEL;
        receiver
            .receive(&datagram(DATAGRAM, &[ack]))
            .expect("keepalive");
        assert!(!receiver.wants_ack());
        assert_eq!(receiver.take_acks(), None);
    }

    #[test]
    fn acks_name_every_received_reliable_datagram() {
        let mut receiver = Receiver::mid_stream(ReceiveLimits::SESSION);
        let mut next = START;
        assert_eq!(receiver.take_acks(), None);
        for sequence in [DATAGRAM, DATAGRAM + 1, DATAGRAM + 2] {
            let datagram = reliable_datagram(&mut next, sequence);
            receiver.receive(&datagram).expect("reliable");
        }
        assert!(receiver.wants_ack());
        assert_eq!(
            receiver.take_acks(),
            Some(Acks::Range {
                last: DATAGRAM + 2,
                first: DATAGRAM
            })
        );
        let single = reliable_datagram(&mut next, DATAGRAM + 5);
        receiver.receive(&single).expect("single");
        assert_eq!(receiver.take_acks(), Some(Acks::Last(DATAGRAM + 5)));
        for sequence in [DATAGRAM + 6, DATAGRAM + 9] {
            let datagram = reliable_datagram(&mut next, sequence);
            receiver.receive(&datagram).expect("gap");
        }
        let acks = receiver.take_acks().expect("bitmap");
        assert_eq!(
            acks,
            Acks::Bitmap {
                last: DATAGRAM + 9,
                history: vec![0b0000_0100]
            }
        );
        assert_eq!(acks.acked(), vec![DATAGRAM + 9, DATAGRAM + 6]);
        receiver
            .receive(&datagram(DATAGRAM + 9, &[]))
            .expect("repeat");
        assert_eq!(receiver.take_acks(), Some(Acks::Last(DATAGRAM + 9)));
    }
}
