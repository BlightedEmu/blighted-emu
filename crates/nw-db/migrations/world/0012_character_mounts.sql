-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The mounts of a character (mounts study, run 15): the mount it chose and each mount it dyed or
-- named (the client's MountComponentPersistentData: m_mountId and m_persistentMountDataMap). The
-- mounts it owns are not stored here: their achievements give them. A mount id is the CRC-32 of
-- the lowercase MountData MountId, a u32 the sim keeps, stored in BIGINT. It belongs to the
-- progression of migration 0006 and is saved with it (progression_saved_at). Only sim writes
-- this column and these rows.

ALTER TABLE player_character
    -- The chosen mount; NULL for none.
    ADD COLUMN mount_id BIGINT CHECK (mount_id IS NULL OR (mount_id >= 0
        AND mount_id <= 4294967295));

CREATE TABLE character_mount (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    mount_id BIGINT NOT NULL CHECK (mount_id >= 0 AND mount_id <= 4294967295),
    -- The color index of each mask channel, R, G, B, A (0 for none).
    dye BYTEA NOT NULL CHECK (octet_length(dye) = 4),
    -- The name bytes the client sent (empty for none).
    name BYTEA NOT NULL,
    -- The order the mounts were first dyed or named: the client's list keeps it.
    position SMALLINT NOT NULL CHECK (position >= 0),
    PRIMARY KEY (character_id, mount_id),
    UNIQUE (character_id, position)
);
