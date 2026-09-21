// SPDX-License-Identifier: AGPL-3.0-or-later
//! The raw id registry holds only the entities some holder has now: after spawn and despawn
//! cycles it is back at its size. Its own test binary, so no other test changes the count.

use nw_messages::{
    IdHolder, hold_raw_id, number_of_seen_raw_id, raw_id_count, raw_id_of_entity, release_raw_id,
    renew_raw_id,
};

/// Spawn and despawn rounds.
const ROUNDS: u64 = 50;
/// Entities spawned per round, as a busy sim spawns and drops mobs and players.
const PER_ROUND: u64 = 2_000;

#[test]
fn spawn_and_despawn_cycles_leave_the_table_at_its_size() {
    let sim = IdHolder::new();
    let neighbour = IdHolder::new();
    let client = IdHolder::new();
    let at_start = raw_id_count();
    for round in 0..ROUNDS {
        let numbers: Vec<u64> = (0..PER_ROUND).map(|i| round * PER_ROUND + i).collect();
        for &number in &numbers {
            let id = hold_raw_id(sim, number);
            // Some are ghosts in a neighbour, seen by a client, or respawn.
            if number % 3 == 0 {
                assert_eq!(hold_raw_id(neighbour, number), id);
            }
            if number % 5 == 0 {
                assert_eq!(number_of_seen_raw_id(client, &id), number);
            }
            if number % 7 == 0 {
                renew_raw_id(number);
            }
        }
        assert_eq!(raw_id_count(), at_start + numbers.len());
        for &number in &numbers {
            release_raw_id(sim, number);
            release_raw_id(neighbour, number);
            release_raw_id(client, number);
        }
        assert_eq!(raw_id_count(), at_start, "round {round}");
    }
    // An id made without a holder goes with the first release of its number.
    let loose = ROUNDS * PER_ROUND;
    raw_id_of_entity(loose);
    release_raw_id(sim, loose);
    assert_eq!(raw_id_count(), at_start);
}
