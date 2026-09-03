// SPDX-License-Identifier: AGPL-3.0-or-later
//! Carrier errors. Every decode failure is a value; no input panics (M3 spec section 5).

use thiserror::Error;

/// Failure from the carrier codec or a connection.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CarrierError {
    /// The bytes ended before a field was complete.
    #[error("truncated carrier data: need {needed} bytes, have {available}")]
    Truncated {
        /// Bytes this read needs.
        needed: usize,
        /// Bytes left.
        available: usize,
    },
    /// The datagram header is not `0x80`/`0x81` followed by `0x01`.
    #[error("unknown datagram header {mode:#04x} {version:#04x}")]
    BadHeader {
        /// First header byte.
        mode: u8,
        /// Second header byte.
        version: u8,
    },
    /// The LZ4 block is invalid or unpacks to more than the limit.
    #[error("invalid or oversized LZ4 block")]
    BadCompression,
    /// A frame sets an unused flag bit.
    #[error("frame flags {flags:#04x} set an unused bit")]
    BadFlags {
        /// Flags byte.
        flags: u8,
    },
    /// A frame names a channel above 3.
    #[error("channel {channel} does not exist")]
    BadChannel {
        /// Channel byte.
        channel: u8,
    },
    /// A frame uses an implicit field and no previous frame on its channel is in the datagram.
    #[error("frame flags {flags:#04x} use an implicit field with no previous frame")]
    ImplicitWithoutPrevious {
        /// Flags byte.
        flags: u8,
    },
    /// A frame payload does not fit the 16-bit length field.
    #[error("frame payload of {length} bytes is too long")]
    FrameTooLong {
        /// Payload length.
        length: usize,
    },
    /// A system message does not fit its opcode.
    #[error("system message with opcode {opcode} has a bad layout")]
    BadSystemMessage {
        /// Opcode (last payload byte).
        opcode: u8,
    },
    /// An unreliable frame has the chunk flag.
    #[error("unreliable frame has the chunk flag")]
    UnreliableChunk,
    /// A chunk count does not follow the previous one.
    #[error("chunk count {found:?} does not follow {previous:?}")]
    BadChunkCount {
        /// Count of the previous chunk on the channel.
        previous: Option<u16>,
        /// Count of this frame; `None` for a frame without the chunk flag.
        found: Option<u16>,
    },
    /// A joined message is longer than the limit.
    #[error("message of {length} bytes is over the limit {limit}")]
    MessageTooLong {
        /// Bytes so far.
        length: usize,
        /// Limit.
        limit: usize,
    },
    /// Too many reliable frames wait for a missing one.
    #[error("more than {limit} reliable frames wait for a missing frame")]
    TooManyHeld {
        /// Limit.
        limit: usize,
    },
}
