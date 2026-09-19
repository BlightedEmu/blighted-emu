-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The quests of a character (admin studio spec section 6.3): at most one active run and the
-- finished objectives. quests_saved_at is NULL until the first save. Only sim writes these rows.

ALTER TABLE player_character ADD COLUMN quests_saved_at TIMESTAMPTZ;

CREATE TABLE character_quest (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    objective_id TEXT NOT NULL,
    -- 'active' with its run, or 'done' with none.
    state TEXT NOT NULL CHECK (state IN ('active', 'done')),
    run JSONB,
    CHECK ((state = 'active') = (run IS NOT NULL)),
    PRIMARY KEY (character_id, objective_id)
);

-- One active quest per character.
CREATE UNIQUE INDEX character_quest_one_active ON character_quest (character_id)
    WHERE state = 'active';
