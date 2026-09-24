-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The rest of a character's travel and housing state (run 15 travel2): its played time, its last
-- inn recall, the camp it placed, and each home's plot reference, house data entity, house type,
-- territory, taxes due time and fast travel cooldown. Saved with the progression like migration
-- 0010. A u64 id is stored in BIGINT as the same 64 bits, a u32 CRC as its value, positions as
-- f32 bit patterns. Times are Unix milliseconds.

ALTER TABLE player_character
    -- The played time before the session, ms (player time state 3290).
    ADD COLUMN played_ms BIGINT NOT NULL DEFAULT 0 CHECK (played_ms >= 0),
    -- The last inn recall (3713 to the bound inn); 0 for never.
    ADD COLUMN inn_recalled_ms BIGINT NOT NULL DEFAULT 0 CHECK (inn_recalled_ms >= 0),
    -- The placed camp (2236): position bits, tier and time, all three or none.
    ADD COLUMN camp_position_bits INTEGER[]
        CHECK (camp_position_bits IS NULL OR (cardinality(camp_position_bits) = 3
            AND array_position(camp_position_bits, NULL) IS NULL)),
    ADD COLUMN camp_tier SMALLINT CHECK (camp_tier IS NULL OR (camp_tier >= 0
        AND camp_tier <= 255)),
    ADD COLUMN camp_placed_ms BIGINT CHECK (camp_placed_ms IS NULL OR camp_placed_ms >= 0),
    ADD CONSTRAINT camp_all_or_none CHECK (
        (camp_position_bits IS NULL) = (camp_tier IS NULL)
        AND (camp_tier IS NULL) = (camp_placed_ms IS NULL));

ALTER TABLE character_home
    -- The plot reference's facet entity and third id, as the client names them.
    ADD COLUMN plot_facet BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN plot_extra BIGINT NOT NULL DEFAULT 0,
    -- The house data entity (544): its 16-byte raw id and facet entity; zero until named.
    ADD COLUMN house_data BYTEA NOT NULL DEFAULT '\x00000000000000000000000000000000'
        CHECK (octet_length(house_data) = 16),
    ADD COLUMN house_data_facet BIGINT NOT NULL DEFAULT 0,
    -- The CRC-32 of the lowercase HouseTypeID.
    ADD COLUMN house_type BIGINT NOT NULL DEFAULT 0 CHECK (house_type >= 0
        AND house_type <= 4294967295),
    ADD COLUMN territory INTEGER NOT NULL DEFAULT 0 CHECK (territory >= 0
        AND territory <= 65535),
    ADD COLUMN taxes_due_ms BIGINT NOT NULL DEFAULT 0 CHECK (taxes_due_ms >= 0),
    ADD COLUMN fast_travel_ready_ms BIGINT NOT NULL DEFAULT 0 CHECK (fast_travel_ready_ms >= 0);
