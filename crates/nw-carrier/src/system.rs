// SPDX-License-Identifier: AGPL-3.0-or-later
//! Messages on the system channel 3 (carrier captures note section 4).
//!
//! The opcode is the last payload byte. Acks name datagram sequence numbers. In the bitmap form,
//! bit `d % 8` of byte `d / 8` set means datagram `last - 1 - d` is acked.

use crate::error::CarrierError;

/// Connect request (client).
pub const OP_CONNECT_REQUEST: u8 = 1;
/// Connect ack (server).
pub const OP_CONNECT_ACK: u8 = 2;
/// Disconnect (both).
pub const OP_DISCONNECT: u8 = 3;
/// Datagram acks; also the keepalive (both).
pub const OP_ACKS: u8 = 6;
/// Connection control (both); meaning of the 4 data bytes Unknown.
pub const OP_CONNECTION_CONTROL: u8 = 7;
/// Version value in every captured connect request and connect ack.
pub const CARRIER_VERSION: u32 = 5;
/// Reason byte of every captured disconnect.
pub const DISCONNECT_REASON_NONE: u8 = 0;

/// Ack flag: a bitmap of `flags & 0x7F` bytes follows the last sequence number.
pub const ACK_BITMAP: u8 = 0x80;
/// Ack flag: a first sequence number follows; every datagram from first to last is acked.
pub const ACK_RANGE: u8 = 0x40;
/// Ack flag: nothing follows; the message only keeps the connection alive.
pub const ACK_KEEP_ALIVE: u8 = 0x20;
/// Bits of the bitmap byte count in a bitmap ack.
pub const ACK_BITMAP_LEN_MASK: u8 = 0x7F;
/// Largest bitmap in bytes (512 datagrams of history).
pub const MAX_ACK_BITMAP_LEN: usize = 64;
/// Widest range ack: the same 512 datagrams of history a bitmap can name. A wider range is
/// refused, because no sender keeps that many datagrams unacked.
pub const MAX_ACK_RANGE: u16 = 512;

const OPCODE_LEN: usize = 1;
const VERSION_LEN: usize = 4;
const SEQUENCE_LEN: usize = 2;
const BITS_PER_BYTE: usize = 8;

/// Datagrams that one ack message confirms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acks {
    /// No datagram; the connection is alive.
    KeepAlive,
    /// One datagram.
    Last(u16),
    /// Every datagram from `first` to `last`.
    Range {
        /// Newest acked datagram.
        last: u16,
        /// Oldest acked datagram.
        first: u16,
    },
    /// `last` and the datagrams before it whose bits are set.
    Bitmap {
        /// Newest acked datagram.
        last: u16,
        /// Bit history, newest first.
        history: Vec<u8>,
    },
}

impl Acks {
    /// Whether this ack confirms datagram `sequence`. The check never lists the datagrams, so
    /// its cost does not depend on the width of a range.
    pub fn contains(&self, sequence: u16) -> bool {
        match self {
            Self::KeepAlive => false,
            Self::Last(last) => *last == sequence,
            Self::Range { last, first } => {
                sequence.wrapping_sub(*first) <= last.wrapping_sub(*first)
            }
            Self::Bitmap { last, history } => {
                let distance = usize::from(last.wrapping_sub(sequence));
                distance == 0
                    || history
                        .get((distance - 1) / BITS_PER_BYTE)
                        .is_some_and(|byte| byte >> ((distance - 1) % BITS_PER_BYTE) & 1 == 1)
            }
        }
    }
}

/// One system message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemMessage {
    /// Connect request. `extra` is the bytes between the version and the opcode: one `0x01` per
    /// retry in the captures.
    ConnectRequest {
        /// Carrier version.
        version: u32,
        /// Bytes after the version.
        extra: Vec<u8>,
    },
    /// Connect ack.
    ConnectAck {
        /// Carrier version.
        version: u32,
        /// Bytes after the version.
        extra: Vec<u8>,
    },
    /// Disconnect.
    Disconnect {
        /// Reason byte.
        reason: u8,
    },
    /// Datagram acks.
    Acks(Acks),
    /// Connection control; the data bytes are kept as they are.
    ConnectionControl(Vec<u8>),
    /// Any other opcode.
    Other {
        /// Opcode.
        opcode: u8,
        /// Bytes before the opcode.
        data: Vec<u8>,
    },
}

impl SystemMessage {
    /// Writes the payload of a system frame.
    pub fn encode(&self) -> Vec<u8> {
        let (mut out, opcode) = match self {
            Self::ConnectRequest { version, extra } => {
                (versioned(*version, extra), OP_CONNECT_REQUEST)
            }
            Self::ConnectAck { version, extra } => (versioned(*version, extra), OP_CONNECT_ACK),
            Self::Disconnect { reason } => (vec![*reason], OP_DISCONNECT),
            Self::Acks(acks) => (encode_acks(acks), OP_ACKS),
            Self::ConnectionControl(data) => (data.clone(), OP_CONNECTION_CONTROL),
            Self::Other { opcode, data } => (data.clone(), *opcode),
        };
        out.push(opcode);
        out
    }

    /// Reads the payload of a system frame.
    ///
    /// # Errors
    ///
    /// Returns [`CarrierError::BadSystemMessage`] when the data does not fit the opcode, and
    /// [`CarrierError::Truncated`] for an empty payload.
    pub fn decode(payload: &[u8]) -> Result<Self, CarrierError> {
        let Some((&opcode, data)) = payload.split_last() else {
            return Err(CarrierError::Truncated {
                needed: OPCODE_LEN,
                available: 0,
            });
        };
        let bad = CarrierError::BadSystemMessage { opcode };
        match opcode {
            OP_CONNECT_REQUEST | OP_CONNECT_ACK => {
                let (version, extra) = data.split_first_chunk::<VERSION_LEN>().ok_or(bad)?;
                let version = u32::from_be_bytes(*version);
                let extra = extra.to_vec();
                Ok(if opcode == OP_CONNECT_REQUEST {
                    Self::ConnectRequest { version, extra }
                } else {
                    Self::ConnectAck { version, extra }
                })
            }
            OP_DISCONNECT => match data {
                [reason] => Ok(Self::Disconnect { reason: *reason }),
                _ => Err(bad),
            },
            OP_ACKS => decode_acks(data).map(Self::Acks).ok_or(bad),
            OP_CONNECTION_CONTROL => Ok(Self::ConnectionControl(data.to_vec())),
            _ => Ok(Self::Other {
                opcode,
                data: data.to_vec(),
            }),
        }
    }
}

fn versioned(version: u32, extra: &[u8]) -> Vec<u8> {
    let mut out = version.to_be_bytes().to_vec();
    out.extend_from_slice(extra);
    out
}

fn encode_acks(acks: &Acks) -> Vec<u8> {
    match acks {
        Acks::KeepAlive => vec![ACK_KEEP_ALIVE],
        Acks::Last(last) => {
            let mut out = vec![0];
            out.extend_from_slice(&last.to_be_bytes());
            out
        }
        Acks::Range { last, first } => {
            let mut out = vec![ACK_RANGE];
            out.extend_from_slice(&last.to_be_bytes());
            out.extend_from_slice(&first.to_be_bytes());
            out
        }
        Acks::Bitmap { last, history } => {
            let len = history.len().min(MAX_ACK_BITMAP_LEN);
            let count = u8::try_from(len).unwrap_or(ACK_BITMAP_LEN_MASK) & ACK_BITMAP_LEN_MASK;
            let mut out = vec![ACK_BITMAP | count];
            out.extend_from_slice(&last.to_be_bytes());
            out.extend_from_slice(&history[..len]);
            out
        }
    }
}

fn decode_acks(data: &[u8]) -> Option<Acks> {
    let (&flags, rest) = data.split_first()?;
    if flags & ACK_BITMAP != 0 {
        let len = usize::from(flags & ACK_BITMAP_LEN_MASK);
        if len > MAX_ACK_BITMAP_LEN || rest.len() != SEQUENCE_LEN + len {
            return None;
        }
        let (last, history) = rest.split_at(SEQUENCE_LEN);
        return Some(Acks::Bitmap {
            last: u16::from_be_bytes([last[0], last[1]]),
            history: history.to_vec(),
        });
    }
    match (flags, rest) {
        (ACK_KEEP_ALIVE, []) => Some(Acks::KeepAlive),
        (ACK_RANGE, [l0, l1, f0, f1]) => {
            let last = u16::from_be_bytes([*l0, *l1]);
            let first = u16::from_be_bytes([*f0, *f1]);
            (last.wrapping_sub(first) <= MAX_ACK_RANGE).then_some(Acks::Range { last, first })
        }
        (0, [l0, l1]) => Some(Acks::Last(u16::from_be_bytes([*l0, *l1]))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// Captured connect request, first try.
    const CONNECT_REQUEST: [u8; 5] = [0x00, 0x00, 0x00, 0x05, 0x01];
    /// Captured connect request, second try.
    const CONNECT_RETRY: [u8; 6] = [0x00, 0x00, 0x00, 0x05, 0x01, 0x01];
    /// Captured connect ack.
    const CONNECT_ACK: [u8; 5] = [0x00, 0x00, 0x00, 0x05, 0x02];
    /// Captured disconnect.
    const DISCONNECT: [u8; 2] = [0x00, 0x03];
    /// Captured keepalive.
    const KEEP_ALIVE: [u8; 2] = [0x20, 0x06];
    /// Single ack of datagram 0x1234, in the captured layout.
    const SINGLE_ACK: [u8; 4] = [0x00, 0x12, 0x34, 0x06];
    /// Range ack of datagrams 0x0010 to 0x0012, in the captured layout.
    const RANGE_ACK: [u8; 6] = [0x40, 0x00, 0x12, 0x00, 0x10, 0x06];
    /// Bitmap ack: last 0x0010; bits 0 and 2 set, so 0x000F and 0x000D are acked too.
    const BITMAP_ACK: [u8; 5] = [0x81, 0x00, 0x10, 0b0000_0101, 0x06];
    /// Connection control in the captured layout: 4 data bytes, then opcode 7.
    const CONNECTION_CONTROL: [u8; 5] = [0x00, 0x00, 0x12, 0x34, 0x07];
    const RETRY_MARK: u8 = 0x01;
    const LAST: u16 = 0x0012;
    const FIRST: u16 = 0x0010;
    const BITMAP_LAST: u16 = 0x0010;
    const WRAP_LAST: u16 = 1;
    const WRAP_FIRST: u16 = u16::MAX;
    const UNKNOWN_OPCODE: u8 = 4;
    const SINGLE: u16 = 0x1234;

    #[test]
    fn decodes_captured_connect_messages() {
        assert_eq!(
            SystemMessage::decode(&CONNECT_REQUEST),
            Ok(SystemMessage::ConnectRequest {
                version: CARRIER_VERSION,
                extra: Vec::new()
            })
        );
        assert_eq!(
            SystemMessage::decode(&CONNECT_RETRY),
            Ok(SystemMessage::ConnectRequest {
                version: CARRIER_VERSION,
                extra: vec![RETRY_MARK]
            })
        );
        let ack = SystemMessage::ConnectAck {
            version: CARRIER_VERSION,
            extra: Vec::new(),
        };
        assert_eq!(SystemMessage::decode(&CONNECT_ACK), Ok(ack.clone()));
        assert_eq!(ack.encode(), CONNECT_ACK);
        assert_eq!(
            SystemMessage::decode(&DISCONNECT),
            Ok(SystemMessage::Disconnect {
                reason: DISCONNECT_REASON_NONE
            })
        );
        assert_eq!(
            SystemMessage::decode(&CONNECTION_CONTROL),
            Ok(SystemMessage::ConnectionControl(
                CONNECTION_CONTROL[..CONNECTION_CONTROL.len() - 1].to_vec()
            ))
        );
    }

    #[test]
    fn decodes_every_ack_form() {
        for (bytes, acks, acked) in [
            (&KEEP_ALIVE[..], Acks::KeepAlive, vec![]),
            (&SINGLE_ACK[..], Acks::Last(SINGLE), vec![SINGLE]),
            (
                &RANGE_ACK[..],
                Acks::Range {
                    last: LAST,
                    first: FIRST,
                },
                vec![FIRST, FIRST + 1, LAST],
            ),
            (
                &BITMAP_ACK[..],
                Acks::Bitmap {
                    last: BITMAP_LAST,
                    history: vec![0b0000_0101],
                },
                vec![BITMAP_LAST, BITMAP_LAST - 1, BITMAP_LAST - 3],
            ),
        ] {
            let message = SystemMessage::decode(bytes).expect("decode");
            assert_eq!(message, SystemMessage::Acks(acks.clone()));
            assert_eq!(message.encode(), bytes);
            for sequence in acked {
                assert!(acks.contains(sequence));
            }
        }
    }

    #[test]
    fn a_range_wider_than_the_ack_history_is_refused() {
        let widest = [ACK_RANGE, 0x00, 0x10, 0x00, 0x11, OP_ACKS];
        assert_eq!(
            SystemMessage::decode(&widest),
            Err(CarrierError::BadSystemMessage { opcode: OP_ACKS })
        );
    }

    #[test]
    fn contains_names_the_acked_datagrams_without_listing_them() {
        let range = Acks::Range {
            last: WRAP_LAST,
            first: WRAP_FIRST,
        };
        assert!(range.contains(WRAP_FIRST) && range.contains(0) && range.contains(WRAP_LAST));
        assert!(!range.contains(WRAP_LAST + 1) && !range.contains(WRAP_FIRST - 1));
        let bitmap = Acks::Bitmap {
            last: BITMAP_LAST,
            history: vec![0b0000_0101],
        };
        assert!(bitmap.contains(BITMAP_LAST) && bitmap.contains(BITMAP_LAST - 1));
        assert!(bitmap.contains(BITMAP_LAST - 3) && !bitmap.contains(BITMAP_LAST - 2));
        assert!(!bitmap.contains(BITMAP_LAST + 1));
        assert!(Acks::Last(SINGLE).contains(SINGLE) && !Acks::Last(SINGLE).contains(SINGLE + 1));
        assert!(!Acks::KeepAlive.contains(SINGLE));
    }

    #[test]
    fn a_range_can_wrap() {
        let acks = Acks::Range {
            last: WRAP_LAST,
            first: WRAP_FIRST,
        };
        for sequence in [WRAP_FIRST, 0, WRAP_LAST] {
            assert!(acks.contains(sequence));
        }
    }

    #[test]
    fn broken_system_messages_are_errors() {
        assert!(matches!(
            SystemMessage::decode(&[]),
            Err(CarrierError::Truncated { .. })
        ));
        assert!(matches!(
            SystemMessage::decode(&[0x00, 0x05, OP_CONNECT_REQUEST]),
            Err(CarrierError::BadSystemMessage { .. })
        ));
        assert!(matches!(
            SystemMessage::decode(&[0x00, 0x00, OP_DISCONNECT]),
            Err(CarrierError::BadSystemMessage { .. })
        ));
        assert!(matches!(
            SystemMessage::decode(&[ACK_RANGE, 0x00, OP_ACKS]),
            Err(CarrierError::BadSystemMessage { .. })
        ));
        assert_eq!(
            SystemMessage::decode(&[UNKNOWN_OPCODE]),
            Ok(SystemMessage::Other {
                opcode: UNKNOWN_OPCODE,
                data: Vec::new()
            })
        );
    }

    proptest! {
        #[test]
        fn random_payloads_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
            let _ = SystemMessage::decode(&bytes);
        }

        #[test]
        fn bitmap_acks_round_trip(last in any::<u16>(), history in proptest::collection::vec(any::<u8>(), 0..=MAX_ACK_BITMAP_LEN)) {
            let message = SystemMessage::Acks(Acks::Bitmap { last, history });
            prop_assert_eq!(SystemMessage::decode(&message.encode()).expect("decode"), message);
        }
    }
}
