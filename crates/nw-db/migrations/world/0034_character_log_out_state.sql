-- SPDX-License-Identifier: AGPL-3.0-or-later
-- What a character had at its last save besides its progression: its health, mana and stamina,
-- its running ability cooldowns, whether it wants its PvP flag on, its puzzle-stone turns and the
-- bait on its fishing pole. Observed (retail, two parts of one recorded run): a player whose
-- health was 0 at the end of one part came back with health 0 at the start of the next part and
-- asked to respawn 32 s later, so retail keeps health over a log out. Inferred: mana and stamina
-- live in the same vitals state and are kept the same way; ability cooldowns, the PvP flag wish,
-- the stone turns and the bait are kept as the consumable cooldowns of migration 0029 are. They
-- belong to the progression of migration 0006 and are saved with it. Only sim writes them.

ALTER TABLE player_character
    ADD COLUMN health BIGINT CHECK (health >= 0 AND health <= 4294967295),
    ADD COLUMN mana_milli BIGINT CHECK (mana_milli >= 0 AND mana_milli <= 4294967295),
    ADD COLUMN stamina_milli BIGINT CHECK (stamina_milli >= 0 AND stamina_milli <= 4294967295),
    ADD COLUMN pvp_wanted BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN cyclic_turns JSONB NOT NULL DEFAULT '{}',
    ADD COLUMN fishing_bait TEXT;

-- The running ability cooldowns of a character: the CRC-32 of the lowercase ability id, the key
-- (0 to 2) of the press that started it, and its start and end in microseconds since
-- 2000-01-01, the server clock the client gets.
CREATE TABLE character_ability_cooldown (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    ability BIGINT NOT NULL CHECK (ability >= 0 AND ability <= 4294967295),
    press_key SMALLINT NOT NULL CHECK (press_key >= 0 AND press_key <= 2),
    start_us BIGINT NOT NULL CHECK (start_us >= 0),
    end_us BIGINT NOT NULL CHECK (end_us >= start_us),
    PRIMARY KEY (character_id, ability)
);

-- The depleted gather nodes of a level (world state, not a character's): the node's id
-- (GdeRef), the level gatherable point it stands at, where its first gatherer stood, and when it
-- grows back (Unix ms). A sim of that level loads the rows whose time has not come at its start,
-- so a restart does not grow every node back. A row whose time came is stale and is removed at
-- the next load. Only sim writes this table.
CREATE TABLE world_gather_node (
    level TEXT NOT NULL,
    gde_ref BYTEA NOT NULL CHECK (octet_length(gde_ref) = 16),
    point INTEGER NOT NULL CHECK (point >= 0),
    x REAL NOT NULL,
    y REAL NOT NULL,
    z REAL NOT NULL,
    respawn_at_unix_ms BIGINT NOT NULL CHECK (respawn_at_unix_ms >= 0),
    PRIMARY KEY (level, gde_ref)
);
