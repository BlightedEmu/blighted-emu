-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The rested experience of a character (findings 2026-11-13-rested-xp): the
-- ProgressionComponentReplicatedState restedExp (899 slot 1 child 1) and the Unix ns of the save
-- it was taken at, from which the next join counts the time away. They belong to the
-- progression of migration 0006 and are saved with it. Only sim writes these columns.

ALTER TABLE player_character
    ADD COLUMN rested_experience BIGINT NOT NULL DEFAULT 0
        CHECK (rested_experience >= 0 AND rested_experience <= 4294967295),
    ADD COLUMN rested_saved_ns BIGINT NOT NULL DEFAULT 0 CHECK (rested_saved_ns >= 0);

-- A character saved before this migration was away since its last progression save.
UPDATE player_character
    SET rested_saved_ns = (EXTRACT(EPOCH FROM progression_saved_at) * 1000000000)::BIGINT
    WHERE progression_saved_at IS NOT NULL;
