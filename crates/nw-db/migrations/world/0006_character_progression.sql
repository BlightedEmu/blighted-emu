-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The progression of a character besides its items and quests: experience, coins, achievement
-- bits, and the counts of its loot limits. progression_saved_at is NULL until the first save.
-- Only sim writes these columns and rows.

ALTER TABLE player_character
    ADD COLUMN progression_saved_at TIMESTAMPTZ,
    -- The sim keeps experience as u32 and coins as u64; coins above the BIGINT range are refused.
    ADD COLUMN experience BIGINT NOT NULL DEFAULT 0
        CHECK (experience >= 0 AND experience <= 4294967295),
    ADD COLUMN coins BIGINT NOT NULL DEFAULT 0 CHECK (coins >= 0),
    -- The set achievement bits, ascending.
    ADD COLUMN achievements INTEGER[] NOT NULL DEFAULT '{}'
        CHECK (0 <= ALL (achievements) AND array_position(achievements, NULL) IS NULL);

CREATE TABLE character_loot_limit (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- The lowercase LootLimitID.
    limit_id TEXT NOT NULL CHECK (limit_id = lower(limit_id) AND limit_id <> ''),
    count BIGINT NOT NULL CHECK (count >= 0),
    -- Unix nanoseconds of the last count and of the count's end.
    counted_ns BIGINT NOT NULL CHECK (counted_ns >= 0),
    ends_ns BIGINT NOT NULL CHECK (ends_ns >= 0),
    PRIMARY KEY (character_id, limit_id)
);
