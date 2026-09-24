// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ghosts over several bus messages. A sim sends the ghosts of one round for another sim in
//! parts of at most [`GHOST_PART_ENTITIES`], so each part fits one bus message however many
//! players stand near the border; the receiver takes the round only once every part came, so a
//! partial round never hides the ghosts of the parts still on the way.

use std::collections::HashMap;

use crate::id::{BusId, MAX_BUS_ID_LEN};
use crate::message::{Ghost, Ghosts, MAX_BUS_MESSAGE_BYTES, MAX_GHOST_BYTES};

/// Most bytes of one [`Ghosts::gde_refs`] entry in `postcard`: a 10-byte entity number and the
/// 16 id bytes.
pub const MAX_GHOST_REF_BYTES: usize = 26;
/// Room in one part for all but the ghosts and their ids: the sender id with its length, the
/// round, the part numbers and the two list lengths, each at its longest `postcard` form.
pub const GHOST_PART_HEADER_BYTES: usize = MAX_BUS_ID_LEN + VARINT_BYTES * VARINTS;
/// Longest `postcard` form of an integer.
const VARINT_BYTES: usize = 10;
/// The integers of a part besides the ghosts: the id length, the round, the part, the parts and
/// the two list lengths.
const VARINTS: usize = 6;
/// Most ghosts of one part.
pub const GHOST_PART_ENTITIES: usize =
    (MAX_BUS_MESSAGE_BYTES - GHOST_PART_HEADER_BYTES) / (MAX_GHOST_BYTES + MAX_GHOST_REF_BYTES);

/// The parts of round `round` of the ghosts `entities` of `from`, without their ids: at least
/// one part (an empty round clears the receiver's ghosts of `from`).
pub fn ghost_parts(from: &BusId, round: u64, entities: Vec<Ghost>) -> Vec<Ghosts> {
    let parts = entities.len().div_ceil(GHOST_PART_ENTITIES).max(1);
    let part_count = u16::try_from(parts).unwrap_or(u16::MAX);
    let mut entities = entities.into_iter();
    (0..part_count)
        .map(|part| Ghosts {
            from: from.clone(),
            entities: entities.by_ref().take(GHOST_PART_ENTITIES).collect(),
            gde_refs: Vec::new(),
            round,
            part,
            parts: part_count,
        })
        .collect()
}

/// The parts of one sender's round that came so far.
#[derive(Debug)]
struct Pending {
    round: u64,
    came: Vec<bool>,
    ghosts: Ghosts,
}

/// Puts the parts of each sender's rounds back together.
#[derive(Debug, Default)]
pub struct GhostAssembly {
    pending: HashMap<BusId, Pending>,
}

impl GhostAssembly {
    /// Takes one part; returns the whole round once its last part came. A part of another
    /// round than the one being put together starts that round over (a lost part loses its
    /// round, never the next one); a part that came already or names no part of its round is
    /// dropped.
    pub fn take(&mut self, part: Ghosts) -> Option<Ghosts> {
        let parts = usize::from(part.parts.max(1));
        let index = usize::from(part.part);
        if index >= parts {
            return None;
        }
        if parts == 1 {
            self.pending.remove(&part.from);
            return Some(part);
        }
        let from = part.from.clone();
        let pending = self
            .pending
            .entry(from.clone())
            .and_modify(|pending| {
                if pending.round != part.round || pending.came.len() != parts {
                    *pending = Pending::start(&part, parts);
                }
            })
            .or_insert_with(|| Pending::start(&part, parts));
        if pending.came[index] {
            return None;
        }
        pending.came[index] = true;
        pending.ghosts.entities.extend(part.entities);
        pending.ghosts.gde_refs.extend(part.gde_refs);
        if pending.came.iter().all(|came| *came) {
            return self.pending.remove(&from).map(|pending| pending.ghosts);
        }
        None
    }
}

impl Pending {
    /// An empty round of the sender and round of `part`, as one whole part.
    fn start(part: &Ghosts, parts: usize) -> Self {
        Self {
            round: part.round,
            came: vec![false; parts],
            ghosts: Ghosts {
                from: part.from.clone(),
                entities: Vec::new(),
                gde_refs: Vec::new(),
                round: part.round,
                part: 0,
                parts: 1,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Point, encode};

    const ROUND: u64 = 7;
    const RAW_ID: [u8; 16] = [0xab; 16];

    fn sim() -> BusId {
        BusId::try_from("sim-a".to_owned()).expect("id")
    }

    fn ghost(entity: u64) -> Ghost {
        Ghost {
            entity,
            position: Point {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            mob: None,
            player: None,
        }
    }

    #[test]
    fn a_full_part_fits_one_bus_message() {
        // Each ghost is at most `MAX_GHOST_BYTES` (`message` tests); each id entry at most:
        let entry = encode(&(u64::MAX, RAW_ID)).expect("encode");
        assert!(entry.len() <= MAX_GHOST_REF_BYTES);
        let id = BusId::try_from("s".repeat(MAX_BUS_ID_LEN)).expect("id");
        let header = encode(&Ghosts {
            from: id,
            entities: Vec::new(),
            gde_refs: Vec::new(),
            round: u64::MAX,
            part: u16::MAX,
            parts: u16::MAX,
        })
        .expect("encode");
        assert!(header.len() <= GHOST_PART_HEADER_BYTES);
        assert!(
            GHOST_PART_ENTITIES * (MAX_GHOST_BYTES + MAX_GHOST_REF_BYTES) + GHOST_PART_HEADER_BYTES
                <= MAX_BUS_MESSAGE_BYTES
        );
    }

    #[test]
    fn many_ghosts_go_in_parts_and_come_back_whole() {
        let count = GHOST_PART_ENTITIES * 2 + 1;
        let entities: Vec<Ghost> = (0..count as u64).map(ghost).collect();
        let parts = ghost_parts(&sim(), ROUND, entities.clone());
        assert_eq!(parts.len(), 3);
        assert!(
            parts
                .iter()
                .all(|part| part.parts == 3 && part.round == ROUND)
        );
        let mut assembly = GhostAssembly::default();
        let mut whole = None;
        for mut part in parts {
            part.gde_refs = part
                .entities
                .iter()
                .map(|ghost| (ghost.entity, RAW_ID))
                .collect();
            assert!(whole.is_none(), "whole only after the last part");
            whole = assembly.take(part);
        }
        let whole = whole.expect("the whole round");
        assert_eq!(whole.entities, entities);
        assert_eq!(whole.gde_refs.len(), count);
        assert_eq!((whole.part, whole.parts), (0, 1));
    }

    #[test]
    fn an_empty_round_is_one_part() {
        let parts = ghost_parts(&sim(), ROUND, Vec::new());
        assert_eq!(parts.len(), 1);
        let mut assembly = GhostAssembly::default();
        let whole = assembly.take(parts[0].clone()).expect("one part is whole");
        assert!(whole.entities.is_empty());
    }

    #[test]
    fn a_new_round_drops_a_round_that_lost_a_part() {
        let count = GHOST_PART_ENTITIES + 1;
        let first = ghost_parts(&sim(), ROUND, (0..count as u64).map(ghost).collect());
        let second = ghost_parts(&sim(), ROUND + 1, (0..count as u64).map(ghost).collect());
        let mut assembly = GhostAssembly::default();
        assert!(assembly.take(first[0].clone()).is_none());
        assert!(
            assembly.take(second[0].clone()).is_none(),
            "round 7 lost its part 1"
        );
        assert!(
            assembly.take(second[0].clone()).is_none(),
            "a repeated part"
        );
        assert!(
            assembly.take(first[1].clone()).is_none(),
            "an old round's part starts it over"
        );
        assert!(assembly.take(second[0].clone()).is_none());
        let whole = assembly.take(second[1].clone()).expect("round 8 whole");
        assert_eq!(whole.round, ROUND + 1);
        assert_eq!(whole.entities.len(), count);
        let mut bad = second[1].clone();
        bad.part = bad.parts;
        assert!(assembly.take(bad).is_none(), "no such part");
    }
}
