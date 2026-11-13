-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The town projects of the territories (wars and sieges findings "Town projects", 2026-11-13).
--
-- Observed (retail claims 3086 and the planning board's 1746): a territory runs one project per
-- progression type (Fortress 1, Settlement 2, Lifestyle 3, ...); a project is Active (0) while
-- the town board missions add points to it, and when it completes its category's upgrade tier
-- rises to the project's level and the project is Blocking (4) until the slot is free again
-- (an empty slot `(0, type, Expired 2)`). Inferred: a project runs `CompletionTimeMinutes` from
-- its start; an Active project at its end without its points is Expired (2); a Blocking one at
-- its end frees the slot.

CREATE TABLE territory_project (
    -- The territory id (the client's TerritoryDefinition TerritoryID).
    territory_id SMALLINT NOT NULL CHECK (territory_id > 0),
    -- The TerritoryProgressionType of the project.
    progression_type SMALLINT NOT NULL CHECK (progression_type > 0),
    -- The CRC-32 of the lowercase ProjectId.
    project_crc BIGINT NOT NULL CHECK (project_crc >= 0 AND project_crc <= 4294967295),
    -- The upgrade id the project raises and the tier it gives.
    upgrade_id SMALLINT NOT NULL CHECK (upgrade_id >= 0),
    level SMALLINT NOT NULL CHECK (level > 0 AND level <= 3),
    -- The SettlementProgressionState: Active 0, Expired 2, Cancelled 3, Blocking 4.
    state SMALLINT NOT NULL CHECK (state IN (0, 2, 3, 4)),
    -- The points the missions added, and the points it needs.
    points INTEGER NOT NULL CHECK (points >= 0),
    needed INTEGER NOT NULL CHECK (needed >= 0),
    -- The id of the start: a retried start whose answer was lost finds its own project.
    start_id UUID NOT NULL,
    -- The company that started it and the player that did.
    company_id UUID NOT NULL REFERENCES company (company_id) ON DELETE CASCADE,
    character_id UUID NOT NULL,
    -- When it started and when it ends, Unix ms.
    started_ms BIGINT NOT NULL,
    ends_ms BIGINT NOT NULL CHECK (ends_ms >= started_ms),
    PRIMARY KEY (territory_id, progression_type)
);

-- The treasury reason of a project paid from the treasury (the client's `TerritoryProgression`
-- treasury log reason, 2).
ALTER TABLE company_treasury_log DROP CONSTRAINT company_treasury_log_reason_check;
ALTER TABLE company_treasury_log ADD CONSTRAINT company_treasury_log_reason_check
    CHECK (reason IN ('deposit', 'withdraw', 'refund', 'upkeep', 'war declaration',
                      'war declaration failed', 'territory progression'));
