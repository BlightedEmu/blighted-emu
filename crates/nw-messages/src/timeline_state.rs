// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::TimelineComponentReplicatedState` (type 1647): the play state of a slice's timeline (a
//! gate's open or close animation, an encounter's death animation). Dungeon gates note
//! 2026-11-11 (Observed): slot 0 with two children, `PlayStates` and `PlayHeads`, each a
//! replicated list in its full form ([`FullList`]) of one entry: the play state (0 stopped,
//! 1 playing) and the play head (a big-endian `f32`, seconds).
//!
//! When a gate opens, retail sends its open timeline playing from 0 in the tick of the gate's
//! gatherable change ([`TimelineState::start`]), then, the timeline's length later, stopped at
//! its end ([`TimelineState::end`]). Each body carries the entity's next version (Amrine boss
//! door: 9, 10, 11 for close start, open start, open end; Inferred: one counter per entity).

use crate::items::FullList;
use crate::slots::{SLOT_0, children_mask, read_slot_0, write_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::TimelineComponentReplicatedState`.
pub const TIMELINE_TYPE_INDEX: u64 = 1647;
/// Children of the timeline state: `PlayStates` and `PlayHeads`.
const TIMELINE_CHILDREN: u8 = 2;
const PLAY_STATES_CHILD: u8 = 0;
/// A stopped timeline's play state.
pub const STOPPED: u8 = 0;
/// A playing timeline's play state.
pub const PLAYING: u8 = 1;
/// The play head of a timeline that starts.
const AT_START_S: f32 = 0.0;

/// `MB::TimelineComponentReplicatedState`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimelineState {
    /// `PlayStates`: [`STOPPED`] or [`PLAYING`] per played sequence.
    pub play_states: Option<FullList<u8>>,
    /// `PlayHeads`: the play head of each, seconds.
    pub play_heads: Option<FullList<f32>>,
}

impl TimelineState {
    /// One sequence in `play_state` at `play_head_s`, both lists at `version`.
    pub fn of(version: u64, play_state: u8, play_head_s: f32) -> Self {
        Self {
            play_states: Some(FullList {
                version: Some(version),
                elements: vec![play_state],
            }),
            play_heads: Some(FullList {
                version: Some(version),
                elements: vec![play_head_s],
            }),
        }
    }

    /// The timeline starts at `version`: playing from 0 (retail
    /// `010300010a010100010a0100000000`, the Amrine boss door's open at 356.931).
    pub fn start(version: u64) -> Self {
        Self::of(version, PLAYING, AT_START_S)
    }

    /// The timeline of `length_s` ended at `version`: stopped at its end (retail
    /// `010300010b010000010b0141000000`, the same door's open timeline 8 s later).
    pub fn end(version: u64, length_s: f32) -> Self {
        Self::of(version, STOPPED, length_s)
    }

    /// The timeline never played at `version`: stopped at 0 (retail
    /// `010300010401000001040100000000`, a closed Amrine gate's open timeline at its spawn).
    pub fn unplayed(version: u64) -> Self {
        Self::of(version, STOPPED, AT_START_S)
    }

    /// The highest version of its lists; `None` when neither has one.
    pub fn version(&self) -> Option<u64> {
        let states = self.play_states.as_ref().and_then(|list| list.version);
        let heads = self.play_heads.as_ref().and_then(|list| list.version);
        states.max(heads)
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, a list is not in its full form, or a
    /// slot or child is not in the table.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, "timeline", TIMELINE_CHILDREN, |reader, child| {
            if child == PLAY_STATES_CHILD {
                state.play_states = Some(FullList::read(reader, |reader| reader.u8())?);
            } else {
                state.play_heads = Some(FullList::read(reader, |reader| reader.f32())?);
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a list is too long for its count.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let present = [self.play_states.is_some(), self.play_heads.is_some()];
        let children = children_mask(&present);
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        write_children(writer, children, TIMELINE_CHILDREN, |writer, child| {
            if child == PLAY_STATES_CHILD {
                if let Some(list) = &self.play_states {
                    list.write(writer, |writer, state| {
                        writer.u8(*state);
                        Ok(())
                    })?;
                }
            } else if let Some(list) = &self.play_heads {
                list.write(writer, |writer, head| {
                    writer.f32(*head);
                    Ok(())
                })?;
            }
            Ok(())
        })
    }

    /// Its body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a list is too long for its count.
    pub fn body(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.write(&mut writer)?;
        Ok(writer.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Amrine gate length (`41000000`) and the Dynasty boss door's close (`40080000`).
    const AMRINE_GATE_S: f32 = 8.0;
    const DYNASTY_CLOSE_S: f32 = 2.125;
    const DYNASTY_OPEN_S: f32 = 1.5;

    fn hex(body: &[u8]) -> String {
        body.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn the_start_and_end_bodies_are_the_retail_bytes() {
        // Amrine (ow/01-Amrine_Excavation, EXP-Amrine Excavation): tall gate entry at 243.836,
        // boss door close start 349.146, open start 356.931, open end 364.894.
        let cases = [
            (
                TimelineState::end(4, AMRINE_GATE_S),
                "010300010401000001040141000000",
            ),
            (TimelineState::unplayed(4), "010300010401000001040100000000"),
            (TimelineState::start(9), "010300010901010001090100000000"),
            (TimelineState::start(10), "010300010a010100010a0100000000"),
            (
                TimelineState::end(11, AMRINE_GATE_S),
                "010300010b010000010b0141000000",
            ),
            // Dynasty Shipyard boss door: close start 656.432, close end 658.532, open end
            // 709.130.
            (TimelineState::start(7), "010300010701010001070100000000"),
            (
                TimelineState::end(8, DYNASTY_CLOSE_S),
                "010300010801000001080140080000",
            ),
            (
                TimelineState::end(11, DYNASTY_OPEN_S),
                "010300010b010000010b013fc00000",
            ),
        ];
        for (state, retail) in cases {
            let body = state.body().expect("body");
            assert_eq!(hex(&body), retail);
            let mut reader = Reader::new(&body);
            assert_eq!(TimelineState::read(&mut reader).expect("read"), state);
            assert!(reader.is_empty());
        }
    }

    #[test]
    fn empty_lists_read_and_give_their_version() {
        // Retail Amrine boss root c5 (`Timeline_Phase_1_IntroAnim`) at 337.097.
        const EMPTY: [u8; 10] = [0x01, 0x03, 0x00, 0x01, 0x02, 0x00, 0x00, 0x01, 0x02, 0x00];
        let state = TimelineState::read(&mut Reader::new(&EMPTY)).expect("read");
        assert_eq!(state.version(), Some(2));
        assert!(
            state
                .play_states
                .as_ref()
                .is_some_and(|list| list.elements.is_empty())
        );
        assert_eq!(state.body().expect("body"), EMPTY);
    }
}
