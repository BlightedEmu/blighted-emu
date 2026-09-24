-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The arena state of a character (expeditions, raids and trials; run 15 instances study): the
-- children of PlayerArenaReplicatedState (type 2406) that persist, and the instance it is in with
-- the spot it goes back to. It belongs to the progression of migration 0006 and is saved with it
-- (progression_saved_at). A u64 time is stored in BIGINT as the same 64 bits; times named
-- *_since_2000_ns count nanoseconds from 2000-01-01, the others are Unix nanoseconds. The return
-- position holds the bit patterns of the f32 metres the sim keeps (f32::to_bits, stored in
-- INTEGER as the same 32 bits). Only sim writes this table.

CREATE TABLE character_arena (
    character_id UUID PRIMARY KEY REFERENCES player_character (character_id) ON DELETE CASCADE,
    dungeon_cooldown_since_2000_ns BIGINT NOT NULL,
    solo_trial_cooldown_since_2000_ns BIGINT NOT NULL,
    -- dungeonRanks: one byte per element.
    ranks SMALLINT[] NOT NULL CHECK (array_position(ranks, NULL) IS NULL),
    -- m_lastDungeonsEntered and m_lastMutatedDungeonEntered: 16 bytes each.
    last_dungeon BYTEA CHECK (last_dungeon IS NULL OR octet_length(last_dungeon) = 16),
    last_mutated_dungeon BYTEA
        CHECK (last_mutated_dungeon IS NULL OR octet_length(last_mutated_dungeon) = 16),
    base_entered BIGINT NOT NULL CHECK (base_entered >= 0 AND base_entered <= 4294967295),
    mutated_entered BIGINT NOT NULL CHECK (mutated_entered >= 0 AND mutated_entered <= 4294967295),
    group_trials_entered BIGINT NOT NULL
        CHECK (group_trials_entered >= 0 AND group_trials_entered <= 4294967295),
    next_base_refresh_ns BIGINT NOT NULL,
    next_mutated_refresh_ns BIGINT NOT NULL,
    next_group_trial_refresh_ns BIGINT NOT NULL,
    mutation_award BOOLEAN NOT NULL,
    -- The instance stay: all of these or none.
    stay_instance UUID,
    stay_game_mode TEXT,
    stay_phase BIGINT,
    stay_return_level TEXT,
    stay_return_bits INTEGER[]
        CHECK (stay_return_bits IS NULL OR (cardinality(stay_return_bits) = 3
            AND array_position(stay_return_bits, NULL) IS NULL)),
    -- m_singlePlayerDungeonTime of the stay's activation, Unix nanoseconds.
    stay_activated_ns BIGINT,
    CHECK ((stay_instance IS NULL) = (stay_activated_ns IS NULL)
        AND (stay_instance IS NULL) = (stay_game_mode IS NULL)
        AND (stay_instance IS NULL) = (stay_phase IS NULL)
        AND (stay_instance IS NULL) = (stay_return_level IS NULL)
        AND (stay_instance IS NULL) = (stay_return_bits IS NULL))
);
