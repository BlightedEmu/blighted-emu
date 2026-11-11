-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The world's clock anchor: the time that SyncTimeChange (4246) gives every client, from which the
-- client draws the time of day and counts its effect timers. Observed (retail
-- newchar-retail-2026-09-18 and general-other-01, three days apart on one world): the same anchor
-- at every login, six and nine days before the session; other worlds had other anchors. The first
-- sim that starts writes it; every later start and every other sim of the world reads it, so the
-- time of day goes on over a restart and is the same on every sim. Nanoseconds since 2000-01-01.

CREATE TABLE world_clock (
    id SMALLINT PRIMARY KEY CHECK (id = 1),
    start_ns BIGINT NOT NULL CHECK (start_ns >= 0)
);
