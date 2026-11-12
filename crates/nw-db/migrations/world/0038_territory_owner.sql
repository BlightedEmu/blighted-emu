-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The company that owns each territory claim of the world, and since when. Observed (retail land
-- claim manager state, type 3086, every ledger of the capture index): companies own every one of
-- the 13 claims, and each owner entry carries the company's id, name, crest and faction and a time
-- days to weeks before the capture (Inferred: since when the company owns the claim). A
-- territory with no row has no owner. How a company gets a claim in retail is Unknown (no capture
-- holds a claim of a free territory); on this server a game master sets and clears owners. A
-- deleted company loses its claims (Inferred).

CREATE TABLE territory_owner (
    -- The territory id of the claim (the client's TerritoryDefinition TerritoryID).
    territory_id SMALLINT PRIMARY KEY CHECK (territory_id > 0),
    company_id UUID NOT NULL REFERENCES company (company_id) ON DELETE CASCADE,
    since TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX territory_owner_company_idx ON territory_owner (company_id);
