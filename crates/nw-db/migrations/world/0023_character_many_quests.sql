-- SPDX-License-Identifier: AGPL-3.0-or-later
-- A character runs many quests at once, up to the journal limit of each kind (client build
-- 25810808 `javelin.objectives-max-*`): one 'active' row per running objective. The primary key
-- still keeps one row per objective.

DROP INDEX character_quest_one_active;
