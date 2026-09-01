// SPDX-License-Identifier: AGPL-3.0-or-later
//! Errors for carrier operations that this crate can decide.

/// Failure from a carrier operation this crate implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CarrierError {
    /// A UDP send would have to write a datagram header. That header is unknown.
    ///
    /// Protocol note `docs/re/2026-08-29-client-protocol-login-gridmate.md`, section 2
    /// and Unknowns item 5: the 4-byte mode/format/sequence header was not found.
    /// `FUN_140f7fc60` copies the caller buffer and writes no mode, format, or sequence byte.
    /// `FUN_140f8b810` does not write those bytes either.
    #[error("DATAGRAM_HEADER_UNKNOWN")]
    DatagramHeaderUnknown,
    /// The caller passed a type index other than the observed index for this message.
    #[error("type index {actual} does not match observed index {expected}")]
    UnexpectedTypeIndex {
        /// Type index from the protocol note.
        expected: u32,
        /// Type index supplied by the caller.
        actual: u32,
    },
    /// The body length does not match the observed layout.
    #[error("message body length {actual} does not match observed length {expected}")]
    UnexpectedBodyLength {
        /// Byte count from the protocol note.
        expected: usize,
        /// Byte count supplied by the caller.
        actual: usize,
    },
    /// `FUN_140f7fc60` returns `0x3e9` when the caller length is above the `0x600` byte slot.
    #[error("send slot length {length} is above capacity {capacity}")]
    SendSlotTooLong {
        /// Caller length that was not copied.
        length: usize,
        /// Slot capacity. See `SEND_SLOT_CAPACITY`.
        capacity: usize,
    },
}

/// Error returned when a UDP datagram would need an unknown header.
pub const DATAGRAM_HEADER_UNKNOWN: CarrierError = CarrierError::DatagramHeaderUnknown;

impl CarrierError {
    /// True when this error is [`DATAGRAM_HEADER_UNKNOWN`].
    #[must_use]
    pub const fn is_datagram_header_unknown(self) -> bool {
        matches!(self, Self::DatagramHeaderUnknown)
    }
}
