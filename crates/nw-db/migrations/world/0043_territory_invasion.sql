-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Territory invasions and the darkness of each territory.
--
-- Observed (retail war data 1739, ledgers `invasion` (won), `Reekwater_Invasion` and
-- `Playthrough_Part_21` (lost)): an invasion is a war entry of kind 2 with no attacking company
-- (zero id, faction 0); the owner company defends; a won invasion's winner is the defender, a
-- lost one has no winner (zero) but its result time. So an invasion is a `territory_war` row
-- with no attacker company, and a lost invasion is resolved without a winner.
--
-- Observed (retail claims 3086, the same ledgers): each claim carries a darkness threshold
-- (0 to 3; every invaded territory at 3) and a darkness cycle end time. Inferred: the level
-- rises one step a cycle, an invasion comes at level 3, and the level returns to 0 at the end
-- of the invasion's cycle (the director's darkness steps).

ALTER TABLE territory_war ALTER COLUMN attacker_company DROP NOT NULL;

-- The old rule `(winner_company IS NULL) = (resolved_ms IS NULL)` holds for wars only.
ALTER TABLE territory_war DROP CONSTRAINT territory_war_check1;

ALTER TABLE territory_war ADD CONSTRAINT territory_war_winner_check
    CHECK (winner_company IS NULL OR resolved_ms IS NOT NULL);

ALTER TABLE territory_war ADD CONSTRAINT territory_war_war_result_check
    CHECK (attacker_company IS NULL OR (winner_company IS NULL) = (resolved_ms IS NULL));

ALTER TABLE territory_war ADD CONSTRAINT territory_war_invasion_winner_check
    CHECK (attacker_company IS NOT NULL OR winner_company IS NULL
           OR winner_company = defender_company);

CREATE TABLE territory_darkness (
    -- The territory id (the client's TerritoryDefinition TerritoryID).
    territory_id SMALLINT PRIMARY KEY CHECK (territory_id > 0),
    -- The claims' darkness threshold.
    level SMALLINT NOT NULL CHECK (level >= 0 AND level <= 3),
    -- The claims' darkness cycle end time, Unix ms: when the level changes next.
    cycle_end_ms BIGINT NOT NULL
);

-- Observed (the claims 3086 of the same ledgers): each claim carries the tiers of 19 upgrades
-- (ids 0 to 9, 11 to 17, 20 and 21; tiers 0 to 3); a lost invasion lowered some of them by one.
-- A territory upgrade without a row has tier 0.
CREATE TABLE territory_upgrade (
    territory_id SMALLINT NOT NULL CHECK (territory_id > 0),
    upgrade_id SMALLINT NOT NULL CHECK (upgrade_id >= 0),
    tier SMALLINT NOT NULL CHECK (tier >= 0 AND tier <= 3),
    PRIMARY KEY (territory_id, upgrade_id)
);
