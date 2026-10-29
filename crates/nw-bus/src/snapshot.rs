// SPDX-License-Identifier: AGPL-3.0-or-later
//! The admin snapshot of a sim (admin tools spec section 5): its players and awake mobs, cut into
//! parts of at most [`SNAPSHOT_PART_ENTITIES`] so that each part fits one bus message.

use serde::{Deserialize, Serialize};

use crate::id::BusId;

/// Most players and mobs together in one part: a full part stays well below one bus message.
pub const SNAPSHOT_PART_ENTITIES: usize = 15_000;
/// Mob states, in `MobState` order: asleep, idle, chase, attack, return, dead.
pub const MOB_STATES: usize = 6;

/// One player of a snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapPlayer {
    /// The entity number.
    pub entity: u64,
    /// The character's name.
    pub name: String,
    /// East, in metres.
    pub x: f32,
    /// North, in metres.
    pub y: f32,
    /// Height, in metres.
    pub z: f32,
    /// The player is dead.
    pub dead: bool,
    /// The player is frozen for a handoff.
    pub frozen: bool,
    /// Current health.
    pub health: u32,
    /// Max health.
    pub max_health: u32,
    /// The entity number the player is attacking, when in a fight.
    pub target_entity: Option<u64>,
}

/// One awake mob of a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SnapMob {
    /// The entity number.
    pub entity: u64,
    /// East, in metres.
    pub x: f32,
    /// North, in metres.
    pub y: f32,
    /// Up, in metres.
    pub z: f32,
    /// The state, in `MobState` order.
    pub state: u8,
    /// Current health.
    pub health: u32,
    /// Max health.
    pub max_health: u32,
    /// The entity number the mob is attacking, when in a fight.
    pub target_entity: Option<u64>,
}

/// One part of a sim's snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimSnapshot {
    /// The sim.
    pub sim: BusId,
    /// Unix milliseconds of the snapshot; every part of one snapshot has the same.
    pub time_ms: u64,
    /// This part, from 0.
    pub part: u16,
    /// Parts of this snapshot.
    pub parts: u16,
    /// Players of this part.
    pub players: Vec<SnapPlayer>,
    /// Awake mobs of this part.
    pub awake_mobs: Vec<SnapMob>,
    /// Mobs by state, in part 0 only.
    pub mob_counts: Option<[u32; MOB_STATES]>,
}

/// The parts of one snapshot: players first, then mobs, at most [`SNAPSHOT_PART_ENTITIES`] per
/// part; `counts` in part 0; at least one part.
pub fn snapshot_parts(
    sim: BusId,
    time_ms: u64,
    players: Vec<SnapPlayer>,
    mobs: Vec<SnapMob>,
    counts: [u32; MOB_STATES],
) -> Vec<SimSnapshot> {
    let total = players.len() + mobs.len();
    let parts = total.div_ceil(SNAPSHOT_PART_ENTITIES).max(1);
    let part_count = u16::try_from(parts).unwrap_or(u16::MAX);
    let mut players = players.into_iter();
    let mut mobs = mobs.into_iter();
    (0..part_count)
        .map(|part| {
            let part_players: Vec<SnapPlayer> =
                players.by_ref().take(SNAPSHOT_PART_ENTITIES).collect();
            let room = SNAPSHOT_PART_ENTITIES - part_players.len();
            SimSnapshot {
                sim: sim.clone(),
                time_ms,
                part,
                parts: part_count,
                players: part_players,
                awake_mobs: mobs.by_ref().take(room).collect(),
                mob_counts: (part == 0).then_some(counts),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{MAX_BUS_MESSAGE_BYTES, encode};

    const TIME_MS: u64 = 1_000;
    const COUNTS: [u32; MOB_STATES] = [1, 2, 3, 4, 5, 6];

    fn sim() -> BusId {
        BusId::try_from("sim-a".to_owned()).expect("id")
    }

    /// The longest character name (`nw_login::names::CHARACTER_NAME_MAX_LEN` letters).
    const LONGEST_NAME: &str = "Abcdefghijklmnop";

    fn player(entity: u64) -> SnapPlayer {
        SnapPlayer {
            entity,
            name: LONGEST_NAME.to_owned(),
            x: f32::MAX,
            y: f32::MAX,
            z: f32::MAX,
            dead: true,
            frozen: true,
            health: u32::MAX,
            max_health: u32::MAX,
            target_entity: Some(u64::MAX),
        }
    }

    fn mob(entity: u64) -> SnapMob {
        SnapMob {
            entity: u64::MAX - entity,
            x: f32::MAX,
            y: f32::MAX,
            z: f32::MAX,
            state: u8::MAX,
            health: u32::MAX,
            max_health: u32::MAX,
            target_entity: Some(u64::MAX),
        }
    }

    #[test]
    fn snapshot_parts_cut_at_the_limit() {
        let total_players = SNAPSHOT_PART_ENTITIES + SNAPSHOT_PART_ENTITIES / 2;
        let total_mobs = SNAPSHOT_PART_ENTITIES + SNAPSHOT_PART_ENTITIES / 2;
        let players = (0..total_players as u64).map(player).collect();
        let mobs = (0..total_mobs as u64).map(mob).collect();
        let parts = snapshot_parts(sim(), TIME_MS, players, mobs, COUNTS);
        assert_eq!(parts.len(), 3);
        assert!(
            parts
                .iter()
                .all(|part| part.parts == 3 && part.time_ms == TIME_MS)
        );
        assert_eq!(
            parts.iter().map(|part| part.part).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(parts[0].players.len(), SNAPSHOT_PART_ENTITIES);
        assert_eq!(
            (parts[1].players.len(), parts[1].awake_mobs.len()),
            (
                total_players - SNAPSHOT_PART_ENTITIES,
                SNAPSHOT_PART_ENTITIES - (total_players - SNAPSHOT_PART_ENTITIES)
            )
        );
        assert_eq!(
            parts[2].awake_mobs.len(),
            total_mobs - (SNAPSHOT_PART_ENTITIES - (total_players - SNAPSHOT_PART_ENTITIES))
        );
        assert_eq!(parts[0].mob_counts, Some(COUNTS));
        assert!(parts[1..].iter().all(|part| part.mob_counts.is_none()));
    }

    #[test]
    fn an_empty_snapshot_is_one_part() {
        let parts = snapshot_parts(sim(), TIME_MS, Vec::new(), Vec::new(), COUNTS);
        assert_eq!(parts.len(), 1);
        assert_eq!((parts[0].part, parts[0].parts), (0, 1));
        assert_eq!(parts[0].mob_counts, Some(COUNTS));
    }

    #[test]
    fn a_part_fits_one_bus_message() {
        let all_players = (0..SNAPSHOT_PART_ENTITIES as u64).map(player).collect();
        let player_part = snapshot_parts(sim(), u64::MAX, all_players, Vec::new(), COUNTS);
        assert!(
            encode(&player_part[0]).expect("encode").len() < MAX_BUS_MESSAGE_BYTES,
            "a part of players with the longest names"
        );
        let players = (0..SNAPSHOT_PART_ENTITIES as u64 / 2).map(player).collect();
        let mobs = (0..SNAPSHOT_PART_ENTITIES as u64 / 2).map(mob).collect();
        let parts = snapshot_parts(sim(), u64::MAX, players, mobs, COUNTS);
        assert_eq!(parts.len(), 1);
        assert!(encode(&parts[0]).expect("encode").len() < MAX_BUS_MESSAGE_BYTES);
        let all_mobs = (0..SNAPSHOT_PART_ENTITIES as u64).map(mob).collect();
        let mob_part = snapshot_parts(sim(), u64::MAX, Vec::new(), all_mobs, COUNTS);
        assert!(encode(&mob_part[0]).expect("encode").len() < MAX_BUS_MESSAGE_BYTES);
    }
}
