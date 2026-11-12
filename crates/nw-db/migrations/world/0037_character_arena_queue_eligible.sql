-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The matchmaking queue penalty (arena flow study 2026-11-11): retail's
-- queueEligibleTimesForGameModes of GameModeParticipantReplicatedState (type 3312) names, per game
-- mode, the Unix time from which the character may queue again; a leave of a running arena match
-- sets it 30 minutes ahead, and it stays across logins (a June value was still sent in October).
-- queue_eligible_modes holds the CRC-32 of each game mode (a u32 in BIGINT), and
-- queue_eligible_unix_ns the time of the same index (a u64 as the same 64 bits). Only sim writes
-- these columns.

ALTER TABLE character_arena
    ADD COLUMN queue_eligible_modes BIGINT[] NOT NULL DEFAULT '{}'
        CHECK (array_position(queue_eligible_modes, NULL) IS NULL),
    ADD COLUMN queue_eligible_unix_ns BIGINT[] NOT NULL DEFAULT '{}'
        CHECK (array_position(queue_eligible_unix_ns, NULL) IS NULL),
    ADD CHECK (cardinality(queue_eligible_modes) = cardinality(queue_eligible_unix_ns));
