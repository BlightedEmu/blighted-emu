// SPDX-License-Identifier: AGPL-3.0-or-later
//! The arena state of a character (world migration 0015): the expedition, raid and trial
//! counters, cooldowns and refresh times of `PlayerArenaReplicatedState`, and the instance it is
//! in with the spot it goes back to. It is part of the progression ([`crate::SavedProgression`]):
//! it loads and saves with it, in the same transaction. A character without a row has the
//! default state.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::DbError;
use crate::progression::{from_column, to_column};
use crate::travel::{bits_array, bits_list, id_column, id_value};

/// Bytes of a dungeon id (`m_lastDungeonsEntered`).
pub const DUNGEON_ID_LEN: usize = 16;
/// Coordinates of a return spot.
const POSITION_LEN: usize = 3;

/// A character's stay in an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedStay {
    /// The instance.
    pub instance: Uuid,
    /// The game mode (`GameModeID`).
    pub game_mode: String,
    /// The phase id of the instance.
    pub phase: u64,
    /// The level it came from.
    pub return_level: String,
    /// Where it was when it entered, as `f32` bit patterns.
    pub return_position: [u32; POSITION_LEN],
    /// Unix nanoseconds of the activation.
    pub activated_ns: u64,
}

/// A character's arena state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedArena {
    /// `dungeonCooldownTime`, nanoseconds since 2000-01-01.
    pub dungeon_cooldown_since_2000_ns: u64,
    /// `enterSoloTrialCooldownTime`, nanoseconds since 2000-01-01.
    pub solo_trial_cooldown_since_2000_ns: u64,
    /// `dungeonRanks`.
    pub ranks: Vec<u8>,
    /// `m_lastDungeonsEntered`.
    pub last_dungeon: Option<[u8; DUNGEON_ID_LEN]>,
    /// `m_lastMutatedDungeonEntered`.
    pub last_mutated_dungeon: Option<[u8; DUNGEON_ID_LEN]>,
    /// Base dungeon entries since the last refresh.
    pub base_entered: u32,
    /// Mutated dungeon entries since the last refresh.
    pub mutated_entered: u32,
    /// Group trial entries since the last refresh.
    pub group_trials_entered: u32,
    /// The next base refresh, Unix nanoseconds.
    pub next_base_refresh_ns: u64,
    /// The next mutated refresh, Unix nanoseconds.
    pub next_mutated_refresh_ns: u64,
    /// The next group trial refresh, Unix nanoseconds.
    pub next_group_trial_refresh_ns: u64,
    /// `hasMutationUnlockAwardBeenGranted`.
    pub mutation_award: bool,
    /// `m_lastDungeonEnteredWasMutated` (world migration 0019).
    pub last_dungeon_mutated: bool,
    /// The instance stay.
    pub stay: Option<SavedStay>,
}

/// The columns of a `character_arena` row.
#[derive(Debug, Clone, sqlx::FromRow)]
struct ArenaColumns {
    dungeon_cooldown_since_2000_ns: i64,
    solo_trial_cooldown_since_2000_ns: i64,
    ranks: Vec<i16>,
    last_dungeon: Option<Vec<u8>>,
    last_mutated_dungeon: Option<Vec<u8>>,
    base_entered: i64,
    mutated_entered: i64,
    group_trials_entered: i64,
    next_base_refresh_ns: i64,
    next_mutated_refresh_ns: i64,
    next_group_trial_refresh_ns: i64,
    mutation_award: bool,
    last_dungeon_mutated: bool,
    stay_instance: Option<Uuid>,
    stay_game_mode: Option<String>,
    stay_phase: Option<i64>,
    stay_return_level: Option<String>,
    stay_return_bits: Option<Vec<i32>>,
    stay_activated_ns: Option<i64>,
}

fn dungeon_id(
    column: Option<Vec<u8>>,
    field: &'static str,
) -> Result<Option<[u8; DUNGEON_ID_LEN]>, DbError> {
    column
        .map(|bytes| bytes.try_into().map_err(|_| DbError::OutOfRange { field }))
        .transpose()
}

/// The arena state of a character; the default one when it has no row.
pub(crate) async fn character_arena(
    world: &PgPool,
    character_id: Uuid,
) -> Result<SavedArena, DbError> {
    let row: Option<ArenaColumns> = sqlx::query_as(
        "SELECT dungeon_cooldown_since_2000_ns, solo_trial_cooldown_since_2000_ns, ranks, \
         last_dungeon, last_mutated_dungeon, base_entered, mutated_entered, \
         group_trials_entered, next_base_refresh_ns, next_mutated_refresh_ns, \
         next_group_trial_refresh_ns, mutation_award, last_dungeon_mutated, stay_instance, \
         stay_game_mode, stay_phase, stay_return_level, stay_return_bits, stay_activated_ns \
         FROM character_arena WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_optional(world)
    .await?;
    row.map_or_else(|| Ok(SavedArena::default()), arena_of)
}

fn arena_of(row: ArenaColumns) -> Result<SavedArena, DbError> {
    let stay = match (
        row.stay_instance,
        row.stay_game_mode,
        row.stay_phase,
        row.stay_return_level,
        row.stay_return_bits,
        row.stay_activated_ns,
    ) {
        (
            Some(instance),
            Some(game_mode),
            Some(phase),
            Some(return_level),
            Some(bits),
            Some(activated),
        ) => Some(SavedStay {
            instance,
            game_mode,
            phase: id_value(phase),
            return_level,
            return_position: bits_array(&bits, "stay_return_bits")?,
            activated_ns: id_value(activated),
        }),
        (None, None, None, None, None, None) => None,
        _ => return Err(DbError::OutOfRange { field: "stay" }),
    };
    Ok(SavedArena {
        dungeon_cooldown_since_2000_ns: id_value(row.dungeon_cooldown_since_2000_ns),
        solo_trial_cooldown_since_2000_ns: id_value(row.solo_trial_cooldown_since_2000_ns),
        ranks: row
            .ranks
            .into_iter()
            .map(|rank| from_column(i64::from(rank), "rank"))
            .collect::<Result<_, _>>()?,
        last_dungeon: dungeon_id(row.last_dungeon, "last_dungeon")?,
        last_mutated_dungeon: dungeon_id(row.last_mutated_dungeon, "last_mutated_dungeon")?,
        base_entered: from_column(row.base_entered, "base_entered")?,
        mutated_entered: from_column(row.mutated_entered, "mutated_entered")?,
        group_trials_entered: from_column(row.group_trials_entered, "group_trials_entered")?,
        next_base_refresh_ns: id_value(row.next_base_refresh_ns),
        next_mutated_refresh_ns: id_value(row.next_mutated_refresh_ns),
        next_group_trial_refresh_ns: id_value(row.next_group_trial_refresh_ns),
        mutation_award: row.mutation_award,
        last_dungeon_mutated: row.last_dungeon_mutated,
        stay,
    })
}

/// Saves `arena` as the whole arena state of a character whose progression row the transaction
/// already updated.
pub(crate) async fn save_character_arena(
    transaction: &mut Transaction<'_, Postgres>,
    character_id: Uuid,
    arena: &SavedArena,
) -> Result<(), DbError> {
    let stay = arena.stay.as_ref();
    sqlx::query(
        "INSERT INTO character_arena (character_id, dungeon_cooldown_since_2000_ns, \
         solo_trial_cooldown_since_2000_ns, ranks, last_dungeon, last_mutated_dungeon, \
         base_entered, mutated_entered, group_trials_entered, next_base_refresh_ns, \
         next_mutated_refresh_ns, next_group_trial_refresh_ns, mutation_award, stay_instance, \
         stay_game_mode, stay_phase, stay_return_level, stay_return_bits, stay_activated_ns, \
         last_dungeon_mutated) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, \
         $19, $20) \
         ON CONFLICT (character_id) DO UPDATE SET \
         dungeon_cooldown_since_2000_ns = $2, solo_trial_cooldown_since_2000_ns = $3, \
         ranks = $4, last_dungeon = $5, last_mutated_dungeon = $6, base_entered = $7, \
         mutated_entered = $8, group_trials_entered = $9, next_base_refresh_ns = $10, \
         next_mutated_refresh_ns = $11, next_group_trial_refresh_ns = $12, \
         mutation_award = $13, stay_instance = $14, stay_game_mode = $15, stay_phase = $16, \
         stay_return_level = $17, stay_return_bits = $18, stay_activated_ns = $19, \
         last_dungeon_mutated = $20",
    )
    .bind(character_id)
    .bind(id_column(arena.dungeon_cooldown_since_2000_ns))
    .bind(id_column(arena.solo_trial_cooldown_since_2000_ns))
    .bind(arena.ranks.iter().map(|rank| i16::from(*rank)).collect::<Vec<_>>())
    .bind(arena.last_dungeon.map(|id| id.to_vec()))
    .bind(arena.last_mutated_dungeon.map(|id| id.to_vec()))
    .bind(to_column(arena.base_entered, "base_entered")?)
    .bind(to_column(arena.mutated_entered, "mutated_entered")?)
    .bind(to_column(arena.group_trials_entered, "group_trials_entered")?)
    .bind(id_column(arena.next_base_refresh_ns))
    .bind(id_column(arena.next_mutated_refresh_ns))
    .bind(id_column(arena.next_group_trial_refresh_ns))
    .bind(arena.mutation_award)
    .bind(stay.map(|stay| stay.instance))
    .bind(stay.map(|stay| stay.game_mode.clone()))
    .bind(stay.map(|stay| id_column(stay.phase)))
    .bind(stay.map(|stay| stay.return_level.clone()))
    .bind(stay.map(|stay| bits_list(&stay.return_position)))
    .bind(stay.map(|stay| id_column(stay.activated_ns)))
    .bind(arena.last_dungeon_mutated)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row of a character that entered the Amrine expedition: no stay.
    fn row() -> ArenaColumns {
        ArenaColumns {
            dungeon_cooldown_since_2000_ns: id_column(u64::MAX),
            solo_trial_cooldown_since_2000_ns: 1,
            ranks: vec![3, 3, 2],
            last_dungeon: Some(vec![0x2d; DUNGEON_ID_LEN]),
            last_mutated_dungeon: None,
            base_entered: 1,
            mutated_entered: 0,
            group_trials_entered: u32::MAX.into(),
            next_base_refresh_ns: id_column(1_792_746_000_226_611_400),
            next_mutated_refresh_ns: id_column(1_793_091_600_226_670_700),
            next_group_trial_refresh_ns: id_column(1_792_746_000_226_725_900),
            mutation_award: true,
            last_dungeon_mutated: true,
            stay_instance: None,
            stay_game_mode: None,
            stay_phase: None,
            stay_return_level: None,
            stay_return_bits: None,
            stay_activated_ns: None,
        }
    }

    #[test]
    fn a_row_reads_every_value_and_its_stay() {
        let bits = [7_700.5_f32.to_bits(), 7_300.25_f32.to_bits(), 120.0_f32.to_bits()];
        let instance = Uuid::from_u128(9);
        let arena = arena_of(ArenaColumns {
            stay_instance: Some(instance),
            stay_game_mode: Some("DungeonAmrine".to_owned()),
            stay_phase: Some(id_column(u64::MAX)),
            stay_return_level: Some("newworld_vitaeeterna".to_owned()),
            stay_return_bits: Some(bits_list(&bits)),
            stay_activated_ns: Some(id_column(u64::MAX)),
            ..row()
        })
        .expect("arena");
        assert_eq!(arena.dungeon_cooldown_since_2000_ns, u64::MAX);
        assert_eq!(arena.ranks, [3, 3, 2]);
        assert_eq!(arena.last_dungeon, Some([0x2d; DUNGEON_ID_LEN]));
        assert_eq!(arena.group_trials_entered, u32::MAX);
        assert_eq!(arena.next_base_refresh_ns, 1_792_746_000_226_611_400);
        assert!(arena.last_dungeon_mutated);
        assert_eq!(
            arena.stay,
            Some(SavedStay {
                instance,
                game_mode: "DungeonAmrine".to_owned(),
                phase: u64::MAX,
                return_level: "newworld_vitaeeterna".to_owned(),
                return_position: bits,
                activated_ns: u64::MAX,
            })
        );
        assert_eq!(arena_of(row()).expect("arena").stay, None);
    }

    #[test]
    fn a_half_stay_or_a_short_id_is_refused() {
        assert!(matches!(
            arena_of(ArenaColumns {
                stay_instance: Some(Uuid::nil()),
                ..row()
            }),
            Err(DbError::OutOfRange { field: "stay" })
        ));
        assert!(matches!(
            arena_of(ArenaColumns {
                last_dungeon: Some(vec![1, 2]),
                ..row()
            }),
            Err(DbError::OutOfRange {
                field: "last_dungeon"
            })
        ));
    }
}
