-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Players' raids (raid type 4): a group that grew into a raid of four groups by the
-- convert-to-raid vote, with its leader, assistants and members.
--
-- Observed (retail `raid_group_influence_race`, `Playthrough_Part_18`, `WURM`, `Hatchery`,
-- `Hive gorgon raid third boss`): a players' raid has a raid id, four raid group ids, one leader
-- and assistants, up to 20 members, each in one raid group; its members share one voice channel
-- (the 4200 answers of every raid group of raid 5861 in `Hive gorgon raid third boss` and of raid
-- 3058 in `Hatchery` named the same channel). Inferred: a players' raid stays across a restart of
-- the social service, as a group does (0014 `social_group`); the raids of wars and matches are
-- the sims' and are not stored.

CREATE TABLE player_raid (
    -- The raid entity id (the client's `RaidDataComponent` entity).
    raid_entity UUID PRIMARY KEY,
    -- The raid id the groups state and the raid entity show.
    raid_id BIGINT NOT NULL UNIQUE CHECK (raid_id > 0),
    -- The four raid group ids, in raid group order.
    group_ids UUID[] NOT NULL CHECK (cardinality(group_ids) = 4),
    -- The id of the raid's voice channel (4200).
    voice_channel TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE player_raid_member (
    character_id UUID PRIMARY KEY REFERENCES player_character (character_id) ON DELETE CASCADE,
    raid_entity UUID NOT NULL REFERENCES player_raid (raid_entity) ON DELETE CASCADE,
    -- The member's place in the raid's join order (moves put a member last).
    seat SMALLINT NOT NULL CHECK (seat >= 0 AND seat < 20),
    -- The raid group index, 0 to 3.
    raid_group SMALLINT NOT NULL CHECK (raid_group >= 0 AND raid_group < 4),
    -- The client's eRaidPermission: 0 normal, 1 assistant, 2 leader.
    permission SMALLINT NOT NULL CHECK (permission >= 0 AND permission <= 2),
    -- The order the assistants got their permission in; NULL for a member that is no assistant.
    assistant_rank SMALLINT CHECK ((permission = 1) = (assistant_rank IS NOT NULL)),
    UNIQUE (raid_entity, seat)
);

CREATE INDEX player_raid_member_raid_idx ON player_raid_member (raid_entity);

-- One leader at most in each raid.
CREATE UNIQUE INDEX player_raid_member_leader_idx ON player_raid_member (raid_entity)
    WHERE permission = 2;
