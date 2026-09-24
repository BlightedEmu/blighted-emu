-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The territory standing points a character spent (a 4040 spend of the TerritoryInteractor
-- flow): the territory pool and the progression point, both CRC-32 ids of the client's
-- 3681 point list, with the points in it (at most 14). The unspent points of a territory
-- pool are not stored: the content holds no territory pools. They belong to the
-- progression of migration 0006 and are saved with it (progression_saved_at). Only sim
-- writes these rows.

CREATE TABLE character_territory_point (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- The territory pool (CRC-32 of its lowercase id).
    pool_id BIGINT NOT NULL CHECK (pool_id >= 0 AND pool_id <= 4294967295),
    -- The progression point (CRC-32 of the lowercase ProgressionPointId).
    point_id BIGINT NOT NULL CHECK (point_id >= 0 AND point_id <= 4294967295),
    -- The sim keeps it as u16.
    level INTEGER NOT NULL CHECK (level >= 0 AND level <= 65535),
    -- The order the points were first spent on: the client's point list keeps it.
    position INTEGER NOT NULL CHECK (position >= 0),
    PRIMARY KEY (character_id, pool_id, point_id),
    UNIQUE (character_id, position)
);
