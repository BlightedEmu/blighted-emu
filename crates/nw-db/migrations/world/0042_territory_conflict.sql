-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The influence race of each territory, its conflict and the war declarations of the conflict's
-- lottery (`nw_db::conflicts`). Observed (every retail 3086 of the capture index): each claim
-- has a race start, a conflict faction and start, a lottery end and one influence percent per
-- faction; the influence goes back to 0 for every faction when the conflict ends. Observed
-- (`GameEventData`): a PvP faction mission's success event gives the player's faction 50, 100
-- or 200 influence (`FactionInfluenceAmount`, source `Missions`).

-- The influence points of each faction in each territory, with the time the decay of the
-- points was last counted.
CREATE TABLE territory_influence (
    territory_id SMALLINT NOT NULL CHECK (territory_id > 0),
    faction SMALLINT NOT NULL CHECK (faction BETWEEN 1 AND 3),
    points BIGINT NOT NULL CHECK (points >= 0),
    decayed_ms BIGINT NOT NULL,
    PRIMARY KEY (territory_id, faction)
);

-- The influence each company's members earned in a territory since its last conflict (the
-- client's `InsufficientContribution` refusal).
CREATE TABLE company_influence (
    territory_id SMALLINT NOT NULL CHECK (territory_id > 0),
    company_id UUID NOT NULL REFERENCES company (company_id) ON DELETE CASCADE,
    points BIGINT NOT NULL CHECK (points >= 0),
    PRIMARY KEY (territory_id, company_id)
);

-- The race and conflict of each territory; a territory without a row runs a race from the
-- first influence it gets. 0 is no faction and no time, as in the 3086 lists.
CREATE TABLE territory_conflict (
    territory_id SMALLINT PRIMARY KEY CHECK (territory_id > 0),
    race_start_ms BIGINT NOT NULL,
    conflict_faction SMALLINT NOT NULL DEFAULT 0 CHECK (conflict_faction BETWEEN 0 AND 3),
    conflict_start_ms BIGINT NOT NULL DEFAULT 0,
    lottery_end_ms BIGINT NOT NULL DEFAULT 0,
    CHECK ((conflict_faction = 0) = (conflict_start_ms = 0))
);

-- One declaration per company and conflict. The coin was taken at the declaration (from the
-- wallet of the declaring player or the company treasury); a lost lottery gives it back.
CREATE TABLE war_declaration (
    territory_id SMALLINT NOT NULL CHECK (territory_id > 0),
    conflict_start_ms BIGINT NOT NULL,
    company_id UUID NOT NULL REFERENCES company (company_id) ON DELETE CASCADE,
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    defender_company UUID NOT NULL REFERENCES company (company_id) ON DELETE CASCADE,
    -- Hundredths of a coin.
    cost BIGINT NOT NULL CHECK (cost >= 0),
    -- The war camp tier, 0 to 2 (tier I to III).
    war_camp SMALLINT NOT NULL CHECK (war_camp BETWEEN 0 AND 2),
    from_treasury BOOLEAN NOT NULL,
    lottery_end_ms BIGINT NOT NULL,
    war_starts_ms BIGINT NOT NULL,
    declared_ms BIGINT NOT NULL,
    -- The war of the drawn declaration; NULL before the draw and for a lost one.
    war_id UUID REFERENCES territory_war (war_id) ON DELETE SET NULL,
    drawn BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (territory_id, conflict_start_ms, company_id)
);

CREATE INDEX war_declaration_undrawn_idx ON war_declaration (lottery_end_ms) WHERE NOT drawn;

-- The treasury reasons of a declaration and of its refund (the client's `WarDeclaration` 4 and
-- `WarDeclarationFailed` 5 log reasons).
ALTER TABLE company_treasury_log DROP CONSTRAINT company_treasury_log_reason_check;
ALTER TABLE company_treasury_log ADD CONSTRAINT company_treasury_log_reason_check
    CHECK (reason IN ('deposit', 'withdraw', 'refund', 'upkeep', 'war declaration',
                      'war declaration failed'));
