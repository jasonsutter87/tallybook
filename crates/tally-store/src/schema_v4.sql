-- Tallybook schema v4: tax center.

-- Which Schedule C line each income/expense account feeds. Missing = not yet
-- confirmed by the user (the app shows a suggestion).
CREATE TABLE account_tax_lines (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id),
    line       TEXT NOT NULL
);

-- 1099 details. Only the last four digits of a TIN are kept; the full number
-- belongs on the W-9 itself and on the IRS filing, not in the book file.
ALTER TABLE contacts ADD COLUMN is_1099 INTEGER NOT NULL DEFAULT 0 CHECK (is_1099 IN (0, 1));
ALTER TABLE contacts ADD COLUMN w9_on_file INTEGER NOT NULL DEFAULT 0 CHECK (w9_on_file IN (0, 1));
ALTER TABLE contacts ADD COLUMN legal_name TEXT;
ALTER TABLE contacts ADD COLUMN tin_last4 TEXT CHECK (tin_last4 IS NULL OR length(tin_last4) = 4);

-- Who was paid (or who paid you). Metadata, so it may be set after posting.
ALTER TABLE journal_entries ADD COLUMN payee_id INTEGER REFERENCES contacts(id);
CREATE INDEX journal_entries_payee ON journal_entries(payee_id);

-- Backfill from documents recorded before v4.
UPDATE journal_entries SET payee_id = (SELECT contact_id FROM payments WHERE payments.entry_id = journal_entries.id)
 WHERE payee_id IS NULL AND id IN (SELECT entry_id FROM payments);
UPDATE journal_entries SET payee_id = (SELECT vendor_id FROM bills WHERE bills.entry_id = journal_entries.id)
 WHERE payee_id IS NULL AND id IN (SELECT entry_id FROM bills);
UPDATE journal_entries SET payee_id = (SELECT customer_id FROM invoices WHERE invoices.entry_id = journal_entries.id)
 WHERE payee_id IS NULL AND id IN (SELECT entry_id FROM invoices WHERE entry_id IS NOT NULL);

-- Estimated tax payments (federal Form 1040-ES or state equivalents).
CREATE TABLE tax_payments (
    id           INTEGER PRIMARY KEY,
    tax_year     INTEGER NOT NULL,
    quarter      INTEGER NOT NULL CHECK (quarter BETWEEN 1 AND 4),
    jurisdiction TEXT NOT NULL CHECK (jurisdiction IN ('federal', 'state')),
    date         TEXT NOT NULL,
    amount       INTEGER NOT NULL CHECK (amount > 0),
    account_id   INTEGER NOT NULL REFERENCES accounts(id),
    entry_id     INTEGER NOT NULL REFERENCES journal_entries(id),
    voided       INTEGER NOT NULL DEFAULT 0 CHECK (voided IN (0, 1)),
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX tax_payments_year ON tax_payments(tax_year);
