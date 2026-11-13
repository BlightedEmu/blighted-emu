// SPDX-License-Identifier: AGPL-3.0-or-later
//! The state of an open-world dungeon entrance, `Javelin::DungeonEntranceReplicatedState`
//! (3105): slot 0 child 0 `dungeonsSpawned`, an array of the runs that were started at the
//! entrance and still run (dungeon entrance note, 2026-11-12).
//!
//! **Observed** (117 retail ledgers, 1,469 bodies): an element is a key (a `u64` and 16 bytes),
//! a `u8` and 16 bytes, all big-endian on the wire. The array is empty (`01 01 00`) when the
//! entrance comes into view with no run; an element comes when a run starts (in the same tick as
//! the starter's 6776 `Success`: `12a_msq_24_defeatgawain` 135.317 and 135.325 s) and goes when
//! the run ends (0.2 to 12 s after the player came back out). One entrance held up to 3 runs at
//! once (`Hive_gorgon_raid_third_boss`). The key is the starter's group: its 16 bytes are the
//! group's id in 9 group runs (the `u64` 0), its `u64` a raid id in 8 raid runs of the gorgon
//! raid (the 16 bytes 0), and both are 0 for a player in no group (940 keys). Two players of one
//! group run had the same element (`HiveNormaleExpedition` and
//! `amrine_group_run_truegrave_hiveshadow_graphx`). The 16-byte value differs per run and shows
//! nowhere else in the ledgers. The `u8` was 0 in every body.
//!
//! **Observed** (client): the state's handler (`FUN_142ff9950`, the change callback of
//! `dungeonsSpawned`, constructor `FUN_142fe0730`, 0x70 bytes per element) copies the array into
//! the entrance component and asks the local player's group or raid id (a raid id when the
//! player is in a raid, else the group id); when an element's key matches it, the component
//! sets its "our run is spawned here" flag and tells its listeners (`FUN_142ffcc60`). The
//! entrance's interaction options (`FUN_142ff6730`) then pass the matching element's 16-byte
//! value to the entrance screen (the re-entry of the run) instead of the activation screen.
//! The same match runs again when the player joins or leaves a group or raid (`FUN_142ffc890`,
//! `FUN_142ffc300`, `FUN_142ffc350`, `FUN_142ffd0c0`). The `u8` is only compared for equality
//! (`FUN_142fb7810`); no traced reader uses it (its meaning is Unknown; this server sends the
//! retail 0).
//!
//! **Inferred**: the 16-byte value is the run's instance id (one per run, the same for every
//! member, never sent elsewhere).

use crate::payload::{BundleMember, MemberState};
use crate::slots::{SLOT_0, children_mask, write_children};
use crate::wire::{MessageError, Writer};

/// `Javelin::DungeonEntranceReplicatedState`.
pub const DUNGEON_ENTRANCE_TYPE_INDEX: u64 = 3105;
/// The children of its slot 0.
const CHILDREN: u8 = 1;
/// The `u8` of every element (Observed: 0 in all 1,469 retail bodies).
pub const SPAWNED_FLAG: u8 = 0;
/// The raid id of a key whose starter is in no raid (Observed: 0).
pub const NO_RAID: u64 = 0;
/// The group id of a key whose starter is in no group (Observed: 16 zero bytes).
pub const NO_GROUP: [u8; ID_LEN] = [0; ID_LEN];
/// The bytes of a group id and of a run id.
pub const ID_LEN: usize = 16;

/// One running run of an entrance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnedDungeon {
    /// The raid id of the starter's raid ([`NO_RAID`] for none).
    pub raid: u64,
    /// The id of the starter's group ([`NO_GROUP`] for none).
    pub group: [u8; ID_LEN],
    /// The `u8` after the key ([`SPAWNED_FLAG`]).
    pub flag: u8,
    /// The run's id.
    pub run: [u8; ID_LEN],
}

/// The entrance state: its running runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DungeonEntranceState {
    /// `dungeonsSpawned`.
    pub spawned: Vec<SpawnedDungeon>,
}

impl DungeonEntranceState {
    /// The state's body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the array has more elements than a count prefix holds.
    pub fn body(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer.u8(SLOT_0);
        write_children(
            &mut writer,
            children_mask(&[true]),
            CHILDREN,
            |writer, _| -> Result<(), MessageError> {
                writer.prefixed(self.spawned.len() as u64)?;
                for spawned in &self.spawned {
                    writer
                        .u64(spawned.raid)
                        .bytes(&spawned.group)
                        .u8(spawned.flag)
                        .bytes(&spawned.run);
                }
                Ok(())
            },
        )?;
        Ok(writer.finish())
    }

    /// The member of the state on component `component` (the entrance slice's number).
    ///
    /// # Errors
    ///
    /// As [`Self::body`].
    pub fn member(&self, component: u64) -> Result<BundleMember, MessageError> {
        Ok(BundleMember {
            component,
            state: MemberState::Recorded {
                class: DUNGEON_ENTRANCE_TYPE_INDEX,
                body: self.body()?,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> [u8; ID_LEN] {
        hex::decode(text)
            .expect("hex")
            .try_into()
            .expect("16 bytes")
    }

    #[test]
    fn an_entrance_with_no_run_is_the_retail_empty_array() {
        // Retail `12a_msq_24_defeatgawain` 81.178 s: the spawn of the entrance.
        assert_eq!(
            hex::encode(DungeonEntranceState::default().body().expect("body")),
            "010100"
        );
    }

    #[test]
    fn a_solo_run_a_group_run_and_a_raid_run_are_the_retail_bodies() {
        let solo = SpawnedDungeon {
            raid: NO_RAID,
            group: NO_GROUP,
            flag: SPAWNED_FLAG,
            run: id("24d0458d2867812abf03ebd452107fc3"),
        };
        // Retail `12a_msq_24_defeatgawain` 135.325 s.
        assert_eq!(
            hex::encode(
                DungeonEntranceState {
                    spawned: vec![solo]
                }
                .body()
                .expect("body")
            ),
            "0101010000000000000000000000000000000000000000000000000024d0458d2867812abf03ebd452107fc3"
        );
        let group = SpawnedDungeon {
            group: id("51a96db2d39147fb8160901f0e38c61f"),
            run: id("09f633e44d0a95687930054cef50624e"),
            ..solo
        };
        // Retail `dungeon_dynasty_tank_` 256.708 s.
        assert_eq!(
            hex::encode(
                DungeonEntranceState {
                    spawned: vec![group]
                }
                .body()
                .expect("body")
            ),
            "010101000000000000000051a96db2d39147fb8160901f0e38c61f0009f633e44d0a95687930054cef50624e"
        );
        let raid = SpawnedDungeon {
            raid: 0x1778,
            run: id("171ee526236efac9addba45037a5e664"),
            ..solo
        };
        assert_eq!(
            hex::encode(
                DungeonEntranceState {
                    spawned: vec![raid]
                }
                .body()
                .expect("body")
            ),
            "01010100000000000017780000000000000000000000000000000000171ee526236efac9addba45037a5e664"
        );
    }

    #[test]
    fn two_runs_follow_each_other_in_one_array() {
        let first = SpawnedDungeon {
            raid: NO_RAID,
            group: NO_GROUP,
            flag: SPAWNED_FLAG,
            run: id("435578693f638a7d65cfb3a9c2e81e0e"),
        };
        let second = SpawnedDungeon {
            run: id("edde319af91d7e1a5719b4f00d8b7066"),
            ..first
        };
        // Retail `MSQ10-lv40` 1016.134 s.
        let member = DungeonEntranceState {
            spawned: vec![first, second],
        }
        .member(3)
        .expect("member");
        assert_eq!(member.component, 3);
        let MemberState::Recorded { class, body } = &member.state else {
            panic!("a recorded state");
        };
        assert_eq!(*class, DUNGEON_ENTRANCE_TYPE_INDEX);
        assert_eq!(
            hex::encode(body),
            "01010200000000000000000000000000000000000000000000000000435578693f638a7d65cfb3a9c2e81e0e00000000000000000000000000000000000000000000000000edde319af91d7e1a5719b4f00d8b7066"
        );
    }
}
