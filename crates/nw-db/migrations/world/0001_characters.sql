-- SPDX-License-Identifier: AGPL-3.0-or-later
-- One world database (M2 spec section 7). account_id points into the global database; there is
-- no foreign key across databases.

CREATE TABLE player_character (
    character_id UUID PRIMARY KEY,
    account_id UUID NOT NULL,
    name TEXT NOT NULL,
    appearance JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX player_character_account_id_idx ON player_character (account_id);
