-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Global database (M2 spec section 5.1): accounts, logins, launcher sessions, character names.

CREATE TABLE account (
    account_id UUID PRIMARY KEY,
    display_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    banned BOOLEAN NOT NULL DEFAULT false
);

-- Password mode. username is stored in lower case.
CREATE TABLE password_login (
    username TEXT PRIMARY KEY,
    account_id UUID NOT NULL UNIQUE REFERENCES account (account_id) ON DELETE CASCADE,
    password_hash TEXT NOT NULL
);

-- Discord mode.
CREATE TABLE discord_login (
    discord_user_id TEXT PRIMARY KEY,
    account_id UUID NOT NULL UNIQUE REFERENCES account (account_id) ON DELETE CASCADE
);

-- token_hash is SHA-256 of the launcher token; the token itself is never stored.
CREATE TABLE launcher_session (
    token_hash BYTEA PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES account (account_id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX launcher_session_expires_at_idx ON launcher_session (expires_at);

-- Character names are unique over all worlds. name_key is the lower-case name.
CREATE TABLE character_name (
    name_key TEXT PRIMARY KEY,
    world_id UUID NOT NULL,
    character_id UUID NOT NULL UNIQUE
);
