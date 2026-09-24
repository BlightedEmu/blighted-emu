-- SPDX-License-Identifier: AGPL-3.0-or-later
-- What an item keeps beyond its id and quantity (run-15 items study), the storage chests and
-- the loadouts of a character. Saved with the item list (character_item), in the same
-- transaction and under the same items_saved_at. Only sim writes these rows.

-- The instance id the client names the stack by; NULL: the id derived from the character and
-- the item id (rows saved before this migration).
ALTER TABLE character_item ADD COLUMN instance_id UUID;
-- Locked by its owner (2549).
ALTER TABLE character_item ADD COLUMN locked BOOLEAN NOT NULL DEFAULT false;

-- The items of the character's storage chests (interaction 26), per storage: the storage's
-- entity id as the client names it.
CREATE TABLE character_storage_item (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    storage_id UUID NOT NULL,
    slot INTEGER NOT NULL CHECK (slot >= 0),
    item_id TEXT NOT NULL,
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    instance_id UUID,
    locked BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (character_id, storage_id, slot)
);

-- The character's saved loadouts (gear sets, 5542 and 5579): loadout 0 is the worn items and is
-- not stored.
CREATE TABLE character_loadout (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    loadout_index SMALLINT NOT NULL CHECK (loadout_index > 0),
    name TEXT NOT NULL,
    -- The attribute points saved with the loadout: [[attribute, points], ...].
    attributes JSONB NOT NULL DEFAULT '[]'::jsonb,
    PRIMARY KEY (character_id, loadout_index)
);

-- The items of a saved loadout: each paperdoll slot and the instance id of its item.
CREATE TABLE character_loadout_item (
    character_id UUID NOT NULL,
    loadout_index SMALLINT NOT NULL,
    paperdoll_slot INTEGER NOT NULL CHECK (paperdoll_slot >= 0),
    instance_id UUID NOT NULL,
    PRIMARY KEY (character_id, loadout_index, paperdoll_slot),
    FOREIGN KEY (character_id, loadout_index)
        REFERENCES character_loadout (character_id, loadout_index) ON DELETE CASCADE
);
