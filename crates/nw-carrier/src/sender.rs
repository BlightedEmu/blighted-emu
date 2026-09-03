// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sending side of one connection (M3 spec section 5).
//!
//! Messages become frames; a message longer than one datagram is split into reliable chunks
//! (count = chunks left, this one included). Frames are packed into datagrams of at most
//! [`MAX_DATAGRAM_LEN`] bytes before LZ4. Reliable frames stay in flight until the peer acks
//! their datagram; after [`RESEND_AFTER_MS`] they go out again in a new datagram, at most
//! [`MAX_SEND_ATTEMPTS`] times.

use std::collections::{BTreeMap, VecDeque};

use crate::datagram::{DATAGRAM_HEADER_LEN, MAX_DATAGRAM_LEN, encode_datagram};
use crate::error::CarrierError;
use crate::frame::{CHANNEL_COUNT, Frame, MAX_FRAME_HEADER_LEN, encode_frames};
use crate::system::Acks;

/// Largest chunk payload: one chunk with the largest frame header fills one datagram.
pub const MAX_CHUNK_PAYLOAD: usize = MAX_DATAGRAM_LEN - DATAGRAM_HEADER_LEN - MAX_FRAME_HEADER_LEN;
/// Time before an unacked reliable frame is sent again.
pub const RESEND_AFTER_MS: u64 = 200;
/// Sends of one reliable frame before the connection fails.
pub const MAX_SEND_ATTEMPTS: u32 = 10;
/// Sequence number of the first datagram (2 in every captured connection).
pub const FIRST_DATAGRAM_SEQUENCE: u16 = 2;
/// Largest chunk count (M3 spec section 5).
pub const MAX_CHUNKS: usize = 1024;

/// How one message travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivery {
    /// Channel 0 to 3.
    pub channel: u8,
    /// Reliable, ordered delivery.
    pub reliable: bool,
    /// Part of the connect handshake.
    pub connecting: bool,
}

#[derive(Debug, Default, Clone, Copy)]
struct ChannelOut {
    next_sequence: u16,
    next_reliable: u16,
}

#[derive(Debug)]
struct InFlight {
    frames: Vec<Frame>,
    sent_at_ms: u64,
    attempt: u32,
}

/// The sending side of one connection.
#[derive(Debug)]
pub struct Sender {
    next_datagram: u16,
    channels: [ChannelOut; CHANNEL_COUNT],
    queue: VecDeque<(Frame, u32)>,
    in_flight: BTreeMap<u16, InFlight>,
}

impl Default for Sender {
    fn default() -> Self {
        Self {
            next_datagram: FIRST_DATAGRAM_SEQUENCE,
            channels: [ChannelOut::default(); CHANNEL_COUNT],
            queue: VecDeque::new(),
            in_flight: BTreeMap::new(),
        }
    }
}

impl Sender {
    /// A sender with nothing queued; the first datagram is [`FIRST_DATAGRAM_SEQUENCE`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues one message.
    ///
    /// # Errors
    ///
    /// Returns [`CarrierError::BadChannel`] for a channel above 3, and
    /// [`CarrierError::MessageTooLong`] for a message over [`MAX_CHUNKS`] chunks.
    pub fn send(&mut self, delivery: Delivery, payload: &[u8]) -> Result<(), CarrierError> {
        let channel = usize::from(delivery.channel);
        if channel >= CHANNEL_COUNT {
            return Err(CarrierError::BadChannel {
                channel: delivery.channel,
            });
        }
        let chunks: Vec<&[u8]> = if payload.len() > MAX_CHUNK_PAYLOAD {
            payload.chunks(MAX_CHUNK_PAYLOAD).collect()
        } else {
            vec![payload]
        };
        let too_long = CarrierError::MessageTooLong {
            length: payload.len(),
            limit: MAX_CHUNKS * MAX_CHUNK_PAYLOAD,
        };
        if chunks.len() > MAX_CHUNKS {
            return Err(too_long);
        }
        let total = u16::try_from(chunks.len()).map_err(|_| too_long)?;
        let reliable = delivery.reliable || total > 1;
        for (chunk, left) in chunks.into_iter().zip((1..=total).rev()) {
            let state = &mut self.channels[channel];
            let reliable_sequence = state.next_reliable;
            if reliable {
                state.next_reliable = state.next_reliable.wrapping_add(1);
            }
            let sequence = state.next_sequence;
            state.next_sequence = state.next_sequence.wrapping_add(1);
            let frame = Frame {
                channel: delivery.channel,
                reliable,
                connecting: delivery.connecting,
                chunks_remaining: (left > 1).then_some(left),
                sequence,
                reliable_sequence,
                payload: chunk.to_vec(),
            };
            self.queue.push_back((frame, 0));
        }
        Ok(())
    }

    /// Removes the acked datagrams from the in-flight list.
    pub fn handle_acks(&mut self, acks: &Acks) {
        for sequence in acks.acked() {
            self.in_flight.remove(&sequence);
        }
    }

    /// Whether any frame is queued or waits for an ack.
    pub fn is_idle(&self) -> bool {
        self.queue.is_empty() && self.in_flight.is_empty()
    }

    /// Datagrams to send now: resends of expired reliable frames first, then queued frames.
    ///
    /// # Errors
    ///
    /// Returns [`CarrierError::TooManyAttempts`] when a reliable frame was sent
    /// [`MAX_SEND_ATTEMPTS`] times without an ack. The caller closes the connection.
    pub fn poll(&mut self, now_ms: u64) -> Result<Vec<Vec<u8>>, CarrierError> {
        let expired: Vec<u16> = self
            .in_flight
            .iter()
            .filter(|(_, flight)| now_ms.saturating_sub(flight.sent_at_ms) >= RESEND_AFTER_MS)
            .map(|(sequence, _)| *sequence)
            .collect();
        let mut resend = Vec::new();
        for sequence in expired {
            let Some(flight) = self.in_flight.remove(&sequence) else {
                continue;
            };
            if flight.attempt >= MAX_SEND_ATTEMPTS {
                return Err(CarrierError::TooManyAttempts {
                    attempts: flight.attempt,
                });
            }
            resend.extend(
                flight
                    .frames
                    .into_iter()
                    .map(|frame| (frame, flight.attempt)),
            );
        }
        for item in resend.into_iter().rev() {
            self.queue.push_front(item);
        }
        let mut datagrams = Vec::new();
        while !self.queue.is_empty() {
            datagrams.push(self.pack(now_ms)?);
        }
        Ok(datagrams)
    }

    /// Packs queued frames into one datagram.
    fn pack(&mut self, now_ms: u64) -> Result<Vec<u8>, CarrierError> {
        let mut frames: Vec<Frame> = Vec::new();
        let mut attempt = 0;
        let mut used = DATAGRAM_HEADER_LEN;
        while let Some((frame, sent)) = self.queue.pop_front() {
            let size = frame.encoded_len(frames.last());
            if !frames.is_empty() && used + size > MAX_DATAGRAM_LEN {
                self.queue.push_front((frame, sent));
                break;
            }
            used += size;
            attempt = attempt.max(sent);
            frames.push(frame);
        }
        let sequence = self.next_datagram;
        self.next_datagram = self.next_datagram.wrapping_add(1);
        let body = encode_frames(&frames)?;
        let reliable: Vec<Frame> = frames.into_iter().filter(|frame| frame.reliable).collect();
        if !reliable.is_empty() {
            self.in_flight.insert(
                sequence,
                InFlight {
                    frames: reliable,
                    sent_at_ms: now_ms,
                    attempt: attempt + 1,
                },
            );
        }
        Ok(encode_datagram(sequence, &body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datagram::decode_datagram;
    use crate::frame::decode_frames;
    use crate::receiver::{Inbound, ReceiveLimits, Receiver};

    const NOW: u64 = 1_000;
    const DATA: Delivery = Delivery {
        channel: 0,
        reliable: true,
        connecting: false,
    };
    const LOOSE: Delivery = Delivery {
        channel: 1,
        reliable: false,
        connecting: false,
    };
    const BAD_CHANNEL: u8 = 4;
    const SMALL: &[u8] = b"hello";
    /// Three chunks: two full ones and one byte.
    const CHUNKED_LEN: usize = 2 * MAX_CHUNK_PAYLOAD + 1;
    const CHUNKED_COUNT: u16 = 3;

    fn delivered(receiver: &mut Receiver, datagrams: &[Vec<u8>]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for bytes in datagrams {
            let datagram = decode_datagram(bytes).expect("datagram");
            for item in receiver.receive(&datagram).expect("receive") {
                if let Inbound::Message(message) = item {
                    out.push(message.payload);
                }
            }
        }
        out
    }

    #[test]
    fn small_messages_share_one_datagram() {
        let mut sender = Sender::new();
        sender.send(DATA, SMALL).expect("first");
        sender.send(DATA, SMALL).expect("second");
        sender.send(LOOSE, SMALL).expect("third");
        let datagrams = sender.poll(NOW).expect("poll");
        assert_eq!(datagrams.len(), 1);
        assert_eq!(
            decode_datagram(&datagrams[0]).expect("decode").sequence,
            FIRST_DATAGRAM_SEQUENCE
        );
        let frames =
            decode_frames(&decode_datagram(&datagrams[0]).expect("decode").body).expect("frames");
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[1].reliable_sequence, frames[0].reliable_sequence + 1);
        assert!(!frames[2].reliable);
        let mut receiver = Receiver::new(ReceiveLimits::SESSION);
        assert_eq!(delivered(&mut receiver, &datagrams).len(), 3);
    }

    #[test]
    fn long_messages_go_as_reliable_chunks() {
        let mut sender = Sender::new();
        let payload: Vec<u8> = (0..CHUNKED_LEN).map(|index| index as u8).collect();
        sender.send(LOOSE, &payload).expect("send");
        let datagrams = sender.poll(NOW).expect("poll");
        assert!(datagrams.len() >= 2);
        for bytes in &datagrams {
            assert!(
                decode_datagram(bytes).expect("decode").body.len() + DATAGRAM_HEADER_LEN
                    <= MAX_DATAGRAM_LEN
            );
        }
        let first =
            decode_frames(&decode_datagram(&datagrams[0]).expect("decode").body).expect("frames");
        assert_eq!(first[0].chunks_remaining, Some(CHUNKED_COUNT));
        assert!(first[0].reliable);
        let mut receiver = Receiver::new(ReceiveLimits::SESSION);
        assert_eq!(delivered(&mut receiver, &datagrams), vec![payload]);
    }

    #[test]
    fn unacked_reliable_frames_are_sent_again() {
        let mut sender = Sender::new();
        sender.send(DATA, SMALL).expect("send");
        let lost = sender.poll(NOW).expect("first");
        assert_eq!(lost.len(), 1);
        assert!(
            sender
                .poll(NOW + RESEND_AFTER_MS - 1)
                .expect("early")
                .is_empty()
        );
        let again = sender.poll(NOW + RESEND_AFTER_MS).expect("resend");
        assert_eq!(again.len(), 1);
        let mut receiver = Receiver::new(ReceiveLimits::SESSION);
        assert_eq!(delivered(&mut receiver, &again), vec![SMALL.to_vec()]);
        let acks = receiver.take_acks().expect("ack");
        sender.handle_acks(&acks);
        assert!(sender.is_idle());
        assert!(
            sender
                .poll(NOW + 2 * RESEND_AFTER_MS)
                .expect("nothing")
                .is_empty()
        );
    }

    #[test]
    fn unreliable_frames_are_not_sent_again() {
        let mut sender = Sender::new();
        sender.send(LOOSE, SMALL).expect("send");
        assert_eq!(sender.poll(NOW).expect("first").len(), 1);
        assert!(sender.is_idle());
    }

    #[test]
    fn a_frame_that_is_never_acked_fails_the_connection() {
        let mut sender = Sender::new();
        sender.send(DATA, SMALL).expect("send");
        let mut now = NOW;
        for _ in 0..MAX_SEND_ATTEMPTS {
            assert_eq!(sender.poll(now).expect("send").len(), 1);
            now += RESEND_AFTER_MS;
        }
        assert_eq!(
            sender.poll(now),
            Err(CarrierError::TooManyAttempts {
                attempts: MAX_SEND_ATTEMPTS
            })
        );
    }

    #[test]
    fn bad_channels_and_huge_messages_are_refused() {
        let mut sender = Sender::new();
        let bad = Delivery {
            channel: BAD_CHANNEL,
            ..DATA
        };
        assert!(matches!(
            sender.send(bad, SMALL),
            Err(CarrierError::BadChannel { .. })
        ));
        let huge = vec![0_u8; MAX_CHUNKS * MAX_CHUNK_PAYLOAD + 1];
        assert!(matches!(
            sender.send(DATA, &huge),
            Err(CarrierError::MessageTooLong { .. })
        ));
    }
}
