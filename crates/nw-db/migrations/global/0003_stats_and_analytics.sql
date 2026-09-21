-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Our stats service and analytics stream (study note
-- docs/re/2026-09-18-leaderboards-and-analytics.md). The client reads stats and leaderboards
-- through edge-http; the sims write them.

-- One value of one stat of one entity. A leaderboard is every row of one stat id (the board id
-- the client asks for, such as top-war-score.<world>.m41). Global stats have the entity ''.
CREATE TABLE stat_value (
    -- The client's namespace, such as 'pc'.
    namespace TEXT NOT NULL,
    stat_id TEXT NOT NULL,
    -- 'Character', 'Company', 'Group' or 'Global'.
    scope TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    value DOUBLE PRECISION NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (namespace, stat_id, entity_id)
);

CREATE INDEX stat_value_by_scope ON stat_value (namespace, scope, stat_id);

-- One record the client put on its analytics stream.
CREATE TABLE analytics_event (
    event_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    stream_name TEXT NOT NULL,
    partition_key TEXT NOT NULL,
    -- The record's "type" field, when the record is a JSON object with one.
    event_type TEXT,
    -- The record when it is JSON, else its bytes.
    payload JSONB,
    raw BYTEA,
    CHECK ((payload IS NULL) <> (raw IS NULL))
);

CREATE INDEX analytics_event_by_type ON analytics_event (event_type, received_at);
