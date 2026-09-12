// SPDX-License-Identifier: AGPL-3.0-or-later
//! Frames inside a datagram body (carrier captures note section 2).
//!
//! `[flags][u16 BE length][channel?][u16 chunks remaining?][u16 sequence?][u16 reliable
//! sequence?][payload]`. A missing channel is the channel of the previous frame in the datagram,
//! or 0 for the first frame. A missing sequence or reliable sequence is the value of the previous
//! frame in the datagram plus 1; that frame is on the same channel in every captured datagram.

use crate::error::CarrierError;

/// Flag: reliable, ordered delivery.
pub const FLAG_RELIABLE: u8 = 0x01;
/// Flag: a chunk of a longer message; more chunks follow.
pub const FLAG_CHUNKS: u8 = 0x04;
/// Flag: the sequence number is the previous frame's + 1 and is not written.
pub const FLAG_IMPLICIT_SEQUENCE: u8 = 0x08;
/// Flag: the reliable sequence number is the previous frame's + 1 and is not written.
pub const FLAG_IMPLICIT_RELIABLE_SEQUENCE: u8 = 0x10;
/// Flag: the channel byte is written.
pub const FLAG_CHANNEL: u8 = 0x20;
/// Flag: a connect request.
pub const FLAG_CONNECTING: u8 = 0x80;
/// Bits that no frame may set (GridMate `MF_UNUSED_FLAGS`).
pub const UNUSED_FLAGS: u8 = 0x42;

/// The channel of registration, ping and other game messages.
pub const MESSAGE_CHANNEL: u8 = 0;
/// The system channel.
pub const SYSTEM_CHANNEL: u8 = 3;
/// Channels: 0, 1, 2 for messages and 3 for the system.
pub const CHANNEL_COUNT: usize = 4;
/// Bytes of the largest frame header: flags, length, channel, chunks, sequence, reliable sequence.
pub const MAX_FRAME_HEADER_LEN: usize = 10;

const FLAGS_LEN: usize = 1;
const U16_LEN: usize = 2;
const CHANNEL_LEN: usize = 1;
const FIRST_FRAME_CHANNEL: u8 = 0;

/// One frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Channel, 0 to 3.
    pub channel: u8,
    /// Reliable, ordered delivery.
    pub reliable: bool,
    /// Part of the connect handshake.
    pub connecting: bool,
    /// Chunks left, this one included, when this frame is a chunk that is not the last. The
    /// first count is 2 or more; the frame after count 2 is the last chunk and has no count.
    pub chunks_remaining: Option<u16>,
    /// Message sequence number.
    pub sequence: u16,
    /// Reliable sequence number. An unreliable frame carries the number of the last reliable
    /// frame sent on its channel (65535 before any), as every captured unreliable frame does.
    /// The real client ignored unreliable frames that named a reliable frame not yet sent.
    pub reliable_sequence: u16,
    /// Payload.
    pub payload: Vec<u8>,
}

impl Frame {
    /// Bytes this frame needs after the previous frame `previous` in the same datagram.
    pub fn encoded_len(&self, previous: Option<&Self>) -> usize {
        header_len(header_flags(self, previous)) + self.payload.len()
    }
}

/// Writes frames into one datagram body, with implicit fields where the rules allow them.
///
/// # Errors
///
/// Returns [`CarrierError::FrameTooLong`] when a payload does not fit the 16-bit length field,
/// and [`CarrierError::BadChannel`] for a channel above 3.
pub fn encode_frames(frames: &[Frame]) -> Result<Vec<u8>, CarrierError> {
    let mut out = Vec::new();
    let mut previous: Option<&Frame> = None;
    for frame in frames {
        if usize::from(frame.channel) >= CHANNEL_COUNT {
            return Err(CarrierError::BadChannel {
                channel: frame.channel,
            });
        }
        let length =
            u16::try_from(frame.payload.len()).map_err(|_| CarrierError::FrameTooLong {
                length: frame.payload.len(),
            })?;
        let flags = header_flags(frame, previous);
        out.push(flags);
        out.extend_from_slice(&length.to_be_bytes());
        if flags & FLAG_CHANNEL != 0 {
            out.push(frame.channel);
        }
        if let Some(chunks) = frame.chunks_remaining {
            out.extend_from_slice(&chunks.to_be_bytes());
        }
        if flags & FLAG_IMPLICIT_SEQUENCE == 0 {
            out.extend_from_slice(&frame.sequence.to_be_bytes());
        }
        if flags & FLAG_IMPLICIT_RELIABLE_SEQUENCE == 0 {
            out.extend_from_slice(&frame.reliable_sequence.to_be_bytes());
        }
        out.extend_from_slice(&frame.payload);
        previous = Some(frame);
    }
    Ok(out)
}

/// Reads every frame of one datagram body.
///
/// # Errors
///
/// Returns [`CarrierError`] when a frame is cut short, sets an unused flag, names a channel
/// above 3, or uses an implicit field with no previous frame on its channel.
pub fn decode_frames(body: &[u8]) -> Result<Vec<Frame>, CarrierError> {
    let mut frames: Vec<Frame> = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        let flags = take(&mut rest, FLAGS_LEN)?[0];
        if flags & UNUSED_FLAGS != 0 {
            return Err(CarrierError::BadFlags { flags });
        }
        let length = usize::from(read_u16(&mut rest)?);
        let previous = frames.last();
        let channel = if flags & FLAG_CHANNEL != 0 {
            take(&mut rest, CHANNEL_LEN)?[0]
        } else {
            previous.map_or(FIRST_FRAME_CHANNEL, |frame| frame.channel)
        };
        if usize::from(channel) >= CHANNEL_COUNT {
            return Err(CarrierError::BadChannel { channel });
        }
        let chunks_remaining = if flags & FLAG_CHUNKS != 0 {
            Some(read_u16(&mut rest)?)
        } else {
            None
        };
        let same_channel = previous.filter(|frame| frame.channel == channel);
        let sequence = if flags & FLAG_IMPLICIT_SEQUENCE != 0 {
            same_channel
                .ok_or(CarrierError::ImplicitWithoutPrevious { flags })?
                .sequence
                .wrapping_add(1)
        } else {
            read_u16(&mut rest)?
        };
        let reliable_sequence = if flags & FLAG_IMPLICIT_RELIABLE_SEQUENCE != 0 {
            same_channel
                .ok_or(CarrierError::ImplicitWithoutPrevious { flags })?
                .reliable_sequence
                .wrapping_add(1)
        } else {
            read_u16(&mut rest)?
        };
        let payload = take(&mut rest, length)?.to_vec();
        frames.push(Frame {
            channel,
            reliable: flags & FLAG_RELIABLE != 0,
            connecting: flags & FLAG_CONNECTING != 0,
            chunks_remaining,
            sequence,
            reliable_sequence,
            payload,
        });
    }
    Ok(frames)
}

/// Flags of `frame` after `previous`.
fn header_flags(frame: &Frame, previous: Option<&Frame>) -> u8 {
    let mut flags = 0;
    if frame.reliable {
        flags |= FLAG_RELIABLE;
    }
    if frame.connecting {
        flags |= FLAG_CONNECTING;
    }
    if frame.chunks_remaining.is_some() {
        flags |= FLAG_CHUNKS;
    }
    let inherited_channel = previous.map_or(FIRST_FRAME_CHANNEL, |prior| prior.channel);
    if frame.channel != inherited_channel {
        flags |= FLAG_CHANNEL;
    }
    if let Some(prior) = previous.filter(|prior| prior.channel == frame.channel) {
        if prior.sequence.wrapping_add(1) == frame.sequence {
            flags |= FLAG_IMPLICIT_SEQUENCE;
        }
        if prior.reliable_sequence.wrapping_add(1) == frame.reliable_sequence {
            flags |= FLAG_IMPLICIT_RELIABLE_SEQUENCE;
        }
    }
    flags
}

fn header_len(flags: u8) -> usize {
    let mut len = FLAGS_LEN + U16_LEN;
    if flags & FLAG_CHANNEL != 0 {
        len += CHANNEL_LEN;
    }
    if flags & FLAG_CHUNKS != 0 {
        len += U16_LEN;
    }
    if flags & FLAG_IMPLICIT_SEQUENCE == 0 {
        len += U16_LEN;
    }
    if flags & FLAG_IMPLICIT_RELIABLE_SEQUENCE == 0 {
        len += U16_LEN;
    }
    len
}

fn take<'a>(rest: &mut &'a [u8], len: usize) -> Result<&'a [u8], CarrierError> {
    if rest.len() < len {
        return Err(CarrierError::Truncated {
            needed: len,
            available: rest.len(),
        });
    }
    let (head, tail) = rest.split_at(len);
    *rest = tail;
    Ok(head)
}

fn read_u16(rest: &mut &[u8]) -> Result<u16, CarrierError> {
    let bytes = take(rest, U16_LEN)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// A keepalive in the captured layout: flags `0x20` (channel 3, unreliable), length 2,
    /// channel 3, sequence `0x0001`, reliable sequence `0x0000`, payload `20 06`.
    const KEEPALIVE_FRAME: [u8; 10] = [0x20, 0x00, 0x02, 0x03, 0x00, 0x01, 0x00, 0x00, 0x20, 0x06];
    const KEEPALIVE_PAYLOAD: [u8; 2] = [0x20, 0x06];
    const ACK_SEQUENCE: u16 = 1;
    const DATA_CHANNEL: u8 = 0;
    const BAD_CHANNEL: u8 = 4;
    const FIRST_SEQUENCE: u16 = 0x0102;
    const FIRST_RELIABLE: u16 = 0x0304;
    const CHUNKS_LEFT: u16 = 2;

    fn data_frame(sequence: u16, reliable_sequence: u16) -> Frame {
        Frame {
            channel: DATA_CHANNEL,
            reliable: true,
            connecting: false,
            chunks_remaining: None,
            sequence,
            reliable_sequence,
            payload: vec![sequence as u8; 3],
        }
    }

    #[test]
    fn decodes_a_keepalive_frame() {
        let frames = decode_frames(&KEEPALIVE_FRAME).expect("decode");
        assert_eq!(
            frames,
            vec![Frame {
                channel: SYSTEM_CHANNEL,
                reliable: false,
                connecting: false,
                chunks_remaining: None,
                sequence: ACK_SEQUENCE,
                reliable_sequence: 0,
                payload: KEEPALIVE_PAYLOAD.to_vec(),
            }]
        );
        assert_eq!(encode_frames(&frames).expect("encode"), KEEPALIVE_FRAME);
    }

    #[test]
    fn following_frames_on_one_channel_use_implicit_fields() {
        let frames = vec![
            data_frame(FIRST_SEQUENCE, FIRST_RELIABLE),
            data_frame(FIRST_SEQUENCE + 1, FIRST_RELIABLE + 1),
        ];
        let body = encode_frames(&frames).expect("encode");
        let second_flags = body[frames[0].encoded_len(None)];
        assert_eq!(
            second_flags,
            FLAG_RELIABLE | FLAG_IMPLICIT_SEQUENCE | FLAG_IMPLICIT_RELIABLE_SEQUENCE
        );
        assert_eq!(decode_frames(&body).expect("decode"), frames);
    }

    #[test]
    fn the_first_frame_without_a_channel_byte_is_channel_0() {
        let frame = data_frame(FIRST_SEQUENCE, FIRST_RELIABLE);
        let body = encode_frames(std::slice::from_ref(&frame)).expect("encode");
        assert_eq!(body[0] & FLAG_CHANNEL, 0);
        assert_eq!(decode_frames(&body).expect("decode"), vec![frame]);
    }

    #[test]
    fn chunk_frames_carry_the_chunks_remaining() {
        let mut frame = data_frame(FIRST_SEQUENCE, FIRST_RELIABLE);
        frame.chunks_remaining = Some(CHUNKS_LEFT);
        let body = encode_frames(std::slice::from_ref(&frame)).expect("encode");
        assert_eq!(body[0] & FLAG_CHUNKS, FLAG_CHUNKS);
        assert_eq!(decode_frames(&body).expect("decode"), vec![frame]);
    }

    #[test]
    fn broken_frames_are_errors() {
        let mut unused = KEEPALIVE_FRAME;
        unused[0] |= UNUSED_FLAGS;
        assert!(matches!(
            decode_frames(&unused),
            Err(CarrierError::BadFlags { .. })
        ));
        let mut channel = KEEPALIVE_FRAME;
        channel[3] = BAD_CHANNEL;
        assert!(matches!(
            decode_frames(&channel),
            Err(CarrierError::BadChannel { .. })
        ));
        let implicit_first = [FLAG_IMPLICIT_SEQUENCE, 0x00, 0x00, 0x00, 0x00];
        assert!(matches!(
            decode_frames(&implicit_first),
            Err(CarrierError::ImplicitWithoutPrevious { .. })
        ));
        assert!(matches!(
            decode_frames(&KEEPALIVE_FRAME[..KEEPALIVE_FRAME.len() - 1]),
            Err(CarrierError::Truncated { .. })
        ));
        let mut bad = data_frame(FIRST_SEQUENCE, FIRST_RELIABLE);
        bad.channel = BAD_CHANNEL;
        assert!(matches!(
            encode_frames(&[bad]),
            Err(CarrierError::BadChannel { .. })
        ));
    }

    fn any_frame() -> impl Strategy<Value = Frame> {
        (
            0..CHANNEL_COUNT as u8,
            any::<bool>(),
            any::<bool>(),
            proptest::option::of(any::<u16>()),
            any::<u16>(),
            any::<u16>(),
            proptest::collection::vec(any::<u8>(), 0..64),
        )
            .prop_map(
                |(
                    channel,
                    reliable,
                    connecting,
                    chunks_remaining,
                    sequence,
                    reliable_sequence,
                    payload,
                )| Frame {
                    channel,
                    reliable,
                    connecting,
                    chunks_remaining,
                    sequence,
                    reliable_sequence,
                    payload,
                },
            )
    }

    proptest! {
        #[test]
        fn frames_round_trip(frames in proptest::collection::vec(any_frame(), 0..8)) {
            let body = encode_frames(&frames).expect("encode");
            let expected_len: usize = frames
                .iter()
                .enumerate()
                .map(|(index, frame)| frame.encoded_len(index.checked_sub(1).map(|prior| &frames[prior])))
                .sum();
            prop_assert_eq!(body.len(), expected_len);
            prop_assert_eq!(decode_frames(&body).expect("decode"), frames);
        }

        #[test]
        fn random_bodies_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..128)) {
            let _ = decode_frames(&bytes);
        }
    }
}
