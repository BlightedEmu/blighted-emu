-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The weapon masteries of a character: the total experience of each categorical progression on
-- the WeaponMastery rank table. They belong to the progression of migration 0006 and are saved
-- with it (progression_saved_at). Only sim writes these rows.

CREATE TABLE character_mastery (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- The CategoricalProgressionID, for example GreatSwordAbilityTable.
    category TEXT NOT NULL CHECK (category <> ''),
    -- The sim keeps it as u32.
    experience BIGINT NOT NULL CHECK (experience >= 0 AND experience <= 4294967295),
    PRIMARY KEY (character_id, category)
);
