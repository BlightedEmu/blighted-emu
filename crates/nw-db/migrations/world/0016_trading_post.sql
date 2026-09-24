-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The trading post of the world (economy note, run 15): the buy and sell orders of every
-- settlement, what the trading post owes each character, the completions by other players, and
-- the audit log of every value transfer of the economy (architecture spec sections 4.6 and 7.1).
-- The orders are global across the world's sims: every sim reads and writes them here, and the
-- world director expires them.

CREATE TABLE trading_post_contract (
    contract_id UUID PRIMARY KEY,
    -- ContractType: 0 buy order, 1 sell order.
    kind SMALLINT NOT NULL CHECK (kind IN (0, 1)),
    -- ContractStatus: 0 available, 3 completed (proceeds wait), 4 resolved.
    status SMALLINT NOT NULL CHECK (status IN (0, 3, 4)),
    -- Grows with each change; 1 at creation.
    version INTEGER NOT NULL CHECK (version > 0),
    creator_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- The settlement the order is posted at.
    outpost BIGINT NOT NULL,
    -- The item: its id, the CRC-32 of the lowercase id, and its trading post fields.
    item_id TEXT NOT NULL,
    item_crc BIGINT NOT NULL CHECK (item_crc >= 0),
    category TEXT NOT NULL,
    family TEXT NOT NULL,
    item_group TEXT NOT NULL,
    tier INTEGER NOT NULL,
    gear_score INTEGER NOT NULL CHECK (gear_score >= 0),
    required_level INTEGER NOT NULL,
    -- The CRC-32 of each lowercase perk id: [crc, ...].
    perks JSONB NOT NULL DEFAULT '[]'::jsonb,
    gem_perk_count INTEGER NOT NULL CHECK (gem_perk_count >= 0),
    -- The price of one unit, hundredths of a coin.
    price BIGINT NOT NULL CHECK (price > 0),
    quantity_posted INTEGER NOT NULL CHECK (quantity_posted > 0),
    quantity_open INTEGER NOT NULL CHECK (quantity_open >= 0 AND quantity_open <= quantity_posted),
    -- ContractCompletionReason of the creator's event: 0 none, 1 succeeded, 3 canceled,
    -- 4 expired.
    reason SMALLINT NOT NULL DEFAULT 0 CHECK (reason IN (0, 1, 3, 4)),
    -- The settlement's trading tax at posting, a fraction.
    tax_rate DOUBLE PRECISION NOT NULL CHECK (tax_rate >= 0 AND tax_rate < 1),
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL
);

-- Searches look at open orders by kind, item and price.
CREATE INDEX trading_post_open_by_item ON trading_post_contract (kind, item_id, price)
    WHERE status = 0;
CREATE INDEX trading_post_open_by_category
    ON trading_post_contract (kind, category, family, item_group, price) WHERE status = 0;
CREATE INDEX trading_post_by_creator ON trading_post_contract (creator_id, status);
CREATE INDEX trading_post_open_by_expiry ON trading_post_contract (expires_at) WHERE status = 0;

-- What the economy owes a character: coins, items, and the notices of its orders. The sim of
-- the character takes the rows when it can give what they hold. A row without an order gives
-- back what a sim took for a call whose player left before the answer.
CREATE TABLE trading_post_delivery (
    delivery_id BIGSERIAL PRIMARY KEY,
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    contract_id UUID REFERENCES trading_post_contract (contract_id) ON DELETE CASCADE,
    -- 0 none, 1 the order was fully completed (2964), 2 it expired (917).
    notice SMALLINT NOT NULL DEFAULT 0 CHECK (notice IN (0, 1, 2)),
    coins BIGINT NOT NULL DEFAULT 0 CHECK (coins >= 0),
    item_id TEXT,
    quantity INTEGER NOT NULL DEFAULT 0 CHECK (quantity >= 0),
    -- The completion that made the row: its units and the request id of its call.
    completed_count INTEGER NOT NULL DEFAULT 0 CHECK (completed_count >= 0),
    request_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (item_id IS NOT NULL OR quantity = 0)
);

CREATE INDEX trading_post_delivery_by_character
    ON trading_post_delivery (character_id, delivery_id);

-- Completions of orders by players other than their creator (the participant lookup's
-- completer role).
CREATE TABLE trading_post_completion (
    completion_id BIGSERIAL PRIMARY KEY,
    contract_id UUID NOT NULL REFERENCES trading_post_contract (contract_id) ON DELETE CASCADE,
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    completed_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX trading_post_completion_by_character
    ON trading_post_completion (character_id, completed_at);

-- Every value transfer of the economy, written in the transfer's own transaction. Coins and
-- quantity are signed changes for the character.
CREATE TABLE economy_audit (
    audit_id BIGSERIAL PRIMARY KEY,
    at TIMESTAMPTZ NOT NULL DEFAULT now(),
    kind TEXT NOT NULL,
    character_id UUID NOT NULL,
    other_character_id UUID,
    contract_id UUID,
    coins BIGINT NOT NULL DEFAULT 0,
    item_id TEXT,
    quantity INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX economy_audit_by_character ON economy_audit (character_id, at);

-- The last purchase of each shop offer with a cooldown (`ItemCurrencyConversionData`
-- `BuyCooldownSeconds`) by each character: a purchase waits until the cooldown passed.
CREATE TABLE shop_purchase (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- `ConversionID`.
    conversion_id TEXT NOT NULL,
    bought_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (character_id, conversion_id)
);
