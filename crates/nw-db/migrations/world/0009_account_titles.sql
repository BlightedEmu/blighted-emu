-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The player titles an account earned (run-15 core study): the client lists them with 4147,
-- the new ones in a second list until it clears them with 4144. A title of TitleType Account
-- earned by one character is every character's of the account, so the rows are per account.
-- Only sim writes these rows.

CREATE TABLE account_title (
    account_id UUID NOT NULL,
    -- The CRC-32 of the lowercase TitleID, the title's id on the wire (sim keeps it as u32).
    title_id BIGINT NOT NULL CHECK (title_id >= 0 AND title_id <= 4294967295),
    -- The client still marks it new.
    is_new BOOLEAN NOT NULL DEFAULT true,
    earned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, title_id)
);
