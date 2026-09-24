-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The travel and housing state of a character: its map marker, the inn and the respawn camp it
-- is bound to, its camp skin, its unstuck cooldown, the homes it owns and their furniture. It
-- belongs to the progression of migration 0006 and is saved with it (progression_saved_at).
-- Positions are the bit patterns of the f32 metres the sim keeps (f32::to_bits, stored in
-- INTEGER as the same 32 bits), so a save gives back every bit. A u64 entity id is stored in
-- BIGINT as the same 64 bits. Times are Unix milliseconds. Only sim writes these columns and
-- rows.

ALTER TABLE player_character
    -- The map marker (2728), all three or none.
    ADD COLUMN waypoint_bits INTEGER[]
        CHECK (waypoint_bits IS NULL OR (cardinality(waypoint_bits) = 3
            AND array_position(waypoint_bits, NULL) IS NULL)),
    -- The facet entity of the inn's spawn point (interaction 18), a u64 as its 64 bits.
    ADD COLUMN inn_spawn_point BIGINT,
    -- The facet entity of the respawn camp (interaction 73), a u64 as its 64 bits.
    ADD COLUMN respawn_camp BIGINT,
    -- The camp skin (742), a CRC-32 the sim keeps as u32.
    ADD COLUMN camp_skin BIGINT CHECK (camp_skin IS NULL OR (camp_skin >= 0
        AND camp_skin <= 4294967295)),
    ADD COLUMN unstuck_ready_ms BIGINT NOT NULL DEFAULT 0 CHECK (unstuck_ready_ms >= 0);

CREATE TABLE character_home (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- The 16-byte raw id of the house plot entity.
    plot BYTEA NOT NULL CHECK (octet_length(plot) = 16),
    position_bits INTEGER[] NOT NULL
        CHECK (cardinality(position_bits) = 3 AND array_position(position_bits, NULL) IS NULL),
    purchased_ms BIGINT NOT NULL CHECK (purchased_ms >= 0),
    -- The order the homes were bought.
    position SMALLINT NOT NULL CHECK (position >= 0),
    PRIMARY KEY (character_id, plot),
    UNIQUE (character_id, position)
);

CREATE TABLE character_home_item (
    character_id UUID NOT NULL,
    plot BYTEA NOT NULL,
    -- The item index in the home; the sim keeps it as u16.
    item_index INTEGER NOT NULL CHECK (item_index >= 0 AND item_index <= 65535),
    -- The placed item as the client names it; the sim keeps it as u32.
    item BIGINT NOT NULL CHECK (item >= 0 AND item <= 4294967295),
    position_bits INTEGER[] NOT NULL
        CHECK (cardinality(position_bits) = 3 AND array_position(position_bits, NULL) IS NULL),
    rotation_bits INTEGER[] NOT NULL
        CHECK (cardinality(rotation_bits) = 4 AND array_position(rotation_bits, NULL) IS NULL),
    PRIMARY KEY (character_id, plot, item_index),
    FOREIGN KEY (character_id, plot) REFERENCES character_home (character_id, plot)
        ON DELETE CASCADE
);
