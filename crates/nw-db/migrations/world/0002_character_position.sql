-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The saved world position of a character (M4 spec section 8). NULL in every column means the
-- character was never in the world. Only sim writes these columns.

ALTER TABLE player_character
    ADD COLUMN level_name TEXT,
    ADD COLUMN pos_x DOUBLE PRECISION,
    ADD COLUMN pos_y DOUBLE PRECISION,
    ADD COLUMN pos_z DOUBLE PRECISION,
    ADD COLUMN yaw REAL,
    ADD COLUMN position_saved_at TIMESTAMPTZ,
    ADD CONSTRAINT player_character_position_complete CHECK (
        (level_name IS NULL AND pos_x IS NULL AND pos_y IS NULL AND pos_z IS NULL
            AND yaw IS NULL AND position_saved_at IS NULL)
        OR (level_name IS NOT NULL AND pos_x IS NOT NULL AND pos_y IS NOT NULL
            AND pos_z IS NOT NULL AND yaw IS NOT NULL AND position_saved_at IS NOT NULL)
    ),
    -- PostgreSQL floats accept NaN and infinity; a position must be a real point.
    ADD CONSTRAINT player_character_position_finite CHECK (
        pos_x IS NULL OR (
            pos_x NOT IN ('NaN', 'Infinity', '-Infinity')
            AND pos_y NOT IN ('NaN', 'Infinity', '-Infinity')
            AND pos_z NOT IN ('NaN', 'Infinity', '-Infinity')
            AND yaw NOT IN ('NaN', 'Infinity', '-Infinity')
        )
    );
