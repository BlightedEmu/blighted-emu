-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The wars of the territories, their sign-ups and their rosters. Observed (retail war data,
-- type 1739, 837 ledgers of the capture index): each war names its territory, the attacking
-- and the defending company, a start time on the hour, its run of 1,800 s and, at its end, the
-- company that won; a war leaves the list 180 s after its result. Observed (territory
-- interactor calls and state 1880): a player signs up for one side of a territory's war; the
-- roster puts signed-up players into 10 groups of 5 slots, each with a permission; a kicked
-- player's sign-up is refused (reason 12). How a company declares a war in retail is Unknown
-- (no capture holds a declaration); on this server a game master declares one. A deleted
-- company loses its wars (Inferred).

CREATE TABLE territory_war (
    war_id UUID PRIMARY KEY,
    -- The territory id (the client's TerritoryDefinition TerritoryID).
    territory_id SMALLINT NOT NULL CHECK (territory_id > 0),
    attacker_company UUID NOT NULL REFERENCES company (company_id) ON DELETE CASCADE,
    defender_company UUID NOT NULL REFERENCES company (company_id) ON DELETE CASCADE,
    -- The war's number: its attacker side number; the defender side is the number + 1
    -- (Observed: 9428 and 9429).
    number BIGSERIAL NOT NULL,
    starts_ms BIGINT NOT NULL,
    winner_company UUID,
    resolved_ms BIGINT,
    declared TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (attacker_company <> defender_company),
    CHECK ((winner_company IS NULL) = (resolved_ms IS NULL))
);

-- One war without a result per territory.
CREATE UNIQUE INDEX territory_war_open_idx ON territory_war (territory_id)
    WHERE resolved_ms IS NULL;

CREATE TABLE war_signup (
    war_id UUID NOT NULL REFERENCES territory_war (war_id) ON DELETE CASCADE,
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- 1 attacker, 2 defender (the 2762 side).
    side SMALLINT NOT NULL CHECK (side IN (1, 2)),
    gear_score SMALLINT NOT NULL CHECK (gear_score >= 0),
    -- The roster place, when the player is on the roster.
    roster_group SMALLINT CHECK (roster_group >= 0 AND roster_group < 10),
    roster_slot SMALLINT CHECK (roster_slot >= 0 AND roster_slot < 5),
    -- 0 member, 1 officer, 2 war leader (the 2782 permission).
    permission SMALLINT NOT NULL DEFAULT 0 CHECK (permission >= 0 AND permission <= 2),
    -- A kicked player stays here, off the roster, and may not sign up again for this war.
    kicked BOOLEAN NOT NULL DEFAULT false,
    signed TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (war_id, character_id),
    CHECK ((roster_group IS NULL) = (roster_slot IS NULL)),
    CHECK (NOT kicked OR roster_group IS NULL)
);

CREATE UNIQUE INDEX war_signup_place_idx ON war_signup (war_id, side, roster_group, roster_slot)
    WHERE roster_group IS NOT NULL;
