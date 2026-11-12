-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The consumable store entitlements of each account (the barbershop token): how many it holds.
-- A store purchase adds the offer's amount; a use (the barber) takes one. The entitlement is the
-- game data's UniqueTagID; purchases counts the store purchases (the catalog's acquisitionCount).
-- An account with no row holds none.

CREATE TABLE account_consumable (
    account_id UUID NOT NULL REFERENCES account (account_id) ON DELETE CASCADE,
    entitlement TEXT NOT NULL CHECK (entitlement <> ''),
    amount BIGINT NOT NULL CHECK (amount >= 0),
    purchases BIGINT NOT NULL DEFAULT 0 CHECK (purchases >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, entitlement)
);
