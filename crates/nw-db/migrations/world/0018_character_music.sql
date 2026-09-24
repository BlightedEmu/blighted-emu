-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The musical performance lockout of a character (music study, run 15): the client's
-- MusicalPerformancePlayerPersistence m_nextPerformanceStartTime, the time from which it may
-- create a performance again after it left one. Server time in ms since 2000-01-01, a u64 the
-- sim keeps, stored in BIGINT; NULL for no lockout. It belongs to the progression of migration
-- 0006 and is saved with it (progression_saved_at). Only sim writes this column.

ALTER TABLE player_character
    ADD COLUMN next_performance_ms BIGINT CHECK (next_performance_ms IS NULL
        OR next_performance_ms >= 0);
