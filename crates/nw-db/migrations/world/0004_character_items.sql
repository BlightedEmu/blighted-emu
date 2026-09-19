-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The saved items of a character (admin studio spec section 6.1). items_saved_at is NULL until
-- the first save: such a character gets its starter kit. Only sim writes these rows.

ALTER TABLE player_character ADD COLUMN items_saved_at TIMESTAMPTZ;

CREATE TABLE character_item (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- 0: worn in the paperdoll slot `slot`; 1: in the bag slot `slot`.
    place SMALLINT NOT NULL CHECK (place IN (0, 1)),
    slot INTEGER NOT NULL CHECK (slot >= 0),
    item_id TEXT NOT NULL,
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    PRIMARY KEY (character_id, place, slot)
);
