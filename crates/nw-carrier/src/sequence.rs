// SPDX-License-Identifier: AGPL-3.0-or-later
//! Connection-object sequence counters.
//!
//! Protocol note `docs/re/2026-08-29-client-protocol-login-gridmate.md`, section 2.
//! `FUN_140f76d60` prints these fields:
//! `conn.sendSeqNum` is four `uint16` values at connection object +0x6c, +0x6e, +0x70, +0x72.
//! `conn.sendReliableSeqNum` is four `uint16` values at connection object +0x74, +0x76, +0x78, +0x7a.
//! These offsets are fields on the connection object. They are not a datagram header.

/// Number of `uint16` fields in `conn.sendSeqNum`, and in `conn.sendReliableSeqNum`.
pub const SEQUENCE_SLOT_COUNT: usize = 4;

/// Width of one sequence field. Each field is a `uint16`.
pub const UINT16_OCTETS: usize = 2;

/// Bit width of a `uint16` sequence counter.
const SEQUENCE_VALUE_BITS: u32 = 16;

/// One bit, used to build the half-range boundary.
const SEQUENCE_UNIT_BIT: u16 = 1;

/// Shift that selects half of the `uint16` range.
const SEQUENCE_HALF_SHIFT: u32 = SEQUENCE_VALUE_BITS - 1;

/// Distance at which two `uint16` counters are opposite on the number circle.
/// A compare at this exact distance has no winner in this crate.
pub const SEQUENCE_HALF_RANGE: u16 = SEQUENCE_UNIT_BIT << SEQUENCE_HALF_SHIFT;

/// Step added by [`wrapping_next`].
pub const SEQUENCE_STEP: u16 = 1;

/// `conn.sendSeqNum` slot 0. Connection object +0x6c.
pub const SEND_SEQ_OFFSET_0: usize = 0x6c;
/// `conn.sendSeqNum` slot 1. Connection object +0x6e.
pub const SEND_SEQ_OFFSET_1: usize = 0x6e;
/// `conn.sendSeqNum` slot 2. Connection object +0x70.
pub const SEND_SEQ_OFFSET_2: usize = 0x70;
/// `conn.sendSeqNum` slot 3. Connection object +0x72.
pub const SEND_SEQ_OFFSET_3: usize = 0x72;

/// `conn.sendReliableSeqNum` slot 0. Connection object +0x74.
pub const SEND_RELIABLE_SEQ_OFFSET_0: usize = 0x74;
/// `conn.sendReliableSeqNum` slot 1. Connection object +0x76.
pub const SEND_RELIABLE_SEQ_OFFSET_1: usize = 0x76;
/// `conn.sendReliableSeqNum` slot 2. Connection object +0x78.
pub const SEND_RELIABLE_SEQ_OFFSET_2: usize = 0x78;
/// `conn.sendReliableSeqNum` slot 3. Connection object +0x7a.
pub const SEND_RELIABLE_SEQ_OFFSET_3: usize = 0x7a;

/// One of the four observed sequence slots.
///
/// The note infers channel indexes 0 through 3 because four fields are present.
/// It does not show which slots appear on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceSlot {
    /// First of the four fields.
    Zero,
    /// Second of the four fields.
    One,
    /// Third of the four fields. Connection object +0x70 and +0x78.
    Two,
    /// Fourth of the four fields.
    Three,
}

impl SequenceSlot {
    /// Every observed slot, in offset order.
    pub const ALL: [Self; SEQUENCE_SLOT_COUNT] = [Self::Zero, Self::One, Self::Two, Self::Three];

    /// Connection-object offset of this slot's `conn.sendSeqNum` field.
    #[must_use]
    pub const fn send_seq_offset(self) -> usize {
        match self {
            Self::Zero => SEND_SEQ_OFFSET_0,
            Self::One => SEND_SEQ_OFFSET_1,
            Self::Two => SEND_SEQ_OFFSET_2,
            Self::Three => SEND_SEQ_OFFSET_3,
        }
    }

    /// Connection-object offset of this slot's `conn.sendReliableSeqNum` field.
    #[must_use]
    pub const fn send_reliable_seq_offset(self) -> usize {
        match self {
            Self::Zero => SEND_RELIABLE_SEQ_OFFSET_0,
            Self::One => SEND_RELIABLE_SEQ_OFFSET_1,
            Self::Two => SEND_RELIABLE_SEQ_OFFSET_2,
            Self::Three => SEND_RELIABLE_SEQ_OFFSET_3,
        }
    }
}

/// Result of comparing two `uint16` counters with wrap.
///
/// This is local arithmetic on the stored counters.
/// The protocol note does not describe a compare routine in the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceOrder {
    /// The two counters are the same value.
    Equal,
    /// `left` is behind `right` inside the half range.
    Before,
    /// `left` is ahead of `right` inside the half range.
    After,
    /// The counters differ by exactly [`SEQUENCE_HALF_RANGE`]. This crate picks no winner.
    Ambiguous,
}

/// Next `uint16` counter. `u16::MAX` wraps to 0.
#[must_use]
pub const fn wrapping_next(value: u16) -> u16 {
    value.wrapping_add(SEQUENCE_STEP)
}

/// Compare `left` with `right` on a `uint16` circle.
///
/// A forward distance below [`SEQUENCE_HALF_RANGE`] means `left` is [`SequenceOrder::After`] `right`.
#[must_use]
pub const fn compare_u16_sequence(left: u16, right: u16) -> SequenceOrder {
    if left == right {
        return SequenceOrder::Equal;
    }
    let ahead = left.wrapping_sub(right);
    if ahead == SEQUENCE_HALF_RANGE {
        SequenceOrder::Ambiguous
    } else if ahead < SEQUENCE_HALF_RANGE {
        SequenceOrder::After
    } else {
        SequenceOrder::Before
    }
}

/// The eight observed sequence counters on one connection object.
///
/// Offsets: send sequence at connection object +0x6c..+0x72, reliable sequence at +0x74..+0x7a.
/// See the constants on [`SequenceSlot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionSequences {
    /// `conn.sendSeqNum` at connection object +0x6c.
    pub send_seq_0: u16,
    /// `conn.sendSeqNum` at connection object +0x6e.
    pub send_seq_1: u16,
    /// `conn.sendSeqNum` at connection object +0x70.
    pub send_seq_2: u16,
    /// `conn.sendSeqNum` at connection object +0x72.
    pub send_seq_3: u16,
    /// `conn.sendReliableSeqNum` at connection object +0x74.
    pub send_reliable_seq_0: u16,
    /// `conn.sendReliableSeqNum` at connection object +0x76.
    pub send_reliable_seq_1: u16,
    /// `conn.sendReliableSeqNum` at connection object +0x78.
    pub send_reliable_seq_2: u16,
    /// `conn.sendReliableSeqNum` at connection object +0x7a.
    pub send_reliable_seq_3: u16,
}

impl ConnectionSequences {
    /// Blank counters. The protocol note does not show the client's start values.
    #[must_use]
    pub const fn zeros() -> Self {
        Self {
            send_seq_0: 0,
            send_seq_1: 0,
            send_seq_2: 0,
            send_seq_3: 0,
            send_reliable_seq_0: 0,
            send_reliable_seq_1: 0,
            send_reliable_seq_2: 0,
            send_reliable_seq_3: 0,
        }
    }

    /// Send-sequence counter for `slot`.
    #[must_use]
    pub const fn send_seq(self, slot: SequenceSlot) -> u16 {
        match slot {
            SequenceSlot::Zero => self.send_seq_0,
            SequenceSlot::One => self.send_seq_1,
            SequenceSlot::Two => self.send_seq_2,
            SequenceSlot::Three => self.send_seq_3,
        }
    }

    /// Reliable send-sequence counter for `slot`.
    #[must_use]
    pub const fn send_reliable_seq(self, slot: SequenceSlot) -> u16 {
        match slot {
            SequenceSlot::Zero => self.send_reliable_seq_0,
            SequenceSlot::One => self.send_reliable_seq_1,
            SequenceSlot::Two => self.send_reliable_seq_2,
            SequenceSlot::Three => self.send_reliable_seq_3,
        }
    }

    /// Store a send-sequence counter.
    pub const fn set_send_seq(&mut self, slot: SequenceSlot, value: u16) {
        match slot {
            SequenceSlot::Zero => self.send_seq_0 = value,
            SequenceSlot::One => self.send_seq_1 = value,
            SequenceSlot::Two => self.send_seq_2 = value,
            SequenceSlot::Three => self.send_seq_3 = value,
        }
    }

    /// Store a reliable send-sequence counter.
    pub const fn set_send_reliable_seq(&mut self, slot: SequenceSlot, value: u16) {
        match slot {
            SequenceSlot::Zero => self.send_reliable_seq_0 = value,
            SequenceSlot::One => self.send_reliable_seq_1 = value,
            SequenceSlot::Two => self.send_reliable_seq_2 = value,
            SequenceSlot::Three => self.send_reliable_seq_3 = value,
        }
    }

    /// Advance one send-sequence counter by [`SEQUENCE_STEP`], with `uint16` wrap.
    pub const fn advance_send_seq(&mut self, slot: SequenceSlot) -> u16 {
        let next = wrapping_next(self.send_seq(slot));
        self.set_send_seq(slot, next);
        next
    }

    /// Advance one reliable send-sequence counter by [`SEQUENCE_STEP`], with `uint16` wrap.
    pub const fn advance_send_reliable_seq(&mut self, slot: SequenceSlot) -> u16 {
        let next = wrapping_next(self.send_reliable_seq(slot));
        self.set_send_reliable_seq(slot, next);
        next
    }

    /// Compare one stored send-sequence counter with `other`.
    #[must_use]
    pub const fn compare_send_seq(self, slot: SequenceSlot, other: u16) -> SequenceOrder {
        compare_u16_sequence(self.send_seq(slot), other)
    }

    /// Compare one stored reliable send-sequence counter with `other`.
    #[must_use]
    pub const fn compare_send_reliable_seq(self, slot: SequenceSlot, other: u16) -> SequenceOrder {
        compare_u16_sequence(self.send_reliable_seq(slot), other)
    }
}

impl Default for ConnectionSequences {
    fn default() -> Self {
        Self::zeros()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAST_SEQUENCE: u16 = u16::MAX;
    const WRAPPED_SEQUENCE: u16 = 0;
    const AHEAD_BY_ONE: u16 = 1;

    #[test]
    fn offsets_match_the_note() {
        assert_eq!(SequenceSlot::ALL.len(), SEQUENCE_SLOT_COUNT);
        assert_eq!(SequenceSlot::Zero.send_seq_offset(), SEND_SEQ_OFFSET_0);
        assert_eq!(SequenceSlot::One.send_seq_offset(), SEND_SEQ_OFFSET_1);
        assert_eq!(SequenceSlot::Two.send_seq_offset(), SEND_SEQ_OFFSET_2);
        assert_eq!(SequenceSlot::Three.send_seq_offset(), SEND_SEQ_OFFSET_3);
        assert_eq!(
            SequenceSlot::Zero.send_reliable_seq_offset(),
            SEND_RELIABLE_SEQ_OFFSET_0
        );
        assert_eq!(
            SequenceSlot::One.send_reliable_seq_offset(),
            SEND_RELIABLE_SEQ_OFFSET_1
        );
        assert_eq!(
            SequenceSlot::Two.send_reliable_seq_offset(),
            SEND_RELIABLE_SEQ_OFFSET_2
        );
        assert_eq!(
            SequenceSlot::Three.send_reliable_seq_offset(),
            SEND_RELIABLE_SEQ_OFFSET_3
        );

        let mut previous_send = SEND_SEQ_OFFSET_0;
        let mut previous_reliable = SEND_RELIABLE_SEQ_OFFSET_0;
        for slot in SequenceSlot::ALL.into_iter().skip(1) {
            assert_eq!(slot.send_seq_offset(), previous_send + UINT16_OCTETS);
            assert_eq!(
                slot.send_reliable_seq_offset(),
                previous_reliable + UINT16_OCTETS
            );
            previous_send = slot.send_seq_offset();
            previous_reliable = slot.send_reliable_seq_offset();
        }
        assert_eq!(
            SEND_RELIABLE_SEQ_OFFSET_0,
            SEND_SEQ_OFFSET_3 + UINT16_OCTETS
        );
    }

    #[test]
    fn uint16_wraps_and_compares() {
        assert_eq!(wrapping_next(LAST_SEQUENCE), WRAPPED_SEQUENCE);
        assert_eq!(wrapping_next(WRAPPED_SEQUENCE), AHEAD_BY_ONE);
        assert_eq!(
            compare_u16_sequence(AHEAD_BY_ONE, WRAPPED_SEQUENCE),
            SequenceOrder::After
        );
        assert_eq!(
            compare_u16_sequence(WRAPPED_SEQUENCE, AHEAD_BY_ONE),
            SequenceOrder::Before
        );
        assert_eq!(
            compare_u16_sequence(WRAPPED_SEQUENCE, LAST_SEQUENCE),
            SequenceOrder::After
        );
        assert_eq!(
            compare_u16_sequence(LAST_SEQUENCE, WRAPPED_SEQUENCE),
            SequenceOrder::Before
        );
        assert_eq!(
            compare_u16_sequence(AHEAD_BY_ONE, AHEAD_BY_ONE),
            SequenceOrder::Equal
        );
        assert_eq!(
            compare_u16_sequence(WRAPPED_SEQUENCE, SEQUENCE_HALF_RANGE),
            SequenceOrder::Ambiguous
        );
        assert_eq!(
            compare_u16_sequence(SEQUENCE_HALF_RANGE, WRAPPED_SEQUENCE),
            SequenceOrder::Ambiguous
        );
    }

    #[test]
    fn each_slot_wraps_without_changing_the_other_counters() {
        let mut sequences = ConnectionSequences::zeros();
        assert_eq!(sequences, ConnectionSequences::default());
        for slot in SequenceSlot::ALL {
            sequences.set_send_seq(slot, LAST_SEQUENCE);
            sequences.set_send_reliable_seq(slot, LAST_SEQUENCE);
        }
        assert_eq!(
            sequences.advance_send_seq(SequenceSlot::Zero),
            WRAPPED_SEQUENCE
        );
        assert_eq!(
            sequences.advance_send_reliable_seq(SequenceSlot::Three),
            WRAPPED_SEQUENCE
        );
        assert_eq!(sequences.send_seq(SequenceSlot::Zero), WRAPPED_SEQUENCE);
        assert_eq!(sequences.send_seq(SequenceSlot::One), LAST_SEQUENCE);
        assert_eq!(sequences.send_seq(SequenceSlot::Two), LAST_SEQUENCE);
        assert_eq!(sequences.send_seq(SequenceSlot::Three), LAST_SEQUENCE);
        assert_eq!(
            sequences.send_reliable_seq(SequenceSlot::Zero),
            LAST_SEQUENCE
        );
        assert_eq!(
            sequences.send_reliable_seq(SequenceSlot::One),
            LAST_SEQUENCE
        );
        assert_eq!(
            sequences.send_reliable_seq(SequenceSlot::Two),
            LAST_SEQUENCE
        );
        assert_eq!(
            sequences.send_reliable_seq(SequenceSlot::Three),
            WRAPPED_SEQUENCE
        );
        assert_eq!(
            sequences.compare_send_seq(SequenceSlot::Zero, LAST_SEQUENCE),
            SequenceOrder::After
        );
        assert_eq!(
            sequences.compare_send_reliable_seq(SequenceSlot::Three, LAST_SEQUENCE),
            SequenceOrder::After
        );
        assert_eq!(
            sequences.compare_send_seq(SequenceSlot::Two, LAST_SEQUENCE),
            SequenceOrder::Equal
        );
    }
}
