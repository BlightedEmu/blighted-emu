-- SPDX-License-Identifier: AGPL-3.0-or-later
-- What an item keeps of its own (run-15 items2 study): its rolled gear score and perks, its
-- durability, its bind state, its dye and the skin applied over it, for the carried items and
-- the storage chests; and the item skins (appearances) an account collected for transmog.
-- Only sim writes these rows.
--
-- A NULL column keeps the rule of rows saved before this migration: the item's data value
-- (gear score, full durability, fixed perks, BindOnPickup).

-- The item record's columns, the same for both item tables.
ALTER TABLE character_item
    -- Its gear score; NULL: the item's GearScoreOverride or MinGearScore.
    ADD COLUMN gear_score INTEGER CHECK (gear_score >= 0),
    -- Its current durability; NULL: full (MasterItemDefinitions Durability).
    ADD COLUMN durability INTEGER CHECK (durability >= 0),
    -- Its perk ids (PerkData PerkID) by perk slot, no gaps; NULL: the item's fixed perks.
    ADD COLUMN perks TEXT[],
    -- Bound to the character; NULL: the item's BindOnPickup.
    ADD COLUMN bound BOOLEAN,
    -- The four DyeColorData indexes of its dye channels (R, G, B, A), 0 for none.
    ADD COLUMN dye SMALLINT[] NOT NULL DEFAULT '{0,0,0,0}',
    -- The skin applied over it: the CRC-32 of the lowercase appearance id, 0 for none.
    ADD COLUMN appearance BIGINT NOT NULL DEFAULT 0
        CHECK (appearance >= 0 AND appearance <= 4294967295),
    -- The four dye indexes of the applied skin.
    ADD COLUMN skin_dye SMALLINT[] NOT NULL DEFAULT '{0,0,0,0}';

ALTER TABLE character_storage_item
    ADD COLUMN gear_score INTEGER CHECK (gear_score >= 0),
    ADD COLUMN durability INTEGER CHECK (durability >= 0),
    ADD COLUMN perks TEXT[],
    ADD COLUMN bound BOOLEAN,
    ADD COLUMN dye SMALLINT[] NOT NULL DEFAULT '{0,0,0,0}',
    ADD COLUMN appearance BIGINT NOT NULL DEFAULT 0
        CHECK (appearance >= 0 AND appearance <= 4294967295),
    ADD COLUMN skin_dye SMALLINT[] NOT NULL DEFAULT '{0,0,0,0}';

-- The appearances an account collected (retail inventory service `SkinCollection`, owned by
-- the account: one entry per appearance with `isCollected`). Every character of the account
-- may apply them (5692).
CREATE TABLE account_skin (
    account_id UUID NOT NULL,
    -- The CRC-32 of the lowercase appearance id, the skin's id on the wire.
    appearance BIGINT NOT NULL CHECK (appearance >= 0 AND appearance <= 4294967295),
    collected_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, appearance)
);
