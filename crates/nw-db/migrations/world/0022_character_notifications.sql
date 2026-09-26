-- SPDX-License-Identifier: AGPL-3.0-or-later
-- How often each event notification was shown to a character (conversation-notify note, section
-- 2): the counts of the client's SyncNotifications (3236). Retail keeps them per character
-- across sessions (the same id shows 10, 11, 12 in later sessions); the client uses them for
-- notification_data MaxDisplayCount. The notification is the CRC-32 of its lowercase
-- NotificationID, a u32 kept in BIGINT. They belong to the progression of migration 0006 and
-- are saved with it. Only sim writes this table.

CREATE TABLE character_notification (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    notification BIGINT NOT NULL CHECK (notification >= 0 AND notification <= 4294967295),
    shown INTEGER NOT NULL CHECK (shown > 0),
    PRIMARY KEY (character_id, notification)
);
