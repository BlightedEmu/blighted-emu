-- SPDX-License-Identifier: AGPL-3.0-or-later
-- War declaring extras (wars and sieges findings "War declaring - helper D pass").
--
-- The id of each war declaration request: the social service tries a declaration again when the
-- database's answer was lost, and the retry finds its own stored declaration by this id instead
-- of a refusal that would give a wallet payment back while the declaration stands. A declaration
-- stored before this migration has no id (no retry of it can come any more).
ALTER TABLE war_declaration ADD COLUMN request_id UUID UNIQUE;

-- The claims' `replicatedWarDecThresholdMetFaction`: the attacking faction that met the war
-- declaration threshold in the race (0 for none). Observed: it is set in the same claims state as
-- a one-percent rise of that faction, never to the owner's faction, and it goes back to 0 when
-- the race starts again.
ALTER TABLE territory_conflict ADD COLUMN war_dec_faction SMALLINT NOT NULL DEFAULT 0
    CHECK (war_dec_faction BETWEEN 0 AND 3);
