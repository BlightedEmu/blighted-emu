-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The upkeep of each owned territory claim. Observed (retail 1746 governance data of a town's
-- governor's desk, ledger `pay-territory-tax`): a time the one upkeep payment (2738) moved by
-- exactly 7 days (Inferred: the upkeep due time), a time 5 days before it that moved with it
-- (Inferred: from when the next payment is taken), and a company treasury change right after
-- the payment. Inferred: a new owner's first upkeep is due one interval after it took the claim.
-- The payment and the move of the due time are one transaction with the treasury change and its
-- log row (reason 'upkeep').

ALTER TABLE territory_owner ADD COLUMN upkeep_due_ms BIGINT;

UPDATE territory_owner
SET upkeep_due_ms = (extract(epoch FROM since) * 1000)::bigint + 604800000;

ALTER TABLE territory_owner ALTER COLUMN upkeep_due_ms SET NOT NULL;

ALTER TABLE company_treasury_log DROP CONSTRAINT company_treasury_log_reason_check;

ALTER TABLE company_treasury_log ADD CONSTRAINT company_treasury_log_reason_check
    CHECK (reason IN ('deposit', 'withdraw', 'refund', 'upkeep'));
