-- Tallybook schema v11: find matching register lines without a full scan.

-- Review asks, for every waiting bank transaction, whether a register line in
-- the same account for the same amount already records it. With only the
-- account indexed, each question read the account's whole history: 2,000
-- waiting transactions against 10,000 lines took seconds. Account and amount
-- together make each question a direct lookup.
CREATE INDEX journal_lines_account_amount ON journal_lines(account_id, amount);
