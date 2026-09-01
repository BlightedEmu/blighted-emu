// SPDX-License-Identifier: AGPL-3.0-or-later
//! Send-slot copy from `FUN_140f7fc60` (`0x140f7fc60`).
//!
//! The function copies the caller buffer into a `0x600` byte slot and copies `0x1c` side bytes.
//! A length above `0x600` returns `0x3e9` and copies nothing.
//! It does not write a mode byte, a format byte, or a sequence.
//! The slot address is vtable `0x147fbe530` offset `+0x18`, and also `0x1484807a8`.

use crate::error::CarrierError;

/// Bytes in one send slot. `FUN_140f69c00` sizes the slot at this length.
pub const SEND_SLOT_CAPACITY: usize = 0x600;

/// Side-record bytes copied with each slot.
pub const SEND_SLOT_SIDE_OCTETS: usize = 0x1c;

/// Return code when the caller length is above [`SEND_SLOT_CAPACITY`].
pub const SEND_SLOT_TOO_LONG: u32 = 0x3e9;

/// `FUN_140f7fc60`.
pub const SEND_FILL_FUNCTION: u64 = 0x140F_7FC60;

/// Vtable stored by `FUN_140f47000`.
pub const SEND_FILL_VTABLE: u64 = 0x147F_BE530;

/// Slot offset of [`SEND_FILL_FUNCTION`] in [`SEND_FILL_VTABLE`].
pub const SEND_FILL_SLOT_OFFSET: u64 = 0x18;

/// Address of that vtable slot (`0x147fbe548`).
pub const SEND_FILL_SLOT_ADDRESS: u64 = 0x147F_BE548;

/// Second vtable. The same fill function is the slot at [`SEND_FILL_SECOND_SLOT_ADDRESS`].
pub const SEND_FILL_SECOND_VTABLE: u64 = 0x1484_80790;

/// Second slot address. Offset `+0x18` from [`SEND_FILL_SECOND_VTABLE`].
pub const SEND_FILL_SECOND_SLOT_ADDRESS: u64 = 0x1484_807A8;

/// Condition variable woken on a successful fill. Object +0x40.
pub const SEND_FILL_CONDVAR_OFFSET: usize = 0x40;

/// Counter incremented on a successful fill. Object +0x48. The note does not give its width.
pub const SEND_FILL_COUNTER_OFFSET: usize = 0x48;

/// Slot index `int`. Object +0xe8.
pub const SEND_FILL_SLOT_INDEX_OFFSET: usize = 0xe8;

/// Length stored in the descriptor. Object +0x150.
pub const SEND_FILL_LENGTH_OFFSET: usize = 0x150;

/// Base of the caller-buffer copies. Object +0x1a0, plus slot * [`SEND_SLOT_CAPACITY`].
pub const SEND_FILL_BYTES_OFFSET: usize = 0x1a0;

/// Base of the side records. Object +0x1a8, plus slot * [`SEND_SLOT_SIDE_OCTETS`].
pub const SEND_FILL_SIDE_OFFSET: usize = 0x1a8;

/// Byte offset of one slot's caller-buffer copy.
#[must_use]
pub const fn send_slot_bytes_offset(slot: usize) -> usize {
    SEND_FILL_BYTES_OFFSET + slot * SEND_SLOT_CAPACITY
}

/// Byte offset of one slot's side record.
#[must_use]
pub const fn send_slot_side_offset(slot: usize) -> usize {
    SEND_FILL_SIDE_OFFSET + slot * SEND_SLOT_SIDE_OCTETS
}

/// One `0x600` byte send slot and its `0x1c` byte side record.
///
/// [`Self::fill`] copies bytes. It does not add a datagram header.
/// The client also wakes the condition variable at [`SEND_FILL_CONDVAR_OFFSET`].
/// This crate does not start `FUN_140f8b810`. [`Self::fill_count`] counts successful fills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendSlot {
    bytes: [u8; SEND_SLOT_CAPACITY],
    length: usize,
    side: [u8; SEND_SLOT_SIDE_OCTETS],
    fill_count: u64,
}

impl SendSlot {
    /// Empty slot. The note does not show the client's initial bytes.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            bytes: [0; SEND_SLOT_CAPACITY],
            length: 0,
            side: [0; SEND_SLOT_SIDE_OCTETS],
            fill_count: 0,
        }
    }

    /// Copy `payload` and `side` into this slot.
    ///
    /// A payload longer than [`SEND_SLOT_CAPACITY`] leaves the slot unchanged.
    /// The copy is the payload bytes only. No mode, format, or sequence byte is written.
    /// Bytes past `payload.len()` stay as they were.
    ///
    /// # Errors
    ///
    /// Returns [`CarrierError::SendSlotTooLong`] when `payload.len()` is above [`SEND_SLOT_CAPACITY`].
    /// The client returns [`SEND_SLOT_TOO_LONG`] on that path. The success integer is not in the note.
    pub fn fill(
        &mut self,
        payload: &[u8],
        side: &[u8; SEND_SLOT_SIDE_OCTETS],
    ) -> Result<(), CarrierError> {
        if payload.len() > SEND_SLOT_CAPACITY {
            return Err(CarrierError::SendSlotTooLong {
                length: payload.len(),
                capacity: SEND_SLOT_CAPACITY,
            });
        }
        self.bytes[..payload.len()].copy_from_slice(payload);
        self.length = payload.len();
        self.side = *side;
        self.fill_count += 1;
        Ok(())
    }

    /// Copied caller buffer. Its length is [`Self::length`].
    #[must_use]
    pub fn copied(&self) -> &[u8] {
        &self.bytes[..self.length]
    }

    /// Full `0x600` byte slot, including bytes past [`Self::length`].
    #[must_use]
    pub const fn slot_bytes(&self) -> &[u8; SEND_SLOT_CAPACITY] {
        &self.bytes
    }

    /// Stored length. This is the caller length, not a header size.
    #[must_use]
    pub const fn length(&self) -> usize {
        self.length
    }

    /// Side record copied on the last successful fill.
    #[must_use]
    pub const fn side(&self) -> &[u8; SEND_SLOT_SIDE_OCTETS] {
        &self.side
    }

    /// Successful fills. This is not a claim about the width of the client counter at +0x48.
    #[must_use]
    pub const fn fill_count(&self) -> u64 {
        self.fill_count
    }
}

impl Default for SendSlot {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr, UdpSocket};

    use super::*;
    use crate::DATAGRAM_HEADER_UNKNOWN;
    use crate::transport::send_udp_datagram;

    const CLIENT_SLOT_BYTES: usize = 0x600;
    const CLIENT_SIDE_BYTES: usize = 0x1c;
    const CLIENT_TOO_LONG: u32 = 0x3e9;
    const SLOT_INDEX: usize = 1;
    const FIRST_FILL: [u8; 4] = [0x11, 0x22, 0x33, 0x44];
    const SECOND_FILL: [u8; 2] = [0x99, 0x88];
    const SIDE_FILL: u8 = 0x5A;
    const OTHER_SIDE: u8 = 0x3C;
    const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;
    const EPHEMERAL_PORT: u16 = 0;

    #[test]
    fn addresses_match_the_note() {
        assert_eq!(SEND_SLOT_CAPACITY, CLIENT_SLOT_BYTES);
        assert_eq!(SEND_SLOT_SIDE_OCTETS, CLIENT_SIDE_BYTES);
        assert_eq!(SEND_SLOT_TOO_LONG, CLIENT_TOO_LONG);
        assert_eq!(
            SEND_FILL_VTABLE + SEND_FILL_SLOT_OFFSET,
            SEND_FILL_SLOT_ADDRESS
        );
        assert_eq!(
            SEND_FILL_SECOND_VTABLE + SEND_FILL_SLOT_OFFSET,
            SEND_FILL_SECOND_SLOT_ADDRESS
        );
        assert_ne!(SEND_FILL_FUNCTION, SEND_FILL_SLOT_ADDRESS);
        assert_eq!(send_slot_bytes_offset(0), SEND_FILL_BYTES_OFFSET);
        assert_eq!(
            send_slot_bytes_offset(SLOT_INDEX),
            SEND_FILL_BYTES_OFFSET + SLOT_INDEX * SEND_SLOT_CAPACITY
        );
        assert_eq!(send_slot_side_offset(0), SEND_FILL_SIDE_OFFSET);
        assert_eq!(
            send_slot_side_offset(SLOT_INDEX),
            SEND_FILL_SIDE_OFFSET + SLOT_INDEX * SEND_SLOT_SIDE_OCTETS
        );
        assert_eq!(SEND_FILL_COUNTER_OFFSET, 0x48);
        assert_eq!(SEND_FILL_CONDVAR_OFFSET, 0x40);
        assert_eq!(SEND_FILL_SLOT_INDEX_OFFSET, 0xe8);
        assert_eq!(SEND_FILL_LENGTH_OFFSET, 0x150);
    }

    #[test]
    fn fill_copies_the_caller_buffer_and_writes_no_header() {
        let mut slot = SendSlot::empty();
        assert_eq!(slot, SendSlot::default());
        let side = [SIDE_FILL; SEND_SLOT_SIDE_OCTETS];
        slot.fill(&FIRST_FILL, &side).expect("first fill");
        assert_eq!(slot.copied(), FIRST_FILL);
        assert_eq!(slot.length(), FIRST_FILL.len());
        assert_eq!(slot.side(), &side);
        assert_eq!(slot.fill_count(), 1);
        assert_eq!(&slot.slot_bytes()[..FIRST_FILL.len()], &FIRST_FILL);

        let other_side = [OTHER_SIDE; SEND_SLOT_SIDE_OCTETS];
        slot.fill(&SECOND_FILL, &other_side).expect("second fill");
        assert_eq!(slot.copied(), SECOND_FILL);
        assert_eq!(slot.length(), SECOND_FILL.len());
        assert_eq!(
            slot.slot_bytes()[SECOND_FILL.len()],
            FIRST_FILL[SECOND_FILL.len()]
        );
        assert_eq!(
            slot.slot_bytes()[FIRST_FILL.len() - 1],
            FIRST_FILL[FIRST_FILL.len() - 1]
        );
        assert_eq!(slot.side(), &other_side);
        assert_eq!(slot.fill_count(), 2);

        let socket = UdpSocket::bind(SocketAddr::from((LOOPBACK, EPHEMERAL_PORT))).expect("bind");
        let destination = socket.local_addr().expect("address");
        assert_eq!(
            send_udp_datagram(&socket, destination, slot.copied()).expect_err("send"),
            DATAGRAM_HEADER_UNKNOWN
        );
    }

    #[test]
    fn a_payload_above_0x600_copies_nothing() {
        let mut slot = SendSlot::empty();
        let side = [SIDE_FILL; SEND_SLOT_SIDE_OCTETS];
        slot.fill(&FIRST_FILL, &side).expect("seed");
        let before = slot.clone();
        let too_long = vec![0xAB_u8; SEND_SLOT_CAPACITY + 1];
        let error = slot
            .fill(&too_long, &[OTHER_SIDE; SEND_SLOT_SIDE_OCTETS])
            .expect_err("long");
        assert_eq!(
            error,
            CarrierError::SendSlotTooLong {
                length: SEND_SLOT_CAPACITY + 1,
                capacity: SEND_SLOT_CAPACITY,
            }
        );
        assert!(!error.is_datagram_header_unknown());
        assert_eq!(slot, before);

        let exact = vec![0xCD_u8; SEND_SLOT_CAPACITY];
        slot.fill(&exact, &side).expect("exact");
        assert_eq!(slot.length(), SEND_SLOT_CAPACITY);
        assert_eq!(slot.copied(), exact.as_slice());
        assert_eq!(slot.fill_count(), 2);
    }
}
