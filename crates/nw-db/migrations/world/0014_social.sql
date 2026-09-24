-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The social state of a world (run 15 social study): the chat and voice options a client sets,
-- friends, friend invites, blocks and chat mutes, player reports, companies with their members,
-- invites, treasury and treasury log, and groups. Only the social service writes these rows.

-- The options a client sets and what other players see of a character while it is offline.
CREATE TABLE character_social (
    character_id UUID PRIMARY KEY REFERENCES player_character (character_id) ON DELETE CASCADE,
    -- 4125: lines come with their listed words starred (on until the client turns it off).
    profanity_filter BOOLEAN NOT NULL DEFAULT true,
    -- 4126: the client's language value; NULL until it sends one.
    language BIGINT CHECK (language IS NULL OR (language >= 0 AND language <= 4294967295)),
    -- 4142: the two streamer-mode bytes.
    streamer_mode BYTEA NOT NULL DEFAULT '\x0000' CHECK (octet_length(streamer_mode) = 2),
    -- 6444: the ten crossplay permission bytes; NULL until the client sends them.
    crossplay BYTEA CHECK (crossplay IS NULL OR octet_length(crossplay) = 10),
    -- 188: the client's voice chat is on.
    voice_active BOOLEAN NOT NULL DEFAULT false,
    -- The character's icon (PlayerIconData, 9 bytes) and level when it was last online.
    icon BYTEA CHECK (icon IS NULL OR octet_length(icon) = 9),
    level BIGINT NOT NULL DEFAULT 0 CHECK (level >= 0 AND level <= 4294967295),
    -- The client's FactionType when it was last online (0 none, 1 to 3).
    faction SMALLINT NOT NULL DEFAULT 0 CHECK (faction >= 0 AND faction <= 3),
    last_online TIMESTAMPTZ,
    -- When the character last left or lost its company (GuildsComponent lastGuildLeaveTime).
    company_left_at TIMESTAMPTZ
);

-- A directed link between two characters: `friend` both ways once accepted, `invite` from the
-- inviter to the invitee until it is answered, `block` and `mute` from the one who set them.
CREATE TABLE character_social_link (
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    other_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('friend', 'invite', 'block', 'mute')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (character_id, other_id, kind),
    CHECK (character_id <> other_id)
);

CREATE INDEX character_social_link_other_idx ON character_social_link (other_id);

-- 1600: a report of a player, for the admins.
CREATE TABLE player_report (
    report_id BIGSERIAL PRIMARY KEY,
    reporter_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    reported_id UUID NOT NULL,
    reported_name TEXT NOT NULL,
    reason BIGINT NOT NULL CHECK (reason >= 0 AND reason <= 4294967295),
    category BIGINT NOT NULL CHECK (category >= 0 AND category <= 4294967295),
    comment TEXT NOT NULL,
    detail TEXT NOT NULL,
    locale TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE company (
    company_id UUID PRIMARY KEY,
    name TEXT NOT NULL CHECK (name <> ''),
    -- The client's FactionType of the company (its creator's).
    faction SMALLINT NOT NULL CHECK (faction >= 0 AND faction <= 3),
    -- The crest as the client sends it (GuildIconData: two u16 indexes and two RGBA colours).
    crest BYTEA NOT NULL CHECK (octet_length(crest) = 36),
    motd TEXT NOT NULL DEFAULT '',
    motd_author UUID,
    motd_at TIMESTAMPTZ,
    -- The hour sieges of the company's territories start (3615).
    siege_hour REAL NOT NULL,
    -- Hundredths of a coin, like every coin value.
    treasury BIGINT NOT NULL DEFAULT 0 CHECK (treasury >= 0),
    -- The daily withdrawal limit of the ranks without full treasury rights; NULL for none.
    withdrawal_limit BIGINT CHECK (withdrawal_limit IS NULL OR withdrawal_limit >= 0),
    renames INTEGER NOT NULL DEFAULT 0 CHECK (renames >= 0),
    crest_changes INTEGER NOT NULL DEFAULT 0 CHECK (crest_changes >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX company_name_idx ON company (lower(name));

CREATE TABLE company_member (
    character_id UUID PRIMARY KEY REFERENCES player_character (character_id) ON DELETE CASCADE,
    company_id UUID NOT NULL REFERENCES company (company_id) ON DELETE CASCADE,
    -- 0 governor, 1 consul, 2 officer, 3 settler (2591).
    rank SMALLINT NOT NULL CHECK (rank >= 0 AND rank <= 3),
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX company_member_company_idx ON company_member (company_id);

CREATE TABLE company_invite (
    company_id UUID NOT NULL REFERENCES company (company_id) ON DELETE CASCADE,
    character_id UUID NOT NULL REFERENCES player_character (character_id) ON DELETE CASCADE,
    inviter_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, character_id)
);

-- Every treasury change with the balance after it, written in the change's transaction.
CREATE TABLE company_treasury_log (
    log_id BIGSERIAL PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES company (company_id) ON DELETE CASCADE,
    character_id UUID NOT NULL,
    change BIGINT NOT NULL,
    balance BIGINT NOT NULL CHECK (balance >= 0),
    reason TEXT NOT NULL CHECK (reason IN ('deposit', 'withdraw', 'refund')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX company_treasury_log_withdraw_idx
    ON company_treasury_log (character_id, created_at) WHERE reason = 'withdraw';

CREATE TABLE social_group (
    group_id UUID PRIMARY KEY,
    leader_id UUID NOT NULL,
    -- The id of the group's voice channel (4200).
    voice_channel TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE social_group_member (
    character_id UUID PRIMARY KEY REFERENCES player_character (character_id) ON DELETE CASCADE,
    group_id UUID NOT NULL REFERENCES social_group (group_id) ON DELETE CASCADE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX social_group_member_group_idx ON social_group_member (group_id);
