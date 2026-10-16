-- Tallybook schema v1.
-- Money is INTEGER minor units. Dates are ISO-8601 TEXT (YYYY-MM-DD) so they sort.
-- Journal line amounts: positive = debit, negative = credit; each entry sums to zero.

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE accounts (
    id         INTEGER PRIMARY KEY,
    code       TEXT UNIQUE,
    name       TEXT NOT NULL,
    subtype    TEXT NOT NULL,
    parent_id  INTEGER REFERENCES accounts(id),
    archived   INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE journal_entries (
    id           INTEGER PRIMARY KEY,
    date         TEXT NOT NULL,
    memo         TEXT NOT NULL DEFAULT '',
    source       TEXT NOT NULL DEFAULT 'manual',
    reverses_id  INTEGER REFERENCES journal_entries(id),
    voided_by_id INTEGER REFERENCES journal_entries(id),
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX journal_entries_date ON journal_entries(date);

CREATE TABLE reconciliations (
    id                INTEGER PRIMARY KEY,
    account_id        INTEGER NOT NULL REFERENCES accounts(id),
    statement_date    TEXT NOT NULL,
    statement_balance INTEGER NOT NULL,
    status            TEXT NOT NULL DEFAULT 'in_progress' CHECK (status IN ('in_progress', 'completed')),
    completed_at      TEXT
);
CREATE UNIQUE INDEX reconciliations_one_open
    ON reconciliations(account_id) WHERE status = 'in_progress';

CREATE TABLE journal_lines (
    id                INTEGER PRIMARY KEY,
    entry_id          INTEGER NOT NULL REFERENCES journal_entries(id),
    account_id        INTEGER NOT NULL REFERENCES accounts(id),
    amount            INTEGER NOT NULL CHECK (amount <> 0),
    memo              TEXT,
    cleared           TEXT NOT NULL DEFAULT 'uncleared' CHECK (cleared IN ('uncleared', 'cleared', 'reconciled')),
    reconciliation_id INTEGER REFERENCES reconciliations(id)
);
CREATE INDEX journal_lines_entry ON journal_lines(entry_id);
CREATE INDEX journal_lines_account ON journal_lines(account_id);

-- The ledger is append-only. Mistakes are fixed by voiding (a reversing
-- entry), never by editing or deleting history.
CREATE TRIGGER journal_entries_no_delete BEFORE DELETE ON journal_entries
BEGIN SELECT RAISE(ABORT, 'journal entries are append-only; void instead'); END;

CREATE TRIGGER journal_entries_no_edit BEFORE UPDATE OF date, reverses_id ON journal_entries
BEGIN SELECT RAISE(ABORT, 'journal entries are append-only; void instead'); END;

CREATE TRIGGER journal_entries_void_once BEFORE UPDATE OF voided_by_id ON journal_entries
WHEN OLD.voided_by_id IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'entry is already voided'); END;

CREATE TRIGGER journal_lines_no_delete BEFORE DELETE ON journal_lines
BEGIN SELECT RAISE(ABORT, 'journal lines are append-only; void instead'); END;

CREATE TRIGGER journal_lines_no_edit BEFORE UPDATE OF entry_id, account_id, amount ON journal_lines
BEGIN SELECT RAISE(ABORT, 'journal lines are append-only; void instead'); END;
