-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Mutated expeditions (run 15 dungeon study): retail's PlayerArenaPersistentData keeps
-- m_lastDungeonEnteredWasMutated beside the arena counters of migration 0015. It is saved with
-- the rest of the arena state. Only sim writes this column.

ALTER TABLE character_arena
    ADD COLUMN last_dungeon_mutated BOOLEAN NOT NULL DEFAULT false;
