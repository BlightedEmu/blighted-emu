-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The season pass state of a character (seasons finding 2026-11-12): what the season rewards
-- state (type 5485) and the tracked stats state (type 5606) hold besides the season experience,
-- which is the SeasonPass categorical progression of migration 0007. Retail keeps it per account
-- (a new character got the account's season); this server keeps it per character. It belongs to
-- the progression of migration 0006 and is saved with it. season_id is the CRC-32 of the lowercase
-- SeasonId the row is for (a u32 in BIGINT): a row of another season is a past season. A u64 is
-- stored in BIGINT as the same 64 bits, a u32 or u16 as its value. Only sim writes this table.

CREATE TABLE character_season (
    character_id UUID PRIMARY KEY REFERENCES player_character (character_id) ON DELETE CASCADE,
    season_id BIGINT NOT NULL CHECK (season_id >= 0 AND season_id <= 4294967295),
    -- RedeemBitmask: bit n of word n / 64 is RewardIndex n.
    redeemed BIGINT[] NOT NULL CHECK (array_position(redeemed, NULL) IS NULL),
    -- The season's tracked stats: the CRC-32 of each lowercase TrackedStatId and its count.
    stat_ids BIGINT[] NOT NULL CHECK (array_position(stat_ids, NULL) IS NULL),
    stat_values BIGINT[] NOT NULL CHECK (array_position(stat_values, NULL) IS NULL),
    -- The activity card: cardTemplate, the CRC-32 of each lowercase ActivitiesTaskID, the bit of
    -- each task that stamped a square, the stamped and the wild-stamped squares (bit row * 4 +
    -- column), wildStampCount, rerollCount and cardCount.
    card_template SMALLINT NOT NULL CHECK (card_template >= 0),
    card_tasks BIGINT[] NOT NULL CHECK (array_position(card_tasks, NULL) IS NULL),
    card_claimed BIGINT NOT NULL CHECK (card_claimed >= 0 AND card_claimed <= 4294967295),
    card_stamped INTEGER NOT NULL CHECK (card_stamped >= 0 AND card_stamped <= 65535),
    card_wild_stamped INTEGER NOT NULL CHECK (card_wild_stamped >= 0 AND card_wild_stamped <= 65535),
    wild_stamps SMALLINT NOT NULL CHECK (wild_stamps >= 0),
    rerolls SMALLINT NOT NULL CHECK (rerolls >= 0),
    cards_done INTEGER NOT NULL CHECK (cards_done >= 0 AND cards_done <= 65535),
    -- The card tasks' tracked stats, as stat_ids and stat_values: counted from the card's start.
    card_stat_ids BIGINT[] NOT NULL CHECK (array_position(card_stat_ids, NULL) IS NULL),
    card_stat_values BIGINT[] NOT NULL CHECK (array_position(card_stat_values, NULL) IS NULL),
    -- firstCharacterConnectTime: Unix nanoseconds.
    first_connect_ns BIGINT NOT NULL,
    CHECK (cardinality(stat_ids) = cardinality(stat_values)),
    CHECK (cardinality(card_stat_ids) = cardinality(card_stat_values))
);
