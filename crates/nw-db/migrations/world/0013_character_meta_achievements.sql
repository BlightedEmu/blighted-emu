-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The mapped ability slots of a character's weapon trees, its meta achievements and the title
-- it shows (progression note, sections 8 to 10). They belong to the progression of migration 0006 and
-- are saved with it (progression_saved_at). Only sim writes these rows.

-- The CRC-32 of the lowercase TitleID of the title the character shows; NULL for none.
ALTER TABLE player_character
    ADD COLUMN shown_title BIGINT CHECK (shown_title >= 0 AND shown_title <= 4294967295);

CREATE TABLE character_ability_slot (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- The weapon mastery of the tree (CategoricalProgressionID), for example
    -- GreatSwordAbilityTable.
    mastery TEXT NOT NULL CHECK (mastery <> ''),
    -- The mapped slot of the tree: 0 to 2.
    slot SMALLINT NOT NULL CHECK (slot >= 0 AND slot <= 2),
    -- The ProgressionPointId of the ability in it.
    ability TEXT NOT NULL CHECK (ability <> ''),
    PRIMARY KEY (character_id, mastery, slot)
);

CREATE TABLE character_meta_counter (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- The lowercase counter key: a ProgressId of MetaAchievementData, else the id.
    counter TEXT NOT NULL CHECK (counter = lower(counter) AND counter <> ''),
    -- The sim keeps it as u64; values above the BIGINT range are refused.
    value BIGINT NOT NULL CHECK (value >= 0),
    PRIMARY KEY (character_id, counter)
);

CREATE TABLE character_meta_achievement (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- The MetaAchievementId.
    meta_achievement TEXT NOT NULL CHECK (meta_achievement <> ''),
    -- The order of completion: the achievements window lists the latest two.
    position INTEGER NOT NULL CHECK (position >= 0),
    -- Its reward (a title) still waits for the player to see it (3802, cleared by 3227).
    pending BOOLEAN NOT NULL,
    PRIMARY KEY (character_id, meta_achievement),
    UNIQUE (character_id, position)
);
