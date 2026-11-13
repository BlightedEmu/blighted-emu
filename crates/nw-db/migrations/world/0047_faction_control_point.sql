-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The faction control points: the fort of each claim and the influence towers
-- (`nw_db::control_points`). Observed (every retail 3086 of the capture index): the claims'
-- `replicatedFactionControlPointData` is a list of four-bit entries, one per control point
-- (0 to 12 the forts of the claims, 18 to 56 the 39 influence towers), each with an active and a
-- contested bit and the owner faction; a fort captured outside an influence race locks for one
-- hour (`fcp_lock_timers`).

-- One row per control point that a faction holds or captures. A tower's row counts only in the
-- race window it was captured in (Observed: the towers go back to no owner when it ends).
CREATE TABLE faction_control_point (
    point_index SMALLINT PRIMARY KEY CHECK (point_index BETWEEN 0 AND 57),
    territory_id SMALLINT NOT NULL CHECK (territory_id > 0),
    -- The owner faction, 0 for none (a tower contested before its first capture).
    owner_faction SMALLINT NOT NULL CHECK (owner_faction BETWEEN 0 AND 3),
    -- When the owner took it, Unix ms.
    captured_ms BIGINT NOT NULL,
    -- The faction whose capture runs now, 0 for none.
    capturing_faction SMALLINT NOT NULL DEFAULT 0 CHECK (capturing_faction BETWEEN 0 AND 3),
    -- When a fort's lock ends, Unix ms; 0 for no lock.
    locked_until_ms BIGINT NOT NULL DEFAULT 0,
    -- Up to when a tower's influence was given, Unix ms.
    credited_ms BIGINT NOT NULL
);

CREATE INDEX faction_control_point_territory_idx ON faction_control_point (territory_id);
