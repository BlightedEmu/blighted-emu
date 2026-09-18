-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Admin site ranks and the audit log of GM commands (admin studio spec section 9).

CREATE TABLE admin_rank (
    account_id UUID PRIMARY KEY REFERENCES account (account_id) ON DELETE CASCADE,
    rank TEXT NOT NULL CHECK (rank IN ('csr', 'game_master', 'head_admin')),
    given_by UUID NULL,
    given_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One row per GM command and per admin site login. account_id is NULL in local mode.
CREATE TABLE admin_audit (
    id BIGSERIAL PRIMARY KEY,
    at TIMESTAMPTZ NOT NULL DEFAULT now(),
    account_id UUID NULL,
    rank TEXT NULL,
    command TEXT NOT NULL,
    target TEXT NOT NULL,
    detail JSONB NOT NULL,
    result TEXT NOT NULL
);

CREATE INDEX admin_audit_account_idx ON admin_audit (account_id, id);
