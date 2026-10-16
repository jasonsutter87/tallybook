-- Tallybook schema v2: bank feeds.
-- Secrets (Plaid access tokens, SimpleFIN access URLs) are NOT stored here;
-- they live in the OS keychain under `secret_ref`.

CREATE TABLE bank_connections (
    id             INTEGER PRIMARY KEY,
    provider       TEXT NOT NULL CHECK (provider IN ('plaid', 'simplefin', 'file')),
    institution    TEXT NOT NULL DEFAULT '',
    external_id    TEXT,              -- Plaid item_id
    secret_ref     TEXT,              -- keychain entry name
    cursor         TEXT NOT NULL DEFAULT '',
    status         TEXT NOT NULL DEFAULT 'ok' CHECK (status IN ('ok', 'needs_relink', 'error', 'disconnected')),
    last_error     TEXT,
    last_synced_at TEXT,
    created_at     TEXT NOT NULL DEFAULT (datetime('now'))
);

-- A provider account wired to a ledger account.
CREATE TABLE bank_links (
    id                  INTEGER PRIMARY KEY,
    connection_id       INTEGER NOT NULL REFERENCES bank_connections(id),
    external_account_id TEXT NOT NULL,
    ledger_account_id   INTEGER NOT NULL UNIQUE REFERENCES accounts(id),
    name                TEXT NOT NULL,
    mask                TEXT,
    bank_balance        INTEGER,       -- natural sign, as last reported
    -- Ignore feed transactions before this date (they're in the opening balance).
    start_date          TEXT NOT NULL,
    UNIQUE (connection_id, external_account_id)
);

CREATE TABLE bank_transactions (
    id                INTEGER PRIMARY KEY,
    ledger_account_id INTEGER NOT NULL REFERENCES accounts(id),
    external_id       TEXT NOT NULL,
    date              TEXT NOT NULL,
    amount            INTEGER NOT NULL,     -- inflow-positive == raw line amount
    description       TEXT NOT NULL,
    normalized        TEXT NOT NULL,        -- see tally_core::bank::normalize_description
    merchant          TEXT,
    category_hint     TEXT,
    pending           INTEGER NOT NULL DEFAULT 0,
    status            TEXT NOT NULL DEFAULT 'new'
                      CHECK (status IN ('new', 'posted', 'matched', 'excluded', 'removed')),
    entry_id          INTEGER REFERENCES journal_entries(id),
    line_id           INTEGER REFERENCES journal_lines(id),
    rule_id           INTEGER REFERENCES rules(id),
    imported_at       TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (ledger_account_id, external_id)
);
CREATE INDEX bank_transactions_inbox ON bank_transactions(status, ledger_account_id, date);
CREATE INDEX bank_transactions_line ON bank_transactions(line_id);
CREATE INDEX bank_transactions_normalized ON bank_transactions(normalized, status);

CREATE TABLE rules (
    id                INTEGER PRIMARY KEY,
    name              TEXT NOT NULL,
    priority          INTEGER NOT NULL DEFAULT 100,
    op                TEXT NOT NULL CHECK (op IN ('contains', 'equals', 'starts_with')),
    pattern           TEXT NOT NULL,
    direction         TEXT NOT NULL DEFAULT 'any' CHECK (direction IN ('any', 'inflow', 'outflow')),
    amount_min        INTEGER,
    amount_max        INTEGER,
    bank_account_id   INTEGER REFERENCES accounts(id),
    target_account_id INTEGER NOT NULL REFERENCES accounts(id),
    rename_to         TEXT,
    auto_post         INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL DEFAULT (datetime('now'))
);
