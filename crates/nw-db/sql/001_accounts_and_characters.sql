-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Accounts and characters in the one Docker PostgreSQL database.
-- The architecture spec also describes one database per world.
-- This milestone keeps both tables in the global database. world_id is a column.

CREATE TABLE IF NOT EXISTS login_account (
    account_id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT login_account_username_unique UNIQUE (username)
);

CREATE TABLE IF NOT EXISTS player_character (
    character_id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    world_id TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT player_character_account_fk
        FOREIGN KEY (account_id) REFERENCES login_account (account_id),
    CONSTRAINT player_character_world_name_unique UNIQUE (world_id, name)
);

CREATE INDEX IF NOT EXISTS player_character_account_world_idx
    ON player_character (account_id, world_id);
