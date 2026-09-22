-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The points a character spent: attribute points on its attributes and weapon points on the
-- progression points of its weapon trees (progression note, section 6). The unspent points
-- are not stored: the level and the weapon ranks give them. They belong to the progression of
-- migration 0006 and are saved with it (progression_saved_at). Only sim writes these rows.

CREATE TABLE character_attribute (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- The client enum CharacterAttributeType: 0 Constitution to 4 Intelligence.
    attribute SMALLINT NOT NULL CHECK (attribute >= 0 AND attribute <= 4),
    -- The sim keeps it as u32.
    points BIGINT NOT NULL CHECK (points >= 0 AND points <= 4294967295),
    -- The order the attributes were first spent on: the client lists them so.
    position SMALLINT NOT NULL CHECK (position >= 0),
    PRIMARY KEY (character_id, attribute),
    UNIQUE (character_id, position)
);

CREATE TABLE character_weapon_point (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- The ProgressionPointId, for example Ability_Greatsword_Combo.
    point_id TEXT NOT NULL CHECK (point_id <> ''),
    -- The sim keeps it as u16.
    level INTEGER NOT NULL CHECK (level >= 0 AND level <= 65535),
    -- The order the points were first assigned: the client's point list keeps it.
    position INTEGER NOT NULL CHECK (position >= 0),
    PRIMARY KEY (character_id, point_id),
    UNIQUE (character_id, position)
);
