-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The pronoun type a character chose (client call 4751, `ePronounType`: 0, 1 or 2). The sim
-- saves it with the shown title (migration 0013) and sends it in the character's social state;
-- the social service keeps its own copy with the icon and the level
-- (character_social) to answer player data calls for a character that is offline.

ALTER TABLE player_character
    ADD COLUMN pronoun_type SMALLINT NOT NULL DEFAULT 0 CHECK (pronoun_type BETWEEN 0 AND 2);

ALTER TABLE character_social
    ADD COLUMN pronoun_type SMALLINT NOT NULL DEFAULT 0 CHECK (pronoun_type BETWEEN 0 AND 2);
